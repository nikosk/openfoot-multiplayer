//! Explicit-fixture bridge. No desktop state, special user match, or parallel save
//! worlds. Ownership is always resolved from the authoritative management registry.
use crate::{
    Club, Error, Management, ManagerView, PublicState, Receipt, Request,
    matches::{self, DelegatedTeam},
};
use engine::{PlayerData, TeamData};
use rand::SeedableRng;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fixture {
    pub id: String,
    pub day: u32,
    pub home: String,
    pub away: String,
    pub seed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinishedFixture {
    pub fixture_id: String,
    pub day: u32,
    pub home: String,
    pub away: String,
    pub home_starting_xi: Vec<String>,
    pub away_starting_xi: Vec<String>,
    pub report: engine::MatchReport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Standing {
    pub club_id: String,
    pub played: u32,
    pub won: u32,
    pub drawn: u32,
    pub lost: u32,
    pub goals_for: u32,
    pub goals_against: u32,
    pub points: u32,
}

/// Explicit scenario inputs, not a client-editable physiology/finances backdoor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoverySetup {
    pub seed: u64,
    pub players: BTreeMap<String, crate::recovery::PlayerRecovery>,
    pub clubs: BTreeMap<String, crate::recovery::ClubRecovery>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryView {
    pub mode: crate::recovery::RecoveryMode,
    pub club: crate::recovery::ClubRecovery,
    pub players: BTreeMap<String, crate::recovery::PlayerRecovery>,
}

pub struct Football {
    management: Management,
    attributes: BTreeMap<String, PlayerData>,
    fixtures: Vec<Fixture>,
    results: Vec<FinishedFixture>,
    standings: BTreeMap<String, Standing>,
    recovery: Option<RecoverySetup>,
    started: bool,
}

impl Football {
    pub fn new(
        management: Management,
        attributes: Vec<PlayerData>,
        fixtures: Vec<Fixture>,
    ) -> Result<Self, String> {
        let attributes_len = attributes.len();
        let attributes: BTreeMap<_, _> =
            attributes.into_iter().map(|p| (p.id.clone(), p)).collect();
        if attributes_len != attributes.len()
            || attributes.len() != management.players.len()
            || attributes
                .keys()
                .any(|id| !management.players.contains_key(id))
        {
            return Err("Player attributes must match the ownership registry exactly".into());
        }
        let mut ids = BTreeSet::new();
        let mut club_days = BTreeSet::new();
        for f in &fixtures {
            if f.id.is_empty()
                || !ids.insert(&f.id)
                || f.day < management.window.day
                || f.home == f.away
                || !management.clubs.contains_key(&f.home)
                || !management.clubs.contains_key(&f.away)
                || !club_days.insert((f.day, &f.home))
                || !club_days.insert((f.day, &f.away))
            {
                return Err("Invalid or conflicting fixture".into());
            }
        }
        let standings = management
            .clubs
            .keys()
            .map(|id| {
                (
                    id.clone(),
                    Standing {
                        club_id: id.clone(),
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
        Ok(Self {
            management,
            attributes,
            fixtures,
            results: vec![],
            standings,
            recovery: None,
            started: false,
        })
    }

    pub fn dispatch(
        &mut self,
        actor: &str,
        request: Request,
        now_ms: u64,
    ) -> Result<Receipt, Error> {
        self.management.dispatch(actor, request, now_ms)
    }

    pub fn configure_recovery(&mut self, setup: RecoverySetup) -> Result<(), String> {
        if self.started || self.recovery.is_some() || self.management.sequence != 0 {
            return Err(
                "Recovery can only be configured once before commands or day processing".into(),
            );
        }
        if setup.players.len() != self.attributes.len()
            || setup.clubs.len() != self.management.clubs.len()
            || setup
                .players
                .keys()
                .any(|id| !self.attributes.contains_key(id))
            || setup
                .clubs
                .keys()
                .any(|id| !self.management.clubs.contains_key(id))
        {
            return Err("Recovery profiles must match world identities exactly".into());
        }
        for (id, attributes) in &self.attributes {
            let club = &self.management.players[id].club_id;
            let mut clone = attributes.clone();
            let mut rng = rand::rngs::StdRng::seed_from_u64(setup.seed);
            crate::recovery::recover(
                &mut clone,
                &setup.players[id],
                &setup.clubs[club],
                crate::recovery::RecoveryMode::Rest,
                &mut rng,
            )?;
        }
        for club in setup.clubs.values() {
            crate::recovery::validate_club(club)?;
        }
        self.recovery = Some(setup);
        self.management.recovery_enabled = true;
        Ok(())
    }

    pub fn recovery_view(&self, actor: &str) -> Result<RecoveryView, Error> {
        let club = self.management.manager_view(actor)?.club;
        let setup = self.recovery.as_ref().ok_or(Error::Unavailable)?;
        Ok(RecoveryView {
            mode: self
                .management
                .recovery_modes
                .get(&club.id)
                .copied()
                .unwrap_or(crate::recovery::RecoveryMode::Rest),
            club: setup.clubs[&club.id].clone(),
            players: self
                .management
                .players
                .values()
                .filter(|p| p.club_id == club.id)
                .map(|p| (p.id.clone(), setup.players[&p.id].clone()))
                .collect(),
        })
    }

    pub fn manager_view(&self, actor: &str) -> Result<ManagerView, Error> {
        self.management.manager_view(actor)
    }

    pub fn lineup(&self, actor: &str) -> Result<Vec<String>, Error> {
        self.management.lineup(actor)
    }

    pub fn squad(&self, actor: &str) -> Result<Vec<PlayerData>, Error> {
        let club = self.management.manager_view(actor)?.club;
        Ok(self
            .management
            .players
            .values()
            .filter(|p| p.club_id == club.id)
            .map(|p| {
                let mut data = self.attributes[&p.id].clone();
                data.name = p.name.clone();
                data
            })
            .collect())
    }

    pub fn public_state(&self) -> PublicState {
        self.management.public_state()
    }

    /// Host metadata; clients should expose only day/deadline, not the ready set.
    pub fn window(&self) -> crate::window::DayWindow {
        self.management.window.clone()
    }

    /// Saved pre-match instructions, private to the authenticated club manager.
    /// Automatic in-match adjustments never write back to this plan.
    pub fn match_plan(&self, actor: &str) -> Result<crate::tactics::MatchPlan, Error> {
        let club = self.management.manager_view(actor)?.club;
        Ok(self
            .management
            .match_plans
            .get(&club.id)
            .cloned()
            .unwrap_or_default())
    }
    pub fn results(&self) -> &[FinishedFixture] {
        &self.results
    }
    pub fn fixtures(&self) -> &[Fixture] {
        &self.fixtures
    }
    pub fn standings(&self) -> Vec<Standing> {
        let mut rows: Vec<_> = self.standings.values().cloned().collect();
        rows.sort_by(|a, b| {
            b.points
                .cmp(&a.points)
                .then_with(|| {
                    (i64::from(b.goals_for) - i64::from(b.goals_against))
                        .cmp(&(i64::from(a.goals_for) - i64::from(a.goals_against)))
                })
                .then_with(|| b.goals_for.cmp(&a.goals_for))
                .then_with(|| a.club_id.cmp(&b.club_id))
        });
        rows
    }

    fn team(&self, club: &Club) -> Result<DelegatedTeam, String> {
        let ids = self
            .management
            .lineups
            .get(&club.id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let data = |id: &String| {
            let mut player = self.attributes[id].clone();
            player.name = self.management.players[id].name.clone();
            player
        };
        let available: Vec<_> = self
            .management
            .players
            .values()
            .filter(|p| p.club_id == club.id)
            .map(|p| data(&p.id))
            .collect();
        let (players, bench) = crate::selection::select(&available, ids)?;
        let plan = self
            .management
            .match_plans
            .get(&club.id)
            .cloned()
            .unwrap_or_default();
        Ok(DelegatedTeam {
            team: TeamData {
                id: club.id.clone(),
                name: club.name.clone(),
                formation: "4-4-2".into(),
                play_style: plan.play_style,
                tactics: plan.engine_tactics(),
                players,
            },
            bench,
            profile: engine::ai::AiProfile::default(),
        })
    }

    /// Trusted host operation. Stages all results before publishing any of them.
    /// An old expected day cannot duplicate a completed matchday.
    pub fn advance_closed_day(
        &mut self,
        expected_day: u32,
        now_ms: u64,
        next_deadline_ms: u64,
    ) -> Result<Vec<FinishedFixture>, String> {
        if expected_day != self.management.window.day {
            return Err("Wrong day".into());
        }
        if next_deadline_ms <= now_ms || expected_day == u32::MAX {
            return Err("Invalid next window".into());
        }
        if !self.management.closed(now_ms) {
            return Err("Management window is still open".into());
        }
        self.started = true;
        let mut staged = vec![];
        let mut staged_attributes = self.attributes.clone();
        for fixture in self.fixtures.iter().filter(|f| f.day == expected_day) {
            let home = self.team(&self.management.clubs[&fixture.home])?;
            let away = self.team(&self.management.clubs[&fixture.away])?;
            let home_starting_xi = home.team.players.iter().map(|p| p.id.clone()).collect();
            let away_starting_xi = away.team.players.iter().map(|p| p.id.clone()).collect();
            let report = matches::play(home, away, fixture.seed)?;
            // Stable order and a separate named-purpose stream; match event RNG
            // consumption cannot accidentally decide post-match physical wear.
            let mut rng = rand::rngs::StdRng::seed_from_u64(fixture.seed ^ 0x7068_7973_6963_616c);
            let mut ids: Vec<_> = report.player_stats.keys().collect();
            ids.sort();
            for id in ids {
                let player = staged_attributes
                    .get_mut(id)
                    .ok_or("Unknown report player")?;
                crate::physical::apply_match_wear(
                    player,
                    report.player_stats[id].minutes_played,
                    &mut rng,
                );
            }
            staged.push(FinishedFixture {
                fixture_id: fixture.id.clone(),
                day: expected_day,
                home: fixture.home.clone(),
                away: fixture.away.clone(),
                report,
                home_starting_xi,
                away_starting_xi,
            });
        }
        if staged.is_empty() {
            if let Some(setup) = &self.recovery {
                let mut rng = rand::rngs::StdRng::seed_from_u64(
                    setup.seed ^ u64::from(expected_day) ^ 0x7265_636f_7665_7279,
                );
                for (id, player) in &mut staged_attributes {
                    let club = &self.management.players[id].club_id;
                    let mode = self
                        .management
                        .recovery_modes
                        .get(club)
                        .copied()
                        .unwrap_or(crate::recovery::RecoveryMode::Rest);
                    crate::recovery::recover(
                        player,
                        &setup.players[id],
                        &setup.clubs[club],
                        mode,
                        &mut rng,
                    )?;
                }
            }
        }
        self.management
            .next_day(expected_day, now_ms, next_deadline_ms)
            .map_err(|e| format!("{e:?}"))?;
        self.attributes = staged_attributes;
        for result in &staged {
            for (id, gf, ga) in [
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
                let row = self.standings.get_mut(id).unwrap();
                row.played += 1;
                row.goals_for += u32::from(gf);
                row.goals_against += u32::from(ga);
                if gf > ga {
                    row.won += 1;
                    row.points += 3;
                } else if gf == ga {
                    row.drawn += 1;
                    row.points += 1;
                } else {
                    row.lost += 1;
                }
            }
        }
        self.results.extend(staged.iter().cloned());
        Ok(staged)
    }
}
