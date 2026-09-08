//! Retain the source's rich renewal state alongside canonical contract terms.
//! Adapted from pinned contracts/{renewals,termination,free_agent}. GPL-3.0-or-later.
use crate::career::{CareerCommand, CareerOutcome, ContractAction};
use crate::contracts::Decision;
use crate::{Error, Management};
use domain::player::{
    ContractExitIntent, RenewalSessionOutcome as Outcome, RenewalSessionStatus as Status,
};

impl Management {
    pub(crate) fn contract_event(
        &mut self,
        player_id: &str,
        club: &str,
        kind: &str,
        extra: Option<(&str, String)>,
    ) -> Result<(), Error> {
        let Some(social) = &mut self.social else {
            return Ok(());
        };
        let source = social
            .source_players
            .get(player_id)
            .ok_or(Error::Unavailable)?;
        let today = self
            .career
            .as_ref()
            .ok_or(Error::Unavailable)?
            .today
            .to_string();
        let mut params = std::collections::HashMap::from([
            ("player".into(), source.match_name.clone()),
            ("team".into(), self.clubs[club].name.clone()),
        ]);
        if let Some((key, value)) = extra {
            params.insert(key.into(), value);
        }
        let (prefix, key) = match kind {
            "signed" => ("free_agent_signed", "freeAgentSigned"),
            "terminated" => ("contract_terminated", "contractTerminated"),
            _ => ("contract_expired", "contractExpired"),
        };
        // Source appends events with repeated IDs. Recipient storage requires an
        // occurrence identity so a later re-sign/release is not silently dropped.
        let id = format!(
            "{prefix}_{player_id}_{today}_{}_{}",
            self.sequence,
            self.career.as_ref().unwrap().releases.len()
        );
        let message = domain::message::InboxMessage::new(
            id,
            String::new(),
            String::new(),
            String::new(),
            today,
        )
        .with_category(domain::message::MessageCategory::Contract)
        .with_priority(domain::message::MessagePriority::Urgent)
        .with_sender_role("")
        .with_i18n(
            &format!("be.msg.{key}.subject"),
            &format!("be.msg.{key}.body"),
            params,
        )
        .with_sender_i18n("be.sender.assistantManager", "be.role.assistantManager");
        for manager in self.managers.values().filter(|m| m.club_id == club) {
            social
                .inbox
                .deliver(&manager.id, message.clone())
                .map_err(|e| Error::Contract(format!("{e:?}")))?;
        }
        Ok(())
    }
    pub(crate) fn sync_contract_social(
        &mut self,
        command: &CareerCommand,
        outcome: &CareerOutcome,
    ) -> Result<(), Error> {
        let Some(social) = self.social.as_mut() else {
            return Ok(());
        };
        let career = self.career.as_mut().ok_or(Error::Unavailable)?;
        let today = career.today.to_string();
        match outcome {
            CareerOutcome::LetExpireSet { player_id, enabled } => {
                let source = social
                    .source_players
                    .get_mut(player_id)
                    .ok_or(Error::Unavailable)?;
                if *enabled {
                    let state = source
                        .morale_core
                        .renewal_state
                        .get_or_insert_with(Default::default);
                    state.status = Status::Blocked;
                    state.manager_blocked_until = None;
                    state.last_attempt_date = Some(today.clone());
                    state.last_outcome = Some(Outcome::BlockedByManager);
                    state.conversation_round = 0;
                    state.exit_intent = Some(ContractExitIntent::LetExpire {
                        set_on: today,
                        reason: None,
                    });
                } else if let Some(state) = &mut source.morale_core.renewal_state {
                    let had = state.exit_intent.take().is_some();
                    if had && state.status == Status::Blocked {
                        state.status = Status::Idle;
                        state.manager_blocked_until = None;
                        state.last_outcome = None;
                        state.conversation_round = 0;
                    }
                }
            }
            CareerOutcome::Decision {
                player_id,
                decision,
            } if matches!(
                command,
                CareerCommand::Review {
                    action: ContractAction::Renew { .. }
                }
            ) =>
            {
                let contract = &career.contracts[player_id];
                // Invalid terms and an already closed session do not constitute
                // a new negotiation attempt in the source.
                if matches!(decision,Decision::Rejected{reason,..} if matches!(reason.as_str(),"Invalid contract years"|"Negotiation blocked"|"Already agreed today"))
                {
                    return Ok(());
                }
                let state = social
                    .source_players
                    .get_mut(player_id)
                    .ok_or(Error::Unavailable)?
                    .morale_core
                    .renewal_state
                    .get_or_insert_with(Default::default);
                state.last_attempt_date = contract.last_attempt.map(|d| d.to_string());
                state.conversation_round = contract.round.min(255) as u8;
                state.manager_blocked_until = contract.blocked_until.map(|d| d.to_string());
                match decision {
                    Decision::Counter { .. } => {
                        state.status = Status::Open;
                        state.last_outcome = Some(Outcome::Stalled);
                    }
                    Decision::Rejected { blocked_until, .. } => {
                        state.status = if blocked_until.is_some() {
                            Status::Blocked
                        } else {
                            Status::Stalled
                        };
                        state.last_outcome = Some(if blocked_until.is_some() {
                            Outcome::BlockedByManager
                        } else {
                            Outcome::RejectedByPlayer
                        });
                    }
                    Decision::Accepted => {}
                }
            }
            CareerOutcome::Applied { player_id, action } => {
                let jersey = if matches!(action, ContractAction::Sign { .. }) {
                    let club = &self.players[player_id].club_id;
                    let used: std::collections::BTreeSet<_> = social
                        .source_players
                        .values()
                        .filter(|p| p.id != *player_id && p.team_id.as_ref() == Some(club))
                        .filter_map(|p| p.jersey_number)
                        .collect();
                    social.source_players[player_id]
                        .jersey_number
                        .filter(|n| !used.contains(n))
                        .or_else(|| (1..=99).find(|n| !used.contains(n)))
                } else {
                    None
                };
                let source = social
                    .source_players
                    .get_mut(player_id)
                    .ok_or(Error::Unavailable)?;
                let contract = career
                    .contracts
                    .get_mut(player_id)
                    .ok_or(Error::Unavailable)?;
                source.wage = contract.weekly_wage;
                source.contract_end = contract.end_date.map(|d| d.to_string());
                match action {
                    ContractAction::Renew { .. } => {
                        let state = source
                            .morale_core
                            .renewal_state
                            .get_or_insert_with(Default::default);
                        state.status = Status::Agreed;
                        state.manager_blocked_until = None;
                        state.last_attempt_date = Some(today);
                        state.last_outcome = Some(Outcome::AcceptedByManager);
                        state.conversation_round = contract.round.min(255) as u8;
                        state.exit_intent = None;
                    }
                    ContractAction::Sign { .. } => {
                        source.jersey_number = jersey;
                        source.team_id = Some(self.players[player_id].club_id.clone());
                        source.transfer_listed = false;
                        source.loan_listed = false;
                        source.transfer_offers.clear();
                        source.morale = contract.morale;
                        source.morale_core.manager_trust = contract.manager_trust;
                        source.morale_core.renewal_state = None;
                        if source
                            .morale_core
                            .unresolved_issue
                            .as_ref()
                            .is_some_and(|i| {
                                i.category == domain::player::PlayerIssueCategory::Contract
                            })
                        {
                            source.morale_core.unresolved_issue = None;
                        }
                        contract.unresolved_issue = source.morale_core.unresolved_issue.is_some();
                        source
                            .movement_history
                            .push(domain::player::PlayerMovementEntry {
                                date: today,
                                kind: domain::player::PlayerMovementKind::FreeAgentSigning,
                                from_team_id: None,
                                from_team_name: None,
                                to_team_id: source.team_id.clone(),
                                to_team_name: Some(
                                    self.clubs[&self.players[player_id].club_id].name.clone(),
                                ),
                                fee: None,
                                loan_end_date: None,
                            });
                    }
                    ContractAction::Terminate { .. } => {
                        source.team_id = None;
                        source.morale_core.renewal_state = None;
                    }
                }
            }
            _ => {}
        }
        if let CareerOutcome::Applied {
            player_id,
            action: ContractAction::Sign { years, .. },
        } = outcome
        {
            let club = self.players[player_id].club_id.clone();
            self.contract_event(
                player_id,
                &club,
                "signed",
                Some(("years", years.to_string())),
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    fn game() -> Management {
        let date = "2026-06-01".parse().unwrap();
        let mut m = Management::new(
            vec![crate::Club {
                id: "a".into(),
                name: "A".into(),
                balance: 1_000_000,
            }],
            ["p", "q"]
                .map(|id| crate::Player {
                    id: id.into(),
                    name: id.into(),
                    club_id: "a".into(),
                })
                .to_vec(),
            vec![crate::Manager {
                id: "manager".into(),
                club_id: "a".into(),
            }],
            1,
            100,
        )
        .unwrap();
        m.configure_career(crate::career::CareerSetup {
            today: date,
            contracts: ["p", "q"]
                .map(|id| {
                    (
                        id.into(),
                        crate::contracts::PlayerContract::new(
                            "2000-01-01".parse().unwrap(),
                            100,
                            Some(date),
                            100_000,
                            60,
                            60,
                        ),
                    )
                })
                .into(),
            wage_budgets: [("a".into(), 100_000)].into(),
            reputations: [("a".into(), 500)].into(),
            staff_annual_wages: Default::default(),
        })
        .unwrap();
        let players=["p","q"].map(|id|{
            let attrs=serde_json::from_value(serde_json::json!({"pace":60,"stamina":60,"strength":60,"passing":60,"shooting":60,"tackling":60,"dribbling":60,"defending":60,"positioning":60,"vision":60,"decisions":60,"composure":60,"leadership":60,"aggression":60})).unwrap();
            let mut p=domain::player::Player::new(id.into(),id.into(),id.into(),"2000-01-01".into(),"ENG".into(),domain::player::Position::Midfielder,attrs);
            p.team_id=Some("a".into());p.jersey_number=Some(7);p.contract_end=Some(date.to_string());p.wage=100;
            p.morale_core.renewal_state=Some(Default::default());
            (id.into(),p)
        }).into();
        m.social = Some(crate::social::SocialState {
            seed: 1,
            source_players: players,
            inbox: Default::default(),
        });
        m
    }
    #[test]
    fn sign_resolves_jersey_and_only_clears_contract_issue_in_both_registries() {
        let mut m = game();
        for category in [
            domain::player::PlayerIssueCategory::Contract,
            domain::player::PlayerIssueCategory::PlayingTime,
        ] {
            m.social
                .as_mut()
                .unwrap()
                .source_players
                .get_mut("p")
                .unwrap()
                .morale_core
                .unresolved_issue = Some(domain::player::PlayerIssue {
                category: category.clone(),
                severity: 1,
            });
            m.career
                .as_mut()
                .unwrap()
                .contracts
                .get_mut("p")
                .unwrap()
                .unresolved_issue = true;
            let action = ContractAction::Sign {
                player_id: "p".into(),
                weekly_wage: 100,
                years: 2,
            };
            m.sync_contract_social(
                &CareerCommand::Confirm { preview_id: 1 },
                &CareerOutcome::Applied {
                    player_id: "p".into(),
                    action,
                },
            )
            .unwrap();
            assert_eq!(
                m.social.as_ref().unwrap().source_players["p"].jersey_number,
                Some(1)
            );
            assert_eq!(
                m.career.as_ref().unwrap().contracts["p"].unresolved_issue,
                category != domain::player::PlayerIssueCategory::Contract
            );
        }
        assert_eq!(m.inbox_view("manager", 0, 20).unwrap().len(), 1);
    }
    #[test]
    fn expiry_records_release_clears_renewal_and_notifies_owner() {
        let mut m = game();
        m.advance_career("2026-06-02".parse().unwrap()).unwrap();
        let p = &m.social.as_ref().unwrap().source_players["p"];
        assert!(p.team_id.is_none() && p.morale_core.renewal_state.is_none());
        assert!(matches!(
            p.movement_history.last().unwrap().kind,
            domain::player::PlayerMovementKind::Released
        ));
        assert_eq!(m.inbox_view("manager", 0, 20).unwrap().len(), 2);
    }
    #[test]
    fn retired_bootstrap_requires_exact_source_metadata() {
        let original = game();
        let mut m = original.clone();
        let c = m.career.take().unwrap();
        m.players.get_mut("p").unwrap().club_id.clear();
        let mut contracts = c.contracts;
        contracts.get_mut("p").unwrap().end_date = None;
        let setup = crate::career::CareerSetup {
            today: c.today,
            contracts,
            wage_budgets: c.wage_budgets,
            reputations: c.reputations,
            staff_annual_wages: BTreeMap::new(),
        };
        assert!(m.configure_career(setup.clone()).is_err());
        m.configure_career_with_retired(setup, ["p".into()].into())
            .unwrap();
        assert!(
            m.validate_initial_retired_source(&m.social.as_ref().unwrap().source_players)
                .is_err()
        );
        let p = m
            .social
            .as_mut()
            .unwrap()
            .source_players
            .get_mut("p")
            .unwrap();
        p.retired = true;
        p.team_id = None;
        p.contract_end = None;
        m.validate_initial_retired_source(&m.social.as_ref().unwrap().source_players)
            .unwrap();
        assert_eq!(m.career.as_ref().unwrap().contracts["p"].weekly_wage, 100);
        m.validate_career_checkpoint().unwrap();
    }
}
