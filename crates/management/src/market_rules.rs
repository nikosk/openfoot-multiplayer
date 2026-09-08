//! Pure rules from pinned 64677fee transfer market; shared bot policies may
//! recommend these terms, but never gain automatic owner consent.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
use chrono::{Days, Months, NaiveDate};
use domain::season::{TransferWindowContext, TransferWindowStatus};
use domain::{player::Player, team::Team};
enum PlayerImportance {
    Key,
    Regular,
    Fringe,
}
fn contract_days_remaining(current_date: NaiveDate, contract_end: Option<&str>) -> Option<i64> {
    let contract_end = contract_end?;
    let contract_end_date = NaiveDate::parse_from_str(contract_end, "%Y-%m-%d").ok()?;
    Some((contract_end_date - current_date).num_days())
}

fn infer_player_importance(
    player: &domain::player::Player,
    owner_team: &domain::team::Team,
) -> PlayerImportance {
    if owner_team.starting_xi_ids.iter().any(|id| id == &player.id) {
        return PlayerImportance::Key;
    }

    if player.market_value >= 1_500_000 {
        return PlayerImportance::Regular;
    }

    PlayerImportance::Fringe
}

pub fn minimum_acceptable_fee(
    current_date: NaiveDate,
    player: &domain::player::Player,
    owner_team: &domain::team::Team,
    buyer_team: &domain::team::Team,
) -> u64 {
    let mut multiplier: f64 = if player.transfer_listed { 0.8 } else { 1.2 };

    if let Some(days_remaining) =
        contract_days_remaining(current_date, player.contract_end.as_deref())
    {
        if days_remaining <= 60 {
            multiplier -= 0.25;
        } else if days_remaining <= 180 {
            multiplier -= 0.15;
        } else if days_remaining <= 365 {
            multiplier -= 0.05;
        }
    }

    match infer_player_importance(player, owner_team) {
        PlayerImportance::Key => multiplier += 0.2,
        PlayerImportance::Regular => multiplier += 0.1,
        PlayerImportance::Fringe => {}
    }

    if player.morale <= 40 {
        multiplier -= 0.05;
    }

    let openness_score = player_move_openness_score(current_date, player, owner_team, buyer_team);
    if openness_score >= 60 {
        multiplier -= 0.20;
    } else if openness_score >= 40 {
        multiplier -= 0.10;
    }

    let multiplier = multiplier.clamp(0.55, 1.6);
    ((player.market_value as f64) * multiplier).round() as u64
}

pub fn player_move_openness_score(
    current_date: NaiveDate,
    player: &domain::player::Player,
    owner_team: &domain::team::Team,
    buyer_team: &domain::team::Team,
) -> i32 {
    let mut score = 0;

    if player.morale <= 45 {
        score += 20;
    } else if player.morale <= 60 {
        score += 10;
    }

    if player.stats.appearances <= 2 {
        score += 15;
    } else if player.stats.appearances <= 5 {
        score += 8;
    }

    if let Some(days_remaining) =
        contract_days_remaining(current_date, player.contract_end.as_deref())
    {
        if days_remaining <= 180 {
            score += 20;
        } else if days_remaining <= 365 {
            score += 10;
        }
    }

    let reputation_gap = buyer_team.reputation as i32 - owner_team.reputation as i32;
    if reputation_gap >= 200 {
        score += 25;
    } else if reputation_gap >= 75 {
        score += 15;
    }

    if player.transfer_listed {
        score += 10;
    }

    score
}

pub fn apply_blocked_move_consequences(player: &mut domain::player::Player, openness_score: i32) {
    if openness_score < 40 {
        return;
    }

    let morale_drop = if openness_score >= 60 { 10 } else { 6 };
    player.morale = (i16::from(player.morale) - morale_drop).clamp(0, 100) as u8;
    player.morale_core.manager_trust =
        (i16::from(player.morale_core.manager_trust) - 5).clamp(0, 100) as u8;
    player.morale_core.unresolved_issue = Some(domain::player::PlayerIssue {
        category: domain::player::PlayerIssueCategory::Contract,
        severity: if openness_score >= 60 { 75 } else { 60 },
    });
}

pub fn minimum_loan_wage_contribution_pct(
    player: &domain::player::Player,
    owner_team: &domain::team::Team,
) -> u8 {
    if owner_team.starting_xi_ids.iter().any(|id| id == &player.id) {
        return 90;
    }

    if player.stats.appearances <= 5 || player.potential >= player.ovr.saturating_add(8) {
        50
    } else if player.ovr >= 72 {
        75
    } else {
        60
    }
}

pub fn minimum_loan_buy_option_fee(
    player: &domain::player::Player,
    owner_team: &domain::team::Team,
) -> u64 {
    let mut multiplier: f64 = if player.loan_listed { 1.0 } else { 1.2 };

    match infer_player_importance(player, owner_team) {
        PlayerImportance::Key => multiplier += 0.25,
        PlayerImportance::Regular => multiplier += 0.1,
        PlayerImportance::Fringe => multiplier -= 0.05,
    }

    if player.potential >= player.ovr.saturating_add(10) {
        multiplier += 0.2;
    }

    if player.stats.appearances <= 3 {
        multiplier -= 0.05;
    }

    round_transfer_fee(((player.market_value as f64) * multiplier.clamp(0.85, 1.65)).round() as u64)
}

pub fn acceptable_loan_buy_option(
    player: &domain::player::Player,
    owner_team: &domain::team::Team,
    buy_option_fee: Option<u64>,
) -> bool {
    buy_option_fee
        .map(|fee| fee >= minimum_loan_buy_option_fee(player, owner_team))
        .unwrap_or(true)
}

pub fn loan_borrower_wage_ceiling(
    player: &domain::player::Player,
    borrower_team: &domain::team::Team,
    offer: &domain::player::LoanOffer,
) -> u8 {
    let mut ceiling = i16::from(offer.wage_contribution_pct);

    if player.potential >= player.ovr.saturating_add(10) {
        ceiling += 30;
    } else if player.ovr >= 72 {
        ceiling += 24;
    } else if player.potential >= player.ovr.saturating_add(6) {
        ceiling += 20;
    } else {
        ceiling += 14;
    }

    if borrower_team.finance >= 5_000_000 {
        ceiling += 8;
    }

    if player.wage <= 750_000 {
        ceiling += 6;
    }

    ceiling.clamp(i16::from(offer.wage_contribution_pct), 100) as u8
}

pub fn loan_borrower_buy_option_ceiling(player: &domain::player::Player) -> u64 {
    let multiplier = if player.potential >= player.ovr.saturating_add(10) {
        1.4
    } else if player.ovr >= 72 {
        1.25
    } else {
        1.15
    };

    round_transfer_fee(((player.market_value as f64) * multiplier).round() as u64)
}

pub fn round_transfer_fee(value: u64) -> u64 {
    value.div_ceil(50_000).saturating_mul(50_000)
}

pub fn window(
    today: NaiveDate,
    season_start: Option<NaiveDate>,
) -> Result<TransferWindowContext, String> {
    let Some(mut anchor) = season_start else {
        return Ok(TransferWindowContext::default());
    };
    let (opens, closes) = loop {
        let opens = anchor
            .checked_sub_days(Days::new(30))
            .ok_or("Transfer window date underflow")?;
        let closes = anchor
            .checked_add_days(Days::new(30))
            .ok_or("Transfer window date overflow")?;
        if today <= closes {
            break (opens, closes);
        }
        anchor = anchor
            .checked_add_months(Months::new(12))
            .ok_or("Transfer window year overflow")?;
    };
    let (status, until, remaining) = if today < opens {
        (
            TransferWindowStatus::Closed,
            Some((opens - today).num_days()),
            None,
        )
    } else {
        let remaining = (closes - today).num_days();
        (
            if remaining == 0 {
                TransferWindowStatus::DeadlineDay
            } else {
                TransferWindowStatus::Open
            },
            None,
            Some(remaining),
        )
    };
    Ok(TransferWindowContext {
        status,
        opens_on: Some(opens.to_string()),
        closes_on: Some(closes.to_string()),
        days_until_opens: until,
        days_remaining: remaining,
    })
}
pub fn registration_date(
    today: NaiveDate,
    window: &TransferWindowContext,
) -> Result<NaiveDate, String> {
    if matches!(
        window.status,
        TransferWindowStatus::Open | TransferWindowStatus::DeadlineDay
    ) {
        return Ok(today);
    }
    window
        .opens_on
        .as_ref()
        .and_then(|s| s.parse::<NaiveDate>().ok())
        .filter(|d| *d > today)
        .ok_or_else(|| "be.error.transfers.transferWindowClosed".into())
}
pub fn validate_loan_dates(
    player: &Player,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<(), String> {
    if !(30..=370).contains(&(end - start).num_days())
        || player
            .contract_end
            .as_ref()
            .is_some_and(|s| s.parse::<NaiveDate>().map_or(true, |date| end >= date))
    {
        return Err("be.error.transfers.invalidLoanEndDate".into());
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BidRecommendation {
    Accept,
    Counter { fee: u64 },
    Reject { fee: u64 },
}
/// Source seller recommendation, not a substitute for a participant's consent.
pub fn recommend_bid(
    today: NaiveDate,
    player: &Player,
    seller: &Team,
    buyer: &Team,
    fee: u64,
    previous: Option<&domain::player::TransferOffer>,
) -> BidRecommendation {
    let threshold = minimum_acceptable_fee(today, player, seller, buyer);
    let round = previous
        .map(|o| o.negotiation_round.max(1).saturating_add(1))
        .unwrap_or(1);
    let respected = previous
        .and_then(|o| o.suggested_counter_fee)
        .is_some_and(|counter| fee >= counter.saturating_mul(95) / 100);
    let stalled = previous.is_some_and(|o| fee <= o.fee.saturating_add(50_000));
    let concession = if respected {
        (threshold as f64 * 0.04).round() as u64
    } else if round >= 3 && !stalled {
        (threshold as f64 * 0.02).round() as u64
    } else {
        0
    };
    let threshold = threshold.saturating_sub(concession);
    let floor = (threshold as f64
        * if round >= 2 && stalled {
            0.94
        } else if round >= 3 {
            0.92
        } else {
            0.88
        })
    .round() as u64;
    if fee >= threshold {
        BidRecommendation::Accept
    } else if fee >= floor {
        BidRecommendation::Counter {
            fee: round_transfer_fee(threshold),
        }
    } else {
        BidRecommendation::Reject {
            fee: round_transfer_fee(threshold),
        }
    }
}
