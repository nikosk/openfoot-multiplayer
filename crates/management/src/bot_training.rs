//! Training decisions adapted from pinned OpenFoot `ofm_core/src/ai_training.rs`.
//! Revision 64677fee9047a1182005d666bafa5dbc025dca5c.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Pure policy, not privileged mutation. The host submits the returned choices
//! through the same management commands as external participants.
use crate::training::{Focus, Intensity, Schedule};
use engine::PlayStyle;

pub fn choose(
    style: PlayStyle,
    schedule: Schedule,
    weekday: u32,
    available_conditions: &[u8],
    days_to_next_fixture: Option<u32>,
    fixtures_in_next_seven: usize,
) -> Result<Option<(Focus, Intensity)>, String> {
    if weekday > 6
        || available_conditions
            .iter()
            .any(|condition| *condition > 100)
    {
        return Err("Invalid training policy inputs".into());
    }
    if !schedule.is_training_day(weekday) {
        return Ok(None);
    }
    let average = if available_conditions.is_empty() {
        100.0
    } else {
        available_conditions
            .iter()
            .map(|condition| f64::from(*condition))
            .sum::<f64>()
            / available_conditions.len() as f64
    };
    if average < 10.0 {
        return Ok(Some((Focus::Recovery, Intensity::Low)));
    }
    let mut intensity = if average < 40.0 {
        Intensity::Low
    } else if average <= 70.0 {
        Intensity::Medium
    } else {
        Intensity::High
    };
    let congestion =
        days_to_next_fixture.is_some_and(|days| days <= 2) || fixtures_in_next_seven >= 2;
    if congestion {
        intensity = match intensity {
            Intensity::High => Intensity::Medium,
            _ => Intensity::Low,
        };
    }
    let cycle = match style {
        PlayStyle::Balanced => [
            Focus::Physical,
            Focus::Technical,
            Focus::Tactical,
            Focus::Defending,
            Focus::Attacking,
        ],
        PlayStyle::Attacking => [
            Focus::Physical,
            Focus::Technical,
            Focus::Attacking,
            Focus::Attacking,
            Focus::Attacking,
        ],
        PlayStyle::Defensive => [
            Focus::Physical,
            Focus::Technical,
            Focus::Defending,
            Focus::Defending,
            Focus::Defending,
        ],
        PlayStyle::Possession => [
            Focus::Physical,
            Focus::Technical,
            Focus::Tactical,
            Focus::Tactical,
            Focus::Tactical,
        ],
        PlayStyle::HighPress => [
            Focus::Physical,
            Focus::Physical,
            Focus::Physical,
            Focus::Technical,
            Focus::Tactical,
        ],
        PlayStyle::Counter => [
            Focus::Physical,
            Focus::Technical,
            Focus::Technical,
            Focus::Technical,
            Focus::Tactical,
        ],
    };
    let focus = match intensity {
        Intensity::Low => Focus::Recovery,
        Intensity::Medium if congestion => Focus::Tactical,
        _ => cycle[weekday as usize % 5].clone(),
    };
    if focus == Focus::Physical && intensity == Intensity::High {
        intensity = Intensity::Medium;
    }
    Ok(Some((focus, intensity)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_condition_and_congestion_boundaries() {
        for (condition, expected) in [
            (9, (Focus::Recovery, Intensity::Low)),
            (39, (Focus::Recovery, Intensity::Low)),
            (40, (Focus::Technical, Intensity::Medium)),
            (70, (Focus::Technical, Intensity::Medium)),
            (71, (Focus::Technical, Intensity::High)),
        ] {
            assert_eq!(
                choose(
                    PlayStyle::Balanced,
                    Schedule::Intense,
                    1,
                    &[condition],
                    None,
                    0
                )
                .unwrap(),
                Some(expected)
            );
        }
        assert_eq!(
            choose(PlayStyle::Balanced, Schedule::Intense, 1, &[90], Some(2), 0).unwrap(),
            Some((Focus::Tactical, Intensity::Medium))
        );
        assert_eq!(
            choose(PlayStyle::Balanced, Schedule::Intense, 1, &[60], None, 2).unwrap(),
            Some((Focus::Recovery, Intensity::Low))
        );
        assert_eq!(
            choose(PlayStyle::Balanced, Schedule::Intense, 0, &[90], None, 0).unwrap(),
            Some((Focus::Physical, Intensity::Medium))
        );
    }
    #[test]
    fn rest_days_and_styles_preserve_source_cycle() {
        assert_eq!(
            choose(PlayStyle::Balanced, Schedule::Balanced, 2, &[9], None, 0).unwrap(),
            None
        );
        assert_eq!(
            choose(PlayStyle::Attacking, Schedule::Intense, 3, &[90], None, 0).unwrap(),
            Some((Focus::Attacking, Intensity::High))
        );
        assert_eq!(
            choose(PlayStyle::Counter, Schedule::Intense, 3, &[90], None, 0).unwrap(),
            Some((Focus::Technical, Intensity::High))
        );
        assert!(choose(PlayStyle::Balanced, Schedule::Intense, 7, &[90], None, 0).is_err());
    }
}
