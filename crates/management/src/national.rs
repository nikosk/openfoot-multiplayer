//! Shared national football, projected onto the single authoritative club world.
//! Pinned OpenFoot 64677fee, GPL-3.0-or-later.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! The temporary source context has no selected manager, club finances, or command
//! authority. Player carry-back is synchronized before the shared day commits.
use crate::football::Football;
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use domain::{
    message::InboxMessage, national_team::NationalTeam, news::NewsArticle, player::Player,
    world_history::WorldHistoryArchive,
};
use rand::{SeedableRng, rngs::StdRng};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[path = "national_matches.rs"]
pub(crate) mod matches;
#[path = "national_nations.rs"]
pub mod nations;
#[path = "national_world_cup.rs"]
pub(crate) mod world_cup;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NationalSetup {
    pub seed: u64,
    pub national_teams: Vec<NationalTeam>,
    pub world_history: WorldHistoryArchive,
    #[serde(default)]
    pub country_regions: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NationalState {
    pub setup: NationalSetup,
    pub news: Vec<NewsArticle>,
    pub announcements: Vec<InboxMessage>,
    pub generated_count: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NationalView {
    pub national_teams: Vec<NationalTeam>,
    pub world_history: WorldHistoryArchive,
    pub news: Vec<NewsArticle>,
}
pub(crate) struct NationalClock {
    current_date: DateTime<Utc>,
}
pub(crate) struct NationalContext {
    players: Vec<Player>,
    national_teams: Vec<NationalTeam>,
    competitions: Vec<domain::league::League>,
    world_history: WorldHistoryArchive,
    news: Vec<NewsArticle>,
    messages: Vec<InboxMessage>,
    active_competition_ids: Vec<String>,
    clock: NationalClock,
    country_regions: BTreeMap<String, String>,
    seed: u64,
    generated_count: u64,
}
impl NationalContext {
    fn region_for_country(&self, code: &str) -> String {
        self.country_regions
            .get(code)
            .cloned()
            .or_else(|| nations::nation_by_code(code).map(|n| n.region_id.to_string()))
            .unwrap_or_default()
    }
}
fn generate_national_player(
    code: &str,
    slot: usize,
    year: u32,
    seed: u64,
    serial: &mut u64,
) -> Player {
    let id = format!("national-pool:{year}:{serial}");
    let mut rng = StdRng::seed_from_u64(seed ^ *serial ^ 0x6e61_7469_6f6e_6765);
    *serial = serial
        .checked_add(1)
        .expect("validated national generation counter");
    crate::youth::generate_national_player(id, code, slot, year, &mut rng)
        .expect("validated source national player generation")
}

// Domain wrappers reuse the already ported authoritative physical/injury rules.
mod wear {
    use domain::player::Player;
    pub fn apply_match_wear(player: &mut Player, minutes: u8, rng: &mut impl rand::Rng) {
        let mut value = serde_json::to_value(&player.attributes).expect("integer attributes");
        let object = value.as_object_mut().unwrap();
        object.insert("id".into(), player.id.clone().into());
        object.insert("name".into(), player.match_name.clone().into());
        object.insert("position".into(), "Midfielder".into());
        object.insert("role".into(), "Standard".into());
        object.insert("ovr".into(), player.ovr.into());
        object.insert("condition".into(), player.condition.into());
        object.insert("fitness".into(), player.fitness.into());
        let mut projected: engine::PlayerData =
            serde_json::from_value(value).expect("complete source physical projection");
        crate::physical::apply_match_wear(&mut projected, minutes, rng);
        player.condition = projected.condition;
        player.fitness = projected.fitness;
    }
    pub fn roll_match_injury(player: &mut Player, rng: &mut impl rand::Rng) -> bool {
        let mut state = crate::availability::Availability {
            injury: player.injury.as_ref().map(|i| crate::availability::Injury {
                name: i.name.clone(),
                days_remaining: i.days_remaining,
            }),
            ..Default::default()
        };
        if crate::availability::roll_match_injury(&mut state, player.fitness, rng)
            .expect("validated national fitness")
        {
            let injury = state.injury.unwrap();
            player.injury = Some(domain::player::Injury {
                name: injury.name,
                days_remaining: injury.days_remaining,
            });
            true
        } else {
            false
        }
    }
}

impl Football {
    pub fn configure_national(&mut self, setup: NationalSetup) -> Result<(), String> {
        if self.started
            || self.management.sequence != 0
            || self.national.is_some()
            || self.management.social.is_none()
            || self.management.availability.is_none()
        {
            return Err(
                "National football requires source players and availability before commands".into(),
            );
        }
        let mut staged = self.clone();
        staged.national = Some(NationalState {
            setup,
            news: vec![],
            announcements: vec![],
            generated_count: 0,
        });
        let players = staged.project_source_players()?;
        // The selection half of source prepare_national_squads, without its
        // tournament-only synthesis: sparse nations outside the field stay sparse.
        for team in &mut staged.national.as_mut().unwrap().setup.national_teams {
            let mut squad: Vec<_> = players
                .values()
                .filter(|p| {
                    !p.retired
                        && (if p.football_nation.is_empty() {
                            &p.nationality
                        } else {
                            &p.football_nation
                        }) == &team.football_nation
                })
                .map(|p| (p.id.clone(), p.ovr))
                .collect();
            squad.sort_by_key(|p| std::cmp::Reverse(p.1));
            team.squad_player_ids = squad.into_iter().take(23).map(|p| p.0).collect();
        }
        staged.validate_national_checkpoint()?;
        *self = staged;
        Ok(())
    }
    pub fn national_view(&self) -> Option<NationalView> {
        self.national.as_ref().map(|state| NationalView {
            national_teams: state.setup.national_teams.clone(),
            world_history: state.setup.world_history.clone(),
            news: state.news.clone(),
        })
    }
    fn national_context(&self, today: NaiveDate) -> Result<NationalContext, String> {
        let state = self
            .national
            .as_ref()
            .ok_or("National football unavailable")?;
        let (competitions, active_competition_ids) = if let Some(s) = &self.competitions {
            let order: Vec<_> = if s.setup.competition_order.is_empty() {
                s.setup.competitions.keys().cloned().collect()
            } else {
                s.setup.competition_order.clone()
            };
            (
                order
                    .iter()
                    .map(|id| s.setup.competitions[id].clone())
                    .collect(),
                s.setup.active_competition_ids.iter().cloned().collect(),
            )
        } else {
            (vec![], vec![])
        };
        Ok(NationalContext {
            players: self.project_source_players()?.into_values().collect(),
            national_teams: state.setup.national_teams.clone(),
            competitions,
            world_history: state.setup.world_history.clone(),
            news: state.news.clone(),
            messages: state.announcements.clone(),
            active_competition_ids,
            clock: NationalClock {
                current_date: today
                    .and_hms_opt(0, 0, 0)
                    .ok_or("National date invalid")?
                    .and_utc(),
            },
            country_regions: state.setup.country_regions.clone(),
            seed: state.setup.seed,
            generated_count: state.generated_count,
        })
    }
    fn commit_national_context(&mut self, context: NationalContext) -> Result<(), String> {
        let previous_announcement_ids: BTreeSet<_> = self
            .national
            .as_ref()
            .unwrap()
            .announcements
            .iter()
            .map(|m| m.id.clone())
            .collect();
        for player in &context.players {
            if !self.management.players.contains_key(&player.id) {
                self.management
                    .register_source_youth(player)
                    .map_err(|e| format!("National player registration: {e:?}"))?;
                self.management
                    .social
                    .as_mut()
                    .unwrap()
                    .source_players
                    .insert(player.id.clone(), player.clone());
            }
        }
        self.register_personnel_signings()
            .map_err(|e| format!("National physical registration: {e:?}"))?;
        for player in &context.players {
            let attr = self
                .attributes
                .get_mut(&player.id)
                .ok_or("National player physical record missing")?;
            let availability = self
                .management
                .availability
                .as_mut()
                .unwrap()
                .get_mut(&player.id)
                .ok_or("National availability missing")?;
            let injury = player.injury.as_ref().map(|i| crate::availability::Injury {
                name: i.name.clone(),
                days_remaining: i.days_remaining,
            });
            if attr.condition != player.condition
                || attr.fitness != player.fitness
                || availability.injury != injury
            {
                let revision = self
                    .management
                    .player_revisions
                    .get_mut(&player.id)
                    .ok_or("National player revision missing")?;
                *revision = revision
                    .checked_add(1)
                    .ok_or("National player revision overflow")?;
            }
            attr.condition = player.condition;
            attr.fitness = player.fitness;
            availability.injury = injury;
        }
        self.management.sync_social_players(context.players)?;
        if let Some(competitions) = &mut self.competitions {
            competitions.setup.competition_order =
                context.competitions.iter().map(|c| c.id.clone()).collect();
            competitions.setup.competitions = context
                .competitions
                .into_iter()
                .map(|c| (c.id.clone(), c))
                .collect();
            competitions.setup.active_competition_ids =
                context.active_competition_ids.into_iter().collect();
        }
        let recipients: Vec<_> = self.management.managers.keys().cloned().collect();
        for message in &context.messages {
            if !previous_announcement_ids.contains(&message.id) {
                for recipient in &recipients {
                    self.management
                        .social
                        .as_mut()
                        .unwrap()
                        .inbox
                        .deliver(recipient, message.clone())
                        .map_err(|e| format!("National message delivery: {e:?}"))?;
                }
            }
        }
        let state = self.national.as_mut().unwrap();
        state.setup.national_teams = context.national_teams;
        state.setup.world_history = context.world_history;
        state.news = context.news;
        state.announcements = context.messages;
        state.generated_count = context.generated_count;
        Ok(())
    }
    pub(crate) fn advance_national_day(&mut self, today: NaiveDate) -> Result<(), String> {
        if self.national.is_none() {
            return Ok(());
        }
        let date = today.to_string();
        let due_friendly = self
            .national
            .as_ref()
            .unwrap()
            .setup
            .national_teams
            .iter()
            .flat_map(|t| &t.fixtures)
            .any(|f| f.date == date && f.status == domain::league::FixtureStatus::Scheduled);
        let due_cup = self.competitions.as_ref().is_some_and(|s| {
            s.setup
                .competitions
                .values()
                .filter(|c| world_cup::is_world_cup_competition(c))
                .flat_map(|c| &c.fixtures)
                .any(|f| f.date == date && f.status == domain::league::FixtureStatus::Scheduled)
        });
        if !due_friendly && !due_cup {
            return Ok(());
        }
        let mut context = self.national_context(today)?;
        let mut rng = StdRng::seed_from_u64(
            context.seed ^ u64::from(self.management.window.day) ^ 0x6e61_7469_6f6e_6461,
        );
        matches::process_national_team_fixtures_due(&mut context, &today.to_string(), &mut rng);
        world_cup::process_world_cup_fixtures_due(&mut context, &today.to_string(), &mut rng);
        self.commit_national_context(context)
    }
    /// Source end-of-season international calendar. The sole stochastic change
    /// is replacing ambient friendly shuffling with a replayable named stream.
    pub(crate) fn roll_national_calendar(&mut self, anchor: NaiveDate) -> Result<(), String> {
        if self.national.is_none() {
            return Ok(());
        }
        let today = self.career_date().ok_or("National career unavailable")?;
        let mut context = self.national_context(today)?;
        let kickoff = context
            .clock
            .current_date
            .checked_add_signed(chrono::Duration::days(2))
            .ok_or("National kickoff overflow")?;
        let next_start = anchor
            .and_hms_opt(0, 0, 0)
            .ok_or("National anchor invalid")?
            .and_utc();
        let due = world_cup::is_world_cup_summer(kickoff.year());
        let mut settle_rng =
            StdRng::seed_from_u64(u64::from(kickoff.year().unsigned_abs()) ^ 0xF1FA);
        let field = if due {
            world_cup::settle_outstanding_qualifying(&mut context, &mut settle_rng);
            let host = world_cup::host_for_year(&context, kickoff.year());
            world_cup::qualified_field_from_game(
                &mut context,
                world_cup::FORMAT_48.field,
                host.as_deref(),
            )
        } else {
            None
        };
        context.competitions.retain(|c| {
            !world_cup::is_world_cup_competition(c)
                || ((world_cup::is_world_cup_qualifying(c) || world_cup::is_world_cup_playoff(c))
                    && c.season > kickoff.year().max(0) as u32)
        });
        let remaining: BTreeSet<_> = context.competitions.iter().map(|c| c.id.clone()).collect();
        context
            .active_competition_ids
            .retain(|id| remaining.contains(id));
        for team in &mut context.national_teams {
            team.fixtures.clear();
        }
        if due {
            if !context
                .competitions
                .iter()
                .any(world_cup::is_world_cup_competition)
            {
                world_cup::schedule_world_cup_with_field(
                    &mut context,
                    kickoff,
                    &world_cup::FORMAT_48,
                    field,
                );
            }
        } else {
            let dates = matches::international_window_dates(next_start);
            if !dates.is_empty() {
                let leads = world_cup::season_leads_into_world_cup(next_start);
                let starts = world_cup::season_starts_world_cup_qualifying(next_start);
                let progress = context
                    .competitions
                    .iter()
                    .any(world_cup::is_world_cup_qualifying);
                let reserved = if leads || starts || progress {
                    matches::international_window_span_dates(&dates)
                } else {
                    dates.clone()
                };
                for c in &mut context.competitions {
                    if !world_cup::is_world_cup_competition(c) {
                        crate::competition_schedule::shift_fixtures_off_reserved_dates(
                            c, &reserved,
                        );
                    }
                }
                crate::competition_schedule::append_south_american_preseason_friendlies(
                    &mut context.competitions,
                    &reserved,
                );
                crate::competition_schedule::append_other_preseason_friendlies(
                    &mut context.competitions,
                    &reserved,
                );
                let mut rng =
                    StdRng::seed_from_u64(u64::from(next_start.year().unsigned_abs()) ^ 0xF1FA);
                if leads {
                    if progress {
                        world_cup::continue_world_cup_qualifying(&mut context, &dates, &mut rng);
                    } else {
                        world_cup::schedule_world_cup_qualifying(
                            &mut context,
                            next_start.year() + 1,
                            &dates,
                        );
                    }
                } else if starts {
                    if !progress {
                        world_cup::schedule_world_cup_qualifying(
                            &mut context,
                            next_start.year() + 2,
                            &dates,
                        );
                    }
                } else if progress {
                    world_cup::continue_world_cup_qualifying(&mut context, &dates, &mut rng);
                } else {
                    let mut rng = StdRng::seed_from_u64(
                        context.seed ^ next_start.year() as u64 ^ 0x6e6174667269656e,
                    );
                    matches::schedule_national_team_friendlies(
                        &mut context.national_teams,
                        &dates,
                        &mut rng,
                    );
                }
            }
        }
        self.commit_national_context(context)
    }
    pub(crate) fn validate_national_checkpoint(&self) -> Result<(), String> {
        let Some(s) = &self.national else {
            return Ok(());
        };
        if self.management.social.is_none()
            || self.management.availability.is_none()
            || !self
                .career_date()
                .is_some_and(|date| (1..=9000).contains(&date.year()))
            || s.setup.national_teams.len() > 256
            || s.generated_count > 1_000_000
        {
            return Err("Invalid national state dependencies or bounds".into());
        }
        let mut ids = BTreeSet::new();
        let mut codes = BTreeSet::new();
        let mut players = BTreeSet::new();
        let mut fixtures = BTreeSet::new();
        for team in &s.setup.national_teams {
            if team.id.is_empty()
                || team.football_nation.is_empty()
                || !ids.insert(&team.id)
                || !codes.insert(&team.football_nation)
                || team.reputation > 1000
                || team.squad_player_ids.len() > 23
            {
                return Err("Invalid national team identity or bounds".into());
            }
            for id in &team.squad_player_ids {
                if !self.management.players.contains_key(id) || !players.insert(id) {
                    return Err("Invalid national squad reference".into());
                }
            }
        }
        for team in &s.setup.national_teams {
            for f in &team.fixtures {
                if f.id.is_empty()
                    || !fixtures.insert(&f.id)
                    || !ids.contains(&f.home_team_id)
                    || !ids.contains(&f.away_team_id)
                    || f.home_team_id == f.away_team_id
                    || !NaiveDate::parse_from_str(&f.date, "%Y-%m-%d")
                        .is_ok_and(|date| (1..=9000).contains(&date.year()))
                    || f.status == domain::league::FixtureStatus::InProgress
                    || (f.status == domain::league::FixtureStatus::Completed) != f.result.is_some()
                {
                    return Err("Invalid national friendly fixture".into());
                }
            }
        }
        if s.setup
            .world_history
            .national_team_ranking
            .iter()
            .any(|r| !r.points.is_finite())
        {
            return Err("Invalid national ranking".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context() -> NationalContext {
        NationalContext {
            players: vec![],
            national_teams: vec![],
            competitions: vec![],
            world_history: Default::default(),
            news: vec![],
            messages: vec![],
            active_competition_ids: vec![],
            clock: NationalClock {
                current_date: NaiveDate::from_ymd_opt(2026, 6, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc(),
            },
            country_regions: Default::default(),
            seed: 77,
            generated_count: 0,
        }
    }
    #[test]
    fn national_friendlies_carry_back_once_without_club_appearance_stats() {
        let mut game = context();
        world_cup::prepare_national_squads(&mut game, &["ENG".into(), "FRA".into()]);
        assert_eq!(game.players.len(), 36);
        let before: Vec<_> = game
            .players
            .iter()
            .map(|p| (p.id.clone(), p.condition, p.stats.clone()))
            .collect();
        let mut rng = StdRng::seed_from_u64(9);
        matches::schedule_national_team_friendlies(
            &mut game.national_teams,
            &["2026-09-09".into()],
            &mut rng,
        );
        assert_eq!(
            matches::process_national_team_fixtures_due(&mut game, "2026-09-09", &mut rng),
            1
        );
        assert!(
            game.players
                .iter()
                .zip(&before)
                .any(|(p, b)| p.condition < b.1)
        );
        for (p, b) in game.players.iter().zip(&before) {
            assert_eq!(
                serde_json::to_value(&p.stats).unwrap(),
                serde_json::to_value(&b.2).unwrap()
            );
        }
        let after = serde_json::to_value(&game.players).unwrap();
        assert_eq!(
            matches::process_national_team_fixtures_due(&mut game, "2026-09-09", &mut rng),
            0
        );
        assert_eq!(after, serde_json::to_value(&game.players).unwrap());
    }
    #[test]
    fn two_season_qualifying_preserves_played_results_and_settles_full_field() {
        let mut game = context();
        let first = NaiveDate::from_ymd_opt(2028, 8, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc();
        game.clock.current_date = first;
        let windows = matches::international_window_dates(first);
        world_cup::schedule_world_cup_qualifying(&mut game, 2030, &windows);
        assert!(
            game.competitions
                .iter()
                .any(world_cup::is_world_cup_qualifying)
        );
        let mut rng = StdRng::seed_from_u64(31);
        for date in matches::international_window_span_dates(&windows) {
            world_cup::process_world_cup_fixtures_due(&mut game, &date, &mut rng);
        }
        let played: BTreeMap<_, _> = game
            .competitions
            .iter()
            .flat_map(|c| &c.fixtures)
            .filter(|f| f.result.is_some())
            .map(|f| (f.id.clone(), serde_json::to_value(&f.result).unwrap()))
            .collect();
        assert!(!played.is_empty());
        let second = NaiveDate::from_ymd_opt(2029, 8, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc();
        game.clock.current_date = second;
        world_cup::continue_world_cup_qualifying(
            &mut game,
            &matches::international_window_dates(second),
            &mut rng,
        );
        for f in game.competitions.iter().flat_map(|c| &c.fixtures) {
            if let Some(old) = played.get(&f.id) {
                assert_eq!(*old, serde_json::to_value(&f.result).unwrap());
            }
        }
        world_cup::settle_outstanding_qualifying(&mut game, &mut rng);
        let host = world_cup::host_for_year(&game, 2030);
        let field = world_cup::qualified_field_from_game(&mut game, 48, host.as_deref()).unwrap();
        assert_eq!(field.len(), 48);
        assert_eq!(field.iter().collect::<BTreeSet<_>>().len(), 48);
    }
    #[test]
    fn complete_world_cup_has_replayable_champion_rankings_and_source_carryback() {
        fn play() -> (serde_json::Value, serde_json::Value, serde_json::Value) {
            let mut game = context();
            let kickoff = game.clock.current_date;
            world_cup::schedule_world_cup(&mut game, kickoff, &world_cup::FORMAT_16);
            assert_eq!(game.competitions.len(), 1);
            let mut rng = StdRng::seed_from_u64(902);
            for day in 0..90 {
                let date = (kickoff + chrono::Duration::days(day))
                    .date_naive()
                    .to_string();
                world_cup::process_world_cup_fixtures_due(&mut game, &date, &mut rng);
            }
            assert!(world_cup::world_cup_champion(&game.competitions[0]).is_some());
            assert_eq!(game.world_history.world_cup_champions.len(), 1);
            assert!(!game.world_history.national_team_ranking.is_empty());
            (
                serde_json::to_value(game.competitions).unwrap(),
                serde_json::to_value(game.world_history).unwrap(),
                serde_json::to_value(game.players).unwrap(),
            )
        }
        assert_eq!(play(), play());
    }
}
