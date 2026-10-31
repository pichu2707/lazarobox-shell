//! The run: its workers, limits and the failure pause. Pure; time comes in as
//! explicit `Instant`s and nothing here starts a process.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::changes::{Change, ChangeLedger};
use super::worker::{ErrorClass, WorkerResult, WorkerSpec, WorkerStatus};
use super::{RunId, Toolset, WorkerId};
use crate::core::ai::chat::engine::{Availability, Usage};
use crate::core::ai::chat::guidance::guidance;
use crate::core::ai::config::Task;
use crate::core::ai::tools::label;

/// Workers running at once. S3 makes it a setting.
const MAX_RUNNING: usize = 1;

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
}

#[derive(Clone, Debug)]
pub struct Run {
    id: RunId,
    state: RunState,
    roster: Roster,
    deadline: Duration,
    /// Worker `n` is `slots[n - 1]`.
    slots: Vec<Slot>,
    /// Queued workers, in the order they started waiting.
    queue: VecDeque<WorkerId>,
    /// Failures that ended while the run was already halted, in order. Each
    /// halts the run in turn once the one before is decided.
    pending: VecDeque<WorkerId>,
}

impl Run {
    pub fn new(id: RunId, roster: Roster, deadline: Duration) -> Self {
        Self {
            id,
            state: RunState::Open,
            roster,
            deadline,
            slots: Vec::new(),
            queue: VecDeque::new(),
            pending: VecDeque::new(),
        }
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

    /// Creates a worker. An unavailable engine still creates it, as `Refused`
    /// with the reason, and halts the run like any failure: no other engine
    /// is tried. Only a paused or ended run refuses outright.
    pub fn spawn(&mut self, now: Instant, task: Task, prompt: &str) -> Result<WorkerId, Refusal> {
        match self.state {
            RunState::Halted { .. } => return Err(Refusal::Paused),
            RunState::Finished | RunState::Cancelled => return Err(Refusal::Ended),
            RunState::Open => {}
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
        while self.state == RunState::Open && self.count(WorkerStatus::Running) < MAX_RUNNING {
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
        let slot = &self.slots[index(worker)];
        // A refused worker has no engine to reuse: it is resolved again.
        let refusal = if reresolve || slot.status == WorkerStatus::Refused {
            let (spec, refusal) = self.resolve(slot.spec.task, &slot.spec.prompt.clone());
            self.slots[index(worker)].spec = spec;
            refusal
        } else {
            None
        };
        // The run stays halted while the worker is queued again, so a refusal
        // lands in `pending` like any other late failure.
        self.enqueue(now, worker, refusal, true);
        self.reopen(now);
        Ok(Vec::new())
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

    fn count(&self, status: WorkerStatus) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.status == status)
            .count()
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
