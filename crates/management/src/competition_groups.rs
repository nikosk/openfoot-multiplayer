//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
//! Pinned 64677fee: only explicit deterministic competition IDs replace UUIDs.
//! Group-and-knockout competitions: a Champions-League-style group stage whose
//! top finishers seed the knockout bracket.

use chrono::{DateTime, Duration, Utc};
use domain::league::{
    CompetitionFormat, CompetitionRules, CompetitionScope, CompetitionType, FixtureCompetition,
    FixtureStatus, GroupState, League, StandingEntry,
};

/// Clubs per group; the last groups may run one short when the entrant count
/// doesn't divide evenly.
const GROUP_SIZE: usize = 4;

/// Shape of a group stage at creation time.
#[derive(Debug, Clone)]
pub struct GroupStageConfig {
    /// Round-robin legs within each group (2 = home and away).
    pub legs: u8,
    /// Days between group matchdays.
    pub matchday_gap_days: i64,
    /// Teams advancing from each group.
    pub qualifiers_per_group: u32,
    /// Additional best next-placed finishers across all groups that advance
    /// (the 2026 World Cup's "best thirds").
    pub best_third_qualifiers: u32,
    /// Days between knockout rounds once the bracket starts.
    pub knockout_round_gap_days: u32,
    /// When `Some(n)`, spread group-stage fixtures so at most `n` matches
    /// happen on any single calendar day. `None` keeps the default behaviour
    /// (all fixtures in a matchday share the same date).
    pub max_concurrent_matches_per_day: Option<usize>,
    /// Maximum fixtures scheduled on the same day within a single knockout
    /// round. Mirrors `CompetitionRules::knockout_matches_per_day`.
    pub knockout_matches_per_day: u32,
}

impl Default for GroupStageConfig {
    fn default() -> Self {
        Self {
            legs: 2,
            matchday_gap_days: 7,
            qualifiers_per_group: 2,
            best_third_qualifiers: 0,
            knockout_round_gap_days: 14,
            max_concurrent_matches_per_day: None,
            knockout_matches_per_day: 1,
        }
    }
}

fn fixture_competition_for(kind: &CompetitionType) -> FixtureCompetition {
    match kind {
        CompetitionType::ContinentalClub => FixtureCompetition::ContinentalClub,
        CompetitionType::InternationalClub => FixtureCompetition::InternationalClub,
        CompetitionType::InternationalNation => FixtureCompetition::InternationalNation,
        CompetitionType::FriendlyCup => FixtureCompetition::FriendlyCup,
        _ => FixtureCompetition::Cup,
    }
}

fn group_label(index: usize) -> String {
    char::from(b'A' + (index % 26) as u8).to_string()
}

/// Snake-seed `team_ids` (strongest first) into groups of ~[`GROUP_SIZE`], so
/// each group gets a comparable spread of strength.
fn seed_groups(competition_id: &str, team_ids: &[String]) -> Vec<GroupState> {
    let group_count = team_ids.len().div_ceil(GROUP_SIZE).max(1);
    let mut groups: Vec<GroupState> = (0..group_count)
        .map(|index| GroupState {
            id: format!("{competition_id}-group-{}", group_label(index)),
            name: group_label(index),
            team_ids: Vec::new(),
            standings: Vec::new(),
        })
        .collect();

    for (position, team_id) in team_ids.iter().enumerate() {
        let row = position / group_count;
        let column = position % group_count;
        let group = if row.is_multiple_of(2) {
            column
        } else {
            group_count - 1 - column
        };
        groups[group].team_ids.push(team_id.clone());
        groups[group]
            .standings
            .push(StandingEntry::new(team_id.clone()));
    }
    groups
}

/// Generate a group-and-knockout competition with the default club shape:
/// double round-robin groups, top two advancing.
pub fn generate_group_knockout_cup(
    name: &str,
    season: u32,
    team_ids: &[String],
    start_date: DateTime<Utc>,
    kind: CompetitionType,
    scope: CompetitionScope,
) -> League {
    generate_group_knockout_cup_with(
        name,
        season,
        team_ids,
        start_date,
        kind,
        scope,
        &GroupStageConfig::default(),
    )
}

/// Generate a group-and-knockout competition: snake-seeded groups playing a
/// round robin shaped by `config`; the knockout bracket is seeded later, once
/// every group fixture has been played.
pub fn generate_group_knockout_cup_with(
    name: &str,
    season: u32,
    team_ids: &[String],
    start_date: DateTime<Utc>,
    kind: CompetitionType,
    scope: CompetitionScope,
    config: &GroupStageConfig,
) -> League {
    let competition_id = serde_json::to_string(&(name, season)).expect("string tuple");
    let group_states = seed_groups(&competition_id, team_ids);
    let groups: Vec<Vec<String>> = group_states
        .into_iter()
        .map(|group| group.team_ids)
        .collect();
    build_group_cup(
        competition_id,
        name,
        season,
        team_ids,
        &groups,
        start_date,
        kind,
        scope,
        config,
    )
}

/// Generate a group-and-knockout competition from an explicit group assignment
/// (e.g. a World Cup draw), instead of snake-seeding. Each inner vector is one
/// group's team ids, in draw order.
pub fn generate_group_knockout_cup_with_groups(
    name: &str,
    season: u32,
    groups: &[Vec<String>],
    start_date: DateTime<Utc>,
    kind: CompetitionType,
    scope: CompetitionScope,
    config: &GroupStageConfig,
) -> League {
    let competition_id = serde_json::to_string(&(name, season)).expect("string tuple");
    let team_ids: Vec<String> = groups.iter().flatten().cloned().collect();
    build_group_cup(
        competition_id,
        name,
        season,
        &team_ids,
        groups,
        start_date,
        kind,
        scope,
        config,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_group_cup(
    competition_id: String,
    name: &str,
    season: u32,
    team_ids: &[String],
    groups: &[Vec<String>],
    start_date: DateTime<Utc>,
    kind: CompetitionType,
    scope: CompetitionScope,
    config: &GroupStageConfig,
) -> League {
    let mut cup = League::new(competition_id.clone(), name.to_string(), season, team_ids);
    cup.kind = kind.clone();
    cup.scope = scope;
    cup.rules = CompetitionRules {
        format: CompetitionFormat::GroupAndKnockout,
        counts_in_season_flow: true,
        group_qualifiers_per_group: config.qualifiers_per_group,
        group_best_third_qualifiers: config.best_third_qualifiers,
        group_stage_legs: config.legs,
        group_matchday_gap_days: config.matchday_gap_days.max(1) as u32,
        knockout_round_gap_days: config.knockout_round_gap_days,
        knockout_matches_per_day: config.knockout_matches_per_day,
    };
    cup.standings.clear();
    cup.groups = groups
        .iter()
        .enumerate()
        .map(|(index, group_team_ids)| GroupState {
            id: format!("{competition_id}-group-{}", group_label(index)),
            name: group_label(index),
            team_ids: group_team_ids.clone(),
            standings: group_team_ids
                .iter()
                .map(|id| StandingEntry::new(id.clone()))
                .collect(),
        })
        .collect();

    let fixture_competition = fixture_competition_for(&kind);
    for group in &cup.groups {
        let fixtures = crate::competition_schedule::build_round_robin_fixtures_with(
            &competition_id,
            &group.team_ids,
            start_date,
            fixture_competition.clone(),
            config.legs,
            config.matchday_gap_days,
        );
        cup.fixtures.extend(fixtures);
    }

    if let Some(max_per_day) = config.max_concurrent_matches_per_day {
        crate::competition_schedule::spread_fixture_dates(
            &mut cup.fixtures,
            start_date,
            max_per_day,
        );
    }

    cup
}

/// A group's table sorted by points, goal difference, then goals for.
pub fn sorted_group_standings(group: &GroupState) -> Vec<StandingEntry> {
    let mut sorted = group.standings.clone();
    sorted.sort_by(|a, b| {
        b.points
            .cmp(&a.points)
            .then(b.goal_difference().cmp(&a.goal_difference()))
            .then(b.goals_for.cmp(&a.goals_for))
    });
    sorted
}

fn is_knockout_fixture(league: &League, fixture_id: &str) -> bool {
    league
        .knockout_rounds
        .iter()
        .any(|round| round.fixture_ids.iter().any(|id| id == fixture_id))
}

/// Record a completed group fixture in its group's table. For a
/// group-and-knockout competition, once the whole group stage is played out the
/// knockout bracket is seeded with each group's top finishers (group winners
/// first, so any byes favour them). For a plain grouped competition (e.g. World
/// Cup qualifying) only the table is updated. A no-op for competitions without
/// groups and for knockout-round fixtures.
pub fn process_completed_fixture(league: &mut League, fixture_index: usize) {
    if league.groups.is_empty() {
        return;
    }
    let Some(fixture) = league.fixtures.get(fixture_index) else {
        return;
    };
    if is_knockout_fixture(league, &fixture.id) {
        return;
    }
    let Some(result) = fixture.result.clone() else {
        return;
    };
    let home_team_id = fixture.home_team_id.clone();
    let away_team_id = fixture.away_team_id.clone();

    if let Some(group) = league
        .groups
        .iter_mut()
        .find(|group| group.team_ids.contains(&home_team_id))
    {
        if let Some(entry) = group
            .standings
            .iter_mut()
            .find(|entry| entry.team_id == home_team_id)
        {
            entry.record_result(result.home_goals, result.away_goals);
        }
        if let Some(entry) = group
            .standings
            .iter_mut()
            .find(|entry| entry.team_id == away_team_id)
        {
            entry.record_result(result.away_goals, result.home_goals);
        }
    }

    if league.rules.format == CompetitionFormat::GroupAndKnockout {
        maybe_seed_knockout_from_groups(league);
    }
}

fn maybe_seed_knockout_from_groups(league: &mut League) {
    if !league.knockout_rounds.is_empty() {
        return;
    }
    let group_stage_complete = league
        .fixtures
        .iter()
        .filter(|fixture| !is_knockout_fixture(league, &fixture.id))
        .all(|fixture| fixture.status == FixtureStatus::Completed);
    if !group_stage_complete {
        return;
    }

    // Group winners (ranked among themselves), then runners-up, and so on, so
    // the strongest group performances receive any knockout byes. The next
    // placed finishers across all groups can also qualify ("best thirds").
    let per_group = (league.rules.group_qualifiers_per_group.max(1)) as usize;
    let best_remainders = league.rules.group_best_third_qualifiers as usize;

    let mut qualifiers_by_rank: Vec<Vec<StandingEntry>> = vec![Vec::new(); per_group];
    let mut remainder_pool: Vec<StandingEntry> = Vec::new();
    for group in &league.groups {
        for (rank, entry) in sorted_group_standings(group)
            .into_iter()
            .take(per_group + 1)
            .enumerate()
        {
            if rank < per_group {
                qualifiers_by_rank[rank].push(entry);
            } else {
                remainder_pool.push(entry);
            }
        }
    }

    let rank_order = |a: &StandingEntry, b: &StandingEntry| {
        b.points
            .cmp(&a.points)
            .then(b.goal_difference().cmp(&a.goal_difference()))
            .then(b.goals_for.cmp(&a.goals_for))
    };
    let mut qualifiers: Vec<String> = Vec::new();
    for mut rank_entries in qualifiers_by_rank {
        rank_entries.sort_by(rank_order);
        qualifiers.extend(rank_entries.into_iter().map(|entry| entry.team_id));
    }
    if best_remainders > 0 {
        remainder_pool.sort_by(rank_order);
        qualifiers.extend(
            remainder_pool
                .into_iter()
                .take(best_remainders)
                .map(|entry| entry.team_id),
        );
    }
    if qualifiers.len() < 2 {
        return;
    }

    let last_group_date = league
        .fixtures
        .iter()
        .map(|fixture| fixture.date.as_str())
        .max()
        .unwrap_or("2026-01-01");
    let knockout_start = chrono::NaiveDate::parse_from_str(last_group_date, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|naive| DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
        .expect("validated group fixture dates")
        + Duration::days(league.rules.knockout_round_gap_days as i64);

    crate::competition_schedule::seed_knockout_round(
        league,
        &qualifiers,
        knockout_start,
        fixture_competition_for(&league.kind.clone()),
    );
}

/// Reset a group-and-knockout competition for a new season in place: fresh
/// snake-seeded groups from `participant_ids`, no fixtures played, no bracket.
pub fn regenerate_for_season(league: &mut League, season: u32, start_date: DateTime<Utc>) {
    league.season = season;
    league.fixtures.clear();
    league.standings.clear();
    league.knockout_rounds.clear();
    league.groups = seed_groups(&league.id, &league.participant_ids);

    let fixture_competition = fixture_competition_for(&league.kind.clone());
    let competition_id = league.id.clone();
    for group in league.groups.clone() {
        let fixtures = crate::competition_schedule::build_round_robin_fixtures_with(
            &competition_id,
            &group.team_ids,
            start_date,
            fixture_competition.clone(),
            league.rules.group_stage_legs.max(1),
            i64::from(league.rules.group_matchday_gap_days.max(1)),
        );
        league.fixtures.extend(fixtures);
    }
}
