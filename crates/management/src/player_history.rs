//! Seasonal player history and source aging/retirement lifecycle.
//! Adapted from OpenFoot Manager 64677fee end_of_season.rs and aging.rs.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
use crate::football::Football;
use chrono::{Datelike, NaiveDate};
use domain::{
    manager::Manager,
    player::{CareerEntry, Player, PlayerSeasonStats},
    staff::{Staff, StaffAttributes, StaffRole},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerSeasonRecord {
    #[serde(default)]
    pub competition_id: Option<String>,
    pub season: u32,
    pub player_id: String,
    pub player_name: String,
    pub club_id: Option<String>,
    pub stats: PlayerSeasonStats,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlayerHistoryState {
    #[serde(default)]
    pub competition_completed: BTreeMap<String, BTreeMap<u32, NaiveDate>>,
    pub records: Vec<PlayerSeasonRecord>,
    pub completed: BTreeMap<u32, NaiveDate>,
    /// Retiree conversions plus imported/generated unemployed candidates.
    pub retired_managers: BTreeMap<String, Manager>,
    #[serde(default)]
    pub generated_candidates: u64,
}

impl Football {
    pub fn configure_unemployed_managers(
        &mut self,
        managers: BTreeMap<String, Manager>,
    ) -> Result<(), String> {
        if self.started
            || self.management.sequence != 0
            || self.management.player_history.is_some()
            || managers.iter().any(|(id, m)| {
                id != &m.id || m.team_id.is_some() || self.management.managers.contains_key(id)
            })
        {
            return Err("Invalid initial unemployed manager market".into());
        }
        self.management.player_history = Some(PlayerHistoryState {
            retired_managers: managers,
            ..Default::default()
        });
        Ok(())
    }
    pub fn unemployed_manager_candidates(&self, actor: &str) -> Result<Vec<Manager>, crate::Error> {
        self.management
            .managers
            .get(actor)
            .ok_or(crate::Error::Unauthorized)?;
        Ok(self
            .management
            .player_history
            .as_ref()
            .map(|h| h.retired_managers.values().cloned().collect())
            .unwrap_or_default())
    }
    /// Source order: preserve career/statistics, apply aging, then reset stats.
    /// Caller invokes inside the atomic completed-season transaction.
    pub(crate) fn settle_player_season(
        &mut self,
        season: u32,
        date: NaiveDate,
    ) -> Result<(), String> {
        let clubs = self
            .management
            .clubs
            .keys()
            .map(|id| (id.clone(), ("primary".into(), season)))
            .collect();
        self.settle_competition_players(season, date, &clubs)
    }
    pub(crate) fn settle_competition_players(
        &mut self,
        season: u32,
        date: NaiveDate,
        clubs: &BTreeMap<String, (String, u32)>,
    ) -> Result<(), String> {
        if self.management.social.is_none() {
            return Ok(());
        }
        if self
            .management
            .player_history
            .as_ref()
            .is_some_and(|h| h.completed.contains_key(&season))
        {
            return Err("Player season already settled".into());
        }
        let mut players = self
            .project_source_players()?
            .into_values()
            .collect::<Vec<_>>();
        let names = self
            .management
            .clubs
            .iter()
            .map(|(id, c)| (id.clone(), c.name.clone()))
            .collect::<BTreeMap<_, _>>();
        let history = self
            .management
            .player_history
            .get_or_insert_with(PlayerHistoryState::default);
        let selected = players
            .iter()
            .filter(|p| p.team_id.as_ref().is_none_or(|id| clubs.contains_key(id)))
            .map(|p| p.id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        for (competition, league_season) in clubs.values() {
            history
                .competition_completed
                .entry(competition.clone())
                .or_default()
                .insert(*league_season, date);
        }
        for player in &mut players {
            if !selected.contains(&player.id) {
                continue;
            }
            let entry = player.team_id.as_ref().and_then(|id| clubs.get(id));
            let player_season = entry.map_or(season, |(_, s)| *s);
            history.records.push(PlayerSeasonRecord {
                competition_id: entry.map(|(id, _)| id.clone()),
                season: player_season,
                player_id: player.id.clone(),
                player_name: player.match_name.clone(),
                club_id: player.team_id.clone(),
                stats: player.stats.clone(),
            });
            if player.stats.appearances > 0 {
                player.career.push(CareerEntry {
                    season: player_season,
                    team_id: player.team_id.clone().unwrap_or_default(),
                    team_name: player
                        .team_id
                        .as_ref()
                        .and_then(|id| names.get(id))
                        .cloned()
                        .unwrap_or_else(|| "Free Agent".into()),
                    appearances: player.stats.appearances,
                    goals: player.stats.goals,
                    assists: player.stats.assists,
                });
            }
        }
        if let Some(personnel) = &mut self.management.personnel {
            crate::aging::apply(&mut players, &mut personnel.teams, date, season);
        } else {
            crate::aging::apply(&mut players, &mut BTreeMap::new(), date, season);
        }
        for player in &mut players {
            if selected.contains(&player.id) {
                player.stats = PlayerSeasonStats::default();
            }
            if player.retired && player.team_id.is_none() && !player.career.is_empty() {
                let (manager, scout) = retired_candidates(player);
                history
                    .retired_managers
                    .entry(manager.id.clone())
                    .or_insert(manager);
                if let Some(personnel) = &mut self.management.personnel {
                    personnel.staff.entry(scout.id.clone()).or_insert(scout);
                }
            }
        }
        history.completed.insert(season, date);
        if let Some(personnel) = &mut self.management.personnel {
            use rand::SeedableRng;
            let floor = self
                .management
                .clubs
                .len()
                .checked_mul(2)
                .ok_or("Candidate floor overflow")?;
            let managers_needed = floor.saturating_sub(history.retired_managers.len());
            let scouts_needed = floor.saturating_sub(
                personnel
                    .staff
                    .values()
                    .filter(|s| s.team_id.is_none() && s.role == StaffRole::Scout)
                    .count(),
            );
            let count =
                u64::try_from(managers_needed + scouts_needed).map_err(|e| e.to_string())?;
            let end = history
                .generated_candidates
                .checked_add(count)
                .ok_or("Candidate counter overflow")?;
            let manager_ids = (0..managers_needed)
                .map(|i| {
                    format!(
                        "career-manager:{:016x}:{}",
                        personnel.seed,
                        history.generated_candidates + i as u64
                    )
                })
                .collect::<Vec<_>>();
            let scout_ids = (managers_needed..managers_needed + scouts_needed)
                .map(|i| {
                    format!(
                        "career-scout:{:016x}:{}",
                        personnel.seed,
                        history.generated_candidates + i as u64
                    )
                })
                .collect::<Vec<_>>();
            if manager_ids.iter().any(|id| {
                history.retired_managers.contains_key(id)
                    || self.management.managers.contains_key(id)
            }) || scout_ids.iter().any(|id| personnel.staff.contains_key(id))
            {
                return Err("Career candidate ID collision".into());
            }
            let mut rng = rand::rngs::StdRng::seed_from_u64(
                personnel.seed ^ u64::from(season) ^ 0x6361_7265_6572_6765,
            );
            let (managers, scouts) = crate::youth::generate_career_candidates(
                date.year() as u32,
                &manager_ids,
                &scout_ids,
                &mut rng,
            )?;
            history
                .retired_managers
                .extend(managers.into_iter().map(|m| (m.id.clone(), m)));
            personnel
                .staff
                .extend(scouts.into_iter().map(|s| (s.id.clone(), s)));
            history.generated_candidates = end;
        }
        for player in &players {
            let original_club = self.management.players[&player.id].club_id.clone();
            let engine = self
                .attributes
                .get_mut(&player.id)
                .ok_or("Missing aging engine player")?;
            // Source seasonal aging changes only these five raw attributes; it
            // does not refresh OVR/potential/traits until a later training pass.
            engine.pace = player.attributes.pace;
            engine.passing = player.attributes.passing;
            engine.vision = player.attributes.vision;
            engine.decisions = player.attributes.decisions;
            engine.composure = player.attributes.composure;
            if player.retired && !original_club.is_empty() {
                self.management
                    .players
                    .get_mut(&player.id)
                    .unwrap()
                    .club_id
                    .clear();
                let career = self
                    .management
                    .career
                    .as_mut()
                    .ok_or("Aging career unavailable")?;
                let contract = career
                    .contracts
                    .get_mut(&player.id)
                    .ok_or("Aging contract missing")?;
                contract.end_date = None;
                contract.let_expire = false;
                if let Some(lineup) = self.management.lineups.get_mut(&original_club) {
                    lineup.retain(|id| id != &player.id);
                }
                self.management.remove_training_membership(&player.id);
                let revision = self.management.club_revisions[&original_club]
                    .checked_add(1)
                    .ok_or("Retirement revision overflow")?;
                self.management
                    .club_revisions
                    .insert(original_club, revision);
            }
            let revision = self.management.player_revisions[&player.id]
                .checked_add(1)
                .ok_or("Aging revision overflow")?;
            self.management
                .player_revisions
                .insert(player.id.clone(), revision);
            if let Some(availability) = &mut self.management.availability {
                if selected.contains(&player.id) {
                    availability
                        .get_mut(&player.id)
                        .ok_or("Aging availability missing")?
                        .reset_season_cards();
                }
            }
            if let Some(recovery) = &mut self.recovery {
                let birth = NaiveDate::parse_from_str(&player.date_of_birth, "%Y-%m-%d")
                    .map_err(|e| e.to_string())?;
                recovery
                    .players
                    .get_mut(&player.id)
                    .ok_or("Aging recovery missing")?
                    .age = date.year().saturating_sub(birth.year()) as u32;
            }
        }
        if let Some(personnel) = &mut self.management.personnel {
            for team in personnel
                .teams
                .values_mut()
                .filter(|team| clubs.contains_key(&team.id))
            {
                team.form.clear();
            }
        }
        self.management.social.as_mut().unwrap().source_players =
            players.into_iter().map(|p| (p.id.clone(), p)).collect();
        Ok(())
    }
    pub fn public_player_seasons(&self) -> Vec<PlayerSeasonRecord> {
        self.management
            .player_history
            .as_ref()
            .map(|h| h.records.clone())
            .unwrap_or_default()
    }
    pub(crate) fn validate_player_history_checkpoint(&self) -> Result<(), String> {
        let Some(h) = &self.management.player_history else {
            return Ok(());
        };
        let today = self
            .management
            .career_date()
            .ok_or("Player history requires career")?;
        let mut keys = std::collections::BTreeSet::new();
        if self.management.social.is_none()
            || h.completed.values().any(|d| *d > today)
            || h.records.iter().any(|r| {
                !keys.insert((&r.competition_id, r.season, &r.player_id))
                    || r.competition_id.as_ref().map_or(
                        !h.completed.contains_key(&r.season),
                        |id| {
                            h.competition_completed
                                .get(id)
                                .is_none_or(|seasons| !seasons.contains_key(&r.season))
                        },
                    )
                    || !self.management.players.contains_key(&r.player_id)
                    || r.club_id
                        .as_ref()
                        .is_some_and(|id| !self.management.clubs.contains_key(id))
                    || !r.stats.avg_rating.is_finite()
            })
            || h.retired_managers
                .iter()
                .any(|(id, m)| id != &m.id || m.team_id.is_some())
        {
            return Err("Invalid player seasonal history".into());
        }
        Ok(())
    }
}

pub fn retired_candidates(player: &Player) -> (Manager, Staff) {
    let mut names = player.full_name.splitn(2, ' ');
    let first = names.next().unwrap_or(&player.full_name).to_string();
    let last = names.next().unwrap_or("").to_string();
    let mut manager = Manager::new(
        format!("mgr_retired_{}", player.id),
        first.clone(),
        last.clone(),
        player.date_of_birth.clone(),
        player.nationality.clone(),
    );
    manager.reputation = 200u32
        .saturating_add(u32::from(player.ovr) * 6)
        .saturating_add(player.career.len() as u32 * 30)
        .clamp(200, 900);
    manager.satisfaction = 50;
    manager.fan_approval = 50;
    let a = &player.attributes;
    let mut scout = Staff::new(
        format!("staff_retired_scout_{}", player.id),
        first,
        last,
        player.date_of_birth.clone(),
        StaffRole::Scout,
        StaffAttributes {
            coaching: (a.leadership / 2).max(10),
            judging_ability: ((u16::from(a.vision) + u16::from(a.decisions)) / 2).min(100) as u8,
            judging_potential: ((u16::from(a.positioning) + u16::from(a.teamwork)) / 2).min(100)
                as u8,
            physiotherapy: 10,
        },
    );
    scout.nationality = player.nationality.clone();
    (manager, scout)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn season_records_precede_retirement_and_keep_live_registry_consistent() {
        let mut game = crate::personnel::tests::game();
        let date = game.management.career_date().unwrap();
        game.management
            .career
            .as_mut()
            .unwrap()
            .contracts
            .get_mut("a-p")
            .unwrap()
            .date_of_birth = NaiveDate::from_ymd_opt(1980, 1, 1).unwrap();
        let p = game
            .management
            .social
            .as_mut()
            .unwrap()
            .source_players
            .get_mut("a-p")
            .unwrap();
        p.stats.appearances = 6;
        p.stats.goals = 3;
        p.stats.assists = 2;
        p.stats.avg_rating = 6.1;
        p.transfer_listed = true;
        p.loan_listed = true;
        game.management
            .lineups
            .insert("a".into(), vec!["a-p".into()]);
        let before = game.attributes["a-p"].pace;
        let mut replay = game.clone();
        game.settle_player_season(1, date).unwrap();
        replay.settle_player_season(1, date).unwrap();
        assert_eq!(
            serde_json::to_value(game.public_player_seasons()).unwrap(),
            serde_json::to_value(replay.public_player_seasons()).unwrap()
        );
        assert!(game.attributes["a-p"].pace < before);
        assert_eq!(game.management.players["a-p"].club_id, "");
        assert!(game.management.lineups["a"].is_empty());
        assert!(
            game.management.career.as_ref().unwrap().contracts["a-p"]
                .end_date
                .is_none()
        );
        let source = &game.management.social.as_ref().unwrap().source_players["a-p"];
        assert!(source.retired);
        assert!(!source.transfer_listed);
        assert_eq!(source.stats.appearances, 0);
        assert_eq!(
            (
                source.career[0].appearances,
                source.career[0].goals,
                source.career[0].assists
            ),
            (6, 3, 2)
        );
        let record = game
            .public_player_seasons()
            .into_iter()
            .find(|r| r.player_id == "a-p")
            .unwrap();
        assert_eq!(record.club_id, Some("a".into()));
        assert_eq!(record.stats.goals, 3);
        assert!(
            game.management
                .personnel
                .as_ref()
                .unwrap()
                .staff
                .contains_key("staff_retired_scout_a-p")
        );
        assert!(
            game.management
                .player_history
                .as_ref()
                .unwrap()
                .retired_managers
                .contains_key("mgr_retired_a-p")
        );
        assert_eq!(game.unemployed_manager_candidates("a").unwrap().len(), 4);
        assert!(game.unemployed_manager_candidates("outsider").is_err());
        assert_eq!(
            game.management
                .personnel
                .as_ref()
                .unwrap()
                .staff
                .values()
                .filter(|s| s.team_id.is_none() && s.role == StaffRole::Scout)
                .count(),
            4
        );
        assert!(game.settle_player_season(1, date).is_err());
        game.validate_player_history_checkpoint().unwrap();
        Football::load_validated(game.save_state().unwrap()).unwrap();
    }
}
