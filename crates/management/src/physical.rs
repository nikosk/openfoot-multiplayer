//! Post-match wear adapted from OpenFoot Manager's
//! `src-tauri/crates/ofm_core/src/player_wear.rs`.
//! Upstream revision: 64677fee9047a1182005d666bafa5dbc025dca5c.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Formulas are unchanged; callers provide the RNG. Like upstream club
//! post-match processing, this applies wear without a separate injury roll.
//! Persistent club injury duration mapping remains outside this module.

use engine::PlayerData;
use rand::{Rng, RngExt};

/// Apply physical wear and fitness sharpening for a participant.
/// An unused player is unchanged and consumes no random draws.
pub fn apply_match_wear(player: &mut PlayerData, minutes: u8, rng: &mut impl Rng) {
    if minutes == 0 {
        return;
    }
    let minutes_factor = minutes as f64 / 90.0;
    let stamina_factor = player.stamina as f64 / 100.0;
    let base_depletion = 40.0 * (1.0 - stamina_factor * 0.4);
    let depletion = (base_depletion * minutes_factor) as u8;
    player.condition = player.condition.saturating_sub(depletion);

    if minutes >= 60 && player.fitness < 100 && rng.random_bool(0.3) {
        player.fitness = player.fitness.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::{PlayerRole, Position};
    use rand::{SeedableRng, rngs::StdRng};

    fn make_player(stamina: u8) -> PlayerData {
        PlayerData {
            id: "p1".into(),
            name: "John Doe".into(),
            position: Position::Midfielder,
            ovr: 70,
            condition: 100,
            fitness: 75,
            pace: 70,
            stamina,
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

    #[test]
    fn wear_depletes_condition_by_minutes_and_stamina() {
        let mut player = make_player(100);
        let mut rng = StdRng::seed_from_u64(1);
        apply_match_wear(&mut player, 90, &mut rng);
        assert_eq!(player.condition, 76);
        player.condition = 100;
        apply_match_wear(&mut player, 45, &mut rng);
        assert_eq!(player.condition, 88);
        player.condition = 1;
        apply_match_wear(&mut player, 90, &mut rng);
        assert_eq!(player.condition, 0);
    }

    #[test]
    fn unused_player_is_unchanged_and_consumes_no_random_draws() {
        let mut player = make_player(70);
        player.condition = 88;
        player.fitness = 90;
        let before = serde_json::to_value(&player).unwrap();
        let mut rng = StdRng::seed_from_u64(2);
        let mut untouched_rng = StdRng::seed_from_u64(2);
        apply_match_wear(&mut player, 0, &mut rng);
        assert_eq!(serde_json::to_value(&player).unwrap(), before);
        assert_eq!(rng.random::<u64>(), untouched_rng.random::<u64>());
    }

    #[test]
    fn wear_never_lowers_fitness_or_exceeds_100() {
        let mut player = make_player(70);
        player.fitness = 80;
        let mut rng = StdRng::seed_from_u64(3);
        for _ in 0..500 {
            apply_match_wear(&mut player, 90, &mut rng);
            assert!((80..=100).contains(&player.fitness));
        }
        assert_eq!(player.fitness, 100);
    }

    #[test]
    fn short_appearance_does_not_sharpen_fitness_or_consume_random_draws() {
        let mut player = make_player(70);
        player.fitness = 80;
        let mut rng = StdRng::seed_from_u64(4);
        let mut untouched_rng = StdRng::seed_from_u64(4);
        apply_match_wear(&mut player, 59, &mut rng);
        assert_eq!(player.fitness, 80);
        assert_eq!(rng.random::<u64>(), untouched_rng.random::<u64>());
    }
}
