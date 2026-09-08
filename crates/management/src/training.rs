//! Daily training and derived ratings adapted from pinned OpenFoot Manager
//! 64677fee9047a1182005d666bafa5dbc025dca5c: ofm_core/training.rs,
//! ofm_core/player_rating.rs and domain/player.rs::compute_traits.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Attributes remain in the engine snapshot, with only missing development and
//! granular-position metadata here. All randomness, including unset potential,
//! comes from the supplied RNG. Training injuries and injury countdown are the
//! separate availability lifecycle. The source's privileged AI fatigue guard is
//! expressed by the host bot's ordinary individual-focus command, not this rule.
use chrono::{Datelike, NaiveDate};
use engine::PlayerData;
use rand::{Rng, RngExt};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Focus {
    #[default]
    Physical,
    Technical,
    Tactical,
    Defending,
    Attacking,
    Recovery,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Intensity {
    Low,
    #[default]
    Medium,
    High,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Schedule {
    Intense,
    #[default]
    Balanced,
    Light,
}
impl Schedule {
    pub fn is_training_day(self, weekday: u32) -> bool {
        match self {
            Self::Intense => weekday < 6,
            Self::Balanced => matches!(weekday, 0 | 1 | 3 | 4),
            Self::Light => matches!(weekday, 1 | 3),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Position {
    Goalkeeper,
    RightBack,
    LeftBack,
    CenterBack,
    RightWingBack,
    LeftWingBack,
    DefensiveMidfielder,
    CentralMidfielder,
    AttackingMidfielder,
    RightMidfielder,
    LeftMidfielder,
    RightWinger,
    LeftWinger,
    Striker,
    Defender,
    Midfielder,
    Forward,
}
impl Position {
    pub fn canonical(self) -> Self {
        match self {
            Self::Defender => Self::CenterBack,
            Self::Midfielder => Self::CentralMidfielder,
            Self::Forward => Self::Striker,
            other => other,
        }
    }
    pub fn is_legacy(self) -> bool {
        matches!(self, Self::Defender | Self::Midfielder | Self::Forward)
    }
    pub fn group(self) -> engine::Position {
        match self.canonical() {
            Self::Goalkeeper => engine::Position::Goalkeeper,
            Self::RightBack
            | Self::LeftBack
            | Self::CenterBack
            | Self::RightWingBack
            | Self::LeftWingBack => engine::Position::Defender,
            Self::DefensiveMidfielder
            | Self::CentralMidfielder
            | Self::AttackingMidfielder
            | Self::RightMidfielder
            | Self::LeftMidfielder => engine::Position::Midfielder,
            _ => engine::Position::Forward,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Specialization {
    Fitness,
    Technique,
    Tactics,
    Defending,
    Attacking,
    GoalKeeping,
    Youth,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coach {
    pub coaching: u8,
    pub specialization: Option<Specialization>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub focus: Focus,
    pub player_ids: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClubTraining {
    pub focus: Focus,
    pub intensity: Intensity,
    pub schedule: Schedule,
    pub groups: Vec<Group>,
    pub coaches: Vec<Coach>,
    pub physiotherapy: Vec<u8>,
    pub medical_level: u8,
    #[serde(default = "baseline_facility_level")]
    pub training_level: u8,
}
fn baseline_facility_level() -> u8 {
    1
}

/// Previously approved career-runtime patch: bounded gains, not free recovery.
pub fn training_facility_multiplier(level: u8) -> f64 {
    1.0 + f64::from(level.clamp(1, 6) - 1) * 0.05
}
impl Default for ClubTraining {
    fn default() -> Self {
        Self {
            focus: Focus::default(),
            intensity: Intensity::default(),
            schedule: Schedule::default(),
            groups: vec![],
            coaches: vec![],
            physiotherapy: vec![],
            medical_level: 1,
            training_level: 1,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerTraining {
    pub birth_year: u32,
    pub potential: u8,
    pub natural_position: Position,
    pub position: Position,
    pub individual_focus: Option<Focus>,
}
impl PlayerTraining {
    pub fn primary_position(&self) -> Position {
        if self.natural_position.is_legacy() {
            self.position.canonical()
        } else {
            self.natural_position.canonical()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingOutcome {
    pub effective_focus: Focus,
    pub training_day: bool,
    pub condition_before: u8,
    pub condition_after: u8,
    pub fitness_before: u8,
    pub fitness_after: u8,
    pub ovr_before: u8,
    pub ovr_after: u8,
    pub potential_before: u8,
    pub potential_after: u8,
    pub attributes_gained: Vec<String>,
}

pub fn validate(
    player: &PlayerData,
    meta: &PlayerTraining,
    plan: &ClubTraining,
    today: NaiveDate,
    morale: u8,
) -> Result<(), String> {
    if today.year() < 0
        || meta.birth_year > today.year() as u32
        || meta.potential > 100
        || player.ovr > 100
        || player.condition > 100
        || player.fitness > 100
        || morale > 100
        || attribute_values(player)
            .iter()
            .any(|(_, value)| *value > 100)
        || plan.coaches.iter().any(|coach| coach.coaching > 100)
        || plan.physiotherapy.iter().any(|rating| *rating > 100)
    {
        return Err("Invalid training player/staff/facility inputs".into());
    }
    Ok(())
}

/// Individual focus wins over the last matching group, then the team default.
pub fn effective_focus(player_id: &str, meta: &PlayerTraining, plan: &ClubTraining) -> Focus {
    meta.individual_focus
        .or_else(|| {
            plan.groups
                .iter()
                .rev()
                .find(|group| group.player_ids.iter().any(|id| id == player_id))
                .map(|group| group.focus)
        })
        .unwrap_or(plan.focus)
}

pub fn train(
    player: &mut PlayerData,
    meta: &mut PlayerTraining,
    plan: &ClubTraining,
    today: NaiveDate,
    morale: u8,
    injured: bool,
    rng: &mut impl Rng,
) -> Result<TrainingOutcome, String> {
    validate(player, meta, plan, today, morale)?;
    let before = player.clone();
    let potential_before = meta.potential;
    let focus = effective_focus(&player.id, meta, plan);
    let training_day = plan
        .schedule
        .is_training_day(today.weekday().num_days_from_monday());
    train_inner(
        player,
        meta,
        plan,
        today.year() as u32,
        morale,
        injured,
        focus,
        training_day,
        rng,
    );
    let old = attribute_values(&before);
    let attributes_gained = attribute_values(player)
        .into_iter()
        .zip(old)
        .filter(|((_, new), (_, old))| new > old)
        .map(|((name, _), _)| name.into())
        .collect();
    Ok(TrainingOutcome {
        effective_focus: focus,
        training_day,
        condition_before: before.condition,
        condition_after: player.condition,
        fitness_before: before.fitness,
        fitness_after: player.fitness,
        ovr_before: before.ovr,
        ovr_after: player.ovr,
        potential_before,
        potential_after: meta.potential,
        attributes_gained,
    })
}

fn train_inner(
    player: &mut PlayerData,
    meta: &mut PlayerTraining,
    plan: &ClubTraining,
    year: u32,
    morale: u8,
    injured: bool,
    focus: Focus,
    training_day: bool,
    rng: &mut impl Rng,
) {
    let intensity = match plan.intensity {
        Intensity::Low => 0.5,
        Intensity::Medium => 1.0,
        Intensity::High => 1.5,
    };
    let physio = if plan.physiotherapy.is_empty() {
        1.0
    } else {
        1.0 + (plan
            .physiotherapy
            .iter()
            .map(|value| f64::from(*value))
            .sum::<f64>()
            / plan.physiotherapy.len() as f64
            / 100.0)
            * 0.4
    };
    let medical = 1.0 + f64::from(plan.medical_level.saturating_sub(1)) * 0.1;
    let base = if !training_day {
        7.0
    } else if focus == Focus::Recovery {
        9.0
    } else {
        3.0
    } * physio
        * medical;
    // Upstream training uses current year minus birth year, not precise birthday.
    let age = year.saturating_sub(meta.birth_year);
    let age_rec = match age {
        0..=21 => 1.10,
        22..=25 => 1.05,
        26..=29 => 1.0,
        30..=33 => 0.85,
        _ => 0.70,
    };
    let morale_rec = if morale >= 70 {
        1.10
    } else if morale >= 40 {
        1.0
    } else {
        0.90
    };
    let condition_rec = if player.condition < 30 {
        0.80
    } else if player.condition < 50 {
        0.90
    } else {
        1.0
    };
    let fitness_rec = match player.fitness {
        0..=29 => 0.75,
        30..=49 => 0.88,
        50..=69 => 1.0,
        70..=89 => 1.12,
        _ => 1.20,
    };
    if injured {
        let recovery = (base * 0.5 * age_rec * morale_rec * fitness_rec) as u8;
        player.condition = player.condition.saturating_add(recovery).min(100);
        player.fitness = player.fitness.saturating_sub(1);
        return;
    }
    if !training_day {
        let recovery = (base
            * (0.5 + f64::from(player.stamina) / 100.0 * 0.5)
            * age_rec
            * morale_rec
            * condition_rec
            * fitness_rec) as u8;
        player.condition = player.condition.saturating_add(recovery).min(100);
        return;
    }
    let coaching = if plan.coaches.is_empty() {
        0.8
    } else {
        0.85 + (plan
            .coaches
            .iter()
            .map(|coach| f64::from(coach.coaching))
            .sum::<f64>()
            / plan.coaches.len() as f64
            / 100.0)
            * 0.5
    };
    // Source computes staff specialization against team focus, even when a
    // player's individual/group focus differs. Preserve that causal detail.
    let target = match plan.focus {
        Focus::Physical => Some(Specialization::Fitness),
        Focus::Technical => Some(Specialization::Technique),
        Focus::Tactical => Some(Specialization::Tactics),
        Focus::Defending => Some(Specialization::Defending),
        Focus::Attacking => Some(Specialization::Attacking),
        Focus::Recovery => None,
    };
    let specialized = if target.is_some()
        && plan
            .coaches
            .iter()
            .any(|coach| coach.specialization == target)
    {
        1.25
    } else {
        1.0
    };
    let age_gain = match age {
        0..=21 => 1.5,
        22..=25 => 1.2,
        26..=29 => 1.0,
        30..=33 => 0.6,
        _ => 0.3,
    };
    let gain = 0.15
        * intensity
        * age_gain
        * coaching
        * specialized
        * training_facility_multiplier(plan.training_level);
    if meta.potential > player.ovr {
        focus_gains(player, focus, gain, rng);
    }
    match focus {
        Focus::Physical => {
            let roll: f64 = rng.random_range(0.0..1.0);
            if roll < 0.015 * intensity && player.fitness < 100 {
                player.fitness += 1;
            }
        }
        Focus::Recovery => {
            let roll: f64 = rng.random_range(0.0..1.0);
            if roll < 0.05 && player.fitness < 100 {
                player.fitness += 1;
            }
        }
        _ if player.fitness > 85 => {
            let roll: f64 = rng.random_range(0.0..1.0);
            if roll < 0.05 {
                player.fitness -= 1;
            }
        }
        _ => {}
    }
    refresh_derived(player, meta, year, rng);
    let cost = if focus == Focus::Recovery {
        0
    } else {
        match plan.intensity {
            Intensity::Low => 3,
            Intensity::Medium => 6,
            Intensity::High => 10,
        }
    };
    player.condition = player.condition.saturating_sub(cost);
    let recovery = (base
        * (0.5 + f64::from(player.stamina) / 100.0 * 0.5)
        * age_rec
        * morale_rec
        * condition_rec
        * fitness_rec) as u8;
    player.condition = player.condition.saturating_add(recovery).min(100);
}

fn try_gain(value: &mut u8, chance: f64, rng: &mut impl Rng) {
    if *value >= 99 {
        return;
    }
    let roll: f64 = rng.random_range(0.0..1.0);
    if roll < chance {
        *value += 1;
    }
}
fn focus_gains(p: &mut PlayerData, focus: Focus, gain: f64, rng: &mut impl Rng) {
    match focus {
        Focus::Physical => {
            try_gain(&mut p.pace, gain, rng);
            try_gain(&mut p.stamina, gain, rng);
            try_gain(&mut p.strength, gain, rng);
            try_gain(&mut p.agility, gain, rng);
        }
        Focus::Technical => {
            try_gain(&mut p.passing, gain, rng);
            try_gain(&mut p.shooting, gain, rng);
            try_gain(&mut p.dribbling, gain, rng);
        }
        Focus::Tactical => {
            try_gain(&mut p.positioning, gain, rng);
            try_gain(&mut p.vision, gain, rng);
            try_gain(&mut p.decisions, gain, rng);
            try_gain(&mut p.composure, gain, rng);
        }
        Focus::Defending => {
            try_gain(&mut p.tackling, gain, rng);
            try_gain(&mut p.defending, gain, rng);
            try_gain(&mut p.strength, gain * 0.5, rng);
            try_gain(&mut p.positioning, gain * 0.5, rng);
        }
        Focus::Attacking => {
            try_gain(&mut p.shooting, gain, rng);
            try_gain(&mut p.dribbling, gain, rng);
            try_gain(&mut p.pace, gain * 0.5, rng);
        }
        Focus::Recovery => {}
    }
}

pub fn refresh_derived(
    player: &mut PlayerData,
    meta: &mut PlayerTraining,
    year: u32,
    rng: &mut impl Rng,
) {
    let ovr = ovr_for_position(player, meta.primary_position()).round() as u8;
    let age = year.saturating_sub(meta.birth_year);
    if meta.potential == 0 {
        let bonus: u8 = match age {
            0..=18 => rng.random_range(15..=30),
            19..=20 => rng.random_range(8..=22),
            21..=22 => rng.random_range(4..=14),
            23..=25 => rng.random_range(0..=7),
            _ => 0,
        };
        meta.potential = ovr.saturating_add(bonus).min(99).max(ovr.max(1));
    } else {
        meta.potential = meta.potential.max(ovr);
    }
    player.ovr = ovr;
    player.traits = compute_traits(player);
    if age <= 20 && meta.potential >= 90 && meta.potential.saturating_sub(ovr) >= 14 {
        player.traits.push("Wonderkid".into());
    }
}

pub fn ovr_for_position(p: &PlayerData, position: Position) -> f64 {
    let (values, critical): (Vec<(u8, u32)>, u8) = match position.canonical() {
        Position::Goalkeeper => (
            vec![
                (p.handling, 28),
                (p.reflexes, 28),
                (p.aerial, 14),
                (p.positioning, 10),
                (p.decisions, 10),
                (p.composure, 5),
                (p.strength, 5),
            ],
            p.handling.min(p.reflexes).min(p.positioning),
        ),
        Position::RightBack | Position::LeftBack => (
            vec![
                (p.pace, 18),
                (p.stamina, 16),
                (p.tackling, 17),
                (p.defending, 16),
                (p.positioning, 12),
                (p.passing, 10),
                (p.dribbling, 6),
                (p.decisions, 5),
            ],
            p.tackling.min(p.defending).min(p.positioning),
        ),
        Position::CenterBack => (
            vec![
                (p.defending, 24),
                (p.tackling, 18),
                (p.positioning, 18),
                (p.strength, 14),
                (p.aerial, 12),
                (p.decisions, 8),
                (p.composure, 6),
            ],
            p.defending.min(p.tackling).min(p.positioning),
        ),
        Position::RightWingBack | Position::LeftWingBack => (
            vec![
                (p.pace, 18),
                (p.stamina, 18),
                (p.tackling, 14),
                (p.defending, 12),
                (p.passing, 13),
                (p.dribbling, 11),
                (p.vision, 7),
                (p.decisions, 7),
            ],
            p.pace.min(p.stamina).min(p.tackling),
        ),
        Position::DefensiveMidfielder => (
            vec![
                (p.tackling, 18),
                (p.positioning, 18),
                (p.decisions, 16),
                (p.passing, 14),
                (p.defending, 12),
                (p.stamina, 10),
                (p.vision, 7),
                (p.strength, 5),
            ],
            p.tackling.min(p.positioning).min(p.passing),
        ),
        Position::CentralMidfielder => (
            vec![
                (p.passing, 20),
                (p.vision, 16),
                (p.decisions, 16),
                (p.stamina, 12),
                (p.dribbling, 10),
                (p.positioning, 9),
                (p.teamwork, 9),
                (p.tackling, 8),
            ],
            p.passing.min(p.vision).min(p.decisions),
        ),
        Position::AttackingMidfielder => (
            vec![
                (p.vision, 20),
                (p.passing, 18),
                (p.dribbling, 16),
                (p.decisions, 14),
                (p.shooting, 10),
                (p.positioning, 8),
                (p.composure, 8),
                (p.pace, 6),
            ],
            p.vision.min(p.passing).min(p.dribbling),
        ),
        Position::RightMidfielder | Position::LeftMidfielder => (
            vec![
                (p.pace, 17),
                (p.stamina, 16),
                (p.passing, 15),
                (p.dribbling, 14),
                (p.vision, 10),
                (p.decisions, 10),
                (p.positioning, 10),
                (p.tackling, 8),
            ],
            p.pace.min(p.passing).min(p.stamina),
        ),
        Position::RightWinger | Position::LeftWinger => (
            vec![
                (p.pace, 22),
                (p.dribbling, 22),
                (p.passing, 14),
                (p.shooting, 12),
                (p.vision, 10),
                (p.decisions, 8),
                (p.positioning, 6),
                (p.stamina, 6),
            ],
            p.pace.min(p.dribbling).min(p.passing),
        ),
        Position::Striker => (
            vec![
                (p.shooting, 26),
                (p.positioning, 18),
                (p.decisions, 14),
                (p.pace, 12),
                (p.dribbling, 10),
                (p.strength, 8),
                (p.composure, 8),
                (p.aerial, 4),
            ],
            p.shooting.min(p.positioning).min(p.decisions),
        ),
        _ => unreachable!("canonical positions remove legacy buckets"),
    };
    let base = values
        .into_iter()
        .map(|(value, weight)| f64::from(value) * f64::from(weight))
        .sum::<f64>()
        / 100.0;
    let penalty = if critical >= 45 {
        0.0
    } else {
        f64::from(45 - critical) * 0.6
    };
    (base - penalty).clamp(1.0, 99.0)
}

pub fn compute_traits(p: &PlayerData) -> Vec<String> {
    [
        (p.pace >= 85, "Speedster"),
        (p.strength >= 85 && p.stamina >= 75, "Tank"),
        (p.agility >= 85, "Agile"),
        (p.stamina >= 90, "Tireless"),
        (p.passing >= 80 && p.vision >= 80, "Playmaker"),
        (p.shooting >= 85, "Sharpshooter"),
        (p.dribbling >= 85, "Dribbler"),
        (p.tackling >= 80 && p.aggression >= 70, "BallWinner"),
        (p.defending >= 85 && p.positioning >= 75, "Rock"),
        (p.leadership >= 85 && p.teamwork >= 75, "Leader"),
        (p.composure >= 85 && p.decisions >= 80, "CoolHead"),
        (p.vision >= 85, "Visionary"),
        (p.aggression >= 85 && p.composure < 50, "HotHead"),
        (p.teamwork >= 85, "TeamPlayer"),
        (p.handling >= 85, "SafeHands"),
        (p.reflexes >= 85, "CatReflexes"),
        (p.aerial >= 85, "AerialDominance"),
        (
            p.shooting >= 75 && p.dribbling >= 75 && p.pace >= 70 && p.strength >= 70,
            "CompleteForward",
        ),
        (
            p.stamina >= 85 && p.pace >= 70 && p.teamwork >= 75,
            "Engine",
        ),
        (
            p.passing >= 80 && p.shooting >= 75 && p.vision >= 75,
            "SetPieceSpecialist",
        ),
    ]
    .into_iter()
    .filter(|(present, _)| *present)
    .map(|(_, name)| name.into())
    .collect()
}

fn attribute_values(p: &PlayerData) -> [(&'static str, u8); 19] {
    [
        ("pace", p.pace),
        ("stamina", p.stamina),
        ("strength", p.strength),
        ("agility", p.agility),
        ("passing", p.passing),
        ("shooting", p.shooting),
        ("tackling", p.tackling),
        ("dribbling", p.dribbling),
        ("defending", p.defending),
        ("positioning", p.positioning),
        ("vision", p.vision),
        ("decisions", p.decisions),
        ("composure", p.composure),
        ("aggression", p.aggression),
        ("teamwork", p.teamwork),
        ("leadership", p.leadership),
        ("handling", p.handling),
        ("reflexes", p.reflexes),
        ("aerial", p.aerial),
    ]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FitnessWarning {
    pub critical: bool,
    pub average_condition: f64,
    pub exhausted_count: usize,
    pub critical_player_ids: Vec<String>,
}
/// Input consists only of this club's uninjured players in source registry order.
/// The host owns per-club/day deduplication and recipient-scoped inbox delivery.
pub fn fitness_warning(players: &[(&str, u8)]) -> Option<FitnessWarning> {
    if players.is_empty() {
        return None;
    }
    let average_condition = players
        .iter()
        .map(|(_, condition)| f64::from(*condition))
        .sum::<f64>()
        / players.len() as f64;
    let exhausted_count = players
        .iter()
        .filter(|(_, condition)| *condition < 40)
        .count();
    let critical_count = players
        .iter()
        .filter(|(_, condition)| *condition < 25)
        .count();
    if critical_count < 3 && average_condition >= 50.0 && exhausted_count < 4 {
        return None;
    }
    Some(FitnessWarning {
        critical: critical_count >= 3,
        average_condition,
        exhausted_count,
        critical_player_ids: players
            .iter()
            .filter(|(_, condition)| *condition < 25)
            .take(5)
            .map(|(id, _)| (*id).into())
            .collect(),
    })
}
