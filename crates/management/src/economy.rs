//! Finance rules adapted from pinned 64677fee ofm_core/finances.rs and sponsor
//! response effects. Balance is an explicit argument, never a second state owner.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Caller owns actor/date scoping, preview dependencies, pending inbox actions,
//! and exactly-once Monday settlement. Actual source weekly payroll ignores loan
//! shares while its projections respect them: these are separate inputs here.
//! Weekly wages/sponsor/attendance change totals but upstream appends no ledger
//! records for them. Upkeep is zero. There is no manual budget-adjustment action
//! in this baseline; support reductions and season rollover are explicit rules.
use chrono::{Datelike, NaiveDate, Weekday};
use domain::team::{
    FinancialTransaction, FinancialTransactionKind, Sponsorship, SponsorshipBonusCriterion,
};
use rand::{Rng, RngExt};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FinanceState {
    pub wage_budget: i64,
    pub transfer_budget: i64,
    pub season_income: i64,
    pub season_expenses: i64,
    pub sponsorship: Option<Sponsorship>,
    pub financial_ledger: Vec<FinancialTransaction>,
    pub reputation: u32,
    pub stadium_capacity: u32,
    pub form: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Context {
    pub club_id: String,
    pub today: NaiveDate,
    pub season: u32,
    pub current_position: Option<u32>,
    pub annual_wage_bill: i64,
    pub weekly_wage_spend: i64,
    pub recent_home_matches: i64,
    pub pending_sponsor_offer: bool,
    pub sponsor_pitch_attempted_today: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    Stable,
    Watch,
    Warning,
    Critical,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub annual_wage_bill: i64,
    pub weekly_wage_spend: i64,
    pub weekly_wage_budget: i64,
    pub weekly_recurring_income: i64,
    pub weekly_sponsor_income: i64,
    pub projected_weekly_net: i64,
    pub cash_runway_weeks: Option<i64>,
    pub wage_budget_usage_percent: u32,
    pub currently_in_debt: bool,
    pub currently_over_budget: bool,
    pub wage_budget_status: Health,
    pub runway_status: Health,
    pub overall_status: Health,
    pub marketing_campaign_cooldown_days_remaining: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardSupport {
    pub support_amount: i64,
    pub transfer_budget_reduction: i64,
    pub satisfaction_penalty: u8,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketingCampaign {
    pub gross_revenue: i64,
    pub campaign_cost: i64,
    pub net_income: i64,
    pub cooldown_days: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SponsorPitch {
    pub message_id: String,
    pub sponsor_name: String,
    pub weekly_amount: i64,
    pub duration_weeks: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Warning {
    Debt { amount: u64 },
    Runway { weekly_wages: i64, weeks_left: i64 },
    OverBudget { annual_wages: i64, wage_budget: i64 },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settlement {
    pub wages: i64,
    pub upkeep: i64,
    pub sponsorship_income: i64,
    pub matchday_income: i64,
    pub attendance_percent: Option<u32>,
    pub average_ticket: Option<u32>,
    pub balance_before: i64,
    pub balance_after: i64,
    pub warning: Option<Warning>,
    pub satisfaction_penalty: u8,
}
fn add(a: i64, b: i64) -> Result<i64, String> {
    a.checked_add(b).ok_or_else(|| "Finance overflow".into())
}
fn sub(a: i64, b: i64) -> Result<i64, String> {
    a.checked_sub(b).ok_or_else(|| "Finance overflow".into())
}

pub fn annual_player_wage(
    wage: u32,
    owner: Option<&str>,
    loan: Option<&domain::player::ActiveLoan>,
    club: &str,
) -> i64 {
    let wage = i64::from(wage);
    if let Some(loan) = loan {
        let share = wage * i64::from(loan.wage_contribution_pct) / 100;
        if loan.loan_team_id == club {
            share
        } else if loan.parent_team_id == club {
            wage.saturating_sub(share)
        } else {
            0
        }
    } else if owner == Some(club) {
        wage
    } else {
        0
    }
}
pub fn count_recent_home_matches(
    fixtures: &[domain::league::Fixture],
    club: &str,
    today: NaiveDate,
) -> i64 {
    fixtures
        .iter()
        .filter(|fixture| {
            fixture.status == domain::league::FixtureStatus::Completed
                && fixture.home_team_id == club
                && fixture.result.is_some()
        })
        .filter(|fixture| {
            fixture
                .date
                .parse::<NaiveDate>()
                .is_ok_and(|date| (today - date).num_days() >= 0 && (today - date).num_days() < 7)
        })
        .count() as i64
}
pub fn calc_matchday(
    capacity: u32,
    count: i64,
    attendance: f64,
    ticket: f64,
) -> Result<i64, String> {
    if count < 0
        || !attendance.is_finite()
        || !ticket.is_finite()
        || attendance < 0.0
        || ticket < 0.0
    {
        return Err("Invalid matchday revenue inputs".into());
    }
    let value = f64::from(capacity) * attendance * ticket;
    if value >= i64::MAX as f64 {
        return Err("Matchday revenue overflow".into());
    }
    (value as i64)
        .checked_mul(count)
        .ok_or_else(|| "Matchday revenue overflow".into())
}
pub fn sponsorship_bonus(
    position: Option<u32>,
    form: &[String],
    sponsorship: &Sponsorship,
) -> Result<i64, String> {
    sponsorship
        .bonus_criteria
        .iter()
        .try_fold(0, |total, criterion| {
            let amount = match criterion {
                SponsorshipBonusCriterion::LeaguePosition {
                    max_position,
                    bonus_amount,
                } if position.is_some_and(|p| p <= *max_position) => *bonus_amount,
                SponsorshipBonusCriterion::UnbeatenRun {
                    required_matches,
                    bonus_amount,
                } if form.len() >= *required_matches
                    && form
                        .iter()
                        .rev()
                        .take(*required_matches)
                        .all(|result| result != "L") =>
                {
                    *bonus_amount
                }
                _ => 0,
            };
            add(total, amount)
        })
}
fn sponsor_income(state: &FinanceState, context: &Context) -> Result<i64, String> {
    state
        .sponsorship
        .as_ref()
        .map(|sponsor| {
            add(
                sponsor.base_value,
                sponsorship_bonus(context.current_position, &state.form, sponsor)?,
            )
        })
        .unwrap_or(Ok(0))
}
pub fn cash_runway(balance: i64, net: i64) -> Option<i64> {
    (net < 0).then(|| {
        (i128::from(balance) / (-i128::from(net)))
            .max(0)
            .min(i128::from(i64::MAX)) as i64
    })
}
fn wage_health(usage: u32) -> Health {
    if usage > 110 {
        Health::Critical
    } else if usage > 100 {
        Health::Warning
    } else if usage >= 85 {
        Health::Watch
    } else {
        Health::Stable
    }
}
fn runway_health(balance: i64, weeks: Option<i64>) -> Health {
    if balance < 0 || weeks.is_some_and(|w| w <= 4) {
        Health::Critical
    } else if weeks.is_some_and(|w| w <= 8) {
        Health::Warning
    } else if weeks.is_some_and(|w| w <= 12) {
        Health::Watch
    } else {
        Health::Stable
    }
}
pub fn marketing_cooldown(state: &FinanceState, today: NaiveDate) -> u32 {
    state
        .financial_ledger
        .iter()
        .filter(|entry| entry.kind == FinancialTransactionKind::CommercialCampaign)
        .filter_map(|entry| entry.date.parse::<NaiveDate>().ok())
        .max()
        .map_or(0, |last| {
            let elapsed = (today - last).num_days();
            if elapsed >= 28 {
                0
            } else {
                (28 - elapsed).min(i64::from(u32::MAX)) as u32
            }
        })
}
pub fn snapshot(state: &FinanceState, balance: i64, context: &Context) -> Result<Snapshot, String> {
    if context.annual_wage_bill < 0
        || context.weekly_wage_spend < 0
        || context.recent_home_matches < 0
        || state.reputation > 1000
    {
        return Err("Invalid finance projection inputs".into());
    }
    let sponsorship = sponsor_income(state, context)?;
    let income = add(
        sponsorship,
        calc_matchday(
            state.stadium_capacity,
            context.recent_home_matches,
            0.76,
            20.0,
        )?,
    )?;
    let net = sub(income, context.weekly_wage_spend)?;
    let usage = (i128::from(context.annual_wage_bill) * 100 / i128::from(state.wage_budget.max(1)))
        .clamp(0, i128::from(u32::MAX)) as u32;
    let runway = cash_runway(balance, net);
    let wage_status = wage_health(usage);
    let runway_status = runway_health(balance, runway);
    Ok(Snapshot {
        annual_wage_bill: context.annual_wage_bill,
        weekly_wage_spend: context.weekly_wage_spend,
        weekly_wage_budget: state.wage_budget / 52,
        weekly_recurring_income: income,
        weekly_sponsor_income: sponsorship,
        projected_weekly_net: net,
        cash_runway_weeks: runway,
        wage_budget_usage_percent: usage,
        currently_in_debt: balance < 0,
        currently_over_budget: context.annual_wage_bill > state.wage_budget,
        wage_budget_status: wage_status,
        runway_status,
        overall_status: wage_status.max(runway_status),
        marketing_campaign_cooldown_days_remaining: marketing_cooldown(state, context.today),
    })
}
pub fn satisfaction_penalty(snapshot: &Snapshot) -> u8 {
    match snapshot.overall_status {
        Health::Critical => 4,
        Health::Warning => 2,
        _ => 0,
    }
}
pub fn warning(state: &FinanceState, balance: i64, snapshot: &Snapshot) -> Option<Warning> {
    if balance < 0 {
        Some(Warning::Debt {
            amount: balance.unsigned_abs(),
        })
    } else if snapshot
        .cash_runway_weeks
        .is_some_and(|weeks| (0..4).contains(&weeks))
    {
        Some(Warning::Runway {
            weekly_wages: snapshot.weekly_wage_spend,
            weeks_left: snapshot.cash_runway_weeks.unwrap(),
        })
    } else if snapshot.annual_wage_bill > state.wage_budget {
        Some(Warning::OverBudget {
            annual_wages: snapshot.annual_wage_bill,
            wage_budget: state.wage_budget,
        })
    } else {
        None
    }
}

/// Purely bounded account settlement; call once per club on the closing Monday.
/// Exactly two RNG draws per club with recent home matches, otherwise none.
pub fn settle_weekly(
    state: &mut FinanceState,
    balance: &mut i64,
    context: &Context,
    actual_weekly_wages: i64,
    rng: &mut impl Rng,
) -> Result<Option<Settlement>, String> {
    if context.today.weekday() != Weekday::Mon {
        return Ok(None);
    }
    if actual_weekly_wages < 0 {
        return Err("Invalid actual payroll".into());
    }
    snapshot(state, *balance, context)?;
    let before = *balance;
    let mut next = state.clone();
    let mut cash = sub(before, actual_weekly_wages)?;
    next.season_expenses = add(next.season_expenses, actual_weekly_wages)?;
    let sponsorship_income = sponsor_income(&next, context)?.max(0);
    if sponsorship_income > 0 {
        cash = add(cash, sponsorship_income)?;
        next.season_income = add(next.season_income, sponsorship_income)?;
    }
    if let Some(sponsor) = &mut next.sponsorship {
        sponsor.remaining_weeks = sponsor.remaining_weeks.saturating_sub(1);
        if sponsor.remaining_weeks == 0 {
            next.sponsorship = None;
        }
    }
    let (attendance_percent, average_ticket, matchday_income) = if context.recent_home_matches > 0 {
        let attendance = rng.random_range(60_u32..=92);
        let ticket = rng.random_range(15_u32..=25);
        (
            Some(attendance),
            Some(ticket),
            calc_matchday(
                next.stadium_capacity,
                context.recent_home_matches,
                f64::from(attendance) / 100.0,
                f64::from(ticket),
            )?,
        )
    } else {
        (None, None, 0)
    };
    cash = add(cash, matchday_income)?;
    next.season_income = add(next.season_income, matchday_income)?;
    let projection = snapshot(&next, cash, context)?;
    let result = Settlement {
        wages: actual_weekly_wages,
        upkeep: 0,
        sponsorship_income,
        matchday_income,
        attendance_percent,
        average_ticket,
        balance_before: before,
        balance_after: cash,
        warning: warning(&next, cash, &projection),
        satisfaction_penalty: satisfaction_penalty(&projection),
    };
    *state = next;
    *balance = cash;
    Ok(Some(result))
}
fn support_description(season: u32) -> String {
    format!("Board support package for season {season}")
}
fn pressured(snapshot: &Snapshot) -> bool {
    snapshot.currently_over_budget
        || snapshot.currently_in_debt
        || snapshot.wage_budget_status >= Health::Warning
        || snapshot.runway_status >= Health::Warning
}

pub fn preview_board_support(
    state: &FinanceState,
    balance: i64,
    context: &Context,
) -> Result<BoardSupport, String> {
    let projection = snapshot(state, balance, context)?;
    if !projection.currently_in_debt && projection.runway_status < Health::Warning {
        return Err("be.error.finance.boardSupportUnavailable".into());
    }
    if state.financial_ledger.iter().any(|entry| {
        entry.kind == FinancialTransactionKind::BoardSupport
            && entry.description == support_description(context.season)
    }) {
        return Err("be.error.finance.boardSupportAlreadyUsed".into());
    }
    let target = (i128::from(projection.weekly_wage_spend) * 8).max(150_000);
    let amount = (target - i128::from(balance)).clamp(150_000, 1_000_000) as i64;
    Ok(BoardSupport {
        support_amount: amount,
        transfer_budget_reduction: state.transfer_budget.max(0).min(amount / 2),
        satisfaction_penalty: 12,
    })
}
pub fn request_board_support(
    state: &mut FinanceState,
    balance: &mut i64,
    context: &Context,
) -> Result<BoardSupport, String> {
    let preview = preview_board_support(state, *balance, context)?;
    let cash = add(*balance, preview.support_amount)?;
    let income = add(state.season_income, preview.support_amount)?;
    let budget = sub(state.transfer_budget, preview.transfer_budget_reduction)?.max(0);
    state.season_income = income;
    state.transfer_budget = budget;
    *balance = cash;
    state.financial_ledger.push(FinancialTransaction {
        date: context.today.to_string(),
        description: support_description(context.season),
        amount: preview.support_amount,
        kind: FinancialTransactionKind::BoardSupport,
    });
    Ok(preview)
}
pub fn preview_marketing_campaign(
    state: &FinanceState,
    balance: i64,
    context: &Context,
) -> Result<MarketingCampaign, String> {
    let projection = snapshot(state, balance, context)?;
    if !pressured(&projection) {
        return Err("be.error.finance.marketingCampaignUnavailable".into());
    }
    if marketing_cooldown(state, context.today) > 0 {
        return Err("be.error.finance.marketingCampaignCoolingDown".into());
    }
    let pressure = match projection.overall_status {
        Health::Stable => 0,
        Health::Watch => 10_000,
        Health::Warning => 25_000,
        Health::Critical => 40_000,
    };
    let gross = (i64::from(state.reputation) * 250
        + i64::from(state.stadium_capacity) * 3
        + pressure
        + if projection.currently_in_debt {
            20_000
        } else {
            0
        }
        + if projection.currently_over_budget {
            15_000
        } else {
            0
        })
    .clamp(60_000, 250_000);
    let cost = (gross / 4).max(15_000);
    Ok(MarketingCampaign {
        gross_revenue: gross,
        campaign_cost: cost,
        net_income: gross - cost,
        cooldown_days: 28,
    })
}
pub fn request_marketing_campaign(
    state: &mut FinanceState,
    balance: &mut i64,
    context: &Context,
) -> Result<MarketingCampaign, String> {
    let preview = preview_marketing_campaign(state, *balance, context)?;
    let cash = add(*balance, preview.net_income)?;
    let income = add(state.season_income, preview.gross_revenue)?;
    let expenses = add(state.season_expenses, preview.campaign_cost)?;
    state.season_income = income;
    state.season_expenses = expenses;
    *balance = cash;
    for (description, amount) in [
        (
            "Marketing campaign activation spend",
            -preview.campaign_cost,
        ),
        (
            "Marketing campaign merchandise revenue",
            preview.gross_revenue,
        ),
    ] {
        state.financial_ledger.push(FinancialTransaction {
            date: context.today.to_string(),
            description: description.into(),
            amount,
            kind: FinancialTransactionKind::CommercialCampaign,
        });
    }
    Ok(preview)
}
pub fn preview_sponsor_pitch(
    state: &FinanceState,
    balance: i64,
    context: &Context,
) -> Result<SponsorPitch, String> {
    let projection = snapshot(state, balance, context)?;
    if !pressured(&projection) {
        return Err("be.error.finance.sponsorPitchUnavailable".into());
    }
    if context.pending_sponsor_offer {
        return Err("be.error.finance.sponsorPitchPendingOffer".into());
    }
    if context.sponsor_pitch_attempted_today {
        return Err("be.error.finance.sponsorPitchAlreadyAttemptedToday".into());
    }
    if state
        .sponsorship
        .as_ref()
        .is_some_and(|sponsor| sponsor.remaining_weeks > 0 && sponsor.base_value > 0)
    {
        return Err("be.error.finance.sponsorPitchActiveSponsor".into());
    }
    const NAMES: [&str; 8] = [
        "Northstar Logistics",
        "Harbor Bank",
        "Crest Mobile",
        "Vertex Nutrition",
        "Iron Peak Tools",
        "Brightline Energy",
        "Summit Capital",
        "Evergreen Foods",
    ];
    // Only mod8 matters, so source usize overflow has identical results on32/64bit.
    let partner = context
        .club_id
        .bytes()
        .fold(context.today.ordinal() as u64, |acc, byte| {
            acc.wrapping_mul(31).wrapping_add(u64::from(byte))
        });
    let position = match context.current_position {
        Some(1) => 18_000,
        Some(2..=4) => 12_000,
        Some(5..=8) => 6_000,
        _ => 0,
    };
    let pressure = match projection.overall_status {
        Health::Stable => 0,
        Health::Watch => 5_000,
        Health::Warning => 15_000,
        Health::Critical => 25_000,
    };
    let amount = (40_000
        + i64::from(state.reputation) * 120
        + position
        + pressure
        + if projection.currently_over_budget {
            15_000
        } else {
            0
        }
        + if projection.currently_in_debt {
            20_000
        } else {
            0
        })
    .clamp(40_000, 180_000);
    Ok(SponsorPitch {
        message_id: format!("sponsor_pitch_{}", context.today),
        sponsor_name: NAMES[(partner % 8) as usize].into(),
        weekly_amount: amount,
        duration_weeks: 12,
    })
}
/// Called only after the recipient resolves an actual pending sponsor offer.
/// Acceptance activates a12-week deal and a25% three-match unbeaten-run bonus;
/// it does not pay money immediately. Host marks the inbox action resolved.
pub fn accepted_sponsorship(name: String, amount: u64) -> Result<Sponsorship, String> {
    let amount = i64::try_from(amount).map_err(|_| "Sponsor amount overflow")?;
    Ok(Sponsorship {
        sponsor_name: name,
        base_value: amount,
        remaining_weeks: 12,
        bonus_criteria: vec![SponsorshipBonusCriterion::UnbeatenRun {
            required_matches: 3,
            bonus_amount: amount / 4,
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    fn state() -> FinanceState {
        FinanceState {
            wage_budget: 400_000,
            transfer_budget: 500_000,
            season_income: 0,
            season_expenses: 0,
            sponsorship: None,
            financial_ledger: vec![],
            reputation: 650,
            stadium_capacity: 22_000,
            form: vec!["W".into(), "D".into(), "W".into()],
        }
    }
    fn context() -> Context {
        Context {
            club_id: "team1".into(),
            today: "2026-02-16".parse().unwrap(),
            season: 2026,
            current_position: Some(1),
            annual_wage_bill: 52_000,
            weekly_wage_spend: 1000,
            recent_home_matches: 0,
            pending_sponsor_offer: false,
            sponsor_pitch_attempted_today: false,
        }
    }
    #[test]
    fn weekly_revenue_sponsor_expiry_and_rng_are_exact_and_seeded() {
        let mut s = state();
        s.sponsorship = Some(accepted_sponsorship("Sponsor".into(), 100_000).unwrap());
        s.sponsorship.as_mut().unwrap().remaining_weeks = 1;
        let mut c = context();
        c.recent_home_matches = 2;
        let mut balance = 500_000;
        let mut expected = rand::rngs::StdRng::seed_from_u64(42);
        let a = expected.random_range(60_u32..=92);
        let ticket = expected.random_range(15_u32..=25);
        let result = settle_weekly(
            &mut s,
            &mut balance,
            &c,
            1000,
            &mut rand::rngs::StdRng::seed_from_u64(42),
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.sponsorship_income, 125_000);
        assert_eq!(result.attendance_percent, Some(a));
        assert_eq!(result.average_ticket, Some(ticket));
        assert_eq!(
            result.matchday_income,
            calc_matchday(22_000, 2, f64::from(a) / 100.0, f64::from(ticket)).unwrap()
        );
        assert_eq!(balance, 500_000 - 1000 + 125_000 + result.matchday_income);
        assert!(s.sponsorship.is_none());
        assert!(s.financial_ledger.is_empty());
        assert_eq!(s.season_expenses, 1000);
        assert_eq!(s.season_income, 125_000 + result.matchday_income);
        let before = s.clone();
        c.today = "2026-02-17".parse().unwrap();
        assert!(
            settle_weekly(&mut s, &mut balance, &c, 1000, &mut expected)
                .unwrap()
                .is_none()
        );
        assert_eq!(s, before);
    }
    #[test]
    fn snapshots_use_estimated_attendance_loan_shares_and_exact_health_thresholds() {
        let mut c = context();
        c.recent_home_matches = 1;
        let s = state();
        let p = snapshot(&s, 500_000, &c).unwrap();
        assert_eq!(p.weekly_recurring_income, 334_400);
        assert_eq!(p.projected_weekly_net, 333_400);
        assert_eq!(p.cash_runway_weeks, None);
        let loan = domain::player::ActiveLoan {
            parent_team_id: "parent".into(),
            loan_team_id: "borrower".into(),
            start_date: "".into(),
            end_date: "".into(),
            wage_contribution_pct: 33,
            buy_option_fee: None,
            loan_start_minutes: 0,
            loan_start_appearances: 0,
            development_reported_minutes: 0,
            development_reported_appearances: 0,
        };
        assert_eq!(
            annual_player_wage(101, Some("borrower"), Some(&loan), "borrower"),
            33
        );
        assert_eq!(annual_player_wage(101, None, Some(&loan), "parent"), 68);
        assert_eq!(cash_runway(i64::MIN, i64::MIN), Some(0));
        assert_eq!(wage_health(110), Health::Warning);
        assert_eq!(wage_health(111), Health::Critical);
        assert_eq!(runway_health(0, Some(4)), Health::Critical);
        assert_eq!(runway_health(0, Some(8)), Health::Warning);
        assert_eq!(runway_health(0, Some(12)), Health::Watch);
    }
    #[test]
    fn support_is_once_per_season_and_changes_budget_and_satisfaction_output() {
        let mut s = state();
        let c = context();
        let mut cash = -25_000;
        let p = request_board_support(&mut s, &mut cash, &c).unwrap();
        assert_eq!(p.support_amount, 175_000);
        assert_eq!(p.transfer_budget_reduction, 87_500);
        assert_eq!(p.satisfaction_penalty, 12);
        assert_eq!(cash, 150_000);
        assert_eq!(s.transfer_budget, 412_500);
        cash = -25_000;
        assert!(
            preview_board_support(&s, cash, &c)
                .unwrap_err()
                .contains("AlreadyUsed")
        );
        let mut next = c;
        next.season += 1;
        assert!(preview_board_support(&s, cash, &next).is_ok());
    }
    #[test]
    fn marketing_accounts_and_twenty_eight_day_cooldown() {
        let mut s = state();
        let mut c = context();
        let mut cash = -25_000;
        let p = request_marketing_campaign(&mut s, &mut cash, &c).unwrap();
        assert_eq!(p.gross_revenue, 250_000);
        assert_eq!(p.campaign_cost, 62_500);
        assert_eq!(cash, 162_500);
        assert_eq!(s.financial_ledger.len(), 2);
        assert_eq!(s.season_income, 250_000);
        assert_eq!(s.season_expenses, 62_500);
        cash = -25_000;
        c.today = "2026-03-15".parse().unwrap();
        assert_eq!(marketing_cooldown(&s, c.today), 1);
        assert!(preview_marketing_campaign(&s, cash, &c).is_err());
        c.today = "2026-03-16".parse().unwrap();
        assert_eq!(marketing_cooldown(&s, c.today), 0);
        assert!(preview_marketing_campaign(&s, cash, &c).is_ok());
    }
    #[test]
    fn sponsor_pitch_gates_then_acceptance_is_deferred_income() {
        let mut s = state();
        let mut c = context();
        let leader = preview_sponsor_pitch(&s, -25_000, &c).unwrap();
        c.current_position = Some(5);
        assert!(
            preview_sponsor_pitch(&s, -25_000, &c)
                .unwrap()
                .weekly_amount
                < leader.weekly_amount
        );
        c.pending_sponsor_offer = true;
        assert!(preview_sponsor_pitch(&s, -25_000, &c).is_err());
        c.pending_sponsor_offer = false;
        c.sponsor_pitch_attempted_today = true;
        assert!(preview_sponsor_pitch(&s, -25_000, &c).is_err());
        c.sponsor_pitch_attempted_today = false;
        s.sponsorship =
            Some(accepted_sponsorship(leader.sponsor_name, leader.weekly_amount as u64).unwrap());
        assert!(preview_sponsor_pitch(&s, -25_000, &c).is_err());
        assert_eq!(s.season_income, 0);
        assert_eq!(s.sponsorship.as_ref().unwrap().remaining_weeks, 12);
    }
    #[test]
    fn overflow_does_not_publish_partial_accounts() {
        let mut s = state();
        s.season_expenses = i64::MAX;
        let before = s.clone();
        let mut cash = 0;
        assert!(
            settle_weekly(
                &mut s,
                &mut cash,
                &context(),
                1,
                &mut rand::rngs::StdRng::seed_from_u64(1)
            )
            .is_err()
        );
        assert_eq!(s, before);
        assert_eq!(cash, 0);
        let mut s = state();
        s.season_income = i64::MAX;
        let before = s.clone();
        let mut cash = -10;
        assert!(request_marketing_campaign(&mut s, &mut cash, &context()).is_err());
        assert_eq!(s, before);
        assert_eq!(cash, -10);
    }

    #[test]
    fn matchday_count_is_completed_home_with_result_and_strict_seven_day_window() {
        use domain::league::{Fixture, FixtureCompetition, FixtureStatus, MatchResult};
        let fixture = |id: &str, day: &str| Fixture {
            id: id.into(),
            competition_id: "league".into(),
            matchday: 1,
            date: day.into(),
            home_team_id: "team1".into(),
            away_team_id: "other".into(),
            competition: FixtureCompetition::League,
            status: FixtureStatus::Completed,
            result: Some(MatchResult::default()),
        };
        let mut scheduled = fixture("scheduled", "2026-02-15");
        scheduled.status = FixtureStatus::Scheduled;
        let mut no_result = fixture("missing-result", "2026-02-15");
        no_result.result = None;
        let mut away = fixture("away", "2026-02-15");
        away.home_team_id = "other".into();
        away.away_team_id = "team1".into();
        let fixtures = vec![
            fixture("old", "2026-02-09"),
            fixture("inside", "2026-02-10"),
            fixture("today", "2026-02-16"),
            fixture("future", "2026-02-17"),
            fixture("invalid", "bad-date"),
            scheduled,
            no_result,
            away,
        ];
        assert_eq!(
            count_recent_home_matches(&fixtures, "team1", context().today),
            2
        );
    }

    #[test]
    fn bonus_criteria_and_warning_boundary_preserve_source_asymmetries() {
        let mut sponsor = accepted_sponsorship("Sponsor".into(), 1000).unwrap();
        sponsor
            .bonus_criteria
            .push(SponsorshipBonusCriterion::LeaguePosition {
                max_position: 4,
                bonus_amount: 300,
            });
        assert_eq!(
            sponsorship_bonus(Some(4), &["W".into(), "D".into(), "W".into()], &sponsor).unwrap(),
            550
        );
        assert_eq!(
            sponsorship_bonus(Some(5), &["W".into(), "L".into(), "W".into()], &sponsor).unwrap(),
            0
        );
        let s = state();
        let c = context();
        let p = snapshot(&s, 4000, &c).unwrap();
        assert_eq!(p.cash_runway_weeks, Some(4));
        assert_eq!(p.overall_status, Health::Critical);
        assert_eq!(satisfaction_penalty(&p), 4);
        assert!(warning(&s, 4000, &p).is_none());
        let p = snapshot(&s, 3999, &c).unwrap();
        assert!(matches!(
            warning(&s, 3999, &p),
            Some(Warning::Runway { weeks_left: 3, .. })
        ));
    }
}
