use engine::{PlayerData, PlayerRole, Position};
use management::football::{Fixture, Football};
use management::{
    Club, Command, Error, Management, Manager, OfferStatus, Outcome, Player, Request,
};

fn attributes(club: &str, i: usize) -> PlayerData {
    PlayerData {
        id: format!("{club}-{i}"),
        name: format!("Player {club}-{i}"),
        position: match i {
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
    }
}

fn request(id: &str, day: u32, command: Command) -> Request {
    Request {
        id: id.into(),
        day,
        command,
    }
}

#[test]
fn later_fixture_failure_does_not_publish_earlier_staged_result() {
    let ids = ["a", "b", "c", "d"];
    let data: Vec<_> = ids
        .iter()
        .flat_map(|club| (0..if *club == "d" { 10 } else { 11 }).map(move |i| attributes(club, i)))
        .collect();
    let clubs = ids
        .iter()
        .map(|id| Club {
            id: (*id).into(),
            name: (*id).into(),
            balance: 1000,
        })
        .collect();
    let managers = ids
        .iter()
        .map(|id| Manager {
            id: (*id).into(),
            club_id: (*id).into(),
        })
        .collect();
    let players = data
        .iter()
        .map(|p| Player {
            id: p.id.clone(),
            name: p.name.clone(),
            club_id: p.id.split('-').next().unwrap().into(),
        })
        .collect();
    let management = Management::new(clubs, players, managers, 1, 1000).unwrap();
    let fixtures = [("first", "a", "b"), ("second", "c", "d")]
        .iter()
        .map(|(id, home, away)| Fixture {
            id: (*id).into(),
            day: 1,
            home: (*home).into(),
            away: (*away).into(),
            seed: 1001,
        })
        .collect();
    let mut game = Football::new(management, data, fixtures).unwrap();
    for club in ["a", "b", "c"] {
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
    let before = game.standings();
    let physical_before = serde_json::to_value(game.squad("a").unwrap()).unwrap();
    assert!(game.advance_closed_day(1, 1000, 2000).is_err());
    assert!(game.results().is_empty());
    assert_eq!(game.standings(), before);
    assert_eq!(game.public_state().day, 1);
    assert_eq!(
        serde_json::to_value(game.squad("a").unwrap()).unwrap(),
        physical_before
    );
}

fn management() -> (Management, Vec<PlayerData>) {
    management_with_size(11)
}

#[test]
fn playable_league_roster_floor_rejects_a_sale_without_partial_mutation() {
    let (mut core, data) = management_with_size(11);
    core.require_match_rosters().unwrap();
    let mut game = Football::new(core, data, vec![]).unwrap();
    let Outcome::Offered(offer) = game
        .dispatch(
            "a",
            request(
                "bid",
                1,
                Command::Offer {
                    player_id: "b-10".into(),
                    fee: 100,
                },
            ),
            10,
        )
        .unwrap()
        .result
        .unwrap()
    else {
        panic!("offer")
    };
    let Outcome::Preview(preview) = game
        .dispatch(
            "b",
            request("preview", 1, Command::Review { offer_id: offer.id }),
            20,
        )
        .unwrap()
        .result
        .unwrap()
    else {
        panic!("preview")
    };
    assert_eq!(
        game.dispatch(
            "b",
            request(
                "accept",
                1,
                Command::Confirm {
                    preview_id: preview.id,
                }
            ),
            30
        )
        .unwrap()
        .result,
        Err(Error::SquadTooSmall)
    );
    for club in ["a", "b"] {
        assert_eq!(game.manager_view(club).unwrap().club.balance, 1000);
        assert_eq!(game.squad(club).unwrap().len(), 11);
        assert_eq!(
            game.manager_view(club).unwrap().offers[0].status,
            OfferStatus::Pending
        );
    }
    let (mut too_small, _) = management_with_size(10);
    assert_eq!(too_small.require_match_rosters(), Err(Error::SquadTooSmall));
}

#[test]
fn configured_recovery_does_not_add_a_matchday_boost() {
    use management::football::RecoverySetup;
    use management::recovery::{ClubRecovery, PlayerRecovery};

    let make_game = |enabled| {
        let (management, attributes) = management_with_size(18);
        let setup = RecoverySetup {
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
        };
        let mut game = Football::new(
            management,
            attributes,
            vec![Fixture {
                id: "match".into(),
                day: 1,
                home: "a".into(),
                away: "b".into(),
                seed: 1001,
            }],
        )
        .unwrap();
        if enabled {
            game.configure_recovery(setup).unwrap();
        }
        game
    };
    let mut enabled = make_game(true);
    let mut disabled = make_game(false);
    for game in [&mut enabled, &mut disabled] {
        game.advance_closed_day(1, 1000, 2000).unwrap();
    }
    for club in ["a", "b"] {
        assert_eq!(
            serde_json::to_value(enabled.squad(club).unwrap()).unwrap(),
            serde_json::to_value(disabled.squad(club).unwrap()).unwrap()
        );
    }
    assert_eq!(
        serde_json::to_value(enabled.results()).unwrap(),
        serde_json::to_value(disabled.results()).unwrap()
    );
}

fn management_with_size(size: usize) -> (Management, Vec<PlayerData>) {
    let attributes: Vec<_> = ["a", "b"]
        .into_iter()
        .flat_map(|club| (0..size).map(move |i| attributes(club, i)))
        .collect();
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
    let players = attributes
        .iter()
        .map(|p| Player {
            id: p.id.clone(),
            name: p.name.clone(),
            club_id: p.id.split('-').next().unwrap().into(),
        })
        .collect();
    (
        Management::new(clubs, players, managers, 1, 1000).unwrap(),
        attributes,
    )
}

#[test]
fn missing_selection_is_repaired_and_match_wear_persists() {
    let (management, attributes) = management_with_size(18);
    let mut game = Football::new(
        management,
        attributes,
        vec![Fixture {
            id: "fixture".into(),
            day: 1,
            home: "a".into(),
            away: "b".into(),
            seed: 1001,
        }],
    )
    .unwrap();
    let results = game.advance_closed_day(1, 1000, 2000).unwrap();
    assert_eq!(results[0].home_starting_xi.len(), 11);
    assert_eq!(results[0].away_starting_xi.len(), 11);
    let report = &results[0].report;
    let mut played = 0;
    let mut unused = 0;
    for club in ["a", "b"] {
        for player in game.squad(club).unwrap() {
            let minutes = report
                .player_stats
                .get(&player.id)
                .map_or(0, |s| s.minutes_played);
            let depletion = (40.0 * (1.0 - 0.65 * 0.4) * f64::from(minutes) / 90.0) as u8;
            assert_eq!(player.condition, 100u8.saturating_sub(depletion));
            assert!(player.fitness >= 75);
            if minutes == 0 {
                unused += 1;
                assert_eq!(player.fitness, 75);
            } else {
                played += 1;
            }
        }
    }
    assert!(played >= 22);
    assert!(unused > 0);
    let after = serde_json::to_value(game.squad("a").unwrap()).unwrap();
    assert!(game.advance_closed_day(1, 1000, 2000).is_err());
    assert_eq!(
        serde_json::to_value(game.squad("a").unwrap()).unwrap(),
        after
    );
}

#[test]
fn transfer_repairs_departed_starter_from_reserves_without_overriding_others() {
    let (management, attributes) = management_with_size(12);
    let mut game = Football::new(
        management,
        attributes,
        vec![Fixture {
            id: "fixture".into(),
            day: 1,
            home: "a".into(),
            away: "b".into(),
            seed: 1001,
        }],
    )
    .unwrap();
    let preferred: Vec<_> = (0..11).map(|i| format!("b-{i}")).collect();
    game.dispatch(
        "b",
        request(
            "lineup",
            1,
            Command::SetLineup {
                player_ids: preferred.clone(),
            },
        ),
        10,
    )
    .unwrap()
    .result
    .unwrap();
    let Outcome::Offered(offer) = game
        .dispatch(
            "a",
            request(
                "bid",
                1,
                Command::Offer {
                    player_id: "b-10".into(),
                    fee: 100,
                },
            ),
            20,
        )
        .unwrap()
        .result
        .unwrap()
    else {
        panic!()
    };
    let Outcome::Preview(preview) = game
        .dispatch(
            "b",
            request("review", 1, Command::Review { offer_id: offer.id }),
            30,
        )
        .unwrap()
        .result
        .unwrap()
    else {
        panic!()
    };
    game.dispatch(
        "b",
        request(
            "confirm",
            1,
            Command::Confirm {
                preview_id: preview.id,
            },
        ),
        40,
    )
    .unwrap()
    .result
    .unwrap();
    let results = game.advance_closed_day(1, 1000, 2000).unwrap();
    let actual = &results[0].away_starting_xi;
    assert_eq!(&actual[..10], &preferred[..10]);
    assert_eq!(actual[10], "b-11");
    assert!(!actual.contains(&"b-10".to_string()));
}

fn setup() -> Football {
    let (management, attributes) = management();
    let fixtures = [(1, "a", "b"), (2, "b", "a")]
        .map(|(day, home, away)| Fixture {
            id: format!("fixture-{day}"),
            day,
            home: home.into(),
            away: away.into(),
            seed: 1000 + u64::from(day),
        })
        .to_vec();
    let mut football = Football::new(management, attributes, fixtures).unwrap();
    for club in ["a", "b"] {
        let result = football
            .dispatch(
                club,
                request(
                    "lineup",
                    1,
                    Command::SetLineup {
                        player_ids: (0..11).map(|i| format!("{club}-{i}")).collect(),
                    },
                ),
                10,
            )
            .unwrap()
            .result;
        assert_eq!(result, Ok(Outcome::LineupSet));
    }
    football
}

#[test]
fn two_deadline_matchdays_have_reproducible_reports_and_consistent_standings() {
    let mut first = setup();
    let mut second = setup();
    for game in [&mut first, &mut second] {
        assert!(game.advance_closed_day(1, 999, 2000).is_err());
        assert_eq!(game.public_state().day, 1);
        assert!(game.results().is_empty());
        // Neither manager is ready: the deadline must still advance football.
        assert!(!game.manager_view("a").unwrap().ready);
        assert!(!game.manager_view("b").unwrap().ready);
        assert_eq!(game.advance_closed_day(1, 1000, 2000).unwrap().len(), 1);
        assert_eq!(game.public_state().day, 2);
        assert!(game.advance_closed_day(1, 1001, 2000).is_err());
        assert_eq!(game.results().len(), 1);
        assert_eq!(game.advance_closed_day(2, 2000, 3000).unwrap().len(), 1);
        assert_eq!(game.public_state().day, 3);
        let rows = game.standings();
        assert_eq!(rows.len(), 2);
        for row in &rows {
            assert_eq!(row.played, 2);
            assert_eq!(row.played, row.won + row.drawn + row.lost);
            assert_eq!(row.points, 3 * row.won + row.drawn);
            let mut gf = 0;
            let mut ga = 0;
            for result in game.results() {
                let (scored, conceded) = if result.home == row.club_id {
                    (result.report.home_goals, result.report.away_goals)
                } else {
                    (result.report.away_goals, result.report.home_goals)
                };
                gf += u32::from(scored);
                ga += u32::from(conceded);
            }
            assert_eq!((row.goals_for, row.goals_against), (gf, ga));
        }
        assert_eq!(rows[0].goals_for, rows[1].goals_against);
        assert_eq!(rows[0].won, rows[1].lost);
    }
    assert_eq!(
        serde_json::to_value(first.results()).unwrap(),
        serde_json::to_value(second.results()).unwrap()
    );
    assert_eq!(first.standings(), second.standings());
}

#[test]
fn private_squad_reads_and_lineup_writes_are_scoped() {
    let selected = setup();
    assert_eq!(selected.lineup("outsider"), Err(Error::Unauthorized));
    assert_eq!(
        selected.lineup("a").unwrap(),
        (0..11).map(|i| format!("a-{i}")).collect::<Vec<_>>()
    );
    let mut game = setup();
    assert!(matches!(game.squad("outsider"), Err(Error::Unauthorized)));
    assert!(
        game.squad("a")
            .unwrap()
            .iter()
            .all(|p| p.id.starts_with("a-"))
    );
    let result = game
        .dispatch(
            "a",
            request(
                "opponent-xi",
                1,
                Command::SetLineup {
                    player_ids: (0..11).map(|i| format!("b-{i}")).collect(),
                },
            ),
            20,
        )
        .unwrap()
        .result;
    assert!(result.is_err());
    assert_eq!(
        game.dispatch("a", request("ready", 1, Command::Ready), 30)
            .unwrap()
            .result,
        Ok(Outcome::Ready)
    );
    assert_eq!(
        game.dispatch(
            "a",
            request(
                "late-xi",
                1,
                Command::SetLineup {
                    player_ids: (0..11).map(|i| format!("a-{i}")).collect(),
                }
            ),
            40
        )
        .unwrap()
        .result,
        Err(Error::AlreadyReady)
    );
}

#[test]
fn transferred_player_changes_squads_and_invalidates_old_lineup_without_advancing() {
    let mut game = setup();
    let offer = match game
        .dispatch(
            "a",
            request(
                "bid",
                1,
                Command::Offer {
                    player_id: "b-10".into(),
                    fee: 100,
                },
            ),
            20,
        )
        .unwrap()
        .result
        .unwrap()
    {
        Outcome::Offered(offer) => offer,
        other => panic!("{other:?}"),
    };
    let preview = match game
        .dispatch(
            "b",
            request("review", 1, Command::Review { offer_id: offer.id }),
            30,
        )
        .unwrap()
        .result
        .unwrap()
    {
        Outcome::Preview(preview) => preview,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        game.dispatch(
            "b",
            request(
                "confirm",
                1,
                Command::Confirm {
                    preview_id: preview.id
                }
            ),
            40
        )
        .unwrap()
        .result,
        Ok(Outcome::Transferred { offer_id: offer.id })
    );
    assert_eq!(game.squad("a").unwrap().len(), 12);
    assert_eq!(game.squad("b").unwrap().len(), 10);
    assert!(game.squad("a").unwrap().iter().any(|p| p.id == "b-10"));
    assert!(
        game.advance_closed_day(1, 1000, 2000)
            .unwrap_err()
            .contains("at least 11 eligible players")
    );
    assert_eq!(game.public_state().day, 1);
    assert!(game.results().is_empty());
    assert!(
        game.standings()
            .iter()
            .all(|row| row.played == 0 && row.points == 0)
    );
}

#[test]
fn next_day_preserves_offers_but_clears_previews_and_readiness() {
    let (mut game, _) = management();
    let offer = match game
        .dispatch(
            "a",
            request(
                "bid",
                1,
                Command::Offer {
                    player_id: "b-10".into(),
                    fee: 100,
                },
            ),
            20,
        )
        .unwrap()
        .result
        .unwrap()
    {
        Outcome::Offered(offer) => offer,
        other => panic!("{other:?}"),
    };
    let preview = match game
        .dispatch(
            "b",
            request("review", 1, Command::Review { offer_id: offer.id }),
            30,
        )
        .unwrap()
        .result
        .unwrap()
    {
        Outcome::Preview(preview) => preview,
        other => panic!("{other:?}"),
    };
    for actor in ["a", "b"] {
        assert_eq!(
            game.dispatch(actor, request("ready", 1, Command::Ready), 40)
                .unwrap()
                .result,
            Ok(Outcome::Ready)
        );
    }
    game.next_day(1, 50, 2000).unwrap();
    for actor in ["a", "b"] {
        let view = game.manager_view(actor).unwrap();
        assert!(!view.ready);
        assert_eq!(view.offers[0].status, OfferStatus::Pending);
    }
    assert!(!game.closed(51));
    assert_eq!(
        game.dispatch(
            "b",
            request(
                "old-preview",
                2,
                Command::Confirm {
                    preview_id: preview.id
                }
            ),
            60
        )
        .unwrap()
        .result,
        Err(Error::Unavailable)
    );
    assert!(matches!(
        game.dispatch(
            "b",
            request("fresh-review", 2, Command::Review { offer_id: offer.id }),
            70
        )
        .unwrap()
        .result,
        Ok(Outcome::Preview(_))
    ));
    assert_eq!(game.next_day(1, 80, 2000), Err(Error::WrongDay));
    assert_eq!(game.public_state().day, 2);
}

#[test]
fn match_plans_are_private_scoped_persistent_and_follow_request_rules() {
    use management::tactics::MatchPlan;

    let mut game = setup();
    let default = MatchPlan::default();
    let attacking = MatchPlan {
        play_style: engine::PlayStyle::Attacking,
        pressing_intensity: engine::PressingIntensity::Aggressive,
        ..default.clone()
    };
    assert_eq!(game.match_plan("outsider"), Err(Error::Unauthorized));
    assert_eq!(game.match_plan("a").unwrap(), default);
    assert_eq!(game.match_plan("b").unwrap(), default);
    let public_before = serde_json::to_value(game.public_state()).unwrap();
    let opponent_before = serde_json::to_value(game.manager_view("b").unwrap()).unwrap();
    let change = request(
        "plan",
        1,
        Command::SetMatchPlan {
            plan: attacking.clone(),
        },
    );
    assert_eq!(
        game.dispatch("outsider", change.clone(), 20),
        Err(Error::Unauthorized)
    );
    let receipt = game.dispatch("a", change.clone(), 20).unwrap();
    assert_eq!(receipt.result, Ok(Outcome::MatchPlanSet));
    assert_eq!(game.match_plan("a").unwrap(), attacking);
    assert_eq!(game.match_plan("b").unwrap(), default);
    assert_eq!(
        serde_json::to_value(game.public_state()).unwrap(),
        public_before
    );
    assert_eq!(
        serde_json::to_value(game.manager_view("b").unwrap()).unwrap(),
        opponent_before
    );
    assert_eq!(game.dispatch("a", change.clone(), 21).unwrap(), receipt);
    assert_eq!(
        game.dispatch(
            "a",
            request(
                "plan",
                1,
                Command::SetMatchPlan {
                    plan: default.clone()
                }
            ),
            22
        ),
        Err(Error::RequestIdReused)
    );
    game.dispatch("a", request("ready", 1, Command::Ready), 30)
        .unwrap()
        .result
        .unwrap();
    assert_eq!(
        game.dispatch(
            "a",
            request(
                "after-ready",
                1,
                Command::SetMatchPlan {
                    plan: default.clone()
                }
            ),
            40
        )
        .unwrap()
        .result,
        Err(Error::AlreadyReady)
    );
    assert_eq!(
        game.dispatch(
            "b",
            request(
                "after-deadline",
                1,
                Command::SetMatchPlan {
                    plan: attacking.clone()
                }
            ),
            1000
        )
        .unwrap()
        .result,
        Err(Error::DayClosed)
    );
    assert_eq!(game.dispatch("a", change.clone(), 1000).unwrap(), receipt);
    game.advance_closed_day(1, 1000, 2000).unwrap();
    assert_eq!(game.match_plan("a").unwrap(), attacking);
    assert_eq!(game.match_plan("b").unwrap(), default);
    assert_eq!(game.dispatch("a", change, 1001).unwrap(), receipt);
    assert_eq!(
        game.dispatch(
            "a",
            request(
                "old-day",
                1,
                Command::SetMatchPlan {
                    plan: default.clone()
                }
            ),
            1001
        )
        .unwrap()
        .result,
        Err(Error::WrongDay)
    );
    assert_eq!(
        game.dispatch(
            "a",
            request(
                "new-day",
                2,
                Command::SetMatchPlan {
                    plan: default.clone()
                }
            ),
            1001
        )
        .unwrap()
        .result,
        Ok(Outcome::MatchPlanSet)
    );
    assert_eq!(game.match_plan("a").unwrap(), default);
}

#[test]
fn both_saved_match_plans_seed_the_delegated_engine() {
    use management::{
        matches::{self, DelegatedTeam},
        tactics::MatchPlan,
    };

    let home_plan = MatchPlan {
        play_style: engine::PlayStyle::HighPress,
        pressing_intensity: engine::PressingIntensity::Aggressive,
        defensive_line: engine::DefensiveLine::High,
        width: engine::TacticsPitchWidth::Wide,
        build_up_style: engine::TacticsBuildUpStyle::Short,
        marking_style: engine::MarkingStyle::ManToMan,
        tempo: engine::Tempo::Patient,
        defensive_shape: engine::DefensiveShape::Compact,
        counter_press_duration: engine::CounterPressDuration::Long,
        break_speed: engine::BreakSpeed::Fast,
    };
    let away_plan = MatchPlan {
        play_style: engine::PlayStyle::Counter,
        pressing_intensity: engine::PressingIntensity::Passive,
        defensive_line: engine::DefensiveLine::VeryLow,
        width: engine::TacticsPitchWidth::Narrow,
        build_up_style: engine::TacticsBuildUpStyle::Long,
        marking_style: engine::MarkingStyle::Mixed,
        tempo: engine::Tempo::Patient,
        defensive_shape: engine::DefensiveShape::Stretched,
        counter_press_duration: engine::CounterPressDuration::Short,
        break_speed: engine::BreakSpeed::Slow,
    };
    let mut game = setup();
    for (actor, plan) in [("a", &home_plan), ("b", &away_plan)] {
        game.dispatch(
            actor,
            request("plan", 1, Command::SetMatchPlan { plan: plan.clone() }),
            20,
        )
        .unwrap()
        .result
        .unwrap();
    }
    let team = |club: &str, plan: &MatchPlan| {
        let available: Vec<_> = (0..11).map(|i| attributes(club, i)).collect();
        let (players, bench) =
            management::selection::select(&available, &game.lineup(club).unwrap()).unwrap();
        DelegatedTeam {
            team: engine::TeamData {
                id: club.into(),
                name: club.into(),
                formation: "4-4-2".into(),
                play_style: plan.play_style,
                tactics: plan.engine_tactics(),
                players,
            },
            bench,
            profile: engine::ai::AiProfile::default(),
        }
    };
    let expected = serde_json::to_value(
        matches::play(team("a", &home_plan), team("b", &away_plan), 1001).unwrap(),
    )
    .unwrap();
    // Each side independently matters; this catches dropping either saved plan.
    for (home, away) in [
        (&MatchPlan::default(), &away_plan),
        (&home_plan, &MatchPlan::default()),
    ] {
        let missing_plan =
            serde_json::to_value(matches::play(team("a", home), team("b", away), 1001).unwrap())
                .unwrap();
        assert_ne!(expected, missing_plan);
    }
    let results = game.advance_closed_day(1, 1000, 2000).unwrap();
    assert_eq!(serde_json::to_value(&results[0].report).unwrap(), expected);
    assert_eq!(game.match_plan("a").unwrap(), home_plan);
    assert_eq!(game.match_plan("b").unwrap(), away_plan);
}

#[test]
fn changing_match_plans_does_not_invalidate_transfer_preview() {
    let mut game = setup();
    let Outcome::Offered(offer) = game
        .dispatch(
            "a",
            request(
                "bid",
                1,
                Command::Offer {
                    player_id: "b-10".into(),
                    fee: 100,
                },
            ),
            20,
        )
        .unwrap()
        .result
        .unwrap()
    else {
        panic!("expected offer")
    };
    let Outcome::Preview(preview) = game
        .dispatch(
            "b",
            request("review", 1, Command::Review { offer_id: offer.id }),
            30,
        )
        .unwrap()
        .result
        .unwrap()
    else {
        panic!("expected preview")
    };
    for actor in ["a", "b"] {
        game.dispatch(
            actor,
            request(
                "plan",
                1,
                Command::SetMatchPlan {
                    plan: management::tactics::MatchPlan {
                        play_style: engine::PlayStyle::Defensive,
                        ..Default::default()
                    },
                },
            ),
            40,
        )
        .unwrap()
        .result
        .unwrap();
    }
    assert_eq!(
        game.dispatch(
            "b",
            request(
                "confirm",
                1,
                Command::Confirm {
                    preview_id: preview.id
                }
            ),
            50
        )
        .unwrap()
        .result,
        Ok(Outcome::Transferred { offer_id: offer.id })
    );
}
