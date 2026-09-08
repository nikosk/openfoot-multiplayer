//! Staff actions from pinned 64677fee commands/staff.rs, using the unchanged
//! domain records. Hiring changes season expenses, NOT cash; release reverses
//! that expense using signed saturating subtraction. Neither action negotiates
//! terms, charges severance, checks wage budgets, or expires staff contracts.
//! Copyright (C) 2020–2026 Pedrenrique G. Guimarães and contributors.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! The host authorizes club ownership, stamps preview day/actor, updates market
//! activity on hire and runs the source market refresh (empty pool or 30-day
//! inactivity). This helper never invents replacement staff or cancels a scout's
//! existing assignments: the source release command does neither.
pub use domain::staff::{CoachingSpecialization, Staff, StaffAttributes, StaffRole};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Hire,
    Release,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Accounts {
    pub balance: i64,
    pub season_expenses: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preview {
    pub staff_id: String,
    pub club_id: String,
    pub action: Action,
    pub balance_after: i64,
    pub season_expenses_after: i64,
    /// Source Staff has no equality implementation. Preserve its complete,
    /// lossless record as a JSON value for preview dependency comparison.
    pub expected_staff: serde_json::Value,
    pub expected_accounts: Accounts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Confirmation {
    Applied,
    RefreshRequired(Preview),
}

pub fn validate(staff: &Staff) -> Result<(), String> {
    if staff.id.trim().is_empty()
        || staff.team_id.as_deref().is_some_and(str::is_empty)
        || [
            staff.attributes.coaching,
            staff.attributes.judging_ability,
            staff.attributes.judging_potential,
            staff.attributes.physiotherapy,
        ]
        .into_iter()
        .any(|value| value > 100)
    {
        return Err("Invalid staff identity or attributes".into());
    }
    Ok(())
}

pub fn review(
    staff: &Staff,
    club_id: &str,
    accounts: &Accounts,
    action: Action,
) -> Result<Preview, String> {
    validate(staff)?;
    if club_id.is_empty() {
        return Err("be.error.noTeamAssigned".into());
    }
    let expenses = match action {
        Action::Hire => {
            if staff.team_id.is_some() {
                return Err("be.error.staffMemberAlreadyEmployed".into());
            }
            accounts
                .season_expenses
                .checked_add(i64::from(staff.wage))
                .ok_or("Staff expense overflow")?
        }
        Action::Release => {
            if staff.team_id.as_deref() != Some(club_id) {
                return Err("be.error.staffMemberNotInTeam".into());
            }
            accounts
                .season_expenses
                .saturating_sub(i64::from(staff.wage))
        }
    };
    Ok(Preview {
        staff_id: staff.id.clone(),
        club_id: club_id.into(),
        action,
        balance_after: accounts.balance,
        season_expenses_after: expenses,
        expected_staff: serde_json::to_value(staff).map_err(|error| error.to_string())?,
        expected_accounts: accounts.clone(),
    })
}

/// Recheck exclusive employment and every staff/account field before commitment.
/// A changed but still valid offer returns a fresh review without mutation.
pub fn confirm(
    staff: &mut Staff,
    club_id: &str,
    accounts: &mut Accounts,
    preview: &Preview,
) -> Result<Confirmation, String> {
    if preview.staff_id != staff.id || preview.club_id != club_id {
        return Err("Staff preview does not belong to this club/member".into());
    }
    let current = review(staff, club_id, accounts, preview.action)?;
    if &current != preview {
        return Ok(Confirmation::RefreshRequired(current));
    }
    accounts.season_expenses = current.season_expenses_after;
    staff.team_id = match preview.action {
        Action::Hire => Some(club_id.into()),
        Action::Release => None,
    };
    Ok(Confirmation::Applied)
}

/// All four role attributes remain available to their source consumers. Training
/// uses only coaches/assistants' coaching and physios' physiotherapy ratings.
pub fn training_staff(staff: &[Staff], club_id: &str) -> (Vec<crate::training::Coach>, Vec<u8>) {
    use crate::training::Specialization as T;
    let mut coaches = vec![];
    let mut physiotherapy = vec![];
    for person in staff
        .iter()
        .filter(|person| person.team_id.as_deref() == Some(club_id))
    {
        match person.role {
            StaffRole::Coach | StaffRole::AssistantManager => {
                coaches.push(crate::training::Coach {
                    coaching: person.attributes.coaching,
                    specialization: person.specialization.as_ref().map(|specialization| {
                        match specialization {
                            CoachingSpecialization::Fitness => T::Fitness,
                            CoachingSpecialization::Technique => T::Technique,
                            CoachingSpecialization::Tactics => T::Tactics,
                            CoachingSpecialization::Defending => T::Defending,
                            CoachingSpecialization::Attacking => T::Attacking,
                            CoachingSpecialization::GoalKeeping => T::GoalKeeping,
                            CoachingSpecialization::Youth => T::Youth,
                        }
                    }),
                })
            }
            StaffRole::Physio => physiotherapy.push(person.attributes.physiotherapy),
            StaffRole::Scout => {}
        }
    }
    (coaches, physiotherapy)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn person() -> Staff {
        let mut staff = Staff::new(
            "staff".into(),
            "Alex".into(),
            "Coach".into(),
            "1985-01-01".into(),
            StaffRole::Coach,
            StaffAttributes {
                coaching: 70,
                judging_ability: 50,
                judging_potential: 60,
                physiotherapy: 40,
            },
        );
        staff.wage = 12_000;
        staff
    }

    #[test]
    fn hiring_changes_expenses_not_cash_even_when_in_debt() {
        let mut staff = person();
        let mut accounts = Accounts {
            balance: -500,
            season_expenses: 100,
        };
        let preview = review(&staff, "club", &accounts, Action::Hire).unwrap();
        assert!(staff.team_id.is_none());
        assert_eq!(accounts.season_expenses, 100);
        assert_eq!(
            confirm(&mut staff, "club", &mut accounts, &preview).unwrap(),
            Confirmation::Applied
        );
        assert_eq!(staff.team_id.as_deref(), Some("club"));
        assert_eq!(accounts.balance, -500);
        assert_eq!(accounts.season_expenses, 12_100);
        assert!(confirm(&mut staff, "club", &mut accounts, &preview).is_err());
    }

    #[test]
    fn release_has_no_severance_and_signed_expenses_can_be_negative() {
        let mut staff = person();
        staff.team_id = Some("club".into());
        let mut accounts = Accounts {
            balance: 1000,
            season_expenses: 0,
        };
        assert!(review(&staff, "other", &accounts, Action::Release).is_err());
        let preview = review(&staff, "club", &accounts, Action::Release).unwrap();
        confirm(&mut staff, "club", &mut accounts, &preview).unwrap();
        assert_eq!(accounts.season_expenses, -12_000);
        assert_eq!(accounts.balance, 1000);
        assert!(staff.team_id.is_none());
        staff.team_id = Some("club".into());
        accounts.season_expenses = i64::MIN;
        assert_eq!(
            review(&staff, "club", &accounts, Action::Release)
                .unwrap()
                .season_expenses_after,
            i64::MIN
        );
    }

    #[test]
    fn preview_detects_wage_account_changes_and_competing_hires() {
        let mut staff = person();
        let mut accounts = Accounts {
            balance: 1000,
            season_expenses: 0,
        };
        let preview = review(&staff, "a", &accounts, Action::Hire).unwrap();
        staff.wage = 13_000;
        let Confirmation::RefreshRequired(updated) =
            confirm(&mut staff, "a", &mut accounts, &preview).unwrap()
        else {
            panic!("Expected refresh")
        };
        assert!(staff.team_id.is_none());
        assert_eq!(accounts.season_expenses, 0);
        let other = review(&staff, "b", &accounts, Action::Hire).unwrap();
        confirm(&mut staff, "b", &mut accounts, &other).unwrap();
        assert!(confirm(&mut staff, "a", &mut accounts, &updated).is_err());
        let mut free = person();
        let max = Accounts {
            balance: 0,
            season_expenses: i64::MAX,
        };
        assert!(review(&free, "a", &max, Action::Hire).is_err());
        assert!(free.team_id.is_none());
        free.attributes.coaching = 101;
        assert!(validate(&free).is_err());
    }

    #[test]
    fn roles_and_attributes_roundtrip_and_training_projection_is_scoped() {
        let mut coach = person();
        coach.team_id = Some("a".into());
        coach.specialization = Some(CoachingSpecialization::Technique);
        let mut physio = person();
        physio.id = "physio".into();
        physio.role = StaffRole::Physio;
        physio.team_id = Some("a".into());
        let mut other = person();
        other.team_id = Some("b".into());
        let (coaches, ratings) = training_staff(&[coach.clone(), physio, other], "a");
        assert_eq!(coaches.len(), 1);
        assert_eq!(ratings, vec![40]);
        assert_eq!(
            coaches[0].specialization,
            Some(crate::training::Specialization::Technique)
        );
        let json = serde_json::to_value(&coach).unwrap();
        assert_eq!(json["attributes"]["judgingAbility"], 50);
        let restored: Staff = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(restored).unwrap(), json);
    }
}
