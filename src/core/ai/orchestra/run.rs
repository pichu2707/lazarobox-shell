//! The run: its workers, limits and the failure pause. Pure; time comes in as
//! explicit `Instant`s and nothing here starts a process.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::changes::{Change, ChangeLedger};
use super::feature::{FeatureDoc, FeatureError};
use super::ledger::{Facts, FinishError, Handoff, HandoffError, Judgement, Ledger, Row, Unit};
use super::receipt::{self, Evidence};
use super::steps::{Grant, Mark, READ_ONLY, Step, StepError, Steps};
use super::worker::{ErrorClass, WorkerResult, WorkerSpec, WorkerStatus};
use super::{RunId, Toolset, WorkerId};
use crate::core::ai::chat::engine::{Availability, Usage};
use crate::core::ai::chat::guidance::guidance;
use crate::core::ai::config::Task;
use crate::core::ai::tools::label;

/// What bounds a run: the workers that run at once, the workers it may spawn
/// in all and its wall clock (`max_parallel`, `max_workers`,
/// `run_timeout_mins`) and the orchestrator turns it may take, the first
/// included (`max_turns`, S4). A run without limits set runs one worker at a
/// time, with no cap, no wall clock and no turn cap.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RunLimits {
    pub parallel: usize,
    pub workers: usize,
    pub wall: Duration,
    pub turns: usize,
}

impl Default for RunLimits {
    fn default() -> Self {
        Self {
            parallel: 1,
            workers: usize::MAX,
            wall: Duration::MAX,
            turns: usize::MAX,
        }
    }
}

/// The availability of every task, as the app resolved it from the menu (and
/// the run's override) for this request.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Roster(Vec<(Task, Availability)>);

impl Roster {
    pub fn new(entries: Vec<(Task, Availability)>) -> Self {
        Self(entries)
    }

    /// The availability of `task`; unassigned when the roster lacks it.
    pub fn get(&self, task: Task) -> &Availability {
        const NONE: &Availability = &Availability::Unassigned;
        let found = self.0.iter().find(|(known, _)| *known == task);
        found.map_or(NONE, |(_, availability)| availability)
    }
}

/// A step that closed: its handoff and the `code` workers the grant ended.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StepDone {
    pub handoff: Handoff,
    /// Queued `code` workers refused because the goal became read-only.
    pub refused: Vec<WorkerId>,
}

/// What follows an orchestrator turn that ended (S4, D8).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NextTurn {
    /// Nothing is pending (or the run waits for the user, or is over): the
    /// run settles as before.
    Settle,
    /// Turn `n` (counting the first) continues the run from its ledger.
    Continue(usize),
    /// Work is pending but `max_turns` turns were taken: the run ends.
    Capped(usize),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RunState {
    Open,
    /// A worker failed: nothing new starts until the user decides.
    Halted {
        worker: WorkerId,
        class: ErrorClass,
        text: String,
    },
    Finished,
    Cancelled,
}

/// The user's answer to a failure.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum UserChoice {
    Retry,
    /// The roster already carries the run override.
    Relaunch(Roster),
    Cancel,
}

/// Why a spawn did not create a worker.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refusal {
    /// `run paused`: a failure is unresolved.
    Paused,
    Ended,
    /// The run already spawned as many workers as `max_workers` allows.
    WorkerCap(usize),
    /// `authorize` made the goal read-only: no `code` worker (S5, D15).
    ReadOnly,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CollectError {
    Unknown,
    NotFinished,
    /// The worker ended without a result to hand over.
    Ended(WorkerStatus),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CancelOutcome {
    Cancelled,
    AlreadyEnded,
    Unknown,
}

/// What the runtime saw a running worker do.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum WorkerEvent {
    /// What one turn of the worker used. Any number may come before the end;
    /// they add up.
    Usage(Usage),
    /// The last report covers only part of its turn (the engine marked it
    /// partial): the worker's usage is a floor, not a whole.
    UsageGap,
    Done {
        text: String,
    },
    Failed {
        class: ErrorClass,
        text: String,
    },
}

/// `decide` was called while no failure was pending.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NotHalted;

/// A receipt and the counts the ledger line is made of.
#[derive(Clone, Debug)]
struct Stored {
    handoff: Handoff,
    summary: receipt::Summary,
}

#[derive(Clone, Debug)]
struct Slot {
    spec: WorkerSpec,
    status: WorkerStatus,
    started: Option<Instant>,
    result: Option<WorkerResult>,
    /// What the worker staged (S7). Kept across a retry: the staged files
    /// stay in the pane's batch whatever the attempt.
    ledger: ChangeLedger,
    /// What the worker's engine reported so far, over every attempt: a failed
    /// attempt spent its tokens too.
    usage: Option<Usage>,
    /// The current attempt reported usage.
    reported: bool,
    /// Some attempt that ran ended without reporting: the sum is partial.
    gap: bool,
    /// The orchestrator's handoff for the worker, or the fallback lazarobox
    /// built (S4, D17).
    handoff: Option<Handoff>,
    /// Waiting on its own review (see [`Run::park`]): not counted as running
    /// for `max_parallel`.
    parked: bool,
}

#[derive(Clone, Debug)]
pub struct Run {
    id: RunId,
    state: RunState,
    roster: Roster,
    deadline: Duration,
    limits: RunLimits,
    /// Worker `n` is `slots[n - 1]`.
    slots: Vec<Slot>,
    /// Queued workers, in the order they started waiting.
    queue: VecDeque<WorkerId>,
    /// Failures that ended while the run was already halted, in order. Each
    /// halts the run in turn once the one before is decided.
    pending: VecDeque<WorkerId>,
    /// The workers a refused `finish` named as missing a handoff: the next
    /// one builds theirs, unless another worker ended since (D17).
    finish_warned: Vec<WorkerId>,
    /// The run was cancelled while a failure waited: the handoffs built
    /// after that are `blocked` too (D17).
    halted_at_cancel: bool,
    /// What the orchestrator said when it finished the run.
    summary: Option<String>,
    /// The orchestrator turns taken so far, the first included.
    turns: usize,
    /// The ODD steps and their handoffs (S5).
    steps: Steps,
    /// The receipts of the run's reviews, one per review (S7b, D16).
    receipts: Vec<Stored>,
    /// The feature document of a large goal, once `track` built it (S5).
    feature: Option<FeatureDoc>,
}

impl Run {
    pub fn new(id: RunId, roster: Roster, deadline: Duration) -> Self {
        Self {
            id,
            state: RunState::Open,
            roster,
            deadline,
            limits: RunLimits::default(),
            slots: Vec::new(),
            queue: VecDeque::new(),
            pending: VecDeque::new(),
            finish_warned: Vec::new(),
            halted_at_cancel: false,
            summary: None,
            turns: 1,
            steps: Steps::default(),
            receipts: Vec::new(),
            feature: None,
        }
    }

    /// Bounds the run by `limits` (at least one worker runs at a time).
    pub fn with_limits(mut self, limits: RunLimits) -> Self {
        self.limits = RunLimits {
            parallel: limits.parallel.max(1),
            ..limits
        };
        self
    }

    pub fn limits(&self) -> RunLimits {
        self.limits
    }

    pub fn id(&self) -> RunId {
        self.id
    }

    pub fn state(&self) -> &RunState {
        &self.state
    }

    /// Takes the roster of a new request. Workers already started keep their
    /// spec; later spawns use this one.
    pub fn set_roster(&mut self, roster: Roster) {
        self.roster = roster;
    }

    /// How many workers the run has spawned so far.
    pub fn worker_count(&self) -> usize {
        self.slots.len()
    }

    pub fn status(&self, id: WorkerId) -> Option<WorkerStatus> {
        self.slot(id).map(|slot| slot.status)
    }

    pub fn spec(&self, id: WorkerId) -> Option<&WorkerSpec> {
        self.slot(id).map(|slot| &slot.spec)
    }

    /// What worker `id` reported using so far; `None` when its engine
    /// reported nothing (shown as `n/a`, never as zero).
    pub fn usage(&self, id: WorkerId) -> Option<Usage> {
        let slot = self.slot(id)?;
        slot.usage.map(|usage| shown(usage, slot.gap))
    }

    /// Whether an attempt of worker `id` ended without reporting, so its
    /// usage covers only the attempts that did. A refused worker never ran
    /// and misses nothing; a cancelled or timed-out one does.
    pub fn usage_partial(&self, id: WorkerId) -> bool {
        self.slot(id).is_some_and(|slot| slot.gap)
    }

    /// The sum over the workers that reported; `None` when none did.
    /// The cost is left out when the sum is partial.
    pub fn total_usage(&self) -> Option<Usage> {
        let sum = self
            .slots
            .iter()
            .filter_map(|slot| slot.usage)
            .reduce(Usage::plus)?;
        Some(shown(sum, self.total_partial()))
    }

    /// Whether the run's sum misses something: a worker that has not ended,
    /// or a worker with an attempt that reported nothing.
    pub fn total_partial(&self) -> bool {
        self.slots.iter().any(|slot| {
            slot.gap || matches!(slot.status, WorkerStatus::Queued | WorkerStatus::Running)
        })
    }

    pub fn result(&self, id: WorkerId) -> Option<&WorkerResult> {
        self.slot(id)?.result.as_ref()
    }

    /// What worker `id` staged.
    pub fn ledger(&self, id: WorkerId) -> Option<&ChangeLedger> {
        self.slot(id).map(|slot| &slot.ledger)
    }

    /// The host staged `change` for worker `id`.
    pub fn staged(&mut self, id: WorkerId, change: Change) {
        if let Some(slot) = self.slots.get_mut(index(id)) {
            slot.ledger.staged(change);
        }
    }

    /// The user's batch review applied a batch holding `paths` that worker
    /// `id` proposed.
    pub fn applied(&mut self, id: WorkerId, paths: &[String]) {
        if let Some(slot) = self.slots.get_mut(index(id)) {
            slot.ledger.applied(paths);
        }
    }

    /// A running worker waits on work of its own run (its change's review):
    /// its slot goes to the next queued worker meanwhile, or that work would
    /// queue behind it for ever. `false` for a worker that is not running.
    pub fn park(&mut self, now: Instant, id: WorkerId) -> bool {
        if self.status(id) != Some(WorkerStatus::Running) {
            return false;
        }
        self.slots[index(id)].parked = true;
        self.promote(now);
        true
    }

    /// The worker is back: it holds its slot again, never queued, so the run
    /// may briefly run one over `max_parallel` until another worker ends.
    pub fn unpark(&mut self, id: WorkerId) {
        if let Some(slot) = self.slots.get_mut(index(id)) {
            slot.parked = false;
        }
    }

    /// Creates a worker. An unavailable engine still creates it, as `Refused`
    /// with the reason, and halts the run like any failure: no other engine
    /// is tried. Only a paused or ended run refuses outright.
    pub fn spawn(&mut self, now: Instant, task: Task, prompt: &str) -> Result<WorkerId, Refusal> {
        match self.state {
            RunState::Halted { .. } => return Err(Refusal::Paused),
            RunState::Finished | RunState::Cancelled => return Err(Refusal::Ended),
            RunState::Open => {}
        }
        if self.steps.read_only() && task == Task::Code {
            return Err(Refusal::ReadOnly);
        }
        if self.slots.len() >= self.limits.workers {
            return Err(Refusal::WorkerCap(self.limits.workers));
        }
        let (spec, refusal) = self.resolve(task, prompt);
        self.slots.push(Slot {
            spec,
            status: WorkerStatus::Queued,
            started: None,
            result: None,
            ledger: ChangeLedger::default(),
            usage: None,
            reported: false,
            gap: false,
            handoff: None,
            parked: false,
        });
        let id = WorkerId(self.slots.len() as u32);
        self.enqueue(now, id, refusal, false);
        self.promote(now);
        Ok(id)
    }

    /// A worker waits in the queue, at the back for a new one and at the
    /// front for a retried one; a refusal ends it at once.
    fn enqueue(&mut self, now: Instant, id: WorkerId, refusal: Option<String>, front: bool) {
        // A new attempt starts the clock over, so a refusal reports no time.
        self.slots[index(id)].started = None;
        // A read-only goal starts no code worker, however it got here (a
        // retry, a relaunch).
        let code = self.slots[index(id)].spec.task == Task::Code;
        let refusal =
            refusal.or_else(|| (code && self.steps.read_only()).then(|| READ_ONLY.to_owned()));
        if let Some(reason) = refusal {
            return self.end(
                now,
                id,
                WorkerStatus::Refused,
                "",
                Some((ErrorClass::Other, reason)),
            );
        }
        let slot = &mut self.slots[index(id)];
        slot.result = None;
        slot.reported = false;
        slot.parked = false;
        slot.status = WorkerStatus::Queued;
        if front {
            self.queue.push_front(id);
        } else {
            self.queue.push_back(id);
        }
    }

    /// Starts queued workers, oldest first, while the run is open and a slot
    /// is free.
    fn promote(&mut self, now: Instant) {
        while self.state == RunState::Open
            && self
                .slots
                .iter()
                .filter(|s| s.status == WorkerStatus::Running && !s.parked)
                .count()
                < self.limits.parallel
        {
            let Some(next) = self.queue.pop_front() else {
                return;
            };
            let slot = &mut self.slots[index(next)];
            (slot.status, slot.started) = (WorkerStatus::Running, Some(now));
        }
    }

    fn resolve(&self, task: Task, prompt: &str) -> (WorkerSpec, Option<String>) {
        let (engine, model, refusal) = match self.roster.get(task) {
            Availability::Ready { engine, model } => (engine.clone(), model.clone(), None),
            Availability::Unassigned => (
                String::new(),
                None,
                Some(format!("no engine assigned to {}", task.id())),
            ),
            other => (String::new(), None, guidance(other)),
        };
        let spec = WorkerSpec {
            task,
            prompt: prompt.to_owned(),
            engine,
            model,
            toolset: Toolset::for_kind(task),
            deadline: self.deadline,
        };
        (spec, refusal)
    }

    /// Applies what the runtime saw. Late events for a worker that is no
    /// longer running (cancelled, timed out) are ignored.
    pub fn on_event(&mut self, now: Instant, id: WorkerId, event: WorkerEvent) {
        if self.status(id) != Some(WorkerStatus::Running) {
            return;
        }
        match event {
            WorkerEvent::Usage(report) => {
                let slot = &mut self.slots[index(id)];
                slot.reported = true;
                slot.usage = Some(slot.usage.map_or(report, |sum| sum.plus(report)));
            }
            WorkerEvent::UsageGap => self.slots[index(id)].gap = true,
            WorkerEvent::Done { text } => self.end(now, id, WorkerStatus::Done, &text, None),
            WorkerEvent::Failed { class, text } => {
                self.end(now, id, WorkerStatus::Failed, "", Some((class, text)));
            }
        }
    }

    /// Times out the running workers past their deadline. Returns them so
    /// the runtime kills their processes.
    pub fn on_tick(&mut self, now: Instant) -> Vec<WorkerId> {
        let late: Vec<_> = self.ids(WorkerStatus::Running, |slot| {
            // An overflowing deadline never expires.
            slot.started.is_some_and(|at| {
                at.checked_add(slot.spec.deadline)
                    .is_some_and(|end| now >= end)
            })
        });
        for id in &late {
            let error = (ErrorClass::Timeout, "deadline passed".to_owned());
            self.end(now, *id, WorkerStatus::TimedOut, "", Some(error));
        }
        late
    }

    /// The status of each id. While a failure is unresolved every id answers
    /// `awaiting_user`, without the error text.
    pub fn wait_report(&self, ids: &[WorkerId]) -> Vec<(WorkerId, &'static str)> {
        let halted = matches!(self.state, RunState::Halted { .. });
        ids.iter()
            .map(|&id| {
                let label = match (halted, self.status(id)) {
                    (true, _) => "awaiting_user",
                    (false, Some(status)) => status.label(),
                    (false, None) => "unknown",
                };
                (id, label)
            })
            .collect()
    }

    /// The final text of a finished worker.
    pub fn collect(&self, id: WorkerId) -> Result<&str, CollectError> {
        let slot = self.slot(id).ok_or(CollectError::Unknown)?;
        let result = slot.result.as_ref().ok_or(CollectError::NotFinished)?;
        match result.status {
            WorkerStatus::Done => Ok(&result.text),
            status => Err(CollectError::Ended(status)),
        }
    }

    /// [`collect`](Self::collect) as the orchestrator receives it: cleaned,
    /// capped and wrapped as untrusted data under a nonce derived from
    /// `nonce_bytes`. The bytes MUST come from the OS CSPRNG on every call and
    /// never be reused or shown to the worker, or its text could forge the
    /// closing tag. A worker that staged files has its `files changed`
    /// ledger first, inside the wrapper: its paths are the worker's names.
    pub fn collect_wrapped(
        &self,
        nonce_bytes: [u8; 8],
        id: WorkerId,
    ) -> Result<String, CollectError> {
        let nonce = label::nonce_from(nonce_bytes);
        let text = self.collect(id)?;
        // The ledger comes first, so the cap on a long answer never hides it.
        let ledger = self.ledger(id).and_then(ChangeLedger::render);
        Ok(match ledger {
            Some(ledger) => label::wrap(&nonce, &format!("{ledger}\n\n{text}")),
            None => label::wrap(&nonce, text),
        })
    }

    /// The handoff stored for worker `id`, the orchestrator's or a fallback.
    pub fn stored_handoff(&self, id: WorkerId) -> Option<&Handoff> {
        self.slot(id)?.handoff.as_ref()
    }

    /// The worker whose unresolved failure forces every verdict to
    /// `blocked`; `None` when the run is not halted.
    pub fn forced_by(&self) -> Option<WorkerId> {
        match &self.state {
            RunState::Halted { worker, .. } => Some(*worker),
            _ => None,
        }
    }

    /// Stores the orchestrator's handoff for the ended worker `id`, replacing
    /// an earlier one. The facts come from the run; the verdict is `blocked`
    /// while a failure is unresolved (D17).
    pub fn handoff(&mut self, id: WorkerId, note: Judgement) -> Result<Handoff, HandoffError> {
        if matches!(self.state, RunState::Finished | RunState::Cancelled) {
            return Err(HandoffError::Ended);
        }
        let slot = self.slot(id).ok_or(HandoffError::Unknown)?;
        if matches!(slot.status, WorkerStatus::Queued | WorkerStatus::Running) {
            return Err(HandoffError::NotFinished);
        }
        let handoff = Handoff::of(self.facts(id), note, self.forced_by().is_some());
        self.slots[index(id)].handoff = Some(handoff.clone());
        Ok(handoff)
    }

    /// Stores the receipt of a review of worker `ev.worker`'s change: built
    /// from the review, never from the model; the same review again (a path
    /// found late) replaces it. The facts are the reviewed worker's.
    pub fn receipt(&mut self, ev: &Evidence) -> Result<Handoff, HandoffError> {
        if matches!(self.state, RunState::Finished | RunState::Cancelled) {
            return Err(HandoffError::Ended);
        }
        self.slot(ev.worker).ok_or(HandoffError::Unknown)?;
        let facts = Facts {
            unit: Unit::Receipt(ev.worker, ev.n),
            ..self.facts(ev.worker)
        };
        let handoff = Handoff::of(facts, receipt::judgement(ev), self.forced_by().is_some());
        let stored = Stored {
            handoff: handoff.clone(),
            summary: receipt::summary(ev),
        };
        match self
            .receipts
            .iter_mut()
            .find(|s| s.handoff.unit == handoff.unit)
        {
            Some(known) => *known = stored,
            None => self.receipts.push(stored),
        }
        Ok(handoff)
    }

    /// The receipt line of a ledger: the reviews, those not reviewed and the
    /// blockers open, and the shorter count for a ledger at its bound.
    fn reviews_lines(&self) -> (String, String) {
        let count = self.receipts.len();
        if count == 0 {
            return (String::new(), String::new());
        }
        let unreviewed = self.receipts.iter().filter(|s| !s.summary.reviewed).count();
        let open: usize = self.receipts.iter().map(|s| s.summary.open).sum();
        (
            format!("Reviews: {count} ({unreviewed} not reviewed, {open} blockers open)"),
            format!("Reviews: {count}"),
        )
    }

    fn facts(&self, id: WorkerId) -> Facts {
        let slot = &self.slots[index(id)];
        Facts {
            run: self.id,
            unit: Unit::Worker(id),
            kind: slot.spec.task,
            engine: slot.spec.engine.clone(),
            model: slot.spec.model.clone(),
            status: slot.status,
        }
    }

    pub fn steps(&self) -> &Steps {
        &self.steps
    }

    #[cfg(test)]
    pub(super) fn steps_mut(&mut self) -> &mut Steps {
        &mut self.steps
    }

    /// Stores the orchestrator's handoff for `step` and closes it. Refused
    /// out of order, or once the run is over; nothing is stored then. A
    /// closed step may be handed off again: the record is replaced. The
    /// orchestrator that writes it is the run's `orchestrate` engine.
    pub fn handoff_step(
        &mut self,
        step: Step,
        mark: Mark,
        note: Judgement,
    ) -> Result<StepDone, StepError> {
        if matches!(self.state, RunState::Finished | RunState::Cancelled) {
            return Err(StepError::Ended);
        }
        self.steps.check(step)?;
        self.steps.check_mark(step, mark)?;
        if step == Step::Classify && note.reason.trim().is_empty() {
            return Err(StepError::NoReason);
        }
        let narrows = mark == Mark::Grant(Grant::ReadOnly) && !self.steps.read_only();
        if narrows {
            // A code worker that runs would keep writing: cancel it first.
            if let Some(id) = self
                .ids(WorkerStatus::Running, |s| s.spec.task == Task::Code)
                .first()
            {
                return Err(StepError::CodeRunning(*id));
            }
        }
        if step == Step::Track && self.feature.is_none() {
            return Err(StepError::Doc(FeatureError::NoDocument));
        }
        Ok(self.close_step(step, mark, note, narrows))
    }

    /// Records and closes a step already checked. `narrows`: the goal just
    /// became read-only, so the queued `code` workers end refused.
    fn close_step(&mut self, step: Step, mark: Mark, note: Judgement, narrows: bool) -> StepDone {
        let (engine, model) = match self.roster.get(Task::Orchestrate) {
            Availability::Ready { engine, model } => (engine.clone(), model.clone()),
            _ => (String::new(), None),
        };
        let facts = Facts {
            run: self.id,
            unit: Unit::Step(step),
            kind: Task::Orchestrate,
            engine,
            model,
            status: WorkerStatus::Done,
        };
        // A step closes even while the run is halted, its verdict forced to
        // `blocked`: the same rule as a worker's handoff (D17).
        let handoff = Handoff::of(facts, note, self.forced_by().is_some());
        self.steps.record(step, handoff.clone());
        // Checked and recorded just above: closing cannot be refused.
        let _ = self.steps.close(step, mark);
        let refused = if narrows {
            self.refuse_queued_code()
        } else {
            Vec::new()
        };
        StepDone { handoff, refused }
    }

    /// Builds the feature document of `goal` and closes `track` with it: the
    /// handoff of the step and the document, which the app stores under its
    /// own key. One per run; later changes are corrections. It takes the same
    /// checks as any step handoff: the run is open and `track` is next.
    pub fn feature_build(
        &mut self,
        goal: &str,
        specs: &[String],
        tasks: &[String],
        note: Judgement,
    ) -> Result<(Handoff, FeatureDoc), StepError> {
        if matches!(self.state, RunState::Finished | RunState::Cancelled) {
            return Err(StepError::Ended);
        }
        if self.feature.is_some() {
            return Err(StepError::Doc(FeatureError::Exists));
        }
        self.steps.check(Step::Track)?;
        let doc = FeatureDoc::build(self.id, goal, specs, tasks).map_err(StepError::Doc)?;
        let done = self.close_step(Step::Track, Mark::None, note, false);
        self.feature = Some(doc.clone());
        Ok((done.handoff, doc))
    }

    /// A user correction of the feature document (S5): the log gains it and
    /// one requirement is rewritten.
    pub fn feature_correct(
        &mut self,
        spec: &str,
        text: &str,
        correction: &str,
    ) -> Result<FeatureDoc, StepError> {
        if matches!(self.state, RunState::Finished | RunState::Cancelled) {
            return Err(StepError::Ended);
        }
        let doc = self
            .feature
            .as_mut()
            .ok_or(StepError::Doc(FeatureError::NoDocument))?;
        doc.correct(spec, text, correction)
            .map_err(StepError::Doc)?;
        Ok(doc.clone())
    }

    /// Ends the queued `code` workers as refused (the goal became read-only).
    /// No halt: nothing failed, and the user has nothing to decide.
    fn refuse_queued_code(&mut self) -> Vec<WorkerId> {
        let queued: Vec<_> = self.queue.iter().copied().collect();
        let code: Vec<_> = queued
            .into_iter()
            .filter(|id| self.slots[index(*id)].spec.task == Task::Code)
            .collect();
        for id in &code {
            let slot = &mut self.slots[index(*id)];
            slot.status = WorkerStatus::Refused;
            slot.result = Some(WorkerResult {
                status: WorkerStatus::Refused,
                text: String::new(),
                elapsed: Duration::ZERO,
                error: Some((ErrorClass::Other, READ_ONLY.to_owned())),
                engine: slot.spec.engine.clone(),
                model: slot.spec.model.clone(),
                usage: slot.usage.map(|usage| shown(usage, slot.gap)),
            });
        }
        self.queue.retain(|queued| !code.contains(queued));
        code
    }

    /// The workers that ended and have no handoff, oldest first.
    fn without_handoff(&self) -> Vec<WorkerId> {
        let numbered = self.slots.iter().enumerate();
        numbered
            .filter(|(_, slot)| {
                slot.handoff.is_none()
                    && !matches!(slot.status, WorkerStatus::Queued | WorkerStatus::Running)
            })
            .map(|(i, _)| WorkerId(i as u32 + 1))
            .collect()
    }

    /// Builds and stores the handoff of every ended worker that has none
    /// (`unknown`, or `blocked` while halted; never worker text) and returns
    /// them. For `finish`, the turn cap, a cancel and an expiry.
    pub fn fallbacks(&mut self) -> Vec<Handoff> {
        let blocked = self.forced_by().is_some() || self.halted_at_cancel;
        let built: Vec<_> = self
            .without_handoff()
            .into_iter()
            .map(|id| (id, Handoff::fallback(self.facts(id), blocked)))
            .collect();
        for (id, handoff) in &built {
            self.slots[index(*id)].handoff = Some(handoff.clone());
        }
        built.into_iter().map(|(_, handoff)| handoff).collect()
    }

    /// The orchestrator is done. Refused while a failure waits for the user
    /// or a worker is queued or running; the first time handoffs are
    /// missing it is refused naming them, and the next one builds them,
    /// keeps `summary` and finishes the run. Returns the handoffs it built.
    pub fn finish(&mut self, summary: &str) -> Result<Vec<Handoff>, FinishError> {
        match self.state {
            RunState::Finished | RunState::Cancelled => return Err(FinishError::Ended),
            RunState::Halted { .. } => return Err(FinishError::Paused),
            RunState::Open => {}
        }
        let live = self.ids(WorkerStatus::Queued, |_| true);
        let live = [live, self.ids(WorkerStatus::Running, |_| true)].concat();
        if !live.is_empty() {
            return Err(FinishError::Live(live));
        }
        let missing = self.without_handoff();
        if missing.iter().any(|id| !self.finish_warned.contains(id)) {
            self.finish_warned.extend(missing.iter().copied());
            return Err(FinishError::Missing(missing));
        }
        let built = self.fallbacks();
        self.summary = Some(summary.to_owned());
        self.state = RunState::Finished;
        Ok(built)
    }

    /// The orchestrator turn ended. A run still `Open` with a worker queued or
    /// running, or a finished worker with no handoff, continues in a new turn
    /// until `max_turns` turns were taken (D8); the turn is counted here.
    pub fn next_turn(&mut self) -> NextTurn {
        let live = [WorkerStatus::Queued, WorkerStatus::Running]
            .iter()
            .any(|status| !self.ids(*status, |_| true).is_empty());
        let pending = live || !self.without_handoff().is_empty();
        if self.state != RunState::Open || !pending {
            return NextTurn::Settle;
        }
        if self.turns >= self.limits.turns {
            return NextTurn::Capped(self.turns);
        }
        self.turns += 1;
        NextTurn::Continue(self.turns)
    }

    /// What the orchestrator said when it finished the run.
    pub fn summary(&self) -> Option<&str> {
        self.summary.as_deref()
    }

    /// The ledger of the run under `goal`: a row per worker.
    pub fn run_ledger(&self, goal: &str) -> Ledger {
        let rows = self.slots.iter().enumerate().map(|(i, slot)| {
            let id = WorkerId(i as u32 + 1);
            Row::new(id, slot.spec.task, slot.status, slot.handoff.as_ref())
        });
        let feature = self.feature.as_ref();
        let (reviews, reviews_short) = self.reviews_lines();
        Ledger::new(goal, rows.collect())
            .with_reviews(reviews, reviews_short)
            .with_steps(self.steps.line(), self.steps.short_line())
            .with_feature(
                feature
                    .map(FeatureDoc::render_for_ledger)
                    .unwrap_or_default(),
                feature.map(FeatureDoc::short_line).unwrap_or_default(),
            )
    }

    /// Cancels one worker. Its slot goes to the next queued worker.
    pub fn cancel(&mut self, now: Instant, id: WorkerId) -> CancelOutcome {
        match self.status(id) {
            None => CancelOutcome::Unknown,
            Some(WorkerStatus::Queued | WorkerStatus::Running) => {
                self.end(now, id, WorkerStatus::Cancelled, "", None);
                CancelOutcome::Cancelled
            }
            Some(_) => CancelOutcome::AlreadyEnded,
        }
    }

    /// Applies the user's answer to a failure. Returns the workers the
    /// runtime must kill (`Cancel` only); after a retry it reads `status`.
    pub fn decide(&mut self, now: Instant, choice: UserChoice) -> Result<Vec<WorkerId>, NotHalted> {
        let RunState::Halted { worker, .. } = self.state else {
            return Err(NotHalted);
        };
        let reresolve = match choice {
            UserChoice::Cancel => return Ok(self.cancel_run(now)),
            UserChoice::Retry => false,
            UserChoice::Relaunch(roster) => {
                self.roster = roster;
                true
            }
        };
        let others = if reresolve {
            self.reresolve_queued(now, worker)
        } else {
            0
        };
        let slot = &self.slots[index(worker)];
        // A refused worker has no engine to reuse: it is resolved again.
        let refusal = if reresolve || slot.status == WorkerStatus::Refused {
            let (spec, refusal) = self.resolve(slot.spec.task, &slot.spec.prompt.clone());
            self.slots[index(worker)].spec = spec;
            // The queued workers refused with it are one failure, said once.
            refusal.map(|reason| match others {
                0 => reason,
                n => format!("{} workers refused: {reason}", n + 1),
            })
        } else {
            None
        };
        // The run stays halted while the worker is queued again, so a refusal
        // lands in `pending` like any other late failure.
        self.enqueue(now, worker, refusal, true);
        self.reopen(now);
        Ok(Vec::new())
    }

    /// The queued workers of `failed`'s task take the roster of a relaunch,
    /// so a run override reaches workers spawned before it; an engine that
    /// cannot run refuses them like the relaunched worker, and they add no
    /// halt of their own: the caller reports the group once, with the count
    /// this returns. Only `Queued` workers of that task: one that is already
    /// `Refused` or in the `pending` list keeps the spec it was spawned with
    /// (a `Running` worker of the task keeps the spec it started with).
    /// `decide` replaces the whole roster: queued workers of OTHER tasks keep
    /// the spec they were spawned with, while later spawns use the new roster.
    fn reresolve_queued(&mut self, now: Instant, failed: WorkerId) -> usize {
        let task = self.slots[index(failed)].spec.task;
        let queued: Vec<WorkerId> = self.queue.iter().copied().collect();
        let mut refused = Vec::new();
        for id in queued {
            let slot = &self.slots[index(id)];
            if slot.spec.task != task {
                continue;
            }
            let (spec, refusal) = self.resolve(task, &slot.spec.prompt.clone());
            self.slots[index(id)].spec = spec;
            if let Some(reason) = refusal {
                self.enqueue(now, id, Some(reason), false);
                refused.push(id);
            }
        }
        self.pending.retain(|id| !refused.contains(id));
        refused.len()
    }

    /// After a decision: halts again on the oldest unresolved failure, else
    /// opens the run and hands out the free slots.
    fn reopen(&mut self, now: Instant) {
        if let Some(next) = self.pending.pop_front() {
            let (class, text) = self.slots[index(next)]
                .result
                .as_ref()
                .and_then(|result| result.error.clone())
                .unwrap_or((ErrorClass::Other, String::new()));
            self.state = RunState::Halted {
                worker: next,
                class,
                text,
            };
        } else {
            self.state = RunState::Open;
            self.promote(now);
        }
    }

    /// Cancels the whole run (Ctrl+C, the user's cancel, a replaced or
    /// quitting run): its running and queued workers end `cancelled`. Returns
    /// the running ones, whose processes the runtime must kill.
    pub fn cancel_run(&mut self, now: Instant) -> Vec<WorkerId> {
        let running = self.ids(WorkerStatus::Running, |_| true);
        self.halted_at_cancel |= self.forced_by().is_some();
        self.state = RunState::Cancelled;
        self.pending.clear();
        for id in running
            .iter()
            .chain(&self.ids(WorkerStatus::Queued, |_| true))
        {
            self.end(now, *id, WorkerStatus::Cancelled, "", None);
        }
        running
    }

    /// Ends a worker. An error halts an open run (the siblings carry on) or,
    /// when already halted, waits its turn in `pending`; otherwise an open
    /// run hands the free slot on.
    fn end(
        &mut self,
        now: Instant,
        id: WorkerId,
        status: WorkerStatus,
        text: &str,
        error: Option<(ErrorClass, String)>,
    ) {
        let slot = &mut self.slots[index(id)];
        slot.gap |= status != WorkerStatus::Refused && !slot.reported;
        slot.status = status;
        slot.result = Some(WorkerResult {
            status,
            text: text.to_owned(),
            elapsed: slot
                .started
                .map_or(Duration::ZERO, |at| now.saturating_duration_since(at)),
            error: error.clone(),
            engine: slot.spec.engine.clone(),
            model: slot.spec.model.clone(),
            usage: slot.usage.map(|usage| shown(usage, slot.gap)),
        });
        self.queue.retain(|queued| *queued != id);
        match (&self.state, error) {
            (RunState::Open, Some((class, text))) => {
                self.state = RunState::Halted {
                    worker: id,
                    class,
                    text,
                };
            }
            (RunState::Halted { .. }, Some(_)) => self.pending.push_back(id),
            _ => self.promote(now),
        }
    }

    fn slot(&self, id: WorkerId) -> Option<&Slot> {
        self.slots.get(index(id))
    }

    fn ids(&self, status: WorkerStatus, keep: impl Fn(&Slot) -> bool) -> Vec<WorkerId> {
        let numbered = self.slots.iter().enumerate();
        numbered
            .filter(|(_, slot)| slot.status == status && keep(slot))
            .map(|(i, _)| WorkerId(i as u32 + 1))
            .collect()
    }
}

/// Worker ids start at 1; id 0 maps to no slot.
/// A sum as it is shown: with no cost when it is partial, because a total
/// that misses a report must not read as the whole.
fn shown(usage: Usage, partial: bool) -> Usage {
    Usage {
        cost_micros: usage.cost_micros.filter(|_| !partial),
        ..usage
    }
}

fn index(id: WorkerId) -> usize {
    (id.0 as usize).wrapping_sub(1)
}
