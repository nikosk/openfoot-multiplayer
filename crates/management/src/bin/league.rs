//! Local JSON-lines transport for a trusted orchestrator. This is not a network
//! authentication boundary: the host supplies actor identity and clock values.
//! Model-facing clients must not be given raw access to this process's stdin.
use engine::PlayerData;
use management::football::{Fixture, Football, RecoverySetup};
use management::{Club, Error, Management, Manager, Player, Request};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};

const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;
const MAX_HISTORY: usize = 1000;

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Input {
    Schedule {
        club_ids: Vec<String>,
        first_day: u32,
        spacing_days: u32,
        seed: u64,
    },
    Init {
        clubs: Vec<Club>,
        players: Vec<Player>,
        managers: Vec<Manager>,
        attributes: Vec<PlayerData>,
        fixtures: Vec<Fixture>,
        recovery: Option<RecoverySetup>,
        career: Option<management::career::CareerSetup>,
        boards: Option<BTreeMap<String, management::football::BoardProfile>>,
        seasons: Option<management::seasons::SeasonSetup>,
        day: u32,
        deadline_ms: u64,
        #[serde(default)]
        require_match_rosters: bool,
    },
    Observe {
        actor: String,
    },
    Command {
        actor: String,
        request: Request,
        now_ms: u64,
    },
    Tick {
        day: u32,
        now_ms: u64,
        next_deadline_ms: u64,
    },
    Public {},
    Managers {},
    Save {},
    SaveFile {
        path: String,
    },
    LoadFile {
        path: String,
    },
    Load {
        checkpoint: Value,
    },
    History {
        after: usize,
        limit: usize,
    },
}

fn public(game: &Football) -> Value {
    json!({"state": game.public_state(), "standings": game.standings(), "results": game.results(),
        "dismissals": game.dismissals(),
        "season_history": game.public_season_history(), "season": game.public_season_state()})
}

fn execute(game: &mut Option<Football>, input: Input) -> Result<Value, String> {
    if let Input::LoadFile { path } = input {
        if game.is_some() {
            return Err("Already initialized".into());
        }
        let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
        let checkpoint =
            serde_json::from_reader(io::BufReader::new(file)).map_err(|error| error.to_string())?;
        return execute(game, Input::Load { checkpoint });
    }
    if let Input::Load { checkpoint } = input {
        if game.is_some() {
            return Err("Already initialized".into());
        }
        let restored = Football::load_validated(checkpoint)?;
        let result = public(&restored);
        *game = Some(restored);
        return Ok(result);
    }
    if let Input::Schedule {
        club_ids,
        first_day,
        spacing_days,
        seed,
    } = input
    {
        return management::calendar::double_round_robin(&club_ids, first_day, spacing_days, seed)
            .map(|fixtures| json!({"fixtures": fixtures}));
    }
    if let Input::Init {
        clubs,
        players,
        managers,
        attributes,
        fixtures,
        recovery,
        career,
        boards,
        seasons,
        day,
        deadline_ms,
        require_match_rosters,
    } = input
    {
        if game.is_some() {
            return Err("Already initialized".into());
        }
        let mut management = Management::new(clubs, players, managers, day, deadline_ms)
            .map_err(|error| format!("{error:?}"))?;
        if let Some(career) = career {
            management
                .configure_career(career)
                .map_err(|error| format!("{error:?}"))?;
        }
        if require_match_rosters {
            management
                .require_match_rosters()
                .map_err(|error| format!("{error:?}"))?;
        }
        let mut initialized = Football::new(management, attributes, fixtures)?;
        if let Some(recovery) = recovery {
            initialized.configure_recovery(recovery)?;
        }
        if let Some(boards) = boards {
            initialized.configure_boards(boards)?;
        }
        if let Some(seasons) = seasons {
            initialized.configure_seasons(seasons)?;
        }
        let result = public(&initialized);
        *game = Some(initialized);
        return Ok(result);
    }
    let game = game.as_mut().ok_or("Not initialized")?;
    match input {
        Input::Observe { actor } => {
            let manager_view = game
                .manager_view(&actor)
                .map_err(|error| format!("{error:?}"))?;
            let squad = game.squad(&actor).map_err(|error| format!("{error:?}"))?;
            let lineup = game.lineup(&actor).map_err(|error| format!("{error:?}"))?;
            let match_plan = game
                .match_plan(&actor)
                .map_err(|error| format!("{error:?}"))?;
            let recovery_view = match game.recovery_view(&actor) {
                Ok(view) => Some(view),
                Err(Error::Unavailable) => None,
                Err(error) => return Err(format!("{error:?}")),
            };
            let window = game.window();
            let board = match game.board_view(&actor) {
                Ok(view) => Some(view),
                Err(Error::Unavailable) => None,
                Err(error) => return Err(format!("{error:?}")),
            };
            let career = match game.career_view(&actor) {
                Ok(view) => Some(view),
                Err(Error::Unavailable) => None,
                Err(error) => return Err(format!("{error:?}")),
            };
            let free_agents = if career.is_some() {
                game.free_agent_squad(&actor)
                    .map_err(|error| format!("{error:?}"))?
            } else {
                vec![]
            };
            let fixtures: Vec<_> = game
                .fixtures()
                .iter()
                .filter(|fixture| {
                    fixture.day >= window.day
                        && (fixture.home == manager_view.club.id
                            || fixture.away == manager_view.club.id)
                })
                .map(|f| json!({"id": f.id, "day": f.day, "home": f.home, "away": f.away}))
                .collect();
            Ok(
                json!({"manager_view": manager_view, "squad": squad, "lineup": lineup,
                "match_plan": match_plan, "recovery_view": recovery_view, "board": board, "career": career, "free_agents": free_agents,
                "day": window.day, "deadline_ms": window.deadline_ms, "fixtures": fixtures}),
            )
        }
        Input::Command {
            actor,
            request,
            now_ms,
        } => {
            let receipt = game
                .dispatch(&actor, request, now_ms)
                .map_err(|error| format!("{error:?}"))?;
            Ok(json!(receipt))
        }
        Input::Tick {
            day,
            now_ms,
            next_deadline_ms,
        } => {
            let results = game.advance_closed_day(day, now_ms, next_deadline_ms)?;
            Ok(
                json!({"day": game.public_state().day, "results": results, "standings": game.standings()}),
            )
        }
        Input::Public {} => Ok(public(game)),
        Input::Managers {} => Ok(json!({"active_managers": game.active_managers()})),
        Input::Save {} => game.save_state(),
        Input::SaveFile { path } => {
            let checkpoint = game.save_state()?;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&path).map_err(|error| error.to_string())?;
            serde_json::to_writer(&mut file, &checkpoint).map_err(|error| error.to_string())?;
            file.sync_all().map_err(|error| error.to_string())?;
            Ok(json!({"saved":path,"version":1}))
        }
        Input::LoadFile { .. } => Err("Already initialized".into()),
        Input::Load { .. } => Err("Already initialized".into()),
        Input::History { after, limit } => {
            let results = game.results();
            if after > results.len() {
                return Err("History cursor is past the end".into());
            }
            let next = after
                .saturating_add(limit.min(MAX_HISTORY))
                .min(results.len());
            Ok(
                json!({"after": after, "next": next, "total": results.len(), "results": &results[after..next]}),
            )
        }
        Input::Init { .. } => Err("Already initialized".into()),
        Input::Schedule { .. } => {
            Err("Schedule must be handled before accessing game state".into())
        }
    }
}

/// Drain an oversized line before returning an error so the next request works.
/// Memory remains bounded even when the sender never supplies a newline.
fn read_line(reader: &mut impl BufRead, line: &mut Vec<u8>) -> io::Result<Option<bool>> {
    line.clear();
    let mut oversized = false;
    let mut seen = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(seen.then_some(oversized));
        }
        seen = true;
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        if !oversized {
            if consumed > MAX_LINE_BYTES.saturating_sub(line.len()) {
                oversized = true;
                line.clear();
            } else {
                line.extend_from_slice(&available[..consumed]);
            }
        }
        reader.consume(consumed);
        if newline.is_some() {
            return Ok(Some(oversized));
        }
    }
}

fn run() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();
    let mut game = None;
    let mut line = Vec::new();
    while let Some(oversized) = read_line(&mut reader, &mut line)? {
        let result = if oversized {
            Err("Input line exceeds 8 MiB".into())
        } else {
            serde_json::from_slice::<Input>(&line)
                .map_err(|error| format!("Invalid input: {error}"))
                .and_then(|input| execute(&mut game, input))
        };
        let response = match result {
            Ok(data) => json!({"ok": true, "data": data}),
            Err(error) => json!({"ok": false, "error": error}),
        };
        serde_json::to_writer(&mut writer, &response)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("league transport: {error}");
        std::process::exit(1);
    }
}
