//! Shared transfer market: source football rules with explicit club consent and
//! atomic FIFO review/confirmation replacing single-player AI transaction privilege.
use crate::football::Football;
use crate::market_rules as rules;
use crate::{Error, Management, Receipt, Request};
use chrono::{Days, NaiveDate};
use domain::{
    player::{ActiveLoan, PlayerMovementEntry, PlayerMovementKind},
    season::TransferWindowContext,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[path = "market_daily.rs"]
mod daily;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MarketSetup {
    pub season_start: Option<NaiveDate>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Terms {
    Transfer {
        fee: u64,
    },
    Loan {
        end_date: NaiveDate,
        wage_contribution_pct: u8,
        buy_option_fee: Option<u64>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Pending,
    PendingRegistration,
    Completed,
    Rejected,
    Withdrawn,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketOffer {
    pub id: u64,
    pub player_id: String,
    pub seller: String,
    pub buyer: String,
    pub proposer: String,
    pub terms: Terms,
    pub date: NaiveDate,
    pub revision: u64,
    pub round: u8,
    pub status: Status,
    pub registration_date: Option<NaiveDate>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarketCommand {
    List {
        player_id: String,
        transfer_listed: bool,
        loan_listed: bool,
    },
    Bid {
        player_id: String,
        terms: Terms,
    },
    Counter {
        offer_id: u64,
        terms: Terms,
    },
    Reject {
        offer_id: u64,
    },
    Review {
        offer_id: u64,
    },
    Confirm {
        preview_id: u64,
    },
    ReviewBuyOption {
        player_id: String,
    },
}
impl MarketCommand {
    pub fn is_response(&self) -> bool {
        matches!(
            self,
            Self::Counter { .. } | Self::Reject { .. } | Self::Review { .. } | Self::Confirm { .. }
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketPreview {
    pub id: u64,
    pub offer: MarketOffer,
    pub registration_date: NaiveDate,
    pub balance_before: i64,
    pub balance_after: i64,
    pub transfer_budget_before: i64,
    pub transfer_budget_after: i64,
    pub projected_annual_wages: i64,
    pub wage_budget: i64,
    pub pending_other_consent: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarketOutcome {
    Listed,
    Offer(MarketOffer),
    Preview(MarketPreview),
    Refreshed(MarketPreview),
    AwaitingConsent(MarketOffer),
    Registered(MarketOffer),
    Scheduled(MarketOffer),
    Rejected,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct FinancialConsent {
    balance: i64,
    transfer_budget: i64,
    wages: i64,
    wage_budget: i64,
    incoming_wage: u32,
    incoming_end: Option<NaiveDate>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ContractTerms {
    weekly_wage: u32,
    end_date: Option<NaiveDate>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Dependencies {
    date: NaiveDate,
    offer: MarketOffer,
    buyer: FinancialConsent,
    seller: FinancialConsent,
    owner: String,
    contract: ContractTerms,
    active_loan: Option<ActiveLoan>,
    seller_roster: usize,
    registration: NaiveDate,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredPreview {
    actor: String,
    day: u32,
    view: MarketPreview,
    dependencies: Dependencies,
    buy_option: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketState {
    pub setup: MarketSetup,
    pub window: TransferWindowContext,
    pub offers: BTreeMap<u64, MarketOffer>,
    #[serde(with = "crate::checkpoint::entries")]
    consents: BTreeMap<(u64, String), FinancialConsent>,
    previews: BTreeMap<u64, StoredPreview>,
    pub completed: Vec<domain::league::CompletedTransfer>,
    pub last_processed: Option<NaiveDate>,
    pub last_registrations: Option<NaiveDate>,
    pub reports: Vec<(String, NaiveDate, bool)>,
    next_offer_id: u64,
    source_offer_ids: BTreeMap<u64, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketView {
    pub window: TransferWindowContext,
    pub offers: Vec<MarketOffer>,
    pub active_loans: BTreeMap<String, ActiveLoan>,
}
fn error(s: impl Into<String>) -> Error {
    Error::Contract(s.into())
}

impl Football {
    pub fn configure_market(&mut self, setup: MarketSetup) -> Result<(), String> {
        if self.started || self.management.sequence != 0 || self.management.market.is_some() {
            return Err("Market must be configured once before play".into());
        }
        if self.management.social.is_none()
            || self.management.economy.is_none()
            || self.management.personnel.is_none()
        {
            return Err("Market requires social, economy and personnel".into());
        }
        let today = self
            .management
            .career_date()
            .ok_or("Market requires career")?;
        let window = rules::window(today, self.market_season_start().or(setup.season_start))?;
        let mut staged = self.management.clone();
        staged.market = Some(MarketState {
            setup,
            window,
            offers: BTreeMap::new(),
            consents: BTreeMap::new(),
            previews: BTreeMap::new(),
            completed: vec![],
            last_processed: None,
            last_registrations: None,
            reports: vec![],
            next_offer_id: 1,
            source_offer_ids: BTreeMap::new(),
        });
        staged.import_market_offers()?;
        staged.validate_market_checkpoint()?;
        self.management = staged;
        Ok(())
    }
    fn market_season_start(&self) -> Option<NaiveDate> {
        if let Some(competitions) = &self.competitions {
            return competitions
                .setup
                .competitions
                .get(&competitions.setup.primary_competition_id)?
                .fixtures
                .iter()
                .filter(|fixture| fixture.counts_for_league_standings())
                .filter_map(|fixture| fixture.date.parse::<NaiveDate>().ok())
                .min();
        }
        let today = self.management.career_date()?;
        let day = self.management.window.day;
        self.fixtures.iter().map(|f| f.day).min().and_then(|first| {
            if first >= day {
                today.checked_add_days(Days::new(u64::from(first - day)))
            } else {
                today.checked_sub_days(Days::new(u64::from(day - first)))
            }
        })
    }
    pub fn market_view(&self, actor: &str) -> Result<MarketView, Error> {
        self.management.market_view(actor)
    }
    pub(crate) fn dispatch_market(
        &mut self,
        actor: &str,
        request: Request,
        now_ms: u64,
    ) -> Result<Receipt, Error> {
        let mut staged = self.clone();
        staged.refresh_market_projection().map_err(error)?;
        let receipt = staged.management.dispatch(actor, request, now_ms)?;
        *self = staged;
        Ok(receipt)
    }
    fn refresh_market_projection(&mut self) -> Result<(), String> {
        if self.management.market.is_none() {
            return Ok(());
        }
        let players = self.project_source_players()?;
        self.management.social.as_mut().unwrap().source_players = players;
        let today = self
            .management
            .career_date()
            .ok_or("Market requires date")?;
        let anchor = self.market_season_start().or(self
            .management
            .market
            .as_ref()
            .unwrap()
            .setup
            .season_start);
        self.management.market.as_mut().unwrap().window = rules::window(today, anchor)?;
        Ok(())
    }
}

impl Management {
    pub(crate) fn execute_market(
        &mut self,
        actor: &str,
        command: &MarketCommand,
    ) -> Result<MarketOutcome, Error> {
        let club = self
            .managers
            .get(actor)
            .ok_or(Error::Unauthorized)?
            .club_id
            .clone();
        self.market.as_ref().ok_or(Error::Unavailable)?;
        if self.window.is_ready(actor) && !command.is_response() {
            return Err(Error::AlreadyReady);
        }
        self.expire_market_offers()?;
        let today = self.career_date().ok_or(Error::Unavailable)?;
        match command {
            MarketCommand::List {
                player_id,
                transfer_listed,
                loan_listed,
            } => {
                if self
                    .players
                    .get(player_id)
                    .is_none_or(|p| p.club_id != club)
                {
                    return Err(Error::Unavailable);
                }
                if self
                    .market
                    .as_ref()
                    .unwrap()
                    .offers
                    .values()
                    .any(|o| o.player_id == *player_id && o.status == Status::PendingRegistration)
                {
                    return Err(Error::Unavailable);
                }
                let source = self
                    .social
                    .as_mut()
                    .unwrap()
                    .source_players
                    .get_mut(player_id)
                    .ok_or(Error::Unavailable)?;
                if source.active_loan.is_some() || source.retired {
                    return Err(Error::Unavailable);
                }
                source.transfer_listed = *transfer_listed;
                source.loan_listed = *loan_listed;
                Ok(MarketOutcome::Listed)
            }
            MarketCommand::Bid { player_id, terms } => {
                let seller = self
                    .players
                    .get(player_id)
                    .ok_or(Error::Unavailable)?
                    .club_id
                    .clone();
                if seller.is_empty() || seller == club {
                    return Err(Error::Unavailable);
                }
                if matches!(terms, Terms::Loan { .. })
                    && !self.social.as_ref().unwrap().source_players[player_id].loan_listed
                {
                    return Err(error("be.error.transfers.playerNotLoanListed"));
                }
                let market = self.market.as_mut().unwrap();
                let id = market.next_offer_id;
                market.next_offer_id = id.checked_add(1).ok_or(Error::Overflow)?;
                let offer = MarketOffer {
                    id,
                    player_id: player_id.clone(),
                    seller,
                    buyer: club.clone(),
                    proposer: club.clone(),
                    terms: terms.clone(),
                    date: today,
                    revision: 0,
                    round: 1,
                    status: Status::Pending,
                    registration_date: None,
                };
                let dep = self.market_dependencies(&offer, false)?;
                let market = self.market.as_mut().unwrap();
                for previous in market.offers.values_mut().filter(|o| {
                    o.player_id == *player_id && o.buyer == club && o.status == Status::Pending
                }) {
                    previous.status = Status::Withdrawn;
                }
                market.source_offer_ids.insert(id, format!("market_{id}"));
                market.offers.insert(id, offer.clone());
                market.consents.insert((id, club), dep.buyer);
                self.sync_market_offer(id)?;
                self.deliver_market_offer(&offer)?;
                Ok(MarketOutcome::Offer(offer))
            }
            MarketCommand::Counter { offer_id, terms } => {
                let mut offer = self.market_offer(actor, *offer_id)?;
                if offer.status != Status::Pending || offer.proposer == club {
                    return Err(Error::Unavailable);
                }
                match (&offer.terms, terms) {
                    (Terms::Transfer { fee: old }, Terms::Transfer { fee: new })
                        if (club == offer.seller && new > old)
                            || (club == offer.buyer && new < old) => {}
                    (
                        Terms::Loan {
                            end_date: old_end,
                            wage_contribution_pct: old_wage,
                            buy_option_fee: old_option,
                        },
                        Terms::Loan {
                            end_date,
                            wage_contribution_pct,
                            buy_option_fee,
                        },
                    ) => {
                        if (old_end == end_date
                            && old_wage == wage_contribution_pct
                            && old_option == buy_option_fee)
                            || (club == offer.seller && wage_contribution_pct < old_wage)
                            || (club == offer.seller
                                && matches!((old_option,buy_option_fee),(Some(a),Some(b)) if b<a))
                        {
                            return Err(error("be.error.transfers.loanCounterMustImproveTerms"));
                        }
                    }
                    _ => return Err(error("Counter must improve terms")),
                }
                offer.terms = terms.clone();
                offer.proposer = club.clone();
                offer.date = today;
                offer.round = offer.round.saturating_add(1);
                offer.revision = offer.revision.checked_add(1).ok_or(Error::Overflow)?;
                let dep = self.market_dependencies(&offer, false)?;
                let consent = if club == offer.buyer {
                    dep.buyer
                } else {
                    dep.seller
                };
                let market = self.market.as_mut().unwrap();
                market.consents.retain(|(id, _), _| id != offer_id);
                market.consents.insert((*offer_id, club), consent);
                market.offers.insert(*offer_id, offer.clone());
                self.sync_market_offer(*offer_id)?;
                self.deliver_market_offer(&offer)?;
                Ok(MarketOutcome::Offer(offer))
            }
            MarketCommand::Reject { offer_id } => {
                let offer = self.market_offer(actor, *offer_id)?;
                if offer.status != Status::Pending || offer.proposer == club {
                    return Err(Error::Unavailable);
                }
                self.market
                    .as_mut()
                    .unwrap()
                    .offers
                    .get_mut(offer_id)
                    .unwrap()
                    .status = Status::Rejected;
                if club == offer.seller && matches!(offer.terms, Terms::Transfer { .. }) {
                    let (seller, buyer) = self.market_source_teams(&offer.seller, &offer.buyer)?;
                    let source = self
                        .social
                        .as_mut()
                        .unwrap()
                        .source_players
                        .get_mut(&offer.player_id)
                        .unwrap();
                    let openness =
                        rules::player_move_openness_score(today, source, &seller, &buyer);
                    rules::apply_blocked_move_consequences(source, openness);
                    let contract = self
                        .career
                        .as_mut()
                        .unwrap()
                        .contracts
                        .get_mut(&offer.player_id)
                        .unwrap();
                    contract.morale = source.morale;
                    contract.manager_trust = source.morale_core.manager_trust;
                    contract.unresolved_issue = source.morale_core.unresolved_issue.is_some();
                }
                self.sync_market_offer(*offer_id)?;
                Ok(MarketOutcome::Rejected)
            }
            MarketCommand::Review { offer_id } => {
                let offer = self.market_offer(actor, *offer_id)?;
                if offer.status != Status::Pending {
                    return Err(Error::Unavailable);
                }
                Ok(MarketOutcome::Preview(
                    self.market_preview(actor, offer, false)?,
                ))
            }
            MarketCommand::ReviewBuyOption { player_id } => {
                let source = self
                    .social
                    .as_ref()
                    .unwrap()
                    .source_players
                    .get(player_id)
                    .ok_or(Error::Unavailable)?;
                let loan = source.active_loan.as_ref().ok_or(Error::Unavailable)?;
                if loan.loan_team_id != club {
                    return Err(Error::Unavailable);
                }
                let fee = loan
                    .buy_option_fee
                    .filter(|fee| *fee > 0)
                    .ok_or(Error::Unavailable)?;
                let offer = MarketOffer {
                    id: 0,
                    player_id: player_id.clone(),
                    seller: loan.parent_team_id.clone(),
                    buyer: club.clone(),
                    proposer: club,
                    terms: Terms::Transfer { fee },
                    date: today,
                    revision: 0,
                    round: 1,
                    status: Status::Pending,
                    registration_date: None,
                };
                Ok(MarketOutcome::Preview(
                    self.market_preview(actor, offer, true)?,
                ))
            }
            MarketCommand::Confirm { preview_id } => {
                let stored = self
                    .market
                    .as_ref()
                    .unwrap()
                    .previews
                    .get(preview_id)
                    .filter(|p| p.actor == actor && p.day == self.window.day)
                    .cloned()
                    .ok_or(Error::Unavailable)?;
                if stored.buy_option && self.window.is_ready(actor) {
                    return Err(Error::AlreadyReady);
                }
                let offer = if stored.buy_option {
                    stored.view.offer.clone()
                } else {
                    self.market_offer(actor, stored.view.offer.id)?
                };
                if offer.status != Status::Pending {
                    return Err(Error::Unavailable);
                }
                let dep = self.market_dependencies(&offer, stored.buy_option)?;
                if dep != stored.dependencies {
                    return Ok(MarketOutcome::Refreshed(self.market_preview(
                        actor,
                        offer,
                        stored.buy_option,
                    )?));
                }
                let own = if club == offer.buyer {
                    dep.buyer.clone()
                } else {
                    dep.seller.clone()
                };
                let market = self.market.as_mut().unwrap();
                market.consents.insert((offer.id, club), own);
                market.previews.remove(preview_id);
                let both = stored.buy_option
                    || (market.consents.get(&(offer.id, offer.buyer.clone())) == Some(&dep.buyer)
                        && market.consents.get(&(offer.id, offer.seller.clone()))
                            == Some(&dep.seller));
                if !both {
                    return Ok(MarketOutcome::AwaitingConsent(offer));
                }
                if dep.registration > today {
                    let mut agreed = offer;
                    agreed.status = Status::PendingRegistration;
                    agreed.registration_date = Some(dep.registration);
                    self.market
                        .as_mut()
                        .unwrap()
                        .offers
                        .insert(agreed.id, agreed.clone());
                    if matches!(agreed.terms, Terms::Loan { .. }) {
                        let source = self
                            .social
                            .as_mut()
                            .unwrap()
                            .source_players
                            .get_mut(&agreed.player_id)
                            .unwrap();
                        source.loan_listed = false;
                        source.transfer_listed = false;
                        self.withdraw_market_conflicts(&agreed.player_id, agreed.id)?;
                    }
                    self.sync_market_offer(agreed.id)?;
                    Ok(MarketOutcome::Scheduled(agreed))
                } else {
                    let mut complete = offer;
                    self.complete_market_move(&complete, stored.buy_option)?;
                    complete.status = Status::Completed;
                    complete.registration_date = Some(today);
                    if !stored.buy_option {
                        self.market
                            .as_mut()
                            .unwrap()
                            .offers
                            .insert(complete.id, complete.clone());
                        self.sync_market_offer(complete.id)?;
                    }
                    Ok(MarketOutcome::Registered(complete))
                }
            }
        }
    }
    fn market_source_teams(
        &self,
        seller: &str,
        buyer: &str,
    ) -> Result<(domain::team::Team, domain::team::Team), Error> {
        let get = |id: &str| -> Result<domain::team::Team, Error> {
            let mut team = self
                .personnel
                .as_ref()
                .ok_or(Error::Unavailable)?
                .teams
                .get(id)
                .ok_or(Error::Unavailable)?
                .clone();
            team.finance = self.clubs[id].balance;
            team.reputation = self.career.as_ref().unwrap().reputations[id];
            team.starting_xi_ids = self.lineups.get(id).cloned().unwrap_or_default();
            Ok(team)
        };
        Ok((get(seller)?, get(buyer)?))
    }
    fn complete_market_move(&mut self, offer: &MarketOffer, buy_option: bool) -> Result<(), Error> {
        let today = self.career_date().ok_or(Error::Unavailable)?;
        let source = self.social.as_ref().unwrap().source_players[&offer.player_id].clone();
        let occupied: std::collections::BTreeSet<_> = self
            .social
            .as_ref()
            .unwrap()
            .source_players
            .values()
            .filter(|p| p.id != offer.player_id && self.players[&p.id].club_id == offer.buyer)
            .filter_map(|p| p.jersey_number)
            .collect();
        let jersey = source
            .jersey_number
            .filter(|n| !occupied.contains(n))
            .or_else(|| (1..=99).find(|n| !occupied.contains(n)));
        if let Terms::Transfer { fee } = offer.terms {
            let fee = i64::try_from(fee).map_err(|_| Error::Overflow)?;
            let buyer = self.clubs.get_mut(&offer.buyer).unwrap();
            buyer.balance = buyer.balance.checked_sub(fee).ok_or(Error::Overflow)?;
            let seller = self.clubs.get_mut(&offer.seller).unwrap();
            seller.balance = seller.balance.checked_add(fee).ok_or(Error::Overflow)?;
            let buyer = self.economy_account_mut(&offer.buyer).map_err(error)?;
            buyer.transfer_budget = buyer
                .transfer_budget
                .checked_sub(fee)
                .ok_or(Error::Overflow)?;
            if !buy_option {
                let seller = self.economy_account_mut(&offer.seller).map_err(error)?;
                seller.transfer_budget = seller
                    .transfer_budget
                    .checked_add(fee)
                    .ok_or(Error::Overflow)?;
            }
            self.market
                .as_mut()
                .unwrap()
                .completed
                .push(domain::league::CompletedTransfer {
                    date: today.to_string(),
                    from_team_id: offer.seller.clone(),
                    to_team_id: offer.buyer.clone(),
                    player_id: offer.player_id.clone(),
                    fee: fee as u64,
                });
        }
        if !buy_option
            && matches!(offer.terms, Terms::Transfer { .. })
            && self
                .lineups
                .get(&offer.seller)
                .is_some_and(|lineup| lineup.contains(&offer.player_id))
        {
            let others = self.lineups[&offer.seller].clone();
            for id in others.into_iter().filter(|id| id != &offer.player_id) {
                let contract = self
                    .career
                    .as_mut()
                    .unwrap()
                    .contracts
                    .get_mut(&id)
                    .unwrap();
                contract.morale = contract.morale.saturating_sub(4);
                self.social
                    .as_mut()
                    .unwrap()
                    .source_players
                    .get_mut(&id)
                    .unwrap()
                    .morale = contract.morale;
            }
        }
        self.clear_market_roster_references(
            &offer.player_id,
            buy_option.then_some(offer.buyer.as_str()),
        );
        self.players.get_mut(&offer.player_id).unwrap().club_id = offer.buyer.clone();
        let player = self
            .social
            .as_mut()
            .unwrap()
            .source_players
            .get_mut(&offer.player_id)
            .unwrap();
        player.team_id = Some(offer.buyer.clone());
        player.jersey_number = jersey;
        player.transfer_listed = false;
        player.loan_listed = false;
        let (kind, fee, end) = match offer.terms {
            Terms::Transfer { fee } => {
                let end = player.active_loan.take().map(|loan| loan.end_date);
                (
                    if buy_option {
                        PlayerMovementKind::LoanToBuy
                    } else {
                        PlayerMovementKind::PermanentTransfer
                    },
                    Some(fee),
                    end,
                )
            }
            Terms::Loan {
                end_date,
                wage_contribution_pct,
                buy_option_fee,
            } => {
                player.active_loan = Some(ActiveLoan {
                    parent_team_id: offer.seller.clone(),
                    loan_team_id: offer.buyer.clone(),
                    start_date: today.to_string(),
                    end_date: end_date.to_string(),
                    wage_contribution_pct,
                    buy_option_fee,
                    loan_start_minutes: player.stats.minutes_played,
                    loan_start_appearances: player.stats.appearances,
                    development_reported_minutes: player.stats.minutes_played,
                    development_reported_appearances: player.stats.appearances,
                });
                (
                    PlayerMovementKind::LoanStart,
                    None,
                    Some(end_date.to_string()),
                )
            }
        };
        player.movement_history.push(PlayerMovementEntry {
            date: today.to_string(),
            kind,
            from_team_id: Some(offer.seller.clone()),
            from_team_name: Some(self.clubs[&offer.seller].name.clone()),
            to_team_id: Some(offer.buyer.clone()),
            to_team_name: Some(self.clubs[&offer.buyer].name.clone()),
            fee,
            loan_end_date: end,
        });
        for club in [&offer.seller, &offer.buyer] {
            let rev = self.club_revisions.get_mut(club).unwrap();
            *rev = rev.checked_add(1).ok_or(Error::Overflow)?;
        }
        let rev = self.player_revisions.get_mut(&offer.player_id).unwrap();
        *rev = rev.checked_add(1).ok_or(Error::Overflow)?;
        self.contract_transferred(&offer.player_id);
        self.withdraw_market_conflicts(&offer.player_id, offer.id)?;
        Ok(())
    }
    fn clear_market_roster_references(&mut self, player: &str, keep: Option<&str>) {
        for (club, lineup) in &mut self.lineups {
            if Some(club.as_str()) != keep {
                lineup.retain(|id| id != player)
            }
        }
        for (club, team) in &mut self.personnel.as_mut().unwrap().teams {
            if Some(club.as_str()) != keep {
                team.remove_player_references(player)
            }
        }
        for (club, plan) in &mut self.squad_plans {
            if Some(club.as_str()) == keep {
                continue;
            }
            plan.player_roles.remove(player);
            for role in [
                &mut plan.match_roles.captain,
                &mut plan.match_roles.penalty_taker,
                &mut plan.match_roles.free_kick_taker,
                &mut plan.match_roles.corner_taker,
            ] {
                if role.as_deref() == Some(player) {
                    *role = None
                }
            }
        }
    }
    fn withdraw_market_conflicts(&mut self, player: &str, except: u64) -> Result<(), Error> {
        let ids: Vec<_> = self
            .market
            .as_ref()
            .unwrap()
            .offers
            .values()
            .filter(|o| {
                o.player_id == player
                    && o.id != except
                    && matches!(o.status, Status::Pending | Status::PendingRegistration)
            })
            .map(|o| o.id)
            .collect();
        for id in ids {
            self.market
                .as_mut()
                .unwrap()
                .offers
                .get_mut(&id)
                .unwrap()
                .status = Status::Withdrawn;
            self.sync_market_offer(id)?;
        }
        Ok(())
    }
    fn sync_market_offer(&mut self, id: u64) -> Result<(), Error> {
        use domain::player::*;
        let market = self.market.as_ref().unwrap();
        let offer = market.offers.get(&id).ok_or(Error::Unavailable)?;
        let source_id = market
            .source_offer_ids
            .get(&id)
            .ok_or(Error::Unavailable)?
            .clone();
        let player = self
            .social
            .as_mut()
            .unwrap()
            .source_players
            .get_mut(&offer.player_id)
            .ok_or(Error::Unavailable)?;
        match offer.terms {
            Terms::Transfer { fee } => {
                let status = match offer.status {
                    Status::Pending => TransferOfferStatus::Pending,
                    Status::PendingRegistration => TransferOfferStatus::PendingRegistration,
                    Status::Completed => TransferOfferStatus::Accepted,
                    Status::Rejected => TransferOfferStatus::Rejected,
                    Status::Withdrawn => TransferOfferStatus::Withdrawn,
                };
                let record = TransferOffer {
                    id: source_id.clone(),
                    from_team_id: offer.buyer.clone(),
                    fee,
                    wage_offered: self.career.as_ref().unwrap().contracts[&offer.player_id]
                        .weekly_wage,
                    last_manager_fee: Some(fee),
                    negotiation_round: offer.round,
                    suggested_counter_fee: None,
                    status,
                    date: offer.date.to_string(),
                    registration_date: offer.registration_date.map(|d| d.to_string()),
                };
                if let Some(existing) = player
                    .transfer_offers
                    .iter_mut()
                    .find(|o| o.id == source_id)
                {
                    *existing = record
                } else {
                    player.transfer_offers.push(record)
                }
            }
            Terms::Loan {
                end_date,
                wage_contribution_pct,
                buy_option_fee,
            } => {
                let status = match offer.status {
                    Status::Pending => LoanOfferStatus::Pending,
                    Status::PendingRegistration => LoanOfferStatus::PendingRegistration,
                    Status::Completed => LoanOfferStatus::Accepted,
                    Status::Rejected => LoanOfferStatus::Rejected,
                    Status::Withdrawn => LoanOfferStatus::Withdrawn,
                };
                let start = offer.registration_date.unwrap_or(offer.date);
                let record = LoanOffer {
                    id: source_id.clone(),
                    from_team_id: offer.buyer.clone(),
                    parent_team_id: offer.seller.clone(),
                    start_date: start.to_string(),
                    end_date: end_date.to_string(),
                    wage_contribution_pct,
                    buy_option_fee,
                    last_manager_wage_contribution_pct: Some(wage_contribution_pct),
                    last_manager_end_date: Some(end_date.to_string()),
                    last_manager_buy_option_fee: buy_option_fee,
                    negotiation_round: offer.round,
                    suggested_wage_contribution_pct: None,
                    suggested_end_date: None,
                    suggested_buy_option_fee: None,
                    status,
                    date: offer.date.to_string(),
                };
                if let Some(existing) = player.loan_offers.iter_mut().find(|o| o.id == source_id) {
                    *existing = record
                } else {
                    player.loan_offers.push(record)
                }
            }
        }
        Ok(())
    }
    fn import_market_offers(&mut self) -> Result<(), String> {
        use domain::player::{LoanOfferStatus as L, TransferOfferStatus as T};
        let players = self.social.as_ref().unwrap().source_players.clone();
        for source in players.values() {
            let Some(owner) = &source.team_id else {
                continue;
            };
            let mut records = vec![];
            for offer in &source.transfer_offers {
                if !matches!(offer.status, T::Pending | T::PendingRegistration) {
                    continue;
                }
                records.push((
                    offer.id.clone(),
                    owner.clone(),
                    offer.from_team_id.clone(),
                    Terms::Transfer { fee: offer.fee },
                    offer
                        .date
                        .parse::<NaiveDate>()
                        .map_err(|_| "Invalid imported transfer date")?,
                    offer.negotiation_round,
                    if offer.status == T::Pending {
                        Status::Pending
                    } else {
                        Status::PendingRegistration
                    },
                    offer
                        .registration_date
                        .as_ref()
                        .map(|s| s.parse::<NaiveDate>())
                        .transpose()
                        .map_err(|_| "Invalid registration date")?,
                ));
            }
            for offer in &source.loan_offers {
                if !matches!(offer.status, L::Pending | L::PendingRegistration) {
                    continue;
                }
                records.push((
                    offer.id.clone(),
                    offer.parent_team_id.clone(),
                    offer.from_team_id.clone(),
                    Terms::Loan {
                        end_date: offer
                            .end_date
                            .parse()
                            .map_err(|_| "Invalid loan end date")?,
                        wage_contribution_pct: offer.wage_contribution_pct,
                        buy_option_fee: offer.buy_option_fee,
                    },
                    offer
                        .date
                        .parse::<NaiveDate>()
                        .map_err(|_| "Invalid imported loan date")?,
                    offer.negotiation_round,
                    if offer.status == L::Pending {
                        Status::Pending
                    } else {
                        Status::PendingRegistration
                    },
                    Some(
                        offer
                            .start_date
                            .parse()
                            .map_err(|_| "Invalid loan registration")?,
                    ),
                ));
            }
            for (source_id, seller, buyer, terms, date, round, status, registration_date) in records
            {
                let market = self.market.as_mut().unwrap();
                let id = market.next_offer_id;
                market.next_offer_id = id.checked_add(1).ok_or("Offer id overflow")?;
                let offer = MarketOffer {
                    id,
                    player_id: source.id.clone(),
                    seller,
                    buyer: buyer.clone(),
                    proposer: buyer.clone(),
                    terms,
                    date,
                    revision: 0,
                    round,
                    status,
                    registration_date,
                };
                market.source_offer_ids.insert(id, source_id);
                market.offers.insert(id, offer.clone());
                if let Ok(dep) = self.market_dependencies(&offer, false) {
                    self.market
                        .as_mut()
                        .unwrap()
                        .consents
                        .insert((id, buyer), dep.buyer);
                }
            }
        }
        Ok(())
    }
    fn expire_market_offers(&mut self) -> Result<(), Error> {
        let today = self.career_date().ok_or(Error::Unavailable)?;
        let ids: Vec<_> = self
            .market
            .as_ref()
            .unwrap()
            .offers
            .values()
            .filter(|o| o.status == Status::Pending && (today - o.date).num_days() >= 14)
            .map(|o| o.id)
            .collect();
        for id in ids {
            self.market
                .as_mut()
                .unwrap()
                .offers
                .get_mut(&id)
                .unwrap()
                .status = Status::Withdrawn;
            self.sync_market_offer(id)?;
        }
        Ok(())
    }
    fn deliver_market_offer(&mut self, offer: &MarketOffer) -> Result<(), Error> {
        use domain::message::*;
        let recipient_club = if offer.proposer == offer.buyer {
            &offer.seller
        } else {
            &offer.buyer
        };
        let recipient = self
            .managers
            .values()
            .find(|m| &m.club_id == recipient_club)
            .map(|m| m.id.clone());
        if let Some(recipient) = recipient {
            // The executable response is Market Review/Confirm, never a second
            // inbox callback which could bypass the transaction dependencies.
            let message = InboxMessage::new(
                format!("market_{}_{}", offer.id, offer.revision),
                "Transfer negotiation".into(),
                format!("{}: {:?}", self.players[&offer.player_id].name, offer.terms),
                self.clubs[&offer.proposer].name.clone(),
                offer.date.to_string(),
            )
            .with_category(MessageCategory::Transfer)
            .with_context(MessageContext {
                player_id: Some(offer.player_id.clone()),
                team_id: Some(recipient_club.clone()),
                ..Default::default()
            });
            self.social
                .as_mut()
                .unwrap()
                .inbox
                .deliver(&recipient, message)
                .map_err(|e| error(format!("{e:?}")))?;
        }
        Ok(())
    }
    pub(crate) fn validate_market_checkpoint(&self) -> Result<(), String> {
        let Some(market) = &self.market else {
            return Ok(());
        };
        if self.career.is_none()
            || self.economy.is_none()
            || self.social.is_none()
            || self.personnel.is_none()
        {
            return Err("Incomplete market dependencies".into());
        }
        for (id, offer) in &market.offers {
            if id != &offer.id
                || *id >= market.next_offer_id
                || !self.players.contains_key(&offer.player_id)
                || !self.clubs.contains_key(&offer.seller)
                || !self.clubs.contains_key(&offer.buyer)
                || offer.buyer == offer.seller
                || !market.source_offer_ids.contains_key(id)
                || (offer.proposer != offer.buyer && offer.proposer != offer.seller)
            {
                return Err("Invalid market offer".into());
            }
            if let Terms::Loan {
                wage_contribution_pct,
                buy_option_fee,
                ..
            } = offer.terms
            {
                if wage_contribution_pct > 100 || buy_option_fee == Some(0) {
                    return Err("Invalid loan terms".into());
                }
            }
        }
        for ((id, club), _) in &market.consents {
            if *id != 0
                && market
                    .offers
                    .get(id)
                    .is_none_or(|o| &o.seller != club && &o.buyer != club)
            {
                return Err("Invalid market consent".into());
            }
        }
        for (id, preview) in &market.previews {
            if *id != preview.view.id
                || *id > self.sequence
                || !self.managers.contains_key(&preview.actor)
                || preview.day != self.window.day
            {
                return Err("Invalid market preview".into());
            }
        }
        for source in self.social.as_ref().unwrap().source_players.values() {
            if let Some(loan) = &source.active_loan {
                if !self.clubs.contains_key(&loan.parent_team_id)
                    || !self.clubs.contains_key(&loan.loan_team_id)
                    || loan.parent_team_id == loan.loan_team_id
                    || loan.wage_contribution_pct > 100
                    || source.team_id.as_deref() != Some(loan.loan_team_id.as_str())
                    || loan.end_date.parse::<NaiveDate>().is_err()
                    || loan.start_date.parse::<NaiveDate>().is_err()
                {
                    return Err("Invalid active loan".into());
                }
            }
        }
        Ok(())
    }
    pub(crate) fn market_manager_eliminated(&mut self, actor: &str) {
        if let Some(market) = &mut self.market {
            market.previews.retain(|_, p| p.actor != actor);
        }
        // Club-owned offers survive replacement. Fired actors lose all core
        // authorization; the new manager inherits decisions, not preview IDs.
    }

    pub(crate) fn market_contract_released(&mut self, player: &str) {
        if let Some(market) = &mut self.market {
            for offer in market.offers.values_mut().filter(|o| {
                o.player_id == player
                    && matches!(o.status, Status::Pending | Status::PendingRegistration)
            }) {
                offer.status = Status::Withdrawn;
            }
            market
                .previews
                .retain(|_, p| p.view.offer.player_id != player);
        }
    }
    pub fn market_view(&self, actor: &str) -> Result<MarketView, Error> {
        let club = &self.managers.get(actor).ok_or(Error::Unauthorized)?.club_id;
        let market = self.market.as_ref().ok_or(Error::Unavailable)?;
        let social = self.social.as_ref().ok_or(Error::Unavailable)?;
        Ok(MarketView {
            window: market.window.clone(),
            offers: market
                .offers
                .values()
                .filter(|o| &o.seller == club || &o.buyer == club)
                .cloned()
                .collect(),
            active_loans: social
                .source_players
                .values()
                .filter_map(|p| {
                    p.active_loan
                        .as_ref()
                        .filter(|loan| &loan.parent_team_id == club || &loan.loan_team_id == club)
                        .map(|loan| (p.id.clone(), loan.clone()))
                })
                .collect(),
        })
    }
    fn market_financial_consent(&self, club: &str) -> Result<FinancialConsent, Error> {
        let (account, snapshot) = self.economy_finance(club).map_err(error)?;
        Ok(FinancialConsent {
            balance: self.clubs[club].balance,
            transfer_budget: account.transfer_budget,
            wages: snapshot.annual_wage_bill,
            wage_budget: account.wage_budget,
            incoming_wage: 0,
            incoming_end: None,
        })
    }
    fn market_offer(&self, actor: &str, id: u64) -> Result<MarketOffer, Error> {
        let club = &self.managers.get(actor).ok_or(Error::Unauthorized)?.club_id;
        let offer = self
            .market
            .as_ref()
            .ok_or(Error::Unavailable)?
            .offers
            .get(&id)
            .ok_or(Error::Unavailable)?;
        if &offer.seller != club && &offer.buyer != club {
            return Err(Error::Unavailable);
        }
        Ok(offer.clone())
    }

    pub(crate) fn market_needs_consent(&self, actor: &str, id: u64) -> Result<bool, Error> {
        let offer = self.market_offer(actor, id)?;
        let deps = self.market_dependencies(&offer, false)?;
        let club = &self.managers[actor].club_id;
        let own = if club == &offer.buyer {
            &deps.buyer
        } else {
            &deps.seller
        };
        Ok(self
            .market
            .as_ref()
            .unwrap()
            .consents
            .get(&(id, club.clone()))
            != Some(own))
    }
    fn market_dependencies(
        &self,
        offer: &MarketOffer,
        buy_option: bool,
    ) -> Result<Dependencies, Error> {
        let market = self.market.as_ref().ok_or(Error::Unavailable)?;
        let career = self.career.as_ref().ok_or(Error::Unavailable)?;
        let player = self
            .players
            .get(&offer.player_id)
            .ok_or(Error::Unavailable)?;
        let source = &self
            .social
            .as_ref()
            .ok_or(Error::Unavailable)?
            .source_players[&player.id];
        if source.retired {
            return Err(Error::Unavailable);
        }
        if (!buy_option && player.club_id != offer.seller)
            || (buy_option && player.club_id != offer.buyer)
        {
            return Err(Error::Unavailable);
        }
        if !buy_option && source.active_loan.is_some() {
            return Err(error("be.error.transfers.playerAlreadyLoaned"));
        }
        if offer.status != Status::PendingRegistration
            && market.offers.values().any(|o| {
                o.id != offer.id
                    && o.player_id == offer.player_id
                    && o.status == Status::PendingRegistration
            })
        {
            return Err(error("be.error.transfers.offerNotPending"));
        }
        let registration = rules::registration_date(career.today, &market.window).map_err(error)?;
        if buy_option && registration != career.today {
            return Err(error("be.error.transfers.transferWindowClosed"));
        }
        let seller_roster = self
            .players
            .values()
            .filter(|p| p.club_id == offer.seller)
            .count();
        if !buy_option && self.minimum_squad_size > 0 && seller_roster <= self.minimum_squad_size {
            return Err(Error::SquadTooSmall);
        }
        let mut buyer = self.market_financial_consent(&offer.buyer)?;
        let mut seller = self.market_financial_consent(&offer.seller)?;
        for consent in [&mut buyer, &mut seller] {
            consent.incoming_wage = career.contracts[&player.id].weekly_wage;
            consent.incoming_end = career.contracts[&player.id].end_date;
        }
        match offer.terms {
            Terms::Transfer { fee } => {
                let fee = i64::try_from(fee).map_err(|_| Error::Overflow)?;
                if buyer.balance < fee {
                    return Err(Error::InsufficientFunds);
                }
                if buyer.transfer_budget < fee {
                    return Err(error("be.error.transfers.transferBudgetTooLow"));
                }
                seller.balance.checked_add(fee).ok_or(Error::Overflow)?;
                if !buy_option {
                    self.validate_contract_transfer(&player.id, &offer.buyer)?;
                }
                if buy_option
                    && !source.active_loan.as_ref().is_some_and(|loan| {
                        loan.parent_team_id == offer.seller
                            && loan.loan_team_id == offer.buyer
                            && loan.buy_option_fee == Some(fee as u64)
                    })
                {
                    return Err(Error::Unavailable);
                }
            }
            Terms::Loan {
                end_date,
                wage_contribution_pct,
                buy_option_fee,
            } => {
                if wage_contribution_pct > 100 || buy_option_fee == Some(0) {
                    return Err(error("Invalid loan terms"));
                }
                if offer.status == Status::PendingRegistration {
                    if end_date <= career.today
                        || source.contract_end.as_ref().is_some_and(|s| {
                            s.parse::<NaiveDate>().map_or(true, |end| end_date >= end)
                        })
                    {
                        return Err(error("be.error.transfers.invalidLoanEndDate"));
                    }
                } else {
                    rules::validate_loan_dates(source, registration, end_date).map_err(error)?;
                }
                let share = i64::from(career.contracts[&player.id].weekly_wage)
                    * i64::from(wage_contribution_pct)
                    / 100;
                let projected = buyer.wages.checked_add(share).ok_or(Error::Overflow)?;
                if buyer.balance < share {
                    return Err(Error::InsufficientFunds);
                }
                if projected > buyer.wages && (buyer.balance < 0 || projected > buyer.wage_budget) {
                    return Err(error("Board wage policy refuses loan"));
                }
            }
        }
        Ok(Dependencies {
            date: career.today,
            offer: offer.clone(),
            buyer,
            seller,
            owner: player.club_id.clone(),
            contract: ContractTerms {
                weekly_wage: career.contracts[&player.id].weekly_wage,
                end_date: career.contracts[&player.id].end_date,
            },
            active_loan: source.active_loan.clone(),
            seller_roster,
            registration,
        })
    }
    fn market_preview(
        &mut self,
        actor: &str,
        offer: MarketOffer,
        buy_option: bool,
    ) -> Result<MarketPreview, Error> {
        let dependencies = self.market_dependencies(&offer, buy_option)?;
        let club = &self.managers[actor].club_id;
        let other = if club == &offer.buyer {
            &offer.seller
        } else {
            &offer.buyer
        };
        let other_facts = if other == &offer.buyer {
            &dependencies.buyer
        } else {
            &dependencies.seller
        };
        let pending_other_consent = !buy_option
            && self
                .market
                .as_ref()
                .unwrap()
                .consents
                .get(&(offer.id, other.clone()))
                != Some(other_facts);
        let (fee, incoming) = match offer.terms {
            Terms::Transfer { fee } => (
                i64::try_from(fee).map_err(|_| Error::Overflow)?,
                if buy_option {
                    0
                } else {
                    i64::from(dependencies.contract.weekly_wage)
                },
            ),
            Terms::Loan {
                wage_contribution_pct,
                ..
            } => (
                0,
                i64::from(dependencies.contract.weekly_wage) * i64::from(wage_contribution_pct)
                    / 100,
            ),
        };
        let (facts, delta, wage_delta) = if club == &offer.buyer {
            (&dependencies.buyer, -fee, incoming)
        } else {
            (&dependencies.seller, fee, -incoming)
        };
        let view = MarketPreview {
            id: self.sequence,
            offer,
            registration_date: dependencies.registration,
            balance_before: facts.balance,
            balance_after: facts.balance.checked_add(delta).ok_or(Error::Overflow)?,
            transfer_budget_before: facts.transfer_budget,
            transfer_budget_after: facts
                .transfer_budget
                .checked_add(delta)
                .ok_or(Error::Overflow)?,
            projected_annual_wages: facts.wages.checked_add(wage_delta).ok_or(Error::Overflow)?,
            wage_budget: facts.wage_budget,
            pending_other_consent,
        };
        self.market.as_mut().unwrap().previews.insert(
            view.id,
            StoredPreview {
                actor: actor.into(),
                day: self.window.day,
                view: view.clone(),
                dependencies,
                buy_option,
            },
        );
        Ok(view)
    }
}
