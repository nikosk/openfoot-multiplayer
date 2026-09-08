use chrono::NaiveDate;
use management::career::CareerSetup;
use management::contracts::PlayerContract;
use management::economy::FinanceState;
use management::economy_runtime::EconomySetup;
use management::football::Football;
use management::market::{
    MarketCommand as C, MarketOutcome as O, MarketPreview, MarketSetup, Status, Terms,
};
use management::{Club, Command, Error, Management, Manager, Outcome, Player, Request};
use std::collections::BTreeMap;
fn date(s: &str) -> NaiveDate {
    s.parse().unwrap()
}
fn game(start: &str) -> Football {
    let mut sources = BTreeMap::new();
    let mut attributes = vec![];
    for club in ["a", "b", "c"] {
        for index in 0..2 {
            let id = format!("{club}-{index}");
            let attrs = serde_json::json!({"pace":60,"stamina":60,"strength":60,"passing":60,"shooting":60,"tackling":60,"dribbling":60,"defending":60,"positioning":60,"vision":60,"decisions":60,"composure":60,"leadership":60,"aggression":60});
            let mut source = domain::player::Player::new(
                id.clone(),
                id.clone(),
                id.clone(),
                "2000-01-01".into(),
                "ENG".into(),
                domain::player::Position::Midfielder,
                serde_json::from_value(attrs.clone()).unwrap(),
            );
            source.team_id = Some(club.into());
            source.morale = 70;
            source.wage = 5200;
            source.market_value = 100_000;
            source.potential = 90;
            source.ovr = 60;
            source.loan_listed = true;
            let mut engine = attrs;
            for (k, v) in [
                ("id", serde_json::json!(id)),
                ("name", serde_json::json!(id)),
                ("position", serde_json::json!("Midfielder")),
                ("condition", serde_json::json!(100)),
                ("fitness", serde_json::json!(100)),
                ("ovr", serde_json::json!(60)),
            ] {
                engine[k] = v;
            }
            attributes.push(serde_json::from_value::<engine::PlayerData>(engine).unwrap());
            sources.insert(id, source);
        }
    }
    let mut m = Management::new(
        ["a", "b", "c"]
            .map(|id| Club {
                id: id.into(),
                name: id.into(),
                balance: 1_000_000,
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
        ["a", "b", "c"]
            .map(|id| Manager {
                id: id.into(),
                club_id: id.into(),
            })
            .to_vec(),
        1,
        100,
    )
    .unwrap();
    m.configure_career(CareerSetup {
        today: date("2026-06-01"),
        contracts: sources
            .keys()
            .map(|id| {
                (
                    id.clone(),
                    PlayerContract::new(
                        date("2000-01-01"),
                        5200,
                        Some(date("2028-01-01")),
                        100_000,
                        70,
                        60,
                    ),
                )
            })
            .collect(),
        wage_budgets: ["a", "b", "c"].map(|id| (id.into(), 500_000)).into(),
        reputations: ["a", "b", "c"].map(|id| (id.into(), 500)).into(),
        staff_annual_wages: BTreeMap::new(),
    })
    .unwrap();
    let mut game = Football::new(m, attributes, vec![]).unwrap();
    game.configure_social(sources, 5).unwrap();
    let teams = ["a", "b", "c"]
        .map(|id| {
            (
                id.into(),
                domain::team::Team::new(
                    id.into(),
                    id.into(),
                    id.into(),
                    "ENG".into(),
                    "City".into(),
                    "Ground".into(),
                    10_000,
                ),
            )
        })
        .into();
    game.configure_personnel(teams, BTreeMap::new(), 7).unwrap();
    game.configure_economy(EconomySetup {
        seed: 8,
        season: 1,
        completed_home_dates: BTreeMap::new(),
        clubs: ["a", "b", "c"]
            .map(|id| {
                (
                    id.into(),
                    FinanceState {
                        wage_budget: 500_000,
                        transfer_budget: 1_000_000,
                        season_income: 0,
                        season_expenses: 0,
                        sponsorship: None,
                        financial_ledger: vec![],
                        reputation: 500,
                        stadium_capacity: 10_000,
                        form: vec![],
                    },
                )
            })
            .into(),
    })
    .unwrap();
    game.configure_market(MarketSetup {
        season_start: Some(date(start)),
    })
    .unwrap();
    game
}
fn issue(game: &mut Football, actor: &str, id: &str, command: C) -> Result<O, Error> {
    match game
        .dispatch(
            actor,
            Request {
                id: id.into(),
                day: game.window().day,
                command: Command::Market(command),
            },
            1,
        )?
        .result?
    {
        Outcome::Market(value) => Ok(value),
        other => panic!("Wrong outcome {other:?}"),
    }
}
fn bid(game: &mut Football, actor: &str, id: &str, player: &str, terms: Terms) -> u64 {
    match issue(
        game,
        actor,
        id,
        C::Bid {
            player_id: player.into(),
            terms,
        },
    )
    .unwrap()
    {
        O::Offer(o) => o.id,
        o => panic!("Expected offer {o:?}"),
    }
}
fn review(game: &mut Football, actor: &str, id: &str, offer_id: u64) -> MarketPreview {
    match issue(game, actor, id, C::Review { offer_id }).unwrap() {
        O::Preview(p) => p,
        o => panic!("Expected preview {o:?}"),
    }
}
fn transfer(game: &mut Football, player: &str, buyer: &str, seller: &str, prefix: &str) -> u64 {
    let id = bid(
        game,
        buyer,
        &format!("{prefix}-bid"),
        player,
        Terms::Transfer { fee: 1000 },
    );
    let p = review(game, seller, &format!("{prefix}-review"), id);
    assert!(matches!(
        issue(
            game,
            seller,
            &format!("{prefix}-confirm"),
            C::Confirm { preview_id: p.id }
        )
        .unwrap(),
        O::Registered(_)
    ));
    id
}

#[test]
fn fifo_consent_duplicate_receipts_and_competing_previews() {
    let mut game = game("2026-06-01");
    let a = bid(
        &mut game,
        "a",
        "a-bid",
        "b-0",
        Terms::Transfer { fee: 1000 },
    );
    let c = bid(
        &mut game,
        "c",
        "c-bid",
        "b-0",
        Terms::Transfer { fee: 2000 },
    );
    assert!(matches!(
        game.market_view("outsider"),
        Err(Error::Unauthorized)
    ));
    assert_eq!(game.market_view("a").unwrap().offers.len(), 1);
    let pa = review(&mut game, "b", "review-a", a);
    let pc = review(&mut game, "b", "review-c", c);
    let result = issue(&mut game, "b", "confirm", C::Confirm { preview_id: pa.id }).unwrap();
    assert!(matches!(result, O::Registered(_)));
    assert_eq!(
        result,
        issue(&mut game, "b", "confirm", C::Confirm { preview_id: pa.id }).unwrap()
    );
    assert!(issue(&mut game, "b", "loser", C::Confirm { preview_id: pc.id }).is_err());
    assert_eq!(game.economy_view("a").unwrap().balance, 999_000);
    assert_eq!(game.economy_view("b").unwrap().balance, 1_001_000);
    assert!(game.career_view("a").unwrap().contracts.contains_key("b-0"));
    assert_eq!(
        game.market_view("c").unwrap().offers[0].status,
        Status::Withdrawn
    );
    Football::load_validated(game.save_state().unwrap()).unwrap();
}

#[test]
fn changed_buyer_cash_requires_refreshed_preview_and_buyers_new_consent() {
    let mut game = game("2026-06-01");
    let offer = bid(&mut game, "a", "bid", "b-0", Terms::Transfer { fee: 1000 });
    let old = review(&mut game, "b", "review", offer);
    transfer(&mut game, "c-0", "a", "c", "other");
    let refreshed = match issue(&mut game, "b", "stale", C::Confirm { preview_id: old.id }).unwrap()
    {
        O::Refreshed(p) => p,
        o => panic!("Expected refresh {o:?}"),
    };
    assert!(refreshed.pending_other_consent);
    assert!(matches!(
        issue(
            &mut game,
            "b",
            "seller",
            C::Confirm {
                preview_id: refreshed.id
            }
        )
        .unwrap(),
        O::AwaitingConsent(_)
    ));
    let p = review(&mut game, "a", "buyer-review", offer);
    assert!(matches!(
        issue(
            &mut game,
            "a",
            "buyer-confirm",
            C::Confirm { preview_id: p.id }
        )
        .unwrap(),
        O::Registered(_)
    ));
}

#[test]
fn loan_preserves_parent_contract_owner_and_source_actual_vs_projected_payroll() {
    let mut game = game("2026-06-01");
    let id = bid(
        &mut game,
        "a",
        "loan",
        "b-0",
        Terms::Loan {
            end_date: date("2026-07-01"),
            wage_contribution_pct: 25,
            buy_option_fee: Some(10_000),
        },
    );
    let p = review(&mut game, "b", "review", id);
    issue(&mut game, "b", "confirm", C::Confirm { preview_id: p.id }).unwrap();
    assert!(game.career_view("b").unwrap().contracts.contains_key("b-0"));
    assert!(!game.career_view("a").unwrap().contracts.contains_key("b-0"));
    assert_eq!(
        game.economy_view("a").unwrap().snapshot.annual_wage_bill,
        11_700
    );
    assert_eq!(
        game.economy_view("b").unwrap().snapshot.annual_wage_bill,
        9_100
    );
    game.advance_closed_day(1, 100, 200).unwrap();
    assert_eq!(game.economy_view("a").unwrap().balance, 999_700);
    assert_eq!(game.economy_view("b").unwrap().balance, 999_900);
}

#[test]
fn closed_window_schedules_no_escrow_and_ready_reply_is_allowed() {
    let mut game = game("2026-09-01");
    let id = bid(&mut game, "a", "bid", "b-0", Terms::Transfer { fee: 1000 });
    game.dispatch(
        "b",
        Request {
            id: "ready".into(),
            day: 1,
            command: Command::Ready,
        },
        1,
    )
    .unwrap()
    .result
    .unwrap();
    let p = review(&mut game, "b", "review", id);
    assert_eq!(p.registration_date, date("2026-08-02"));
    assert!(matches!(
        issue(&mut game, "b", "confirm", C::Confirm { preview_id: p.id }).unwrap(),
        O::Scheduled(_)
    ));
    assert_eq!(game.economy_view("a").unwrap().balance, 1_000_000);
    assert!(game.career_view("b").unwrap().contracts.contains_key("b-0"));
    assert!(matches!(
        issue(
            &mut game,
            "b",
            "new-bid",
            C::Bid {
                player_id: "c-0".into(),
                terms: Terms::Transfer { fee: 1 }
            }
        ),
        Err(Error::AlreadyReady)
    ));
    Football::load_validated(game.save_state().unwrap()).unwrap();
}

#[test]
fn deferred_registration_rechecks_funds_and_commits_only_at_opening() {
    for insufficient in [false, true] {
        let mut game = game("2026-09-01");
        let id = bid(&mut game, "a", "bid", "b-0", Terms::Transfer { fee: 1000 });
        let p = review(&mut game, "b", "review", id);
        issue(&mut game, "b", "confirm", C::Confirm { preview_id: p.id }).unwrap();
        let mut saved = game.save_state().unwrap();
        saved["state"]["management"]["career"]["today"] = serde_json::json!("2026-08-02");
        if insufficient {
            saved["state"]["management"]["clubs"]["a"]["balance"] = serde_json::json!(999);
        }
        let mut game = Football::load_validated(saved).unwrap();
        game.advance_closed_day(1, 100, 200).unwrap();
        let offer = &game.market_view("a").unwrap().offers[0];
        assert_eq!(
            offer.status,
            if insufficient {
                Status::Withdrawn
            } else {
                Status::Completed
            }
        );
        assert_eq!(
            game.economy_view("a").unwrap().balance,
            if insufficient { 999 } else { 999_000 }
        );
        assert_eq!(
            game.career_view("a").unwrap().contracts.contains_key("b-0"),
            !insufficient
        );
        Football::load_validated(game.save_state().unwrap()).unwrap();
    }
}

#[test]
fn loan_return_develops_once_and_restores_parent_roster_and_checkpoint() {
    let mut game = game("2026-06-01");
    let id = bid(
        &mut game,
        "a",
        "loan",
        "b-0",
        Terms::Loan {
            end_date: date("2026-07-01"),
            wage_contribution_pct: 50,
            buy_option_fee: None,
        },
    );
    let p = review(&mut game, "b", "review", id);
    issue(&mut game, "b", "confirm", C::Confirm { preview_id: p.id }).unwrap();
    let before = game.project_source_players().unwrap()["b-0"]
        .attributes
        .passing;
    let mut saved = game.save_state().unwrap();
    saved["state"]["management"]["career"]["today"] = serde_json::json!("2026-07-01");
    saved["state"]["management"]["social"]["source_players"]["b-0"]["stats"]["minutes_played"] =
        serde_json::json!(900);
    saved["state"]["management"]["social"]["source_players"]["b-0"]["stats"]["appearances"] =
        serde_json::json!(8);
    let mut game = Football::load_validated(saved).unwrap();
    game.advance_closed_day(1, 100, 200).unwrap();
    let sources = game.project_source_players().unwrap();
    let player = &sources["b-0"];
    assert_eq!(player.team_id.as_deref(), Some("b"));
    assert!(player.active_loan.is_none());
    assert_eq!(player.attributes.passing, before + 2);
    assert!(game.market_view("a").unwrap().active_loans.is_empty());
    let mut restored = Football::load_validated(game.save_state().unwrap()).unwrap();
    game.advance_closed_day(2, 200, 300).unwrap();
    restored.advance_closed_day(2, 200, 300).unwrap();
    assert_eq!(game.save_state().unwrap(), restored.save_state().unwrap());
}

#[test]
fn loan_buy_option_is_reviewed_no_second_seller_consent_and_no_parent_budget_credit() {
    let mut game = game("2026-06-01");
    let id = bid(
        &mut game,
        "a",
        "loan",
        "b-0",
        Terms::Loan {
            end_date: date("2026-07-01"),
            wage_contribution_pct: 50,
            buy_option_fee: Some(10_000),
        },
    );
    let p = review(&mut game, "b", "review", id);
    issue(&mut game, "b", "confirm", C::Confirm { preview_id: p.id }).unwrap();
    let p = match issue(
        &mut game,
        "a",
        "buy-review",
        C::ReviewBuyOption {
            player_id: "b-0".into(),
        },
    )
    .unwrap()
    {
        O::Preview(p) => p,
        o => panic!("{o:?}"),
    };
    issue(
        &mut game,
        "a",
        "buy-confirm",
        C::Confirm { preview_id: p.id },
    )
    .unwrap();
    assert_eq!(game.economy_view("a").unwrap().balance, 990_000);
    assert_eq!(game.economy_view("b").unwrap().balance, 1_010_000);
    assert_eq!(
        game.economy_view("a").unwrap().account.transfer_budget,
        990_000
    );
    assert_eq!(
        game.economy_view("b").unwrap().account.transfer_budget,
        1_000_000
    );
    assert!(game.market_view("a").unwrap().active_loans.is_empty());
    assert!(game.career_view("a").unwrap().contracts.contains_key("b-0"));
    Football::load_validated(game.save_state().unwrap()).unwrap();
}

#[test]
fn native_plan_is_read_only_scoped_and_ignores_undisclosed_opponent_state() {
    let game = game("2026-06-01");
    let before = game.save_state().unwrap();
    let plan = game.bot_manager_plan("a", false).unwrap();
    assert_eq!(game.save_state().unwrap(), before);
    assert!(matches!(
        game.bot_manager_plan("outsider", false),
        Err(Error::Unauthorized)
    ));
    let mut changed = before;
    for id in ["b-0", "b-1", "c-0", "c-1"] {
        changed["state"]["management"]["career"]["contracts"][id]["morale"] = serde_json::json!(1);
        changed["state"]["management"]["career"]["contracts"][id]["market_value"] =
            serde_json::json!(999_999_999);
    }
    changed["state"]["management"]["clubs"]["b"]["balance"] = serde_json::json!(1);
    let changed = Football::load_validated(changed).unwrap();
    assert_eq!(
        serde_json::to_value(plan).unwrap(),
        serde_json::to_value(changed.bot_manager_plan("a", false).unwrap()).unwrap()
    );
}

#[test]
fn native_seller_counter_and_buyer_confirmation_use_identical_shared_commands() {
    let mut game = game("2026-06-01");
    bid(
        &mut game,
        "a",
        "external-bid",
        "b-0",
        Terms::Transfer { fee: 1000 },
    );
    let original = game.save_state().unwrap();
    let seller = game.bot_manager_plan("b", true).unwrap();
    assert_eq!(game.save_state().unwrap(), original);
    assert!(
        seller
            .commands
            .iter()
            .any(|c| matches!(c, Command::Market(C::Counter { .. })))
    );
    for (index, command) in seller.commands.into_iter().enumerate() {
        game.dispatch(
            "b",
            Request {
                id: format!("bot-seller-{index}"),
                day: 1,
                command,
            },
            1,
        )
        .unwrap()
        .result
        .unwrap();
    }
    // The seller's counter is not an automatic purchase from the other actor.
    assert!(!game.career_view("a").unwrap().contracts.contains_key("b-0"));
    let buyer = game.bot_manager_plan("a", true).unwrap();
    for (index, command) in buyer.commands.into_iter().enumerate() {
        let receipt = game
            .dispatch(
                "a",
                Request {
                    id: format!("bot-buyer-{index}"),
                    day: 1,
                    command,
                },
                1,
            )
            .unwrap();
        if let Outcome::Market(O::Preview(p)) = receipt.result.unwrap() {
            issue(
                &mut game,
                "a",
                "shared-confirm",
                C::Confirm { preview_id: p.id },
            )
            .unwrap();
        }
    }
    assert!(game.career_view("a").unwrap().contracts.contains_key("b-0"));
    assert_eq!(game.economy_view("a").unwrap().balance, 850_000);
    assert!(
        game.bot_manager_plan("a", true)
            .unwrap()
            .commands
            .is_empty()
    );
}

#[test]
fn ready_native_plan_contains_only_existing_negotiation_responses() {
    let mut game = game("2026-06-01");
    game.dispatch(
        "b",
        Request {
            id: "ready".into(),
            day: 1,
            command: Command::Ready,
        },
        1,
    )
    .unwrap()
    .result
    .unwrap();
    bid(
        &mut game,
        "a",
        "incoming",
        "b-0",
        Terms::Transfer { fee: 200_000 },
    );
    let plan = game.bot_manager_plan("b", false).unwrap();
    assert!(!plan.commands.is_empty());
    assert!(
        plan.commands
            .iter()
            .all(|c| matches!(c,Command::Market(m) if m.is_response()))
    );
}
