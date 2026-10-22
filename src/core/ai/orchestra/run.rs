//! The run: its workers, limits and the failure pause. Pure; time comes in as
//! explicit `Instant`s and nothing here starts a process.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::worker::{ErrorClass, WorkerResult, WorkerSpec, WorkerStatus};
use super::{RunId, Toolset, WorkerId};
use crate::core::ai::chat::engine::Availability;
use crate::core::ai::chat::guidance::guidance;
use crate::core::ai::config::Task;

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

    fn get(&self, task: Task) -> &Availability {
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
    Done { text: String },
    Failed { class: ErrorClass, text: String },
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

    pub fn result(&self, id: WorkerId) -> Option<&WorkerResult> {
        self.slot(id)?.result.as_ref()
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
        });
        let id = WorkerId(self.slots.len() as u32);
        self.enqueue(now, id, refusal);
        self.promote(now);
        Ok(id)
    }

    /// A worker waits at the back of the queue; a refusal ends it at once.
    fn enqueue(&mut self, now: Instant, id: WorkerId, refusal: Option<String>) {
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
        (slot.status, slot.started) = (WorkerStatus::Queued, None);
        self.queue.push_back(id);
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
        self.enqueue(now, worker, refusal);
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

    fn cancel_run(&mut self, now: Instant) -> Vec<WorkerId> {
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
fn index(id: WorkerId) -> usize {
    (id.0 as usize).wrapping_sub(1)
}
