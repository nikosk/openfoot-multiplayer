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
#[serde(deny_unknown_fields)]
struct PersonnelSetup {
    seed: u64,
    teams: BTreeMap<String, domain::team::Team>,
    staff: BTreeMap<String, domain::staff::Staff>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TeamHistorySetup {
    managers: BTreeMap<String, domain::manager::Manager>,
    actor_manager_ids: BTreeMap<String, String>,
    #[serde(default)]
    archived_identities: management::team_history::ArchivedIdentities,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Input {
    InitFile {
        path: String,
        deadline_ms: u64,
    },
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
        training: Option<management::training_commands::TrainingSetup>,
        availability: Option<BTreeMap<String, management::availability::Availability>>,
        squads: Option<management::squad_plan::SquadSetup>,
        lineups: Option<BTreeMap<String, Vec<String>>>,
        match_plans: Option<BTreeMap<String, management::tactics::MatchPlan>>,
        social: Option<management::social::SocialSetup>,
        economy: Option<management::economy_runtime::EconomySetup>,
        personnel: Option<PersonnelSetup>,
        team_history: Option<TeamHistorySetup>,
        competitions: Option<management::competitions::CompetitionSetup>,
        market: Option<management::market::MarketSetup>,
        news: Option<management::news_runtime::NewsSetup>,
        statistics: Option<domain::stats::StatsState>,
        national: Option<management::national::NationalSetup>,
        #[serde(default)]
        retired_player_ids: std::collections::BTreeSet<String>,
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
    Inbox {
        actor: String,
        offset: usize,
        limit: usize,
    },
    StaffMarket {
        actor: String,
    },
    TransferMarket {
        actor: String,
        filter: management::scouting::MarketFilter,
    },
    News {
        offset: usize,
        limit: usize,
    },
    Competitions {},
    National {
        nation_id: Option<String>,
        #[serde(default)]
        offset: usize,
        #[serde(default = "public_page_limit")]
        limit: usize,
    },
    WorldHistory {
        category: String,
        #[serde(default)]
        offset: usize,
        #[serde(default = "public_page_limit")]
        limit: usize,
    },
    TeamHistory {},
    HistoricalIdentity {
        id: String,
    },
    PlayerStatistics {
        player_id: String,
        offset: usize,
        limit: usize,
    },
    TeamStatistics {
        club_id: String,
        offset: usize,
        limit: usize,
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
    OpenWindow {
        day: u32,
        now_ms: u64,
        deadline_ms: u64,
    },
    Public {},
    Managers {},
    BotPlan {
        actor: String,
        #[serde(default)]
        responses_only: bool,
        #[serde(default)]
        preparation_only: bool,
    },
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

fn public_page_limit() -> usize {
    20
}
fn national_projection(
    game: &Football,
    nation_id: Option<&str>,
    offset: usize,
    limit: usize,
) -> Result<Value, String> {
    let Some(view) = game.national_view() else {
        return Ok(json!({"total":0,"nations":[]}));
    };
    let limit = limit.min(100);
    let metadata = |nation: &domain::national_team::NationalTeam| {
        json!({"id":nation.id,"name":nation.name,
        "football_nation":nation.football_nation,"region_id":nation.region_id,"manager_name":nation.manager_name})
    };
    let Some(id) = nation_id else {
        return Ok(json!({"total":view.national_teams.len(),
        "nations":view.national_teams.iter().skip(offset).take(limit).map(metadata).collect::<Vec<_>>()}));
    };
    let nation = view
        .national_teams
        .iter()
        .find(|n| n.id == id || n.football_nation == id)
        .ok_or("Unknown national team")?;
    let mut fixtures = nation
        .fixtures
        .iter()
        .map(|f| (f.id.clone(), f.clone()))
        .collect::<BTreeMap<_, _>>();
    for league in game.competitions_view().unwrap_or_default().values() {
        for fixture in &league.fixtures {
            if fixture.home_team_id == nation.id || fixture.away_team_id == nation.id {
                fixtures.insert(fixture.id.clone(), fixture.clone());
            }
        }
    }
    let mut fixtures = fixtures.into_values().collect::<Vec<_>>();
    fixtures.sort_by(|a, b| a.date.cmp(&b.date).then(a.id.cmp(&b.id)));
    let today = game
        .career_date()
        .map(|d| d.to_string())
        .unwrap_or_default();
    let players = game
        .public_state()
        .players
        .into_iter()
        .map(|p| (p.id.clone(), p))
        .collect::<BTreeMap<_, _>>();
    Ok(
        json!({"nation":metadata(nation),"squad_player_ids":nation.squad_player_ids.iter().take(100).collect::<Vec<_>>(),
        "total_roster":nation.squad_player_ids.len(),
        "roster":nation.squad_player_ids.iter().take(100).filter_map(|id|players.get(id)).collect::<Vec<_>>(),
        "ranking":view.world_history.national_team_ranking.iter().find(|r|r.nation_code==nation.football_nation),
        "total_fixtures":fixtures.len(),"fixtures":fixtures.iter().skip(offset).take(limit).map(|f| {
            let visible=f.date.as_str()<=today.as_str();
            json!({"id":f.id,"competition_id":f.competition_id,"date":f.date,"matchday":f.matchday,
                "home_team_id":f.home_team_id,"away_team_id":f.away_team_id,
                "status":if visible {serde_json::to_value(&f.status).unwrap()} else {json!("Scheduled")},
                "score":if visible {f.result.as_ref().map(|r|json!({"home_goals":r.home_goals,"away_goals":r.away_goals}))} else {None}})
        }).collect::<Vec<_>>()}),
    )
}

fn world_history_projection(
    game: &Football,
    category: &str,
    offset: usize,
    limit: usize,
) -> Result<Value, String> {
    let Some(view) = game.national_view() else {
        return Ok(json!({"total":0,"rows":[]}));
    };
    let history = serde_json::to_value(view.world_history).map_err(|e| e.to_string())?;
    let rows = history
        .get(category)
        .and_then(Value::as_array)
        .ok_or("Unknown world-history category")?;
    Ok(
        json!({"category":category,"total":rows.len(),"rows":rows.iter().skip(offset).take(limit.min(100)).collect::<Vec<_>>()}),
    )
}

fn execute(game: &mut Option<Football>, input: Input) -> Result<Value, String> {
    if let Input::InitFile { path, deadline_ms } = input {
        if game.is_some() {
            return Err("Already initialized".into());
        }
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut document: Value =
            serde_json::from_reader(io::BufReader::new(file)).map_err(|e| e.to_string())?;
        let mut init = document
            .get_mut("init")
            .ok_or("Scenario must contain init")?
            .take();
        let object = init
            .as_object_mut()
            .ok_or("Scenario init must be an object")?;
        if object.get("op").and_then(Value::as_str) != Some("init") {
            return Err("Scenario must initialize a game".into());
        }
        object.insert("deadline_ms".into(), json!(deadline_ms));
        return execute(
            game,
            serde_json::from_value(init).map_err(|e| e.to_string())?,
        );
    }
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
        training,
        availability,
        squads,
        lineups,
        match_plans,
        social,
        economy,
        personnel,
        team_history,
        news,
        statistics,
        national,
        retired_player_ids,
        competitions,
        market,
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
        if !retired_player_ids.is_empty() && (career.is_none() || social.is_none()) {
            return Err("Retired imports require matching career and source player records".into());
        }
        let mut management = Management::new(clubs, players, managers, day, deadline_ms)
            .map_err(|error| format!("{error:?}"))?;
        if let Some(career) = career {
            management
                .configure_career_with_retired(career, retired_player_ids.clone())
                .map_err(|error| format!("{error:?}"))?;
        }
        if require_match_rosters {
            management
                .require_match_rosters()
                .map_err(|error| format!("{error:?}"))?;
        }
        let mut initialized = Football::new(management, attributes, fixtures)?;
        if let Some(recovery) = recovery {
            if training.is_none() {
                initialized.configure_recovery(recovery)?;
            }
        }
        if let Some(availability) = availability {
            initialized.configure_availability(availability)?;
        }
        if let Some(training) = training {
            initialized.configure_training(training)?;
        }
        if let Some(lineups) = lineups {
            initialized.configure_lineups(lineups)?;
        }
        if let Some(plans) = match_plans {
            initialized.configure_match_plans(plans)?;
        }
        if let Some(squads) = squads {
            initialized.configure_squads(squads.profiles, squads.plans)?;
        }
        if let Some(social) = social {
            initialized.configure_social(social.source_players, social.seed)?;
        }
        if let Some(setup) = national {
            initialized.configure_national(setup)?;
        }
        if let Some(economy) = economy {
            initialized.configure_economy(economy)?;
        }
        if let Some(personnel) = personnel {
            initialized.configure_personnel(personnel.teams, personnel.staff, personnel.seed)?;
        }
        if let Some(competitions) = competitions {
            initialized.configure_competitions(competitions)?;
        }
        if let Some(stats) = statistics {
            initialized.configure_statistics(stats)?;
        }
        if let Some(market) = market {
            initialized.configure_market(market)?;
        }
        if let Some(history) = team_history {
            initialized.configure_team_history(history.managers, history.actor_manager_ids)?;
            initialized.configure_archived_identities(history.archived_identities)?;
        }
        if let Some(setup) = news {
            initialized.configure_news_setup(setup)?;
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
        Input::OpenWindow {
            day,
            now_ms,
            deadline_ms,
        } => {
            game.open_window(day, now_ms, deadline_ms)?;
            Ok(json!({"day":day,"deadline_ms":deadline_ms}))
        }
        Input::News { offset, limit } => Ok(json!({"articles":game.news_view(offset,limit)})),
        Input::Competitions {} => Ok(json!({"competitions":game.competitions_view()})),
        Input::National {
            nation_id,
            offset,
            limit,
        } => national_projection(game, nation_id.as_deref(), offset, limit),
        Input::WorldHistory {
            category,
            offset,
            limit,
        } => world_history_projection(game, &category, offset, limit),
        Input::TeamHistory {} => Ok(json!({"history":game.public_team_history()})),
        Input::HistoricalIdentity { id } => Ok(json!({"identity":game.historical_identity(&id)})),
        Input::PlayerStatistics {
            player_id,
            offset,
            limit,
        } => Ok(json!({"matches":game.player_match_statistics(&player_id,offset,limit)})),
        Input::TeamStatistics {
            club_id,
            offset,
            limit,
        } => Ok(json!({"matches":game.team_match_statistics(&club_id,offset,limit)})),
        Input::StaffMarket { actor } => game
            .staff_market(&actor)
            .map(|staff| json!({"staff":staff}))
            .map_err(|error| format!("{error:?}")),
        Input::TransferMarket { actor, filter } => game
            .browse_transfer_market(&actor, &filter)
            .map(|market| json!(market))
            .map_err(|error| format!("{error:?}")),
        Input::Inbox {
            actor,
            offset,
            limit,
        } => game
            .inbox_view(&actor, offset, limit)
            .map(|messages| json!({"messages":messages}))
            .map_err(|error| format!("{error:?}")),
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
            let optional = |result: Result<Value, Error>| match result {
                Ok(value) => Ok(value),
                Err(Error::Unavailable) => Ok(Value::Null),
                Err(error) => Err(format!("{error:?}")),
            };
            let training = optional(game.training_view(&actor).map(|view| json!(view)))?;
            let availability = optional(game.availability_view(&actor).map(|view| json!(view)))?;
            let squad_plan = optional(game.squad_plan(&actor).map(|view| json!(view)))?;
            let positions = optional(game.position_profiles(&actor).map(|view| json!(view)))?;
            let social = optional(game.social_view(&actor).map(|view| json!(view)))?;
            let inbox = optional(game.inbox_view(&actor, 0, 20).map(|view| json!(view)))?;
            let economy = optional(game.economy_view(&actor).map(|view| json!(view)))?;
            let personnel = optional(game.personnel_view(&actor).map(|view| json!(view)))?;
            let market = optional(game.market_view(&actor).map(|view| json!(view)))?;
            let manager = optional(game.source_manager(&actor).map(|view| json!(view)))?;
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
                "training": training, "availability": availability, "squad_plan": squad_plan, "positions": positions,
                "social": social, "inbox": inbox,
                "economy": economy, "personnel": personnel,
                "market": market, "manager": manager,
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
        Input::BotPlan {
            actor,
            responses_only,
            preparation_only,
        } => {
            let policy = if preparation_only {
                game.bot_preparation_plan(&actor)
            } else {
                game.bot_manager_plan(&actor, responses_only)
            }
            .map_err(|error| format!("{error:?}"))?;
            Ok(json!({"commands":policy.commands,"policy":policy.policy}))
        }
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
        Input::InitFile { .. } => Err("Already initialized".into()),
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
