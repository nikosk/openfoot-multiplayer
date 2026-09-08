//! Two explicit scripted fixtures, not the final league scenario or world generator.
use engine::{PlayerData, PlayerRole, Position};
use management::{
    Club, Command, Management, Manager, Player, Request,
    football::{Fixture, Football},
};
use management::{
    football::RecoverySetup,
    recovery::{ClubRecovery, PlayerRecovery, RecoveryMode},
};

fn main() {
    let clubs = ["north", "south"].map(|id| Club {
        id: id.into(),
        name: format!("{id} FC"),
        balance: 1000,
    });
    let managers = ["north", "south"].map(|id| Manager {
        id: format!("manager-{id}"),
        club_id: id.into(),
    });
    let mut registry = vec![];
    let mut attributes = vec![];
    for club in ["north", "south"] {
        for index in 0..11 {
            let id = format!("{club}-{index}");
            registry.push(Player {
                id: id.clone(),
                name: id.clone(),
                club_id: club.into(),
            });
            attributes.push(PlayerData {
                id: id.clone(),
                name: id,
                position: match index {
                    0 => Position::Goalkeeper,
                    1..=4 => Position::Defender,
                    5..=8 => Position::Midfielder,
                    _ => Position::Forward,
                },
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
                traits: vec![],
                role: PlayerRole::Standard,
            });
        }
    }
    let management = Management::new(clubs.into(), registry, managers.into(), 1, 1000).unwrap();
    let fixtures = vec![
        Fixture {
            id: "leg-1".into(),
            day: 1,
            home: "north".into(),
            away: "south".into(),
            seed: 1001,
        },
        Fixture {
            id: "leg-2".into(),
            day: 2,
            home: "south".into(),
            away: "north".into(),
            seed: 1002,
        },
    ];
    let recovery = RecoverySetup {
        seed: 1001,
        players: attributes
            .iter()
            .map(|p| {
                (
                    p.id.clone(),
                    PlayerRecovery {
                        age: 25,
                        morale: 60,
                    },
                )
            })
            .collect(),
        clubs: ["north", "south"]
            .iter()
            .map(|id| {
                (
                    (*id).into(),
                    ClubRecovery {
                        physiotherapy: vec![],
                        medical_level: 1,
                    },
                )
            })
            .collect(),
    };
    let mut game = Football::new(management, attributes, fixtures).unwrap();
    game.configure_recovery(recovery).unwrap();
    for club in ["north", "south"] {
        game.dispatch(
            &format!("manager-{club}"),
            Request {
                id: "lineup".into(),
                day: 1,
                command: Command::SetLineup {
                    player_ids: (0..11).map(|i| format!("{club}-{i}")).collect(),
                },
            },
            100,
        )
        .unwrap()
        .result
        .unwrap();
    }
    // Deadlines close both days even though the scripted managers never say ready.
    for day in 1..=2 {
        let results = game
            .advance_closed_day(day, u64::from(day) * 1000, u64::from(day + 1) * 1000)
            .unwrap();
        for result in results {
            println!(
                "{} {}-{} {}",
                result.home, result.report.home_goals, result.report.away_goals, result.away
            );
        }
    }
    let before = game.squad("manager-north").unwrap()[0].condition;
    game.dispatch(
        "manager-north",
        Request {
            id: "recovery".into(),
            day: 3,
            command: Command::SetRecovery {
                mode: RecoveryMode::Recovery,
            },
        },
        2100,
    )
    .unwrap()
    .result
    .unwrap();
    assert!(game.advance_closed_day(3, 3000, 4000).unwrap().is_empty());
    assert!(game.squad("manager-north").unwrap()[0].condition > before);
    println!(
        "{}",
        serde_json::to_string_pretty(&game.standings()).unwrap()
    );
}
