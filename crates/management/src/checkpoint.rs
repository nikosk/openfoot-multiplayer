//! Versioned, private checkpoints for the trusted host only. Checkpoints contain
//! balances, receipts, negotiation previews, and former managers' private outcomes.
//! Never expose save/load through a manager or spectator interface. Validation
//! checks structural game invariants; it does not authenticate an arbitrary file
//! or prove that historical engine reports were produced by an honest execution.

use crate::football::{Football, Standing};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// JSON object keys cannot represent tuple keys. A sorted entry array retains
/// them without encoding delimiters into IDs; duplicates are rejected on load.
pub(crate) mod entries {
    use super::*;

    pub fn serialize<S, K, V>(map: &BTreeMap<K, V>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
        K: Serialize,
        V: Serialize,
    {
        map.iter().collect::<Vec<_>>().serialize(serializer)
    }

    pub fn deserialize<'de, D, K, V>(deserializer: D) -> Result<BTreeMap<K, V>, D::Error>
    where
        D: serde::Deserializer<'de>,
        K: Deserialize<'de> + Ord,
        V: Deserialize<'de>,
    {
        let mut map = BTreeMap::new();
        for (key, value) in Vec::<(K, V)>::deserialize(deserializer)? {
            if map.insert(key, value).is_some() {
                return Err(serde::de::Error::custom("duplicate checkpoint map key"));
            }
        }
        Ok(map)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    version: u32,
    state: Football,
}

impl Football {
    /// Trusted-host only. The result is sensitive even when there is no career.
    pub fn save_state(&self) -> Result<serde_json::Value, String> {
        self.validate_checkpoint()?;
        serde_json::to_value(Checkpoint {
            version: 1,
            state: self.clone(),
        })
        .map_err(|error| format!("Cannot serialize checkpoint: {error}"))
    }

    /// Restore a complete independent owner. Existing state is untouched if any
    /// decoding or validation fails; the host replaces it only after success.
    pub fn load_validated(value: serde_json::Value) -> Result<Self, String> {
        if value.get("version").and_then(serde_json::Value::as_u64) != Some(1) {
            return Err("Unsupported checkpoint version".into());
        }
        let checkpoint: Checkpoint = serde_json::from_value(value)
            .map_err(|error| format!("Cannot decode checkpoint: {error}"))?;
        checkpoint.state.validate_checkpoint()?;
        Ok(checkpoint.state)
    }

    fn validate_checkpoint(&self) -> Result<(), String> {
        let m = &self.management;
        let fail = |message: &str| Err::<(), String>(format!("Invalid checkpoint: {message}"));
        if m.clubs.is_empty() || m.managers.is_empty() {
            return fail("empty clubs or active managers");
        }
        if m.clubs
            .iter()
            .any(|(id, club)| id.trim().is_empty() || id != &club.id)
            || m.players.iter().any(|(id, player)| {
                id.trim().is_empty()
                    || id != &player.id
                    || (!player.club_id.is_empty() && !m.clubs.contains_key(&player.club_id))
            })
            || m.managers.iter().any(|(id, manager)| {
                id.trim().is_empty() || id != &manager.id || !m.clubs.contains_key(&manager.club_id)
            })
            || m.managers
                .values()
                .map(|manager| &manager.club_id)
                .collect::<BTreeSet<_>>()
                .len()
                != m.managers.len()
        {
            return fail("identity keys or ownership");
        }
        if m.clubs.keys().ne(m.club_revisions.keys())
            || m.players.keys().ne(m.player_revisions.keys())
            || m.players.keys().ne(self.attributes.keys())
        {
            return fail("revision or attribute registry coverage");
        }
        if ![0, 11].contains(&m.minimum_squad_size) {
            return fail("roster policy");
        }
        // Legal expiry may temporarily leave a club short of eleven. Preserve
        // that state: loading must not fabricate replacements or heal the roster.
        for (id, player) in &self.attributes {
            if id != &player.id
                || [
                    player.ovr,
                    player.condition,
                    player.fitness,
                    player.pace,
                    player.stamina,
                    player.strength,
                    player.agility,
                    player.passing,
                    player.shooting,
                    player.tackling,
                    player.dribbling,
                    player.defending,
                    player.positioning,
                    player.vision,
                    player.decisions,
                    player.composure,
                    player.aggression,
                    player.teamwork,
                    player.leadership,
                    player.handling,
                    player.reflexes,
                    player.aerial,
                ]
                .into_iter()
                .any(|value| value > 100)
            {
                return fail("player engine attributes");
            }
        }
        for (club, lineup) in &m.lineups {
            if !m.clubs.contains_key(club)
                || lineup.len() > 11
                || lineup.iter().collect::<BTreeSet<_>>().len() != lineup.len()
                || lineup.iter().any(|id| !m.players.contains_key(id))
            {
                return fail("saved lineup references");
            }
        }
        if m.match_plans
            .keys()
            .chain(m.recovery_modes.keys())
            .any(|club| !m.clubs.contains_key(club))
            || m.recovery_enabled != self.recovery.is_some()
        {
            return fail("club plan or recovery configuration");
        }
        if let Some(recovery) = &self.recovery {
            if recovery.players.keys().ne(m.players.keys())
                || recovery.clubs.keys().ne(m.clubs.keys())
                || recovery
                    .players
                    .values()
                    .any(|player| player.age > 120 || player.morale > 100)
            {
                return fail("recovery registry");
            }
            for club in recovery.clubs.values() {
                crate::recovery::validate_club(club)?;
            }
        }
        let mut sequences = BTreeSet::new();
        for ((actor, id), (request, receipt)) in &m.receipts {
            if actor.trim().is_empty()
                || id.is_empty()
                || id != &request.id
                || receipt.sequence == 0
                || receipt.sequence > m.sequence
                || !sequences.insert(receipt.sequence)
            {
                return fail("request receipt identity or sequence");
            }
        }
        for (id, offer) in &m.offers {
            if id != &offer.id
                || *id == 0
                || *id > m.sequence
                || !m.players.contains_key(&offer.player_id)
                || offer.buyer == offer.seller
                || !m.clubs.contains_key(&offer.buyer)
                || !m.clubs.contains_key(&offer.seller)
                || offer.fee == 0
                || offer.fee > i64::MAX as u64
            {
                return fail("transfer offer references");
            }
        }
        for (id, preview) in &m.previews {
            if id != &preview.view.id
                || *id == 0
                || *id > m.sequence
                || !m.managers.contains_key(&preview.actor)
                || preview.day != m.window.day
                || !m.offers.contains_key(&preview.view.offer.id)
            {
                return fail("transfer preview references");
            }
        }
        self.validate_fixture_checkpoint()?;
        self.validate_board_checkpoint()?;
        m.validate_career_checkpoint()?;
        self.validate_season_checkpoint()?;
        Ok(())
    }

    fn validate_fixture_checkpoint(&self) -> Result<(), String> {
        let clubs = &self.management.clubs;
        let mut fixture_ids = BTreeMap::new();
        let mut slots = BTreeSet::new();
        let current_ids: BTreeSet<_> = self.fixtures.iter().map(|fixture| &fixture.id).collect();
        let archived = self
            .seasons
            .iter()
            .flat_map(|state| &state.archives)
            .flat_map(|archive| &archive.fixtures);
        for fixture in self.fixtures.iter().chain(archived) {
            if fixture.id.trim().is_empty()
                || fixture_ids.insert(&fixture.id, fixture).is_some()
                || fixture.home == fixture.away
                || !clubs.contains_key(&fixture.home)
                || !clubs.contains_key(&fixture.away)
                || !slots.insert((fixture.day, &fixture.home))
                || !slots.insert((fixture.day, &fixture.away))
            {
                return Err("Invalid checkpoint fixture calendar".into());
            }
        }
        let mut expected: BTreeMap<_, _> = clubs
            .keys()
            .map(|club| {
                (
                    club.clone(),
                    Standing {
                        club_id: club.clone(),
                        played: 0,
                        won: 0,
                        drawn: 0,
                        lost: 0,
                        goals_for: 0,
                        goals_against: 0,
                        points: 0,
                    },
                )
            })
            .collect();
        let mut results = BTreeSet::new();
        for result in &self.results {
            let fixture = fixture_ids
                .get(&result.fixture_id)
                .ok_or("Checkpoint result has no fixture")?;
            if !results.insert(&result.fixture_id)
                || result.day >= self.management.window.day
                || (result.day, &result.home, &result.away)
                    != (fixture.day, &fixture.home, &fixture.away)
                || !result.report.home_possession.is_finite()
                || !(0.0..=100.0).contains(&result.report.home_possession)
                || result
                    .report
                    .player_stats
                    .keys()
                    .any(|id| !self.attributes.contains_key(id))
            {
                return Err("Invalid checkpoint result references".into());
            }
            let starters: BTreeSet<_> = result
                .home_starting_xi
                .iter()
                .chain(&result.away_starting_xi)
                .collect();
            if result.home_starting_xi.len() != 11
                || result.away_starting_xi.len() != 11
                || starters.len() != 22
                || starters.iter().any(|id| !self.attributes.contains_key(*id))
            {
                return Err("Invalid checkpoint historical starting lineups".into());
            }
            // The canonical trajectory spans seasons; the active table does not.
            if !current_ids.contains(&result.fixture_id) {
                continue;
            }
            for (club, gf, ga) in [
                (
                    &result.home,
                    result.report.home_goals,
                    result.report.away_goals,
                ),
                (
                    &result.away,
                    result.report.away_goals,
                    result.report.home_goals,
                ),
            ] {
                let standing = expected
                    .get_mut(club)
                    .ok_or("Checkpoint standing missing club")?;
                let add = |value: &mut u32, amount: u32| -> Result<(), String> {
                    *value = value
                        .checked_add(amount)
                        .ok_or("Checkpoint standings overflow")?;
                    Ok(())
                };
                add(&mut standing.played, 1)?;
                add(&mut standing.goals_for, gf.into())?;
                add(&mut standing.goals_against, ga.into())?;
                if gf > ga {
                    add(&mut standing.won, 1)?;
                    add(&mut standing.points, 3)?;
                } else if gf == ga {
                    add(&mut standing.drawn, 1)?;
                    add(&mut standing.points, 1)?;
                } else {
                    add(&mut standing.lost, 1)?;
                }
            }
        }
        if expected != self.standings
            || fixture_ids.values().any(|fixture| {
                fixture.day < self.management.window.day && !results.contains(&fixture.id)
            })
        {
            return Err("Checkpoint standings or completed-fixture coverage mismatch".into());
        }
        Ok(())
    }

    fn validate_board_checkpoint(&self) -> Result<(), String> {
        let Some(boards) = &self.boards else {
            return if self.dismissals.is_empty() {
                Ok(())
            } else {
                Err("Checkpoint dismissal without boards".into())
            };
        };
        let m = &self.management;
        for manager in m.managers.values() {
            let record = boards
                .get(&manager.id)
                .ok_or("Checkpoint active manager has no board")?;
            if record.dismissed_day.is_some() || record.club_id != manager.club_id {
                return Err("Checkpoint dismissed manager remains authorized".into());
            }
        }
        for (id, board) in boards {
            if id.trim().is_empty()
                || !m.clubs.contains_key(&board.club_id)
                || board.state.satisfaction > 100
                || board.state.warning_stage > 2
                || board.state.league_size as usize != m.clubs.len()
                || board.state.objectives
                    != crate::board::ObjectiveTargets::new(
                        board.reputation,
                        board.state.league_size,
                    )?
                || (board.dismissed_day.is_none() && !m.managers.contains_key(id))
                || board.dismissed_day.is_some_and(|day| day >= m.window.day)
            {
                return Err("Invalid checkpoint board record".into());
            }
            let mut prior_day = None;
            let mut warning_stage = 0;
            for warning in &board.warnings {
                let stage = match warning.decision {
                    crate::board::Decision::Warning => 1,
                    crate::board::Decision::FinalWarning => 2,
                    _ => return Err("Invalid checkpoint private warning type".into()),
                };
                if stage <= warning_stage
                    || prior_day.is_some_and(|day| warning.day <= day)
                    || warning.day >= m.window.day
                {
                    return Err("Invalid checkpoint warning chronology".into());
                }
                warning_stage = stage;
                prior_day = Some(warning.day);
            }
            if warning_stage != board.state.warning_stage {
                return Err("Checkpoint warning stage/history mismatch".into());
            }
        }
        let mut dismissed = BTreeSet::new();
        for dismissal in &self.dismissals {
            let former = boards
                .get(&dismissal.manager_id)
                .ok_or("Checkpoint dismissal has no former board")?;
            let replacement = boards
                .get(&dismissal.replacement_manager_id)
                .ok_or("Checkpoint dismissal has no replacement board")?;
            if !dismissed.insert(&dismissal.manager_id)
                || dismissal.manager_id == dismissal.replacement_manager_id
                || former.dismissed_day != Some(dismissal.day)
                || former.club_id != dismissal.club_id
                || replacement.club_id != dismissal.club_id
                || dismissal.standing.club_id != dismissal.club_id
                || m.managers.contains_key(&dismissal.manager_id)
            {
                return Err("Invalid checkpoint dismissal identities".into());
            }
        }
        if boards
            .iter()
            .any(|(id, board)| board.dismissed_day.is_some() && !dismissed.contains(id))
        {
            return Err("Checkpoint former manager is missing dismissal outcome".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::football::BoardProfile;
    use crate::{Club, Command, Management, Manager, Request};

    fn game() -> Football {
        let m = Management::new(
            vec![Club {
                id: "club".into(),
                name: "Club".into(),
                balance: 100,
            }],
            vec![],
            vec![Manager {
                id: "manager".into(),
                club_id: "club".into(),
            }],
            1,
            100,
        )
        .unwrap();
        let mut game = Football::new(m, vec![], vec![]).unwrap();
        game.configure_boards(BTreeMap::from([(
            "manager".into(),
            BoardProfile {
                reputation: 500,
                initial_satisfaction: 10,
            },
        )]))
        .unwrap();
        game
    }

    #[test]
    fn roundtrip_preserves_receipts_warnings_dismissal_and_future_behavior() {
        let mut original = game();
        let request = Request {
            id: "ready".into(),
            day: 1,
            command: Command::Ready,
        };
        let receipt = original.dispatch("manager", request.clone(), 1).unwrap();
        let saved = original.save_state().unwrap();
        let mut loaded = Football::load_validated(saved.clone()).unwrap();
        assert_eq!(saved, loaded.save_state().unwrap());
        assert_eq!(
            loaded.dispatch("manager", request.clone(), 1).unwrap(),
            receipt
        );
        for (day, now, deadline) in [(1, 100, 200), (2, 200, 300)] {
            original.advance_closed_day(day, now, deadline).unwrap();
            loaded.advance_closed_day(day, now, deadline).unwrap();
            assert_eq!(original.save_state().unwrap(), loaded.save_state().unwrap());
            loaded = Football::load_validated(loaded.save_state().unwrap()).unwrap();
        }
        assert_eq!(
            loaded.dispatch("manager", request, 201),
            Err(crate::Error::Unauthorized)
        );
        assert!(matches!(
            loaded.manager_view("manager"),
            Err(crate::Error::Unauthorized)
        ));
        assert_eq!(loaded.dismissals(), original.dismissals());
        let replacement = loaded.dismissals()[0].replacement_manager_id.clone();
        assert_eq!(loaded.manager_view(&replacement).unwrap().club.id, "club");
    }

    #[test]
    fn rejects_versions_duplicate_receipts_and_corrupt_identity_or_history() {
        let mut original = game();
        original
            .dispatch(
                "manager",
                Request {
                    id: "ready".into(),
                    day: 1,
                    command: Command::Ready,
                },
                1,
            )
            .unwrap();
        let saved = original.save_state().unwrap();
        let mut value = saved.clone();
        value["version"] = 2.into();
        assert!(Football::load_validated(value).is_err());
        let mut value = saved.clone();
        let receipts = value["state"]["management"]["receipts"]
            .as_array_mut()
            .unwrap();
        receipts.push(receipts[0].clone());
        let Err(error) = Football::load_validated(value) else {
            panic!("expected duplicate rejection");
        };
        assert!(error.contains("duplicate"));
        for (pointer, replacement) in [
            (
                "/state/management/managers/manager/club_id",
                serde_json::json!("missing"),
            ),
            (
                "/state/management/clubs/club/id",
                serde_json::json!("mismatch"),
            ),
            ("/state/standings/club/points", serde_json::json!(5)),
            (
                "/state/boards/manager/state/warning_stage",
                serde_json::json!(2),
            ),
        ] {
            let mut value = saved.clone();
            *value.pointer_mut(pointer).unwrap() = replacement;
            assert!(Football::load_validated(value).is_err());
        }
    }

    #[test]
    fn expired_selected_starter_roundtrips_without_repairing_short_roster() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        let attributes: Vec<engine::PlayerData> = (0..11)
            .map(|i| {
                serde_json::from_value(serde_json::json!({
            "id": format!("player-{i}"), "name": format!("Player {i}"), "position": "Midfielder",
            "condition": 80, "fitness": 70, "pace": 60, "stamina": 60, "strength": 60,
            "passing": 60, "shooting": 60, "tackling": 60, "dribbling": 60,
            "defending": 60, "positioning": 60, "vision": 60, "decisions": 60
        })).unwrap()
            })
            .collect();
        let mut m = Management::new(
            vec![Club {
                id: "club".into(),
                name: "Club".into(),
                balance: 10_000,
            }],
            attributes
                .iter()
                .map(|p| crate::Player {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    club_id: "club".into(),
                })
                .collect(),
            vec![Manager {
                id: "manager".into(),
                club_id: "club".into(),
            }],
            1,
            100,
        )
        .unwrap();
        m.require_match_rosters().unwrap();
        let mut game = Football::new(m, attributes.clone(), vec![]).unwrap();
        game.configure_career(crate::career::CareerSetup {
            today,
            contracts: attributes
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    (
                        p.id.clone(),
                        crate::contracts::PlayerContract::new(
                            chrono::NaiveDate::from_ymd_opt(2000, 1, 1).unwrap(),
                            1000,
                            Some(if i == 0 {
                                today
                            } else {
                                chrono::NaiveDate::from_ymd_opt(2027, 6, 1).unwrap()
                            }),
                            100_000,
                            70,
                            70,
                        ),
                    )
                })
                .collect(),
            wage_budgets: BTreeMap::from([("club".into(), 100_000)]),
            reputations: BTreeMap::from([("club".into(), 500)]),
            staff_annual_wages: BTreeMap::new(),
        })
        .unwrap();
        game.dispatch(
            "manager",
            Request {
                id: "lineup".into(),
                day: 1,
                command: Command::SetLineup {
                    player_ids: attributes.iter().map(|p| p.id.clone()).collect(),
                },
            },
            1,
        )
        .unwrap();
        game.advance_closed_day(1, 100, 200).unwrap();
        assert_eq!(game.lineup("manager").unwrap().len(), 10);
        let saved = game.save_state().unwrap();
        let restored = Football::load_validated(saved.clone()).unwrap();
        assert_eq!(saved, restored.save_state().unwrap());
        assert_eq!(restored.squad("manager").unwrap().len(), 10);
        assert_eq!(
            restored.free_agent_squad("manager").unwrap()[0].id,
            "player-0"
        );
        assert_eq!(restored.lineup("manager").unwrap().len(), 10);
    }
}
