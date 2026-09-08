//! Single-owner management commands. The host supplies authenticated identity and
//! trusted time; neither belongs in an untrusted client command payload.
pub mod football;
pub mod matches;
pub mod physical;
pub mod recovery;
pub mod selection;
pub mod window;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use window::DayWindow;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Club {
    pub id: String,
    pub name: String,
    pub balance: u64,
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
pub enum Command {
    SetRecovery { mode: recovery::RecoveryMode },
    SetLineup { player_ids: Vec<String> },
    Offer { player_id: String, fee: u64 },
    Review { offer_id: u64 },
    Confirm { preview_id: u64 },
    Reject { offer_id: u64 },
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    pub day: u32,
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Error {
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
    pub seller_balance_after: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct Dependencies {
    buyer_balance: u64,
    seller_balance: u64,
    buyer_revision: u64,
    seller_revision: u64,
    player_revision: u64,
}

struct StoredPreview {
    actor: String,
    day: u32,
    view: Preview,
    dependencies: Dependencies,
}

/// Call only from one authoritative dispatcher. No asynchronous confirmation is
/// awaited inside this state owner. It is not itself a network authentication layer.
pub struct Management {
    clubs: BTreeMap<String, Club>,
    players: BTreeMap<String, Player>,
    managers: BTreeMap<String, Manager>,
    club_revisions: BTreeMap<String, u64>,
    player_revisions: BTreeMap<String, u64>,
    offers: BTreeMap<u64, Offer>,
    previews: BTreeMap<u64, StoredPreview>,
    receipts: BTreeMap<(String, String), (Request, Receipt)>,
    window: DayWindow,
    closed: bool,
    sequence: u64,
    lineups: BTreeMap<String, Vec<String>>,
    recovery_modes: BTreeMap<String, recovery::RecoveryMode>,
    recovery_enabled: bool,
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
                .any(|p| !clubs.iter().any(|c| c.id == p.club_id))
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
        })
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

    /// Host-only elimination seam, not a manager tool. Football firing evaluation
    /// and bot replacement are intentionally outside this first transaction slice.
    pub fn eliminate(&mut self, actor: &str) -> Result<(), Error> {
        let manager = self.managers.remove(actor).ok_or(Error::Unauthorized)?;
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
        let buyer = &self.clubs[&offer.buyer];
        let seller = &self.clubs[&offer.seller];
        if buyer.balance < offer.fee {
            return Err(Error::InsufficientFunds);
        }
        seller
            .balance
            .checked_add(offer.fee)
            .ok_or(Error::Overflow)?;
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
            seller_balance_after: dependencies.seller_balance + offer.fee,
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
        match command {
            Command::SetRecovery { mode } => {
                if !self.recovery_enabled {
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
                    || player_ids
                        .iter()
                        .any(|id| self.players.get(id).is_none_or(|p| p.club_id != club))
                {
                    return Err(Error::Unavailable);
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
                if self.clubs[&club].balance < *fee {
                    return Err(Error::InsufficientFunds);
                }
                let player = self.players.get(player_id).ok_or(Error::Unavailable)?;
                if player.club_id == club {
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
                self.clubs.get_mut(&offer.buyer).unwrap().balance -= offer.fee;
                self.clubs.get_mut(&offer.seller).unwrap().balance += offer.fee;
                self.club_revisions
                    .insert(offer.buyer.clone(), buyer_revision);
                self.club_revisions
                    .insert(offer.seller.clone(), seller_revision);
                self.players.get_mut(&offer.player_id).unwrap().club_id = offer.buyer;
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
