//! Weekly board pressure adapted from OpenFoot Manager's
//! `ofm_core/src/finances.rs`, revision
//! 64677fee9047a1182005d666bafa5dbc025dca5c.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! The caller supplies the economy's actual projection. Upstream includes
//! sponsorship and estimated gate receipts; this multiplayer slice currently
//! models wages only, so its runway is explicitly a wage-only projection.
//! Call once per Monday after financial posting, for every active manager.

/// Exact source warning/critical thresholds, returning satisfaction subtraction.
/// Watch/stable finances have no penalty. Negative cash is always critical.
pub fn weekly_finance_penalty(
    balance: i64,
    wage_usage_percent: u32,
    projected_weekly_net: i64,
) -> u8 {
    let wage_penalty = if wage_usage_percent > 110 {
        4
    } else if wage_usage_percent > 100 {
        2
    } else {
        0
    };
    let runway_penalty = if balance < 0 {
        4
    } else if projected_weekly_net < 0 {
        // unsigned_abs handles i64::MIN without overflow; cash is nonnegative.
        let weeks = balance as u64 / projected_weekly_net.unsigned_abs();
        if weeks <= 4 {
            4
        } else if weeks <= 8 {
            2
        } else {
            0
        }
    } else {
        0
    };
    wage_penalty.max(runway_penalty)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wage_boundaries_preserve_strict_source_comparisons() {
        for usage in [0, 84, 85, 99, 100] {
            assert_eq!(weekly_finance_penalty(1_000_000, usage, 0), 0);
        }
        for usage in [101, 110] {
            assert_eq!(weekly_finance_penalty(1_000_000, usage, 0), 2);
        }
        for usage in [111, u32::MAX] {
            assert_eq!(weekly_finance_penalty(1_000_000, usage, 0), 4);
        }
    }
    #[test]
    fn runway_uses_integer_weeks_and_worst_pressure_not_sum() {
        for cash in [0, 400, 499] {
            assert_eq!(weekly_finance_penalty(cash, 0, -100), 4);
        }
        for cash in [500, 800, 899] {
            assert_eq!(weekly_finance_penalty(cash, 0, -100), 2);
        }
        for cash in [900, 1200, 1300] {
            assert_eq!(weekly_finance_penalty(cash, 0, -100), 0);
        }
        assert_eq!(weekly_finance_penalty(400, 111, -100), 4);
        assert_eq!(weekly_finance_penalty(800, 101, -100), 2);
    }
    #[test]
    fn debt_and_nonnegative_cashflow_match_source_without_overflow() {
        assert_eq!(weekly_finance_penalty(-1, 0, 100), 4);
        assert_eq!(weekly_finance_penalty(0, 0, 0), 0);
        assert_eq!(weekly_finance_penalty(0, 0, 1), 0);
        assert_eq!(weekly_finance_penalty(i64::MAX, 0, i64::MIN), 4);
    }
}
