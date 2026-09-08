//! Seeded source news templates and causal daily delivery. News is derived from
//! committed facts; it never changes match/player statistics or narrates models.
//! Source OpenFoot Manager 64677fee, GPL-3.0-or-later.
use crate::football::Football;
use chrono::{Datelike, NaiveDate};
use domain::{
    league::{FixtureCompetition, League, TransferRumour},
    message::*,
    news::NewsArticle,
    player::Player,
    team::Team,
};
use rand::{SeedableRng, rngs::StdRng};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
#[path = "news_currency.rs"]
mod currency;
#[path = "news_digest.rs"]
mod digest;
#[path = "news_prematch.rs"]
mod prematch;
#[path = "news_result.rs"]
mod result_message;
#[path = "news_templates.rs"]
pub mod templates;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsState {
    pub seed: u64,
    pub articles: Vec<NewsArticle>,
    pub rumours: Vec<TransferRumour>,
    pub protected_clubs: BTreeSet<String>,
    pub last_processed: Option<NaiveDate>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NewsSetup {
    pub seed: u64,
    #[serde(default)]
    pub articles: Vec<NewsArticle>,
    pub protected_clubs: BTreeSet<String>,
}
struct NewsClock {
    current_date: chrono::DateTime<chrono::Utc>,
}
/// Ephemeral read-only projection, not a second authoritative game or manager
/// loop. Only generated article/rumour outputs are returned to durable state.
struct NewsContext {
    clock: NewsClock,
    league: Option<League>,
    teams: Vec<Team>,
    players: Vec<Player>,
    news: Vec<NewsArticle>,
    protected_clubs: BTreeSet<String>,
}
fn params(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}
fn action(id: &str, label: &str, key: &str, action_type: ActionType) -> MessageAction {
    MessageAction {
        id: id.into(),
        label: label.into(),
        action_type,
        resolved: false,
        label_key: Some(key.into()),
    }
}
fn publish(articles: &mut Vec<NewsArticle>, article: NewsArticle) {
    if !articles.iter().any(|a| a.id == article.id) {
        articles.push(article);
    }
}

impl Football {
    pub fn configure_news(&mut self, seed: u64) -> Result<(), String> {
        self.configure_news_setup(NewsSetup {
            seed,
            articles: vec![],
            protected_clubs: BTreeSet::new(),
        })
    }
    pub fn configure_news_setup(&mut self, setup: NewsSetup) -> Result<(), String> {
        if self.started
            || self.management.sequence != 0
            || self.management.news.is_some()
            || self.management.personnel.is_none()
            || self.management.social.is_none()
        {
            return Err("News requires personnel/social before commands".into());
        }
        if setup
            .protected_clubs
            .iter()
            .any(|id| !self.management.clubs.contains_key(id))
            || setup
                .articles
                .iter()
                .any(|article| article.id.trim().is_empty())
            || setup
                .articles
                .iter()
                .map(|article| &article.id)
                .collect::<BTreeSet<_>>()
                .len()
                != setup.articles.len()
        {
            return Err("Invalid protected clubs or imported news identities".into());
        }
        self.management.news = Some(NewsState {
            seed: setup.seed,
            articles: setup.articles,
            rumours: vec![],
            protected_clubs: setup.protected_clubs,
            last_processed: None,
        });
        Ok(())
    }
    pub fn news_view(&self, offset: usize, limit: usize) -> Vec<NewsArticle> {
        self.management
            .news
            .as_ref()
            .map(|s| {
                s.articles
                    .iter()
                    .rev()
                    .filter(|article| {
                        let date = NaiveDate::parse_from_str(&article.date, "%Y-%m-%d")
                            .ok()
                            .or_else(|| {
                                chrono::DateTime::parse_from_rfc3339(&article.date)
                                    .ok()
                                    .map(|d| d.date_naive())
                            });
                        date.zip(self.career_date())
                            .is_some_and(|(date, today)| date <= today)
                    })
                    .skip(offset)
                    .take(limit.min(100))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
    pub(crate) fn advance_news(&mut self, today: NaiveDate) -> Result<(), String> {
        let Some(state) = &self.management.news else {
            return Ok(());
        };
        if state
            .last_processed
            .is_some_and(|last| last.succ_opt() != Some(today))
        {
            return Err("News daily sweep must be consecutive".into());
        }
        let mut state = state.clone();
        let players = self.project_source_players()?;
        let personnel = self
            .management
            .personnel
            .as_ref()
            .ok_or("News personnel missing")?;
        let mut teams = personnel.teams.clone();
        for (id, team) in &mut teams {
            if let Some(reputation) = self
                .management
                .career
                .as_ref()
                .and_then(|c| c.reputations.get(id))
            {
                team.reputation = *reputation;
            }
            if let Some(account) = self
                .management
                .economy
                .as_ref()
                .and_then(|e| e.setup.clubs.get(id))
            {
                team.form = account.form.clone();
            }
        }
        let name = |id: &str| {
            teams
                .get(id)
                .map(|t| t.name.clone())
                .unwrap_or_else(|| id.into())
        };
        let date = today.and_hms_opt(12, 0, 0).unwrap().and_utc();
        let stamp = date.to_rfc3339();
        let mut rng = StdRng::seed_from_u64(
            state.seed ^ today.num_days_from_ce() as u64 ^ 0x6e65_7773_5f64_6179,
        );
        let source_fixtures = self
            .competitions
            .as_ref()
            .map(|c| {
                c.setup
                    .competitions
                    .values()
                    .flat_map(|league| league.fixtures.iter())
                    .map(|f| (f.id.clone(), f.clone()))
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        for result in &self.results {
            for (actor, manager) in &self.management.managers {
                if manager.club_id != result.home && manager.club_id != result.away {
                    continue;
                }
                let inbox = &mut self.management.social.as_mut().unwrap().inbox;
                if !inbox.contains_delivery(actor, &format!("result_{}", result.fixture_id)) {
                    let message = result_message::match_result_message(
                        &result.fixture_id,
                        &name(&result.home),
                        &name(&result.away),
                        result.report.home_goals,
                        result.report.away_goals,
                        &result.home,
                        &result.away,
                        &manager.club_id,
                        source_fixtures
                            .get(&result.fixture_id)
                            .map_or(result.day, |f| f.matchday),
                        &stamp,
                        &mut rng,
                    );
                    inbox
                        .deliver(actor, message)
                        .map_err(|e| format!("{e:?}"))?;
                }
            }
            let id = format!("report_{}", result.fixture_id);
            if state.articles.iter().any(|a| a.id == id) {
                continue;
            }
            let fixture = source_fixtures.get(&result.fixture_id);
            let scorers = |side| {
                result
                    .report
                    .goals
                    .iter()
                    .filter(|g| g.side == side)
                    .map(|g| {
                        (
                            players
                                .get(&g.scorer_id)
                                .map(|p| p.match_name.clone())
                                .unwrap_or_else(|| g.scorer_id.clone()),
                            u32::from(g.minute),
                        )
                    })
                    .collect::<Vec<_>>()
            };
            let mut article = templates::match_report_article(
                &result.fixture_id,
                &name(&result.home),
                &name(&result.away),
                result.report.home_goals,
                result.report.away_goals,
                &result.home,
                &result.away,
                fixture
                    .map(|f| f.competition.clone())
                    .unwrap_or(FixtureCompetition::League),
                fixture.map_or(result.day, |f| f.matchday),
                &scorers(engine::Side::Home),
                &scorers(engine::Side::Away),
                &stamp,
                &mut rng,
            );
            // Source template stores scorer names in this ID field; correct the
            // reference projection to actual engine IDs without changing prose.
            article.player_ids = result
                .report
                .goals
                .iter()
                .map(|g| g.scorer_id.clone())
                .collect();
            publish(&mut state.articles, article);
        }
        if let Some(competition) = &self.competitions {
            let primary =
                &competition.setup.competitions[&competition.setup.primary_competition_id];
            let prefix = format!("competition:{}:season{}:", primary.id, primary.season);
            let mut league = primary.clone();
            league.transfer_rumours = state.rumours.clone();
            if let Some(market) = &self.management.market {
                league.transfer_log = market.completed.clone();
            }
            let mut context = NewsContext {
                clock: NewsClock { current_date: date },
                league: Some(league),
                teams: teams.values().cloned().collect(),
                players: players.values().cloned().collect(),
                news: state
                    .articles
                    .iter()
                    .map(|article| {
                        let mut a = article.clone();
                        if let Some(id) = a.id.strip_prefix(&prefix) {
                            a.id = id.into();
                        }
                        a
                    })
                    .collect(),
                protected_clubs: state.protected_clubs.clone(),
            };
            let old_len = context.news.len();
            digest::generate_matchday_news(&mut context, &today.to_string(), &mut rng);
            digest::generate_weekly_digest_news(&mut context, &today.to_string(), &mut rng);
            state.rumours = context.league.unwrap().transfer_rumours;
            for mut article in context.news.into_iter().skip(old_len) {
                article.id = format!("{prefix}{}", article.id);
                publish(&mut state.articles, article);
            }
        }
        // Recipient-owned pre-match reminders are exactly three calendar days
        // ahead; all fixture categories and both managers share the same rule.
        let target = today + chrono::Days::new(3);
        // The daily hook runs after the career clock advances but before the
        // management day does: the window still identifies this closing day.
        let target_day = self
            .management
            .window
            .day
            .checked_add(3)
            .ok_or("News target day overflow")?;
        for (actor, manager) in &self.management.managers {
            for fixture in self.fixtures.iter().filter(|f| {
                f.day == target_day && (f.home == manager.club_id || f.away == manager.club_id)
            }) {
                let inbox = &mut self.management.social.as_mut().unwrap().inbox;
                if inbox.contains_delivery(actor, &format!("prematch_{}", fixture.id)) {
                    continue;
                }
                let home = fixture.home == manager.club_id;
                let opponent = if home { &fixture.away } else { &fixture.home };
                let message = prematch::pre_match_message(
                    &fixture.id,
                    &name(opponent),
                    opponent,
                    home,
                    source_fixtures
                        .get(&fixture.id)
                        .map_or(fixture.day, |f| f.matchday),
                    &target.to_string(),
                    &stamp,
                    &mut rng,
                );
                inbox
                    .deliver(actor, message)
                    .map_err(|e| format!("{e:?}"))?;
            }
        }
        if let Some(market) = &self.management.market {
            for (index, movement) in market.completed.iter().enumerate() {
                let Some(player) = players.get(&movement.player_id) else {
                    continue;
                };
                if movement.fee < 1_000_000 && player.market_value < 1_000_000 {
                    continue;
                }
                let id = format!("major-transfer:{index}:{}", movement.player_id);
                publish(
                    &mut state.articles,
                    templates::major_transfer_article(
                        &id,
                        &player.id,
                        &player.match_name,
                        &movement.from_team_id,
                        &name(&movement.from_team_id),
                        &movement.to_team_id,
                        &name(&movement.to_team_id),
                        movement.fee,
                        &movement.date,
                    ),
                );
            }
        }
        for player in players.values() {
            for (index, movement) in player
                .movement_history
                .iter()
                .enumerate()
                .filter(|(_, m)| m.kind == domain::player::PlayerMovementKind::LoanStart)
            {
                if let (Some(from), Some(to), Some(end)) = (
                    &movement.from_team_id,
                    &movement.to_team_id,
                    &movement.loan_end_date,
                ) {
                    publish(
                        &mut state.articles,
                        templates::loan_move_article(
                            &format!("loan-move:{}:{index}", player.id),
                            &player.id,
                            &player.match_name,
                            from,
                            &name(from),
                            to,
                            &name(to),
                            end,
                            &movement.date,
                        ),
                    );
                }
            }
        }
        for (club, event) in &self.management.injury_events {
            let id = format!("news:{}", event.id);
            if state.articles.iter().any(|a| a.id == id) {
                continue;
            }
            if let Some(player) = players.get(&event.player_id) {
                publish(
                    &mut state.articles,
                    templates::injury_news_article(
                        &id,
                        &player.id,
                        &player.match_name,
                        club,
                        &name(club),
                        event.injury.days_remaining,
                        &stamp,
                        &mut rng,
                    ),
                );
            }
        }
        if let Some(history) = &self.management.team_history {
            for appointment in &history.appointments {
                let manager = &history.managers[&appointment.manager_id];
                publish(
                    &mut state.articles,
                    templates::managerial_appointment_article(
                        &manager.id,
                        &manager.full_name(),
                        &appointment.club_id,
                        &name(&appointment.club_id),
                        &appointment.date.to_string(),
                    ),
                );
            }
            for (competition, seasons) in &history.competition_awards {
                for (season, awards) in seasons {
                    if let Some(mut article) =
                        templates::season_awards_article(awards, *season, &stamp)
                    {
                        article.id = format!("competition:{competition}:{}", article.id);
                        publish(&mut state.articles, article);
                    }
                }
            }
        }
        state.last_processed = Some(today);
        self.management.news = Some(state);
        Ok(())
    }
    pub(crate) fn capture_news_settlement_events(
        &mut self,
        today: NaiveDate,
    ) -> Result<(), String> {
        let Some(state) = &mut self.management.news else {
            return Ok(());
        };
        if let Some(history) = &self.management.team_history {
            for appointment in &history.appointments {
                let manager = &history.managers[&appointment.manager_id];
                let team = &self.management.clubs[&appointment.club_id];
                publish(
                    &mut state.articles,
                    templates::managerial_appointment_article(
                        &manager.id,
                        &manager.full_name(),
                        &team.id,
                        &team.name,
                        &appointment.date.to_string(),
                    ),
                );
            }
            for (competition, seasons) in &history.competition_awards {
                for (season, awards) in seasons {
                    let date = history
                        .competition_completed
                        .get(competition)
                        .and_then(|s| s.get(season))
                        .copied()
                        .unwrap_or(today);
                    if let Some(mut article) =
                        templates::season_awards_article(awards, *season, &date.to_string())
                    {
                        article.id = format!("competition:{competition}:{}", article.id);
                        publish(&mut state.articles, article);
                    }
                }
            }
            // Source publishes one all-world season preview after regeneration.
            // Composite IDs retain each season instead of reusing source's ID.
            for (season, date) in &history.completed {
                let id = format!("season_preview_{}", season + 1);
                if !state.articles.iter().any(|article| article.id == id) {
                    let names = self
                        .management
                        .personnel
                        .as_ref()
                        .ok_or("News personnel missing")?
                        .teams
                        .values()
                        .map(|team| team.name.clone())
                        .collect::<Vec<_>>();
                    let mut rng = StdRng::seed_from_u64(
                        state.seed ^ u64::from(*season) ^ 0x7072_6576_6965_77,
                    );
                    let mut article =
                        templates::season_preview_article(&names, &date.to_string(), &mut rng);
                    article.id = id;
                    publish(&mut state.articles, article);
                }
            }
        }
        Ok(())
    }
    pub(crate) fn validate_news_checkpoint(&self) -> Result<(), String> {
        let Some(news) = &self.management.news else {
            return Ok(());
        };
        let today = self.career_date().ok_or("News requires career")?;
        let mut ids = BTreeSet::new();
        if news.last_processed.is_some_and(|d| d > today)
            || news
                .articles
                .iter()
                .any(|a| a.id.trim().is_empty() || !ids.insert(&a.id))
            || news
                .protected_clubs
                .iter()
                .any(|id| !self.management.clubs.contains_key(id))
        {
            return Err("Invalid news state".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_hook_delivers_both_owned_prematches_three_days_ahead_and_replays() {
        let mut game = crate::personnel::tests::game();
        game.fixtures.push(crate::football::Fixture {
            id: "future".into(),
            day: 4,
            home: "a".into(),
            away: "b".into(),
            seed: 99,
        });
        game.configure_news_setup(NewsSetup {
            seed: 17,
            articles: vec![],
            protected_clubs: BTreeSet::from(["a".into()]),
        })
        .unwrap();
        let mut replay = game.clone();
        game.advance_closed_day(1, 100, 200).unwrap();
        replay.advance_closed_day(1, 100, 200).unwrap();
        assert_eq!(game.save_state().unwrap(), replay.save_state().unwrap());
        for actor in ["a", "b"] {
            let inbox = &game.management.social.as_ref().unwrap().inbox;
            let messages: Vec<_> = inbox
                .list(actor)
                .into_iter()
                .filter(|m| m.id == "prematch_future")
                .collect();
            assert_eq!(messages.len(), 1, "{actor}");
        }
        let loaded = Football::load_validated(game.save_state().unwrap()).unwrap();
        assert_eq!(loaded.save_state().unwrap(), game.save_state().unwrap());
        game.advance_closed_day(2, 200, 300).unwrap();
        assert_eq!(
            game.management
                .social
                .as_ref()
                .unwrap()
                .inbox
                .history("a")
                .iter()
                .filter(|m| m.id == "prematch_future")
                .count(),
            1
        );
    }

    #[test]
    fn imported_articles_and_explicit_protected_clubs_survive_setup() {
        let mut game = crate::personnel::tests::game();
        let article =
            templates::managerial_appointment_article("coach", "Coach", "a", "A", "2026-05-31");
        game.configure_news_setup(NewsSetup {
            seed: 4,
            articles: vec![article.clone()],
            protected_clubs: BTreeSet::from(["a".into()]),
        })
        .unwrap();
        assert_eq!(game.news_view(0, 1)[0].id, article.id);
        assert!(
            !game
                .management
                .news
                .as_ref()
                .unwrap()
                .protected_clubs
                .contains("b")
        );
        let mut future = article;
        future.id = "future-import".into();
        future.date = "2026-06-03T12:00:00Z".into();
        game.management.news.as_mut().unwrap().articles.push(future);
        assert_eq!(game.news_view(0, 100).len(), 1);
        assert_eq!(game.management.news.as_ref().unwrap().articles.len(), 2);
        game.validate_news_checkpoint().unwrap();
    }

    #[test]
    fn result_template_preserves_owned_outcome_priority_and_fixture_facts() {
        for (club, outcome, priority) in [
            ("a", "victory", MessagePriority::Normal),
            ("b", "defeat", MessagePriority::High),
        ] {
            let message = result_message::match_result_message(
                "fixture",
                "A",
                "B",
                2,
                1,
                "a",
                "b",
                club,
                8,
                "2026-06-01",
                &mut StdRng::seed_from_u64(5),
            );
            assert_eq!(
                message.subject_key.as_deref(),
                Some(format!("be.msg.matchResult.subject.{outcome}").as_str())
            );
            assert_eq!(message.priority, priority);
            assert_eq!(message.context.fixture_id.as_deref(), Some("fixture"));
        }
    }
}
