//! Shared competition lifecycle adapted from pinned OpenFoot 64677fee.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
//! Competition membership is independent of the global club/player registries.
//! The primary league is a display/episode anchor, never the whole world.
use crate::football::{FinishedFixture, Fixture, Football, Standing};
use chrono::Datelike;
use chrono::{Duration, NaiveDate};
use domain::league::{CompactMatchEvent, CompactMatchReport, CompactTeamMatchStats};
use domain::league::{
    CompetitionFormat, CompetitionType, FixtureStatus, GoalEvent, League, MatchResult,
};
use rand::{Rng, RngExt, SeedableRng, rngs::StdRng};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Source promotion assumes disjoint divisions. The immutable world also has
/// Argentina's Apertura/Clausura: separate tables with the SAME clubs and
/// different priorities, not two tiers. Applying swaps there duplicates four
/// clubs and deletes four others. Keep that competition family intact rather
/// than invent promotion between overlapping phases; true disjoint pyramids
/// still execute the pinned swap algorithm unchanged. Prize policy is separate.
fn apply_disjoint_pyramid(divisions: &mut [League]) {
    let mut clubs = BTreeSet::new();
    if divisions
        .iter()
        .flat_map(|c| &c.participant_ids)
        .any(|id| !clubs.insert(id.clone()))
    {
        return;
    }
    crate::promotion::apply_promotion_relegation(divisions);
}

fn compact_team_stats(stats: &engine::TeamStats, possession_pct: u8) -> CompactTeamMatchStats {
    CompactTeamMatchStats {
        possession_pct,
        shots: stats.shots,
        shots_on_target: stats.shots_on_target,
        fouls: stats.fouls,
        corners: stats.corners,
        yellow_cards: stats.yellow_cards,
        red_cards: stats.red_cards,
    }
}

fn compact_report(report: &engine::MatchReport) -> CompactMatchReport {
    let home_possession_pct = report.home_possession.round().clamp(0.0, 100.0) as u8;
    let away_possession_pct = (100.0 - report.home_possession).round().clamp(0.0, 100.0) as u8;

    let events = report
        .events
        .iter()
        .filter(|event| {
            matches!(
                event.event_type,
                engine::EventType::Goal
                    | engine::EventType::PenaltyGoal
                    | engine::EventType::PenaltyMiss
                    | engine::EventType::YellowCard
                    | engine::EventType::RedCard
                    | engine::EventType::SecondYellow
                    | engine::EventType::Injury
                    | engine::EventType::Substitution
            )
        })
        .map(|event| CompactMatchEvent {
            minute: event.minute,
            event_type: format!("{:?}", event.event_type),
            side: format!("{:?}", event.side),
            player_id: event.player_id.clone(),
            secondary_player_id: event.secondary_player_id.clone(),
        })
        .collect();

    CompactMatchReport {
        total_minutes: report.total_minutes,
        home_stats: compact_team_stats(&report.home_stats, home_possession_pct),
        away_stats: compact_team_stats(&report.away_stats, away_possession_pct),
        events,
    }
}

pub(crate) fn simulate_scoreline(
    home_strength: f64,
    away_strength: f64,
    rng: &mut impl Rng,
) -> (u8, u8) {
    let edge = (home_strength - away_strength) / 10.0;
    let home_xg = (1.3 + 0.25 * edge).clamp(0.2, 4.0);
    let away_xg = (1.1 - 0.25 * edge).clamp(0.2, 4.0);
    (sample_goals(home_xg, rng), sample_goals(away_xg, rng))
}

/// A penalty shootout decided from squad strength: five kicks each, then sudden
/// death until one side leads after equal kicks. Returns `(home, away)` — never
/// a tie.
pub(crate) fn simulate_shootout(
    home_strength: f64,
    away_strength: f64,
    rng: &mut impl Rng,
) -> (u8, u8) {
    // Conversion rates nudged a little by the strength edge around a ~0.75 base.
    let edge = (home_strength - away_strength) / 100.0;
    let home_rate = (0.75 + edge).clamp(0.55, 0.92);
    let away_rate = (0.75 - edge).clamp(0.55, 0.92);
    let mut home = 0u8;
    let mut away = 0u8;
    for _ in 0..5 {
        if rng.random_range(0.0..1.0) < home_rate {
            home += 1;
        }
        if rng.random_range(0.0..1.0) < away_rate {
            away += 1;
        }
    }
    while home == away {
        if rng.random_range(0.0..1.0) < home_rate {
            home += 1;
        }
        if rng.random_range(0.0..1.0) < away_rate {
            away += 1;
        }
    }
    (home, away)
}

fn sample_goals(mean: f64, rng: &mut impl Rng) -> u8 {
    let threshold = (-mean).exp();
    let mut goals = 0u8;
    let mut product = 1.0;
    loop {
        product *= rng.random_range(0.0..1.0);
        if product <= threshold || goals >= 9 {
            break;
        }
        goals += 1;
    }
    goals
}
#[path = "competition_groups.rs"]
pub mod groups;
#[path = "competition_qualification.rs"]
pub mod qualification;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompetitionSetup {
    pub seed: u64,
    pub primary_competition_id: String,
    pub competitions: BTreeMap<String, League>,
    pub active_competition_ids: BTreeSet<String>,
    /// Pinned source executes competitions in registry order on overlapping dates.
    #[serde(default)]
    pub competition_order: Vec<String>,
    /// Explicit immutable-import catch-up; consumed by configuration, never a
    /// license for a checkpoint to retain unreachable past scheduled fixtures.
    #[serde(default)]
    pub catch_up_past: bool,
    /// Explicit source football-nation region lookup, required for continental top-up.
    #[serde(default)]
    pub club_regions: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompetitionState {
    pub setup: CompetitionSetup,
    pub epoch_date: NaiveDate,
    pub epoch_day: u32,
    pub archives: Vec<CompetitionArchive>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompetitionArchive {
    pub competitions: BTreeMap<String, League>,
    pub summary: crate::seasons::SeasonArchive,
}

fn fixture_seed(seed: u64, id: &str) -> u64 {
    id.bytes().fold(seed ^ 0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}
fn league_ended(c: &League) -> bool {
    c.rules.format == CompetitionFormat::LeagueTable
        && c.fixtures
            .iter()
            .any(|f| f.counts_for_league_standings() && f.status == FixtureStatus::Completed)
        && !c
            .fixtures
            .iter()
            .any(|f| f.counts_for_league_standings() && f.status != FixtureStatus::Completed)
}
fn next_start(anchor: NaiveDate, month: u8, day: u8) -> Result<NaiveDate, String> {
    let same = NaiveDate::from_ymd_opt(anchor.year(), month.into(), day.into())
        .ok_or("Invalid competition start date")?;
    if same >= anchor {
        return Ok(same);
    }
    NaiveDate::from_ymd_opt(
        anchor
            .year()
            .checked_add(1)
            .ok_or("Competition year overflow")?,
        month.into(),
        day.into(),
    )
    .ok_or("Invalid next competition start date".into())
}
fn row(source: &domain::league::StandingEntry) -> Standing {
    Standing {
        club_id: source.team_id.clone(),
        played: source.played,
        won: source.won,
        drawn: source.drawn,
        lost: source.lost,
        goals_for: source.goals_for,
        goals_against: source.goals_against,
        points: source.points,
    }
}

impl Football {
    /// Automatic multiplayer owns the EndSeason timing: do not implicitly press
    /// the desktop's manual discard button while an active season-flow cup runs.
    /// Foreign mid-season league tables never block the primary league rollover.
    pub(crate) fn settle_competition_season(
        &mut self,
    ) -> Result<Option<crate::seasons::SeasonArchive>, String> {
        let Some(state) = &self.competitions else {
            return Ok(None);
        };
        let primary = &state.setup.competitions[&state.setup.primary_competition_id];
        let primary_games: Vec<_> = primary
            .fixtures
            .iter()
            .filter(|f| f.counts_for_league_standings())
            .collect();
        let n = primary.participant_ids.len();
        if n < 2
            || primary_games.len() != n * (n - 1)
            || primary_games
                .iter()
                .any(|f| f.status != FixtureStatus::Completed)
        {
            return Ok(None);
        }
        if state.setup.competitions.iter().any(|(id, c)| {
            state.setup.active_competition_ids.contains(id)
                && c.kind != CompetitionType::InternationalNation
                && c.rules.counts_in_season_flow
                && c.rules.format != CompetitionFormat::LeagueTable
                && (c.fixtures.is_empty()
                    || c.fixtures
                        .iter()
                        .any(|f| f.status != FixtureStatus::Completed))
        }) {
            return Ok(None);
        }
        let mut staged = self.clone();
        let archive = staged.roll_competitions()?;
        *self = staged;
        Ok(Some(archive))
    }

    fn roll_competitions(&mut self) -> Result<crate::seasons::SeasonArchive, String> {
        let today = self.career_date().ok_or("Competition career unavailable")?;
        let completed_date = today
            .pred_opt()
            .ok_or("Competition completion date overflow")?;
        let completed_day = self
            .management
            .window
            .day
            .checked_sub(1)
            .ok_or("Competition completion day overflow")?;
        let state = self.competitions.as_ref().unwrap();
        let old = state.setup.competitions.clone();
        let primary_id = state.setup.primary_competition_id.clone();
        let season = old[&primary_id].season;
        let ended: BTreeSet<_> = old
            .iter()
            .filter(|(_, c)| c.kind != CompetitionType::InternationalNation && league_ended(c))
            .map(|(id, _)| id.clone())
            .collect();
        let retired: BTreeSet<_> = old
            .iter()
            .filter(|(id, c)| {
                c.kind != CompetitionType::InternationalNation
                    && (ended.contains(*id) || c.rules.format != CompetitionFormat::LeagueTable)
            })
            .map(|(id, _)| id.clone())
            .collect();
        let retired_ids: BTreeSet<_> = old
            .iter()
            .filter(|(id, _)| retired.contains(*id))
            .flat_map(|(_, c)| c.fixtures.iter().map(|f| f.id.clone()))
            .collect();
        let fixtures: Vec<_> = self
            .fixtures
            .iter()
            .filter(|f| retired_ids.contains(&f.id))
            .cloned()
            .collect();
        let results: Vec<_> = self
            .results
            .iter()
            .filter(|f| retired_ids.contains(&f.fixture_id))
            .cloned()
            .collect();
        let standings: Vec<_> = old[&primary_id]
            .sorted_standings()
            .iter()
            .map(row)
            .collect();
        let reputations_before = self.management.career.as_ref().unwrap().reputations.clone();
        let mut reputations_after = reputations_before.clone();
        let mut prizes = BTreeMap::new();
        let mut countries: BTreeMap<String, Vec<League>> = BTreeMap::new();
        let mut standalone = vec![];
        for id in &ended {
            let league = old[id].clone();
            if let Some(country) = &league.country_id {
                countries.entry(country.clone()).or_default().push(league);
            } else {
                standalone.push(league);
            }
        }
        for leagues in countries.values_mut() {
            leagues.sort_by(|a, b| (a.priority, &a.id).cmp(&(b.priority, &b.id)));
        }
        let divisions: Vec<_> = countries
            .values()
            .flat_map(|cs| cs.iter().enumerate())
            .chain(standalone.iter().map(|c| (0, c)))
            .collect();
        for (tier, league) in divisions {
            let mut expected = league.participant_ids.clone();
            expected.sort_by(|a, b| {
                reputations_before[b]
                    .cmp(&reputations_before[a])
                    .then_with(|| {
                        self.management.clubs[a]
                            .name
                            .cmp(&self.management.clubs[b].name)
                    })
                    .then_with(|| a.cmp(b))
            });
            for (index, standing) in league.sorted_standings().iter().enumerate() {
                let position = index + 1;
                let amount = [
                    5_000_000i64,
                    3_000_000,
                    1_500_000,
                    750_000,
                    400_000,
                    300_000,
                    250_000,
                    200_000,
                    175_000,
                    150_000,
                ]
                .get(index)
                .copied()
                .unwrap_or(150_000)
                .checked_shr(tier as u32)
                .unwrap_or(0);
                let club = &standing.team_id;
                let balance = &mut self
                    .management
                    .clubs
                    .get_mut(club)
                    .ok_or("Prize club unavailable")?
                    .balance;
                *balance = balance
                    .checked_add(amount)
                    .ok_or("Prize balance overflow")?;
                let revision = self
                    .management
                    .club_revisions
                    .get_mut(club)
                    .ok_or("Prize revision unavailable")?;
                *revision = revision.checked_add(1).ok_or("Prize revision overflow")?;
                let total = prizes.entry(club.clone()).or_insert(0i64);
                *total = total.checked_add(amount).ok_or("Prize total overflow")?;
                self.management
                    .career
                    .as_mut()
                    .unwrap()
                    .ledger
                    .push(crate::career::LedgerEntry {
                        date: completed_date,
                        club_id: club.clone(),
                        amount,
                        reason: format!(
                            "season-{}-{}-prize-position-{position}",
                            league.season, league.id
                        ),
                    });
                let expected_position = expected
                    .iter()
                    .position(|id| id == club)
                    .ok_or("Missing expected club")?
                    + 1;
                let reputation = (reputations_before[club] as i32
                    + 12 * (expected_position as i32 - position as i32)
                    + if position == 1 { 12 } else { 0 }
                    - if position == league.participant_ids.len() {
                        8
                    } else {
                        0
                    })
                .clamp(0, 1000) as u32;
                reputations_after.insert(club.clone(), reputation);
            }
        }
        let finance = self.management.season_board_finances()?;
        let settled_clubs: BTreeSet<_> = ended
            .iter()
            .flat_map(|id| old[id].participant_ids.iter().cloned())
            .collect();
        let mut manager_outcomes = BTreeMap::new();
        if let Some(boards) = &mut self.boards {
            for manager in self
                .management
                .managers
                .values()
                .filter(|m| settled_clubs.contains(&m.club_id))
            {
                let league = ended
                    .iter()
                    .map(|id| &old[id])
                    .filter(|c| c.participant_ids.contains(&manager.club_id))
                    .min_by_key(|c| (c.priority, &c.id))
                    .ok_or("Missing settled manager league")?;
                let rows = league.sorted_standings();
                let (position, row) = rows
                    .iter()
                    .enumerate()
                    .find(|(_, r)| r.team_id == manager.club_id)
                    .ok_or("Missing settled club standing")?;
                let finance = finance
                    .get(&manager.club_id)
                    .ok_or("Missing settled board finance")?;
                let board = boards.get_mut(&manager.id).ok_or("Missing settled board")?;
                manager_outcomes.insert(
                    manager.id.clone(),
                    board.state.evaluate_season(
                        position as u32 + 1,
                        row.won,
                        row.goals_for,
                        finance.wage_usage_percent,
                        finance.in_debt,
                    )?,
                );
            }
        }
        self.management.career.as_mut().unwrap().reputations = reputations_after.clone();
        self.update_board_reputations(&reputations_after)?;
        let fields = qualification::fields(
            &old.values().cloned().collect::<Vec<_>>(),
            &reputations_after,
            &self.competitions.as_ref().unwrap().setup.club_regions,
        );
        let mut updated = old.clone();
        for divisions in countries.values_mut() {
            apply_disjoint_pyramid(divisions);
            for league in divisions {
                updated.get_mut(&league.id).unwrap().participant_ids =
                    league.participant_ids.clone();
            }
        }
        for (id, entrants) in fields {
            if entrants.len() >= 2 {
                updated.get_mut(&id).unwrap().participant_ids = entrants;
            }
        }
        let anchor = completed_date
            .checked_add_signed(Duration::days(28))
            .ok_or("Competition anchor overflow")?;
        for (id, league) in &mut updated {
            if !retired.contains(id) {
                continue;
            }
            let start = next_start(anchor, league.season_start_month, league.season_start_day)?;
            let season = u32::try_from(start.year()).map_err(|_| "Invalid competition season")?;
            let start = start.and_hms_opt(0, 0, 0).unwrap().and_utc();
            match league.rules.format {
                CompetitionFormat::LeagueTable => {
                    crate::competition_schedule::regenerate_league_for_season(league, season, start)
                }
                CompetitionFormat::Knockout => {
                    crate::competition_schedule::regenerate_knockout_for_season(
                        league, season, start,
                    )
                }
                CompetitionFormat::GroupAndKnockout => {
                    groups::regenerate_for_season(league, season, start)
                }
            }
        }
        let next_season = updated[&primary_id].season;
        if next_season <= season {
            return Err("Competition season did not advance".into());
        }
        self.management
            .settle_economy_season(completed_date, season, next_season, &prizes)?;
        let mut club_seasons = BTreeMap::new();
        for id in &ended {
            let league = &old[id];
            let standings: Vec<_> = league.sorted_standings().iter().map(row).collect();
            self.settle_competition_history(id, league.season, completed_date, &standings)?;
            for club in &league.participant_ids {
                club_seasons
                    .entry(club.clone())
                    .or_insert((id.clone(), league.season));
            }
        }
        self.settle_competition_players(season, completed_date, &club_seasons)?;
        self.competitions.as_mut().unwrap().setup.competitions = updated;
        self.sync_primary_standings();
        let mut boards = self.boards.clone();
        if let Some(boards) = &mut boards {
            for manager in self
                .management
                .managers
                .values()
                .filter(|m| settled_clubs.contains(&m.club_id))
            {
                let size = self
                    .club_competition_standings(&manager.club_id)
                    .ok_or("Missing next manager league")?
                    .len() as u32;
                let board = boards
                    .get_mut(&manager.id)
                    .ok_or("Missing next manager board")?;
                board.state.reset_objectives(board.reputation, size)?;
            }
        }
        self.boards = boards;
        self.roll_national_calendar(anchor)?;
        self.refresh_competition_schedule()?;
        let next_start_date = self.competitions.as_ref().unwrap().setup.competitions[&primary_id]
            .fixtures
            .iter()
            .filter(|f| f.counts_for_league_standings())
            .map(|f| NaiveDate::parse_from_str(&f.date, "%Y-%m-%d").unwrap())
            .min()
            .ok_or("Missing next primary schedule")?;
        let next_first_day = self
            .management
            .window
            .day
            .checked_add(
                u32::try_from((next_start_date - today).num_days())
                    .map_err(|_| "Next competition backwards")?,
            )
            .ok_or("Next competition day overflow")?;
        let summary = crate::seasons::SeasonArchive {
            season,
            completed_day,
            completed_date,
            next_first_day,
            next_start_date,
            fixtures,
            results,
            standings,
            prizes,
            reputations_before,
            reputations_after,
            manager_outcomes,
        };
        self.competitions
            .as_mut()
            .unwrap()
            .archives
            .push(CompetitionArchive {
                competitions: old,
                summary: summary.clone(),
            });
        Ok(summary)
    }
    pub fn configure_competitions(&mut self, setup: CompetitionSetup) -> Result<(), String> {
        if self.started
            || self.management.sequence != 0
            || self.competitions.is_some()
            || self.seasons.is_some()
            || !self.fixtures.is_empty()
        {
            return Err("Competitions require an empty initial calendar and cannot replace an active season".into());
        }
        let date = self
            .career_date()
            .ok_or("Competitions require career dates")?;
        let mut staged = self.clone();
        staged.competitions = Some(CompetitionState {
            setup,
            epoch_date: date,
            epoch_day: self.management.window.day,
            archives: vec![],
        });
        staged.validate_competition_checkpoint()?;
        if staged.competitions.as_ref().unwrap().setup.catch_up_past {
            staged.catch_up_competitions(date)?;
            staged.competitions.as_mut().unwrap().setup.catch_up_past = false;
            staged.validate_competition_checkpoint()?;
        }
        staged.refresh_competition_schedule()?;
        staged.sync_primary_standings();
        *self = staged;
        Ok(())
    }

    pub fn competitions_view(&self) -> Option<BTreeMap<String, League>> {
        self.competitions
            .as_ref()
            .map(|s| s.setup.competitions.clone())
    }
    pub(crate) fn primary_competition_contains_fixture(&self, id: &str) -> bool {
        self.competitions.as_ref().map_or(true, |s| {
            s.setup.competitions[&s.setup.primary_competition_id]
                .fixtures
                .iter()
                .any(|f| f.id == id && f.counts_for_league_standings())
        })
    }
    pub(crate) fn club_competition_standings(&self, club: &str) -> Option<Vec<Standing>> {
        let s = self.competitions.as_ref()?;
        s.setup
            .competitions
            .values()
            .filter(|c| {
                c.kind == CompetitionType::League && c.participant_ids.iter().any(|id| id == club)
            })
            .min_by_key(|c| (c.priority, &c.id))
            .map(|c| c.sorted_standings().iter().map(row).collect())
    }
    pub(crate) fn competition_fixture_is_knockout(&self, id: &str) -> bool {
        self.competitions.as_ref().is_some_and(|s| {
            s.setup
                .competitions
                .values()
                .any(|c| c.is_knockout_fixture(id))
        })
    }
    fn sync_primary_standings(&mut self) {
        if let Some(s) = &self.competitions {
            self.standings = s.setup.competitions[&s.setup.primary_competition_id]
                .standings
                .iter()
                .map(|r| (r.team_id.clone(), row(r)))
                .collect();
        }
    }

    pub(crate) fn source_primary_standings(&self) -> Option<Vec<Standing>> {
        let state = self.competitions.as_ref()?;
        Some(
            state.setup.competitions[&state.setup.primary_competition_id]
                .sorted_standings()
                .iter()
                .map(row)
                .collect(),
        )
    }

    /// Rebuild only the engine calendar. Dormant fixtures remain authoritative
    /// domain scorelines and never receive fabricated full engine reports.
    pub(crate) fn refresh_competition_schedule(&mut self) -> Result<(), String> {
        let Some(s) = &self.competitions else {
            return Ok(());
        };
        let mut output = vec![];
        let order: Vec<_> = if s.setup.competition_order.is_empty() {
            s.setup.competitions.keys().cloned().collect()
        } else {
            s.setup.competition_order.clone()
        };
        for id in &order {
            let c = &s.setup.competitions[id];
            if !s.setup.active_competition_ids.contains(id)
                || c.kind == CompetitionType::InternationalNation
            {
                continue;
            }
            for f in &c.fixtures {
                let date = NaiveDate::parse_from_str(&f.date, "%Y-%m-%d")
                    .map_err(|_| "Invalid competition date")?;
                let delta = (date - s.epoch_date).num_days();
                if delta < 0 {
                    continue;
                }
                let day = s
                    .epoch_day
                    .checked_add(u32::try_from(delta).map_err(|_| "Competition day overflow")?)
                    .ok_or("Competition day overflow")?;
                // Pinned turn/mod.rs runs every due competition sequentially,
                // even if a club has league and cup fixtures on the same date.
                // Do not invent a rescheduling algorithm or reject source data.
                output.push(Fixture {
                    id: f.id.clone(),
                    day,
                    home: f.home_team_id.clone(),
                    away: f.away_team_id.clone(),
                    seed: fixture_seed(s.setup.seed, &f.id),
                });
            }
        }
        // Stable sort retains source competition/fixture order within each date.
        output.sort_by_key(|f| f.day);
        self.fixtures = output;
        Ok(())
    }

    pub(crate) fn complete_competition_fixture(
        &mut self,
        finished: &FinishedFixture,
    ) -> Result<(), String> {
        let Some(s) = &mut self.competitions else {
            return Ok(());
        };
        let (competition, index) = s
            .setup
            .competitions
            .values_mut()
            .find_map(|c| {
                c.fixtures
                    .iter()
                    .position(|f| f.id == finished.fixture_id)
                    .map(|i| (c, i))
            })
            .ok_or("Unknown competition fixture")?;
        let fixture = &competition.fixtures[index];
        if fixture.status != FixtureStatus::Scheduled
            || fixture.home_team_id != finished.home
            || fixture.away_team_id != finished.away
        {
            return Err("Competition fixture already completed or mismatched".into());
        }
        let r = &finished.report;
        if competition.is_knockout_fixture(&fixture.id)
            && r.home_goals == r.away_goals
            && !matches!((r.home_penalties,r.away_penalties),(Some(a),Some(b)) if a!=b)
        {
            return Err("Knockout draw requires a completed engine shootout".into());
        }
        let result = MatchResult {
            home_goals: r.home_goals,
            away_goals: r.away_goals,
            home_scorers: r
                .goals
                .iter()
                .filter(|g| g.side == engine::Side::Home)
                .map(|g| GoalEvent {
                    player_id: g.scorer_id.clone(),
                    minute: g.minute,
                })
                .collect(),
            away_scorers: r
                .goals
                .iter()
                .filter(|g| g.side == engine::Side::Away)
                .map(|g| GoalEvent {
                    player_id: g.scorer_id.clone(),
                    minute: g.minute,
                })
                .collect(),
            report: Some(compact_report(r)),
            home_penalties: r.home_penalties,
            away_penalties: r.away_penalties,
        };
        record_result(competition, index, result);
        Ok(())
    }

    pub(crate) fn advance_dormant_competitions(&mut self, today: NaiveDate) -> Result<(), String> {
        let Some(state) = &self.competitions else {
            return Ok(());
        };
        let strengths: BTreeMap<_, _> = self
            .management
            .clubs
            .keys()
            .map(|club| {
                let mut ratings: Vec<_> = self
                    .management
                    .players
                    .values()
                    .filter(|p| &p.club_id == club)
                    .map(|p| self.attributes[&p.id].ovr)
                    .collect();
                ratings.sort_unstable_by(|a, b| b.cmp(a));
                let n = ratings.len().min(11);
                (
                    club.clone(),
                    if n == 0 {
                        50.0
                    } else {
                        ratings.iter().take(n).map(|x| f64::from(*x)).sum::<f64>() / n as f64
                    },
                )
            })
            .collect();
        let seed = state.setup.seed;
        let state = self.competitions.as_mut().unwrap();
        for (id, competition) in &mut state.setup.competitions {
            if state.setup.active_competition_ids.contains(id)
                || competition.kind == CompetitionType::InternationalNation
            {
                continue;
            }
            let due: Vec<_> = competition
                .fixtures
                .iter()
                .enumerate()
                .filter(|(_, f)| {
                    f.date == today.to_string() && f.status == FixtureStatus::Scheduled
                })
                .map(|(i, _)| i)
                .collect();
            for index in due {
                let f = &competition.fixtures[index];
                let home = strengths[&f.home_team_id];
                let away = strengths[&f.away_team_id];
                let mut rng = StdRng::seed_from_u64(fixture_seed(seed, &f.id));
                let (hg, ag) = simulate_scoreline(home, away, &mut rng);
                let pens = (hg == ag && competition.is_knockout_fixture(&f.id))
                    .then(|| simulate_shootout(home, away, &mut rng));
                record_result(
                    competition,
                    index,
                    MatchResult {
                        home_goals: hg,
                        away_goals: ag,
                        home_penalties: pens.map(|p| p.0),
                        away_penalties: pens.map(|p| p.1),
                        ..Default::default()
                    },
                );
            }
        }
        Ok(())
    }

    fn catch_up_competitions(&mut self, cutoff: NaiveDate) -> Result<(), String> {
        let strengths: BTreeMap<_, _> = self
            .management
            .clubs
            .keys()
            .map(|club| {
                let mut ratings: Vec<_> = self
                    .management
                    .players
                    .values()
                    .filter(|p| &p.club_id == club)
                    .map(|p| self.attributes[&p.id].ovr)
                    .collect();
                ratings.sort_unstable_by(|a, b| b.cmp(a));
                let n = ratings.len().min(11);
                (
                    club.clone(),
                    if n == 0 {
                        50.0
                    } else {
                        ratings.iter().take(n).map(|x| f64::from(*x)).sum::<f64>() / n as f64
                    },
                )
            })
            .collect();
        let state = self.competitions.as_mut().unwrap();
        let seed = state.setup.seed;
        for competition in state.setup.competitions.values_mut() {
            if competition.kind == CompetitionType::InternationalNation {
                continue;
            }
            // Use the pinned catchup scoreline path, including new overdue rounds
            // produced by its bracket advancement, until the imported day is reachable.
            for _ in 0..128 {
                let due: Vec<_> = competition
                    .fixtures
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| {
                        f.status == FixtureStatus::Scheduled
                            && NaiveDate::parse_from_str(&f.date, "%Y-%m-%d")
                                .is_ok_and(|d| d < cutoff)
                    })
                    .map(|(i, _)| i)
                    .collect();
                if due.is_empty() {
                    break;
                }
                for index in due {
                    let f = &competition.fixtures[index];
                    let home = strengths[&f.home_team_id];
                    let away = strengths[&f.away_team_id];
                    let mut rng =
                        StdRng::seed_from_u64(fixture_seed(seed ^ 0x6361_7463_6875_7021, &f.id));
                    let (hg, ag) = simulate_scoreline(home, away, &mut rng);
                    let pens = (hg == ag && competition.is_knockout_fixture(&f.id))
                        .then(|| simulate_shootout(home, away, &mut rng));
                    record_result(
                        competition,
                        index,
                        MatchResult {
                            home_goals: hg,
                            away_goals: ag,
                            home_penalties: pens.map(|p| p.0),
                            away_penalties: pens.map(|p| p.1),
                            ..Default::default()
                        },
                    );
                }
            }
        }
        Ok(())
    }

    pub(crate) fn validate_competition_checkpoint(&self) -> Result<(), String> {
        let Some(s) = &self.competitions else {
            return Ok(());
        };
        if s.setup.competitions.is_empty()
            || s.setup.competitions.len() > 256
            || s.epoch_day == 0
            || self.career_date().is_none()
        {
            return Err("Invalid competition setup".into());
        }
        let primary = s
            .setup
            .competitions
            .get(&s.setup.primary_competition_id)
            .ok_or("Unknown primary competition")?;
        if primary.kind != CompetitionType::League
            || primary.rules.format != CompetitionFormat::LeagueTable
            || !s.setup.active_competition_ids.contains(&primary.id)
        {
            return Err("Primary must be an active domestic league table".into());
        }
        if s.setup
            .active_competition_ids
            .iter()
            .any(|id| !s.setup.competitions.contains_key(id))
        {
            return Err("Unknown active competition".into());
        }
        if !s.setup.competition_order.is_empty() {
            let order: BTreeSet<_> = s.setup.competition_order.iter().collect();
            if order.len() != s.setup.competition_order.len()
                || order.into_iter().ne(s.setup.competitions.keys())
            {
                return Err(
                    "Competition execution order must name every competition exactly once".into(),
                );
            }
        }
        if s.setup
            .competitions
            .values()
            .map(|c| c.fixtures.len())
            .sum::<usize>()
            > 200_000
        {
            return Err("Competition fixture collection exceeds bound".into());
        }
        if s.setup
            .competitions
            .values()
            .any(|c| c.kind == CompetitionType::ContinentalClub)
            && s.setup.club_regions.keys().ne(self.management.clubs.keys())
        {
            return Err(
                "Continental qualification requires an explicit source region for every club"
                    .into(),
            );
        }
        for collection in
            std::iter::once(&s.setup.competitions).chain(s.archives.iter().map(|a| &a.competitions))
        {
            let mut fixture_ids = BTreeSet::new();
            for (id, c) in collection {
                if id.is_empty()
                    || id != &c.id
                    || c.season == 0
                    || !(2..=128).contains(&c.participant_ids.len())
                    || c.groups.len() > 128
                    || c.knockout_rounds.len() > 128
                    || !(1..=128).contains(&c.rules.group_qualifiers_per_group)
                    || c.rules.group_best_third_qualifiers > 128
                    || !(1..=128).contains(&c.rules.knockout_matches_per_day)
                    || c.fixtures.len() > 65536
                    || c.rules.knockout_round_gap_days == 0
                {
                    return Err("Invalid competition identity or bounds".into());
                }
                if c.kind == CompetitionType::InternationalNation && self.national.is_none() {
                    return Err(
                        "National-team records require the national lifecycle, not the club engine"
                            .into(),
                    );
                }
                let members: BTreeSet<_> = c.participant_ids.iter().collect();
                if members.len() != c.participant_ids.len()
                    || members.iter().any(|id| {
                        if c.kind == CompetitionType::InternationalNation {
                            !self
                                .national
                                .as_ref()
                                .unwrap()
                                .setup
                                .national_teams
                                .iter()
                                .any(|t| &t.id == *id)
                        } else {
                            !self.management.clubs.contains_key(*id)
                        }
                    })
                {
                    return Err(
                        "Competition membership must reference unique registered clubs".into(),
                    );
                }
                if !(1..=3660).contains(&c.rules.knockout_round_gap_days)
                    || !(1..=3660).contains(&c.rules.group_matchday_gap_days)
                    || !(1..=4).contains(&c.rules.group_stage_legs)
                    || NaiveDate::from_ymd_opt(
                        2001,
                        c.season_start_month.into(),
                        c.season_start_day.into(),
                    )
                    .is_none()
                {
                    return Err("Invalid competition calendar bounds".into());
                }
                let mut expected: BTreeMap<_, _> = c
                    .participant_ids
                    .iter()
                    .map(|id| (id.clone(), domain::league::StandingEntry::new(id.clone())))
                    .collect();
                for f in &c.fixtures {
                    let date = NaiveDate::parse_from_str(&f.date, "%Y-%m-%d")
                        .map_err(|_| "Invalid competition fixture date")?;
                    if date.year_ce().1 > 9000
                        || !fixture_ids.insert(&f.id)
                        || f.id.is_empty()
                        || !members.contains(&f.home_team_id)
                        || !members.contains(&f.away_team_id)
                        || f.home_team_id == f.away_team_id
                    {
                        return Err("Invalid competition fixture identity or participants".into());
                    }
                    if (f.status == FixtureStatus::Completed) != f.result.is_some()
                        || f.status == FixtureStatus::InProgress
                    {
                        return Err("Competition fixture status/result mismatch".into());
                    }
                    if f.status == FixtureStatus::Scheduled
                        && date < s.epoch_date
                        && !s.setup.catch_up_past
                    {
                        return Err("Past scheduled fixtures require explicit catch-up".into());
                    }
                    if f.counts_for_league_standings() {
                        if let Some(result) = &f.result {
                            expected
                                .get_mut(&f.home_team_id)
                                .unwrap()
                                .record_result(result.home_goals, result.away_goals);
                            expected
                                .get_mut(&f.away_team_id)
                                .unwrap()
                                .record_result(result.away_goals, result.home_goals);
                        }
                    }
                }
                if c.rules.format == CompetitionFormat::LeagueTable {
                    let actual: BTreeMap<_, _> =
                        c.standings.iter().map(|r| (r.team_id.clone(), r)).collect();
                    if actual.len() != c.standings.len()
                        || actual.keys().ne(expected.keys())
                        || actual.iter().any(|(id, r)| {
                            serde_json::to_value(r).unwrap()
                                != serde_json::to_value(&expected[id]).unwrap()
                        })
                    {
                        return Err("Competition standings do not match completed results".into());
                    }
                }
                let mut group_members = BTreeSet::new();
                let mut group_ids = BTreeSet::new();
                for group in &c.groups {
                    if !group_ids.insert(&group.id)
                        || group.id.is_empty()
                        || group
                            .team_ids
                            .iter()
                            .any(|id| !members.contains(id) || !group_members.insert(id))
                    {
                        return Err("Invalid competition group membership".into());
                    }
                    let mut expected: BTreeMap<_, _> = group
                        .team_ids
                        .iter()
                        .map(|id| (id.clone(), domain::league::StandingEntry::new(id.clone())))
                        .collect();
                    for fixture in c.fixtures.iter().filter(|f| {
                        !c.is_knockout_fixture(&f.id) && group.team_ids.contains(&f.home_team_id)
                    }) {
                        if !group.team_ids.contains(&fixture.away_team_id) {
                            return Err("Group fixture crosses groups".into());
                        }
                        if let Some(result) = &fixture.result {
                            expected
                                .get_mut(&fixture.home_team_id)
                                .unwrap()
                                .record_result(result.home_goals, result.away_goals);
                            expected
                                .get_mut(&fixture.away_team_id)
                                .unwrap()
                                .record_result(result.away_goals, result.home_goals);
                        }
                    }
                    let actual: BTreeMap<_, _> = group
                        .standings
                        .iter()
                        .map(|r| (r.team_id.clone(), r))
                        .collect();
                    if actual.len() != group.standings.len()
                        || actual.keys().ne(expected.keys())
                        || actual.iter().any(|(id, r)| {
                            serde_json::to_value(r).unwrap()
                                != serde_json::to_value(&expected[id]).unwrap()
                        })
                    {
                        return Err("Group standings do not match completed results".into());
                    }
                }
                if c.rules.format == CompetitionFormat::GroupAndKnockout && group_members != members
                {
                    return Err("Group competition membership incomplete".into());
                }
                for round in &c.knockout_rounds {
                    if round
                        .fixture_ids
                        .iter()
                        .any(|id| !c.fixtures.iter().any(|f| &f.id == id))
                        || round.bye_team_ids.iter().any(|id| !members.contains(id))
                    {
                        return Err("Knockout references unknown participants or fixtures".into());
                    }
                    if round.fixture_ids.iter().collect::<BTreeSet<_>>().len()
                        != round.fixture_ids.len()
                        || (round.completed
                            && round.fixture_ids.iter().any(|id| {
                                c.fixtures.iter().find(|f| &f.id == id).unwrap().status
                                    != FixtureStatus::Completed
                            }))
                    {
                        return Err("Invalid knockout completion state".into());
                    }
                }
            }
        }
        Ok(())
    }
}

fn record_result(c: &mut League, index: usize, result: MatchResult) {
    let f = &mut c.fixtures[index];
    f.status = FixtureStatus::Completed;
    if f.counts_for_league_standings() {
        for (club, gf, ga) in [
            (&f.home_team_id, result.home_goals, result.away_goals),
            (&f.away_team_id, result.away_goals, result.home_goals),
        ] {
            if let Some(row) = c.standings.iter_mut().find(|r| &r.team_id == club) {
                row.record_result(gf, ga);
            }
        }
    }
    f.result = Some(result);
    groups::process_completed_fixture(c, index);
    crate::competition_schedule::advance_knockout_competition_round(c);
}

#[cfg(test)]
mod tests {
    #[test]
    fn overlapping_apertura_clausura_are_not_a_promotion_pyramid() {
        let ids: Vec<_> = (0..20).map(|i| format!("ar-{i}")).collect();
        let mut apertura = League::new("ar-d1-apertura".into(), "Apertura".into(), 2026, &ids);
        let mut clausura = League::new("ar-d1-clausura".into(), "Clausura".into(), 2026, &ids);
        apertura.priority = 0;
        clausura.priority = 1;
        for (i, row) in apertura.standings.iter_mut().enumerate() {
            row.points = i as u32;
        }
        for (i, row) in clausura.standings.iter_mut().enumerate() {
            row.points = (20 - i) as u32;
        }
        let mut phases = vec![apertura, clausura];
        let before = serde_json::to_value(&phases).unwrap();
        apply_disjoint_pyramid(&mut phases);
        assert_eq!(before, serde_json::to_value(&phases).unwrap());
        let lower_ids: Vec<_> = (0..20).map(|i| format!("lower-{i}")).collect();
        let lower = League::new("lower".into(), "Lower".into(), 2026, &lower_ids);
        let mut disjoint = vec![phases.remove(0), lower];
        let mut expected = disjoint.clone();
        crate::promotion::apply_promotion_relegation(&mut expected);
        apply_disjoint_pyramid(&mut disjoint);
        assert_eq!(
            serde_json::to_value(&expected).unwrap(),
            serde_json::to_value(&disjoint).unwrap()
        );
    }
    use super::*;
    use crate::career::CareerSetup;
    use crate::contracts::PlayerContract;
    use crate::{Club, Management, Manager, Player};
    use domain::league::CompetitionScope;
    fn date(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }
    fn league(id: &str, clubs: &[&str], start: &str, country: &str, priority: u32) -> League {
        let members = clubs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let mut league = crate::competition_schedule::generate_league(
            id,
            2026,
            &members,
            date(start).and_hms_opt(0, 0, 0).unwrap().and_utc(),
        );
        league.country_id = Some(country.into());
        league.priority = priority;
        league
    }
    fn game() -> Football {
        let clubs = ["a", "b", "c", "d", "e", "f"];
        let mut players = vec![];
        let mut attrs = vec![];
        let mut contracts = BTreeMap::new();
        for club in clubs {
            for i in 0..11 {
                let id = format!("{club}-p{i}");
                players.push(Player {
                    id: id.clone(),
                    name: id.clone(),
                    club_id: club.into(),
                });
                let position = match i {
                    0 => "Goalkeeper",
                    1..=4 => "Defender",
                    5..=8 => "Midfielder",
                    _ => "Forward",
                };
                attrs.push(serde_json::from_value(serde_json::json!({"id":id,"name":id,"position":position,"ovr":60,"condition":100,"fitness":80,"pace":60,"stamina":60,"strength":60,"agility":60,"passing":60,"shooting":60,"tackling":60,"dribbling":60,"defending":60,"positioning":60,"vision":60,"decisions":60,"composure":60,"aggression":60,"teamwork":60,"leadership":60,"handling":60,"reflexes":60,"aerial":60,"traits":[],"role":"Standard"})).unwrap());
                contracts.insert(
                    id,
                    PlayerContract::new(
                        date("2000-01-01"),
                        0,
                        Some(date("2029-08-01")),
                        100000,
                        70,
                        50,
                    ),
                );
            }
        }
        let management = Management::new(
            clubs
                .map(|id| Club {
                    id: id.into(),
                    name: id.into(),
                    balance: 1_000_000,
                })
                .to_vec(),
            players,
            clubs
                .map(|id| Manager {
                    id: format!("m-{id}"),
                    club_id: id.into(),
                })
                .to_vec(),
            1,
            100,
        )
        .unwrap();
        let mut game = Football::new(management, attrs, vec![]).unwrap();
        game.configure_career(CareerSetup {
            today: date("2026-08-01"),
            contracts,
            wage_budgets: clubs.map(|id| (id.into(), 100_000)).into(),
            reputations: clubs.map(|id| (id.into(), 500)).into(),
            staff_annual_wages: BTreeMap::new(),
        })
        .unwrap();
        game
    }
    fn setup() -> CompetitionSetup {
        let upper = league("upper", &["a", "b"], "2026-08-01", "ENG", 0);
        let lower = league("lower", &["c", "d"], "2026-08-01", "ENG", 1);
        let foreign = league("foreign", &["e", "f"], "2026-08-05", "BRA", 0);
        let mut cup = crate::competition_schedule::generate_knockout_cup(
            "cup",
            2026,
            &["a".into(), "b".into()],
            date("2026-08-10").and_hms_opt(0, 0, 0).unwrap().and_utc(),
            CompetitionType::Cup,
            CompetitionScope::Domestic,
        );
        cup.season_start_day = 10;
        CompetitionSetup {
            seed: 7,
            primary_competition_id: upper.id.clone(),
            active_competition_ids: [upper.id.clone(), lower.id.clone(), cup.id.clone()].into(),
            competitions: [upper, lower, foreign, cup]
                .into_iter()
                .map(|c| (c.id.clone(), c))
                .collect(),
            club_regions: Default::default(),
            competition_order: vec![],
            catch_up_past: false,
        }
    }
    #[test]
    fn tied_primary_table_preserves_source_order_instead_of_legacy_id_order() {
        let mut game = game();
        assert_eq!(game.standings()[0].club_id, "a");
        let mut setup = setup();
        setup
            .competitions
            .get_mut(&setup.primary_competition_id)
            .unwrap()
            .standings
            .reverse();
        game.configure_competitions(setup).unwrap();
        assert_eq!(
            game.standings()
                .iter()
                .map(|r| r.club_id.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "a"]
        );
        assert_eq!(game.source_primary_standings().unwrap(), game.standings());
        let restored = Football::load_validated(game.save_state().unwrap()).unwrap();
        assert_eq!(restored.standings(), game.standings());
    }

    #[test]
    fn memberships_are_not_global_and_dormant_has_no_full_report_or_player_effects() {
        let mut game = game();
        game.configure_competitions(setup()).unwrap();
        assert_eq!(game.standings.len(), 2);
        assert_eq!(game.club_competition_standings("c").unwrap().len(), 2);
        let before = serde_json::to_value(&game.attributes).unwrap();
        game.advance_dormant_competitions(date("2026-08-05"))
            .unwrap();
        let foreign = game
            .competitions
            .as_ref()
            .unwrap()
            .setup
            .competitions
            .values()
            .find(|c| c.name == "foreign")
            .unwrap();
        assert_eq!(foreign.standings.iter().map(|r| r.played).sum::<u32>(), 2);
        assert!(
            foreign.fixtures[0]
                .result
                .as_ref()
                .unwrap()
                .report
                .is_none()
        );
        assert_eq!(serde_json::to_value(&game.attributes).unwrap(), before);
        assert!(game.results.is_empty());
        let once = serde_json::to_value(&game.competitions).unwrap();
        game.advance_dormant_competitions(date("2026-08-05"))
            .unwrap();
        assert_eq!(serde_json::to_value(&game.competitions).unwrap(), once);
    }
    #[test]
    fn live_calendar_waits_for_cup_promotes_and_preserves_midseason_foreign_league() {
        let mut game = game();
        game.configure_competitions(setup()).unwrap();
        for day in 1..=8 {
            game.advance_closed_day(day, u64::from(day) * 100, u64::from(day + 1) * 100)
                .unwrap();
        }
        assert!(
            game.competitions.as_ref().unwrap().archives.is_empty(),
            "cup still awaits final"
        );
        assert_eq!(
            game.standings.values().map(|s| s.played).sum::<u32>(),
            4,
            "other leagues cannot enter primary standings"
        );
        for day in 9..=10 {
            game.advance_closed_day(day, u64::from(day) * 100, u64::from(day + 1) * 100)
                .unwrap();
        }
        let state = game.competitions.as_ref().unwrap();
        assert_eq!(state.archives.len(), 1);
        let archive = &state.archives[0];
        let oldupper = &archive.competitions[&state.setup.primary_competition_id];
        let oldlower = archive
            .competitions
            .values()
            .find(|c| c.name == "lower")
            .unwrap();
        let upper = &state.setup.competitions[&state.setup.primary_competition_id];
        assert_eq!(upper.season, 2027);
        assert!(
            upper
                .participant_ids
                .contains(&oldlower.sorted_standings()[0].team_id)
        );
        assert!(
            !upper
                .participant_ids
                .contains(&oldupper.sorted_standings().last().unwrap().team_id)
        );
        let foreign = state
            .setup
            .competitions
            .values()
            .find(|c| c.name == "foreign")
            .unwrap();
        assert_eq!(foreign.season, 2026);
        assert!(
            foreign
                .fixtures
                .iter()
                .any(|f| f.status == FixtureStatus::Completed)
        );
        assert!(
            foreign
                .fixtures
                .iter()
                .any(|f| f.status == FixtureStatus::Scheduled)
        );
        assert_eq!(archive.summary.prizes.values().sum::<i64>(), 12_000_000);
        let saved = game.save_state().unwrap();
        let restored = Football::load_validated(saved.clone()).unwrap();
        assert_eq!(restored.save_state().unwrap(), saved);
    }
    #[test]
    fn conflicts_invalid_dates_and_national_club_routing_are_rejected_atomically() {
        let mut game = game();
        let original = serde_json::to_value(&game.management).unwrap();
        let mut config = setup();
        let cup = config
            .competitions
            .values_mut()
            .find(|c| c.kind == CompetitionType::Cup)
            .unwrap();
        cup.fixtures[0].date = "2026-08-01".into();
        game.configure_competitions(config).unwrap();
        assert_eq!(
            game.fixtures
                .iter()
                .filter(|f| f.day == 1 && (f.home == "a" || f.away == "a"))
                .count(),
            2
        );
        let mut game = self::game();
        let mut config = setup();
        config.competitions.values_mut().next().unwrap().fixtures[0].date = "bad".into();
        assert!(game.configure_competitions(config).is_err());
        assert!(game.competitions.is_none());
        assert_eq!(serde_json::to_value(&game.management).unwrap(), original);
        let mut config = setup();
        config
            .competitions
            .values_mut()
            .find(|c| c.kind == CompetitionType::Cup)
            .unwrap()
            .kind = CompetitionType::InternationalNation;
        assert!(game.configure_competitions(config).is_err());
    }

    #[test]
    fn group_knockout_advances_real_winners_and_cup_never_updates_league_table() {
        let clubs: Vec<_> = (0..8).map(|n| format!("c{n}")).collect();
        let mut cup = groups::generate_group_knockout_cup(
            "groupcup",
            2026,
            &clubs,
            date("2026-08-01").and_hms_opt(0, 0, 0).unwrap().and_utc(),
            CompetitionType::ContinentalClub,
            CompetitionScope::Continental,
        );
        let initial = cup.fixtures.len();
        assert!(initial > 0);
        for _ in 0..100 {
            let Some(index) = cup
                .fixtures
                .iter()
                .position(|f| f.status == FixtureStatus::Scheduled)
            else {
                break;
            };
            record_result(
                &mut cup,
                index,
                MatchResult {
                    home_goals: 0,
                    away_goals: 1,
                    ..Default::default()
                },
            );
        }
        assert!(cup.fixtures.len() > initial);
        assert!(
            cup.fixtures
                .iter()
                .all(|f| f.status == FixtureStatus::Completed)
        );
        assert!(qualification::champion(&cup).is_some());
        assert!(cup.standings.is_empty());
        assert!(
            cup.groups
                .iter()
                .all(|g| g.standings.iter().all(|r| r.played == 6))
        );
    }

    #[test]
    fn friendly_does_not_count_in_table_and_shootout_never_ties() {
        let mut l = league("friendly", &["a", "b"], "2026-08-01", "ENG", 0);
        l.fixtures[0].competition = domain::league::FixtureCompetition::Friendly;
        record_result(
            &mut l,
            0,
            MatchResult {
                home_goals: 8,
                away_goals: 0,
                ..Default::default()
            },
        );
        assert!(l.standings.iter().all(|r| r.played == 0));
        for seed in 0..100 {
            let (a, b) = simulate_shootout(60.0, 60.0, &mut StdRng::seed_from_u64(seed));
            assert_ne!(a, b);
        }
    }

    #[test]
    #[ignore = "read-only diagnostic requires the immutable local OpenFoot export"]
    fn immutable_actual_english_calendar_loads_without_rescheduling() {
        let path = std::env::var("OPENFOOT_SOURCE_COMPETITIONS")
            .expect("set immutable competitions shard path");
        let all: Vec<League> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let comps: Vec<_> = all
            .into_iter()
            .filter(|c| {
                c.country_id.as_deref() == Some("ENG")
                    && (c.kind == CompetitionType::League || c.kind == CompetitionType::Cup)
            })
            .collect();
        let members: BTreeSet<_> = comps
            .iter()
            .flat_map(|c| c.participant_ids.iter().cloned())
            .collect();
        let management = Management::new(
            members
                .iter()
                .map(|id| Club {
                    id: id.clone(),
                    name: id.clone(),
                    balance: 0,
                })
                .collect(),
            vec![],
            members
                .iter()
                .map(|id| Manager {
                    id: format!("m:{id}"),
                    club_id: id.clone(),
                })
                .collect(),
            1,
            100,
        )
        .unwrap();
        let mut game = Football::new(management, vec![], vec![]).unwrap();
        game.configure_career(CareerSetup {
            today: date("2026-06-01"),
            contracts: Default::default(),
            wage_budgets: members.iter().map(|id| (id.clone(), 0)).collect(),
            reputations: members.iter().map(|id| (id.clone(), 500)).collect(),
            staff_annual_wages: Default::default(),
        })
        .unwrap();
        let original = serde_json::to_value(&comps).unwrap();
        game.configure_competitions(CompetitionSetup {
            seed: 1,
            primary_competition_id: "eng-d1".into(),
            competition_order: comps.iter().map(|c| c.id.clone()).collect(),
            active_competition_ids: comps.iter().map(|c| c.id.clone()).collect(),
            competitions: comps.iter().map(|c| (c.id.clone(), c.clone())).collect(),
            club_regions: Default::default(),
            catch_up_past: false,
        })
        .unwrap();
        let stored: Vec<_> = game
            .competitions
            .as_ref()
            .unwrap()
            .setup
            .competition_order
            .iter()
            .map(|id| game.competitions.as_ref().unwrap().setup.competitions[id].clone())
            .collect();
        assert_eq!(serde_json::to_value(stored).unwrap(), original);
        let mut seen = BTreeSet::new();
        let mut overlaps = 0;
        for f in &game.fixtures {
            for club in [&f.home, &f.away] {
                if !seen.insert((f.day, club)) {
                    overlaps += 1;
                }
            }
        }
        assert!(
            overlaps > 0,
            "source league/cup overlap must remain executable"
        );
        eprintln!(
            "immutable English calendar: {} clubs, {} competitions, {} fixtures, {overlaps} repeated club-day slots",
            members.len(),
            comps.len(),
            game.fixtures.len()
        );
    }

    #[test]
    fn explicit_import_catchup_resolves_past_calendar_without_fabricating_player_stats() {
        let mut game = game();
        let mut setup = setup();
        let foreign = setup
            .competitions
            .values_mut()
            .find(|c| c.name == "foreign")
            .unwrap();
        foreign.fixtures[0].date = "2026-07-01".into();
        foreign.fixtures[1].date = "2026-07-08".into();
        assert!(game.configure_competitions(setup.clone()).is_err());
        let attrs = serde_json::to_value(&game.attributes).unwrap();
        setup.catch_up_past = true;
        game.configure_competitions(setup).unwrap();
        let state = game.competitions.as_ref().unwrap();
        assert!(!state.setup.catch_up_past);
        let foreign = state
            .setup
            .competitions
            .values()
            .find(|c| c.name == "foreign")
            .unwrap();
        assert!(
            foreign
                .fixtures
                .iter()
                .all(|f| f.status == FixtureStatus::Completed
                    && f.result.as_ref().unwrap().report.is_none())
        );
        assert_eq!(foreign.standings.iter().map(|r| r.played).sum::<u32>(), 4);
        assert!(game.results.is_empty());
        assert_eq!(serde_json::to_value(&game.attributes).unwrap(), attrs);
    }

    #[test]
    fn overlapping_source_dates_execute_in_declared_order_and_carry_wear() {
        let mut game = game();
        let mut setup = setup();
        let cup = setup
            .competitions
            .values_mut()
            .find(|c| c.kind == CompetitionType::Cup)
            .unwrap();
        cup.fixtures[0].date = "2026-08-01".into();
        let cup_id = cup.id.clone();
        let lower = setup
            .competitions
            .values()
            .find(|c| c.name == "lower")
            .unwrap()
            .id
            .clone();
        let foreign = setup
            .competitions
            .values()
            .find(|c| c.name == "foreign")
            .unwrap()
            .id
            .clone();
        setup.competition_order = vec![
            setup.primary_competition_id.clone(),
            lower,
            cup_id.clone(),
            foreign,
        ];
        game.configure_competitions(setup).unwrap();
        let expected: Vec<_> = game
            .fixtures
            .iter()
            .filter(|f| f.day == 1)
            .map(|f| f.id.clone())
            .collect();
        game.advance_closed_day(1, 100, 200).unwrap();
        assert_eq!(
            game.results
                .iter()
                .map(|r| r.fixture_id.clone())
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(game.results.len(), 3);
        assert!(game.attributes["a-p0"].condition < game.attributes["c-p0"].condition);
        assert_eq!(game.standings.values().map(|r| r.played).sum::<u32>(), 2);
        let saved = game.save_state().unwrap();
        assert!(Football::load_validated(saved).is_ok());
    }
}
