//! Explicit new calendars, not a reconstruction of source-world fixtures or outcomes.
use crate::football::Fixture;
use rand::{RngExt, SeedableRng, rngs::StdRng};

/// Construct a double round robin in canonical club-ID order using the circle method.
/// Every return fixture reverses its first-leg venue; an odd field has one bye each
/// round. `first_day` is inclusive and spacing is between rounds, including legs.
///
/// At most 128 clubs are accepted to bound allocation (16,256 fixtures). IDs are
/// unique within this generated calendar; callers merging calendars must provide
/// their own namespace. Club existence remains the fixture registry's concern.
/// The seed controls match seeds, not pairings, and is reproducible with the pinned
/// RNG implementation; dependency upgrades may change the generated match seeds.
pub fn double_round_robin(
    club_ids: &[String],
    first_day: u32,
    spacing_days: u32,
    seed: u64,
) -> Result<Vec<Fixture>, String> {
    if !(2..=128).contains(&club_ids.len()) {
        return Err("calendar requires between 2 and 128 clubs".into());
    }
    if spacing_days == 0 {
        return Err("calendar spacing must be positive".into());
    }
    let mut clubs: Vec<_> = club_ids.iter().collect();
    clubs.sort_unstable();
    if clubs.iter().any(|id| id.trim().is_empty())
        || clubs.windows(2).any(|pair| pair[0] == pair[1])
    {
        return Err("calendar club IDs must be nonblank and distinct".into());
    }
    let mut circle: Vec<_> = clubs.into_iter().map(Some).collect();
    if circle.len() % 2 != 0 {
        circle.push(None);
    }
    let rounds_per_leg = circle.len() - 1;
    let total_rounds = 2 * rounds_per_leg;
    let day_span = spacing_days
        .checked_mul((total_rounds - 1) as u32)
        .ok_or("calendar day overflow")?;
    first_day
        .checked_add(day_span)
        .ok_or("calendar day overflow")?;

    let mut fixtures = Vec::with_capacity(club_ids.len() * (club_ids.len() - 1));
    let mut rng = StdRng::seed_from_u64(seed);
    for leg in 0..2 {
        // The rotating portion returns to its initial ordering after a full leg.
        for round in 0..rounds_per_leg {
            let day = first_day + spacing_days * (leg * rounds_per_leg + round) as u32;
            for pair in 0..circle.len() / 2 {
                let (Some(a), Some(b)) = (circle[pair], circle[circle.len() - 1 - pair]) else {
                    continue;
                };
                let (home, away) = if (round + leg) % 2 == 0 {
                    (a, b)
                } else {
                    (b, a)
                };
                fixtures.push(Fixture {
                    id: format!("round-robin-{:05}", fixtures.len() + 1),
                    day,
                    home: home.clone(),
                    away: away.clone(),
                    seed: rng.random(),
                });
            }
            circle[1..].rotate_right(1);
        }
    }
    Ok(fixtures)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    fn ids(count: usize) -> Vec<String> {
        (0..count).map(|i| format!("club-{i}")).collect()
    }

    fn assert_calendar(count: usize) {
        let clubs = ids(count);
        let fixtures = double_round_robin(&clubs, 3, 7, 42).unwrap();
        assert_eq!(fixtures.len(), count * (count - 1));
        let mut fixture_ids = BTreeSet::new();
        let mut pairs = BTreeSet::new();
        let mut days: BTreeMap<u32, BTreeSet<&str>> = BTreeMap::new();
        let mut home_counts = BTreeMap::new();
        let mut away_counts = BTreeMap::new();
        for fixture in &fixtures {
            assert!(fixture_ids.insert(&fixture.id));
            assert_ne!(fixture.home, fixture.away);
            assert!(pairs.insert((&fixture.home, &fixture.away)));
            assert_eq!((fixture.day - 3) % 7, 0);
            let playing = days.entry(fixture.day).or_default();
            assert!(playing.insert(&fixture.home));
            assert!(playing.insert(&fixture.away));
            *home_counts.entry(&fixture.home).or_insert(0) += 1;
            *away_counts.entry(&fixture.away).or_insert(0) += 1;
        }
        let rounds_per_leg = if count % 2 == 0 { count - 1 } else { count };
        assert_eq!(days.len(), 2 * rounds_per_leg);
        let mut byes: BTreeMap<&String, usize> = BTreeMap::new();
        for (round, (day, playing)) in days.iter().enumerate() {
            assert_eq!(*day, 3 + 7 * round as u32);
            assert_eq!(playing.len(), count - count % 2);
            for club in &clubs {
                if !playing.contains(club.as_str()) {
                    *byes.entry(club).or_default() += 1;
                }
            }
        }
        for home in &clubs {
            assert_eq!(home_counts[home], count - 1);
            assert_eq!(away_counts[home], count - 1);
            if count % 2 != 0 {
                assert_eq!(byes[home], 2);
            }
            for away in &clubs {
                if home != away {
                    assert!(pairs.contains(&(home, away)));
                }
            }
        }
    }

    #[test]
    fn even_and_odd_fields_cover_every_ordered_pair_and_balance_venues() {
        for count in [2, 3, 4, 5, 8, 127, 128] {
            assert_calendar(count);
        }
    }

    #[test]
    fn input_permutations_and_repeated_seeds_are_deterministic() {
        let clubs = ids(5);
        let expected =
            serde_json::to_value(double_round_robin(&clubs, 0, 1, u64::MAX).unwrap()).unwrap();
        let mut permuted = clubs.clone();
        for _ in 0..clubs.len() {
            permuted.rotate_left(1);
            assert_eq!(
                expected,
                serde_json::to_value(double_round_robin(&permuted, 0, 1, u64::MAX).unwrap())
                    .unwrap()
            );
        }
        permuted.reverse();
        assert_eq!(
            expected,
            serde_json::to_value(double_round_robin(&permuted, 0, 1, u64::MAX).unwrap()).unwrap()
        );
        let changed = double_round_robin(&clubs, 0, 1, 1).unwrap();
        assert_ne!(expected[0]["seed"], changed[0].seed);
    }

    #[test]
    fn rejects_invalid_fields_spacing_and_overflow() {
        for clubs in [
            vec![],
            ids(1),
            ids(129),
            vec!["x".into(), "x".into()],
            vec!["x".into(), " \t".into()],
        ] {
            assert!(double_round_robin(&clubs, 0, 1, 0).is_err());
        }
        assert!(double_round_robin(&ids(2), 0, 0, 0).is_err());
        assert!(double_round_robin(&ids(2), u32::MAX, 1, 0).is_err());
        assert!(double_round_robin(&ids(3), 0, u32::MAX, 0).is_err());
        let boundary = double_round_robin(&ids(2), u32::MAX - 1, 1, 0).unwrap();
        assert_eq!(boundary.last().unwrap().day, u32::MAX);
    }
}
