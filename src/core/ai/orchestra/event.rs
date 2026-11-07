//! What the run manager tells the app about a run (agent-orchestrator D1):
//! its own channel, because a worker can outlive a turn. Pure data, so the
//! app's state handles it without the runtime.

use std::time::Duration;

use super::{
    RunId, WorkerId,
    changes::Disclosure,
    feature::FeatureDoc,
    ledger::Handoff,
    steps::{Gate, Step},
    worker::{ErrorClass, WorkerStatus},
};
use crate::core::ai::{
    chat::{
        engine::{CallId, ToolStatus, Usage},
        usage::TurnUsage,
    },
    config::Task,
};

/// Why a worker turn ran without tools; one reason per way out of the
/// session setup.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Degrade {
    /// The runtime has no tool host.
    NoTooling,
    /// The tools were not on for the engine (it is not verified for them).
    ToolsOff,
    /// The engine's locality is unknown.
    NoLocality,
    /// The hub of the worker's tool sockets could not open.
    NoHub,
}

/// What the pane is told, once per run and engine, when two workers may queue
/// on one local server (owner decision 5, design D11). `first` and `second`
/// are the two worker numbers, lowest first. It names only a key that exists.
pub fn shared_engine_notice(first: WorkerId, second: WorkerId, engine: &str) -> String {
    let text = format!(
        "workers {} and {} share {engine}, a local server: it may queue their requests; set max_parallel = 1 in [ai.orchestrator] to run one at a time",
        first.0, second.0
    );
    // Like the other worker notices: ASCII, whatever the engine name holds.
    text.chars()
        .map(|c| if c.is_ascii() { c } else { '?' })
        .collect()
}

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

/// One step of a worker's turn as its transcript keeps it (S9). Raw: the
/// transcript sanitizes whatever it keeps.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TraceItem {
    /// An attempt of the worker starts: what an earlier attempt left goes.
    Begin,
    /// A piece of the worker's answer.
    Text(String),
    /// A line about how the worker's turn runs, shown as a system row: its
    /// prompt passes the model's window (#557).
    Notice(String),
    /// A tool call of the worker.
    Call {
        id: CallId,
        name: String,
        summary: String,
    },
    /// The end of the call `id`.
    Result {
        id: CallId,
        status: ToolStatus,
        summary: String,
    },
}

/// What the run manager tells the app.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum OrchestraEvent {
    /// What a worker's turn produced, for its transcript view (S9). Content,
    /// so never logged.
    Trace {
        run: RunId,
        worker: WorkerId,
        item: TraceItem,
    },
    /// A worker's status changed.
    Worker(WorkerRow),
    /// What one worker turn used, summed over its requests: one record for
    /// the usage store (`partial` when the turn did not end well). Counts and
    /// identifiers only.
    Usage {
        run: RunId,
        worker: WorkerId,
        engine: String,
        usage: Box<TurnUsage>,
    },
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
    /// transcript, in lazarobox's words and counts, plus (for an open
    /// blocker) the first blocker's text, a lens worker's own words clipped
    /// to 120 characters on one line. The receipt handoff is 7b.3.
    Review { run: RunId, text: String },
    /// A line for the pane's transcript that belongs to no run yet (a floor
    /// of run ids that could not be read, S9). `text` is lazarobox's own.
    Notice { text: String },
    /// A fact about how a worker ended that nothing else shows (agent-
    /// orchestrator [W1]): the app turns it into a transcript line naming the
    /// worker, its kind and `engine>model`. Never sent to a model.
    Disclosed {
        run: RunId,
        worker: WorkerId,
        /// Workers the run has made so far: the `m` of `worker n/m`.
        workers: usize,
        kind: Task,
        /// Empty when the engine was unavailable.
        engine: String,
        model: Option<String>,
        what: Disclosure,
    },
    /// A spawn that made no worker, or made a worker refused for its engine
    /// or model (that path does create a slot): `reason` is the cause the
    /// coordinator got, redacted and capped, without the worker's status
    /// prefix (agent-orchestrator [W1]). Never sent to a model.
    SpawnRefused {
        run: RunId,
        kind: Task,
        reason: String,
    },
    /// A step gate refused a spawn or the `finish` (agent-orchestrator [W3]):
    /// `reason` is the text the coordinator got, redacted and capped, and it
    /// names the gate, the step and the user's override. Never sent to a
    /// model. The run goes on: nothing pauses and no failure alert opens.
    GateRefused {
        run: RunId,
        gate: Gate,
        step: Step,
        reason: String,
    },
    /// A worker turn that runs as text because its tools could not be
    /// offered (agent-orchestrator [W1]); the turn goes on as before.
    Degraded {
        run: RunId,
        worker: WorkerId,
        /// Workers the run has made so far: the `m` of `worker n/m`.
        workers: usize,
        kind: Task,
        engine: String,
        model: Option<String>,
        why: Degrade,
    },
    /// An orchestrator turn ended with a text answer and the run made no
    /// worker (agent-orchestrator [W1]). Sent once per run; the app shows a
    /// notice when its pane is idle. Never sent to a model.
    Undelegated { run: RunId },
    /// The run is over: cancelled, replaced by a new run of the pane, or
    /// ended by quitting. Its last rows came before; nothing of it follows.
    Ended { run: RunId },
}
