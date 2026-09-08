//! Assistant renewal policy adapted from pinned ofm_core/delegated_renewals.rs.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
use crate::contracts::{WarningStage, wage_policy_allows};
use crate::{Error, Management};
use domain::message::{
    DelegatedRenewalCaseData, DelegatedRenewalReportData, InboxMessage, MessageCategory,
    MessageContext, MessagePriority,
};
use domain::player::{RenewalSessionOutcome as Outcome, RenewalSessionStatus as Status};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delegation {
    pub player_ids: Option<Vec<String>>,
    pub max_wage_increase_pct: u32,
    pub max_contract_years: u32,
}

impl Management {
    pub(crate) fn delegate_contracts(
        &mut self,
        actor: &str,
        options: &Delegation,
    ) -> Result<serde_json::Value, Error> {
        let club = self
            .managers
            .get(actor)
            .ok_or(Error::Unauthorized)?
            .club_id
            .clone();
        if self.window.is_ready(actor) {
            return Err(Error::AlreadyReady);
        }
        let personnel = self.personnel.as_ref().ok_or(Error::Unavailable)?;
        let assistant = personnel
            .staff
            .values()
            .find(|s| {
                s.team_id.as_deref() == Some(&club)
                    && s.role == domain::staff::StaffRole::AssistantManager
            })
            .ok_or_else(|| Error::Contract("be.error.contracts.noAssistantManagerAssigned".into()))?
            .clone();
        let today = self.career_date().ok_or(Error::Unavailable)?;
        let social = self.social.as_ref().ok_or(Error::Unavailable)?;
        let career = self.career.as_ref().ok_or(Error::Unavailable)?;
        let selected = options
            .player_ids
            .as_ref()
            .map(|ids| ids.iter().collect::<BTreeSet<_>>());
        let ids: Vec<_> = self
            .players
            .values()
            .filter(|p| p.club_id == club)
            .filter(|p| career.contracts[&p.id].end_date.is_some())
            .filter(|p| {
                selected.as_ref().map_or_else(
                    || {
                        !career.contracts[&p.id].let_expire
                            && social.source_players[&p.id]
                                .morale_core
                                .renewal_state
                                .as_ref()
                                .is_none_or(|r| r.exit_intent.is_none())
                            && career.contracts[&p.id].warning_stage(today).is_some()
                    },
                    |ids| ids.contains(&p.id),
                )
            })
            .map(|p| p.id.clone())
            .collect();
        let mut report = DelegatedRenewalReportData {
            success_count: 0,
            failure_count: 0,
            stalled_count: 0,
            cases: vec![],
        };
        for id in ids {
            let contract = self.career.as_ref().unwrap().contracts[&id].clone();
            let core = self.social.as_ref().unwrap().source_players[&id]
                .morale_core
                .clone();
            let reputation = self.career.as_ref().unwrap().reputations[&club];
            let expected = contract
                .expected_wage(reputation, today)
                .map_err(Error::Contract)?;
            let years = contract.expected_years(today);
            let max_years = options.max_contract_years.max(1);
            let percent = 100u32
                .checked_add(options.max_wage_increase_pct)
                .ok_or(Error::Overflow)?;
            let cap = contract.weekly_wage.saturating_mul(percent) / 100;
            let cap = cap.checked_add(999).ok_or(Error::Overflow)? / 1000 * 1000;
            let blocked = self.validate_social_renewal(actor, &id).is_err()
                || contract.blocked_until.is_some_and(|until| until >= today);
            let let_expire = contract.let_expire
                || core
                    .renewal_state
                    .as_ref()
                    .is_some_and(|r| r.exit_intent.is_some());
            let urgency = match contract.warning_stage(today) {
                Some(WarningStage::FinalWeeks) => 18,
                Some(WarningStage::ThreeMonths) => 14,
                Some(WarningStage::SixMonths) => 10,
                Some(WarningStage::TwelveMonths) => 6,
                None => 2,
            };
            let a = &assistant.attributes;
            let score = (i32::from(a.coaching) * 4
                + i32::from(a.judging_ability) * 3
                + i32::from(a.judging_potential) * 3)
                / 10
                + i32::from(contract.manager_trust) / 3
                + i32::from(contract.morale) / 2
                + urgency
                - if contract.market_value >= 2_000_000 {
                    22
                } else if contract.market_value >= 750_000 {
                    10
                } else {
                    0
                }
                - core
                    .unresolved_issue
                    .as_ref()
                    .map_or(0, |i| i32::from(i.severity) / 2);
            let total = self
                .contract_wage_total(self.career.as_ref().unwrap(), &club)
                .map_err(Error::Contract)?;
            let policy = wage_policy_allows(
                self.clubs[&club].balance,
                self.career.as_ref().unwrap().wage_budgets[&club],
                total,
                contract.weekly_wage,
                expected,
            );
            let (status, note, state_status, state_outcome) = if let_expire {
                ("failed", "markedLetExpire", None, None)
            } else if blocked {
                ("failed", "managerBlocked", None, None)
            } else if cap < expected || max_years < years {
                (
                    "stalled",
                    "beyondLimits",
                    Some(Status::Stalled),
                    Some(Outcome::Stalled),
                )
            } else if score >= 95 && !policy {
                (
                    "stalled",
                    "boardWagePolicy",
                    Some(Status::Stalled),
                    Some(Outcome::Stalled),
                )
            } else if score >= 95 {
                (
                    "successful",
                    "completed",
                    Some(Status::Agreed),
                    Some(Outcome::AcceptedByAssistant),
                )
            } else if score >= 72 {
                (
                    "stalled",
                    "prefersManager",
                    Some(Status::Open),
                    Some(Outcome::Stalled),
                )
            } else {
                (
                    "failed",
                    "relationshipBlocked",
                    Some(Status::Stalled),
                    Some(Outcome::RejectedByPlayer),
                )
            };
            let mut params = HashMap::new();
            if matches!(note, "beyondLimits" | "prefersManager") {
                params.insert("wage".into(), expected.to_string());
                params.insert("years".into(), years.to_string());
            }
            if note == "boardWagePolicy" {
                params.insert(
                    "budget".into(),
                    self.career.as_ref().unwrap().wage_budgets[&club].to_string(),
                );
            }
            if let Some(state_status) = state_status {
                let state = self
                    .social
                    .as_mut()
                    .unwrap()
                    .source_players
                    .get_mut(&id)
                    .unwrap()
                    .morale_core
                    .renewal_state
                    .get_or_insert_with(Default::default);
                state.status = state_status;
                state.last_assistant_attempt_date = Some(today.to_string());
                state.last_outcome = state_outcome;
                state.conversation_round = 0;
                if status == "successful" {
                    state.manager_blocked_until = None;
                    state.exit_intent = None;
                }
                self.player_revisions.insert(
                    id.clone(),
                    self.player_revisions[&id]
                        .checked_add(1)
                        .ok_or(Error::Overflow)?,
                );
            }
            if status == "successful" {
                let contract = self
                    .career
                    .as_mut()
                    .unwrap()
                    .contracts
                    .get_mut(&id)
                    .unwrap();
                contract
                    .apply_agreement(today, expected, years.min(max_years))
                    .map_err(Error::Contract)?;
                self.club_revisions.insert(
                    club.clone(),
                    self.club_revisions[&club]
                        .checked_add(1)
                        .ok_or(Error::Overflow)?,
                );
                report.success_count += 1;
            } else if status == "stalled" {
                report.stalled_count += 1;
            } else {
                report.failure_count += 1;
            }
            report.cases.push(DelegatedRenewalCaseData {
                player_id: id.clone(),
                player_name: self.players[&id].name.clone(),
                status: status.into(),
                agreed_wage: (status == "successful").then_some(expected),
                agreed_years: (status == "successful").then_some(years.min(max_years)),
                note_key: Some(format!("be.msg.delegatedRenewals.notes.{note}")),
                note_params: params,
            });
        }
        if !report.cases.is_empty() {
            let params = HashMap::from([
                ("team".into(), self.clubs[&club].name.clone()),
                ("successes".into(), report.success_count.to_string()),
                ("stalled".into(), report.stalled_count.to_string()),
                ("failures".into(), report.failure_count.to_string()),
            ]);
            let message = InboxMessage::new(
                format!("delegated-renewals:{actor}:{}", self.sequence),
                String::new(),
                String::new(),
                String::new(),
                today.to_string(),
            )
            .with_category(MessageCategory::Contract)
            .with_priority(MessagePriority::High)
            .with_i18n(
                "be.msg.delegatedRenewals.subject",
                "be.msg.delegatedRenewals.body",
                params,
            )
            .with_sender_i18n("be.sender.assistantManager", "be.role.assistantManager")
            .with_context(MessageContext {
                team_id: Some(club),
                delegated_renewal_report: Some(report.clone()),
                ..Default::default()
            });
            self.social
                .as_mut()
                .unwrap()
                .inbox
                .deliver(actor, message)
                .map_err(|e| Error::Social(format!("{e:?}")))?;
        }
        serde_json::to_value(report).map_err(|e| Error::Contract(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::career::{CareerCommand, CareerOutcome};
    use crate::{Command, Outcome, Request};
    fn game() -> crate::football::Football {
        let mut game = crate::personnel::tests::game();
        let staff = game
            .management
            .personnel
            .as_mut()
            .unwrap()
            .staff
            .get_mut("coach")
            .unwrap();
        staff.team_id = Some("a".into());
        staff.role = domain::staff::StaffRole::AssistantManager;
        staff.attributes.coaching = 100;
        staff.attributes.judging_ability = 100;
        staff.attributes.judging_potential = 100;
        let personnel = game.management.personnel.take().unwrap();
        game.configure_personnel(personnel.teams, personnel.staff, personnel.seed)
            .unwrap();
        game
    }
    fn request(id: &str, command: CareerCommand) -> Request {
        Request {
            id: id.into(),
            day: 1,
            command: Command::Career(command),
        }
    }
    #[test]
    fn assistant_renews_own_selected_player_once_and_publishes_private_report() {
        let mut game = game();
        let old_other = game.management.career.as_ref().unwrap().contracts["b-p"].clone();
        let command = request(
            "delegate",
            CareerCommand::Delegate(Delegation {
                player_ids: Some(vec!["a-p".into(), "b-p".into()]),
                max_wage_increase_pct: 100,
                max_contract_years: 3,
            }),
        );
        let receipt = game.dispatch("a", command.clone(), 1).unwrap();
        let Ok(Outcome::Career(CareerOutcome::Delegated(report))) = &receipt.result else {
            panic!("{receipt:?}")
        };
        assert_eq!(report["success_count"], 1);
        assert_eq!(report["cases"].as_array().unwrap().len(), 1);
        assert_eq!(
            game.management.career.as_ref().unwrap().contracts["b-p"],
            old_other
        );
        assert_eq!(game.inbox_view("a", 0, 20).unwrap().len(), 1);
        assert!(game.inbox_view("b", 0, 20).unwrap().is_empty());
        let state = game.management.social.as_ref().unwrap().source_players["a-p"]
            .morale_core
            .renewal_state
            .as_ref()
            .unwrap();
        assert_eq!(
            state.last_outcome,
            Some(domain::player::RenewalSessionOutcome::AcceptedByAssistant)
        );
        assert_eq!(game.dispatch("a", command, 2).unwrap(), receipt);
        let saved = game.save_state().unwrap();
        crate::football::Football::load_validated(saved).unwrap();
    }
    #[test]
    fn limits_and_exit_intent_are_not_overridden_by_a_good_assistant() {
        let mut game = game();
        let original = game.management.career.as_ref().unwrap().contracts["a-p"].weekly_wage;
        let command = CareerCommand::Delegate(Delegation {
            player_ids: Some(vec!["a-p".into()]),
            max_wage_increase_pct: 0,
            max_contract_years: 1,
        });
        let receipt = game
            .dispatch("a", request("limits", command.clone()), 1)
            .unwrap();
        let Ok(Outcome::Career(CareerOutcome::Delegated(report))) = receipt.result else {
            panic!()
        };
        assert_eq!(report["stalled_count"], 1);
        assert_eq!(
            game.management.career.as_ref().unwrap().contracts["a-p"].weekly_wage,
            original
        );
        game.dispatch(
            "a",
            request(
                "exit",
                CareerCommand::LetExpire {
                    player_id: "a-p".into(),
                    enabled: true,
                },
            ),
            2,
        )
        .unwrap();
        let receipt = game.dispatch("a", request("blocked", command), 3).unwrap();
        let Ok(Outcome::Career(CareerOutcome::Delegated(report))) = receipt.result else {
            panic!()
        };
        assert_eq!(report["failure_count"], 1);
        assert_eq!(
            report["cases"][0]["note_key"],
            "be.msg.delegatedRenewals.notes.markedLetExpire"
        );
        game.dispatch(
            "a",
            request(
                "clear",
                CareerCommand::LetExpire {
                    player_id: "a-p".into(),
                    enabled: false,
                },
            ),
            4,
        )
        .unwrap();
        assert!(game.management.validate_social_renewal("a", "a-p").is_ok());
    }
}
