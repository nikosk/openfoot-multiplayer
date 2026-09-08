//! Pinned OpenFoot Manager 64677fee. Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
use super::{action, params};
use domain::message::*;
use rand::RngExt;

pub fn pre_match_message(
    fixture_id: &str,
    opponent_name: &str,
    opponent_id: &str,
    is_home: bool,
    matchday: u32,
    match_date: &str,
    date: &str,
    rng: &mut impl rand::Rng,
) -> InboxMessage {
    let idx = rng.random_range(0..2);
    let venue_short = if is_home { "H" } else { "A" };
    let body_key = format!(
        "be.msg.preMatch.body{}{}",
        idx,
        if is_home { "Home" } else { "Away" }
    );

    InboxMessage::new(
        format!("prematch_{}", fixture_id),
        String::new(),
        String::new(),
        String::new(),
        date.to_string(),
    )
    .with_category(MessageCategory::MatchPreview)
    .with_priority(MessagePriority::Normal)
    .with_sender_role("")
    .with_action(action(
        "set_tactics",
        "",
        "be.msg.preMatch.actionTactics",
        ActionType::NavigateTo {
            route: "/dashboard?tab=Tactics".to_string(),
        },
    ))
    .with_action(action(
        "view_opponent",
        "",
        "be.msg.preMatch.actionScout",
        ActionType::NavigateTo {
            route: format!("/team/{}", opponent_id),
        },
    ))
    .with_context(MessageContext {
        fixture_id: Some(fixture_id.to_string()),
        team_id: Some(opponent_id.to_string()),
        ..Default::default()
    })
    .with_i18n(
        "be.msg.preMatch.subject",
        &body_key,
        params(&[
            ("venue", venue_short),
            ("opponent", opponent_name),
            ("matchDate", match_date),
            ("matchday", &matchday.to_string()),
        ]),
    )
    .with_sender_i18n("be.sender.assistantManager", "be.role.assistantManager")
}
