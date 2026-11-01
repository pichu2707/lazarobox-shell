//! The store thread: one `std::thread` owns the connection and runs the
//! commands of every pane in the order they were sent.

use std::{
    collections::HashMap,
    io,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::mpsc as std_mpsc,
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use tokio::sync::mpsc;

use super::{StoreError, ensure_usage, open};
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
            usage::TurnUsage,
            usage_view::UsageRow,
        },
        config::Profile,
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

/// Usage rows older than this many days are removed.
const USAGE_DAYS: i64 = 90;

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
    /// Why the prune on open failed, until the next usage reply says so.
    usage_problem: Option<String>,
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
            usage_problem: None,
        }
    }

    /// The open connection, or the bounded reason there is none.
    fn conn(&mut self) -> Result<&mut Connection, String> {
        if self.conn.is_none() {
            let opened = match self.opener.take() {
                Some(open) => open()
                    .map(|mut conn| {
                        // Old usage goes when the store opens; a failure waits
                        // for the next usage reply.
                        if let Err(err) = prune_usage(&mut conn, now()) {
                            self.usage_problem = Some(usage_reason(&err).to_owned());
                        }
                        conn
                    })
                    .map_err(|err| bounded_error(&err.to_string())),
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
                folder,
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
                // The column is open-ended since schema 3: the known profiles
                // are checked here, before anything is written.
                if Profile::from_id(&profile).is_none() {
                    return Some(self.failure(pane));
                }
                let tags = Tags::new(tag, extras);
                let made = create(conn, &profile, &tags, &first, folder.as_deref(), &open);
                Some(match made {
                    Ok((conversation, created_at, gone)) => {
                        self.failing = false;
                        self.created.insert(pane, conversation);
                        StoreReply::Created {
                            pane,
                            conversation,
                            evicted: gone.conversations > 0,
                            evicted_notes: gone.notes,
                            created_at,
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
                replace,
            } => {
                let reply = self.conn().ok().and_then(|conn| {
                    link_session(conn, conversation, (&session_id, &project), replace).ok()
                });
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
            StoreCmd::SetFolder {
                pane,
                conversation,
                folder,
            } => {
                let conversation = conversation.or_else(|| self.created.get(&pane).copied())?;
                if let Ok(conn) = self.conn() {
                    // A failed write only means a resume asks again.
                    let _ = replace_folder(conn, conversation, &folder);
                }
                None
            }
            StoreCmd::RecordUsage {
                pane,
                conversation,
                engine,
                role,
                usage,
            } => {
                let conversation = conversation.or_else(|| self.created.get(&pane).copied());
                let earlier = self.usage_problem.take();
                let saved = match self.conn() {
                    Ok(conn) => record_usage(conn, conversation, (&engine, &role), &usage),
                    Err(reason) => Err(reason),
                };
                Some(match saved {
                    Ok((read, pruned)) => day_reply(pane, read, pruned.or(earlier)),
                    Err(reason) => day_reply(pane, Err(String::new()), Some(reason)),
                })
            }
            // Opening the store may prune old usage, as for any command; the
            // read adds nothing and runs no DDL.
            StoreCmd::UsageToday { pane } => {
                let read = match self.conn() {
                    Ok(conn) => day_figures(conn, now()),
                    Err(reason) => Err(reason),
                };
                Some(day_reply(pane, read, None))
            }
            StoreCmd::UsageReport { pane, conversation } => {
                let conversation = conversation.or_else(|| self.created.get(&pane).copied());
                let rows = self
                    .conn()
                    .ok()
                    .and_then(|conn| usage_report(conn, conversation, now()).ok());
                Some(StoreReply::UsageReport { pane, rows })
            }
            StoreCmd::RecentFolders { pane, limit } => Some(StoreReply::RecentFolders {
                pane,
                // A store that cannot be read has no recent folders.
                folders: self
                    .conn()
                    .ok()
                    .and_then(|conn| recent_folders(conn, limit).ok())
                    .unwrap_or_default(),
            }),
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
                        folder,
                        skipped,
                    }) => {
                        // The pane now follows another conversation: what it
                        // created before must not link its later usage.
                        self.created.remove(&pane);
                        StoreReply::Resumed {
                            pane,
                            conversation,
                            profile,
                            tag,
                            extras,
                            messages,
                            session,
                            folder,
                            skipped,
                        }
                    }
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
            StoreCmd::SetFolder { pane, .. } => (*pane, Panicked::SetFolder),
            StoreCmd::RecentFolders { pane, .. } => (*pane, Panicked::Recent),
            StoreCmd::RecordUsage { pane, .. } => (*pane, Panicked::Usage),
            StoreCmd::UsageToday { pane } => (*pane, Panicked::Today),
            StoreCmd::UsageReport { pane, .. } => (*pane, Panicked::Report),
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
                    Panicked::Link(_)
                        | Panicked::Resume
                        | Panicked::Delete
                        | Panicked::SetTags
                        | Panicked::SetFolder
                        | Panicked::Recent
                        | Panicked::Usage
                        | Panicked::Today
                        | Panicked::Report
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
                    Panicked::SetFolder => return None,
                    Panicked::Recent => StoreReply::RecentFolders {
                        pane,
                        folders: Vec::new(),
                    },
                    Panicked::Report => StoreReply::UsageReport { pane, rows: None },
                    // A fixed reason: panic text can quote anything.
                    // A fixed reason, and a read problem: the write notice is
                    // not spent on a read.
                    Panicked::Today => day_reply(pane, Err("internal error".to_owned()), None),
                    Panicked::Usage => {
                        day_reply(pane, Err(String::new()), Some("internal error".to_owned()))
                    }
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
    Today,
    Create,
    Append,
    Link(LinkKind),
    List,
    Resume,
    Delete,
    SetTags,
    SetFolder,
    Recent,
    Usage,
    Report,
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

/// A short reason for a failed usage statement. It names the kind of failure
/// and never quotes the raw error, which can hold text or a path.
fn usage_reason(err: &rusqlite::Error) -> &'static str {
    use rusqlite::ErrorCode::{
        DatabaseBusy, DatabaseCorrupt, DatabaseLocked, DiskFull, NotADatabase, PermissionDenied,
        ReadOnly, SystemIoFailure,
    };
    match err.sqlite_error_code() {
        Some(DatabaseBusy | DatabaseLocked) => "database busy",
        Some(DiskFull) => "disk full",
        Some(ReadOnly | PermissionDenied) => "database read-only",
        Some(DatabaseCorrupt | NotADatabase) => "database damaged",
        Some(SystemIoFailure) => "disk error",
        _ => "database error",
    }
}

/// Removes usage rows older than [`USAGE_DAYS`] days: a range delete on the
/// timestamp index. Conversations are not touched, and neither is a row
/// stamped after `at` (a clock set back). Returns how many rows went.
fn prune_usage(conn: &mut Connection, at: i64) -> Result<usize, rusqlite::Error> {
    ensure_usage(conn)?;
    // A clock behind the newest row cannot be trusted: nothing goes then. A
    // clock that jumped forward can still prune early.
    conn.execute(
        "DELETE FROM turn_usage WHERE ts < ?1
         AND (SELECT COALESCE(MAX(ts), 0) FROM turn_usage) <= ?2",
        [at.saturating_sub(USAGE_DAYS * 86_400), at],
    )
}

/// The most one stored count can hold, so a day's SUM cannot overflow.
const COUNT_CAP: u64 = 1_000_000_000_000_000;

fn count_of(n: Option<u64>) -> Option<i64> {
    n.map(|n| i64::try_from(n.min(COUNT_CAP)).unwrap_or(i64::MAX))
}

/// Today's figures: the known tokens (input plus output of the records that
/// have both; `None` when there are none) and the dollars per engine.
type DayFigures = (Option<u64>, Vec<(String, u64)>);

/// Stores one usage row, prunes, and returns today's figures (empty when that
/// read failed: the row is stored all the same) plus the reason a prune
/// failed, if it did (the row is kept then).
fn record_usage(
    conn: &mut Connection,
    conversation: Option<i64>,
    (engine, role): (&str, &str),
    usage: &TurnUsage,
) -> Result<(DayRead, Option<String>), String> {
    let at = now();
    ensure_usage(conn).map_err(|err| usage_reason(&err).to_owned())?;
    // The subselect leaves the link NULL for a conversation that is gone.
    conn.execute(
        "INSERT INTO turn_usage(ts, conversation_id, engine, model, role, input, output,
             cache_read, cache_write, reasoning, cost_micros, cost_source, duration_ms, partial)
         VALUES (?1, (SELECT id FROM conversations WHERE id = ?2), ?3, ?4, ?5, ?6, ?7, ?8, ?9,
             ?10, ?11, ?12, ?13, ?14)",
        rusqlite::params![
            at,
            conversation,
            engine,
            usage.model,
            role,
            count_of(usage.input),
            count_of(usage.output),
            count_of(usage.cache_read),
            count_of(usage.cache_write),
            count_of(usage.reasoning),
            count_of(usage.cost_micros),
            usage.cost_source.map(|c| c.id()),
            count_of(usage.duration_ms),
            usage.partial,
        ],
    )
    .map_err(|err| usage_reason(&err).to_owned())?;
    let pruned = prune_usage(conn, at)
        .err()
        .map(|err| usage_reason(&err).to_owned());
    Ok((day_figures(conn, at), pruned))
}

/// Today's figures and the second they stop being today's, or why the day
/// could not be read (a fixed reason, free of text and paths).
type DayRead = Result<(DayFigures, i64), String>;

/// The reply to a usage command: the figures when the day was read, the
/// reason when it was not, and the reason a write failed, if it did.
fn day_reply(pane: PaneId, read: DayRead, problem: Option<String>) -> StoreReply {
    let (figures, read_problem) = match read {
        Ok((figures, until)) => (Some((figures, until)), None),
        Err(reason) => (None, (!reason.is_empty()).then_some(reason)),
    };
    let ((today, cost), until) = figures.map_or(((None, Vec::new()), None), |(f, u)| (f, Some(u)));
    StoreReply::UsageRecorded {
        pane,
        today,
        cost,
        until,
        read_problem,
        problem,
    }
}

/// The figures of the local day of `at`, read without DDL: a store without
/// the table has none. A read that fails is an error, never "no data".
fn day_figures(conn: &Connection, at: i64) -> DayRead {
    const SAME_DAY: &str =
        "date(ts, 'unixepoch', 'localtime') = date(?1, 'unixepoch', 'localtime')";
    let read = |err: rusqlite::Error| usage_reason(&err).to_owned();
    let until: i64 = conn
        .query_row(
            "SELECT CAST(strftime('%s', date(?1, 'unixepoch', 'localtime', '+1 day'), 'utc')
                         AS INTEGER)",
            [at],
            |r| r.get(0),
        )
        .map_err(read)?;
    // A zone with no midnight on a DST day can answer a moment in the past.
    let until = until.max(at.saturating_add(1));
    let present: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = 'turn_usage')",
            [],
            |r| r.get(0),
        )
        .map_err(read)?;
    if !present {
        return Ok(((None, Vec::new()), until));
    }
    let today: Option<i64> = conn
        .query_row(
            &format!("SELECT SUM(input + output) FROM turn_usage WHERE {SAME_DAY}"),
            [at],
            |r| r.get(0),
        )
        .map_err(read)?;
    let cost = conn
        .prepare(&format!(
            "SELECT engine, SUM(cost_micros) FROM turn_usage
             WHERE cost_micros IS NOT NULL AND {SAME_DAY} GROUP BY engine ORDER BY engine"
        ))
        .and_then(|mut q| {
            q.query_map([at], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(read)?;
    let cost = cost
        .into_iter()
        .filter_map(|(engine, micros)| Some((engine, u64::try_from(micros).ok()?)))
        .collect();
    Ok(((today.and_then(|t| u64::try_from(t).ok()), cost), until))
}

/// The usage records folded for `/cost`: by local day (days before `at`),
/// engine and model, with the rows of `conversation` kept apart. A missing
/// count stays NULL (`None`), never 0; a row is partial when a record was or
/// lacked its input or output.
fn usage_report(
    conn: &Connection,
    conversation: Option<i64>,
    at: i64,
) -> Result<Vec<UsageRow>, rusqlite::Error> {
    // Reading never runs DDL: a store without the table has no usage.
    let present: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = 'turn_usage')",
        [],
        |r| r.get(0),
    )?;
    if !present {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT MAX(0, CAST(julianday(date(?1, 'unixepoch', 'localtime'))
                      - julianday(date(ts, 'unixepoch', 'localtime')) AS INTEGER)) AS day,
                engine, model, COALESCE(conversation_id = ?2, 0) AS mine,
                COALESCE(SUM(role = 'chat'), 0), COUNT(*), COUNT(cost_micros),
                SUM(input), SUM(output), SUM(cache_read), SUM(cost_micros),
                SUM(CASE WHEN input IS NOT NULL AND output IS NOT NULL
                         THEN input + output END),
                MAX(partial OR input IS NULL OR output IS NULL)
         FROM turn_usage GROUP BY day, engine, model, mine ORDER BY day, engine, model",
    )?;
    let count = |v: Option<i64>| v.and_then(|n| u64::try_from(n).ok());
    let rows = stmt.query_map(rusqlite::params![at, conversation], |r| {
        Ok(UsageRow {
            days_ago: u32::try_from(r.get::<_, i64>(0)?).unwrap_or(u32::MAX),
            engine: r.get(1)?,
            model: r.get(2)?,
            in_chat: r.get(3)?,
            turns: count(Some(r.get(4)?)).unwrap_or(0),
            records: count(Some(r.get(5)?)).unwrap_or(0),
            costed: count(Some(r.get(6)?)).unwrap_or(0),
            input: count(r.get(7)?),
            output: count(r.get(8)?),
            cache_read: count(r.get(9)?),
            cost_micros: count(r.get(10)?),
            total: count(r.get(11)?),
            partial: r.get(12)?,
        })
    })?;
    rows.collect()
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
    folder: Option<&Path>,
    open: &[i64],
) -> Result<(i64, i64, Evicted), rusqlite::Error> {
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
        "INSERT INTO conversations(created_at, updated_at, profile, title, folder)
         VALUES (?1, ?1, ?2, ?3, ?4)",
        (
            stamp,
            profile,
            title_of(&text),
            folder.and_then(Path::to_str),
        ),
    )?;
    let id = tx.last_insert_rowid();
    insert_tags(&tx, id, tags)?;
    insert_message(&tx, id, first, &text, stamp)?;
    tx.commit()?;
    Ok((id, stamp, evicted))
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

/// Replaces the folder of a conversation and bumps its last activity. A
/// conversation that does not exist is an error. A path that is not UTF-8 is
/// stored as `NULL` (a resume then asks).
fn replace_folder(
    conn: &mut Connection,
    conversation: i64,
    folder: &Path,
) -> Result<(), rusqlite::Error> {
    let touched = conn.execute(
        "UPDATE conversations SET folder = ?2, updated_at = ?3 WHERE id = ?1",
        (conversation, folder.to_str(), now()),
    )?;
    if touched == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    Ok(())
}

/// The folders conversations were kept under, each once, the most recently
/// used first. Read-only.
fn recent_folders(conn: &mut Connection, limit: usize) -> Result<Vec<PathBuf>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT folder FROM conversations WHERE folder IS NOT NULL
         GROUP BY folder ORDER BY MAX(updated_at) DESC, folder LIMIT ?1",
    )?;
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    stmt.query_map([limit], |r| r.get::<_, String>(0))?
        .map(|folder| folder.map(PathBuf::from))
        .collect()
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
    (session_id, project): (&str, &str),
    replace: bool,
) -> Result<(String, String), rusqlite::Error> {
    if !is_session_id(session_id) {
        return Err(rusqlite::Error::InvalidParameterName("session id".into()));
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // The stored pair wins (a resume) unless the conversation changed project.
    let (verb, tail) = if replace {
        (
            "INSERT",
            "ON CONFLICT(conversation_id) DO UPDATE
             SET session_id = excluded.session_id, project = excluded.project",
        )
    } else {
        ("INSERT OR IGNORE", "")
    };
    tx.execute(
        &format!(
            "{verb} INTO memory_sessions(conversation_id, session_id, project)
             VALUES (?1, ?2, ?3) {tail}"
        ),
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
    /// The folder it was kept under, when one was stored.
    folder: Option<PathBuf>,
    /// Rows left out for a role this build does not know.
    skipped: usize,
}

/// The profile, tags and messages of a conversation, in order.
fn resume(conn: &mut Connection, conversation: i64) -> Result<Resumed, rusqlite::Error> {
    let tx = conn.transaction()?;
    let profile: String = tx.query_row(
        "SELECT profile FROM conversations WHERE id = ?1",
        [conversation],
        |r| r.get(0),
    )?;
    let folder: Option<String> = tx.query_row(
        "SELECT folder FROM conversations WHERE id = ?1",
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
    // Role and status are open-ended since schema 3. A row of a role this
    // build does not know (a newer one wrote it) is left out and counted rather
    // than shown as the assistant's; an unknown status reads as a cut message.
    let mut skipped = 0;
    let mut messages = Vec::new();
    {
        let mut stmt = tx.prepare(
            "SELECT role, content, status FROM messages
             WHERE conversation_id = ?1 ORDER BY seq",
        )?;
        let mut rows = stmt.query([conversation])?;
        while let Some(r) = rows.next()? {
            let role = match r.get::<_, String>(0)?.as_str() {
                "user" => Role::User,
                "assistant" => Role::Assistant,
                _ => {
                    skipped += 1;
                    continue;
                }
            };
            let status: String = r.get(2)?;
            messages.push(Msg {
                role,
                text: r.get(1)?,
                status: MsgStatus::from_name(&status).unwrap_or(MsgStatus::Interrupted),
            });
        }
    }
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
        folder: folder.map(PathBuf::from),
        skipped,
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
            StoreCmd::RecentFolders { pane, .. } => StoreReply::RecentFolders {
                pane,
                folders: Vec::new(),
            },
            StoreCmd::RecordUsage { pane, .. } => day_reply(pane, Err(String::new()), Some(reason)),
            StoreCmd::UsageToday { pane } => day_reply(pane, Err(reason), None),
            StoreCmd::UsageReport { pane, .. } => StoreReply::UsageReport { pane, rows: None },
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
