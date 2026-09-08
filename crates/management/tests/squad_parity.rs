use engine::{PlayerData, PlayerRole};
use management::football::{Fixture, Football};
use management::matches::{self, DelegatedTeam};
use management::squad_plan::{self, Footedness, PositionProfile, SquadPlan};
use management::training::Position;
use management::{Club, Command, Error, Management, Manager, Outcome, Player, Request};
use std::collections::BTreeMap;

fn data(id: &str, position: Position) -> PlayerData {
    PlayerData {
        id: id.into(),
        name: id.into(),
        position: position.group(),
        role: PlayerRole::Standard,
        traits: vec![],
        ovr: 65,
        condition: 100,
        fitness: 75,
        pace: 65,
        stamina: 65,
        strength: 65,
        agility: 65,
        passing: 65,
        shooting: 65,
        tackling: 65,
        dribbling: 65,
        defending: 65,
        positioning: 65,
        vision: 65,
        decisions: 65,
        composure: 65,
        aggression: 65,
        teamwork: 65,
        leadership: 65,
        handling: 65,
        reflexes: 65,
        aerial: 65,
    }
}
fn request(id: &str, day: u32, command: Command) -> Request {
    Request {
        id: id.into(),
        day,
        command,
    }
}
fn setup() -> Football {
    let slots = squad_plan::formation_slots("4-4-2").unwrap();
    let mut attributes = vec![];
    let mut profiles = BTreeMap::new();
    for club in ["a", "b"] {
        for i in 0..26 {
            let position = slots[i % 11];
            let id = format!("{club}-{i}");
            attributes.push(data(&id, position));
            profiles.insert(
                id,
                PositionProfile {
                    position,
                    natural_position: position,
                    alternate_positions: vec![],
                    footedness: Footedness::Both,
                    weak_foot: 5,
                },
            );
        }
    }
    let players = attributes
        .iter()
        .map(|p| Player {
            id: p.id.clone(),
            name: p.name.clone(),
            club_id: p.id[..1].into(),
        })
        .collect();
    let management = Management::new(
        ["a", "b"]
            .map(|id| Club {
                id: id.into(),
                name: id.into(),
                balance: 1_000_000,
            })
            .to_vec(),
        players,
        ["a", "b"]
            .map(|id| Manager {
                id: id.into(),
                club_id: id.into(),
            })
            .to_vec(),
        1,
        1000,
    )
    .unwrap();
    let mut game = Football::new(
        management,
        attributes,
        vec![
            Fixture {
                id: "fixture-1".into(),
                day: 1,
                home: "a".into(),
                away: "b".into(),
                seed: 1001,
            },
            Fixture {
                id: "fixture-2".into(),
                day: 2,
                home: "b".into(),
                away: "a".into(),
                seed: 1002,
            },
        ],
    )
    .unwrap();
    game.configure_squads(
        profiles,
        ["a", "b"]
            .map(|id| (id.into(), SquadPlan::default()))
            .into(),
    )
    .unwrap();
    for club in ["a", "b"] {
        game.dispatch(
            club,
            request(
                "xi",
                1,
                Command::SetLineup {
                    player_ids: (0..11).map(|i| format!("{club}-{i}")).collect(),
                },
            ),
            10,
        )
        .unwrap()
        .result
        .unwrap();
    }
    game
}

#[test]
fn manager_plans_are_private_authenticated_idempotent_and_persistent() {
    let mut game = setup();
    let mut plan = SquadPlan::default();
    plan.player_roles.insert("a-1".into(), PlayerRole::WingBack);
    plan.player_roles.insert("a-2".into(), PlayerRole::Stopper);
    plan.match_roles.captain = Some("a-2".into());
    plan.match_roles.vice_captain = Some("a-1".into());
    let public = serde_json::to_value(game.public_state()).unwrap();
    let opponent = game.manager_view("b").unwrap();
    let req = request("plan", 1, Command::SetSquadPlan { plan: plan.clone() });
    assert_eq!(
        game.dispatch("outsider", req.clone(), 20),
        Err(Error::Unauthorized)
    );
    assert_eq!(game.squad_plan("outsider"), Err(Error::Unauthorized));
    assert_eq!(game.position_profiles("outsider"), Err(Error::Unauthorized));
    let receipt = game.dispatch("a", req.clone(), 20).unwrap();
    assert_eq!(receipt.result, Ok(Outcome::SquadPlanSet));
    assert_eq!(game.dispatch("a", req.clone(), 21).unwrap(), receipt);
    assert_eq!(
        game.dispatch(
            "a",
            request(
                "plan",
                1,
                Command::SetSquadPlan {
                    plan: SquadPlan::default()
                }
            ),
            22
        ),
        Err(Error::RequestIdReused)
    );
    assert_eq!(game.squad_plan("a").unwrap(), plan);
    assert_eq!(game.squad_plan("b").unwrap(), SquadPlan::default());
    assert!(
        game.position_profiles("a")
            .unwrap()
            .keys()
            .all(|id| id.starts_with("a-"))
    );
    assert_eq!(serde_json::to_value(game.public_state()).unwrap(), public);
    assert_eq!(game.manager_view("b").unwrap(), opponent);
    for mutate in [0, 1] {
        let mut bad = plan.clone();
        if mutate == 0 {
            bad.player_roles.insert("b-9".into(), PlayerRole::Poacher);
        } else {
            bad.match_roles.captain = Some("b-9".into());
        }
        assert_eq!(
            game.dispatch(
                "a",
                request(
                    &format!("bad-{mutate}"),
                    1,
                    Command::SetSquadPlan { plan: bad }
                ),
                30
            )
            .unwrap()
            .result,
            Err(Error::InvalidRequest)
        );
    }
    let mut lineup = game.lineup("a").unwrap();
    lineup.swap(1, 9);
    game.dispatch(
        "a",
        request("move", 1, Command::SetLineup { player_ids: lineup }),
        40,
    )
    .unwrap()
    .result
    .unwrap();
    let reconciled = game.squad_plan("a").unwrap();
    assert!(!reconciled.player_roles.contains_key("a-1"));
    assert_eq!(
        reconciled.player_roles.get("a-2"),
        Some(&PlayerRole::Stopper)
    );
    game.dispatch("a", request("ready", 1, Command::Ready), 50)
        .unwrap()
        .result
        .unwrap();
    assert_eq!(
        game.dispatch(
            "a",
            request(
                "after-ready",
                1,
                Command::SetSquadPlan {
                    plan: SquadPlan::default()
                }
            ),
            51
        )
        .unwrap()
        .result,
        Err(Error::AlreadyReady)
    );
    assert_eq!(
        game.dispatch(
            "b",
            request(
                "late",
                1,
                Command::SetSquadPlan {
                    plan: SquadPlan::default()
                }
            ),
            1000
        )
        .unwrap()
        .result,
        Err(Error::DayClosed)
    );
    game.advance_closed_day(1, 1000, 2000).unwrap();
    assert_eq!(game.squad_plan("a").unwrap(), reconciled);
    let mut restored = Football::load_validated(game.save_state().unwrap()).unwrap();
    assert_eq!(restored.squad_plan("a").unwrap(), reconciled);
    assert_eq!(
        restored.position_profiles("a").unwrap(),
        game.position_profiles("a").unwrap()
    );
    assert_eq!(restored.dispatch("a", req, 1001).unwrap(), receipt);
}

#[test]
fn both_formation_role_and_set_piece_plans_feed_the_same_delegated_engine() {
    let mut game = setup();
    let mut home = SquadPlan::default();
    home.formation = "4-3-3".into();
    home.player_roles = [
        ("a-5".into(), PlayerRole::AnchorMan),
        ("a-8".into(), PlayerRole::InsideForward),
        ("a-9".into(), PlayerRole::Poacher),
    ]
    .into();
    home.match_roles.penalty_taker = Some("a-9".into());
    home.match_roles.free_kick_taker = Some("a-8".into());
    home.match_roles.captain = Some("a-2".into());
    home.match_roles.corner_taker = Some("a-10".into());
    let mut away = SquadPlan::default();
    away.formation = "5-3-2".into();
    away.player_roles = [
        ("b-1".into(), PlayerRole::WingBack),
        ("b-2".into(), PlayerRole::Stopper),
        ("b-6".into(), PlayerRole::BallWinner),
        ("b-8".into(), PlayerRole::ShadowStriker),
    ]
    .into();
    away.match_roles.penalty_taker = Some("b-10".into());
    away.match_roles.free_kick_taker = Some("b-8".into());
    away.match_roles.captain = Some("b-3".into());
    away.match_roles.corner_taker = Some("b-5".into());
    for (club, plan) in [("a", &home), ("b", &away)] {
        game.dispatch(
            club,
            request("plan", 1, Command::SetSquadPlan { plan: plan.clone() }),
            20,
        )
        .unwrap()
        .result
        .unwrap();
    }
    let team = |club: &str, plan: &SquadPlan| {
        let selected = squad_plan::select(
            &game.squad(club).unwrap(),
            &game.position_profiles(club).unwrap(),
            &game.lineup(club).unwrap(),
            plan,
        )
        .unwrap();
        assert_eq!(
            selected.bench.len(),
            15,
            "source retains all available reserves beyond the old twelve cap"
        );
        DelegatedTeam {
            team: engine::TeamData {
                id: club.into(),
                name: club.into(),
                formation: plan.formation.clone(),
                play_style: engine::PlayStyle::Balanced,
                tactics: Default::default(),
                players: selected.players,
            },
            bench: selected.bench,
            match_roles: Some(selected.match_roles),
            profile: Default::default(),
        }
    };
    let expected =
        serde_json::to_value(matches::play(team("a", &home), team("b", &away), 1001).unwrap())
            .unwrap();
    for (h, a) in [
        (&SquadPlan::default(), &away),
        (&home, &SquadPlan::default()),
    ] {
        assert_ne!(
            serde_json::to_value(matches::play(team("a", h), team("b", a), 1001).unwrap()).unwrap(),
            expected,
            "dropping either team's plan must change this seeded match"
        );
    }
    let results = game.advance_closed_day(1, 1000, 2000).unwrap();
    assert_eq!(serde_json::to_value(&results[0].report).unwrap(), expected);
    assert_eq!(game.squad_plan("a").unwrap(), home);
    assert_eq!(game.squad_plan("b").unwrap(), away);
    assert_eq!(
        game.position_profiles("a").unwrap()["a-8"].natural_position,
        Position::RightMidfielder
    );
}
