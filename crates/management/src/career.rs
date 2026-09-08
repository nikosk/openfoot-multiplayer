//! Optional dated contract lifecycle. Empty player club IDs denote free agents;
//! every player remains in the authoritative registry and engine attribute table.
//! Source naming calls offers "weekly_wage", but executable finance charges each
//! raw player/staff wage divided by 52 on Mondays. Preserve that asymmetry.
use crate::contracts::{Decision, PlayerContract, wage_policy_allows};
use crate::{Error, Management, OfferStatus};
use chrono::{Datelike, Days, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CareerSetup {
    pub today: NaiveDate,
    pub contracts: BTreeMap<String, PlayerContract>,
    pub wage_budgets: BTreeMap<String, u64>,
    pub reputations: BTreeMap<String, u32>,
    #[serde(default)]
    pub staff_annual_wages: BTreeMap<String, Vec<u32>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CareerState {
    #[serde(default)]
    initial_retired_player_ids: BTreeSet<String>,
    pub today: NaiveDate,
    pub contracts: BTreeMap<String, PlayerContract>,
    pub wage_budgets: BTreeMap<String, u64>,
    pub reputations: BTreeMap<String, u32>,
    pub staff_annual_wages: BTreeMap<String, Vec<u32>>,
    pub ledger: Vec<LedgerEntry>,
    pub releases: Vec<ReleaseRecord>,
    #[serde(with = "crate::checkpoint::entries")]
    negotiations: BTreeMap<(String, String), PlayerContract>,
    previews: BTreeMap<u64, StoredContractPreview>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub date: NaiveDate,
    pub club_id: String,
    pub amount: i64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseRecord {
    pub date: NaiveDate,
    pub player_id: String,
    pub club_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContractAction {
    Renew {
        player_id: String,
        weekly_wage: u32,
        years: u32,
    },
    Sign {
        player_id: String,
        weekly_wage: u32,
        years: u32,
    },
    Terminate {
        player_id: String,
    },
}
impl ContractAction {
    fn player_id(&self) -> &str {
        match self {
            Self::Renew { player_id, .. }
            | Self::Sign { player_id, .. }
            | Self::Terminate { player_id } => player_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CareerCommand {
    Delegate(crate::delegated_contracts::Delegation),
    Review { action: ContractAction },
    Confirm { preview_id: u64 },
    LetExpire { player_id: String, enabled: bool },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CareerPreview {
    pub id: u64,
    pub player_id: String,
    pub action: ContractAction,
    pub balance_after: i64,
    pub projected_wage_total: u64,
    pub contract_end: Option<NaiveDate>,
    pub severance: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CareerOutcome {
    Delegated(serde_json::Value),
    Preview(CareerPreview),
    RefreshRequired(CareerPreview),
    Decision {
        player_id: String,
        decision: Decision,
    },
    Applied {
        player_id: String,
        action: ContractAction,
    },
    LetExpireSet {
        player_id: String,
        enabled: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreeAgentView {
    pub id: String,
    pub name: String,
    pub expected_wage: u32,
    pub expected_years: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenewalTerms {
    pub expected_wage: u32,
    pub expected_years: u32,
    pub days_remaining: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CareerView {
    pub date: NaiveDate,
    pub contracts: BTreeMap<String, PlayerContract>,
    pub renewal_terms: BTreeMap<String, RenewalTerms>,
    pub free_agents: Vec<FreeAgentView>,
    pub wage_budget: u64,
    pub current_wage_total: u64,
    pub ledger: Vec<LedgerEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Dependencies {
    date: NaiveDate,
    balance: i64,
    club_revision: u64,
    player_revision: u64,
    owner: String,
    contract: PlayerContract,
    negotiation: PlayerContract,
    wage_total: u64,
    wage_budget: u64,
    reputation: u32,
    squad_size: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredContractPreview {
    actor: String,
    view: CareerPreview,
    dependencies: Dependencies,
}

impl Management {
    pub(crate) fn validate_career_checkpoint(&self) -> Result<(), String> {
        let Some(career) = &self.career else {
            return Ok(());
        };
        for id in &career.initial_retired_player_ids {
            if !self.players.get(id).is_some_and(|p| p.club_id.is_empty())
                || !career
                    .contracts
                    .get(id)
                    .is_some_and(|c| c.end_date.is_none())
                || !self
                    .social
                    .as_ref()
                    .and_then(|s| s.source_players.get(id))
                    .is_some_and(|p| p.retired && p.team_id.is_none() && p.contract_end.is_none())
            {
                return Err("Invalid retired bootstrap checkpoint metadata".into());
            }
        }
        if career.contracts.len() != self.players.len()
            || self
                .players
                .keys()
                .any(|id| !career.contracts.contains_key(id))
            || career.wage_budgets.len() != self.clubs.len()
            || career.reputations.len() != self.clubs.len()
            || self.clubs.keys().any(|id| {
                !career.wage_budgets.contains_key(id) || !career.reputations.contains_key(id)
            })
            || career
                .staff_annual_wages
                .keys()
                .any(|id| !self.clubs.contains_key(id))
            || career
                .reputations
                .values()
                .any(|reputation| *reputation > 1000)
        {
            return Err("Invalid career checkpoint registry".into());
        }
        for (id, contract) in &career.contracts {
            contract.validate(career.today)?;
            if self.players[id].club_id.is_empty() {
                let retired = self
                    .social
                    .as_ref()
                    .and_then(|social| social.source_players.get(id))
                    .is_some_and(|player| player.retired);
                if contract.end_date.is_some() || (contract.weekly_wage != 0 && !retired) {
                    return Err("Active free-agent contract in checkpoint".into());
                }
            } else if contract.end_date.is_some_and(|end| end < career.today) {
                return Err("Expired attached contract in checkpoint".into());
            }
        }
        for ((actor, player), negotiation) in &career.negotiations {
            if !self.managers.contains_key(actor)
                || self
                    .players
                    .get(player)
                    .is_none_or(|player| !player.club_id.is_empty())
            {
                return Err("Invalid free-agent negotiation owner in checkpoint".into());
            }
            negotiation.validate(career.today)?;
            if negotiation.end_date.is_some() || negotiation.weekly_wage != 0 {
                return Err("Committed terms in free-agent negotiation checkpoint".into());
            }
        }
        for (id, preview) in &career.previews {
            if *id == 0
                || *id > self.sequence
                || *id != preview.view.id
                || !self.managers.contains_key(&preview.actor)
                || !self.players.contains_key(&preview.view.player_id)
                || preview.view.player_id != preview.view.action.player_id()
                || preview.dependencies.date != career.today
                || (!preview.dependencies.owner.is_empty()
                    && !self.clubs.contains_key(&preview.dependencies.owner))
            {
                return Err("Invalid contract preview checkpoint".into());
            }
            preview
                .dependencies
                .contract
                .validate(preview.dependencies.date)?;
            preview
                .dependencies
                .negotiation
                .validate(preview.dependencies.date)?;
        }
        if career
            .ledger
            .iter()
            .any(|entry| !self.clubs.contains_key(&entry.club_id) || entry.date > career.today)
            || career.releases.iter().any(|entry| {
                !self.clubs.contains_key(&entry.club_id)
                    || !self.players.contains_key(&entry.player_id)
                    || entry.date > career.today
            })
        {
            return Err("Invalid career history checkpoint".into());
        }
        for club in self.clubs.keys() {
            self.contract_wage_total(career, club)?;
        }
        Ok(())
    }

    pub fn season_board_finances(
        &self,
    ) -> Result<BTreeMap<String, crate::football::BoardFinance>, String> {
        let career = self
            .career
            .as_ref()
            .ok_or("Career finance is not configured")?;
        self.clubs
            .iter()
            .map(|(id, club)| {
                if self.economy.is_some() {
                    let (_, snapshot) = self.economy_finance(id)?;
                    return Ok((
                        id.clone(),
                        crate::football::BoardFinance {
                            wage_usage_percent: snapshot.wage_budget_usage_percent,
                            in_debt: snapshot.currently_in_debt,
                        },
                    ));
                }
                let total = self.contract_wage_total(career, id)?;
                let usage = (u128::from(total) * 100 / u128::from(career.wage_budgets[id].max(1)))
                    .min(u128::from(u32::MAX)) as u32;
                Ok((
                    id.clone(),
                    crate::football::BoardFinance {
                        wage_usage_percent: usage,
                        in_debt: club.balance < 0,
                    },
                ))
            })
            .collect()
    }

    pub(crate) fn career_manager_eliminated(&mut self, actor: &str) {
        if let Some(career) = &mut self.career {
            career.previews.retain(|_, preview| preview.actor != actor);
            career
                .negotiations
                .retain(|(manager, _), _| manager != actor);
        }
    }

    /// Existing paid transfers retain the existing salary/end date. Validate the
    /// incoming salary against the same board policy before the ownership commit.
    pub(crate) fn validate_contract_transfer(
        &self,
        player_id: &str,
        buyer: &str,
    ) -> Result<(), Error> {
        let Some(career) = &self.career else {
            return Ok(());
        };
        let club = self.clubs.get(buyer).ok_or(Error::Unavailable)?;
        let contract = career.contracts.get(player_id).ok_or(Error::Unavailable)?;
        let total = self
            .contract_wage_total(career, buyer)
            .map_err(Error::Contract)?;
        if !wage_policy_allows(
            club.balance,
            career.wage_budgets[buyer],
            total,
            0,
            contract.weekly_wage,
        ) {
            return Err(Error::Contract(
                "Board wage policy refuses incoming contract".into(),
            ));
        }
        Ok(())
    }
    pub fn configure_career(&mut self, setup: CareerSetup) -> Result<(), String> {
        self.configure_career_with_retired(setup, BTreeSet::new())
    }

    /// Trusted bootstrap metadata, subsequently checked against the source registry.
    pub fn configure_career_with_retired(
        &mut self,
        setup: CareerSetup,
        retired_player_ids: BTreeSet<String>,
    ) -> Result<(), String> {
        if self.career.is_some() || self.sequence != 0 {
            return Err("Career can only be configured once before commands".into());
        }
        if setup.contracts.len() != self.players.len()
            || self
                .players
                .keys()
                .any(|id| !setup.contracts.contains_key(id))
            || setup.wage_budgets.len() != self.clubs.len()
            || setup.reputations.len() != self.clubs.len()
            || self.clubs.keys().any(|id| {
                !setup.wage_budgets.contains_key(id) || !setup.reputations.contains_key(id)
            })
            || setup
                .staff_annual_wages
                .keys()
                .any(|id| !self.clubs.contains_key(id))
        {
            return Err("Career records must match registered players and clubs".into());
        }
        if setup
            .reputations
            .values()
            .any(|reputation| *reputation > 1000)
        {
            return Err("Club reputation must be 0..=1000".into());
        }
        for id in &retired_player_ids {
            if !self.players.get(id).is_some_and(|p| p.club_id.is_empty())
                || !setup
                    .contracts
                    .get(id)
                    .is_some_and(|c| c.end_date.is_none())
            {
                return Err("Retired bootstrap player must be registered, unowned and without a contract end".into());
            }
        }
        for (id, contract) in &setup.contracts {
            contract.validate(setup.today)?;
            if self.players[id].club_id.is_empty()
                && (contract.end_date.is_some()
                    || (contract.weekly_wage != 0 && !retired_player_ids.contains(id)))
            {
                return Err("Free agent cannot have an active salary or contract".into());
            }
            if !self.players[id].club_id.is_empty()
                && contract.end_date.is_some_and(|end| end < setup.today)
            {
                return Err("Attached player contract is already expired at initialization".into());
            }
        }
        let state = CareerState {
            initial_retired_player_ids: retired_player_ids,
            today: setup.today,
            contracts: setup.contracts,
            wage_budgets: setup.wage_budgets,
            reputations: setup.reputations,
            staff_annual_wages: setup.staff_annual_wages,
            ledger: vec![],
            releases: vec![],
            negotiations: BTreeMap::new(),
            previews: BTreeMap::new(),
        };
        // Ensure every aggregate is representable before publishing the setup.
        for club in self.clubs.keys() {
            self.contract_wage_total(&state, club)?;
        }
        self.career = Some(state);
        Ok(())
    }

    pub(crate) fn validate_initial_retired_source(
        &self,
        players: &BTreeMap<String, domain::player::Player>,
    ) -> Result<(), String> {
        let Some(career) = &self.career else {
            return Ok(());
        };
        let retired: BTreeSet<_> = players
            .iter()
            .filter(|(_, p)| p.retired)
            .map(|(id, _)| id.clone())
            .collect();
        if retired != career.initial_retired_player_ids
            || retired
                .iter()
                .any(|id| players[id].team_id.is_some() || players[id].contract_end.is_some())
        {
            return Err("Retired bootstrap declaration does not match source players".into());
        }
        Ok(())
    }

    pub fn career_date(&self) -> Option<NaiveDate> {
        self.career.as_ref().map(|career| career.today)
    }

    pub(crate) fn contract_wage_total(
        &self,
        career: &CareerState,
        club: &str,
    ) -> Result<u64, String> {
        if self.economy.is_some() {
            return u64::try_from(self.economy_finance(club)?.1.annual_wage_bill)
                .map_err(|_| "Invalid wage bill".into());
        }
        self.players
            .values()
            .filter(|p| p.club_id == club)
            .map(|p| career.contracts[&p.id].weekly_wage)
            .chain(
                career
                    .staff_annual_wages
                    .get(club)
                    .into_iter()
                    .flatten()
                    .copied(),
            )
            .try_fold(0_u64, |total, wage| {
                total
                    .checked_add(u64::from(wage))
                    .ok_or_else(|| "Wage total overflow".into())
            })
    }

    pub fn career_view(&self, actor: &str) -> Result<CareerView, Error> {
        let club = &self.managers.get(actor).ok_or(Error::Unauthorized)?.club_id;
        let career = self.career.as_ref().ok_or(Error::Unavailable)?;
        let contracts: BTreeMap<String, PlayerContract> = self
            .players
            .values()
            .filter(|p| self.contract_owner(&p.id) == Some(club.as_str()))
            .map(|p| (p.id.clone(), career.contracts[&p.id].clone()))
            .collect();
        let renewal_terms = contracts
            .iter()
            .filter(|(_, contract)| contract.end_date.is_some_and(|end| end >= career.today))
            .map(|(id, contract)| {
                Ok((
                    id.clone(),
                    RenewalTerms {
                        expected_wage: contract
                            .expected_wage(career.reputations[club], career.today)
                            .map_err(Error::Contract)?,
                        expected_years: contract.expected_years(career.today),
                        days_remaining: contract.remaining_days(career.today),
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>, Error>>()?;
        let free_agents = self
            .players
            .values()
            .filter(|p| {
                p.club_id.is_empty()
                    && !self.social.as_ref().is_some_and(|s| {
                        s.source_players
                            .get(&p.id)
                            .is_some_and(|source| source.retired)
                    })
            })
            .map(|player| {
                let contract = career
                    .negotiations
                    .get(&(actor.into(), player.id.clone()))
                    .unwrap_or(&career.contracts[&player.id]);
                Ok(FreeAgentView {
                    id: player.id.clone(),
                    name: player.name.clone(),
                    expected_wage: contract
                        .expected_wage(career.reputations[club], career.today)
                        .map_err(Error::Contract)?,
                    expected_years: contract.expected_years(career.today),
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(CareerView {
            date: career.today,
            contracts,
            renewal_terms,
            free_agents,
            wage_budget: career.wage_budgets[club],
            current_wage_total: self
                .contract_wage_total(career, club)
                .map_err(Error::Contract)?,
            ledger: career
                .ledger
                .iter()
                .filter(|entry| &entry.club_id == club)
                .cloned()
                .collect(),
        })
    }

    fn contract_dependencies(
        &self,
        actor: &str,
        action: &ContractAction,
    ) -> Result<Dependencies, Error> {
        let club = &self.managers.get(actor).ok_or(Error::Unauthorized)?.club_id;
        let career = self.career.as_ref().ok_or(Error::Unavailable)?;
        let player = self
            .players
            .get(action.player_id())
            .ok_or(Error::Unavailable)?;
        if matches!(action, ContractAction::Renew { .. }) {
            self.validate_social_renewal(actor, &player.id)?;
        }
        match action {
            ContractAction::Sign { .. }
                if self.social.as_ref().is_some_and(|s| {
                    s.source_players.get(&player.id).is_some_and(|p| p.retired)
                }) =>
            {
                return Err(Error::Unavailable);
            }
            ContractAction::Sign { .. } if !player.club_id.is_empty() => {
                return Err(Error::Unavailable);
            }
            ContractAction::Renew { .. } | ContractAction::Terminate { .. }
                if self.contract_owner(&player.id) != Some(club.as_str()) =>
            {
                return Err(Error::Unavailable);
            }
            _ => {}
        }
        if matches!(action, ContractAction::Terminate { .. })
            && self
                .social
                .as_ref()
                .is_some_and(|s| s.source_players[&player.id].active_loan.is_some())
        {
            return Err(Error::Contract("Cannot terminate an active loan".into()));
        }
        let contract = career.contracts[&player.id].clone();
        let negotiation = if matches!(action, ContractAction::Sign { .. }) {
            career
                .negotiations
                .get(&(actor.into(), player.id.clone()))
                .unwrap_or(&contract)
                .clone()
        } else {
            contract.clone()
        };
        Ok(Dependencies {
            date: career.today,
            balance: self.clubs[club].balance,
            club_revision: self.club_revisions[club],
            player_revision: self.player_revisions[&player.id],
            owner: player.club_id.clone(),
            contract,
            negotiation,
            wage_total: self
                .contract_wage_total(career, club)
                .map_err(Error::Contract)?,
            wage_budget: career.wage_budgets[club],
            reputation: career.reputations[club],
            squad_size: self.players.values().filter(|p| &p.club_id == club).count(),
        })
    }

    fn contract_preview(
        &self,
        action: &ContractAction,
        dep: &Dependencies,
    ) -> Result<(CareerPreview, PlayerContract, Decision), Error> {
        let mut negotiated = dep.negotiation.clone();
        let mut balance_after = dep.balance;
        let mut projected = dep.wage_total;
        let mut severance = 0;
        let decision = match action {
            ContractAction::Renew {
                weekly_wage, years, ..
            }
            | ContractAction::Sign {
                weekly_wage, years, ..
            } => {
                let free = matches!(action, ContractAction::Sign { .. });
                let decision = negotiated
                    .evaluate(dep.reputation, dep.date, *weekly_wage, *years, free)
                    .map_err(Error::Contract)?;
                if decision == Decision::Accepted {
                    let old = if free { 0 } else { dep.contract.weekly_wage };
                    if !wage_policy_allows(
                        dep.balance,
                        dep.wage_budget,
                        dep.wage_total,
                        old,
                        *weekly_wage,
                    ) {
                        return Err(Error::Contract(
                            "Board wage policy refuses these terms".into(),
                        ));
                    }
                    projected = dep
                        .wage_total
                        .saturating_sub(u64::from(old))
                        .checked_add(u64::from(*weekly_wage))
                        .ok_or(Error::Overflow)?;
                    negotiated
                        .apply_agreement(dep.date, *weekly_wage, *years)
                        .map_err(Error::Contract)?;
                }
                decision
            }
            ContractAction::Terminate { .. } => {
                if dep.contract.end_date.is_none() {
                    return Err(Error::Contract("Player has no active contract".into()));
                }
                if dep.squad_size <= 11 {
                    return Err(Error::SquadTooSmall);
                }
                severance = dep.contract.severance(dep.date).map_err(Error::Contract)?;
                balance_after = dep.balance.checked_sub(severance).ok_or(Error::Overflow)?;
                projected = dep
                    .wage_total
                    .checked_sub(u64::from(dep.contract.weekly_wage))
                    .ok_or(Error::Overflow)?;
                negotiated.end_date = None;
                Decision::Accepted
            }
        };
        Ok((
            CareerPreview {
                id: self.sequence,
                player_id: action.player_id().into(),
                action: action.clone(),
                balance_after,
                projected_wage_total: projected,
                contract_end: negotiated.end_date,
                severance,
            },
            negotiated,
            decision,
        ))
    }

    pub(crate) fn execute_career(
        &mut self,
        actor: &str,
        command: &CareerCommand,
    ) -> Result<CareerOutcome, Error> {
        if self.window.is_ready(actor) {
            return Err(Error::AlreadyReady);
        }
        let club = self
            .managers
            .get(actor)
            .ok_or(Error::Unauthorized)?
            .club_id
            .clone();
        if self.career.is_none() {
            return Err(Error::Unavailable);
        }
        match command {
            CareerCommand::Delegate(options) => self
                .delegate_contracts(actor, options)
                .map(CareerOutcome::Delegated),
            CareerCommand::LetExpire { player_id, enabled } => {
                if self
                    .players
                    .get(player_id)
                    .is_none_or(|p| p.club_id != club)
                {
                    return Err(Error::Unavailable);
                }
                let revision = self.player_revisions[player_id]
                    .checked_add(1)
                    .ok_or(Error::Overflow)?;
                let career = self.career.as_mut().unwrap();
                career
                    .contracts
                    .get_mut(player_id)
                    .unwrap()
                    .set_let_expire(career.today, *enabled)
                    .map_err(Error::Contract)?;
                self.player_revisions.insert(player_id.clone(), revision);
                Ok(CareerOutcome::LetExpireSet {
                    player_id: player_id.clone(),
                    enabled: *enabled,
                })
            }
            CareerCommand::Review { action } => {
                let dependencies = self.contract_dependencies(actor, action)?;
                let (view, negotiated, decision) = self.contract_preview(action, &dependencies)?;
                if decision != Decision::Accepted {
                    let career = self.career.as_mut().unwrap();
                    if matches!(action, ContractAction::Sign { .. }) {
                        career
                            .negotiations
                            .insert((actor.into(), action.player_id().into()), negotiated);
                    } else {
                        let revision = self.player_revisions[action.player_id()]
                            .checked_add(1)
                            .ok_or(Error::Overflow)?;
                        career
                            .contracts
                            .insert(action.player_id().into(), negotiated);
                        self.player_revisions
                            .insert(action.player_id().into(), revision);
                    }
                    return Ok(CareerOutcome::Decision {
                        player_id: action.player_id().into(),
                        decision,
                    });
                }
                self.career.as_mut().unwrap().previews.insert(
                    view.id,
                    StoredContractPreview {
                        actor: actor.into(),
                        view: view.clone(),
                        dependencies,
                    },
                );
                Ok(CareerOutcome::Preview(view))
            }
            CareerCommand::Confirm { preview_id } => {
                let stored = self
                    .career
                    .as_ref()
                    .unwrap()
                    .previews
                    .get(preview_id)
                    .filter(|preview| preview.actor == actor)
                    .cloned()
                    .ok_or(Error::Unavailable)?;
                let dependencies = self.contract_dependencies(actor, &stored.view.action)?;
                let (view, mut negotiated, decision) =
                    self.contract_preview(&stored.view.action, &dependencies)?;
                if dependencies != stored.dependencies {
                    self.career.as_mut().unwrap().previews.remove(preview_id);
                    if decision != Decision::Accepted {
                        return Ok(CareerOutcome::Decision {
                            player_id: view.player_id,
                            decision,
                        });
                    }
                    self.career.as_mut().unwrap().previews.insert(
                        view.id,
                        StoredContractPreview {
                            actor: actor.into(),
                            view: view.clone(),
                            dependencies,
                        },
                    );
                    return Ok(CareerOutcome::RefreshRequired(view));
                }
                if decision != Decision::Accepted {
                    return Err(Error::Unavailable);
                }
                let player_revision = self.player_revisions[&view.player_id]
                    .checked_add(1)
                    .ok_or(Error::Overflow)?;
                let club_revision = self.club_revisions[&club]
                    .checked_add(1)
                    .ok_or(Error::Overflow)?;
                let career = self.career.as_mut().unwrap();
                if matches!(view.action, ContractAction::Sign { .. }) {
                    negotiated.morale = negotiated.morale.saturating_add(6).min(100);
                    negotiated.manager_trust = negotiated.manager_trust.max(55);
                    // The compact issue flag has no category. Preserve it rather
                    // than clearing unrelated issues when the player signs.
                    negotiated.last_attempt = None;
                    negotiated.last_agreed = None;
                    negotiated.round = 0;
                    self.players.get_mut(&view.player_id).unwrap().club_id = club.clone();
                }
                career.contracts.insert(view.player_id.clone(), negotiated);
                career.previews.remove(preview_id);
                self.player_revisions
                    .insert(view.player_id.clone(), player_revision);
                self.club_revisions.insert(club.clone(), club_revision);
                if matches!(view.action, ContractAction::Terminate { .. }) {
                    if let Some(economy) = &mut self.economy {
                        let account = economy
                            .setup
                            .clubs
                            .get_mut(&club)
                            .ok_or(Error::Unavailable)?;
                        account.season_expenses = account
                            .season_expenses
                            .checked_add(view.severance)
                            .ok_or(Error::Overflow)?;
                        account
                            .financial_ledger
                            .push(domain::team::FinancialTransaction {
                                date: dependencies.date.to_string(),
                                description: format!(
                                    "be.msg.contractTerminated.ledgerDescription?player={}",
                                    self.players[&view.player_id].name
                                ),
                                amount: -view.severance,
                                kind: domain::team::FinancialTransactionKind::ContractTermination,
                            });
                    }
                    self.clubs.get_mut(&club).unwrap().balance = view.balance_after;
                    self.career.as_mut().unwrap().ledger.push(LedgerEntry {
                        date: dependencies.date,
                        club_id: club,
                        amount: -view.severance,
                        reason: "contract_termination".into(),
                    });
                    self.contract_event(
                        &view.player_id,
                        &self.players[&view.player_id].club_id.clone(),
                        "terminated",
                        Some(("severance", view.severance.to_string())),
                    )?;
                    self.release_contract(&view.player_id, "terminated");
                } else {
                    self.contract_transferred(&view.player_id);
                }
                Ok(CareerOutcome::Applied {
                    player_id: view.player_id,
                    action: view.action,
                })
            }
        }
    }

    pub(crate) fn contract_transferred(&mut self, player_id: &str) {
        self.remove_training_membership(player_id);
        if let Some(career) = &mut self.career {
            career
                .negotiations
                .retain(|(_, player), _| player != player_id);
            career
                .previews
                .retain(|_, preview| preview.view.player_id != player_id);
        }
    }

    fn release_contract(&mut self, player_id: &str, reason: &str) {
        self.market_contract_released(player_id);
        let contract_owner = self.contract_owner(player_id).map(str::to_owned);
        let player = self.players.get_mut(player_id).unwrap();
        let registration_club = std::mem::take(&mut player.club_id);
        let old_club = contract_owner.unwrap_or(registration_club);
        if let Some(social) = &mut self.social {
            let source = social.source_players.get_mut(player_id).unwrap();
            source.team_id = None;
            source.active_loan = None;
            source.contract_end = None;
            source.wage = 0;
            source.transfer_listed = false;
            source.loan_listed = false;
            source.transfer_offers.clear();
            source.loan_offers.clear();
            source.morale_core.renewal_state = None;
            source
                .movement_history
                .push(domain::player::PlayerMovementEntry {
                    date: self.career.as_ref().unwrap().today.to_string(),
                    kind: domain::player::PlayerMovementKind::Released,
                    from_team_id: Some(old_club.clone()),
                    from_team_name: self.clubs.get(&old_club).map(|c| c.name.clone()),
                    to_team_id: None,
                    to_team_name: None,
                    fee: None,
                    loan_end_date: None,
                });
        }
        if let Some(personnel) = &mut self.personnel {
            for team in personnel.teams.values_mut() {
                team.remove_player_references(player_id);
            }
        }
        let career = self.career.as_mut().unwrap();
        let contract = career.contracts.get_mut(player_id).unwrap();
        contract.weekly_wage = 0;
        contract.end_date = None;
        contract.let_expire = false;
        contract.blocked_until = None;
        contract.last_attempt = None;
        contract.last_agreed = None;
        contract.round = 0;
        career.releases.push(ReleaseRecord {
            date: career.today,
            player_id: player_id.into(),
            club_id: old_club,
            reason: reason.into(),
        });
        for lineup in self.lineups.values_mut() {
            lineup.retain(|id| id != player_id);
        }
        for plan in self.squad_plans.values_mut() {
            plan.player_roles.remove(player_id);
            for role in [
                &mut plan.match_roles.captain,
                &mut plan.match_roles.penalty_taker,
                &mut plan.match_roles.free_kick_taker,
                &mut plan.match_roles.corner_taker,
            ] {
                if role.as_deref() == Some(player_id) {
                    *role = None;
                }
            }
        }
        for offer in self
            .offers
            .values_mut()
            .filter(|offer| offer.player_id == player_id && offer.status == OfferStatus::Pending)
        {
            offer.status = OfferStatus::Withdrawn;
        }
        self.previews
            .retain(|_, preview| preview.view.offer.player_id != player_id);
        self.contract_transferred(player_id);
    }

    /// Exactly one following calendar date. Stage balance/revision arithmetic
    /// before publishing any expiry or wage charge; expiry has no roster exemption.
    pub fn advance_career(&mut self, next_date: NaiveDate) -> Result<(), Error> {
        let mut staged = self.clone();
        staged.advance_career_inner(next_date)?;
        *self = staged;
        Ok(())
    }

    pub(crate) fn contract_owner(&self, player_id: &str) -> Option<&str> {
        self.social
            .as_ref()
            .and_then(|s| s.source_players.get(player_id))
            .and_then(|p| p.active_loan.as_ref())
            .map(|loan| loan.parent_team_id.as_str())
            .or_else(|| {
                self.players
                    .get(player_id)
                    .filter(|p| !p.club_id.is_empty())
                    .map(|p| p.club_id.as_str())
            })
    }

    fn advance_career_inner(&mut self, next_date: NaiveDate) -> Result<(), Error> {
        let Some(career) = self.career.as_ref() else {
            return Ok(());
        };
        if career.today.checked_add_days(Days::new(1)) != Some(next_date) {
            return Err(Error::WrongDay);
        }
        let closing_date = career.today;
        let expired: Vec<_> = self
            .players
            .values()
            .filter(|player| {
                !player.club_id.is_empty()
                    && career.contracts[&player.id]
                        .end_date
                        .is_some_and(|end| end <= closing_date)
            })
            .map(|player| (player.id.clone(), player.club_id.clone()))
            .collect();
        let mut balances: BTreeMap<_, _> = self
            .clubs
            .iter()
            .map(|(id, club)| (id.clone(), club.balance))
            .collect();
        let mut club_revisions = self.club_revisions.clone();
        let mut player_revisions = self.player_revisions.clone();
        let mut ledger = vec![];
        for (id, club) in &expired {
            let revision = player_revisions[id].checked_add(1).ok_or(Error::Overflow)?;
            player_revisions.insert(id.clone(), revision);
            let revision = club_revisions[club].checked_add(1).ok_or(Error::Overflow)?;
            club_revisions.insert(club.clone(), revision);
        }
        if closing_date.weekday() == Weekday::Mon && self.economy.is_none() {
            for club in self.clubs.keys() {
                let mut wages = self
                    .players
                    .values()
                    .filter(|player| {
                        &player.club_id == club && !expired.iter().any(|(id, _)| id == &player.id)
                    })
                    .map(|player| career.contracts[&player.id].weekly_wage)
                    .chain(
                        career
                            .staff_annual_wages
                            .get(club)
                            .into_iter()
                            .flatten()
                            .copied(),
                    );
                let charge = wages.try_fold(0_i64, |total, wage| {
                    total
                        .checked_add(i64::from(wage / 52))
                        .ok_or(Error::Overflow)
                })?;
                balances.insert(
                    club.clone(),
                    balances[club].checked_sub(charge).ok_or(Error::Overflow)?,
                );
                let revision = club_revisions[club].checked_add(1).ok_or(Error::Overflow)?;
                club_revisions.insert(club.clone(), revision);
                ledger.push(LedgerEntry {
                    date: closing_date,
                    club_id: club.clone(),
                    amount: -charge,
                    reason: "weekly_wages".into(),
                });
            }
        }
        for (id, _) in expired {
            if let Some(club) = self.contract_owner(&id).map(str::to_owned) {
                self.contract_event(&id, &club, "expired", None)?;
            }
            self.release_contract(&id, "expired");
        }
        for (id, balance) in balances {
            self.clubs.get_mut(&id).unwrap().balance = balance;
        }
        self.club_revisions = club_revisions;
        self.player_revisions = player_revisions;
        self.settle_economy()?;
        let career = self.career.as_mut().unwrap();
        career.today = next_date;
        career.ledger.extend(ledger);
        career.previews.clear();
        Ok(())
    }
}
