//! The engram worker: one tokio task that owns every call to the server.
//!
//! Commands arrive on a bounded FIFO queue and run one at a time, so a
//! session is always registered before the note that needs it. The worker is
//! generic over a `Transport`, so its logic is tested against a scripted fake.

use std::{
    collections::{HashMap, HashSet},
    panic::AssertUnwindSafe,
    path::Path,
    time::Duration,
};

use futures::FutureExt;
use tokio::{
    select,
    sync::mpsc::{self, error::TrySendError},
    task::JoinHandle,
    time::{Instant, sleep, timeout},
};

use crate::{
    core::{
        ai::{
            config::MemoryConfig,
            memory::{
                context::ContextBlock,
                health::{Health, classify_health},
                offer::{self, Binary, Decision, Facts, Offer},
                port::{MemCmd, MemReply, Note, Reason, SaveId, ServerOutcome},
                reply::{
                    DeleteOutcome, ProjectError, parse_context, parse_delete, parse_end,
                    parse_project, parse_saved, parse_session, session_missing,
                },
                route::{self, Call, Method},
            },
        },
        layout::PaneId,
    },
    runtime::{
        chat,
        engram::serve::{BinaryChanged, Launcher, NoServer, Running},
        loopback::{self, CallError, Limits, Reply},
    },
};

/// Commands that may wait for the worker.
pub const QUEUE: usize = 64;
/// How long a failed probe is trusted before the next one.
pub const REPROBE: Duration = Duration::from_secs(10);

/// How often a started server is probed, and for how long.
const START_POLL: Duration = Duration::from_millis(100);
const START_WAIT: Duration = Duration::from_secs(3);
/// The longest the worker waits for the launcher to look for the binary or
/// to spawn it, whatever the launcher does.
const LAUNCH_BOUND: Duration = Duration::from_secs(3);

/// How a call reaches the server.
pub trait Transport: Send + Sync + 'static {
    fn call(
        &self,
        call: &Call,
        limits: Limits,
    ) -> impl Future<Output = Result<Reply, CallError>> + Send;
}

/// The production transport: loopback HTTP to the configured port. The token
/// is only ever put on the one call that asks for it, and has no `Debug`.
pub struct Loopback {
    port: u16,
    token: Option<String>,
}

impl Loopback {
    pub fn new(port: u16, token: Option<String>) -> Self {
        Self {
            port,
            token: token.filter(|t| !t.is_empty()),
        }
    }

    pub fn token_present(&self) -> bool {
        self.token.is_some()
    }
}

impl Transport for Loopback {
    async fn call(&self, call: &Call, limits: Limits) -> Result<Reply, CallError> {
        let auth = if call.auth {
            self.token.as_deref()
        } else {
            None
        };
        loopback::request(
            self.port,
            call.method.into(),
            &call.target,
            call.body.as_deref(),
            auth,
            limits,
        )
        .await
    }
}

/// The time limit of a call: the probe and reads are short, writes longer.
fn limits_for(call: &Call) -> Limits {
    match call.method {
        Method::Get if call.target.starts_with("/health") => Limits::HEALTH,
        Method::Get => Limits::READ,
        Method::Post | Method::Delete => Limits::WRITE,
    }
}

/// What a save writes: the note and its optional caller-supplied topic key.
#[derive(Clone, Copy)]
struct Draft<'a> {
    note: &'a Note,
    topic_key: Option<&'a str>,
}

/// The last `/health` verdict. `reason` is `None` while the server is ready.
struct Probe {
    reason: Option<Reason>,
    at: Instant,
}

struct Worker<T, L> {
    transport: T,
    launcher: L,
    port: u16,
    replies: mpsc::Sender<MemReply>,
    probe: Option<Probe>,
    /// The last health state told to the app, so it hears changes only.
    announced: Option<Health>,
    /// The `instance_id` of the last ready server.
    instance: Option<String>,
    /// The project of each cwd, for the current instance.
    projects: HashMap<String, Result<String, ProjectError>>,
    /// The sessions registered on the current instance.
    registered: HashSet<String>,
    /// The `engram serve` offer was weighed (offered, told why not, or not
    /// possible): it happens once per run.
    weighed: bool,
    /// The offer the user was asked, which is the binary a start runs.
    offered: Option<Offer>,
    /// A server was spawned: never a second one in this run.
    spawned: bool,
}

impl<T: Transport, L: Launcher> Worker<T, L> {
    fn new(transport: T, launcher: L, port: u16, replies: mpsc::Sender<MemReply>) -> Self {
        Self {
            transport,
            launcher,
            port,
            replies,
            probe: None,
            announced: None,
            instance: None,
            projects: HashMap::new(),
            registered: HashSet::new(),
            weighed: false,
            offered: None,
            spawned: false,
        }
    }

    async fn say(&self, reply: MemReply) {
        // A closed channel means the app is gone: nothing to tell.
        let _ = self.replies.send(reply).await;
    }

    async fn run(mut self, mut queue: mpsc::Receiver<MemCmd>) {
        while let Some(cmd) = queue.recv().await {
            if cmd == MemCmd::Quit {
                break;
            }
            let refused = cmd.unavailable(Reason::NoAnswer);
            let handled = AssertUnwindSafe(chat::scoped(self.handle(cmd)))
                .catch_unwind()
                .await;
            if handled.is_err() {
                // The state may be half updated: forget it and probe again.
                self.forget();
                self.say(refused).await;
            }
        }
    }

    /// After a panic: probe again and look projects up again. The registered
    /// sessions and the instance stay. A registration is recorded only after
    /// the server accepted it, so a panic cannot leave a session registered
    /// that is not; keeping them lets a pending `End` still go out (it is
    /// idempotent, and a stale one only earns a 404 that is ignored). The kept
    /// `instance` is what makes the next probe notice a restart and clear them.
    fn forget(&mut self) {
        self.probe = None;
        self.projects.clear();
    }

    async fn handle(&mut self, cmd: MemCmd) {
        match cmd {
            MemCmd::Resolve { pane, cwd, epoch } => {
                let reply = match self.resolve(&cwd).await {
                    Ok(result) => MemReply::Project {
                        pane,
                        epoch,
                        result,
                    },
                    Err(reason) => unavailable(pane, reason),
                };
                self.say(reply).await;
            }
            MemCmd::Context {
                pane,
                project,
                epoch,
            } => {
                let reply = match self.context(&project).await {
                    Ok(block) => MemReply::Context { pane, epoch, block },
                    Err(reason) => unavailable(pane, reason),
                };
                self.say(reply).await;
            }
            MemCmd::Save {
                pane,
                id,
                session,
                project,
                cwd,
                note,
                topic_key,
            } => {
                let draft = Draft {
                    note: &note,
                    topic_key: topic_key.as_deref(),
                };
                let saved = self.save(pane, id, session, &project, &cwd, draft).await;
                let reply = match saved {
                    Ok(reply) => reply,
                    Err(reason) => MemReply::SaveUnavailable { pane, id, reason },
                };
                self.say(reply).await;
            }
            MemCmd::End {
                session,
                project,
                summary,
                cwd,
            } => self.end(&session, &project, cwd.as_deref(), &summary).await,
            MemCmd::Delete { pane, notes } => {
                let reply = match self.ready().await {
                    Ok(()) => self.delete(pane, &notes).await,
                    Err(reason) => MemReply::NotesUnavailable { pane, reason },
                };
                self.say(reply).await;
            }
            MemCmd::StartServer => {
                let outcome = self.start_server().await;
                self.say(MemReply::Server(outcome)).await;
            }
            MemCmd::Quit => {}
        }
    }

    // ---- health ----

    /// Ok when the server is known to be ready; else why not. A failed probe
    /// is trusted for `REPROBE`, so a dead server costs one probe per 10 s.
    async fn ready(&mut self) -> Result<(), Reason> {
        if let Some(probe) = &self.probe {
            match &probe.reason {
                None => return Ok(()),
                Some(reason) if probe.at.elapsed() < REPROBE => return Err(reason.clone()),
                Some(_) => {}
            }
        }
        self.probe_now().await
    }

    async fn probe_now(&mut self) -> Result<(), Reason> {
        let call = route::health();
        let answer = self.raw(&call, limits_for(&call)).await;
        let (health, reason) = match answer {
            Err(error) => (Health::Down, Some(reason_of(error))),
            Ok(reply) => match classify_health(reply.status, &reply.body) {
                Health::Old { version } => (
                    Health::Old { version },
                    Some(Reason::OldServer { port: self.port }),
                ),
                Health::Foreign => (Health::Foreign, Some(Reason::Foreign { port: self.port })),
                Health::Down => (Health::Down, Some(Reason::NoAnswer)),
                ready @ Health::Ready { .. } => {
                    self.adopt(&ready);
                    (ready, None)
                }
            },
        };
        self.settle(health, reason.clone()).await;
        reason.map_or(Ok(()), Err)
    }

    /// A new instance has none of the old one's sessions.
    fn adopt(&mut self, health: &Health) {
        let Health::Ready { instance_id, .. } = health else {
            return;
        };
        if self.instance.as_ref().is_some_and(|old| old != instance_id) {
            self.projects.clear();
            self.registered.clear();
        }
        self.instance = Some(instance_id.clone());
    }

    async fn settle(&mut self, health: Health, reason: Option<Reason>) {
        let changed = self.announced.as_ref() != Some(&health);
        let offer = reason == Some(Reason::NotRunning);
        self.probe = Some(Probe {
            reason,
            at: Instant::now(),
        });
        if changed {
            self.announced = Some(health.clone());
            self.say(MemReply::Health {
                state: health,
                offer,
            })
            .await;
        }
        if offer && !self.weighed {
            self.weigh_offer().await;
        }
    }

    // ---- engram serve ----

    /// Once per run, when nothing runs on the port: looks for `engram` and
    /// either offers to start it or says why that is not possible.
    async fn weigh_offer(&mut self) {
        self.weighed = true;
        let binary = self.find_binary().await;
        // The worker only exists for an active memory.
        let config = MemoryConfig {
            enabled: true,
            acknowledged: true,
            port: self.port,
            save_summaries: false,
        };
        let reason = self.probe.as_ref().and_then(|p| p.reason.clone());
        let facts = Facts {
            config: &config,
            health: self.announced.as_ref(),
            reason: reason.as_ref(),
            project: None,
            used: false,
            binary: &binary,
        };
        match offer::decide(&facts) {
            Decision::Offer(offer) => {
                self.offered = Some(offer.clone());
                self.say(MemReply::ServerOffer(offer)).await;
            }
            Decision::Notice(reason) => {
                self.say(MemReply::Unavailable { pane: None, reason }).await
            }
            Decision::Nothing => {}
        }
    }

    async fn find_binary(&self) -> Binary {
        timeout(LAUNCH_BOUND, self.launcher.find())
            .await
            .unwrap_or(Binary::Missing)
    }

    /// The confirmed start. It probes first (a server may have appeared),
    /// then spawns the offered binary (never another path) at most once per
    /// run, and polls until the server answers, the child is gone or three
    /// seconds passed. It never signals the server it started.
    async fn start_server(&mut self) -> ServerOutcome {
        // The user decided: there is nothing left to offer in this run.
        self.weighed = true;
        match self.probe_now().await {
            Ok(()) => return ServerOutcome::Adopted,
            Err(Reason::NotRunning) => {}
            Err(other) => return ServerOutcome::Failed(other),
        }
        if self.spawned {
            return ServerOutcome::Failed(Reason::NotRunning);
        }
        // Only the absolute path the user was shown may run: without an offer
        // the PATH is not consulted again and nothing is spawned.
        let Some(offer) = self.offered.clone() else {
            return ServerOutcome::Failed(Reason::StartFailed(Some("nothing was offered".into())));
        };
        self.spawned = true;
        let spawn = self.launcher.spawn(Path::new(&offer.path), self.port);
        let running = match timeout(LAUNCH_BOUND, spawn).await {
            Ok(Ok(running)) => running,
            // The launcher found the file at that path is no longer the one
            // that was looked up and offered.
            Ok(Err(error)) if BinaryChanged::is(&error) => {
                return ServerOutcome::Failed(Reason::Changed);
            }
            Ok(Err(error)) => {
                let hint = offer::spawn_hint(&error.to_string());
                return ServerOutcome::Failed(Reason::StartFailed(hint));
            }
            // The launcher did not answer in time.
            Err(_) => return ServerOutcome::Failed(Reason::StartFailed(None)),
        };
        self.await_server(running).await
    }

    async fn await_server(&mut self, running: Running) -> ServerOutcome {
        let Running {
            pid,
            mut exit,
            log_from,
        } = running;
        let deadline = Instant::now() + START_WAIT;
        let mut exited = false;
        loop {
            select! {
                () = sleep(START_POLL) => {}
                _ = &mut exit, if !exited => exited = true,
            }
            match self.probe_now().await {
                // The child exited and a server answers: another process won
                // the port race, and that server is the one we adopt.
                Ok(()) if exited => return ServerOutcome::Adopted,
                Ok(()) => return ServerOutcome::Started { pid },
                Err(Reason::NotRunning | Reason::NoAnswer) => {}
                Err(other) => return ServerOutcome::Failed(other),
            }
            if exited || Instant::now() >= deadline {
                let line = timeout(LAUNCH_BOUND, self.launcher.log_line(log_from))
                    .await
                    .ok()
                    .flatten();
                return ServerOutcome::Failed(Reason::StartFailed(line));
            }
        }
    }

    // ---- calls ----

    /// One call, bounded by its limit even if the transport ignores it.
    async fn raw(&self, call: &Call, limits: Limits) -> Result<Reply, CallError> {
        match timeout(limits.total, self.transport.call(call, limits)).await {
            Ok(result) => result,
            Err(_) => Err(CallError::Timeout),
        }
    }

    /// One call of a ready server. A transport failure sets the state down.
    async fn send(&mut self, call: &Call) -> Result<Reply, Reason> {
        match self.raw(call, limits_for(call)).await {
            Ok(reply) => Ok(reply),
            Err(error) => {
                let reason = reason_of(error);
                self.settle(Health::Down, Some(reason.clone())).await;
                Err(reason)
            }
        }
    }

    // ---- commands ----

    async fn resolve(&mut self, cwd: &str) -> Result<Result<String, ProjectError>, Reason> {
        self.ready().await?;
        if let Some(known) = self.projects.get(cwd) {
            return Ok(known.clone());
        }
        let reply = self.send(&route::project_current(cwd)).await?;
        let result = parse_project(reply.status, &reply.body);
        if result != Err(ProjectError::Invalid) {
            self.projects.insert(cwd.to_string(), result.clone());
        }
        Ok(result)
    }

    async fn context(&mut self, project: &str) -> Result<Option<ContextBlock>, Reason> {
        self.ready().await?;
        let reply = self.send(&route::context(project)).await?;
        match parse_context(reply.status, &reply.body, project) {
            Ok(block) => Ok(block),
            // A 200 whose body cannot be read is no context: chat goes on.
            Err(_) if reply.status == 200 => Ok(None),
            // Any other status (404 `unknown_project` was handled above as no
            // memory yet) is a server that is not answering well: degrade.
            Err(_) => {
                let reason = Reason::NoAnswer;
                self.settle(Health::Down, Some(reason.clone())).await;
                Err(reason)
            }
        }
    }

    async fn save(
        &mut self,
        pane: PaneId,
        id: SaveId,
        session: Option<String>,
        project: &str,
        cwd: &str,
        draft: Draft<'_>,
    ) -> Result<MemReply, Reason> {
        self.ready().await?;
        let failed = |reason: &str| MemReply::SaveFailed {
            pane,
            id,
            reason: reason.to_string(),
        };
        let Some(session) = session.or_else(new_session_id) else {
            return Ok(failed("could not create a session id"));
        };
        if !self.registered.contains(&session)
            && let Some(reason) = self.register(&session, project, cwd).await?
        {
            return Ok(failed(&reason));
        }
        let call = route::save(&session, project, draft.note, draft.topic_key);
        let mut reply = self.send(&call).await?;
        if session_missing(reply.status, &reply.body) {
            // The server forgot the session (a restart): register it again and
            // retry once. A second failure is a failure, not a loop.
            self.registered.remove(&session);
            if let Some(reason) = self.register(&session, project, cwd).await? {
                return Ok(failed(&reason));
            }
            reply = self.send(&call).await?;
        }
        match parse_saved(reply.status, &reply.body) {
            Ok(saved) => {
                let (title, topic_key) = sent_labels(&call);
                Ok(MemReply::Saved {
                    pane,
                    id,
                    project: project.to_owned(),
                    session,
                    observation_id: saved.id,
                    topic_key,
                    title,
                })
            }
            Err(failure) => Ok(failed(&failure.reason)),
        }
    }

    /// Registers a session with the chat's `cwd` as its directory. `Some`
    /// holds the short reason when the server refused it.
    async fn register(
        &mut self,
        session: &str,
        project: &str,
        cwd: &str,
    ) -> Result<Option<String>, Reason> {
        let Ok(call) = route::register_session(session, project, cwd) else {
            return Ok(Some("invalid session id".into()));
        };
        let reply = self.send(&call).await?;
        Ok(match parse_session(reply.status, &reply.body) {
            Ok(()) => {
                self.registered.insert(session.to_string());
                None
            }
            Err(failure) => Some(failure.reason),
        })
    }

    /// Ends a registered session, once. Best effort: nobody waits for a reply.
    /// With a `cwd` (the end carries an approved summary) a session that is
    /// not registered yet is registered first; without it, it is not ended.
    async fn end(&mut self, session: &str, project: &str, cwd: Option<&str>, summary: &str) {
        if !self.registered.contains(session) && cwd.is_none() {
            return;
        }
        if self.ready().await.is_err() {
            return;
        }
        // The probe may have found a new instance, which forgot the session.
        if !self.registered.contains(session)
            && let Some(cwd) = cwd
            && !matches!(self.register(session, project, cwd).await, Ok(None))
        {
            return;
        }
        let Ok(call) = route::end_session(session, summary) else {
            return;
        };
        if let Ok(reply) = self.send(&call).await
            && parse_end(reply.status, &reply.body).is_ok()
        {
            self.registered.remove(session);
        }
    }

    async fn delete(&mut self, pane: PaneId, notes: &[(String, i64)]) -> MemReply {
        let (mut ok, mut failed, mut refused) = (0, 0, false);
        for (index, (project, id)) in notes.iter().enumerate() {
            match self.send(&route::delete(*id, project)).await {
                Ok(reply) => match parse_delete(reply.status, &reply.body) {
                    DeleteOutcome::Deleted => ok += 1,
                    DeleteOutcome::Failed => failed += 1,
                    DeleteOutcome::Refused => {
                        refused = true;
                        failed += notes.len() - index;
                        break;
                    }
                },
                Err(_) => {
                    failed += notes.len() - index;
                    break;
                }
            }
        }
        MemReply::Deleted {
            pane,
            ok,
            failed,
            refused,
        }
    }
}

fn unavailable(pane: PaneId, reason: Reason) -> MemReply {
    MemReply::Unavailable {
        pane: Some(pane),
        reason,
    }
}

/// Only `Connect` means nothing runs there (a refused connect, or one dropped
/// while the port is free); any other failure is a server that did not answer.
fn reason_of(error: CallError) -> Reason {
    match error {
        CallError::Connect => Reason::NotRunning,
        _ => Reason::NoAnswer,
    }
}

/// The title and topic key the save call carries (the builder cleaned them).
fn sent_labels(call: &Call) -> (String, String) {
    let field = |name: &str| {
        call.body
            .as_deref()
            .and_then(|body| serde_json::from_slice::<serde_json::Value>(body).ok())
            .and_then(|value| value.get(name)?.as_str().map(str::to_string))
            .unwrap_or_default()
    };
    (field("title"), field("topic_key"))
}

/// The app's end of the worker.
pub struct MemoryLink {
    tx: mpsc::Sender<MemCmd>,
    replies: mpsc::Sender<MemReply>,
    task: JoinHandle<()>,
}

impl MemoryLink {
    /// Queues a command without waiting. A full queue answers `Busy` at once.
    pub fn send(&self, cmd: MemCmd) {
        let (cmd, reason) = match self.tx.try_send(cmd) {
            Ok(()) => return,
            Err(TrySendError::Full(cmd)) => (cmd, Reason::Busy),
            Err(TrySendError::Closed(cmd)) => (cmd, Reason::NoAnswer),
        };
        let _ = self.replies.try_send(cmd.unavailable(reason));
    }

    /// Asks the worker to stop after the commands already queued (so pending
    /// `End`s go out) and waits at most `bound` for it; then it is aborted.
    pub async fn shutdown(self, bound: Duration) {
        let started = Instant::now();
        let _ = timeout(bound, self.tx.send(MemCmd::Quit)).await;
        let mut task = self.task;
        let left = bound.saturating_sub(started.elapsed());
        if timeout(left, &mut task).await.is_err() {
            task.abort();
        }
    }
}

/// Starts the worker task, without the `engram serve` offer.
pub fn spawn<T: Transport>(transport: T, port: u16, replies: mpsc::Sender<MemReply>) -> MemoryLink {
    spawn_with(transport, NoServer, port, replies)
}

/// Starts the worker task with a launcher for the `engram serve` offer.
pub fn spawn_with<T: Transport, L: Launcher>(
    transport: T,
    launcher: L,
    port: u16,
    replies: mpsc::Sender<MemReply>,
) -> MemoryLink {
    let (tx, queue) = mpsc::channel(QUEUE);
    let worker = Worker::new(transport, launcher, port, replies.clone());
    MemoryLink {
        tx,
        replies,
        task: tokio::spawn(worker.run(queue)),
    }
}

/// `lazarobox-` and 32 lowercase hex digits from 16 random bytes.
pub fn session_id_from(bytes: [u8; 16]) -> String {
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("lazarobox-{hex}")
}

/// A fresh session id from the operating system's randomness, `None` when the
/// system gives none.
///
/// Linux only: it needs `getrandom(2)`, which libc exposes on Linux (and some
/// other systems, but this project targets Linux). Elsewhere there is no id,
/// so a save is a short `SaveFailed` rather than a weaker, guessable id.
#[cfg(target_os = "linux")]
pub fn new_session_id() -> Option<String> {
    let mut bytes = [0u8; 16];
    // SAFETY: `bytes` is valid for 16 writes; flags 0 blocks until seeded.
    let got = unsafe { libc::getrandom(bytes.as_mut_ptr().cast(), bytes.len(), 0) };
    (got == bytes.len() as isize).then(|| session_id_from(bytes))
}

/// No `getrandom` here: no session id (see the Linux version).
#[cfg(not(target_os = "linux"))]
pub fn new_session_id() -> Option<String> {
    None
}

#[cfg(test)]
mod tests;
