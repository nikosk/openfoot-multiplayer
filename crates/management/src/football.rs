//! Explicit-fixture bridge. No desktop state, special user match, or parallel save
//! worlds. Ownership is always resolved from the authoritative management registry.
use crate::recovery::PlayerRecovery;
use crate::{
    Club, Error, Management, ManagerView, PublicState, Receipt, Request,
    matches::{self, DelegatedTeam},
};
use engine::{PlayerData, TeamData};
use rand::SeedableRng;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fixture {
    pub id: String,
    pub day: u32,
    pub home: String,
    pub away: String,
    pub seed: u64,
}

#[cfg(test)]
mod board_integration_tests {
    use super::*;
    use crate::{Command, Manager, Player};

    fn game(with_match: bool, satisfaction: u8) -> Football {
        let attributes: Vec<PlayerData> = if with_match {
            ["a", "b"]
                .into_iter()
                .flat_map(|club| {
                    (0..11).map(move |i| {
                        serde_json::from_value(serde_json::json!({
                            "id": format!("{club}-{i}"), "name": format!("Player {club}-{i}"),
                        "position": match i { 0 => "Goalkeeper", 1..=4 => "Defender", 5..=8 => "Midfielder", _ => "Forward" },
                            "condition": 100, "fitness": 75, "pace": 65, "stamina": 65,
                            "strength": 65, "passing": 65, "shooting": 65, "tackling": 65,
                            "dribbling": 65, "defending": 65, "positioning": 65,
                            "vision": 65, "decisions": 65
                        }))
                        .unwrap()
                    })
                })
                .collect()
        } else {
            vec![]
        };
        let management = Management::new(
            ["a", "b"]
                .into_iter()
                .map(|id| Club {
                    id: id.into(),
                    name: id.into(),
                    balance: 1000,
                })
                .collect(),
            attributes
                .iter()
                .map(|p| Player {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    club_id: p.id[..1].into(),
                })
                .collect(),
            ["a", "b"]
                .into_iter()
                .map(|id| Manager {
                    id: format!("manager-{id}"),
                    club_id: id.into(),
                })
                .collect(),
            1,
            100,
        )
        .unwrap();
        let fixtures = if with_match {
            vec![Fixture {
                id: "match".into(),
                day: 1,
                home: "a".into(),
                away: "b".into(),
                seed: 45,
            }]
        } else {
            vec![]
        };
        let mut game = Football::new(management, attributes, fixtures).unwrap();
        game.configure_boards(BTreeMap::from([
            (
                "manager-a".into(),
                BoardProfile {
                    reputation: 800,
                    initial_satisfaction: satisfaction,
                },
            ),
            (
                "manager-b".into(),
                BoardProfile {
                    reputation: 800,
                    initial_satisfaction: 70,
                },
            ),
        ]))
        .unwrap();
        game
    }

    #[test]
    fn daily_warning_then_irreversible_dismissal_denies_old_reads_and_replays() {
        let mut game = game(false, 10);
        let request = Request {
            id: "ready".into(),
            day: 1,
            command: Command::Ready,
        };
        game.dispatch("manager-a", request.clone(), 1).unwrap();
        game.advance_closed_day(1, 100, 200).unwrap();
        assert_eq!(game.board_view("manager-a").unwrap().state.warning_stage, 1);
        assert_eq!(game.board_view("manager-a").unwrap().warnings.len(), 1);
        assert!(game.dismissals().is_empty());
        assert!(
            !serde_json::to_string(&game.public_state())
                .unwrap()
                .contains("warning")
        );
        game.advance_closed_day(2, 200, 300).unwrap();
        let dismissal = game.dismissals()[0].clone();
        assert_eq!(dismissal.manager_id, "manager-a");
        assert_eq!(dismissal.day, 2);
        assert_eq!(
            game.board_records().unwrap()["manager-a"].dismissed_day,
            Some(2)
        );
        assert_eq!(
            game.board_records().unwrap()["manager-a"]
                .state
                .satisfaction,
            10
        );
        assert_eq!(
            game.dispatch("manager-a", request, 201),
            Err(Error::Unauthorized)
        );
        assert_eq!(game.manager_view("manager-a"), Err(Error::Unauthorized));
        assert!(matches!(
            game.board_view("manager-a"),
            Err(Error::Unauthorized)
        ));
        assert!(matches!(game.squad("manager-a"), Err(Error::Unauthorized)));
        assert!(matches!(
            game.career_view("manager-a"),
            Err(Error::Unauthorized)
        ));
        assert_eq!(game.lineup("manager-a"), Err(Error::Unauthorized));
        assert!(matches!(
            game.match_plan("manager-a"),
            Err(Error::Unauthorized)
        ));
        let replacement = &dismissal.replacement_manager_id;
        assert_ne!(replacement, "manager-a");
        assert_eq!(game.manager_view(replacement).unwrap().club.id, "a");
        assert!(!game.manager_view(replacement).unwrap().ready);
        assert_eq!(game.board_view(replacement).unwrap().state.satisfaction, 50);
        let set_plan = Request {
            id: "plan".into(),
            day: 3,
            command: Command::SetMatchPlan {
                plan: crate::tactics::MatchPlan::default(),
            },
        };
        assert!(
            game.dispatch(replacement, set_plan, 201)
                .unwrap()
                .result
                .is_ok()
        );
        let former = game.board_records().unwrap()["manager-a"].clone();
        game.reset_board_objectives().unwrap();
        assert_eq!(game.board_records().unwrap()["manager-a"], former);
        assert_eq!(game.active_managers().len(), 2);
    }

    #[test]
    fn each_match_updates_both_boards_with_same_rule() {
        let mut game = game(true, 70);
        let results = game.advance_closed_day(1, 100, 200).unwrap();
        let report = &results[0].report;
        let mut expected_a = crate::board::BoardState::new(800, 2, 70).unwrap();
        let mut expected_b = expected_a.clone();
        expected_a.after_match(report.home_goals, report.away_goals);
        expected_b.after_match(report.away_goals, report.home_goals);
        assert_eq!(game.board_view("manager-a").unwrap().state, expected_a);
        assert_eq!(game.board_view("manager-b").unwrap().state, expected_b);
    }

    #[test]
    fn failed_day_publishes_no_warning_dismissal_or_board_mutation() {
        let mut game = game(false, 10);
        // An explicit invalid roster makes execution fail before any day is committed.
        game.fixtures.push(Fixture {
            id: "unplayable".into(),
            day: 1,
            home: "a".into(),
            away: "b".into(),
            seed: 1,
        });
        let before = game.board_records().cloned();
        assert!(game.advance_closed_day(1, 100, 200).is_err());
        assert_eq!(game.board_records().cloned(), before);
        assert!(game.dismissals().is_empty());
        assert_eq!(game.window().day, 1);
        assert!(!game.started);
    }

    #[test]
    fn season_evaluation_is_atomic_and_preserves_dismissed_manager_outcome() {
        let mut game = game(false, 10);
        game.advance_closed_day(1, 100, 200).unwrap();
        game.advance_closed_day(2, 200, 300).unwrap();
        let before = game.board_records().cloned();
        assert!(game.evaluate_season_boards(&BTreeMap::new()).is_err());
        assert_eq!(game.board_records().cloned(), before);
        let finances = ["a", "b"]
            .into_iter()
            .map(|id| {
                (
                    id.into(),
                    BoardFinance {
                        wage_usage_percent: 100,
                        in_debt: false,
                    },
                )
            })
            .collect();
        let outcomes = game.evaluate_season_boards(&finances).unwrap();
        assert_eq!(outcomes.len(), 2);
        assert!(!outcomes.contains_key("manager-a"));
        assert_eq!(
            game.board_records().unwrap()["manager-a"],
            before.unwrap()["manager-a"]
        );
    }

    #[test]
    fn monday_finance_pressure_and_wages_apply_once_not_again_on_tuesday_or_replay() {
        let mut game = game(false, 70);
        game.management.clubs.get_mut("a").unwrap().balance = -1;
        game.configure_career(crate::career::CareerSetup {
            today: chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            contracts: BTreeMap::new(),
            wage_budgets: BTreeMap::from([("a".into(), 10_000), ("b".into(), 10_000)]),
            reputations: BTreeMap::from([("a".into(), 800), ("b".into(), 800)]),
            staff_annual_wages: BTreeMap::from([("a".into(), vec![5200])]),
        })
        .unwrap();
        game.advance_closed_day(1, 100, 200).unwrap();
        assert_eq!(game.board_view("manager-a").unwrap().state.satisfaction, 66);
        assert_eq!(game.board_view("manager-b").unwrap().state.satisfaction, 70);
        assert_eq!(game.manager_view("manager-a").unwrap().club.balance, -101);
        let after_monday = game.save_state().unwrap();
        assert!(game.advance_closed_day(1, 100, 200).is_err());
        assert_eq!(after_monday, game.save_state().unwrap());
        game.advance_closed_day(2, 200, 300).unwrap();
        assert_eq!(game.board_view("manager-a").unwrap().state.satisfaction, 66);
        assert_eq!(game.manager_view("manager-a").unwrap().club.balance, -101);
        assert_eq!(game.career_view("manager-a").unwrap().ledger.len(), 1);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinishedFixture {
    pub fixture_id: String,
    pub day: u32,
    pub home: String,
    pub away: String,
    pub home_starting_xi: Vec<String>,
    pub away_starting_xi: Vec<String>,
    pub report: engine::MatchReport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Standing {
    pub club_id: String,
    pub played: u32,
    pub won: u32,
    pub drawn: u32,
    pub lost: u32,
    pub goals_for: u32,
    pub goals_against: u32,
    pub points: u32,
}

/// Explicit scenario inputs, not a client-editable physiology/finances backdoor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoverySetup {
    pub seed: u64,
    pub players: BTreeMap<String, crate::recovery::PlayerRecovery>,
    pub clubs: BTreeMap<String, crate::recovery::ClubRecovery>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryView {
    pub mode: crate::recovery::RecoveryMode,
    pub club: crate::recovery::ClubRecovery,
    pub players: BTreeMap<String, crate::recovery::PlayerRecovery>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardProfile {
    pub reputation: u32,
    pub initial_satisfaction: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardWarning {
    pub day: u32,
    pub decision: crate::board::Decision,
}

/// Private host record retained after dismissal; never part of the public snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardRecord {
    pub club_id: String,
    pub reputation: u32,
    pub state: crate::board::BoardState,
    pub warnings: Vec<BoardWarning>,
    pub dismissed_day: Option<u32>,
}

/// Public employment change. Private board satisfaction and warnings stay private.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dismissal {
    pub day: u32,
    pub manager_id: String,
    pub club_id: String,
    pub replacement_manager_id: String,
    pub standing: Standing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardFinance {
    pub wage_usage_percent: u32,
    pub in_debt: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Football {
    pub(crate) management: Management,
    pub(crate) attributes: BTreeMap<String, PlayerData>,
    pub(crate) fixtures: Vec<Fixture>,
    pub(crate) results: Vec<FinishedFixture>,
    pub(crate) standings: BTreeMap<String, Standing>,
    pub(crate) recovery: Option<RecoverySetup>,
    pub(crate) started: bool,
    pub(crate) seasons: Option<crate::seasons::SeasonState>,
    #[serde(default)]
    pub(crate) competitions: Option<crate::competitions::CompetitionState>,
    #[serde(default)]
    pub(crate) statistics: Option<domain::stats::StatsState>,
    #[serde(default)]
    pub(crate) national: Option<crate::national::NationalState>,
    pub(crate) boards: Option<BTreeMap<String, BoardRecord>>,
    pub(crate) dismissals: Vec<Dismissal>,
}

impl Football {
    pub fn new(
        management: Management,
        attributes: Vec<PlayerData>,
        fixtures: Vec<Fixture>,
    ) -> Result<Self, String> {
        let attributes_len = attributes.len();
        let attributes: BTreeMap<_, _> =
            attributes.into_iter().map(|p| (p.id.clone(), p)).collect();
        if attributes_len != attributes.len()
            || attributes.len() != management.players.len()
            || attributes
                .keys()
                .any(|id| !management.players.contains_key(id))
        {
            return Err("Player attributes must match the ownership registry exactly".into());
        }
        let mut ids = BTreeSet::new();
        let mut club_days = BTreeSet::new();
        for f in &fixtures {
            if f.id.is_empty()
                || !ids.insert(&f.id)
                || f.day < management.window.day
                || f.home == f.away
                || !management.clubs.contains_key(&f.home)
                || !management.clubs.contains_key(&f.away)
                || !club_days.insert((f.day, &f.home))
                || !club_days.insert((f.day, &f.away))
            {
                return Err("Invalid or conflicting fixture".into());
            }
        }
        let standings = management
            .clubs
            .keys()
            .map(|id| {
                (
                    id.clone(),
                    Standing {
                        club_id: id.clone(),
                        played: 0,
                        won: 0,
                        drawn: 0,
                        lost: 0,
                        goals_for: 0,
                        goals_against: 0,
                        points: 0,
                    },
                )
            })
            .collect();
        Ok(Self {
            management,
            attributes,
            fixtures,
            results: vec![],
            standings,
            recovery: None,
            started: false,
            seasons: None,
            competitions: None,
            statistics: None,
            national: None,
            boards: None,
            dismissals: vec![],
        })
    }

    /// Trusted host starts a fresh decision window after initialization/simulation
    /// work. It cannot extend a day in which any participant has submitted work.
    pub fn open_window(&mut self, day: u32, now_ms: u64, deadline_ms: u64) -> Result<(), String> {
        if self.management.window.day != day
            || deadline_ms <= now_ms
            || self.management.closed
            || self
                .management
                .receipts
                .values()
                .any(|(request, _)| request.day == day)
        {
            return Err("Only an untouched decision day can be opened".into());
        }
        self.management.window.deadline_ms = deadline_ms;
        Ok(())
    }

    pub fn dispatch(
        &mut self,
        actor: &str,
        request: Request,
        now_ms: u64,
    ) -> Result<Receipt, Error> {
        if matches!(&request.command, crate::Command::Social(_)) {
            return self.dispatch_social(actor, request, now_ms);
        }
        if matches!(&request.command, crate::Command::Personnel(_)) {
            return self.dispatch_personnel(actor, request, now_ms);
        }
        if matches!(&request.command, crate::Command::Market(_)) {
            return self.dispatch_market(actor, request, now_ms);
        }
        if matches!(&request.command, crate::Command::Economy(_)) {
            let mut staged = self.clone();
            let result = staged.management.dispatch(actor, request, now_ms);
            staged
                .apply_economy_board_penalties()
                .map_err(Error::Contract)?;
            *self = staged;
            return result;
        }
        self.management.dispatch(actor, request, now_ms)
    }

    pub fn configure_recovery(&mut self, setup: RecoverySetup) -> Result<(), String> {
        if self.started || self.recovery.is_some() || self.management.sequence != 0 {
            return Err(
                "Recovery can only be configured once before commands or day processing".into(),
            );
        }
        if setup.players.len() != self.attributes.len()
            || setup.clubs.len() != self.management.clubs.len()
            || setup
                .players
                .keys()
                .any(|id| !self.attributes.contains_key(id))
            || setup
                .clubs
                .keys()
                .any(|id| !self.management.clubs.contains_key(id))
        {
            return Err("Recovery profiles must match world identities exactly".into());
        }
        for (id, attributes) in &self.attributes {
            let club = &self.management.players[id].club_id;
            if club.is_empty() {
                continue;
            }
            let mut clone = attributes.clone();
            let mut rng = rand::rngs::StdRng::seed_from_u64(setup.seed);
            crate::recovery::recover(
                &mut clone,
                &setup.players[id],
                &setup.clubs[club],
                crate::recovery::RecoveryMode::Rest,
                &mut rng,
            )?;
        }
        for club in setup.clubs.values() {
            crate::recovery::validate_club(club)?;
        }
        self.recovery = Some(setup);
        self.management.recovery_enabled = true;
        Ok(())
    }

    pub fn configure_career(&mut self, setup: crate::career::CareerSetup) -> Result<(), String> {
        if self.started {
            return Err("Career must be configured before day processing".into());
        }
        self.management
            .configure_career(setup)
            .map_err(|error| format!("{error:?}"))
    }

    pub fn career_view(&self, actor: &str) -> Result<crate::career::CareerView, Error> {
        self.management.career_view(actor)
    }

    /// Free-agent engine attributes are available only to an active manager.
    pub fn free_agent_squad(&self, actor: &str) -> Result<Vec<PlayerData>, Error> {
        let view = self.management.career_view(actor)?;
        view.free_agents
            .iter()
            .map(|player| {
                let mut attributes = self
                    .attributes
                    .get(&player.id)
                    .cloned()
                    .ok_or(Error::Unavailable)?;
                attributes.name = player.name.clone();
                Ok(attributes)
            })
            .collect()
    }

    pub fn career_date(&self) -> Option<chrono::NaiveDate> {
        self.management.career_date()
    }

    pub fn configure_training(
        &mut self,
        setup: crate::training_commands::TrainingSetup,
    ) -> Result<(), String> {
        if self.started || self.management.sequence != 0 || self.management.training.is_some() {
            return Err(
                "Training must be configured once before commands or daily processing".into(),
            );
        }
        self.validate_training_setup(&setup)?;
        self.management.training = Some(setup);
        Ok(())
    }

    pub fn configure_match_plans(
        &mut self,
        plans: BTreeMap<String, crate::tactics::MatchPlan>,
    ) -> Result<(), String> {
        if self.started
            || self.management.sequence != 0
            || !self.management.match_plans.is_empty()
            || plans.keys().ne(self.management.clubs.keys())
        {
            return Err("Initial match plans must cover all clubs before commands".into());
        }
        self.management.match_plans = plans;
        Ok(())
    }

    pub fn configure_lineups(
        &mut self,
        lineups: BTreeMap<String, Vec<String>>,
    ) -> Result<(), String> {
        if self.started || self.management.sequence != 0 || !self.management.lineups.is_empty() {
            return Err("Initial lineups can only be configured before commands".into());
        }
        for (club, ids) in &lineups {
            if !self.management.clubs.contains_key(club)
                || ids.len() > 11
                || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
                || ids.iter().any(|id| {
                    self.management
                        .players
                        .get(id)
                        .is_none_or(|p| &p.club_id != club)
                })
            {
                return Err("Initial lineup contains duplicate, excess or unowned players".into());
            }
        }
        // A saved source XI may contain injured players. Selection repairs it;
        // importing must not silently heal them or discard the manager's order.
        self.management.lineups = lineups;
        Ok(())
    }

    pub fn configure_squads(
        &mut self,
        profiles: BTreeMap<String, crate::squad_plan::PositionProfile>,
        plans: BTreeMap<String, crate::squad_plan::SquadPlan>,
    ) -> Result<(), String> {
        if self.started
            || self.management.sequence != 0
            || self.management.squad_profiles.is_some()
            || profiles.keys().ne(self.management.players.keys())
            || plans.keys().ne(self.management.clubs.keys())
        {
            return Err("Squad metadata must cover players and clubs once at setup".into());
        }
        crate::squad_plan::validate_profiles(&profiles)?;
        for (club, plan) in &plans {
            let owned = profiles
                .iter()
                .filter(|(id, _)| self.management.players[*id].club_id == *club)
                .map(|(id, profile)| (id.clone(), profile.clone()))
                .collect();
            crate::squad_plan::validate_plan(
                plan,
                &owned,
                self.management
                    .lineups
                    .get(club)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
            )?;
        }
        self.management.squad_profiles = Some(profiles);
        self.management.squad_plans = plans;
        Ok(())
    }

    pub fn squad_plan(&self, actor: &str) -> Result<crate::squad_plan::SquadPlan, Error> {
        let club = self.manager_view(actor)?.club.id;
        self.management
            .squad_profiles
            .as_ref()
            .ok_or(Error::Unavailable)?;
        Ok(self
            .management
            .squad_plans
            .get(&club)
            .cloned()
            .unwrap_or_default())
    }

    pub fn position_profiles(
        &self,
        actor: &str,
    ) -> Result<BTreeMap<String, crate::squad_plan::PositionProfile>, Error> {
        let club = self.manager_view(actor)?.club.id;
        let profiles = self
            .management
            .squad_profiles
            .as_ref()
            .ok_or(Error::Unavailable)?;
        Ok(profiles
            .iter()
            .filter(|(id, _)| self.management.players[*id].club_id == club)
            .map(|(id, profile)| (id.clone(), profile.clone()))
            .collect())
    }

    pub(crate) fn validate_training_setup(
        &self,
        setup: &crate::training_commands::TrainingSetup,
    ) -> Result<(), String> {
        let date = self
            .career_date()
            .ok_or("Full training requires career dates")?;
        if setup.players.keys().ne(self.management.players.keys())
            || setup.clubs.keys().ne(self.management.clubs.keys())
        {
            return Err("Training metadata must cover every player and club".into());
        }
        for (club, plan) in &setup.clubs {
            let mut group_ids = BTreeSet::new();
            let mut members = BTreeSet::new();
            if plan.groups.iter().any(|group| {
                group.id.trim().is_empty()
                    || group.name.trim().is_empty()
                    || !group_ids.insert(&group.id)
                    || group.player_ids.iter().any(|id| {
                        !members.insert(id)
                            || self
                                .management
                                .players
                                .get(id)
                                .is_none_or(|player| &player.club_id != club)
                    })
            }) {
                return Err("Invalid training group ownership or duplicate membership".into());
            }
        }
        for (id, player) in &self.attributes {
            let club = &self.management.players[id].club_id;
            let plan = if club.is_empty() {
                setup.clubs.values().next().ok_or("No clubs")?
            } else {
                &setup.clubs[club]
            };
            let morale = self.management.career.as_ref().unwrap().contracts[id].morale;
            crate::training::validate(player, &setup.players[id], plan, date, morale)?;
        }
        Ok(())
    }

    pub fn configure_availability(
        &mut self,
        states: BTreeMap<String, crate::availability::Availability>,
    ) -> Result<(), String> {
        if self.started
            || self.management.sequence != 0
            || self.management.availability.is_some()
            || states.keys().ne(self.management.players.keys())
        {
            return Err("Availability must cover registered players once at setup".into());
        }
        for state in states.values() {
            state.validate()?;
        }
        self.management.availability = Some(states);
        Ok(())
    }

    pub fn training_view(
        &self,
        actor: &str,
    ) -> Result<crate::training_commands::TrainingView, Error> {
        self.management.training_view(actor)
    }

    pub fn availability_view(
        &self,
        actor: &str,
    ) -> Result<BTreeMap<String, crate::availability::Availability>, Error> {
        let club = self.manager_view(actor)?.club.id;
        let states = self
            .management
            .availability
            .as_ref()
            .ok_or(Error::Unavailable)?;
        Ok(self
            .management
            .players
            .values()
            .filter(|player| player.club_id == club || player.club_id.is_empty())
            .map(|player| (player.id.clone(), states[&player.id].clone()))
            .collect())
    }

    pub fn configure_boards(
        &mut self,
        profiles: BTreeMap<String, BoardProfile>,
    ) -> Result<(), String> {
        if self.started || self.boards.is_some() || self.management.sequence != 0 {
            return Err(
                "Boards can only be configured once before commands or day processing".into(),
            );
        }
        if profiles.len() != self.management.managers.len()
            || profiles
                .keys()
                .any(|id| !self.management.managers.contains_key(id))
        {
            return Err("Board profiles must match active managers exactly".into());
        }
        let league_size =
            u32::try_from(self.management.clubs.len()).map_err(|_| "Too many clubs")?;
        let records = profiles
            .into_iter()
            .map(|(id, profile)| {
                let league_size = self
                    .club_competition_standings(&self.management.managers[&id].club_id)
                    .map_or(league_size, |rows| rows.len() as u32);
                Ok((
                    id.clone(),
                    BoardRecord {
                        club_id: self.management.managers[&id].club_id.clone(),
                        reputation: profile.reputation,
                        state: crate::board::BoardState::new(
                            profile.reputation,
                            league_size,
                            profile.initial_satisfaction,
                        )?,
                        warnings: vec![],
                        dismissed_day: None,
                    },
                ))
            })
            .collect::<Result<_, String>>()?;
        self.boards = Some(records);
        Ok(())
    }

    /// Authenticated private view: a former manager cannot inspect any club board.
    pub fn board_view(&self, actor: &str) -> Result<BoardRecord, Error> {
        self.management.manager_view(actor)?;
        self.boards
            .as_ref()
            .and_then(|boards| boards.get(actor))
            .cloned()
            .ok_or(Error::Unavailable)
    }

    /// Trusted-host metadata for bot routing, not an actor-selectable manager tool.
    pub fn active_managers(&self) -> Vec<crate::Manager> {
        self.management.managers.values().cloned().collect()
    }

    pub fn dismissals(&self) -> &[Dismissal] {
        &self.dismissals
    }

    /// Host-only private outcome archive, including original dismissed managers.
    pub fn board_records(&self) -> Option<&BTreeMap<String, BoardRecord>> {
        self.boards.as_ref()
    }

    /// Caller supplies real financial snapshots and owns once-only season settlement.
    /// Inactive managers retain their dismissal outcomes and are not reevaluated.
    pub fn evaluate_season_boards(
        &mut self,
        finances: &BTreeMap<String, BoardFinance>,
    ) -> Result<BTreeMap<String, crate::board::SeasonOutcome>, String> {
        let Some(mut boards) = self.boards.clone() else {
            return Ok(BTreeMap::new());
        };
        let standings = self.standings();
        let mut outcomes = BTreeMap::new();
        for manager in self.management.managers.values() {
            let standings = self
                .club_competition_standings(&manager.club_id)
                .unwrap_or_else(|| standings.clone());
            let (position, standing) = standings
                .iter()
                .enumerate()
                .find(|(_, row)| row.club_id == manager.club_id)
                .ok_or("Manager club missing from standings")?;
            let finance = finances
                .get(&manager.club_id)
                .ok_or("Missing club board finances")?;
            let board = boards
                .get_mut(&manager.id)
                .ok_or("Missing active manager board")?;
            outcomes.insert(
                manager.id.clone(),
                board.state.evaluate_season(
                    position as u32 + 1,
                    standing.won,
                    standing.goals_for,
                    finance.wage_usage_percent,
                    finance.in_debt,
                )?,
            );
        }
        self.boards = Some(boards);
        Ok(outcomes)
    }

    pub fn update_board_reputations(
        &mut self,
        reputations: &BTreeMap<String, u32>,
    ) -> Result<(), String> {
        let Some(mut boards) = self.boards.clone() else {
            return Ok(());
        };
        for manager in self.management.managers.values() {
            let reputation = *reputations
                .get(&manager.club_id)
                .ok_or("Missing board club reputation")?;
            if reputation > 1000 {
                return Err("Invalid board reputation".into());
            }
            boards
                .get_mut(&manager.id)
                .ok_or("Missing active manager board")?
                .reputation = reputation;
        }
        self.boards = Some(boards);
        Ok(())
    }

    pub fn reset_board_objectives(&mut self) -> Result<(), String> {
        let Some(mut boards) = self.boards.clone() else {
            return Ok(());
        };
        let size = u32::try_from(self.management.clubs.len()).map_err(|_| "Too many clubs")?;
        for manager in self.management.managers.values() {
            let board = boards
                .get_mut(&manager.id)
                .ok_or("Missing active manager board")?;
            let size = self
                .club_competition_standings(&manager.club_id)
                .map_or(size, |rows| rows.len() as u32);
            board.state.reset_objectives(board.reputation, size)?;
        }
        self.boards = Some(boards);
        Ok(())
    }

    fn check_boards_on_day(
        &mut self,
        day: u32,
        standings: &BTreeMap<String, Standing>,
    ) -> Result<(), String> {
        let club_tables: BTreeMap<_, _> = self
            .management
            .managers
            .values()
            .filter_map(|manager| {
                self.club_competition_standings(&manager.club_id)
                    .map(|rows| (manager.club_id.clone(), rows))
            })
            .collect();
        let Some(boards) = self.boards.as_mut() else {
            return Ok(());
        };
        // Snapshot active identities so a replacement is not checked on its hire day.
        let managers: Vec<_> = self.management.managers.values().cloned().collect();
        for manager in managers {
            if self
                .management
                .team_history
                .as_ref()
                .is_some_and(|history| {
                    history
                        .vacancies
                        .values()
                        .any(|v| v.caretaker_actor == manager.id)
                })
            {
                continue;
            }
            let record = boards
                .get_mut(&manager.id)
                .ok_or("Missing active manager board")?;
            match record.state.evaluate_firing() {
                crate::board::Decision::None => {}
                decision @ (crate::board::Decision::Warning
                | crate::board::Decision::FinalWarning) => {
                    record.warnings.push(BoardWarning { day, decision });
                }
                crate::board::Decision::Fired => {
                    let reputation = record.reputation;
                    let league_size = record.state.league_size;
                    record.dismissed_day = Some(day);
                    let mut serial = self.dismissals.len();
                    let replacement_id = loop {
                        let candidate = format!("manager:bot:{}:{serial}", manager.club_id);
                        if !boards.contains_key(&candidate)
                            && !self.management.managers.contains_key(&candidate)
                            && !self.management.clubs.contains_key(&candidate)
                            && !self.management.players.contains_key(&candidate)
                        {
                            break candidate;
                        }
                        serial = serial.checked_add(1).ok_or("Replacement ID overflow")?;
                    };
                    self.management
                        .eliminate(&manager.id)
                        .map_err(|error| format!("{error:?}"))?;
                    self.management.managers.insert(
                        replacement_id.clone(),
                        crate::Manager {
                            id: replacement_id.clone(),
                            club_id: manager.club_id.clone(),
                        },
                    );
                    boards.insert(
                        replacement_id.clone(),
                        BoardRecord {
                            club_id: manager.club_id.clone(),
                            reputation,
                            // Source AI hiring baseline, shared by every replacement.
                            state: crate::board::BoardState::new(reputation, league_size, 50)?,
                            warnings: vec![],
                            dismissed_day: None,
                        },
                    );
                    self.dismissals.push(Dismissal {
                        day,
                        manager_id: manager.id,
                        club_id: manager.club_id.clone(),
                        replacement_manager_id: replacement_id,
                        standing: standings
                            .get(&manager.club_id)
                            .or_else(|| {
                                club_tables.get(&manager.club_id).and_then(|rows| {
                                    rows.iter().find(|row| row.club_id == manager.club_id)
                                })
                            })
                            .ok_or("Missing dismissed club standing")?
                            .clone(),
                    });
                }
            }
        }
        Ok(())
    }

    pub fn recovery_view(&self, actor: &str) -> Result<RecoveryView, Error> {
        let club = self.management.manager_view(actor)?.club;
        if self.management.training.is_some() {
            return Err(Error::Unavailable);
        }
        let setup = self.recovery.as_ref().ok_or(Error::Unavailable)?;
        Ok(RecoveryView {
            mode: self
                .management
                .recovery_modes
                .get(&club.id)
                .copied()
                .unwrap_or(crate::recovery::RecoveryMode::Rest),
            club: setup.clubs[&club.id].clone(),
            players: self
                .management
                .players
                .values()
                .filter(|p| p.club_id == club.id)
                .map(|p| (p.id.clone(), self.recovery_profile(&p.id, setup)))
                .collect(),
        })
    }

    fn recovery_profile(&self, id: &str, setup: &RecoverySetup) -> PlayerRecovery {
        if let Some(career) = &self.management.career {
            let contract = &career.contracts[id];
            PlayerRecovery {
                age: contract.age_on(career.today).max(0) as u32,
                morale: contract.morale,
            }
        } else {
            setup.players[id]
        }
    }

    pub fn manager_view(&self, actor: &str) -> Result<ManagerView, Error> {
        self.management.manager_view(actor)
    }

    pub fn lineup(&self, actor: &str) -> Result<Vec<String>, Error> {
        self.management.lineup(actor)
    }

    pub fn squad(&self, actor: &str) -> Result<Vec<PlayerData>, Error> {
        let club = self.management.manager_view(actor)?.club;
        Ok(self
            .management
            .players
            .values()
            .filter(|p| p.club_id == club.id)
            .map(|p| {
                let mut data = self.attributes[&p.id].clone();
                data.name = p.name.clone();
                data
            })
            .collect())
    }

    pub fn public_state(&self) -> PublicState {
        self.management.public_state()
    }

    /// Host metadata; clients should expose only day/deadline, not the ready set.
    pub fn window(&self) -> crate::window::DayWindow {
        self.management.window.clone()
    }

    /// Saved pre-match instructions, private to the authenticated club manager.
    /// Automatic in-match adjustments never write back to this plan.
    pub fn match_plan(&self, actor: &str) -> Result<crate::tactics::MatchPlan, Error> {
        let club = self.management.manager_view(actor)?.club;
        Ok(self
            .management
            .match_plans
            .get(&club.id)
            .cloned()
            .unwrap_or_default())
    }
    pub fn results(&self) -> &[FinishedFixture] {
        &self.results
    }
    pub fn fixtures(&self) -> &[Fixture] {
        &self.fixtures
    }
    pub fn standings(&self) -> Vec<Standing> {
        if let Some(rows) = self.source_primary_standings() {
            return rows;
        }
        let mut rows: Vec<_> = self.standings.values().cloned().collect();
        rows.sort_by(|a, b| {
            b.points
                .cmp(&a.points)
                .then_with(|| {
                    (i64::from(b.goals_for) - i64::from(b.goals_against))
                        .cmp(&(i64::from(a.goals_for) - i64::from(a.goals_against)))
                })
                .then_with(|| b.goals_for.cmp(&a.goals_for))
                .then_with(|| a.club_id.cmp(&b.club_id))
        });
        rows
    }

    fn team(&self, club: &Club) -> Result<DelegatedTeam, String> {
        let ids = self
            .management
            .lineups
            .get(&club.id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let data = |id: &String| {
            let mut player = self.attributes[id].clone();
            player.name = self.management.players[id].name.clone();
            player
        };
        let available: Vec<_> = self
            .management
            .players
            .values()
            .filter(|p| {
                p.club_id == club.id
                    && self
                        .management
                        .availability
                        .as_ref()
                        .is_none_or(|states| states[&p.id].is_available())
            })
            .map(|p| data(&p.id))
            .collect();
        let (players, bench, formation, match_roles) =
            if let Some(profiles) = &self.management.squad_profiles {
                let plan = self
                    .management
                    .squad_plans
                    .get(&club.id)
                    .cloned()
                    .unwrap_or_default();
                let selected = crate::squad_plan::select(&available, profiles, ids, &plan)?;
                (
                    selected.players,
                    selected.bench,
                    plan.formation,
                    Some(selected.match_roles),
                )
            } else {
                let (players, bench) = crate::selection::select(&available, ids)?;
                (players, bench, "4-4-2".into(), None)
            };
        let plan = self
            .management
            .match_plans
            .get(&club.id)
            .cloned()
            .unwrap_or_default();
        let actor = self
            .management
            .managers
            .values()
            .find(|manager| manager.club_id == club.id);
        let manager = actor.and_then(|actor| self.source_manager(&actor.id).ok());
        let profile = self
            .management
            .career
            .as_ref()
            .map_or_else(engine::ai::AiProfile::default, |career| {
                matches::source_profile(career.reputations[&club.id], manager.as_ref())
            });
        Ok(DelegatedTeam {
            match_roles,
            team: TeamData {
                id: club.id.clone(),
                name: club.name.clone(),
                formation,
                play_style: plan.play_style,
                tactics: plan.engine_tactics(),
                players,
            },
            bench,
            profile,
        })
    }

    /// Trusted host operation. Stages all results before publishing any of them.
    /// An old expected day cannot duplicate a completed matchday.
    pub fn advance_closed_day(
        &mut self,
        expected_day: u32,
        now_ms: u64,
        next_deadline_ms: u64,
    ) -> Result<Vec<FinishedFixture>, String> {
        let mut staged = self.clone();
        let results = staged.advance_closed_day_inner(expected_day, now_ms, next_deadline_ms)?;
        staged.apply_weekly_board_pressure()?;
        let mut closing_standings = staged.standings.clone();
        for club in staged.management.clubs.keys() {
            if let Some(row) = staged
                .club_competition_standings(club)
                .and_then(|rows| rows.into_iter().find(|row| &row.club_id == club))
            {
                closing_standings.insert(club.clone(), row);
            }
        }
        staged.settle_completed_season()?;
        staged.check_boards_on_day(expected_day, &closing_standings)?;
        if let Some(date) = staged.management.career_date().and_then(|d| d.pred_opt()) {
            staged.advance_team_history(date)?;
            staged.capture_news_settlement_events(date)?;
        }
        *self = staged;
        Ok(results)
    }

    fn apply_weekly_board_pressure(&mut self) -> Result<(), String> {
        if self.management.economy.is_some() {
            return self.apply_economy_board_penalties();
        }
        use chrono::Datelike;
        let Some(career) = &self.management.career else {
            return Ok(());
        };
        let closing_date = career.today.pred_opt().ok_or("Career date underflow")?;
        if closing_date.weekday() != chrono::Weekday::Mon || self.boards.is_none() {
            return Ok(());
        }
        let finances = self.management.season_board_finances()?;
        for manager in self.management.managers.values() {
            // Wage-only cashflow projection: sponsorship/gate income is not yet
            // part of this simulator's economy. Do not fabricate that revenue.
            let net = career
                .ledger
                .iter()
                .rev()
                .find(|entry| {
                    entry.date == closing_date
                        && entry.club_id == manager.club_id
                        && entry.reason == "weekly_wages"
                })
                .ok_or("Missing Monday wage posting")?
                .amount;
            let penalty = crate::finances::weekly_finance_penalty(
                self.management.clubs[&manager.club_id].balance,
                finances[&manager.club_id].wage_usage_percent,
                net,
            );
            let board = self
                .boards
                .as_mut()
                .unwrap()
                .get_mut(&manager.id)
                .ok_or("Missing active board")?;
            board.state.satisfaction = board.state.satisfaction.saturating_sub(penalty);
        }
        Ok(())
    }

    fn advance_closed_day_inner(
        &mut self,
        expected_day: u32,
        now_ms: u64,
        next_deadline_ms: u64,
    ) -> Result<Vec<FinishedFixture>, String> {
        if expected_day != self.management.window.day {
            return Err("Wrong day".into());
        }
        if next_deadline_ms <= now_ms || expected_day == u32::MAX {
            return Err("Invalid next window".into());
        }
        if !self.management.closed(now_ms) {
            return Err("Management window is still open".into());
        }
        if let Some(today) = self.management.career_date() {
            self.advance_market_before_matches(today)?;
        }
        self.started = true;
        let mut staged = vec![];
        let mut staged_attributes = self.attributes.clone();
        for fixture in self.fixtures.iter().filter(|f| f.day == expected_day) {
            let home = self.team(&self.management.clubs[&fixture.home])?;
            let away = self.team(&self.management.clubs[&fixture.away])?;
            let home_starting_xi = home.team.players.iter().map(|p| p.id.clone()).collect();
            let away_starting_xi = away.team.players.iter().map(|p| p.id.clone()).collect();
            let report = matches::play_competition(
                home,
                away,
                fixture.seed,
                self.competition_fixture_is_knockout(&fixture.id),
            )?;
            // Stable order and a separate named-purpose stream; match event RNG
            // consumption cannot accidentally decide post-match physical wear.
            let mut rng = rand::rngs::StdRng::seed_from_u64(fixture.seed ^ 0x7068_7973_6963_616c);
            let mut ids: Vec<_> = report.player_stats.keys().collect();
            ids.sort();
            for id in ids {
                let player = staged_attributes
                    .get_mut(id)
                    .ok_or("Unknown report player")?;
                crate::physical::apply_match_wear(
                    player,
                    report.player_stats[id].minutes_played,
                    &mut rng,
                );
                // Source competitions can schedule a club twice on one date.
                // Later fixtures select from the physical state left by the
                // earlier one; the outer staged Football keeps the day atomic.
                self.attributes.insert(id.clone(), player.clone());
                if let Some(states) = &mut self.management.availability {
                    states
                        .get_mut(id)
                        .ok_or("Missing player availability")?
                        .add_match_cards(
                            report.player_stats[id].yellow_cards,
                            report.player_stats[id].red_cards,
                        )?;
                }
            }
            staged.push(FinishedFixture {
                fixture_id: fixture.id.clone(),
                day: expected_day,
                home: fixture.home.clone(),
                away: fixture.away.clone(),
                report,
                home_starting_xi,
                away_starting_xi,
            });
        }
        if staged.is_empty() && self.management.training.is_some() {
            let date = self.career_date().ok_or("Training requires career date")?;
            let setup = self.management.training.as_mut().unwrap();
            let mut rng = rand::rngs::StdRng::seed_from_u64(
                setup.seed ^ u64::from(expected_day) ^ 0x7472_6169_6e69_6e67,
            );
            for (id, player) in &mut staged_attributes {
                let club = &self.management.players[id].club_id;
                if club.is_empty() {
                    continue;
                }
                let morale = self.management.career.as_ref().unwrap().contracts[id].morale;
                let injured = self
                    .management
                    .availability
                    .as_ref()
                    .is_some_and(|states| !states[id].is_available());
                crate::training::train(
                    player,
                    setup.players.get_mut(id).unwrap(),
                    &setup.clubs[club],
                    date,
                    morale,
                    injured,
                    &mut rng,
                )?;
            }
        } else if staged.is_empty() {
            if let Some(setup) = &self.recovery {
                let mut rng = rand::rngs::StdRng::seed_from_u64(
                    setup.seed ^ u64::from(expected_day) ^ 0x7265_636f_7665_7279,
                );
                for (id, player) in &mut staged_attributes {
                    let club = &self.management.players[id].club_id;
                    if club.is_empty() {
                        continue;
                    }
                    let mode = self
                        .management
                        .recovery_modes
                        .get(club)
                        .copied()
                        .unwrap_or(crate::recovery::RecoveryMode::Rest);
                    crate::recovery::recover(
                        player,
                        &self.recovery_profile(id, setup),
                        &setup.clubs[club],
                        mode,
                        &mut rng,
                    )?;
                }
            }
        }
        self.attributes = staged_attributes;
        let closing_date = self.management.career_date();
        for (index, result) in staged.iter().enumerate() {
            self.capture_statistics(result)?;
            self.apply_social_match(result, &staged[..index])?;
        }
        if let Some(today) = closing_date {
            self.advance_dormant_competitions(today)?;
        }
        self.prepare_economy_day(&staged)?;
        if let Some(today) = closing_date {
            self.deliver_social_training_warnings(today)?;
            self.advance_national_day(today)?;
            self.management
                .advance_career(today.succ_opt().ok_or("Career date overflow")?)
                .map_err(|error| format!("{error:?}"))?;
            self.advance_social_day(today)?;
        }
        let mut new_injuries = Vec::new();
        if let Some(states) = &mut self.management.availability {
            for state in states.values_mut() {
                state.progress_recovery();
            }
            if let (Some(training), Some(today)) = (&self.management.training, closing_date) {
                let date = today.to_string();
                let mut event_ids: BTreeSet<_> = self
                    .management
                    .injury_events
                    .iter()
                    .map(|(_, event)| event.id.clone())
                    .collect();
                let mut rng = rand::rngs::StdRng::seed_from_u64(
                    training.seed ^ u64::from(expected_day) ^ 0x696e_6a75_7279_6576,
                );
                for club in self.management.clubs.keys() {
                    let candidates: Vec<_> = self
                        .management
                        .players
                        .values()
                        .filter(|player| &player.club_id == club)
                        .map(|player| crate::availability::InjuryCandidate {
                            player_id: &player.id,
                            fitness: self.attributes[&player.id].fitness,
                            availability: &states[&player.id],
                        })
                        .collect();
                    // Today's fixtures have just resolved. Source random events
                    // check still-scheduled fixtures, not whether a match was played.
                    if let Some(event) = crate::availability::roll_training_ground_injury(
                        &candidates,
                        false,
                        &date,
                        &event_ids,
                        &mut rng,
                    )? {
                        states.get_mut(&event.player_id).unwrap().injury =
                            Some(event.injury.clone());
                        event_ids.insert(event.id.clone());
                        new_injuries.push((club.clone(), event.clone()));
                        self.management.injury_events.push((club.clone(), event));
                    }
                }
            }
        }
        if let Some(today) = closing_date {
            for (club, event) in new_injuries {
                self.deliver_social_injury(&club, &event, today)?;
            }
            self.advance_personnel(today)?;
            self.advance_market_registrations(today)?;
            self.advance_news(today)?;
        }
        self.management
            .next_day(expected_day, now_ms, next_deadline_ms)
            .map_err(|e| format!("{e:?}"))?;
        for result in &staged {
            let counts = self.primary_competition_contains_fixture(&result.fixture_id);
            self.complete_competition_fixture(result)?;
            for (id, gf, ga) in [
                (
                    &result.home,
                    result.report.home_goals,
                    result.report.away_goals,
                ),
                (
                    &result.away,
                    result.report.away_goals,
                    result.report.home_goals,
                ),
            ] {
                if let Some(boards) = self.boards.as_mut() {
                    if let Some(manager) = self
                        .management
                        .managers
                        .values()
                        .find(|manager| &manager.club_id == id)
                    {
                        boards
                            .get_mut(&manager.id)
                            .ok_or("Missing active manager board")?
                            .state
                            .after_match(gf, ga);
                    }
                }
                if !counts {
                    continue;
                }
                let row = self
                    .standings
                    .get_mut(id)
                    .ok_or("Primary standing unavailable")?;
                row.played += 1;
                row.goals_for += u32::from(gf);
                row.goals_against += u32::from(ga);
                if gf > ga {
                    row.won += 1;
                    row.points += 3;
                } else if gf == ga {
                    row.drawn += 1;
                    row.points += 1;
                } else {
                    row.lost += 1;
                }
            }
        }
        self.results.extend(staged.iter().cloned());
        self.refresh_competition_schedule()?;
        Ok(staged)
    }
}
