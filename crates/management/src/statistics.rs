//! Source persistent per-match statistics, pinned 64677fee turn/post_match.rs.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
use crate::football::{FinishedFixture, Football};
use domain::stats::{PlayerMatchStatsRecord, StatsState, TeamMatchStatsRecord};
use std::collections::BTreeMap;

impl Football {
    pub fn configure_statistics(&mut self, stats: StatsState) -> Result<(), String> {
        if self.started || self.management.sequence != 0 || self.statistics.is_some() {
            return Err("Statistics can only be configured once before commands".into());
        }
        validate(&stats)?;
        self.statistics = Some(stats);
        Ok(())
    }
    pub(crate) fn capture_statistics(&mut self, finished: &FinishedFixture) -> Result<(), String> {
        let Some(stats) = &mut self.statistics else {
            return Ok(());
        };
        let state = self
            .competitions
            .as_ref()
            .ok_or("Statistics require competition metadata")?;
        let (league, fixture) = state
            .setup
            .competitions
            .values()
            .find_map(|league| {
                league
                    .fixtures
                    .iter()
                    .find(|f| f.id == finished.fixture_id)
                    .map(|f| (league, f))
            })
            .ok_or("Statistics fixture missing")?;
        if stats
            .team_matches
            .iter()
            .any(|row| row.fixture_id == fixture.id)
        {
            return Err("Statistics fixture already captured".into());
        }
        let ownership = self
            .management
            .players
            .values()
            .filter(|p| !p.club_id.is_empty())
            .map(|p| (p.id.as_str(), p.club_id.as_str()))
            .collect();
        let mut captured = capture(
            league,
            fixture,
            &ownership,
            &finished.home,
            &finished.away,
            &finished.report,
        );
        // Engine report maps have no ordering contract. Stable storage changes
        // no causal statistic and avoids serialization depending on hash salt.
        captured
            .player_matches
            .sort_by(|a, b| a.player_id.cmp(&b.player_id));
        stats.append(captured);
        Ok(())
    }
    pub fn player_match_statistics(
        &self,
        player_id: &str,
        offset: usize,
        limit: usize,
    ) -> Vec<PlayerMatchStatsRecord> {
        self.statistics
            .as_ref()
            .map(|s| {
                s.player_matches
                    .iter()
                    .rev()
                    .filter(|r| r.player_id == player_id)
                    .skip(offset)
                    .take(limit.min(100))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn team_match_statistics(
        &self,
        club_id: &str,
        offset: usize,
        limit: usize,
    ) -> Vec<TeamMatchStatsRecord> {
        self.statistics
            .as_ref()
            .map(|s| {
                s.team_matches
                    .iter()
                    .rev()
                    .filter(|r| r.team_id == club_id)
                    .skip(offset)
                    .take(limit.min(100))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
    pub(crate) fn validate_statistics_checkpoint(&self) -> Result<(), String> {
        if let Some(stats) = &self.statistics {
            validate(stats)?;
        }
        Ok(())
    }
}
fn validate(stats: &StatsState) -> Result<(), String> {
    let mut players = std::collections::BTreeSet::new();
    let mut teams = std::collections::BTreeSet::new();
    for row in &stats.player_matches {
        if row.fixture_id.is_empty()
            || row.player_id.is_empty()
            || !row.rating.is_finite()
            || !players.insert((&row.fixture_id, &row.player_id))
            || chrono::NaiveDate::parse_from_str(&row.date, "%Y-%m-%d").is_err()
        {
            return Err("Invalid or duplicate player match statistic".into());
        }
    }
    for row in &stats.team_matches {
        if row.fixture_id.is_empty()
            || row.team_id.is_empty()
            || row.possession_pct > 100
            || !teams.insert((&row.fixture_id, &row.team_id))
            || chrono::NaiveDate::parse_from_str(&row.date, "%Y-%m-%d").is_err()
        {
            return Err("Invalid or duplicate team match statistic".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_capture_preserves_units_competition_and_both_sides() {
        let league = domain::league::League {
            season: 2026,
            ..Default::default()
        };
        let fixture:domain::league::Fixture=serde_json::from_value(serde_json::json!({
            "id":"match","competition_id":"cup","matchday":4,"date":"2026-08-01",
            "home_team_id":"a","away_team_id":"b","competition":"Cup","status":"Scheduled","result":null
        })).unwrap();
        let mut report = engine::MatchReport::from_events(vec![], 55, 45, 93);
        report.home_goals = 2;
        report.away_goals = 1;
        report.home_stats.passes_completed = 100;
        report.home_stats.passes_intercepted = 20;
        report.away_stats.tackles = 7;
        report.player_stats.insert(
            "a-p".into(),
            engine::PlayerMatchStats {
                minutes_played: 93,
                goals: 2,
                rating: 8.5,
                ..Default::default()
            },
        );
        report
            .player_stats
            .insert("stranger".into(), Default::default());
        let stats = capture(
            &league,
            &fixture,
            &BTreeMap::from([("a-p", "a"), ("stranger", "c")]),
            "a",
            "b",
            &report,
        );
        assert_eq!(stats.player_matches.len(), 1);
        assert_eq!(stats.player_matches[0].minutes_played, 93);
        assert_eq!(
            stats.player_matches[0].competition,
            domain::league::FixtureCompetition::Cup
        );
        assert_eq!(stats.team_matches[0].passes_attempted, 120);
        assert_eq!(stats.team_matches[1].tackles_won, 7);
        assert_eq!(stats.team_matches[1].goals_for, 1);
        assert_eq!(stats.team_matches[0].possession_pct, 55);
        validate(&stats).unwrap();
        let restored: StatsState =
            serde_json::from_value(serde_json::to_value(&stats).unwrap()).unwrap();
        assert_eq!(restored.player_matches, stats.player_matches);
        let mut duplicated = stats.clone();
        duplicated.append(stats);
        assert!(validate(&duplicated).is_err());
    }
}
fn capture(
    league: &domain::league::League,
    fixture: &domain::league::Fixture,
    team_by_player_id: &BTreeMap<&str, &str>,
    home_team_id: &str,
    away_team_id: &str,
    report: &engine::MatchReport,
) -> StatsState {
    let home_possession_pct = report.home_possession.round().clamp(0.0, 100.0) as u8;
    let away_possession_pct = (100.0 - report.home_possession).round().clamp(0.0, 100.0) as u8;

    let player_matches = report
        .player_stats
        .iter()
        .filter_map(|(player_id, stats)| {
            let team_id = *team_by_player_id.get(player_id.as_str())?;
            if team_id != home_team_id && team_id != away_team_id {
                return None;
            }

            let opponent_team_id = if team_id == home_team_id {
                away_team_id
            } else {
                home_team_id
            };

            Some(PlayerMatchStatsRecord {
                fixture_id: fixture.id.clone(),
                season: league.season,
                matchday: fixture.matchday,
                date: fixture.date.clone(),
                competition: fixture.competition.clone(),
                player_id: player_id.clone(),
                team_id: team_id.to_string(),
                opponent_team_id: opponent_team_id.to_string(),
                home_team_id: home_team_id.to_string(),
                away_team_id: away_team_id.to_string(),
                home_goals: report.home_goals,
                away_goals: report.away_goals,
                minutes_played: stats.minutes_played,
                goals: stats.goals,
                assists: stats.assists,
                shots: stats.shots,
                shots_on_target: stats.shots_on_target,
                passes_completed: stats.passes_completed,
                passes_attempted: stats.passes_attempted,
                tackles_won: stats.tackles_won,
                interceptions: stats.interceptions,
                fouls_committed: stats.fouls_committed,
                yellow_cards: stats.yellow_cards,
                red_cards: stats.red_cards,
                rating: stats.rating,
            })
        })
        .collect();

    let team_matches = vec![
        TeamMatchStatsRecord {
            fixture_id: fixture.id.clone(),
            season: league.season,
            matchday: fixture.matchday,
            date: fixture.date.clone(),
            competition: fixture.competition.clone(),
            team_id: home_team_id.to_string(),
            opponent_team_id: away_team_id.to_string(),
            home_team_id: home_team_id.to_string(),
            away_team_id: away_team_id.to_string(),
            goals_for: report.home_goals,
            goals_against: report.away_goals,
            possession_pct: home_possession_pct,
            shots: report.home_stats.shots,
            shots_on_target: report.home_stats.shots_on_target,
            passes_completed: report.home_stats.passes_completed,
            passes_attempted: report.home_stats.passes_completed
                + report.home_stats.passes_intercepted,
            tackles_won: report.home_stats.tackles,
            interceptions: report.home_stats.interceptions,
            fouls_committed: report.home_stats.fouls,
            yellow_cards: report.home_stats.yellow_cards,
            red_cards: report.home_stats.red_cards,
        },
        TeamMatchStatsRecord {
            fixture_id: fixture.id.clone(),
            season: league.season,
            matchday: fixture.matchday,
            date: fixture.date.clone(),
            competition: fixture.competition.clone(),
            team_id: away_team_id.to_string(),
            opponent_team_id: home_team_id.to_string(),
            home_team_id: home_team_id.to_string(),
            away_team_id: away_team_id.to_string(),
            goals_for: report.away_goals,
            goals_against: report.home_goals,
            possession_pct: away_possession_pct,
            shots: report.away_stats.shots,
            shots_on_target: report.away_stats.shots_on_target,
            passes_completed: report.away_stats.passes_completed,
            passes_attempted: report.away_stats.passes_completed
                + report.away_stats.passes_intercepted,
            tackles_won: report.away_stats.tackles,
            interceptions: report.away_stats.interceptions,
            fouls_committed: report.away_stats.fouls,
            yellow_cards: report.away_stats.yellow_cards,
            red_cards: report.away_stats.red_cards,
        },
    ];

    StatsState {
        player_matches,
        team_matches,
    }
}
