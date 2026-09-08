use engine::{PlayerData, PlayerRole, Position};
use management::football::{Football, RecoverySetup};
use management::recovery::{ClubRecovery, PlayerRecovery, RecoveryMode};
use management::{Club, Command, Error, Management, Manager, Outcome, Player, Request};

fn attributes(id: &str) -> PlayerData {
    PlayerData {
        id: id.into(),
        name: id.into(),
        position: Position::Midfielder,
        ovr: 65,
        condition: 40,
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
        traits: vec![],
        role: PlayerRole::Standard,
    }
}

fn game() -> Football {
    let clubs = ["a", "b"]
        .map(|id| Club {
            id: id.into(),
            name: id.into(),
            balance: 1000,
        })
        .to_vec();
    let managers = ["a", "b"]
        .map(|id| Manager {
            id: id.into(),
            club_id: id.into(),
        })
        .to_vec();
    let players = ["a", "b"]
        .map(|id| Player {
            id: format!("{id}-p"),
            name: id.into(),
            club_id: id.into(),
        })
        .to_vec();
    Football::new(
        Management::new(clubs, players, managers, 1, 1000).unwrap(),
        vec![attributes("a-p"), attributes("b-p")],
        vec![],
    )
    .unwrap()
}

fn setup() -> RecoverySetup {
    RecoverySetup {
        seed: 1001,
        players: ["a-p", "b-p"]
            .map(|id| {
                (
                    id.into(),
                    PlayerRecovery {
                        age: 25,
                        morale: 60,
                    },
                )
            })
            .into(),
        clubs: ["a", "b"]
            .map(|id| {
                (
                    id.into(),
                    ClubRecovery {
                        physiotherapy: vec![],
                        medical_level: 1,
                    },
                )
            })
            .into(),
    }
}

fn request(id: &str, command: Command) -> Request {
    Request {
        id: id.into(),
        day: 1,
        command,
    }
}

fn command(game: &mut Football, actor: &str, id: &str, command: Command) -> Result<Outcome, Error> {
    game.dispatch(actor, request(id, command), 10)
        .unwrap()
        .result
}

#[test]
fn rest_recovers_exactly_once_on_a_nonmatch_day() {
    let mut game = game();
    game.configure_recovery(setup()).unwrap();
    assert_eq!(game.recovery_view("a").unwrap().mode, RecoveryMode::Rest);
    assert!(game.advance_closed_day(1, 999, 2000).is_err());
    assert_eq!(game.squad("a").unwrap()[0].condition, 40);
    assert!(game.advance_closed_day(1, 1000, 2000).unwrap().is_empty());
    // floor(7 * .825 stamina * 1.05 age * .9 condition * 1.12 fitness) = 6.
    assert_eq!(game.squad("a").unwrap()[0].condition, 46);
    assert_eq!(game.squad("a").unwrap()[0].fitness, 75);
    assert!(game.advance_closed_day(1, 1000, 2000).is_err());
    assert_eq!(game.squad("a").unwrap()[0].condition, 46);
    game.advance_closed_day(2, 2000, 3000).unwrap();
    assert_eq!(game.squad("a").unwrap()[0].condition, 52);
}

#[test]
fn recovery_is_scoped_persistent_seeded_and_blocked_after_ready() {
    let mut first = game();
    let mut second = game();
    for game in [&mut first, &mut second] {
        game.configure_recovery(setup()).unwrap();
        assert_eq!(
            command(
                game,
                "a",
                "mode",
                Command::SetRecovery {
                    mode: RecoveryMode::Recovery
                }
            ),
            Ok(Outcome::RecoverySet)
        );
        assert_eq!(game.recovery_view("b").unwrap().mode, RecoveryMode::Rest);
        command(game, "a", "ready", Command::Ready).unwrap();
        assert_eq!(
            command(
                game,
                "a",
                "too-late",
                Command::SetRecovery {
                    mode: RecoveryMode::Rest
                }
            ),
            Err(Error::AlreadyReady)
        );
        game.advance_closed_day(1, 1000, 2000).unwrap();
        assert_eq!(game.squad("a").unwrap()[0].condition, 47);
        assert_eq!(game.squad("b").unwrap()[0].condition, 46);
        assert!((75..=76).contains(&game.squad("a").unwrap()[0].fitness));
        assert_eq!(
            game.recovery_view("a").unwrap().mode,
            RecoveryMode::Recovery
        );
    }
    assert_eq!(
        serde_json::to_value(first.squad("a").unwrap()).unwrap(),
        serde_json::to_value(second.squad("a").unwrap()).unwrap()
    );
}

#[test]
fn private_profiles_are_authorized_and_only_cover_owned_players() {
    let mut game = game();
    game.configure_recovery(setup()).unwrap();
    let view = game.recovery_view("a").unwrap();
    assert_eq!(
        view.players.keys().cloned().collect::<Vec<_>>(),
        vec!["a-p"]
    );
    assert_eq!(
        game.recovery_view("unknown").unwrap_err(),
        Error::Unauthorized
    );
    assert_eq!(
        game.dispatch(
            "unknown",
            request(
                "mode",
                Command::SetRecovery {
                    mode: RecoveryMode::Recovery
                }
            ),
            10
        ),
        Err(Error::Unauthorized)
    );
}

#[test]
fn setup_must_match_world_and_invalid_setup_does_not_enable_recovery() {
    let mut game = game();
    let mut missing = setup();
    missing.players.remove("a-p");
    assert!(game.configure_recovery(missing).is_err());
    let mut wrong_club = setup();
    let club = wrong_club.clubs.remove("a").unwrap();
    wrong_club.clubs.insert("stranger".into(), club);
    assert!(game.configure_recovery(wrong_club).is_err());
    let mut invalid = setup();
    invalid.players.get_mut("a-p").unwrap().morale = 101;
    assert!(game.configure_recovery(invalid).is_err());
    assert_eq!(game.recovery_view("a").unwrap_err(), Error::Unavailable);
    assert_eq!(game.squad("a").unwrap()[0].condition, 40);
    game.configure_recovery(setup()).unwrap();
    assert!(game.configure_recovery(setup()).is_err());
}

#[test]
fn setup_cannot_be_enabled_after_commands_or_day_processing() {
    let mut commanded = game();
    command(&mut commanded, "a", "ready", Command::Ready).unwrap();
    assert!(commanded.configure_recovery(setup()).is_err());
    let mut advanced = game();
    advanced.advance_closed_day(1, 1000, 2000).unwrap();
    assert!(advanced.configure_recovery(setup()).is_err());
}

#[test]
fn disabled_recovery_returns_stable_idempotent_error() {
    let mut game = game();
    let request = request(
        "mode",
        Command::SetRecovery {
            mode: RecoveryMode::Recovery,
        },
    );
    let first = game.dispatch("a", request.clone(), 10).unwrap();
    assert_eq!(first.result, Err(Error::Unavailable));
    assert_eq!(game.dispatch("a", request, 20).unwrap(), first);
    game.advance_closed_day(1, 1000, 2000).unwrap();
    assert_eq!(game.squad("a").unwrap()[0].condition, 40);
}

#[test]
fn transferred_player_recovers_using_new_clubs_facilities() {
    let mut game = game();
    let mut setup = setup();
    setup.clubs.get_mut("a").unwrap().medical_level = 5;
    game.configure_recovery(setup).unwrap();
    let Outcome::Offered(offer) = command(
        &mut game,
        "a",
        "bid",
        Command::Offer {
            player_id: "b-p".into(),
            fee: 100,
        },
    )
    .unwrap() else {
        panic!("offer expected")
    };
    let Outcome::Preview(preview) = command(
        &mut game,
        "b",
        "review",
        Command::Review { offer_id: offer.id },
    )
    .unwrap() else {
        panic!("preview expected")
    };
    assert_eq!(
        command(
            &mut game,
            "b",
            "confirm",
            Command::Confirm {
                preview_id: preview.id
            }
        ),
        Ok(Outcome::Transferred { offer_id: offer.id })
    );
    assert!(game.recovery_view("b").unwrap().players.is_empty());
    assert_eq!(game.recovery_view("a").unwrap().players.len(), 2);
    game.advance_closed_day(1, 1000, 2000).unwrap();
    let transferred = game
        .squad("a")
        .unwrap()
        .into_iter()
        .find(|p| p.id == "b-p")
        .unwrap();
    // Level five supplies 1.4x recovery: floor(6.11226 * 1.4) = 8.
    assert_eq!(transferred.condition, 48);
}
