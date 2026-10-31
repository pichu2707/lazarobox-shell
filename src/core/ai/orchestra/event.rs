//! What the run manager tells the app about a run (agent-orchestrator D1):
//! its own channel, because a worker can outlive a turn. Pure data, so the
//! app's state handles it without the runtime.

use std::time::Duration;

use super::{
    RunId, WorkerId,
    feature::FeatureDoc,
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

/// A continuation turn (S4, D8): turn `turn` of at most `max` of run `run`
/// starts from `text`, the run ledger wrapped as data.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Continuation {
    pub run: RunId,
    pub turn: usize,
    pub max: usize,
    pub text: String,
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
    /// The feature document of a large goal was built or corrected (S5): the
    /// app stores it under its one key, or the ledger alone keeps it.
    Feature { run: RunId, doc: Box<FeatureDoc> },
    /// The orchestrator turn ended with work pending and the run goes on: turn
    /// `turn` of at most `max` starts from `text`, the run ledger wrapped as
    /// data (S4, D8). The app shows a line and starts the turn on the run.
    Continue(Continuation),
    /// The run was stopped by lazarobox, not by the user: the turn cap
    /// (`max_turns`) or a ledger that could not be wrapped. `reason` is the
    /// line to show; `Ended` follows.
    Stopped { run: RunId, reason: String },
    /// lazarobox wrote the handoffs of these workers itself: the orchestrator
    /// never did. Sent once per batch, after their `Handoff` events.
    Filled { run: RunId, workers: Vec<WorkerId> },
    /// The orchestrator finished the run: its ledger as rendered and its
    /// summary, for the app to store (D17). Sent once, after the `Handoff`
    /// and `Filled` events of the fallbacks.
    Closed {
        run: RunId,
        ledger: String,
        summary: String,
    },
    /// Where a worker's receipt-driven review stands (S7b): one line for the
    /// transcript, made of lazarobox's words and counts only, never worker
    /// text. The receipt handoff is 7b.3.
    Review { run: RunId, text: String },
    /// The run is over: cancelled, replaced by a new run of the pane, or
    /// ended by quitting. Its last rows came before; nothing of it follows.
    Ended { run: RunId },
}
