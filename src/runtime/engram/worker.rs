//! The engram worker: one tokio task that owns every call to the server.
//!
//! Commands arrive on a bounded FIFO queue and run one at a time, so a
//! session is always registered before the note that needs it. The worker is
//! generic over a `Transport`, so its logic is tested against a scripted fake.

use std::{
    collections::{HashMap, HashSet},
    panic::AssertUnwindSafe,
    time::Duration,
};

use futures::FutureExt;
use tokio::{
    sync::mpsc::{self, error::TrySendError},
    task::JoinHandle,
    time::{Instant, timeout},
};

use crate::{
    core::{
        ai::memory::{
            context::ContextBlock,
            health::{Health, classify_health},
            port::{MemCmd, MemReply, Note, Reason},
            reply::{
                DeleteOutcome, ProjectError, parse_context, parse_delete, parse_end, parse_project,
                parse_saved, parse_session,
            },
            route::{self, Call, Method},
        },
        layout::PaneId,
    },
    runtime::{
        chat,
        loopback::{self, CallError, Limits, Reply},
    },
};

/// Commands that may wait for the worker.
pub const QUEUE: usize = 64;
/// How long a failed probe is trusted before the next one.
pub const REPROBE: Duration = Duration::from_secs(10);

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

/// The last `/health` verdict. `reason` is `None` while the server is ready.
struct Probe {
    reason: Option<Reason>,
    at: Instant,
}

struct Worker<T> {
    transport: T,
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
}

impl<T: Transport> Worker<T> {
    fn new(transport: T, port: u16, replies: mpsc::Sender<MemReply>) -> Self {
        Self {
            transport,
            port,
            replies,
            probe: None,
            announced: None,
            instance: None,
            projects: HashMap::new(),
            registered: HashSet::new(),
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
            let pane = cmd.pane();
            let handled = AssertUnwindSafe(chat::scoped(self.handle(cmd)))
                .catch_unwind()
                .await;
            if handled.is_err() {
                // The state may be half updated: forget it and probe again.
                self.forget();
                let reason = Reason::NoAnswer;
                self.say(MemReply::Unavailable { pane, reason }).await;
            }
        }
    }

    fn forget(&mut self) {
        self.probe = None;
        self.instance = None;
        self.projects.clear();
        self.registered.clear();
    }

    async fn handle(&mut self, cmd: MemCmd) {
        match cmd {
            MemCmd::Resolve { pane, cwd } => {
                let reply = match self.resolve(&cwd).await {
                    Ok(result) => MemReply::Project { pane, result },
                    Err(reason) => unavailable(pane, reason),
                };
                self.say(reply).await;
            }
            MemCmd::Context { pane, project } => {
                let reply = match self.context(&project).await {
                    Ok(block) => MemReply::Context { pane, block },
                    Err(reason) => unavailable(pane, reason),
                };
                self.say(reply).await;
            }
            MemCmd::Save {
                pane,
                session,
                project,
                note,
            } => {
                let reply = match self.save(pane, session, &project, &note).await {
                    Ok(reply) => reply,
                    Err(reason) => unavailable(pane, reason),
                };
                self.say(reply).await;
            }
            MemCmd::End {
                session, summary, ..
            } => self.end(&session, &summary).await,
            MemCmd::Delete { pane, notes } => {
                let reply = match self.ready().await {
                    Ok(()) => self.delete(pane, &notes).await,
                    Err(reason) => unavailable(pane, reason),
                };
                self.say(reply).await;
            }
            // The server start lands with the `engram serve` offer (M5).
            MemCmd::StartServer | MemCmd::Quit => {}
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
        // A context that cannot be read is no context: chat goes on.
        Ok(parse_context(reply.status, &reply.body, project).unwrap_or(None))
    }

    async fn save(
        &mut self,
        pane: PaneId,
        session: Option<String>,
        project: &str,
        note: &Note,
    ) -> Result<MemReply, Reason> {
        self.ready().await?;
        let failed = |reason: &str| MemReply::SaveFailed {
            pane,
            reason: reason.to_string(),
        };
        let Some(session) = session.or_else(new_session_id) else {
            return Ok(failed("could not create a session id"));
        };
        if !self.registered.contains(&session) {
            let directory = self.directory_of(project);
            let Ok(call) = route::register_session(&session, project, &directory) else {
                return Ok(failed("invalid session id"));
            };
            let reply = self.send(&call).await?;
            match parse_session(reply.status, &reply.body) {
                Ok(()) => {
                    self.registered.insert(session.clone());
                }
                Err(failure) => return Ok(failed(&failure.reason)),
            }
        }
        let call = route::save(&session, project, note);
        let reply = self.send(&call).await?;
        match parse_saved(reply.status, &reply.body) {
            Ok(saved) => {
                let (title, topic_key) = sent_labels(&call);
                Ok(MemReply::Saved {
                    pane,
                    session,
                    observation_id: saved.id,
                    topic_key,
                    title,
                })
            }
            Err(failure) => Ok(failed(&failure.reason)),
        }
    }

    /// A directory the project was resolved from, for the session record.
    fn directory_of(&self, project: &str) -> String {
        self.projects
            .iter()
            .filter(|(_, found)| found.as_deref() == Ok(project))
            .map(|(cwd, _)| cwd.clone())
            .min()
            .unwrap_or_default()
    }

    /// Ends a registered session, once. Best effort: nobody waits for a reply.
    async fn end(&mut self, session: &str, summary: &str) {
        if !self.registered.contains(session) || self.ready().await.is_err() {
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

/// Only a refused connection means nothing runs there; any other failure is a
/// server that did not answer.
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
        let pane = cmd.pane();
        let _ = self
            .replies
            .try_send(MemReply::Unavailable { pane, reason });
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

/// Starts the worker task.
pub fn spawn<T: Transport>(transport: T, port: u16, replies: mpsc::Sender<MemReply>) -> MemoryLink {
    let (tx, queue) = mpsc::channel(QUEUE);
    let worker = Worker::new(transport, port, replies.clone());
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
pub fn new_session_id() -> Option<String> {
    let mut bytes = [0u8; 16];
    // SAFETY: `bytes` is valid for 16 writes; flags 0 blocks until seeded.
    let got = unsafe { libc::getrandom(bytes.as_mut_ptr().cast(), bytes.len(), 0) };
    (got == bytes.len() as isize).then(|| session_id_from(bytes))
}

#[cfg(test)]
mod tests;
