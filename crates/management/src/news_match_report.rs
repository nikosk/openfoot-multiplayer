//! Pinned OpenFoot Manager 64677fee. Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
use super::params;
use domain::league::FixtureCompetition;
use domain::news::*;
use rand::RngExt;
use serde::Serialize;

#[derive(Serialize)]
struct MatchReportScorerParam<'a> {
    player: &'a str,
    minute: u32,
    team: &'a str,
}

fn scorer_parts(
    home_name: &str,
    away_name: &str,
    home_scorers: &[(String, u32)],
    away_scorers: &[(String, u32)],
) -> Vec<String> {
    let mut parts = Vec::new();
    for (name, minute) in home_scorers {
        parts.push(format!("{} ({}', {})", name, minute, home_name));
    }
    for (name, minute) in away_scorers {
        parts.push(format!("{} ({}', {})", name, minute, away_name));
    }
    parts
}

fn scorer_player_ids(
    home_scorers: &[(String, u32)],
    away_scorers: &[(String, u32)],
) -> Vec<String> {
    home_scorers
        .iter()
        .chain(away_scorers.iter())
        .map(|(name, _)| name.clone())
        .collect()
}

fn scorer_params_json(
    home_name: &str,
    away_name: &str,
    home_scorers: &[(String, u32)],
    away_scorers: &[(String, u32)],
) -> String {
    let scorers: Vec<MatchReportScorerParam<'_>> = home_scorers
        .iter()
        .map(|(player, minute)| MatchReportScorerParam {
            player,
            minute: *minute,
            team: home_name,
        })
        .chain(
            away_scorers
                .iter()
                .map(|(player, minute)| MatchReportScorerParam {
                    player,
                    minute: *minute,
                    team: away_name,
                }),
        )
        .collect();

    serde_json::to_string(&scorers).unwrap_or_else(|_| "[]".to_string())
}

fn outcome_key(home_goals: u8, away_goals: u8) -> &'static str {
    if home_goals > away_goals {
        "homeWin"
    } else if away_goals > home_goals {
        "awayWin"
    } else {
        "draw"
    }
}

/// Generate a match report news article for a completed fixture.
#[allow(clippy::too_many_arguments)]
pub fn match_report_article(
    fixture_id: &str,
    home_name: &str,
    away_name: &str,
    home_goals: u8,
    away_goals: u8,
    home_team_id: &str,
    away_team_id: &str,
    competition: FixtureCompetition,
    matchday: u32,
    home_scorers: &[(String, u32)], // (player_name, minute)
    away_scorers: &[(String, u32)],
    date: &str,
    rng: &mut impl rand::Rng,
) -> NewsArticle {
    let is_league_fixture = matches!(competition, FixtureCompetition::League);

    let scorer_parts = scorer_parts(home_name, away_name, home_scorers, away_scorers);
    let scorers_data = scorer_params_json(home_name, away_name, home_scorers, away_scorers);

    let source_keys = [
        "be.source.sportsGazette",
        "be.source.footballHerald",
        "be.source.matchDayPress",
        "be.source.leagueChronicle",
    ];
    let src_idx = rng.random_range(0..source_keys.len());
    let source_key = source_keys[src_idx];

    let player_ids = scorer_player_ids(home_scorers, away_scorers);

    if !is_league_fixture {
        let (title_key, body_key) = match competition {
            FixtureCompetition::Friendly => (
                "be.news.matchReport.reportFriendly.title",
                "be.news.matchReport.reportFriendly.body",
            ),
            FixtureCompetition::PreseasonTournament => (
                "be.news.matchReport.reportPreseason.title",
                "be.news.matchReport.reportPreseason.body",
            ),
            FixtureCompetition::Cup
            | FixtureCompetition::ContinentalClub
            | FixtureCompetition::InternationalClub
            | FixtureCompetition::InternationalNation
            | FixtureCompetition::FriendlyCup => (
                "be.news.matchReport.reportFriendly.title",
                "be.news.matchReport.reportFriendly.body",
            ),
            FixtureCompetition::League => unreachable!(),
        };

        return NewsArticle::new(
            format!("report_{}", fixture_id),
            String::new(),
            String::new(),
            String::new(),
            date.to_string(),
            NewsCategory::MatchReport,
        )
        .with_teams(vec![home_team_id.to_string(), away_team_id.to_string()])
        .with_players(player_ids)
        .with_i18n(
            title_key,
            body_key,
            source_key,
            params(&[
                ("home", home_name),
                ("away", away_name),
                ("homeGoals", &home_goals.to_string()),
                ("awayGoals", &away_goals.to_string()),
                ("scorers", ""),
                ("scorersSection", ""),
                ("scorersData", &scorers_data),
            ]),
        )
        .with_score(NewsMatchScore {
            home_team_id: home_team_id.to_string(),
            away_team_id: away_team_id.to_string(),
            home_goals,
            away_goals,
        });
    }

    let idx = rng.random_range(0..3);

    // Determine outcome for i18n key
    let outcome = outcome_key(home_goals, away_goals);
    let headline_variant = rng.random_range(0..3u8);
    let body_key = if scorer_parts.is_empty() {
        format!("be.news.matchReport.body{}.noScorers", idx)
    } else {
        format!("be.news.matchReport.body{}", idx)
    };

    NewsArticle::new(
        format!("report_{}", fixture_id),
        String::new(),
        String::new(),
        String::new(),
        date.to_string(),
        NewsCategory::MatchReport,
    )
    .with_teams(vec![home_team_id.to_string(), away_team_id.to_string()])
    .with_players(player_ids)
    .with_score(NewsMatchScore {
        home_team_id: home_team_id.to_string(),
        away_team_id: away_team_id.to_string(),
        home_goals,
        away_goals,
    })
    .with_i18n(
        &format!(
            "be.news.matchReport.headline.{}.{}",
            outcome, headline_variant
        ),
        &body_key,
        source_key,
        {
            let mut p = params(&[
                ("home", home_name),
                ("away", away_name),
                ("homeGoals", &home_goals.to_string()),
                ("awayGoals", &away_goals.to_string()),
                ("matchday", &matchday.to_string()),
                ("scorers", ""),
                ("scorersData", &scorers_data),
            ]);
            // For winner-specific headlines
            if home_goals > away_goals {
                p.insert("winner".to_string(), home_name.to_string());
                p.insert("loser".to_string(), away_name.to_string());
            } else if away_goals > home_goals {
                p.insert("winner".to_string(), away_name.to_string());
                p.insert("loser".to_string(), home_name.to_string());
            }
            p
        },
    )
}
