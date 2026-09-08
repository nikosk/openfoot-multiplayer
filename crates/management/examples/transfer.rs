//! A model-free transfer through the same participant command boundary.
use management::{Club, Command, Management, Manager, Outcome, Player, Request};

fn main() {
    let clubs = ["north", "south"].map(|id| Club {
        id: id.into(),
        name: format!("{id} FC"),
        balance: 1_000,
    });
    let managers = ["north", "south"].map(|id| Manager {
        id: format!("manager-{id}"),
        club_id: id.into(),
    });
    let mut game = Management::new(
        clubs.into(),
        vec![Player {
            id: "striker".into(),
            name: "Alex Example".into(),
            club_id: "south".into(),
        }],
        managers.into(),
        1,
        10_000,
    )
    .unwrap();
    let request = |id: &str, command| Request {
        id: id.into(),
        day: 1,
        command,
    };
    let receipt = game
        .dispatch(
            "manager-north",
            request(
                "bid",
                Command::Offer {
                    player_id: "striker".into(),
                    fee: 250,
                },
            ),
            10,
        )
        .unwrap();
    let Outcome::Offered(offer) = receipt.result.unwrap() else {
        panic!("offer expected")
    };
    let receipt = game
        .dispatch(
            "manager-south",
            request("review", Command::Review { offer_id: offer.id }),
            20,
        )
        .unwrap();
    let Outcome::Preview(preview) = receipt.result.unwrap() else {
        panic!("preview expected")
    };
    // Other reads and commands could occur here; the preview holds no world lock.
    let receipt = game
        .dispatch(
            "manager-south",
            request(
                "confirm",
                Command::Confirm {
                    preview_id: preview.id,
                },
            ),
            30,
        )
        .unwrap();
    assert!(matches!(receipt.result, Ok(Outcome::Transferred { .. })));
    assert_eq!(
        game.manager_view("manager-north").unwrap().club.balance,
        750
    );
    assert_eq!(
        game.manager_view("manager-south").unwrap().club.balance,
        1_250
    );
    // Only the public projection is printed, not private offers or finances.
    println!(
        "{}",
        serde_json::to_string_pretty(&game.public_state()).unwrap()
    );
}
