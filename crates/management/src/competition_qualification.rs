//! Qualification rules from pinned end_of_season.rs (64677fee).
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
use domain::league::{BerthRule, CompetitionScope, CompetitionType, League};
use std::collections::BTreeMap;
const CONTINENTAL_LEAGUE_SLOTS: usize = 4;
struct ClubSeed {
    id: String,
    reputation: u32,
    region: String,
}
struct QualificationContext {
    competitions: Vec<League>,
    teams: Vec<ClubSeed>,
}
pub fn fields(
    competitions: &[League],
    reputations: &BTreeMap<String, u32>,
    regions: &BTreeMap<String, String>,
) -> BTreeMap<String, Vec<String>> {
    let game = QualificationContext {
        competitions: competitions.to_vec(),
        teams: reputations
            .iter()
            .map(|(id, r)| ClubSeed {
                id: id.clone(),
                reputation: *r,
                region: regions.get(id).cloned().unwrap_or_default(),
            })
            .collect(),
    };
    let berth_fields = resolve_continental_fields(&game);
    game.competitions
        .iter()
        .filter(|c| {
            c.scope == CompetitionScope::Continental && c.kind == CompetitionType::ContinentalClub
        })
        .map(|c| {
            (
                c.id.clone(),
                berth_fields
                    .get(&c.id)
                    .cloned()
                    .unwrap_or_else(|| continental_qualified_entrants(&game, c)),
            )
        })
        .collect()
}
pub fn champion(competition: &League) -> Option<String> {
    let round = competition.knockout_rounds.last()?;
    if !round.completed || round.fixture_ids.len() != 1 {
        return None;
    }
    let fixture = competition
        .fixtures
        .iter()
        .find(|f| round.fixture_ids.contains(&f.id))?;
    Some(if fixture.result.as_ref()?.advancing_is_home() {
        fixture.home_team_id.clone()
    } else {
        fixture.away_team_id.clone()
    })
}
fn continental_qualified_entrants(
    game: &QualificationContext,
    competition: &League,
) -> Vec<String> {
    use std::collections::{BTreeMap, HashSet};

    // Feeder regions: the competition's declared regions, or — if it declares
    // none — every region present in the domestic competition set.
    let feeder_regions: HashSet<String> = if competition.required_region_ids.is_empty() {
        game.competitions
            .iter()
            .filter_map(|c| c.region_id.clone())
            .collect()
    } else {
        competition.required_region_ids.iter().cloned().collect()
    };
    let in_feeder = |c: &League| {
        c.region_id
            .as_deref()
            .is_some_and(|region| feeder_regions.contains(region))
    };

    let mut qualified: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // The first division of each feeder country is its lowest-priority league.
    let mut first_division: BTreeMap<&str, &League> = BTreeMap::new();
    for competition in &game.competitions {
        if competition.scope != CompetitionScope::Domestic
            || competition.kind != CompetitionType::League
            || !in_feeder(competition)
        {
            continue;
        }
        let Some(country) = competition.country_id.as_deref() else {
            continue;
        };
        first_division
            .entry(country)
            .and_modify(|best| {
                if competition.priority < best.priority {
                    *best = competition;
                }
            })
            .or_insert(competition);
    }
    for league in first_division.values() {
        for entry in league
            .sorted_standings()
            .into_iter()
            .take(CONTINENTAL_LEAGUE_SLOTS)
        {
            if seen.insert(entry.team_id.clone()) {
                qualified.push(entry.team_id);
            }
        }
    }

    // Domestic cup winners earn a berth too.
    for competition in &game.competitions {
        if competition.scope != CompetitionScope::Domestic
            || competition.kind != CompetitionType::Cup
            || !in_feeder(competition)
        {
            continue;
        }
        if let Some(winner) = champion(competition)
            && seen.insert(winner.clone())
        {
            qualified.push(winner);
        }
    }

    seed_cap_and_fill(game, competition, qualified, seen)
}

/// Whether any competition awards a berth into `target_id` — i.e. continental
/// qualification for that competition is data-defined rather than inferred.
fn competition_has_incoming_berths(game: &QualificationContext, target_id: &str) -> bool {
    game.competitions
        .iter()
        .flat_map(|source| &source.berths)
        .any(|berth| berth.target == target_id || berth.fallback_to.as_deref() == Some(target_id))
}

/// Teams a single berth rule selects from a competition's finished results.
/// `PlayoffWinner` is scheduled and resolved separately (Phase C.3b).
fn evaluate_berth_rule(source: &League, rule: &BerthRule) -> Vec<String> {
    match rule {
        BerthRule::PositionRange { from, to } => {
            let start = (*from as usize).saturating_sub(1);
            let count = (*to).saturating_sub(*from).saturating_add(1) as usize;
            source
                .sorted_standings()
                .into_iter()
                .skip(start)
                .take(count)
                .map(|entry| entry.team_id)
                .collect()
        }
        BerthRule::CupWinner => champion(source).into_iter().collect(),
        BerthRule::PlayoffWinner { .. } => Vec::new(),
    }
}

/// Resolve every berth-fed continental field at once, honouring cross-target
/// exclusivity and the `fallbackTo` cascade: a club ends in the single most
/// prestigious target (lowest priority) it earns, and a berth's `fallbackTo`
/// is a lower-preference target used when the club doesn't earn the primary.
/// Returns `target_id -> field`; targets without incoming berths are absent
/// (the caller keeps the inferred path for those).
fn resolve_continental_fields(
    game: &QualificationContext,
) -> std::collections::HashMap<String, Vec<String>> {
    use std::collections::{HashMap, HashSet};

    // Berth-fed continental targets, most prestigious (lowest priority) first.
    let mut targets: Vec<&League> = game
        .competitions
        .iter()
        .filter(|competition| {
            competition.scope == CompetitionScope::Continental
                && competition_has_incoming_berths(game, &competition.id)
        })
        .collect();
    targets.sort_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.id.cmp(&b.id)));
    let priority_of: HashMap<&str, u32> = targets
        .iter()
        .map(|c| (c.id.as_str(), c.priority))
        .collect();

    // Each club keeps the most prestigious target any of its berths award it;
    // a berth's primary target outranks its fallback for the same club.
    let mut best: HashMap<String, (u32, String)> = HashMap::new();
    let mut consider = |club: &str, target: &str| {
        if let Some(&prio) = priority_of.get(target) {
            let slot = best
                .entry(club.to_string())
                .or_insert((u32::MAX, String::new()));
            if prio < slot.0 {
                *slot = (prio, target.to_string());
            }
        }
    };
    for source in &game.competitions {
        for berth in &source.berths {
            for winner in evaluate_berth_rule(source, &berth.rule) {
                consider(&winner, &berth.target);
                if let Some(fallback) = &berth.fallback_to {
                    consider(&winner, fallback);
                }
            }
        }
    }

    // Every club placed in any target — excluded from all targets' reputation
    // top-up so a thin field never pulls in a club already qualified elsewhere.
    let all_placed: HashSet<String> = best.keys().cloned().collect();
    let mut raw: HashMap<String, Vec<String>> = HashMap::new();
    for (club, (_prio, target)) in best {
        raw.entry(target).or_default().push(club);
    }

    let mut fields = HashMap::new();
    for target in &targets {
        let qualified = raw.remove(&target.id).unwrap_or_default();
        fields.insert(
            target.id.clone(),
            seed_cap_and_fill(game, target, qualified, all_placed.clone()),
        );
    }
    fields
}

/// Shared tail for both qualification paths: seed by reputation, cap to the
/// target's field size, and top up a thin field from the feeder regions.
fn seed_cap_and_fill(
    game: &QualificationContext,
    competition: &League,
    mut qualified: Vec<String>,
    seen: std::collections::HashSet<String>,
) -> Vec<String> {
    let field_size = competition.participant_ids.len().max(4);
    let feeder_regions: std::collections::HashSet<String> =
        if competition.required_region_ids.is_empty() {
            game.competitions
                .iter()
                .filter_map(|c| c.region_id.clone())
                .collect()
        } else {
            competition.required_region_ids.iter().cloned().collect()
        };

    let reputation = |id: &str| {
        game.teams
            .iter()
            .find(|team| team.id == id)
            .map(|team| team.reputation)
            .unwrap_or(0)
    };
    qualified.sort_by(|a, b| reputation(b).cmp(&reputation(a)).then_with(|| a.cmp(b)));
    qualified.truncate(field_size);

    if qualified.len() < field_size {
        let mut fillers: Vec<_> = game
            .teams
            .iter()
            .filter(|team| !seen.contains(&team.id))
            .filter(|team| feeder_regions.contains(team.region.as_str()))
            .collect();
        fillers.sort_by(|a, b| {
            b.reputation
                .cmp(&a.reputation)
                .then_with(|| a.id.cmp(&b.id))
        });
        for team in fillers {
            if qualified.len() >= field_size {
                break;
            }
            qualified.push(team.id.clone());
        }
    }

    qualified
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::league::{Berth, StandingEntry};
    #[test]
    fn position_berths_prefer_high_priority_and_fill_excludes_other_targets() {
        let members: Vec<_> = (0..8).map(|i| format!("c{i}")).collect();
        let mut domestic = League::new("league".into(), "league".into(), 2026, &members);
        domestic.region_id = Some("eu".into());
        domestic.country_id = Some("ENG".into());
        domestic.standings = members
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let mut s = StandingEntry::new(id.clone());
                s.points = (8 - i) as u32;
                s
            })
            .collect();
        domestic.berths = vec![
            Berth {
                target: "high".into(),
                rule: BerthRule::PositionRange { from: 1, to: 2 },
                fallback_to: Some("low".into()),
            },
            Berth {
                target: "low".into(),
                rule: BerthRule::PositionRange { from: 1, to: 4 },
                fallback_to: None,
            },
        ];
        let mut high = League::new("high".into(), "high".into(), 2026, &members[..4]);
        high.kind = CompetitionType::ContinentalClub;
        high.scope = CompetitionScope::Continental;
        high.required_region_ids = vec!["eu".into()];
        high.priority = 0;
        let mut low = high.clone();
        low.id = "low".into();
        low.priority = 1;
        let reps = members
            .iter()
            .enumerate()
            .map(|(i, id)| (id.clone(), 800 - i as u32))
            .collect();
        let regions = members.iter().map(|id| (id.clone(), "eu".into())).collect();
        let output = fields(&[domestic, high, low], &reps, &regions);
        assert!(output["high"].contains(&"c0".into()));
        assert!(output["high"].contains(&"c1".into()));
        assert!(!output["low"].contains(&"c0".into()));
        assert!(!output["low"].contains(&"c1".into()));
        assert!(output["low"].contains(&"c2".into()));
        assert!(output["low"].contains(&"c3".into()));
        // Source fills independently from clubs without a berth: preserve that
        // exact rule rather than silently inventing cross-target filler exclusion.
        assert_eq!(output["high"].len(), 4);
        assert_eq!(output["low"].len(), 4);
    }
}
