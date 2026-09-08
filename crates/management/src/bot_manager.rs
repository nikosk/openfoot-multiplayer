//! Native participant policy, not a second simulator or privileged AI path.
//! Source training/selection and owned-player sale thresholds are retained.
//! Shopping deliberately uses the same disclosed market rows as external actors:
//! no opponent morale, contract dates, potential, cash, or private plans.
//! Other repertoire is a conservative shared-rule policy (not claimed as exact
//! single-player AI cadence). Every recommendation is an ordinary receipted command.
use crate::career::{CareerCommand as C, ContractAction};
use crate::economy_runtime::EconomyCommand as E;
use crate::football::Football;
use crate::market::{MarketCommand as M, Status, Terms};
use crate::personnel::PersonnelCommand as P;
use crate::social::SocialCommand as S;
use crate::{Command, Error};
use chrono::{Datelike, Days};
use domain::message::ActionType;
use domain::staff::StaffRole;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotManagerPlan {
    pub policy: String,
    pub commands: Vec<Command>,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn game() -> Football {
        let clubs = ["a", "b", "c", "d"];
        let mut m = crate::Management::new(
            clubs
                .map(|id| crate::Club {
                    id: id.into(),
                    name: id.into(),
                    balance: 100_000,
                })
                .to_vec(),
            vec![],
            clubs
                .map(|id| crate::Manager {
                    id: id.into(),
                    club_id: id.into(),
                })
                .to_vec(),
            1,
            100,
        )
        .unwrap();
        let today = "2026-06-02".parse().unwrap(); // Tuesday: Technical/High for a healthy empty squad.
        m.configure_career(crate::career::CareerSetup {
            today,
            contracts: Default::default(),
            wage_budgets: clubs.map(|id| (id.into(), 10_000)).into(),
            reputations: clubs.map(|id| (id.into(), 500)).into(),
            staff_annual_wages: Default::default(),
        })
        .unwrap();
        m.training = Some(crate::training_commands::TrainingSetup {
            seed: 1,
            players: Default::default(),
            clubs: clubs.map(|id| (id.into(), Default::default())).into(),
        });
        let mut game = Football::new(m, vec![], vec![]).unwrap();
        let active = domain::league::League::new(
            "active".into(),
            "Active".into(),
            2026,
            &["a".into(), "b".into()],
        );
        let dormant = domain::league::League::new(
            "dormant".into(),
            "Dormant".into(),
            2026,
            &["c".into(), "d".into()],
        );
        game.competitions = Some(crate::competitions::CompetitionState {
            epoch_date: today,
            epoch_day: 1,
            archives: vec![],
            setup: crate::competitions::CompetitionSetup {
                seed: 1,
                primary_competition_id: "active".into(),
                competitions: [("active".into(), active), ("dormant".into(), dormant)].into(),
                active_competition_ids: ["active".into()].into(),
                competition_order: vec!["active".into(), "dormant".into()],
                catch_up_past: false,
                club_regions: Default::default(),
            },
        });
        game
    }
    #[test]
    fn dormant_clubs_train_via_same_commands_without_proactive_market_work() {
        let mut game = game();
        assert!(game.bot_club_active("a"));
        assert!(!game.bot_club_active("c"));
        let plan = game.bot_manager_plan("c", false).unwrap();
        assert_eq!(plan.commands.len(), 1);
        assert!(matches!(&plan.commands[0], Command::Training(_)));
        for (i, command) in plan.commands.into_iter().enumerate() {
            assert!(
                game.dispatch(
                    "c",
                    crate::Request {
                        id: format!("prep-{i}"),
                        day: 1,
                        command
                    },
                    1
                )
                .unwrap()
                .result
                .is_ok()
            );
        }
        assert!(game.bot_preparation_plan("c").unwrap().commands.is_empty());
        assert!(matches!(
            game.bot_preparation_plan("outsider"),
            Err(Error::Unauthorized)
        ));
    }
    #[test]
    fn an_active_match_day_skips_all_club_training_but_not_dormant_matches() {
        let mut game = game();
        let fixture = domain::league::Fixture {
            id: "match".into(),
            date: "2026-06-02".into(),
            home_team_id: "a".into(),
            away_team_id: "b".into(),
            ..Default::default()
        };
        game.competitions
            .as_mut()
            .unwrap()
            .setup
            .competitions
            .get_mut("active")
            .unwrap()
            .fixtures
            .push(fixture.clone());
        assert!(game.bot_preparation_plan("c").unwrap().commands.is_empty());
        game.competitions
            .as_mut()
            .unwrap()
            .setup
            .competitions
            .get_mut("active")
            .unwrap()
            .fixtures
            .clear();
        game.competitions
            .as_mut()
            .unwrap()
            .setup
            .competitions
            .get_mut("dormant")
            .unwrap()
            .fixtures
            .push(fixture);
        assert_eq!(game.bot_preparation_plan("c").unwrap().commands.len(), 1);
    }
}

impl Football {
    fn bot_club_active(&self, club: &str) -> bool {
        self.competitions.as_ref().is_none_or(|state| {
            state.setup.active_competition_ids.is_empty()
                || state.setup.competitions.values().any(|competition| {
                    state.setup.active_competition_ids.contains(&competition.id)
                        && competition.standings.iter().any(|row| row.team_id == club)
                })
        })
    }

    pub fn bot_preparation_plan(&self, actor: &str) -> Result<BotManagerPlan, Error> {
        let view = self.manager_view(actor)?;
        if view.ready {
            return Ok(BotManagerPlan {
                policy: "shared-observation-native-v1".into(),
                commands: vec![],
            });
        }
        let availability = self.availability_view(actor).ok();
        let squad: Vec<_> = self
            .squad(actor)?
            .into_iter()
            .filter(|p| {
                availability
                    .as_ref()
                    .is_none_or(|a| a.get(&p.id).is_none_or(|a| a.is_available()))
            })
            .collect();
        let selected = if squad.len() < 11 || !self.bot_club_active(&view.club.id) {
            None
        } else if let (Ok(plan), Ok(profiles)) =
            (self.squad_plan(actor), self.position_profiles(actor))
        {
            crate::squad_plan::select(&squad, &profiles, &[], &plan)
                .ok()
                .map(|s| s.players.into_iter().map(|p| p.id).collect())
        } else {
            crate::selection::select(&squad, &[])
                .ok()
                .map(|s| s.0.into_iter().map(|p| p.id).collect())
        };
        self.bot_preparation(actor, selected)
    }
    pub fn bot_manager_plan(
        &self,
        actor: &str,
        responses_only: bool,
    ) -> Result<BotManagerPlan, Error> {
        let view = self.manager_view(actor)?;
        let club = &view.club.id;
        // The source market shops only active-scope clubs. Dormant source clubs
        // still train; there is no implemented periodic dormant shopping pass.
        if !responses_only && !view.ready && !self.bot_club_active(club) {
            return self.bot_preparation_plan(actor);
        }
        let mut commands = vec![];
        let mut squad = self.squad(actor)?;
        let availability = self.availability_view(actor).ok();
        squad.retain(|p| {
            availability
                .as_ref()
                .is_none_or(|a| a.get(&p.id).is_none_or(|a| a.is_available()))
        });
        let selected = if squad.len() >= 11 {
            if let (Ok(plan), Ok(profiles)) =
                (self.squad_plan(actor), self.position_profiles(actor))
            {
                crate::squad_plan::select(&squad, &profiles, &[], &plan)
                    .ok()
                    .map(|s| s.players.into_iter().map(|p| p.id).collect::<Vec<_>>())
            } else {
                crate::selection::select(&squad, &[])
                    .ok()
                    .map(|s| s.0.into_iter().map(|p| p.id).collect())
            }
        } else {
            None
        };
        let starters: BTreeSet<_> = selected.clone().unwrap_or_default().into_iter().collect();
        let career = self.career_view(actor).ok();
        let finance = self.economy_view(actor).ok();
        let personnel = self.personnel_view(actor).ok();
        let date = self.career_date();
        // Both sides inspect pending offers; proposer re-confirms only when its
        // original financial consent was invalidated by an intervening purchase.
        if let (Ok(market), Some(career)) = (self.market_view(actor), &career) {
            for offer in market
                .offers
                .iter()
                .filter(|o| o.status == Status::Pending)
                .take(12)
            {
                if self.management.market_needs_consent(actor, offer.id) == Ok(false) {
                    continue;
                }
                if offer.proposer == *club {
                    if self
                        .management
                        .market_needs_consent(actor, offer.id)
                        .unwrap_or(false)
                    {
                        commands.push(Command::Market(M::Review { offer_id: offer.id }));
                    }
                    continue;
                }
                let accept = if offer.seller == *club {
                    match &offer.terms {
                        Terms::Transfer { fee } => {
                            let threshold = career
                                .contracts
                                .get(&offer.player_id)
                                .map(|contract| {
                                    let mut mult: f64 = if self
                                        .management
                                        .social
                                        .as_ref()
                                        .and_then(|s| s.source_players.get(&offer.player_id))
                                        .is_some_and(|p| p.transfer_listed)
                                    {
                                        0.8
                                    } else {
                                        1.2
                                    };
                                    let remaining =
                                        contract.end_date.map(|end| (end - career.date).num_days());
                                    if remaining.is_some_and(|d| d <= 60) {
                                        mult -= 0.25
                                    } else if remaining.is_some_and(|d| d <= 180) {
                                        mult -= 0.15
                                    } else if remaining.is_some_and(|d| d <= 365) {
                                        mult -= 0.05
                                    }
                                    if starters.contains(&offer.player_id) {
                                        mult += 0.2
                                    } else if contract.market_value >= 1_500_000 {
                                        mult += 0.1
                                    }
                                    if contract.morale <= 40 {
                                        mult -= 0.05
                                    }
                                    // No undisclosed buyer reputation/opponent profile.
                                    (contract.market_value as f64 * mult.clamp(0.55, 1.6)).round()
                                        as u64
                                })
                                .unwrap_or(u64::MAX);
                            if *fee >= threshold {
                                true
                            } else {
                                commands.push(Command::Market(M::Counter {
                                    offer_id: offer.id,
                                    terms: Terms::Transfer {
                                        fee: crate::market_rules::round_transfer_fee(threshold),
                                    },
                                }));
                                false
                            }
                        }
                        Terms::Loan {
                            wage_contribution_pct,
                            ..
                        } => !starters.contains(&offer.player_id) && *wage_contribution_pct >= 50,
                    }
                } else {
                    match offer.terms {
                        Terms::Transfer { fee } => finance.as_ref().is_some_and(|f| {
                            fee <= f.balance.max(0) as u64 / 4
                                && fee <= f.account.transfer_budget.max(0) as u64
                        }),
                        Terms::Loan {
                            wage_contribution_pct,
                            ..
                        } => wage_contribution_pct <= 100 && squad.len() < 24,
                    }
                };
                if accept {
                    commands.push(Command::Market(M::Review { offer_id: offer.id }));
                } else if !commands.iter().any(
                    |c| matches!(c,Command::Market(M::Counter{offer_id,..}) if *offer_id==offer.id),
                ) {
                    commands.push(Command::Market(M::Reject { offer_id: offer.id }));
                }
            }
        } else {
            // Legacy fixtures remain supported without silently giving their
            // fee-only offers richer configured-market transaction semantics.
            for offer in &view.offers {
                if offer.seller == *club && offer.status == crate::OfferStatus::Pending {
                    commands.push(Command::Reject { offer_id: offer.id });
                }
            }
        }
        if view.ready || responses_only {
            return Ok(BotManagerPlan {
                policy: "shared-observation-native-v1".into(),
                commands,
            });
        }

        if let Ok(messages) = self.inbox_view(actor, 0, 100) {
            for message in messages {
                for action in message.actions.iter().filter(|a| !a.resolved) {
                    let option = match &action.action_type {
                        ActionType::ChooseOption { options } => {
                            let wanted =
                                if message.id.starts_with("sponsor_") {
                                    Some("accept")
                                } else if message.id.starts_with("morale_talk_") {
                                    Some("encourage")
                                } else if message.id.starts_with("bench_complaint_") {
                                    Some("explain")
                                } else if message.id.starts_with("happy_player_") {
                                    Some("praise_back")
                                } else if message.id.starts_with("contract_concern_") {
                                    Some("reassure")
                                } else if action.id.starts_with("prospect:") {
                                    let prospect =
                                        message.context.youth_prospects.as_ref().and_then(
                                            |players| {
                                                players.iter().find(|p| {
                                                    action.id == format!("prospect:{}", p.id)
                                                })
                                            },
                                        );
                                    Some(
                                        if prospect
                                            .is_some_and(|p| p.potential >= 75 || squad.len() < 18)
                                        {
                                            "sign"
                                        } else {
                                            "discard"
                                        },
                                    )
                                } else {
                                    None
                                };
                            let Some(wanted) =
                                wanted.filter(|id| options.iter().any(|o| o.id == *id))
                            else {
                                continue;
                            };
                            Some(wanted.into())
                        }
                        _ => None,
                    };
                    commands.push(Command::Social(S::Respond {
                        message_id: message.id.clone(),
                        action_id: action.id.clone(),
                        option_id: option,
                    }));
                }
                if !message.read {
                    commands.push(Command::Social(S::MarkRead {
                        message_id: message.id,
                    }));
                }
                if commands.len() >= 24 {
                    break;
                }
            }
        }
        if let Some(career) = &career {
            let due: Vec<_> = career
                .renewal_terms
                .iter()
                .filter(|(id, term)| {
                    term.days_remaining <= 180 && !career.contracts[*id].let_expire
                })
                .collect();
            if due.len() >= 3
                && personnel.as_ref().is_some_and(|p| {
                    p.staff
                        .iter()
                        .any(|s| s.role == StaffRole::AssistantManager)
                })
            {
                commands.push(Command::Career(C::Delegate(
                    crate::delegated_contracts::Delegation {
                        player_ids: Some(due.iter().map(|(id, _)| (*id).clone()).collect()),
                        max_wage_increase_pct: 50,
                        max_contract_years: 5,
                    },
                )));
            } else {
                for (id, terms) in due.into_iter().take(5) {
                    commands.push(Command::Career(C::Review {
                        action: ContractAction::Renew {
                            player_id: id.clone(),
                            weekly_wage: terms.expected_wage,
                            years: terms.expected_years,
                        },
                    }));
                }
            }
            if squad.len() < 22 {
                if let Ok(free) = self.free_agent_squad(actor) {
                    let target = free
                        .iter()
                        .filter(|p| {
                            squad.len() < 18
                                || p.ovr > squad.iter().map(|p| p.ovr).min().unwrap_or(0)
                        })
                        .max_by_key(|p| (p.ovr, std::cmp::Reverse(&p.id)));
                    if let Some((target, terms)) = target.and_then(|p| {
                        career
                            .free_agents
                            .iter()
                            .find(|f| f.id == p.id)
                            .map(|f| (p, f))
                    }) {
                        commands.push(Command::Career(C::Review {
                            action: ContractAction::Sign {
                                player_id: target.id.clone(),
                                weekly_wage: terms.expected_wage,
                                years: terms.expected_years,
                            },
                        }));
                    }
                }
            }
        }
        if let Some(finance) = &finance {
            if finance.board_support.is_some() {
                commands.push(Command::Economy(E::RequestBoardSupport));
            } else if finance.marketing_campaign.is_some() {
                commands.push(Command::Economy(E::RequestMarketingCampaign));
            }
            if finance.sponsor_pitch.is_some() {
                commands.push(Command::Economy(E::RequestSponsorPitch));
            }
        }
        let market_rows = self
            .browse_transfer_market(
                actor,
                &crate::scouting::MarketFilter {
                    position: None,
                    max_price: None,
                    listed_only: None,
                },
            )
            .ok();
        if let (Some(personnel), Some(finance)) = (&personnel, &finance) {
            if let Ok(staff_market) = self.staff_market(actor) {
                for role in [
                    StaffRole::AssistantManager,
                    StaffRole::Coach,
                    StaffRole::Scout,
                    StaffRole::Physio,
                ] {
                    if personnel.staff.iter().any(|staff| staff.role == role) {
                        continue;
                    }
                    let score = |s: &domain::staff::Staff| match role {
                        StaffRole::AssistantManager | StaffRole::Coach => s.attributes.coaching,
                        StaffRole::Scout => s.attributes.judging_ability,
                        StaffRole::Physio => s.attributes.physiotherapy,
                    };
                    if let Some(staff) = staff_market
                        .iter()
                        .filter(|s| {
                            s.role == role
                                && finance
                                    .snapshot
                                    .annual_wage_bill
                                    .saturating_add(i64::from(s.wage))
                                    <= finance.account.wage_budget
                                && finance.balance >= i64::from(s.wage)
                        })
                        .max_by_key(|s| (score(s), std::cmp::Reverse(&s.id)))
                    {
                        commands.push(Command::Personnel(P::ReviewStaff {
                            staff_id: staff.id.clone(),
                            action: crate::staff::Action::Hire,
                        }));
                        break;
                    }
                }
                // Release only duplicate-role staff when payroll is over budget.
                if finance.snapshot.currently_over_budget {
                    if let Some(staff) = personnel
                        .staff
                        .iter()
                        .filter(|s| {
                            personnel
                                .staff
                                .iter()
                                .filter(|other| other.role == s.role)
                                .count()
                                > 1
                        })
                        .max_by_key(|s| s.wage)
                    {
                        commands.push(Command::Personnel(P::ReviewStaff {
                            staff_id: staff.id.clone(),
                            action: crate::staff::Action::Release,
                        }));
                    }
                }
            }
            let used: BTreeSet<_> = personnel
                .scouting
                .as_ref()
                .map(|desk| {
                    desk.assignments
                        .iter()
                        .map(|a| a.scout_id.clone())
                        .chain(desk.youth_assignments.iter().map(|a| a.scout_id.clone()))
                        .collect()
                })
                .unwrap_or_default();
            if let Some(scout) = personnel
                .staff
                .iter()
                .find(|s| s.role == StaffRole::Scout && !used.contains(&s.id))
            {
                let scout_target = market_rows.as_ref().and_then(|market| {
                    market
                        .players
                        .iter()
                        .filter(|p| p.team != view.club.name)
                        .max_by_key(|p| (p.ovr, std::cmp::Reverse(&p.id)))
                });
                if date.is_some_and(|d| d.ordinal() % 2 == 0) && scout_target.is_some() {
                    commands.push(Command::Personnel(P::ScoutPlayer {
                        scout_id: scout.id.clone(),
                        player_id: scout_target.unwrap().id.clone(),
                    }));
                } else {
                    commands.push(Command::Personnel(P::StartYouth {
                        scout_id: scout.id.clone(),
                        region: crate::scouting::YouthRegion::Domestic,
                        objective: crate::scouting::YouthObjective::HighPotential,
                        target_position: None,
                    }));
                }
            }
            if date.is_some_and(|d| d.weekday() == chrono::Weekday::Mon)
                && finance.snapshot.overall_status == crate::economy::Health::Stable
            {
                let facility = if personnel.facilities.training < 6 {
                    Some(("Training", domain::team::FacilityType::Training))
                } else if personnel.facilities.scouting < 3 {
                    Some(("Scouting", domain::team::FacilityType::Scouting))
                } else if personnel.facilities.medical < 5 {
                    Some(("Medical", domain::team::FacilityType::Medical))
                } else {
                    None
                };
                if let Some((name, kind)) = facility {
                    let cost = crate::facilities::next_upgrade_cost(&personnel.facilities, &kind);
                    if cost <= finance.balance / 10 {
                        commands.push(Command::Personnel(P::ReviewFacility {
                            facility: name.into(),
                        }));
                    }
                }
            }
        }
        if let (Ok(market), Some(finance), Some(rows), Some(date)) =
            (self.market_view(actor), &finance, &market_rows, date)
        {
            if let Some(source) = &self.management.social {
                let mut candidates: Vec<_> = source
                    .source_players
                    .values()
                    .filter(|p| {
                        self.management.players[&p.id].club_id == *club
                            && !starters.contains(&p.id)
                            && !p.retired
                            && p.active_loan.is_none()
                            && !p.transfer_listed
                            && !p.loan_listed
                    })
                    .collect();
                candidates.sort_by(|a, b| {
                    b.date_of_birth
                        .cmp(&a.date_of_birth)
                        .then_with(|| b.potential.cmp(&a.potential))
                        .then_with(|| a.id.cmp(&b.id))
                });
                let listed = source
                    .source_players
                    .values()
                    .filter(|p| self.management.players[&p.id].club_id == *club && p.loan_listed)
                    .count();
                for player in candidates.into_iter().take(2usize.saturating_sub(listed)) {
                    if squad.len() > 18 {
                        commands.push(Command::Market(M::List {
                            player_id: player.id.clone(),
                            transfer_listed: false,
                            loan_listed: true,
                        }));
                    }
                }
                if finance.snapshot.currently_over_budget {
                    if let Some(player) = squad
                        .iter()
                        .filter(|p| !starters.contains(&p.id))
                        .min_by_key(|p| p.ovr)
                    {
                        commands.push(Command::Market(M::List {
                            player_id: player.id.clone(),
                            transfer_listed: true,
                            loan_listed: false,
                        }));
                    }
                }
            }
            if matches!(
                market.window.status,
                domain::season::TransferWindowStatus::Open
                    | domain::season::TransferWindowStatus::DeadlineDay
            ) && squad.len() < 24
            {
                let max_fee =
                    (finance.balance.max(0) / 5).min(finance.account.transfer_budget.max(0));
                let target = rows
                    .players
                    .iter()
                    .filter(|p| {
                        p.team != view.club.name
                            && p.team != "Free"
                            && !market.offers.iter().any(|o| {
                                o.player_id == p.id
                                    && matches!(
                                        o.status,
                                        Status::Pending | Status::PendingRegistration
                                    )
                            })
                    })
                    .filter(|p| {
                        squad.len() < 18 || p.ovr > squad.iter().map(|p| p.ovr).min().unwrap_or(0)
                    })
                    .filter(|p| p.listed == "L" || u64::from(p.wage) * 52 <= max_fee as u64)
                    .max_by_key(|p| (p.ovr, std::cmp::Reverse(&p.id)));
                if let Some(player) = target {
                    let terms = if player.listed == "L" {
                        date.checked_add_days(Days::new(180))
                            .map(|end_date| Terms::Loan {
                                end_date,
                                wage_contribution_pct: 100,
                                buy_option_fee: None,
                            })
                    } else {
                        Some(Terms::Transfer {
                            fee: u64::from(player.wage) * 52,
                        })
                    };
                    if let Some(terms) = terms {
                        commands.push(Command::Market(M::Bid {
                            player_id: player.id.clone(),
                            terms,
                        }));
                    }
                }
                for (id, loan) in &market.active_loans {
                    if loan.loan_team_id == *club
                        && loan.buy_option_fee.is_some_and(|fee| fee <= max_fee as u64)
                        && starters.contains(id)
                    {
                        commands.push(Command::Market(M::ReviewBuyOption {
                            player_id: id.clone(),
                        }));
                        break;
                    }
                }
            }
        }
        commands.extend(self.bot_preparation(actor, selected)?.commands);
        Ok(BotManagerPlan {
            policy: "shared-observation-native-v1".into(),
            commands,
        })
    }

    fn bot_preparation(
        &self,
        actor: &str,
        selected: Option<Vec<String>>,
    ) -> Result<BotManagerPlan, Error> {
        let club = self.manager_view(actor)?.club.id;
        let mut commands = vec![];
        let availability = self.availability_view(actor).ok();
        // Training's source availability excludes injury, not fatigue. Omitting
        // exhausted players here would suppress the recovery-crisis threshold.
        let squad: Vec<_> = self
            .squad(actor)?
            .into_iter()
            .filter(|p| {
                availability
                    .as_ref()
                    .is_none_or(|a| a.get(&p.id).is_none_or(|a| a.injury.is_none()))
            })
            .collect();
        // Source applies the all-team AI training policy only on days without
        // an actively simulated club match, including for dormant clubs.
        let match_day = if let (Some(state), Some(date)) = (&self.competitions, self.career_date())
        {
            state.setup.competitions.values().any(|c| {
                state.setup.active_competition_ids.contains(&c.id)
                    && c.kind != domain::league::CompetitionType::InternationalNation
                    && c.fixtures.iter().any(|f| {
                        f.date == date.to_string()
                            && f.status == domain::league::FixtureStatus::Scheduled
                    })
            })
        } else {
            self.fixtures
                .iter()
                .any(|f| f.day == self.management.window.day)
        };
        if let (Ok(training), Some(date), false) =
            (self.training_view(actor), self.career_date(), match_day)
        {
            let upcoming: Vec<_> = if let Some(state) = &self.competitions {
                // Pinned snapshot_team reads the legacy primary league only.
                state.setup.competitions[&state.setup.primary_competition_id]
                    .fixtures
                    .iter()
                    .filter(|f| {
                        f.status == domain::league::FixtureStatus::Scheduled
                            && (f.home_team_id == club || f.away_team_id == club)
                    })
                    .filter_map(|f| f.date.parse::<chrono::NaiveDate>().ok())
                    .filter_map(|d| u32::try_from((d - date).num_days()).ok())
                    .collect()
            } else {
                self.fixtures
                    .iter()
                    .filter(|f| {
                        f.day >= self.management.window.day && (f.home == club || f.away == club)
                    })
                    .map(|f| f.day - self.management.window.day)
                    .collect()
            };
            let style = self.match_plan(actor)?.play_style;
            if let Some((focus, intensity)) = crate::bot_training::choose(
                style,
                training.plan.schedule,
                date.weekday().num_days_from_monday(),
                &squad.iter().map(|p| p.condition).collect::<Vec<_>>(),
                upcoming.iter().copied().min(),
                upcoming.iter().filter(|d| **d <= 7).count(),
            )
            .map_err(Error::Contract)?
            {
                if training.plan.focus != focus || training.plan.intensity != intensity {
                    commands.push(Command::Training(
                        crate::training_commands::TrainingCommand::SetPlan {
                            focus,
                            intensity,
                            schedule: training.plan.schedule,
                        },
                    ));
                }
                for player in &squad {
                    let focus = (player.condition < 40).then_some(crate::training::Focus::Recovery);
                    if training.individual_focus.get(&player.id) != Some(&focus) {
                        commands.push(Command::Training(
                            crate::training_commands::TrainingCommand::SetIndividualFocus {
                                player_id: player.id.clone(),
                                focus,
                            },
                        ));
                    }
                }
            }
        }
        if let Some(player_ids) = selected {
            if self.lineup(actor)? != player_ids {
                commands.push(Command::SetLineup { player_ids });
            }
        }
        if self
            .recovery_view(actor)
            .is_ok_and(|v| v.mode != crate::recovery::RecoveryMode::Recovery)
        {
            commands.push(Command::SetRecovery {
                mode: crate::recovery::RecoveryMode::Recovery,
            });
        }
        Ok(BotManagerPlan {
            policy: "shared-observation-native-v1".into(),
            commands,
        })
    }
}
