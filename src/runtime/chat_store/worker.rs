//! The store thread: one `std::thread` owns the connection and runs the
//! commands of every pane in the order they were sent.

use std::{
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

    fn failure(&mut self, pane: PaneId) -> StoreReply {
        let notify = !self.failing;
        self.failing = true;
        StoreReply::Failed { pane, notify }
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// The first line of the first message, cut to `TITLE_MAX` characters.
fn title_of(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
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
    done: std_mpsc::Receiver<()>,
    thread: Option<JoinHandle<()>>,
}

impl StoreHandle {
    /// Queues `cmd`; never waits for the database.
    pub fn send(&self, cmd: StoreCmd) {
        let _ = self.tx.send(cmd);
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
    let (tx, rx) = std_mpsc::channel::<StoreCmd>();
    let (done_tx, done) = std_mpsc::channel();
    let thread = thread::Builder::new()
        .name("chat-store".into())
        .spawn(move || {
            let mut worker = Worker::new(open);
            while let Ok(cmd) = rx.recv() {
                if cmd == StoreCmd::Quit {
                    break;
                }
                if let Some(reply) = worker.handle(cmd) {
                    // A closed channel means the loop is gone; the writes
                    // still matter, so keep going.
                    let _ = replies.blocking_send(reply);
                }
            }
            let _ = done_tx.send(());
        })
        .ok();
    StoreHandle { tx, done, thread }
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
