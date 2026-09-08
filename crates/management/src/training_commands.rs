//! Per-club training ownership and authenticated commands. Football owns applying
//! effects once per closing day; commands cannot advance training themselves.
use crate::training::{ClubTraining, Focus, Group, Intensity, PlayerTraining, Schedule};
use crate::{Error, Management};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingSetup {
    pub seed: u64,
    pub clubs: BTreeMap<String, ClubTraining>,
    pub players: BTreeMap<String, PlayerTraining>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum TrainingCommand {
    SetPlan {
        focus: Focus,
        intensity: Intensity,
        schedule: Schedule,
    },
    SetGroups {
        groups: Vec<Group>,
    },
    SetIndividualFocus {
        player_id: String,
        focus: Option<Focus>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingView {
    pub plan: ClubTraining,
    pub individual_focus: BTreeMap<String, Option<Focus>>,
}

impl Management {
    pub fn training_view(&self, actor: &str) -> Result<TrainingView, Error> {
        let club = &self.managers.get(actor).ok_or(Error::Unauthorized)?.club_id;
        let setup = self.training.as_ref().ok_or(Error::Unavailable)?;
        Ok(TrainingView {
            plan: setup.clubs[club].clone(),
            individual_focus: self
                .players
                .values()
                .filter(|player| &player.club_id == club)
                .map(|player| {
                    (
                        player.id.clone(),
                        setup.players[&player.id].individual_focus.clone(),
                    )
                })
                .collect(),
        })
    }

    pub(crate) fn execute_training(
        &mut self,
        actor: &str,
        command: &TrainingCommand,
    ) -> Result<(), Error> {
        let club = self
            .managers
            .get(actor)
            .ok_or(Error::Unauthorized)?
            .club_id
            .clone();
        if self.window.is_ready(actor) {
            return Err(Error::AlreadyReady);
        }
        let setup = self.training.as_ref().ok_or(Error::Unavailable)?;
        match command {
            TrainingCommand::SetGroups { groups } => {
                let mut ids = BTreeSet::new();
                let mut assigned = BTreeSet::new();
                if groups.iter().any(|group| {
                    group.id.trim().is_empty()
                        || group.name.trim().is_empty()
                        || !ids.insert(&group.id)
                        || group.player_ids.iter().any(|id| {
                            !assigned.insert(id)
                                || self
                                    .players
                                    .get(id)
                                    .is_none_or(|player| player.club_id != club)
                        })
                }) {
                    return Err(Error::InvalidRequest);
                }
            }
            TrainingCommand::SetIndividualFocus { player_id, .. } => {
                if self
                    .players
                    .get(player_id)
                    .is_none_or(|player| player.club_id != club)
                    || !setup.players.contains_key(player_id)
                {
                    return Err(Error::Unavailable);
                }
            }
            _ => {}
        }
        let setup = self.training.as_mut().unwrap();
        match command {
            TrainingCommand::SetPlan {
                focus,
                intensity,
                schedule,
            } => {
                let plan = setup.clubs.get_mut(&club).unwrap();
                plan.focus = focus.clone();
                plan.intensity = intensity.clone();
                plan.schedule = schedule.clone();
            }
            TrainingCommand::SetGroups { groups } => {
                setup.clubs.get_mut(&club).unwrap().groups = groups.clone()
            }
            TrainingCommand::SetIndividualFocus { player_id, focus } => {
                setup.players.get_mut(player_id).unwrap().individual_focus = focus.clone()
            }
        }
        Ok(())
    }

    pub(crate) fn remove_training_membership(&mut self, player: &str) {
        if let Some(setup) = &mut self.training {
            let owner = &self.players[player].club_id;
            for (club, plan) in &mut setup.clubs {
                if club != owner {
                    for group in &mut plan.groups {
                        group.player_ids.retain(|id| id != player);
                    }
                }
            }
        }
        let owner = &self.players[player].club_id;
        for (club, plan) in &mut self.squad_plans {
            if club == owner {
                continue;
            }
            plan.player_roles.remove(player);
            for slot in [
                &mut plan.match_roles.captain,
                &mut plan.match_roles.vice_captain,
                &mut plan.match_roles.penalty_taker,
                &mut plan.match_roles.free_kick_taker,
                &mut plan.match_roles.corner_taker,
            ] {
                if slot.as_deref() == Some(player) {
                    *slot = None;
                }
            }
        }
    }
}
