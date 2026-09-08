//! Seasonal age curves/retirement from pinned OpenFoot Manager 64677fee.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
//! Preserves source DefaultHasher id/season/salt draws, not ambient RNG.
use chrono::{Datelike, NaiveDate};
use domain::player::Player;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const MIN_ATTRIBUTE: u8 = 1;
const MAX_ATTRIBUTE: u8 = 99;

fn player_age_on(current_date: NaiveDate, date_of_birth: &str) -> i32 {
    let Ok(dob) = NaiveDate::parse_from_str(date_of_birth, "%Y-%m-%d") else {
        return 30;
    };

    let mut age = current_date.year() - dob.year();
    if current_date.ordinal() < dob.ordinal() {
        age -= 1;
    }
    age
}

fn seeded_value(player_id: &str, season: u32, salt: &str) -> u32 {
    let mut hasher = DefaultHasher::new();
    player_id.hash(&mut hasher);
    season.hash(&mut hasher);
    salt.hash(&mut hasher);
    (hasher.finish() % u32::MAX as u64) as u32
}

fn veteran_pace_loss(player_id: &str, age: i32, season: u32) -> u8 {
    if age < 30 {
        return 0;
    }

    1 + (seeded_value(player_id, season, "pace-loss") % 3) as u8
}

fn technical_growth(player_id: &str, age: i32, season: u32) -> u8 {
    if age > 32 {
        return 0;
    }

    (seeded_value(player_id, season, "technical-growth") % 2) as u8
}

fn increase_attribute(value: &mut u8, delta: u8) {
    *value = value.saturating_add(delta).min(MAX_ATTRIBUTE);
}

fn decrease_attribute(value: &mut u8, delta: u8) {
    *value = value.saturating_sub(delta).max(MIN_ATTRIBUTE);
}

fn apply_attribute_curve(player: &mut Player, age: i32, season: u32) {
    let pace_loss = veteran_pace_loss(&player.id, age, season);
    if pace_loss > 0 {
        decrease_attribute(&mut player.attributes.pace, pace_loss);
    }

    let growth = technical_growth(&player.id, age, season);
    if growth > 0 {
        increase_attribute(&mut player.attributes.passing, growth);
        increase_attribute(&mut player.attributes.vision, growth);
        increase_attribute(&mut player.attributes.decisions, growth);
        increase_attribute(&mut player.attributes.composure, growth);
    }
}

fn has_expired_contract(player: &Player, current_date: NaiveDate) -> bool {
    player
        .contract_end
        .as_deref()
        .and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
        .is_some_and(|contract_end| contract_end < current_date)
}

fn retirement_chance(player: &Player, age: i32, current_date: NaiveDate) -> u32 {
    if player.retired || age < 33 {
        return 0;
    }

    let mut chance: u32 = match age {
        33 => 12,
        34 => 24,
        35 => 42,
        36 => 60,
        37 => 78,
        _ => 100,
    };

    if player.contract_end.is_none() || has_expired_contract(player, current_date) {
        chance += 18;
    }
    if player.team_id.is_none() {
        chance += 10;
    }
    if player.stats.appearances < 10 {
        chance += 8;
    }
    if player.stats.avg_rating <= 6.4 {
        chance += 8;
    }
    if player.stats.avg_rating >= 7.4 {
        chance = chance.saturating_sub(10);
    }
    if player.ovr >= 80 {
        chance = chance.saturating_sub(15);
    }

    chance.min(100)
}

fn should_retire(player: &Player, age: i32, current_date: NaiveDate, season: u32) -> bool {
    let chance = retirement_chance(player, age, current_date);
    if chance == 0 {
        return false;
    }

    let roll = seeded_value(&player.id, season, "retirement-roll") % 100;
    roll < chance
}

fn retire_player(player: &mut Player) {
    player.retired = true;
    player.team_id = None;
    player.contract_end = None;
    player.transfer_listed = false;
    player.loan_listed = false;
    player.transfer_offers.clear();
}

pub fn apply(
    players: &mut [Player],
    teams: &mut std::collections::BTreeMap<String, domain::team::Team>,
    date: NaiveDate,
    season: u32,
) {
    for player in players {
        if player.retired {
            continue;
        }
        let age = player_age_on(date, &player.date_of_birth);
        apply_attribute_curve(player, age, season);
        if should_retire(player, age, date, season) {
            if let Some(team) = player.team_id.as_ref().and_then(|id| teams.get_mut(id)) {
                team.remove_player_references(&player.id);
            }
            retire_player(player);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_age_curves_and_retirement_modifiers_are_exact() {
        let game = crate::personnel::tests::game();
        let mut p = game.project_source_players().unwrap()["a-p"].clone();
        let date = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        p.date_of_birth = "1993-01-01".into();
        p.contract_end = Some("2028-01-01".into());
        p.stats.appearances = 20;
        p.stats.avg_rating = 7.0;
        p.ovr = 70;
        assert_eq!(retirement_chance(&p, 33, date), 12);
        p.contract_end = None;
        assert_eq!(retirement_chance(&p, 33, date), 30);
        p.team_id = None;
        assert_eq!(retirement_chance(&p, 33, date), 40);
        p.stats.appearances = 9;
        p.stats.avg_rating = 6.4;
        assert_eq!(retirement_chance(&p, 33, date), 56);
        p.stats.avg_rating = 7.4;
        p.ovr = 80;
        assert_eq!(retirement_chance(&p, 33, date), 23);
        assert_eq!(retirement_chance(&p, 32, date), 0);
        assert_eq!(veteran_pace_loss("p", 29, 1), 0);
        assert!((1..=3).contains(&veteran_pace_loss("p", 30, 1)));
        assert_eq!(technical_growth("p", 33, 1), 0);
        assert!(technical_growth("p", 32, 1) <= 1);
        let mut low = 1;
        decrease_attribute(&mut low, 3);
        assert_eq!(low, 1);
        let mut high = 99;
        increase_attribute(&mut high, 1);
        assert_eq!(high, 99);
    }
}
