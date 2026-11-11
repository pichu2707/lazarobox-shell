//! The run: its workers, limits and the failure pause. Pure; time comes in as
//! explicit `Instant`s and nothing here starts a process.

use std::collections::VecDeque;
use std::path::Path;
use std::time::{Duration, Instant};

use super::brief::{Brief, BriefError};
use super::changes::{Change, ChangeLedger, Disclosure};
use super::claims::Audit;
use super::commands::{Commands, Ran};
use super::feature::{FeatureDoc, FeatureError};
use super::ledger::{Facts, FinishError, Handoff, HandoffError, Judgement, Ledger, Row, Unit};
use super::receipt::{self, Evidence};
use super::review;
use super::steps::{Gate, Grant, Mark, READ_ONLY, Step, StepError, Steps, WRITE_OFF};
use super::textcall;
use super::worker::{ErrorClass, WorkerResult, WorkerSpec, WorkerStatus};
use super::{RunId, Toolset, WorkerId};
use crate::core::ai::chat::engine::{Availability, Usage, locality};
use crate::core::ai::chat::guidance::guidance;
use crate::core::ai::config::Task;
use crate::core::ai::tools::Locality;
use crate::core::ai::tools::github::forwards_off_machine;
use crate::core::ai::tools::label;
use crate::core::ai::tools::registry::{Family, FamilySet};

/// Whether a worker on `engine` and `model` is a cheap author (design D20):
/// a local server that keeps the model on the machine. An Ollama `*-cloud`
/// model is not, nor is a CLI engine, an unknown engine or an empty one.
pub fn cheap(engine: &str, model: Option<&str>) -> bool {
    locality(engine) == Some(Locality::Local) && !forwards_off_machine(engine, model)
}

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
    /// Nothing is pending but a spawn refusal of a step gate stands, naming
    /// this step: the run stays open for the user's reply, who may lift the
    /// gates (W3, option B). The reply turn is counted here.
    Wait(Step),
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
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Refusal {
    /// The brief of a `simple` worker is incomplete or names a bad path
    /// (W8b).
    Brief(BriefError),
    /// `run paused`: a failure is unresolved.
    Paused,
    Ended,
    /// The run already spawned as many workers as `max_workers` allows.
    WorkerCap(usize),
    /// `authorize` made the goal read-only: no `code` worker (S5, D15).
    ReadOnly,
    /// The menu has Write off: no `code` worker (D18). The text is
    /// [`WRITE_OFF`].
    WriteOff,
    /// A write-capable worker needs the steps before `implement` closed; the
    /// step is the first one still open (W3, D23).
    Gate(Step),
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

/// Told to the coordinator ahead of the answer of a worker that changed no
/// file and whose reply holds tool calls written as text: ASCII and neutral,
/// and true whatever the cause. It stands outside the worker's wrapper.
const CALLS_AS_TEXT: &str = "lazarobox: this worker changed no file, and its reply holds tool calls written as text. Do not report its files as created.";

/// What lazarobox says before the check of a handoff, outside the wrapper.
const CHECK_PREFIX: &str = "lazarobox: handoff check: ";

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
    /// stay in the writer's own batch whatever the attempt.
    ledger: ChangeLedger,
    /// The commands it called that reached the user's ask (W6), kept
    /// across a retry like the ledger.
    commands: Commands,
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
    /// The current turn runs without tools: no write tool was offered to it.
    no_tools: bool,
    /// Started by lazarobox itself (a review lens, the refuter, a
    /// correction): outside `max_workers` and with no handoff to wait for
    /// (D22), still shown with its row.
    internal: bool,
}

#[derive(Clone, Debug)]
pub struct Run {
    id: RunId,
    state: RunState,
    roster: Roster,
    deadline: Duration,
    limits: RunLimits,
    /// What the AI menu allows the workers spawned from now on (D17): Write
    /// and Exec, everything else is the role's own.
    ceiling: FamilySet,
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
    /// The user lifted the step gates for this run (W3, D24).
    gates_lifted: bool,
    /// The last gate that refused and the step it saw; cleared when that gate
    /// passes, so the turn cap names only a gate that still stands.
    last_gate: Option<(Gate, Step)>,
}

impl Run {
    pub fn new(id: RunId, roster: Roster, deadline: Duration) -> Self {
        Self {
            id,
            state: RunState::Open,
            roster,
            deadline,
            limits: RunLimits::default(),
            ceiling: FamilySet::ALL,
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
            gates_lifted: false,
            last_gate: None,
        }
    }

    /// Test fixture (W3): closes the steps a `code` spawn needs, granting
    /// `implement` for a small goal, so tests of workers need not hand off.
    #[cfg(test)]
    pub(crate) fn close_steps_for_tests(&mut self) {
        if self.steps.is_closed(Step::Classify) {
            return;
        }
        for step in [
            Step::Authorize,
            Step::Explore,
            Step::Resolve,
            Step::Classify,
        ] {
            let mark = match step {
                Step::Authorize => Mark::Grant(Grant::Implement),
                Step::Classify => Mark::Size(super::steps::Size::Small),
                _ => Mark::None,
            };
            let note = Judgement {
                verdict: super::ledger::Verdict::OnCourse,
                reason: "fixture".to_owned(),
                done: "fixture".to_owned(),
                left: String::new(),
                decisions: String::new(),
            };
            self.handoff_step(step, mark, note).expect("fixture step");
        }
    }

    /// The user lifts both step gates for this run, all or nothing (W3, D24).
    pub fn lift_gates(&mut self) {
        self.gates_lifted = true;
        self.last_gate = None;
    }

    /// Whether the user lifted the step gates of this run.
    pub fn gates_lifted(&self) -> bool {
        self.gates_lifted
    }

    /// The orchestrator turns taken so far, the first included.
    pub fn turns(&self) -> usize {
        self.turns
    }

    /// Forgets the standing refusal: the reply turn starts clean, and a new
    /// refusal sets it again.
    pub fn forget_gate(&mut self) {
        self.last_gate = None;
    }

    /// The gate that last refused and has not passed since.
    pub fn last_gate(&self) -> Option<(Gate, Step)> {
        self.last_gate
    }

    /// Bounds the run by `limits` (at least one worker runs at a time).
    pub fn with_limits(mut self, limits: RunLimits) -> Self {
        self.limits = RunLimits {
            parallel: limits.parallel.max(1),
            ..limits
        };
        self
    }

    /// Bounds the workers spawned from now on by what the menu allows.
    pub fn with_ceiling(mut self, ceiling: FamilySet) -> Self {
        self.ceiling = ceiling;
        self
    }

    /// Takes the menu of a new request. Workers already started keep their
    /// spec (D17: running workers keep what they started with); later
    /// spawns use this one.
    pub fn set_ceiling(&mut self, ceiling: FamilySet) {
        self.ceiling = ceiling;
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

    /// How many workers the coordinator spawned: what `max_workers` bounds.
    /// Internal workers (D22) are not counted.
    fn public_count(&self) -> usize {
        self.slots.iter().filter(|slot| !slot.internal).count()
    }

    /// Whether lazarobox started the worker itself (a review lens, the
    /// refuter, a correction).
    pub fn is_internal(&self, id: WorkerId) -> bool {
        self.slots.get(index(id)).is_some_and(|slot| slot.internal)
    }

    /// Whether the run made no worker and was not cancelled: an orchestrator
    /// turn that ends now answered in text alone (agent-orchestrator [W1]).
    pub fn undelegated(&self) -> bool {
        self.slots.is_empty() && self.state != RunState::Cancelled
    }

    pub fn status(&self, id: WorkerId) -> Option<WorkerStatus> {
        self.slot(id).map(|slot| slot.status)
    }

    pub fn spec(&self, id: WorkerId) -> Option<&WorkerSpec> {
        self.slot(id).map(|slot| &slot.spec)
    }

    /// Another worker whose turn is in flight on the same local server as
    /// worker `id` (owner decision 5, design D11): the server may queue their
    /// requests. A worker counts only while it runs and is not parked, and an
    /// engine only when it is a local server that does not forward the model
    /// off the machine (an Ollama `*-cloud` model is not).
    pub fn shares_engine(&self, id: WorkerId) -> Option<WorkerId> {
        let local = |slot: &Slot| {
            slot.status == WorkerStatus::Running
                && !slot.parked
                && cheap(&slot.spec.engine, slot.spec.model.as_deref())
        };
        let mine = self.slot(id).filter(|slot| local(slot))?;
        let other = |(n, slot): &(usize, &Slot)| {
            WorkerId(*n as u32 + 1) != id && local(slot) && slot.spec.engine == mine.spec.engine
        };
        let found = self.slots.iter().enumerate().find(other)?;
        Some(WorkerId(found.0 as u32 + 1))
    }

    /// Whether worker `id` is a cheap author (design D20), by the engine and
    /// model it was spawned with (or relaunched on), never the menu now: its
    /// staged change is always reviewed, with no consent ask (D21).
    pub fn cheap(&self, id: WorkerId) -> bool {
        self.spec(id)
            .is_some_and(|spec| cheap(&spec.engine, spec.model.as_deref()))
    }

    /// The notices a review of worker `id`'s change says as it starts (D22):
    /// the Review row as the roster resolved it against the author's spec.
    /// Empty when the row cannot run (the lens is refused, and that says so).
    pub fn review_notices(&self, id: WorkerId) -> Vec<String> {
        let (Some(author), Availability::Ready { engine, model }) =
            (self.spec(id), self.roster.get(Task::Review))
        else {
            return Vec::new();
        };
        let author = (author.engine.as_str(), author.model.as_deref());
        review::notices(id, author, (engine.as_str(), model.as_deref()))
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

    /// What the user should be told about how worker `id` ended, when nothing
    /// else shows it (agent-orchestrator [W1]): a `code` worker that ended
    /// `done` and staged nothing, or a worker with staged paths whose batch
    /// is still unproposed. `unproposed` asks the worker's own batch and runs only
    /// when the worker has paths staged and not on disk.
    pub fn disclosure(
        &self,
        id: WorkerId,
        unproposed: impl FnOnce() -> bool,
    ) -> Option<Disclosure> {
        let slot = self.slot(id)?;
        if matches!(slot.status, WorkerStatus::Queued | WorkerStatus::Running) {
            return None;
        }
        let files = slot.ledger.pending();
        if slot.ledger.changes().is_empty() {
            let silent = slot.status == WorkerStatus::Done && slot.spec.writes();
            if !silent {
                return None;
            }
            return Some(if slot.said_calls_as_text() {
                Disclosure::ToolCallAsText
            } else {
                Disclosure::EmptyLedger
            });
        }
        (files > 0 && unproposed()).then_some(Disclosure::StagedNotProposed { files })
    }

    /// The turn of worker `id` runs without tools (agent-orchestrator,
    /// degraded turn).
    pub fn ran_without_tools(&mut self, id: WorkerId) {
        if let Some(slot) = self.slots.get_mut(index(id)) {
            slot.no_tools = true;
        }
    }

    /// The host staged `change` for worker `id`.
    pub fn staged(&mut self, id: WorkerId, change: Change) {
        if let Some(slot) = self.slots.get_mut(index(id)) {
            slot.ledger.staged(change);
        }
    }

    /// Worker `id` called a command that reached the user's ask, and it
    /// ended as `ran` says (W6).
    pub fn ran(&mut self, id: WorkerId, ran: Ran) {
        if let Some(slot) = self.slots.get_mut(index(id)) {
            slot.commands.ran(ran);
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
        self.spawn_as(now, task, prompt, None, true)
    }

    /// Creates a `simple` worker from its brief (W8b). A brief that is
    /// incomplete or names a bad path creates no worker and changes nothing:
    /// the run stays as it was. The worker's one message is built from the
    /// brief.
    pub fn spawn_simple(
        &mut self,
        now: Instant,
        brief: Brief,
        origin: &Path,
    ) -> Result<WorkerId, Refusal> {
        self.check_open()?;
        let brief = brief.validate(origin).map_err(Refusal::Brief)?;
        self.spawn_as(now, Task::Simple, &brief.prompt(), Some(brief), true)
    }

    /// A worker lazarobox starts itself (a review lens, a correction): the
    /// step gates are the coordinator's, so they do not apply (W3).
    pub fn spawn_internal(
        &mut self,
        now: Instant,
        task: Task,
        prompt: &str,
    ) -> Result<WorkerId, Refusal> {
        self.spawn_as(now, task, prompt, None, false)
    }

    /// A paused or ended run takes no worker.
    fn check_open(&self) -> Result<(), Refusal> {
        match self.state {
            RunState::Halted { .. } => Err(Refusal::Paused),
            RunState::Finished | RunState::Cancelled => Err(Refusal::Ended),
            RunState::Open => Ok(()),
        }
    }

    fn spawn_as(
        &mut self,
        now: Instant,
        task: Task,
        prompt: &str,
        brief: Option<Brief>,
        gated: bool,
    ) -> Result<WorkerId, Refusal> {
        self.check_open()?;
        let (spec, refusal) = self.resolve(task, prompt, brief);
        // A read-only goal is judged by the role, before the menu: turning
        // Write on would not help. A `simple` worker with no `outputs` is no
        // writer: `resolve` already took its Write away (one rule, W8b).
        let writer = spec.toolset.families().contains(Family::Write)
            && !self.strips_tools(task, spec.outputs());
        if self.steps.read_only() && writer {
            return Err(Refusal::ReadOnly);
        }
        if gated && self.public_count() >= self.limits.workers {
            return Err(Refusal::WorkerCap(self.limits.workers));
        }
        if writer && gated && !self.gates_lifted {
            // `Implement` has no `Locked` or `NotLarge` case: only an open
            // step in front of it can refuse.
            if let Err(StepError::Order { open, .. }) = self.steps.check(Step::Implement) {
                self.last_gate = Some((Gate::Spawn, open));
                return Err(Refusal::Gate(open));
            }
        }
        if writer && !spec.writes() {
            return Err(Refusal::WriteOff);
        }
        self.slots.push(Slot {
            spec,
            status: WorkerStatus::Queued,
            started: None,
            result: None,
            ledger: ChangeLedger::default(),
            commands: Commands::default(),
            usage: None,
            reported: false,
            gap: false,
            handoff: None,
            parked: false,
            no_tools: false,
            internal: !gated,
        });
        let id = WorkerId(self.slots.len() as u32);
        if writer {
            // A code worker exists: neither gate stands any more.
            self.last_gate = None;
        }
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
        let code = self.refuses_on_read_only(&self.slots[index(id)].spec);
        // An engine that cannot run is a setup problem; a read-only goal is not.
        let refusal = refusal
            .map(|reason| (ErrorClass::Setup, reason))
            .or_else(|| {
                (code && self.steps.read_only()).then(|| (ErrorClass::Other, READ_ONLY.to_owned()))
            });
        if let Some(error) = refusal {
            return self.end(now, id, WorkerStatus::Refused, "", Some(error));
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
            // Starting is a new process: the menu is read again now, and a
            // writer that lost Write meanwhile is refused, not run bare.
            let strip = self.strips(&self.slots[index(next)].spec);
            let ceiling = self.ceiling;
            let spec = &mut self.slots[index(next)].spec;
            spec.families = Self::capped_families(spec.toolset, ceiling, strip);
            if !strip && spec.toolset.families().contains(Family::Write) && !spec.writes() {
                let error = (ErrorClass::Setup, WRITE_OFF.to_owned());
                self.end(now, next, WorkerStatus::Refused, "", Some(error));
                continue;
            }
            let slot = &mut self.slots[index(next)];
            (slot.status, slot.started) = (WorkerStatus::Running, Some(now));
            slot.no_tools = false;
        }
    }

    /// Whether a worker of `task` starts with Write and Exec taken away
    /// instead of being refused: only a `simple` worker (owner decision
    /// 2026-11-11, option B: it keeps Read only). With a brief, it is when
    /// the brief lists no `outputs` (W8b: a writing `simple` on a read-only
    /// goal is refused instead); without one, when the goal is read-only (the
    /// W8a rule for a worker spawned from free text).
    fn strips_tools(&self, task: Task, outputs: Option<&[String]>) -> bool {
        task == Task::Simple && outputs.map_or(self.steps.read_only(), <[String]>::is_empty)
    }

    fn strips(&self, spec: &WorkerSpec) -> bool {
        self.strips_tools(spec.task, spec.outputs())
    }

    /// A toolset under the menu's ceiling, without Write and Exec when
    /// `strip`.
    fn capped_families(toolset: Toolset, ceiling: FamilySet, strip: bool) -> FamilySet {
        let capped = toolset.capped(ceiling);
        if strip {
            capped.without(Family::Write).without(Family::Exec)
        } else {
            capped
        }
    }

    /// Whether a read-only goal refuses the worker: it writes and its tools
    /// are not taken away. A `simple` worker queued before the goal turned
    /// read-only still holds Write until `promote` strips it, so it is
    /// exempt through [`Self::strips_tools`], the one rule.
    fn refuses_on_read_only(&self, spec: &WorkerSpec) -> bool {
        spec.writes() && !self.strips(spec)
    }

    fn resolve(
        &self,
        task: Task,
        prompt: &str,
        brief: Option<Brief>,
    ) -> (WorkerSpec, Option<String>) {
        let (engine, model, refusal) = match self.roster.get(task) {
            Availability::Ready { engine, model } => (engine.clone(), model.clone(), None),
            Availability::Unassigned => (
                String::new(),
                None,
                Some(format!("no engine assigned to {}", task.id())),
            ),
            other => (String::new(), None, guidance(other)),
        };
        let strip = self.strips_tools(task, brief.as_ref().map(Brief::outputs));
        let spec = WorkerSpec {
            task,
            prompt: prompt.to_owned(),
            engine,
            model,
            toolset: Toolset::for_kind(task),
            families: Self::capped_families(Toolset::for_kind(task), self.ceiling, strip),
            deadline: self.deadline,
            brief,
        };
        // A role that writes, under a menu that no longer allows it, is
        // refused and never run read-only without a word. `spawn` refuses it
        // first; this covers the re-resolves of a retry and a relaunch.
        let refusal = refusal.or_else(|| {
            (spec.toolset.families().contains(Family::Write) && !spec.writes() && !strip)
                .then(|| WRITE_OFF.to_owned())
        });
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

    /// Worker `id`'s own deadline passed: its turn's clock, which stops while
    /// it waits for the user or its review (D5, D6), is the judge, never the
    /// wall time since it started. Ends it `timed_out` if it still runs.
    pub fn time_out(&mut self, now: Instant, id: WorkerId) {
        if self.status(id) == Some(WorkerStatus::Running) {
            let error = (ErrorClass::Timeout, "deadline passed".to_owned());
            self.end(now, id, WorkerStatus::TimedOut, "", Some(error));
        }
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
    /// Anything before the wrapper is lazarobox's own word, never the
    /// worker's.
    pub fn collect_wrapped(
        &self,
        nonce_bytes: [u8; 8],
        id: WorkerId,
    ) -> Result<String, CollectError> {
        let nonce = label::nonce_from(nonce_bytes);
        let text = self.collect(id)?;
        // The ledger comes first, so the cap on a long answer never hides
        // it, then the commands the worker ran (W6), then its text.
        let ledger = self.ledger(id).and_then(ChangeLedger::render);
        let commands = self.slot(id).and_then(|slot| slot.commands.render());
        let body: Vec<&str> = [ledger.as_deref(), commands.as_deref(), Some(text)]
            .into_iter()
            .flatten()
            .collect();
        let wrapped = label::wrap(&nonce, &body.join("\n\n"));
        // How its commands ended is lazarobox's own word, outside the
        // wrapper, so the worker's text cannot pass for it (W6).
        let record = self.slot(id).and_then(|slot| slot.commands.attested());
        let wrapped = match record {
            Some(record) => format!("{record}\n\n{wrapped}"),
            None => wrapped,
        };
        // lazarobox's own word, outside the worker's wrapper: the worker
        // cannot forge it, and the coordinator must not claim files exist.
        let said = self.slot(id).is_some_and(Slot::said_calls_as_text);
        let wrapped = if said {
            format!("{CALLS_AS_TEXT}\n\n{wrapped}")
        } else {
            wrapped
        };
        // The same for the check of its handoff (W4): app evidence, built
        // from the stored claims and the ledger, never from the worker's text.
        let lines = self
            .audit(id)
            .map(|audit| audit.lines())
            .unwrap_or_default();
        Ok(if lines.is_empty() {
            wrapped
        } else {
            format!("{CHECK_PREFIX}{}\n\n{wrapped}", lines.join("; "))
        })
    }

    /// The claim check of worker `id`'s stored handoff, against its ledger as
    /// it is now.
    pub fn audit(&self, id: WorkerId) -> Option<Audit> {
        let slot = self.slot(id)?;
        let handoff = slot.handoff.as_ref()?;
        let audit = handoff.audit.as_ref()?;
        Some(audit.recheck(slot.writer_ledger(), handoff.verdict))
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
            ledger: slot.writer_ledger().cloned(),
            prior: slot.handoff.as_ref().and_then(|h| h.audit.clone()),
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
            if let Some(id) = self.ids(WorkerStatus::Running, |s| s.spec.writes()).first() {
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
            ledger: None,
            prior: None,
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
        // The spawn gate that last refused now stands at the next open step,
        // or no longer stands: the turn cap names only what is true.
        if matches!(self.last_gate, Some((Gate::Spawn, _))) {
            self.last_gate = match self.steps.check(Step::Implement) {
                Err(StepError::Order { open, .. }) => Some((Gate::Spawn, open)),
                _ => None,
            };
        }
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
            .filter(|id| self.refuses_on_read_only(&self.slots[index(*id)].spec))
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
                    && !slot.internal
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
        // A run granted `implement` is not done before a code worker ran. A
        // spawn that was refused made no slot, so it does not count (W3).
        let ran =
            (self.slots.iter()).any(|slot| slot.spec.toolset.families().contains(Family::Write));
        if !self.gates_lifted && self.steps.grant() == Some(Grant::Implement) && !ran {
            self.last_gate = Some((Gate::Finish, Step::Implement));
            return Err(FinishError::Gate);
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
        if self.state == RunState::Open
            && !pending
            && let Some((Gate::Spawn, step)) = self.last_gate
        {
            if self.turns >= self.limits.turns {
                return NextTurn::Capped(self.turns);
            }
            self.turns += 1;
            return NextTurn::Wait(step);
        }
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
            let row = Row::new(id, slot.spec.task, slot.status, slot.handoff.as_ref());
            let row = if slot.internal { row.internal() } else { row };
            row.flagged(self.audit(id).map(|a| a.flags()).unwrap_or_default())
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
            let (task, prompt) = (slot.spec.task, slot.spec.prompt.clone());
            let (spec, refusal) = self.resolve(task, &prompt, slot.spec.brief.clone());
            self.slots[index(worker)].spec = spec;
            // The queued workers refused with it are one failure, said once.
            refusal.map(|reason| match others {
                0 => reason,
                n => format!("{} workers refused: {reason}", n + 1),
            })
        } else {
            // A retry is a new process: the spec is capped by the menu as it
            // is now, and a writer that lost Write is refused, not run bare.
            let strip = self.strips(&self.slots[index(worker)].spec);
            let ceiling = self.ceiling;
            let spec = &mut self.slots[index(worker)].spec;
            spec.families = Self::capped_families(spec.toolset, ceiling, strip);
            (!strip && spec.toolset.families().contains(Family::Write) && !spec.writes())
                .then(|| WRITE_OFF.to_owned())
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
            let (spec, refusal) =
                self.resolve(task, &slot.spec.prompt.clone(), slot.spec.brief.clone());
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

impl Slot {
    /// The ledger of a worker whose role can write; the claim check needs it.
    fn writer_ledger(&self) -> Option<&ChangeLedger> {
        (self.spec.toolset.families().contains(Family::Write)).then_some(&self.ledger)
    }

    /// The worker ended `done` with a write tool offered and nothing staged,
    /// and its answer holds a call to a write tool written as text.
    fn said_calls_as_text(&self) -> bool {
        self.status == WorkerStatus::Done
            && !self.no_tools
            && self.spec.writes()
            && self.ledger.changes().is_empty()
            && self.result.as_ref().is_some_and(|result| {
                textcall::mentions_call(&result.text, &textcall::write_tool_names())
            })
    }
}

/// A sum as it is shown: with no cost when it is partial, because a total
/// that misses a report must not read as the whole.
fn shown(usage: Usage, partial: bool) -> Usage {
    Usage {
        cost_micros: usage.cost_micros.filter(|_| !partial),
        ..usage
    }
}

/// Worker ids start at 1; id 0 maps to no slot.
fn index(id: WorkerId) -> usize {
    (id.0 as usize).wrapping_sub(1)
}
