use chrono::NaiveDate;
use engine::{PlayerData, PlayerRole, Position};
use management::availability::{Availability, Injury};
use management::career::CareerSetup;
use management::contracts::PlayerContract;
use management::football::{Fixture, Football};
use management::training::{
    ClubTraining, Coach, Focus, Group, Intensity, PlayerTraining, Schedule,
};
use management::training_commands::{TrainingCommand, TrainingSetup};
use management::{Club, Command, Error, Management, Manager, Outcome, Player, Request};
use std::collections::BTreeMap;

fn attributes(club: &str, index: usize) -> PlayerData {
    PlayerData {
        id: format!("{club}-{index:02}"),
        name: format!("Player {club} {index}"),
        position: match index {
            0 | 12 => Position::Goalkeeper,
            1..=4 => Position::Defender,
            5..=8 | 11 => Position::Midfielder,
            _ => Position::Forward,
        },
        ovr: 50,
        condition: 60,
        fitness: 70,
        pace: 50,
        stamina: 50,
        strength: 50,
        agility: 50,
        passing: 50,
        shooting: 50,
        tackling: 50,
        dribbling: 50,
        defending: 50,
        positioning: 50,
        vision: 50,
        decisions: 50,
        composure: 50,
        aggression: 50,
        teamwork: 50,
        leadership: 50,
        handling: 50,
        reflexes: 50,
        aerial: 50,
        traits: vec![],
        role: PlayerRole::Standard,
    }
}

fn game(match_day: Option<u32>, injured_starter: bool) -> Football {
    let attributes: Vec<_> = ["a", "b"]
        .into_iter()
        .flat_map(|club| (0..13).map(move |i| attributes(club, i)))
        .collect();
    let mut management = Management::new(
        ["a", "b"]
            .into_iter()
            .map(|id| Club {
                id: id.into(),
                name: id.into(),
                balance: 1_000_000,
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
    management.require_match_rosters().unwrap();
    let fixtures = match_day
        .into_iter()
        .map(|day| Fixture {
            id: "fixture".into(),
            day,
            home: "a".into(),
            away: "b".into(),
            seed: 42,
        })
        .collect();
    let mut game = Football::new(management, attributes.clone(), fixtures).unwrap();
    game.configure_career(CareerSetup {
        today: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
        contracts: attributes
            .iter()
            .map(|p| {
                (
                    p.id.clone(),
                    PlayerContract::new(
                        NaiveDate::from_ymd_opt(2010, 1, 1).unwrap(),
                        5200,
                        Some(NaiveDate::from_ymd_opt(2028, 6, 1).unwrap()),
                        100_000,
                        70,
                        70,
                    ),
                )
            })
            .collect(),
        wage_budgets: BTreeMap::from([("a".into(), 1_000_000), ("b".into(), 1_000_000)]),
        reputations: BTreeMap::from([("a".into(), 500), ("b".into(), 500)]),
        staff_annual_wages: BTreeMap::new(),
    })
    .unwrap();
    game.configure_training(TrainingSetup {
        seed: 1001,
        clubs: ["a", "b"]
            .into_iter()
            .map(|id| {
                (
                    id.into(),
                    ClubTraining {
                        focus: if id == "a" {
                            Focus::Physical
                        } else {
                            Focus::Recovery
                        },
                        intensity: Intensity::High,
                        schedule: Schedule::Intense,
                        coaches: vec![Coach {
                            coaching: 100,
                            specialization: None,
                        }],
                        ..ClubTraining::default()
                    },
                )
            })
            .collect(),
        players: attributes
            .iter()
            .map(|p| {
                (
                    p.id.clone(),
                    PlayerTraining {
                        birth_year: 2010,
                        potential: 90,
                        individual_focus: None,
                        natural_position: match p.position {
                            Position::Goalkeeper => management::training::Position::Goalkeeper,
                            Position::Defender => management::training::Position::CenterBack,
                            Position::Midfielder => {
                                management::training::Position::CentralMidfielder
                            }
                            Position::Forward => management::training::Position::Striker,
                        },
                        position: match p.position {
                            Position::Goalkeeper => management::training::Position::Goalkeeper,
                            Position::Defender => management::training::Position::CenterBack,
                            Position::Midfielder => {
                                management::training::Position::CentralMidfielder
                            }
                            Position::Forward => management::training::Position::Striker,
                        },
                    },
                )
            })
            .collect(),
    })
    .unwrap();
    game.configure_availability(
        attributes
            .iter()
            .map(|p| {
                (
                    p.id.clone(),
                    Availability {
                        injury: (injured_starter && p.id == "a-00").then(|| Injury {
                            name: "existing-injury".into(),
                            days_remaining: 2,
                        }),
                        ..Availability::default()
                    },
                )
            })
            .collect(),
    )
    .unwrap();
    game
}

fn request(id: &str, day: u32, command: Command) -> Request {
    Request {
        id: id.into(),
        day,
        command,
    }
}

#[test]
fn closing_nonmatch_day_applies_growth_and_focus_specific_condition_once() {
    let mut game = game(None, false);
    let before = game.save_state().unwrap();
    assert!(game.advance_closed_day(1, 1, 200).is_err());
    assert_eq!(before, game.save_state().unwrap());
    assert!(game.advance_closed_day(1, 100, 200).unwrap().is_empty());
    assert!(game.results().is_empty());
    assert_eq!(game.career_date(), NaiveDate::from_ymd_opt(2026, 6, 2));
    let physical = game.squad("manager-a").unwrap();
    let recovery = game.squad("manager-b").unwrap();
    assert!(physical.iter().all(|p| p.condition < 60));
    assert!(recovery.iter().all(|p| p.condition > 60));
    assert!(physical.iter().any(|p| {
        [p.pace, p.stamina, p.strength, p.agility]
            .into_iter()
            .any(|value| value > 50)
    }));
    assert!(
        physical
            .iter()
            .all(|p| p.passing == 50 && p.shooting == 50 && p.tackling == 50)
    );
    assert!(
        recovery
            .iter()
            .all(|p| p.pace == 50 && p.stamina == 50 && p.strength == 50 && p.agility == 50)
    );
    let after = game.save_state().unwrap();
    assert!(game.advance_closed_day(1, 100, 200).is_err());
    assert_eq!(after, game.save_state().unwrap());
    let loaded = Football::load_validated(after.clone()).unwrap();
    assert_eq!(after, loaded.save_state().unwrap());
}

#[test]
fn injured_starter_is_excluded_and_countdown_survives_checkpoint_and_replay() {
    let mut game = game(Some(1), true);
    assert_eq!(game.availability_view("manager-a").unwrap().len(), 13);
    assert!(
        !game
            .availability_view("manager-b")
            .unwrap()
            .contains_key("a-00")
    );
    let lineup = (0..11).map(|i| format!("a-{i:02}")).collect();
    let receipt = game
        .dispatch(
            "manager-a",
            request(
                "injured-lineup",
                1,
                Command::SetLineup { player_ids: lineup },
            ),
            1,
        )
        .unwrap();
    assert_eq!(receipt.result, Err(Error::Unavailable));
    let results = game.advance_closed_day(1, 100, 200).unwrap();
    assert_eq!(results.len(), 1);
    assert!(!results[0].home_starting_xi.contains(&"a-00".into()));
    assert!(results[0].home_starting_xi.contains(&"a-12".into()));
    assert_eq!(
        game.availability_view("manager-a").unwrap()["a-00"]
            .injury
            .as_ref()
            .unwrap()
            .days_remaining,
        1
    );
    let checkpoint = game.save_state().unwrap();
    let mut restored = Football::load_validated(checkpoint.clone()).unwrap();
    assert!(restored.advance_closed_day(1, 100, 200).is_err());
    assert_eq!(checkpoint, restored.save_state().unwrap());
    game.advance_closed_day(2, 200, 300).unwrap();
    restored.advance_closed_day(2, 200, 300).unwrap();
    assert_eq!(game.save_state().unwrap(), restored.save_state().unwrap());
    assert!(restored.availability_view("manager-a").unwrap()["a-00"].is_available());
    // Injury recovery does not restore lost match fitness: the injured player
    // followed the injured training branch and lost one fitness point on day two.
    assert_eq!(
        restored
            .squad("manager-a")
            .unwrap()
            .iter()
            .find(|p| p.id == "a-00")
            .unwrap()
            .fitness,
        69
    );
}

#[test]
fn training_groups_and_individual_overrides_enforce_ownership_and_ready_boundary() {
    let mut game = game(None, false);
    let before = serde_json::to_value(game.training_view("manager-a").unwrap()).unwrap();
    let foreign_group = Group {
        id: "g".into(),
        name: "Group".into(),
        focus: Focus::Technical,
        player_ids: vec!["b-01".into()],
    };
    assert_eq!(
        game.dispatch(
            "manager-a",
            request(
                "foreign-group",
                1,
                Command::Training(TrainingCommand::SetGroups {
                    groups: vec![foreign_group]
                })
            ),
            1
        )
        .unwrap()
        .result,
        Err(Error::InvalidRequest)
    );
    assert_eq!(
        game.dispatch(
            "manager-a",
            request(
                "foreign-player",
                1,
                Command::Training(TrainingCommand::SetIndividualFocus {
                    player_id: "b-01".into(),
                    focus: Some(Focus::Recovery)
                })
            ),
            1
        )
        .unwrap()
        .result,
        Err(Error::Unavailable)
    );
    assert_eq!(
        before,
        serde_json::to_value(game.training_view("manager-a").unwrap()).unwrap()
    );
    let group = Group {
        id: "g".into(),
        name: "Passing".into(),
        focus: Focus::Technical,
        player_ids: vec!["a-01".into(), "a-02".into()],
    };
    assert_eq!(
        game.dispatch(
            "manager-a",
            request(
                "own-group",
                1,
                Command::Training(TrainingCommand::SetGroups {
                    groups: vec![group.clone()]
                })
            ),
            1
        )
        .unwrap()
        .result,
        Ok(Outcome::TrainingSet)
    );
    assert_eq!(
        game.dispatch(
            "manager-a",
            request(
                "own-player",
                1,
                Command::Training(TrainingCommand::SetIndividualFocus {
                    player_id: "a-01".into(),
                    focus: Some(Focus::Tactical)
                })
            ),
            1
        )
        .unwrap()
        .result,
        Ok(Outcome::TrainingSet)
    );
    game.dispatch("manager-a", request("ready", 1, Command::Ready), 1)
        .unwrap();
    let ready_view = serde_json::to_value(game.training_view("manager-a").unwrap()).unwrap();
    for (id, command) in [
        (
            "late-plan",
            TrainingCommand::SetPlan {
                focus: Focus::Recovery,
                intensity: Intensity::Low,
                schedule: Schedule::Light,
            },
        ),
        ("late-group", TrainingCommand::SetGroups { groups: vec![] }),
        (
            "late-individual",
            TrainingCommand::SetIndividualFocus {
                player_id: "a-01".into(),
                focus: None,
            },
        ),
    ] {
        assert_eq!(
            game.dispatch("manager-a", request(id, 1, Command::Training(command)), 1)
                .unwrap()
                .result,
            Err(Error::AlreadyReady)
        );
    }
    assert_eq!(
        ready_view,
        serde_json::to_value(game.training_view("manager-a").unwrap()).unwrap()
    );
    assert_eq!(
        game.training_view("manager-a").unwrap().plan.groups,
        vec![group]
    );
    assert_eq!(
        game.training_view("manager-a").unwrap().individual_focus["a-01"],
        Some(Focus::Tactical)
    );
    assert!(
        !game
            .training_view("manager-b")
            .unwrap()
            .individual_focus
            .contains_key("a-01")
    );
    assert!(matches!(
        game.training_view("outsider"),
        Err(Error::Unauthorized)
    ));
    let checkpoint = game.save_state().unwrap();
    let restored = Football::load_validated(checkpoint).unwrap();
    assert_eq!(
        ready_view,
        serde_json::to_value(restored.training_view("manager-a").unwrap()).unwrap()
    );
    assert!(restored.manager_view("manager-a").unwrap().ready);
}
