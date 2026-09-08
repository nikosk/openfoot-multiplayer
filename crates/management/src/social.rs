//! Multiplayer integration for pinned source inbox/conversation behavior.
//! Full source player records retain fields not yet modeled elsewhere; current
//! ownership, engine state and contract scalars remain authoritative in their
//! existing registries. A conversation writes back only morale and morale_core.
//! No per-manager copy of the world or privileged selected-manager Game is used.
use crate::football::Football;
use crate::inbox::{ActionResolution, InboxError, InboxStore, conversations};
use crate::{Command, Error, Management, Receipt, Request};
use chrono::NaiveDate;
use domain::message::InboxMessage;
use domain::player::{Player as SourcePlayer, PlayerMoraleCore, RenewalSessionStatus};
use rand::{SeedableRng, rngs::StdRng};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[path = "social_daily.rs"]
mod daily;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialSetup {
    pub seed: u64,
    pub source_players: BTreeMap<String, SourcePlayer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialState {
    pub seed: u64,
    pub source_players: BTreeMap<String, SourcePlayer>,
    pub inbox: InboxStore,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum SocialCommand {
    MarkRead {
        message_id: String,
    },
    MarkAllRead,
    Delete {
        message_id: String,
    },
    ClearOld,
    Respond {
        message_id: String,
        action_id: String,
        option_id: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SocialOutcome {
    MarkedRead,
    Deleted,
    Cleared { count: usize },
    Responded(ActionResolution),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SocialView {
    pub players: BTreeMap<String, PlayerMoraleCore>,
}

fn inbox_error(error: InboxError) -> Error {
    match error {
        InboxError::Unavailable => Error::Unavailable,
        other => Error::Social(format!("{other:?}")),
    }
}

impl Football {
    /// Called once per newly completed fixture, before that result enters results.
    /// Outer day advancement stages the entire Football, including RNG-derived effects.
    pub(crate) fn apply_social_match(
        &mut self,
        fixture: &crate::football::FinishedFixture,
        prior_same_day: &[crate::football::FinishedFixture],
    ) -> Result<(), String> {
        let Some(state) = &self.management.social else {
            return Ok(());
        };
        let mut rng =
            StdRng::seed_from_u64(state.seed ^ u64::from(fixture.day) ^ 0x6d61_7463_6873_6f63);
        let mut players: Vec<_> = self.project_source_players()?.into_values().collect();
        daily::apply_player_stats(&mut players, &fixture.report, &fixture.home, &fixture.away)?;
        if let Some(states) = &self.management.availability {
            for player in &mut players {
                player.stats.yellow_cards = states[&player.id].yellow_cards;
                player.stats.red_cards = states[&player.id].red_cards;
            }
        }
        daily::resolve_post_match_promises(
            &mut players,
            &fixture.report,
            &fixture.home,
            &fixture.away,
        );
        daily::update_post_match_morale(
            &mut players,
            &mut rng,
            &fixture.report,
            &fixture.home,
            &fixture.away,
        );
        for club in [&fixture.home, &fixture.away] {
            let mut form: Vec<String> = self
                .results
                .iter()
                .chain(prior_same_day.iter())
                .filter(|r| &r.home == club || &r.away == club)
                // Source end_of_season clears club form; archives are not form.
                .filter(|r| self.fixtures.iter().any(|f| f.id == r.fixture_id))
                .map(|r| {
                    let (own, other) = if &r.home == club {
                        (r.report.home_goals, r.report.away_goals)
                    } else {
                        (r.report.away_goals, r.report.home_goals)
                    };
                    if own > other {
                        "W"
                    } else if own < other {
                        "L"
                    } else {
                        "D"
                    }
                    .into()
                })
                .collect();
            let (own, other) = if &fixture.home == club {
                (fixture.report.home_goals, fixture.report.away_goals)
            } else {
                (fixture.report.away_goals, fixture.report.home_goals)
            };
            form.push(
                if own > other {
                    "W"
                } else if own < other {
                    "L"
                } else {
                    "D"
                }
                .into(),
            );
            daily::apply_streak(&mut players, club, &form, &mut rng);
        }
        self.management.sync_social_players(players)
    }

    /// Source fatigue warnings occur after training, before same-date expiry.
    pub(crate) fn deliver_social_training_warnings(
        &mut self,
        closing_date: NaiveDate,
    ) -> Result<(), String> {
        if self.management.social.is_none() {
            return Ok(());
        }
        let players: Vec<_> = self.project_source_players()?.into_values().collect();
        let managers: Vec<_> = self.management.managers.values().cloned().collect();
        for manager in &managers {
            if !self
                .fixtures
                .iter()
                .any(|f| f.day == self.management.window.day)
            {
                if let Some(training) = &self.management.training {
                    let staff: Vec<_> = self
                        .management
                        .personnel
                        .as_ref()
                        .map(|p| p.staff.values().cloned().collect())
                        .unwrap_or_default();
                    if let Some(message) = daily::fitness_message(
                        &players,
                        &manager.club_id,
                        closing_date,
                        &training.clubs[&manager.club_id],
                        &staff,
                    ) {
                        self.management
                            .social
                            .as_mut()
                            .unwrap()
                            .inbox
                            .deliver(&manager.id, message)
                            .map_err(|e| format!("Fitness delivery: {e:?}"))?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Closing-date conversations, after contract settlement and before injury recovery.
    pub(crate) fn advance_social_day(&mut self, closing_date: NaiveDate) -> Result<(), String> {
        let Some(state) = &self.management.social else {
            return Ok(());
        };
        let seed = state.seed;
        let mut players: Vec<_> = self.project_source_players()?.into_values().collect();
        let managers: Vec<_> = self.management.managers.values().cloned().collect();
        for (index, manager) in managers.iter().enumerate() {
            let mut rng = StdRng::seed_from_u64(
                seed ^ u64::from(self.management.window.day)
                    ^ (index as u64).rotate_left(32)
                    ^ 0x6461_696c_7973_6f63,
            );
            let messages = self
                .management
                .social
                .as_ref()
                .unwrap()
                .inbox
                .history(&manager.id);
            let played = self
                .fixtures
                .iter()
                .filter(|f| {
                    (f.home == manager.club_id || f.away == manager.club_id)
                        && f.day <= self.management.window.day
                })
                .count();
            let suppressed = self
                .management
                .career
                .as_ref()
                .unwrap()
                .contracts
                .iter()
                .filter(|(_, c)| c.let_expire)
                .map(|(id, _)| id.clone())
                .collect();
            let generated = daily::generate(
                &mut players,
                &manager.club_id,
                played,
                closing_date,
                &messages,
                &suppressed,
                &mut rng,
            );
            for message in generated {
                self.management
                    .social
                    .as_mut()
                    .unwrap()
                    .inbox
                    .deliver(&manager.id, message)
                    .map_err(|e| format!("Social delivery: {e:?}"))?;
            }
        }
        self.management.sync_social_players(players)
    }

    /// Publish only newly generated injury events; an unchanged ID is a no-op.
    pub(crate) fn deliver_social_injury(
        &mut self,
        club: &str,
        event: &crate::availability::InjuryEvent,
        date: NaiveDate,
    ) -> Result<(), String> {
        let Some(social) = &mut self.management.social else {
            return Ok(());
        };
        let Some(manager) = self
            .management
            .managers
            .values()
            .find(|m| m.club_id == club)
        else {
            return Ok(());
        };
        let name = &self.management.players[&event.player_id].name;
        let mut rng = StdRng::seed_from_u64(
            social.seed ^ u64::from(self.management.window.day) ^ 0x696e_6a75_7279_6d73,
        );
        social
            .inbox
            .deliver(
                &manager.id,
                daily::injury_message(event, name, date, &mut rng),
            )
            .map_err(|e| format!("Injury delivery: {e:?}"))?;
        Ok(())
    }
    pub fn configure_social(
        &mut self,
        source_players: BTreeMap<String, SourcePlayer>,
        seed: u64,
    ) -> Result<(), String> {
        if self.started
            || self.management.sequence != 0
            || self.management.social.is_some()
            || self.management.career.is_none()
        {
            return Err(
                "Social records require career and can only be configured once before commands"
                    .into(),
            );
        }
        if source_players.keys().ne(self.management.players.keys())
            || source_players.iter().any(|(id, source)| {
                id != &source.id
                    || source.team_id.as_deref().unwrap_or("")
                        != self.management.players[id].club_id
            })
        {
            return Err(
                "Source social players must match registered identities and initial ownership"
                    .into(),
            );
        }
        let mut staged = self.management.clone();
        staged.validate_initial_retired_source(&source_players)?;
        staged.social = Some(SocialState {
            seed,
            source_players,
            inbox: InboxStore::default(),
        });
        staged.validate_social_checkpoint()?;
        self.management = staged;
        Ok(())
    }

    /// Project current implemented fields onto full retained source records.
    /// Granular positions come from training metadata, never the engine's deployed
    /// coarse position. Full names, relationships and unimplemented data survive.
    pub fn project_source_players(&self) -> Result<BTreeMap<String, SourcePlayer>, String> {
        let social = self
            .management
            .social
            .as_ref()
            .ok_or("Social records unavailable")?;
        let career = self
            .management
            .career
            .as_ref()
            .ok_or("Social career unavailable")?;
        let mut projected = social.source_players.clone();
        for (id, source) in &mut projected {
            let owner = self
                .management
                .players
                .get(id)
                .ok_or("Missing source ownership")?;
            let engine = self
                .attributes
                .get(id)
                .ok_or("Missing source engine attributes")?;
            let contract = career.contracts.get(id).ok_or("Missing source contract")?;
            source.id = id.clone();
            source.match_name = owner.name.clone();
            source.team_id = (!owner.club_id.is_empty()).then(|| owner.club_id.clone());
            source.attributes = serde_json::from_value(
                serde_json::to_value(engine).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
            source.condition = engine.condition;
            source.fitness = engine.fitness;
            source.ovr = engine.ovr;
            source.traits = serde_json::from_value(
                serde_json::to_value(&engine.traits).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
            source.date_of_birth = contract.date_of_birth.to_string();
            source.wage = contract.weekly_wage;
            source.contract_end = contract.end_date.map(|date| date.to_string());
            source.market_value = contract.market_value;
            source.morale = contract.morale;
            source.morale_core.manager_trust = contract.manager_trust;
            // Do not reconstruct issue categories, treatment memory, promises,
            // or renewal sessions from lossy compatibility booleans.
            if let Some(training) = &self.management.training {
                let meta = training
                    .players
                    .get(id)
                    .ok_or("Missing social training profile")?;
                source.potential = meta.potential;
                source.position = serde_json::from_value(
                    serde_json::to_value(meta.position).map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
                source.natural_position = serde_json::from_value(
                    serde_json::to_value(meta.natural_position)
                        .map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
                source.training_focus = serde_json::from_value(
                    serde_json::to_value(meta.individual_focus)
                        .map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
            }
            if let Some(availability) = &self.management.availability {
                let state = availability.get(id).ok_or("Missing social availability")?;
                source.injury = state.injury.as_ref().map(|injury| domain::player::Injury {
                    name: injury.name.clone(),
                    days_remaining: injury.days_remaining,
                });
                source.stats.yellow_cards = state.yellow_cards;
                source.stats.red_cards = state.red_cards;
            }
        }
        Ok(projected)
    }

    /// Dispatcher seam: revoked actors and cached request IDs are checked before
    /// projection. Management still owns receipts, Ready/deadline checks and FIFO.
    pub(crate) fn dispatch_social(
        &mut self,
        actor: &str,
        request: Request,
        now_ms: u64,
    ) -> Result<Receipt, Error> {
        if !self.management.managers.contains_key(actor) {
            return Err(Error::Unauthorized);
        }
        if !matches!(&request.command, Command::Social(_)) {
            return Err(Error::InvalidRequest);
        }
        if self
            .management
            .receipts
            .contains_key(&(actor.to_owned(), request.id.clone()))
        {
            return self.management.dispatch(actor, request, now_ms);
        }
        let mut staged = self.clone();
        if matches!(
            &request.command,
            Command::Social(SocialCommand::Respond { .. })
        ) && staged.management.social.is_some()
        {
            staged.management.social.as_mut().unwrap().source_players =
                self.project_source_players().map_err(Error::Social)?;
        }
        let receipt = staged.management.dispatch(actor, request, now_ms)?;
        if receipt.result.is_ok() {
            staged.register_personnel_signings()?;
            staged.sync_personnel_recovery();
        }
        *self = staged;
        Ok(receipt)
    }

    pub fn social_view(&self, actor: &str) -> Result<SocialView, Error> {
        self.management.social_view(actor)
    }

    pub fn inbox_view(
        &self,
        actor: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<InboxMessage>, Error> {
        self.management.inbox_view(actor, offset, limit)
    }

    /// Host-only event delivery; actor tools cannot choose a recipient or invent a
    /// message. Replacement managers never inherit a departed manager's inbox.
    pub fn deliver_message(
        &mut self,
        recipient: &str,
        message: InboxMessage,
    ) -> Result<bool, Error> {
        if !self.management.managers.contains_key(recipient) {
            return Err(Error::Unauthorized);
        }
        self.management
            .social
            .as_mut()
            .ok_or(Error::Unavailable)?
            .inbox
            .deliver(recipient, message)
            .map_err(inbox_error)
    }
}

impl Management {
    pub(crate) fn sync_social_players(&mut self, players: Vec<SourcePlayer>) -> Result<(), String> {
        let social = self.social.as_mut().ok_or("Social unavailable")?;
        for player in players {
            let old = &social.source_players[&player.id];
            if old.morale != player.morale || old.morale_core != player.morale_core {
                let revision = self.player_revisions[&player.id]
                    .checked_add(1)
                    .ok_or("Social revision overflow")?;
                self.player_revisions.insert(player.id.clone(), revision);
            }
            let contract = self
                .career
                .as_mut()
                .unwrap()
                .contracts
                .get_mut(&player.id)
                .ok_or("Social contract unavailable")?;
            contract.morale = player.morale;
            contract.manager_trust = player.morale_core.manager_trust;
            contract.unresolved_issue = player.morale_core.unresolved_issue.is_some();
            contract.recent_poor_treatment = player.morale_core.recent_treatment.is_some();
            social.source_players.insert(player.id.clone(), player);
        }
        Ok(())
    }
    pub fn social_view(&self, actor: &str) -> Result<SocialView, Error> {
        let club = &self.managers.get(actor).ok_or(Error::Unauthorized)?.club_id;
        let social = self.social.as_ref().ok_or(Error::Unavailable)?;
        let career = self.career.as_ref().ok_or(Error::Unavailable)?;
        let players = self
            .players
            .values()
            .filter(|player| &player.club_id == club)
            .map(|player| {
                let mut core = social.source_players[&player.id].morale_core.clone();
                core.manager_trust = career.contracts[&player.id].manager_trust;
                (player.id.clone(), core)
            })
            .collect();
        Ok(SocialView { players })
    }

    pub fn inbox_view(
        &self,
        actor: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<InboxMessage>, Error> {
        self.managers.get(actor).ok_or(Error::Unauthorized)?;
        if limit == 0 || limit > 100 {
            return Err(Error::InvalidRequest);
        }
        Ok(self
            .social
            .as_ref()
            .ok_or(Error::Unavailable)?
            .inbox
            .list(actor)
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect())
    }

    /// Core execute calls this on a cloned Management and commits only on success.
    pub(crate) fn execute_social(
        &mut self,
        actor: &str,
        command: &SocialCommand,
    ) -> Result<SocialOutcome, Error> {
        let club = self
            .managers
            .get(actor)
            .ok_or(Error::Unauthorized)?
            .club_id
            .clone();
        if matches!(command, SocialCommand::Respond { .. }) && self.window.is_ready(actor) {
            return Err(Error::AlreadyReady);
        }
        if let SocialCommand::Respond {
            message_id,
            action_id,
            option_id,
        } = command
        {
            if let Some(result) =
                self.respond_economy_sponsor(actor, message_id, action_id, option_id.as_deref())?
            {
                return Ok(SocialOutcome::Responded(result));
            }
        }
        let today = self.career_date().ok_or(Error::Unavailable)?;
        if let SocialCommand::Respond {
            message_id,
            action_id,
            option_id,
        } = command
        {
            if action_id.starts_with("prospect:") {
                let value = self.respond_youth(
                    actor,
                    &club,
                    message_id,
                    action_id,
                    option_id.as_deref().ok_or(Error::InvalidRequest)?,
                    today,
                )?;
                return Ok(SocialOutcome::Responded(
                    serde_json::from_value(value).map_err(|e| Error::Social(e.to_string()))?,
                ));
            }
        }
        let social = self.social.as_mut().ok_or(Error::Unavailable)?;
        match command {
            SocialCommand::MarkRead { message_id } => {
                social
                    .inbox
                    .mark_read(actor, message_id)
                    .map_err(inbox_error)?;
                Ok(SocialOutcome::MarkedRead)
            }
            SocialCommand::MarkAllRead => {
                social.inbox.mark_all_read(actor);
                Ok(SocialOutcome::MarkedRead)
            }
            SocialCommand::Delete { message_id } => {
                social
                    .inbox
                    .delete(actor, message_id)
                    .map_err(inbox_error)?;
                Ok(SocialOutcome::Deleted)
            }
            SocialCommand::ClearOld => Ok(SocialOutcome::Cleared {
                count: social.inbox.clear_old(actor, today),
            }),
            SocialCommand::Respond {
                message_id,
                action_id,
                option_id,
            } => {
                let before = social.source_players.clone();
                let mut players: Vec<_> = before.values().cloned().collect();
                let mut rng =
                    StdRng::seed_from_u64(social.seed ^ self.sequence ^ 0x736f_6369_616c_7273);
                let resolution = social
                    .inbox
                    .resolve_with(
                        actor,
                        message_id,
                        action_id,
                        option_id.as_deref(),
                        |message, action, option| {
                            conversations::apply_player_response(
                                &mut players,
                                &club,
                                message,
                                &action.id,
                                option,
                                today,
                                &mut rng,
                            )
                        },
                    )
                    .map_err(inbox_error)?;
                for player in players {
                    let previous = &before[&player.id];
                    if previous.morale == player.morale
                        && previous.morale_core == player.morale_core
                    {
                        continue;
                    }
                    if self.players[&player.id].club_id != club {
                        return Err(Error::Unauthorized);
                    }
                    let revision = self.player_revisions[&player.id]
                        .checked_add(1)
                        .ok_or(Error::Overflow)?;
                    let contract = self
                        .career
                        .as_mut()
                        .unwrap()
                        .contracts
                        .get_mut(&player.id)
                        .ok_or(Error::Unavailable)?;
                    contract.morale = player.morale;
                    contract.manager_trust = player.morale_core.manager_trust;
                    contract.unresolved_issue = player.morale_core.unresolved_issue.is_some();
                    contract.recent_poor_treatment = player.morale_core.recent_treatment.is_some();
                    if previous.morale_core.renewal_state != player.morale_core.renewal_state {
                        if player
                            .morale_core
                            .renewal_state
                            .as_ref()
                            .is_some_and(|state| {
                                matches!(
                                    state.exit_intent,
                                    Some(domain::player::ContractExitIntent::LetExpire { .. })
                                )
                            })
                        {
                            if contract.end_date.is_none() {
                                return Err(Error::Unavailable);
                            }
                            contract.let_expire = true;
                        }
                    }
                    let source = social.source_players.get_mut(&player.id).unwrap();
                    source.morale = player.morale;
                    source.morale_core = player.morale_core;
                    self.player_revisions.insert(player.id, revision);
                }
                Ok(SocialOutcome::Responded(resolution))
            }
        }
    }

    /// Source manager-decision renewal block is distinct from the player's own
    /// negotiation cooldown. Core career review/confirm must call this for renewals.
    pub(crate) fn validate_social_renewal(
        &self,
        actor: &str,
        player_id: &str,
    ) -> Result<(), Error> {
        let club = &self.managers.get(actor).ok_or(Error::Unauthorized)?.club_id;
        if self.contract_owner(player_id) != Some(club.as_str()) {
            return Err(Error::Unavailable);
        }
        let Some(social) = &self.social else {
            return Ok(());
        };
        let Some(state) = social.source_players[player_id]
            .morale_core
            .renewal_state
            .as_ref()
        else {
            return Ok(());
        };
        if state.status != RenewalSessionStatus::Blocked {
            return Ok(());
        }
        let today = self.career_date().ok_or(Error::Unavailable)?;
        if state
            .manager_blocked_until
            .as_deref()
            .and_then(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
            .is_none_or(|blocked| blocked >= today)
        {
            return Err(Error::Social(
                "Manager renewal decision is still blocked".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn validate_social_checkpoint(&self) -> Result<(), String> {
        let Some(social) = &self.social else {
            return Ok(());
        };
        if self.career.is_none() || social.source_players.keys().ne(self.players.keys()) {
            return Err("Social source player coverage mismatch".into());
        }
        for (id, player) in &social.source_players {
            if id != &player.id
                || player.morale > 100
                || player.morale_core.manager_trust > 100
                || player
                    .morale_core
                    .unresolved_issue
                    .as_ref()
                    .is_some_and(|issue| issue.severity > 100)
            {
                return Err("Invalid retained source social profile".into());
            }
        }
        social
            .inbox
            .validate()
            .map_err(|error| format!("Invalid social inbox: {error:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_day_generates_private_contract_concern_once_and_survives_checkpoint() {
        let mut game = game();
        game.management
            .career
            .as_mut()
            .unwrap()
            .contracts
            .get_mut("a-p")
            .unwrap()
            .end_date = Some(NaiveDate::from_ymd_opt(2026, 6, 30).unwrap());
        game.advance_closed_day(1, 100, 200).unwrap();
        let inbox = game.inbox_view("a", 0, 20).unwrap();
        let concern = inbox
            .iter()
            .find(|m| m.id == "contract_concern_a-p_final")
            .unwrap();
        assert_eq!(concern.date, "2026-06-01");
        assert_eq!(game.career_view("a").unwrap().contracts["a-p"].morale, 41);
        assert!(
            game.inbox_view("b", 0, 20)
                .unwrap()
                .iter()
                .all(|m| m.id != concern.id)
        );
        let saved = game.save_state().unwrap();
        let mut restored = Football::load_validated(saved.clone()).unwrap();
        assert!(restored.advance_closed_day(1, 100, 200).is_err());
        assert_eq!(restored.save_state().unwrap(), saved);
    }
    use crate::career::CareerSetup;
    use crate::contracts::PlayerContract;
    use crate::football::BoardProfile;
    use crate::{Club, Manager, Outcome, Player};
    use domain::message::{ActionOption, ActionType, MessageAction, MessageContext};

    fn game() -> Football {
        let mut sources = BTreeMap::new();
        let mut attributes = vec![];
        for club in ["a", "b"] {
            let id = format!("{club}-p");
            let attrs = serde_json::json!({"pace":50,"stamina":50,"strength":50,"passing":50,"shooting":50,"tackling":50,"dribbling":50,"defending":50,"positioning":50,"vision":50,"decisions":50,"composure":50,"leadership":50,"aggression":50});
            let mut source = SourcePlayer::new(
                id.clone(),
                id.clone(),
                format!("Full name {id}"),
                "2000-01-01".into(),
                "ENG".into(),
                domain::player::Position::Midfielder,
                serde_json::from_value(attrs.clone()).unwrap(),
            );
            source.team_id = Some(club.into());
            source.morale = 50;
            let mut engine = attrs;
            for (key, value) in [
                ("id", serde_json::json!(id)),
                ("name", serde_json::json!(format!("Registry {id}"))),
                ("position", serde_json::json!("Midfielder")),
                ("condition", serde_json::json!(70)),
                ("fitness", serde_json::json!(80)),
            ] {
                engine.as_object_mut().unwrap().insert(key.into(), value);
            }
            attributes.push(serde_json::from_value::<engine::PlayerData>(engine).unwrap());
            sources.insert(id, source);
        }
        let management = Management::new(
            ["a", "b"]
                .map(|id| Club {
                    id: id.into(),
                    name: id.into(),
                    balance: 100_000,
                })
                .to_vec(),
            attributes
                .iter()
                .map(|p| Player {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    club_id: p.id[..1].into(),
                })
                .collect(),
            ["a", "b"]
                .map(|id| Manager {
                    id: id.into(),
                    club_id: id.into(),
                })
                .to_vec(),
            1,
            100,
        )
        .unwrap();
        let mut game = Football::new(management, attributes, vec![]).unwrap();
        game.configure_career(CareerSetup {
            today: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            contracts: ["a-p", "b-p"]
                .map(|id| {
                    (
                        id.into(),
                        PlayerContract::new(
                            NaiveDate::from_ymd_opt(2000, 1, 1).unwrap(),
                            5200,
                            Some(NaiveDate::from_ymd_opt(2028, 6, 1).unwrap()),
                            100_000,
                            50,
                            50,
                        ),
                    )
                })
                .into(),
            wage_budgets: [("a".into(), 100_000), ("b".into(), 100_000)].into(),
            reputations: [("a".into(), 500), ("b".into(), 500)].into(),
            staff_annual_wages: BTreeMap::new(),
        })
        .unwrap();
        game.configure_boards(
            [
                (
                    "a".into(),
                    BoardProfile {
                        reputation: 500,
                        initial_satisfaction: 10,
                    },
                ),
                (
                    "b".into(),
                    BoardProfile {
                        reputation: 500,
                        initial_satisfaction: 80,
                    },
                ),
            ]
            .into(),
        )
        .unwrap();
        game.configure_social(sources, 1001).unwrap();
        game
    }

    fn message(family: &str, option: &str) -> InboxMessage {
        InboxMessage::new(
            format!("{family}_a-p"),
            String::new(),
            String::new(),
            String::new(),
            "2026-06-01".into(),
        )
        .with_context(MessageContext {
            player_id: Some("a-p".into()),
            team_id: Some("b".into()),
            ..MessageContext::default()
        })
        .with_action(MessageAction {
            id: "respond".into(),
            label: String::new(),
            label_key: None,
            resolved: false,
            action_type: ActionType::ChooseOption {
                options: vec![ActionOption {
                    id: option.into(),
                    label: option.into(),
                    description: String::new(),
                    label_key: None,
                    description_key: None,
                }],
            },
        })
    }

    fn response(id: &str, message_id: &str, option: &str) -> Request {
        Request {
            id: id.into(),
            day: 1,
            command: Command::Social(SocialCommand::Respond {
                message_id: message_id.into(),
                action_id: "respond".into(),
                option_id: Some(option.into()),
            }),
        }
    }

    #[test]
    fn response_updates_authoritative_morale_and_history_and_replays_without_reroll() {
        let mut game = game();
        let msg = message("bench_complaint", "promise_chance");
        let msg_id = msg.id.clone();
        game.deliver_message("a", msg).unwrap();
        assert!(game.inbox_view("b", 0, 20).unwrap().is_empty());
        assert_eq!(
            game.dispatch("b", response("foreign", &msg_id, "promise_chance"), 1)
                .unwrap()
                .result,
            Err(Error::Unavailable)
        );
        let request = response("respond", &msg_id, "promise_chance");
        let before_squad = serde_json::to_value(game.squad("a").unwrap()).unwrap();
        let receipt = game.dispatch("a", request.clone(), 1).unwrap();
        assert!(matches!(
            receipt.result,
            Ok(Outcome::Social(SocialOutcome::Responded(_)))
        ));
        let contract = &game.career_view("a").unwrap().contracts["a-p"];
        assert!((58..=64).contains(&contract.morale));
        assert_eq!(contract.manager_trust, 56);
        assert!(contract.recent_poor_treatment);
        let core = &game.social_view("a").unwrap().players["a-p"];
        assert_eq!(core.manager_trust, 56);
        assert_eq!(core.pending_promise.as_ref().unwrap().matches_remaining, 1);
        assert!(game.social_view("b").unwrap().players.get("a-p").is_none());
        assert_eq!(
            serde_json::to_value(game.squad("a").unwrap()).unwrap(),
            before_squad
        );
        let projected = game.project_source_players().unwrap();
        assert_eq!(projected["a-p"].full_name, "Full name a-p");
        assert_eq!(projected["a-p"].match_name, "Registry a-p");
        assert_eq!(projected["a-p"].condition, 70);
        assert_eq!(projected["a-p"].wage, 5200);
        let checkpoint = game.save_state().unwrap();
        let mut restored = Football::load_validated(checkpoint.clone()).unwrap();
        assert_eq!(restored.dispatch("a", request, 1).unwrap(), receipt);
        assert_eq!(restored.save_state().unwrap(), checkpoint);
        let repeat = restored
            .dispatch(
                "a",
                response("fresh-request-same-action", &msg_id, "promise_chance"),
                1,
            )
            .unwrap();
        assert_eq!(repeat.result, receipt.result);
        assert_eq!(
            restored.career_view("a").unwrap().contracts["a-p"].morale,
            contract.morale
        );
    }

    #[test]
    fn ready_restricts_response_but_keeps_private_retrieval_and_read_marker() {
        let mut game = game();
        let msg = message("happy_player", "praise_back");
        let msg_id = msg.id.clone();
        game.deliver_message("a", msg).unwrap();
        game.dispatch(
            "a",
            Request {
                id: "ready".into(),
                day: 1,
                command: Command::Ready,
            },
            1,
        )
        .unwrap();
        assert_eq!(
            game.dispatch("a", response("late", &msg_id, "praise_back"), 1)
                .unwrap()
                .result,
            Err(Error::AlreadyReady)
        );
        assert!(!game.inbox_view("a", 0, 20).unwrap()[0].actions[0].resolved);
        assert!(
            game.dispatch(
                "a",
                Request {
                    id: "read".into(),
                    day: 1,
                    command: Command::Social(SocialCommand::MarkRead { message_id: msg_id })
                },
                1
            )
            .unwrap()
            .result
            .is_ok()
        );
        assert!(game.inbox_view("a", 0, 20).unwrap()[0].read);
        assert!(game.inbox_view("a", 0, 101).is_err());
        assert!(matches!(
            game.social_view("outsider"),
            Err(Error::Unauthorized)
        ));
    }

    #[test]
    fn failed_writeback_is_atomic_and_dismissal_blocks_old_inbox_and_receipt() {
        let mut game = game();
        let msg = message("bench_complaint", "promise_chance");
        let msg_id = msg.id.clone();
        game.deliver_message("a", msg).unwrap();
        game.management
            .player_revisions
            .insert("a-p".into(), u64::MAX);
        let before = game.career_view("a").unwrap().contracts["a-p"].clone();
        let request = response("overflow", &msg_id, "promise_chance");
        assert_eq!(
            game.dispatch("a", request.clone(), 1).unwrap().result,
            Err(Error::Overflow)
        );
        assert_eq!(game.career_view("a").unwrap().contracts["a-p"], before);
        assert!(
            game.social_view("a").unwrap().players["a-p"]
                .pending_promise
                .is_none()
        );
        assert!(!game.inbox_view("a", 0, 20).unwrap()[0].actions[0].resolved);
        game.management.player_revisions.insert("a-p".into(), 0);
        game.advance_closed_day(1, 100, 200).unwrap();
        game.advance_closed_day(2, 200, 300).unwrap();
        let replacement = game.dismissals()[0].replacement_manager_id.clone();
        assert_eq!(game.dispatch("a", request, 201), Err(Error::Unauthorized));
        assert!(matches!(
            game.inbox_view("a", 0, 20),
            Err(Error::Unauthorized)
        ));
        assert!(matches!(game.social_view("a"), Err(Error::Unauthorized)));
        assert!(game.inbox_view(&replacement, 0, 20).unwrap().is_empty());
        let restored = Football::load_validated(game.save_state().unwrap()).unwrap();
        assert!(matches!(
            restored.inbox_view("a", 0, 20),
            Err(Error::Unauthorized)
        ));
    }

    #[test]
    fn no_renewal_keeps_manager_block_distinct_from_player_negotiation_block() {
        let mut game = game();
        let msg = message("contract_concern", "no_renewal");
        let msg_id = msg.id.clone();
        game.deliver_message("a", msg).unwrap();
        assert!(
            game.dispatch("a", response("no-renewal", &msg_id, "no_renewal"), 1)
                .unwrap()
                .result
                .is_ok()
        );
        let contract = &game.career_view("a").unwrap().contracts["a-p"];
        assert!(contract.let_expire);
        assert_eq!(contract.blocked_until, None);
        let core = &game.social_view("a").unwrap().players["a-p"];
        assert_eq!(
            core.renewal_state
                .as_ref()
                .unwrap()
                .manager_blocked_until
                .as_deref(),
            Some("2026-07-31")
        );
        assert!(game.management.validate_social_renewal("a", "a-p").is_err());
    }
}
