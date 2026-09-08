//! Retained source manager identities, team/manager career history and awards.
//! Pinned 64677fee end_of_season.rs, season_awards.rs and ai_hiring.rs.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
use crate::football::{Football, Standing};
use chrono::{Datelike, NaiveDate};
use domain::{
    manager::{Manager, ManagerCareerEntry},
    staff::StaffRole,
    team::TeamSeasonRecord,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[path = "season_awards.rs"]
pub mod awards;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TeamHistoryState {
    #[serde(default)]
    pub archived_identities: ArchivedIdentities,
    #[serde(default)]
    pub competition_awards: BTreeMap<String, BTreeMap<u32, awards::SeasonAwards>>,
    #[serde(default)]
    pub competition_completed: BTreeMap<String, BTreeMap<u32, NaiveDate>>,
    pub managers: BTreeMap<String, Manager>,
    pub actor_manager_ids: BTreeMap<String, String>,
    pub teams: BTreeMap<String, Vec<TeamSeasonRecord>>,
    pub awards: BTreeMap<u32, awards::SeasonAwards>,
    pub completed: BTreeMap<u32, NaiveDate>,
    pub vacancies: BTreeMap<String, Vacancy>,
    pub appointments: Vec<Appointment>,
    pub processed_dismissals: usize,
    pub last_processed: Option<NaiveDate>,
}

/// Removed clone-source identities remain historical records only. Never feed
/// these maps into selection, payroll, player markets or manager hiring.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArchivedIdentities {
    pub teams: BTreeMap<String, domain::team::Team>,
    pub players: BTreeMap<String, domain::player::Player>,
    pub managers: BTreeMap<String, Manager>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vacancy {
    pub since: NaiveDate,
    pub days: u32,
    pub caretaker_actor: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Appointment {
    pub date: NaiveDate,
    pub club_id: String,
    pub manager_id: String,
    pub actor_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicManagerHistory {
    pub id: String,
    pub name: String,
    pub club_id: Option<String>,
    pub career_stats: domain::manager::ManagerCareerStats,
    pub career_history: Vec<ManagerCareerEntry>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicTeamHistory {
    pub competition_awards: BTreeMap<String, BTreeMap<u32, awards::SeasonAwards>>,
    pub teams: BTreeMap<String, Vec<TeamSeasonRecord>>,
    pub managers: Vec<PublicManagerHistory>,
    pub awards: BTreeMap<u32, awards::SeasonAwards>,
    pub appointments: Vec<Appointment>,
}

fn next_id(base: String, managers: &BTreeMap<String, Manager>) -> String {
    if !managers.contains_key(&base) {
        return base;
    }
    let mut index = 2_u64;
    loop {
        let id = format!("{base}_{index}");
        if !managers.contains_key(&id) {
            return id;
        }
        index += 1;
    }
}

impl Football {
    pub fn configure_archived_identities(
        &mut self,
        archive: ArchivedIdentities,
    ) -> Result<(), String> {
        if self.started || self.management.sequence != 0 || self.management.team_history.is_none() {
            return Err("Archived identities require initial team history".into());
        }
        let previous = &self
            .management
            .team_history
            .as_ref()
            .unwrap()
            .archived_identities;
        if !previous.teams.is_empty()
            || !previous.players.is_empty()
            || !previous.managers.is_empty()
        {
            return Err("Archived identities are immutable after configuration".into());
        }
        if archive
            .teams
            .iter()
            .any(|(id, t)| id != &t.id || self.management.clubs.contains_key(id))
            || archive
                .players
                .iter()
                .any(|(id, p)| id != &p.id || self.management.players.contains_key(id))
            || archive.managers.iter().any(|(id, m)| {
                id != &m.id
                    || self
                        .management
                        .team_history
                        .as_ref()
                        .unwrap()
                        .managers
                        .contains_key(id)
            })
        {
            return Err("Archived identities must be removed original identities".into());
        }
        self.management
            .team_history
            .as_mut()
            .unwrap()
            .archived_identities = archive;
        Ok(())
    }
    pub fn historical_identity(&self, id: &str) -> Option<serde_json::Value> {
        let archive = &self.management.team_history.as_ref()?.archived_identities;
        if let Some(team) = archive.teams.get(id) {
            return Some(
                serde_json::json!({"kind":"club","id":team.id,"name":team.name,"history":team.history}),
            );
        }
        if let Some(player) = archive.players.get(id) {
            return Some(
                serde_json::json!({"kind":"player","id":player.id,"name":player.full_name,
                "club_id":player.team_id,"nationality":player.nationality,"career":player.career,"stats":player.stats}),
            );
        }
        archive.managers.get(id).map(|manager| serde_json::json!({"kind":"manager","id":manager.id,
            "name":manager.full_name(),"club_id":manager.team_id,"career_stats":manager.career_stats,"career_history":manager.career_history}))
    }
    pub fn configure_team_history(
        &mut self,
        managers: BTreeMap<String, Manager>,
        actor_manager_ids: BTreeMap<String, String>,
    ) -> Result<(), String> {
        let personnel = self
            .management
            .personnel
            .as_ref()
            .ok_or("Team history requires personnel")?;
        if self.started
            || self.management.sequence != 0
            || self.management.team_history.is_some()
            || actor_manager_ids.keys().ne(self.management.managers.keys())
            || actor_manager_ids.values().collect::<BTreeSet<_>>().len() != actor_manager_ids.len()
            || managers.iter().any(|(id, m)| {
                id != &m.id
                    || m.team_id
                        .as_ref()
                        .is_some_and(|club| !self.management.clubs.contains_key(club))
            })
            || actor_manager_ids.iter().any(|(actor, id)| {
                managers.get(id).is_none_or(|m| {
                    m.team_id.as_deref() != Some(&self.management.managers[actor].club_id)
                })
            })
        {
            return Err("Invalid initial rich manager identities/bindings".into());
        }
        self.management.team_history = Some(TeamHistoryState {
            managers,
            actor_manager_ids,
            teams: personnel
                .teams
                .iter()
                .map(|(id, t)| (id.clone(), t.history.clone()))
                .collect(),
            ..Default::default()
        });
        self.validate_team_history_checkpoint()
    }
    /// Runs after board dismissals. Replacement actors are caretaker controls;
    /// the rich person is appointed only after seven source vacancy day sweeps.
    pub(crate) fn advance_team_history(&mut self, today: NaiveDate) -> Result<(), String> {
        let Some(history) = &mut self.management.team_history else {
            return Ok(());
        };
        if history
            .last_processed
            .is_some_and(|last| last.succ_opt() != Some(today))
        {
            return Err("Manager history daily sweep must be consecutive".into());
        }
        let personnel = self
            .management
            .personnel
            .as_mut()
            .ok_or("Team history personnel missing")?;
        for dismissal in self.dismissals.iter().skip(history.processed_dismissals) {
            if let Some(id) = history.actor_manager_ids.get(&dismissal.manager_id) {
                history
                    .managers
                    .get_mut(id)
                    .ok_or("Dismissed rich manager missing")?
                    .fire(&today.to_string());
            }
            personnel
                .teams
                .get_mut(&dismissal.club_id)
                .ok_or("Dismissed team missing")?
                .manager_id = None;
            history.vacancies.insert(
                dismissal.club_id.clone(),
                Vacancy {
                    since: today,
                    days: 0,
                    caretaker_actor: dismissal.replacement_manager_id.clone(),
                },
            );
        }
        history.processed_dismissals = self.dismissals.len();
        let mut filled = vec![];
        for (club, vacancy) in &mut history.vacancies {
            vacancy.days = vacancy
                .days
                .checked_add(1)
                .ok_or("Manager vacancy overflow")?;
            if vacancy.days < 7 {
                continue;
            }
            let team = &personnel.teams[club];
            let mut manager = if let Some(staff) = personnel.staff.values().find(|s| {
                s.team_id.as_deref() == Some(club) && s.role == StaffRole::AssistantManager
            }) {
                let id = next_id(format!("mgr_{club}_{}", staff.id), &history.managers);
                let nationality = if staff.nationality.is_empty() {
                    if team.football_nation.is_empty() {
                        team.country.clone()
                    } else {
                        team.football_nation.clone()
                    }
                } else {
                    staff.nationality.clone()
                };
                let mut m = Manager::new(
                    id,
                    staff.first_name.clone(),
                    staff.last_name.clone(),
                    staff.date_of_birth.clone(),
                    nationality,
                );
                m.reputation = 200 + u32::from(staff.attributes.coaching.min(100)) * 5;
                m
            } else {
                crate::youth::generate_club_manager(
                    team,
                    &next_id(format!("mgr_{club}"), &history.managers),
                    today.year() as u32,
                )?
            };
            manager.satisfaction = 50;
            manager.fan_approval = 50;
            manager.hire(club.clone());
            manager.career_history.push(ManagerCareerEntry::open(
                club.clone(),
                team.name.clone(),
                today.to_string(),
            ));
            personnel.teams.get_mut(club).unwrap().manager_id = Some(manager.id.clone());
            history
                .actor_manager_ids
                .insert(vacancy.caretaker_actor.clone(), manager.id.clone());
            history.appointments.push(Appointment {
                date: today,
                club_id: club.clone(),
                manager_id: manager.id.clone(),
                actor_id: vacancy.caretaker_actor.clone(),
            });
            history.managers.insert(manager.id.clone(), manager);
            filled.push(club.clone());
        }
        for club in filled {
            history.vacancies.remove(&club);
        }
        if let Some(boards) = &self.boards {
            for (actor, id) in &history.actor_manager_ids {
                if self.management.managers.contains_key(actor) {
                    if let Some(board) = boards.get(actor) {
                        let m = history.managers.get_mut(id).unwrap();
                        m.satisfaction = board.state.satisfaction;
                        m.warning_stage = board.state.warning_stage;
                    }
                }
            }
        }
        history.last_processed = Some(today);
        Ok(())
    }
    pub(crate) fn settle_team_history(
        &mut self,
        season: u32,
        date: NaiveDate,
        standings: &[Standing],
    ) -> Result<(), String> {
        self.settle_competition_history("primary", season, date, standings)
    }
    pub(crate) fn settle_competition_history(
        &mut self,
        competition_id: &str,
        season: u32,
        date: NaiveDate,
        standings: &[Standing],
    ) -> Result<(), String> {
        if self.management.team_history.is_none() {
            return Ok(());
        }
        let players = self
            .project_source_players()?
            .into_values()
            .collect::<Vec<_>>();
        let names = self
            .management
            .clubs
            .iter()
            .map(|(id, c)| (id.clone(), c.name.clone()))
            .collect::<BTreeMap<_, _>>();
        let primary = self
            .competitions
            .as_ref()
            .map_or(competition_id == "primary", |c| {
                c.setup.primary_competition_id == competition_id
            });
        let history = self.management.team_history.as_mut().unwrap();
        if history
            .competition_completed
            .get(competition_id)
            .is_some_and(|s| s.contains_key(&season))
        {
            return Err("Team season already settled".into());
        }
        let mut managers = history.managers.values().cloned().collect::<Vec<_>>();
        // A fired original cannot win an award through a stale rich binding even
        // when season settlement precedes today's board-dismissal daily hook.
        for (actor, id) in &history.actor_manager_ids {
            if !self.management.managers.contains_key(actor) {
                if let Some(m) = managers.iter_mut().find(|m| &m.id == id) {
                    m.team_id = None;
                }
            }
        }
        let awards = awards::compute(&players, &names, &managers, standings, date);
        if primary {
            history.awards.insert(season, awards.clone());
            history.completed.insert(season, date);
        }
        history
            .competition_awards
            .entry(competition_id.into())
            .or_default()
            .insert(season, awards);
        for (index, s) in standings.iter().enumerate() {
            let position = u32::try_from(index + 1).map_err(|e| e.to_string())?;
            let record = TeamSeasonRecord {
                season,
                league_position: position,
                played: s.played,
                won: s.won,
                drawn: s.drawn,
                lost: s.lost,
                goals_for: s.goals_for,
                goals_against: s.goals_against,
            };
            history
                .teams
                .get_mut(&s.club_id)
                .ok_or("History club missing")?
                .push(record.clone());
            if let Some(personnel) = &mut self.management.personnel {
                let team = personnel
                    .teams
                    .get_mut(&s.club_id)
                    .ok_or("Personnel team missing")?;
                team.history.push(record);
                team.form.clear();
            }
            for manager in history
                .managers
                .values_mut()
                .filter(|m| m.team_id.as_deref() == Some(&s.club_id))
            {
                let active = history
                    .actor_manager_ids
                    .iter()
                    .find(|(_, id)| *id == &manager.id)
                    .is_none_or(|(actor, _)| self.management.managers.contains_key(actor));
                if !active {
                    continue;
                }
                update_manager_season(manager, &s.club_id, &names[&s.club_id], s, position, date)?;
            }
        }
        history
            .competition_completed
            .entry(competition_id.into())
            .or_default()
            .insert(season, date);
        Ok(())
    }
    pub fn public_team_history(&self) -> Option<PublicTeamHistory> {
        self.management
            .team_history
            .as_ref()
            .map(|h| PublicTeamHistory {
                competition_awards: h.competition_awards.clone(),
                teams: h.teams.clone(),
                managers: h
                    .managers
                    .values()
                    .map(|m| PublicManagerHistory {
                        id: m.id.clone(),
                        name: m.full_name(),
                        club_id: m.team_id.clone(),
                        career_stats: m.career_stats.clone(),
                        career_history: m.career_history.clone(),
                    })
                    .collect(),
                awards: h.awards.clone(),
                appointments: h.appointments.clone(),
            })
    }
    pub fn source_manager(&self, actor: &str) -> Result<Manager, crate::Error> {
        self.management
            .managers
            .get(actor)
            .ok_or(crate::Error::Unauthorized)?;
        let h = self
            .management
            .team_history
            .as_ref()
            .ok_or(crate::Error::Unavailable)?;
        h.actor_manager_ids
            .get(actor)
            .and_then(|id| h.managers.get(id))
            .cloned()
            .ok_or(crate::Error::Unavailable)
    }
    pub(crate) fn validate_team_history_checkpoint(&self) -> Result<(), String> {
        let Some(h) = &self.management.team_history else {
            return Ok(());
        };
        if h.teams.keys().ne(self.management.clubs.keys())
            || h.archived_identities
                .teams
                .iter()
                .any(|(id, t)| id != &t.id || self.management.clubs.contains_key(id))
            || h.archived_identities
                .players
                .iter()
                .any(|(id, p)| id != &p.id || self.management.players.contains_key(id))
            || h.archived_identities
                .managers
                .iter()
                .any(|(id, m)| id != &m.id || h.managers.contains_key(id))
            || h.processed_dismissals > self.dismissals.len()
            || h.managers.iter().any(|(id, m)| {
                id != &m.id
                    || m.team_id
                        .as_ref()
                        .is_some_and(|id| !self.management.clubs.contains_key(id))
            })
            || h.actor_manager_ids
                .values()
                .any(|id| !h.managers.contains_key(id))
            || h.awards.keys().ne(h.completed.keys())
            || h.competition_awards
                .keys()
                .ne(h.competition_completed.keys())
            || h.competition_awards
                .iter()
                .any(|(id, seasons)| seasons.keys().ne(h.competition_completed[id].keys()))
        {
            return Err("Invalid team/manager history registries".into());
        }
        Ok(())
    }
}

fn update_manager_season(
    manager: &mut Manager,
    club: &str,
    name: &str,
    s: &Standing,
    position: u32,
    date: NaiveDate,
) -> Result<(), String> {
    let add = |a: u32, b: u32| {
        a.checked_add(b)
            .ok_or_else(|| "Manager career overflow".to_string())
    };
    let matches = add(add(s.won, s.drawn)?, s.lost)?;
    let stats = &mut manager.career_stats;
    stats.matches_managed = add(stats.matches_managed, matches)?;
    stats.wins = add(stats.wins, s.won)?;
    stats.draws = add(stats.draws, s.drawn)?;
    stats.losses = add(stats.losses, s.lost)?;
    if position == 1 {
        stats.trophies = add(stats.trophies, 1)?;
    }
    stats.best_finish = Some(stats.best_finish.map_or(position, |old| old.min(position)));
    if let Some(entry) = manager
        .career_history
        .iter_mut()
        .find(|e| e.team_id == club && e.end_date.is_none())
    {
        entry.matches = add(entry.matches, matches)?;
        entry.wins = add(entry.wins, s.won)?;
        entry.draws = add(entry.draws, s.drawn)?;
        entry.losses = add(entry.losses, s.lost)?;
        entry.best_league_position = Some(
            entry
                .best_league_position
                .map_or(position, |old| old.min(position)),
        );
    } else {
        manager.career_history.push(ManagerCareerEntry {
            team_id: club.into(),
            team_name: name.into(),
            start_date: date.to_string(),
            end_date: None,
            matches,
            wins: s.won,
            draws: s.drawn,
            losses: s.lost,
            best_league_position: Some(position),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn competition_keys_and_real_season_numbers_preserve_unfinished_foreign_players() {
        let mut game = game();
        let date = game.management.career_date().unwrap();
        for (id, goals) in [("a-p", 3), ("b-p", 7)] {
            let p = game
                .management
                .social
                .as_mut()
                .unwrap()
                .source_players
                .get_mut(id)
                .unwrap();
            p.stats.appearances = 5;
            p.stats.goals = goals;
        }
        game.settle_competition_history("domestic", 10, date, &table()[..1])
            .unwrap();
        game.settle_competition_players(1, date, &[("a".into(), ("domestic".into(), 10))].into())
            .unwrap();
        let sources = &game.management.social.as_ref().unwrap().source_players;
        assert_eq!(sources["a-p"].stats.goals, 0);
        assert_eq!(sources["a-p"].career.last().unwrap().season, 10);
        assert_eq!(sources["b-p"].stats.goals, 7);
        assert!(sources["b-p"].career.is_empty());
        let history = game.public_team_history().unwrap();
        assert_eq!(
            history.competition_awards["domestic"][&10].golden_boot[0].value,
            3.0
        );
        assert!(history.teams["b"].is_empty());
        assert_eq!(
            game.source_manager("b")
                .unwrap()
                .career_stats
                .matches_managed,
            100
        );
        Football::load_validated(game.save_state().unwrap()).unwrap();
    }
    use super::*;
    fn game() -> Football {
        let mut game = crate::personnel::tests::game();
        let managers = ["a", "b"]
            .map(|club| {
                let id = format!("source-{club}");
                let mut m = Manager::new(
                    id.clone(),
                    "Real".into(),
                    club.into(),
                    "1980-01-01".into(),
                    "ENG".into(),
                );
                m.hire(club.into());
                m.career_stats.matches_managed = 100;
                m.career_history.push(ManagerCareerEntry::open(
                    club.into(),
                    club.into(),
                    "2025-08-01".into(),
                ));
                (id, m)
            })
            .into();
        game.configure_team_history(
            managers,
            [
                ("a".into(), "source-a".into()),
                ("b".into(), "source-b".into()),
            ]
            .into(),
        )
        .unwrap();
        game
    }
    fn table() -> Vec<Standing> {
        vec![
            Standing {
                club_id: "a".into(),
                played: 2,
                won: 1,
                drawn: 1,
                lost: 0,
                goals_for: 3,
                goals_against: 1,
                points: 4,
            },
            Standing {
                club_id: "b".into(),
                played: 2,
                won: 0,
                drawn: 1,
                lost: 1,
                goals_for: 1,
                goals_against: 3,
                points: 1,
            },
        ]
    }
    #[test]
    fn removed_original_identity_is_archived_private_and_never_a_live_candidate() {
        let mut game = game();
        let mut player = game.management.social.as_ref().unwrap().source_players["a-p"].clone();
        player.id = "original-player".into();
        player.team_id = Some("original-club".into());
        let mut team = game.management.personnel.as_ref().unwrap().teams["a"].clone();
        team.id = "original-club".into();
        let mut manager =
            game.management.team_history.as_ref().unwrap().managers["source-a"].clone();
        manager.id = "original-manager".into();
        manager.team_id = Some(team.id.clone());
        let archive = ArchivedIdentities {
            teams: [(team.id.clone(), team)].into(),
            players: [(player.id.clone(), player)].into(),
            managers: [(manager.id.clone(), manager)].into(),
        };
        let raw = serde_json::to_value(&archive).unwrap();
        game.configure_archived_identities(archive).unwrap();
        assert!(!game.management.players.contains_key("original-player"));
        assert!(
            !game
                .management
                .career
                .as_ref()
                .unwrap()
                .contracts
                .contains_key("original-player")
        );
        assert!(
            !game
                .management
                .team_history
                .as_ref()
                .unwrap()
                .managers
                .contains_key("original-manager")
        );
        let public = game
            .historical_identity("original-player")
            .unwrap()
            .to_string();
        for private in ["morale", "wage", "condition", "attributes", "grievance"] {
            assert!(!public.contains(private));
        }
        assert!(
            game.configure_archived_identities(ArchivedIdentities::default())
                .is_err()
        );
        let restored = Football::load_validated(game.save_state().unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(
                &restored
                    .management
                    .team_history
                    .as_ref()
                    .unwrap()
                    .archived_identities
            )
            .unwrap(),
            raw
        );
        assert_eq!(
            restored.historical_identity("original-player"),
            game.historical_identity("original-player")
        );
    }

    #[test]
    fn full_season_preserves_team_and_manager_history_and_computes_before_reset() {
        let mut game = game();
        let date = game.management.career_date().unwrap();
        let p = game
            .management
            .social
            .as_mut()
            .unwrap()
            .source_players
            .get_mut("a-p")
            .unwrap();
        p.stats.appearances = 8;
        p.stats.goals = 5;
        p.stats.assists = 2;
        p.stats.avg_rating = 7.8;
        game.settle_team_history(1, date, &table()).unwrap();
        game.settle_player_season(1, date).unwrap();
        let history = game.public_team_history().unwrap();
        assert_eq!(history.teams["a"][0].goals_for, 3);
        assert_eq!(history.awards[&1].golden_boot[0].player_id, "a-p");
        assert_eq!(history.awards[&1].golden_boot[0].value, 5.0);
        assert_eq!(history.awards[&1].player_of_year.len(), 1);
        assert_eq!(
            history.awards[&1].manager_of_season[0].manager_id,
            "source-a"
        );
        let a = game.source_manager("a").unwrap();
        assert_eq!(a.career_stats.matches_managed, 102);
        assert_eq!(a.career_stats.trophies, 1);
        assert_eq!(a.career_history[0].start_date, "2025-08-01");
        assert_eq!(a.career_history[0].matches, 2);
        assert!(game.settle_team_history(1, date, &table()).is_err());
        Football::load_validated(game.save_state().unwrap()).unwrap();
    }
    #[test]
    fn original_manager_stays_fired_and_rich_replacement_waits_seven_sweeps() {
        let mut game = game();
        let date = game.management.career_date().unwrap();
        let board = game.boards.as_mut().unwrap().get_mut("a").unwrap();
        board.state.satisfaction = 0;
        for day in 1..=3 {
            game.advance_closed_day(day, u64::from(day) * 100, u64::from(day + 1) * 100)
                .unwrap();
            if !game.management.managers.contains_key("a") {
                break;
            }
        }
        assert!(game.source_manager("a").is_err());
        let replacement = game.dismissals[0].replacement_manager_id.clone();
        let dismissal_day = game.dismissals[0].day;
        assert!(game.source_manager(&replacement).is_err());
        let fired = &game.management.team_history.as_ref().unwrap().managers["source-a"];
        assert!(fired.team_id.is_none());
        assert_eq!(
            fired.career_history[0].end_date,
            Some((date + chrono::Days::new(u64::from(dismissal_day - 1))).to_string())
        );
        for day in dismissal_day + 1..=dismissal_day + 5 {
            game.advance_closed_day(day, u64::from(day) * 100, u64::from(day + 1) * 100)
                .unwrap();
        }
        assert!(game.source_manager(&replacement).is_err());
        let hire_day = dismissal_day + 6;
        game.advance_closed_day(
            hire_day,
            u64::from(hire_day) * 100,
            u64::from(hire_day + 1) * 100,
        )
        .unwrap();
        let appointed = game.source_manager(&replacement).unwrap();
        assert_ne!(appointed.id, "source-a");
        assert_eq!(appointed.team_id, Some("a".into()));
        assert_eq!(
            appointed.career_history[0].start_date,
            (date + chrono::Days::new(u64::from(hire_day - 1))).to_string()
        );
        assert!(game.source_manager("a").is_err());
        assert_eq!(game.public_team_history().unwrap().appointments.len(), 1);
        Football::load_validated(game.save_state().unwrap()).unwrap();
    }
}
