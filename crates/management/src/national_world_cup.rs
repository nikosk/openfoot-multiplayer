//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
//! Pinned 64677fee World Cup/qualification/host/ranking rules. Shared projected
//! context replaces the single-player Game; generated identities/RNG are explicit.
//! The World Cup: a quadrennial national-team tournament played in the summer
//! break (the real-world calendar: 2022, 2026, 2030, …). The field is filled
//! from the strongest national pools in the world; nations without enough
//! players get squads synthesised as free agents, so any world can stage it.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Datelike, Utc};
use domain::league::{
    CompetitionFormat, CompetitionScope, CompetitionType, FixtureCompetition, FixtureStatus,
    GroupState, League, MatchResult, StandingEntry,
};
use domain::message::{InboxMessage, MessageCategory, MessagePriority};
use domain::national_team::NationalTeam;
use domain::news::{NewsArticle, NewsCategory};
use domain::world_history::{WorldCupChampionRecord, WorldCupHostRecord};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, RngExt, SeedableRng};

use super::NationalContext as Game;
use super::nations;
use crate::competition_schedule::round_robin_matchdays;
use crate::competitions::groups::GroupStageConfig;

/// A World Cup format preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldCupFormat {
    pub field: usize,
    pub qualifiers_per_group: u32,
    pub best_third_qualifiers: u32,
}

/// The 2026 format: 48 teams in 12 groups; top two per group plus the eight
/// best third-placed teams reach a round of 32.
pub const FORMAT_48: WorldCupFormat = WorldCupFormat {
    field: 48,
    qualifiers_per_group: 2,
    best_third_qualifiers: 8,
};

/// The 1998–2022 format: 32 teams in 8 groups; top two reach a round of 16.
pub const FORMAT_32: WorldCupFormat = WorldCupFormat {
    field: 32,
    qualifiers_per_group: 2,
    best_third_qualifiers: 0,
};

/// A compact format for small worlds: 16 teams in 4 groups; top two reach the
/// quarterfinals.
pub const FORMAT_16: WorldCupFormat = WorldCupFormat {
    field: 16,
    qualifiers_per_group: 2,
    best_third_qualifiers: 0,
};

/// Squads synthesised or topped up reach this size.
const TOPPED_UP_POOL: usize = 18;
/// Days between group matchdays — the tournament must fit the summer break.
const GROUP_MATCHDAY_GAP_DAYS: i64 = 2;
/// Days between knockout rounds, kept tight so the finals fit a real ~5-week
/// World Cup window.
const KNOCKOUT_GAP_DAYS: u32 = 3;

/// World Cups take place in the summers of 2022, 2026, 2030, …
pub fn is_world_cup_summer(year: i32) -> bool {
    year.rem_euclid(4) == 2
}

/// Whether a competition is a World Cup (a national-team tournament).
pub fn is_world_cup_competition(competition: &League) -> bool {
    competition.kind == CompetitionType::InternationalNation
        && competition.scope == CompetitionScope::International
}

/// Player OVRs per nation, strongest first (non-retired players only).
fn national_pools(game: &Game) -> BTreeMap<String, Vec<u8>> {
    let mut pools: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for player in game.players.iter().filter(|player| !player.retired) {
        let nation = if player.football_nation.is_empty() {
            player.nationality.clone()
        } else {
            player.football_nation.clone()
        };
        pools.entry(nation).or_default().push(player.ovr);
    }
    for ovrs in pools.values_mut() {
        ovrs.sort_unstable_by(|a, b| b.cmp(a));
    }
    pools
}

/// Average OVR of a pool's best XI.
fn pool_strength(ovrs: &[u8]) -> f64 {
    let xi: Vec<u8> = ovrs.iter().copied().take(11).collect();
    if xi.is_empty() {
        return 0.0;
    }
    xi.iter().map(|&ovr| ovr as u32).sum::<u32>() as f64 / xi.len() as f64
}

/// Pick the World Cup field, balanced across confederations like a real
/// tournament: each region gets a berth share (largest-remainder, via
/// [`berths_by_region`]) and contributes its strongest nations. A
/// confederation-balanced field keeps the FIFA draw's per-group caps
/// satisfiable and the bracket realistic.
fn select_field(game: &Game, format: &WorldCupFormat) -> Vec<String> {
    let pools = national_pools(game);

    // Candidate nations by region (catalog guarantees coverage in empty worlds).
    let mut candidate_codes: Vec<String> = pools.keys().cloned().collect();
    for nation in nations::NATION_CATALOG {
        candidate_codes.push(nation.code.to_string());
    }
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut by_region: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for code in candidate_codes {
        if !seen.insert(code.clone()) {
            continue;
        }
        by_region
            .entry(region_of_code(game, &code))
            .or_default()
            .push(code);
    }
    let strength = |code: &str| {
        pools
            .get(code)
            .map(|ovrs| pool_strength(ovrs))
            .unwrap_or(0.0)
    };
    for codes in by_region.values_mut() {
        codes.sort_by(|a, b| {
            strength(b)
                .partial_cmp(&strength(a))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.cmp(b))
        });
    }

    let counts: BTreeMap<String, usize> = by_region
        .iter()
        .map(|(region, codes)| (region.clone(), codes.len()))
        .collect();
    let berths = berths_by_region(format.field, &counts);

    let mut field: Vec<String> = Vec::new();
    for (region, codes) in &by_region {
        let take = berths.get(region).copied().unwrap_or(0);
        field.extend(codes.iter().take(take).cloned());
    }
    field.truncate(format.field);
    field
}

fn national_team_id(code: &str) -> String {
    format!("nt-{}", code.to_lowercase())
}

/// Real-world World Cup hosts (and the known near-future editions), so a
/// generated world honours actual history for these years and never reuses a
/// recent real host when awarding new ones.
const REAL_WORLD_HOSTS: &[(u32, &str)] = &[
    (1930, "UY"),
    (1934, "IT"),
    (1938, "FR"),
    (1950, "BR"),
    (1954, "CH"),
    (1958, "SE"),
    (1962, "CL"),
    (1966, "ENG"),
    (1970, "MX"),
    (1974, "DE"),
    (1978, "AR"),
    (1982, "ES"),
    (1986, "MX"),
    (1990, "IT"),
    (1994, "US"),
    (1998, "FR"),
    (2002, "JP"),
    (2006, "DE"),
    (2010, "ZA"),
    (2014, "BR"),
    (2018, "RU"),
    (2022, "QA"),
    (2026, "US"),
    (2030, "ES"),
    (2034, "SA"),
];

/// The host nation code for a World Cup `year`: a host the game awarded, else
/// the real-world host where known.
pub(crate) fn host_for_year(game: &Game, year: i32) -> Option<String> {
    let year = year as u32;
    game.world_history
        .world_cup_host(year)
        .map(str::to_string)
        .or_else(|| {
            REAL_WORLD_HOSTS
                .iter()
                .find(|(host_year, _)| *host_year == year)
                .map(|(_, code)| code.to_string())
        })
}

/// Initial ranking points for a nation with the given squad strength: a neutral
/// base lifted by best-XI quality, so a fresh world's pots reflect strength
/// before any results have moved the ranking.
fn seed_points_for(strength: f64) -> f64 {
    1000.0 + strength * 10.0
}

/// Seed world-ranking points (from squad strength) for any field nation that
/// has not been ranked yet. Existing ranking points are left untouched so
/// accumulated results are preserved across tournaments.
fn seed_world_ranking(game: &mut Game, field: &[String], pools: &BTreeMap<String, Vec<u8>>) {
    for code in field {
        let strength = pools
            .get(code)
            .map(|ovrs| pool_strength(ovrs))
            .unwrap_or(0.0);
        game.world_history
            .seed_ranking(code, seed_points_for(strength));
    }
}

/// Order nation `codes` by world ranking, strongest first, falling back to
/// squad strength for any nation not yet ranked.
pub(crate) fn ranked_field(game: &Game, codes: &[String]) -> Vec<String> {
    ranked_field_with_pools(game, codes, &national_pools(game))
}

/// As [`ranked_field`], but reusing pools already computed by the caller.
fn ranked_field_with_pools(
    game: &Game,
    codes: &[String],
    pools: &BTreeMap<String, Vec<u8>>,
) -> Vec<String> {
    let points_of = |code: &str| -> f64 {
        game.world_history.ranking_points(code).unwrap_or_else(|| {
            seed_points_for(
                pools
                    .get(code)
                    .map(|ovrs| pool_strength(ovrs))
                    .unwrap_or(0.0),
            )
        })
    };
    let mut ordered: Vec<String> = codes.to_vec();
    ordered.sort_by(|a, b| {
        points_of(b)
            .partial_cmp(&points_of(a))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.cmp(b))
    });
    ordered
}

/// Confederation cap for one World Cup group: at most one team per confederation
/// except UEFA (`europe`), where up to two may share a group.
fn confederation_cap(region: &str) -> usize {
    if region == "europe" { 2 } else { 1 }
}

/// Whether `code` may be added to `group` without breaching the confederation
/// cap. Regions come from a precomputed map so the draw honours the same
/// (possibly world-overridden) confederation classification as qualifying.
fn group_admits(group: &[String], code: &str, regions: &HashMap<String, String>) -> bool {
    let region = regions.get(code).map(String::as_str).unwrap_or_default();
    let cap = confederation_cap(region);
    group
        .iter()
        .filter(|other| regions.get(*other).map(String::as_str) == Some(region))
        .count()
        < cap
}

/// Place each team of one pot into a distinct group (one per group) without
/// breaching the confederation cap, by backtracking. Returns whether it found a
/// full assignment.
fn place_pot(
    pot: &[String],
    used: &mut [bool],
    group_index: usize,
    groups: &mut [Vec<String>],
    regions: &HashMap<String, String>,
) -> bool {
    if group_index == groups.len() {
        return true;
    }
    for (i, code) in pot.iter().enumerate() {
        if used[i] || !group_admits(&groups[group_index], code, regions) {
            continue;
        }
        groups[group_index].push(code.clone());
        used[i] = true;
        if place_pot(pot, used, group_index + 1, groups, regions) {
            return true;
        }
        used[i] = false;
        groups[group_index].pop();
    }
    false
}

/// Draw the field into groups of four following FIFA rules: pots seeded by world
/// ranking (the host forced into Pot 1), then a random draw keeping at most one
/// team per confederation in a group — except UEFA, where two may share one.
/// Returns the groups as nation codes.
fn draw_world_cup_groups(
    game: &Game,
    field_codes: &[String],
    host_code: Option<&str>,
    pools: &BTreeMap<String, Vec<u8>>,
    rng: &mut impl Rng,
) -> Vec<Vec<String>> {
    const GROUP_SIZE: usize = 4;
    let mut ranked = ranked_field_with_pools(game, field_codes, pools);
    // The host is seeded into Pot 1 regardless of its ranking.
    if let Some(host) = host_code
        && let Some(position) = ranked.iter().position(|code| code == host)
    {
        let host = ranked.remove(position);
        ranked.insert(0, host);
    }
    // Confederation per nation, resolved once via the world's region map (which
    // honours league-defined overrides), shared by every cap check below.
    let regions: HashMap<String, String> = ranked
        .iter()
        .map(|code| (code.clone(), region_of_code(game, code)))
        .collect();
    // Round up so every team lands in a group: a field that is not a multiple of
    // four yields a few groups of three rather than silently dropping teams.
    let group_count = ranked.len().div_ceil(GROUP_SIZE).max(1);
    let mut groups: Vec<Vec<String>> = vec![Vec::new(); group_count];

    for pot_index in 0..GROUP_SIZE {
        let start = pot_index * group_count;
        let end = ((pot_index + 1) * group_count).min(ranked.len());
        if start >= end {
            break;
        }
        let mut pot: Vec<String> = ranked[start..end].to_vec();
        // A short final pot (degenerate field) can't fill one team per group, so
        // skip the constraint search and distribute it directly.
        let mut placed = pot.len() < group_count;
        // Reshuffle a few times until the confederation constraint is satisfiable.
        if !placed {
            for _ in 0..64 {
                pot.shuffle(rng);
                let mut used = vec![false; pot.len()];
                let mut trial = groups.clone();
                if place_pot(&pot, &mut used, 0, &mut trial, &regions) {
                    groups = trial;
                    placed = true;
                    break;
                }
            }
        }
        if !placed || pot.len() < group_count {
            // Either the constraint was unsatisfiable for a full pot, or this is
            // the short final pot. Still honour the confederation cap: drop each
            // team into the smallest group that admits it, falling back to an
            // even modulo spread only when no group can (an over-constrained
            // degenerate field).
            pot.shuffle(rng);
            for (offset, code) in pot.iter().enumerate() {
                let target = (0..group_count)
                    .filter(|&g| group_admits(&groups[g], code, &regions))
                    .min_by_key(|&g| groups[g].len())
                    .unwrap_or(offset % group_count);
                groups[target].push(code.clone());
            }
        }
    }
    groups
}

/// Top up thin national pools with generated free agents and (re)build the
/// national-team squads for every nation in the field.
pub(crate) fn prepare_national_squads(game: &mut Game, field: &[String]) {
    use chrono::Datelike;

    let current_year = game.clock.current_date.year() as u32;
    let pools = national_pools(game);
    for code in field {
        let have = pools.get(code).map(|ovrs| ovrs.len()).unwrap_or(0);
        for slot in have..TOPPED_UP_POOL {
            game.players.push(super::generate_national_player(
                code,
                slot,
                current_year,
                game.seed,
                &mut game.generated_count,
            ));
        }
    }

    for code in field {
        let mut squad: Vec<(String, u8)> = game
            .players
            .iter()
            .filter(|player| !player.retired)
            .filter(|player| {
                let nation = if player.football_nation.is_empty() {
                    &player.nationality
                } else {
                    &player.football_nation
                };
                nation == code
            })
            .map(|player| (player.id.clone(), player.ovr))
            .collect();
        squad.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        let squad_player_ids: Vec<String> = squad.into_iter().take(23).map(|(id, _)| id).collect();

        let nation_name_key = Some(format!("nations.{}", code.to_lowercase()));
        if let Some(team) = game
            .national_teams
            .iter_mut()
            .find(|team| &team.football_nation == code)
        {
            team.squad_player_ids = squad_player_ids;
            if team.name_key.is_none() {
                team.name_key = nation_name_key;
            }
        } else {
            let mut team = NationalTeam::new(
                national_team_id(code),
                nations::nation_display_name(code),
                code.clone(),
                nations::nation_by_code(code).map(|nation| nation.region_id.to_string()),
            );
            team.squad_player_ids = squad_player_ids;
            team.name_key = nation_name_key;
            game.national_teams.push(team);
        }
    }
}

/// Schedule a World Cup for the summer beginning at `kickoff` when the year is
/// a World Cup year and none is already running. Returns whether one was
/// scheduled.
pub fn schedule_world_cup_if_due(game: &mut Game, kickoff: DateTime<Utc>) -> bool {
    if !is_world_cup_summer(kickoff.year()) {
        return false;
    }
    if game.competitions.iter().any(is_world_cup_competition) {
        return false;
    }
    schedule_world_cup(game, kickoff, &FORMAT_48);
    true
}

/// Schedule a World Cup with an explicit format (48 by default; 32 and 16
/// reproduce the older tournaments). The field is chosen by ranking.
pub fn schedule_world_cup(game: &mut Game, kickoff: DateTime<Utc>, format: &WorldCupFormat) {
    schedule_world_cup_with_field(game, kickoff, format, None);
}

/// Schedule a World Cup, optionally with a pre-determined field (e.g. from
/// qualifying). When `predetermined_field` is `None` the field is chosen by
/// ranking the strongest nations.
pub fn schedule_world_cup_with_field(
    game: &mut Game,
    kickoff: DateTime<Utc>,
    format: &WorldCupFormat,
    predetermined_field: Option<Vec<String>>,
) {
    let year = kickoff.year();
    let host_code = host_for_year(game, year);
    let mut field = predetermined_field.unwrap_or_else(|| select_field(game, format));
    if field.len() < 4 {
        return;
    }
    // The host auto-qualifies: ensure it is in the field, displacing the
    // genuinely weakest entrant (by ranking) if the field is already full so the
    // size stays constant.
    if let Some(host) = host_code.as_deref()
        && !field.iter().any(|code| code == host)
    {
        if field.len() >= format.field
            && let Some(weakest) = ranked_field(game, &field).pop()
            && let Some(position) = field.iter().position(|code| code == &weakest)
        {
            field.remove(position);
        }
        field.push(host.to_string());
    }
    prepare_national_squads(game, &field);
    // Pools are stable now that squads are built; reuse them for both the
    // ranking seed and the draw rather than scanning the player base twice.
    let pools = national_pools(game);
    seed_world_ranking(game, &field, &pools);

    // FIFA draw: pots seeded by world ranking (host into Pot 1), one team per
    // confederation per group except UEFA (≤2). Deterministic per cup year.
    let mut draw_rng = StdRng::seed_from_u64(year as u64);
    let group_ids: Vec<Vec<String>> =
        draw_world_cup_groups(game, &field, host_code.as_deref(), &pools, &mut draw_rng)
            .iter()
            .map(|group| {
                group
                    .iter()
                    .map(|code| national_team_id_for(game, code))
                    .collect()
            })
            .collect();

    let mut cup = crate::competitions::groups::generate_group_knockout_cup_with_groups(
        &format!("World Cup {year}"),
        year as u32,
        &group_ids,
        kickoff,
        CompetitionType::InternationalNation,
        CompetitionScope::International,
        &GroupStageConfig {
            legs: 1,
            matchday_gap_days: GROUP_MATCHDAY_GAP_DAYS,
            qualifiers_per_group: format.qualifiers_per_group,
            best_third_qualifiers: format.best_third_qualifiers,
            knockout_round_gap_days: KNOCKOUT_GAP_DAYS,
            max_concurrent_matches_per_day: Some(4),
            knockout_matches_per_day: 4,
        },
    );
    // Sort after every club competition in browsing lists.
    cup.priority = 10_000;
    cup.name_key = Some("tournaments.competitions.worldCup".to_string());
    let cup_id = cup.id.clone();
    game.competitions.push(cup);
    if !game.active_competition_ids.is_empty() {
        game.active_competition_ids.push(cup_id);
    }

    // The whole world hears about a World Cup, participant or not.
    let kickoff_news_id = format!("world_cup_kickoff_{year}");
    if !game
        .news
        .iter()
        .any(|article| article.id == kickoff_news_id)
    {
        let mut params = std::collections::HashMap::new();
        params.insert("year".to_string(), year.to_string());
        params.insert("nations".to_string(), field.len().to_string());
        game.news.push(
            NewsArticle::new(
                kickoff_news_id,
                String::new(),
                String::new(),
                String::new(),
                kickoff.format("%Y-%m-%d").to_string(),
                NewsCategory::Editorial,
            )
            .with_i18n(
                "be.news.worldCupKickoff.headline",
                "be.news.worldCupKickoff.body",
                "be.source.footballHerald",
                params,
            ),
        );
    }
}

// ---------------------------------------------------------------------------
// Qualifying
// ---------------------------------------------------------------------------

const QUALIFYING_COMPETITION_PREFIX: &str = "world-cup-qualifying-";
const PLAYOFF_COMPETITION_PREFIX: &str = "world-cup-playoff-";
/// Maximum nations per qualifying group in a compressed single-season campaign;
/// a single round robin of this size fits the five international windows.
const QUALIFYING_GROUP_SIZE: usize = 6;
/// Maximum nations per group in a full two-season campaign, where groups play
/// home-and-away legs.
const FULL_CAMPAIGN_GROUP_SIZE: usize = 5;
/// Matchdays one international window hosts in a full campaign — real FIFA
/// windows fit two matches each.
const MATCHDAYS_PER_WINDOW: usize = 2;
/// Confederations that play one all-play-all league (CONMEBOL's real format,
/// and the natural shape for tiny OFC) in a full campaign, provided the double
/// round robin fits the campaign's matchday slots.
const SINGLE_LEAGUE_CONFEDERATIONS: &[&str] = &["conmebol", "ofc"];

/// Whether a competition is a World Cup qualifying campaign.
pub fn is_world_cup_qualifying(competition: &League) -> bool {
    competition.id.starts_with(QUALIFYING_COMPETITION_PREFIX)
}

/// Whether a competition is the inter-confederation playoff for the final
/// World Cup berths.
pub fn is_world_cup_playoff(competition: &League) -> bool {
    competition.id.starts_with(PLAYOFF_COMPETITION_PREFIX)
}

/// A season "leads into" a World Cup when the following summer is a cup summer.
pub fn season_leads_into_world_cup(season_start: DateTime<Utc>) -> bool {
    is_world_cup_summer(season_start.year() + 1)
}

/// A season "starts" World Cup qualifying when the cup is two summers away:
/// the full campaign spans this season and the next, home and away.
pub fn season_starts_world_cup_qualifying(season_start: DateTime<Utc>) -> bool {
    is_world_cup_summer(season_start.year() + 2)
}

fn national_team_id_for(game: &Game, code: &str) -> String {
    game.national_teams
        .iter()
        .find(|team| team.football_nation == code)
        .map(|team| team.id.clone())
        .unwrap_or_else(|| national_team_id(code))
}

pub(crate) fn nation_code_of_national_team(team_id: &str) -> String {
    team_id
        .strip_prefix("nt-")
        .map(|code| code.to_uppercase())
        .unwrap_or_else(|| team_id.to_string())
}

fn region_of_code(game: &Game, code: &str) -> String {
    game.region_for_country(code)
}

/// The FIFA confederation of a nation code: its (override-aware) region folded
/// into the six real confederations. World Cup qualifying and berth quotas
/// group by confederation, so the catalog's split Americas qualify as CONCACAF.
fn confederation_of_code(game: &Game, code: &str) -> String {
    nations::confederation_of_region(&region_of_code(game, code)).to_string()
}

/// Candidate nations grouped by confederation: every world nation with players
/// plus the catalog nations, deduped. The catalog's split Americas qualify
/// together as CONCACAF, so each confederation's campaign is a single pool.
fn qualifying_candidates_by_confederation(game: &Game) -> BTreeMap<String, Vec<String>> {
    let pools = national_pools(game);
    let mut codes: Vec<String> = pools.keys().cloned().collect();
    for nation in nations::NATION_CATALOG {
        codes.push(nation.code.to_string());
    }

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut by_confederation: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for code in codes {
        if !seen.insert(code.clone()) {
            continue;
        }
        by_confederation
            .entry(confederation_of_code(game, &code))
            .or_default()
            .push(code);
    }
    by_confederation
}

/// Allocate `field_size` World Cup berths across regions, proportional to how
/// many nations each region has (largest-remainder method), at least one berth
/// per region with a nation, capped at each region's nation count.
pub fn berths_by_region(
    field_size: usize,
    nations_by_region: &BTreeMap<String, usize>,
) -> BTreeMap<String, usize> {
    let regions: Vec<(String, usize)> = nations_by_region
        .iter()
        .filter(|(_, count)| **count > 0)
        .map(|(region, count)| (region.clone(), *count))
        .collect();
    let total: usize = regions.iter().map(|(_, count)| count).sum();
    if total == 0 || field_size == 0 {
        return BTreeMap::new();
    }

    let mut berths: BTreeMap<String, usize> = BTreeMap::new();

    // More regions than berths: one berth each to the largest regions.
    if field_size <= regions.len() {
        let mut sorted = regions;
        sorted.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        for (region, _) in sorted.into_iter().take(field_size) {
            berths.insert(region, 1);
        }
        return berths;
    }

    let mut allocated = 0usize;
    let mut remainders: Vec<(String, f64)> = Vec::new();
    for (region, count) in &regions {
        let exact = field_size as f64 * *count as f64 / total as f64;
        let base = (exact.floor() as usize).max(1).min(*count);
        berths.insert(region.clone(), base);
        allocated += base;
        remainders.push((region.clone(), exact - exact.floor()));
    }
    remainders.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    while allocated > field_size {
        if let Some((region, _)) = remainders
            .iter()
            .rev()
            .find(|(r, _)| berths.get(r).copied().unwrap_or(0) > 1)
        {
            *berths.get_mut(region).unwrap() -= 1;
            allocated -= 1;
        } else {
            break;
        }
    }
    let mut index = 0usize;
    let guard = remainders.len() * (field_size + 1);
    while allocated < field_size && index < guard {
        let (region, _) = &remainders[index % remainders.len()];
        let cap = *nations_by_region.get(region).unwrap_or(&0);
        if berths.get(region).copied().unwrap_or(0) < cap {
            *berths.entry(region.clone()).or_insert(0) += 1;
            allocated += 1;
        }
        index += 1;
    }
    berths
}

/// Real FIFA 2026 direct World Cup berths per confederation (46 of 48; the
/// final two are decided by the inter-confederation playoff).
const WORLD_CUP_DIRECT_BERTHS: &[(&str, usize)] = &[
    ("uefa", 16),
    ("caf", 9),
    ("afc", 8),
    ("conmebol", 6),
    ("concacaf", 6),
    ("ofc", 1),
];

/// Slots filled by the inter-confederation playoff (the 47th and 48th teams).
const INTER_CONFED_PLAYOFF_SPOTS: usize = 2;

/// Direct (non-playoff) World Cup berths per confederation, starting from the
/// real FIFA quota and adapting to the world's actual entrant counts: a
/// confederation can never earn more direct berths than it has entrants, and any
/// resulting shortfall is redistributed to confederations that still have spare
/// entrants, largest-quota first. The directs sum to the quota total (46) when
/// enough nations exist. The host (handled by the caller) occupies one of its
/// confederation's slots — it does not change the per-confederation counts.
fn direct_berths(entrants_by_confed: &BTreeMap<String, usize>) -> BTreeMap<String, usize> {
    let target: usize = WORLD_CUP_DIRECT_BERTHS.iter().map(|(_, n)| n).sum();
    let entrants_of = |confed: &str| entrants_by_confed.get(confed).copied().unwrap_or(0);

    let mut berths: BTreeMap<String, usize> = BTreeMap::new();
    for (confed, quota) in WORLD_CUP_DIRECT_BERTHS {
        berths.insert(confed.to_string(), (*quota).min(entrants_of(confed)));
    }
    let mut allocated: usize = berths.values().sum();

    // Redistribute the shortfall (from confederations short of entrants) to those
    // with spare capacity, in quota order so the biggest confederations absorb it.
    while allocated < target {
        let mut progressed = false;
        for (confed, _) in WORLD_CUP_DIRECT_BERTHS {
            if allocated >= target {
                break;
            }
            let current = berths.get(*confed).copied().unwrap_or(0);
            if current < entrants_of(confed) {
                berths.insert(confed.to_string(), current + 1);
                allocated += 1;
                progressed = true;
            }
        }
        if !progressed {
            break; // not enough entrants anywhere to reach the quota total
        }
    }
    berths
}

/// Matchday slots a full campaign offers across `window_count` windows:
/// [`MATCHDAYS_PER_WINDOW`] per window, minus the final window, which belongs
/// to the inter-confederation playoff. The scheduling and rollover-continue
/// paths must agree on this layout, so both derive it here.
fn full_campaign_slots(window_count: usize) -> usize {
    window_count.saturating_sub(1) * MATCHDAYS_PER_WINDOW
}

/// The campaign slot a group's `matchday` occupies. Slots run across the
/// campaign's windows ([`MATCHDAYS_PER_WINDOW`] per window in a full campaign,
/// one in a compressed season); each group's schedule is back-loaded so every
/// campaign finishes together on the last slot — short campaigns (a two-leg
/// OFC tie, a six-matchday group) start later, long ones (CONMEBOL's
/// eighteen-round league) span the whole run, mirroring the real staggered
/// starts.
fn campaign_slot(matchday: u32, group_matchdays: usize, campaign_slots: usize) -> usize {
    let index = (matchday as usize).saturating_sub(1);
    (campaign_slots.saturating_sub(group_matchdays) + index).min(campaign_slots.saturating_sub(1))
}

/// Date every `(slot, fixture)` entry: slot `s` opens on day
/// `(s % matchdays_per_window) * block` of window `s / matchdays_per_window`,
/// and a slot's matches fan out across its `block`-day share of the window span
/// so no single calendar day is swamped. Deterministic: entries are ordered by
/// `(slot, fixture id)` before day assignment.
fn date_fixtures_into_slots(
    entries: &mut [(usize, domain::league::Fixture)],
    window_dates: &[String],
    matchdays_per_window: usize,
) {
    use std::collections::BTreeMap;
    if window_dates.is_empty() {
        return;
    }
    let per_window = matchdays_per_window.max(1);
    let span = super::matches::INTERNATIONAL_WINDOW_SPAN_DAYS;
    let block = (span / per_window as i64).max(1);

    let mut per_slot: BTreeMap<usize, i64> = BTreeMap::new();
    for (slot, _) in entries.iter() {
        *per_slot.entry(*slot).or_default() += 1;
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.id.cmp(&b.1.id)));

    let mut seen: BTreeMap<usize, i64> = BTreeMap::new();
    for (slot, fixture) in entries.iter_mut() {
        let window = (*slot / per_window).min(window_dates.len() - 1);
        let Some(base) = chrono::NaiveDate::parse_from_str(&window_dates[window], "%Y-%m-%d").ok()
        else {
            continue;
        };
        let sub = (*slot % per_window) as i64;
        let count = per_slot.get(slot).copied().unwrap_or(1);
        let per_day = ((count + block - 1) / block).max(1);
        let index = seen.entry(*slot).or_default();
        let offset = sub * block + (*index / per_day).min(block - 1);
        *index += 1;
        fixture.date = (base + chrono::Duration::days(offset))
            .format("%Y-%m-%d")
            .to_string();
    }
}

/// The international windows of the season starting in `start_year`'s August —
/// window dates are fixed calendar days, so a campaign scheduled two seasons
/// ahead can date its second half exactly (and the intermediate rollover
/// re-anchors it anyway).
fn season_window_dates(start_year: i32) -> Vec<String> {
    let start = crate::competition_schedule::date_str_to_utc(&format!("{start_year}-08-01"))
        .expect("validated international calendar date");
    super::matches::international_window_dates(start)
}

fn standing_order(a: &StandingEntry, b: &StandingEntry) -> std::cmp::Ordering {
    b.points
        .cmp(&a.points)
        .then(b.goal_difference().cmp(&a.goal_difference()))
        .then(b.goals_for.cmp(&a.goals_for))
}

/// Schedule World Cup qualifying for `wc_year` across the season's
/// international windows. When the cup is two summers away (a **full
/// campaign**), confederations play their real formats home and away —
/// CONMEBOL one all-play-all league, the rest groups of up to
/// [`FULL_CAMPAIGN_GROUP_SIZE`] — across both seasons' windows, two matchdays
/// per window, with the final window kept free for the inter-confederation
/// playoff. When only one season remains (a fresh save starting late), the
/// campaign is **compressed**: single-leg groups of up to
/// [`QUALIFYING_GROUP_SIZE`], one matchday per window, as before. National
/// squads are prepared for every candidate nation.
pub fn schedule_world_cup_qualifying(game: &mut Game, wc_year: i32, window_dates: &[String]) {
    if window_dates.is_empty() {
        return;
    }
    let candidates = qualifying_candidates_by_confederation(game);
    let all_codes: Vec<String> = candidates.values().flatten().cloned().collect();
    if all_codes.len() < 4 {
        return;
    }
    prepare_national_squads(game, &all_codes);

    // The span is inferred from the windows: windows opening two years before
    // the cup mean the campaign has both seasons to play with.
    let first_window_year = chrono::NaiveDate::parse_from_str(&window_dates[0], "%Y-%m-%d")
        .map(|date| date.year())
        .unwrap_or(wc_year - 1);
    let full_campaign = wc_year - first_window_year >= 2;
    let (campaign_windows, matchdays_per_window, campaign_slots, group_size, legs) =
        if full_campaign {
            let mut windows = window_dates.to_vec();
            windows.extend(season_window_dates(first_window_year + 1));
            let slots = full_campaign_slots(windows.len());
            (
                windows,
                MATCHDAYS_PER_WINDOW,
                slots,
                FULL_CAMPAIGN_GROUP_SIZE,
                2u8,
            )
        } else {
            let slots = window_dates.len();
            (
                window_dates.to_vec(),
                1usize,
                slots,
                QUALIFYING_GROUP_SIZE,
                1u8,
            )
        };

    let competition_id = format!("{QUALIFYING_COMPETITION_PREFIX}{wc_year}");
    let mut competition = League::new(
        competition_id.clone(),
        format!("World Cup Qualifying {wc_year}"),
        wc_year as u32,
        &[],
    );
    competition.kind = CompetitionType::InternationalNation;
    competition.scope = CompetitionScope::International;
    competition.rules.format = CompetitionFormat::LeagueTable;
    competition.standings.clear();
    competition.priority = 9_000;
    competition.name_key = Some("tournaments.competitions.worldCupQualifying".to_string());

    let base_date = crate::competition_schedule::date_str_to_utc(&window_dates[0])
        .expect("validated international calendar date");

    let mut participant_ids: Vec<String> = Vec::new();
    let mut dated_fixtures: Vec<(usize, domain::league::Fixture)> = Vec::new();
    for (confederation, codes) in &candidates {
        // A single-league confederation plays one all-play-all table when its
        // double round robin fits the campaign; everyone else plays groups.
        let single_league = full_campaign
            && SINGLE_LEAGUE_CONFEDERATIONS.contains(&confederation.as_str())
            && round_robin_matchdays(codes.len(), legs as usize) <= campaign_slots;
        let group_count = if single_league {
            1
        } else {
            codes.len().div_ceil(group_size).max(1)
        };
        let mut groups: Vec<Vec<String>> = vec![Vec::new(); group_count];
        for (index, code) in codes.iter().enumerate() {
            groups[index % group_count].push(code.clone());
        }

        for (group_index, group_codes) in groups.iter().enumerate() {
            if group_codes.len() < 2 {
                continue;
            }
            let team_ids: Vec<String> = group_codes
                .iter()
                .map(|code| national_team_id_for(game, code))
                .collect();
            participant_ids.extend(team_ids.iter().cloned());

            let group_matchdays = round_robin_matchdays(team_ids.len(), legs as usize);
            let fixtures = crate::competition_schedule::build_round_robin_fixtures_with(
                &competition_id,
                &team_ids,
                base_date,
                FixtureCompetition::InternationalNation,
                legs,
                3,
            );
            dated_fixtures.extend(fixtures.into_iter().map(|fixture| {
                let slot = campaign_slot(fixture.matchday, group_matchdays, campaign_slots);
                (slot, fixture)
            }));
            competition.groups.push(GroupState {
                id: format!("{competition_id}-{confederation}-{group_index}"),
                name: format!("{confederation} {}", group_index + 1),
                team_ids: team_ids.clone(),
                standings: team_ids
                    .iter()
                    .map(|id| StandingEntry::new(id.clone()))
                    .collect(),
            });
        }
    }

    if competition.groups.is_empty() {
        return;
    }
    // Slots share windows; spread each slot's matches across its share of the
    // window's multi-day block so no single calendar day is swamped.
    date_fixtures_into_slots(&mut dated_fixtures, &campaign_windows, matchdays_per_window);
    competition.fixtures = dated_fixtures
        .into_iter()
        .map(|(_, fixture)| fixture)
        .collect();
    competition.participant_ids = participant_ids;
    let competition_id = competition.id.clone();
    game.competitions.push(competition);
    if !game.active_competition_ids.is_empty() {
        game.active_competition_ids.push(competition_id);
    }

    // News: the qualifying campaign gets under way.
    let news_id = format!("world_cup_qualifying_{wc_year}");
    if !game.news.iter().any(|article| article.id == news_id) {
        let mut params = std::collections::HashMap::new();
        params.insert("year".to_string(), wc_year.to_string());
        params.insert("nations".to_string(), all_codes.len().to_string());
        game.news.push(
            NewsArticle::new(
                news_id,
                String::new(),
                String::new(),
                String::new(),
                window_dates[0].clone(),
                NewsCategory::Editorial,
            )
            .with_i18n(
                "be.news.worldCupQualifying.headline",
                "be.news.worldCupQualifying.body",
                "be.source.footballHerald",
                params,
            ),
        );
    }
}

/// Carry an in-progress two-season qualifying campaign across the intermediate
/// rollover: settle any first-season fixtures the rollover outran (the club
/// season can end in mid-May, before the June window plays), re-date every
/// still-scheduled fixture onto the new season's actual international windows
/// (the second half of the campaign's slot layout), and refresh national
/// squads after a summer of transfers and retirements. Fixtures already
/// played keep their dates and results untouched.
pub fn continue_world_cup_qualifying(game: &mut Game, window_dates: &[String], rng: &mut impl Rng) {
    if window_dates.is_empty() {
        return;
    }
    let qualifying_indices: Vec<usize> = game
        .competitions
        .iter()
        .enumerate()
        .filter(|(_, competition)| is_world_cup_qualifying(competition))
        .map(|(index, _)| index)
        .collect();
    if qualifying_indices.is_empty() {
        return;
    }

    // Fixtures dated before the new season's first window belong to the
    // finished half of the campaign; play them out now — as the cup-summer
    // rollover does — instead of dragging them into the new season and
    // double-booking nations on its opening window.
    let mut stranded: Vec<String> = game
        .competitions
        .iter()
        .filter(|competition| is_world_cup_qualifying(competition))
        .flat_map(|competition| competition.fixtures.iter())
        .filter(|fixture| {
            fixture.status == FixtureStatus::Scheduled
                && fixture.date.as_str() < window_dates[0].as_str()
        })
        .map(|fixture| fixture.date.clone())
        .collect();
    stranded.sort();
    stranded.dedup();
    for date in stranded {
        process_world_cup_fixtures_due(game, &date, rng);
    }

    // Squads next: the campaign's second half should field current players.
    let codes: Vec<String> = qualifying_indices
        .iter()
        .flat_map(|index| game.competitions[*index].participant_ids.iter())
        .map(|team_id| nation_code_of_national_team(team_id))
        .collect();
    prepare_national_squads(game, &codes);

    // The campaign was laid out over both seasons' windows (minus the playoff
    // window); the second season's slots start after the first season's share.
    let first_season_slots = window_dates.len() * MATCHDAYS_PER_WINDOW;
    let campaign_slots = full_campaign_slots(window_dates.len() * 2);

    for competition_index in qualifying_indices {
        let competition = &mut game.competitions[competition_index];

        // A surviving campaign is always the full home-and-away span (a
        // compressed one finishes within its own season), so each group's
        // schedule length is the double round robin of its size — the same
        // derivation the scheduler used. Keyed by team; groups are disjoint.
        let mut matchdays_by_team: HashMap<String, usize> = HashMap::new();
        for group in &competition.groups {
            let group_matchdays = round_robin_matchdays(group.team_ids.len(), 2);
            for team_id in &group.team_ids {
                matchdays_by_team.insert(team_id.clone(), group_matchdays);
            }
        }

        let fixtures = std::mem::take(&mut competition.fixtures);
        let (played, scheduled): (Vec<_>, Vec<_>) = fixtures
            .into_iter()
            .partition(|fixture| fixture.status != FixtureStatus::Scheduled);

        let mut dated: Vec<(usize, domain::league::Fixture)> = scheduled
            .into_iter()
            .map(|fixture| {
                let group_matchdays = matchdays_by_team
                    .get(&fixture.home_team_id)
                    .copied()
                    .unwrap_or(campaign_slots);
                let slot = campaign_slot(fixture.matchday, group_matchdays, campaign_slots);
                // A fixture stranded in the finished first season (a window the
                // save never simulated) squeezes into the new season's opening
                // slot so it still gets played.
                (slot.saturating_sub(first_season_slots), fixture)
            })
            .collect();
        date_fixtures_into_slots(&mut dated, window_dates, MATCHDAYS_PER_WINDOW);

        competition.fixtures = played;
        competition
            .fixtures
            .extend(dated.into_iter().map(|(_, fixture)| fixture));
    }
}

/// Inter-confederation playoff entrants per confederation: the best teams beyond
/// each confederation's direct quota. CONCACAF (the host confederation) sends
/// two, the other non-UEFA confederations one each — six teams contesting the
/// final two berths, as in the real 2026 format.
const PLAYOFF_ENTRANTS_BY_CONFED: &[(&str, usize)] = &[
    ("caf", 1),
    ("afc", 1),
    ("conmebol", 1),
    ("ofc", 1),
    ("concacaf", 2),
];

/// Nation codes finishing across a confederation's groups, best first: every
/// group winner (ordered among themselves by `standing_order`), then every
/// runner-up, and so on down the tables.
fn rank_confederation_finishers(groups: &[Vec<StandingEntry>]) -> Vec<String> {
    let max_rank = groups.iter().map(|group| group.len()).max().unwrap_or(0);
    let mut finishers = Vec::new();
    for rank in 0..max_rank {
        let mut at_rank: Vec<&StandingEntry> =
            groups.iter().filter_map(|group| group.get(rank)).collect();
        at_rank.sort_by(|a, b| standing_order(a, b));
        finishers.extend(
            at_rank
                .into_iter()
                .map(|e| nation_code_of_national_team(&e.team_id)),
        );
    }
    finishers
}

/// Resolve a single playoff tie through the national-team knockout engine
/// (regulation → extra time → penalties); returns the advancing nation code.
fn play_playoff_tie(game: &mut Game, home: &str, away: &str, rng: &mut impl Rng) -> String {
    let home_id = national_team_id_for(game, home);
    let away_id = national_team_id_for(game, away);
    let (home_goals, away_goals, _, _, home_pens, away_pens) =
        super::matches::play_national_knockout_match(game, &home_id, &away_id, rng);
    let home_advances = if home_goals != away_goals {
        home_goals > away_goals
    } else {
        home_pens.unwrap_or(0) >= away_pens.unwrap_or(0)
    };
    if home_advances {
        home.to_string()
    } else {
        away.to_string()
    }
}

/// The inter-confederation playoff: `entrants` (ranking order) contest the final
/// [`INTER_CONFED_PLAYOFF_SPOTS`] berths. The top seeds bye to the finals; the
/// rest play a preliminary round; the final winners qualify.
fn resolve_inter_confed_playoff(
    game: &mut Game,
    entrants: &[String],
    rng: &mut impl Rng,
) -> Vec<String> {
    let spots = INTER_CONFED_PLAYOFF_SPOTS;
    if entrants.len() <= spots {
        return entrants.to_vec();
    }
    let ranked = ranked_field(game, entrants);
    let seeds: Vec<String> = ranked.iter().take(spots).cloned().collect();
    let rest: Vec<String> = ranked.iter().skip(spots).cloned().collect();

    // Preliminary round: strongest of the rest against weakest.
    let mut prelim_winners: Vec<String> = Vec::new();
    let half = rest.len() / 2;
    for i in 0..half {
        prelim_winners.push(play_playoff_tie(
            game,
            &rest[i],
            &rest[rest.len() - 1 - i],
            rng,
        ));
    }
    if rest.len() % 2 == 1 {
        prelim_winners.push(rest[half].clone());
    }

    // Finals: each seed faces a preliminary winner.
    let mut qualifiers: Vec<String> = Vec::new();
    for (index, seed) in seeds.iter().enumerate() {
        match prelim_winners.get(index) {
            Some(opponent) => qualifiers.push(play_playoff_tie(game, seed, opponent, rng)),
            None => qualifiers.push(seed.clone()),
        }
    }
    qualifiers.truncate(spots);
    qualifiers
}

/// Announce the inter-confederation playoff qualifiers to the world.
fn announce_inter_confed_playoff(game: &mut Game, year: u32, winners: &[String]) {
    if winners.is_empty() {
        return;
    }
    let news_id = format!("world_cup_playoff_{year}");
    if game.news.iter().any(|article| article.id == news_id) {
        return;
    }
    let mut params = std::collections::HashMap::new();
    params.insert("year".to_string(), year.to_string());
    let names: Vec<String> = winners
        .iter()
        .map(|code| nations::nation_display_name(code))
        .collect();
    params.insert("nations".to_string(), names.join(", "));
    let date = game.clock.current_date.format("%Y-%m-%d").to_string();
    game.news.push(
        NewsArticle::new(
            news_id,
            String::new(),
            String::new(),
            String::new(),
            date,
            NewsCategory::Editorial,
        )
        .with_i18n(
            "be.news.worldCupPlayoff.headline",
            "be.news.worldCupPlayoff.body",
            "be.source.footballHerald",
            params,
        ),
    );
}

/// The playoff's two qualifiers (nation codes), once its finals round — two
/// parallel ties, no byes — has been played. `None` while it is undecided.
fn playoff_winners_of(cup: &League) -> Option<Vec<String>> {
    if cup.knockout_rounds.len() < 2 {
        return None;
    }
    let finals = cup.knockout_rounds.last()?;
    if !finals.completed || !finals.bye_team_ids.is_empty() {
        return None;
    }
    let mut winners = Vec::new();
    for fixture_id in &finals.fixture_ids {
        let fixture = cup
            .fixtures
            .iter()
            .find(|fixture| &fixture.id == fixture_id)?;
        winners.push(nation_code_of_national_team(fixture.advancing_team_id()?));
    }
    (winners.len() == INTER_CONFED_PLAYOFF_SPOTS).then_some(winners)
}

/// The qualifiers of `year`'s inter-confederation playoff competition, when it
/// was staged and has been decided.
fn decided_playoff_winners(game: &Game, year: u32) -> Option<Vec<String>> {
    let cup = game
        .competitions
        .iter()
        .find(|competition| is_world_cup_playoff(competition) && competition.season == year)?;
    playoff_winners_of(cup)
}

/// Advance the inter-confederation playoff once its current round is fully
/// played. Unlike a standard bracket it crowns no single champion: completing
/// the preliminaries seeds two parallel finals — each bye seed against a
/// preliminary winner, the top seed meeting the winner of the weaker-seeded
/// tie — and completing the finals simply decides the two berths, with no
/// further round.
fn advance_world_cup_playoff(cup: &mut League) {
    let rounds_before = cup.knockout_rounds.len();
    crate::competition_schedule::advance_knockout_round_with(cup, |winners, seeds| {
        if seeds.is_empty() {
            return None; // the finals: berths decided, the bracket ends here
        }
        let mut order: Vec<String> = Vec::new();
        for (seed, winner) in seeds.iter().zip(winners.iter().rev()) {
            order.push(seed.clone());
            order.push(winner.clone());
        }
        (!order.is_empty()).then_some(order)
    });
    // The freshly seeded round is the pair of parallel finals.
    if cup.knockout_rounds.len() > rounds_before
        && let Some(finals) = cup.knockout_rounds.last_mut()
    {
        finals.name = "Final".to_string();
    }
}

/// Stage the inter-confederation playoff as a real, visible knockout in the
/// campaign's final international window once every qualifying group has
/// finished. Only a full campaign gets one — it completes in the March window
/// with the June window free — and only in the real six-entrant shape (two
/// seeds bye to the finals, four contest the preliminaries); a compressed
/// campaign, or a degenerate world, resolves its playoff synthetically at the
/// rollover instead.
fn stage_world_cup_playoff_if_ready(game: &mut Game, today: &str) {
    // The cheap gates run first: this is called on every day that simulates a
    // qualifying fixture, and sorting every group's standings below is only
    // worth it once the campaign has actually finished.
    if game.competitions.iter().any(is_world_cup_playoff) {
        return;
    }
    let Some(year) = game
        .competitions
        .iter()
        .find(|competition| is_world_cup_qualifying(competition))
        .map(|competition| competition.season)
    else {
        return;
    };
    let all_played = game
        .competitions
        .iter()
        .filter(|competition| is_world_cup_qualifying(competition))
        .all(|competition| {
            competition
                .fixtures
                .iter()
                .all(|fixture| fixture.status != FixtureStatus::Scheduled)
        });
    if !all_played {
        return;
    }
    // The playoff owns the campaign's last window (June of the cup year); a
    // campaign still running by then has no room for a scheduled bracket.
    let Some(playoff_date) = season_window_dates(year as i32 - 1).pop() else {
        return;
    };
    if today >= playoff_date.as_str() {
        return;
    }
    let Some((year, groups_by_confed, entrants_by_confed)) =
        qualifying_groups_by_confederation(game)
    else {
        return;
    };
    // Quotas and the playoff only exist at 48-team scale.
    if entrants_by_confed.values().sum::<usize>() < FORMAT_48.field {
        return;
    }

    let host = host_for_year(game, year as i32);
    let outcome = split_qualifying_outcome(
        game,
        &groups_by_confed,
        &entrants_by_confed,
        host.as_deref(),
    );
    let ranked = ranked_field(game, &outcome.playoff_entrants);
    if ranked.len() != INTER_CONFED_PLAYOFF_SPOTS + 4 {
        return;
    }
    let entrant_ids: Vec<String> = {
        let seeds = &ranked[..INTER_CONFED_PLAYOFF_SPOTS];
        let rest = &ranked[INTER_CONFED_PLAYOFF_SPOTS..];
        // Seeds first (they bye to the finals), then the preliminaries pair
        // the strongest of the rest against the weakest.
        let order = [
            seeds[0].clone(),
            seeds[1].clone(),
            rest[0].clone(),
            rest[3].clone(),
            rest[1].clone(),
            rest[2].clone(),
        ];
        order
            .iter()
            .map(|code| national_team_id_for(game, code))
            .collect()
    };

    let competition_id = format!("{PLAYOFF_COMPETITION_PREFIX}{year}");
    let mut cup = League::new(
        competition_id.clone(),
        format!("World Cup Play-off {year}"),
        year,
        &[],
    );
    cup.kind = CompetitionType::InternationalNation;
    cup.scope = CompetitionScope::International;
    cup.rules.format = CompetitionFormat::Knockout;
    cup.rules.knockout_round_gap_days = KNOCKOUT_GAP_DAYS;
    cup.rules.knockout_matches_per_day = 2;
    cup.standings.clear();
    cup.priority = 9_500;
    cup.name_key = Some("tournaments.competitions.worldCupPlayoff".to_string());
    cup.participant_ids = entrant_ids.clone();

    let prelim_start = crate::competition_schedule::date_str_to_utc(&playoff_date)
        .expect("validated international calendar date");
    crate::competition_schedule::seed_knockout_round(
        &mut cup,
        &entrant_ids,
        prelim_start,
        FixtureCompetition::InternationalNation,
    );
    if let Some(preliminaries) = cup.knockout_rounds.last_mut() {
        preliminaries.name = "Semifinal".to_string();
    }

    game.competitions.push(cup);
    if !game.active_competition_ids.is_empty() {
        game.active_competition_ids.push(competition_id);
    }

    // News: the playoff line-up is set.
    let news_id = format!("world_cup_playoff_draw_{year}");
    if !game.news.iter().any(|article| article.id == news_id) {
        let mut params = std::collections::HashMap::new();
        params.insert("year".to_string(), year.to_string());
        let names: Vec<String> = ranked
            .iter()
            .map(|code| nations::nation_display_name(code))
            .collect();
        params.insert("nations".to_string(), names.join(", "));
        game.news.push(
            NewsArticle::new(
                news_id,
                String::new(),
                String::new(),
                String::new(),
                today.to_string(),
                NewsCategory::Editorial,
            )
            .with_i18n(
                "be.news.worldCupPlayoffDraw.headline",
                "be.news.worldCupPlayoffDraw.body",
                "be.source.footballHerald",
                params,
            ),
        );
    }
}

/// Fast-forward every World Cup qualifying and playoff fixture still scheduled
/// when the cup-summer rollover arrives. Club seasons can finish in mid-May,
/// before the campaign's June dates play out, and the finals field must be
/// derived from a *finished* campaign — so the outstanding international dates
/// are settled here, through the ordinary fixture processor (results, group
/// tables, the playoff bracket, rankings, and news all land as if those days
/// had been played), rather than dropped with the retired competitions.
///
/// Runs to a fixpoint because settling can itself add fixtures: completing the
/// groups stages the playoff, and completing its preliminaries seeds the
/// finals.
pub fn settle_outstanding_qualifying(game: &mut Game, rng: &mut impl Rng) {
    // Each pass drains one generation of fixtures — the groups, then the
    // playoff preliminaries its completion stages, then the finals — so four
    // passes always reach quiescence; the bound is just a hard stop.
    for _ in 0..4 {
        let mut dates: Vec<String> = game
            .competitions
            .iter()
            .filter(|competition| {
                is_world_cup_qualifying(competition) || is_world_cup_playoff(competition)
            })
            .flat_map(|competition| competition.fixtures.iter())
            .filter(|fixture| fixture.status == FixtureStatus::Scheduled)
            .map(|fixture| fixture.date.clone())
            .collect();
        if dates.is_empty() {
            return;
        }
        dates.sort();
        dates.dedup();
        for date in dates {
            process_world_cup_fixtures_due(game, &date, rng);
        }
    }
}

/// Sorted final standings of every qualifying group, keyed by confederation.
type GroupsByConfederation = BTreeMap<String, Vec<Vec<StandingEntry>>>;

/// Every qualifying group's sorted standings grouped by confederation, plus
/// per-confederation entrant counts and the campaign's cup year. `None` when
/// no qualifying campaign exists.
fn qualifying_groups_by_confederation(
    game: &Game,
) -> Option<(u32, GroupsByConfederation, BTreeMap<String, usize>)> {
    let competition = game
        .competitions
        .iter()
        .find(|competition| is_world_cup_qualifying(competition))?;
    let mut groups_by_confed: BTreeMap<String, Vec<Vec<StandingEntry>>> = BTreeMap::new();
    let mut entrants_by_confed: BTreeMap<String, usize> = BTreeMap::new();
    for group in &competition.groups {
        let confederation = group
            .team_ids
            .first()
            .map(|id| confederation_of_code(game, &nation_code_of_national_team(id)))
            .unwrap_or_else(|| "uefa".to_string());
        let sorted = crate::competitions::groups::sorted_group_standings(group);
        *entrants_by_confed.entry(confederation.clone()).or_insert(0) += sorted.len();
        groups_by_confed
            .entry(confederation)
            .or_default()
            .push(sorted);
    }
    Some((competition.season, groups_by_confed, entrants_by_confed))
}

/// A completed qualifying campaign split by the real quotas: the direct
/// qualifiers (host slot reserved inside its confederation), the
/// inter-confederation playoff entrants, and the below-the-cut reserves kept
/// strongest-first per confederation as a backfill pool.
struct QualifyingOutcome {
    direct_field: Vec<String>,
    playoff_entrants: Vec<String>,
    reserves: Vec<String>,
}

fn split_qualifying_outcome(
    game: &Game,
    groups_by_confed: &GroupsByConfederation,
    entrants_by_confed: &BTreeMap<String, usize>,
    host_code: Option<&str>,
) -> QualifyingOutcome {
    let directs = direct_berths(entrants_by_confed);
    let host = host_code.map(str::to_string);
    let host_confed = host
        .as_deref()
        .map(|code| confederation_of_code(game, code));

    let mut direct_field: Vec<String> = Vec::new();
    let mut playoff_entrants: Vec<String> = Vec::new();
    let mut reserves: Vec<String> = Vec::new();
    for (confederation, groups) in groups_by_confed {
        let direct_quota = directs.get(confederation).copied().unwrap_or(0);
        let playoff_count = PLAYOFF_ENTRANTS_BY_CONFED
            .iter()
            .find(|(confed, _)| confed == confederation)
            .map(|(_, count)| *count)
            .unwrap_or(0);
        let reserve_host = host_confed.as_deref() == Some(confederation.as_str());

        // Reserve the host inside its confederation's quota; it fills one slot.
        let mut directs_taken: Vec<String> = Vec::new();
        if reserve_host && let Some(host_code) = &host {
            directs_taken.push(host_code.clone());
        }
        let mut playoff_taken = 0usize;
        for code in rank_confederation_finishers(groups) {
            if reserve_host && host.as_deref() == Some(code.as_str()) {
                continue; // the host already holds a slot
            }
            if directs_taken.len() < direct_quota {
                directs_taken.push(code);
            } else if playoff_taken < playoff_count {
                playoff_entrants.push(code.clone());
                playoff_taken += 1;
                reserves.push(code);
            } else {
                reserves.push(code);
            }
        }
        direct_field.extend(directs_taken);
    }
    QualifyingOutcome {
        direct_field,
        playoff_entrants,
        reserves,
    }
}

/// The qualified field (nation codes) from a completed qualifying campaign.
///
/// For the 48-team finals this applies the real FIFA confederation quotas
/// (UEFA 16, CAF 9, AFC 8, CONMEBOL 6, CONCACAF 6, OFC 1), reserves a slot for
/// the host within its confederation, and decides the last two berths through
/// an inter-confederation playoff. Smaller formats keep the simple
/// size-proportional split. Returns `None` when there is no qualifying campaign.
pub fn qualified_field_from_game(
    game: &mut Game,
    field_size: usize,
    host_code: Option<&str>,
) -> Option<Vec<String>> {
    // Collect every group's sorted standings, grouped by confederation, into
    // owned data so the immutable borrow ends before the playoff.
    let (year, groups_by_confed, entrants_by_confed) = qualifying_groups_by_confederation(game)?;

    // Smaller formats (small worlds, tests) keep the proportional split — real
    // FIFA quotas and the playoff only make sense for the 48-team finals.
    if field_size != FORMAT_48.field {
        let berths = berths_by_region(field_size, &entrants_by_confed);
        let mut field: Vec<String> = Vec::new();
        for (confederation, groups) in &groups_by_confed {
            let want = berths.get(confederation).copied().unwrap_or(0);
            field.extend(rank_confederation_finishers(groups).into_iter().take(want));
        }
        return Some(field);
    }

    let outcome = split_qualifying_outcome(game, &groups_by_confed, &entrants_by_confed, host_code);
    let mut field = outcome.direct_field;
    let reserves = outcome.reserves;

    // Last two berths via the inter-confederation playoff: read the visible
    // knockout when a full campaign staged one; a compressed campaign (no time
    // for a scheduled bracket) resolves it synthetically here instead.
    let playoff_winners = match decided_playoff_winners(game, year) {
        Some(winners) => winners,
        None => {
            let mut rng = StdRng::seed_from_u64(u64::from(year) ^ 0xF1FA);
            let winners = resolve_inter_confed_playoff(game, &outcome.playoff_entrants, &mut rng);
            announce_inter_confed_playoff(game, year, &winners);
            winners
        }
    };
    field.extend(playoff_winners);

    // Safety net: a lopsided world (e.g. one confederation holding nearly every
    // entrant) can starve the non-UEFA playoff pool of two winners, leaving the
    // field short. Backfill from the strongest remaining qualifiers so the field
    // still reaches its target when entrants allow. With the real catalog the
    // pool always yields two winners, so this never fires there; B2's distinct
    // per-confederation formats keep the pools full too, leaving it a B1 net.
    if field.len() < field_size {
        let chosen: std::collections::HashSet<String> = field.iter().cloned().collect();
        let leftovers: Vec<String> = reserves
            .into_iter()
            .filter(|code| !chosen.contains(code))
            .collect();
        for code in ranked_field(game, &leftovers) {
            if field.len() >= field_size {
                break;
            }
            field.push(code);
        }
    }

    Some(field)
}

/// Simulate every World Cup fixture due on `today` with the national-team
/// engine (carry-back included), progressing groups and knockout rounds, and
/// announcing the champion when the final is decided. Returns the number of
/// fixtures simulated.
pub fn process_world_cup_fixtures_due(game: &mut Game, today: &str, rng: &mut impl Rng) -> usize {
    let competition_indices: Vec<usize> = game
        .competitions
        .iter()
        .enumerate()
        .filter(|(_, competition)| is_world_cup_competition(competition))
        .map(|(index, _)| index)
        .collect();

    let mut simulated = 0;
    for competition_index in competition_indices {
        let due: Vec<usize> = game.competitions[competition_index]
            .fixtures
            .iter()
            .enumerate()
            .filter(|(_, fixture)| {
                fixture.date == today && fixture.status == FixtureStatus::Scheduled
            })
            .map(|(fixture_index, _)| fixture_index)
            .collect();

        for fixture_index in due {
            let (home_id, away_id, fixture_id) = {
                let fixture = &game.competitions[competition_index].fixtures[fixture_index];
                (
                    fixture.home_team_id.clone(),
                    fixture.away_team_id.clone(),
                    fixture.id.clone(),
                )
            };
            // Knockout ties must produce a winner: extra time, then penalties.
            let is_knockout = game.competitions[competition_index]
                .knockout_rounds
                .iter()
                .any(|round| round.fixture_ids.contains(&fixture_id));
            let (
                home_goals,
                away_goals,
                home_scorers,
                away_scorers,
                home_penalties,
                away_penalties,
            ) = if is_knockout {
                super::matches::play_national_knockout_match(game, &home_id, &away_id, rng)
            } else {
                let (home_goals, away_goals, home_scorers, away_scorers) =
                    super::matches::play_national_match(game, &home_id, &away_id, rng);
                (
                    home_goals,
                    away_goals,
                    home_scorers,
                    away_scorers,
                    None,
                    None,
                )
            };

            let competition = &mut game.competitions[competition_index];
            let fixture = &mut competition.fixtures[fixture_index];
            fixture.status = FixtureStatus::Completed;
            fixture.result = Some(MatchResult {
                home_goals,
                away_goals,
                home_scorers,
                away_scorers,
                report: None,
                home_penalties,
                away_penalties,
            });
            crate::competitions::groups::process_completed_fixture(competition, fixture_index);
            // The playoff bracket stops at two parallel finals instead of
            // converging on a champion, so it advances through its own logic.
            if is_world_cup_playoff(competition) {
                advance_world_cup_playoff(competition);
            } else {
                crate::competition_schedule::advance_knockout_competition_round(competition);
            }
            game.world_history.apply_national_result(
                &nation_code_of_national_team(&home_id),
                &nation_code_of_national_team(&away_id),
                home_goals,
                away_goals,
                home_penalties.is_some(),
            );
            simulated += 1;
        }

        announce_champion_if_decided(game, competition_index, today, rng);
        // The playoff's completion is news the moment its finals are played.
        let competition = &game.competitions[competition_index];
        if is_world_cup_playoff(competition)
            && let Some(winners) = playoff_winners_of(competition)
        {
            let year = competition.season;
            announce_inter_confed_playoff(game, year, &winners);
        }
    }

    // A full campaign that just finished (all groups decided by the March
    // window) stages the inter-confederation playoff in the free June window.
    if simulated > 0 {
        stage_world_cup_playoff_if_ready(game, today);
    }
    simulated
}

/// The tournament winner, once the final has been played.
pub fn world_cup_champion(competition: &League) -> Option<String> {
    let last_round = competition.knockout_rounds.last()?;
    if !last_round.completed || last_round.fixture_ids.len() != 1 {
        return None;
    }
    let final_fixture = competition
        .fixtures
        .iter()
        .find(|fixture| last_round.fixture_ids.contains(&fixture.id))?;
    let result = final_fixture.result.as_ref()?;
    Some(if result.advancing_is_home() {
        final_fixture.home_team_id.clone()
    } else {
        final_fixture.away_team_id.clone()
    })
}

fn announce_champion_if_decided(
    game: &mut Game,
    competition_index: usize,
    today: &str,
    rng: &mut impl Rng,
) {
    let competition = &game.competitions[competition_index];
    let Some(champion_id) = world_cup_champion(competition) else {
        return;
    };
    let year = competition.season;
    let msg_id = format!("world_cup_champion_{year}");
    if game.messages.iter().any(|message| message.id == msg_id) {
        return;
    }

    let (nation, nation_code) = game
        .national_teams
        .iter()
        .find(|team| team.id == champion_id)
        .map(|team| (team.name.clone(), team.football_nation.clone()))
        .unwrap_or((champion_id.clone(), String::new()));

    // The game world's highest honour goes into the hall of fame.
    game.world_history
        .record_world_cup_champion(WorldCupChampionRecord {
            year,
            nation_code,
            nation_name: nation.clone(),
        });

    let mut params = std::collections::HashMap::new();
    params.insert("nation".to_string(), nation);
    params.insert("year".to_string(), year.to_string());

    let message = InboxMessage::new(
        msg_id,
        String::new(),
        String::new(),
        String::new(),
        today.to_string(),
    )
    .with_category(MessageCategory::LeagueInfo)
    .with_priority(MessagePriority::High)
    .with_sender_role("")
    .with_i18n(
        "be.msg.worldCupChampion.subject",
        "be.msg.worldCupChampion.body",
        params.clone(),
    )
    .with_sender_i18n("be.sender.intlLiaison", "be.role.intlLiaison");
    game.messages.push(message);

    // Front-page news for everyone, participant or not.
    let news_id = format!("world_cup_champion_news_{year}");
    if !game.news.iter().any(|article| article.id == news_id) {
        game.news.push(
            NewsArticle::new(
                news_id,
                String::new(),
                String::new(),
                String::new(),
                today.to_string(),
                NewsCategory::Editorial,
            )
            .with_i18n(
                "be.news.worldCupChampion.headline",
                "be.news.worldCupChampion.body",
                "be.source.footballHerald",
                params,
            ),
        );
    }

    // With the champion crowned, the bid race for a future edition is decided.
    award_next_world_cup_host(game, year, today, rng);
}

/// After a World Cup ends, award the next edition whose host is not already
/// known (real-world hosts stand for their years). A handful of strong nations
/// that have not hosted recently form a shortlist; one is chosen at random.
/// Both the bid race and the award make the news, and the host is stored so it
/// auto-qualifies and is seeded into Pot 1 of that tournament's draw.
fn award_next_world_cup_host(game: &mut Game, played_year: u32, today: &str, rng: &mut impl Rng) {
    let next_year = played_year + 4;
    if host_for_year(game, next_year as i32).is_some() {
        return;
    }
    // Hosts used within the last ~24 years are off the table.
    let mut recent: std::collections::HashSet<String> = std::collections::HashSet::new();
    for year in next_year.saturating_sub(24)..next_year {
        if let Some(code) = host_for_year(game, year as i32) {
            recent.insert(code);
        }
    }
    let candidates: Vec<String> = nations::NATION_CATALOG
        .iter()
        .map(|nation| nation.code.to_string())
        .filter(|code| !recent.contains(code))
        .collect();
    let shortlist = ranked_field(game, &candidates);
    if shortlist.is_empty() {
        return;
    }
    let shortlist_len = shortlist.len().min(4);
    let chosen = shortlist[rng.random_range(0..shortlist_len)].clone();
    let nation_name = nations::nation_display_name(&chosen);

    game.world_history
        .record_world_cup_host(WorldCupHostRecord {
            year: next_year,
            nation_code: chosen,
            nation_name: nation_name.clone(),
        });

    let bid_id = format!("world_cup_host_bid_{next_year}");
    if !game.news.iter().any(|article| article.id == bid_id) {
        let mut params = std::collections::HashMap::new();
        params.insert("count".to_string(), shortlist_len.to_string());
        params.insert("year".to_string(), next_year.to_string());
        game.news.push(
            NewsArticle::new(
                bid_id,
                String::new(),
                String::new(),
                String::new(),
                today.to_string(),
                NewsCategory::Editorial,
            )
            .with_i18n(
                "be.news.worldCupHostBid.headline",
                "be.news.worldCupHostBid.body",
                "be.source.footballHerald",
                params,
            ),
        );
    }

    let host_id = format!("world_cup_host_{next_year}");
    if !game.news.iter().any(|article| article.id == host_id) {
        let mut params = std::collections::HashMap::new();
        params.insert("nation".to_string(), nation_name);
        params.insert("year".to_string(), next_year.to_string());
        game.news.push(
            NewsArticle::new(
                host_id,
                String::new(),
                String::new(),
                String::new(),
                today.to_string(),
                NewsCategory::Editorial,
            )
            .with_i18n(
                "be.news.worldCupHostChosen.headline",
                "be.news.worldCupHostChosen.body",
                "be.source.footballHerald",
                params,
            ),
        );
    }
}
