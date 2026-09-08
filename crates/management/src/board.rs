//! Board arithmetic adapted from OpenFoot Manager's `ofm_core/src/board_objectives.rs`,
//! `firing.rs`, and `turn/post_match.rs`.
//! Upstream revision: 64677fee9047a1182005d666bafa5dbc025dca5c.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Multiplayer policy: every manager uses the source external-manager match rule.
//! The different source AI rule (`ai_hiring.rs`: recomputed form satisfaction,
//! W +8/D +1/L -12, losing-streak and user-rivalry penalties) is intentionally not
//! ported. No manager type or privileged user identity enters this module.
//! The caller owns employment, private warnings, public dismissal, and once-only
//! season settlement. A firing decision never resets the departing board record;
//! a replacement manager receives a separate, newly initialized record.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectiveTargets {
    pub expected_pos: u32,
    pub win_target: u32,
    pub goals_target: u32,
    pub finance_target: u32,
}

impl ObjectiveTargets {
    /// Use the participating league's count, never the number of clubs in the world.
    /// The 128-club bound matches the calendar; one club preserves upstream's
    /// degenerate objective arithmetic even though no competitive calendar exists.
    pub fn new(reputation: u32, league_size: u32) -> Result<Self, String> {
        if reputation > 1000 || !(1..=128).contains(&league_size) {
            return Err("board reputation must be 0..=1000 and league size 1..=128".into());
        }
        let expected_pos = if reputation >= 800 {
            1
        } else if reputation >= 650 {
            (league_size / 4).max(2)
        } else if reputation >= 400 {
            (league_size / 2).max(1)
        } else {
            (league_size * 3 / 4).max(league_size / 2 + 1)
        }
        .min(league_size);
        let matchdays = (league_size - 1) * 2;
        let win_target = if matchdays == 0 {
            0
        } else {
            let percentage = if reputation >= 800 {
                60
            } else if reputation >= 650 {
                45
            } else if reputation >= 400 {
                30
            } else {
                10
            };
            (matchdays * percentage / 100).max(1)
        };
        let goals_target = if matchdays == 0 {
            0
        } else if reputation >= 800 {
            (matchdays * 3 / 2).max(20)
        } else if reputation >= 650 {
            (matchdays / 2).max(15)
        } else {
            (matchdays / 5).max(10)
        };
        Ok(Self {
            expected_pos,
            win_target,
            goals_target,
            finance_target: 100,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardState {
    pub satisfaction: u8,
    /// 0: no warning, 1: warning issued, 2: final warning issued.
    pub warning_stage: u8,
    pub league_size: u32,
    pub objectives: ObjectiveTargets,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    None,
    Warning,
    FinalWarning,
    Fired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeasonOutcome {
    pub position_met: bool,
    pub wins_met: bool,
    pub goals_met: bool,
    pub finance_met: bool,
    /// Rule delta, before clamping satisfaction to 0..=100.
    pub satisfaction_delta: i8,
}

impl BoardState {
    pub fn new(
        reputation: u32,
        league_size: u32,
        initial_satisfaction: u8,
    ) -> Result<Self, String> {
        if initial_satisfaction > 100 {
            return Err("board satisfaction must be 0..=100".into());
        }
        Ok(Self {
            satisfaction: initial_satisfaction,
            warning_stage: 0,
            league_size,
            objectives: ObjectiveTargets::new(reputation, league_size)?,
        })
    }

    /// Apply once for each completed match that counts for league standings.
    /// Returns the rule delta, before clamping. Friendlies must not call this.
    pub fn after_match(&mut self, scored: u8, conceded: u8) -> i8 {
        let delta = match scored.cmp(&conceded) {
            std::cmp::Ordering::Greater => 2,
            std::cmp::Ordering::Equal => -1,
            std::cmp::Ordering::Less => -3,
        };
        self.apply_delta(delta);
        delta
    }

    /// Issue each warning once. At <=10, a prior warning is required to fire;
    /// at 11..=18, the first warning can be a final warning, as in upstream.
    /// The caller must stop evaluating a manager after acting on `Fired`.
    pub fn evaluate_firing(&mut self) -> Decision {
        let decision = if self.satisfaction <= 10 {
            if self.warning_stage >= 1 {
                Decision::Fired
            } else {
                Decision::Warning
            }
        } else if self.satisfaction <= 18 && self.warning_stage < 2 {
            Decision::FinalWarning
        } else if self.satisfaction <= 25 && self.warning_stage < 1 {
            Decision::Warning
        } else {
            Decision::None
        };
        match decision {
            Decision::Warning => self.warning_stage = 1,
            Decision::FinalWarning => self.warning_stage = 2,
            Decision::None | Decision::Fired => {}
        }
        decision
    }

    /// Evaluate and apply the four objectives at a completed season's settlement.
    /// The caller must invoke this exactly once per season, after obtaining actual
    /// standings and finances. Wage usage is already rounded by the finance layer.
    pub fn evaluate_season(
        &mut self,
        position: u32,
        wins: u32,
        goals: u32,
        wage_usage_percent: u32,
        in_debt: bool,
    ) -> Result<SeasonOutcome, String> {
        if position == 0 || position > self.league_size {
            return Err("season position must be within the participating league".into());
        }
        let mut outcome = SeasonOutcome {
            position_met: position <= self.objectives.expected_pos,
            wins_met: wins >= self.objectives.win_target,
            goals_met: goals >= self.objectives.goals_target,
            finance_met: !in_debt && wage_usage_percent <= self.objectives.finance_target,
            satisfaction_delta: 0,
        };
        let met = [
            outcome.position_met,
            outcome.wins_met,
            outcome.goals_met,
            outcome.finance_met,
        ]
        .into_iter()
        .filter(|value| *value)
        .count();
        outcome.satisfaction_delta = match met {
            4 => 15,
            3 => 5,
            1 | 2 => -5,
            _ => -15,
        };
        self.apply_delta(outcome.satisfaction_delta);
        Ok(outcome)
    }

    /// New-season targets; neither success nor rollover clears an old warning.
    /// Failed reconfiguration leaves the complete prior record unchanged.
    pub fn reset_objectives(&mut self, reputation: u32, league_size: u32) -> Result<(), String> {
        let objectives = ObjectiveTargets::new(reputation, league_size)?;
        self.objectives = objectives;
        self.league_size = league_size;
        Ok(())
    }

    fn apply_delta(&mut self, delta: i8) {
        self.satisfaction = (i16::from(self.satisfaction) + i16::from(delta)).clamp(0, 100) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_reputation_thresholds_use_league_size() {
        for (rep, position, wins, goals) in [
            (0, 15, 3, 10),
            (399, 15, 3, 10),
            (400, 10, 11, 10),
            (649, 10, 11, 10),
            (650, 5, 17, 19),
            (799, 5, 17, 19),
            (800, 1, 22, 57),
            (1000, 1, 22, 57),
        ] {
            assert_eq!(
                ObjectiveTargets::new(rep, 20).unwrap(),
                ObjectiveTargets {
                    expected_pos: position,
                    win_target: wins,
                    goals_target: goals,
                    finance_target: 100
                }
            );
        }
        for rep in [0, 400, 650, 800] {
            let solo = ObjectiveTargets::new(rep, 1).unwrap();
            assert_eq!(
                (solo.expected_pos, solo.win_target, solo.goals_target),
                (1, 0, 0)
            );
            let pair = ObjectiveTargets::new(rep, 2).unwrap();
            assert_eq!(pair.win_target, 1);
            assert!(pair.expected_pos <= 2);
        }
        // No fallback to all 440 world clubs is accepted.
        assert!(ObjectiveTargets::new(800, 440).is_err());
    }

    #[test]
    fn matches_saturate_and_identical_manager_boards_remain_equal() {
        let mut a = BoardState::new(650, 20, 99).unwrap();
        let mut b = a.clone();
        for (scored, conceded, satisfaction) in [(2, 1, 100), (0, 0, 99), (0, 1, 96)] {
            a.after_match(scored, conceded);
            b.after_match(scored, conceded);
            assert_eq!(a, b);
            assert_eq!(a.satisfaction, satisfaction);
        }
        a.satisfaction = 1;
        a.after_match(0, 255);
        assert_eq!(a.satisfaction, 0);
        a.after_match(255, 255);
        assert_eq!(a.satisfaction, 0);
    }

    #[test]
    fn firing_thresholds_and_prior_warning_are_exact() {
        for (sat, first, stage, second) in [
            (26, Decision::None, 0, Decision::None),
            (25, Decision::Warning, 1, Decision::None),
            (19, Decision::Warning, 1, Decision::None),
            (18, Decision::FinalWarning, 2, Decision::None),
            (11, Decision::FinalWarning, 2, Decision::None),
            (10, Decision::Warning, 1, Decision::Fired),
            (0, Decision::Warning, 1, Decision::Fired),
        ] {
            let mut board = BoardState::new(500, 20, sat).unwrap();
            assert_eq!(board.evaluate_firing(), first);
            assert_eq!(board.warning_stage, stage);
            assert_eq!(board.evaluate_firing(), second);
            assert_eq!(board.warning_stage, stage);
            assert_eq!(board.satisfaction, sat);
        }
        let mut board = BoardState::new(500, 20, 25).unwrap();
        board.evaluate_firing();
        board.satisfaction = 18;
        assert_eq!(board.evaluate_firing(), Decision::FinalWarning);
        board.satisfaction = 100;
        assert_eq!(board.evaluate_firing(), Decision::None);
        assert_eq!(board.warning_stage, 2);
        board.satisfaction = 10;
        assert_eq!(board.evaluate_firing(), Decision::Fired);
        assert_eq!(board.warning_stage, 2);
    }

    #[test]
    fn season_objective_boundaries_and_delta_counts() {
        for (position, wins, goals, wage, debt, delta) in [
            (1, 22, 57, 100, false, 15),
            (2, 22, 57, 100, false, 5),
            (2, 21, 57, 100, false, -5),
            (2, 21, 56, 100, false, -5),
            (2, 21, 56, 101, false, -15),
            (2, 21, 56, 0, true, -15),
        ] {
            let mut board = BoardState::new(800, 20, 50).unwrap();
            let outcome = board
                .evaluate_season(position, wins, goals, wage, debt)
                .unwrap();
            assert_eq!(outcome.satisfaction_delta, delta);
            assert_eq!(board.satisfaction, (50 + delta) as u8);
            assert_eq!(outcome.position_met, position == 1);
            assert_eq!(outcome.wins_met, wins >= 22);
            assert_eq!(outcome.goals_met, goals >= 57);
            assert_eq!(outcome.finance_met, wage <= 100 && !debt);
        }
        let mut board = BoardState::new(800, 20, 99).unwrap();
        board.evaluate_season(1, 22, 57, 100, false).unwrap();
        assert_eq!(board.satisfaction, 100);
        board.satisfaction = 1;
        board.evaluate_season(20, 0, 0, 101, false).unwrap();
        assert_eq!(board.satisfaction, 0);
    }

    #[test]
    fn reset_preserves_warning_and_satisfaction_and_errors_are_atomic() {
        let mut board = BoardState::new(800, 20, 10).unwrap();
        board.evaluate_firing();
        board.reset_objectives(0, 2).unwrap();
        assert_eq!(
            (board.satisfaction, board.warning_stage, board.league_size),
            (10, 1, 2)
        );
        assert_eq!(board.objectives, ObjectiveTargets::new(0, 2).unwrap());
        let original = board.clone();
        assert!(board.reset_objectives(1001, 2).is_err());
        assert!(board.evaluate_season(0, 0, 0, 0, false).is_err());
        assert!(board.evaluate_season(3, 0, 0, 0, false).is_err());
        assert_eq!(board, original);
        assert!(BoardState::new(0, 0, 0).is_err());
        assert!(BoardState::new(0, 129, 0).is_err());
        assert!(BoardState::new(0, 20, 101).is_err());
    }
}
