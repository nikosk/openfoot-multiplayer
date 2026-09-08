//! Injury and card-stat arithmetic adapted from OpenFoot Manager:
//! `ofm_core/src/{player_wear.rs,random_events/mod.rs,turn/mod.rs,turn/post_match.rs,
//! end_of_season.rs,live_match_manager/team_builder.rs}` and `domain/src/player.rs`.
//! Upstream revision: 64677fee9047a1182005d666bafa5dbc025dca5c.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Source boundaries matter here:
//! - The pinned source has no persistent card suspension, accumulation threshold,
//!   medical treatment action, or emergency override of injury eligibility.
//!   Cards persist as season statistics; only an injury blocks team selection.
//! - Club post-match processing calls physical wear but NOT `roll_match_injury`.
//!   That helper is called for the selected national-team XI after 90-minute wear.
//! - The training-ground injury event is a daily random-event branch, independent
//!   of training intensity, schedule, medical facilities, or training completion.
//!   It is scoped to the source user club; the caller supplies the intended club.
//! - Source day order is match/training, injury recovery, then random events. New
//!   random-event injuries therefore retain their full duration on the first day.
//!   The random-event guard checks remaining *scheduled* league fixtures, so a
//!   completed match does not itself block that event despite its source comment.
//!
//! The caller owns once-only day/report/event publication, source candidate order,
//! and RNG stream selection. These pure helpers do not introduce a new ban or
//! treatment rule or claim equivalence to upstream's global thread-RNG stream.

use rand::{Rng, RngExt};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const INJURY_NAMES: [&str; 5] = [
    "common.injuries.minorMuscleStrain",
    "common.injuries.twistedAnkle",
    "common.injuries.kneeBruise",
    "common.injuries.hamstringTightness",
    "common.injuries.calfStrain",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Injury {
    pub name: String,
    pub days_remaining: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Availability {
    pub injury: Option<Injury>,
    /// Source PlayerSeasonStats counters, not suspension points.
    pub yellow_cards: u32,
    pub red_cards: u32,
}

impl Availability {
    pub fn validate(&self) -> Result<(), String> {
        if self
            .injury
            .as_ref()
            .is_some_and(|injury| injury.name.trim().is_empty())
        {
            return Err("Injury name must be nonblank".into());
        }
        // Zero-duration injuries are representable in source; recovery clears them.
        Ok(())
    }

    /// Exact source team-builder predicate: neither fatigue nor cards imposes an
    /// additional exclusion here. The engine handles dismissals within a match.
    pub fn is_available(&self) -> bool {
        self.injury.is_none()
    }

    /// One daily recovery tick. Returns true only when an existing injury clears.
    /// No condition, fitness, or medical multiplier changes this duration rule.
    pub fn progress_recovery(&mut self) -> bool {
        let Some(mut injury) = self.injury.take() else {
            return false;
        };
        if injury.days_remaining > 1 {
            injury.days_remaining -= 1;
            self.injury = Some(injury);
            false
        } else {
            true
        }
    }

    /// Apply once for each report entry, as in source apply_player_stats. Both
    /// counters update atomically; overflowing malformed histories are rejected.
    pub fn add_match_cards(&mut self, yellow_cards: u8, red_cards: u8) -> Result<(), String> {
        let yellows = self
            .yellow_cards
            .checked_add(yellow_cards.into())
            .ok_or("Yellow-card counter overflow")?;
        let reds = self
            .red_cards
            .checked_add(red_cards.into())
            .ok_or("Red-card counter overflow")?;
        self.yellow_cards = yellows;
        self.red_cards = reds;
        Ok(())
    }

    /// Source season-stat reset clears cards but does not heal existing injuries.
    pub fn reset_season_cards(&mut self) {
        self.yellow_cards = 0;
        self.red_cards = 0;
    }
}

fn fitness_multiplier(fitness: u8) -> f64 {
    if fitness < 30 {
        3.0
    } else if fitness < 50 {
        2.0
    } else if fitness < 70 {
        1.5
    } else if fitness >= 90 {
        0.7
    } else {
        1.0
    }
}

/// The source helper used by national-team match effects. Use post-wear fitness.
/// Do not call for club matches or unused substitutes to add a new source rule.
pub fn roll_match_injury(
    availability: &mut Availability,
    fitness: u8,
    rng: &mut impl Rng,
) -> Result<bool, String> {
    if fitness > 100 {
        return Err("Injury fitness must be 0..=100".into());
    }
    if availability.injury.is_some() {
        return Ok(false);
    }
    let probability = (1.0_f64 / 40.0 * fitness_multiplier(fitness)).min(1.0);
    if !rng.random_bool(probability) {
        return Ok(false);
    }
    let days_remaining = rng.random_range(5..=21);
    let name = INJURY_NAMES[rng.random_range(0..INJURY_NAMES.len())];
    availability.injury = Some(Injury {
        name: name.into(),
        days_remaining,
    });
    Ok(true)
}

pub struct InjuryCandidate<'a> {
    pub player_id: &'a str,
    pub fitness: u8,
    pub availability: &'a Availability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InjuryEvent {
    /// Same event identity as source training-injury inbox messages.
    pub id: String,
    pub player_id: String,
    pub injury: Injury,
}

/// Pick ONE uniformly random uninjured player from the supplied club, then roll
/// 1/50 times that player's fitness multiplier. Selection itself is not weighted.
/// The caller applies the returned injury and publishes its event atomically.
/// `date` is the source YYYY-MM-DD identity; the dated host validates it.
/// Existing event IDs are checked after the probability roll, matching upstream.
pub fn roll_training_ground_injury(
    candidates: &[InjuryCandidate<'_>],
    has_scheduled_match: bool,
    date: &str,
    existing_event_ids: &BTreeSet<String>,
    rng: &mut impl Rng,
) -> Result<Option<InjuryEvent>, String> {
    if has_scheduled_match {
        return Ok(None);
    }
    let mut seen = BTreeSet::new();
    for candidate in candidates {
        if candidate.player_id.trim().is_empty()
            || !seen.insert(candidate.player_id)
            || candidate.fitness > 100
        {
            return Err(
                "Training injury candidates require distinct nonblank IDs and fitness 0..=100"
                    .into(),
            );
        }
    }
    let eligible: Vec<_> = candidates
        .iter()
        .filter(|candidate| candidate.availability.is_available())
        .collect();
    if eligible.is_empty() {
        return Ok(None);
    }
    let player = eligible[rng.random_range(0..eligible.len())];
    let probability = (1.0_f64 / 50.0 * fitness_multiplier(player.fitness)).min(1.0);
    if !rng.random_bool(probability) {
        return Ok(None);
    }
    let id = format!("training_injury_{}_{}", player.player_id, date);
    if existing_event_ids.contains(&id) {
        return Ok(None);
    }
    let days_remaining = rng.random_range(3..=14);
    let name = INJURY_NAMES[rng.random_range(0..INJURY_NAMES.len())];
    Ok(Some(InjuryEvent {
        id,
        player_id: player.player_id.into(),
        injury: Injury {
            name: name.into(),
            days_remaining,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};

    #[test]
    fn recovery_keeps_source_duration_boundaries_and_does_not_clear_cards() {
        for (days, remaining) in [
            (0, None),
            (1, None),
            (2, Some(1)),
            (u32::MAX, Some(u32::MAX - 1)),
        ] {
            let mut state = Availability {
                injury: Some(Injury {
                    name: "existing".into(),
                    days_remaining: days,
                }),
                yellow_cards: 5,
                red_cards: 1,
            };
            assert!(!state.is_available());
            assert_eq!(state.progress_recovery(), remaining.is_none());
            assert_eq!(state.injury.map(|injury| injury.days_remaining), remaining);
            assert_eq!((state.yellow_cards, state.red_cards), (5, 1));
        }
        assert!(!Availability::default().progress_recovery());
    }

    #[test]
    fn cards_remain_stats_not_invented_suspensions_and_reset_preserves_injury() {
        let mut state = Availability::default();
        state.add_match_cards(5, 1).unwrap();
        assert!(state.is_available());
        assert_eq!((state.yellow_cards, state.red_cards), (5, 1));
        state.injury = Some(Injury {
            name: "existing".into(),
            days_remaining: 3,
        });
        state.reset_season_cards();
        assert_eq!((state.yellow_cards, state.red_cards), (0, 0));
        assert_eq!(state.injury.as_ref().unwrap().days_remaining, 3);
        assert!(!state.is_available());
        state.red_cards = u32::MAX;
        let before = state.clone();
        assert!(state.add_match_cards(1, 1).is_err());
        assert_eq!(state, before);
    }

    #[test]
    fn fitness_thresholds_are_exact() {
        for (fitness, multiplier) in [
            (0, 3.0),
            (29, 3.0),
            (30, 2.0),
            (49, 2.0),
            (50, 1.5),
            (69, 1.5),
            (70, 1.0),
            (89, 1.0),
            (90, 0.7),
            (100, 0.7),
        ] {
            assert_eq!(fitness_multiplier(fitness), multiplier);
        }
    }

    #[test]
    fn match_helper_matches_source_seeded_draws_and_skips_existing_injury() {
        let mut hits = 0;
        for seed in 0..2048 {
            let mut rng = StdRng::seed_from_u64(seed);
            let mut reference = StdRng::seed_from_u64(seed);
            let hit = reference.random_bool((1.0_f64 / 40.0 * 3.0).min(1.0));
            let expected = hit.then(|| {
                let days = reference.random_range(5..=21);
                let name = INJURY_NAMES[reference.random_range(0..5)];
                Injury {
                    name: name.into(),
                    days_remaining: days,
                }
            });
            let mut state = Availability::default();
            assert_eq!(roll_match_injury(&mut state, 20, &mut rng).unwrap(), hit);
            assert_eq!(state.injury, expected);
            assert_eq!(rng.random::<u64>(), reference.random::<u64>());
            hits += usize::from(hit);
            if hit {
                let before = state.clone();
                let mut untouched = StdRng::seed_from_u64(seed);
                let mut skip_rng = StdRng::seed_from_u64(seed);
                assert!(!roll_match_injury(&mut state, 20, &mut skip_rng).unwrap());
                assert_eq!(state, before);
                assert_eq!(skip_rng.random::<u64>(), untouched.random::<u64>());
            }
        }
        assert!(hits > 0);
    }

    #[test]
    fn training_event_selects_one_eligible_player_and_deduplicates_after_risk_draw() {
        let healthy = Availability::default();
        let injured = Availability {
            injury: Some(Injury {
                name: "existing".into(),
                days_remaining: 4,
            }),
            ..Availability::default()
        };
        let candidates = [
            InjuryCandidate {
                player_id: "injured",
                fitness: 0,
                availability: &injured,
            },
            InjuryCandidate {
                player_id: "a",
                fitness: 20,
                availability: &healthy,
            },
            InjuryCandidate {
                player_id: "b",
                fitness: 100,
                availability: &healthy,
            },
        ];
        let mut hits = 0;
        for seed in 0..2048 {
            let mut rng = StdRng::seed_from_u64(seed);
            let mut reference = StdRng::seed_from_u64(seed);
            let chosen = reference.random_range(0..2_usize);
            let multiplier = if chosen == 0 { 3.0 } else { 0.7 };
            let hit = reference.random_bool((1.0_f64 / 50.0 * multiplier).min(1.0));
            let event = roll_training_ground_injury(
                &candidates,
                false,
                "2026-06-01",
                &BTreeSet::new(),
                &mut rng,
            )
            .unwrap();
            assert_eq!(event.is_some(), hit);
            if let Some(event) = event {
                hits += 1;
                assert_eq!(event.player_id, if chosen == 0 { "a" } else { "b" });
                let mut dedup_rng = StdRng::seed_from_u64(seed);
                assert!(
                    roll_training_ground_injury(
                        &candidates,
                        false,
                        "2026-06-01",
                        &BTreeSet::from([event.id.clone()]),
                        &mut dedup_rng
                    )
                    .unwrap()
                    .is_none()
                );
                let mut after_probability = StdRng::seed_from_u64(seed);
                let _ = after_probability.random_range(0..2_usize);
                let _ = after_probability.random_bool((1.0_f64 / 50.0 * multiplier).min(1.0));
                assert_eq!(dedup_rng.random::<u64>(), after_probability.random::<u64>());
                assert_eq!(event.injury.days_remaining, reference.random_range(3..=14));
                assert_eq!(
                    event.injury.name,
                    INJURY_NAMES[reference.random_range(0..5)]
                );
            }
            assert_eq!(rng.random::<u64>(), reference.random::<u64>());
        }
        assert!(hits > 0);
    }

    #[test]
    fn scheduled_match_empty_candidates_and_invalid_inputs_consume_no_rng() {
        let healthy = Availability::default();
        let candidates = [InjuryCandidate {
            player_id: "a",
            fitness: 70,
            availability: &healthy,
        }];
        let mut rng = StdRng::seed_from_u64(42);
        let mut untouched = StdRng::seed_from_u64(42);
        assert!(
            roll_training_ground_injury(
                &candidates,
                true,
                "2026-06-01",
                &BTreeSet::new(),
                &mut rng
            )
            .unwrap()
            .is_none()
        );
        assert!(
            roll_training_ground_injury(&[], false, "2026-06-01", &BTreeSet::new(), &mut rng)
                .unwrap()
                .is_none()
        );
        assert!(roll_match_injury(&mut Availability::default(), 101, &mut rng).is_err());
        assert_eq!(rng.random::<u64>(), untouched.random::<u64>());
    }
}
