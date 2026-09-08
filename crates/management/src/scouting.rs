//! Player scouting extracted from OpenFoot Manager `ofm_core/scouting.rs` and
//! MCP `tools_impl/transfers.rs::transfer_market_browse`, revision
//! 64677fee9047a1182005d666bafa5dbc025dca5c.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Caller authenticates (manager, club), supplies live domain records, stable
//! assignment IDs, and an explicit seeded RNG. Reports belong to the requesting
//! manager; another manager may independently scout the same player. Source
//! assignment delays, discovery, uncertainty and message payloads are preserved.
//! Youth searches become ready for the seeded `youth::generate_pool` adapter;
//! `complete_youth` consumes its real pool and creates the private source report.

use chrono::{Datelike, NaiveDate};
use domain::message::*;
use domain::player::{Player, Position};
use domain::staff::{Staff, StaffRole};
use domain::team::Team;
use rand::{Rng, RngExt};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
    pub id: String,
    pub scout_id: String,
    pub player_id: String,
    pub days_remaining: u32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum YouthRegion {
    #[default]
    Domestic,
    International,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum YouthObjective {
    #[default]
    Balanced,
    HighPotential,
    ReadySoon,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct YouthAssignment {
    pub id: String,
    pub scout_id: String,
    pub region: YouthRegion,
    pub objective: YouthObjective,
    pub target_position: Option<Position>,
    pub days_remaining: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScoutingDesk {
    pub club_id: String,
    pub assignments: Vec<Assignment>,
    pub youth_assignments: Vec<YouthAssignment>,
    pub messages: Vec<InboxMessage>,
    used_ids: BTreeSet<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ScoutingState {
    desks: BTreeMap<String, ScoutingDesk>,
    last_processed: Option<NaiveDate>,
}

pub fn player_assignment_days(judging_ability: u8) -> u32 {
    match judging_ability {
        80.. => 2,
        60.. => 3,
        40.. => 4,
        _ => 5,
    }
}
pub fn youth_assignment_days(
    judging_potential: u8,
    region: YouthRegion,
    objective: YouthObjective,
) -> u32 {
    (match judging_potential {
        80.. => 4,
        60.. => 5,
        40.. => 6,
        _ => 7,
    }) + u32::from(region == YouthRegion::International)
        + u32::from(objective == YouthObjective::HighPotential)
}

impl ScoutingState {
    pub(crate) fn validate_checkpoint(
        &self,
        clubs: &BTreeSet<String>,
        today: NaiveDate,
    ) -> Result<(), String> {
        if self.last_processed.is_some_and(|d| d > today) {
            return Err("Scouting processed a future day".into());
        }
        let mut assigned = BTreeSet::new();
        for (owner, desk) in &self.desks {
            if owner.trim().is_empty() || !clubs.contains(&desk.club_id) {
                return Err("Invalid scouting desk owner/club".into());
            }
            let mut ids = BTreeSet::new();
            let mut targets = BTreeSet::new();
            for a in &desk.assignments {
                if a.id.trim().is_empty()
                    || !desk.used_ids.contains(&a.id)
                    || !ids.insert(&a.id)
                    || a.player_id.trim().is_empty()
                    || !targets.insert(&a.player_id)
                    || a.scout_id.trim().is_empty()
                    || !assigned.insert(&a.scout_id)
                    || !(1..=5).contains(&a.days_remaining)
                {
                    return Err("Invalid player scouting assignment".into());
                }
            }
            for a in &desk.youth_assignments {
                if a.id.trim().is_empty()
                    || !desk.used_ids.contains(&a.id)
                    || !ids.insert(&a.id)
                    || a.scout_id.trim().is_empty()
                    || !assigned.insert(&a.scout_id)
                    || a.days_remaining > 9
                {
                    return Err("Invalid youth scouting assignment".into());
                }
            }
        }
        Ok(())
    }
    pub fn assign_player_at_facility(
        &mut self,
        owner: &str,
        club: &str,
        id: &str,
        scout: &Staff,
        player: &Player,
        facility_level: u8,
    ) -> Result<(), String> {
        self.assign_player(owner, club, id, scout, player)?;
        let assignment = self
            .desks
            .get_mut(owner)
            .unwrap()
            .assignments
            .last_mut()
            .unwrap();
        assignment.days_remaining =
            crate::facilities::scouting_assignment_days(assignment.days_remaining, facility_level);
        Ok(())
    }
    pub fn start_youth_at_facility(
        &mut self,
        owner: &str,
        club: &str,
        id: &str,
        scout: &Staff,
        region: YouthRegion,
        objective: YouthObjective,
        target_position: Option<Position>,
        facility_level: u8,
    ) -> Result<(), String> {
        self.start_youth(owner, club, id, scout, region, objective, target_position)?;
        let assignment = self
            .desks
            .get_mut(owner)
            .unwrap()
            .youth_assignments
            .last_mut()
            .unwrap();
        assignment.days_remaining =
            crate::facilities::scouting_assignment_days(assignment.days_remaining, facility_level);
        Ok(())
    }
    /// Finish a ready search only after the real generator supplies its complete
    /// pool. Ownership and pool validation happen before changing the desk.
    pub fn complete_youth(
        &mut self,
        owner: &str,
        id: &str,
        pool: Vec<Player>,
        team: &Team,
        scout: &Staff,
        date: NaiveDate,
    ) -> Result<(), String> {
        let desk = self.desks.get(owner).ok_or("Unknown scouting owner")?;
        let index = desk
            .youth_assignments
            .iter()
            .position(|a| a.id == id)
            .ok_or("Unknown youth search")?;
        let assignment = &desk.youth_assignments[index];
        if desk.club_id != team.id
            || assignment.days_remaining != 0
            || assignment.scout_id != scout.id
            || self.last_processed != Some(date)
        {
            return Err(
                "Youth search completion does not match owner, scout or processing date".into(),
            );
        }
        if pool.iter().any(|p| {
            p.id.trim().is_empty()
                || p.team_id.is_some()
                || p.squad_role != domain::player::SquadRole::Youth
        }) || pool.iter().map(|p| &p.id).collect::<BTreeSet<_>>().len() != pool.len()
        {
            return Err("Invalid generated youth pool".into());
        }
        let prospects = rank_youth_candidates(pool, assignment.objective)?;
        let message = crate::youth::build_youth_recruitment_report(
            id,
            &format!("{} {}", scout.first_name, scout.last_name),
            &team.id,
            &team.name,
            &prospects,
            assignment.region,
            assignment.objective,
            assignment.target_position.as_ref(),
            &date.to_string(),
        );
        let desk = self.desks.get_mut(owner).unwrap();
        desk.youth_assignments.remove(index);
        desk.messages.push(message);
        Ok(())
    }
    pub fn view(&self, owner: &str) -> Option<&ScoutingDesk> {
        self.desks.get(owner)
    }
    fn check_owner(&self, owner: &str, club: &str, id: &str) -> Result<(), String> {
        if owner.trim().is_empty() || club.trim().is_empty() || id.trim().is_empty() {
            return Err("Invalid scouting identity".into());
        }
        if self
            .desks
            .get(owner)
            .is_some_and(|desk| desk.club_id != club || desk.used_ids.contains(id))
        {
            return Err("Scouting owner or assignment ID mismatch".into());
        }
        Ok(())
    }
    fn check_scout(&self, club: &str, scout: &Staff) -> Result<(), String> {
        if scout.role != StaffRole::Scout {
            return Err("be.error.scouting.staffMemberNotScout".into());
        }
        if scout.team_id.as_deref() != Some(club) {
            return Err("be.error.scouting.scoutNotInTeam".into());
        }
        if self.desks.values().any(|desk| {
            desk.assignments.iter().any(|a| a.scout_id == scout.id)
                || desk
                    .youth_assignments
                    .iter()
                    .any(|a| a.scout_id == scout.id)
        }) {
            return Err("be.error.scouting.scoutAssignmentFull?currentCount=1&maxSlots=1".into());
        }
        Ok(())
    }
    fn desk(&mut self, owner: &str, club: &str, id: &str) -> &mut ScoutingDesk {
        let desk = self
            .desks
            .entry(owner.into())
            .or_insert_with(|| ScoutingDesk {
                club_id: club.into(),
                assignments: vec![],
                youth_assignments: vec![],
                messages: vec![],
                used_ids: BTreeSet::new(),
            });
        desk.used_ids.insert(id.into());
        desk
    }
    pub fn assign_player(
        &mut self,
        owner: &str,
        club: &str,
        id: &str,
        scout: &Staff,
        player: &Player,
    ) -> Result<(), String> {
        self.check_owner(owner, club, id)?;
        self.check_scout(club, scout)?;
        if player.team_id.as_deref() == Some(club) {
            return Err("be.error.scouting.cannotScoutOwnPlayer".into());
        }
        if self
            .desks
            .get(owner)
            .is_some_and(|desk| desk.assignments.iter().any(|a| a.player_id == player.id))
        {
            return Err("be.error.scouting.playerAlreadyScouted".into());
        }
        self.desk(owner, club, id).assignments.push(Assignment {
            id: id.into(),
            scout_id: scout.id.clone(),
            player_id: player.id.clone(),
            days_remaining: player_assignment_days(scout.attributes.judging_ability),
        });
        Ok(())
    }
    pub fn start_youth(
        &mut self,
        owner: &str,
        club: &str,
        id: &str,
        scout: &Staff,
        region: YouthRegion,
        objective: YouthObjective,
        target_position: Option<Position>,
    ) -> Result<(), String> {
        self.check_owner(owner, club, id)?;
        self.check_scout(club, scout)?;
        let target_position = target_position.map(|p| p.to_group_position());
        if self.desks.get(owner).is_some_and(|desk| {
            desk.youth_assignments.iter().any(|a| {
                a.region == region
                    && a.objective == objective
                    && a.target_position == target_position
            })
        }) {
            return Err("be.error.scouting.youthSearchAlreadyActive".into());
        }
        self.desk(owner, club, id)
            .youth_assignments
            .push(YouthAssignment {
                id: id.into(),
                scout_id: scout.id.clone(),
                region,
                objective,
                target_position,
                days_remaining: youth_assignment_days(
                    scout.attributes.judging_potential,
                    region,
                    objective,
                ),
            });
        Ok(())
    }
    pub fn cancel_youth(&mut self, owner: &str, id: &str) -> Result<(), String> {
        let desk = self
            .desks
            .get_mut(owner)
            .ok_or("be.error.scouting.youthAssignmentNotFound")?;
        let index = desk
            .youth_assignments
            .iter()
            .position(|a| a.id == id)
            .ok_or("be.error.scouting.youthAssignmentNotFound")?;
        desk.youth_assignments.remove(index);
        Ok(())
    }
    pub fn reassign_youth(&mut self, owner: &str, id: &str, scout: &Staff) -> Result<(), String> {
        let desk = self
            .desks
            .get(owner)
            .ok_or("be.error.scouting.youthAssignmentNotFound")?;
        let assignment = desk
            .youth_assignments
            .iter()
            .find(|a| a.id == id)
            .ok_or("be.error.scouting.youthAssignmentNotFound")?;
        if assignment.scout_id == scout.id {
            return Err("be.error.scouting.scoutAlreadyAssignedToSearch".into());
        }
        self.check_scout(&desk.club_id, scout)?;
        self.desks
            .get_mut(owner)
            .unwrap()
            .youth_assignments
            .iter_mut()
            .find(|a| a.id == id)
            .unwrap()
            .scout_id = scout.id.clone();
        Ok(())
    }
    /// A due youth assignment remains reserved until the real generator adapter
    /// is integrated. It is not counted as a completed report.
    pub fn pending_youth_generation(&self, owner: &str) -> Vec<&YouthAssignment> {
        self.desks
            .get(owner)
            .map(|desk| {
                desk.youth_assignments
                    .iter()
                    .filter(|a| a.days_remaining == 0)
                    .collect()
            })
            .unwrap_or_default()
    }
    /// Exactly one daily sweep; duplicate or skipped dates are rejected before RNG
    /// consumption. Source drops completed assignments whose scout/player vanished.
    pub fn advance_day<R: Rng>(
        &mut self,
        today: NaiveDate,
        staff: &BTreeMap<String, Staff>,
        players: &BTreeMap<String, Player>,
        teams: &BTreeMap<String, Team>,
        rng: &mut R,
    ) -> Result<(), String> {
        if self
            .last_processed
            .is_some_and(|last| last.succ_opt() != Some(today))
        {
            return Err("Scouting days must advance exactly once and consecutively".into());
        }
        for desk in self.desks.values_mut() {
            for assignment in &mut desk.assignments {
                assignment.days_remaining = assignment.days_remaining.saturating_sub(1);
            }
            for assignment in &mut desk.youth_assignments {
                assignment.days_remaining = assignment.days_remaining.saturating_sub(1);
            }
            for assignment in desk.assignments.iter().filter(|a| a.days_remaining == 0) {
                if let (Some(scout), Some(player)) = (
                    staff.get(&assignment.scout_id),
                    players.get(&assignment.player_id),
                ) {
                    let team_name = player
                        .team_id
                        .as_ref()
                        .and_then(|id| teams.get(id))
                        .map(|t| t.name.as_str());
                    desk.messages.push(build_report(
                        &assignment.id,
                        scout,
                        player,
                        team_name,
                        today,
                        rng,
                    ));
                }
            }
            desk.assignments.retain(|a| a.days_remaining > 0);
        }
        self.last_processed = Some(today);
        Ok(())
    }
}

/// Six attribute fuzzes, Fisher-Yates discovery shuffle, OVR fuzz, then optional
/// potential fuzz: preserve source RNG call order as well as numeric bounds.
pub fn build_report<R: Rng>(
    assignment_id: &str,
    scout: &Staff,
    player: &Player,
    team_name: Option<&str>,
    date: NaiveDate,
    rng: &mut R,
) -> InboxMessage {
    let ability = scout.attributes.judging_ability;
    let potential = scout.attributes.judging_potential;
    let (noise, count) = match ability {
        80..=u8::MAX => (2_i16, 6),
        60..=79 => (5, 5),
        40..=59 => (8, 3),
        _ => (12, 2),
    };
    let fuzz = |value: u8, rng: &mut R| -> u8 {
        (i16::from(value) + rng.random_range(-noise..=noise)).clamp(1, 99) as u8
    };
    let a = &player.attributes;
    let values = [
        a.pace,
        a.shooting,
        a.passing,
        a.dribbling,
        a.defending,
        a.strength,
    ]
    .map(|value| fuzz(value, rng));
    let mut indices = vec![0_usize, 1, 2, 3, 4, 5];
    for i in (1..indices.len()).rev() {
        let j = rng.random_range(0..=i);
        indices.swap(i, j);
    }
    let revealed = |index: usize| {
        if indices[..count].contains(&index) {
            Some(values[index])
        } else {
            None
        }
    };
    let rating = if player.ovr > 0 {
        u32::from(fuzz(player.ovr, rng))
    } else {
        (0..6).filter_map(revealed).map(u32::from).sum::<u32>() / count as u32
    };
    let rating_key = match rating {
        80.. => "common.scoutRatings.excellent",
        70.. => "common.scoutRatings.veryGood",
        60.. => "common.scoutRatings.good",
        50.. => "common.scoutRatings.average",
        _ => "common.scoutRatings.belowAverage",
    };
    let potential_key = if potential >= 70 {
        let value = if player.potential > 0 {
            u32::from(fuzz(player.potential, rng))
        } else {
            rating
        };
        match value {
            85.. => "common.scoutPotential.worldClass",
            70.. => "common.scoutPotential.strong",
            _ => "common.scoutPotential.moderate",
        }
    } else {
        "common.scoutPotential.unclear"
    };
    let confidence_key = match ability {
        80.. => "common.scoutConfidence.high",
        60.. => "common.scoutConfidence.moderate",
        _ => "common.scoutConfidence.low",
    };
    let report = ScoutReportData {
        player_id: player.id.clone(),
        player_name: player.match_name.clone(),
        position: format!("{:?}", player.position),
        nationality: player.nationality.clone(),
        dob: player.date_of_birth.clone(),
        team_name: team_name.map(str::to_owned),
        pace: revealed(0),
        shooting: revealed(1),
        passing: revealed(2),
        dribbling: revealed(3),
        defending: revealed(4),
        physical: revealed(5),
        condition: (ability >= 60).then_some(player.condition),
        morale: (ability >= 80).then_some(player.morale),
        avg_rating: Some(rating),
        rating_key: rating_key.into(),
        potential_key: potential_key.into(),
        confidence_key: confidence_key.into(),
    };
    let scout_name = format!("{} {}", scout.first_name, scout.last_name);
    InboxMessage::new(
        format!("scout_report_{assignment_id}"),
        String::new(),
        String::new(),
        scout_name.clone(),
        date.to_string(),
    )
    .with_category(MessageCategory::ScoutReport)
    .with_priority(MessagePriority::Normal)
    .with_sender_role("Scout")
    .with_action(MessageAction {
        id: "ack".into(),
        label: "Noted".into(),
        action_type: ActionType::Acknowledge,
        resolved: false,
        label_key: Some("be.msg.event.ack".into()),
    })
    .with_context(MessageContext {
        player_id: Some(player.id.clone()),
        scout_report: Some(report),
        ..Default::default()
    })
    .with_i18n(
        "be.msg.scoutReport.subject",
        "be.msg.scoutReport.body",
        HashMap::from([
            ("player".into(), player.match_name.clone()),
            ("scout".into(), scout_name),
            ("ratingDesc".into(), rating_key.into()),
            ("potentialDesc".into(), potential_key.into()),
            ("confidence".into(), confidence_key.into()),
        ]),
    )
    .with_sender_i18n("be.sender.scout", "be.role.scout")
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketFilter {
    pub position: Option<String>,
    pub max_price: Option<u64>,
    pub listed_only: Option<bool>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketPlayer {
    pub id: String,
    pub name: String,
    pub position: String,
    pub age: String,
    pub ovr: u8,
    pub team: String,
    pub listed: String,
    pub wage: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketView {
    pub total: usize,
    pub players: Vec<MarketPlayer>,
}
fn position_label(position: &Position) -> &'static str {
    use Position::*;
    match position {
        Goalkeeper => "GK",
        Defender => "DF",
        Midfielder => "MF",
        Forward => "FW",
        RightBack => "RB",
        LeftBack => "LB",
        CenterBack => "CB",
        RightWingBack => "RWB",
        LeftWingBack => "LWB",
        DefensiveMidfielder => "DM",
        CentralMidfielder => "CM",
        AttackingMidfielder => "AM",
        RightMidfielder => "RM",
        LeftMidfielder => "LM",
        RightWinger => "RW",
        LeftWinger => "LW",
        Striker => "ST",
    }
}
/// Preserve the selected MCP baseline's public market fields and first-30 cap.
/// Its max-price filter is raw wage *52, NOT market_value or finance charge.
pub fn browse_market(
    club: &str,
    players: &[Player],
    teams: &BTreeMap<String, Team>,
    today: NaiveDate,
    filter: &MarketFilter,
) -> MarketView {
    let matching = players
        .iter()
        .filter(|p| p.team_id.as_deref() != Some(club))
        .filter(|p| filter.listed_only != Some(true) || p.transfer_listed || p.loan_listed)
        .filter(|p| {
            filter.position.as_ref().is_none_or(|pos| {
                position_label(&p.position).eq_ignore_ascii_case(pos)
                    || format!("{:?}", p.position).eq_ignore_ascii_case(pos)
            })
        })
        .filter(|p| {
            filter
                .max_price
                .is_none_or(|max| u64::from(p.wage) * 52 <= max)
        })
        .collect::<Vec<_>>();
    let rows = matching
        .iter()
        .take(30)
        .map(|p| {
            let age = NaiveDate::parse_from_str(&p.date_of_birth, "%Y-%m-%d")
                .map(|dob| {
                    (today.year()
                        - dob.year()
                        - i32::from((today.month(), today.day()) < (dob.month(), dob.day())))
                    .to_string()
                })
                .unwrap_or_else(|_| "?".into());
            MarketPlayer {
                id: p.id.clone(),
                name: p.match_name.clone(),
                position: position_label(&p.position).into(),
                age,
                ovr: p.ovr,
                team: p
                    .team_id
                    .as_ref()
                    .and_then(|id| teams.get(id))
                    .map(|t| t.name.clone())
                    .unwrap_or_else(|| "Free".into()),
                listed: if p.transfer_listed {
                    "T"
                } else if p.loan_listed {
                    "L"
                } else {
                    "-"
                }
                .into(),
                wage: p.wage,
            }
        })
        .collect();
    MarketView {
        total: matching.len(),
        players: rows,
    }
}

/// Adapter-side selection AFTER the real source generator supplies its pool.
/// Required pool sizes: Balanced 4, other objectives 6; retain the best three.
pub fn rank_youth_candidates(
    mut candidates: Vec<Player>,
    objective: YouthObjective,
) -> Result<Vec<Player>, String> {
    let expected = if objective == YouthObjective::Balanced {
        4
    } else {
        6
    };
    if candidates.len() != expected {
        return Err("Youth adapter must supply the full source candidate pool".into());
    }
    let score = |p: &Player| match objective {
        YouthObjective::Balanced => (p.ovr.saturating_add(p.potential / 2), p.potential),
        YouthObjective::HighPotential => (p.potential, p.ovr),
        YouthObjective::ReadySoon => (p.ovr, p.potential),
    };
    candidates.sort_by(|a, b| score(b).cmp(&score(a)));
    candidates.truncate(3);
    Ok(candidates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{player::PlayerAttributes, staff::StaffAttributes};
    use rand::{SeedableRng, rngs::StdRng};

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 8, 1).unwrap()
    }
    fn scout(id: &str, club: &str, skill: u8) -> Staff {
        let mut s = Staff::new(
            id.into(),
            "A".into(),
            "Scout".into(),
            "1980-01-01".into(),
            StaffRole::Scout,
            StaffAttributes {
                coaching: 50,
                judging_ability: skill,
                judging_potential: skill,
                physiotherapy: 50,
            },
        );
        s.team_id = Some(club.into());
        s
    }
    fn player(id: &str) -> Player {
        let attributes: PlayerAttributes = serde_json::from_value(serde_json::json!({
            "pace":50,"stamina":50,"strength":50,"passing":50,"shooting":50,"tackling":50,
            "dribbling":50,"defending":50,"positioning":50,"vision":50,"decisions":50
        }))
        .unwrap();
        let mut p = Player::new(
            id.into(),
            id.into(),
            id.into(),
            "2000-08-02".into(),
            "GB".into(),
            Position::Striker,
            attributes,
        );
        p.ovr = 50;
        p.potential = 90;
        p.wage = 100;
        p
    }

    #[test]
    fn source_discovery_thresholds_and_noise_are_preserved() {
        for (skill, count, noise, days) in [
            (39, 2, 12, 5),
            (40, 3, 8, 4),
            (59, 3, 8, 4),
            (60, 5, 5, 3),
            (79, 5, 5, 3),
            (80, 6, 2, 2),
        ] {
            assert_eq!(player_assignment_days(skill), days);
            let report = build_report(
                "a",
                &scout("s", "a", skill),
                &player("p"),
                None,
                date(),
                &mut StdRng::seed_from_u64(9),
            );
            let r = report.context.scout_report.unwrap();
            let values = [
                r.pace,
                r.shooting,
                r.passing,
                r.dribbling,
                r.defending,
                r.physical,
            ];
            assert_eq!(values.iter().flatten().count(), count);
            assert!(
                values
                    .iter()
                    .flatten()
                    .all(|v| (50 - noise..=50 + noise).contains(v))
            );
            assert_eq!(r.condition.is_some(), skill >= 60);
            assert_eq!(r.morale.is_some(), skill >= 80);
            assert_eq!(r.potential_key.ends_with("unclear"), skill < 70);
        }
    }

    #[test]
    fn independent_private_reports_delay_and_checkpoint_replay() {
        let mut state = ScoutingState::default();
        let a = scout("sa", "a", 80);
        let b = scout("sb", "b", 80);
        let p = player("p");
        state.assign_player("ma", "a", "aa", &a, &p).unwrap();
        state.assign_player("mb", "b", "ab", &b, &p).unwrap();
        assert!(state.view("outsider").is_none());
        assert!(
            state
                .assign_player("ma", "a", "other", &scout("spare", "a", 80), &p)
                .is_err()
        );
        let staff = BTreeMap::from([(a.id.clone(), a), (b.id.clone(), b)]);
        let mut players = BTreeMap::from([(p.id.clone(), p)]);
        let mut rng = StdRng::seed_from_u64(4);
        state
            .advance_day(date(), &staff, &players, &BTreeMap::new(), &mut rng)
            .unwrap();
        assert!(state.view("ma").unwrap().messages.is_empty());
        let mut restored: ScoutingState =
            serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
        // No reports completed on day one, so no random values were consumed.
        let mut restored_rng = StdRng::seed_from_u64(4);
        assert!(
            state
                .advance_day(date(), &staff, &players, &BTreeMap::new(), &mut rng)
                .is_err()
        );
        players.get_mut("p").unwrap().condition = 17;
        state
            .advance_day(
                date().succ_opt().unwrap(),
                &staff,
                &players,
                &BTreeMap::new(),
                &mut rng,
            )
            .unwrap();
        restored
            .advance_day(
                date().succ_opt().unwrap(),
                &staff,
                &players,
                &BTreeMap::new(),
                &mut restored_rng,
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(&state).unwrap(),
            serde_json::to_value(restored).unwrap()
        );
        for owner in ["ma", "mb"] {
            let desk = state.view(owner).unwrap();
            assert!(desk.assignments.is_empty());
            assert_eq!(desk.messages.len(), 1);
            assert_eq!(
                desk.messages[0]
                    .context
                    .scout_report
                    .as_ref()
                    .unwrap()
                    .condition,
                Some(17)
            );
        }
        assert_ne!(
            state.view("ma").unwrap().messages[0].id,
            state.view("mb").unwrap().messages[0].id
        );
    }

    #[test]
    fn ownership_capacity_and_youth_generator_boundary() {
        let mut state = ScoutingState::default();
        let s = scout("s", "a", 80);
        let p = player("p");
        assert!(state.assign_player("ma", "b", "a", &s, &p).is_err());
        let mut own = p.clone();
        own.team_id = Some("a".into());
        assert!(state.assign_player("ma", "a", "a", &s, &own).is_err());
        state
            .start_youth(
                "ma",
                "a",
                "y",
                &s,
                YouthRegion::Domestic,
                YouthObjective::Balanced,
                Some(Position::Striker),
            )
            .unwrap();
        assert!(state.assign_player("ma", "a", "a", &s, &p).is_err());
        assert!(state.cancel_youth("other", "y").is_err());
        let replacement = scout("r", "a", 20);
        state.reassign_youth("ma", "y", &replacement).unwrap();
        assert_eq!(
            state.view("ma").unwrap().youth_assignments[0].days_remaining,
            4
        );
        let mut rng = StdRng::seed_from_u64(1);
        for offset in 0..5 {
            state
                .advance_day(
                    date() + chrono::Days::new(offset),
                    &BTreeMap::new(),
                    &BTreeMap::new(),
                    &BTreeMap::new(),
                    &mut rng,
                )
                .unwrap();
        }
        assert_eq!(state.pending_youth_generation("ma").len(), 1);
        assert!(state.view("ma").unwrap().messages.is_empty());
        state.cancel_youth("ma", "y").unwrap();
        assert!(
            state
                .start_youth(
                    "ma",
                    "a",
                    "y",
                    &s,
                    YouthRegion::Domestic,
                    YouthObjective::Balanced,
                    None
                )
                .is_err()
        );
        assert_eq!(
            youth_assignment_days(
                80,
                YouthRegion::International,
                YouthObjective::HighPotential
            ),
            6
        );
    }

    #[test]
    fn market_preserves_source_price_listing_age_and_limit() {
        let mut players: Vec<_> = (0..35).map(|i| player(&format!("p{i}"))).collect();
        players[0].team_id = Some("a".into());
        let view = browse_market(
            "a",
            &players,
            &BTreeMap::new(),
            date(),
            &MarketFilter::default(),
        );
        assert_eq!((view.total, view.players.len()), (34, 30));
        assert_eq!(view.players[0].age, "25");
        players[1].loan_listed = true;
        let filter = MarketFilter {
            position: Some("ST".into()),
            max_price: Some(5200),
            listed_only: Some(true),
        };
        assert_eq!(
            browse_market("a", &players, &BTreeMap::new(), date(), &filter).total,
            1
        );
        let filter = MarketFilter {
            max_price: Some(5199),
            ..filter
        };
        assert_eq!(
            browse_market("a", &players, &BTreeMap::new(), date(), &filter).total,
            0
        );
    }

    #[test]
    fn real_generated_pool_is_required_before_source_ranking() {
        assert!(rank_youth_candidates(vec![player("p")], YouthObjective::Balanced).is_err());
        let pool: Vec<_> = (0..6)
            .map(|i| {
                let mut p = player(&format!("p{i}"));
                p.ovr = 60 - i;
                p.potential = 70 + i;
                p
            })
            .collect();
        let potential = rank_youth_candidates(pool.clone(), YouthObjective::HighPotential).unwrap();
        let ready = rank_youth_candidates(pool, YouthObjective::ReadySoon).unwrap();
        assert_eq!(
            potential.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            ["p5", "p4", "p3"]
        );
        assert_eq!(
            ready.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            ["p0", "p1", "p2"]
        );
    }
}
