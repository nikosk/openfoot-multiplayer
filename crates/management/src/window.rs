//! Readiness and wall-clock closure for one management day.
//!
//! Participant authorization and advancing the simulation belong to the caller.
//! This module only determines whether the current window has closed.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayWindow {
    pub day: u32,
    pub deadline_ms: u64,
    ready: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowClosure {
    AllReady,
    Deadline,
}

impl DayWindow {
    pub fn new(day: u32, deadline_ms: u64) -> Self {
        Self {
            day,
            deadline_ms,
            ready: BTreeSet::new(),
        }
    }

    /// Mark an authorized participant ready. Readiness cannot be withdrawn.
    /// Returns whether this is the participant's first readiness declaration.
    pub fn mark_ready(&mut self, manager: impl Into<String>) -> bool {
        self.ready.insert(manager.into())
    }

    pub fn is_ready(&self, manager: &str) -> bool {
        self.ready.contains(manager)
    }

    /// Determine closure against the currently active participants.
    ///
    /// The deadline is inclusive and takes precedence when both conditions hold.
    /// Removed participants do not block closure, and an empty participant set
    /// does not count as all-ready. No game time or readiness is modified.
    pub fn closure<I, S>(&self, now_ms: u64, active_managers: I) -> Option<WindowClosure>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        if now_ms >= self.deadline_ms {
            return Some(WindowClosure::Deadline);
        }

        let mut any_active = false;
        for manager in active_managers {
            any_active = true;
            if !self.is_ready(manager.as_ref()) {
                return None;
            }
        }

        any_active.then_some(WindowClosure::AllReady)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadline_is_inclusive_even_with_unresponsive_managers() {
        let window = DayWindow::new(7, 1_000);
        assert_eq!(window.closure(999, ["manager-a", "manager-b"]), None);
        assert_eq!(
            window.closure(1_000, ["manager-a", "manager-b"]),
            Some(WindowClosure::Deadline)
        );
        assert_eq!(
            window.closure(1_001, ["manager-a", "manager-b"]),
            Some(WindowClosure::Deadline)
        );
        assert_eq!(window.day, 7);
    }

    #[test]
    fn all_active_managers_must_be_ready() {
        let mut window = DayWindow::new(7, 1_000);
        assert!(!window.is_ready("manager-a"));
        assert!(window.mark_ready("manager-a"));
        assert!(!window.mark_ready("manager-a"));
        assert!(window.is_ready("manager-a"));
        assert_eq!(window.closure(500, ["manager-a", "manager-b"]), None);
        window.mark_ready("manager-b");
        assert_eq!(
            window.closure(500, ["manager-a", "manager-b"]),
            Some(WindowClosure::AllReady)
        );
        assert_eq!(
            window.closure(1_000, ["manager-a", "manager-b"]),
            Some(WindowClosure::Deadline)
        );
    }

    #[test]
    fn removed_or_fired_managers_do_not_block_remaining_managers() {
        let mut window = DayWindow::new(7, 1_000);
        window.mark_ready("manager-a");
        assert_eq!(window.closure(500, ["manager-a", "manager-b"]), None);
        assert_eq!(
            window.closure(500, ["manager-a"]),
            Some(WindowClosure::AllReady)
        );
        // A departed participant's old readiness does not ready a new one.
        assert_eq!(window.closure(500, ["replacement"]), None);
    }

    #[test]
    fn empty_active_set_waits_for_deadline() {
        let mut window = DayWindow::new(7, 1_000);
        window.mark_ready("departed");
        assert_eq!(window.closure(999, std::iter::empty::<&str>()), None);
        assert_eq!(
            window.closure(1_000, std::iter::empty::<&str>()),
            Some(WindowClosure::Deadline)
        );
    }

    #[test]
    fn new_day_does_not_inherit_readiness() {
        let mut previous = DayWindow::new(7, 1_000);
        previous.mark_ready("manager-a");
        let next = DayWindow::new(8, 2_000);
        assert!(!next.is_ready("manager-a"));
        assert_eq!(next.closure(1_001, ["manager-a"]), None);
    }
}
