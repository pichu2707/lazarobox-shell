//! What the run manager tells the app about a run (agent-orchestrator D1):
//! its own channel, because a worker can outlive a turn. Pure data, so the
//! app's state handles it without the runtime.

use std::time::Duration;

use super::{
    RunId, WorkerId,
    ledger::Handoff,
    worker::{ErrorClass, WorkerStatus},
};
use crate::core::ai::{chat::engine::Usage, config::Task};

/// A worker as its row shows it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WorkerRow {
    pub run: RunId,
    pub id: WorkerId,
    pub kind: Task,
    /// Empty when the engine was unavailable.
    pub engine: String,
    pub model: Option<String>,
    pub status: WorkerStatus,
    /// How long it ran, once it ended.
    pub elapsed: Option<Duration>,
    /// What the worker's engine reported using; `None` when it reported
    /// nothing (shown as `n/a` once the worker is done).
    pub usage: Option<Usage>,
    /// Some attempt of the worker ended without reporting: `usage` (cost
    /// omitted) covers only the attempts that did.
    pub partial: bool,
    /// The worker runs on a run override (the app sets it; the run manager
    /// never does).
    pub overridden: bool,
}

/// What the run manager tells the app.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum OrchestraEvent {
    /// A worker's status changed.
    Worker(WorkerRow),
    /// A worker failed: the run waits for the user. `text` is for the alert
    /// only, sanitized and capped by the pipe.
    Halted {
        run: RunId,
        worker: WorkerId,
        class: ErrorClass,
        text: String,
    },
    /// A decision could not be applied (a retry that could not wrap the
    /// error): the run is halted as before and the alert comes back with
    /// `reason`.
    Declined {
        run: RunId,
        worker: WorkerId,
        class: ErrorClass,
        text: String,
        reason: String,
    },
    /// The user's answer for `run` was not applied: that run had ended or
    /// been replaced, or no failure waited any more.
    Dropped { run: RunId },
    /// The user's answer was not applied: this app has no run manager.
    Unavailable,
    /// The user's `y` for a worker's ask reached nothing: the worker ended
    /// first. The answer was not applied.
    AnswerLost,
    /// The run reached its wall clock (`run_timeout_mins`) and was stopped:
    /// its running workers were killed. `Ended` follows.
    Expired { run: RunId, after: Duration },
    /// The orchestrator (or lazarobox, for a missing one) wrote a handoff.
    /// The ledger keeps it in memory; persisting it is the app's seam (4.2).
    Handoff { run: RunId, handoff: Box<Handoff> },
    /// The run is over: cancelled, replaced by a new run of the pane, or
    /// ended by quitting. Its last rows came before; nothing of it follows.
    Ended { run: RunId },
}
