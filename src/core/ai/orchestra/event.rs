//! What the run manager tells the app about a run (agent-orchestrator D1):
//! its own channel, because a worker can outlive a turn. Pure data, so the
//! app's state handles it without the runtime.

use std::time::Duration;

use super::{
    RunId, WorkerId,
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
    /// The run is over: cancelled, replaced by a new run of the pane, or
    /// ended by quitting. Its last rows came before; nothing of it follows.
    Ended { run: RunId },
}
