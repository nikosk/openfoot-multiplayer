//! Recipient-owned inbox foundation using the unmodified pinned domain message
//! model. Storage/read/cleanup semantics follow `src-tauri/src/commands/messages.rs`;
//! conversations below adapt `ofm_core/src/player_events/responses.rs`.
//! Upstream revision: 64677fee9047a1182005d666bafa5dbc025dca5c.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! A message belongs to its manager recipient, never to `context.team_id`: context
//! may identify another club. The Management dispatcher MUST authenticate current
//! employment before any operation and enforce Ready rules on consequential choices.
//! Domain effects and this store must commit together in that dispatcher's staged
//! transaction. No universal TTL exists in the source; owning domains explicitly
//! expire actions. The source's 14-day cleanup rule is retention, not action expiry.
//! Resolutions and deleted-message tombstones are retained privately to prevent
//! duplicate effects when a host retries delivery or an action with a fresh request.

use chrono::NaiveDate;

/// Source response formulas, with Game/global RNG replaced by scoped arguments.
pub mod conversations {
    use chrono::{Days, NaiveDate};
    use domain::player::{
        ContractExitIntent, ContractRenewalState, Player, PlayerPromise, PlayerPromiseKind,
        RecentTreatmentMemory, RenewalSessionOutcome, RenewalSessionStatus,
    };
    use rand::RngExt;
    use serde::Serialize;
    use std::collections::HashMap;

    /// Personality factor derived from player attributes. Affects how they react.
    /// Returns a value from -20 to +20, where positive = more receptive, negative = more volatile.
    fn personality_factor(player: &domain::player::Player) -> i8 {
        let composure = player.attributes.composure as i16;
        let leadership = player.attributes.leadership as i16;
        let aggression = player.attributes.aggression as i16;
        // Composed leaders are receptive; aggressive low-composure players are volatile
        ((composure + leadership - aggression) / 6).clamp(-20, 20) as i8
    }

    #[derive(Debug, Clone, Serialize)]
    pub struct PlayerResponseEffect {
        pub message: String,
        pub i18n_key: String,
        pub i18n_params: HashMap<String, String>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ResponseOutcomeBand {
        StrongPositive,
        MildPositive,
        Neutral,
        MildNegative,
        StrongNegative,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ResponseBandWeights {
        pub strong_positive: u32,
        pub mild_positive: u32,
        pub neutral: u32,
        pub mild_negative: u32,
        pub strong_negative: u32,
    }

    impl ResponseBandWeights {
        fn total(self) -> u32 {
            self.strong_positive
                + self.mild_positive
                + self.neutral
                + self.mild_negative
                + self.strong_negative
        }
    }

    struct ResponseOutcome {
        delta: i8,
        effect_key: String,
        i18n_params: HashMap<String, String>,
    }

    fn signed_delta(delta: i8) -> String {
        if delta >= 0 {
            format!("+{}", delta)
        } else {
            delta.to_string()
        }
    }

    fn base_effect_params(delta: i8) -> HashMap<String, String> {
        HashMap::from([("delta".to_string(), signed_delta(delta))])
    }

    fn outcome(delta: i8, effect_key: &str) -> ResponseOutcome {
        ResponseOutcome {
            delta,
            effect_key: effect_key.to_string(),
            i18n_params: base_effect_params(delta),
        }
    }

    fn adjust_weight(weight: &mut u32, delta: i32) {
        let adjusted = (*weight as i32 + delta).max(0) as u32;
        *weight = adjusted;
    }

    pub fn pick_response_band(weights: &ResponseBandWeights, roll: u32) -> ResponseOutcomeBand {
        let mut cursor = weights.strong_positive;
        if roll < cursor {
            return ResponseOutcomeBand::StrongPositive;
        }

        cursor += weights.mild_positive;
        if roll < cursor {
            return ResponseOutcomeBand::MildPositive;
        }

        cursor += weights.neutral;
        if roll < cursor {
            return ResponseOutcomeBand::Neutral;
        }

        cursor += weights.mild_negative;
        if roll < cursor {
            return ResponseOutcomeBand::MildNegative;
        }

        ResponseOutcomeBand::StrongNegative
    }

    fn treatment_key(message_id: &str, option_id: &str) -> String {
        let family = if message_id.starts_with("morale_talk_") {
            "morale_talk"
        } else if message_id.starts_with("bench_complaint_") {
            "bench_complaint"
        } else if message_id.starts_with("happy_player_") {
            "happy_player"
        } else if message_id.starts_with("contract_concern_") {
            "contract_concern"
        } else {
            "player_event"
        };

        format!("{}:{}", family, option_id)
    }

    fn base_trust_delta(message_id: &str, option_id: &str) -> i16 {
        if message_id.starts_with("morale_talk_") {
            return match option_id {
                "encourage" => 4,
                "promise_time" => 8,
                "work_harder" => -3,
                _ => 0,
            };
        }

        if message_id.starts_with("bench_complaint_") {
            return match option_id {
                "explain" => 3,
                "promise_chance" => 6,
                "prove_yourself" => -2,
                _ => 0,
            };
        }

        if message_id.starts_with("happy_player_") {
            return match option_id {
                "praise_back" => 2,
                "stay_professional" => 0,
                "higher_expectations" => -1,
                _ => 0,
            };
        }

        if message_id.starts_with("contract_concern_") {
            return match option_id {
                "reassure" => 5,
                "noncommittal" => -4,
                "no_renewal" => -8,
                _ => 0,
            };
        }

        0
    }

    pub fn build_response_band_weights(
        player: &Player,
        message_id: &str,
        option_id: &str,
    ) -> ResponseBandWeights {
        let action_key = treatment_key(message_id, option_id);
        let pf = i32::from(personality_factor(player));
        let trust = i32::from(player.morale_core.manager_trust);

        let mut weights = if message_id.starts_with("morale_talk_") {
            match option_id {
                "encourage" => ResponseBandWeights {
                    strong_positive: 2,
                    mild_positive: 5,
                    neutral: 2,
                    mild_negative: 1,
                    strong_negative: 0,
                },
                "promise_time" => ResponseBandWeights {
                    strong_positive: 5,
                    mild_positive: 4,
                    neutral: 1,
                    mild_negative: 0,
                    strong_negative: 0,
                },
                "work_harder" => ResponseBandWeights {
                    strong_positive: 1,
                    mild_positive: 2,
                    neutral: 2,
                    mild_negative: 3,
                    strong_negative: 2,
                },
                _ => ResponseBandWeights {
                    strong_positive: 0,
                    mild_positive: 0,
                    neutral: 1,
                    mild_negative: 0,
                    strong_negative: 0,
                },
            }
        } else {
            return ResponseBandWeights {
                strong_positive: 0,
                mild_positive: 0,
                neutral: 1,
                mild_negative: 0,
                strong_negative: 0,
            };
        };

        if option_id == "encourage" {
            adjust_weight(&mut weights.mild_positive, pf / 8 + (trust - 50) / 25);
            adjust_weight(&mut weights.mild_negative, -pf / 12 - (trust - 50) / 30);
        }

        if option_id == "promise_time" {
            adjust_weight(&mut weights.strong_positive, (trust - 50) / 20);
            adjust_weight(&mut weights.neutral, -(trust - 50) / 30);
        }

        if option_id == "work_harder" {
            adjust_weight(&mut weights.strong_negative, (-pf) / 6 + (50 - trust) / 20);
            adjust_weight(&mut weights.mild_negative, (-pf) / 8 + (50 - trust) / 25);
            adjust_weight(&mut weights.mild_positive, pf / 8 + (trust - 50) / 25);
            adjust_weight(&mut weights.strong_positive, pf / 10 + (trust - 50) / 30);
        }

        if let Some(issue) = player.morale_core.unresolved_issue.as_ref() {
            let severity = i32::from(issue.severity);
            if severity >= 50 {
                adjust_weight(&mut weights.strong_positive, -((severity - 40) / 15));
                adjust_weight(&mut weights.mild_positive, -((severity - 40) / 12));
                adjust_weight(&mut weights.neutral, 1);
            }
            if severity >= 75 {
                adjust_weight(&mut weights.mild_negative, 1);
                adjust_weight(&mut weights.strong_negative, 1);
            }
        }

        if let Some(memory) = player.morale_core.recent_treatment.as_ref()
            && memory.action_key == action_key
        {
            let penalty = i32::from(memory.times_recently_used) * 2;
            adjust_weight(&mut weights.strong_positive, -penalty);
            adjust_weight(&mut weights.mild_positive, -penalty);
            adjust_weight(&mut weights.neutral, i32::from(memory.times_recently_used));
        }

        if weights.total() == 0 {
            weights.neutral = 1;
        }

        weights
    }

    fn banded_morale_talk_outcome<R: rand::Rng + ?Sized>(
        player: &Player,
        message_id: &str,
        option_id: &str,
        rng: &mut R,
    ) -> ResponseOutcome {
        let weights = build_response_band_weights(player, message_id, option_id);
        let roll = rng.random_range(0..weights.total());
        let band = pick_response_band(&weights, roll);

        match option_id {
            "encourage" => match band {
                ResponseOutcomeBand::StrongPositive => outcome(
                    8,
                    "be.msg.playerEvent.effects.moraleCrisis.encourage.positive",
                ),
                ResponseOutcomeBand::MildPositive => outcome(
                    4,
                    "be.msg.playerEvent.effects.moraleCrisis.encourage.positive",
                ),
                ResponseOutcomeBand::Neutral => outcome(
                    0,
                    "be.msg.playerEvent.effects.moraleCrisis.encourage.negative",
                ),
                ResponseOutcomeBand::MildNegative => outcome(
                    -2,
                    "be.msg.playerEvent.effects.moraleCrisis.encourage.negative",
                ),
                ResponseOutcomeBand::StrongNegative => outcome(
                    -5,
                    "be.msg.playerEvent.effects.moraleCrisis.encourage.negative",
                ),
            },
            "promise_time" => match band {
                ResponseOutcomeBand::StrongPositive => {
                    outcome(14, "be.msg.playerEvent.effects.moraleCrisis.promiseTime")
                }
                ResponseOutcomeBand::MildPositive => {
                    outcome(10, "be.msg.playerEvent.effects.moraleCrisis.promiseTime")
                }
                ResponseOutcomeBand::Neutral => {
                    outcome(4, "be.msg.playerEvent.effects.moraleCrisis.promiseTime")
                }
                ResponseOutcomeBand::MildNegative => {
                    outcome(-2, "be.msg.playerEvent.effects.moraleCrisis.promiseTime")
                }
                ResponseOutcomeBand::StrongNegative => {
                    outcome(-6, "be.msg.playerEvent.effects.moraleCrisis.promiseTime")
                }
            },
            "work_harder" => match band {
                ResponseOutcomeBand::StrongPositive => outcome(
                    6,
                    "be.msg.playerEvent.effects.moraleCrisis.workHarder.positive",
                ),
                ResponseOutcomeBand::MildPositive => outcome(
                    2,
                    "be.msg.playerEvent.effects.moraleCrisis.workHarder.positive",
                ),
                ResponseOutcomeBand::Neutral => outcome(
                    0,
                    "be.msg.playerEvent.effects.moraleCrisis.workHarder.negative",
                ),
                ResponseOutcomeBand::MildNegative => outcome(
                    -5,
                    "be.msg.playerEvent.effects.moraleCrisis.workHarder.negative",
                ),
                ResponseOutcomeBand::StrongNegative => outcome(
                    -10,
                    "be.msg.playerEvent.effects.moraleCrisis.workHarder.negative",
                ),
            },
            _ => outcome(
                0,
                "be.msg.playerEvent.effects.moraleCrisis.encourage.negative",
            ),
        }
    }

    fn reduced_by_recent_treatment(delta: i8, player: &Player, action_key: &str) -> i8 {
        let Some(memory) = player.morale_core.recent_treatment.as_ref() else {
            return delta;
        };

        if memory.action_key != action_key || delta <= 0 {
            return delta;
        }

        let reduced = i16::from(delta) - i16::from(memory.times_recently_used) * 4;
        reduced.max(0) as i8
    }

    fn capped_by_unresolved_issue(delta: i8, player: &Player) -> i8 {
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
            return ((i16::from(delta) + 1) / 2).max(1) as i8;
        }

        delta
    }

    fn trust_delta_with_memory(base_delta: i16, player: &Player, action_key: &str) -> i16 {
        let Some(memory) = player.morale_core.recent_treatment.as_ref() else {
            return base_delta;
        };

        if memory.action_key != action_key || base_delta <= 0 {
            return base_delta;
        }

        base_delta / (i16::from(memory.times_recently_used) + 1)
    }

    fn update_recent_treatment(player: &mut Player, action_key: &str) {
        match player.morale_core.recent_treatment.as_mut() {
            Some(memory) if memory.action_key == action_key => {
                memory.times_recently_used = memory.times_recently_used.saturating_add(1);
            }
            Some(memory) => {
                memory.action_key = action_key.to_string();
                memory.times_recently_used = 1;
            }
            None => {
                player.morale_core.recent_treatment = Some(RecentTreatmentMemory {
                    action_key: action_key.to_string(),
                    times_recently_used: 1,
                });
            }
        }
    }

    fn implied_promise(message_id: &str, option_id: &str) -> Option<PlayerPromise> {
        if message_id.starts_with("morale_talk_") && option_id == "promise_time" {
            return Some(PlayerPromise {
                kind: PlayerPromiseKind::PlayingTime,
                matches_remaining: 1,
            });
        }

        if message_id.starts_with("bench_complaint_") && option_id == "promise_chance" {
            return Some(PlayerPromise {
                kind: PlayerPromiseKind::PlayingTime,
                matches_remaining: 1,
            });
        }

        None
    }

    fn should_apply_talk_cooldown(message_id: &str) -> bool {
        message_id.starts_with("morale_talk_")
            || message_id.starts_with("bench_complaint_")
            || message_id.starts_with("happy_player_")
            || message_id.starts_with("contract_concern_")
    }

    /// Apply the effect of a player conversation choice.
    /// Returns a description of what happened, or None if the message wasn't a player event.
    fn apply_response_inner(
        players: &mut [Player],
        club_id: &str,
        player_id: &str,
        message_id: &str,
        option_id: &str,
        today: NaiveDate,
        rng: &mut impl rand::Rng,
    ) -> Option<PlayerResponseEffect> {
        // Get personality factor for this player
        let pf = players
            .iter()
            .find(|p| p.id == player_id)
            .map(personality_factor)
            .unwrap_or(0);

        // Base deltas are now more punishing; personality modifies the outcome
        let mut outcome = if message_id.starts_with("morale_talk_") {
            match option_id {
                "encourage" | "promise_time" | "work_harder" => {
                    let player = players.iter().find(|p| p.id == player_id)?;
                    banded_morale_talk_outcome(player, message_id, option_id, rng)
                }
                _ => return None,
            }
        } else if message_id.starts_with("bench_complaint_") {
            match option_id {
                "explain" => {
                    // Moderate; only works on composed players
                    let d = rng.random_range(-2..=6) + (pf / 4);
                    if d >= 0 {
                        outcome(
                            d,
                            "be.msg.playerEvent.effects.benchComplaint.explain.positive",
                        )
                    } else {
                        outcome(
                            d,
                            "be.msg.playerEvent.effects.benchComplaint.explain.negative",
                        )
                    }
                }
                "promise_chance" => {
                    // PROMISE — big boost now, tracked for consequences
                    let d = rng.random_range(8..=14);
                    outcome(d, "be.msg.playerEvent.effects.benchComplaint.promiseChance")
                }
                "prove_yourself" => {
                    // Very risky — high-aggression players rebel
                    let d = rng.random_range(-10..=6) + (pf / 3);
                    if d >= 0 {
                        outcome(
                            d,
                            "be.msg.playerEvent.effects.benchComplaint.proveYourself.positive",
                        )
                    } else {
                        outcome(
                            d,
                            "be.msg.playerEvent.effects.benchComplaint.proveYourself.negative",
                        )
                    }
                }
                _ => return None,
            }
        } else if message_id.starts_with("happy_player_") {
            match option_id {
                "praise_back" => {
                    let d = rng.random_range(2..=5);
                    outcome(d, "be.msg.playerEvent.effects.happyPlayer.praiseBack")
                }
                "stay_professional" => {
                    // Neutral — can slightly drop morale on volatile players
                    let d = rng.random_range(-2..=3) + (pf / 6);
                    if d >= 0 {
                        outcome(
                            d,
                            "be.msg.playerEvent.effects.happyPlayer.stayProfessional.positive",
                        )
                    } else {
                        outcome(
                            d,
                            "be.msg.playerEvent.effects.happyPlayer.stayProfessional.negative",
                        )
                    }
                }
                "higher_expectations" => {
                    // Risky: leaders respond well, others feel pressured
                    let d = rng.random_range(-6..=4) + (pf / 3);
                    if d >= 0 {
                        outcome(
                            d,
                            "be.msg.playerEvent.effects.happyPlayer.higherExpectations.positive",
                        )
                    } else {
                        outcome(
                            d,
                            "be.msg.playerEvent.effects.happyPlayer.higherExpectations.negative",
                        )
                    }
                }
                _ => return None,
            }
        } else if message_id.starts_with("contract_concern_") {
            match option_id {
                "reassure" => {
                    // Sets expectation of renewal — moderate boost
                    let d = rng.random_range(4..=10);
                    outcome(d, "be.msg.playerEvent.effects.contractConcern.reassure")
                }
                "noncommittal" => {
                    // Almost always negative — players hate uncertainty
                    let d = rng.random_range(-8..=0) + (pf / 5);
                    if d >= 0 {
                        outcome(
                            d,
                            "be.msg.playerEvent.effects.contractConcern.noncommittal.positive",
                        )
                    } else {
                        outcome(
                            d,
                            "be.msg.playerEvent.effects.contractConcern.noncommittal.negative",
                        )
                    }
                }
                "no_renewal" => {
                    let d = rng.random_range(-15..=-8);
                    outcome(d, "be.msg.playerEvent.effects.contractConcern.noRenewal")
                }
                _ => return None,
            }
        } else {
            return None;
        };

        // Clamp delta to prevent extreme swings
        outcome.delta = outcome.delta.clamp(-20, 20);

        // Apply morale change
        if let Some(player) = players.iter_mut().find(|p| p.id == player_id) {
            let action_key = treatment_key(message_id, option_id);
            let current_day = today.format("%Y-%m-%d").to_string();
            let adjusted_delta = capped_by_unresolved_issue(
                reduced_by_recent_treatment(outcome.delta, player, &action_key),
                player,
            );
            let trust_delta = trust_delta_with_memory(
                base_trust_delta(message_id, option_id),
                player,
                &action_key,
            );

            outcome.delta = adjusted_delta.clamp(-20, 20);
            outcome
                .i18n_params
                .insert("delta".to_string(), signed_delta(outcome.delta));

            let base = player.morale as i16;
            player.morale = (base + outcome.delta as i16).clamp(5, 100) as u8;

            let trust =
                (i16::from(player.morale_core.manager_trust) + trust_delta).clamp(0, 100) as u8;
            player.morale_core.manager_trust = trust;
            update_recent_treatment(player, &action_key);

            if let Some(promise) = implied_promise(message_id, option_id) {
                player.morale_core.pending_promise = Some(promise);
            }

            if should_apply_talk_cooldown(message_id) {
                player.morale_core.talk_cooldown_until = Some(current_day.clone());
            }

            if message_id.starts_with("contract_concern_") {
                let renewal_state = player
                    .morale_core
                    .renewal_state
                    .get_or_insert_with(ContractRenewalState::default);

                match option_id {
                    "reassure" => {
                        renewal_state.status = RenewalSessionStatus::Open;
                        renewal_state.manager_blocked_until = None;
                        renewal_state.last_outcome = Some(RenewalSessionOutcome::Stalled);
                    }
                    "noncommittal" => {
                        renewal_state.status = RenewalSessionStatus::Stalled;
                        renewal_state.last_outcome = Some(RenewalSessionOutcome::Stalled);
                    }
                    "no_renewal" => {
                        let blocked_until = today
                            .checked_add_days(Days::new(60))
                            .map(|date| date.format("%Y-%m-%d").to_string());
                        renewal_state.status = RenewalSessionStatus::Blocked;
                        renewal_state.manager_blocked_until = blocked_until;
                        renewal_state.last_outcome = Some(RenewalSessionOutcome::BlockedByManager);
                        renewal_state.exit_intent = Some(ContractExitIntent::LetExpire {
                            set_on: current_day.clone(),
                            reason: Some("manager_inbox_response".to_string()),
                        });
                    }
                    _ => {}
                }
            }
        }

        if message_id.starts_with("contract_concern_") && option_id == "no_renewal" {
            // Teammates lose 2-5 morale
            let mut affected = 0u8;
            for p in players.iter_mut() {
                if p.id != player_id && p.team_id.as_deref() == Some(club_id) {
                    let loss = rng.random_range(2..=5);
                    p.morale = (p.morale as i16 - loss as i16).clamp(10, 100) as u8;
                    affected += 1;
                }
            }
            if affected > 0 {
                outcome.effect_key =
                    "be.msg.playerEvent.effects.contractConcern.noRenewalWithDressingRoom"
                        .to_string();
                outcome
                    .i18n_params
                    .insert("affected".to_string(), affected.to_string());
            }
        }

        Some(PlayerResponseEffect {
            message: String::new(),
            i18n_key: outcome.effect_key,
            i18n_params: outcome.i18n_params,
        })
    }

    /// Exact four-family source conversation effects over caller-owned domain
    /// players. Current manager ownership is supplied explicitly. The caller must
    /// stage all players alongside InboxStore and reconcile other authoritative
    /// contract/morale representations before publication.
    pub fn apply_player_response(
        players: &mut [Player],
        club_id: &str,
        message: &domain::message::InboxMessage,
        action_id: &str,
        option_id: &str,
        today: NaiveDate,
        rng: &mut impl rand::Rng,
    ) -> Result<super::EffectReceipt, super::InboxError> {
        let action = message
            .actions
            .iter()
            .find(|action| action.id == action_id)
            .ok_or(super::InboxError::Unavailable)?;
        if action.resolved {
            return Err(super::InboxError::AlreadyResolved);
        }
        let domain::message::ActionType::ChooseOption { options } = &action.action_type else {
            return Err(super::InboxError::InvalidOption);
        };
        if !options.iter().any(|option| option.id == option_id) {
            return Err(super::InboxError::InvalidOption);
        }
        let player_id = message
            .context
            .player_id
            .as_deref()
            .ok_or(super::InboxError::Unavailable)?;
        let player = players
            .iter()
            .find(|player| player.id == player_id && player.team_id.as_deref() == Some(club_id))
            .ok_or(super::InboxError::Unavailable)?;
        if player.morale > 100
            || player.morale_core.manager_trust > 100
            || [
                player.attributes.composure,
                player.attributes.leadership,
                player.attributes.aggression,
            ]
            .into_iter()
            .any(|rating| rating > 100)
            || player
                .morale_core
                .unresolved_issue
                .as_ref()
                .is_some_and(|issue| issue.severity > 100)
        {
            return Err(super::InboxError::Effect(
                "Invalid source player morale profile".into(),
            ));
        }
        // Source counts affected teammates in u8. Reject impossible oversized
        // club rosters rather than overflowing halfway through the side effect.
        if players
            .iter()
            .filter(|player| player.team_id.as_deref() == Some(club_id))
            .count()
            > 256
            || players
                .iter()
                .map(|player| &player.id)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != players.len()
        {
            return Err(super::InboxError::Effect(
                "Invalid source player roster".into(),
            ));
        }
        let effect = apply_response_inner(
            players,
            club_id,
            player_id,
            &message.id,
            option_id,
            today,
            rng,
        )
        .ok_or(super::InboxError::UnsupportedAction)?;
        Ok(super::EffectReceipt {
            i18n_key: effect.i18n_key,
            i18n_params: effect.i18n_params.into_iter().collect(),
        })
    }
}

use domain::message::{ActionType, InboxMessage, MessageAction};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InboxError {
    Unavailable,
    InvalidMessage,
    InvalidOption,
    AlreadyResolved,
    Expired,
    UnsupportedAction,
    Effect(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectReceipt {
    pub i18n_key: String,
    pub i18n_params: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolutionEffect {
    Acknowledged,
    Dismissed,
    NavigateTo { route: String },
    Domain(EffectReceipt),
    Expired { on: NaiveDate, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionResolution {
    pub action_id: String,
    pub option_id: Option<String>,
    pub effect: ResolutionEffect,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Delivery {
    recipient_manager_id: String,
    message: InboxMessage,
    deleted: bool,
    resolutions: BTreeMap<String, ActionResolution>,
    #[serde(default)]
    removed_actions: BTreeMap<String, MessageAction>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InboxStore {
    deliveries: Vec<Delivery>,
}

fn validate_message(message: &InboxMessage) -> Result<(), InboxError> {
    if message.id.trim().is_empty() {
        return Err(InboxError::InvalidMessage);
    }
    let mut actions = BTreeSet::new();
    for action in &message.actions {
        if action.id.trim().is_empty() || !actions.insert(&action.id) {
            return Err(InboxError::InvalidMessage);
        }
        if let ActionType::ChooseOption { options } = &action.action_type {
            let mut ids = BTreeSet::new();
            if options.is_empty()
                || options
                    .iter()
                    .any(|option| option.id.trim().is_empty() || !ids.insert(&option.id))
            {
                return Err(InboxError::InvalidMessage);
            }
        }
    }
    Ok(())
}

impl InboxStore {
    /// Host-only recurrence/history access, including deleted delivery tombstones.
    pub fn history(&self, recipient: &str) -> Vec<InboxMessage> {
        self.deliveries
            .iter()
            .filter(|d| d.recipient_manager_id == recipient)
            .map(|d| d.message.clone())
            .collect()
    }
    pub fn contains_delivery(&self, recipient: &str, message_id: &str) -> bool {
        self.deliveries
            .iter()
            .any(|d| d.recipient_manager_id == recipient && d.message.id == message_id)
    }
    /// Trusted domain completion only: preserve action receipts when source
    /// effects remove a resolved action (youth shortlist/discard).
    pub(crate) fn replace_domain_message(
        &mut self,
        recipient: &str,
        message: InboxMessage,
    ) -> Result<(), InboxError> {
        validate_message(&message)?;
        let delivery = self.delivery_mut(recipient, &message.id)?;
        let removed: Vec<_> = delivery
            .message
            .actions
            .iter()
            .filter(|old| !message.actions.iter().any(|new| new.id == old.id))
            .cloned()
            .collect();
        if removed
            .iter()
            .any(|action| !action.resolved || !delivery.resolutions.contains_key(&action.id))
            || message
                .actions
                .iter()
                .any(|action| delivery.removed_actions.contains_key(&action.id))
        {
            return Err(InboxError::InvalidMessage);
        }
        for action in removed {
            delivery.removed_actions.insert(action.id.clone(), action);
        }
        delivery.message = message;
        Ok(())
    }
    /// Host-delivery seam. Duplicate (recipient, message ID) deliveries are no-ops,
    /// including deleted messages; a new event requires its own source event ID.
    pub fn deliver(&mut self, recipient: &str, message: InboxMessage) -> Result<bool, InboxError> {
        if recipient.trim().is_empty() {
            return Err(InboxError::InvalidMessage);
        }
        validate_message(&message)?;
        if self.deliveries.iter().any(|delivery| {
            delivery.recipient_manager_id == recipient && delivery.message.id == message.id
        }) {
            return Ok(false);
        }
        self.deliveries.push(Delivery {
            recipient_manager_id: recipient.into(),
            message,
            deleted: false,
            resolutions: BTreeMap::new(),
            removed_actions: BTreeMap::new(),
        });
        Ok(true)
    }

    pub fn list(&self, recipient: &str) -> Vec<InboxMessage> {
        self.deliveries
            .iter()
            .filter(|delivery| delivery.recipient_manager_id == recipient && !delivery.deleted)
            .map(|delivery| delivery.message.clone())
            .collect()
    }

    pub fn get(&self, recipient: &str, message_id: &str) -> Result<InboxMessage, InboxError> {
        Ok(self.delivery(recipient, message_id)?.message.clone())
    }

    pub fn mark_read(&mut self, recipient: &str, message_id: &str) -> Result<(), InboxError> {
        self.delivery_mut(recipient, message_id)?.message.read = true;
        Ok(())
    }

    pub fn mark_all_read(&mut self, recipient: &str) {
        for delivery in &mut self.deliveries {
            if delivery.recipient_manager_id == recipient && !delivery.deleted {
                delivery.message.read = true;
            }
        }
    }

    pub fn delete(&mut self, recipient: &str, message_id: &str) -> Result<(), InboxError> {
        self.delivery_mut(recipient, message_id)?.deleted = true;
        Ok(())
    }

    /// Source cleanup keeps unread messages, unresolved actions, and messages at
    /// most 14 days old (including future dates). Invalid old date strings drop.
    pub fn clear_old(&mut self, recipient: &str, today: NaiveDate) -> usize {
        let mut removed = 0;
        for delivery in &mut self.deliveries {
            let message = &delivery.message;
            if delivery.recipient_manager_id != recipient
                || delivery.deleted
                || !message.read
                || message.actions.iter().any(|action| !action.resolved)
            {
                continue;
            }
            if NaiveDate::parse_from_str(&message.date, "%Y-%m-%d")
                .is_ok_and(|date| (today - date).num_days() <= 14)
            {
                continue;
            }
            delivery.deleted = true;
            removed += 1;
        }
        removed
    }

    /// Validates before calling executable domain logic. An unsupported choice
    /// must return UnsupportedAction, never an invented acknowledgment/effect.
    /// Replaying a resolved choice returns its old receipt without invoking apply.
    pub fn resolve_with<F>(
        &mut self,
        recipient: &str,
        message_id: &str,
        action_id: &str,
        option_id: Option<&str>,
        apply: F,
    ) -> Result<ActionResolution, InboxError>
    where
        F: FnOnce(&InboxMessage, &MessageAction, &str) -> Result<EffectReceipt, InboxError>,
    {
        let delivery = self.delivery_mut(recipient, message_id)?;
        if let Some(resolution) = delivery.resolutions.get(action_id) {
            if matches!(resolution.effect, ResolutionEffect::Expired { .. }) {
                return Err(InboxError::Expired);
            }
            return if resolution.option_id.as_deref() == option_id {
                Ok(resolution.clone())
            } else {
                Err(InboxError::AlreadyResolved)
            };
        }
        let action = delivery
            .message
            .actions
            .iter()
            .find(|action| action.id == action_id)
            .ok_or(InboxError::Unavailable)?;
        if action.resolved {
            return Err(InboxError::AlreadyResolved);
        }
        let effect = match &action.action_type {
            ActionType::ChooseOption { options } => {
                let selected = option_id
                    .filter(|selected| options.iter().any(|option| option.id == *selected))
                    .ok_or(InboxError::InvalidOption)?;
                ResolutionEffect::Domain(apply(&delivery.message, action, selected)?)
            }
            _ if option_id.is_some() => return Err(InboxError::InvalidOption),
            ActionType::Acknowledge => ResolutionEffect::Acknowledged,
            ActionType::Dismiss => ResolutionEffect::Dismissed,
            ActionType::NavigateTo { route } => ResolutionEffect::NavigateTo {
                route: route.clone(),
            },
        };
        let resolution = ActionResolution {
            action_id: action_id.into(),
            option_id: option_id.map(str::to_owned),
            effect,
        };
        delivery
            .message
            .actions
            .iter_mut()
            .find(|action| action.id == action_id)
            .unwrap()
            .resolved = true;
        delivery
            .resolutions
            .insert(action_id.into(), resolution.clone());
        Ok(resolution)
    }

    /// Explicit host-domain expiration, e.g. a no-longer-open offer. Not a generic
    /// invented deadline. A completed action is never relabeled as expired.
    pub fn expire_action(
        &mut self,
        recipient: &str,
        message_id: &str,
        action_id: &str,
        on: NaiveDate,
        reason: &str,
    ) -> Result<bool, InboxError> {
        let delivery = self.delivery_mut(recipient, message_id)?;
        let action = delivery
            .message
            .actions
            .iter_mut()
            .find(|action| action.id == action_id)
            .ok_or(InboxError::Unavailable)?;
        if action.resolved {
            return Ok(false);
        }
        action.resolved = true;
        delivery.resolutions.insert(
            action_id.into(),
            ActionResolution {
                action_id: action_id.into(),
                option_id: None,
                effect: ResolutionEffect::Expired {
                    on,
                    reason: reason.into(),
                },
            },
        );
        Ok(true)
    }

    /// Checkpoint seam. Recipients may be dismissed managers: their archived inbox
    /// must remain theirs. The caller validates them against retained identities.
    pub fn validate(&self) -> Result<(), InboxError> {
        let mut keys = BTreeSet::new();
        for delivery in &self.deliveries {
            if delivery.recipient_manager_id.trim().is_empty()
                || !keys.insert((&delivery.recipient_manager_id, &delivery.message.id))
            {
                return Err(InboxError::InvalidMessage);
            }
            validate_message(&delivery.message)?;
            if delivery.removed_actions.iter().any(|(id, action)| {
                id != &action.id
                    || !action.resolved
                    || !delivery.resolutions.contains_key(id)
                    || delivery.message.actions.iter().any(|a| &a.id == id)
            }) {
                return Err(InboxError::InvalidMessage);
            }
            for (id, resolution) in &delivery.resolutions {
                let action = delivery
                    .message
                    .actions
                    .iter()
                    .chain(delivery.removed_actions.values())
                    .find(|action| &action.id == id)
                    .ok_or(InboxError::InvalidMessage)?;
                if id != &resolution.action_id || !action.resolved {
                    return Err(InboxError::InvalidMessage);
                }
                match (
                    &action.action_type,
                    &resolution.effect,
                    resolution.option_id.as_deref(),
                ) {
                    (_, ResolutionEffect::Expired { .. }, None) => {}
                    (ActionType::Acknowledge, ResolutionEffect::Acknowledged, None) => {}
                    (ActionType::Dismiss, ResolutionEffect::Dismissed, None) => {}
                    (
                        ActionType::NavigateTo { route },
                        ResolutionEffect::NavigateTo { route: recorded },
                        None,
                    ) if route == recorded => {}
                    (
                        ActionType::ChooseOption { options },
                        ResolutionEffect::Domain(_),
                        Some(selected),
                    ) if options.iter().any(|option| option.id == selected) => {}
                    _ => return Err(InboxError::InvalidMessage),
                }
            }
        }
        Ok(())
    }

    pub fn recipients(&self) -> BTreeSet<&str> {
        self.deliveries
            .iter()
            .map(|delivery| delivery.recipient_manager_id.as_str())
            .collect()
    }

    fn delivery(&self, recipient: &str, message_id: &str) -> Result<&Delivery, InboxError> {
        self.deliveries
            .iter()
            .find(|delivery| {
                delivery.recipient_manager_id == recipient
                    && delivery.message.id == message_id
                    && !delivery.deleted
            })
            .ok_or(InboxError::Unavailable)
    }

    fn delivery_mut(
        &mut self,
        recipient: &str,
        message_id: &str,
    ) -> Result<&mut Delivery, InboxError> {
        self.deliveries
            .iter_mut()
            .find(|delivery| {
                delivery.recipient_manager_id == recipient
                    && delivery.message.id == message_id
                    && !delivery.deleted
            })
            .ok_or(InboxError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::message::{ActionOption, MessageContext};

    fn message(id: &str, date: &str) -> InboxMessage {
        InboxMessage::new(
            id.into(),
            "Subject".into(),
            "Body".into(),
            "Sender".into(),
            date.into(),
        )
        .with_context(MessageContext {
            team_id: Some("rival-club".into()),
            ..MessageContext::default()
        })
        .with_action(MessageAction {
            id: "respond".into(),
            label: "Respond".into(),
            resolved: false,
            label_key: None,
            action_type: ActionType::ChooseOption {
                options: ["yes", "no"]
                    .map(|id| ActionOption {
                        id: id.into(),
                        label: id.into(),
                        description: String::new(),
                        label_key: None,
                        description_key: None,
                    })
                    .to_vec(),
            },
        })
    }

    #[test]
    fn recipient_is_immutable_and_independent_of_context_club() {
        let mut store = InboxStore::default();
        store
            .deliver("original-manager", message("same-id", "2026-06-01"))
            .unwrap();
        assert_eq!(store.list("original-manager").len(), 1);
        for actor in ["replacement-manager", "rival-club", "outsider"] {
            assert!(store.list(actor).is_empty());
            assert!(matches!(
                store.get(actor, "same-id"),
                Err(InboxError::Unavailable)
            ));
            assert_eq!(
                store.mark_read(actor, "same-id"),
                Err(InboxError::Unavailable)
            );
        }
        store
            .deliver("other-manager", message("same-id", "2026-06-01"))
            .unwrap();
        store.mark_all_read("original-manager");
        assert!(!store.get("other-manager", "same-id").unwrap().read);
        store.validate().unwrap();
    }

    #[test]
    fn unknown_failed_expired_and_replayed_choices_never_repeat_effects() {
        let mut store = InboxStore::default();
        store
            .deliver("manager", message("event", "2026-06-01"))
            .unwrap();
        let before = serde_json::to_value(&store).unwrap();
        assert_eq!(
            store.resolve_with(
                "manager",
                "event",
                "respond",
                Some("unknown"),
                |_, _, _| panic!("invalid choice executed")
            ),
            Err(InboxError::InvalidOption)
        );
        assert_eq!(
            store.resolve_with("manager", "event", "respond", Some("yes"), |_, _, _| Err(
                InboxError::UnsupportedAction
            )),
            Err(InboxError::UnsupportedAction)
        );
        assert_eq!(before, serde_json::to_value(&store).unwrap());
        let receipt = store
            .resolve_with("manager", "event", "respond", Some("yes"), |_, _, _| {
                Ok(EffectReceipt {
                    i18n_key: "effect".into(),
                    i18n_params: BTreeMap::new(),
                })
            })
            .unwrap();
        let mut restored: InboxStore =
            serde_json::from_value(serde_json::to_value(&store).unwrap()).unwrap();
        restored.validate().unwrap();
        assert_eq!(
            restored.resolve_with(
                "manager",
                "event",
                "respond",
                Some("yes"),
                |_, _, _| panic!("replayed effect")
            ),
            Ok(receipt)
        );
        assert_eq!(
            restored.resolve_with("manager", "event", "respond", Some("no"), |_, _, _| panic!(
                "changed effect"
            )),
            Err(InboxError::AlreadyResolved)
        );
        restored
            .deliver("manager", message("expiring", "2026-06-01"))
            .unwrap();
        restored
            .expire_action(
                "manager",
                "expiring",
                "respond",
                NaiveDate::from_ymd_opt(2026, 6, 2).unwrap(),
                "Offer closed",
            )
            .unwrap();
        assert_eq!(
            restored.resolve_with(
                "manager",
                "expiring",
                "respond",
                Some("yes"),
                |_, _, _| panic!("expired effect")
            ),
            Err(InboxError::Expired)
        );
    }

    #[test]
    fn cleanup_uses_source_fourteen_day_boundary_and_retains_pending_or_unread() {
        let mut store = InboxStore::default();
        for (id, date, read, resolved) in [
            ("old-read", "2026-06-01", true, true),
            ("boundary", "2026-06-02", true, true),
            ("unread", "2026-06-01", false, true),
            ("pending", "2026-06-01", true, false),
        ] {
            let mut msg = message(id, date);
            msg.read = read;
            msg.actions[0].resolved = resolved;
            store.deliver("manager", msg).unwrap();
        }
        assert_eq!(
            store.clear_old("manager", NaiveDate::from_ymd_opt(2026, 6, 16).unwrap()),
            1
        );
        assert_eq!(store.list("manager").len(), 3);
        assert!(
            !store
                .deliver("manager", message("old-read", "2026-06-01"))
                .unwrap()
        );
        store.validate().unwrap();
    }

    fn player(id: &str, club: &str) -> domain::player::Player {
        let attributes = serde_json::from_value(serde_json::json!({
            "pace": 50, "stamina": 50, "strength": 50, "passing": 50,
            "shooting": 50, "tackling": 50, "dribbling": 50, "defending": 50,
            "positioning": 50, "vision": 50, "decisions": 50,
            "composure": 50, "leadership": 50, "aggression": 50
        }))
        .unwrap();
        let mut player = domain::player::Player::new(
            id.into(),
            id.into(),
            id.into(),
            "2000-01-01".into(),
            "ENG".into(),
            domain::player::Position::Midfielder,
            attributes,
        );
        player.team_id = Some(club.into());
        player.morale = 50;
        player
    }

    fn conversation(family: &str, option: &str) -> InboxMessage {
        let mut msg = message(&format!("{family}_p_2026-06-01"), "2026-06-01");
        msg.context.player_id = Some("p".into());
        msg.actions[0].action_type = ActionType::ChooseOption {
            options: vec![ActionOption {
                id: option.into(),
                label: option.into(),
                description: String::new(),
                label_key: None,
                description_key: None,
            }],
        };
        msg
    }

    #[test]
    fn source_playing_time_promise_and_receipt_apply_only_once() {
        use rand::{RngExt, SeedableRng, rngs::StdRng};
        let today = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        let mut players = vec![player("p", "own"), player("rival", "other")];
        let mut store = InboxStore::default();
        let message = conversation("bench_complaint", "promise_chance");
        let message_id = message.id.clone();
        store.deliver("manager", message).unwrap();
        let mut rng = StdRng::seed_from_u64(42);
        let expected: i8 = StdRng::seed_from_u64(42).random_range(8..=14);
        let result = store
            .resolve_with(
                "manager",
                &message_id,
                "respond",
                Some("promise_chance"),
                |message, action, option| {
                    conversations::apply_player_response(
                        &mut players,
                        "own",
                        message,
                        &action.id,
                        option,
                        today,
                        &mut rng,
                    )
                },
            )
            .unwrap();
        assert_eq!(players[0].morale, (50 + expected) as u8);
        assert_eq!(players[0].morale_core.manager_trust, 56);
        assert_eq!(
            players[0]
                .morale_core
                .pending_promise
                .as_ref()
                .unwrap()
                .matches_remaining,
            1
        );
        assert_eq!(
            players[0].morale_core.talk_cooldown_until.as_deref(),
            Some("2026-06-01")
        );
        assert_eq!(
            players[0]
                .morale_core
                .recent_treatment
                .as_ref()
                .unwrap()
                .action_key,
            "bench_complaint:promise_chance"
        );
        assert_eq!(players[1].morale, 50);
        assert_eq!(
            store.resolve_with(
                "manager",
                &message_id,
                "respond",
                Some("promise_chance"),
                |_, _, _| panic!("repeat promise effect")
            ),
            Ok(result)
        );
    }

    #[test]
    fn source_no_renewal_updates_exit_intent_and_only_own_teammates() {
        use rand::{RngExt, SeedableRng, rngs::StdRng};
        let today = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        let mut players = vec![
            player("p", "own"),
            player("teammate", "own"),
            player("rival", "other"),
        ];
        let message = conversation("contract_concern", "no_renewal");
        let mut reference = StdRng::seed_from_u64(7);
        let delta: i8 = reference.random_range(-15..=-8);
        let teammate_loss: i16 = reference.random_range(2..=5);
        let mut rng = StdRng::seed_from_u64(7);
        let effect = conversations::apply_player_response(
            &mut players,
            "own",
            &message,
            "respond",
            "no_renewal",
            today,
            &mut rng,
        )
        .unwrap();
        assert_eq!(players[0].morale, (50 + delta) as u8);
        assert_eq!(players[0].morale_core.manager_trust, 42);
        let renewal = players[0].morale_core.renewal_state.as_ref().unwrap();
        assert_eq!(
            renewal.status,
            domain::player::RenewalSessionStatus::Blocked
        );
        assert_eq!(renewal.manager_blocked_until.as_deref(), Some("2026-07-31"));
        assert!(matches!(
            renewal.exit_intent,
            Some(domain::player::ContractExitIntent::LetExpire { .. })
        ));
        assert_eq!(players[1].morale, (50 - teammate_loss) as u8);
        assert_eq!(players[2].morale, 50);
        assert_eq!(effect.i18n_params["affected"], "1");
        assert_eq!(rng.random::<u64>(), reference.random::<u64>());
    }

    #[test]
    fn conversation_scope_and_unsupported_actions_fail_without_mutation_or_rng() {
        use rand::{RngExt, SeedableRng, rngs::StdRng};
        let mut players = vec![player("p", "own")];
        let before = serde_json::to_value(&players).unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        let mut rng = StdRng::seed_from_u64(42);
        assert_eq!(
            conversations::apply_player_response(
                &mut players,
                "rival",
                &conversation("bench_complaint", "promise_chance"),
                "respond",
                "promise_chance",
                today,
                &mut rng
            ),
            Err(InboxError::Unavailable)
        );
        assert_eq!(
            conversations::apply_player_response(
                &mut players,
                "own",
                &conversation("unsupported", "unknown"),
                "respond",
                "unknown",
                today,
                &mut rng
            ),
            Err(InboxError::UnsupportedAction)
        );
        assert_eq!(before, serde_json::to_value(&players).unwrap());
        assert_eq!(
            rng.random::<u64>(),
            StdRng::seed_from_u64(42).random::<u64>()
        );
    }
}
