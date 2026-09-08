//! Optional source finance lifecycle. Cash and contracts stay authoritative in
//! Management; this module owns commercial accounts and dated gate receipts.
use crate::economy::{self, Context, FinanceState, Settlement, Snapshot};
use crate::football::{FinishedFixture, Football};
use crate::inbox::{ActionResolution, EffectReceipt, InboxError};
use crate::{Error, Management};
use chrono::{Datelike, Days, NaiveDate, Weekday};
use rand::{SeedableRng, rngs::StdRng};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomySetup {
    pub seed: u64,
    pub clubs: BTreeMap<String, FinanceState>,
    pub season: u32,
    #[serde(default)]
    pub completed_home_dates: BTreeMap<String, Vec<NaiveDate>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomyState {
    pub setup: EconomySetup,
    pub positions: BTreeMap<String, u32>,
    pub settlements: Vec<(NaiveDate, String, Settlement)>,
    pub last_settlement: Option<NaiveDate>,
    pub penalties: BTreeMap<String, u8>,
    #[serde(with = "crate::checkpoint::entries")]
    pub offers: BTreeMap<(String, String), economy::SponsorPitch>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EconomyCommand {
    RequestBoardSupport,
    RequestMarketingCampaign,
    RequestSponsorPitch,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EconomyOutcome {
    BoardSupport(economy::BoardSupport),
    MarketingCampaign(economy::MarketingCampaign),
    SponsorPitch(economy::SponsorPitch),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomyView {
    pub date: NaiveDate,
    pub balance: i64,
    pub account: FinanceState,
    pub snapshot: Snapshot,
    pub board_support: Option<economy::BoardSupport>,
    pub marketing_campaign: Option<economy::MarketingCampaign>,
    pub sponsor_pitch: Option<economy::SponsorPitch>,
    pub settlements: Vec<(NaiveDate, Settlement)>,
}
fn err(error: String) -> Error {
    Error::Contract(error)
}

impl Management {
    fn economy_context(&self, club: &str) -> Result<Context, String> {
        let state = self.economy.as_ref().ok_or("Economy unavailable")?;
        let career = self.career.as_ref().ok_or("Career unavailable")?;
        let mut annual = 0_i64;
        let mut weekly = 0_i64;
        for player in self.players.values() {
            let contract = &career.contracts[&player.id];
            let loan = self
                .social
                .as_ref()
                .and_then(|s| s.source_players.get(&player.id))
                .and_then(|p| p.active_loan.as_ref());
            let owner = (!player.club_id.is_empty()).then_some(player.club_id.as_str());
            let wage = economy::annual_player_wage(contract.weekly_wage, owner, loan, club);
            annual = annual.checked_add(wage).ok_or("Wage overflow")?;
            weekly = weekly.checked_add(wage / 52).ok_or("Wage overflow")?;
        }
        for wage in career.staff_annual_wages.get(club).into_iter().flatten() {
            annual = annual
                .checked_add(i64::from(*wage))
                .ok_or("Wage overflow")?;
            weekly = weekly
                .checked_add(i64::from(*wage / 52))
                .ok_or("Wage overflow")?;
        }
        let week_ago = career
            .today
            .checked_sub_days(Days::new(7))
            .ok_or("Date underflow")?;
        let recent = state
            .setup
            .completed_home_dates
            .get(club)
            .into_iter()
            .flatten()
            .filter(|date| **date > week_ago && **date <= career.today)
            .count();
        let actor = self
            .managers
            .values()
            .find(|m| m.club_id == club)
            .map(|m| m.id.as_str());
        let messages = actor
            .and_then(|a| self.social.as_ref().map(|s| s.inbox.list(a)))
            .unwrap_or_default();
        Ok(Context {
            club_id: club.into(),
            today: career.today,
            season: state.setup.season,
            current_position: state.positions.get(club).copied(),
            annual_wage_bill: annual,
            weekly_wage_spend: weekly,
            recent_home_matches: i64::try_from(recent).map_err(|_| "Match count overflow")?,
            pending_sponsor_offer: messages
                .iter()
                .any(|m| m.id.starts_with("sponsor_") && m.actions.iter().any(|a| !a.resolved)),
            sponsor_pitch_attempted_today: actor.is_some_and(|a| {
                state
                    .offers
                    .contains_key(&(a.into(), format!("sponsor_pitch_{}", career.today)))
            }),
        })
    }
    pub(crate) fn economy_finance(&self, club: &str) -> Result<(FinanceState, Snapshot), String> {
        let mut account = self
            .economy
            .as_ref()
            .ok_or("Economy unavailable")?
            .setup
            .clubs
            .get(club)
            .ok_or("Unknown economy club")?
            .clone();
        let career = self.career.as_ref().ok_or("Career unavailable")?;
        account.wage_budget =
            i64::try_from(career.wage_budgets[club]).map_err(|_| "Wage budget overflow")?;
        account.reputation = career.reputations[club];
        let snapshot = economy::snapshot(
            &account,
            self.clubs[club].balance,
            &self.economy_context(club)?,
        )?;
        Ok((account, snapshot))
    }
    pub(crate) fn economy_account_mut(&mut self, club: &str) -> Result<&mut FinanceState, String> {
        self.economy
            .as_mut()
            .ok_or("Economy unavailable")?
            .setup
            .clubs
            .get_mut(club)
            .ok_or_else(|| "Unknown economy club".into())
    }
    pub fn economy_view(&self, actor: &str) -> Result<EconomyView, Error> {
        let club = &self.managers.get(actor).ok_or(Error::Unauthorized)?.club_id;
        self.economy.as_ref().ok_or(Error::Unavailable)?;
        let context = self.economy_context(club).map_err(err)?;
        let (account, snapshot) = self.economy_finance(club).map_err(err)?;
        let balance = self.clubs[club].balance;
        Ok(EconomyView {
            date: context.today,
            balance,
            board_support: economy::preview_board_support(&account, balance, &context).ok(),
            marketing_campaign: economy::preview_marketing_campaign(&account, balance, &context)
                .ok(),
            sponsor_pitch: self
                .social
                .as_ref()
                .and_then(|_| economy::preview_sponsor_pitch(&account, balance, &context).ok()),
            settlements: self
                .economy
                .as_ref()
                .unwrap()
                .settlements
                .iter()
                .filter(|(_, id, _)| id == club)
                .rev()
                .take(12)
                .map(|(date, _, s)| (*date, s.clone()))
                .collect(),
            account,
            snapshot,
        })
    }
    pub(crate) fn execute_economy(
        &mut self,
        actor: &str,
        command: &EconomyCommand,
    ) -> Result<EconomyOutcome, Error> {
        let club = self
            .managers
            .get(actor)
            .ok_or(Error::Unauthorized)?
            .club_id
            .clone();
        if self.window.is_ready(actor) {
            return Err(Error::AlreadyReady);
        }
        let context = self.economy_context(&club).map_err(err)?;
        let (mut account, _) = self.economy_finance(&club).map_err(err)?;
        let mut balance = self.clubs[&club].balance;
        let revision = self.club_revisions[&club]
            .checked_add(1)
            .ok_or(Error::Overflow)?;
        let outcome = match command {
            EconomyCommand::RequestBoardSupport => {
                let result = economy::request_board_support(&mut account, &mut balance, &context)
                    .map_err(err)?;
                let penalty = self
                    .economy
                    .as_mut()
                    .unwrap()
                    .penalties
                    .entry(club.clone())
                    .or_default();
                *penalty = penalty.saturating_add(result.satisfaction_penalty);
                EconomyOutcome::BoardSupport(result)
            }
            EconomyCommand::RequestMarketingCampaign => {
                let result =
                    economy::request_marketing_campaign(&mut account, &mut balance, &context)
                        .map_err(err)?;
                if let Some(social) = &mut self.social {
                    social
                        .inbox
                        .deliver(actor, marketing_message(context.today, &result))
                        .map_err(|e| err(format!("{e:?}")))?;
                }
                EconomyOutcome::MarketingCampaign(result)
            }
            EconomyCommand::RequestSponsorPitch => {
                let offer =
                    economy::preview_sponsor_pitch(&account, balance, &context).map_err(err)?;
                let message =
                    sponsor_message(&offer, &club, &self.clubs[&club].name, context.today)?;
                self.social
                    .as_mut()
                    .ok_or(Error::Unavailable)?
                    .inbox
                    .deliver(actor, message)
                    .map_err(|e| err(format!("{e:?}")))?;
                self.economy
                    .as_mut()
                    .unwrap()
                    .offers
                    .insert((actor.into(), offer.message_id.clone()), offer.clone());
                EconomyOutcome::SponsorPitch(offer)
            }
        };
        self.economy
            .as_mut()
            .unwrap()
            .setup
            .clubs
            .insert(club.clone(), account);
        self.clubs.get_mut(&club).unwrap().balance = balance;
        self.club_revisions.insert(club, revision);
        Ok(outcome)
    }
    pub(crate) fn respond_economy_sponsor(
        &mut self,
        actor: &str,
        message_id: &str,
        action_id: &str,
        option: Option<&str>,
    ) -> Result<Option<ActionResolution>, Error> {
        let Some(offer) = self
            .economy
            .as_ref()
            .and_then(|s| s.offers.get(&(actor.into(), message_id.into())))
            .cloned()
        else {
            return Ok(None);
        };
        let club = self
            .managers
            .get(actor)
            .ok_or(Error::Unauthorized)?
            .club_id
            .clone();
        let mut sponsorship = None;
        let resolution = self
            .social
            .as_mut()
            .ok_or(Error::Unavailable)?
            .inbox
            .resolve_with(
                actor,
                message_id,
                action_id,
                option,
                |_, _, selected| match selected {
                    "accept" => {
                        sponsorship = Some(
                            economy::accepted_sponsorship(
                                offer.sponsor_name.clone(),
                                offer.weekly_amount as u64,
                            )
                            .map_err(InboxError::Effect)?,
                        );
                        Ok(EffectReceipt {
                            i18n_key: "be.msg.sponsor.effects.accepted".into(),
                            i18n_params: BTreeMap::from([(
                                "amount".into(),
                                offer.weekly_amount.to_string(),
                            )]),
                        })
                    }
                    "decline" => Ok(EffectReceipt {
                        i18n_key: "be.msg.sponsor.effects.declined".into(),
                        i18n_params: BTreeMap::new(),
                    }),
                    _ => Err(InboxError::UnsupportedAction),
                },
            )
            .map_err(|e| err(format!("{e:?}")))?;
        if let Some(sponsor) = sponsorship {
            let revision = self.club_revisions[&club]
                .checked_add(1)
                .ok_or(Error::Overflow)?;
            self.economy_account_mut(&club).map_err(err)?.sponsorship = Some(sponsor);
            self.club_revisions.insert(club, revision);
        }
        Ok(Some(resolution))
    }
    /// Called after closing-date expiry has released players, before date advance.
    pub(crate) fn settle_economy(&mut self) -> Result<(), Error> {
        let Some(state) = self.economy.as_ref() else {
            return Ok(());
        };
        let today = self.career_date().ok_or(Error::Unavailable)?;
        if today.weekday() != Weekday::Mon || state.last_settlement == Some(today) {
            return Ok(());
        }
        let mut rng = StdRng::seed_from_u64(
            state.setup.seed ^ today.num_days_from_ce() as u64 ^ 0x6669_6e61_6e63_6573,
        );
        let clubs: Vec<_> = self.clubs.keys().cloned().collect();
        for club in clubs {
            let context = self.economy_context(&club).map_err(err)?;
            let (mut account, _) = self.economy_finance(&club).map_err(err)?;
            let career = self.career.as_ref().unwrap();
            let actual = self
                .players
                .values()
                .filter(|p| p.club_id == club)
                .map(|p| career.contracts[&p.id].weekly_wage)
                .chain(
                    career
                        .staff_annual_wages
                        .get(&club)
                        .into_iter()
                        .flatten()
                        .copied(),
                )
                .try_fold(0_i64, |sum, w| {
                    sum.checked_add(i64::from(w / 52)).ok_or(Error::Overflow)
                })?;
            let mut balance = self.clubs[&club].balance;
            let settlement =
                economy::settle_weekly(&mut account, &mut balance, &context, actual, &mut rng)
                    .map_err(err)?
                    .ok_or(Error::Unavailable)?;
            if let (Some(warning), Some(social)) = (&settlement.warning, &mut self.social) {
                for manager in self.managers.values().filter(|m| m.club_id == club) {
                    social
                        .inbox
                        .deliver(&manager.id, warning_message(today, warning))
                        .map_err(|e| err(format!("{e:?}")))?;
                }
            }
            let revision = self.club_revisions[&club]
                .checked_add(1)
                .ok_or(Error::Overflow)?;
            self.clubs.get_mut(&club).unwrap().balance = balance;
            self.club_revisions.insert(club.clone(), revision);
            let state = self.economy.as_mut().unwrap();
            state.setup.clubs.insert(club.clone(), account);
            let penalty = state.penalties.entry(club.clone()).or_default();
            *penalty = penalty.saturating_add(settlement.satisfaction_penalty);
            state.settlements.push((today, club, settlement));
        }
        self.economy.as_mut().unwrap().last_settlement = Some(today);
        Ok(())
    }
    pub(crate) fn validate_economy_checkpoint(&self) -> Result<(), String> {
        let Some(state) = &self.economy else {
            return Ok(());
        };
        let career = self.career.as_ref().ok_or("Economy requires career")?;
        if state.setup.clubs.keys().ne(self.clubs.keys()) || state.setup.season == 0 {
            return Err("Economy clubs/season mismatch".into());
        }
        if state.setup.completed_home_dates.iter().any(|(id, dates)| {
            !self.clubs.contains_key(id) || dates.iter().any(|d| *d > career.today)
        }) {
            return Err("Invalid economy history".into());
        }
        if state
            .last_settlement
            .is_some_and(|d| d > career.today || d.weekday() != Weekday::Mon)
        {
            return Err("Invalid economy settlement date".into());
        }
        for id in self.clubs.keys() {
            self.economy_finance(id)?;
        }
        for ((actor, id), offer) in &state.offers {
            if id != &offer.message_id
                || offer.weekly_amount < 0
                || actor.is_empty()
                || self.social.is_none()
            {
                return Err("Invalid sponsor offer".into());
            }
        }
        Ok(())
    }

    /// Cash prizes were posted by the season transaction. Preserve source's
    /// cumulative season totals (despite their names, it never resets them).
    pub(crate) fn settle_economy_season(
        &mut self,
        date: NaiveDate,
        closing_season: u32,
        next_season: u32,
        prizes: &BTreeMap<String, i64>,
    ) -> Result<(), String> {
        let Some(state) = &mut self.economy else {
            return Ok(());
        };
        for (club, amount) in prizes {
            let account = state
                .setup
                .clubs
                .get_mut(club)
                .ok_or("Unknown prize club")?;
            let position = *state.positions.get(club).ok_or("Missing final position")?;
            let suffix = match position {
                1 => "st",
                2 => "nd",
                3 => "rd",
                _ => "th",
            };
            account.season_income = account
                .season_income
                .checked_add(*amount)
                .ok_or("Season income overflow")?;
            account.financial_ledger.push(domain::team::FinancialTransaction {date:date.to_string(),description:format!("be.msg.seasonPayout.ledgerDescription?season={closing_season}&position={position}&suffix={suffix}"),amount:*amount,kind:domain::team::FinancialTransactionKind::PrizeMoney});
            account.transfer_budget = ((self.clubs[club].balance as f64 * 0.15) as i64).max(0);
            account.form.clear();
        }
        state.setup.season = next_season;
        // The source replaces its primary league; its previous fixtures no
        // longer contribute next week's attendance estimate.
        state.setup.completed_home_dates.clear();
        state.positions.clear();
        Ok(())
    }
}

impl Football {
    pub fn configure_economy(&mut self, setup: EconomySetup) -> Result<(), String> {
        if self.started || self.management.sequence != 0 || self.management.economy.is_some() {
            return Err("Economy must be configured once before play".into());
        }
        let career = self
            .management
            .career
            .as_ref()
            .ok_or("Economy requires career")?;
        for (id, account) in &setup.clubs {
            if career
                .wage_budgets
                .get(id)
                .and_then(|w| i64::try_from(*w).ok())
                != Some(account.wage_budget)
                || career.reputations.get(id) != Some(&account.reputation)
            {
                return Err("Economy/career budgets or reputation mismatch".into());
            }
        }
        let mut staged = self.management.clone();
        staged.economy = Some(EconomyState {
            setup,
            positions: BTreeMap::new(),
            settlements: vec![],
            last_settlement: None,
            penalties: BTreeMap::new(),
            offers: BTreeMap::new(),
        });
        staged.validate_economy_checkpoint()?;
        self.management = staged;
        Ok(())
    }
    pub fn economy_view(&self, actor: &str) -> Result<EconomyView, Error> {
        self.management.economy_view(actor)
    }
    pub(crate) fn prepare_economy_day(
        &mut self,
        results: &[FinishedFixture],
    ) -> Result<(), String> {
        if self.management.economy.is_none() {
            return Ok(());
        }
        let today = self
            .management
            .career_date()
            .ok_or("Economy requires career")?;
        // Each club's financial preview uses its domestic league context, not
        // the spectator's primary table. Cup scores affect form and attendance,
        // but must never award league-table points.
        let mut tables = Vec::new();
        let mut fixture_tables = BTreeMap::new();
        if let Some(competitions) = &self.competitions {
            for competition in competitions.setup.competitions.values().filter(|c| {
                c.kind == domain::league::CompetitionType::League
                    && c.scope == domain::league::CompetitionScope::Domestic
            }) {
                let index = tables.len();
                for fixture in competition
                    .fixtures
                    .iter()
                    .filter(|f| f.counts_for_league_standings())
                {
                    fixture_tables.insert(fixture.id.clone(), index);
                }
                tables.push(
                    competition
                        .standings
                        .iter()
                        .map(|row| {
                            (
                                row.team_id.clone(),
                                crate::football::Standing {
                                    club_id: row.team_id.clone(),
                                    played: row.played,
                                    won: row.won,
                                    drawn: row.drawn,
                                    lost: row.lost,
                                    goals_for: row.goals_for,
                                    goals_against: row.goals_against,
                                    points: row.points,
                                },
                            )
                        })
                        .collect::<BTreeMap<_, _>>(),
                );
            }
        } else {
            tables.push(self.standings.clone());
            for result in results {
                fixture_tables.insert(result.fixture_id.clone(), 0);
            }
        }
        let state = self.management.economy.as_mut().unwrap();
        if let Some(seasons) = &self.seasons {
            state.setup.season = seasons.setup.season;
        }
        for result in results {
            state
                .setup
                .completed_home_dates
                .entry(result.home.clone())
                .or_default()
                .push(today);
            for (id, gf, ga) in [
                (
                    &result.home,
                    result.report.home_goals,
                    result.report.away_goals,
                ),
                (
                    &result.away,
                    result.report.away_goals,
                    result.report.home_goals,
                ),
            ] {
                let form = &mut state
                    .setup
                    .clubs
                    .get_mut(id)
                    .ok_or("Unknown result club")?
                    .form;
                form.push(
                    if gf > ga {
                        "W"
                    } else if gf == ga {
                        "D"
                    } else {
                        "L"
                    }
                    .into(),
                );
                if form.len() > 5 {
                    form.remove(0);
                }
                let Some(index) = fixture_tables.get(&result.fixture_id) else {
                    continue;
                };
                let row = tables[*index]
                    .get_mut(id)
                    .ok_or("Missing domestic standing")?;
                row.points += if gf > ga {
                    3
                } else if gf == ga {
                    1
                } else {
                    0
                };
                row.goals_for += u32::from(gf);
                row.goals_against += u32::from(ga);
            }
        }
        state.positions.clear();
        for table in &tables {
            let mut rows: Vec<_> = table.values().collect();
            rows.sort_by(|a, b| {
                b.points
                    .cmp(&a.points)
                    .then_with(|| {
                        (i64::from(b.goals_for) - i64::from(b.goals_against))
                            .cmp(&(i64::from(a.goals_for) - i64::from(a.goals_against)))
                    })
                    .then_with(|| b.goals_for.cmp(&a.goals_for))
                    .then_with(|| a.club_id.cmp(&b.club_id))
            });
            state.positions.extend(
                rows.iter()
                    .enumerate()
                    .map(|(i, row)| (row.club_id.clone(), i as u32 + 1)),
            );
        }
        Ok(())
    }
    pub(crate) fn apply_economy_board_penalties(&mut self) -> Result<(), String> {
        let Some(state) = self.management.economy.as_mut() else {
            return Ok(());
        };
        let penalties = std::mem::take(&mut state.penalties);
        if let Some(boards) = &mut self.boards {
            for manager in self.management.managers.values() {
                if let Some(penalty) = penalties.get(&manager.club_id) {
                    let board = boards.get_mut(&manager.id).ok_or("Missing board")?;
                    board.state.satisfaction = board.state.satisfaction.saturating_sub(*penalty);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod context_tests {
    use super::*;
    #[test]
    fn lower_division_match_forecasts_own_table_and_cup_scores_only_affect_form() {
        let ids = ["a", "b", "c", "d"];
        let mut m = Management::new(
            ids.map(|id| crate::Club {
                id: id.into(),
                name: id.into(),
                balance: 100_000,
            })
            .to_vec(),
            vec![],
            ids.map(|id| crate::Manager {
                id: id.into(),
                club_id: id.into(),
            })
            .to_vec(),
            1,
            100,
        )
        .unwrap();
        let today = "2026-08-01".parse().unwrap();
        m.configure_career(crate::career::CareerSetup {
            today,
            contracts: Default::default(),
            wage_budgets: ids.map(|id| (id.into(), 10_000)).into(),
            reputations: ids.map(|id| (id.into(), 500)).into(),
            staff_annual_wages: Default::default(),
        })
        .unwrap();
        let mut game = Football::new(m, vec![], vec![]).unwrap();
        game.configure_economy(EconomySetup {
            seed: 1,
            season: 2026,
            completed_home_dates: Default::default(),
            clubs: ids
                .map(|id| {
                    (
                        id.into(),
                        FinanceState {
                            wage_budget: 10_000,
                            transfer_budget: 10_000,
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
        let upper = domain::league::League::new(
            "upper".into(),
            "Upper".into(),
            2026,
            &["a".into(), "b".into()],
        );
        let mut lower = domain::league::League::new(
            "lower".into(),
            "Lower".into(),
            2026,
            &["c".into(), "d".into()],
        );
        lower.fixtures.push(domain::league::Fixture {
            id: "lower-match".into(),
            home_team_id: "c".into(),
            away_team_id: "d".into(),
            date: today.to_string(),
            ..Default::default()
        });
        game.standings.retain(|id, _| id == "a" || id == "b");
        game.competitions = Some(crate::competitions::CompetitionState {
            epoch_date: today,
            epoch_day: 1,
            archives: vec![],
            setup: crate::competitions::CompetitionSetup {
                seed: 1,
                primary_competition_id: "upper".into(),
                competitions: [("upper".into(), upper), ("lower".into(), lower)].into(),
                active_competition_ids: ["upper".into(), "lower".into()].into(),
                competition_order: vec!["upper".into(), "lower".into()],
                catch_up_past: false,
                club_regions: Default::default(),
            },
        });
        let mut report = engine::MatchReport::from_events(vec![], 1, 1, 90);
        report.away_goals = 1;
        let result = FinishedFixture {
            fixture_id: "lower-match".into(),
            day: 1,
            home: "c".into(),
            away: "d".into(),
            home_starting_xi: vec![],
            away_starting_xi: vec![],
            report,
        };
        game.prepare_economy_day(&[result.clone()]).unwrap();
        assert_eq!(game.management.economy.as_ref().unwrap().positions["d"], 1);
        assert_eq!(game.management.economy.as_ref().unwrap().positions["c"], 2);
        assert_eq!(
            game.management
                .economy
                .as_ref()
                .unwrap()
                .setup
                .completed_home_dates["c"],
            vec![today]
        );
        let lower = game
            .competitions
            .as_mut()
            .unwrap()
            .setup
            .competitions
            .get_mut("lower")
            .unwrap();
        lower.standings[0].record_result(0, 1);
        lower.standings[1].record_result(1, 0);
        let mut cup = result;
        cup.fixture_id = "cup-match".into();
        cup.report.home_goals = 9;
        cup.report.away_goals = 0;
        game.prepare_economy_day(&[cup]).unwrap();
        // Nine cup goals must not reverse the league position.
        assert_eq!(game.management.economy.as_ref().unwrap().positions["c"], 2);
        assert_eq!(
            game.management.economy.as_ref().unwrap().setup.clubs["c"].form,
            vec!["L", "W"]
        );
        assert_eq!(game.management.economy.as_ref().unwrap().positions["a"], 1);
    }
}

fn sponsor_message(
    offer: &economy::SponsorPitch,
    club: &str,
    name: &str,
    today: NaiveDate,
) -> Result<domain::message::InboxMessage, Error> {
    use domain::message::*;
    Ok(InboxMessage::new(
        offer.message_id.clone(),
        String::new(),
        String::new(),
        String::new(),
        today.to_string(),
    )
    .with_category(MessageCategory::Finance)
    .with_priority(MessagePriority::Normal)
    .with_sender_role("")
    .with_context(MessageContext {
        team_id: Some(club.into()),
        ..Default::default()
    })
    .with_action(MessageAction {
        id: "respond".into(),
        label: String::new(),
        label_key: Some("be.msg.event.respond".into()),
        resolved: false,
        action_type: ActionType::ChooseOption {
            options: ["accept", "decline"]
                .into_iter()
                .map(|id| ActionOption {
                    id: id.into(),
                    label: String::new(),
                    description: String::new(),
                    label_key: Some(format!("be.msg.sponsor.options.{id}.label")),
                    description_key: Some(format!("be.msg.sponsor.options.{id}.description")),
                })
                .collect(),
        },
    })
    .with_i18n(
        "be.msg.sponsor.subject",
        "be.msg.sponsor.body",
        std::collections::HashMap::from([
            ("sponsor".into(), offer.sponsor_name.clone()),
            ("team".into(), name.into()),
            ("amount".into(), offer.weekly_amount.to_string()),
        ]),
    )
    .with_sender_i18n("be.sender.commercialDirector", "be.role.commercialDirector"))
}

fn warning_message(today: NaiveDate, warning: &economy::Warning) -> domain::message::InboxMessage {
    use domain::message::*;
    use std::collections::HashMap;
    let money = |amount: u64| {
        let amount = (amount as f64).round().clamp(0.0, u64::MAX as f64) as u64;
        if amount >= 1_000_000 {
            format!("{:.1}M", amount as f64 / 1_000_000.0)
        } else if amount >= 1_000 {
            format!("{}K", amount / 1_000)
        } else {
            amount.to_string()
        }
    };
    let (id, key, priority, sender, role, params) = match warning {
        economy::Warning::Debt { amount } => (
            "finance_critical",
            "financeCritical",
            MessagePriority::Urgent,
            "boardOfDirectors",
            "chairman",
            HashMap::from([("amount".into(), money(*amount))]),
        ),
        economy::Warning::Runway {
            weekly_wages,
            weeks_left,
        } => (
            "finance_warning",
            "financeWarning",
            MessagePriority::High,
            "financialDirector",
            "financialDirector",
            HashMap::from([
                ("weeklyWages".into(), money(*weekly_wages as u64)),
                ("weeksLeft".into(), weeks_left.to_string()),
            ]),
        ),
        economy::Warning::OverBudget {
            annual_wages,
            wage_budget,
        } => (
            "wage_over_budget",
            "wageOverBudget",
            MessagePriority::Normal,
            "financialDirector",
            "financialDirector",
            HashMap::from([
                ("annualWages".into(), money(*annual_wages as u64)),
                ("wageBudget".into(), money(*wage_budget as u64)),
            ]),
        ),
    };
    InboxMessage::new(
        format!("{id}_{today}"),
        String::new(),
        String::new(),
        String::new(),
        today.to_string(),
    )
    .with_category(MessageCategory::Finance)
    .with_priority(priority)
    .with_sender_role("")
    .with_i18n(
        &format!("be.msg.{key}.subject"),
        &format!("be.msg.{key}.body"),
        params,
    )
    .with_sender_i18n(&format!("be.sender.{sender}"), &format!("be.role.{role}"))
    .with_action(MessageAction {
        id: "view_finances".into(),
        label: String::new(),
        label_key: Some("be.msg.action.viewFinances".into()),
        resolved: false,
        action_type: ActionType::NavigateTo {
            route: "/dashboard?tab=Finances".into(),
        },
    })
}

fn marketing_message(
    today: NaiveDate,
    result: &economy::MarketingCampaign,
) -> domain::message::InboxMessage {
    use domain::message::*;
    InboxMessage::new(
        format!("marketing_campaign_{today}"),
        String::new(),
        String::new(),
        String::new(),
        today.to_string(),
    )
    .with_category(MessageCategory::Finance)
    .with_priority(MessagePriority::Normal)
    .with_sender_role("")
    .with_i18n(
        "be.msg.marketingCampaign.subject",
        "be.msg.marketingCampaign.body",
        std::collections::HashMap::from([
            ("grossRevenue".into(), result.gross_revenue.to_string()),
            ("campaignCost".into(), result.campaign_cost.to_string()),
            ("netIncome".into(), result.net_income.to_string()),
            ("days".into(), result.cooldown_days.to_string()),
        ]),
    )
    .with_sender_i18n("be.sender.commercialDirector", "be.role.commercialDirector")
    .with_action(MessageAction {
        id: "ack".into(),
        label: String::new(),
        label_key: Some("be.msg.event.ack".into()),
        resolved: false,
        action_type: ActionType::Acknowledge,
    })
}
