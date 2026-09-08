//! Squad deployment adapted from pinned OpenFoot Manager's `player_rating.rs`,
//! `live_match_manager/team_builder.rs`, `live_match_manager.rs`, and
//! `commands/squad.rs`, revision 64677fee9047a1182005d666bafa5dbc025dca5c.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Natural attributes remain authoritative. Deployed positions and assigned
//! functions affect only engine snapshots. Callers filter unavailable players
//! and authenticate ownership before writing plans. Both external managers use
//! the source saved-XI selection branch; native participants may choose their XI.

use crate::training::{Position, ovr_for_position};
use engine::{LiveMatchState, MatchCommand, PlayerData, PlayerRole, Side};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Footedness {
    Left,
    Right,
    Both,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PositionProfile {
    pub position: Position,
    pub natural_position: Position,
    pub alternate_positions: Vec<Position>,
    pub footedness: Footedness,
    pub weak_foot: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SquadSetup {
    pub profiles: BTreeMap<String, PositionProfile>,
    pub plans: BTreeMap<String, SquadPlan>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchRoles {
    pub captain: Option<String>,
    pub vice_captain: Option<String>,
    pub penalty_taker: Option<String>,
    pub free_kick_taker: Option<String>,
    pub corner_taker: Option<String>,
}

impl MatchRoles {
    pub fn assigned_ids(&self) -> impl Iterator<Item = &String> {
        [
            &self.captain,
            &self.vice_captain,
            &self.penalty_taker,
            &self.free_kick_taker,
            &self.corner_taker,
        ]
        .into_iter()
        .filter_map(Option::as_ref)
    }

    /// Source engine has no vice-captain command. That saved metadata is retained
    /// but is not substituted for the source's automatic captain fallback.
    pub fn apply(&self, state: &mut LiveMatchState, side: Side) -> Result<(), String> {
        if let Some(player_id) = &self.captain {
            state.apply_command(MatchCommand::SetCaptain {
                side,
                player_id: player_id.clone(),
            })?;
        }
        if let Some(player_id) = &self.penalty_taker {
            state.apply_command(MatchCommand::SetPenaltyTaker {
                side,
                player_id: player_id.clone(),
            })?;
        }
        if let Some(player_id) = &self.free_kick_taker {
            state.apply_command(MatchCommand::SetFreeKickTaker {
                side,
                player_id: player_id.clone(),
            })?;
        }
        if let Some(player_id) = &self.corner_taker {
            state.apply_command(MatchCommand::SetCornerTaker {
                side,
                player_id: player_id.clone(),
            })?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SquadPlan {
    pub formation: String,
    pub player_roles: BTreeMap<String, PlayerRole>,
    pub match_roles: MatchRoles,
}
impl Default for SquadPlan {
    fn default() -> Self {
        Self {
            formation: "4-4-2".into(),
            player_roles: BTreeMap::new(),
            match_roles: MatchRoles::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SelectedSquad {
    pub players: Vec<PlayerData>,
    pub bench: Vec<PlayerData>,
    pub match_roles: MatchRoles,
}

/// Source row maps for three- and four-line formations. The wire boundary rejects
/// malformed shapes instead of allocating unbounded or incomplete engine teams.
pub fn formation_slots(formation: &str) -> Result<Vec<Position>, String> {
    use Position::*;
    let parts = formation
        .split('-')
        .map(str::parse::<usize>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Invalid formation")?;
    if !matches!(parts.len(), 3 | 4)
        || parts.iter().any(|n| *n > 10)
        || parts.iter().sum::<usize>() != 10
    {
        return Err("Formation must describe ten outfield slots in three or four lines".into());
    }
    let defenders = |n| match n {
        3 => vec![CenterBack; 3],
        4 => vec![LeftBack, CenterBack, CenterBack, RightBack],
        5 => vec![
            LeftWingBack,
            CenterBack,
            CenterBack,
            CenterBack,
            RightWingBack,
        ],
        _ => vec![CenterBack; n],
    };
    let midfield = |n| match n {
        2 => vec![CentralMidfielder; 2],
        3 => vec![DefensiveMidfielder, CentralMidfielder, AttackingMidfielder],
        4 => vec![
            LeftMidfielder,
            CentralMidfielder,
            CentralMidfielder,
            RightMidfielder,
        ],
        5 => vec![
            LeftMidfielder,
            DefensiveMidfielder,
            CentralMidfielder,
            AttackingMidfielder,
            RightMidfielder,
        ],
        _ => vec![CentralMidfielder; n],
    };
    let deep = |n| match n {
        1 => vec![DefensiveMidfielder],
        2 => vec![DefensiveMidfielder, CentralMidfielder],
        _ => vec![DefensiveMidfielder; n],
    };
    let attacking = |n| match n {
        1 => vec![AttackingMidfielder],
        2 => vec![AttackingMidfielder; 2],
        3 => vec![LeftMidfielder, AttackingMidfielder, RightMidfielder],
        _ => vec![AttackingMidfielder; n],
    };
    let forwards = |n| match n {
        3 => vec![LeftWinger, Striker, RightWinger],
        _ => vec![Striker; n],
    };
    let mut slots = vec![Goalkeeper];
    slots.extend(defenders(parts[0]));
    if parts.len() == 3 {
        slots.extend(midfield(parts[1]));
        slots.extend(forwards(parts[2]));
    } else {
        slots.extend(deep(parts[1]));
        slots.extend(attacking(parts[2]));
        slots.extend(forwards(parts[3]));
    }
    Ok(slots)
}

pub fn role_valid_for_position(role: PlayerRole, position: Position) -> bool {
    use PlayerRole::*;
    use Position::*;
    match position {
        Goalkeeper => matches!(role, Standard | BallPlayingKeeper | SweeperKeeper),
        CenterBack => matches!(role, Standard | Stopper | CoverCB | BallPlayingCB),
        RightBack | LeftBack | RightWingBack | LeftWingBack => matches!(
            role,
            Standard | AttackingFB | DefensiveFB | InvertedFB | WingBack
        ),
        DefensiveMidfielder => {
            matches!(role, Standard | AnchorMan | BallWinner | DeepLyingPlaymaker)
        }
        CentralMidfielder => matches!(role, Standard | BoxToBox | Carrilero | Mezzala),
        AttackingMidfielder => matches!(role, Standard | AdvancedPlaymaker | ShadowStriker),
        RightMidfielder | LeftMidfielder | RightWinger | LeftWinger => matches!(
            role,
            Standard | WideForward | InsideForward | InvertedWinger
        ),
        Striker => matches!(
            role,
            Standard
                | Poacher
                | TargetMan
                | DeepLyingForward
                | False9
                | PressingForward
                | CompleteForward
        ),
        Defender => matches!(
            role,
            Standard
                | Stopper
                | CoverCB
                | BallPlayingCB
                | AttackingFB
                | DefensiveFB
                | InvertedFB
                | WingBack
        ),
        Midfielder => matches!(
            role,
            Standard
                | AnchorMan
                | BallWinner
                | DeepLyingPlaymaker
                | BoxToBox
                | Carrilero
                | Mezzala
                | AdvancedPlaymaker
                | ShadowStriker
                | WideForward
                | InsideForward
                | InvertedWinger
        ),
        Forward => matches!(
            role,
            Standard
                | WideForward
                | InsideForward
                | InvertedWinger
                | Poacher
                | TargetMan
                | DeepLyingForward
                | False9
                | PressingForward
                | CompleteForward
        ),
    }
}

pub fn validate_profiles(profiles: &BTreeMap<String, PositionProfile>) -> Result<(), String> {
    if profiles
        .iter()
        .any(|(id, p)| id.trim().is_empty() || !(1..=5).contains(&p.weak_foot))
    {
        return Err("Invalid position profile or weak foot".into());
    }
    Ok(())
}

pub fn validate_plan(
    plan: &SquadPlan,
    owned_profiles: &BTreeMap<String, PositionProfile>,
    saved_xi: &[String],
) -> Result<(), String> {
    let slots = formation_slots(&plan.formation)?;
    if saved_xi.len() > 11 || saved_xi.iter().collect::<BTreeSet<_>>().len() != saved_xi.len() {
        return Err("Saved XI must have at most eleven unique players".into());
    }
    for (id, role) in &plan.player_roles {
        let profile = owned_profiles
            .get(id)
            .ok_or("Assigned function belongs to an unowned player")?;
        let position = saved_xi
            .iter()
            .position(|p| p == id)
            .and_then(|i| slots.get(i))
            .copied()
            .unwrap_or(profile.natural_position);
        if !role_valid_for_position(*role, position) {
            return Err(format!(
                "Player function is not valid for deployed position: {id}"
            ));
        }
    }
    if plan
        .match_roles
        .assigned_ids()
        .any(|id| !owned_profiles.contains_key(id))
    {
        return Err("Match role belongs to an unowned player".into());
    }
    Ok(())
}

/// Formation/XI edits clear functions no longer valid at their deployed slot;
/// bench assignments are checked against natural position, as upstream.
pub fn reconcile_roles(
    plan: &mut SquadPlan,
    profiles: &BTreeMap<String, PositionProfile>,
    saved_xi: &[String],
) -> Result<(), String> {
    let slots = formation_slots(&plan.formation)?;
    plan.player_roles.retain(|id, role| {
        profiles.get(id).is_some_and(|profile| {
            let position = saved_xi
                .iter()
                .position(|p| p == id)
                .and_then(|i| slots.get(i))
                .copied()
                .unwrap_or(profile.natural_position);
            role_valid_for_position(*role, position)
        })
    });
    Ok(())
}

fn primary(profile: &PositionProfile) -> Position {
    let natural = profile.natural_position;
    if matches!(
        natural,
        Position::Defender | Position::Midfielder | Position::Forward
    ) {
        profile.position.canonical()
    } else {
        natural.canonical()
    }
}

pub fn positional_fit(player: &PlayerData, profile: &PositionProfile, slot: Position) -> f64 {
    let slot = slot.canonical();
    let natural = primary(profile);
    let compatibility = if natural == slot {
        0.0
    } else if profile
        .alternate_positions
        .iter()
        .any(|p| p.canonical() == slot)
    {
        4.0
    } else if natural.group() == slot.group() {
        8.0
    } else {
        14.0
    };
    use Position::*;
    let required = match slot {
        LeftBack | LeftWingBack | LeftMidfielder | LeftWinger => Some(Footedness::Left),
        RightBack | RightWingBack | RightMidfielder | RightWinger => Some(Footedness::Right),
        _ => None,
    };
    let foot = if required.is_none()
        || profile.footedness == Footedness::Both
        || Some(profile.footedness) == required
    {
        0.0
    } else {
        (10 - i32::from(profile.weak_foot.clamp(1, 5)) * 2).max(0) as f64
    };
    (ovr_for_position(player, slot) - compatibility - foot).max(1.0)
}

/// Source formulas use original player position for goalkeeper exclusion and
/// preserve iterator tie behavior (the last tied candidate wins).
pub fn auto_select_set_pieces(
    players: &[PlayerData],
    profiles: &BTreeMap<String, PositionProfile>,
) -> Result<MatchRoles, String> {
    if players.iter().any(|p| !profiles.contains_key(&p.id)) {
        return Err("Missing set-piece position profile".into());
    }
    let captain = players
        .iter()
        .max_by_key(|p| u16::from(p.leadership) + u16::from(p.teamwork))
        .map(|p| p.id.clone());
    let outfield = || {
        players
            .iter()
            .filter(|p| profiles[&p.id].position != Position::Goalkeeper)
    };
    let penalty_taker = outfield()
        .max_by_key(|p| u16::from(p.shooting) + u16::from(p.composure))
        .map(|p| p.id.clone());
    let free_kick_taker = outfield()
        .max_by_key(|p| u16::from(p.passing) + u16::from(p.vision) + u16::from(p.shooting) / 2)
        .map(|p| p.id.clone());
    let corner_taker = outfield()
        .max_by_key(|p| {
            let score = u16::from(p.passing) + u16::from(p.vision);
            if free_kick_taker.as_ref() == Some(&p.id) {
                score.saturating_sub(5)
            } else {
                score
            }
        })
        .map(|p| p.id.clone());
    Ok(MatchRoles {
        captain,
        vice_captain: None,
        penalty_taker,
        free_kick_taker,
        corner_taker,
    })
}

pub fn select(
    available: &[PlayerData],
    profiles: &BTreeMap<String, PositionProfile>,
    saved_xi: &[String],
    plan: &SquadPlan,
) -> Result<SelectedSquad, String> {
    let slots = formation_slots(&plan.formation)?;
    if available.len() < 11 {
        return Err("At least eleven eligible players are required".into());
    }
    let by_id: BTreeMap<_, _> = available.iter().map(|p| (&p.id, p)).collect();
    if by_id.len() != available.len() || available.iter().any(|p| !profiles.contains_key(&p.id)) {
        return Err("Available players need unique IDs and complete position profiles".into());
    }
    let valid_saved = saved_xi
        .iter()
        .filter(|id| by_id.contains_key(id))
        .collect::<BTreeSet<_>>()
        .len();
    let mut chosen = vec![None; 11];
    let mut used = BTreeSet::new();
    if valid_saved >= 8 {
        for (index, entry) in chosen.iter_mut().enumerate() {
            if let Some(player) = saved_xi.get(index).and_then(|id| by_id.get(id)) {
                if used.insert(player.id.clone()) {
                    *entry = Some(*player);
                }
            }
        }
    }
    for (index, entry) in chosen.iter_mut().enumerate() {
        if entry.is_some() {
            continue;
        }
        let best = available
            .iter()
            .filter(|p| !used.contains(&p.id))
            .max_by(|a, b| {
                let fit = |p: &PlayerData| {
                    positional_fit(p, &profiles[&p.id], slots[index]) * f64::from(p.condition)
                        / 100.0
                };
                fit(a)
                    .partial_cmp(&fit(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .ok_or("Insufficient eligible players")?;
        used.insert(best.id.clone());
        *entry = Some(best);
    }
    let selected = chosen
        .into_iter()
        .map(Option::unwrap)
        .cloned()
        .collect::<Vec<_>>();
    let mut resolved = auto_select_set_pieces(&selected, profiles)?;
    for (target, saved) in [
        (&mut resolved.captain, &plan.match_roles.captain),
        (&mut resolved.penalty_taker, &plan.match_roles.penalty_taker),
        (
            &mut resolved.free_kick_taker,
            &plan.match_roles.free_kick_taker,
        ),
        (&mut resolved.corner_taker, &plan.match_roles.corner_taker),
    ] {
        if saved.as_ref().is_some_and(|id| used.contains(id)) {
            *target = saved.clone();
        }
    }
    resolved.vice_captain = plan.match_roles.vice_captain.clone();
    let players = selected
        .into_iter()
        .enumerate()
        .map(|(i, mut p)| {
            p.position = slots[i].group();
            p.role = plan.player_roles.get(&p.id).copied().unwrap_or_default();
            p
        })
        .collect();
    let mut bench = available
        .iter()
        .filter(|p| !used.contains(&p.id))
        .cloned()
        .collect::<Vec<_>>();
    bench.sort_by(|a, b| {
        ovr_for_position(b, primary(&profiles[&b.id]))
            .partial_cmp(&ovr_for_position(a, primary(&profiles[&a.id])))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for p in &mut bench {
        p.position = profiles[&p.id].natural_position.group();
        p.role = plan.player_roles.get(&p.id).copied().unwrap_or_default();
    }
    Ok(SelectedSquad {
        players,
        bench,
        match_roles: resolved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(id: &str, position: Position) -> PlayerData {
        PlayerData {
            id: id.into(),
            name: id.into(),
            position: position.group(),
            role: PlayerRole::Standard,
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
        }
    }

    fn squad() -> (
        Vec<PlayerData>,
        BTreeMap<String, PositionProfile>,
        Vec<String>,
    ) {
        let mut positions = formation_slots("4-4-2").unwrap();
        positions.extend([Position::Goalkeeper, Position::Striker]);
        let players = positions
            .iter()
            .enumerate()
            .map(|(i, p)| player(&format!("p{i}"), *p))
            .collect();
        let profiles = positions
            .into_iter()
            .enumerate()
            .map(|(i, p)| {
                (
                    format!("p{i}"),
                    PositionProfile {
                        position: p,
                        natural_position: p,
                        alternate_positions: vec![],
                        footedness: Footedness::Both,
                        weak_foot: 5,
                    },
                )
            })
            .collect();
        (
            players,
            profiles,
            (0..11).map(|i| format!("p{i}")).collect(),
        )
    }

    #[test]
    fn formation_rows_match_source_granular_maps() {
        use Position::*;
        assert_eq!(
            formation_slots("4-4-2").unwrap(),
            vec![
                Goalkeeper,
                LeftBack,
                CenterBack,
                CenterBack,
                RightBack,
                LeftMidfielder,
                CentralMidfielder,
                CentralMidfielder,
                RightMidfielder,
                Striker,
                Striker
            ]
        );
        assert_eq!(
            formation_slots("4-2-3-1").unwrap(),
            vec![
                Goalkeeper,
                LeftBack,
                CenterBack,
                CenterBack,
                RightBack,
                DefensiveMidfielder,
                CentralMidfielder,
                LeftMidfielder,
                AttackingMidfielder,
                RightMidfielder,
                Striker
            ]
        );
        assert_eq!(
            formation_slots("5-3-2").unwrap(),
            vec![
                Goalkeeper,
                LeftWingBack,
                CenterBack,
                CenterBack,
                CenterBack,
                RightWingBack,
                DefensiveMidfielder,
                CentralMidfielder,
                AttackingMidfielder,
                Striker,
                Striker
            ]
        );
        assert_eq!(
            &formation_slots("4-3-3").unwrap()[8..],
            &[LeftWinger, Striker, RightWinger]
        );
        for invalid in [
            "garbage",
            "4-4",
            "4-4-3",
            "10000000-0-0",
            "4--4-2",
            "4-2-1-1-2",
        ] {
            assert!(formation_slots(invalid).is_err(), "{invalid}");
        }
        assert_eq!(SquadPlan::default().formation, "4-4-2");
    }

    #[test]
    fn positional_fit_uses_exact_compatibility_and_footedness_penalties() {
        let p = player("test", Position::RightBack);
        let mut profile = PositionProfile {
            position: Position::RightBack,
            natural_position: Position::RightBack,
            alternate_positions: vec![],
            footedness: Footedness::Right,
            weak_foot: 1,
        };
        assert_eq!(positional_fit(&p, &profile, Position::RightBack), 65.0);
        assert_eq!(positional_fit(&p, &profile, Position::LeftBack), 49.0); // group -8, wrong foot -8
        profile.alternate_positions.push(Position::LeftBack);
        assert_eq!(positional_fit(&p, &profile, Position::LeftBack), 53.0); // alternate -4
        profile.footedness = Footedness::Both;
        assert_eq!(positional_fit(&p, &profile, Position::LeftBack), 61.0);
        assert_eq!(positional_fit(&p, &profile, Position::Striker), 51.0); // outside group -14
        profile.natural_position = Position::Defender;
        assert_eq!(positional_fit(&p, &profile, Position::RightBack), 65.0); // legacy natural uses current detailed position
    }

    #[test]
    fn saved_slot_holes_are_repaired_without_shifting_starters_or_mutating_natural_data() {
        let (mut players, mut profiles, mut saved) = squad();
        profiles.get_mut("p1").unwrap().natural_position = Position::Striker;
        profiles.get_mut("p1").unwrap().position = Position::Striker;
        players[1].position = engine::Position::Forward;
        players.retain(|p| p.id != "p0");
        saved[0] = "departed-gk".into();
        let mut plan = SquadPlan::default();
        plan.player_roles
            .insert("p1".into(), PlayerRole::DefensiveFB);
        validate_plan(&plan, &profiles, &saved).unwrap();
        let selection = select(&players, &profiles, &saved, &plan).unwrap();
        assert_eq!(selection.players[0].id, "p11");
        assert_eq!(selection.players[1].id, "p1");
        assert_eq!(selection.players[1].position, engine::Position::Defender);
        assert_eq!(selection.players[1].role, PlayerRole::DefensiveFB);
        assert_eq!(
            players.iter().find(|p| p.id == "p1").unwrap().position,
            engine::Position::Forward
        );
        assert_eq!(profiles["p1"].natural_position, Position::Striker);
        assert_eq!(
            selection
                .players
                .iter()
                .filter(|p| p.position == engine::Position::Goalkeeper)
                .count(),
            1
        );
    }

    #[test]
    fn fewer_than_eight_saved_starters_rebuild_and_condition_changes_assignment() {
        let (mut players, profiles, saved) = squad();
        let repaired = select(&players, &profiles, &saved[..7], &SquadPlan::default()).unwrap();
        assert_eq!(repaired.players[0].id, "p11"); // equal source ratings: last candidate wins
        players[11].condition = 1;
        let repaired = select(&players, &profiles, &[], &SquadPlan::default()).unwrap();
        assert_eq!(repaired.players[0].id, "p0");
        assert!(select(&players[..10], &profiles, &[], &SquadPlan::default()).is_err());
    }

    #[test]
    fn function_permissions_follow_deployed_slot_and_reconcile_after_benching() {
        let (_, mut profiles, mut saved) = squad();
        profiles.get_mut("p1").unwrap().natural_position = Position::Striker;
        let mut plan = SquadPlan::default();
        plan.player_roles
            .insert("p1".into(), PlayerRole::DefensiveFB);
        validate_plan(&plan, &profiles, &saved).unwrap();
        saved[1] = "p12".into();
        assert!(validate_plan(&plan, &profiles, &saved).is_err());
        reconcile_roles(&mut plan, &profiles, &saved).unwrap();
        assert!(!plan.player_roles.contains_key("p1"));
        plan.match_roles.captain = Some("opponent-player".into());
        assert!(validate_plan(&plan, &profiles, &saved).is_err());
        assert!(role_valid_for_position(
            PlayerRole::DefensiveFB,
            Position::Defender
        ));
        assert!(!role_valid_for_position(
            PlayerRole::Poacher,
            Position::CentralMidfielder
        ));
        assert!(role_valid_for_position(
            PlayerRole::InsideForward,
            Position::Forward
        ));
        assert!(role_valid_for_position(
            PlayerRole::InsideForward,
            Position::Midfielder
        ));
        assert!(!role_valid_for_position(
            PlayerRole::BoxToBox,
            Position::DefensiveMidfielder
        ));
    }

    #[test]
    fn set_piece_auto_fallback_and_manual_assignments_reach_both_engine_sides() {
        let (mut players, profiles, saved) = squad();
        players[0].leadership = 100;
        players[0].teamwork = 100;
        players[0].shooting = 100;
        let auto = auto_select_set_pieces(&players[..11], &profiles).unwrap();
        assert_eq!(auto.captain.as_deref(), Some("p0"));
        assert_eq!(auto.penalty_taker.as_deref(), Some("p10"));
        assert_eq!(auto.free_kick_taker.as_deref(), Some("p10"));
        assert_eq!(auto.corner_taker.as_deref(), Some("p9"));
        let mut plan = SquadPlan::default();
        plan.match_roles.penalty_taker = Some("p1".into());
        plan.match_roles.free_kick_taker = Some("p12".into()); // benched assignment falls back
        plan.match_roles.vice_captain = Some("p2".into());
        let home = select(&players, &profiles, &saved, &plan).unwrap();
        assert_eq!(home.match_roles.penalty_taker.as_deref(), Some("p1"));
        assert_eq!(home.match_roles.free_kick_taker, auto.free_kick_taker);
        assert_eq!(home.match_roles.captain, auto.captain); // vice is not invented fallback
        let mut away = home.clone();
        for p in away.players.iter_mut().chain(away.bench.iter_mut()) {
            p.id = format!("away-{}", p.id);
        }
        let away_roles = MatchRoles {
            captain: Some("away-p3".into()),
            penalty_taker: Some("away-p9".into()),
            free_kick_taker: Some("away-p8".into()),
            corner_taker: Some("away-p7".into()),
            vice_captain: Some("away-p2".into()),
        };
        let team = |id: &str, players| engine::TeamData {
            id: id.into(),
            name: id.into(),
            formation: "4-4-2".into(),
            play_style: engine::PlayStyle::Balanced,
            tactics: Default::default(),
            players,
        };
        let mut state = LiveMatchState::new(
            team("home", home.players),
            team("away", away.players),
            Default::default(),
            home.bench,
            away.bench,
            false,
        );
        home.match_roles.apply(&mut state, Side::Home).unwrap();
        away_roles.apply(&mut state, Side::Away).unwrap();
        let snapshot = state.snapshot();
        assert_eq!(
            snapshot.home_set_pieces.penalty_taker.as_deref(),
            Some("p1")
        );
        assert_eq!(
            snapshot.away_set_pieces.penalty_taker.as_deref(),
            Some("away-p9")
        );
        assert_eq!(snapshot.away_set_pieces.captain.as_deref(), Some("away-p3"));
        assert_eq!(snapshot.home_set_pieces.corner_taker, auto.corner_taker);
    }

    #[test]
    fn plan_roundtrip_preserves_all_assignments_and_rejects_unknown_enums() {
        let mut plan = SquadPlan::default();
        plan.formation = "4-2-3-1".into();
        plan.player_roles
            .insert("p10".into(), PlayerRole::CompleteForward);
        plan.match_roles.vice_captain = Some("p1".into());
        let value = serde_json::to_value(&plan).unwrap();
        assert_eq!(
            serde_json::from_value::<SquadPlan>(value.clone()).unwrap(),
            plan
        );
        let mut invalid = value;
        invalid["player_roles"]["p10"] = "InventedRole".into();
        assert!(serde_json::from_value::<SquadPlan>(invalid).is_err());
    }
}
