//! Youth generation adapted from pinned OpenFoot Manager 64677fee9047a1182005d666bafa5dbc025dca5c.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
//! Source nationality catalogs, name pools, distributions and generation arithmetic
//! are preserved. Ambient RNG/UUIDs are replaced by explicit RNG and caller IDs.
use crate::scouting::{YouthAssignment, YouthObjective, YouthRegion};
use domain::message::*;
use domain::staff::StaffAttributes;

fn params(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_youth_recruitment_report(
    assignment_id: &str,
    scout_name: &str,
    team_id: &str,
    team_name: &str,
    prospects: &[Player],
    region: YouthRegion,
    objective: YouthObjective,
    target_position: Option<&Position>,
    date: &str,
) -> InboxMessage {
    let target_position = target_position.map(|position| position.to_group_position());
    let message = InboxMessage::new(
        format!("youth-scout-{}", assignment_id),
        String::new(),
        String::new(),
        scout_name.to_string(),
        date.to_string(),
    )
    .with_category(MessageCategory::ScoutReport)
    .with_sender_role("");

    let message = prospects.iter().fold(message, |message, prospect| {
        message.with_action(MessageAction {
            id: format!("prospect:{}", prospect.id),
            label: prospect.full_name.clone(),
            action_type: ActionType::ChooseOption {
                options: youth_prospect_options(),
            },
            resolved: false,
            label_key: None,
        })
    });

    let message = message.with_context(MessageContext {
        team_id: Some(team_id.to_string()),
        youth_target_position: target_position
            .as_ref()
            .map(|position| format!("{:?}", position)),
        youth_search_region: Some(format!("{:?}", region)),
        youth_search_objective: Some(format!("{:?}", objective)),
        youth_prospects: Some(prospects.to_vec()),
        ..MessageContext::default()
    });

    let mut i18n_params = params(&[
        ("scout", scout_name),
        ("count", &prospects.len().to_string()),
        ("team", team_name),
        ("regionLabel", region_i18n_key(region)),
        ("objectiveLabel", objective_i18n_key(objective)),
    ]);
    let body_key = if let Some(target_position) = target_position.as_ref() {
        i18n_params.insert(
            "targetLabel".to_string(),
            youth_target_position_i18n_key(target_position).to_string(),
        );
        "be.msg.youthRecruitmentReport.bodyTargeted"
    } else {
        "be.msg.youthRecruitmentReport.bodyAny"
    };

    let mut message = message.with_i18n(
        "be.msg.youthRecruitmentReport.subject",
        body_key,
        i18n_params,
    );
    message.sender_role_key = Some("be.role.scout".to_string());
    message
}

fn youth_prospect_options() -> Vec<ActionOption> {
    vec![
        ActionOption {
            id: "sign".to_string(),
            label: String::new(),
            description: String::new(),
            label_key: Some("be.msg.youthRecruitment.option.sign.label".to_string()),
            description_key: Some("be.msg.youthRecruitment.option.sign.description".to_string()),
        },
        ActionOption {
            id: "shortlist".to_string(),
            label: String::new(),
            description: String::new(),
            label_key: Some("be.msg.youthRecruitment.option.shortlist.label".to_string()),
            description_key: Some(
                "be.msg.youthRecruitment.option.shortlist.description".to_string(),
            ),
        },
        ActionOption {
            id: "discard".to_string(),
            label: String::new(),
            description: String::new(),
            label_key: Some("be.msg.youthRecruitment.option.discard.label".to_string()),
            description_key: Some("be.msg.youthRecruitment.option.discard.description".to_string()),
        },
    ]
}

fn region_i18n_key(region: YouthRegion) -> &'static str {
    match region {
        YouthRegion::Domestic => "scouting.regionDomestic",
        YouthRegion::International => "scouting.regionInternational",
    }
}

fn objective_i18n_key(objective: YouthObjective) -> &'static str {
    match objective {
        YouthObjective::Balanced => "scouting.objectiveBalanced",
        YouthObjective::HighPotential => "scouting.objectiveHighPotential",
        YouthObjective::ReadySoon => "scouting.objectiveReadySoon",
    }
}

fn youth_target_position_i18n_key(position: &Position) -> &'static str {
    match position {
        Position::Goalkeeper => "common.positions.Goalkeeper",
        Position::Defender => "common.positions.Defender",
        Position::Midfielder => "common.positions.Midfielder",
        Position::Forward => "common.positions.Forward",
        _ => "scouting.youthAnyPosition",
    }
}
use domain::{
    player::{Player, PlayerAttributes, PlayerTrait, Position, SquadRole},
    staff::{Staff, StaffRole},
    team::Team,
};
use rand::{Rng, RngExt};
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap};
#[path = "youth_source/nations.rs"]
mod nations;
#[derive(Deserialize)]
struct NamesDefinition {
    pools: HashMap<String, NamePool>,
}
#[derive(Deserialize)]
struct NamePool {
    first_names: Vec<String>,
    last_names: Vec<String>,
}
fn default_names_definition() -> NamesDefinition {
    serde_json::from_str(include_str!("youth_source/default_names.json"))
        .expect("pinned names asset")
}
pub fn generate_club_manager(
    team: &Team,
    id: &str,
    year: u32,
) -> Result<domain::manager::Manager, String> {
    use rand::SeedableRng;
    if id.trim().is_empty() || !(65..=9999).contains(&year) {
        return Err("Invalid generated manager identity/year".into());
    }
    let mut seed = 0xcbf2_9ce4_8422_2325_u64;
    for byte in id.as_bytes() {
        seed ^= u64::from(*byte);
        seed = seed.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let local = if team.football_nation.is_empty() {
        &team.country
    } else {
        &team.football_nation
    };
    let country = pick_nationality_from_def(local, nationality_distribution(), &mut rng);
    Ok(generate_random_unemployed_manager(
        id.into(),
        &country,
        &default_names_definition(),
        year,
        &mut rng,
    ))
}
/// Season-end candidate top-ups draw directly from the source global weighted
/// nationality pool (no club-local bias), then source manager/scout generators.
pub fn generate_career_candidates<R: Rng>(
    year: u32,
    manager_ids: &[String],
    scout_ids: &[String],
    rng: &mut R,
) -> Result<(Vec<domain::manager::Manager>, Vec<Staff>), String> {
    let mut seen = BTreeSet::new();
    if !(65..=9999).contains(&year)
        || manager_ids
            .iter()
            .chain(scout_ids)
            .any(|id| id.trim().is_empty() || !seen.insert(id))
    {
        return Err("Invalid career candidate IDs/year".into());
    }
    let names = default_names_definition();
    let codes = nationality_distribution();
    let nationality = |rng: &mut R| {
        if codes.is_empty() {
            "ENG".to_string()
        } else {
            codes[rng.random_range(0..codes.len())].clone()
        }
    };
    let managers = manager_ids
        .iter()
        .map(|id| {
            let country = nationality(rng);
            generate_random_unemployed_manager(id.clone(), &country, &names, year, rng)
        })
        .collect();
    let scouts = scout_ids
        .iter()
        .map(|id| {
            let country = nationality(rng);
            generate_random_staff_unattached_from_def(
                id.clone(),
                StaffRole::Scout,
                &country,
                year,
                &names,
                rng,
            )
        })
        .collect();
    Ok((managers, scouts))
}
fn generate_random_unemployed_manager(
    id: String,
    nationality: &str,
    names_def: &NamesDefinition,
    current_year: u32,
    rng: &mut impl Rng,
) -> domain::manager::Manager {
    let (first_name, last_name) = pick_name_from_def(nationality, names_def, rng);
    let age: u32 = rng.random_range(35..65);
    let birth_year = current_year.saturating_sub(age);
    let dob = format!(
        "{:04}-{:02}-{:02}",
        birth_year,
        rng.random_range(1u32..13u32),
        rng.random_range(1u32..29u32)
    );
    let reputation = rng.random_range(200u32..=700u32);

    let mut mgr =
        domain::manager::Manager::new(id, first_name, last_name, dob, nationality.to_string());
    mgr.reputation = reputation;
    mgr.satisfaction = 50;
    mgr.fan_approval = 50;
    mgr
}
/// Exact source 12-role available market, including source wage=0 and no
/// specialization/contract. These values are not invented hiring terms.
pub fn generate_available_staff(
    teams: &[Team],
    year: u32,
    ids: &[String],
    rng: &mut impl Rng,
) -> Result<Vec<Staff>, String> {
    if ids.len() != 12
        || ids.iter().any(|id| id.trim().is_empty())
        || ids.iter().collect::<BTreeSet<_>>().len() != 12
        || !(60..=9999).contains(&year)
    {
        return Err("Invalid available staff generation IDs/year".into());
    }
    let local = |team: &Team| {
        if team.football_nation.is_empty() {
            team.country.clone()
        } else {
            team.football_nation.clone()
        }
    };
    let fallback = teams.first().map(local).unwrap_or("England".into());
    let names = default_names_definition();
    let codes = nationality_distribution();
    let roles = [
        StaffRole::Coach,
        StaffRole::Scout,
        StaffRole::Physio,
        StaffRole::Coach,
        StaffRole::AssistantManager,
        StaffRole::Scout,
        StaffRole::Physio,
        StaffRole::Coach,
        StaffRole::Coach,
        StaffRole::Physio,
        StaffRole::Scout,
        StaffRole::AssistantManager,
    ];
    Ok(roles
        .into_iter()
        .zip(ids)
        .map(|(role, id)| {
            let country = if codes.is_empty() {
                canonicalize_generated_nationality(&fallback)
            } else {
                let seed = teams
                    .get(rng.random_range(0..teams.len().max(1)))
                    .map(local)
                    .unwrap_or_else(|| fallback.clone());
                pick_nationality_from_def(&seed, codes, rng)
            };
            generate_random_staff_unattached_from_def(id.clone(), role, &country, year, &names, rng)
        })
        .collect())
}
fn generate_random_staff_unattached_from_def(
    id: String,
    role: StaffRole,
    nationality: &str,
    opening_year: u32,
    names_def: &NamesDefinition,
    rng: &mut impl Rng,
) -> Staff {
    let (first_name, last_name) = pick_name_from_def(nationality, names_def, rng);
    let age = rng.random_range(28..55);
    let birth_year = opening_year.saturating_sub(age);
    let dob = format!(
        "{:04}-{:02}-{:02}",
        birth_year,
        rng.random_range(1..13),
        rng.random_range(1..29)
    );

    let attributes = StaffAttributes {
        coaching: rng.random_range(30..80),
        judging_ability: rng.random_range(30..80),
        judging_potential: rng.random_range(25..75),
        physiotherapy: rng.random_range(25..75),
    };

    let mut s = Staff::new(id, first_name, last_name, dob, role, attributes);
    s.nationality = nationality.to_string();
    s
}

// ---------------------------------------------------------------------------
// Authored staff (world packages)
fn engine_attributes(
    a: &PlayerAttributes,
    position: &Position,
) -> (engine::PlayerData, crate::training::Position) {
    let position: crate::training::Position =
        serde_json::from_value(serde_json::to_value(position).unwrap()).unwrap();
    let mut value = serde_json::to_value(a).unwrap();
    let fields = value.as_object_mut().unwrap();
    fields.insert("id".into(), "".into());
    fields.insert("name".into(), "".into());
    fields.insert(
        "position".into(),
        serde_json::to_value(position.group()).unwrap(),
    );
    fields.insert("condition".into(), 100.into());
    (
        serde_json::from_value(value).expect("complete domain attributes"),
        position,
    )
}
fn attribute_ovr(a: &PlayerAttributes, p: &Position) -> f64 {
    let (engine, position) = engine_attributes(a, p);
    crate::training::ovr_for_position(&engine, position)
}
fn refresh_derived(player: &mut Player, year: u32, rng: &mut impl Rng) {
    let (mut engine, position) = engine_attributes(&player.attributes, &player.natural_position);
    let birth_year = player
        .date_of_birth
        .split('-')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let mut meta = crate::training::PlayerTraining {
        birth_year,
        potential: player.potential,
        natural_position: position,
        position,
        individual_focus: None,
    };
    crate::training::refresh_derived(&mut engine, &mut meta, year, rng);
    player.ovr = engine.ovr;
    player.potential = meta.potential;
    player.traits = domain::player::compute_traits(&player.attributes, &player.natural_position);
    if year.saturating_sub(birth_year) <= 20
        && player.potential >= 90
        && player.potential.saturating_sub(player.ovr) >= 14
    {
        player.traits.push(PlayerTrait::Wonderkid);
    }
}
/// Full unranked source pool. Caller supplies globally unique IDs (including
/// discarded candidates); source scout judging affects delay, not generated talent.
pub fn generate_pool(
    team: &Team,
    year: u32,
    assignment: &YouthAssignment,
    scout: &Staff,
    ids: &[String],
    rng: &mut impl Rng,
) -> Result<Vec<Player>, String> {
    let count = if assignment.objective == YouthObjective::Balanced {
        4
    } else {
        6
    };
    if !(22..=9993).contains(&year)
        || ids.len() != count
        || ids.iter().any(|id| id.trim().is_empty())
        || ids.iter().collect::<BTreeSet<_>>().len() != count
    {
        return Err("Invalid youth year or candidate IDs".into());
    }
    if assignment.scout_id != scout.id || assignment.days_remaining != 0 {
        return Err("Youth search is not ready for its assigned club scout".into());
    }
    let names = default_names_definition();
    let local = if team.football_nation.is_empty() {
        &team.country
    } else {
        &team.football_nation
    };
    let mut players = Vec::with_capacity(count);
    for id in ids {
        let nationality = if assignment.region == YouthRegion::Domestic {
            canonicalize_generated_nationality(local)
        } else {
            pick_nationality_from_def(local, nationality_distribution(), rng)
        };
        let slots = youth_slots_for_target(
            assignment
                .target_position
                .as_ref()
                .map(Position::to_group_position),
        );
        let index = slots[rng.random_range(0..slots.len())];
        let mut player = generate_random_player_from_def(
            id.clone(),
            &team.id,
            index,
            &nationality,
            year,
            &names,
            rng,
        );
        player.team_id = None;
        player.squad_role = SquadRole::Youth;
        player.transfer_listed = false;
        player.loan_listed = false;
        players.push(player);
    }
    Ok(players)
}
/// Compute a sensible alternate position based on primary position and attributes.
fn compute_alternate_position(primary: &Position, attrs: &PlayerAttributes) -> Option<Position> {
    match primary.to_group_position() {
        Position::Goalkeeper => None,
        Position::Defender => {
            // Defenders with good passing/vision → Midfielder
            if attrs.passing >= 65 && attrs.vision >= 60 {
                Some(Position::Midfielder)
            } else {
                None
            }
        }
        Position::Midfielder => {
            // Midfielders with strong defending/tackling → Defender
            if attrs.defending >= 65 && attrs.tackling >= 60 {
                Some(Position::Defender)
            }
            // Midfielders with good shooting/dribbling → Forward
            else if attrs.shooting >= 65 && attrs.dribbling >= 60 {
                Some(Position::Forward)
            } else {
                None
            }
        }
        Position::Forward => {
            // Forwards with good passing/vision → Midfielder
            if attrs.passing >= 65 && attrs.vision >= 60 {
                Some(Position::Midfielder)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Pick a nationality: 60% the club's own country, 40% from the wider draw.
///
/// When the club's country resolves to no known nation the local weight has
/// nothing to apply to, so the whole draw comes from `available_codes`. That is
/// the #453 fix: an unrecognised country used to mean England, specifically and
/// silently, for 60% of the squad.
fn pick_nationality_from_def(
    team_country: &str,
    available_codes: &[String],
    rng: &mut impl Rng,
) -> String {
    /// Share of a squad drawn from the club's own country.
    const LOCAL_SHARE_PERCENT: u32 = 60;

    // The catalog first, then the world's own list. A package may declare
    // countries the catalog has never heard of — that is the point of authoring
    // one — and for a club in such a country the id *is* the nationality. Only a
    // country neither the catalog nor this world recognises is unresolvable.
    let local = resolve_nationality_code(team_country)
        .or_else(|| declared_code(team_country, available_codes));

    // Nothing to draw from: the club's own country is the only answer available,
    // and when that is unknown too there is genuinely none to give.
    if available_codes.is_empty() {
        return canonicalize_generated_nationality(local.as_deref().unwrap_or_default());
    }

    // An unresolvable country skips the local roll entirely rather than losing
    // it — the whole draw comes from the wider pool.
    let selected = match local {
        Some(code) if rng.random_range(0..100) < LOCAL_SHARE_PERCENT => code,
        _ => available_codes[rng.random_range(0..available_codes.len())].clone(),
    };

    canonicalize_generated_nationality(&selected)
}

/// Match a club's country against the nationalities this world actually draws
/// from, for countries the shipped catalog does not contain.
///
/// `available_codes` is the world's own nationality list, so a package country
/// reaches here only if [`super::build_world_data_from_package`] put it there —
/// which it does for every country the package declares. Comparing against that
/// list rather than accepting any unknown string keeps an outright typo
/// unresolvable, which is what #453 was about.
fn declared_code(team_country: &str, available_codes: &[String]) -> Option<String> {
    let trimmed = team_country.trim();
    if trimmed.is_empty() {
        return None;
    }
    available_codes
        .iter()
        .find(|code| code.eq_ignore_ascii_case(trimmed))
        .cloned()
}

fn canonicalize_generated_nationality(value: &str) -> String {
    match value.trim().to_ascii_uppercase().as_str() {
        // Freshly generated football identities should never persist the ambiguous GB code.
        "GB" => "ENG".to_string(),
        other => other.to_string(),
    }
}

/// Pick a name from the NamesDefinition for a given nationality code.
fn pick_name_from_def(
    nationality: &str,
    names_def: &NamesDefinition,
    rng: &mut impl Rng,
) -> (String, String) {
    let candidate_codes = match nationality {
        "ENG" | "SCO" | "WAL" | "NIR" => vec![nationality, "GB"],
        _ => vec![nationality],
    };

    for candidate in candidate_codes {
        if let Some(pool) = names_def.pools.get(candidate)
            && !pool.first_names.is_empty()
            && !pool.last_names.is_empty()
        {
            let first = pool.first_names[rng.random_range(0..pool.first_names.len())].clone();
            let last = pool.last_names[rng.random_range(0..pool.last_names.len())].clone();
            return (first, last);
        }
    }

    match fallback_pool(nationality, names_def, rng) {
        Some(pool) => draw_name(pool, rng),
        None => ("Player".to_string(), "Unknown".to_string()),
    }
}

/// A random first/last pair from `pool`. Callers must have checked it is usable.
fn draw_name(pool: &NamePool, rng: &mut impl Rng) -> (String, String) {
    let first = pool.first_names[rng.random_range(0..pool.first_names.len())].clone();
    let last = pool.last_names[rng.random_range(0..pool.last_names.len())].clone();
    (first, last)
}

/// The pool to borrow from when a nationality has none of its own — the common
/// case, since only 17 pools ship against ~210 selectable nations.
///
/// Prefers a pool from the same confederation, then any usable pool. This used to
/// take `pools.keys().min()`, the lexicographically smallest key, which for the
/// shipped set is always `AR`: a Pole, a Nigerian and a Japanese player were
/// all named Ezequiel, deterministically.
///
/// Both sides of the region comparison must resolve to a *declared* region.
/// `region_for_code` answers `europe` for anything it does not recognise, so
/// matching on it would file every unknown key — a package keying its pools
/// `BRA`/`ESP`/`JPN`, say — as European, then hand a Pole a Japanese name
/// while a Brazilian matched none of them. An unknown code on either side falls
/// straight through to "any usable pool" instead, which is merely arbitrary
/// rather than confidently wrong.
///
/// Candidates are sorted before the draw because `pools` is a `HashMap` whose
/// iteration order is randomized per process.
fn fallback_pool<'a>(
    nationality: &str,
    names_def: &'a NamesDefinition,
    rng: &mut impl Rng,
) -> Option<&'a NamePool> {
    let usable = |pool: &NamePool| !pool.first_names.is_empty() && !pool.last_names.is_empty();
    let region_of = |code: &str| nations::nation_by_code(code).map(|nation| nation.region_id);

    let mut candidates: Vec<(&String, &NamePool)> = Vec::new();
    if let Some(region) = region_of(nationality) {
        candidates = names_def
            .pools
            .iter()
            .filter(|(code, pool)| usable(pool) && region_of(code) == Some(region))
            .collect();
    }
    if candidates.is_empty() {
        candidates = names_def
            .pools
            .iter()
            .filter(|(_, pool)| usable(pool))
            .collect();
    }
    if candidates.is_empty() {
        return None;
    }
    candidates.sort_by(|left, right| left.0.cmp(right.0));

    Some(candidates[rng.random_range(0..candidates.len())].1)
}

/// Resolve a club's declared country to a nationality code, or `None` when it
/// names no nation the game knows.
///
/// Replaces `country_to_iso`, which matched 17 country names by hand and
/// answered `"ENG"` for everything else — so `"country": "Japan"` filled 60% of
/// every Japanese club with English players, with nothing in the log or the UI
/// to say so. Its length heuristic was a second wrong answer: any 2–3 character
/// string was passed through as though it were a code.
///
/// `None` is the important part. An unresolvable country must stay explicitly
/// unknown so the caller can draw from the whole distribution, rather than
/// being handed a real, specific, wrong nationality.
fn resolve_nationality_code(country: &str) -> Option<String> {
    let trimmed = country.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Football identities first: this is what maps "England"/"english"/"eng" —
    // and the UK home nations generally — onto the codes the game uses.
    let normalized = domain::identity::normalize_football_nation_code(trimmed);
    if nations::nation_by_code(&normalized).is_some() {
        return Some(normalized);
    }
    // `GB` is a real football identity but never a generated nationality; the
    // caller canonicalises it to ENG.
    if normalized == "GB" {
        return Some(normalized);
    }

    // Then the catalog's own display names, which is what makes "Japan",
    // "Nigeria" and the other 190-odd nations resolve at all.
    nations::nation_by_name(trimmed).map(|nation| nation.code.to_string())
}

/// Every nationality a generated player can have, repeated in proportion to how
/// often it should come up.
///
/// #452: this used to be `names_def.pools.keys()` — the 17 name-pool keys. A
/// lookup table was doing the job of a population model, so a world contained
/// at most ~16 nationalities (14 of them European) no matter how many countries
/// it had, and no amount of catalog work could change that.
///
/// Weight has two factors, because a nation's standing has two parts and the
/// catalog only records one of them.
///
/// *Rank within region* comes from [`nations::NATION_CATALOG`], already
/// documented as "strongest footballing traditions first within each region" —
/// a signal the codebase maintains anyway.
///
/// *Region depth* has to be declared, and [`REGION_WEIGHT`] declares it. Rank
/// alone is a **rank among unequal fields**: it makes the top of every region
/// equal, so Costa Rica draws as often as Brazil and France, New Zealand
/// outranks Spain, and a world ends up with more Central American players than
/// South American ones. Multiplying restores the comparison the catalog cannot
/// express, and keeps the ordering rank already gets right inside a region.
///
/// [`nations::ADDITIONAL_NATIONS`] form a flat low-weight tail: reachable, but
/// rare, and deliberately not region-scaled — they are outside the World Cup
/// pool, which is the only claim being made about them.
///
/// Deliberately *not* `NationGen.strength`: that exists for only the 16
/// generation nations and is already spoken for by club reputation, so using it
/// here would privilege exactly the nations this issue is about and couple two
/// unrelated models.
///
/// Expanded into a plain `Vec` so callers keep drawing with a uniform index —
/// the weighting lives here, once, instead of at every draw site. Built once:
/// it derives purely from `&'static` catalogs, and the order must be stable or
/// seeded generation stops reproducing.
fn nationality_distribution() -> &'static Vec<String> {
    static DISTRIBUTION: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    DISTRIBUTION.get_or_init(|| {
        /// Weight of the strongest nation in a region. Ranks below it step down
        /// by one, so a region's top handful dominate its share without
        /// shutting the rest out.
        const TOP_WEIGHT: usize = 12;
        /// Floor for a World Cup nation, and the flat weight of the tail. Two
        /// tiers rather than one: Europe alone has 26 catalog nations, so a
        /// single floor of 1 made everything past rank 11 exactly as likely as
        /// a merely-selectable nation — Poland would have drawn as often as
        /// Andorra. A qualifying nation should always outrank a non-entrant.
        const CATALOG_FLOOR: usize = 2;
        const TAIL_WEIGHT: usize = 1;

        let mut pool = Vec::new();
        let mut seen_in_region: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();

        for nation in nations::NATION_CATALOG {
            let rank = seen_in_region.entry(nation.region_id).or_insert(0);
            let within_region = TOP_WEIGHT.saturating_sub(*rank).max(CATALOG_FLOOR);
            let weight = within_region * region_weight(nation.region_id);
            *rank += 1;
            for _ in 0..weight {
                pool.push(nation.code.to_string());
            }
        }
        for nation in nations::ADDITIONAL_NATIONS {
            for _ in 0..TAIL_WEIGHT {
                pool.push(nation.code.to_string());
            }
        }
        pool
    })
}
fn region_weight(region_id: &str) -> usize {
    match region_id {
        "europe" => 6,
        "south-america" => 5,
        "africa" => 3,
        "asia" => 2,
        "north-america" | "central-america" => 2,
        "oceania" => 1,
        // `region_for_code` defaults unknown codes to europe, so an unrecognised
        // region here means the catalog gained one this table has not been told
        // about. Weight it as a modest region rather than silently as Europe.
        _ => 2,
    }
}
/// Squad slots reserved as youth-aged, one per position group in
/// `[GK, DEF, MID, FWD]` order. Scouted youth recruits target these slots so they
/// generate at a consistent academy age, and senior generation must avoid them.
/// This is the single source of truth shared by the youth-recruit targeting,
/// youth-age generation, and national-team senior remap logic.
const YOUTH_RESERVED_SLOTS: [usize; 4] = [1, 8, 15, 21];

/// Candidate youth slots for a (group) position target. A specific group yields
/// its single reserved slot; `None` (or any other position) yields all of them.
fn youth_slots_for_target(group: Option<Position>) -> &'static [usize] {
    match group {
        Some(Position::Goalkeeper) => &YOUTH_RESERVED_SLOTS[0..1],
        Some(Position::Defender) => &YOUTH_RESERVED_SLOTS[1..2],
        Some(Position::Midfielder) => &YOUTH_RESERVED_SLOTS[2..3],
        Some(Position::Forward) => &YOUTH_RESERVED_SLOTS[3..4],
        _ => &YOUTH_RESERVED_SLOTS,
    }
}

/// Whether a squad slot is reserved for a youth-aged player.
fn is_youth_reserved_slot(slot: usize) -> bool {
    YOUTH_RESERVED_SLOTS.contains(&slot)
}

/// Pinned generator::generate_national_team_player, with explicit identity/RNG.
pub(crate) fn generate_national_player(
    id: String,
    nationality: &str,
    slot: usize,
    year: u32,
    rng: &mut impl Rng,
) -> Result<Player, String> {
    if id.trim().is_empty() || !(55..=9993).contains(&year) {
        return Err("Invalid national player identity or year".into());
    }
    let slot = slot % 22;
    let slot = if is_youth_reserved_slot(slot) {
        slot - 1
    } else {
        slot
    };
    let mut player = generate_random_player_from_def(
        id,
        "national-pool",
        slot,
        &canonicalize_generated_nationality(nationality),
        year,
        &default_names_definition(),
        rng,
    );
    player.team_id = None;
    player.contract_end = None;
    player.wage = 0;
    player.transfer_listed = false;
    player.loan_listed = false;
    Ok(player)
}

fn generate_random_player_from_def(
    p_id: String,
    team_id: &str,
    index: usize,
    nationality: &str,
    opening_year: u32,
    names_def: &NamesDefinition,
    rng: &mut impl Rng,
) -> Player {
    let (first_name, last_name) = pick_name_from_def(nationality, names_def, rng);
    let full_name = format!("{} {}", first_name, last_name);
    let match_name = last_name.clone();

    // Distribute positions: GK:0-1, DEF:2-8, MID:9-15, FWD:16-21
    let position = if index < 2 {
        Position::Goalkeeper
    } else if index < 9 {
        Position::Defender
    } else if index < 16 {
        Position::Midfielder
    } else {
        Position::Forward
    };

    let nationality = nationality.to_string();

    // Reserve one slot per position group (GK + back line + midfield + attack) as
    // youth-aged so scouted youth recruits land at a consistent age across positions
    // and clubs can open with real academy prospects instead of an empty youth squad.
    let age = if is_youth_reserved_slot(index) {
        rng.random_range(17..22)
    } else {
        rng.random_range(17..36)
    };
    let birth_year = opening_year.saturating_sub(age);
    let birth_month = rng.random_range(1..13);
    let birth_day = rng.random_range(1..29);
    let dob = format!("{:04}-{:02}-{:02}", birth_year, birth_month, birth_day);

    let group = position.to_group_position();
    let is_gk = matches!(group, Position::Goalkeeper);
    let is_def = matches!(group, Position::Defender);
    let is_fwd = matches!(group, Position::Forward);

    let attributes = PlayerAttributes {
        pace: rng.random_range(40..95),
        stamina: rng.random_range(40..95),
        strength: rng.random_range(40..95),
        agility: rng.random_range(40..95),
        passing: rng.random_range(40..95),
        shooting: if is_gk {
            rng.random_range(20..50)
        } else {
            rng.random_range(40..95)
        },
        tackling: if is_gk || is_fwd {
            rng.random_range(20..60)
        } else {
            rng.random_range(40..95)
        },
        dribbling: if is_gk {
            rng.random_range(20..50)
        } else {
            rng.random_range(40..95)
        },
        defending: if is_gk {
            rng.random_range(25..55)
        } else if is_def {
            rng.random_range(55..95)
        } else {
            rng.random_range(40..95)
        },
        positioning: rng.random_range(40..95),
        vision: rng.random_range(40..95),
        decisions: rng.random_range(40..95),
        composure: rng.random_range(40..95),
        aggression: rng.random_range(30..90),
        teamwork: rng.random_range(45..95),
        leadership: rng.random_range(30..90),
        handling: if is_gk {
            rng.random_range(50..95)
        } else {
            rng.random_range(10..35)
        },
        reflexes: if is_gk {
            rng.random_range(50..95)
        } else {
            rng.random_range(20..50)
        },
        aerial: if is_gk {
            rng.random_range(50..95)
        } else if is_def {
            rng.random_range(45..90)
        } else {
            rng.random_range(30..75)
        },
    };

    // Size market value and wage from the same position-weighted rating the
    // player will be shown with, so a keeper is priced on keeping.
    let current_year: u32 = opening_year;

    let approx_ovr = attribute_ovr(&attributes, &position).round() as u32;

    let age_factor = if age <= 23 {
        1.5
    } else if age <= 28 {
        1.2
    } else if age <= 32 {
        0.8
    } else {
        0.4
    };
    let base_value = (approx_ovr as f64).powi(2) * 500.0;
    let market_value = (base_value * age_factor) as u64;
    let wage = (market_value / 200).max(500) as u32;
    let contract_years = if age <= 21 {
        rng.random_range(3..6)
    } else if age <= 27 {
        rng.random_range(2..5)
    } else if age <= 31 {
        rng.random_range(2..4)
    } else if rng.random_range(0..100) < 40 {
        1
    } else {
        2
    };
    let contract_end = format!("{}-06-30", opening_year.saturating_add(contract_years));

    let mut player = Player::new(
        p_id,
        match_name,
        full_name,
        dob,
        nationality,
        position,
        attributes,
    );
    player.team_id = Some(team_id.to_string());
    player.market_value = market_value;
    player.wage = wage;
    player.contract_end = Some(contract_end);
    player.condition = rng.random_range(75..100);
    player.morale = rng.random_range(40..76);

    // ~40% of outfield players get an alternate position based on attributes
    if !is_gk && rng.random_range(0..5) < 2 {
        let alt = compute_alternate_position(&player.position, &player.attributes);
        if let Some(pos) = alt {
            player.alternate_positions.push(pos);
        }
    }

    // Set position-weighted OVR, potential, and traits (Wonderkid included if applicable)
    refresh_derived(&mut player, current_year, rng);

    player.jersey_number = jersey_number_for_slot(index);

    player
}

fn jersey_number_for_slot(index: usize) -> Option<u8> {
    let n: u8 = match index {
        0 => 1,
        1 => 13,
        2 => 2,
        3 => 5,
        4 => 6,
        5 => 3,
        6 => 4,
        7 => 12,
        8 => 22,
        9 => 8,
        10 => 7,
        11 => 10,
        12 => 14,
        13 => 11,
        14 => 16,
        15 => 23,
        16 => 9,
        17 => 17,
        18 => 18,
        19 => 19,
        20 => 20,
        21 => 24,
        _ => return None,
    };
    Some(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};
    fn team() -> Team {
        Team::new(
            "club".into(),
            "Club".into(),
            "C".into(),
            "England".into(),
            "City".into(),
            "Stadium".into(),
            1000,
        )
    }
    fn scout() -> Staff {
        let mut s = Staff::new(
            "scout".into(),
            "A".into(),
            "Scout".into(),
            "1980-01-01".into(),
            StaffRole::Scout,
            domain::staff::StaffAttributes {
                coaching: 50,
                judging_ability: 80,
                judging_potential: 80,
                physiotherapy: 50,
            },
        );
        s.team_id = Some("club".into());
        s
    }
    fn assignment() -> YouthAssignment {
        YouthAssignment {
            id: "search".into(),
            scout_id: "scout".into(),
            region: YouthRegion::Domestic,
            objective: YouthObjective::Balanced,
            target_position: Some(Position::Goalkeeper),
            days_remaining: 0,
        }
    }
    fn ids() -> Vec<String> {
        (0..4)
            .map(|i| format!("seed17-search-candidate-{i}"))
            .collect()
    }
    #[test]
    fn seeded_real_players_replay_with_exact_source_youth_arithmetic() {
        let a = generate_pool(
            &team(),
            2026,
            &assignment(),
            &scout(),
            &ids(),
            &mut StdRng::seed_from_u64(17),
        )
        .unwrap();
        let b = generate_pool(
            &team(),
            2026,
            &assignment(),
            &scout(),
            &ids(),
            &mut StdRng::seed_from_u64(17),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&a).unwrap(),
            serde_json::to_value(&b).unwrap()
        );
        let c = generate_pool(
            &team(),
            2026,
            &assignment(),
            &scout(),
            &ids(),
            &mut StdRng::seed_from_u64(18),
        )
        .unwrap();
        assert_ne!(
            serde_json::to_value(&a).unwrap(),
            serde_json::to_value(c).unwrap()
        );
        for p in a {
            assert_eq!(p.nationality, "ENG");
            assert_eq!(p.position, Position::Goalkeeper);
            assert_eq!(p.squad_role, SquadRole::Youth);
            assert!(p.team_id.is_none());
            assert_eq!(p.jersey_number, Some(13));
            let age = 2026 - p.date_of_birth[..4].parse::<u32>().unwrap();
            assert!((17..=21).contains(&age));
            assert!((75..100).contains(&p.condition));
            assert!((40..76).contains(&p.morale));
            assert!((20..50).contains(&p.attributes.shooting));
            assert!((50..95).contains(&p.attributes.handling));
            assert_eq!(
                p.ovr,
                attribute_ovr(&p.attributes, &p.position).round() as u8
            );
            assert_eq!(
                p.market_value,
                ((f64::from(p.ovr)).powi(2) * 500.0 * 1.5) as u64
            );
            assert_eq!(p.wage, (p.market_value / 200).max(500) as u32);
            let (lo, hi) = match age {
                17..=18 => (15, 30),
                19..=20 => (8, 22),
                _ => (4, 14),
            };
            assert!(
                (p.ovr.saturating_add(lo).min(99)..=p.ovr.saturating_add(hi).min(99))
                    .contains(&p.potential)
            );
            assert!(!p.full_name.contains("Unknown"));
        }
    }
    #[test]
    fn data_distribution_and_names_keep_source_fallbacks() {
        let distribution = nationality_distribution();
        assert_eq!(
            distribution.iter().filter(|c| c.as_str() == "BR").count(),
            60
        );
        assert_eq!(
            distribution.iter().filter(|c| c.as_str() == "AD").count(),
            1
        );
        assert_eq!(resolve_nationality_code("Japan"), Some("JP".into()));
        let names = default_names_definition();
        let mut rng = StdRng::seed_from_u64(2);
        for country in ["ENG", "SCO", "JP", "NG", "PL"] {
            let (first, last) = pick_name_from_def(country, &names, &mut rng);
            assert_ne!(last, "Unknown");
            assert!(!first.is_empty());
        }
    }
    #[test]
    fn completion_is_private_once_only_and_has_three_actionable_prospects() {
        use crate::scouting::ScoutingState;
        use std::collections::BTreeMap;
        let mut state = ScoutingState::default();
        let scout = scout();
        let team = team();
        state
            .start_youth(
                "manager",
                "club",
                "search",
                &scout,
                YouthRegion::Domestic,
                YouthObjective::Balanced,
                Some(Position::Goalkeeper),
            )
            .unwrap();
        let day = chrono::NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
        let mut rng = StdRng::seed_from_u64(17);
        for offset in 0..4 {
            state
                .advance_day(
                    day + chrono::Days::new(offset),
                    &BTreeMap::new(),
                    &BTreeMap::new(),
                    &BTreeMap::new(),
                    &mut rng,
                )
                .unwrap();
        }
        let pool = generate_pool(
            &team,
            2026,
            state.pending_youth_generation("manager")[0],
            &scout,
            &ids(),
            &mut rng,
        )
        .unwrap();
        let complete_day = day + chrono::Days::new(3);
        assert!(
            state
                .complete_youth(
                    "outsider",
                    "search",
                    pool.clone(),
                    &team,
                    &scout,
                    complete_day
                )
                .is_err()
        );
        state
            .complete_youth(
                "manager",
                "search",
                pool.clone(),
                &team,
                &scout,
                complete_day,
            )
            .unwrap();
        assert!(
            state
                .complete_youth("manager", "search", pool, &team, &scout, complete_day)
                .is_err()
        );
        assert!(state.pending_youth_generation("manager").is_empty());
        assert!(state.view("outsider").is_none());
        let message = &state.view("manager").unwrap().messages[0];
        assert_eq!(message.context.youth_prospects.as_ref().unwrap().len(), 3);
        assert_eq!(message.actions.len(), 3);
        assert!(message.actions.iter().all(|a|matches!(&a.action_type,ActionType::ChooseOption{options} if options.iter().map(|o|o.id.as_str()).collect::<Vec<_>>()==["sign","shortlist","discard"])));
    }
    #[test]
    fn rejects_bad_identity_or_early_generation_before_rng_draws() {
        let mut a = assignment();
        a.days_remaining = 1;
        let mut rng = StdRng::seed_from_u64(17);
        assert!(generate_pool(&team(), 2026, &a, &scout(), &ids(), &mut rng).is_err());
        a.days_remaining = 0;
        assert!(
            generate_pool(
                &team(),
                2026,
                &a,
                &scout(),
                &vec!["same".into(); 4],
                &mut rng
            )
            .is_err()
        );
        let actual = generate_pool(&team(), 2026, &a, &scout(), &ids(), &mut rng).unwrap();
        let expected = generate_pool(
            &team(),
            2026,
            &a,
            &scout(),
            &ids(),
            &mut StdRng::seed_from_u64(17),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }
}
#[test]
fn national_generation_uses_senior_slot_and_clears_only_contract_ownership() {
    use rand::SeedableRng;
    for slot in 0..22 {
        let mut rng = rand::rngs::StdRng::seed_from_u64(72);
        let generated =
            generate_national_player("national-id".into(), "ENG", slot, 2026, &mut rng).unwrap();
        let senior = if is_youth_reserved_slot(slot) {
            slot - 1
        } else {
            slot
        };
        let mut expected = generate_random_player_from_def(
            "national-id".into(),
            "national-pool",
            senior,
            &canonicalize_generated_nationality("ENG"),
            2026,
            &default_names_definition(),
            &mut rand::rngs::StdRng::seed_from_u64(72),
        );
        expected.team_id = None;
        expected.contract_end = None;
        expected.wage = 0;
        expected.transfer_listed = false;
        expected.loan_listed = false;
        assert_eq!(
            serde_json::to_value(generated).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }
}
