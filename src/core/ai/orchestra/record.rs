//! A run as the store keeps it (S9): its goal, its phase and a row per worker,
//! enough to show an interrupted run after a restart and, later, to resume it.
//! Pure: the table lives in `runtime::chat_store`. Everything in a record that
//! came from a model or the user is data, sanitized when it is shown.

use super::worker::WorkerStatus;
use crate::core::ai::sanitize::clean;

/// The most bytes of a goal a record keeps.
pub const GOAL_MAX: usize = 2048;
/// The most characters of a goal an interruption line shows.
const GOAL_SHOWN: usize = 60;
/// The most interrupted runs one restart names; the rest are counted.
pub const SHOWN_MAX: usize = 3;

/// Where a stored run stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunPhase {
    /// A process owns it and its workers may be running.
    Live,
    /// It reached its end (finished, cancelled, expired or replaced).
    Ended,
    /// The process that owned it is gone; nothing runs.
    Interrupted,
}

impl RunPhase {
    pub fn id(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Ended => "ended",
            Self::Interrupted => "interrupted",
        }
    }

    /// The phase a stored id names; a value a newer build wrote is `None`.
    pub fn from_id(id: &str) -> Option<Self> {
        [Self::Live, Self::Ended, Self::Interrupted]
            .into_iter()
            .find(|phase| phase.id() == id)
    }
}

/// One worker of a stored run, as its row last showed it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WorkerRecord {
    pub id: u32,
    pub kind: String,
    /// Empty when the engine was unavailable.
    pub engine: String,
    pub model: Option<String>,
    /// The `WorkerStatus` label.
    pub status: String,
}

impl WorkerRecord {
    /// It had not finished when the record was last saved.
    pub fn unfinished(&self) -> bool {
        self.status == WorkerStatus::Queued.label() || self.status == WorkerStatus::Running.label()
    }
}

/// A run to store, or one read back.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RunRecord {
    pub id: u64,
    pub goal: String,
    pub phase: RunPhase,
    pub workers: Vec<WorkerRecord>,
}

impl RunRecord {
    /// The goal as stored: sanitized and cut to [`GOAL_MAX`] bytes on a
    /// character boundary.
    pub fn stored_goal(goal: &str) -> String {
        let mut text = clean(goal);
        if text.len() > GOAL_MAX {
            let mut end = GOAL_MAX;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
        }
        text
    }

    /// The transcript line for a run found interrupted: lazarobox's words
    /// around the goal, which is data (first line, sanitized, clipped).
    pub fn interrupted_line(&self) -> String {
        let goal = clean(&self.goal);
        let goal = goal.lines().map(str::trim).find(|l| !l.is_empty());
        let goal: String = goal
            .unwrap_or("(no goal)")
            .chars()
            .take(GOAL_SHOWN)
            .collect();
        let left = self.workers.iter().filter(|w| w.unfinished()).count();
        let noun = if left == 1 { "worker" } else { "workers" };
        format!(
            "run {} was interrupted by a restart: {left} {noun} unfinished, nothing runs \u{b7} {goal}",
            self.id
        )
    }
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
