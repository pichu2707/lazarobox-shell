//! The store thread: one `std::thread` owns the connection and runs the
//! commands of every pane in the order they were sent.

use std::{
    io,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    sync::mpsc as std_mpsc,
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, TransactionBehavior};
use tokio::sync::mpsc;

use super::{StoreError, open};
use crate::core::{
    ai::{
        chat::{
            MsgStatus,
            engine::Role,
            store::{RETENTION_LIMIT, StoreCmd, StoreReply, StoredMessage, bounded_error},
        },
        sanitize::clean,
    },
    layout::PaneId,
};

type Opener = Box<dyn FnOnce() -> Result<Connection, StoreError> + Send>;

/// Name of the store thread; the panic hook keys off it.
pub const THREAD_NAME: &str = "chat-store";

/// The longest conversation title, in characters.
const TITLE_MAX: usize = 80;

/// Runs commands against the database. The connection is opened on the first
/// command, so a session that never chats never touches the data directory.
pub(crate) struct Worker {
    opener: Option<Opener>,
    conn: Option<Result<Connection, String>>,
    /// The previous write failed: only the first failure of a run is notified.
    failing: bool,
}

impl Worker {
    pub(crate) fn new(
        open: impl FnOnce() -> Result<Connection, StoreError> + Send + 'static,
    ) -> Self {
        Self {
            opener: Some(Box::new(open)),
            conn: None,
            failing: false,
        }
    }

    /// The open connection, or the bounded reason there is none.
    fn conn(&mut self) -> Result<&mut Connection, String> {
        if self.conn.is_none() {
            let opened = match self.opener.take() {
                Some(open) => open().map_err(|err| bounded_error(&err.to_string())),
                None => Err(bounded_error("store not available")),
            };
            self.conn = Some(opened);
        }
        match self.conn.as_mut() {
            Some(Ok(conn)) => Ok(conn),
            Some(Err(reason)) => Err(reason.clone()),
            None => Err(bounded_error("store not available")),
        }
    }

    /// Runs one command; the reply to send, if any. `Quit` is the thread's
    /// business, not the worker's.
    pub(crate) fn handle(&mut self, cmd: StoreCmd) -> Option<StoreReply> {
        match cmd {
            StoreCmd::Quit => None,
            StoreCmd::Create {
                pane,
                profile,
                tag,
                first,
                open,
            } => {
                let conn = match self.conn() {
                    Ok(conn) => conn,
                    Err(reason) => return Some(StoreReply::Unavailable { pane, reason }),
                };
                let made = create(conn, &profile, tag.as_deref(), &first, &open);
                Some(match made {
                    Ok((conversation, evicted)) => {
                        self.failing = false;
                        StoreReply::Created {
                            pane,
                            conversation,
                            evicted,
                        }
                    }
                    Err(_) => self.failure(pane),
                })
            }
            StoreCmd::Append {
                pane,
                conversation,
                message,
            } => {
                if message.status == MsgStatus::Error {
                    return None;
                }
                // An unusable store was already reported by the `Create`.
                let conn = self.conn().ok()?;
                match append(conn, conversation, &message) {
                    Ok(()) => {
                        self.failing = false;
                        None
                    }
                    Err(_) => Some(self.failure(pane)),
                }
            }
        }
    }

    /// `handle` that survives a panic. A panic leaves the connection in an
    /// unknown state, so the store is marked unusable: the command that
    /// panicked gets a reply (`Unavailable` for a `Create`, `Failed` for an
    /// `Append`) and every later `Create` answers `Unavailable`. The thread
    /// keeps running so it still drains its queue and honours `Quit`.
    pub(crate) fn guarded(&mut self, cmd: StoreCmd) -> Option<StoreReply> {
        self.guarded_with(cmd, Self::handle)
    }

    fn guarded_with(
        &mut self,
        cmd: StoreCmd,
        run: impl FnOnce(&mut Self, StoreCmd) -> Option<StoreReply>,
    ) -> Option<StoreReply> {
        let (pane, is_create) = match &cmd {
            StoreCmd::Create { pane, .. } => (*pane, true),
            StoreCmd::Append { pane, .. } => (*pane, false),
            StoreCmd::Quit => return None,
        };
        match catch_unwind(AssertUnwindSafe(|| run(self, cmd))) {
            Ok(reply) => reply,
            Err(payload) => {
                let reason = bounded_error(&panic_text(payload.as_ref()));
                self.conn = Some(Err(reason.clone()));
                self.opener = None;
                Some(if is_create {
                    StoreReply::Unavailable { pane, reason }
                } else {
                    self.failure(pane)
                })
            }
        }
    }

    fn failure(&mut self, pane: PaneId) -> StoreReply {
        let notify = !self.failing;
        self.failing = true;
        StoreReply::Failed { pane, notify }
    }
}

/// The message of a panic payload, or a generic line.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    let text = payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    format!("store thread panicked: {text}")
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// The first non-empty line of the first message, cut to `TITLE_MAX` characters.
fn title_of(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("(untitled)");
    line.chars().take(TITLE_MAX).collect()
}

/// Creates the conversation and its first message in one transaction, first
/// making room under the retention limit. Returns the id and whether
/// anything was evicted.
fn create(
    conn: &mut Connection,
    profile: &str,
    tag: Option<&str>,
    first: &StoredMessage,
    open: &[i64],
) -> Result<(i64, bool), rusqlite::Error> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let stored: i64 = tx.query_row("SELECT COUNT(*) FROM conversations", [], |r| r.get(0))?;
    let excess = usize::try_from(stored)
        .unwrap_or(usize::MAX)
        .saturating_add(1)
        .saturating_sub(RETENTION_LIMIT);
    let mut evicted = false;
    if excess > 0 {
        // The oldest last activity goes first; tags do not protect, open
        // conversations are skipped. Ids tie-break so the order is stable.
        let marks = (0..open.len())
            .map(|i| format!("?{}", i + 2))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "DELETE FROM conversations WHERE id IN (
                 SELECT id FROM conversations WHERE id NOT IN ({marks})
                 ORDER BY updated_at, id LIMIT ?1)"
        );
        let limit = i64::try_from(excess).unwrap_or(i64::MAX);
        let params = std::iter::once(limit).chain(open.iter().copied());
        evicted = tx.execute(&sql, rusqlite::params_from_iter(params))? > 0;
    }
    let text = clean(&first.text);
    let stamp = now();
    tx.execute(
        "INSERT INTO conversations(created_at, updated_at, profile, title)
         VALUES (?1, ?1, ?2, ?3)",
        (stamp, profile, title_of(&text)),
    )?;
    let id = tx.last_insert_rowid();
    if let Some(tag) = tag {
        tx.execute(
            "INSERT INTO tags(conversation_id, tag, is_project) VALUES (?1, ?2, 1)",
            (id, tag),
        )?;
    }
    insert_message(&tx, id, first, &text, stamp)?;
    tx.commit()?;
    Ok((id, evicted))
}

/// Appends a message and bumps the conversation's last activity.
fn append(
    conn: &mut Connection,
    conversation: i64,
    message: &StoredMessage,
) -> Result<(), rusqlite::Error> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let stamp = now();
    let touched = tx.execute(
        "UPDATE conversations SET updated_at = ?2 WHERE id = ?1",
        (conversation, stamp),
    )?;
    if touched == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    insert_message(&tx, conversation, message, &clean(&message.text), stamp)?;
    tx.commit()
}

fn insert_message(
    tx: &rusqlite::Transaction<'_>,
    conversation: i64,
    message: &StoredMessage,
    text: &str,
    stamp: i64,
) -> Result<(), rusqlite::Error> {
    let role = match message.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    tx.execute(
        "INSERT INTO messages(conversation_id, seq, role, content, engine, model, status, created_at)
         VALUES (?1,
                 (SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE conversation_id = ?1),
                 ?2, ?3, ?4, ?5, ?6, ?7)",
        (
            conversation,
            role,
            text,
            &message.engine,
            &message.model,
            message.status.as_str(),
            stamp,
        ),
    )?;
    Ok(())
}

/// The owner's end of the store thread.
pub struct StoreHandle {
    tx: std_mpsc::Sender<StoreCmd>,
    /// To answer a `Create` the thread can no longer read.
    replies: mpsc::Sender<StoreReply>,
    done: std_mpsc::Receiver<()>,
    thread: Option<JoinHandle<()>>,
}

impl StoreHandle {
    /// Queues `cmd`; never waits for the database.
    ///
    /// With no thread behind the queue (it never started) a `Create` is
    /// answered `Unavailable` so the pane detaches instead of waiting.
    pub fn send(&self, cmd: StoreCmd) {
        let Err(std_mpsc::SendError(cmd)) = self.tx.send(cmd) else {
            return;
        };
        if let StoreCmd::Create { pane, .. } = cmd {
            let reason = bounded_error("store thread not running");
            let _ = self
                .replies
                .try_send(StoreReply::Unavailable { pane, reason });
        }
    }

    /// Queues `Quit` and waits up to `bound` for everything before it to be
    /// written. A thread still busy then is left running: the process is
    /// about to exit.
    pub fn shutdown(mut self, bound: Duration) {
        let _ = self.tx.send(StoreCmd::Quit);
        if self.done.recv_timeout(bound).is_ok()
            && let Some(thread) = self.thread.take()
        {
            let _ = thread.join();
        }
    }
}

/// Starts the store thread with a way to open the database. Replies leave
/// over `replies`, in the order the commands arrived.
pub fn spawn(
    open: impl FnOnce() -> Result<Connection, StoreError> + Send + 'static,
    replies: mpsc::Sender<StoreReply>,
) -> StoreHandle {
    spawn_with(
        |body| thread::Builder::new().name(THREAD_NAME.into()).spawn(body),
        open,
        replies,
    )
}

/// `spawn` with the thread start injected, so a failing start can be tested.
fn spawn_with(
    start: impl FnOnce(Box<dyn FnOnce() + Send>) -> io::Result<JoinHandle<()>>,
    open: impl FnOnce() -> Result<Connection, StoreError> + Send + 'static,
    replies: mpsc::Sender<StoreReply>,
) -> StoreHandle {
    let (tx, rx) = std_mpsc::channel::<StoreCmd>();
    let (done_tx, done) = std_mpsc::channel();
    let thread_replies = replies.clone();
    let body = move || {
        let mut worker = Worker::new(open);
        while let Ok(cmd) = rx.recv() {
            if cmd == StoreCmd::Quit {
                break;
            }
            if let Some(reply) = worker.guarded(cmd) {
                // A closed channel means the loop is gone; the writes
                // still matter, so keep going.
                let _ = thread_replies.blocking_send(reply);
            }
        }
        let _ = done_tx.send(());
    };
    // On failure the body (and its receiver) is dropped, so `send` sees a
    // closed queue and answers for the thread.
    let thread = start(Box::new(body)).ok();
    StoreHandle {
        tx,
        replies,
        done,
        thread,
    }
}

/// Starts the store thread on the database at `path`, or on none.
pub fn spawn_at(path: Option<PathBuf>, replies: mpsc::Sender<StoreReply>) -> StoreHandle {
    spawn(
        move || match path {
            Some(path) => open(&path),
            None => Err(StoreError::NoPath),
        },
        replies,
    )
}

#[cfg(test)]
mod tests;
