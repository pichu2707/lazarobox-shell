//! The clarification rounds of a brief (W8c, D4): how many a brief may have
//! and why a spawn that names (or fails to name) one is refused. Pure.

use super::WorkerId;

/// The most clarification rounds one brief may have (spec "Clarification
/// rounds"): the third `needs_specification` stops the run.
pub const ROUNDS: u32 = 2;

/// Why a `simple` spawn was refused for its `answers`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoundError {
    /// The clarification of this worker is open and the spawn did not answer it.
    Open(WorkerId),
    /// `answers` names no worker.
    Unknown(u32),
    /// `answers` names a worker with no open clarification.
    NotAsked(u32),
    /// The brief (named by its first worker) used its rounds.
    Over(WorkerId),
}

impl RoundError {
    pub fn text(&self) -> String {
        match self {
            Self::Open(worker) => format!(
                "worker {n} asked for a clearer brief: answer it first with answers: {n}",
                n = worker.0
            ),
            Self::Unknown(n) => format!("answers refused: there is no worker {n}"),
            Self::NotAsked(n) => {
                format!("answers refused: worker {n} has no open clarification")
            }
            Self::Over(brief) => format!(
                "answers refused: brief {} used its two rounds of clarification",
                brief.0
            ),
        }
    }
}
