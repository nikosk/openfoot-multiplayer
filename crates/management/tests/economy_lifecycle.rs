use chrono::NaiveDate;
use management::career::CareerSetup;
use management::economy::FinanceState;
use management::economy_runtime::{EconomyCommand, EconomySetup};
use management::football::Football;
use management::social::SocialCommand;
use management::{Club, Command, Error, Management, Manager, Outcome, Request};
use std::collections::BTreeMap;

#[test]
fn attendance_uses_exclusive_seven_day_boundary_and_is_seed_reproducible() {
    let mut saved = game(10_000, false).save_state().unwrap();
    saved["state"]["management"]["economy"]["setup"]["completed_home_dates"] =
        serde_json::json!({"a":["2025-12-29","2025-12-30","2026-01-05"]});
    let mut first = Football::load_validated(saved.clone()).unwrap();
    let mut second = Football::load_validated(saved).unwrap();
    first.advance_closed_day(1, 100, 200).unwrap();
    second.advance_closed_day(1, 100, 200).unwrap();
    let view = first.economy_view("a").unwrap();
    let settlement = &view.settlements[0].1;
    let expected = management::economy::calc_matchday(
        10_000,
        2,
        f64::from(settlement.attendance_percent.unwrap()) / 100.0,
        f64::from(settlement.average_ticket.unwrap()),
    )
    .unwrap();
    assert_eq!(settlement.matchday_income, expected);
    assert_eq!(view.balance, 9_000 + expected);
    assert_eq!(first.economy_view("b").unwrap().balance, 9_000);
    assert_eq!(first.save_state().unwrap(), second.save_state().unwrap());
}

#[test]
fn a_late_club_overflow_rolls_back_the_entire_financial_day() {
    let mut saved = game(10_000, false).save_state().unwrap();
    saved["state"]["management"]["economy"]["setup"]["clubs"]["b"]["season_expenses"] =
        serde_json::json!(i64::MAX);
    let mut game = Football::load_validated(saved).unwrap();
    let before = game.save_state().unwrap();
    assert!(game.advance_closed_day(1, 100, 200).is_err());
    assert_eq!(game.save_state().unwrap(), before);
}

fn date(s: &str) -> NaiveDate {
    s.parse().unwrap()
}
fn game(balance: i64, sponsor: bool) -> Football {
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
    let mut management = Management::new(clubs, vec![], managers, 1, 100).unwrap();
    management
        .configure_career(CareerSetup {
            today: date("2026-01-05"),
            contracts: BTreeMap::new(),
            wage_budgets: ["a", "b"].map(|id| (id.into(), 52_000)).into(),
            reputations: ["a", "b"].map(|id| (id.into(), 50)).into(),
            staff_annual_wages: ["a", "b"].map(|id| (id.into(), vec![52_000])).into(),
        })
        .unwrap();
    let mut game = Football::new(management, vec![], vec![]).unwrap();
    game.configure_social(BTreeMap::new(), 3).unwrap();
    game.configure_economy(EconomySetup {
        seed: 77,
        season: 1,
        completed_home_dates: BTreeMap::new(),
        clubs: ["a", "b"]
            .map(|id| {
                (
                    id.into(),
                    FinanceState {
                        wage_budget: 52_000,
                        transfer_budget: 1_000_000,
                        season_income: 0,
                        season_expenses: 0,
                        sponsorship: sponsor.then(|| {
                            management::economy::accepted_sponsorship("Partner".into(), 500)
                                .unwrap()
                        }),
                        financial_ledger: vec![],
                        reputation: 50,
                        stadium_capacity: 10_000,
                        form: vec![],
                    },
                )
            })
            .into(),
    })
    .unwrap();
    game
}
fn command(game: &mut Football, actor: &str, id: &str, command: Command) -> Result<Outcome, Error> {
    game.dispatch(
        actor,
        Request {
            id: id.into(),
            day: 1,
            command,
        },
        1,
    )?
    .result
}

#[test]
fn monday_settles_once_without_legacy_double_payroll_and_checkpoint_replays() {
    let mut game = game(10_000, true);
    game.advance_closed_day(1, 100, 200).unwrap();
    let view = game.economy_view("a").unwrap();
    assert_eq!(view.balance, 9_500);
    assert_eq!(view.account.season_expenses, 1_000);
    assert_eq!(view.account.season_income, 500);
    assert_eq!(view.account.sponsorship.unwrap().remaining_weeks, 11);
    assert_eq!(view.settlements.len(), 1);
    let mut restored = Football::load_validated(game.save_state().unwrap()).unwrap();
    assert!(game.advance_closed_day(1, 100, 200).is_err());
    game.advance_closed_day(2, 200, 300).unwrap();
    restored.advance_closed_day(2, 200, 300).unwrap();
    assert_eq!(game.save_state().unwrap(), restored.save_state().unwrap());
    assert_eq!(game.economy_view("a").unwrap().balance, 9_500);
}

#[test]
fn support_and_marketing_are_scoped_receipted_and_cooldown_protected() {
    let mut game = game(-50_000, false);
    assert!(matches!(
        game.economy_view("outsider"),
        Err(Error::Unauthorized)
    ));
    let request = Command::Economy(EconomyCommand::RequestBoardSupport);
    let first = command(&mut game, "a", "support", request.clone()).unwrap();
    assert_eq!(first, command(&mut game, "a", "support", request).unwrap());
    assert_eq!(game.economy_view("a").unwrap().balance, 150_000);
    assert_eq!(game.economy_view("b").unwrap().balance, -50_000);
    assert!(
        command(
            &mut game,
            "a",
            "support2",
            Command::Economy(EconomyCommand::RequestBoardSupport)
        )
        .is_err()
    );
    command(
        &mut game,
        "b",
        "marketing",
        Command::Economy(EconomyCommand::RequestMarketingCampaign),
    )
    .unwrap();
    let view = game.economy_view("b").unwrap();
    assert_eq!(view.account.financial_ledger.len(), 2);
    assert_eq!(view.snapshot.marketing_campaign_cooldown_days_remaining, 28);
    assert!(
        command(
            &mut game,
            "b",
            "marketing2",
            Command::Economy(EconomyCommand::RequestMarketingCampaign)
        )
        .is_err()
    );
    command(&mut game, "b", "ready", Command::Ready).unwrap();
    assert!(matches!(
        command(
            &mut game,
            "b",
            "late",
            Command::Economy(EconomyCommand::RequestBoardSupport)
        ),
        Err(Error::AlreadyReady)
    ));
}

#[test]
fn sponsor_acceptance_is_private_no_instant_cash_and_no_replay_reset() {
    let mut game = game(-100, false);
    command(
        &mut game,
        "a",
        "pitch",
        Command::Economy(EconomyCommand::RequestSponsorPitch),
    )
    .unwrap();
    let message_id = "sponsor_pitch_2026-01-05";
    let response = Command::Social(SocialCommand::Respond {
        message_id: message_id.into(),
        action_id: "respond".into(),
        option_id: Some("accept".into()),
    });
    assert!(command(&mut game, "b", "steal", response.clone()).is_err());
    command(&mut game, "a", "accept", response.clone()).unwrap();
    let before = game.economy_view("a").unwrap();
    assert_eq!(before.balance, -100);
    assert_eq!(before.account.sponsorship.unwrap().remaining_weeks, 12);
    assert!(
        game.economy_view("b")
            .unwrap()
            .account
            .sponsorship
            .is_none()
    );
    game.advance_closed_day(1, 100, 200).unwrap();
    game.dispatch(
        "a",
        Request {
            id: "repeat-response".into(),
            day: 2,
            command: response,
        },
        101,
    )
    .unwrap()
    .result
    .unwrap();
    assert_eq!(
        game.economy_view("a")
            .unwrap()
            .account
            .sponsorship
            .unwrap()
            .remaining_weeks,
        11
    );
}
