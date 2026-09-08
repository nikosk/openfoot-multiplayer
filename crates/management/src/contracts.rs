//! Pure contract rules adapted from pinned ofm_core contracts/{helpers,renewals,
//! expiry,termination,free_agent} and contract_wage_policy. Ownership, affordability
//! commitment, releases and per-actor free-agent negotiation copies belong to the
//! authoritative caller. A preview evaluates a clone; only confirmation commits it.
use chrono::{Datelike, Days, Months, NaiveDate};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerContract {
    pub date_of_birth: NaiveDate,
    pub weekly_wage: u32,
    pub end_date: Option<NaiveDate>,
    pub market_value: u64,
    pub morale: u8,
    pub manager_trust: u8,
    #[serde(default)]
    pub unresolved_issue: bool,
    #[serde(default)]
    pub recent_poor_treatment: bool,
    #[serde(default)]
    pub let_expire: bool,
    #[serde(default)]
    pub blocked_until: Option<NaiveDate>,
    #[serde(default)]
    pub last_attempt: Option<NaiveDate>,
    #[serde(default)]
    pub last_agreed: Option<NaiveDate>,
    #[serde(default)]
    pub round: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    Accepted,
    Counter {
        weekly_wage: u32,
        years: u32,
    },
    Rejected {
        reason: String,
        blocked_until: Option<NaiveDate>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WarningStage {
    FinalWeeks,
    ThreeMonths,
    SixMonths,
    TwelveMonths,
}

impl PlayerContract {
    pub fn new(
        date_of_birth: NaiveDate,
        weekly_wage: u32,
        end_date: Option<NaiveDate>,
        market_value: u64,
        morale: u8,
        manager_trust: u8,
    ) -> Self {
        Self {
            date_of_birth,
            weekly_wage,
            end_date,
            market_value,
            morale,
            manager_trust,
            unresolved_issue: false,
            recent_poor_treatment: false,
            let_expire: false,
            blocked_until: None,
            last_attempt: None,
            last_agreed: None,
            round: 0,
        }
    }

    pub fn validate(&self, today: NaiveDate) -> Result<(), String> {
        if self.date_of_birth > today || self.morale > 100 || self.manager_trust > 100 {
            return Err("Invalid contract player profile".into());
        }
        if self.last_attempt.is_some_and(|date| date > today)
            || self.last_agreed.is_some_and(|date| date > today)
        {
            return Err("Contract negotiation date is in the future".into());
        }
        if self.let_expire && self.end_date.is_none() {
            return Err("Let-expire requires an active contract".into());
        }
        self.reference_wage()?;
        Ok(())
    }

    /// Preserve upstream's ordinal-day age comparison (including leap-year edge).
    pub fn age_on(&self, today: NaiveDate) -> i32 {
        today.year()
            - self.date_of_birth.year()
            - i32::from(today.ordinal() < self.date_of_birth.ordinal())
    }

    pub fn reference_wage(&self) -> Result<u32, String> {
        if self.weekly_wage > 0 {
            return Ok(self.weekly_wage);
        }
        let wage = (self.market_value / 200).max(500).min(u64::from(u32::MAX)) as u32;
        round_thousand(wage)
    }

    pub fn expected_wage(&self, reputation: u32, today: NaiveDate) -> Result<u32, String> {
        let reference = self.reference_wage()?;
        let mut wage = reference as f32;
        let age = self.age_on(today);
        if age <= 27 {
            wage *= 1.05;
        } else if age >= 32 {
            wage *= 0.95;
        }
        if self.morale <= 50 {
            wage *= 1.10;
        }
        wage *= if self.market_value >= 2_000_000 {
            1.18
        } else if self.market_value >= 750_000 {
            1.10
        } else if self.market_value <= 150_000 {
            0.95
        } else {
            1.0
        };
        if reputation < 40 {
            wage *= 1.05;
        }
        let remaining = self.remaining_days(today);
        if remaining <= 180 {
            wage *= 1.10;
        } else if remaining <= 365 {
            wage *= 1.05;
        }
        let rounded = wage.ceil();
        if !rounded.is_finite() || f64::from(rounded) > f64::from(u32::MAX) {
            return Err("Contract wage overflow".into());
        }
        Ok(round_thousand(rounded as u32)?.max(reference))
    }

    pub fn expected_years(&self, today: NaiveDate) -> u32 {
        match self.age_on(today) {
            ..=28 => 3,
            29..=32 => 2,
            _ => 1,
        }
    }

    pub fn remaining_days(&self, today: NaiveDate) -> i64 {
        self.end_date
            .map_or(0, |end| (end - today).num_days())
            .max(0)
    }

    /// Cooling resets conversation rounds after 14 days, not a waiting period.
    /// Upstream never cools blocked/agreed states, even after their date passes.
    pub fn cool_stale(&mut self, today: NaiveDate) -> bool {
        if self.let_expire
            || self.blocked_until.is_some()
            || (self.last_agreed.is_some() && self.last_agreed == self.last_attempt)
        {
            return false;
        }
        if self.round > 0
            && self
                .last_attempt
                .is_some_and(|last| (today - last).num_days() >= 14)
        {
            self.round = 0;
            return true;
        }
        false
    }

    pub fn set_let_expire(&mut self, today: NaiveDate, enabled: bool) -> Result<(), String> {
        if enabled && self.end_date.is_none() {
            return Err("Player has no active contract".into());
        }
        if enabled || self.let_expire {
            self.blocked_until = None;
            self.last_agreed = None;
            self.round = 0;
            if enabled {
                self.last_attempt = Some(today);
            }
        }
        self.let_expire = enabled;
        Ok(())
    }

    /// Updates negotiation state but never salary or contract end. Upstream free
    /// agents bypass existing blocks and relationship gating; renewals do not.
    pub fn evaluate(
        &mut self,
        reputation: u32,
        today: NaiveDate,
        wage: u32,
        years: u32,
        free_agent: bool,
    ) -> Result<Decision, String> {
        self.validate(today)?;
        let mut staged = self.clone();
        let result = staged.evaluate_inner(reputation, today, wage, years, free_agent)?;
        *self = staged;
        Ok(result)
    }

    fn evaluate_inner(
        &mut self,
        reputation: u32,
        today: NaiveDate,
        wage: u32,
        years: u32,
        free_agent: bool,
    ) -> Result<Decision, String> {
        let rejected = |reason: &str, blocked_until| Decision::Rejected {
            reason: reason.into(),
            blocked_until,
        };
        // Invalid renewal duration leaves state untouched; free-agent path cools first.
        if free_agent {
            self.cool_stale(today);
        }
        if !(1..=5).contains(&years) {
            return Ok(rejected("Invalid contract years", None));
        }
        if !free_agent {
            self.cool_stale(today);
            if self.let_expire || self.blocked_until.is_some_and(|until| until >= today) {
                return Ok(rejected("Negotiation blocked", self.blocked_until));
            }
            if self.last_agreed == Some(today) && self.last_attempt == Some(today) {
                return Ok(rejected("Already agreed today", None));
            }
        }
        let expected = self.expected_wage(reputation, today)?;
        let expected_years = self.expected_years(today);
        let reference = if free_agent {
            self.reference_wage()?
        } else {
            self.weekly_wage
        };
        let next_round = if self.last_attempt == Some(today) {
            self.round.saturating_add(1).clamp(1, 255)
        } else {
            1
        };
        let insulting = wage < ((reference.max(expected) as f32) * 0.65).floor() as u32;
        let below_minimum = wage < minimum_acceptable_wage(reference);
        let margin = match self.manager_trust {
            0..=20 => 2000,
            21..=30 => 1000,
            _ => 0,
        };
        let relationship_blocked =
            !free_agent && margin > 0 && wage < expected.saturating_add(margin);
        if free_agent && !insulting && below_minimum {
            return Ok(rejected("Below minimum wage", None));
        }
        self.last_attempt = Some(today);
        self.round = next_round;
        if insulting {
            self.blocked_until = Some(
                today
                    .checked_add_days(Days::new(30))
                    .ok_or("Contract block date overflow")?,
            );
            return Ok(rejected("Insulting wage offer", self.blocked_until));
        }
        self.blocked_until = None;
        if relationship_blocked {
            return Ok(rejected("Manager relationship", None));
        }
        if below_minimum {
            return Ok(rejected("Below minimum wage", None));
        }
        if wage >= expected && years >= expected_years {
            self.last_agreed = Some(today);
            self.let_expire = false;
            Ok(Decision::Accepted)
        } else {
            Ok(Decision::Counter {
                weekly_wage: expected,
                years: expected_years,
            })
        }
    }

    pub fn apply_agreement(
        &mut self,
        today: NaiveDate,
        wage: u32,
        years: u32,
    ) -> Result<(), String> {
        if !(1..=5).contains(&years) {
            return Err("Invalid contract years".into());
        }
        let end = today
            .checked_add_months(Months::new(years * 12))
            .ok_or("Contract end date overflow")?;
        self.weekly_wage = wage;
        self.end_date = Some(end);
        self.blocked_until = None;
        self.last_attempt = Some(today);
        self.last_agreed = Some(today);
        self.let_expire = false;
        Ok(())
    }

    pub fn severance(&self, today: NaiveDate) -> Result<i64, String> {
        let weeks = self
            .remaining_days(today)
            .checked_add(6)
            .ok_or("Severance overflow")?
            / 7;
        weeks
            .checked_mul(i64::from(self.weekly_wage))
            .ok_or_else(|| "Severance overflow".into())
    }

    pub fn warning_stage(&self, today: NaiveDate) -> Option<WarningStage> {
        match (self.end_date? - today).num_days() {
            1..=30 => Some(WarningStage::FinalWeeks),
            31..=90 => Some(WarningStage::ThreeMonths),
            91..=180 => Some(WarningStage::SixMonths),
            181..=365 => Some(WarningStage::TwelveMonths),
            _ => None,
        }
    }
}

fn round_thousand(value: u32) -> Result<u32, String> {
    value
        .div_ceil(1000)
        .checked_mul(1000)
        .ok_or_else(|| "Contract wage overflow".into())
}

pub fn minimum_acceptable_wage(wage: u32) -> u32 {
    ((wage as f32) * 0.85).floor() as u32
}

/// Pinned policy compares raw wage totals with the source wage budget. Cash
/// balance affects its projection display, not permission. Use u128 intermediates
/// to preserve the inequalities without signed/unsigned overflow.
pub fn wage_policy_allows(
    _balance: i64,
    wage_budget: u64,
    current_total: u64,
    old_wage: u32,
    offered: u32,
) -> bool {
    let Some(retained) = current_total.checked_sub(u64::from(old_wage)) else {
        return false;
    };
    let current = u128::from(current_total);
    let projected = u128::from(retained) + u128::from(offered);
    let budget = u128::from(wage_budget);
    if current <= budget {
        return projected <= budget * 110 / 100;
    }
    projected <= current + (budget * 3 / 100).max(25_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn date(text: &str) -> NaiveDate {
        text.parse().unwrap()
    }
    fn profile() -> PlayerContract {
        PlayerContract::new(
            date("2000-01-01"),
            10_000,
            Some(date("2029-01-01")),
            500_000,
            70,
            60,
        )
    }

    #[test]
    fn expected_wages_and_years_preserve_source_thresholds() {
        let today = date("2026-01-01");
        let mut player = profile();
        assert_eq!(player.expected_wage(40, today).unwrap(), 11_000);
        player.morale = 50;
        player.market_value = 2_000_000;
        player.end_date = Some(date("2026-06-30"));
        assert_eq!(player.expected_wage(39, today).unwrap(), 16_000);
        assert_eq!(player.expected_years(date("2028-01-01")), 3);
        assert_eq!(player.expected_years(date("2029-01-01")), 2);
        assert_eq!(player.expected_years(date("2033-01-01")), 1);
        player.weekly_wage = 0;
        player.market_value = 0;
        assert_eq!(player.reference_wage().unwrap(), 1000);
        player.market_value = u64::MAX;
        assert!(player.reference_wage().is_err());
        player.weekly_wage = u32::MAX;
        assert!(player.expected_wage(0, today).is_err());
    }

    #[test]
    fn counters_acceptance_and_relationship_are_consequential_but_not_a_commit() {
        let today = date("2026-01-01");
        let mut player = profile();
        assert_eq!(
            player.evaluate(50, today, 10_000, 2, false).unwrap(),
            Decision::Counter {
                weekly_wage: 11_000,
                years: 3
            }
        );
        assert_eq!(
            player.evaluate(50, today, 11_000, 3, false).unwrap(),
            Decision::Accepted
        );
        assert_eq!(player.weekly_wage, 10_000);
        assert_eq!(player.end_date, Some(date("2029-01-01")));
        assert!(
            matches!(player.evaluate(50, today, 20_000, 3, false).unwrap(), Decision::Rejected { reason, .. } if reason == "Already agreed today")
        );
        let mut strained = profile();
        strained.manager_trust = 20;
        assert!(
            matches!(strained.evaluate(50, today, 12_999, 3, false).unwrap(), Decision::Rejected { reason, .. } if reason == "Manager relationship")
        );
        assert_eq!(
            strained.evaluate(50, today, 13_000, 3, false).unwrap(),
            Decision::Accepted
        );
    }

    #[test]
    fn insult_floor_block_is_inclusive_and_let_expire_is_indefinite() {
        let today = date("2026-01-01");
        let mut player = profile();
        assert!(matches!(
            player.evaluate(50, today, 7148, 3, false).unwrap(),
            Decision::Rejected {
                blocked_until: Some(_),
                ..
            }
        ));
        assert!(matches!(
            player
                .evaluate(50, date("2026-01-31"), 99_000, 5, false)
                .unwrap(),
            Decision::Rejected { .. }
        ));
        assert_eq!(
            player
                .evaluate(50, date("2026-02-01"), 99_000, 5, false)
                .unwrap(),
            Decision::Accepted
        );
        let mut boundary = profile();
        // f32 source arithmetic floors 11000 * 0.65 to 7149, not 7150.
        assert!(matches!(
            boundary.evaluate(50, today, 7149, 3, false).unwrap(),
            Decision::Rejected {
                blocked_until: None,
                ..
            }
        ));
        assert!(
            matches!(boundary.evaluate(50, today, 8499, 3, false).unwrap(),
            Decision::Rejected { reason, .. } if reason == "Below minimum wage")
        );
        assert_eq!(
            boundary.evaluate(50, today, 8500, 3, false).unwrap(),
            Decision::Counter {
                weekly_wage: 11_000,
                years: 3
            }
        );
        boundary.set_let_expire(today, true).unwrap();
        assert!(matches!(
            boundary
                .evaluate(50, date("2027-01-01"), 99_000, 5, false)
                .unwrap(),
            Decision::Rejected { .. }
        ));
        boundary.set_let_expire(date("2027-01-01"), false).unwrap();
        assert_eq!(
            boundary
                .evaluate(50, date("2027-01-01"), 99_000, 5, false)
                .unwrap(),
            Decision::Accepted
        );
    }

    #[test]
    fn cooling_and_free_agent_asymmetry_match_pinned_source() {
        let today = date("2026-01-01");
        let mut player = profile();
        player.evaluate(50, today, 10_000, 2, false).unwrap();
        assert!(!player.cool_stale(date("2026-01-14")));
        assert!(player.cool_stale(date("2026-01-15")));
        assert_eq!(player.round, 0);
        let mut free = profile();
        free.end_date = None;
        free.weekly_wage = 0;
        free.manager_trust = 0;
        free.evaluate(50, today, 1, 3, true).unwrap();
        assert!(free.blocked_until.is_some());
        assert_eq!(
            free.evaluate(50, today, 99_000, 5, true).unwrap(),
            Decision::Accepted
        );
    }

    #[test]
    fn calendar_severance_and_warning_boundaries() {
        let mut player = profile();
        player.apply_agreement(date("2024-02-29"), 1234, 1).unwrap();
        assert_eq!(player.end_date, Some(date("2025-02-28")));
        assert_eq!(player.severance(date("2025-02-20")).unwrap(), 2468);
        assert_eq!(player.severance(date("2025-02-21")).unwrap(), 1234);
        assert_eq!(player.severance(date("2025-02-28")).unwrap(), 0);
        let today = date("2026-01-01");
        for (days, expected) in [
            (0, None),
            (30, Some(WarningStage::FinalWeeks)),
            (31, Some(WarningStage::ThreeMonths)),
            (90, Some(WarningStage::ThreeMonths)),
            (91, Some(WarningStage::SixMonths)),
            (180, Some(WarningStage::SixMonths)),
            (181, Some(WarningStage::TwelveMonths)),
            (365, Some(WarningStage::TwelveMonths)),
            (366, None),
        ] {
            player.end_date = today.checked_add_days(Days::new(days));
            assert_eq!(player.warning_stage(today), expected);
        }
        assert_eq!(
            PlayerContract::new(date("2000-03-01"), 1, None, 0, 50, 50).age_on(date("2025-03-01")),
            24
        );
    }

    #[test]
    fn wage_policy_preserves_soft_cap_and_legacy_grace_not_cash_gate() {
        assert!(wage_policy_allows(-100, 100_000, 100_000, 10_000, 20_000));
        assert!(!wage_policy_allows(
            999_999, 100_000, 100_000, 10_000, 20_001
        ));
        assert!(wage_policy_allows(0, 100_000, 110_000, 10_000, 35_000));
        assert!(!wage_policy_allows(0, 100_000, 110_000, 10_000, 35_001));
        assert!(wage_policy_allows(0, 1_000_000, 1_100_000, 10_000, 40_000));
        assert!(!wage_policy_allows(0, 1_000_000, 1_100_000, 10_000, 40_001));
        assert!(!wage_policy_allows(0, 100, 0, 1, 1));
        assert!(wage_policy_allows(0, u64::MAX, u64::MAX, 0, u32::MAX));
    }
}
