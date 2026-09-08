//! Single domestic league rollover adapted from OpenFoot Manager's
//! `ofm_core/src/end_of_season.rs`, `reputation.rs`, and
//! `generator/competition_def.rs`, revision
//! 64677fee9047a1182005d666bafa5dbc025dca5c.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! The date keeps advancing one day at a time through the off-season, so wages
//! and expiry are never skipped. Player assets are not recreated. Aging,
//! retirement, youth generation, cups and promotion/relegation are not implemented.
//! Manager outcomes in archives are PRIVATE and require public projection.

use crate::football::{FinishedFixture, Fixture, Football, Standing};
use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeasonSetup {
    pub season: u32,
    pub season_start_month: u8,
    pub season_start_day: u8,
    pub spacing_days: u32,
    pub seed: u64,
    /// Top flight is zero; each lower tier halves prize money.
    pub division_tier: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SeasonArchive {
    pub season: u32,
    pub completed_day: u32,
    pub completed_date: NaiveDate,
    pub next_first_day: u32,
    pub next_start_date: NaiveDate,
    pub fixtures: Vec<Fixture>,
    pub results: Vec<FinishedFixture>,
    pub standings: Vec<Standing>,
    pub prizes: BTreeMap<String, i64>,
    pub reputations_before: BTreeMap<String, u32>,
    pub reputations_after: BTreeMap<String, u32>,
    /// Private board evaluation; never include this field in spectator responses.
    pub manager_outcomes: BTreeMap<String, crate::board::SeasonOutcome>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SeasonState {
    pub setup: SeasonSetup,
    pub archives: Vec<SeasonArchive>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublicSeasonArchive {
    pub season: u32,
    pub completed_day: u32,
    pub completed_date: NaiveDate,
    pub standings: Vec<Standing>,
    pub manager_dismissals: Vec<crate::football::Dismissal>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublicSeasonState {
    pub season: u32,
    pub first_day: u32,
    pub last_day: u32,
    pub completed_seasons: usize,
}

fn validate_schedule(fixtures: &[Fixture], clubs: &BTreeSet<String>) -> Result<(), String> {
    if !(2..=128).contains(&clubs.len()) || fixtures.len() != clubs.len() * (clubs.len() - 1) {
        return Err("Season requires a complete double round robin".into());
    }
    let mut pairs = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut club_days = BTreeSet::new();
    for f in fixtures {
        if !clubs.contains(&f.home)
            || !clubs.contains(&f.away)
            || f.home == f.away
            || f.id.trim().is_empty()
            || !ids.insert(&f.id)
            || !pairs.insert((&f.home, &f.away))
            || !club_days.insert((f.day, &f.home))
            || !club_days.insert((f.day, &f.away))
        {
            return Err("Season contains duplicate, missing, or conflicting pairings".into());
        }
    }
    Ok(())
}

fn next_start(after: NaiveDate, month: u8, day: u8) -> Result<NaiveDate, String> {
    let same_year = NaiveDate::from_ymd_opt(after.year(), month.into(), day.into())
        .ok_or("Invalid season start date")?;
    if same_year >= after {
        return Ok(same_year);
    }
    NaiveDate::from_ymd_opt(
        after.year().checked_add(1).ok_or("Season year overflow")?,
        month.into(),
        day.into(),
    )
    .ok_or_else(|| "Invalid next season start date".into())
}

fn prize(position: usize, tier: u32) -> i64 {
    const PAYOUTS: [i64; 10] = [
        5_000_000, 3_000_000, 1_500_000, 750_000, 400_000, 300_000, 250_000, 200_000, 175_000,
        150_000,
    ];
    PAYOUTS.get(position - 1).copied().unwrap_or(150_000) >> tier
}

impl Football {
    pub(crate) fn validate_season_checkpoint(&self) -> Result<(), String> {
        let Some(state) = &self.seasons else {
            return Ok(());
        };
        let clubs = self
            .management
            .clubs
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        validate_schedule(&self.fixtures, &clubs)?;
        let today = self
            .management
            .career_date()
            .ok_or("Season checkpoint lacks career date")?;
        if state.setup.season == 0
            || state.setup.spacing_days == 0
            || state.setup.division_tier > 62
            || NaiveDate::from_ymd_opt(
                2001,
                state.setup.season_start_month.into(),
                state.setup.season_start_day.into(),
            )
            .is_none()
        {
            return Err("Invalid season checkpoint setup".into());
        }
        let all_results: BTreeMap<_, _> = self.results.iter().map(|r| (&r.fixture_id, r)).collect();
        let mut ids = BTreeSet::new();
        let mut previous: Option<&SeasonArchive> = None;
        for archive in &state.archives {
            validate_schedule(&archive.fixtures, &clubs)?;
            if archive.season >= state.setup.season
                || archive.results.len() != archive.fixtures.len()
                || archive.completed_day != archive.fixtures.iter().map(|f| f.day).max().unwrap()
                || archive.completed_day >= self.management.window.day
                || archive.completed_date >= archive.next_start_date
                || archive.completed_day >= archive.next_first_day
            {
                return Err("Invalid archived season chronology".into());
            }
            let elapsed = self
                .management
                .window
                .day
                .checked_sub(archive.completed_day)
                .ok_or("Archive day ahead of clock")?;
            if today.checked_sub_signed(Duration::days(elapsed.into()))
                != Some(archive.completed_date)
                || archive
                    .completed_date
                    .checked_add_signed(Duration::days(i64::from(
                        archive.next_first_day - archive.completed_day,
                    )))
                    != Some(archive.next_start_date)
            {
                return Err("Archived dates disagree with monotonic career clock".into());
            }
            if let Some(prev) = previous {
                if archive.season <= prev.season
                    || archive.completed_day <= prev.completed_day
                    || archive.fixtures.iter().map(|f| f.day).min() != Some(prev.next_first_day)
                    || archive.reputations_before != prev.reputations_after
                {
                    return Err("Archived seasons are not a continuous sequence".into());
                }
            }
            let expected: BTreeSet<_> = archive.fixtures.iter().map(|f| f.id.as_str()).collect();
            let mut seen = BTreeSet::new();
            for result in &archive.results {
                if !expected.contains(result.fixture_id.as_str())
                    || !seen.insert(&result.fixture_id)
                    || !ids.insert(result.fixture_id.clone())
                    || all_results.get(&result.fixture_id).is_none_or(|global| {
                        serde_json::to_value(global).ok() != serde_json::to_value(result).ok()
                    })
                {
                    return Err("Archived results do not match canonical trajectory".into());
                }
            }
            for map_keys in [
                archive.prizes.keys().cloned().collect::<BTreeSet<_>>(),
                archive.reputations_before.keys().cloned().collect(),
                archive.reputations_after.keys().cloned().collect(),
            ] {
                if map_keys != clubs {
                    return Err("Archived club records are incomplete".into());
                }
            }
            if archive
                .reputations_before
                .values()
                .chain(archive.reputations_after.values())
                .any(|rep| *rep > 1000)
                || archive
                    .standings
                    .iter()
                    .map(|s| s.club_id.clone())
                    .collect::<BTreeSet<_>>()
                    != clubs
                || archive.standings.len() != clubs.len()
                || archive.standings.iter().enumerate().any(|(i, s)| {
                    s.played as usize != 2 * (clubs.len() - 1)
                        || archive.prizes[&s.club_id] != prize(i + 1, state.setup.division_tier)
                })
            {
                return Err("Invalid archived standings or settlement".into());
            }
            for standing in &archive.standings {
                let mut expected = Standing {
                    club_id: standing.club_id.clone(),
                    played: 0,
                    won: 0,
                    drawn: 0,
                    lost: 0,
                    goals_for: 0,
                    goals_against: 0,
                    points: 0,
                };
                for result in &archive.results {
                    let goals = if result.home == standing.club_id {
                        Some((result.report.home_goals, result.report.away_goals))
                    } else if result.away == standing.club_id {
                        Some((result.report.away_goals, result.report.home_goals))
                    } else {
                        None
                    };
                    if let Some((gf, ga)) = goals {
                        expected.played += 1;
                        expected.goals_for += u32::from(gf);
                        expected.goals_against += u32::from(ga);
                        match gf.cmp(&ga) {
                            std::cmp::Ordering::Greater => {
                                expected.won += 1;
                                expected.points += 3;
                            }
                            std::cmp::Ordering::Equal => {
                                expected.drawn += 1;
                                expected.points += 1;
                            }
                            std::cmp::Ordering::Less => expected.lost += 1,
                        }
                    }
                }
                if &expected != standing {
                    return Err("Archived standings disagree with completed results".into());
                }
            }
            previous = Some(archive);
        }
        if let Some(previous) = previous {
            if self.fixtures.iter().map(|f| f.day).min() != Some(previous.next_first_day)
                || state.setup.season
                    != u32::try_from(previous.next_start_date.year())
                        .map_err(|_| "Invalid season year")?
            {
                return Err("Current season disagrees with last rollover".into());
            }
        }
        if self.fixtures.iter().any(|f| ids.contains(&f.id)) {
            return Err("Current fixture IDs collide with archived seasons".into());
        }
        Ok(())
    }

    pub fn configure_seasons(&mut self, setup: SeasonSetup) -> Result<(), String> {
        if self.started || self.seasons.is_some() || self.management.sequence != 0 {
            return Err(
                "Seasons can only be configured once before commands or day processing".into(),
            );
        }
        let today = self
            .management
            .career_date()
            .ok_or("Seasons require configured career dates")?;
        if setup.spacing_days == 0 || setup.season == 0 || setup.division_tier > 62 {
            return Err("Invalid season setup".into());
        }
        next_start(today, setup.season_start_month, setup.season_start_day)?;
        // Reject dates that cannot recur every year rather than inventing a leap-day rule.
        NaiveDate::from_ymd_opt(
            2001,
            setup.season_start_month.into(),
            setup.season_start_day.into(),
        )
        .ok_or("Season start must exist every year")?;
        let clubs: BTreeSet<_> = self.management.clubs.keys().cloned().collect();
        validate_schedule(&self.fixtures, &clubs)?;
        let career = self.management.career.as_ref().ok_or("Missing career")?;
        if career.reputations.keys().cloned().collect::<BTreeSet<_>>() != clubs
            || career.reputations.values().any(|rep| *rep > 1000)
        {
            return Err("Season reputations must cover every club in 0..=1000".into());
        }
        self.seasons = Some(SeasonState {
            setup,
            archives: Vec::new(),
        });
        Ok(())
    }

    pub fn season_state(&self) -> Option<&SeasonState> {
        self.seasons.as_ref()
    }

    pub fn season_history(&self) -> &[SeasonArchive] {
        self.seasons
            .as_ref()
            .map(|s| s.archives.as_slice())
            .unwrap_or(&[])
    }

    pub fn public_season_state(&self) -> Option<PublicSeasonState> {
        self.seasons.as_ref().map(|s| PublicSeasonState {
            season: s.setup.season,
            first_day: self
                .fixtures
                .iter()
                .map(|f| f.day)
                .min()
                .unwrap_or(self.management.window.day),
            last_day: self
                .fixtures
                .iter()
                .map(|f| f.day)
                .max()
                .unwrap_or(self.management.window.day),
            completed_seasons: s.archives.len(),
        })
    }

    pub fn public_season_history(&self) -> Vec<PublicSeasonArchive> {
        let mut previous_end = None;
        self.season_history()
            .iter()
            .map(|archive| {
                let public = PublicSeasonArchive {
                    season: archive.season,
                    completed_day: archive.completed_day,
                    completed_date: archive.completed_date,
                    standings: archive.standings.clone(),
                    manager_dismissals: self
                        .dismissals()
                        .iter()
                        .filter(|d| {
                            d.day <= archive.completed_day
                                && previous_end.is_none_or(|end| d.day > end)
                        })
                        .cloned()
                        .collect(),
                };
                previous_end = Some(archive.completed_day);
                public
            })
            .collect()
    }

    /// Atomic and replay-safe: a successful settlement replaces the current
    /// calendar with an unplayed one, so another call cannot pay it twice.
    pub fn settle_completed_season(&mut self) -> Result<Option<SeasonArchive>, String> {
        if self.seasons.is_none() {
            return Ok(None);
        }
        let mut staged = self.clone();
        let archive = staged.settle_completed_season_inner()?;
        if archive.is_some() {
            *self = staged;
        }
        Ok(archive)
    }

    fn settle_completed_season_inner(&mut self) -> Result<Option<SeasonArchive>, String> {
        let state = self.seasons.as_ref().ok_or("Missing seasons")?;
        let setup = state.setup.clone();
        let clubs: BTreeSet<_> = self.management.clubs.keys().cloned().collect();
        validate_schedule(&self.fixtures, &clubs)?;
        let fixture_ids: BTreeSet<_> = self.fixtures.iter().map(|f| f.id.as_str()).collect();
        let results: Vec<_> = self
            .results
            .iter()
            .filter(|r| fixture_ids.contains(r.fixture_id.as_str()))
            .cloned()
            .collect();
        if results.len() < self.fixtures.len() {
            return Ok(None);
        }
        let unique_results: BTreeSet<_> = results.iter().map(|r| &r.fixture_id).collect();
        if unique_results.len() != fixture_ids.len() || results.len() != fixture_ids.len() {
            return Err("Season result IDs must match the completed schedule exactly".into());
        }
        for result in &results {
            let f = self
                .fixtures
                .iter()
                .find(|f| f.id == result.fixture_id)
                .ok_or("Unknown result")?;
            if (result.day, &result.home, &result.away) != (f.day, &f.home, &f.away) {
                return Err("Completed fixture does not match its schedule".into());
            }
        }
        let completed_day = self
            .fixtures
            .iter()
            .map(|f| f.day)
            .max()
            .ok_or("Empty season")?;
        let current_day = self.management.window.day;
        let today = self.management.career_date().ok_or("Missing career date")?;
        let elapsed = current_day
            .checked_sub(completed_day)
            .ok_or("Completion is ahead of the career clock")?;
        let completed_date = today
            .checked_sub_signed(Duration::days(elapsed.into()))
            .ok_or("Completion date overflow")?;
        let anchor = completed_date
            .checked_add_signed(Duration::days(28))
            .ok_or("Rollover date overflow")?;
        let next_start_date = next_start(anchor, setup.season_start_month, setup.season_start_day)?;
        let offset: u32 = (next_start_date - today)
            .num_days()
            .try_into()
            .map_err(|_| "Next season would move backwards")?;
        let next_first_day = current_day
            .checked_add(offset)
            .ok_or("Next season day overflow")?;
        let next_season: u32 = next_start_date
            .year()
            .try_into()
            .map_err(|_| "Invalid season year")?;
        if next_season <= setup.season {
            return Err("Next season identifier must increase".into());
        }
        let mut next_fixtures = crate::calendar::double_round_robin(
            &clubs.iter().cloned().collect::<Vec<_>>(),
            next_first_day,
            setup.spacing_days,
            setup.seed ^ u64::from(next_season),
        )?;
        for fixture in &mut next_fixtures {
            fixture.id = format!("season-{next_season}:{}", fixture.id);
        }
        let historic_ids: BTreeSet<_> =
            self.results.iter().map(|r| r.fixture_id.as_str()).collect();
        if next_fixtures
            .iter()
            .any(|f| historic_ids.contains(f.id.as_str()))
        {
            return Err("Next season fixture ID already exists".into());
        }
        let standings = self.standings();
        if standings.len() != clubs.len()
            || standings
                .iter()
                .any(|s| s.played as usize != 2 * (clubs.len() - 1))
        {
            return Err("Season standings do not cover a complete schedule".into());
        }
        let reputations_before = self
            .management
            .career
            .as_ref()
            .ok_or("Missing career")?
            .reputations
            .clone();
        let mut expected: Vec<_> = self.management.clubs.values().collect();
        expected.sort_by(|a, b| {
            reputations_before[&b.id]
                .cmp(&reputations_before[&a.id])
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.id.cmp(&b.id))
        });
        let expected: BTreeMap<_, _> = expected
            .iter()
            .enumerate()
            .map(|(i, c)| (c.id.clone(), i as i32 + 1))
            .collect();
        let mut reputations_after = BTreeMap::new();
        let mut prizes = BTreeMap::new();
        for (index, standing) in standings.iter().enumerate() {
            let position = index + 1;
            let amount = prize(position, setup.division_tier);
            let club = self
                .management
                .clubs
                .get_mut(&standing.club_id)
                .ok_or("Unknown club")?;
            club.balance = club
                .balance
                .checked_add(amount)
                .ok_or("Prize balance overflow")?;
            let revision = self
                .management
                .club_revisions
                .get_mut(&club.id)
                .ok_or("Missing club revision")?;
            *revision = revision.checked_add(1).ok_or("Prize revision overflow")?;
            prizes.insert(club.id.clone(), amount);
            self.management
                .career
                .as_mut()
                .ok_or("Missing career")?
                .ledger
                .push(crate::career::LedgerEntry {
                    date: completed_date,
                    club_id: club.id.clone(),
                    amount,
                    reason: format!("season-{}-prize-position-{position}", setup.season),
                });
            let new_rep = (reputations_before[&club.id] as i32
                + 12 * (expected[&club.id] - position as i32)
                + if position == 1 { 12 } else { 0 }
                - if position == clubs.len() { 8 } else { 0 })
            .clamp(0, 1000) as u32;
            reputations_after.insert(club.id.clone(), new_rep);
        }
        let finance = self.management.season_board_finances()?;
        let manager_outcomes = self.evaluate_season_boards(&finance)?;
        self.management
            .career
            .as_mut()
            .ok_or("Missing career")?
            .reputations = reputations_after.clone();
        self.update_board_reputations(&reputations_after)?;
        self.reset_board_objectives()?;
        let archive = SeasonArchive {
            season: setup.season,
            completed_day,
            completed_date,
            next_first_day,
            next_start_date,
            fixtures: self.fixtures.clone(),
            results,
            standings,
            prizes,
            reputations_before,
            reputations_after,
            manager_outcomes,
        };
        self.fixtures = next_fixtures;
        for row in self.standings.values_mut() {
            *row = Standing {
                club_id: row.club_id.clone(),
                played: 0,
                won: 0,
                drawn: 0,
                lost: 0,
                goals_for: 0,
                goals_against: 0,
                points: 0,
            };
        }
        let state = self.seasons.as_mut().ok_or("Missing seasons")?;
        state.setup.season = next_season;
        state.archives.push(archive.clone());
        Ok(Some(archive))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_pairings_required_not_just_the_fixture_count() {
        let clubs = ["a".to_owned(), "b".to_owned()].into_iter().collect();
        let mut fixtures =
            crate::calendar::double_round_robin(&["a".into(), "b".into()], 1, 7, 1).unwrap();
        assert!(validate_schedule(&fixtures, &clubs).is_ok());
        fixtures[1].home = "a".into();
        fixtures[1].away = "b".into();
        assert!(validate_schedule(&fixtures, &clubs).is_err());
        assert!(validate_schedule(&fixtures[..1], &clubs).is_err());
    }
    #[test]
    fn upstream_prizes_tiers_and_anchor_are_exact() {
        assert_eq!(prize(1, 0), 5_000_000);
        assert_eq!(prize(2, 1), 1_500_000);
        assert_eq!(prize(12, 2), 37_500);
        let anchor = NaiveDate::from_ymd_opt(2027, 8, 1).unwrap();
        assert_eq!(next_start(anchor, 8, 1).unwrap(), anchor);
        assert_eq!(
            next_start(anchor + Duration::days(1), 8, 1).unwrap(),
            NaiveDate::from_ymd_opt(2028, 8, 1).unwrap()
        );
    }

    fn game() -> Football {
        game_with_satisfaction(75)
    }

    fn game_with_satisfaction(initial_satisfaction: u8) -> Football {
        use crate::{Club, Management, Manager, Player};
        let mut attributes = vec![];
        for club in ["a", "b"] {
            for i in 0..12 {
                attributes.push(engine::PlayerData {
                    id: format!("{club}-{i}"),
                    name: format!("{club}-{i}"),
                    position: match i {
                        0 => engine::Position::Goalkeeper,
                        1..=4 => engine::Position::Defender,
                        5..=8 => engine::Position::Midfielder,
                        _ => engine::Position::Forward,
                    },
                    role: engine::PlayerRole::Standard,
                    traits: vec![],
                    ovr: 65,
                    condition: 100,
                    fitness: 75,
                    pace: 65,
                    stamina: 65,
                    strength: 65,
                    agility: 65,
                    passing: 65,
                    shooting: 65,
                    tackling: 65,
                    dribbling: 65,
                    defending: 65,
                    positioning: 65,
                    vision: 65,
                    decisions: 65,
                    composure: 65,
                    aggression: 65,
                    teamwork: 65,
                    leadership: 65,
                    handling: 65,
                    reflexes: 65,
                    aerial: 65,
                });
            }
        }
        let players = attributes
            .iter()
            .map(|p| Player {
                id: p.id.clone(),
                name: p.name.clone(),
                club_id: p.id[..1].into(),
            })
            .collect();
        let management = Management::new(
            ["a", "b"]
                .map(|id| Club {
                    id: id.into(),
                    name: id.into(),
                    balance: 1_000_000,
                })
                .to_vec(),
            players,
            ["a", "b"]
                .map(|id| Manager {
                    id: id.into(),
                    club_id: id.into(),
                })
                .to_vec(),
            1,
            1000,
        )
        .unwrap();
        let fixtures =
            crate::calendar::double_round_robin(&["a".into(), "b".into()], 1, 1, 73).unwrap();
        let mut game = Football::new(management, attributes, fixtures).unwrap();
        let contracts = game
            .attributes
            .keys()
            .map(|id| {
                (
                    id.clone(),
                    crate::contracts::PlayerContract::new(
                        NaiveDate::from_ymd_opt(2001, 1, 1).unwrap(),
                        5200,
                        Some(NaiveDate::from_ymd_opt(2035, 8, 1).unwrap()),
                        100_000,
                        60,
                        60,
                    ),
                )
            })
            .collect();
        game.configure_career(crate::career::CareerSetup {
            staff_annual_wages: BTreeMap::new(),
            today: NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(),
            contracts,
            wage_budgets: [("a".into(), 1_000_000), ("b".into(), 1_000_000)].into(),
            reputations: [("a".into(), 500), ("b".into(), 500)].into(),
        })
        .unwrap();
        game.configure_boards(
            ["a", "b"]
                .map(|id| {
                    (
                        id.into(),
                        crate::football::BoardProfile {
                            reputation: 500,
                            initial_satisfaction,
                        },
                    )
                })
                .into(),
        )
        .unwrap();
        game.configure_seasons(SeasonSetup {
            season: 2026,
            season_start_month: 8,
            season_start_day: 1,
            spacing_days: 1,
            seed: 73,
            division_tier: 0,
        })
        .unwrap();
        game
    }

    fn advance(game: &mut Football) {
        let window = game.window();
        game.advance_closed_day(window.day, window.deadline_ms, window.deadline_ms + 1000)
            .unwrap();
    }

    #[test]
    fn two_seasons_preserve_assets_wages_receipts_plans_and_archive_once() {
        use crate::{Command, Request};
        let mut game = game();
        let plan = crate::tactics::MatchPlan {
            play_style: engine::PlayStyle::HighPress,
            ..Default::default()
        };
        let request = Request {
            id: "persistent-plan".into(),
            day: 1,
            command: Command::SetMatchPlan { plan: plan.clone() },
        };
        let receipt = game.dispatch("a", request.clone(), 10).unwrap();
        let contracts = game.management.career.as_ref().unwrap().contracts.clone();
        let owners = game.public_state().players;
        advance(&mut game);
        advance(&mut game);
        assert_eq!(game.season_history().len(), 1);
        assert_eq!(game.season_history()[0].season, 2026);
        assert_eq!(game.season_history()[0].results.len(), 2);
        assert_eq!(
            game.season_history()[0].prizes.values().sum::<i64>(),
            8_000_000
        );
        assert!(game.standings().iter().all(|s| s.played == 0));
        assert_eq!(game.results().len(), 2);
        let physical_after = serde_json::to_value(game.squad("a").unwrap()).unwrap();
        assert!(game.squad("a").unwrap().iter().any(|p| p.condition < 100));
        let money_after = game.management.clubs["a"].balance;
        let board_after = game.board_view("a").unwrap().clone();
        assert!(game.settle_completed_season().unwrap().is_none());
        assert_eq!(game.management.clubs["a"].balance, money_after);
        assert_eq!(game.board_view("a").unwrap(), board_after);
        assert_eq!(game.dispatch("a", request, 1001).unwrap(), receipt);
        advance(&mut game);
        assert_eq!(
            serde_json::to_value(game.squad("a").unwrap()).unwrap(),
            physical_after
        );
        assert_eq!(game.public_state().players, owners);
        assert_eq!(
            game.management.career.as_ref().unwrap().contracts,
            contracts
        );
        assert_eq!(game.match_plan("a").unwrap(), plan);
        let second_first = game.fixtures().iter().map(|f| f.day).min().unwrap();
        assert!(second_first > 300);
        while game.window().day < second_first {
            advance(&mut game);
        }
        assert!(
            game.management.clubs["a"].balance < money_after,
            "off-season wages must actually be charged"
        );
        assert_eq!(
            game.career_date().unwrap(),
            NaiveDate::from_ymd_opt(2027, 8, 1).unwrap()
        );
        advance(&mut game);
        advance(&mut game);
        assert_eq!(game.season_history().len(), 2);
        assert_eq!(game.season_history()[1].season, 2027);
        game = Football::load_validated(game.save_state().unwrap()).unwrap();
        assert_eq!(game.results().len(), 4);
        let ids: BTreeSet<_> = game.results().iter().map(|r| &r.fixture_id).collect();
        assert_eq!(ids.len(), 4);
        assert_eq!(game.public_state().players, owners);
        assert_eq!(
            game.management.career.as_ref().unwrap().contracts,
            contracts
        );
        assert_eq!(game.match_plan("a").unwrap(), plan);
        assert_eq!(
            game.season_history()[1].reputations_before,
            game.season_history()[0].reputations_after
        );
        let public = serde_json::to_value(game.public_season_history()).unwrap();
        for season in public.as_array().unwrap() {
            assert!(season.get("manager_outcomes").is_none());
            assert!(season.get("prizes").is_none());
            assert!(season.get("fixtures").is_none());
            assert!(season.get("reputations_after").is_none());
        }
    }

    #[test]
    fn settlement_failure_does_not_publish_final_match_or_partial_prizes() {
        let mut game = game();
        advance(&mut game);
        game.management.clubs.get_mut("a").unwrap().balance = i64::MAX;
        let before = serde_json::to_value(game.results()).unwrap();
        let standings = game.standings();
        let day = game.window();
        assert!(
            game.advance_closed_day(day.day, day.deadline_ms, day.deadline_ms + 1000)
                .is_err()
        );
        assert_eq!(game.window().day, day.day);
        assert_eq!(serde_json::to_value(game.results()).unwrap(), before);
        assert_eq!(game.standings(), standings);
        assert!(game.season_history().is_empty());
        assert_eq!(game.management.clubs["a"].balance, i64::MAX);
    }

    #[test]
    fn dismissed_managers_never_regain_control_in_the_next_season() {
        let mut game = game_with_satisfaction(0);
        let request = crate::Request {
            id: "old-plan".into(),
            day: 1,
            command: crate::Command::SetMatchPlan {
                plan: Default::default(),
            },
        };
        game.dispatch("a", request.clone(), 10)
            .unwrap()
            .result
            .unwrap();
        advance(&mut game);
        advance(&mut game);
        assert_eq!(game.dismissals().len(), 2);
        let dismissed = game.dismissals().to_vec();
        assert_eq!(
            game.dispatch("a", request.clone(), 2001),
            Err(crate::Error::Unauthorized)
        );
        assert_eq!(game.board_view("a"), Err(crate::Error::Unauthorized));
        let next_last = game.fixtures().iter().map(|f| f.day).max().unwrap();
        while game.window().day <= next_last {
            advance(&mut game);
        }
        assert_eq!(game.season_history().len(), 2);
        assert_eq!(&game.dismissals()[..2], dismissed.as_slice());
        game = Football::load_validated(game.save_state().unwrap()).unwrap();
        assert_eq!(
            game.dispatch("a", request, 999_999),
            Err(crate::Error::Unauthorized)
        );
        assert!(!game.season_history()[1].manager_outcomes.contains_key("a"));
        assert!(!game.season_history()[1].manager_outcomes.contains_key("b"));
        assert_eq!(
            game.public_season_history()[0].manager_dismissals,
            dismissed
        );
    }

    #[test]
    fn checkpoint_seasons_reject_colliding_history_and_broken_dates() {
        let mut game = game();
        advance(&mut game);
        advance(&mut game);
        game.validate_season_checkpoint().unwrap();
        let mut bad = game.clone();
        bad.seasons.as_mut().unwrap().archives[0].completed_date += Duration::days(1);
        assert!(bad.validate_season_checkpoint().is_err());
        let mut bad = game.clone();
        bad.fixtures[0].id = bad.results[0].fixture_id.clone();
        assert!(bad.validate_season_checkpoint().is_err());
        let mut bad = game.clone();
        bad.seasons.as_mut().unwrap().archives[0].results[0]
            .report
            .home_goals += 1;
        assert!(bad.validate_season_checkpoint().is_err());
        let mut bad = game;
        let archive = bad.seasons.as_ref().unwrap().archives[0].clone();
        bad.seasons.as_mut().unwrap().archives.push(archive);
        assert!(bad.validate_season_checkpoint().is_err());
    }
}
