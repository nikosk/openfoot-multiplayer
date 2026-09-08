//! Pinned OpenFoot Manager match-result template, GPL-3.0-or-later.
use super::{action, params};
use domain::message::*;
use rand::RngExt;
pub fn match_result_message(
    fixture_id: &str,
    home_name: &str,
    away_name: &str,
    home_goals: u8,
    away_goals: u8,
    home_team_id: &str,
    away_team_id: &str,
    user_team_id: &str,
    matchday: u32,
    date: &str,
    rng: &mut impl rand::Rng,
) -> InboxMessage {
    let is_home = home_team_id == user_team_id;
    let user_goals = if is_home { home_goals } else { away_goals };
    let opp_goals = if is_home { away_goals } else { home_goals };

    let outcome = if user_goals > opp_goals {
        "Victory"
    } else if user_goals < opp_goals {
        "Defeat"
    } else {
        "Draw"
    };

    let body_key = format!(
        "be.msg.matchResult.body.{}{}",
        outcome.to_lowercase(),
        if outcome == "Draw" {
            String::new()
        } else {
            rng.random_range(0..2u8).to_string()
        }
    );

    InboxMessage::new(
        format!("result_{}", fixture_id),
        String::new(),
        String::new(),
        String::new(),
        date.to_string(),
    )
    .with_category(MessageCategory::MatchResult)
    .with_priority(if outcome == "Victory" {
        MessagePriority::Normal
    } else {
        MessagePriority::High
    })
    .with_sender_role("")
    .with_action(action(
        "view_standings",
        "",
        "be.msg.matchResult.actionStandings",
        ActionType::NavigateTo {
            route: "/dashboard?tab=Schedule".to_string(),
        },
    ))
    .with_context(MessageContext {
        fixture_id: Some(fixture_id.to_string()),
        match_result: Some(ContextMatchResult {
            home_team_id: home_team_id.to_string(),
            away_team_id: away_team_id.to_string(),
            home_team_name: home_name.to_string(),
            away_team_name: away_name.to_string(),
            home_goals,
            away_goals,
        }),
        ..Default::default()
    })
    .with_i18n(
        &format!("be.msg.matchResult.subject.{}", outcome.to_lowercase()),
        &body_key,
        {
            let mut p = params(&[
                ("home", home_name),
                ("away", away_name),
                ("homeGoals", &home_goals.to_string()),
                ("awayGoals", &away_goals.to_string()),
                ("matchday", &matchday.to_string()),
            ]);
            p.insert("outcome".to_string(), outcome.to_string());
            p
        },
    )
    .with_sender_i18n("be.sender.matchReporter", "be.role.pressOfficer")
}
