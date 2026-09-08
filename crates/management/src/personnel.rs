//! Authenticated staff, facilities and scouting runtime over pinned domain data.
//! Source 64677fee9047a1182005d666bafa5dbc025dca5c, GPL-3.0-or-later.
//! Personnel is optional; raw source records preserve data outside implemented
//! effects. Cash belongs to Management; finances belong to the economy adapter.
use crate::football::Football;
use crate::scouting::{MarketFilter, MarketView, ScoutingState, YouthObjective, YouthRegion};
use crate::{Error, Management, Receipt, Request};
use chrono::{Datelike, NaiveDate};
use domain::{
    player::{Player as SourcePlayer, Position},
    staff::{Staff, StaffRole},
    team::{FacilityType, Team},
};
use rand::{SeedableRng, rngs::StdRng};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelState {
    pub teams: BTreeMap<String, Team>,
    pub staff: BTreeMap<String, Staff>,
    pub scouting: ScoutingState,
    pub seed: u64,
    pub generated_count: u64,
    #[serde(default)]
    pub generated_staff_count: u64,
    previews: BTreeMap<u64, PersonnelPreview>,
    pub last_staff_market_activity: Option<NaiveDate>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersonnelPreview {
    actor: String,
    day: u32,
    value: Value,
    kind: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum PersonnelCommand {
    ReviewStaff {
        staff_id: String,
        action: crate::staff::Action,
    },
    ReviewFacility {
        facility: String,
    },
    Confirm {
        preview_id: u64,
    },
    ScoutPlayer {
        scout_id: String,
        player_id: String,
    },
    StartYouth {
        scout_id: String,
        region: YouthRegion,
        objective: YouthObjective,
        target_position: Option<String>,
    },
    CancelYouth {
        assignment_id: String,
    },
    ReassignYouth {
        assignment_id: String,
        scout_id: String,
    },
    YouthResponse {
        message_id: String,
        action_id: String,
        option_id: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelView {
    pub staff: Vec<Staff>,
    pub facilities: domain::team::Facilities,
    pub scouting: Option<crate::scouting::ScoutingDesk>,
}
fn err(s: impl ToString) -> Error {
    Error::Personnel(s.to_string())
}

#[cfg(test)]
pub(crate) mod tests {
    #[test]
    fn source_staff_market_rotates_only_at_thirty_days_and_replenishes_empty() {
        let mut game = game();
        let date = game.management.career_date().unwrap();
        game.management.advance_staff_market(date).unwrap();
        assert_eq!(game.staff_market("a").unwrap()[0].id, "coach");
        game.management
            .advance_staff_market(date + chrono::Days::new(29))
            .unwrap();
        assert_eq!(game.staff_market("a").unwrap()[0].id, "coach");
        let mut clone = game.clone();
        game.management
            .advance_staff_market(date + chrono::Days::new(30))
            .unwrap();
        clone
            .management
            .advance_staff_market(date + chrono::Days::new(30))
            .unwrap();
        assert_eq!(
            serde_json::to_value(game.staff_market("a").unwrap()).unwrap(),
            serde_json::to_value(clone.staff_market("a").unwrap()).unwrap()
        );
        let available = game.staff_market("a").unwrap();
        assert_eq!(available.len(), 12);
        assert!(
            game.personnel_view("a")
                .unwrap()
                .staff
                .iter()
                .any(|s| s.id == "as")
        );
        for s in available {
            assert_eq!(s.wage, 0);
            assert!(s.contract_end.is_none());
            assert!(s.specialization.is_none());
            assert!((30..80).contains(&s.attributes.coaching));
            assert!((25..75).contains(&s.attributes.physiotherapy));
            assert_ne!(s.last_name, "Unknown");
        }
        game.management
            .personnel
            .as_mut()
            .unwrap()
            .staff
            .retain(|_, s| s.team_id.is_some());
        game.management
            .advance_staff_market(date + chrono::Days::new(31))
            .unwrap();
        assert_eq!(game.staff_market("a").unwrap().len(), 12);
        assert_eq!(
            game.management
                .personnel
                .as_ref()
                .unwrap()
                .generated_staff_count,
            24
        );
    }
    use super::*;
    use crate::career::CareerSetup;
    use crate::contracts::PlayerContract;
    use crate::football::BoardProfile;
    use crate::{Club, Command, Manager, Outcome, Player};
    pub(crate) fn game() -> Football {
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
                        initial_satisfaction: 80,
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
        for club in game.management.clubs.values_mut() {
            club.balance = 10_000_000;
        }
        game.configure_training(crate::training_commands::TrainingSetup {
            seed: 2,
            clubs: ["a", "b"]
                .map(|id| (id.into(), crate::training::ClubTraining::default()))
                .into(),
            players: ["a-p", "b-p"]
                .map(|id| {
                    (
                        id.into(),
                        crate::training::PlayerTraining {
                            birth_year: 2000,
                            potential: 80,
                            natural_position: crate::training::Position::Midfielder,
                            position: crate::training::Position::Midfielder,
                            individual_focus: None,
                        },
                    )
                })
                .into(),
        })
        .unwrap();
        let teams = ["a", "b"]
            .map(|id| {
                (
                    id.into(),
                    Team::new(
                        id.into(),
                        id.into(),
                        id.into(),
                        "ENG".into(),
                        "City".into(),
                        "Stadium".into(),
                        10000,
                    ),
                )
            })
            .into();
        let mut staff = BTreeMap::new();
        for (id, club, role) in [
            ("as", Some("a"), StaffRole::Scout),
            ("bs", Some("b"), StaffRole::Scout),
            ("coach", None, StaffRole::Coach),
        ] {
            let mut s = Staff::new(
                id.into(),
                "A".into(),
                id.into(),
                "1980-01-01".into(),
                role,
                domain::staff::StaffAttributes {
                    coaching: 90,
                    judging_ability: 80,
                    judging_potential: 80,
                    physiotherapy: 80,
                },
            );
            s.team_id = club.map(str::to_owned);
            s.wage = 5200;
            staff.insert(id.into(), s);
        }
        game.configure_personnel(teams, staff, 17).unwrap();
        game
    }

    fn send(game: &mut Football, actor: &str, id: &str, command: PersonnelCommand) -> Receipt {
        game.dispatch(
            actor,
            Request {
                id: id.into(),
                day: game.management.window.day,
                command: Command::Personnel(command),
            },
            0,
        )
        .unwrap()
    }
    fn preview(receipt: Receipt) -> u64 {
        match receipt.result.unwrap() {
            Outcome::Personnel(value) => value["preview_id"].as_u64().unwrap(),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn staff_hiring_is_exclusive_scoped_and_changes_real_training_payroll() {
        let mut game = game();
        assert!(game.personnel_view("outsider").is_err());
        assert_eq!(game.personnel_view("a").unwrap().staff.len(), 1);
        assert_eq!(game.staff_market("b").unwrap().len(), 1);
        let a = preview(send(
            &mut game,
            "a",
            "review-a",
            PersonnelCommand::ReviewStaff {
                staff_id: "coach".into(),
                action: crate::staff::Action::Hire,
            },
        ));
        let b = preview(send(
            &mut game,
            "b",
            "review-b",
            PersonnelCommand::ReviewStaff {
                staff_id: "coach".into(),
                action: crate::staff::Action::Hire,
            },
        ));
        assert!(
            send(
                &mut game,
                "b",
                "foreign-confirm",
                PersonnelCommand::Confirm { preview_id: a }
            )
            .result
            .is_err()
        );
        let before = game.management.clubs["a"].balance;
        assert!(
            send(
                &mut game,
                "a",
                "confirm-a",
                PersonnelCommand::Confirm { preview_id: a }
            )
            .result
            .is_ok()
        );
        assert_eq!(game.management.clubs["a"].balance, before);
        assert_eq!(
            game.management.training.as_ref().unwrap().clubs["a"].coaches[0].coaching,
            90
        );
        assert_eq!(
            game.management.career.as_ref().unwrap().staff_annual_wages["a"],
            [5200, 5200]
        );
        assert!(
            send(
                &mut game,
                "b",
                "confirm-b",
                PersonnelCommand::Confirm { preview_id: b }
            )
            .result
            .is_err()
        );
        let release = preview(send(
            &mut game,
            "a",
            "release-review",
            PersonnelCommand::ReviewStaff {
                staff_id: "coach".into(),
                action: crate::staff::Action::Release,
            },
        ));
        send(
            &mut game,
            "a",
            "release-confirm",
            PersonnelCommand::Confirm {
                preview_id: release,
            },
        )
        .result
        .unwrap();
        assert!(
            game.management.training.as_ref().unwrap().clubs["a"]
                .coaches
                .is_empty()
        );
        assert_eq!(
            game.management.career.as_ref().unwrap().staff_annual_wages["a"],
            [5200]
        );
    }
    #[test]
    fn youth_search_completes_signs_into_live_registries_and_survives_checkpoint() {
        let mut game = game();
        send(
            &mut game,
            "a",
            "youth",
            PersonnelCommand::StartYouth {
                scout_id: "as".into(),
                region: YouthRegion::Domestic,
                objective: YouthObjective::Balanced,
                target_position: Some("Goalkeeper".into()),
            },
        )
        .result
        .unwrap();
        for day in 1..=4 {
            game.advance_closed_day(day, u64::from(day) * 100, u64::from(day + 1) * 100)
                .unwrap();
        }
        let message = game
            .inbox_view("a", 0, 30)
            .unwrap()
            .into_iter()
            .find(|m| m.id.starts_with("youth-scout-"))
            .unwrap();
        assert!(game.inbox_view("b", 0, 30).unwrap().is_empty());
        let action = message.actions[0].id.clone();
        let player_id = action.strip_prefix("prospect:").unwrap().to_string();
        let before = game.management.players.len();
        let command = PersonnelCommand::YouthResponse {
            message_id: message.id.clone(),
            action_id: action.clone(),
            option_id: "sign".into(),
        };
        assert!(
            send(&mut game, "b", "steal", command.clone())
                .result
                .is_err()
        );
        send(&mut game, "a", "sign", command.clone())
            .result
            .unwrap();
        send(&mut game, "a", "logical-retry", command)
            .result
            .unwrap();
        assert_eq!(game.management.players.len(), before + 1);
        assert_eq!(game.management.players[&player_id].club_id, "a");
        assert!(game.attributes.contains_key(&player_id));
        assert!(
            game.management
                .career
                .as_ref()
                .unwrap()
                .contracts
                .contains_key(&player_id)
        );
        assert!(
            game.management
                .training
                .as_ref()
                .unwrap()
                .players
                .contains_key(&player_id)
        );
        let loaded = Football::load_validated(game.save_state().unwrap()).unwrap();
        assert_eq!(loaded.management.players.len(), before + 1);
        let second = message.actions[1].id.clone();
        let shortlist = PersonnelCommand::YouthResponse {
            message_id: message.id.clone(),
            action_id: second,
            option_id: "shortlist".into(),
        };
        send(&mut game, "a", "shortlist", shortlist.clone())
            .result
            .unwrap();
        send(&mut game, "a", "shortlist-retry", shortlist)
            .result
            .unwrap();
        let short = game
            .inbox_view("a", 0, 30)
            .unwrap()
            .into_iter()
            .find(|m| m.id.starts_with("youth-shortlist-"))
            .unwrap();
        send(
            &mut game,
            "a",
            "discard",
            PersonnelCommand::YouthResponse {
                message_id: short.id,
                action_id: short.actions[0].id.clone(),
                option_id: "discard".into(),
            },
        )
        .result
        .unwrap();
        game.management
            .social
            .as_ref()
            .unwrap()
            .inbox
            .validate()
            .unwrap();
        Football::load_validated(game.save_state().unwrap()).unwrap();
    }
    #[test]
    fn facility_finance_and_approved_training_effect_are_integrated() {
        let mut game = game();
        let accounts = ["a", "b"]
            .map(|id| {
                (
                    id.into(),
                    crate::economy::FinanceState {
                        wage_budget: 100_000,
                        transfer_budget: 100_000,
                        season_income: 0,
                        season_expenses: 0,
                        sponsorship: None,
                        financial_ledger: vec![],
                        reputation: 500,
                        stadium_capacity: 10000,
                        form: vec![],
                    },
                )
            })
            .into();
        game.configure_economy(crate::economy_runtime::EconomySetup {
            seed: 1,
            clubs: accounts,
            season: 1,
            completed_home_dates: BTreeMap::new(),
        })
        .unwrap();
        let id = preview(send(
            &mut game,
            "a",
            "review",
            PersonnelCommand::ReviewFacility {
                facility: "Training".into(),
            },
        ));
        send(
            &mut game,
            "a",
            "confirm",
            PersonnelCommand::Confirm { preview_id: id },
        )
        .result
        .unwrap();
        assert_eq!(game.personnel_view("a").unwrap().facilities.training, 2);
        assert_eq!(
            game.management.training.as_ref().unwrap().clubs["a"].training_level,
            2
        );
        assert_eq!(game.management.clubs["a"].balance, 9_750_000);
        assert_eq!(
            game.management
                .economy_finance("a")
                .unwrap()
                .0
                .season_expenses,
            250_000
        );
    }
}
fn decode<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, Error> {
    serde_json::from_value(v).map_err(err)
}

impl Football {
    pub(crate) fn register_personnel_signings(&mut self) -> Result<(), Error> {
        let sources = self
            .management
            .social
            .as_ref()
            .ok_or(Error::Unavailable)?
            .source_players
            .clone();
        for (id, source) in sources {
            if self.attributes.contains_key(&id) {
                continue;
            }
            if !self.management.players.contains_key(&id) {
                self.management.register_source_youth(&source)?;
            }
            let position: crate::training::Position =
                decode(serde_json::to_value(&source.position).map_err(err)?)?;
            let mut value = serde_json::to_value(&source.attributes).map_err(err)?;
            let fields = value.as_object_mut().unwrap();
            fields.insert("id".into(), json!(id));
            fields.insert("name".into(), json!(source.match_name));
            fields.insert(
                "position".into(),
                serde_json::to_value(position.group()).map_err(err)?,
            );
            fields.insert("condition".into(), json!(source.condition));
            fields.insert("fitness".into(), json!(source.fitness));
            fields.insert("ovr".into(), json!(source.ovr));
            fields.insert(
                "traits".into(),
                serde_json::to_value(&source.traits).map_err(err)?,
            );
            self.attributes.insert(id.clone(), decode(value)?);
            if let Some(recovery) = &mut self.recovery {
                let birth =
                    NaiveDate::parse_from_str(&source.date_of_birth, "%Y-%m-%d").map_err(err)?;
                recovery.players.insert(
                    id,
                    crate::recovery::PlayerRecovery {
                        age: self
                            .management
                            .career_date()
                            .ok_or(Error::Unavailable)?
                            .year()
                            .saturating_sub(birth.year()) as u32,
                        morale: source.morale,
                    },
                );
            }
        }
        Ok(())
    }
    pub fn configure_personnel(
        &mut self,
        teams: BTreeMap<String, Team>,
        staff: BTreeMap<String, Staff>,
        seed: u64,
    ) -> Result<(), String> {
        if self.started
            || self.management.sequence != 0
            || self.management.personnel.is_some()
            || self.management.social.is_none()
            || self.management.career.is_none()
        {
            return Err(
                "Personnel requires career/social and can only be configured once before commands"
                    .into(),
            );
        }
        if teams.keys().ne(self.management.clubs.keys())
            || teams.iter().any(|(id, t)| id != &t.id)
            || staff.iter().any(|(id, s)| {
                id != &s.id
                    || crate::staff::validate(s).is_err()
                    || s.team_id.as_ref().is_some_and(|id| !teams.contains_key(id))
            })
        {
            return Err("Personnel identities do not match world".into());
        }
        let mut staged = self.clone();
        staged.management.personnel = Some(PersonnelState {
            teams,
            staff,
            scouting: ScoutingState::default(),
            seed,
            generated_count: 0,
            generated_staff_count: 0,
            previews: BTreeMap::new(),
            last_staff_market_activity: None,
        });
        staged
            .management
            .sync_personnel_staff()
            .map_err(|e| format!("{e:?}"))?;
        staged.sync_personnel_recovery();
        staged.management.validate_personnel_checkpoint()?;
        *self = staged;
        Ok(())
    }
    pub fn personnel_view(&self, actor: &str) -> Result<PersonnelView, Error> {
        self.management.personnel_view(actor)
    }
    pub fn staff_market(&self, actor: &str) -> Result<Vec<Staff>, Error> {
        self.management
            .managers
            .get(actor)
            .ok_or(Error::Unauthorized)?;
        Ok(self
            .management
            .personnel
            .as_ref()
            .ok_or(Error::Unavailable)?
            .staff
            .values()
            .filter(|s| s.team_id.is_none())
            .cloned()
            .collect())
    }
    pub fn browse_transfer_market(
        &self,
        actor: &str,
        filter: &MarketFilter,
    ) -> Result<MarketView, Error> {
        let club = &self
            .management
            .managers
            .get(actor)
            .ok_or(Error::Unauthorized)?
            .club_id;
        let p = self
            .management
            .personnel
            .as_ref()
            .ok_or(Error::Unavailable)?;
        let players = self
            .project_source_players()
            .map_err(err)?
            .into_values()
            .collect::<Vec<_>>();
        Ok(crate::scouting::browse_market(
            club,
            &players,
            &p.teams,
            self.management.career_date().ok_or(Error::Unavailable)?,
            filter,
        ))
    }
    pub(crate) fn dispatch_personnel(
        &mut self,
        actor: &str,
        request: Request,
        now_ms: u64,
    ) -> Result<Receipt, Error> {
        self.management
            .managers
            .get(actor)
            .ok_or(Error::Unauthorized)?;
        if self
            .management
            .receipts
            .contains_key(&(actor.to_owned(), request.id.clone()))
        {
            return self.management.dispatch(actor, request, now_ms);
        }
        let mut staged = self.clone();
        if staged.management.personnel.is_some() {
            staged
                .management
                .social
                .as_mut()
                .ok_or(Error::Unavailable)?
                .source_players = self.project_source_players().map_err(err)?;
        }
        let receipt = staged.management.dispatch(actor, request, now_ms)?;
        if receipt.result.is_ok() {
            staged.register_personnel_signings()?;
            staged.sync_personnel_recovery();
        }
        *self = staged;
        Ok(receipt)
    }
    pub(crate) fn advance_personnel(&mut self, today: NaiveDate) -> Result<(), String> {
        if self.management.personnel.is_none() {
            return Ok(());
        }
        let players = self.project_source_players()?;
        let managers = self.management.managers.clone();
        let state = self.management.personnel.as_mut().unwrap();
        let mut rng = StdRng::seed_from_u64(
            state.seed ^ today.num_days_from_ce() as u64 ^ 0x7363_6f75_7469_6e67,
        );
        state
            .scouting
            .advance_day(today, &state.staff, &players, &state.teams, &mut rng)?;
        for (actor, manager) in &managers {
            let pending = state
                .scouting
                .pending_youth_generation(actor)
                .into_iter()
                .cloned()
                .collect::<Vec<_>>();
            for assignment in pending {
                let Some(scout) = state.staff.get(&assignment.scout_id) else {
                    continue;
                };
                let count = if assignment.objective == YouthObjective::Balanced {
                    4
                } else {
                    6
                };
                let end = state
                    .generated_count
                    .checked_add(count)
                    .ok_or("Youth ID counter overflow")?;
                let ids = (state.generated_count..end)
                    .map(|n| format!("youth:{:016x}:{n}", state.seed))
                    .collect::<Vec<_>>();
                if ids.iter().any(|id| players.contains_key(id)) {
                    return Err("Generated youth ID collision".into());
                }
                let pool = crate::youth::generate_pool(
                    &state.teams[&manager.club_id],
                    today.year() as u32,
                    &assignment,
                    scout,
                    &ids,
                    &mut rng,
                )?;
                state.scouting.complete_youth(
                    actor,
                    &assignment.id,
                    pool,
                    &state.teams[&manager.club_id],
                    scout,
                    today,
                )?;
                state.generated_count = end;
            }
            if let Some(desk) = state.scouting.view(actor) {
                for message in &desk.messages {
                    self.management
                        .social
                        .as_mut()
                        .ok_or("Social unavailable")?
                        .inbox
                        .deliver(actor, message.clone())
                        .map_err(|e| format!("{e:?}"))?;
                }
            }
        }
        self.management.advance_staff_market(today)?;
        Ok(())
    }
    pub(crate) fn sync_personnel_recovery(&mut self) {
        let Some(p) = &self.management.personnel else {
            return;
        };
        if let Some(recovery) = &mut self.recovery {
            for (id, club) in &mut recovery.clubs {
                club.physiotherapy = p
                    .staff
                    .values()
                    .filter(|s| s.team_id.as_deref() == Some(id) && s.role == StaffRole::Physio)
                    .map(|s| s.attributes.physiotherapy)
                    .collect();
                club.medical_level = p.teams[id].facilities.medical;
            }
        }
    }
}

impl Management {
    fn advance_staff_market(&mut self, today: NaiveDate) -> Result<(), String> {
        let p = self.personnel.as_mut().ok_or("Personnel unavailable")?;
        let empty = !p.staff.values().any(|s| s.team_id.is_none());
        let rotate = p
            .last_staff_market_activity
            .is_some_and(|last| (today - last).num_days() >= 30);
        if empty || rotate {
            let end = p
                .generated_staff_count
                .checked_add(12)
                .ok_or("Staff ID counter overflow")?;
            let ids = (p.generated_staff_count..end)
                .map(|n| format!("staff:{:016x}:{n}", p.seed))
                .collect::<Vec<_>>();
            if ids.iter().any(|id| p.staff.contains_key(id)) {
                return Err("Generated staff ID collision".into());
            }
            let mut rng = StdRng::seed_from_u64(
                p.seed
                    ^ today.num_days_from_ce() as u64
                    ^ p.generated_staff_count
                    ^ 0x7374_6166_665f_6765,
            );
            let generated = crate::youth::generate_available_staff(
                &p.teams.values().cloned().collect::<Vec<_>>(),
                today.year() as u32,
                &ids,
                &mut rng,
            )?;
            p.staff.retain(|_, s| s.team_id.is_some());
            p.staff
                .extend(generated.into_iter().map(|s| (s.id.clone(), s)));
            p.generated_staff_count = end;
            p.last_staff_market_activity = Some(today);
        } else if p.last_staff_market_activity.is_none() {
            p.last_staff_market_activity = Some(today);
        }
        Ok(())
    }
    pub(crate) fn validate_personnel_checkpoint(&self) -> Result<(), String> {
        let Some(p) = &self.personnel else {
            return Ok(());
        };
        let today = self.career_date().ok_or("Personnel requires career")?;
        if self.social.is_none()
            || p.teams.keys().ne(self.clubs.keys())
            || p.teams.iter().any(|(id, t)| id != &t.id)
            || p.staff.iter().any(|(id, s)| {
                id != &s.id
                    || crate::staff::validate(s).is_err()
                    || s.team_id
                        .as_ref()
                        .is_some_and(|id| !self.clubs.contains_key(id))
            })
            || p.last_staff_market_activity.is_some_and(|d| d > today)
            || p.previews.values().any(|v| {
                v.actor.trim().is_empty()
                    || v.day > self.window.day
                    || !matches!(v.kind.as_str(), "staff" | "facility")
            })
        {
            return Err("Invalid personnel registries or previews".into());
        }
        p.scouting
            .validate_checkpoint(&self.clubs.keys().cloned().collect(), today)?;
        let mut expected = self.clone();
        expected
            .sync_personnel_staff()
            .map_err(|e| format!("{e:?}"))?;
        if expected.career.as_ref().unwrap().staff_annual_wages
            != self.career.as_ref().unwrap().staff_annual_wages
        {
            return Err("Personnel payroll mismatch".into());
        }
        if let (Some(actual), Some(expected)) = (&self.training, &expected.training) {
            if actual.clubs != expected.clubs {
                return Err("Personnel training staff/facilities mismatch".into());
            }
        }
        Ok(())
    }
    pub(crate) fn respond_youth(
        &mut self,
        actor: &str,
        club: &str,
        message_id: &str,
        action_id: &str,
        option_id: &str,
        _today: NaiveDate,
    ) -> Result<Value, Error> {
        use crate::inbox::{EffectReceipt, InboxError};
        use domain::message::*;
        let social = self.social.as_mut().ok_or(Error::Unavailable)?;
        let mut changed = None;
        let mut shortlisted = None;
        let mut signed = None;
        let resolution = social
            .inbox
            .resolve_with(
                actor,
                message_id,
                action_id,
                Some(option_id),
                |message, action, option| {
                    if message.context.team_id.as_deref() != Some(club) {
                        return Err(InboxError::Unavailable);
                    }
                    let id = action
                        .id
                        .strip_prefix("prospect:")
                        .ok_or(InboxError::Unavailable)?;
                    let mut next = message.clone();
                    let prospects = next
                        .context
                        .youth_prospects
                        .as_mut()
                        .ok_or(InboxError::Unavailable)?;
                    let index = prospects
                        .iter()
                        .position(|p| p.id == id)
                        .ok_or(InboxError::Unavailable)?;
                    let mut prospect = prospects[index].clone();
                    let name = prospect.full_name.clone();
                    match option {
                        "sign" => {
                            if self.players.contains_key(id) {
                                return Err(InboxError::Effect(
                                    "Prospect already registered".into(),
                                ));
                            }
                            prospect.team_id = Some(club.into());
                            prospect.squad_role = domain::player::SquadRole::Youth;
                            let occupied = social
                                .source_players
                                .values()
                                .filter(|p| p.team_id.as_deref() == Some(club))
                                .filter_map(|p| p.jersey_number)
                                .collect::<BTreeSet<_>>();
                            prospect.jersey_number = match prospect.jersey_number {
                                Some(n) if !occupied.contains(&n) => Some(n),
                                _ => (1..=99).find(|n| !occupied.contains(n)),
                            };
                            prospects[index] = prospect.clone();
                            next.context.player_id = Some(id.into());
                            next.actions
                                .iter_mut()
                                .find(|a| a.id == action.id)
                                .unwrap()
                                .resolved = true;
                            signed = Some(prospect);
                        }
                        "discard" => {
                            prospects.remove(index);
                            next.actions.retain(|a| a.id != action.id);
                        }
                        "shortlist" => {
                            let options = match &action.action_type {
                                ActionType::ChooseOption { options } => options
                                    .iter()
                                    .filter(|o| o.id != "shortlist")
                                    .cloned()
                                    .collect(),
                                _ => return Err(InboxError::Unavailable),
                            };
                            let mut context = message.context.clone();
                            context.player_id = None;
                            context.youth_prospects = Some(vec![prospect.clone()]);
                            let mut report = InboxMessage::new(
                                format!("youth-shortlist-{}", prospect.id),
                                String::new(),
                                String::new(),
                                message.sender.clone(),
                                message.date.clone(),
                            )
                            .with_category(MessageCategory::ScoutReport)
                            .with_sender_role("")
                            .with_action(MessageAction {
                                id: action.id.clone(),
                                label: prospect.full_name.clone(),
                                action_type: ActionType::ChooseOption { options },
                                resolved: false,
                                label_key: None,
                            })
                            .with_context(context)
                            .with_i18n(
                                "be.msg.youthRecruitmentShortlist.subject",
                                "be.msg.youthRecruitmentShortlist.body",
                                std::collections::HashMap::from([("player".into(), name.clone())]),
                            );
                            report.sender_role_key = Some("be.role.scout".into());
                            shortlisted = Some(report);
                            prospects.remove(index);
                            next.actions.retain(|a| a.id != action.id);
                        }
                        _ => return Err(InboxError::InvalidOption),
                    }
                    changed = Some(next);
                    Ok(EffectReceipt {
                        i18n_key: format!("be.msg.youthRecruitment.effect.{option}"),
                        i18n_params: BTreeMap::from([("player".into(), name)]),
                    })
                },
            )
            .map_err(|e| err(format!("{e:?}")))?;
        if let Some(message) = changed {
            social
                .inbox
                .replace_domain_message(actor, message)
                .map_err(|e| err(format!("{e:?}")))?;
        }
        if let Some(message) = shortlisted {
            social
                .inbox
                .deliver(actor, message)
                .map_err(|e| err(format!("{e:?}")))?;
        }
        if let Some(player) = signed {
            social
                .source_players
                .insert(player.id.clone(), player.clone());
            self.register_source_youth(&player)?;
        }
        serde_json::to_value(resolution).map_err(err)
    }
    pub(crate) fn register_source_youth(&mut self, source: &SourcePlayer) -> Result<(), Error> {
        let club = source.team_id.as_deref().unwrap_or("");
        if self.players.contains_key(&source.id)
            || (!club.is_empty() && !self.clubs.contains_key(club))
            || (club.is_empty() && (source.wage != 0 || source.contract_end.is_some()))
        {
            return Err(Error::Unavailable);
        }
        let birth = NaiveDate::parse_from_str(&source.date_of_birth, "%Y-%m-%d").map_err(err)?;
        let end = source
            .contract_end
            .as_ref()
            .map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").map_err(err))
            .transpose()?;
        self.career
            .as_mut()
            .ok_or(Error::Unavailable)?
            .contracts
            .insert(
                source.id.clone(),
                crate::contracts::PlayerContract::new(
                    birth,
                    source.wage,
                    end,
                    source.market_value,
                    source.morale,
                    source.morale_core.manager_trust,
                ),
            );
        let position: crate::training::Position =
            decode(serde_json::to_value(&source.position).map_err(err)?)?;
        let natural_position =
            decode(serde_json::to_value(&source.natural_position).map_err(err)?)?;
        if let Some(training) = &mut self.training {
            training.players.insert(
                source.id.clone(),
                crate::training::PlayerTraining {
                    birth_year: birth.year() as u32,
                    potential: source.potential,
                    position,
                    natural_position,
                    individual_focus: None,
                },
            );
        }
        if let Some(profiles) = &mut self.squad_profiles {
            profiles.insert(
                source.id.clone(),
                crate::squad_plan::PositionProfile {
                    position,
                    natural_position,
                    alternate_positions: decode(
                        serde_json::to_value(&source.alternate_positions).map_err(err)?,
                    )?,
                    footedness: decode(serde_json::to_value(&source.footedness).map_err(err)?)?,
                    weak_foot: source.weak_foot,
                },
            );
        }
        if let Some(availability) = &mut self.availability {
            availability.insert(
                source.id.clone(),
                crate::availability::Availability::default(),
            );
        }
        self.players.insert(
            source.id.clone(),
            crate::Player {
                id: source.id.clone(),
                name: source.match_name.clone(),
                club_id: club.to_string(),
            },
        );
        self.player_revisions.insert(source.id.clone(), 0);
        if !club.is_empty() {
            self.club_revisions.insert(
                club.to_string(),
                self.club_revisions[club]
                    .checked_add(1)
                    .ok_or(Error::InvalidRequest)?,
            );
        }
        Ok(())
    }
    fn staff_accounts(&self, club: &str) -> Result<crate::staff::Accounts, Error> {
        let expenses = if self.economy.is_some() {
            self.economy_finance(club).map_err(err)?.0.season_expenses
        } else {
            self.personnel.as_ref().ok_or(Error::Unavailable)?.teams[club].season_expenses
        };
        Ok(crate::staff::Accounts {
            balance: self.clubs[club].balance,
            season_expenses: expenses,
        })
    }
    fn set_personnel_expenses(&mut self, club: &str, expenses: i64) {
        if let Ok(account) = self.economy_account_mut(club) {
            account.season_expenses = expenses;
        }
        self.personnel
            .as_mut()
            .unwrap()
            .teams
            .get_mut(club)
            .unwrap()
            .season_expenses = expenses;
    }
    fn facility_finance(&self, club: &str) -> Result<crate::facilities::Finance, Error> {
        let (account, snapshot) = self.economy_finance(club).map_err(err)?;
        Ok(crate::facilities::Finance {
            balance: self.clubs[club].balance,
            season_expenses: account.season_expenses,
            wage_bill: snapshot.annual_wage_bill,
            wage_budget: account.wage_budget,
            projected_weekly_net: snapshot.projected_weekly_net,
        })
    }
    fn confirm_personnel(&mut self, actor: &str, club: &str, id: u64) -> Result<Value, Error> {
        let stored = self
            .personnel
            .as_ref()
            .unwrap()
            .previews
            .get(&id)
            .filter(|p| p.actor == actor && p.day == self.window.day)
            .cloned()
            .ok_or(Error::Unavailable)?;
        if stored.kind == "staff" {
            let preview: crate::staff::Preview = decode(stored.value)?;
            let mut accounts = self.staff_accounts(club)?;
            let staff = self
                .personnel
                .as_mut()
                .unwrap()
                .staff
                .get_mut(&preview.staff_id)
                .ok_or(Error::Unavailable)?;
            match crate::staff::confirm(staff, club, &mut accounts, &preview).map_err(err)? {
                crate::staff::Confirmation::RefreshRequired(next) => {
                    return self.store_personnel_preview(
                        actor,
                        "staff",
                        serde_json::to_value(next).map_err(err)?,
                    );
                }
                crate::staff::Confirmation::Applied => {}
            }
            self.set_personnel_expenses(club, accounts.season_expenses);
            if preview.action == crate::staff::Action::Hire {
                self.personnel.as_mut().unwrap().last_staff_market_activity = self.career_date();
            }
            self.sync_personnel_staff()?;
        } else {
            let preview: crate::facilities::UpgradePreview = decode(stored.value)?;
            let mut finance = self.facility_finance(club)?;
            let facilities = &mut self
                .personnel
                .as_mut()
                .unwrap()
                .teams
                .get_mut(club)
                .unwrap()
                .facilities;
            match crate::facilities::confirm_upgrade(facilities, &mut finance, &preview)
                .map_err(err)?
            {
                crate::facilities::UpgradeConfirmation::RefreshRequired(next) => {
                    return self.store_personnel_preview(
                        actor,
                        "facility",
                        serde_json::to_value(next).map_err(err)?,
                    );
                }
                crate::facilities::UpgradeConfirmation::Applied => {}
            }
            self.clubs.get_mut(club).unwrap().balance = finance.balance;
            self.set_personnel_expenses(club, finance.season_expenses);
            self.sync_personnel_staff()?;
        }
        self.personnel.as_mut().unwrap().previews.remove(&id);
        let revision = self.club_revisions[club]
            .checked_add(1)
            .ok_or(Error::InvalidRequest)?;
        self.club_revisions.insert(club.into(), revision);
        Ok(json!({"applied":true}))
    }
    pub fn personnel_view(&self, actor: &str) -> Result<PersonnelView, Error> {
        let club = &self.managers.get(actor).ok_or(Error::Unauthorized)?.club_id;
        let p = self.personnel.as_ref().ok_or(Error::Unavailable)?;
        let scouting = p.scouting.view(actor).cloned().map(|mut desk| {
            desk.messages.clear();
            desk
        });
        Ok(PersonnelView {
            staff: p
                .staff
                .values()
                .filter(|s| s.team_id.as_deref() == Some(club))
                .cloned()
                .collect(),
            facilities: p.teams[club].facilities.clone(),
            scouting,
        })
    }
    pub(crate) fn sync_personnel_staff(&mut self) -> Result<(), Error> {
        let p = self.personnel.as_ref().ok_or(Error::Unavailable)?;
        for club in self.clubs.keys() {
            let staff = p
                .staff
                .values()
                .filter(|s| s.team_id.as_deref() == Some(club))
                .collect::<Vec<_>>();
            self.career
                .as_mut()
                .ok_or(Error::Unavailable)?
                .staff_annual_wages
                .insert(club.clone(), staff.iter().map(|s| s.wage).collect());
            if let Some(training) = &mut self.training {
                let plan = training.clubs.get_mut(club).ok_or(Error::Unavailable)?;
                plan.coaches = staff
                    .iter()
                    .filter(|s| matches!(s.role, StaffRole::Coach | StaffRole::AssistantManager))
                    .map(|s| {
                        Ok(crate::training::Coach {
                            coaching: s.attributes.coaching,
                            specialization: decode(
                                serde_json::to_value(&s.specialization).map_err(err)?,
                            )?,
                        })
                    })
                    .collect::<Result<_, Error>>()?;
                plan.physiotherapy = staff
                    .iter()
                    .filter(|s| s.role == StaffRole::Physio)
                    .map(|s| s.attributes.physiotherapy)
                    .collect();
                plan.medical_level = p.teams[club].facilities.medical;
                plan.training_level = p.teams[club].facilities.training;
            }
        }
        Ok(())
    }
    pub(crate) fn execute_personnel(
        &mut self,
        actor: &str,
        command: &PersonnelCommand,
    ) -> Result<Value, Error> {
        let club = self
            .managers
            .get(actor)
            .ok_or(Error::Unauthorized)?
            .club_id
            .clone();
        if self.window.is_ready(actor) {
            return Err(Error::AlreadyReady);
        }
        self.personnel.as_ref().ok_or(Error::Unavailable)?;
        let today = self.career_date().ok_or(Error::Unavailable)?;
        match command {
            PersonnelCommand::ScoutPlayer {
                scout_id,
                player_id,
            } => {
                let p = self.personnel.as_mut().unwrap();
                let scout = p.staff.get(scout_id).ok_or(Error::Unavailable)?;
                let player = self
                    .social
                    .as_ref()
                    .ok_or(Error::Unavailable)?
                    .source_players
                    .get(player_id)
                    .ok_or(Error::Unavailable)?;
                let id = format!("scout:{actor}:{}", self.sequence);
                p.scouting
                    .assign_player_at_facility(
                        actor,
                        &club,
                        &id,
                        scout,
                        player,
                        p.teams[&club].facilities.scouting,
                    )
                    .map_err(err)?;
                Ok(json!({"assignment_id":id}))
            }
            PersonnelCommand::StartYouth {
                scout_id,
                region,
                objective,
                target_position,
            } => {
                let position: Option<Position> = target_position
                    .as_ref()
                    .map(|s| decode(json!(s)))
                    .transpose()?;
                let p = self.personnel.as_mut().unwrap();
                let scout = p.staff.get(scout_id).ok_or(Error::Unavailable)?;
                let id = format!("youth-search:{actor}:{}", self.sequence);
                p.scouting
                    .start_youth_at_facility(
                        actor,
                        &club,
                        &id,
                        scout,
                        *region,
                        *objective,
                        position,
                        p.teams[&club].facilities.scouting,
                    )
                    .map_err(err)?;
                Ok(json!({"assignment_id":id}))
            }
            PersonnelCommand::CancelYouth { assignment_id } => {
                self.personnel
                    .as_mut()
                    .unwrap()
                    .scouting
                    .cancel_youth(actor, assignment_id)
                    .map_err(err)?;
                Ok(json!({"cancelled":true}))
            }
            PersonnelCommand::ReassignYouth {
                assignment_id,
                scout_id,
            } => {
                let p = self.personnel.as_mut().unwrap();
                let s = p.staff.get(scout_id).ok_or(Error::Unavailable)?;
                p.scouting
                    .reassign_youth(actor, assignment_id, s)
                    .map_err(err)?;
                Ok(json!({"reassigned":true}))
            }
            PersonnelCommand::ReviewStaff { staff_id, action } => {
                let accounts = self.staff_accounts(&club)?;
                let staff = self
                    .personnel
                    .as_ref()
                    .unwrap()
                    .staff
                    .get(staff_id)
                    .ok_or(Error::Unavailable)?;
                let preview =
                    crate::staff::review(staff, &club, &accounts, *action).map_err(err)?;
                self.store_personnel_preview(
                    actor,
                    "staff",
                    serde_json::to_value(preview).map_err(err)?,
                )
            }
            PersonnelCommand::ReviewFacility { facility } => {
                let facility: FacilityType = decode(json!(facility))?;
                let finance = self.facility_finance(&club)?;
                let preview = crate::facilities::review_upgrade(
                    &self.personnel.as_ref().unwrap().teams[&club].facilities,
                    facility,
                    &finance,
                )
                .map_err(err)?;
                self.store_personnel_preview(
                    actor,
                    "facility",
                    serde_json::to_value(preview).map_err(err)?,
                )
            }
            PersonnelCommand::Confirm { preview_id } => {
                self.confirm_personnel(actor, &club, *preview_id)
            }
            PersonnelCommand::YouthResponse {
                message_id,
                action_id,
                option_id,
            } => self.respond_youth(actor, &club, message_id, action_id, option_id, today),
        }
    }
    fn store_personnel_preview(
        &mut self,
        actor: &str,
        kind: &str,
        value: Value,
    ) -> Result<Value, Error> {
        let id = self.sequence;
        self.personnel.as_mut().unwrap().previews.insert(
            id,
            PersonnelPreview {
                actor: actor.into(),
                day: self.window.day,
                kind: kind.into(),
                value: value.clone(),
            },
        );
        Ok(json!({"preview_id":id,"preview":value}))
    }
}
