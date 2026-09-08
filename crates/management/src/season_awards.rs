//! Pinned OpenFoot Manager 64677fee season_awards.rs, scoped inputs instead of Game.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
use chrono::{Datelike, NaiveDate};
use domain::{
    manager::Manager,
    player::{Player, Position},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
/// A single award entry (player + stat value).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AwardEntry {
    pub player_id: String,
    pub player_name: String,
    pub team_id: String,
    pub team_name: String,
    pub value: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ManagerAwardEntry {
    pub manager_id: String,
    pub manager_name: String,
    pub team_id: String,
    pub team_name: String,
    pub value: f64,
    pub win_rate: f64,
}

/// Season award standings — top 5 in each category.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SeasonAwards {
    pub golden_boot: Vec<AwardEntry>,      // Top scorers
    pub assist_king: Vec<AwardEntry>,      // Top assists
    pub player_of_year: Vec<AwardEntry>,   // Best avg rating (min 5 apps)
    pub clean_sheet_king: Vec<AwardEntry>, // Most clean sheets (GKs only)
    pub most_appearances: Vec<AwardEntry>,
    pub young_player: Vec<AwardEntry>, // Best avg rating, age <= 21
    pub manager_of_season: Vec<ManagerAwardEntry>,
}

struct PlayerAwardContext<'a> {
    player: &'a Player,
    team_id: String,
    team_name: String,
    age: i32,
}

struct ManagerAwardContext<'a> {
    manager: &'a Manager,
    team_id: String,
    team_name: String,
    league_position: u32,
    points: u32,
    win_rate: f64,
}

fn free_agent_team_name() -> String {
    ["Free", "Agent"].join(" ")
}

fn player_age_on(today: &NaiveDate, date_of_birth: &str) -> i32 {
    if let Ok(dob) = NaiveDate::parse_from_str(date_of_birth, "%Y-%m-%d") {
        let mut age = today.year() - dob.year();
        if today.ordinal() < dob.ordinal() {
            age -= 1;
        }
        age
    } else {
        30
    }
}

fn award_entry<'a>(context: &PlayerAwardContext<'a>, value: f64) -> AwardEntry {
    AwardEntry {
        player_id: context.player.id.clone(),
        player_name: context.player.match_name.clone(),
        team_id: context.team_id.clone(),
        team_name: context.team_name.clone(),
        value,
    }
}

fn manager_award_entry<'a>(context: &ManagerAwardContext<'a>) -> ManagerAwardEntry {
    ManagerAwardEntry {
        manager_id: context.manager.id.clone(),
        manager_name: context.manager.full_name(),
        team_id: context.team_id.clone(),
        team_name: context.team_name.clone(),
        value: context.points as f64,
        win_rate: context.win_rate,
    }
}
fn top_manager_awards(contexts: &[ManagerAwardContext<'_>]) -> Vec<ManagerAwardEntry> {
    let mut awards: Vec<_> = contexts.iter().map(manager_award_entry).collect();

    awards.sort_by(|left, right| {
        let left_position = contexts
            .iter()
            .find(|context| context.manager.id == left.manager_id)
            .map(|context| context.league_position)
            .unwrap_or(u32::MAX);
        let right_position = contexts
            .iter()
            .find(|context| context.manager.id == right.manager_id)
            .map(|context| context.league_position)
            .unwrap_or(u32::MAX);

        left_position
            .cmp(&right_position)
            .then_with(|| {
                right
                    .value
                    .partial_cmp(&left.value)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| {
                right
                    .win_rate
                    .partial_cmp(&left.win_rate)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| left.manager_name.cmp(&right.manager_name))
    });
    awards.truncate(5);
    awards
}

fn top_awards<'a, F, G>(
    contexts: &[PlayerAwardContext<'a>],
    include: F,
    value: G,
) -> Vec<AwardEntry>
where
    F: Fn(&PlayerAwardContext<'a>) -> bool,
    G: Fn(&PlayerAwardContext<'a>) -> f64,
{
    let mut awards: Vec<_> = contexts
        .iter()
        .filter(|context| include(context))
        .map(|context| award_entry(context, value(context)))
        .collect();

    awards.sort_by(|a, b| {
        b.value
            .partial_cmp(&a.value)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    awards.truncate(5);
    awards
}

/// Source division-scoped awards: retired/unattached/nonparticipants excluded.
/// Stable player ordering is provided by the caller, preserving tie behavior.
pub fn compute(
    players: &[Player],
    teams: &BTreeMap<String, String>,
    managers: &[Manager],
    standings: &[crate::football::Standing],
    date: NaiveDate,
) -> SeasonAwards {
    let scope = standings
        .iter()
        .map(|s| s.club_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let contexts = players
        .iter()
        .filter(|p| {
            !p.retired
                && p.stats.appearances > 0
                && p.team_id.as_deref().is_some_and(|id| scope.contains(id))
        })
        .map(|player| {
            let team_id = player.team_id.clone().unwrap();
            let team_name = teams
                .get(&team_id)
                .cloned()
                .unwrap_or_else(free_agent_team_name);
            PlayerAwardContext {
                player,
                team_id,
                team_name,
                age: player_age_on(&date, &player.date_of_birth),
            }
        })
        .collect::<Vec<_>>();
    let manager_contexts = managers
        .iter()
        .filter_map(|manager| {
            let team_id = manager.team_id.as_ref()?;
            let (index, standing) = standings
                .iter()
                .enumerate()
                .find(|(_, s)| &s.club_id == team_id)?;
            Some(ManagerAwardContext {
                manager,
                team_id: team_id.clone(),
                team_name: teams.get(team_id)?.clone(),
                league_position: index as u32 + 1,
                points: standing.points,
                win_rate: if standing.played == 0 {
                    0.0
                } else {
                    f64::from(standing.won) / f64::from(standing.played) * 100.0
                },
            })
        })
        .collect::<Vec<_>>();
    // Golden Boot — top scorers
    let golden_boot = top_awards(
        &contexts,
        |context| context.player.stats.goals > 0,
        |context| context.player.stats.goals as f64,
    );

    // Assist King
    let assist_king = top_awards(
        &contexts,
        |context| context.player.stats.assists > 0,
        |context| context.player.stats.assists as f64,
    );

    // Player of the Year — best avg rating, min 5 appearances
    let player_of_year = top_awards(
        &contexts,
        |context| context.player.stats.appearances >= 5 && context.player.stats.avg_rating > 0.0,
        |context| context.player.stats.avg_rating as f64,
    );

    // Clean Sheet King — GKs only
    let clean_sheet_king = top_awards(
        &contexts,
        |context| {
            context.player.position == Position::Goalkeeper && context.player.stats.clean_sheets > 0
        },
        |context| context.player.stats.clean_sheets as f64,
    );

    // Most Appearances
    let most_appearances = top_awards(
        &contexts,
        |_| true,
        |context| context.player.stats.appearances as f64,
    );

    // Young Player of the Year — age <= 21, best avg rating, min 3 apps
    let young_player = top_awards(
        &contexts,
        |context| {
            context.age <= 21
                && context.player.stats.appearances >= 3
                && context.player.stats.avg_rating > 0.0
        },
        |context| context.player.stats.avg_rating as f64,
    );
    let manager_of_season = top_manager_awards(&manager_contexts);

    SeasonAwards {
        golden_boot,
        assist_king,
        player_of_year,
        clean_sheet_king,
        most_appearances,
        young_player,
        manager_of_season,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_award_filters_minimum_apps_age_keeper_scope_and_top_five() {
        let game = crate::personnel::tests::game();
        let base = game.project_source_players().unwrap()["a-p"].clone();
        let mut players = (0..8)
            .map(|i| {
                let mut p = base.clone();
                p.id = format!("p{i}");
                p.match_name = p.id.clone();
                p.date_of_birth = "2007-01-01".into();
                p.stats.appearances = 5;
                p.stats.goals = i + 1;
                p.stats.assists = i + 1;
                p.stats.avg_rating = 7.0 + i as f32 / 10.0;
                p
            })
            .collect::<Vec<_>>();
        players[0].stats.appearances = 4;
        players[1].stats.appearances = 2;
        players[2].position = Position::Goalkeeper;
        players[2].stats.clean_sheets = 3;
        players[6].team_id = None;
        players[7].retired = true;
        let standings = vec![crate::football::Standing {
            club_id: "a".into(),
            played: 2,
            won: 2,
            drawn: 0,
            lost: 0,
            goals_for: 5,
            goals_against: 0,
            points: 6,
        }];
        let awards = compute(
            &players,
            &[("a".into(), "A".into())].into(),
            &[],
            &standings,
            NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
        );
        assert_eq!(
            awards
                .golden_boot
                .iter()
                .map(|a| a.player_id.as_str())
                .collect::<Vec<_>>(),
            ["p5", "p4", "p3", "p2", "p1"]
        );
        assert!(!awards.player_of_year.iter().any(|a| a.player_id == "p0"));
        assert!(!awards.young_player.iter().any(|a| a.player_id == "p1"));
        assert_eq!(awards.clean_sheet_king.len(), 1);
        assert_eq!(awards.clean_sheet_king[0].player_id, "p2");
    }
}
