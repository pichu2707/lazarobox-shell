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
            approval::worker_label,
            engine::{
                ChatError, ChatMessage, ChatRequest, MCP_VERIFIED, Role, StreamEvent, Tools,
                locality, tools_for,
            },
            guidance::guidance,
            jail_error,
        },
        config::{Profile, Task},
        jail,
        orchestra::{
            RunId, WorkerId, WorkerKey,
            run::{
                CancelOutcome, CollectError, NotHalted, Refusal, Roster, Run, RunState, UserChoice,
                WorkerEvent,
            },
            scope::{Dropped, ReadScope},
            tools::OrchCall,
            worker::{ErrorClass, WorkerSpec, WorkerStatus},
        },
        tools::{
            label,
            paths::SecretRoots,
            registry::Family,
            wiring::{McpWire, Secret},
        },
    },
    layout::PaneId,
};

/// What a call of a run that ended, or was replaced, answers.
const ENDED: &str = "run ended";

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
    /// Each worker's deadline (`worker_timeout_secs`).
    pub deadline: Duration,
    pub profile: Profile,
    /// The orchestrator turn's channel and approver: a worker's tool rows and
    /// asks go there (D4), and `pending` lets that turn's timers see a
    /// worker's ask waiting.
    pub events: mpsc::Sender<StreamEvent>,
    pub approver: Arc<dyn Approver>,
    pub pending: Pending,
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
    scope: ReadScope,
    /// The roots the scope refused, sanitized, with why.
    dropped: Vec<Dropped>,
    origin: PathBuf,
    profile: Profile,
    events: mpsc::Sender<StreamEvent>,
    approver: Arc<dyn Approver>,
    pending: Pending,
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
}

impl Drop for Live {
    /// The backstop of a run dropped without being ended, its manager's
    /// included: its worker turns would otherwise run on to their deadline.
    fn drop(&mut self) {
        for task in self.tasks.values() {
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
            run: Run::new(run, scope.roster, scope.deadline),
            origin: scope.origin,
            profile: scope.profile,
            events: scope.events,
            approver: scope.approver,
            pending: scope.pending,
            tasks: HashMap::new(),
            shown: HashMap::new(),
            alerted: None,
            retry: HashMap::new(),
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

    /// The user's answer to the failure that halted the run of `pane`.
    /// `Retry` relaunches the failed worker on its spec, its prompt followed
    /// by the failure's text wrapped as untrusted data; `Cancel` ends the run.
    /// `NotHalted` when no failure waits for the user, and nothing changes.
    pub fn decide(self: &Arc<Self>, pane: PaneId, choice: UserChoice) -> Result<(), NotHalted> {
        let mut runs = lock(&self.runs);
        let live = runs.get_mut(&pane).ok_or(NotHalted)?;
        let RunState::Halted { worker, text, .. } = live.run.state() else {
            return Err(NotHalted);
        };
        let (worker, text) = (*worker, text.clone());
        let retry = choice == UserChoice::Retry;
        live.run.decide(now(), choice)?;
        if *live.run.state() == RunState::Cancelled {
            let live = runs.remove(&pane);
            drop(runs);
            if let Some(live) = live {
                self.end(pane, live);
            }
            return Ok(());
        }
        if retry {
            live.retry.insert(worker, text);
        } else {
            live.retry.remove(&worker);
        }
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
        live.run.cancel_run(now());
        let tasks: Vec<_> = live
            .tasks
            .drain()
            .map(|(worker, task)| {
                task.abort();
                self.close_session(WorkerKey { run, worker });
                task
            })
            .collect();
        self.sync(pane, &mut live);
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
            let Some(bytes) = nonce_bytes() else {
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

    /// Cancels one worker: its turn is aborted and its session closed, and
    /// its slot goes to the next queued worker. One that already ended stays
    /// as it was.
    fn cancel_worker(self: &Arc<Self>, pane: PaneId, run: RunId, id: WorkerId) -> Outcome {
        let cancelled = self.on_run(pane, run, |live| match live.run.cancel(now(), id) {
            CancelOutcome::Cancelled => {
                if let Some(task) = live.tasks.remove(&id) {
                    task.abort();
                }
                self.close_session(WorkerKey { run, worker: id });
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
        if let Some(failure) = live.retry.remove(&id) {
            // Wrapped or left out: an error text never reaches a model bare.
            if let Some(bytes) = nonce_bytes() {
                let wrapped = label::wrap(&label::nonce_from(bytes), &failure);
                prompt = format!("{prompt}\n\n{wrapped}");
            }
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
        let session = self
            .open_session(pane, key, live, &spec, &mut request)
            .map(|dispatch| (live.pending.clone(), dispatch));
        let turn = Turn {
            runner: self.runner.clone(),
            request,
            session,
            deadline: spec.deadline,
        };
        let manager = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            let event = turn.run().await;
            if let Some(manager) = manager.upgrade() {
                manager.finish(pane, key, event);
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
        live: &Live,
        spec: &WorkerSpec,
        request: &mut ChatRequest,
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
            events: live.events.clone(),
            locality,
            origin: origin.clone().unwrap_or_else(|| PathBuf::from("/")),
            approver: live.approver.clone(),
            external: Slot::default(),
            families,
            stage_key: None,
            allowed: Default::default(),
            orchestra: None,
            read_scope: Some(WorkerScope {
                handle: self.handle(pane, key.run),
                run: key.run,
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

    fn close_session(&self, key: WorkerKey) {
        if let Some(tooling) = &self.tooling
            && let Some(hub) = lock(&tooling.hub).as_mut()
        {
            hub.close(key);
        }
    }

    /// Applies how a worker turn ended; `None` means its deadline passed. A
    /// run that ended or was replaced meanwhile ignores it.
    fn finish(self: &Arc<Self>, pane: PaneId, key: WorkerKey, event: Option<WorkerEvent>) {
        self.close_session(key);
        self.on_run(pane, key.run, |live| {
            live.tasks.remove(&key.worker);
            match event {
                Some(event) => live.run.on_event(now(), key.worker, event),
                None => {
                    live.run.on_tick(now());
                }
            }
            self.sync(pane, live);
        });
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
            live.scope.allows(&live.run, secrets, origin, input)
        });
        allowed.flatten()
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
    /// How the turn ended; `None` when the deadline passed. The adapter is
    /// dropped then, which kills its process group.
    async fn run(self) -> Option<WorkerEvent> {
        let Turn {
            runner,
            request,
            session,
            deadline,
        } = self;
        let (tx, rx) = mpsc::channel(TURN_CHANNEL);
        let (pending, dispatch) = session.map_or((None, None), |(p, d)| (Some(p), d));
        // The call sits inside the guarded future, as in a chat turn.
        let adapter = Box::pin(async move { runner(request, tx).await });
        let adapter = chat::in_session(adapter, pending, dispatch);
        tokio::time::timeout(deadline, drive(adapter, rx))
            .await
            .ok()
    }
}

/// Runs the adapter to its final event and keeps the answer: `run_turn`
/// with the run instead of the app at the end.
async fn drive(
    adapter: BoxFuture<'static, ()>,
    mut rx: mpsc::Receiver<StreamEvent>,
) -> WorkerEvent {
    let run = AssertUnwindSafe(chat::scoped(adapter))
        .catch_unwind()
        .fuse();
    tokio::pin!(run);
    let (mut pipe, mut text, mut running) = (Pipe::new(), String::new(), true);
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
                // A capped answer still ends the worker: `collect` caps it
                // again.
                StreamEvent::Done(_) | StreamEvent::Error(ChatError::Truncated) => {
                    return WorkerEvent::Done { text };
                }
                StreamEvent::Error(error) => return failed(error),
                _ => {}
            }
        }
    }
}

/// The failure a worker turn ended with. The adapters' classifier arrives in
/// S2; a jail refusal already is its own class (D10).
fn failed(error: ChatError) -> WorkerEvent {
    let (class, text) = match error {
        ChatError::Jail(reason) => (ErrorClass::JailRefusal, jail_error(&reason)),
        ChatError::Failed(text) => (ErrorClass::Other, text),
        ChatError::Interrupted | ChatError::Truncated => {
            (ErrorClass::Other, INTERRUPTED.to_owned())
        }
    };
    WorkerEvent::Failed { class, text }
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
