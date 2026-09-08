use management::{
    Club, Command, Error, Management, Manager, Offer, OfferStatus, Outcome, Player, Preview,
    Request,
};

fn setup() -> Management {
    let clubs = ["a", "b", "c"]
        .map(|id| Club {
            id: id.into(),
            name: format!("Club {id}"),
            balance: 1_000,
        })
        .to_vec();
    let players = ["a", "b", "c"]
        .into_iter()
        .flat_map(|club| {
            (1..=2).map(move |number| Player {
                id: format!("{club}{number}"),
                name: format!("Player {club}{number}"),
                club_id: club.into(),
            })
        })
        .collect();
    let managers = ["a", "b", "c"]
        .map(|id| Manager {
            id: id.into(),
            club_id: id.into(),
        })
        .to_vec();
    Management::new(clubs, players, managers, 1, 1_000).unwrap()
}

fn request(id: &str, command: Command) -> Request {
    Request {
        id: id.into(),
        day: 1,
        command,
    }
}

fn act(game: &mut Management, actor: &str, id: &str, command: Command) -> Result<Outcome, Error> {
    game.dispatch(actor, request(id, command), 100)
        .unwrap()
        .result
}

fn offer(game: &mut Management, buyer: &str, id: &str, player: &str, fee: u64) -> Offer {
    match act(
        game,
        buyer,
        id,
        Command::Offer {
            player_id: player.into(),
            fee,
        },
    )
    .unwrap()
    {
        Outcome::Offered(offer) => offer,
        other => panic!("expected offer, got {other:?}"),
    }
}

fn review(game: &mut Management, seller: &str, id: &str, offer: &Offer) -> Preview {
    match act(game, seller, id, Command::Review { offer_id: offer.id }).unwrap() {
        Outcome::Preview(preview) => preview,
        other => panic!("expected preview, got {other:?}"),
    }
}

fn balance(game: &Management, actor: &str) -> u64 {
    game.manager_view(actor).unwrap().club.balance
}

fn owner(game: &Management, player: &str) -> String {
    game.public_state()
        .players
        .into_iter()
        .find(|p| p.id == player)
        .unwrap()
        .club_id
}

#[test]
fn seller_consent_is_required_and_private_projection_is_scoped() {
    let mut game = setup();
    let bid = offer(&mut game, "a", "offer", "b1", 200);
    assert_eq!(owner(&game, "b1"), "b");
    assert_eq!(balance(&game, "a"), 1_000);
    assert_eq!(game.manager_view("a").unwrap().offers, vec![bid.clone()]);
    assert_eq!(game.manager_view("b").unwrap().offers, vec![bid.clone()]);
    assert!(game.manager_view("c").unwrap().offers.is_empty());
    assert_eq!(game.manager_view("outsider"), Err(Error::Unauthorized));
    assert_eq!(
        game.dispatch("outsider", request("r", Command::Ready), 100),
        Err(Error::Unauthorized)
    );
    for actor in ["a", "c"] {
        assert_eq!(
            act(
                &mut game,
                actor,
                "review",
                Command::Review { offer_id: bid.id }
            ),
            Err(Error::Unavailable)
        );
        assert_eq!(
            act(
                &mut game,
                actor,
                "reject",
                Command::Reject { offer_id: bid.id }
            ),
            Err(Error::Unavailable)
        );
    }
    let preview = review(&mut game, "b", "review", &bid);
    assert_eq!(preview.seller_balance_after, 1_200);
    assert_eq!(
        act(
            &mut game,
            "c",
            "confirm",
            Command::Confirm {
                preview_id: preview.id
            }
        ),
        Err(Error::Unavailable)
    );
    let public = serde_json::to_value(game.public_state()).unwrap();
    assert_eq!(public.as_object().unwrap().len(), 3);
    assert!(public.get("offers").is_none());
    for club in public["clubs"].as_array().unwrap() {
        assert_eq!(club.as_object().unwrap().len(), 2);
        assert!(club.get("balance").is_none());
    }
    let serialized_preview = serde_json::to_value(&preview).unwrap();
    assert!(serialized_preview.get("dependencies").is_none());
    assert!(serialized_preview.get("buyer_balance").is_none());
    assert_eq!(
        act(
            &mut game,
            "b",
            "confirm",
            Command::Confirm {
                preview_id: preview.id
            }
        ),
        Ok(Outcome::Transferred { offer_id: bid.id })
    );
    assert_eq!(owner(&game, "b1"), "a");
    assert_eq!(
        (
            balance(&game, "a"),
            balance(&game, "b"),
            balance(&game, "c")
        ),
        (800, 1_200, 1_000)
    );
}

#[test]
fn unrelated_pending_offer_does_not_invalidate_preview() {
    let mut game = setup();
    let bid = offer(&mut game, "a", "offer", "b1", 200);
    let preview = review(&mut game, "b", "review", &bid);
    offer(&mut game, "c", "unrelated", "a2", 100);
    assert_eq!(
        act(
            &mut game,
            "b",
            "confirm",
            Command::Confirm {
                preview_id: preview.id
            }
        ),
        Ok(Outcome::Transferred { offer_id: bid.id })
    );
}

#[test]
fn competing_commit_withdraws_old_offer_and_cannot_double_sell_player() {
    let mut game = setup();
    let first = offer(&mut game, "a", "offer", "b1", 200);
    let first_preview = review(&mut game, "b", "review-first", &first);
    let second = offer(&mut game, "c", "offer", "b1", 300);
    let second_preview = review(&mut game, "b", "review-second", &second);
    assert_eq!(
        act(
            &mut game,
            "b",
            "confirm-second",
            Command::Confirm {
                preview_id: second_preview.id
            }
        ),
        Ok(Outcome::Transferred {
            offer_id: second.id
        })
    );
    assert_eq!(
        act(
            &mut game,
            "b",
            "confirm-first",
            Command::Confirm {
                preview_id: first_preview.id
            }
        ),
        Err(Error::Unavailable)
    );
    assert_eq!(owner(&game, "b1"), "c");
    assert_eq!(
        (
            balance(&game, "a"),
            balance(&game, "b"),
            balance(&game, "c")
        ),
        (1_000, 1_300, 700)
    );
    assert_eq!(
        game.manager_view("a").unwrap().offers[0].status,
        OfferStatus::Withdrawn
    );
}

#[test]
fn changed_buyer_funds_require_new_confirmation_without_mutating_target_deal() {
    let mut game = setup();
    let first = offer(&mut game, "a", "offer-b", "b1", 200);
    let original = review(&mut game, "b", "review", &first);
    let other = offer(&mut game, "a", "offer-c", "c1", 100);
    let other_preview = review(&mut game, "c", "review", &other);
    assert_eq!(
        act(
            &mut game,
            "c",
            "confirm",
            Command::Confirm {
                preview_id: other_preview.id
            }
        ),
        Ok(Outcome::Transferred { offer_id: other.id })
    );
    let refreshed = match act(
        &mut game,
        "b",
        "stale-confirm",
        Command::Confirm {
            preview_id: original.id,
        },
    )
    .unwrap()
    {
        Outcome::RefreshRequired(preview) => preview,
        other => panic!("expected fresh approval request, got {other:?}"),
    };
    assert_ne!(refreshed.id, original.id);
    assert_eq!(refreshed.offer, first);
    assert_eq!(owner(&game, "b1"), "b");
    assert_eq!((balance(&game, "a"), balance(&game, "b")), (900, 1_000));
    assert_eq!(
        act(
            &mut game,
            "b",
            "reuse-stale",
            Command::Confirm {
                preview_id: original.id
            }
        ),
        Err(Error::Unavailable)
    );
    assert_eq!(
        act(
            &mut game,
            "b",
            "fresh-confirm",
            Command::Confirm {
                preview_id: refreshed.id
            }
        ),
        Ok(Outcome::Transferred { offer_id: first.id })
    );
    assert_eq!(
        (
            balance(&game, "a"),
            balance(&game, "b"),
            balance(&game, "c")
        ),
        (700, 1_200, 1_100)
    );
}

#[test]
fn ready_manager_can_respond_until_all_ready_but_cannot_start_new_business() {
    let mut game = setup();
    let bid = offer(&mut game, "a", "offer", "b1", 200);
    assert_eq!(
        act(&mut game, "a", "ready", Command::Ready),
        Ok(Outcome::Ready)
    );
    assert_eq!(
        act(&mut game, "b", "ready", Command::Ready),
        Ok(Outcome::Ready)
    );
    assert!(!game.closed(100));
    assert_eq!(
        act(
            &mut game,
            "b",
            "new-offer",
            Command::Offer {
                player_id: "c1".into(),
                fee: 50
            }
        ),
        Err(Error::AlreadyReady)
    );
    let preview = review(&mut game, "b", "review", &bid);
    assert_eq!(
        act(
            &mut game,
            "b",
            "confirm",
            Command::Confirm {
                preview_id: preview.id
            }
        ),
        Ok(Outcome::Transferred { offer_id: bid.id })
    );
    assert_eq!(
        act(&mut game, "c", "ready", Command::Ready),
        Ok(Outcome::Ready)
    );
    assert!(game.closed(100));
    assert_eq!(
        act(
            &mut game,
            "b",
            "late-review",
            Command::Review { offer_id: bid.id }
        ),
        Err(Error::DayClosed)
    );
}

#[test]
fn deadline_is_inclusive_and_stale_day_cannot_execute() {
    let mut game = setup();
    assert!(!game.closed(999));
    let bid = game
        .dispatch(
            "a",
            request(
                "offer",
                Command::Offer {
                    player_id: "b1".into(),
                    fee: 200,
                },
            ),
            999,
        )
        .unwrap();
    assert!(matches!(bid.result, Ok(Outcome::Offered(_))));
    assert!(game.closed(1_000));
    let receipt = game
        .dispatch("b", request("ready", Command::Ready), 1_000)
        .unwrap();
    assert_eq!(receipt.result, Err(Error::DayClosed));
    assert!(!game.manager_view("b").unwrap().ready);
    let wrong_day = Request {
        day: 2,
        ..request("tomorrow", Command::Ready)
    };
    assert_eq!(
        game.dispatch("c", wrong_day, 1_000).unwrap().result,
        Err(Error::WrongDay)
    );
}

#[test]
fn request_identity_is_actor_scoped_and_retries_cannot_spend_twice() {
    let mut game = setup();
    let command = request(
        "same",
        Command::Offer {
            player_id: "b1".into(),
            fee: 200,
        },
    );
    let first = game.dispatch("a", command.clone(), 100).unwrap();
    let duplicate = game.dispatch("a", command.clone(), 200).unwrap();
    assert_eq!(first, duplicate);
    assert_eq!(game.manager_view("a").unwrap().offers.len(), 1);
    assert_eq!(
        game.dispatch("a", request("same", Command::Ready), 200),
        Err(Error::RequestIdReused)
    );
    let independent = game.dispatch("c", command, 200).unwrap();
    assert!(independent.sequence > first.sequence);
    let bid = match first.result {
        Ok(Outcome::Offered(offer)) => offer,
        other => panic!("{other:?}"),
    };
    let preview = review(&mut game, "b", "review", &bid);
    let confirmation = request(
        "confirm",
        Command::Confirm {
            preview_id: preview.id,
        },
    );
    let committed = game.dispatch("b", confirmation.clone(), 300).unwrap();
    assert_eq!(
        committed.result,
        Ok(Outcome::Transferred { offer_id: bid.id })
    );
    assert_eq!(game.dispatch("b", confirmation, 2_000).unwrap(), committed);
    assert_eq!((balance(&game, "a"), balance(&game, "b")), (800, 1_200));
}

#[test]
fn firing_revokes_access_and_preserves_club_and_players() {
    let mut game = setup();
    let before = game.public_state();
    let bid = offer(&mut game, "a", "offer", "b1", 200);
    let preview = review(&mut game, "b", "review", &bid);
    game.eliminate("b").unwrap();
    assert_eq!(game.public_state(), before);
    assert_eq!(game.manager_view("b"), Err(Error::Unauthorized));
    assert_eq!(
        game.dispatch(
            "b",
            request("review", Command::Review { offer_id: bid.id }),
            100
        ),
        Err(Error::Unauthorized)
    );
    assert_eq!(
        game.dispatch(
            "b",
            request(
                "confirm",
                Command::Confirm {
                    preview_id: preview.id
                }
            ),
            100
        ),
        Err(Error::Unauthorized)
    );
    assert_eq!(
        game.manager_view("a").unwrap().offers[0].status,
        OfferStatus::Withdrawn
    );
    assert_eq!(game.eliminate("b"), Err(Error::Unauthorized));
    act(&mut game, "a", "ready", Command::Ready).unwrap();
    act(&mut game, "c", "ready", Command::Ready).unwrap();
    assert!(game.closed(100));
}

#[test]
fn rejected_offer_invalidates_its_pending_preview() {
    let mut game = setup();
    let bid = offer(&mut game, "a", "offer", "b1", 200);
    let preview = review(&mut game, "b", "review", &bid);
    assert_eq!(
        act(
            &mut game,
            "b",
            "reject",
            Command::Reject { offer_id: bid.id }
        ),
        Ok(Outcome::Rejected)
    );
    assert_eq!(
        act(
            &mut game,
            "b",
            "confirm",
            Command::Confirm {
                preview_id: preview.id
            }
        ),
        Err(Error::Unavailable)
    );
    assert_eq!(owner(&game, "b1"), "b");
    assert_eq!((balance(&game, "a"), balance(&game, "b")), (1_000, 1_000));
}

#[test]
fn failed_receipt_is_stable_after_the_world_changes() {
    let mut game = setup();
    let too_large = request(
        "large",
        Command::Offer {
            player_id: "b1".into(),
            fee: 1_100,
        },
    );
    let failed = game.dispatch("a", too_large.clone(), 100).unwrap();
    assert_eq!(failed.result, Err(Error::InsufficientFunds));
    let sale = offer(&mut game, "c", "buy", "a1", 200);
    let preview = review(&mut game, "a", "review", &sale);
    act(
        &mut game,
        "a",
        "confirm",
        Command::Confirm {
            preview_id: preview.id,
        },
    )
    .unwrap();
    assert_eq!(balance(&game, "a"), 1_200);
    assert_eq!(game.dispatch("a", too_large, 200).unwrap(), failed);
    let fresh = offer(&mut game, "a", "fresh-large", "b1", 1_100);
    assert_eq!(fresh.fee, 1_100);
}

#[test]
fn seller_overflow_rejects_review_without_changing_ownership_or_money() {
    let mut game = Management::new(
        vec![
            Club {
                id: "a".into(),
                name: "Buyer".into(),
                balance: 1_000,
            },
            Club {
                id: "b".into(),
                name: "Seller".into(),
                balance: u64::MAX,
            },
        ],
        vec![Player {
            id: "b1".into(),
            name: "Player".into(),
            club_id: "b".into(),
        }],
        vec![
            Manager {
                id: "a".into(),
                club_id: "a".into(),
            },
            Manager {
                id: "b".into(),
                club_id: "b".into(),
            },
        ],
        1,
        1_000,
    )
    .unwrap();
    let bid = offer(&mut game, "a", "offer", "b1", 1);
    assert_eq!(
        act(
            &mut game,
            "b",
            "review",
            Command::Review { offer_id: bid.id }
        ),
        Err(Error::Overflow)
    );
    assert_eq!(owner(&game, "b1"), "b");
    assert_eq!(
        (balance(&game, "a"), balance(&game, "b")),
        (1_000, u64::MAX)
    );
}

#[test]
fn observed_deadline_closure_cannot_be_reopened_by_an_earlier_clock_reading() {
    let mut game = setup();
    assert!(game.closed(1_000));
    assert!(game.closed(999));
    assert_eq!(
        game.dispatch("a", request("ready", Command::Ready), 999)
            .unwrap()
            .result,
        Err(Error::DayClosed)
    );
    assert!(!game.manager_view("a").unwrap().ready);
}
