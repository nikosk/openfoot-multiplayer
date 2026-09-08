//! Pinned OpenFoot Manager 64677fee. Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
#[path = "news_match_report.rs"]
mod match_report;
pub use match_report::match_report_article;

use crate::team_history::awards::SeasonAwards;
use domain::news::*;
use rand::{Rng, RngExt};
use serde::Serialize;
use std::collections::HashMap;

/// Helper to build a HashMap<String, String> from key-value pairs.
fn params(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn result_lines(results: &[(String, u8, String, u8)]) -> Vec<String> {
    results
        .iter()
        .map(|(home, hg, away, ag)| {
            let home_goals = hg.to_string();
            let away_goals = ag.to_string();
            let mut line = String::with_capacity(
                home.len() + away.len() + home_goals.len() + away_goals.len() + 7,
            );
            line.push(' ');
            line.push(' ');
            line.push_str(home);
            line.push(' ');
            line.push_str(&home_goals);
            line.push(' ');
            line.push('-');
            line.push(' ');
            line.push_str(&away_goals);
            line.push(' ');
            line.push_str(away);
            line
        })
        .collect()
}

#[derive(Serialize)]
struct RoundupResultParam<'a> {
    home: &'a str,
    #[serde(rename = "homeGoals")]
    home_goals: u8,
    away: &'a str,
    #[serde(rename = "awayGoals")]
    away_goals: u8,
}

fn roundup_results_data(results: &[(String, u8, String, u8)]) -> String {
    let entries: Vec<RoundupResultParam<'_>> = results
        .iter()
        .map(|(home, home_goals, away, away_goals)| RoundupResultParam {
            home,
            home_goals: *home_goals,
            away,
            away_goals: *away_goals,
        })
        .collect();

    serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_string())
}

fn biggest_winner_name(results: &[(String, u8, String, u8)]) -> String {
    results
        .iter()
        .filter(|(_, hg, _, ag)| hg != ag)
        .max_by_key(|(_, hg, _, ag)| (*hg as i8 - *ag as i8).unsigned_abs())
        .map(
            |(home, hg, away, ag)| {
                if hg > ag { home.clone() } else { away.clone() }
            },
        )
        .unwrap_or_default()
}

fn goal_difference_text(goal_difference: i16) -> String {
    if goal_difference >= 0 {
        let goal_difference_text = goal_difference.to_string();
        let mut text = String::with_capacity(goal_difference_text.len() + 1);
        text.push('+');
        text.push_str(&goal_difference_text);
        text
    } else {
        goal_difference.to_string()
    }
}

fn standings_lines(top_teams: &[(String, u32, i16)]) -> Vec<String> {
    top_teams
        .iter()
        .enumerate()
        .map(|(idx, (name, points, goal_difference))| {
            let rank = (idx + 1).to_string();
            let points_text = points.to_string();
            let goal_difference_text = goal_difference_text(*goal_difference);
            let mut line = String::with_capacity(
                rank.len() + name.len() + points_text.len() + goal_difference_text.len() + 15,
            );
            line.push(' ');
            line.push(' ');
            line.push_str(&rank);
            line.push('.');
            line.push(' ');
            line.push_str(name);
            line.push(' ');
            line.push('—');
            line.push(' ');
            line.push_str(&points_text);
            line.push(' ');
            line.push('p');
            line.push('t');
            line.push('s');
            line.push(' ');
            line.push('(');
            line.push('G');
            line.push('D');
            line.push(':');
            line.push(' ');
            line.push_str(&goal_difference_text);
            line.push(')');
            line
        })
        .collect()
}

#[derive(Serialize)]
struct StandingsLineParam<'a> {
    rank: usize,
    team: &'a str,
    points: u32,
    #[serde(rename = "goalDifference")]
    goal_difference: String,
}

fn standings_data(top_teams: &[(String, u32, i16)]) -> String {
    let entries: Vec<StandingsLineParam<'_>> = top_teams
        .iter()
        .enumerate()
        .map(
            |(idx, (name, points, goal_difference))| StandingsLineParam {
                rank: idx + 1,
                team: name,
                points: *points,
                goal_difference: goal_difference_text(*goal_difference),
            },
        )
        .collect();

    serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_string())
}

fn preseason_unbeaten_line(unbeaten_teams: &[String]) -> String {
    match unbeaten_teams {
        [] => String::new(),
        [team] => format!("\n\n{} remain unbeaten in preseason.", team),
        [first, second, ..] => {
            format!("\n\n{} and {} remain unbeaten in preseason.", first, second)
        }
    }
}

fn preseason_unbeaten_data(unbeaten_teams: &[String]) -> String {
    serde_json::to_string(unbeaten_teams).unwrap_or_else(|_| "[]".to_string())
}

/// Generate a league roundup article summarising all matchday results.
pub fn league_roundup_article(
    matchday: u32,
    results: &[(String, u8, String, u8)], // (home_name, home_goals, away_name, away_goals)
    date: &str,
    rng: &mut impl rand::Rng,
) -> NewsArticle {
    let results_data = roundup_results_data(results);
    let biggest_winner = biggest_winner_name(results);

    let total_goals: u32 = results
        .iter()
        .map(|(_, hg, _, ag)| u32::from(*hg) + u32::from(*ag))
        .sum();

    let source_keys = [
        "be.source.leagueWire",
        "be.source.footballHerald",
        "be.source.sportsGazette",
    ];
    let src_idx = rng.random_range(0..source_keys.len());
    let headline_idx = rng.random_range(0..3);

    NewsArticle::new(
        format!("roundup_md{}", matchday),
        String::new(),
        String::new(),
        String::new(),
        date.to_string(),
        NewsCategory::LeagueRoundup,
    )
    .with_i18n(
        &format!("be.news.roundup.headline{}", headline_idx),
        "be.news.roundup.body",
        source_keys[src_idx],
        params(&[
            ("matchday", &matchday.to_string()),
            ("totalGoals", &total_goals.to_string()),
            ("matchCount", &results.len().to_string()),
            ("results", &result_lines(results).join("\n")),
            ("resultsData", &results_data),
            ("biggestWinner", &biggest_winner),
        ]),
    )
}

/// Generate a standings update article after a matchday.
pub fn standings_update_article(
    matchday: u32,
    top_teams: &[(String, u32, i16)], // (team_name, points, goal_diff)
    date: &str,
    rng: &mut impl rand::Rng,
) -> NewsArticle {
    let leader = top_teams
        .first()
        .map(|(n, _, _)| n.as_str())
        .unwrap_or("Unknown");
    let standings_data = standings_data(top_teams);

    let source_keys = [
        "be.source.leagueWire",
        "be.source.footballHerald",
        "be.source.leagueChronicle",
    ];
    let src_idx = rng.random_range(0..source_keys.len());
    let headline_idx = rng.random_range(0..3);

    NewsArticle::new(
        format!("standings_md{}", matchday),
        String::new(),
        String::new(),
        String::new(),
        date.to_string(),
        NewsCategory::StandingsUpdate,
    )
    .with_i18n(
        &format!("be.news.standings.headline{}", headline_idx),
        "be.news.standings.body",
        source_keys[src_idx],
        params(&[
            ("matchday", &matchday.to_string()),
            ("leader", leader),
            ("standings", &standings_lines(top_teams).join("\n")),
            ("standingsData", &standings_data),
        ]),
    )
}

fn preview_contenders<'a>(team_names: &'a [String], rng: &mut impl Rng) -> (&'a str, &'a str) {
    if team_names.is_empty() {
        return ("", "");
    }
    let favourite = &team_names[rng.random_range(0..team_names.len())];

    // Draw the dark horse from the clubs actually named differently, falling back
    // to the favourite when there are none. The obvious formulation — keep picking
    // at random until the name differs — never terminates if every club shares a
    // name, and a package is free to do that; guarding on `team_names.len()`, as
    // this used to, does not catch it.
    let others: Vec<&String> = team_names
        .iter()
        .filter(|name| *name != favourite)
        .collect();
    let dark_horse = if others.is_empty() {
        favourite
    } else {
        others[rng.random_range(0..others.len())]
    };

    (favourite.as_str(), dark_horse.as_str())
}

/// Generate a season preview article at the start of the season.
pub fn season_preview_article(
    team_names: &[String],
    date: &str,
    rng: &mut impl rand::Rng,
) -> NewsArticle {
    let (favourite, dark_horse) = preview_contenders(team_names, rng);
    let headline_idx = rng.random_range(0..3);

    NewsArticle::new(
        "season_preview".to_string(),
        String::new(),
        String::new(),
        String::new(),
        date.to_string(),
        NewsCategory::SeasonPreview,
    )
    .with_i18n(
        &format!("be.news.seasonPreview.headline{}", headline_idx),
        "be.news.seasonPreview.body",
        "be.source.footballHerald",
        params(&[
            ("teamCount", &team_names.len().to_string()),
            ("favourite", favourite),
            ("darkHorse", dark_horse),
            ("teamList", &team_names.join(", ")),
        ]),
    )
}

pub fn managerial_appointment_article(
    manager_id: &str,
    manager_name: &str,
    team_id: &str,
    team_name: &str,
    date: &str,
) -> NewsArticle {
    NewsArticle::new(
        format!("managerial_appointment_{}_{}", team_id, date),
        String::new(),
        String::new(),
        String::new(),
        date.to_string(),
        NewsCategory::ManagerialChange,
    )
    .with_teams(vec![team_id.to_string()])
    .with_i18n(
        "be.news.managerialAppointment.headline",
        "be.news.managerialAppointment.body",
        "be.source.leagueWire",
        params(&[
            ("team", team_name),
            ("manager", manager_name),
            ("managerId", manager_id),
        ]),
    )
}

fn format_transfer_fee(fee: u64) -> String {
    super::currency::format_compact_money(fee, super::currency::DEFAULT_CURRENCY_CODE)
        .unwrap_or_else(|| format!("{}{}", super::currency::default_currency_symbol(), fee))
}

pub fn transfer_roundup_article(
    id: &str,
    week_start: &str,
    transfers: &[(String, String, String, String, String, String, u64)],
    date: &str,
) -> NewsArticle {
    let deals_data = serde_json::to_string(
        &transfers
            .iter()
            .map(|(player, from_team, to_team, _, _, _, fee)| {
                serde_json::json!({
                    "player": player,
                    "fromTeam": from_team,
                    "toTeam": to_team,
                    "fee": format_transfer_fee(*fee),
                })
            })
            .collect::<Vec<_>>(),
    )
    .unwrap_or_default();
    let deals = transfers
        .iter()
        .map(|(player, from_team, to_team, _, _, _, fee)| {
            format!(
                "  {}: {} -> {} ({})",
                player,
                from_team,
                to_team,
                format_transfer_fee(*fee)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut team_ids = Vec::new();
    let mut player_ids = Vec::new();
    for (_, _, _, player_id, from_team_id, to_team_id, _) in transfers {
        if !player_ids.contains(player_id) {
            player_ids.push(player_id.clone());
        }
        if !team_ids.contains(from_team_id) {
            team_ids.push(from_team_id.clone());
        }
        if !team_ids.contains(to_team_id) {
            team_ids.push(to_team_id.clone());
        }
    }

    NewsArticle::new(
        id.to_string(),
        String::new(),
        String::new(),
        String::new(),
        date.to_string(),
        NewsCategory::TransferRoundup,
    )
    .with_teams(team_ids)
    .with_players(player_ids)
    .with_i18n(
        "be.news.transferRoundup.headline",
        "be.news.transferRoundup.body",
        "be.source.transferIntelligence",
        params(&[
            ("weekStart", week_start),
            ("transferCount", &transfers.len().to_string()),
            ("deals", &deals),
            ("dealsData", &deals_data),
        ]),
    )
}

/// Generate the end-of-season awards ceremony news article.
///
/// Returns `None` when neither marquee award (Golden Boot, Player of the Year) has a winner —
/// nothing to celebrate, so no article.
pub fn season_awards_article(
    awards: &SeasonAwards,
    season: u32,
    date: &str,
) -> Option<NewsArticle> {
    let golden_boot = awards.golden_boot.first();
    let poty = awards.player_of_year.first();
    let manager = awards.manager_of_season.first();
    if golden_boot.is_none() && poty.is_none() {
        return None;
    }

    let mut i18n_params = HashMap::new();
    i18n_params.insert("season".to_string(), season.to_string());
    if let Some(gb) = golden_boot {
        i18n_params.insert("goldenBootWinner".to_string(), gb.player_name.clone());
        i18n_params.insert("goldenBootTeam".to_string(), gb.team_name.clone());
        i18n_params.insert("goldenBootGoals".to_string(), (gb.value as u32).to_string());
    }
    if let Some(p) = poty {
        i18n_params.insert("potyWinner".to_string(), p.player_name.clone());
        i18n_params.insert("potyTeam".to_string(), p.team_name.clone());
        i18n_params.insert("potyRating".to_string(), format!("{:.1}", p.value));
    }
    if let Some(manager) = manager {
        i18n_params.insert("managerWinner".to_string(), manager.manager_name.clone());
        i18n_params.insert("managerTeam".to_string(), manager.team_name.clone());
        i18n_params.insert(
            "managerWinRate".to_string(),
            format!("{:.0}", manager.win_rate),
        );
    }

    let body_key = match (golden_boot, poty) {
        (Some(_), Some(_)) => "be.news.seasonAwards.bodyBoth",
        (Some(_), None) => "be.news.seasonAwards.bodyGoldenBootOnly",
        (None, Some(_)) => "be.news.seasonAwards.bodyPotyOnly",
        (None, None) => unreachable!(),
    };

    let mut player_ids = Vec::new();
    let mut team_ids = Vec::new();
    for entry in [golden_boot, poty].into_iter().flatten() {
        if !entry.player_id.is_empty() && !player_ids.contains(&entry.player_id) {
            player_ids.push(entry.player_id.clone());
        }
        if !entry.team_id.is_empty() && !team_ids.contains(&entry.team_id) {
            team_ids.push(entry.team_id.clone());
        }
    }
    if let Some(manager) = manager
        && !manager.team_id.is_empty()
        && !team_ids.contains(&manager.team_id)
    {
        team_ids.push(manager.team_id.clone());
    }

    Some(
        NewsArticle::new(
            format!("season_awards_{}", season),
            String::new(),
            String::new(),
            String::new(),
            date.to_string(),
            NewsCategory::Editorial,
        )
        .with_teams(team_ids)
        .with_players(player_ids)
        .with_i18n(
            "be.news.seasonAwards.headline",
            body_key,
            "be.source.footballHerald",
            i18n_params,
        ),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn major_transfer_article(
    id: &str,
    player_id: &str,
    player_name: &str,
    from_team_id: &str,
    from_team_name: &str,
    to_team_id: &str,
    to_team_name: &str,
    fee: u64,
    date: &str,
) -> NewsArticle {
    let fee_display = format_transfer_fee(fee);

    NewsArticle::new(
        id.to_string(),
        String::new(),
        String::new(),
        String::new(),
        date.to_string(),
        NewsCategory::TransferRumour,
    )
    .with_teams(vec![from_team_id.to_string(), to_team_id.to_string()])
    .with_players(vec![player_id.to_string()])
    .with_i18n(
        "be.news.majorTransfer.headline",
        "be.news.majorTransfer.body",
        "be.source.leagueChronicle",
        params(&[
            ("player", player_name),
            ("fromTeam", from_team_name),
            ("toTeam", to_team_name),
            ("fee", &fee_display),
        ]),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn loan_move_article(
    id: &str,
    player_id: &str,
    player_name: &str,
    from_team_id: &str,
    from_team_name: &str,
    to_team_id: &str,
    to_team_name: &str,
    end_date: &str,
    date: &str,
) -> NewsArticle {
    NewsArticle::new(
        id.to_string(),
        String::new(),
        String::new(),
        String::new(),
        date.to_string(),
        NewsCategory::TransferRumour,
    )
    .with_teams(vec![from_team_id.to_string(), to_team_id.to_string()])
    .with_players(vec![player_id.to_string()])
    .with_i18n(
        "be.news.loanMove.headline",
        "be.news.loanMove.body",
        "be.source.transferIntelligence",
        params(&[
            ("player", player_name),
            ("fromTeam", from_team_name),
            ("toTeam", to_team_name),
            ("endDate", end_date),
        ]),
    )
}

pub fn weekly_digest_article(
    id: &str,
    week_start: &str,
    leader: &str,
    top_scorer: &str,
    top_scorer_goals: u32,
    storyline_count: usize,
    date: &str,
) -> NewsArticle {
    let body_key = if top_scorer.is_empty() {
        "be.news.weeklyDigest.bodyNoTopScorer"
    } else {
        "be.news.weeklyDigest.bodyWithTopScorer"
    };

    NewsArticle::new(
        id.to_string(),
        String::new(),
        String::new(),
        String::new(),
        date.to_string(),
        NewsCategory::Editorial,
    )
    .with_i18n(
        "be.news.weeklyDigest.headline",
        body_key,
        "be.source.leagueChronicle",
        params(&[
            ("weekStart", week_start),
            ("leader", leader),
            ("topScorer", top_scorer),
            ("topScorerGoals", &top_scorer_goals.to_string()),
            ("storylineCount", &storyline_count.to_string()),
        ]),
    )
}

pub fn preseason_digest_article(
    id: &str,
    week_start: &str,
    results: &[(String, u8, String, u8)],
    unbeaten_teams: &[String],
    date: &str,
) -> NewsArticle {
    let results_data = roundup_results_data(results);
    let total_goals: u32 = results
        .iter()
        .map(|(_, home_goals, _, away_goals)| u32::from(*home_goals) + u32::from(*away_goals))
        .sum();
    let unbeaten_line = preseason_unbeaten_line(unbeaten_teams);
    let unbeaten_teams_data = preseason_unbeaten_data(unbeaten_teams);

    let body_key = if results.is_empty() {
        "be.news.preseasonDigest.bodyNoResults"
    } else {
        "be.news.preseasonDigest.bodyWithResults"
    };

    NewsArticle::new(
        id.to_string(),
        String::new(),
        String::new(),
        String::new(),
        date.to_string(),
        NewsCategory::Editorial,
    )
    .with_i18n(
        "be.news.preseasonDigest.headline",
        body_key,
        "be.source.leagueChronicle",
        params(&[
            ("weekStart", week_start),
            ("resultCount", &results.len().to_string()),
            ("totalGoals", &total_goals.to_string()),
            ("results", &result_lines(results).join("\n")),
            ("resultsData", &results_data),
            ("unbeatenLine", &unbeaten_line),
            ("unbeatenTeamsData", &unbeaten_teams_data),
        ]),
    )
}

pub fn title_race_storyline_article(
    id: &str,
    leader_team_id: &str,
    leader: &str,
    challenger_team_id: &str,
    challenger: &str,
    gap: u32,
    date: &str,
) -> NewsArticle {
    NewsArticle::new(
        id.to_string(),
        String::new(),
        String::new(),
        String::new(),
        date.to_string(),
        NewsCategory::Editorial,
    )
    .with_teams(vec![
        leader_team_id.to_string(),
        challenger_team_id.to_string(),
    ])
    .with_i18n(
        "be.news.storyline.titleRace.headline",
        "be.news.storyline.titleRace.body",
        "be.source.leagueChronicle",
        params(&[
            ("leader", leader),
            ("challenger", challenger),
            ("gap", &gap.to_string()),
        ]),
    )
}

pub fn unbeaten_streak_storyline_article(
    id: &str,
    team_id: &str,
    team: &str,
    run_length: u32,
    date: &str,
) -> NewsArticle {
    NewsArticle::new(
        id.to_string(),
        String::new(),
        String::new(),
        String::new(),
        date.to_string(),
        NewsCategory::Editorial,
    )
    .with_teams(vec![team_id.to_string()])
    .with_i18n(
        "be.news.storyline.unbeatenStreak.headline",
        "be.news.storyline.unbeatenStreak.body",
        "be.source.leagueChronicle",
        params(&[("team", team), ("runLength", &run_length.to_string())]),
    )
}

/// Generate a speculative transfer rumour article linking a player to other clubs.
///
/// Unlike `major_transfer_article` (which reports a completed move), this function
/// produces gossip-style speculation. The article is attributed to a tabloid-leaning
/// source and uses intentionally hedged language.
pub fn transfer_rumour_gossip_article(
    id: &str,
    player_id: &str,
    player_name: &str,
    from_team_id: &str,
    from_team_name: &str,
    date: &str,
    rng: &mut impl rand::Rng,
) -> NewsArticle {
    let headline_idx = rng.random_range(0..3);
    let body_idx = rng.random_range(0..3);

    let source_keys = [
        "be.source.transferIntelligence",
        "be.source.sportsGazette",
        "be.source.footballHerald",
    ];
    let src_idx = rng.random_range(0..source_keys.len());

    NewsArticle::new(
        id.to_string(),
        String::new(),
        String::new(),
        String::new(),
        date.to_string(),
        NewsCategory::TransferRumour,
    )
    .with_teams(vec![from_team_id.to_string()])
    .with_players(vec![player_id.to_string()])
    .with_i18n(
        &format!("be.news.transferRumour.headline{}", headline_idx),
        &format!("be.news.transferRumour.body{}", body_idx),
        source_keys[src_idx],
        params(&[("player", player_name), ("team", from_team_name)]),
    )
}

/// Generate a news article reporting that a notable player has been injured.
pub fn injury_news_article(
    id: &str,
    player_id: &str,
    player_name: &str,
    team_id: &str,
    team_name: &str,
    days_out: u32,
    date: &str,
    rng: &mut impl rand::Rng,
) -> NewsArticle {
    let is_short = days_out <= 7;
    let weeks = days_out.div_ceil(7);
    // Body keys: locale-specific phrasing for days (short) vs weeks (long).
    // headline2 (contains duration) is only picked for long injuries so the locale
    // template can use {{weeksOut}} without needing a conditional.
    let duration_suffix = if is_short { "Days" } else { "Weeks" };

    let headline_count = if is_short { 2 } else { 3 };
    let headline_idx = rng.random_range(0..headline_count);

    let body_idx = rng.random_range(0..2_usize);

    let source_keys = [
        "be.source.leagueWire",
        "be.source.footballHerald",
        "be.source.matchDayPress",
    ];
    let src_idx = rng.random_range(0..source_keys.len());

    NewsArticle::new(
        id.to_string(),
        String::new(),
        String::new(),
        String::new(),
        date.to_string(),
        NewsCategory::InjuryNews,
    )
    .with_teams(vec![team_id.to_string()])
    .with_players(vec![player_id.to_string()])
    .with_i18n(
        &format!("be.news.injuryNews.headline{}", headline_idx),
        &format!("be.news.injuryNews.body{}{}", body_idx, duration_suffix),
        source_keys[src_idx],
        params(&[
            ("player", player_name),
            ("team", team_name),
            ("daysOut", &days_out.to_string()),
            ("weeksOut", &weeks.to_string()),
        ]),
    )
}
