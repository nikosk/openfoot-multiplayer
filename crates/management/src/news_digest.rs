//! Pinned source turn/news.rs; explicit RNG and symmetric protected club set.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
use super::NewsContext as Game;
use super::templates as news;
use chrono::{Datelike, Duration, NaiveDate};
use domain::league::{Fixture, FixtureStatus, League, StandingEntry, TransferRumour};
use rand::seq::SliceRandom;
use std::collections::{HashMap, HashSet};

const MAX_WEEKLY_TRANSFER_RUMOURS: usize = 2;
const TRANSFER_RUMOUR_RETENTION_DAYS: i64 = 28;
const MAX_DAILY_WORLD_NEWS_ARTICLES: usize = 5;

fn completed_fixtures_for_day<'a>(league: &'a League, today: &str) -> Vec<&'a Fixture> {
    league
        .fixtures
        .iter()
        .filter(|fixture| {
            fixture.date == today
                && fixture.status == FixtureStatus::Completed
                && fixture.counts_for_league_standings()
        })
        .collect()
}

fn team_name_or(game: &Game, team_id: &str, fallback: &str) -> String {
    game.teams
        .iter()
        .find(|team| team.id == team_id)
        .map(|team| team.name.clone())
        .unwrap_or_else(|| fallback.to_string())
}

fn team_name(game: &Game, team_id: &str) -> String {
    team_name_or(game, team_id, "")
}

fn player_match_name_or_id(game: &Game, player_id: &str) -> String {
    game.players
        .iter()
        .find(|player| player.id == player_id)
        .map(|player| player.match_name.clone())
        .unwrap_or_else(|| player_id.to_string())
}

fn scorers_for_side(
    game: &Game,
    report: &engine::MatchReport,
    side: engine::Side,
) -> Vec<(String, u32)> {
    report
        .goals
        .iter()
        .filter(|goal| goal.side == side)
        .map(|goal| {
            (
                player_match_name_or_id(game, &goal.scorer_id),
                goal.minute as u32,
            )
        })
        .collect()
}

fn matchday_results(game: &Game, fixtures: &[&Fixture]) -> Vec<(String, u8, String, u8)> {
    fixtures
        .iter()
        .map(|fixture| {
            let (home_goals, away_goals) = fixture
                .result
                .as_ref()
                .map(|result| (result.home_goals, result.away_goals))
                .unwrap_or((0, 0));
            (
                team_name(game, &fixture.home_team_id),
                home_goals,
                team_name(game, &fixture.away_team_id),
                away_goals,
            )
        })
        .collect()
}

fn standings_rows(game: &Game, league: &League) -> Vec<(String, u32, i16)> {
    let mut standings: Vec<(String, u32, i16)> = league
        .standings
        .iter()
        .map(|entry| {
            (
                team_name(game, &entry.team_id),
                entry.points,
                entry.goal_difference() as i16,
            )
        })
        .collect();
    standings.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)));
    standings
}

fn pre_match_target_date(today: &str) -> Option<String> {
    let today_date = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d").ok()?;
    Some(
        (today_date + chrono::Duration::days(3))
            .format("%Y-%m-%d")
            .to_string(),
    )
}

fn scheduled_user_fixtures_for_date<'a>(
    league: &'a League,
    user_team_id: &str,
    target_date: &str,
) -> Vec<&'a Fixture> {
    league
        .fixtures
        .iter()
        .filter(|fixture| {
            fixture.date == target_date
                && fixture.status == FixtureStatus::Scheduled
                && fixture.counts_for_league_standings()
                && (fixture.home_team_id == user_team_id || fixture.away_team_id == user_team_id)
        })
        .collect()
}

fn opponent_for_fixture<'a>(fixture: &'a Fixture, user_team_id: &str) -> (&'a str, bool) {
    if fixture.home_team_id == user_team_id {
        (&fixture.away_team_id, true)
    } else {
        (&fixture.home_team_id, false)
    }
}

fn weekly_digest_suffix(game: &Game) -> String {
    let iso_week = game.clock.current_date.iso_week();
    format!("{}_w{:02}", iso_week.year(), iso_week.week())
}

fn season_has_started(league: &League) -> bool {
    league
        .fixtures
        .iter()
        .any(|f| f.counts_for_league_standings() && f.status == FixtureStatus::Completed)
        || league.standings.iter().any(|s| s.played > 0)
}

fn title_race_is_newsworthy(leader: &StandingEntry, challenger: &StandingEntry) -> bool {
    leader.played >= 5
        && challenger.played >= 5
        && leader.points > 0
        && leader.points.saturating_sub(challenger.points) <= 3
}

fn has_equivalent_storyline(game: &Game, candidate: &domain::news::NewsArticle) -> bool {
    game.news.iter().any(|article| {
        article.category == candidate.category
            && article.headline_key == candidate.headline_key
            && article.body_key == candidate.body_key
            && article.source_key == candidate.source_key
            && article.team_ids == candidate.team_ids
            && article.player_ids == candidate.player_ids
            && article.i18n_params == candidate.i18n_params
    })
}

fn unbeaten_run_length(form: &[String]) -> u32 {
    let mut streak = 0;

    for result in form.iter().rev() {
        if result == "L" {
            break;
        }

        if result == "W" || result == "D" {
            streak += 1;
        }
    }

    streak
}

fn top_scorer_summary(game: &Game) -> Option<(String, u32)> {
    game.players
        .iter()
        .filter(|player| player.stats.goals > 0)
        .max_by(|a, b| {
            a.stats
                .goals
                .cmp(&b.stats.goals)
                .then_with(|| a.match_name.cmp(&b.match_name))
        })
        .map(|player| (player.match_name.clone(), player.stats.goals))
}

fn weekly_storyline_articles(
    game: &Game,
    suffix: &str,
    date: &str,
) -> Vec<domain::news::NewsArticle> {
    let mut articles = Vec::new();
    let league = match &game.league {
        Some(league) => league,
        None => return articles,
    };

    let sorted_standings = league.sorted_standings();
    if sorted_standings.len() >= 2 {
        let leader = &sorted_standings[0];
        let challenger = &sorted_standings[1];

        if title_race_is_newsworthy(leader, challenger) {
            let leader_name = team_name(game, &leader.team_id);
            let challenger_name = team_name(game, &challenger.team_id);
            let gap = leader.points.saturating_sub(challenger.points);
            let article = news::title_race_storyline_article(
                &format!("storyline_title_race_{}", suffix),
                &leader.team_id,
                &leader_name,
                &challenger.team_id,
                &challenger_name,
                gap,
                date,
            );

            if !has_equivalent_storyline(game, &article) {
                articles.push(article);
            }
        }
    }

    if let Some(team) = game
        .teams
        .iter()
        .map(|team| (team, unbeaten_run_length(&team.form)))
        .filter(|(_, streak)| *streak >= 5)
        .max_by_key(|(_, streak)| *streak)
        .map(|(team, streak)| (team.id.clone(), team.name.clone(), streak))
    {
        let article = news::unbeaten_streak_storyline_article(
            &format!("storyline_unbeaten_streak_{}", suffix),
            &team.0,
            &team.1,
            team.2,
            date,
        );

        if !has_equivalent_storyline(game, &article) {
            articles.push(article);
        }
    }

    articles
}

/// Selects interesting players from non-user AI teams who could plausibly be the subject
/// of transfer speculation (high market value, expiring contract, or low morale).
fn rumour_candidates(game: &Game) -> Vec<(String, String, String, String)> {
    // (player_id, player_name, team_id, team_name)

    let current_date = game.clock.current_date.date_naive();

    game.players
        .iter()
        .filter(|p| {
            let Some(tid) = p.team_id.as_deref() else {
                return false;
            };
            if game.protected_clubs.contains(tid) {
                return false;
            }
            if p.injury.is_some() {
                return false;
            }
            let high_value = p.market_value >= 800_000;
            let short_contract = p
                .contract_end
                .as_deref()
                .and_then(|end| chrono::NaiveDate::parse_from_str(end, "%Y-%m-%d").ok())
                .map(|end| {
                    let days = (end - current_date).num_days();
                    (1..=365).contains(&days)
                })
                .unwrap_or(false);
            let low_morale = p.morale <= 45;
            high_value || short_contract || low_morale
        })
        .filter_map(|p| {
            let tid = p.team_id.as_deref()?;
            let team_name = team_name(game, tid);
            Some((
                p.id.clone(),
                p.match_name.clone(),
                tid.to_string(),
                team_name,
            ))
        })
        .collect()
}

fn prune_stale_transfer_rumours(league: &mut League, current_date: NaiveDate) {
    let earliest_kept_date = current_date - Duration::days(TRANSFER_RUMOUR_RETENTION_DAYS - 1);

    league.transfer_rumours.retain(|rumour| {
        chrono::DateTime::parse_from_rfc3339(&rumour.date)
            .map(|created_at| created_at.date_naive() >= earliest_kept_date)
            .unwrap_or(true)
    });
}

fn weekly_rumour_articles(
    game: &mut Game,
    suffix: &str,
    date: &str,
    rng: &mut impl rand::Rng,
) -> Vec<domain::news::NewsArticle> {
    let current_date = game.clock.current_date.date_naive();
    let existing_article_ids: HashSet<String> =
        game.news.iter().map(|article| article.id.clone()).collect();
    let Some(league) = game.league.as_mut() else {
        return vec![];
    };
    prune_stale_transfer_rumours(league, current_date);
    let candidates = rumour_candidates(game);
    if candidates.is_empty() {
        return vec![];
    }
    let Some(league) = game.league.as_mut() else {
        return vec![];
    };

    // Pick at most 2 distinct players
    let count = candidates.len().min(MAX_WEEKLY_TRANSFER_RUMOURS);
    let mut chosen_indices: Vec<usize> = (0..candidates.len()).collect();
    chosen_indices.shuffle(rng);
    chosen_indices.truncate(count);

    let mut articles = Vec::new();
    for idx in chosen_indices {
        let (player_id, player_name, team_id, team_name) = &candidates[idx];
        let article_id = format!("rumour_{}_{}", player_id, suffix);
        if existing_article_ids.contains(&article_id)
            || league
                .transfer_rumours
                .iter()
                .any(|rumour| rumour.id == article_id)
        {
            continue;
        }

        league.transfer_rumours.push(TransferRumour {
            id: article_id.clone(),
            date: date.to_string(),
            player_id: player_id.clone(),
            player_name: player_name.clone(),
            team_id: team_id.clone(),
            team_name: team_name.clone(),
        });
        articles.push(news::transfer_rumour_gossip_article(
            &article_id,
            player_id,
            player_name,
            team_id,
            team_name,
            date,
            rng,
        ));
    }

    articles
}

fn completed_preseason_fixtures_for_window(
    league: &League,
    current_date: NaiveDate,
    window_days: i64,
) -> Vec<&Fixture> {
    let window_start = current_date - Duration::days(window_days.saturating_sub(1));

    league
        .fixtures
        .iter()
        .filter(|fixture| {
            fixture.status == FixtureStatus::Completed
                && matches!(
                    fixture.competition,
                    domain::league::FixtureCompetition::Friendly
                        | domain::league::FixtureCompetition::PreseasonTournament
                )
                && NaiveDate::parse_from_str(&fixture.date, "%Y-%m-%d")
                    .map(|fixture_date| {
                        fixture_date >= window_start && fixture_date <= current_date
                    })
                    .unwrap_or(false)
        })
        .collect()
}

fn world_news_category_priority(category: &domain::news::NewsCategory) -> i64 {
    match category {
        domain::news::NewsCategory::Editorial => 500_000,
        domain::news::NewsCategory::TransferRoundup => 400_000,
        domain::news::NewsCategory::ManagerialChange => 350_000,
        domain::news::NewsCategory::InjuryNews => 300_000,
        domain::news::NewsCategory::TransferRumour => 200_000,
        domain::news::NewsCategory::LeagueRoundup => 150_000,
        domain::news::NewsCategory::StandingsUpdate => 125_000,
        domain::news::NewsCategory::SeasonPreview => 100_000,
        domain::news::NewsCategory::MatchReport => 0,
    }
}

fn world_news_priority(game: &Game, article: &domain::news::NewsArticle) -> i64 {
    let team_reputation = article
        .team_ids
        .iter()
        .filter_map(|team_id| game.teams.iter().find(|team| team.id == *team_id))
        .map(|team| i64::from(team.reputation))
        .max()
        .unwrap_or(0);
    let player_heat = article
        .player_ids
        .iter()
        .filter_map(|player_id| game.players.iter().find(|player| player.id == *player_id))
        .map(|player| (player.market_value / 10_000) as i64)
        .max()
        .unwrap_or(0);
    let rivalry_multiplier = if article
        .team_ids
        .iter()
        .any(|id| game.protected_clubs.contains(id))
    {
        2
    } else {
        1
    };

    (world_news_category_priority(&article.category) + (team_reputation * 100) + player_heat)
        * rivalry_multiplier
}

fn apply_daily_world_news_cap(
    game: &Game,
    date: &str,
    candidates: Vec<domain::news::NewsArticle>,
) -> Vec<domain::news::NewsArticle> {
    let existing_non_match_count = game
        .news
        .iter()
        .filter(|article| {
            article.date == date && article.category != domain::news::NewsCategory::MatchReport
        })
        .count();
    let available_slots = MAX_DAILY_WORLD_NEWS_ARTICLES.saturating_sub(existing_non_match_count);

    if candidates.len() <= available_slots {
        return candidates;
    }

    let mut ranked: Vec<(usize, i64)> = candidates
        .iter()
        .enumerate()
        .map(|(index, article)| (index, world_news_priority(game, article)))
        .collect();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));

    let kept_indexes: HashSet<usize> = ranked
        .into_iter()
        .take(available_slots)
        .map(|(index, _)| index)
        .collect();

    candidates
        .into_iter()
        .enumerate()
        .filter_map(|(index, article)| kept_indexes.contains(&index).then_some(article))
        .collect()
}

fn completed_transfers_for_window(
    game: &Game,
    current_date: NaiveDate,
    window_days: i64,
) -> Vec<(String, String, String, String, String, String, u64)> {
    let Some(league) = &game.league else {
        return Vec::new();
    };

    let window_start = current_date - Duration::days(window_days.saturating_sub(1));
    let mut transfers: Vec<_> = league
        .transfer_log
        .iter()
        .filter_map(|transfer| {
            let transfer_date = NaiveDate::parse_from_str(&transfer.date, "%Y-%m-%d").ok()?;
            if transfer_date < window_start || transfer_date > current_date {
                return None;
            }

            Some((
                player_match_name_or_id(game, &transfer.player_id),
                team_name(game, &transfer.from_team_id),
                team_name(game, &transfer.to_team_id),
                transfer.player_id.clone(),
                transfer.from_team_id.clone(),
                transfer.to_team_id.clone(),
                transfer.fee,
                transfer.date.clone(),
            ))
        })
        .collect();

    transfers.sort_by(|left, right| right.6.cmp(&left.6).then(right.7.cmp(&left.7)));
    transfers.truncate(3);
    transfers
        .into_iter()
        .map(
            |(player, from_team, to_team, player_id, from_team_id, to_team_id, fee, _)| {
                (
                    player,
                    from_team,
                    to_team,
                    player_id,
                    from_team_id,
                    to_team_id,
                    fee,
                )
            },
        )
        .collect()
}

fn weekly_transfer_roundup_article(
    game: &Game,
    suffix: &str,
    week_start: &str,
    date: &str,
) -> Option<domain::news::NewsArticle> {
    let article_id = format!("weekly_transfer_roundup_{}", suffix);
    if game.news.iter().any(|article| article.id == article_id) {
        return None;
    }

    let transfers = completed_transfers_for_window(game, game.clock.current_date.date_naive(), 7);
    if transfers.is_empty() {
        return None;
    }

    Some(news::transfer_roundup_article(
        &article_id,
        week_start,
        &transfers,
        date,
    ))
}

fn preseason_unbeaten_teams(game: &Game) -> Vec<String> {
    let Some(league) = &game.league else {
        return Vec::new();
    };

    let mut records: HashMap<String, (u32, u32)> = HashMap::new();

    for fixture in &league.fixtures {
        if fixture.status != FixtureStatus::Completed
            || !matches!(
                fixture.competition,
                domain::league::FixtureCompetition::Friendly
                    | domain::league::FixtureCompetition::PreseasonTournament
            )
        {
            continue;
        }

        let Some(result) = &fixture.result else {
            continue;
        };

        let home_record = records
            .entry(fixture.home_team_id.clone())
            .or_insert((0, 0));
        home_record.0 += 1;
        if result.home_goals < result.away_goals {
            home_record.1 += 1;
        }

        let away_record = records
            .entry(fixture.away_team_id.clone())
            .or_insert((0, 0));
        away_record.0 += 1;
        if result.away_goals < result.home_goals {
            away_record.1 += 1;
        }
    }

    let mut unbeaten: Vec<(String, u32)> = records
        .into_iter()
        .filter(|(_, (played, losses))| *played >= 2 && *losses == 0)
        .map(|(team_id, (played, _))| (team_name(game, &team_id), played))
        .collect();
    unbeaten.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));

    unbeaten.into_iter().map(|(team, _)| team).collect()
}

fn generate_preseason_digest_news(game: &mut Game, today: &str, rng: &mut impl rand::Rng) {
    let suffix = weekly_digest_suffix(game);
    let digest_id = format!("preseason_digest_{}", suffix);
    if game.news.iter().any(|article| article.id == digest_id) {
        return;
    }

    let current_date = game.clock.current_date.date_naive();
    let date = game.clock.current_date.to_rfc3339();
    let (results, unbeaten_teams) = {
        let Some(league) = &game.league else {
            return;
        };

        let fixtures = completed_preseason_fixtures_for_window(league, current_date, 7);
        (
            matchday_results(game, &fixtures),
            preseason_unbeaten_teams(game),
        )
    };

    let mut candidates = vec![news::preseason_digest_article(
        &digest_id,
        today,
        &results,
        &unbeaten_teams,
        &date,
    )];
    if let Some(roundup) = weekly_transfer_roundup_article(game, &suffix, today, &date) {
        candidates.push(roundup);
    }
    let rumours = weekly_rumour_articles(game, &suffix, &date, rng);
    candidates.extend(rumours);
    game.news
        .extend(apply_daily_world_news_cap(game, &date, candidates));
}

pub(super) fn generate_weekly_digest_news(game: &mut Game, today: &str, rng: &mut impl rand::Rng) {
    if game.clock.current_date.weekday().num_days_from_monday() != 0 {
        return;
    }

    let league = match &game.league {
        Some(league) => league,
        None => return,
    };

    if !season_has_started(league) {
        generate_preseason_digest_news(game, today, rng);
        return;
    }

    let suffix = weekly_digest_suffix(game);
    let digest_id = format!("weekly_digest_{}", suffix);
    if game.news.iter().any(|article| article.id == digest_id) {
        return;
    }

    let date = game.clock.current_date.to_rfc3339();
    let sorted_standings = league.sorted_standings();
    let leader = sorted_standings
        .first()
        .map(|entry| team_name(game, &entry.team_id))
        .unwrap_or_else(|| "Unknown".to_string());
    let storylines = weekly_storyline_articles(game, &suffix, &date);
    let rumours = weekly_rumour_articles(game, &suffix, &date, rng);
    let (top_scorer, top_scorer_goals) =
        top_scorer_summary(game).unwrap_or_else(|| (String::new(), 0));

    let mut candidates = vec![news::weekly_digest_article(
        &digest_id,
        today,
        &leader,
        &top_scorer,
        top_scorer_goals,
        storylines.len(),
        &date,
    )];
    if let Some(roundup) = weekly_transfer_roundup_article(game, &suffix, today, &date) {
        candidates.push(roundup);
    }
    candidates.extend(storylines);
    candidates.extend(rumours);
    game.news
        .extend(apply_daily_world_news_cap(game, &date, candidates));
}

/// Generate a match report news article for the completed fixture.
pub fn generate_matchday_news(game: &mut Game, today: &str, rng: &mut impl rand::Rng) {
    let league = match &game.league {
        Some(l) => l,
        None => return,
    };

    let todays_fixtures = completed_fixtures_for_day(league, today);

    if todays_fixtures.is_empty() {
        return;
    }

    let matchday = todays_fixtures[0].matchday;
    let date_str = game.clock.current_date.to_rfc3339();

    // Don't duplicate
    let roundup_id = format!("roundup_md{}", matchday);
    if game.news.iter().any(|n| n.id == roundup_id) {
        return;
    }

    let results = matchday_results(game, &todays_fixtures);

    let roundup = news::league_roundup_article(matchday, &results, &date_str, rng);
    game.news.push(roundup);

    let standings = standings_rows(game, league);

    let standings_article = news::standings_update_article(matchday, &standings, &date_str, rng);
    game.news.push(standings_article);
}
