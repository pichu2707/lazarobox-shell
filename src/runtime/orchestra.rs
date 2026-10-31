//! The run manager: the runtime side of the orchestrator (D1). It holds the
//! run of each orchestrating pane, starts its workers as single engine turns
//! and answers the orchestrator's calls through the handle
//! [`RunManager::start`] returns.
//!
//! A worker turn is a chat turn apart from its prompt and folder: the chat's
//! [`Runner`] (adapter flags, jail, environment scrub, timeouts), a fresh
//! [`Pipe`] (sanitizer, answer cap, error texts), the system prompt of its
//! kind and a tool session keyed by worker that offers its toolset only (D7).
//! Its tool rows and asks go to the orchestrator turn's channel and approver
//! (D4). Nothing falls back to another engine: an unavailable one refuses the
//! worker, which halts the run (D5, D9).
//!
//! What the app hears goes on its own channel, `(PaneId, OrchestraEvent)`,
//! because a worker can outlive a turn. The log carries ids, kinds, statuses
//! and error classes, never a prompt, an answer, an error text or a path.

use std::{
    collections::HashMap,
    fmt,
    io::Read,
    panic::AssertUnwindSafe,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures::{
    FutureExt,
    future::{BoxFuture, join_all},
};
use tokio::{
    sync::{Notify, mpsc},
    task::JoinHandle,
    time::Instant,
};

use super::{
    chat::{self, Pipe, Runner, TURN_CHANNEL},
    mcp::{Dispatch, Hub, Outcome, external::Slot},
    prompts::load_prompt,
    tools::{
        Approver, OrchHandle, Orchestra, Pending, ToolHost, TurnScope, WorkerScope, fs::load_roots,
    },
};
use crate::core::{
    ai::{
        chat::{
            INTERRUPTED,
            adapters::{Adapter, adapter_for},
            approval::{ApprovalAsk, worker_label},
            engine::{
                Answer, CallId, ChatError, ChatMessage, ChatRequest, MCP_VERIFIED, Role,
                StreamEvent, Tools, locality, tools_for,
            },
            guidance::guidance,
            jail_error,
        },
        config::{Profile, Task},
        jail,
        orchestra::{
            RunId, WorkerId, WorkerKey,
            changes::Change,
            errors::classify,
            ledger::{FinishError, Handoff, HandoffError, Judgement, Unit},
            run::{
                CancelOutcome, CollectError, NextTurn, NotHalted, Refusal, Roster, Run, RunLimits,
                RunState, UserChoice, WorkerEvent,
            },
            scope::{Dropped, ReadScope},
            steps::{Mark, Step, StepError},
            tools::OrchCall,
            worker::{ErrorClass, WorkerSpec, WorkerStatus},
        },
        tools::{
            label,
            paths::SecretRoots,
            registry::Family,
            stage::StageKey,
            wiring::{McpWire, Secret},
        },
    },
    layout::PaneId,
};

pub use crate::core::ai::orchestra::event::{Continuation, OrchestraEvent, WorkerRow};

/// What a call of a run that ended, or was replaced, answers.
const ENDED: &str = "run ended";

/// What opens a continuation turn's message; the ledger follows, wrapped.
const CONTINUE_HEAD: &str = "Your previous turn ended while the run is still open. Below is the run ledger: \
     the goal and one row per worker. Collect finished workers, write a `handoff` for each, \
     and call `finish` when the goal is met.";

/// Where a worker's tool rows and asks go. A worker outlives the orchestrator
/// turn that spawned it (S4 continuations), so its channel and approver are
/// looked up when it acts, not captured at spawn. They change together, under
/// one lock.
struct Route {
    key: WorkerKey,
    /// The count the worker's own turn watches for its timers: the one it was
    /// started with, whatever turn it has outlived since.
    own: Pending,
    state: Mutex<Current>,
}

struct Current {
    events: mpsc::Sender<StreamEvent>,
    approver: Arc<dyn Approver>,
    /// The current orchestrator turn's count, which its approver feeds.
    pending: Pending,
    /// Counts the retargets, so a failed send can tell whether the route moved.
    generation: u64,
}

impl Route {
    fn new(
        key: WorkerKey,
        events: mpsc::Sender<StreamEvent>,
        approver: Arc<dyn Approver>,
        pending: Pending,
    ) -> Arc<Self> {
        Arc::new(Self {
            key,
            own: pending.clone(),
            state: Mutex::new(Current {
                events,
                approver,
                pending,
                generation: 0,
            }),
        })
    }

    /// Points the worker at a new turn. Its asks in flight under the old turn
    /// are denied first (the worker may ask again): their wait sits under a
    /// turn the new prompt would not name. Then the new turn's approver of
    /// this worker takes over, a new attempt of its session.
    fn retarget(
        &self,
        events: mpsc::Sender<StreamEvent>,
        turn: &Arc<dyn Approver>,
        pending: &Pending,
    ) {
        let mut state = lock(&self.state);
        state.approver.end_worker(self.key);
        state.approver = turn.for_worker(self.key).unwrap_or_else(|| turn.clone());
        state.events = events;
        state.pending = pending.clone();
        state.generation += 1;
    }

    /// A channel for the worker's scope whose events reach the current turn's.
    /// It lives as long as the worker's session: a failed send never stops it.
    fn forward(route: &Arc<Self>) -> mpsc::Sender<StreamEvent> {
        let (tx, mut rx) = mpsc::channel(TURN_CHANNEL);
        let route = route.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                route.deliver(event).await;
            }
        });
        tx
    }

    /// Sends `event` to the current turn. A send that fails is retried once
    /// on the new turn when the route moved meanwhile (a plain row only: an
    /// ask in flight was denied by the move). Between turns nobody can see an
    /// ask, so its announcement ends the worker's session (denying the wait)
    /// until the next turn attaches; a plain row is dropped.
    async fn deliver(&self, event: StreamEvent) {
        let (to, seen) = {
            let state = lock(&self.state);
            (state.events.clone(), state.generation)
        };
        let asks = matches!(
            &event,
            StreamEvent::ToolCall {
                approval: Some(_),
                ..
            }
        );
        let retry = if to.send(event.clone()).await.is_ok() {
            return;
        } else {
            let state = lock(&self.state);
            if state.generation == seen {
                if asks {
                    state.approver.end_worker(self.key);
                }
                return;
            }
            state.events.clone()
        };
        if !asks {
            let _ = retry.send(event).await;
        }
    }
}

/// The approver of a worker: whatever its route points at now.
struct Rerouted(Arc<Route>);

impl Approver for Rerouted {
    fn ask(&self, id: CallId, ask: &ApprovalAsk) -> BoxFuture<'static, Answer> {
        // The turn's approver counts the ask for the turn's timers; the
        // worker's own timers, if they watch another count, hear it too.
        let (current, also) = {
            let state = lock(&self.0.state);
            let also = (!state.pending.same(&self.0.own)).then(|| self.0.own.enter());
            (state.approver.clone(), also)
        };
        let wait = current.ask(id, ask);
        async move {
            let _also = also;
            wait.await
        }
        .boxed()
    }

    fn is_over(&self) -> bool {
        lock(&self.0.state).approver.is_over()
    }
}

/// What follows an orchestrator turn that ended.
enum Next {
    Settle,
    Go(OrchestraEvent),
    Stop(String),
}

/// What `collect` answers when no nonce can be drawn: unwrapped text never
/// goes out.
const NO_NONCE: &str = "the result cannot be wrapped now";

/// What an orchestrator turn hands its run when it starts. Nothing is asked:
/// the run starts at once (agent-orchestrator, Read scope of a run).
pub struct OrchScope {
    /// The availability of every task, from the menu and the run's override.
    pub roster: Roster,
    /// The chat's origin: what a worker's relative path is read against.
    pub origin: PathBuf,
    /// The read-scope roots, as typed: the chat origin by default, never
    /// model or worker text. Each one passes the path policy or is dropped.
    pub roots: Vec<String>,
    /// What the user asked, verbatim: the ledger of a continuation turn
    /// carries it (S4). Unused when the scope attaches to a run.
    pub goal: String,
    /// Each worker's deadline (`worker_timeout_secs`).
    pub deadline: Duration,
    pub limits: RunLimits,
    pub profile: Profile,
    /// The orchestrator turn's channel and approver: a worker's tool rows and
    /// asks go there (D4), and `pending` lets that turn's timers see a
    /// worker's ask waiting.
    pub events: mpsc::Sender<StreamEvent>,
    pub approver: Arc<dyn Approver>,
    pub pending: Pending,
}

/// Where the run manager's events go.
pub type Events = mpsc::UnboundedSender<(PaneId, OrchestraEvent)>;

/// One line of the run manager's log. Made of ids, kinds, statuses and error
/// classes only, so no prompt, answer, error text or path can reach a log.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LogLine {
    pub run: RunId,
    pub worker: WorkerId,
    pub kind: Task,
    pub status: WorkerStatus,
    pub class: Option<ErrorClass>,
}

impl fmt::Display for LogLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (run, worker) = (self.run.0, self.worker.0);
        write!(f, "run {run} worker {worker} {}", self.kind.id())?;
        write!(f, " {}", self.status.label())?;
        match self.class {
            Some(class) => write!(f, " {}", class.id()),
            None => Ok(()),
        }
    }
}

/// Where the log lines go.
pub type Log = Arc<dyn Fn(LogLine) + Send + Sync>;

/// Hands out run ids: microseconds of the wall clock, always above the last
/// one. A run id names sockets and, later, handoff keys and stored runs, so
/// it is never reused, not even by a later process: the clock has moved on.
#[derive(Default)]
struct RunIds {
    last: u64,
}

impl RunIds {
    fn next(&mut self, now_micros: u64) -> RunId {
        self.last = now_micros.max(self.last.saturating_add(1));
        RunId(self.last)
    }
}

fn clock_micros() -> u64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH);
    since.map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
}

/// The run of one pane, with what its workers' turns need.
struct Live {
    run: Run,
    /// The user's goal, verbatim: the ledger of every continuation holds it.
    goal: String,
    scope: ReadScope,
    /// The roots the scope refused, sanitized, with why.
    dropped: Vec<Dropped>,
    origin: PathBuf,
    profile: Profile,
    events: mpsc::Sender<StreamEvent>,
    approver: Arc<dyn Approver>,
    pending: Pending,
    /// Where each running worker's tool rows and asks go: the run's current
    /// orchestrator turn, retargeted by [`RunManager::attach`].
    routes: HashMap<WorkerId, Arc<Route>>,
    /// The turn of each worker that runs. Aborting one drops its adapter,
    /// which kills its process group.
    tasks: HashMap<WorkerId, JoinHandle<()>>,
    /// The status each worker's row last showed.
    shown: HashMap<WorkerId, WorkerStatus>,
    /// The failure the last alert showed.
    alerted: Option<WorkerId>,
    /// The error of a retried worker's failed attempt, which its next prompt
    /// is followed by (wrapped).
    retry: HashMap<WorkerId, String>,
    /// The orchestrator turn that started the run is over: the run ends once
    /// it has nothing left to do (see [`RunManager::turn_ended`]).
    turn_over: bool,
    /// Ends the run when its wall clock passes (`run_timeout_mins`).
    clock: Option<JoinHandle<()>>,
}

impl Drop for Live {
    /// The backstop of a run dropped without being ended, its manager's
    /// included: its worker turns would otherwise run on to their deadline.
    fn drop(&mut self) {
        for task in self.tasks.values().chain(&self.clock) {
            task.abort();
        }
    }
}

/// What worker turns need to offer tools: the host that runs the calls and
/// the hub of per-worker sockets, made when the first CLI worker starts.
struct Tooling {
    host: Arc<ToolHost>,
    dirs: Vec<PathBuf>,
    hub: Mutex<Option<Hub<WorkerKey>>>,
}

/// What the alert says when a retry cannot be sealed.
const NO_SEAL: &str = "could not seal the previous error";

/// Why a decision about a failure was not applied.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecideError {
    /// No failure waits for the user.
    NotHalted,
    /// The run the answer was for has ended or been replaced.
    Moved,
    /// A retry needs a nonce to wrap the failure and none could be drawn.
    NoNonce,
}

impl From<NotHalted> for DecideError {
    fn from(_: NotHalted) -> Self {
        Self::NotHalted
    }
}

/// The runs of every orchestrating pane. Shared as an `Arc`: the handles it
/// gives out and its workers' turns hold it weakly.
pub struct RunManager {
    runner: Runner,
    /// The config folder the prompt overrides are read from; `None` reads
    /// none.
    prompts: Option<PathBuf>,
    events: Events,
    tooling: Option<Tooling>,
    log: Option<Log>,
    /// Whether a read-scope root is a broad scratch folder (`jail::is_scratch`).
    scratch: fn(&Path) -> bool,
    /// Where a nonce's bytes come from: the OS CSPRNG (a test swaps it).
    nonce: fn() -> Option<[u8; 8]>,
    ids: Mutex<RunIds>,
    runs: Mutex<HashMap<PaneId, Live>>,
    /// Set by `shutdown`, under the `runs` lock: no run starts after it.
    closed: AtomicBool,
    /// Woken whenever a worker's status changes, for `wait`.
    changed: Notify,
}

impl RunManager {
    pub fn new(runner: Runner, prompts: Option<PathBuf>, events: Events) -> Self {
        Self {
            runner,
            prompts,
            events,
            tooling: None,
            log: None,
            scratch: jail::is_scratch,
            nonce: nonce_bytes,
            ids: Mutex::default(),
            runs: Mutex::default(),
            closed: AtomicBool::new(false),
            changed: Notify::new(),
        }
    }

    /// Lets workers run their tools through `host`, with session sockets in
    /// the first of `dirs` that is a private folder of ours. Without it a
    /// worker runs as text and its run's read scope grants nothing.
    pub fn with_tools(mut self, host: Arc<ToolHost>, dirs: Vec<PathBuf>) -> Self {
        self.tooling = Some(Tooling {
            host,
            dirs,
            hub: Mutex::new(None),
        });
        self
    }

    /// Judges broad scratch folders with `scratch` instead of the jail's:
    /// tests build their homes under `/tmp`, which the jail counts as one.
    #[cfg(test)]
    pub(crate) fn with_scratch(mut self, scratch: fn(&Path) -> bool) -> Self {
        self.scratch = scratch;
        self
    }

    /// Draws nonce bytes with `nonce`, so a test can make the draw fail.
    #[cfg(test)]
    pub(crate) fn with_nonce(mut self, nonce: fn() -> Option<[u8; 8]>) -> Self {
        self.nonce = nonce;
        self
    }

    /// Sends a line to `log` for every change of a worker's status.
    pub fn with_log(mut self, log: Log) -> Self {
        self.log = Some(log);
        self
    }

    /// Starts the run of an orchestrator turn of `pane`, replacing any run
    /// the pane had, which ends as [`RunManager::cancel`] ends it. Its handle
    /// is what the turn's host (`TurnScope::orchestra`) calls. The turn itself
    /// runs on `orchestrate`: when that engine is unusable no run starts, no
    /// other engine is tried, and the error is the engine guidance. After
    /// `shutdown` no run starts.
    pub fn start(
        self: &Arc<Self>,
        pane: PaneId,
        scope: OrchScope,
    ) -> Result<(RunId, OrchHandle), String> {
        if let Some(reason) = guidance(scope.roster.get(Task::Orchestrate)) {
            return Err(reason);
        }
        let run = lock(&self.ids).next(clock_micros());
        let (read_scope, dropped) = self.read_scope(run, &scope.origin, &scope.roots);
        let live = Live {
            scope: read_scope,
            dropped,
            goal: scope.goal,
            run: Run::new(run, scope.roster, scope.deadline).with_limits(scope.limits),
            origin: scope.origin,
            profile: scope.profile,
            events: scope.events,
            approver: scope.approver,
            pending: scope.pending,
            routes: HashMap::new(),
            tasks: HashMap::new(),
            shown: HashMap::new(),
            alerted: None,
            retry: HashMap::new(),
            turn_over: false,
            clock: self.start_clock(pane, run, scope.limits.wall),
        };
        let old = {
            let mut runs = lock(&self.runs);
            if self.closed.load(Ordering::SeqCst) {
                return Err(ENDED.to_owned());
            }
            runs.insert(pane, live)
        };
        if let Some(old) = old {
            self.end(pane, old);
        }
        Ok((run, self.handle(pane, run)))
    }

    /// Starts the continuation turn of run `run` of `pane` (S4, D8): the new
    /// turn's channel, approver and pending count replace the old turn's for
    /// the workers spawned from now on, and the run, its workers, roster and
    /// limits stay as they are. `Err` when the run ended or was replaced.
    pub fn attach(
        self: &Arc<Self>,
        pane: PaneId,
        run: RunId,
        scope: OrchScope,
    ) -> Result<(RunId, OrchHandle), String> {
        let attached = self.on_run(pane, run, |live| {
            // The workers that survive into this turn ask and report through
            // it: the earlier turn's channel and approver are dead.
            // A worker that is over keeps its session closed: only the ones
            // still queued or running are pointed at the new turn.
            let run = &live.run;
            live.routes.retain(|id, _| {
                matches!(
                    run.status(*id),
                    Some(WorkerStatus::Queued | WorkerStatus::Running)
                )
            });
            for route in live.routes.values() {
                route.retarget(scope.events.clone(), &scope.approver, &scope.pending);
            }
            live.events = scope.events;
            live.approver = scope.approver;
            live.pending = scope.pending;
            live.turn_over = false;
        });
        attached.ok_or_else(|| ENDED.to_owned())?;
        Ok((run, self.handle(pane, run)))
    }

    /// The timer of a run's wall clock; none for a run with no limit.
    fn start_clock(
        self: &Arc<Self>,
        pane: PaneId,
        run: RunId,
        wall: Duration,
    ) -> Option<JoinHandle<()>> {
        Instant::now().checked_add(wall)?;
        let manager = Arc::downgrade(self);
        Some(tokio::spawn(async move {
            tokio::time::sleep(wall).await;
            if let Some(manager) = manager.upgrade() {
                manager.expire(pane, run, wall);
            }
        }))
    }

    /// The wall clock of run `run` of `pane` passed: the run ends as a
    /// cancel ends it (its workers are killed), after `Expired` names why.
    /// A run that ended or was replaced ignores it.
    fn expire(self: &Arc<Self>, pane: PaneId, run: RunId, after: Duration) {
        let live = {
            let mut runs = lock(&self.runs);
            if runs.get(&pane).is_none_or(|live| live.run.id() != run) {
                return;
            }
            runs.remove(&pane)
        };
        if let Some(live) = live {
            self.send(pane, OrchestraEvent::Expired { run, after });
            self.end(pane, live);
        }
    }

    fn handle(self: &Arc<Self>, pane: PaneId, run: RunId) -> OrchHandle {
        Arc::new(Bound {
            manager: Arc::downgrade(self),
            pane,
            run,
        })
    }

    /// The read scope of `run` over `roots`, with the roots it dropped; it
    /// grants nothing without tooling or when the secret roots cannot be
    /// built.
    fn read_scope(&self, run: RunId, origin: &Path, roots: &[String]) -> (ReadScope, Vec<Dropped>) {
        let Some(tooling) = &self.tooling else {
            return (ReadScope::denied(run), Vec::new());
        };
        let Ok(secrets) = load_roots(&tooling.host.env(origin)) else {
            return (ReadScope::denied(run), Vec::new());
        };
        let inputs: Vec<&str> = roots.iter().map(String::as_str).collect();
        ReadScope::new(run, &secrets, origin, &inputs, &self.scratch)
    }

    /// The read scope of the run of `pane`, for display: the roots its reads
    /// run unasked under, and the roots it dropped with why. `None` without a
    /// run.
    pub fn scope(&self, pane: PaneId) -> Option<(Vec<PathBuf>, Vec<Dropped>)> {
        let runs = lock(&self.runs);
        let live = runs.get(&pane)?;
        Some((live.scope.roots(), live.dropped.clone()))
    }

    /// Ctrl+C on `pane`: ends its run, if it has one. Returns at once.
    pub fn cancel(self: &Arc<Self>, pane: PaneId) {
        let live = lock(&self.runs).remove(&pane);
        if let Some(live) = live {
            self.end(pane, live);
        }
    }

    /// The orchestrator turn of run `run` of `pane` ended. With work pending
    /// (a worker live, or a finished one with no handoff) the run continues
    /// in a new turn, or stops at `max_turns` (S4, D8). Otherwise it ends by
    /// itself, with its `Ended`, once none of its workers is queued or
    /// running and no failure waits for the user: at once, or when its last
    /// worker finishes (user decision, 2026-10-29). A run that ended or was
    /// replaced ignores it.
    pub fn turn_ended(self: &Arc<Self>, pane: PaneId, run: RunId) {
        let next = self.on_run(pane, run, |live| {
            let next = self.next_turn(live);
            // A run that continues waits for the turn the app starts: it must
            // not end under it when its last worker finishes meanwhile.
            live.turn_over = !matches!(next, Next::Go(_));
            next
        });
        match next {
            Some(Next::Settle) => self.settle(pane, run),
            Some(Next::Go(event)) => self.send(pane, event),
            Some(Next::Stop(reason)) => {
                let live = lock(&self.runs).remove(&pane);
                if let Some(live) = live {
                    self.send(pane, OrchestraEvent::Stopped { run, reason });
                    self.end(pane, live);
                }
            }
            None => {}
        }
    }

    /// What follows the orchestrator turn of `live` (D8): the continuation
    /// built from the run ledger, wrapped as data under a nonce drawn for it
    /// (never a worker transcript), the stop at `max_turns`, or nothing.
    fn next_turn(&self, live: &mut Live) -> Next {
        let max = live.run.limits().turns;
        match live.run.next_turn() {
            NextTurn::Settle => Next::Settle,
            NextTurn::Capped(taken) => Next::Stop(format!(
                "run stopped: it took max_turns ({taken}) orchestrator turns; running workers were cancelled"
            )),
            NextTurn::Continue(turn) => match (self.nonce)() {
                Some(bytes) => {
                    let ledger = live.run.run_ledger(&live.goal).render();
                    let text = label::wrap(&label::nonce_from(bytes), &ledger);
                    let run = live.run.id();
                    Next::Go(OrchestraEvent::Continue(Continuation {
                        run,
                        turn,
                        max,
                        text: format!("{CONTINUE_HEAD}\n\n{text}"),
                    }))
                }
                None => Next::Stop(format!(
                    "run stopped: the ledger cannot go to the orchestrator ({NO_NONCE})"
                )),
            },
        }
    }

    /// Ends run `run` of `pane` if its turn is over and it has nothing left
    /// to do: no worker queued or running, no failure waiting for the user.
    fn settle(self: &Arc<Self>, pane: PaneId, run: RunId) {
        let done = {
            let mut runs = lock(&self.runs);
            let over = runs.get(&pane).is_some_and(|live| {
                live.run.id() == run
                    && live.turn_over
                    && !matches!(live.run.state(), RunState::Halted { .. })
                    && (1..)
                        .map_while(|n| live.run.status(WorkerId(n)))
                        .all(|status| {
                            !matches!(status, WorkerStatus::Queued | WorkerStatus::Running)
                        })
            });
            if over { runs.remove(&pane) } else { None }
        };
        if let Some(live) = done {
            self.end(pane, live);
        }
    }

    /// The user's answer to the failure that halted the run of `pane`.
    /// `Retry` relaunches the failed worker on its spec, its prompt followed
    /// by the failure's text wrapped as untrusted data; `Cancel` ends the run.
    /// `NotHalted` when no failure waits for the user, and `NoNonce` when a
    /// retry cannot wrap the failure for the model (as `collect`, never the
    /// plain prompt); nothing changes in either case.
    pub fn decide(
        self: &Arc<Self>,
        pane: PaneId,
        run: RunId,
        choice: UserChoice,
    ) -> Result<(), DecideError> {
        let mut runs = lock(&self.runs);
        // An answer is for the run it was given for; one that finds it ended
        // or replaced is dropped, and said.
        let Some(live) = runs.get_mut(&pane).filter(|live| live.run.id() == run) else {
            self.send(pane, OrchestraEvent::Dropped { run });
            return Err(DecideError::Moved);
        };
        let RunState::Halted {
            worker,
            class,
            text,
        } = live.run.state()
        else {
            self.send(pane, OrchestraEvent::Dropped { run });
            return Err(NotHalted.into());
        };
        let (worker, class, text) = (*worker, *class, text.clone());
        let retry = choice == UserChoice::Retry;
        // Drawn before the run changes, so a failed draw leaves it halted.
        let wrapped = if retry {
            let Some(bytes) = (self.nonce)() else {
                // The app dropped its alert on the key: bring it back, with why.
                let reason = NO_SEAL.to_owned();
                let declined = OrchestraEvent::Declined {
                    run,
                    worker,
                    class,
                    text,
                    reason,
                };
                self.send(pane, declined);
                return Err(DecideError::NoNonce);
            };
            Some(label::wrap(&label::nonce_from(bytes), &text))
        } else {
            None
        };
        live.run.decide(now(), choice)?;
        if *live.run.state() == RunState::Cancelled {
            let live = runs.remove(&pane);
            drop(runs);
            if let Some(live) = live {
                self.end(pane, live);
            }
            return Ok(());
        }
        match wrapped {
            Some(wrapped) => live.retry.insert(worker, wrapped),
            None => live.retry.remove(&worker),
        };
        // A failure after a decision alerts again, the same worker's too.
        live.alerted = None;
        self.sync(pane, live);
        Ok(())
    }

    /// Quitting: ends every run now (see [`RunManager::cancel`]) and starts
    /// none after. The future resolves once every worker turn is gone, its
    /// adapter dropped and so its process group killed.
    pub fn shutdown(self: &Arc<Self>) -> BoxFuture<'static, ()> {
        let ended: Vec<_> = {
            let mut runs = lock(&self.runs);
            self.closed.store(true, Ordering::SeqCst);
            runs.drain().collect()
        };
        let tasks: Vec<_> = ended
            .into_iter()
            .flat_map(|(pane, live)| self.end(pane, live))
            .collect();
        // No session is left: dropping the hub removes its sockets.
        if let Some(tooling) = &self.tooling {
            lock(&tooling.hub).take();
        }
        join_all(tasks).map(drop).boxed()
    }

    /// Ends a run already taken out of `runs`: its workers end `cancelled`,
    /// their turns are aborted (which drops their adapters and kills their
    /// process groups) and their sessions closed (which denies their asks
    /// and drops their calls in flight). Their rows go out, then `Ended`;
    /// nothing of the run follows, since no call or turn finds it any more.
    /// Returns the aborted turns, for `shutdown` to wait on.
    fn end(self: &Arc<Self>, pane: PaneId, mut live: Live) -> Vec<JoinHandle<()>> {
        let run = live.run.id();
        // A handoff the orchestrator never wrote is built here, so the ledger
        // is whole whatever ended the run (D17): the ones for workers that
        // already ended first, `blocked` while a failure waited.
        let mut built = live.run.fallbacks();
        live.run.cancel_run(now());
        built.extend(live.run.fallbacks());
        let approver = live.approver.clone();
        let tasks: Vec<_> = live
            .tasks
            .drain()
            .map(|(worker, task)| {
                task.abort();
                self.close_session(&approver, WorkerKey { run, worker });
                task
            })
            .collect();
        self.sync(pane, &mut live);
        self.announce_built(pane, run, built);
        self.send(pane, OrchestraEvent::Ended { run });
        tasks
    }

    /// Runs `f` on the run `run` of `pane`; `None` once it ended or was
    /// replaced.
    fn on_run<T>(&self, pane: PaneId, run: RunId, f: impl FnOnce(&mut Live) -> T) -> Option<T> {
        let mut runs = lock(&self.runs);
        let live = runs.get_mut(&pane).filter(|live| live.run.id() == run)?;
        Some(f(live))
    }

    /// One orchestration call of the run `run` of `pane`.
    fn call(
        self: &Arc<Self>,
        pane: PaneId,
        run: RunId,
        call: OrchCall,
    ) -> BoxFuture<'static, Outcome> {
        let now = match call {
            OrchCall::Spawn { task, prompt } => self.spawn(pane, run, task, &prompt),
            OrchCall::Wait { ids, timeout } => {
                return self.clone().wait(pane, run, ids, timeout).boxed();
            }
            OrchCall::Collect { id } => self.collect(pane, run, id),
            OrchCall::Cancel { id } => self.cancel_worker(pane, run, id),
            OrchCall::Handoff { id, note } => self.handoff(pane, run, id, note),
            OrchCall::StepHandoff { step, mark, note } => {
                self.handoff_step(pane, run, step, mark, note)
            }
            OrchCall::Finish { summary } => self.finish_run(pane, run, &summary),
        };
        async move { now }.boxed()
    }

    /// Creates a worker: it starts when a slot is free, or ends `refused`
    /// when its engine is unavailable, which halts the run.
    fn spawn(self: &Arc<Self>, pane: PaneId, run: RunId, task: Task, prompt: &str) -> Outcome {
        let spawned = self.on_run(pane, run, |live| {
            let id = match live.run.spawn(now(), task, prompt) {
                Ok(id) => id,
                Err(Refusal::Paused) => return error("run paused"),
                Err(Refusal::Ended) => return error(ENDED),
                Err(Refusal::ReadOnly) => {
                    return error("code workers are not spawned for a read-only goal");
                }
                Err(Refusal::WorkerCap(cap)) => {
                    return error(format!("worker limit reached: max_workers is {cap}"));
                }
            };
            self.sync(pane, live);
            let status = live.run.status(id).unwrap_or(WorkerStatus::Refused);
            Outcome {
                text: format!("worker {}: {}", id.0, status.label()),
                is_error: status == WorkerStatus::Refused,
            }
        });
        spawned.unwrap_or_else(|| error(ENDED))
    }

    /// The status of each id once none of them is queued or running, the run
    /// halts, or `timeout` seconds pass.
    async fn wait(
        self: Arc<Self>,
        pane: PaneId,
        run: RunId,
        ids: Vec<WorkerId>,
        timeout: u16,
    ) -> Outcome {
        let until = Instant::now() + Duration::from_secs(u64::from(timeout));
        loop {
            // Made before the check: a change between the check and the
            // sleep still wakes it.
            let changed = self.changed.notified();
            let report = self.on_run(pane, run, |live| {
                let report = live.run.wait_report(&ids);
                let settled = report
                    .iter()
                    .all(|(_, label)| !matches!(*label, "queued" | "running"));
                let lines: Vec<_> = report
                    .iter()
                    .map(|(id, label)| format!("{}: {label}", id.0))
                    .collect();
                (settled, lines.join("\n"))
            });
            let Some((settled, text)) = report else {
                return error(ENDED);
            };
            if settled || Instant::now() >= until {
                return Outcome {
                    text,
                    is_error: false,
                };
            }
            tokio::select! {
                () = changed => {}
                () = tokio::time::sleep_until(until) => {}
            }
        }
    }

    /// The final text of a finished worker, wrapped as untrusted data under
    /// a nonce drawn for this call alone. A worker that ended otherwise
    /// hands over no text: its error stays on the screen.
    fn collect(&self, pane: PaneId, run: RunId, id: WorkerId) -> Outcome {
        let collected = self.on_run(pane, run, |live| {
            let Some(bytes) = (self.nonce)() else {
                return error(NO_NONCE);
            };
            match live.run.collect_wrapped(bytes, id) {
                Ok(text) => Outcome {
                    text,
                    is_error: false,
                },
                Err(CollectError::Unknown) => error(format!("no worker {}", id.0)),
                Err(CollectError::NotFinished) => error(format!("worker {} is not finished", id.0)),
                Err(CollectError::Ended(status)) => error(format!(
                    "worker {} ended {} with no text",
                    id.0,
                    status.label()
                )),
            }
        });
        collected.unwrap_or_else(|| error(ENDED))
    }

    /// Stores the orchestrator's handoff for worker `id` and tells the app.
    /// The ack says what was stored. The run keeps the handoff in memory; the
    /// app stores it in engram on hearing the event.
    fn handoff(&self, pane: PaneId, run: RunId, id: WorkerId, note: Judgement) -> Outcome {
        let stored = self.on_run(pane, run, |live| {
            let forced = live.run.forced_by();
            match live.run.handoff(id, note) {
                Ok(handoff) => {
                    let text = match forced {
                        Some(failed) => format!(
                            "handoff {}: {} (forced: worker {} failed)",
                            id.0,
                            handoff.verdict.label(),
                            failed.0
                        ),
                        None => format!("handoff {}: {}", id.0, handoff.verdict.label()),
                    };
                    self.announce_handoff(pane, run, handoff);
                    fine(text)
                }
                Err(HandoffError::Unknown) => error(format!("no worker {}", id.0)),
                Err(HandoffError::NotFinished) => error(format!("worker {} is not finished", id.0)),
                Err(HandoffError::Ended) => error(ENDED),
            }
        });
        stored.unwrap_or_else(|| error(ENDED))
    }

    /// Stores the orchestrator's handoff for an ODD step and closes the step;
    /// refused out of order. The app stores it like a worker's.
    fn handoff_step(
        &self,
        pane: PaneId,
        run: RunId,
        step: Step,
        mark: Mark,
        note: Judgement,
    ) -> Outcome {
        let stored = self.on_run(pane, run, |live| {
            match live.run.handoff_step(step, mark, note) {
                Ok(handoff) => {
                    let text = format!("step {}: {}", step.label(), handoff.verdict.label());
                    self.announce_handoff(pane, run, handoff);
                    fine(text)
                }
                Err(StepError::Ended) => error(ENDED),
                Err(StepError::Order { step, open }) => error(format!(
                    "step {} needs {} closed first",
                    step.label(),
                    open.label()
                )),
                Err(StepError::NotLarge) => error("track is for a large goal only"),
                Err(StepError::NoHandoff(step)) => {
                    error(format!("step {} has no handoff", step.label()))
                }
            }
        });
        stored.unwrap_or_else(|| error(ENDED))
    }

    /// Ends the run for the orchestrator: refused with a reason while a
    /// worker is live, a failure waits or handoffs are missing (once); then
    /// lazarobox builds the missing handoffs and the run is `Finished`. The
    /// run leaves the manager when the turn ends, as ever.
    fn finish_run(&self, pane: PaneId, run: RunId, summary: &str) -> Outcome {
        let finished = self.on_run(pane, run, |live| match live.run.finish(summary) {
            Ok(built) => {
                self.announce_built(pane, run, built);
                let ledger = live.run.run_ledger(&live.goal).render();
                let summary = live.run.summary().unwrap_or(summary).to_owned();
                self.send(
                    pane,
                    OrchestraEvent::Closed {
                        run,
                        ledger,
                        summary,
                    },
                );
                fine("run finished")
            }
            Err(FinishError::Ended) => error(ENDED),
            Err(FinishError::Paused) => error("run paused"),
            Err(FinishError::Live(ids)) => {
                error(format!("workers not finished: {}", ids_text(&ids)))
            }
            Err(FinishError::Missing(ids)) => {
                error(format!("handoffs missing: {}", ids_text(&ids)))
            }
        });
        finished.unwrap_or_else(|| error(ENDED))
    }

    /// The handoffs lazarobox built: the app hears each, then which workers
    /// they were for (never a silent fallback).
    fn announce_built(&self, pane: PaneId, run: RunId, built: Vec<Handoff>) {
        let workers: Vec<_> = built
            .iter()
            .filter_map(|handoff| match handoff.unit {
                Unit::Worker(worker) => Some(worker),
                Unit::Step(_) => None,
            })
            .collect();
        for handoff in built {
            self.announce_handoff(pane, run, handoff);
        }
        if !workers.is_empty() {
            self.send(pane, OrchestraEvent::Filled { run, workers });
        }
    }

    /// A handoff exists: the app hears it and stores it (engram, or the
    /// degraded mode).
    fn announce_handoff(&self, pane: PaneId, run: RunId, handoff: Handoff) {
        self.send(
            pane,
            OrchestraEvent::Handoff {
                run,
                handoff: Box::new(handoff),
            },
        );
    }

    /// Cancels one worker: its turn is aborted and its session closed, and
    /// its slot goes to the next queued worker. One that already ended stays
    /// as it was.
    pub fn cancel_worker(self: &Arc<Self>, pane: PaneId, run: RunId, id: WorkerId) -> Outcome {
        let cancelled = self.on_run(pane, run, |live| match live.run.cancel(now(), id) {
            CancelOutcome::Cancelled => {
                live.routes.remove(&id);
                if let Some(task) = live.tasks.remove(&id) {
                    task.abort();
                }
                self.close_session(&live.approver, WorkerKey { run, worker: id });
                self.sync(pane, live);
                fine(format!("worker {}: cancelled", id.0))
            }
            CancelOutcome::AlreadyEnded => {
                let status = live.run.status(id).map_or("", WorkerStatus::label);
                fine(format!("worker {} already ended: {status}", id.0))
            }
            CancelOutcome::Unknown => error(format!("no worker {}", id.0)),
        });
        cancelled.unwrap_or_else(|| error(ENDED))
    }

    /// Brings the app and the processes in line with the run after a change:
    /// a row (and a log line) for each worker whose status moved, a turn for
    /// each worker that now runs, the alert of a new failure, and a wake-up
    /// for the waits.
    fn sync(self: &Arc<Self>, pane: PaneId, live: &mut Live) {
        let run = live.run.id();
        for n in 1.. {
            let id = WorkerId(n);
            let Some(status) = live.run.status(id) else {
                break;
            };
            if live.shown.insert(id, status) != Some(status) {
                self.announce(pane, live, id, status);
            }
            if status == WorkerStatus::Running && !live.tasks.contains_key(&id) {
                self.launch(pane, live, id);
            }
        }
        match live.run.state() {
            RunState::Halted {
                worker,
                class,
                text,
            } if live.alerted != Some(*worker) => {
                let halted = OrchestraEvent::Halted {
                    run,
                    worker: *worker,
                    class: *class,
                    text: text.clone(),
                };
                live.alerted = Some(*worker);
                self.send(pane, halted);
            }
            RunState::Halted { .. } => {}
            _ => live.alerted = None,
        }
        self.changed.notify_waiters();
    }

    fn announce(&self, pane: PaneId, live: &Live, id: WorkerId, status: WorkerStatus) {
        let (Some(spec), result) = (live.run.spec(id), live.run.result(id)) else {
            return;
        };
        let run = live.run.id();
        if let Some(log) = &self.log {
            log(LogLine {
                run,
                worker: id,
                kind: spec.task,
                status,
                class: result
                    .and_then(|r| r.error.as_ref())
                    .map(|(class, _)| *class),
            });
        }
        let row = WorkerRow {
            run,
            id,
            kind: spec.task,
            engine: spec.engine.clone(),
            model: spec.model.clone(),
            status,
            elapsed: result.map(|r| r.elapsed),
            usage: live.run.usage(id),
            partial: live.run.usage_partial(id),
            overridden: false,
        };
        self.send(pane, OrchestraEvent::Worker(row));
    }

    fn send(&self, pane: PaneId, event: OrchestraEvent) {
        // Nobody listens once the app is gone.
        let _ = self.events.send((pane, event));
    }

    /// Starts the turn of worker `id`, which the run has just set running:
    /// a chat request on its spec's engine and model, with the system prompt
    /// of its kind and the worker prompt as the one message.
    fn launch(self: &Arc<Self>, pane: PaneId, live: &mut Live, id: WorkerId) {
        let Some(spec) = live.run.spec(id).cloned() else {
            return;
        };
        let mut prompt = spec.prompt.clone();
        // Wrapped when the retry was decided: an error text never reaches a
        // model bare.
        if let Some(wrapped) = live.retry.remove(&id) {
            prompt = format!("{prompt}\n\n{wrapped}");
        }
        let mut request = ChatRequest {
            engine: spec.engine.clone(),
            model: spec.model.clone(),
            system: load_prompt(self.prompts.as_deref(), live.profile, spec.task).text,
            memory: None,
            messages: vec![ChatMessage {
                role: Role::User,
                content: prompt,
            }],
            tools: tools_for(&spec.engine, true, Some(live.origin.clone()), MCP_VERIFIED),
            families: spec.toolset.families(),
        };
        let key = WorkerKey {
            run: live.run.id(),
            worker: id,
        };
        let approver = live.approver.for_worker(key);
        let route = Route::new(
            key,
            live.events.clone(),
            approver.unwrap_or_else(|| live.approver.clone()),
            live.pending.clone(),
        );
        live.routes.insert(id, route.clone());
        let session = self
            .open_session(pane, key, &spec, &mut request, route)
            .map(|dispatch| (live.pending.clone(), dispatch));
        let turn = Turn {
            runner: self.runner.clone(),
            request,
            session,
            deadline: spec.deadline,
        };
        let manager = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            let events = turn.run().await;
            if let Some(manager) = manager.upgrade() {
                manager.finish(pane, key, events);
            }
        });
        live.tasks.insert(id, task);
    }

    /// The tool session of a worker turn, opened as a chat turn opens one
    /// (`Chats::open_session`) but keyed by worker and offering its toolset
    /// only: a server engine gets the dispatch of its tool loop, a CLI a
    /// socket of the worker hub, wired into `request`. `None` runs the turn
    /// as text.
    fn open_session(
        self: &Arc<Self>,
        pane: PaneId,
        key: WorkerKey,
        spec: &WorkerSpec,
        request: &mut ChatRequest,
        route: Arc<Route>,
    ) -> Option<Option<Arc<dyn Dispatch>>> {
        let tooling = self.tooling.as_ref()?;
        let (Tools::On { origin, mcp, .. }, Some(locality)) =
            (&mut request.tools, locality(&request.engine))
        else {
            return None;
        };
        let families = spec.toolset.families();
        // Only a chat pane may type into the user's session.
        debug_assert!(!families.contains(Family::Pane));
        let model = spec.model.as_deref().unwrap_or("default");
        let scope = TurnScope {
            events: Route::forward(&route),
            locality,
            origin: origin.clone().unwrap_or_else(|| PathBuf::from("/")),
            approver: Arc::new(Rerouted(route)),
            external: Slot::default(),
            families,
            // A `code` worker stages in its pane's batch (S7, user decision
            // "El worker propone su lote", 2026-10-29): the batch review, its
            // badge and its orphan are the pane's, as for a chat's batch.
            stage_key: families
                .contains(Family::Write)
                .then_some(StageKey::Pane(pane)),
            allowed: Default::default(),
            orchestra: None,
            read_scope: Some(WorkerScope {
                handle: self.handle(pane, key.run),
                run: key.run,
                worker: key.worker,
                label: worker_label(spec.task, &spec.engine, model),
            }),
        };
        let dispatch = tooling.host.dispatch(scope);
        if matches!(
            adapter_for(&request.engine),
            Some(Adapter::Ollama | Adapter::Llamacpp)
        ) {
            return Some(Some(dispatch));
        }
        let mut hub = lock(&tooling.hub);
        if hub.is_none() {
            *hub = Hub::new(&tooling.dirs).ok();
        }
        // The toolset's families, never `FamilySet::LEGACY`; a worker key
        // gets no user servers whatever the slot holds.
        let started = hub
            .as_mut()?
            .open(key, dispatch, Slot::default(), families)
            .ok()?;
        *mcp = Some(Box::new(McpWire {
            socket: started.socket.clone(),
            token: Secret::new(started.token.reveal()),
            unwire: started.end.clone(),
        }));
        Some(None)
    }

    /// Closes the tool session of a worker that is over and denies the asks
    /// it still has waiting, its own and no other worker's.
    fn close_session(&self, approver: &Arc<dyn Approver>, key: WorkerKey) {
        approver.end_worker(key);
        if let Some(tooling) = &self.tooling
            && let Some(hub) = lock(&tooling.hub).as_mut()
        {
            hub.close(key);
        }
    }

    /// Applies how a worker turn ended; `None` means its deadline passed. A
    /// run that ended or was replaced meanwhile ignores it.
    fn finish(self: &Arc<Self>, pane: PaneId, key: WorkerKey, events: Option<Vec<WorkerEvent>>) {
        self.on_run(pane, key.run, |live| {
            self.close_session(&live.approver, key);
            live.tasks.remove(&key.worker);
            live.routes.remove(&key.worker);
            match events {
                Some(events) => {
                    for event in events {
                        live.run.on_event(now(), key.worker, event);
                    }
                }
                None => {
                    live.run.on_tick(now());
                }
            }
            self.sync(pane, live);
        });
        self.settle(pane, key.run);
    }
}

/// The run of one orchestrator turn, as its host and its workers' hosts see
/// it.
struct Bound {
    manager: Weak<RunManager>,
    pane: PaneId,
    run: RunId,
}

impl Orchestra for Bound {
    fn call(&self, call: OrchCall) -> BoxFuture<'static, Outcome> {
        match self.manager.upgrade() {
            Some(manager) => manager.call(self.pane, self.run, call),
            None => async { error(ENDED) }.boxed(),
        }
    }

    fn allows(
        &self,
        run: RunId,
        worker: WorkerId,
        secrets: &SecretRoots,
        origin: &Path,
        input: &str,
    ) -> Option<PathBuf> {
        // Only this handle's run has a scope here: a run of an earlier
        // process, resumed, asks the user again.
        if run != self.run {
            return None;
        }
        let manager = self.manager.upgrade()?;
        let allowed = manager.on_run(self.pane, run, |live| {
            // The grant is the worker's own: it lasts while that worker runs.
            (live.run.status(worker) == Some(WorkerStatus::Running))
                .then(|| live.scope.allows(&live.run, secrets, origin, input))
                .flatten()
        });
        allowed.flatten()
    }

    fn allows_write(&self, run: RunId, worker: WorkerId, real: &Path) -> bool {
        // As `allows`: only this handle's run has a scope here.
        if run != self.run {
            return false;
        }
        let Some(manager) = self.manager.upgrade() else {
            return false;
        };
        let allowed = manager.on_run(self.pane, run, |live| {
            live.run.status(worker) == Some(WorkerStatus::Running)
                && live.scope.allows_write(&live.run, real)
        });
        allowed.unwrap_or(false)
    }

    fn workers(&self, run: RunId) -> usize {
        let counted = self
            .manager
            .upgrade()
            .and_then(|manager| manager.on_run(self.pane, run, |live| live.run.worker_count()));
        counted.unwrap_or(0)
    }

    fn staged(&self, run: RunId, worker: WorkerId, change: Change) {
        if let Some(manager) = self.manager.upgrade() {
            manager.on_run(self.pane, run, |live| live.run.staged(worker, change));
        }
    }

    fn applied(&self, run: RunId, worker: WorkerId, paths: &[String]) {
        if let Some(manager) = self.manager.upgrade() {
            manager.on_run(self.pane, run, |live| live.run.applied(worker, paths));
        }
    }
}

/// One worker turn: the adapter inside its session's task-locals, read
/// through a fresh pipe and cut at the deadline.
struct Turn {
    runner: Runner,
    request: ChatRequest,
    session: Option<(Pending, Option<Arc<dyn Dispatch>>)>,
    deadline: Duration,
}

impl Turn {
    /// What the turn reported and how it ended, in order; `None` when the
    /// deadline passed (a worker cut off then reports no usage). The adapter
    /// is dropped then, which kills its process group.
    async fn run(self) -> Option<Vec<WorkerEvent>> {
        let Turn {
            runner,
            request,
            session,
            deadline,
        } = self;
        let engine = request.engine.clone();
        let (tx, rx) = mpsc::channel(TURN_CHANNEL);
        let (pending, dispatch) = session.map_or((None, None), |(p, d)| (Some(p), d));
        // The call sits inside the guarded future, as in a chat turn.
        let adapter = Box::pin(async move { runner(request, tx).await });
        let adapter = chat::in_session(adapter, pending, dispatch);
        tokio::time::timeout(deadline, drive(adapter, rx, &engine))
            .await
            .ok()
    }
}

/// Runs the adapter to its final event and keeps the answer: `run_turn`
/// with the run instead of the app at the end.
async fn drive(
    adapter: BoxFuture<'static, ()>,
    mut rx: mpsc::Receiver<StreamEvent>,
    engine: &str,
) -> Vec<WorkerEvent> {
    let run = AssertUnwindSafe(chat::scoped(adapter))
        .catch_unwind()
        .fuse();
    tokio::pin!(run);
    let (mut pipe, mut text, mut running) = (Pipe::new(), String::new(), true);
    // The usage of every round the engine reported, then the end.
    let mut events = Vec::new();
    loop {
        let event = tokio::select! {
            _ = &mut run, if running => {
                // What is buffered still comes, then `None`.
                running = false;
                rx.close();
                continue;
            }
            event = rx.recv() => event,
        };
        let out = match event {
            Some(event) => pipe.event(event),
            None => pipe.end_without_final(),
        };
        for event in out {
            match event {
                StreamEvent::Delta(delta) => text.push_str(&delta),
                StreamEvent::Usage(usage) => events.push(WorkerEvent::Usage(usage)),
                // A capped answer still ends the worker: `collect` caps it
                // again.
                StreamEvent::Done(_) | StreamEvent::Error(ChatError::Truncated) => {
                    events.push(WorkerEvent::Done { text });
                    return events;
                }
                StreamEvent::Error(error) => {
                    events.push(failed(engine, error));
                    return events;
                }
                _ => {}
            }
        }
    }
}

/// The failure a worker turn ended with: the adapters' classifier gives the
/// class and the redacted provider text. A jail refusal keeps its own text.
fn failed(engine: &str, error: ChatError) -> WorkerEvent {
    let classified = classify(engine, &error);
    let text = match error {
        ChatError::Jail(_) => jail_error(&classified.message),
        ChatError::Interrupted | ChatError::Truncated => INTERRUPTED.to_owned(),
        ChatError::Failed(_) => classified.message,
    };
    WorkerEvent::Failed {
        class: classified.class,
        text,
    }
}

/// Eight bytes from the OS CSPRNG, read anew for every call; `None` when they
/// cannot be read.
fn nonce_bytes() -> Option<[u8; 8]> {
    let mut bytes = [0u8; 8];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .ok()?;
    Some(bytes)
}

fn ids_text(ids: &[WorkerId]) -> String {
    let ids: Vec<_> = ids.iter().map(|id| id.0.to_string()).collect();
    ids.join(", ")
}

/// The run's clock: tokio's, so a paused test clock moves the deadlines.
fn now() -> std::time::Instant {
    Instant::now().into_std()
}

fn fine(text: impl Into<String>) -> Outcome {
    Outcome {
        text: text.into(),
        is_error: false,
    }
}

fn error(text: impl Into<String>) -> Outcome {
    Outcome {
        text: text.into(),
        is_error: true,
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
