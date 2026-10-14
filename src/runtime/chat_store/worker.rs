//! The store thread: one `std::thread` owns the connection and runs the
//! commands of every pane in the order they were sent.

use std::{
    collections::HashMap,
    io,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    sync::mpsc as std_mpsc,
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use tokio::sync::mpsc;

use super::{StoreError, open};
use crate::core::{
    ai::{
        chat::{
            Msg, MsgStatus,
            engine::Role,
            store::{
                ConvRow, FIRST_MAX, LinkKind, NoteRow, RETENTION_LIMIT, StoreCmd, StoreReply,
                StoredMessage, bounded_error,
            },
            tags::Tags,
        },
        sanitize::clean,
    },
    layout::PaneId,
};
use crate::runtime::workspace::project_tag;

type Opener = Box<dyn FnOnce() -> Result<Connection, StoreError> + Send>;

/// Name of the store thread; the panic hook keys off it.
pub const THREAD_NAME: &str = "chat-store";

/// The longest conversation title, in characters.
const TITLE_MAX: usize = 80;

/// The longest note title kept in a link row, in characters (the engram client
/// accepts 120).
const NOTE_TITLE_MAX: usize = 120;

/// Runs commands against the database. The connection is opened on the first
/// command, so a session that never chats never touches the data directory.
pub(crate) struct Worker {
    opener: Option<Opener>,
    conn: Option<Result<Connection, String>>,
    /// The previous write failed: only the first failure of a run is notified.
    failing: bool,
    /// The conversation each pane created, so an append that names only its
    /// pane lands in the right place. Pane ids are never reused.
    created: HashMap<PaneId, i64>,
}

impl Worker {
    pub(crate) fn new(
        open: impl FnOnce() -> Result<Connection, StoreError> + Send + 'static,
    ) -> Self {
        Self {
            opener: Some(Box::new(open)),
            conn: None,
            failing: false,
            created: HashMap::new(),
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
                extras,
                first,
                origin,
                open,
            } => {
                // The pane may have sent before its list answered.
                let tag = tag.or_else(|| origin.as_deref().and_then(project_tag));
                // A pane that creates again must not keep pointing at the
                // conversation it had if this one fails.
                self.created.remove(&pane);
                let conn = match self.conn() {
                    Ok(conn) => conn,
                    Err(reason) => return Some(StoreReply::Unavailable { pane, reason }),
                };
                let tags = Tags::new(tag, extras);
                let made = create(conn, &profile, &tags, &first, &open);
                Some(match made {
                    Ok((conversation, gone)) => {
                        self.failing = false;
                        self.created.insert(pane, conversation);
                        StoreReply::Created {
                            pane,
                            conversation,
                            evicted: gone.conversations > 0,
                            evicted_notes: gone.notes,
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
                // Without an id the pane's own `Create` must have made one; if
                // it failed, that was reported already and this is dropped.
                let conversation = conversation.or_else(|| self.created.get(&pane).copied())?;
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
            StoreCmd::LinkSession {
                pane,
                conversation,
                session_id,
                project,
            } => {
                let reply = self
                    .conn()
                    .ok()
                    .and_then(|conn| link_session(conn, conversation, &session_id, &project).ok());
                Some(match reply {
                    Some((session_id, project)) => StoreReply::SessionLinked {
                        pane,
                        conversation,
                        session_id,
                        project,
                    },
                    None => StoreReply::LinkFailed {
                        pane,
                        kind: LinkKind::Session,
                    },
                })
            }
            StoreCmd::LinkNote {
                pane,
                conversation,
                project,
                topic_key,
                observation_id,
                title,
            } => {
                let linked = self.conn().ok().is_some_and(|conn| {
                    link_note(
                        conn,
                        conversation,
                        (&project, &topic_key),
                        observation_id,
                        &title,
                    )
                    .is_ok()
                });
                (!linked).then_some(StoreReply::LinkFailed {
                    pane,
                    kind: LinkKind::Note,
                })
            }
            StoreCmd::SetTags {
                pane,
                conversation,
                tags,
            } => {
                // Without an id the pane's own `Create` made one; if it
                // failed, that was reported already and this is dropped.
                let conversation = conversation.or_else(|| self.created.get(&pane).copied())?;
                let tags = Tags::new(tags.project, tags.extras);
                let saved = self
                    .conn()
                    .ok()
                    .and_then(|conn| replace_tags(conn, conversation, &tags).ok());
                Some(match saved {
                    Some(updated_at) => StoreReply::TagsSaved {
                        pane,
                        conversation,
                        tags,
                        updated_at,
                    },
                    None => StoreReply::TagsFailed { pane },
                })
            }
            StoreCmd::List { pane, origin } => {
                // The walk to the repository root is filesystem IO: it is
                // done here, not on the UI thread.
                let tag = origin.as_deref().and_then(project_tag);
                Some(match self.conn() {
                    Err(reason) => StoreReply::Unavailable { pane, reason },
                    Ok(conn) => match list(conn) {
                        Ok(rows) => StoreReply::Listed { pane, tag, rows },
                        Err(_) => StoreReply::ListFailed { pane, tag },
                    },
                })
            }
            StoreCmd::Resume { pane, conversation } => {
                let read = self
                    .conn()
                    .ok()
                    .and_then(|conn| resume(conn, conversation).ok());
                Some(match read {
                    Some(Resumed {
                        profile,
                        tag,
                        extras,
                        messages,
                        session,
                    }) => StoreReply::Resumed {
                        pane,
                        conversation,
                        profile,
                        tag,
                        extras,
                        messages,
                        session,
                    },
                    None => StoreReply::ResumeFailed { pane },
                })
            }
            StoreCmd::Delete { pane, conversation } => {
                let gone = self
                    .conn()
                    .ok()
                    .and_then(|conn| delete(conn, conversation).ok());
                if gone.is_some() {
                    // No pane appends to it any more.
                    self.created.retain(|_, id| *id != conversation);
                }
                Some(match gone {
                    Some(notes) => StoreReply::Deleted {
                        pane,
                        conversation,
                        notes,
                    },
                    None => StoreReply::DeleteFailed { pane },
                })
            }
        }
    }

    /// `handle` that survives a panic. A panic in a `Create` or `Append`
    /// leaves the connection in an unknown state, so the store is marked
    /// unusable: the command that panicked gets a reply (`Unavailable` for a
    /// `Create`, `Failed` for an `Append`) and every later `Create` answers
    /// `Unavailable`. A panic in a link command answers `LinkFailed` and keeps
    /// an open connection, so one bad link does not take the history down, but
    /// only when the connection is back in autocommit (no transaction left
    /// open); otherwise the store is marked unusable like the others. The
    /// thread keeps running so it still drains its queue and honours `Quit`.
    pub(crate) fn guarded(&mut self, cmd: StoreCmd) -> Option<StoreReply> {
        self.guarded_with(cmd, Self::handle)
    }

    fn guarded_with(
        &mut self,
        cmd: StoreCmd,
        run: impl FnOnce(&mut Self, StoreCmd) -> Option<StoreReply>,
    ) -> Option<StoreReply> {
        let (pane, on_panic) = match &cmd {
            StoreCmd::Create { pane, .. } => (*pane, Panicked::Create),
            StoreCmd::Append { pane, .. } => (*pane, Panicked::Append),
            StoreCmd::LinkSession { pane, .. } => (*pane, Panicked::Link(LinkKind::Session)),
            StoreCmd::LinkNote { pane, .. } => (*pane, Panicked::Link(LinkKind::Note)),
            StoreCmd::List { pane, .. } => (*pane, Panicked::List),
            StoreCmd::Resume { pane, .. } => (*pane, Panicked::Resume),
            StoreCmd::Delete { pane, .. } => (*pane, Panicked::Delete),
            StoreCmd::SetTags { pane, .. } => (*pane, Panicked::SetTags),
            StoreCmd::Quit => return None,
        };
        match catch_unwind(AssertUnwindSafe(|| run(self, cmd))) {
            Ok(reply) => reply,
            Err(payload) => {
                let reason = bounded_error(&panic_text(payload.as_ref()));
                // A link runs in one transaction that rolled back on unwind, so
                // an open connection in autocommit is still sound and the
                // history stays available; any other panic, or a link that
                // left a transaction open, leaves it in an unknown state.
                let keep = matches!(
                    on_panic,
                    Panicked::Link(_) | Panicked::Resume | Panicked::Delete | Panicked::SetTags
                ) && matches!(&self.conn, Some(Ok(conn)) if conn.is_autocommit());
                if !keep {
                    self.conn = Some(Err(reason.clone()));
                }
                self.opener = None;
                Some(match on_panic {
                    Panicked::Create => StoreReply::Unavailable { pane, reason },
                    Panicked::Append => self.failure(pane),
                    Panicked::Link(kind) => StoreReply::LinkFailed { pane, kind },
                    Panicked::List => StoreReply::Unavailable { pane, reason },
                    Panicked::Resume => StoreReply::ResumeFailed { pane },
                    Panicked::Delete => StoreReply::DeleteFailed { pane },
                    Panicked::SetTags => StoreReply::TagsFailed { pane },
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

/// What a command that panicked is answered with.
enum Panicked {
    Create,
    Append,
    Link(LinkKind),
    List,
    Resume,
    Delete,
    SetTags,
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

/// What retention removed to make room.
struct Evicted {
    conversations: usize,
    /// Note rows that went with them (the notes themselves stay in engram).
    notes: u32,
}

/// Creates the conversation and its first message in one transaction, first
/// making room under the retention limit. Returns the id and what was
/// evicted; the notes are counted in the same transaction, before the cascade
/// removes their rows.
fn create(
    conn: &mut Connection,
    profile: &str,
    tags: &Tags,
    first: &StoredMessage,
    open: &[i64],
) -> Result<(i64, Evicted), rusqlite::Error> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let stored: i64 = tx.query_row("SELECT COUNT(*) FROM conversations", [], |r| r.get(0))?;
    let excess = usize::try_from(stored)
        .unwrap_or(usize::MAX)
        .saturating_add(1)
        .saturating_sub(RETENTION_LIMIT);
    let mut evicted = Evicted {
        conversations: 0,
        notes: 0,
    };
    if excess > 0 {
        // The oldest last activity goes first; tags do not protect, open
        // conversations are skipped. Ids tie-break so the order is stable.
        let marks = (0..open.len())
            .map(|i| format!("?{}", i + 2))
            .collect::<Vec<_>>()
            .join(",");
        let pick = format!(
            "SELECT id FROM conversations WHERE id NOT IN ({marks})
             ORDER BY updated_at, id LIMIT ?1"
        );
        let limit = i64::try_from(excess).unwrap_or(i64::MAX);
        let params = || std::iter::once(limit).chain(open.iter().copied());
        let notes: i64 = tx.query_row(
            &format!("SELECT COUNT(*) FROM memory_notes WHERE conversation_id IN ({pick})"),
            rusqlite::params_from_iter(params()),
            |r| r.get(0),
        )?;
        evicted.notes = u32::try_from(notes).unwrap_or(u32::MAX);
        evicted.conversations = tx.execute(
            &format!("DELETE FROM conversations WHERE id IN ({pick})"),
            rusqlite::params_from_iter(params()),
        )?;
    }
    let text = clean(&first.text);
    let stamp = now();
    tx.execute(
        "INSERT INTO conversations(created_at, updated_at, profile, title)
         VALUES (?1, ?1, ?2, ?3)",
        (stamp, profile, title_of(&text)),
    )?;
    let id = tx.last_insert_rowid();
    insert_tags(&tx, id, tags)?;
    insert_message(&tx, id, first, &text, stamp)?;
    tx.commit()?;
    Ok((id, evicted))
}

/// Writes the project tag and the extras of a conversation that has none.
fn insert_tags(
    tx: &rusqlite::Transaction<'_>,
    conversation: i64,
    tags: &Tags,
) -> Result<(), rusqlite::Error> {
    let rows = tags
        .project
        .iter()
        .map(|tag| (tag, 1))
        .chain(tags.extras.iter().map(|tag| (tag, 0)));
    for (tag, is_project) in rows {
        tx.execute(
            "INSERT INTO tags(conversation_id, tag, is_project) VALUES (?1, ?2, ?3)",
            (conversation, tag, is_project),
        )?;
    }
    Ok(())
}

/// Replaces the tags of a conversation and bumps its last activity, in one
/// transaction, and returns that last activity. A conversation that does not
/// exist is an error.
fn replace_tags(
    conn: &mut Connection,
    conversation: i64,
    tags: &Tags,
) -> Result<i64, rusqlite::Error> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let stamp = now();
    let touched = tx.execute(
        "UPDATE conversations SET updated_at = ?2 WHERE id = ?1",
        (conversation, stamp),
    )?;
    if touched == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    tx.execute(
        "DELETE FROM tags WHERE conversation_id = ?1",
        [conversation],
    )?;
    insert_tags(&tx, conversation, tags)?;
    tx.commit()?;
    Ok(stamp)
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

/// Whether `id` has the form of a session id this build makes:
/// `lazarobox-` and 32 lowercase hex digits.
fn is_session_id(id: &str) -> bool {
    id.strip_prefix("lazarobox-").is_some_and(|hex| {
        hex.len() == 32 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    })
}

/// The `(session id, project)` of `conversation`: the stored pair, else the
/// proposed one, which is stored first.
fn link_session(
    conn: &mut Connection,
    conversation: i64,
    session_id: &str,
    project: &str,
) -> Result<(String, String), rusqlite::Error> {
    if !is_session_id(session_id) {
        return Err(rusqlite::Error::InvalidParameterName("session id".into()));
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute(
        "INSERT OR IGNORE INTO memory_sessions(conversation_id, session_id, project)
         VALUES (?1, ?2, ?3)",
        (conversation, session_id, clean(project)),
    )?;
    let stored = tx.query_row(
        "SELECT session_id, project FROM memory_sessions WHERE conversation_id = ?1",
        [conversation],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    tx.commit()?;
    Ok(stored)
}

/// Upserts the note row: the last conversation to write a note owns it.
fn link_note(
    conn: &mut Connection,
    conversation: i64,
    (project, topic_key): (&str, &str),
    observation_id: i64,
    title: &str,
) -> Result<(), rusqlite::Error> {
    let title: String = clean(title)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(NOTE_TITLE_MAX)
        .collect();
    conn.execute(
        "INSERT INTO memory_notes(project, topic_key, observation_id, conversation_id, title)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(project, topic_key) DO UPDATE SET
             observation_id = excluded.observation_id,
             conversation_id = excluded.conversation_id,
             title = excluded.title",
        (
            clean(project),
            clean(topic_key),
            observation_id,
            conversation,
            title,
        ),
    )?;
    Ok(())
}

/// Every conversation, newest last activity first, with its project tag, the
/// first user message and the number of note links it has.
fn list(conn: &mut Connection) -> Result<Vec<ConvRow>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.updated_at, c.profile,
                (SELECT tag FROM tags t WHERE t.conversation_id = c.id AND t.is_project = 1),
                COALESCE((SELECT substr(content, 1, ?1) FROM messages m
                          WHERE m.conversation_id = c.id AND m.role = 'user'
                          ORDER BY seq LIMIT 1), ''),
                (SELECT count(*) FROM memory_notes n WHERE n.conversation_id = c.id)
         FROM conversations c
         ORDER BY c.updated_at DESC, c.id DESC",
    )?;
    let limit = i64::try_from(FIRST_MAX).unwrap_or(i64::MAX);
    let mut rows = stmt
        .query_map([limit], |r| {
            Ok(ConvRow {
                id: r.get(0)?,
                updated_at: r.get(1)?,
                profile: r.get(2)?,
                tag: r.get(3)?,
                extras: Vec::new(),
                first: r.get(4)?,
                notes: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    // The extras are kept by name (the key of the table has no position), so
    // they come back in alphabetical order.
    let mut extras =
        conn.prepare("SELECT conversation_id, tag FROM tags WHERE is_project = 0 ORDER BY tag")?;
    let pairs = extras
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    for (conversation, tag) in pairs {
        if let Some(row) = rows.iter_mut().find(|row| row.id == conversation) {
            row.extras.push(tag);
        }
    }
    Ok(rows)
}

/// What a stored conversation holds.
struct Resumed {
    profile: String,
    tag: Option<String>,
    extras: Vec<String>,
    messages: Vec<Msg>,
    /// The engram `(session id, project)` pair of its notes, if it has one.
    session: Option<(String, String)>,
}

/// The profile, tags and messages of a conversation, in order.
fn resume(conn: &mut Connection, conversation: i64) -> Result<Resumed, rusqlite::Error> {
    let tx = conn.transaction()?;
    let profile: String = tx.query_row(
        "SELECT profile FROM conversations WHERE id = ?1",
        [conversation],
        |r| r.get(0),
    )?;
    let tag = tx
        .query_row(
            "SELECT tag FROM tags WHERE conversation_id = ?1 AND is_project = 1",
            [conversation],
            |r| r.get(0),
        )
        .ok();
    let extras = tx
        .prepare("SELECT tag FROM tags WHERE conversation_id = ?1 AND is_project = 0 ORDER BY tag")?
        .query_map([conversation], |r| r.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    let messages = tx
        .prepare(
            "SELECT role, content, status FROM messages
             WHERE conversation_id = ?1 ORDER BY seq",
        )?
        .query_map([conversation], |r| {
            let role: String = r.get(0)?;
            let status: String = r.get(2)?;
            Ok(Msg {
                role: if role == "user" {
                    Role::User
                } else {
                    Role::Assistant
                },
                text: r.get(1)?,
                // The column is constrained to the known names; an unknown one
                // reads as a cut message rather than a finished one.
                status: MsgStatus::from_name(&status).unwrap_or(MsgStatus::Interrupted),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    // A read only: the resume never writes a link.
    let session = tx
        .query_row(
            "SELECT session_id, project FROM memory_sessions WHERE conversation_id = ?1",
            [conversation],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(Resumed {
        profile,
        tag,
        extras,
        messages,
        session,
    })
}

/// Removes the conversation (its messages, tags and links go with it) and
/// returns the note rows it had, read before the cascade removes them.
fn delete(conn: &mut Connection, conversation: i64) -> Result<Vec<NoteRow>, rusqlite::Error> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let notes = tx
        .prepare(
            "SELECT project, observation_id FROM memory_notes
             WHERE conversation_id = ?1 ORDER BY project, topic_key",
        )?
        .query_map([conversation], |r| {
            Ok(NoteRow {
                project: r.get(0)?,
                observation_id: r.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let removed = tx.execute("DELETE FROM conversations WHERE id = ?1", [conversation])?;
    if removed == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    tx.commit()?;
    Ok(notes)
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
    /// With no thread behind the queue (it never started) a `Create` or a
    /// `List` is answered `Unavailable`, a `Resume` and a `Delete` with their
    /// failures, so the pane detaches or reports instead of waiting.
    pub fn send(&self, cmd: StoreCmd) {
        let Err(std_mpsc::SendError(cmd)) = self.tx.send(cmd) else {
            return;
        };
        let reason = bounded_error("store thread not running");
        let reply = match cmd {
            StoreCmd::Create { pane, .. } | StoreCmd::List { pane, .. } => {
                StoreReply::Unavailable { pane, reason }
            }
            StoreCmd::Resume { pane, .. } => StoreReply::ResumeFailed { pane },
            StoreCmd::Delete { pane, .. } => StoreReply::DeleteFailed { pane },
            StoreCmd::SetTags { pane, .. } => StoreReply::TagsFailed { pane },
            _ => return,
        };
        let _ = self.replies.try_send(reply);
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
