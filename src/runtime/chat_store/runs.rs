//! The orchestrator runs in the chat database (agent-orchestrator S9). The
//! tables are added with `IF NOT EXISTS` and no schema version, like the
//! folder column: an older build never touches them, so it keeps working on
//! the same file, and a file from before them just gains them on open. Nothing
//! writes to them unless a run happens.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OpenFlags, TransactionBehavior};

use super::StoreError;
use crate::core::ai::orchestra::record::{RunPhase, RunRecord, WorkerRecord};

/// The most runs the store keeps; a live run is never removed.
pub const RUNS_KEPT: usize = 50;

/// How long an interrupted run stays offered before recovery closes it, in
/// seconds (30 days).
const STALE_AFTER: i64 = 30 * 24 * 60 * 60;
/// How long the run-id floor waits for a writer before it gives up.
const FLOOR_BUSY: Duration = Duration::from_millis(100);

/// Who owns a live run: the process id plus a token of this process only.
/// A pid alone is not enough: after a reboot or in a container a dead owner's
/// pid can be ours, or belong to an unrelated live process. The token tells a
/// row of this process from a row of an earlier one that had the same pid.
/// Limit: an unrelated live process holding a dead owner's pid (not ours) is
/// not told apart without the process start time, which is not read portably
/// here, so that row stays `live` until that pid is free. The same goes for
/// two instances in different pid namespaces (containers) that share one
/// database: equal pids make each look dead or reused to the other, so one can
/// falsely mark the other's live run as interrupted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Owner {
    pub pid: u32,
    pub token: i64,
}

impl Owner {
    /// This process: its pid and a token drawn once from the clock and the pid.
    pub(crate) fn this_process() -> Self {
        static TOKEN: OnceLock<i64> = OnceLock::new();
        let pid = std::process::id();
        let token = *TOKEN.get_or_init(|| {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            ((nanos ^ (u128::from(pid) << 64)) as u64).rotate_left(13) as i64
        });
        Self { pid, token }
    }
}

/// Creates the tables and adds the `token` column to a `runs` table made
/// before it existed (a nullable column, so nothing else changes).
pub(super) fn ensure(tx: &rusqlite::Transaction<'_>) -> Result<(), rusqlite::Error> {
    tx.execute_batch(SCHEMA_RUNS)?;
    // Columns added after the table first shipped: nullable, so nothing else
    // changes and an older build ignores them.
    for (column, kind) in [("token", "INTEGER"), ("folder", "TEXT")] {
        let present: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('runs') WHERE name = ?1)",
            [column],
            |r| r.get(0),
        )?;
        if !present {
            tx.execute_batch(&format!("ALTER TABLE runs ADD COLUMN {column} {kind}"))?;
        }
    }
    Ok(())
}

pub(super) const SCHEMA_RUNS: &str = "
CREATE TABLE IF NOT EXISTS runs (
    id         INTEGER PRIMARY KEY,
    owner      INTEGER NOT NULL,
    token      INTEGER,
    folder     TEXT,
    goal       TEXT NOT NULL,
    phase      TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS run_workers (
    run_id INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    worker INTEGER NOT NULL,
    kind   TEXT NOT NULL,
    engine TEXT NOT NULL,
    model  TEXT,
    status TEXT NOT NULL,
    PRIMARY KEY (run_id, worker)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS run_high (
    id   INTEGER PRIMARY KEY CHECK (id = 1),
    high INTEGER NOT NULL
);
";

fn id_of(run: u64) -> Result<i64, rusqlite::Error> {
    i64::try_from(run).map_err(|_| rusqlite::Error::InvalidQuery)
}

/// Stores `run` (owned by `owner`) in one transaction: the row is upserted,
/// the workers it carries are upserted and the others kept, the highest id
/// issued is raised, and the oldest runs beyond [`RUNS_KEPT`] go (a live one
/// never). A run another instance marked `interrupted` is not flipped back to
/// live (nor its workers touched); only its end closes it.
pub(crate) fn save_run(
    conn: &mut Connection,
    run: &RunRecord,
    owner: Owner,
    now: i64,
) -> Result<(), rusqlite::Error> {
    let id = id_of(run.id)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let written = tx.execute(
        "INSERT INTO runs(id, owner, token, goal, phase, created_at, updated_at, folder)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET goal = ?4, phase = ?5, updated_at = ?6
         WHERE runs.phase = 'live' OR excluded.phase = 'ended'",
        (
            id,
            owner.pid,
            owner.token,
            &run.goal,
            run.phase.id(),
            now,
            run.folder.as_deref().and_then(Path::to_str),
        ),
    )?;
    // The first save of a resumed run closes the run it resumes, if nobody
    // did already (another instance may have resumed the same row).
    if let Some(old) = run.supersedes {
        tx.execute(
            "UPDATE runs SET phase = 'ended', updated_at = ?2
             WHERE id = ?1 AND phase = 'interrupted'",
            (id_of(old)?, now),
        )?;
    }
    tx.execute(
        "INSERT INTO run_high(id, high) VALUES (1, ?1)
         ON CONFLICT(id) DO UPDATE SET high = max(high, ?1)",
        [id],
    )?;
    if written > 0 {
        for w in &run.workers {
            tx.execute(
                "INSERT INTO run_workers(run_id, worker, kind, engine, model, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(run_id, worker) DO UPDATE SET
                     kind = excluded.kind, engine = excluded.engine,
                     model = excluded.model, status = excluded.status",
                (id, w.id, &w.kind, &w.engine, &w.model, &w.status),
            )?;
        }
    }
    let kept = i64::try_from(RUNS_KEPT).unwrap_or(i64::MAX);
    tx.execute(
        "DELETE FROM runs WHERE id IN (
             SELECT id FROM runs WHERE phase <> 'live'
             ORDER BY updated_at DESC, id DESC LIMIT -1 OFFSET ?1)",
        [kept],
    )?;
    tx.commit()
}

/// Marks every live run of another process as interrupted and returns them,
/// with the ones still interrupted from an earlier start (nobody resumed or
/// discarded them), newest first. A row is another process's when its token is not `me`'s (a
/// row without one counts as different) and its pid is gone (`alive`) or is
/// `me`'s own: a reused pid. A run of another instance that still runs is left
/// alone. Limits: see [`Owner`] (an unrelated live process on a dead owner's
/// pid, and instances in different pid namespaces sharing one database).
pub(crate) fn recover_runs(
    conn: &mut Connection,
    me: Owner,
    alive: impl Fn(u32) -> bool,
    now: i64,
) -> Result<Vec<RunRecord>, rusqlite::Error> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // A run nobody answered for [`STALE_AFTER`] is closed, not named again.
    tx.execute(
        "UPDATE runs SET phase = 'ended' WHERE phase = 'interrupted' AND updated_at < ?1",
        [now.saturating_sub(STALE_AFTER)],
    )?;
    let mut found = Vec::new();
    {
        let mut live = tx.prepare(
            "SELECT id, owner, token, goal, phase, folder FROM runs
             WHERE phase IN ('live', 'interrupted') ORDER BY id DESC",
        )?;
        let rows = live.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, u32>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?;
        for row in rows {
            let (id, pid, token, goal, phase, folder) = row?;
            // A run already interrupted and not yet resumed or discarded is
            // named again; a live one only when its owner is gone.
            let gone = token != Some(me.token) && (pid == me.pid || !alive(pid));
            if phase == "interrupted" || gone {
                found.push((id, goal, folder));
            }
        }
    }
    let mut runs = Vec::new();
    for (id, goal, folder) in found {
        tx.execute(
            "UPDATE runs SET phase = 'interrupted', updated_at = ?2
             WHERE id = ?1 AND phase = 'live'",
            (id, now),
        )?;
        let mut stmt = tx.prepare(
            "SELECT worker, kind, engine, model, status FROM run_workers
             WHERE run_id = ?1 ORDER BY worker",
        )?;
        let workers = stmt
            .query_map([id], |r| {
                Ok(WorkerRecord {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    engine: r.get(2)?,
                    model: r.get(3)?,
                    status: r.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        runs.push(RunRecord {
            id: u64::try_from(id).unwrap_or(0),
            goal,
            phase: RunPhase::Interrupted,
            workers,
            folder: folder.map(PathBuf::from),
            supersedes: None,
        });
    }
    tx.commit()?;
    Ok(runs)
}

/// The highest run id the store has seen: 0 when there is no file or no run
/// tables. Run ids start above it, so a clock set back across a restart never
/// reuses one. A read-only open with no migration and a short busy timeout, so
/// a busy database never holds a run start up for long; why it failed is the
/// error.
pub fn highest_run(path: &Path) -> Result<u64, StoreError> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(StoreError::NoPath);
    };
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    let path = match fs::canonicalize(dir) {
        Ok(dir) => dir.join(name),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(err.into()),
    };
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return Err(StoreError::Unsafe("store file is not a regular file")),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(err.into()),
    }
    let conn = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    conn.busy_timeout(FLOOR_BUSY)?;
    let has = |table: &str| -> Result<bool, rusqlite::Error> {
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table],
            |r| r.get(0),
        )
    };
    let mut high = 0i64;
    if has("run_high")? {
        let stored: Option<i64> =
            conn.query_row("SELECT max(high) FROM run_high", [], |r| r.get(0))?;
        high = high.max(stored.unwrap_or(0));
    }
    if has("runs")? {
        let top: Option<i64> = conn.query_row("SELECT max(id) FROM runs", [], |r| r.get(0))?;
        high = high.max(top.unwrap_or(0));
    }
    Ok(u64::try_from(high).unwrap_or(0))
}

/// Whether process `pid` exists (a process of another user counts: it does).
pub(crate) fn pid_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return true;
    };
    // SAFETY: signal 0 only checks that the process exists.
    let sent = unsafe { libc::kill(pid, 0) };
    sent == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

#[cfg(test)]
mod tests;
