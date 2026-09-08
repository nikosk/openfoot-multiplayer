//! Source-derived daily conversations (pinned 64677fee; GPL-3.0-or-later).
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
use chrono::NaiveDate;
use domain::message::InboxMessage;
use domain::player::{Player, PlayerIssue, PlayerIssueCategory, PlayerPromiseKind};
use rand::RngExt;

pub fn injury_message(
    event: &crate::availability::InjuryEvent,
    name: &str,
    date: NaiveDate,
    rng: &mut impl rand::Rng,
) -> InboxMessage {
    use domain::message::*;
    InboxMessage::new(
        event.id.clone(),
        String::new(),
        String::new(),
        String::new(),
        date.to_string(),
    )
    .with_category(MessageCategory::Injury)
    .with_priority(MessagePriority::High)
    .with_sender_role("")
    .with_action(MessageAction {
        id: "ack".into(),
        label: String::new(),
        label_key: Some("be.msg.event.ack".into()),
        action_type: ActionType::Acknowledge,
        resolved: false,
    })
    .with_context(MessageContext {
        player_id: Some(event.player_id.clone()),
        ..Default::default()
    })
    .with_i18n(
        "be.msg.trainingInjury.subject",
        &format!("be.msg.trainingInjury.body{}", rng.random_range(0..2)),
        [
            ("player".into(), name.into()),
            ("injury".into(), event.injury.name.clone()),
            ("days".into(), event.injury.days_remaining.to_string()),
        ]
        .into(),
    )
    .with_sender_i18n("be.sender.headPhysio", "be.role.headPhysio")
}

pub fn fitness_message(
    players: &[Player],
    club: &str,
    date: NaiveDate,
    training: &crate::training::ClubTraining,
    staff: &[domain::staff::Staff],
) -> Option<InboxMessage> {
    use domain::message::*;
    let available: Vec<_> = players
        .iter()
        .filter(|p| p.team_id.as_deref() == Some(club) && p.injury.is_none())
        .collect();
    let data: Vec<_> = available
        .iter()
        .map(|p| (p.id.as_str(), p.condition))
        .collect();
    let warning = crate::training::fitness_warning(&data)?;
    let physio = staff
        .iter()
        .find(|s| s.team_id.as_deref() == Some(club) && s.role == domain::staff::StaffRole::Physio);
    let sender = physio.or_else(|| {
        staff.iter().find(|s| {
            s.team_id.as_deref() == Some(club)
                && s.role == domain::staff::StaffRole::AssistantManager
        })
    });
    let name = sender
        .map(|s| format!("{} {}", s.first_name, s.last_name))
        .unwrap_or_default();
    let schedule = match training.schedule {
        crate::training::Schedule::Intense => "intense",
        crate::training::Schedule::Balanced => "balanced",
        crate::training::Schedule::Light => "light",
    };
    let kind = if warning.critical {
        "critical"
    } else {
        "warning"
    };
    let mut params = std::collections::HashMap::from([
        (
            "avgCondition".into(),
            format!("{:.0}", warning.average_condition),
        ),
        ("schedule".into(), schedule.into()),
    ]);
    if warning.critical {
        params.insert(
            "criticalCount".into(),
            available
                .iter()
                .filter(|p| p.condition < 25)
                .count()
                .to_string(),
        );
        params.insert(
            "players".into(),
            available
                .iter()
                .filter(|p| p.condition < 25)
                .take(5)
                .map(|p| format!("{} ({}%)", p.match_name, p.condition))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        params.insert(
            "intensity".into(),
            if training.intensity == crate::training::Intensity::High {
                "high"
            } else {
                ""
            }
            .into(),
        );
    } else {
        params.insert("exhaustedCount".into(), warning.exhausted_count.to_string());
    }
    Some(
        InboxMessage::new(
            format!("fitness_warn_{date}"),
            String::new(),
            String::new(),
            name,
            format!("{date}T00:00:00+00:00"),
        )
        .with_category(MessageCategory::Training)
        .with_priority(if warning.critical {
            MessagePriority::Urgent
        } else {
            MessagePriority::High
        })
        .with_sender_role("")
        .with_action(MessageAction {
            id: "go_training".into(),
            label: String::new(),
            label_key: Some("be.msg.fitness.actionAdjust".into()),
            resolved: false,
            action_type: ActionType::NavigateTo {
                route: "/dashboard?tab=Training".into(),
            },
        })
        .with_context(MessageContext {
            team_id: Some(club.into()),
            ..Default::default()
        })
        .with_i18n(
            &format!("be.msg.fitness.{kind}.subject"),
            &format!("be.msg.fitness.{kind}.body.{schedule}"),
            params,
        )
        .with_sender_i18n(
            if physio.is_some() {
                "be.sender.headPhysio"
            } else {
                "be.sender.assistantManager"
            },
            if physio.is_some() {
                "be.role.headPhysio"
            } else {
                "be.role.assistantManager"
            },
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};
    fn player(id: &str) -> Player {
        let mut p=Player::new(id.into(),id.into(),id.into(),"2000-01-01".into(),"ENG".into(),domain::player::Position::Midfielder,serde_json::from_value(serde_json::json!({"pace":50,"stamina":50,"strength":50,"passing":50,"shooting":50,"tackling":50,"dribbling":50,"defending":50,"positioning":50,"vision":50,"decisions":50,"composure":50,"leadership":50,"aggression":50})).unwrap());
        p.team_id = Some("a".into());
        p.morale = 20;
        p.ovr = 60;
        p
    }
    #[test]
    fn recurring_patch_blocks_pending_and_same_date_but_allows_later_date() {
        let mut rng = StdRng::seed_from_u64(1);
        let mut m =
            builders::low_morale_message("morale_talk_p", "p", "p", 20, "2026-06-01", &mut rng);
        assert_eq!(recurring_id(&[], &m.id, "2026-06-01"), Some(m.id.clone()));
        assert_eq!(recurring_id(&[m.clone()], &m.id, "2026-06-02"), None);
        m.actions.iter_mut().for_each(|a| a.resolved = true);
        assert_eq!(recurring_id(&[m.clone()], &m.id, "2026-06-01"), None);
        assert_eq!(
            recurring_id(&[m.clone()], &m.id, "2026-06-02"),
            Some("morale_talk_p_2026-06-02".into())
        );
    }
    #[test]
    fn daily_cap_scope_injury_and_contract_pressure_are_exact_and_deterministic() {
        let today = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        let mut players: Vec<_> = (0..30).map(|i| player(&format!("p{i:02}"))).collect();
        players[0].team_id = Some("b".into());
        players[1].injury = Some(domain::player::Injury {
            name: "test".into(),
            days_remaining: 2,
        });
        players[2].contract_end = Some("2026-06-30".into());
        let mut duplicate = players.clone();
        let output = generate(
            &mut players,
            "a",
            5,
            today,
            &[],
            &Default::default(),
            &mut StdRng::seed_from_u64(1001),
        );
        let other = generate(
            &mut duplicate,
            "a",
            5,
            today,
            &[],
            &Default::default(),
            &mut StdRng::seed_from_u64(1001),
        );
        assert_eq!(
            serde_json::to_value(&output).unwrap(),
            serde_json::to_value(other).unwrap()
        );
        assert_eq!(
            output
                .iter()
                .filter(|m| !m.id.starts_with("contract_concern_"))
                .count(),
            2
        );
        assert!(
            output
                .iter()
                .all(|m| m.context.player_id.as_deref() != Some("p00")
                    && m.context.player_id.as_deref() != Some("p01"))
        );
        assert_eq!(players[2].morale, 11);
        generate(
            &mut players,
            "a",
            5,
            today,
            &output,
            &Default::default(),
            &mut StdRng::seed_from_u64(1001),
        );
        assert_eq!(players[2].morale, 11);
    }
    #[test]
    fn actual_minutes_repair_broken_promise_without_refunding_trust() {
        let mut p = player("p");
        p.morale_core.manager_trust = 30;
        p.morale_core.unresolved_issue = Some(PlayerIssue {
            category: PlayerIssueCategory::PlayingTime,
            severity: 75,
        });
        let mut report = engine::MatchReport::from_events(vec![], 0, 0, 90);
        report.player_stats.insert("p".into(), Default::default());
        resolve_post_match_promises(std::slice::from_mut(&mut p), &report, "a", "b");
        assert!(p.morale_core.unresolved_issue.is_some());
        report.player_stats.get_mut("p").unwrap().minutes_played = 1;
        resolve_post_match_promises(std::slice::from_mut(&mut p), &report, "a", "b");
        assert!(p.morale_core.unresolved_issue.is_none());
        assert_eq!(p.morale_core.manager_trust, 30);
        p.morale_core.pending_promise = Some(domain::player::PlayerPromise {
            kind: PlayerPromiseKind::PlayingTime,
            matches_remaining: 1,
        });
        report.player_stats.clear();
        resolve_post_match_promises(std::slice::from_mut(&mut p), &report, "a", "b");
        assert_eq!(p.morale_core.manager_trust, 18);
        assert_eq!(p.morale_core.unresolved_issue.unwrap().severity, 75);
    }
}

// ---------------------------------------------------------------------------
// Post-match: feed engine report stats back into domain Player models
// ---------------------------------------------------------------------------

pub fn apply_player_stats(
    players: &mut [Player],
    report: &engine::MatchReport,
    home_team_id: &str,
    away_team_id: &str,
) -> Result<(), String> {
    for player in players.iter_mut() {
        if let Some(ps) = report.player_stats.get(&player.id) {
            player.stats.appearances = player
                .stats
                .appearances
                .checked_add(1)
                .ok_or("Source player stats overflow")?;
            player.stats.goals = player
                .stats
                .goals
                .checked_add(ps.goals as u32)
                .ok_or("Source player stats overflow")?;
            player.stats.assists = player
                .stats
                .assists
                .checked_add(ps.assists as u32)
                .ok_or("Source player stats overflow")?;
            player.stats.yellow_cards = player
                .stats
                .yellow_cards
                .checked_add(ps.yellow_cards as u32)
                .ok_or("Source player stats overflow")?;
            player.stats.red_cards = player
                .stats
                .red_cards
                .checked_add(ps.red_cards as u32)
                .ok_or("Source player stats overflow")?;
            player.stats.minutes_played = player
                .stats
                .minutes_played
                .checked_add(ps.minutes_played as u32)
                .ok_or("Source player stats overflow")?;
            player.stats.shots = player
                .stats
                .shots
                .checked_add(ps.shots as u32)
                .ok_or("Source player stats overflow")?;
            player.stats.shots_on_target = player
                .stats
                .shots_on_target
                .checked_add(ps.shots_on_target as u32)
                .ok_or("Source player stats overflow")?;
            player.stats.passes_completed = player
                .stats
                .passes_completed
                .checked_add(ps.passes_completed as u32)
                .ok_or("Source player stats overflow")?;
            player.stats.passes_attempted = player
                .stats
                .passes_attempted
                .checked_add(ps.passes_attempted as u32)
                .ok_or("Source player stats overflow")?;
            player.stats.tackles_won = player
                .stats
                .tackles_won
                .checked_add(ps.tackles_won as u32)
                .ok_or("Source player stats overflow")?;
            player.stats.interceptions = player
                .stats
                .interceptions
                .checked_add(ps.interceptions as u32)
                .ok_or("Source player stats overflow")?;
            player.stats.fouls_committed = player
                .stats
                .fouls_committed
                .checked_add(ps.fouls_committed as u32)
                .ok_or("Source player stats overflow")?;

            // Update average rating (running average)
            if player.stats.appearances == 1 {
                player.stats.avg_rating = ps.rating;
            } else {
                let n = player.stats.appearances as f32;
                player.stats.avg_rating = (player.stats.avg_rating * (n - 1.0) + ps.rating) / n;
            }

            // Clean sheet for goalkeepers
            if matches!(player.position, domain::player::Position::Goalkeeper) {
                let tid = player.team_id.as_deref().unwrap_or("");
                let conceded_zero = if tid == home_team_id {
                    report.away_goals == 0
                } else if tid == away_team_id {
                    report.home_goals == 0
                } else {
                    false
                };
                if conceded_zero {
                    player.stats.clean_sheets = player
                        .stats
                        .clean_sheets
                        .checked_add(1)
                        .ok_or("Source player stats overflow")?;
                }
            }
        }
    }
    Ok(())
}

// Approved career-runtime-v1 patch: resolved low-morale/bench conversations may
// recur on later dates, never alongside another pending choice for that family.
fn recurring_id(messages: &[InboxMessage], base: &str, today: &str) -> Option<String> {
    let prefix = format!("{base}_");
    let prior: Vec<_> = messages
        .iter()
        .filter(|m| m.id == base || m.id.starts_with(&prefix))
        .collect();
    if prior
        .iter()
        .any(|m| m.date == today || m.actions.iter().any(|a| !a.resolved))
    {
        return None;
    }
    Some(if prior.is_empty() {
        base.into()
    } else {
        format!("{base}_{today}")
    })
}

pub fn generate(
    players: &mut [Player],
    club: &str,
    played: usize,
    today: NaiveDate,
    messages: &[InboxMessage],
    suppressed_contracts: &std::collections::BTreeSet<String>,
    rng: &mut impl rand::Rng,
) -> Vec<InboxMessage> {
    let date = today.to_string();
    let mut output = Vec::new();
    let existing_count = messages
        .iter()
        .filter(|m| {
            m.date == date
                && ["morale_talk_", "bench_complaint_", "happy_player_"]
                    .iter()
                    .any(|p| m.id.starts_with(p))
        })
        .count();
    for family in 0..3 {
        for player in players.iter() {
            if existing_count + output.len() >= 2 {
                break;
            }
            if player.team_id.as_deref() != Some(club)
                || player.morale_core.talk_cooldown_until.as_deref() == Some(&date)
            {
                continue;
            }
            if family < 2 && player.injury.is_some() {
                continue;
            }
            let prefix = ["morale_talk", "bench_complaint", "happy_player"][family];
            let base = format!("{prefix}_{}", player.id);
            let id = if family < 2 {
                let Some(id) = recurring_id(messages, &base, &date) else {
                    continue;
                };
                id
            } else {
                if messages.iter().any(|m| m.id == base) {
                    continue;
                }
                base
            };
            let message = match family {
                0 if player.morale < 30 && rng.random_range(0..5) == 0 => {
                    Some(builders::low_morale_message(
                        &id,
                        &player.id,
                        &player.match_name,
                        player.morale,
                        &date,
                        rng,
                    ))
                }
                1 if played >= 5
                    && player.position != domain::player::Position::Goalkeeper
                    && player.ovr >= 55
                    && player.morale < 50
                    && (player.stats.appearances as f64 / played as f64) < 0.3
                    && rng.random_range(0..10) == 0 =>
                {
                    Some(builders::bench_complaint_message(
                        &id,
                        &player.id,
                        &player.match_name,
                        &date,
                        rng,
                    ))
                }
                2 if player.morale >= 90 && rng.random_range(0..100) == 0 => Some(
                    builders::happy_player_message(&id, &player.id, &player.match_name, &date, rng),
                ),
                _ => None,
            };
            if let Some(message) = message {
                output.push(message);
            }
        }
    }
    // Contract concern stages are outside the two-conversation cap.
    for player in players
        .iter_mut()
        .filter(|p| p.team_id.as_deref() == Some(club))
    {
        if suppressed_contracts.contains(&player.id) {
            continue;
        }
        if player.morale_core.renewal_state.as_ref().is_some_and(|s| {
            matches!(
                s.exit_intent,
                Some(domain::player::ContractExitIntent::LetExpire { .. })
            )
        }) {
            continue;
        }
        let Some(end) = player
            .contract_end
            .as_ref()
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        else {
            continue;
        };
        let days = (end - today).num_days();
        let (suffix, pressure) = match days {
            1..=30 => ("final", 9),
            31..=90 => ("3m", 6),
            91..=180 => ("6m", 4),
            _ => continue,
        };
        let id = format!("contract_concern_{}_{}", player.id, suffix);
        if messages.iter().any(|m| m.id == id) {
            continue;
        }
        player.morale = (i16::from(player.morale) - pressure).clamp(5, 100) as u8;
        output.push(builders::contract_concern_message(
            &id,
            &player.id,
            &player.match_name,
            days,
            &date,
            rng,
        ));
    }
    output
}

pub fn apply_streak(players: &mut [Player], club: &str, form: &[String], rng: &mut impl rand::Rng) {
    if form.len() < 3 {
        return;
    }
    let last: Vec<_> = form.iter().rev().take(3).collect();
    let delta = if last.iter().all(|s| s.as_str() == "W") {
        rng.random_range(2..=5)
    } else if last.iter().all(|s| s.as_str() == "L") {
        rng.random_range(-10..=-5)
    } else {
        0
    };
    if delta != 0 {
        for p in players
            .iter_mut()
            .filter(|p| p.team_id.as_deref() == Some(club))
        {
            p.morale =
                (i16::from(p.morale) + capped_positive_recovery(delta, p)).clamp(10, 100) as u8;
        }
    }
}
pub fn resolve_post_match_promises(
    players: &mut [Player],
    report: &engine::MatchReport,
    home_team_id: &str,
    away_team_id: &str,
) {
    for player in players.iter_mut() {
        let Some(team_id) = player.team_id.as_deref() else {
            continue;
        };
        if team_id != home_team_id && team_id != away_team_id {
            continue;
        }

        let played = report
            .player_stats
            .get(&player.id)
            .is_some_and(|stats| stats.minutes_played > 0);
        // Approved career-runtime-v1 fix: actual minutes repair a playing-time
        // grievance, but never refund the broken promise trust loss.
        if played
            && player
                .morale_core
                .unresolved_issue
                .as_ref()
                .is_some_and(|issue| issue.category == PlayerIssueCategory::PlayingTime)
        {
            player.morale_core.unresolved_issue = None;
        }
        let Some(promise) = player.morale_core.pending_promise.clone() else {
            continue;
        };

        match promise.kind {
            PlayerPromiseKind::PlayingTime => {
                if played {
                    player.morale_core.pending_promise = None;
                    player.morale_core.manager_trust =
                        (i16::from(player.morale_core.manager_trust) + 3).clamp(0, 100) as u8;

                    if player
                        .morale_core
                        .unresolved_issue
                        .as_ref()
                        .is_some_and(|issue| issue.category == PlayerIssueCategory::PlayingTime)
                    {
                        player.morale_core.unresolved_issue = None;
                    }
                } else if promise.matches_remaining <= 1 {
                    player.morale_core.pending_promise = None;
                    player.morale_core.manager_trust =
                        (i16::from(player.morale_core.manager_trust) - 12).clamp(0, 100) as u8;
                    player.morale_core.unresolved_issue = Some(PlayerIssue {
                        category: PlayerIssueCategory::PlayingTime,
                        severity: 75,
                    });
                } else {
                    player.morale_core.pending_promise = Some(domain::player::PlayerPromise {
                        kind: PlayerPromiseKind::PlayingTime,
                        matches_remaining: promise.matches_remaining - 1,
                    });
                }
            }
        }
    }
}

fn capped_positive_recovery(delta: i16, player: &domain::player::Player) -> i16 {
    let Some(issue) = player.morale_core.unresolved_issue.as_ref() else {
        return delta;
    };

    if delta <= 0 {
        return delta;
    }

    if issue.severity >= 75 {
        return 0;
    }

    if issue.severity >= 50 {
        return ((delta + 1) / 2).max(1);
    }

    delta
}

/// Update player morale based on match result and individual performance.
pub fn update_post_match_morale(
    players: &mut [Player],
    rng: &mut impl rand::Rng,
    report: &engine::MatchReport,
    home_team_id: &str,
    away_team_id: &str,
) {
    use rand::RngExt;

    let home_won = report.home_goals > report.away_goals;
    let away_won = report.away_goals > report.home_goals;
    let is_draw = report.home_goals == report.away_goals;

    for player in players.iter_mut() {
        let tid = match player.team_id.as_deref() {
            Some(t) if t == home_team_id || t == away_team_id => t.to_string(),
            _ => continue,
        };

        let is_home = tid == home_team_id;
        let base_morale = player.morale as i16;

        // Team result effect — scale loss impact by goal difference
        let goal_diff = (report.home_goals as i16 - report.away_goals as i16).abs();
        let result_delta: i16 = if (is_home && home_won) || (!is_home && away_won) {
            rng.random_range(3..=8) // Win boost
        } else if is_draw {
            rng.random_range(-2..=3) // Draw: mild
        } else {
            // Base loss: -5 to -2, plus extra -3 per goal margin beyond 1
            let base_loss = rng.random_range(-5..=-2);
            let margin_penalty = (goal_diff - 1).max(0) * -3;
            base_loss + margin_penalty // e.g. 3-0 loss → -5..-2 + -6 = -11..-8
        };

        // Individual performance effect
        let mut individual_delta: i16 = 0;
        if let Some(ps) = report.player_stats.get(&player.id) {
            // Goals scored boost morale
            individual_delta += ps.goals as i16 * 3;
            // Assists boost morale
            individual_delta += ps.assists as i16 * 2;
            // Red card tanks morale
            if ps.red_cards > 0 {
                individual_delta -= 8;
            }
            // Poor rating lowers morale
            if ps.rating < 5.5 {
                individual_delta -= 3;
            } else if ps.rating > 7.5 {
                individual_delta += 2;
            }
        }

        let total_delta = capped_positive_recovery(result_delta + individual_delta, player);
        let new_morale = (base_morale + total_delta).clamp(10, 100) as u8;
        player.morale = new_morale;
    }
}

mod builders {
    use domain::message::*;
    use rand::RngExt;
    use std::collections::HashMap;

    /// Helper to build a HashMap<String, String> from key-value pairs.
    fn params(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn action(id: &str, label_key: &str, action_type: ActionType) -> MessageAction {
        MessageAction {
            id: id.to_string(),
            label: String::new(),
            action_type,
            resolved: false,
            label_key: Some(label_key.to_string()),
        }
    }

    fn option(id: &str, label_key: &str, description_key: &str) -> ActionOption {
        ActionOption {
            id: id.to_string(),
            label: String::new(),
            description: String::new(),
            label_key: Some(label_key.to_string()),
            description_key: Some(description_key.to_string()),
        }
    }

    pub(crate) fn low_morale_message(
        msg_id: &str,
        player_id: &str,
        player_name: &str,
        morale: u8,
        date: &str,
        rng: &mut impl rand::Rng,
    ) -> InboxMessage {
        let idx = rng.random_range(0..2);

        InboxMessage::new(
            msg_id.to_string(),
            String::new(),
            String::new(),
            String::new(),
            date.to_string(),
        )
        .with_category(MessageCategory::PlayerMorale)
        .with_priority(MessagePriority::High)
        .with_sender_role("")
        .with_action(action(
            "respond",
            "be.msg.playerEvent.respond",
            ActionType::ChooseOption {
                options: vec![
                    option(
                        "encourage",
                        "be.msg.playerEvent.options.moraleCrisis.encourage.label",
                        "be.msg.playerEvent.options.moraleCrisis.encourage.description",
                    ),
                    option(
                        "promise_time",
                        "be.msg.playerEvent.options.moraleCrisis.promiseTime.label",
                        "be.msg.playerEvent.options.moraleCrisis.promiseTime.description",
                    ),
                    option(
                        "work_harder",
                        "be.msg.playerEvent.options.moraleCrisis.workHarder.label",
                        "be.msg.playerEvent.options.moraleCrisis.workHarder.description",
                    ),
                ],
            },
        ))
        .with_context(MessageContext {
            player_id: Some(player_id.to_string()),
            ..Default::default()
        })
        .with_i18n(
            "be.msg.moraleCrisis.subject",
            &format!("be.msg.moraleCrisis.body{}", idx),
            params(&[("player", player_name), ("morale", &morale.to_string())]),
        )
        .with_sender_i18n("be.sender.player", "be.role.player")
    }

    pub(crate) fn bench_complaint_message(
        msg_id: &str,
        player_id: &str,
        player_name: &str,
        date: &str,
        rng: &mut impl rand::Rng,
    ) -> InboxMessage {
        let idx = rng.random_range(0..2);

        InboxMessage::new(
            msg_id.to_string(),
            String::new(),
            String::new(),
            String::new(),
            date.to_string(),
        )
        .with_category(MessageCategory::PlayerMorale)
        .with_priority(MessagePriority::Normal)
        .with_sender_role("")
        .with_action(action(
            "respond",
            "be.msg.playerEvent.respond",
            ActionType::ChooseOption {
                options: vec![
                    option(
                        "explain",
                        "be.msg.playerEvent.options.benchComplaint.explain.label",
                        "be.msg.playerEvent.options.benchComplaint.explain.description",
                    ),
                    option(
                        "promise_chance",
                        "be.msg.playerEvent.options.benchComplaint.promiseChance.label",
                        "be.msg.playerEvent.options.benchComplaint.promiseChance.description",
                    ),
                    option(
                        "prove_yourself",
                        "be.msg.playerEvent.options.benchComplaint.proveYourself.label",
                        "be.msg.playerEvent.options.benchComplaint.proveYourself.description",
                    ),
                ],
            },
        ))
        .with_context(MessageContext {
            player_id: Some(player_id.to_string()),
            ..Default::default()
        })
        .with_i18n(
            "be.msg.benchComplaint.subject",
            &format!("be.msg.benchComplaint.body{}", idx),
            params(&[("player", player_name)]),
        )
        .with_sender_i18n("be.sender.player", "be.role.player")
    }

    pub(crate) fn happy_player_message(
        msg_id: &str,
        player_id: &str,
        player_name: &str,
        date: &str,
        rng: &mut impl rand::Rng,
    ) -> InboxMessage {
        let idx = rng.random_range(0..2);

        InboxMessage::new(
            msg_id.to_string(),
            String::new(),
            String::new(),
            String::new(),
            date.to_string(),
        )
        .with_category(MessageCategory::PlayerMorale)
        .with_priority(MessagePriority::Low)
        .with_sender_role("")
        .with_action(action(
            "respond",
            "be.msg.playerEvent.respond",
            ActionType::ChooseOption {
                options: vec![
                    option(
                        "praise_back",
                        "be.msg.playerEvent.options.happyPlayer.praiseBack.label",
                        "be.msg.playerEvent.options.happyPlayer.praiseBack.description",
                    ),
                    option(
                        "stay_professional",
                        "be.msg.playerEvent.options.happyPlayer.stayProfessional.label",
                        "be.msg.playerEvent.options.happyPlayer.stayProfessional.description",
                    ),
                    option(
                        "higher_expectations",
                        "be.msg.playerEvent.options.happyPlayer.higherExpectations.label",
                        "be.msg.playerEvent.options.happyPlayer.higherExpectations.description",
                    ),
                ],
            },
        ))
        .with_context(MessageContext {
            player_id: Some(player_id.to_string()),
            ..Default::default()
        })
        .with_i18n(
            "be.msg.happyPlayer.subject",
            &format!("be.msg.happyPlayer.body{}", idx),
            params(&[("player", player_name)]),
        )
        .with_sender_i18n("be.sender.player", "be.role.player")
    }

    pub(crate) fn contract_concern_message(
        msg_id: &str,
        player_id: &str,
        player_name: &str,
        days_remaining: i64,
        date: &str,
        rng: &mut impl rand::Rng,
    ) -> InboxMessage {
        let months = (days_remaining as f64 / 30.0).ceil() as u32;
        let idx = rng.random_range(0..2);

        InboxMessage::new(
            msg_id.to_string(),
            String::new(),
            String::new(),
            String::new(),
            date.to_string(),
        )
        .with_category(MessageCategory::Contract)
        .with_priority(MessagePriority::High)
        .with_sender_role("")
        .with_action(action(
            "respond",
            "be.msg.playerEvent.respond",
            ActionType::ChooseOption {
                options: vec![
                    option(
                        "reassure",
                        "be.msg.playerEvent.options.contractConcern.reassure.label",
                        "be.msg.playerEvent.options.contractConcern.reassure.description",
                    ),
                    option(
                        "noncommittal",
                        "be.msg.playerEvent.options.contractConcern.noncommittal.label",
                        "be.msg.playerEvent.options.contractConcern.noncommittal.description",
                    ),
                    option(
                        "no_renewal",
                        "be.msg.playerEvent.options.contractConcern.noRenewal.label",
                        "be.msg.playerEvent.options.contractConcern.noRenewal.description",
                    ),
                ],
            },
        ))
        .with_context(MessageContext {
            player_id: Some(player_id.to_string()),
            ..Default::default()
        })
        .with_i18n(
            "be.msg.contractConcern.subject",
            &format!("be.msg.contractConcern.body{}", idx),
            params(&[
                ("player", player_name),
                ("days", &days_remaining.to_string()),
                ("months", &months.to_string()),
            ]),
        )
        .with_sender_i18n("be.sender.assistantManager", "be.role.assistantManager")
    }
}
