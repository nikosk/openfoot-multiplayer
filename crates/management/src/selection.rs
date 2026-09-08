//! Deterministic repair of a manager's requested XI, not a strategy override.
//!
//! Inspired by OpenFoot's `ofm_core/live_match_manager/team_builder.rs`
//! (`select_starting_xi` / `auto_select_starting_xi`), revision
//! 64677fee9047a1182005d666bafa5dbc025dca5c.
//! OpenFoot: Copyright (C) 2020–2026 Pedrenrique G. Guimarães, GPL-3.0-or-later.
//!
//! Deviations: retain every available requested starter even when fewer than
//! eight remain; fill broad 4-4-2 position-group deficits rather than detailed
//! formation slots, which the current engine snapshots do not represent. Rank
//! replacements by natural position, then OVR (attribute mean if OVR is absent),
//! then ID. No condition-based rotation or tactical reassignment is performed.

use engine::{PlayerData, Position};
use std::collections::{BTreeMap, HashSet};

/// Select eleven starters and at most twelve substitutes from eligible players.
///
/// The caller must filter injuries and ownership. Available preferred starters
/// retain their relative order and are never displaced by higher-rated players.
/// Replacements are appended; this API does not encode individual pitch slots.
/// Duplicate available IDs are rejected rather than selecting ambiguous records.
pub fn select(
    available: &[PlayerData],
    preferred: &[String],
) -> Result<(Vec<PlayerData>, Vec<PlayerData>), String> {
    let mut by_id = BTreeMap::new();
    for player in available {
        if by_id.insert(player.id.as_str(), player).is_some() {
            return Err(format!("duplicate available player ID: {}", player.id));
        }
    }
    if by_id.len() < 11 {
        return Err("at least 11 eligible players are required for a match".into());
    }

    let mut used = HashSet::new();
    let mut xi = Vec::with_capacity(11);
    for id in preferred {
        if xi.len() == 11 {
            break;
        }
        if let Some(player) = by_id.get(id.as_str())
            && used.insert(player.id.as_str())
        {
            xi.push((*player).clone());
        }
    }

    for (position, target) in [
        (Position::Goalkeeper, 1),
        (Position::Defender, 4),
        (Position::Midfielder, 4),
        (Position::Forward, 2),
    ] {
        let selected = xi.iter().filter(|p| p.position == position).count();
        for _ in selected..target {
            if xi.len() == 11 {
                break;
            }
            // Choose a natural-position candidate before considering an
            // out-of-position fallback. ID ties are independent of input order.
            let player = by_id
                .values()
                .copied()
                .filter(|p| !used.contains(p.id.as_str()))
                .max_by(|a, b| {
                    (a.position == position)
                        .cmp(&(b.position == position))
                        .then_with(|| quality(a).total_cmp(&quality(b)))
                        .then_with(|| b.id.cmp(&a.id))
                })
                .expect("eleven distinct eligible players were checked above");
            used.insert(player.id.as_str());
            xi.push(player.clone());
        }
    }

    let mut remaining: Vec<_> = by_id
        .values()
        .copied()
        .filter(|p| !used.contains(p.id.as_str()))
        .collect();
    remaining.sort_by(|a, b| {
        quality(b)
            .total_cmp(&quality(a))
            .then_with(|| a.id.cmp(&b.id))
    });
    let bench = remaining.into_iter().take(12).cloned().collect();
    Ok((xi, bench))
}

fn quality(player: &PlayerData) -> f64 {
    if player.ovr == 0 {
        player.overall()
    } else {
        f64::from(player.ovr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(id: usize, position: Position, ovr: u8) -> PlayerData {
        serde_json::from_value(serde_json::json!({
            "id": format!("player-{id:02}"), "name": format!("Player {id}"),
            "position": position, "ovr": ovr, "condition": 100,
            "pace": 50, "stamina": 50, "strength": 50, "passing": 50,
            "shooting": 50, "tackling": 50, "dribbling": 50,
            "defending": 50, "positioning": 50, "vision": 50, "decisions": 50
        }))
        .unwrap()
    }

    fn squad() -> Vec<PlayerData> {
        (0..28)
            .map(|id| {
                let position = match id {
                    0..=2 => Position::Goalkeeper,
                    3..=11 => Position::Defender,
                    12..=20 => Position::Midfielder,
                    _ => Position::Forward,
                };
                player(id, position, 60)
            })
            .collect()
    }

    fn ids(players: &[PlayerData]) -> Vec<String> {
        players.iter().map(|p| p.id.clone()).collect()
    }

    #[test]
    fn no_preferred_builds_442_and_bounded_unique_bench() {
        let (xi, bench) = select(&squad(), &[]).unwrap();
        assert_eq!(xi.len(), 11);
        assert_eq!(bench.len(), 12);
        for (position, count) in [
            (Position::Goalkeeper, 1),
            (Position::Defender, 4),
            (Position::Midfielder, 4),
            (Position::Forward, 2),
        ] {
            assert_eq!(xi.iter().filter(|p| p.position == position).count(), count);
        }
        let unique: HashSet<_> = xi.iter().chain(&bench).map(|p| &p.id).collect();
        assert_eq!(unique.len(), 23);
    }

    #[test]
    fn departed_and_duplicate_requests_do_not_displace_available_choices() {
        let mut available = squad();
        available[3].ovr = 1;
        let preferred = vec![
            "departed".into(),
            available[3].id.clone(),
            available[0].id.clone(),
            available[3].id.clone(),
        ];
        let (xi, _) = select(&available, &preferred).unwrap();
        assert_eq!(&ids(&xi)[..2], &["player-03", "player-00"]);
        assert_eq!(xi[0].ovr, 1);
        assert_eq!(xi.len(), 11);
    }

    #[test]
    fn insufficient_or_duplicate_available_players_are_rejected() {
        assert!(select(&squad()[..10], &[]).is_err());
        let mut available = squad();
        available.push(available[0].clone());
        assert!(select(&available, &[]).is_err());
    }

    #[test]
    fn repeated_selection_and_input_reordering_are_deterministic() {
        let mut available = squad();
        let (xi, bench) = select(&available, &[]).unwrap();
        available.reverse();
        for _ in 0..3 {
            let (actual_xi, actual_bench) = select(&available, &[]).unwrap();
            assert_eq!(ids(&actual_xi), ids(&xi));
            assert_eq!(ids(&actual_bench), ids(&bench));
        }
    }

    #[test]
    fn unusual_manager_choices_are_preserved_and_missing_positions_can_fill() {
        let available: Vec<_> = (0..13)
            .map(|id| player(id, Position::Forward, 60))
            .collect();
        let preferred: Vec<_> = available[..9].iter().rev().map(|p| p.id.clone()).collect();
        let (xi, bench) = select(&available, &preferred).unwrap();
        assert_eq!(&ids(&xi)[..9], preferred);
        assert_eq!(xi.len(), 11);
        assert_eq!(bench.len(), 2);
    }
}
