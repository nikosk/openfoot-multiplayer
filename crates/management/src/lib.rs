//! Single-owner management commands. The host supplies authenticated identity and
//! trusted time; neither belongs in an untrusted client command payload.
pub mod aging;
pub mod availability;
pub mod board;
pub mod bot_manager;
pub mod bot_training;
pub mod calendar;
pub mod career;
pub mod checkpoint;
pub mod competition_schedule;
pub mod competitions;
pub mod contract_social;
pub mod contracts;
pub mod delegated_contracts;
pub mod economy;
pub mod economy_runtime;
pub mod facilities;
pub mod finances;
pub mod football;
pub mod inbox;
pub mod market;
pub mod market_rules;
pub mod matches;
pub mod national;
pub mod news_runtime;
pub mod personnel;
pub mod physical;
pub mod player_history;
pub mod promotion;
pub mod recovery;
pub mod scouting;
pub mod seasons;
pub mod selection;
pub mod social;
pub mod squad_plan;
pub mod staff;
pub mod statistics;
pub mod tactics;
pub mod team_history;
pub mod training;
pub mod training_commands;
pub mod window;
pub mod youth;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use window::DayWindow;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Club {
    pub id: String,
    pub name: String,
    pub balance: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    pub id: String,
    pub name: String,
    pub club_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manager {
    pub id: String,
    pub club_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Command {
    Market(market::MarketCommand),
    Economy(economy_runtime::EconomyCommand),
    Personnel(personnel::PersonnelCommand),
    Social(social::SocialCommand),
    SetSquadPlan { plan: squad_plan::SquadPlan },
    Training(training_commands::TrainingCommand),
    Career(career::CareerCommand),
    SetMatchPlan { plan: tactics::MatchPlan },
    SetRecovery { mode: recovery::RecoveryMode },
    SetLineup { player_ids: Vec<String> },
    Offer { player_id: String, fee: u64 },
    Review { offer_id: u64 },
    Confirm { preview_id: u64 },
    Reject { offer_id: u64 },
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: String,
    pub day: u32,
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Error {
    Personnel(String),
    Social(String),
    Contract(String),
    InvalidSetup,
    Unauthorized,
    InvalidRequest,
    RequestIdReused,
    WrongDay,
    DayClosed,
    AlreadyReady,
    Unavailable,
    InsufficientFunds,
    InvalidFee,
    Overflow,
    SquadTooSmall,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OfferStatus {
    Pending,
    Accepted,
    Rejected,
    Withdrawn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    pub id: u64,
    pub player_id: String,
    pub buyer: String,
    pub seller: String,
    pub fee: u64,
    pub status: OfferStatus,
}

/// Permission-filtered preview. Internal dependency values are never returned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preview {
    pub id: u64,
    pub offer: Offer,
    pub seller_balance_after: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Market(market::MarketOutcome),
    Economy(economy_runtime::EconomyOutcome),
    Personnel(serde_json::Value),
    Social(social::SocialOutcome),
    SquadPlanSet,
    TrainingSet,
    Career(career::CareerOutcome),
    MatchPlanSet,
    RecoverySet,
    LineupSet,
    Offered(Offer),
    Preview(Preview),
    RefreshRequired(Preview),
    Transferred { offer_id: u64 },
    Rejected,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub sequence: u64,
    pub result: Result<Outcome, Error>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicClub {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicState {
    pub day: u32,
    pub clubs: Vec<PublicClub>,
    pub players: Vec<Player>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagerView {
    pub club: Club,
    pub offers: Vec<Offer>,
    pub ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Dependencies {
    buyer_balance: i64,
    seller_balance: i64,
    buyer_revision: u64,
    seller_revision: u64,
    player_revision: u64,
}

#[derive(Clone, Serialize, Deserialize)]
struct StoredPreview {
    actor: String,
    day: u32,
    view: Preview,
    dependencies: Dependencies,
}

/// Call only from one authoritative dispatcher. No asynchronous confirmation is
/// awaited inside this state owner. It is not itself a network authentication layer.
#[derive(Clone, Serialize, Deserialize)]
pub struct Management {
    clubs: BTreeMap<String, Club>,
    players: BTreeMap<String, Player>,
    managers: BTreeMap<String, Manager>,
    club_revisions: BTreeMap<String, u64>,
    player_revisions: BTreeMap<String, u64>,
    offers: BTreeMap<u64, Offer>,
    previews: BTreeMap<u64, StoredPreview>,
    #[serde(with = "crate::checkpoint::entries")]
    receipts: BTreeMap<(String, String), (Request, Receipt)>,
    window: DayWindow,
    closed: bool,
    sequence: u64,
    lineups: BTreeMap<String, Vec<String>>,
    recovery_modes: BTreeMap<String, recovery::RecoveryMode>,
    recovery_enabled: bool,
    match_plans: BTreeMap<String, tactics::MatchPlan>,
    minimum_squad_size: usize,
    career: Option<career::CareerState>,
    #[serde(default)]
    training: Option<training_commands::TrainingSetup>,
    #[serde(default)]
    availability: Option<BTreeMap<String, availability::Availability>>,
    #[serde(default)]
    injury_events: Vec<(String, availability::InjuryEvent)>,
    #[serde(default)]
    squad_profiles: Option<BTreeMap<String, squad_plan::PositionProfile>>,
    #[serde(default)]
    squad_plans: BTreeMap<String, squad_plan::SquadPlan>,
    #[serde(default)]
    social: Option<social::SocialState>,
    #[serde(default)]
    personnel: Option<personnel::PersonnelState>,
    #[serde(default)]
    economy: Option<economy_runtime::EconomyState>,
    #[serde(default)]
    player_history: Option<player_history::PlayerHistoryState>,
    #[serde(default)]
    market: Option<market::MarketState>,
    #[serde(default)]
    team_history: Option<team_history::TeamHistoryState>,
    #[serde(default)]
    news: Option<news_runtime::NewsState>,
}

impl Management {
    pub fn new(
        clubs: Vec<Club>,
        players: Vec<Player>,
        managers: Vec<Manager>,
        day: u32,
        deadline_ms: u64,
    ) -> Result<Self, Error> {
        fn unique<'a>(ids: impl Iterator<Item = &'a str>) -> bool {
            let mut seen = std::collections::BTreeSet::new();
            ids.into_iter().all(|id| !id.is_empty() && seen.insert(id))
        }
        if clubs.is_empty()
            || managers.is_empty()
            || !unique(clubs.iter().map(|x| x.id.as_str()))
            || !unique(players.iter().map(|x| x.id.as_str()))
            || !unique(managers.iter().map(|x| x.id.as_str()))
            || !unique(managers.iter().map(|x| x.club_id.as_str()))
            || players
                .iter()
                .any(|p| !p.club_id.is_empty() && !clubs.iter().any(|c| c.id == p.club_id))
            || managers
                .iter()
                .any(|m| !clubs.iter().any(|c| c.id == m.club_id))
        {
            return Err(Error::InvalidSetup);
        }
        Ok(Self {
            club_revisions: clubs.iter().map(|c| (c.id.clone(), 0)).collect(),
            player_revisions: players.iter().map(|p| (p.id.clone(), 0)).collect(),
            clubs: clubs.into_iter().map(|c| (c.id.clone(), c)).collect(),
            players: players.into_iter().map(|p| (p.id.clone(), p)).collect(),
            managers: managers.into_iter().map(|m| (m.id.clone(), m)).collect(),
            offers: BTreeMap::new(),
            previews: BTreeMap::new(),
            receipts: BTreeMap::new(),
            window: DayWindow::new(day, deadline_ms),
            closed: false,
            sequence: 0,
            lineups: BTreeMap::new(),
            recovery_modes: BTreeMap::new(),
            recovery_enabled: false,
            match_plans: BTreeMap::new(),
            minimum_squad_size: 0,
            career: None,
            training: None,
            availability: None,
            injury_events: vec![],
            squad_profiles: None,
            squad_plans: BTreeMap::new(),
            social: None,
            personnel: None,
            economy: None,
            player_history: None,
            market: None,
            team_history: None,
            news: None,
        })
    }

    /// Setup-only registration safety for scheduled leagues. A transfer cannot
    /// leave the seller below the engine's eleven-player match requirement.
    pub fn require_match_rosters(&mut self) -> Result<(), Error> {
        if self.sequence != 0 {
            return Err(Error::InvalidRequest);
        }
        if self
            .clubs
            .keys()
            .any(|club| self.players.values().filter(|p| &p.club_id == club).count() < 11)
        {
            return Err(Error::SquadTooSmall);
        }
        self.minimum_squad_size = 11;
        Ok(())
    }

    pub fn public_state(&self) -> PublicState {
        PublicState {
            day: self.window.day,
            clubs: self
                .clubs
                .values()
                .map(|c| PublicClub {
                    id: c.id.clone(),
                    name: c.name.clone(),
                })
                .collect(),
            players: self.players.values().cloned().collect(),
        }
    }

    pub fn manager_view(&self, actor: &str) -> Result<ManagerView, Error> {
        let manager = self.managers.get(actor).ok_or(Error::Unauthorized)?;
        Ok(ManagerView {
            club: self.clubs[&manager.club_id].clone(),
            offers: self
                .offers
                .values()
                .filter(|o| o.buyer == manager.club_id || o.seller == manager.club_id)
                .cloned()
                .collect(),
            ready: self.window.is_ready(actor),
        })
    }

    pub fn closed(&mut self, now_ms: u64) -> bool {
        self.closed |= self
            .window
            .closure(now_ms, self.managers.keys().map(String::as_str))
            .is_some();
        self.closed
    }

    pub fn lineup(&self, actor: &str) -> Result<Vec<String>, Error> {
        let manager = self.managers.get(actor).ok_or(Error::Unauthorized)?;
        Ok(self
            .lineups
            .get(&manager.club_id)
            .cloned()
            .unwrap_or_default())
    }

    /// Host-only calendar seam. Expected day prevents duplicate advancement.
    pub fn next_day(
        &mut self,
        expected_day: u32,
        now_ms: u64,
        next_deadline_ms: u64,
    ) -> Result<(), Error> {
        if expected_day != self.window.day {
            return Err(Error::WrongDay);
        }
        if next_deadline_ms <= now_ms {
            return Err(Error::InvalidRequest);
        }
        let next = expected_day.checked_add(1).ok_or(Error::Overflow)?;
        if !self.closed(now_ms) {
            return Err(Error::InvalidRequest);
        }
        self.window = DayWindow::new(next, next_deadline_ms);
        self.closed = false;
        self.previews.clear();
        Ok(())
    }

    /// Host-only elimination seam, not a manager tool. Authorization is removed
    /// before receipt lookup, so previously accepted requests cannot be replayed.
    pub fn eliminate(&mut self, actor: &str) -> Result<(), Error> {
        let manager = self.managers.remove(actor).ok_or(Error::Unauthorized)?;
        self.career_manager_eliminated(actor);
        self.market_manager_eliminated(actor);
        for offer in self.offers.values_mut() {
            if offer.status == OfferStatus::Pending
                && (offer.buyer == manager.club_id || offer.seller == manager.club_id)
            {
                offer.status = OfferStatus::Withdrawn;
            }
        }
        self.previews.retain(|_, p| p.actor != actor);
        self.closed(0);
        Ok(())
    }

    pub fn dispatch(
        &mut self,
        actor: &str,
        request: Request,
        now_ms: u64,
    ) -> Result<Receipt, Error> {
        if !self.managers.contains_key(actor) {
            return Err(Error::Unauthorized);
        }
        if request.id.is_empty() {
            return Err(Error::InvalidRequest);
        }
        let key = (actor.to_owned(), request.id.clone());
        if let Some((original, receipt)) = self.receipts.get(&key) {
            return if original == &request {
                Ok(receipt.clone())
            } else {
                Err(Error::RequestIdReused)
            };
        }
        self.sequence = self.sequence.checked_add(1).ok_or(Error::Overflow)?;
        let result = if request.day != self.window.day {
            Err(Error::WrongDay)
        } else if self.closed(now_ms) {
            Err(Error::DayClosed)
        } else {
            self.execute(actor, &request.command)
        };
        let receipt = Receipt {
            sequence: self.sequence,
            result,
        };
        self.receipts.insert(key, (request, receipt.clone()));
        self.closed(now_ms);
        Ok(receipt)
    }

    fn seller_offer(&self, actor: &str, id: u64) -> Result<Offer, Error> {
        let club = &self.managers.get(actor).ok_or(Error::Unauthorized)?.club_id;
        self.offers
            .get(&id)
            .filter(|o| &o.seller == club && o.status == OfferStatus::Pending)
            .cloned()
            .ok_or(Error::Unavailable)
    }

    fn dependencies(&self, offer: &Offer) -> Result<Dependencies, Error> {
        let player = self
            .players
            .get(&offer.player_id)
            .ok_or(Error::Unavailable)?;
        if player.club_id != offer.seller {
            return Err(Error::Unavailable);
        }
        self.validate_contract_transfer(&offer.player_id, &offer.buyer)?;
        let buyer = &self.clubs[&offer.buyer];
        let seller = &self.clubs[&offer.seller];
        let fee = i64::try_from(offer.fee).map_err(|_| Error::Overflow)?;
        if buyer.balance < fee {
            return Err(Error::InsufficientFunds);
        }
        seller.balance.checked_add(fee).ok_or(Error::Overflow)?;
        Ok(Dependencies {
            buyer_balance: buyer.balance,
            seller_balance: seller.balance,
            buyer_revision: self.club_revisions[&offer.buyer],
            seller_revision: self.club_revisions[&offer.seller],
            player_revision: self.player_revisions[&offer.player_id],
        })
    }

    fn preview(&mut self, actor: &str, offer: Offer) -> Result<Preview, Error> {
        let dependencies = self.dependencies(&offer)?;
        let view = Preview {
            id: self.sequence,
            seller_balance_after: dependencies.seller_balance
                + i64::try_from(offer.fee).map_err(|_| Error::Overflow)?,
            offer,
        };
        self.previews.insert(
            view.id,
            StoredPreview {
                actor: actor.into(),
                day: self.window.day,
                view: view.clone(),
                dependencies,
            },
        );
        Ok(view)
    }

    fn execute(&mut self, actor: &str, command: &Command) -> Result<Outcome, Error> {
        let club = self.managers[actor].club_id.clone();
        if self.market.is_some()
            && matches!(
                command,
                Command::Offer { .. }
                    | Command::Review { .. }
                    | Command::Confirm { .. }
                    | Command::Reject { .. }
            )
        {
            return Err(Error::Unavailable);
        }
        match command {
            Command::Social(command) => {
                let mut staged = self.clone();
                let outcome = staged.execute_social(actor, command)?;
                *self = staged;
                Ok(Outcome::Social(outcome))
            }
            Command::Personnel(command) => {
                let mut staged = self.clone();
                let outcome = staged.execute_personnel(actor, command)?;
                *self = staged;
                Ok(Outcome::Personnel(outcome))
            }
            Command::Economy(command) => {
                let mut staged = self.clone();
                let outcome = staged.execute_economy(actor, command)?;
                *self = staged;
                Ok(Outcome::Economy(outcome))
            }
            Command::Market(command) => {
                let mut staged = self.clone();
                let outcome = staged.execute_market(actor, command)?;
                *self = staged;
                Ok(Outcome::Market(outcome))
            }
            Command::SetSquadPlan { plan } => {
                if self.window.is_ready(actor) {
                    return Err(Error::AlreadyReady);
                }
                let profiles = self.squad_profiles.as_ref().ok_or(Error::Unavailable)?;
                let owned = profiles
                    .iter()
                    .filter(|(id, _)| self.players[*id].club_id == club)
                    .map(|(id, profile)| (id.clone(), profile.clone()))
                    .collect();
                squad_plan::validate_plan(
                    plan,
                    &owned,
                    self.lineups.get(&club).map(Vec::as_slice).unwrap_or(&[]),
                )
                .map_err(|_| Error::InvalidRequest)?;
                self.squad_plans.insert(club, plan.clone());
                Ok(Outcome::SquadPlanSet)
            }
            Command::Training(command) => {
                self.execute_training(actor, command)?;
                Ok(Outcome::TrainingSet)
            }
            Command::Career(command) => {
                if self.window.is_ready(actor) {
                    return Err(Error::AlreadyReady);
                }
                let mut staged = self.clone();
                let outcome = staged.execute_career(actor, command)?;
                staged.sync_contract_social(command, &outcome)?;
                *self = staged;
                Ok(Outcome::Career(outcome))
            }
            Command::SetMatchPlan { plan } => {
                if self.window.is_ready(actor) {
                    return Err(Error::AlreadyReady);
                }
                self.match_plans.insert(club, plan.clone());
                Ok(Outcome::MatchPlanSet)
            }
            Command::SetRecovery { mode } => {
                if !self.recovery_enabled || self.training.is_some() {
                    return Err(Error::Unavailable);
                }
                if self.window.is_ready(actor) {
                    return Err(Error::AlreadyReady);
                }
                self.recovery_modes.insert(club, *mode);
                Ok(Outcome::RecoverySet)
            }
            Command::SetLineup { player_ids } => {
                if self.window.is_ready(actor) {
                    return Err(Error::AlreadyReady);
                }
                let ids: std::collections::BTreeSet<_> = player_ids.iter().collect();
                if ids.len() != 11
                    || player_ids.len() != 11
                    || player_ids.iter().any(|id| {
                        self.players.get(id).is_none_or(|p| p.club_id != club)
                            || self
                                .availability
                                .as_ref()
                                .is_some_and(|states| !states[id].is_available())
                    })
                {
                    return Err(Error::Unavailable);
                }
                if let Some(profiles) = &self.squad_profiles {
                    let owned = profiles
                        .iter()
                        .filter(|(id, _)| self.players[*id].club_id == club)
                        .map(|(id, profile)| (id.clone(), profile.clone()))
                        .collect();
                    let mut plan = self.squad_plans.get(&club).cloned().unwrap_or_default();
                    squad_plan::reconcile_roles(&mut plan, &owned, player_ids)
                        .map_err(|_| Error::InvalidRequest)?;
                    self.squad_plans.insert(club.clone(), plan);
                }
                self.lineups.insert(club, player_ids.clone());
                Ok(Outcome::LineupSet)
            }
            Command::Ready => {
                self.window.mark_ready(actor);
                Ok(Outcome::Ready)
            }
            Command::Offer { player_id, fee } => {
                if self.window.is_ready(actor) {
                    return Err(Error::AlreadyReady);
                }
                if *fee == 0 {
                    return Err(Error::InvalidFee);
                }
                let signed_fee = i64::try_from(*fee).map_err(|_| Error::Overflow)?;
                if self.clubs[&club].balance < signed_fee {
                    return Err(Error::InsufficientFunds);
                }
                let player = self.players.get(player_id).ok_or(Error::Unavailable)?;
                if player.club_id == club || player.club_id.is_empty() {
                    return Err(Error::Unavailable);
                }
                let offer = Offer {
                    id: self.sequence,
                    player_id: player_id.clone(),
                    buyer: club,
                    seller: player.club_id.clone(),
                    fee: *fee,
                    status: OfferStatus::Pending,
                };
                self.offers.insert(offer.id, offer.clone());
                Ok(Outcome::Offered(offer))
            }
            Command::Review { offer_id } => {
                let offer = self.seller_offer(actor, *offer_id)?;
                Ok(Outcome::Preview(self.preview(actor, offer)?))
            }
            Command::Reject { offer_id } => {
                self.seller_offer(actor, *offer_id)?;
                self.offers.get_mut(offer_id).unwrap().status = OfferStatus::Rejected;
                Ok(Outcome::Rejected)
            }
            Command::Confirm { preview_id } => {
                let stored = self
                    .previews
                    .get(preview_id)
                    .filter(|p| p.actor == actor && p.day == self.window.day)
                    .ok_or(Error::Unavailable)?;
                let offer = self.seller_offer(actor, stored.view.offer.id)?;
                if self.minimum_squad_size > 0
                    && self
                        .players
                        .values()
                        .filter(|p| p.club_id == offer.seller)
                        .count()
                        <= self.minimum_squad_size
                {
                    return Err(Error::SquadTooSmall);
                }
                let dependencies = self.dependencies(&offer)?;
                if dependencies != stored.dependencies || offer != stored.view.offer {
                    let updated = self.preview(actor, offer)?;
                    self.previews.remove(preview_id);
                    return Ok(Outcome::RefreshRequired(updated));
                }
                // All fallible validation precedes mutation; no await or callback
                // can interleave between this point and the complete commit.
                let buyer_revision = dependencies
                    .buyer_revision
                    .checked_add(1)
                    .ok_or(Error::Overflow)?;
                let seller_revision = dependencies
                    .seller_revision
                    .checked_add(1)
                    .ok_or(Error::Overflow)?;
                let player_revision = dependencies
                    .player_revision
                    .checked_add(1)
                    .ok_or(Error::Overflow)?;
                let fee = i64::try_from(offer.fee).map_err(|_| Error::Overflow)?;
                self.clubs.get_mut(&offer.buyer).unwrap().balance -= fee;
                self.clubs.get_mut(&offer.seller).unwrap().balance += fee;
                self.club_revisions
                    .insert(offer.buyer.clone(), buyer_revision);
                self.club_revisions
                    .insert(offer.seller.clone(), seller_revision);
                self.players.get_mut(&offer.player_id).unwrap().club_id = offer.buyer;
                self.contract_transferred(&offer.player_id);
                self.player_revisions
                    .insert(offer.player_id.clone(), player_revision);
                for other in self
                    .offers
                    .values_mut()
                    .filter(|o| o.player_id == offer.player_id && o.status == OfferStatus::Pending)
                {
                    other.status = if other.id == offer.id {
                        OfferStatus::Accepted
                    } else {
                        OfferStatus::Withdrawn
                    };
                }
                self.previews.remove(preview_id);
                Ok(Outcome::Transferred { offer_id: offer.id })
            }
        }
    }
}
