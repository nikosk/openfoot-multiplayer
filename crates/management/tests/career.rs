use chrono::NaiveDate;
use management::career::{
    CareerCommand, CareerOutcome, CareerPreview, CareerSetup, ContractAction,
};
use management::contracts::{Decision, PlayerContract};
use management::{Club, Command, Error, Management, Manager, Outcome, Player, Request};
use std::collections::BTreeMap;

fn date(text: &str) -> NaiveDate {
    text.parse().unwrap()
}
fn world(balance: i64, expire: bool) -> Management {
    let clubs = ["a", "b"]
        .map(|id| Club {
            id: id.into(),
            name: id.into(),
            balance,
        })
        .to_vec();
    let managers = ["a", "b"]
        .map(|id| Manager {
            id: id.into(),
            club_id: id.into(),
        })
        .to_vec();
    let players: Vec<_> = ["a", "b"]
        .into_iter()
        .flat_map(|club| {
            (0..12).map(move |i| Player {
                id: format!("{club}-{i}"),
                name: format!("Player {club}-{i}"),
                club_id: club.into(),
            })
        })
        .collect();
    let contracts = players
        .iter()
        .map(|player| {
            (
                player.id.clone(),
                PlayerContract::new(
                    date("2000-01-01"),
                    100,
                    Some(if expire && player.id == "a-0" {
                        date("2026-01-05")
                    } else {
                        date("2028-01-01")
                    }),
                    100_000,
                    70,
                    60,
                ),
            )
        })
        .collect();
    let mut game = Management::new(clubs, players, managers, 1, 1000).unwrap();
    game.configure_career(CareerSetup {
        today: date("2026-01-04"),
        contracts,
        wage_budgets: BTreeMap::from([("a".into(), 100_000), ("b".into(), 100_000)]),
        reputations: BTreeMap::from([("a".into(), 50), ("b".into(), 50)]),
        staff_annual_wages: BTreeMap::from([("a".into(), vec![100]), ("b".into(), vec![100])]),
    })
    .unwrap();
    game
}
fn issue(
    game: &mut Management,
    actor: &str,
    id: &str,
    command: CareerCommand,
) -> Result<CareerOutcome, Error> {
    let receipt = game.dispatch(
        actor,
        Request {
            id: id.into(),
            day: 1,
            command: Command::Career(command),
        },
        10,
    )?;
    match receipt.result? {
        Outcome::Career(outcome) => Ok(outcome),
        _ => panic!("Wrong command outcome"),
    }
}
fn review(game: &mut Management, actor: &str, id: &str, action: ContractAction) -> CareerPreview {
    match issue(game, actor, id, CareerCommand::Review { action }).unwrap() {
        CareerOutcome::Preview(preview) => preview,
        outcome => panic!("Expected preview, got {outcome:?}"),
    }
}
fn renew(player: &str) -> ContractAction {
    ContractAction::Renew {
        player_id: player.into(),
        weekly_wage: 3000,
        years: 3,
    }
}

#[test]
fn reviews_are_private_and_only_current_confirmation_commits_terms() {
    let mut game = world(100, false);
    let initial = game.career_view("a").unwrap();
    assert_eq!(initial.contracts.len(), 12);
    assert!(initial.contracts.keys().all(|id| id.starts_with("a-")));
    assert_eq!(game.career_view("outsider"), Err(Error::Unauthorized));
    assert_eq!(
        issue(
            &mut game,
            "b",
            "foreign",
            CareerCommand::Review {
                action: renew("a-0")
            }
        ),
        Err(Error::Unavailable)
    );
    let preview = review(&mut game, "a", "review", renew("a-0"));
    assert_eq!(game.career_view("a").unwrap(), initial);
    assert_eq!(
        issue(
            &mut game,
            "b",
            "steal-preview",
            CareerCommand::Confirm {
                preview_id: preview.id
            }
        ),
        Err(Error::Unavailable)
    );
    assert!(matches!(
        issue(
            &mut game,
            "a",
            "confirm",
            CareerCommand::Confirm {
                preview_id: preview.id
            }
        )
        .unwrap(),
        CareerOutcome::Applied { .. }
    ));
    let confirmed = game.career_view("a").unwrap();
    assert_eq!(confirmed.contracts["a-0"].weekly_wage, 3000);
    assert_eq!(
        confirmed.contracts["a-0"].end_date,
        Some(date("2029-01-04"))
    );
    let duplicate = issue(
        &mut game,
        "a",
        "confirm",
        CareerCommand::Confirm {
            preview_id: preview.id,
        },
    )
    .unwrap();
    assert!(matches!(duplicate, CareerOutcome::Applied { .. }));
    assert_eq!(game.career_view("a").unwrap(), confirmed);
}

#[test]
fn changed_contract_invalidates_review_and_negotiation_rejections_persist() {
    let mut game = world(100, false);
    let preview = review(&mut game, "a", "review", renew("a-0"));
    issue(
        &mut game,
        "a",
        "let-expire",
        CareerCommand::LetExpire {
            player_id: "a-0".into(),
            enabled: true,
        },
    )
    .unwrap();
    assert!(matches!(
        issue(
            &mut game,
            "a",
            "confirm",
            CareerCommand::Confirm {
                preview_id: preview.id
            }
        )
        .unwrap(),
        CareerOutcome::Decision {
            decision: Decision::Rejected { .. },
            ..
        }
    ));
    assert_eq!(
        game.career_view("a").unwrap().contracts["a-0"].weekly_wage,
        100
    );
    let insulting = ContractAction::Renew {
        player_id: "a-1".into(),
        weekly_wage: 1,
        years: 3,
    };
    assert!(matches!(
        issue(
            &mut game,
            "a",
            "insult",
            CareerCommand::Review { action: insulting }
        )
        .unwrap(),
        CareerOutcome::Decision {
            decision: Decision::Rejected { .. },
            ..
        }
    ));
    assert_eq!(
        game.career_view("a").unwrap().contracts["a-1"].blocked_until,
        Some(date("2026-02-03"))
    );
    assert!(matches!(
        issue(
            &mut game,
            "a",
            "blocked",
            CareerCommand::Review {
                action: renew("a-1")
            }
        )
        .unwrap(),
        CareerOutcome::Decision {
            decision: Decision::Rejected { .. },
            ..
        }
    ));
}

#[test]
fn expiry_releases_without_roster_exception_and_monday_wages_use_individual_division() {
    let mut game = world(10, true);
    game.advance_career(date("2026-01-05")).unwrap();
    assert_eq!(game.manager_view("a").unwrap().club.balance, 10);
    assert_eq!(game.career_view("a").unwrap().contracts.len(), 12);
    assert_eq!(
        game.career_view("a").unwrap().renewal_terms["a-0"].days_remaining,
        0
    );
    game.advance_career(date("2026-01-06")).unwrap();
    assert_eq!(game.career_date(), Some(date("2026-01-06")));
    assert_eq!(game.career_view("a").unwrap().contracts.len(), 11);
    assert_eq!(game.career_view("b").unwrap().free_agents[0].id, "a-0");
    assert_eq!(game.manager_view("a").unwrap().club.balance, -2); // 11 * (100/52) + staff(100/52)
    assert_eq!(game.manager_view("b").unwrap().club.balance, -3);
    assert_eq!(
        game.advance_career(date("2026-01-06")),
        Err(Error::WrongDay)
    );
    let before = game.manager_view("a").unwrap().club.balance;
    game.advance_career(date("2026-01-07")).unwrap();
    assert_eq!(game.manager_view("a").unwrap().club.balance, before);
    assert!(
        game.public_state()
            .players
            .iter()
            .any(|player| player.id == "a-0" && player.club_id.is_empty())
    );
}

#[test]
fn competing_free_agent_previews_cannot_sign_twice() {
    let mut game = world(100, true);
    game.advance_career(date("2026-01-05")).unwrap();
    game.advance_career(date("2026-01-06")).unwrap();
    let action = ContractAction::Sign {
        player_id: "a-0".into(),
        weekly_wage: 3000,
        years: 3,
    };
    let first = review(&mut game, "a", "sign-a", action.clone());
    let other = review(&mut game, "b", "sign-b", action);
    issue(
        &mut game,
        "a",
        "confirm-a",
        CareerCommand::Confirm {
            preview_id: first.id,
        },
    )
    .unwrap();
    assert_eq!(
        issue(
            &mut game,
            "b",
            "confirm-b",
            CareerCommand::Confirm {
                preview_id: other.id
            }
        ),
        Err(Error::Unavailable)
    );
    let signed = &game.career_view("a").unwrap().contracts["a-0"];
    assert_eq!(signed.weekly_wage, 3000);
    assert_eq!(signed.morale, 76);
    assert!(game.career_view("b").unwrap().free_agents.is_empty());
}

#[test]
fn termination_charges_severance_and_respects_remaining_squad() {
    let mut game = world(100, false);
    let preview = review(
        &mut game,
        "a",
        "terminate",
        ContractAction::Terminate {
            player_id: "a-0".into(),
        },
    );
    assert!(preview.severance > 100);
    issue(
        &mut game,
        "a",
        "confirm",
        CareerCommand::Confirm {
            preview_id: preview.id,
        },
    )
    .unwrap();
    assert_eq!(
        game.manager_view("a").unwrap().club.balance,
        preview.balance_after
    );
    assert!(game.manager_view("a").unwrap().club.balance < 0);
    assert_eq!(game.career_view("a").unwrap().contracts.len(), 11);
    assert_eq!(
        issue(
            &mut game,
            "a",
            "too-short",
            CareerCommand::Review {
                action: ContractAction::Terminate {
                    player_id: "a-1".into()
                }
            }
        ),
        Err(Error::SquadTooSmall)
    );
}

#[test]
fn failed_wage_day_is_atomic_and_ready_managers_cannot_change_contracts() {
    let mut game = world(i64::MIN, true);
    game.advance_career(date("2026-01-05")).unwrap();
    let before = game.career_view("a").unwrap();
    assert_eq!(
        game.advance_career(date("2026-01-06")),
        Err(Error::Overflow)
    );
    assert_eq!(game.career_view("a").unwrap(), before);
    assert_eq!(game.manager_view("a").unwrap().club.balance, i64::MIN);
    game.dispatch(
        "a",
        Request {
            id: "ready".into(),
            day: 1,
            command: Command::Ready,
        },
        10,
    )
    .unwrap();
    assert_eq!(
        issue(
            &mut game,
            "a",
            "late",
            CareerCommand::Review {
                action: renew("a-0")
            }
        ),
        Err(Error::AlreadyReady)
    );
}

#[test]
fn changed_wage_bill_requires_a_fresh_review_and_budget_failure_changes_nothing() {
    let mut game = world(100, false);
    let first = review(&mut game, "a", "first-review", renew("a-0"));
    let second = review(&mut game, "a", "second-review", renew("a-1"));
    issue(
        &mut game,
        "a",
        "first-confirm",
        CareerCommand::Confirm {
            preview_id: first.id,
        },
    )
    .unwrap();
    let refreshed = match issue(
        &mut game,
        "a",
        "stale-confirm",
        CareerCommand::Confirm {
            preview_id: second.id,
        },
    )
    .unwrap()
    {
        CareerOutcome::RefreshRequired(preview) => preview,
        other => panic!("Expected refreshed review, got {other:?}"),
    };
    assert_eq!(
        game.career_view("a").unwrap().contracts["a-1"].weekly_wage,
        100
    );
    issue(
        &mut game,
        "a",
        "fresh-confirm",
        CareerCommand::Confirm {
            preview_id: refreshed.id,
        },
    )
    .unwrap();
    let before = game.career_view("a").unwrap();
    assert!(matches!(
        issue(
            &mut game,
            "a",
            "too-expensive",
            CareerCommand::Review {
                action: ContractAction::Renew {
                    player_id: "a-2".into(),
                    weekly_wage: 200_000,
                    years: 3
                },
            }
        ),
        Err(Error::Contract(_))
    ));
    assert_eq!(game.career_view("a").unwrap(), before);
}

#[test]
fn initialization_rejects_expired_attached_contracts_and_invalid_reputation() {
    let mut game = Management::new(
        vec![Club {
            id: "a".into(),
            name: "A".into(),
            balance: 0,
        }],
        vec![Player {
            id: "p".into(),
            name: "Player".into(),
            club_id: "a".into(),
        }],
        vec![Manager {
            id: "a".into(),
            club_id: "a".into(),
        }],
        1,
        1000,
    )
    .unwrap();
    let mut setup = CareerSetup {
        today: date("2026-01-04"),
        contracts: BTreeMap::from([(
            "p".into(),
            PlayerContract::new(
                date("2000-01-01"),
                100,
                Some(date("2026-01-03")),
                100_000,
                70,
                60,
            ),
        )]),
        wage_budgets: BTreeMap::from([("a".into(), 100_000)]),
        reputations: BTreeMap::from([("a".into(), 50)]),
        staff_annual_wages: BTreeMap::new(),
    };
    assert!(game.configure_career(setup.clone()).is_err());
    assert_eq!(game.career_date(), None);
    setup.contracts.get_mut("p").unwrap().end_date = Some(date("2027-01-04"));
    setup.reputations.insert("a".into(), 1001);
    assert!(game.configure_career(setup.clone()).is_err());
    setup.reputations.insert("a".into(), 1000);
    game.configure_career(setup).unwrap();
    let terms = &game.career_view("a").unwrap().renewal_terms["p"];
    assert_eq!(terms.expected_years, 3);
    assert_eq!(terms.days_remaining, 365);
}

#[test]
fn private_checkpoint_preserves_previews_and_actor_negotiations_and_rejects_duplicate_keys() {
    use management::football::Football;
    let mut management = world(100, true);
    management.advance_career(date("2026-01-05")).unwrap();
    management.advance_career(date("2026-01-06")).unwrap();
    let preview = review(&mut management, "a", "review-before-save", renew("a-1"));
    issue(
        &mut management,
        "b",
        "free-agent-negotiation",
        CareerCommand::Review {
            action: ContractAction::Sign {
                player_id: "a-0".into(),
                weekly_wage: 1,
                years: 3,
            },
        },
    )
    .unwrap();
    let attributes = management
        .public_state()
        .players
        .iter()
        .map(|player| {
            let mut data = serde_json::json!({"id": player.id, "name": player.name,
            "position": "Midfielder", "role": "Standard", "traits": []});
            for field in [
                "ovr",
                "condition",
                "fitness",
                "pace",
                "stamina",
                "strength",
                "agility",
                "passing",
                "shooting",
                "tackling",
                "dribbling",
                "defending",
                "positioning",
                "vision",
                "decisions",
                "composure",
                "aggression",
                "teamwork",
                "leadership",
                "handling",
                "reflexes",
                "aerial",
            ] {
                data[field] = serde_json::json!(65);
            }
            serde_json::from_value::<engine::PlayerData>(data).unwrap()
        })
        .collect();
    let mut game = Football::new(management, attributes, vec![]).unwrap();
    let saved = game.save_state().unwrap();
    let mut restored = Football::load_validated(saved.clone()).unwrap();
    assert_eq!(
        game.career_view("a").unwrap(),
        restored.career_view("a").unwrap()
    );
    let request = Request {
        id: "after-save-confirm".into(),
        day: 1,
        command: Command::Career(CareerCommand::Confirm {
            preview_id: preview.id,
        }),
    };
    assert_eq!(
        game.dispatch("a", request.clone(), 10).unwrap(),
        restored.dispatch("a", request, 10).unwrap()
    );
    assert_eq!(game.save_state().unwrap(), restored.save_state().unwrap());
    let mut duplicate = saved;
    let negotiations = duplicate["state"]["management"]["career"]["negotiations"]
        .as_array_mut()
        .unwrap();
    assert_eq!(negotiations.len(), 1);
    negotiations.push(negotiations[0].clone());
    assert!(Football::load_validated(duplicate).is_err());
}
