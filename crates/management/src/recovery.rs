//! Uninjured rest/Recovery-focus branches adapted from OpenFoot Manager's
//! `src-tauri/crates/ofm_core/src/training.rs`.
//! Upstream revision: 64677fee9047a1182005d666bafa5dbc025dca5c.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Callers must exclude injured players. This module does not model injuries,
//! injury rehabilitation, attribute gains, or other training focuses. Recovery
//! multipliers use starting fitness, before the Recovery-focus fitness nudge.

use engine::PlayerData;
use rand::{Rng, RngExt};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryMode {
    Rest,
    Recovery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerRecovery {
    pub age: u32,
    pub morale: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClubRecovery {
    /// Physiotherapy ratings of this club's physio staff only.
    pub physiotherapy: Vec<u8>,
    pub medical_level: u8,
}

/// Validate club settings even when the club currently has no players.
pub fn validate_club(club: &ClubRecovery) -> Result<(), String> {
    if !(1..=10).contains(&club.medical_level)
        || club.physiotherapy.iter().any(|rating| *rating > 100)
    {
        return Err("Recovery medical level must be 1..=10 and physio ratings 0..=100".into());
    }
    Ok(())
}

/// Recover one uninjured player. Invalid inputs change neither the player nor RNG.
/// Rest consumes no randomness; Recovery always draws once, even at full fitness.
pub fn recover(
    player: &mut PlayerData,
    person: &PlayerRecovery,
    club: &ClubRecovery,
    mode: RecoveryMode,
    rng: &mut impl Rng,
) -> Result<(), String> {
    if person.age > 120 || person.morale > 100 {
        return Err("Recovery age must be 0..=120 and morale 0..=100".into());
    }
    validate_club(club)?;
    if player.condition > 100 || player.fitness > 100 || player.stamina > 100 {
        return Err("Recovery condition, fitness and stamina must be 0..=100".into());
    }

    let physio_mult = if club.physiotherapy.is_empty() {
        1.0
    } else {
        let average = club.physiotherapy.iter().map(|v| *v as f64).sum::<f64>()
            / club.physiotherapy.len() as f64;
        1.0 + (average / 100.0) * 0.4
    };
    let medical_mult = 1.0 + f64::from(club.medical_level.saturating_sub(1)) * 0.1;
    let base = match mode {
        RecoveryMode::Rest => 7.0,
        RecoveryMode::Recovery => 9.0,
    } * physio_mult
        * medical_mult;
    let recovery = (base
        * (0.5 + player.stamina as f64 / 100.0 * 0.5)
        * age_factor(person.age)
        * morale_factor(person.morale)
        * condition_factor(player.condition)
        * fitness_factor(player.fitness)) as u8;

    if mode == RecoveryMode::Recovery {
        let roll: f64 = rng.random_range(0.0..1.0);
        if roll < 0.05 && player.fitness < 100 {
            player.fitness = player.fitness.saturating_add(1);
        }
    }
    player.condition = player.condition.saturating_add(recovery).min(100);
    Ok(())
}

fn age_factor(age: u32) -> f64 {
    match age {
        0..=21 => 1.10,
        22..=25 => 1.05,
        26..=29 => 1.00,
        30..=33 => 0.85,
        _ => 0.70,
    }
}

fn morale_factor(morale: u8) -> f64 {
    match morale {
        0..=39 => 0.90,
        40..=69 => 1.00,
        _ => 1.10,
    }
}

fn condition_factor(condition: u8) -> f64 {
    match condition {
        0..=29 => 0.80,
        30..=49 => 0.90,
        _ => 1.00,
    }
}

fn fitness_factor(fitness: u8) -> f64 {
    match fitness {
        0..=29 => 0.75,
        30..=49 => 0.88,
        50..=69 => 1.00,
        70..=89 => 1.12,
        _ => 1.20,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::{PlayerRole, Position};
    use rand::{SeedableRng, rngs::StdRng};

    fn player() -> PlayerData {
        PlayerData {
            id: "p1".into(),
            name: "Player".into(),
            position: Position::Midfielder,
            ovr: 70,
            condition: 50,
            fitness: 50,
            pace: 70,
            stamina: 100,
            strength: 70,
            agility: 70,
            passing: 70,
            shooting: 70,
            tackling: 70,
            dribbling: 70,
            defending: 70,
            positioning: 70,
            vision: 70,
            decisions: 70,
            composure: 70,
            aggression: 70,
            teamwork: 70,
            leadership: 70,
            handling: 50,
            reflexes: 50,
            aerial: 60,
            traits: vec![],
            role: PlayerRole::Standard,
        }
    }

    fn person() -> PlayerRecovery {
        PlayerRecovery {
            age: 26,
            morale: 50,
        }
    }
    fn club() -> ClubRecovery {
        ClubRecovery {
            physiotherapy: vec![],
            medical_level: 1,
        }
    }

    #[test]
    fn reference_multiplier_boundaries() {
        for (input, expected) in [
            (0, 1.10),
            (21, 1.10),
            (22, 1.05),
            (25, 1.05),
            (26, 1.0),
            (29, 1.0),
            (30, 0.85),
            (33, 0.85),
            (34, 0.70),
            (120, 0.70),
        ] {
            assert_eq!(age_factor(input), expected);
        }
        for (input, expected) in [
            (0, 0.90),
            (39, 0.90),
            (40, 1.0),
            (69, 1.0),
            (70, 1.10),
            (100, 1.10),
        ] {
            assert_eq!(morale_factor(input), expected);
        }
        for (input, expected) in [
            (0, 0.80),
            (29, 0.80),
            (30, 0.90),
            (49, 0.90),
            (50, 1.0),
            (100, 1.0),
        ] {
            assert_eq!(condition_factor(input), expected);
        }
        for (input, expected) in [
            (0, 0.75),
            (29, 0.75),
            (30, 0.88),
            (49, 0.88),
            (50, 1.0),
            (69, 1.0),
            (70, 1.12),
            (89, 1.12),
            (90, 1.20),
            (100, 1.20),
        ] {
            assert_eq!(fitness_factor(input), expected);
        }
    }

    #[test]
    fn rest_reference_arithmetic_and_no_rng_draw() {
        let mut p = player();
        let mut rng = StdRng::seed_from_u64(9);
        let mut untouched = StdRng::seed_from_u64(9);
        recover(&mut p, &person(), &club(), RecoveryMode::Rest, &mut rng).unwrap();
        assert_eq!((p.condition, p.fitness), (57, 50));
        assert_eq!(rng.random::<u64>(), untouched.random::<u64>());

        p.condition = 20;
        p.fitness = 20;
        p.stamina = 50;
        // floor(7 * 1.2 * 1.9 * .75 * .7 * .9 * .8 * .75) = 4.
        recover(
            &mut p,
            &PlayerRecovery {
                age: 34,
                morale: 39,
            },
            &ClubRecovery {
                physiotherapy: vec![0, 100],
                medical_level: 10,
            },
            RecoveryMode::Rest,
            &mut rng,
        )
        .unwrap();
        assert_eq!(p.condition, 24);
    }

    #[test]
    fn recovery_uses_starting_fitness_and_exact_single_draw() {
        // Pick a deterministic seed whose first roll triggers the 5% nudge.
        let seed = (0..10000)
            .find(|seed| StdRng::seed_from_u64(*seed).random_range(0.0..1.0) < 0.05)
            .unwrap();
        let mut rng = StdRng::seed_from_u64(seed);
        let mut expected_rng = StdRng::seed_from_u64(seed);
        let _: f64 = expected_rng.random_range(0.0..1.0);
        let mut p = player();
        p.fitness = 29;
        recover(&mut p, &person(), &club(), RecoveryMode::Recovery, &mut rng).unwrap();
        assert_eq!((p.condition, p.fitness), (56, 30)); // floor(9*.75), not floor(9*.88).
        assert_eq!(rng.random::<u64>(), expected_rng.random::<u64>());
        let mut full = player();
        full.condition = 100;
        full.fitness = 100;
        let mut rng = StdRng::seed_from_u64(seed);
        let mut expected_rng = StdRng::seed_from_u64(seed);
        let _: f64 = expected_rng.random_range(0.0..1.0);
        recover(
            &mut full,
            &person(),
            &club(),
            RecoveryMode::Recovery,
            &mut rng,
        )
        .unwrap();
        assert_eq!((full.condition, full.fitness), (100, 100));
        assert_eq!(rng.random::<u64>(), expected_rng.random::<u64>());
    }

    #[test]
    fn invalid_inputs_never_mutate_player_or_rng() {
        for case in 0..8 {
            let mut p = player();
            let mut person = person();
            let mut club = club();
            match case {
                0 => person.age = 121,
                1 => person.morale = 101,
                2 => club.medical_level = 0,
                3 => club.medical_level = 11,
                4 => club.physiotherapy = vec![101],
                5 => p.condition = 101,
                6 => p.fitness = 101,
                _ => p.stamina = 101,
            }
            let before = serde_json::to_value(&p).unwrap();
            let mut rng = StdRng::seed_from_u64(19);
            let mut untouched = StdRng::seed_from_u64(19);
            assert!(recover(&mut p, &person, &club, RecoveryMode::Recovery, &mut rng).is_err());
            assert_eq!(serde_json::to_value(&p).unwrap(), before);
            assert_eq!(rng.random::<u64>(), untouched.random::<u64>());
        }
    }
}
