use chrono::NaiveDate;
use management::training::*;
use rand::{Rng, SeedableRng, TryRng};
use std::convert::Infallible;

struct Fixed(u64);
impl TryRng for Fixed {
    type Error = Infallible;
    fn try_next_u32(&mut self) -> Result<u32, Infallible> {
        Ok((self.0 >> 32) as u32)
    }
    fn try_next_u64(&mut self) -> Result<u64, Infallible> {
        Ok(self.0)
    }
    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Infallible> {
        for chunk in dst.chunks_mut(8) {
            chunk.copy_from_slice(&self.0.to_le_bytes()[..chunk.len()]);
        }
        Ok(())
    }
}
fn roll(value: f64) -> Fixed {
    Fixed((value * u64::MAX as f64) as u64)
}
fn date(text: &str) -> NaiveDate {
    text.parse().unwrap()
}
fn player() -> engine::PlayerData {
    let mut data = serde_json::json!({"id":"p", "name":"Player", "position":"Midfielder", "role":"Standard", "traits":[]});
    for field in [
        "ovr",
        "condition",
        "fitness",
        "pace",
        "stamina",
        "strength",
        "agility",
        "passing",
        "shooting",
        "tackling",
        "dribbling",
        "defending",
        "positioning",
        "vision",
        "decisions",
        "composure",
        "aggression",
        "teamwork",
        "leadership",
        "handling",
        "reflexes",
        "aerial",
    ] {
        data[field] = serde_json::json!(65);
    }
    serde_json::from_value(data).unwrap()
}
fn meta() -> PlayerTraining {
    PlayerTraining {
        birth_year: 2000,
        potential: 90,
        natural_position: Position::CentralMidfielder,
        position: Position::CentralMidfielder,
        individual_focus: None,
    }
}

#[test]
fn each_focus_changes_exact_source_attributes_and_potential_stops_growth() {
    for (focus, expected) in [
        (
            Focus::Physical,
            vec!["pace", "stamina", "strength", "agility"],
        ),
        (Focus::Technical, vec!["passing", "shooting", "dribbling"]),
        (
            Focus::Tactical,
            vec!["positioning", "vision", "decisions", "composure"],
        ),
        (
            Focus::Defending,
            vec!["strength", "tackling", "defending", "positioning"],
        ),
        (Focus::Attacking, vec!["pace", "shooting", "dribbling"]),
        (Focus::Recovery, vec![]),
    ] {
        let mut p = player();
        let mut m = meta();
        let plan = ClubTraining {
            focus,
            ..Default::default()
        };
        let result = train(
            &mut p,
            &mut m,
            &plan,
            date("2026-01-05"),
            70,
            false,
            &mut Fixed(0),
        )
        .unwrap();
        assert_eq!(
            result.attributes_gained, expected,
            "Wrong attributes for {focus:?}"
        );
        let mut capped = player();
        let mut ceiling = meta();
        ceiling.potential = capped.ovr;
        let result = train(
            &mut capped,
            &mut ceiling,
            &plan,
            date("2026-01-05"),
            70,
            false,
            &mut Fixed(0),
        )
        .unwrap();
        assert!(result.attributes_gained.is_empty());
        assert_eq!(ceiling.potential, 65);
    }
}

#[test]
fn schedules_and_intensity_have_exact_rest_days_and_condition_costs() {
    assert_eq!(
        (0..7)
            .filter(|day| Schedule::Intense.is_training_day(*day))
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4, 5]
    );
    assert_eq!(
        (0..7)
            .filter(|day| Schedule::Balanced.is_training_day(*day))
            .collect::<Vec<_>>(),
        vec![0, 1, 3, 4]
    );
    assert_eq!(
        (0..7)
            .filter(|day| Schedule::Light.is_training_day(*day))
            .collect::<Vec<_>>(),
        vec![1, 3]
    );
    for (intensity, condition) in [
        (Intensity::Low, 64),
        (Intensity::Medium, 61),
        (Intensity::High, 57),
    ] {
        let mut p = player();
        let mut m = meta();
        let plan = ClubTraining {
            intensity,
            ..Default::default()
        };
        train(
            &mut p,
            &mut m,
            &plan,
            date("2026-01-05"),
            70,
            false,
            &mut Fixed(u64::MAX),
        )
        .unwrap();
        assert_eq!(p.condition, condition);
    }
    let mut p = player();
    let mut m = meta();
    let rest = train(
        &mut p,
        &mut m,
        &ClubTraining::default(),
        date("2026-01-04"),
        70,
        false,
        &mut Fixed(0),
    )
    .unwrap();
    assert_eq!(p.condition, 71);
    assert_eq!(p.fitness, 65);
    assert!(rest.attributes_gained.is_empty());
}

#[test]
fn individual_then_last_group_then_default_focus_and_team_specialization() {
    let mut m = meta();
    let mut plan = ClubTraining::default();
    plan.groups = vec![
        Group {
            id: "first".into(),
            name: "First".into(),
            focus: Focus::Defending,
            player_ids: vec!["p".into()],
        },
        Group {
            id: "second".into(),
            name: "Second".into(),
            focus: Focus::Tactical,
            player_ids: vec!["p".into()],
        },
    ];
    assert_eq!(effective_focus("p", &m, &plan), Focus::Tactical);
    assert_eq!(effective_focus("other", &m, &plan), Focus::Physical);
    m.individual_focus = Some(Focus::Attacking);
    assert_eq!(effective_focus("p", &m, &plan), Focus::Attacking);
    plan.coaches = vec![Coach {
        coaching: 0,
        specialization: Some(Specialization::Fitness),
    }];
    let mut p = player();
    let specialist = train(
        &mut p,
        &mut m.clone(),
        &plan,
        date("2026-01-05"),
        70,
        false,
        &mut roll(0.14),
    )
    .unwrap();
    assert!(specialist.attributes_gained.contains(&"shooting".into()));
    plan.coaches[0].specialization = Some(Specialization::Attacking);
    let regular = train(
        &mut player(),
        &mut m,
        &plan,
        date("2026-01-05"),
        70,
        false,
        &mut roll(0.14),
    )
    .unwrap();
    assert!(
        regular.attributes_gained.is_empty(),
        "Specialist is matched to team focus, not override"
    );
}

#[test]
fn recovery_uses_starting_fitness_and_injured_branch_has_no_training_cost_or_growth() {
    let mut p = player();
    p.fitness = 69;
    let result = train(
        &mut p,
        &mut meta(),
        &ClubTraining {
            focus: Focus::Recovery,
            ..Default::default()
        },
        date("2026-01-05"),
        70,
        false,
        &mut Fixed(0),
    )
    .unwrap();
    assert_eq!(p.fitness, 70);
    assert_eq!(p.condition, 73);
    assert!(result.attributes_gained.is_empty());
    let mut injured = player();
    injured.condition = 20;
    injured.fitness = 70;
    let result = train(
        &mut injured,
        &mut meta(),
        &ClubTraining {
            intensity: Intensity::High,
            ..Default::default()
        },
        date("2026-01-05"),
        70,
        true,
        &mut Fixed(0),
    )
    .unwrap();
    assert_eq!(injured.condition, 21);
    assert_eq!(injured.fitness, 69);
    assert!(result.attributes_gained.is_empty());
    assert_eq!(injured.ovr, 65);
    let mut exhausted = player();
    exhausted.condition = 20;
    let result = train(
        &mut exhausted,
        &mut meta(),
        &ClubTraining::default(),
        date("2026-01-05"),
        70,
        false,
        &mut Fixed(0),
    )
    .unwrap();
    assert_eq!(
        result.effective_focus,
        Focus::Physical,
        "No privileged automatic AI fatigue branch"
    );
}

#[test]
fn staff_medical_age_and_morale_factors_and_fitness_decay_match_source() {
    let mut p = player();
    let mut m = meta();
    m.birth_year = 1986;
    let plan = ClubTraining {
        medical_level: 10,
        physiotherapy: vec![100],
        ..Default::default()
    };
    train(
        &mut p,
        &mut m,
        &plan,
        date("2026-01-04"),
        30,
        false,
        &mut Fixed(0),
    )
    .unwrap();
    assert_eq!(p.condition, 74); // floor(7*1.4*1.9*.825*.7*.9)=9
    let mut p = player();
    p.fitness = 86;
    train(
        &mut p,
        &mut meta(),
        &ClubTraining {
            focus: Focus::Tactical,
            ..Default::default()
        },
        date("2026-01-05"),
        70,
        false,
        &mut Fixed(0),
    )
    .unwrap();
    assert_eq!(p.fitness, 85);
    let mut p = player();
    let mut m = meta();
    let coached = ClubTraining {
        coaches: vec![Coach {
            coaching: 100,
            specialization: Some(Specialization::Fitness),
        }],
        ..Default::default()
    };
    let result = train(
        &mut p,
        &mut m,
        &coached,
        date("2026-01-05"),
        70,
        false,
        &mut roll(0.20),
    )
    .unwrap();
    assert_eq!(result.attributes_gained.len(), 4);
    let result = train(
        &mut player(),
        &mut meta(),
        &ClubTraining::default(),
        date("2026-01-05"),
        70,
        false,
        &mut roll(0.20),
    )
    .unwrap();
    assert!(result.attributes_gained.is_empty());
}

#[test]
fn ratings_traits_potential_generation_and_legacy_position_fallback() {
    let mut p = player();
    p.handling = 90;
    p.reflexes = 90;
    p.aerial = 85;
    p.positioning = 85;
    assert_eq!(ovr_for_position(&p, Position::Goalkeeper), 83.8);
    assert_eq!(
        ovr_for_position(&p, Position::Defender),
        ovr_for_position(&p, Position::CenterBack)
    );
    let mut m = meta();
    m.natural_position = Position::Defender;
    m.position = Position::Goalkeeper;
    m.potential = 0;
    refresh_derived(&mut p, &mut m, 2026, &mut Fixed(0));
    assert_eq!(p.ovr, 84);
    assert_eq!(m.potential, 84);
    assert!(p.traits.contains(&"SafeHands".into()));
    let mut p = player();
    let mut m = meta();
    m.birth_year = 2008;
    m.potential = 0;
    refresh_derived(&mut p, &mut m, 2026, &mut Fixed(0));
    assert_eq!(m.potential, 80);
    m.potential = 90;
    refresh_derived(&mut p, &mut m, 2026, &mut Fixed(0));
    assert!(p.traits.contains(&"Wonderkid".into()));
    m.birth_year = 2005;
    refresh_derived(&mut p, &mut m, 2026, &mut Fixed(0));
    assert!(!p.traits.contains(&"Wonderkid".into()));
}

#[test]
fn deterministic_seed_and_invalid_input_do_not_consume_rng_or_mutate_state() {
    let mut p = player();
    let mut m = meta();
    let before = serde_json::to_value(&p).unwrap();
    let mut first = rand::rngs::StdRng::seed_from_u64(42);
    let mut second = rand::rngs::StdRng::seed_from_u64(42);
    let invalid = ClubTraining {
        physiotherapy: vec![101],
        ..Default::default()
    };
    assert!(
        train(
            &mut p,
            &mut m,
            &invalid,
            date("2026-01-05"),
            70,
            false,
            &mut first
        )
        .is_err()
    );
    assert_eq!(serde_json::to_value(&p).unwrap(), before);
    assert_eq!(m, meta());
    assert_eq!(first.next_u64(), second.next_u64());
    let a = train(
        &mut p,
        &mut m,
        &ClubTraining::default(),
        date("2026-01-05"),
        70,
        false,
        &mut first,
    )
    .unwrap();
    let b = train(
        &mut player(),
        &mut meta(),
        &ClubTraining::default(),
        date("2026-01-05"),
        70,
        false,
        &mut second,
    )
    .unwrap();
    assert_eq!(a, b);
}

#[test]
fn fitness_warning_thresholds_are_recipient_ready_publication_inputs() {
    assert!(fitness_warning(&[]).is_none());
    assert!(fitness_warning(&[("p", 50)]).is_none());
    assert!(!fitness_warning(&[("p", 49)]).unwrap().critical);
    assert!(
        fitness_warning(&[("a", 24), ("b", 24), ("c", 24), ("d", 100)])
            .unwrap()
            .critical
    );
    let crowded = fitness_warning(&[
        ("a", 39),
        ("b", 39),
        ("c", 39),
        ("d", 39),
        ("e", 100),
        ("f", 100),
    ])
    .unwrap();
    assert_eq!(crowded.exhausted_count, 4);
    assert!(!crowded.critical);
}

#[test]
fn approved_training_facility_effect_increases_growth_not_recovery() {
    assert_eq!(training_facility_multiplier(0), 1.0);
    assert_eq!(training_facility_multiplier(1), 1.0);
    assert_eq!(training_facility_multiplier(6), 1.25);
    assert_eq!(training_facility_multiplier(255), 1.25);
    let mut base_gains = 0;
    let mut upgraded_gains = 0;
    for seed in 1001..3001 {
        let mut base = player();
        let mut improved = base.clone();
        let mut base_meta = meta();
        let mut improved_meta = base_meta.clone();
        let plan = ClubTraining {
            focus: Focus::Technical,
            ..Default::default()
        };
        let upgraded = ClubTraining {
            training_level: 6,
            ..plan.clone()
        };
        let first = train(
            &mut base,
            &mut base_meta,
            &plan,
            date("2026-01-05"),
            70,
            false,
            &mut rand::rngs::StdRng::seed_from_u64(seed),
        )
        .unwrap();
        let second = train(
            &mut improved,
            &mut improved_meta,
            &upgraded,
            date("2026-01-05"),
            70,
            false,
            &mut rand::rngs::StdRng::seed_from_u64(seed),
        )
        .unwrap();
        base_gains += first.attributes_gained.len();
        upgraded_gains += second.attributes_gained.len();
        assert_eq!(base.condition, improved.condition);
        assert_eq!(base.fitness, improved.fitness);
    }
    assert!(upgraded_gains > base_gains);
    assert!(upgraded_gains < base_gains * 3 / 2);
}
