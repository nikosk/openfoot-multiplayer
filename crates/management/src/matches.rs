//! Fully delegated league fixtures. Both clubs use the same engine and match AI.
//!
//! This is match execution only: the caller owns fixtures, eligibility, persistent
//! player consequences and standings. There is no external live-match control.

use std::collections::HashSet;

use engine::{LiveMatchState, MatchConfig, MatchReport, PlayerData, Side, TeamData};
use rand::{SeedableRng, rngs::StdRng};

/// An already selected match-day squad and its delegated manager profile.
#[derive(Clone, Debug)]
pub struct DelegatedTeam {
    pub team: TeamData,
    pub bench: Vec<PlayerData>,
    pub profile: engine::ai::AiProfile,
}

/// Play one regulation-time fixture, including stoppage time, without extra time.
///
/// A seed reproduces football results for this pinned engine/RNG implementation;
/// it is not a promise of stability across dependency upgrades. AI command order
/// is explicitly home then away after each engine step, as in the upstream tests.
pub fn play(home: DelegatedTeam, away: DelegatedTeam, seed: u64) -> Result<MatchReport, String> {
    let mut ids = HashSet::new();
    validate(&home, &mut ids)?;
    validate(&away, &mut ids)?;
    let mut rng = StdRng::seed_from_u64(seed);
    let mut state = LiveMatchState::new(
        home.team,
        away.team,
        MatchConfig::default(),
        home.bench,
        away.bench,
        false,
    );
    for _ in 0..500 {
        state.step_minute(&mut rng);
        for (side, profile) in [(Side::Home, &home.profile), (Side::Away, &away.profile)] {
            for command in engine::ai::ai_decide(&state, side, profile, &mut rng) {
                // Upstream's delegated loop also treats rejected AI proposals as
                // no-ops. The engine remains the authority on valid substitutions.
                let _ = state.apply_command(command);
            }
        }
        if state.is_finished() {
            return Ok(state.into_report());
        }
    }
    Err("delegated fixture did not finish within 500 engine steps".into())
}

fn validate(squad: &DelegatedTeam, ids: &mut HashSet<String>) -> Result<(), String> {
    if squad.team.players.len() != 11 {
        return Err("a starting lineup must contain exactly 11 players".into());
    }
    if squad.bench.len() > 12 {
        return Err("a match-day bench must contain at most 12 players".into());
    }
    if squad.team.id.trim().is_empty() || !ids.insert(squad.team.id.clone()) {
        return Err("club and player IDs must be nonempty and globally distinct".into());
    }
    if squad.profile.experience > 100 || squad.profile.reputation > 1000 {
        return Err("delegated manager profile is out of range".into());
    }
    for p in squad.team.players.iter().chain(&squad.bench) {
        if p.id.trim().is_empty() || !ids.insert(p.id.clone()) {
            return Err("club and player IDs must be nonempty and globally distinct".into());
        }
        // All numeric player inputs are integers; no NaN/Infinity can enter.
        if [
            p.ovr,
            p.condition,
            p.fitness,
            p.pace,
            p.stamina,
            p.strength,
            p.agility,
            p.passing,
            p.shooting,
            p.tackling,
            p.dribbling,
            p.defending,
            p.positioning,
            p.vision,
            p.decisions,
            p.composure,
            p.aggression,
            p.teamwork,
            p.leadership,
            p.handling,
            p.reflexes,
            p.aerial,
        ]
        .into_iter()
        .any(|value| value > 100)
        {
            return Err(format!("player {} has an attribute outside 0..=100", p.id));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::{PlayStyle, PlayerRole, Position, TacticsConfig};

    fn player(id: String, position: Position) -> PlayerData {
        PlayerData {
            id: id.clone(),
            name: id,
            position,
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
            traits: vec![],
            role: PlayerRole::Standard,
        }
    }

    fn team(id: &str) -> DelegatedTeam {
        let position = |i| match i {
            0 => Position::Goalkeeper,
            1..=4 => Position::Defender,
            5..=8 => Position::Midfielder,
            _ => Position::Forward,
        };
        DelegatedTeam {
            team: TeamData {
                id: id.into(),
                name: id.into(),
                formation: "4-4-2".into(),
                play_style: PlayStyle::Balanced,
                tactics: TacticsConfig::default(),
                players: (0..11)
                    .map(|i| player(format!("{id}-p{i}"), position(i)))
                    .collect(),
            },
            bench: (0..7)
                .map(|i| player(format!("{id}-b{i}"), position(i)))
                .collect(),
            profile: engine::ai::AiProfile::default(),
        }
    }

    #[test]
    fn complete_match_is_reproducible() {
        let home = team("home");
        let away = team("away");
        let first = play(home.clone(), away.clone(), 1001).unwrap();
        let second = play(home, away, 1001).unwrap();
        // JSON values compare maps independent of randomized HashMap iteration.
        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::to_value(second).unwrap()
        );
        assert!(first.total_minutes >= 90);
        assert!(first.home_penalties.is_none());
        assert!(first.away_penalties.is_none());
        assert!(!first.events.is_empty());
    }

    #[test]
    fn rejects_empty_or_oversized_rosters() {
        let mut home = team("home");
        home.team.players.clear();
        assert!(play(home, team("away"), 1).is_err());
        let mut home = team("home");
        home.bench = (0..13)
            .map(|i| player(format!("reserve{i}"), Position::Forward))
            .collect();
        assert!(play(home, team("away"), 1).is_err());
    }

    #[test]
    fn rejects_duplicate_and_empty_ids_across_squads() {
        assert!(play(team("same"), team("same"), 1).is_err());
        let home = team("home");
        let mut away = team("away");
        away.bench[0].id = home.team.players[0].id.clone();
        assert!(play(home, away, 1).is_err());
        let mut home = team("home");
        home.team.players[0].id = " ".into();
        assert!(play(home, team("away"), 1).is_err());
    }

    #[test]
    fn rejects_out_of_range_attributes() {
        let mut home = team("home");
        home.bench[0].fitness = 101;
        assert!(play(home, team("away"), 1).is_err());
    }
}
