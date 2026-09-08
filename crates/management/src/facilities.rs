//! Pinned 64677fee commands/club.rs authorization gates and ofm_core/club.rs
//! upgrade arithmetic plus approved openfoot-career-runtime-v1.patch. Training
//! caps at 6, Scouting at 3; Medical retains uncapped saturating source behavior.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Medical increases training condition recovery by 10% per level above one.
//! Approved patch: training gains +5%/level through6; scouting saves one day per
//! level through3, minimum1. Medical does not shorten injury duration upstream.
//! Finance.projected_weekly_net must be the full source income/spending projection,
//! not an invented wage-only estimate. Actor/date/club preview scope is host-owned.
pub use domain::team::{Facilities, FacilityType};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finance {
    pub balance: i64,
    pub season_expenses: i64,
    pub wage_bill: i64,
    pub wage_budget: i64,
    pub projected_weekly_net: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Health {
    Stable,
    Watch,
    Warning,
    Critical,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpgradePreview {
    pub facility: FacilityType,
    pub level_before: u8,
    pub level_after: u8,
    pub cost: i64,
    pub balance_after: i64,
    pub season_expenses_after: i64,
    pub expected_facilities: Facilities,
    pub expected_finance: Finance,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum UpgradeConfirmation {
    Applied,
    RefreshRequired(UpgradePreview),
}

pub fn finance_health(finance: &Finance) -> Health {
    let usage = (i128::from(finance.wage_bill) * 100 / i128::from(finance.wage_budget.max(1)))
        .clamp(0, i128::from(u32::MAX));
    let runway = if finance.projected_weekly_net >= 0 {
        None
    } else {
        Some((i128::from(finance.balance) / (-i128::from(finance.projected_weekly_net))).max(0))
    };
    if usage > 110 || finance.balance < 0 || runway.is_some_and(|weeks| weeks <= 4) {
        Health::Critical
    } else if usage > 100 || runway.is_some_and(|weeks| weeks <= 8) {
        Health::Warning
    } else if usage >= 85 || runway.is_some_and(|weeks| weeks <= 12) {
        Health::Watch
    } else {
        Health::Stable
    }
}

pub fn level(facilities: &Facilities, facility: &FacilityType) -> u8 {
    match facility {
        FacilityType::Training => facilities.training,
        FacilityType::Medical => facilities.medical,
        FacilityType::Scouting => facilities.scouting,
    }
}
pub fn next_upgrade_cost(facilities: &Facilities, facility: &FacilityType) -> i64 {
    let current = level(facilities, facility);
    i64::from(if matches!(facility, FacilityType::Medical) {
        current
    } else {
        current.max(1)
    }) * 250_000
}
pub fn scouting_assignment_days(base_days: u32, level: u8) -> u32 {
    base_days
        .saturating_sub(u32::from(level.clamp(1, 3) - 1))
        .max(1)
}
pub fn medical_recovery_multiplier(facilities: &Facilities) -> f64 {
    1.0 + f64::from(facilities.medical.saturating_sub(1)) * 0.1
}

pub fn review_upgrade(
    facilities: &Facilities,
    facility: FacilityType,
    finance: &Finance,
) -> Result<UpgradePreview, String> {
    let before = level(facilities, &facility);
    let max = match facility {
        FacilityType::Training => Some(6),
        FacilityType::Scouting => Some(3),
        FacilityType::Medical => None,
    };
    if max.is_some_and(|max| before >= max) {
        return Err("be.error.facilityUpgradeMaxLevel".into());
    }
    if finance.wage_bill > finance.wage_budget {
        return Err("be.error.finance.facilityUpgradeOverBudget".into());
    }
    if matches!(finance_health(finance), Health::Warning | Health::Critical) {
        return Err("be.error.finance.facilityUpgradeCritical".into());
    }
    let cost = next_upgrade_cost(facilities, &facility);
    if finance.balance < cost {
        return Err(format!(
            "be.error.finance.facilityUpgradeInsufficientFunds?amount={cost}"
        ));
    }
    let balance_after = finance
        .balance
        .checked_sub(cost)
        .ok_or("Facility balance overflow")?;
    let season_expenses_after = finance
        .season_expenses
        .checked_add(cost)
        .ok_or("Facility expenses overflow")?;
    let before = level(facilities, &facility);
    Ok(UpgradePreview {
        facility,
        level_before: before,
        level_after: if max.is_some() {
            before.max(1) + 1
        } else {
            before.saturating_add(1)
        },
        cost,
        balance_after,
        season_expenses_after,
        expected_facilities: facilities.clone(),
        expected_finance: finance.clone(),
    })
}

pub fn confirm_upgrade(
    facilities: &mut Facilities,
    finance: &mut Finance,
    preview: &UpgradePreview,
) -> Result<UpgradeConfirmation, String> {
    let current = review_upgrade(facilities, preview.facility.clone(), finance)?;
    if &current != preview {
        return Ok(UpgradeConfirmation::RefreshRequired(current));
    }
    match preview.facility {
        FacilityType::Training => facilities.training = current.level_after,
        FacilityType::Medical => facilities.medical = current.level_after,
        FacilityType::Scouting => facilities.scouting = current.level_after,
    }
    finance.balance = current.balance_after;
    finance.season_expenses = current.season_expenses_after;
    Ok(UpgradeConfirmation::Applied)
}

#[cfg(test)]
mod tests {
    #[test]
    fn approved_patch_training_scouting_caps_and_day_effect() {
        for (facility, max) in [
            (super::FacilityType::Training, 6),
            (super::FacilityType::Scouting, 3),
        ] {
            let mut facilities = super::Facilities::default();
            match facility {
                super::FacilityType::Training => facilities.training = 0,
                _ => facilities.scouting = 0,
            }
            let preview = super::review_upgrade(&facilities, facility.clone(), &cash()).unwrap();
            assert_eq!((preview.cost, preview.level_after), (250_000, 2));
            match facility {
                super::FacilityType::Training => facilities.training = max,
                _ => facilities.scouting = max,
            }
            assert_eq!(
                super::review_upgrade(&facilities, facility, &cash()).unwrap_err(),
                "be.error.facilityUpgradeMaxLevel"
            );
        }
        assert_eq!(super::scouting_assignment_days(5, 1), 5);
        assert_eq!(super::scouting_assignment_days(5, 3), 3);
        assert_eq!(super::scouting_assignment_days(2, 255), 1);
    }
    use super::*;
    fn cash() -> Finance {
        Finance {
            balance: 100_000_000,
            season_expenses: 0,
            wage_bill: 100_000,
            wage_budget: 1_000_000,
            projected_weekly_net: 0,
        }
    }
    #[test]
    fn exact_cost_and_effect_include_source_level_zero_and_255_behavior() {
        for (before, after, cost) in [
            (0, 1, 0),
            (1, 2, 250_000),
            (10, 11, 2_500_000),
            (255, 255, 63_750_000),
        ] {
            let mut facilities = Facilities {
                medical: before,
                ..Default::default()
            };
            let mut finance = cash();
            let preview = review_upgrade(&facilities, FacilityType::Medical, &finance).unwrap();
            assert_eq!(preview.cost, cost);
            assert_eq!(facilities.medical, before);
            assert_eq!(
                confirm_upgrade(&mut facilities, &mut finance, &preview).unwrap(),
                UpgradeConfirmation::Applied
            );
            assert_eq!(facilities.medical, after);
            assert_eq!(finance.balance, 100_000_000 - cost);
            assert_eq!(finance.season_expenses, cost);
        }
        assert_eq!(
            medical_recovery_multiplier(&Facilities {
                medical: 11,
                ..Default::default()
            }),
            2.0
        );
    }
    #[test]
    fn finance_health_gates_runway_and_budget_before_available_cash() {
        let facilities = Facilities::default();
        let mut finance = cash();
        finance.wage_bill = finance.wage_budget + 1;
        assert_eq!(
            review_upgrade(&facilities, FacilityType::Training, &finance).unwrap_err(),
            "be.error.finance.facilityUpgradeOverBudget"
        );
        finance.wage_bill = 0;
        finance.balance = 8_000_000;
        finance.projected_weekly_net = -1_000_000;
        assert_eq!(finance_health(&finance), Health::Warning);
        assert_eq!(
            review_upgrade(&facilities, FacilityType::Scouting, &finance).unwrap_err(),
            "be.error.finance.facilityUpgradeCritical"
        );
        finance.balance = 9_000_000;
        assert_eq!(finance_health(&finance), Health::Watch);
        assert!(review_upgrade(&facilities, FacilityType::Scouting, &finance).is_ok());
        finance.projected_weekly_net = 0;
        finance.balance = 249_999;
        assert!(
            review_upgrade(&facilities, FacilityType::Scouting, &finance)
                .unwrap_err()
                .contains("InsufficientFunds")
        );
        finance.balance = 250_000;
        assert!(review_upgrade(&facilities, FacilityType::Scouting, &finance).is_ok());
    }
    #[test]
    fn stale_preview_refreshes_and_overflow_is_atomic() {
        let mut facilities = Facilities::default();
        let mut finance = cash();
        let preview = review_upgrade(&facilities, FacilityType::Training, &finance).unwrap();
        finance.balance -= 100;
        assert!(matches!(
            confirm_upgrade(&mut facilities, &mut finance, &preview).unwrap(),
            UpgradeConfirmation::RefreshRequired(_)
        ));
        assert_eq!(facilities.training, 1);
        assert_eq!(finance.season_expenses, 0);
        finance.season_expenses = i64::MAX;
        assert!(review_upgrade(&facilities, FacilityType::Training, &finance).is_err());
        assert_eq!(facilities.training, 1);
        finance.projected_weekly_net = i64::MIN;
        assert_eq!(finance_health(&finance), Health::Critical);
    }
}
