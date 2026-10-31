//! The orchestrator runs in the chat database (agent-orchestrator S9). The
//! tables are added with `IF NOT EXISTS` and no schema version, like the
//! folder column: an older build never touches them, so it keeps working on
//! the same file, and a file from before them just gains them on open. Nothing
//! writes to them unless a run happens.

use std::path::Path;

use rusqlite::{Connection, TransactionBehavior};

use super::open;
use crate::core::ai::orchestra::record::{RunPhase, RunRecord, WorkerRecord};

/// The most runs the store keeps; a live run is never removed.
pub const RUNS_KEPT: usize = 50;

pub(super) const SCHEMA_RUNS: &str = "
CREATE TABLE IF NOT EXISTS runs (
    id         INTEGER PRIMARY KEY,
    owner      INTEGER NOT NULL,
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

/// Stores `run` (owned by process `owner`) with its workers, in one
/// transaction: the row and its workers are replaced, the highest id issued
/// is raised, and the oldest runs beyond [`RUNS_KEPT`] go (a live one never).
pub(crate) fn save_run(
    conn: &mut Connection,
    run: &RunRecord,
    owner: u32,
    now: i64,
) -> Result<(), rusqlite::Error> {
    let id = id_of(run.id)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute(
        "INSERT INTO runs(id, owner, goal, phase, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)
         ON CONFLICT(id) DO UPDATE SET goal = ?3, phase = ?4, updated_at = ?5",
        (id, owner, &run.goal, run.phase.id(), now),
    )?;
    tx.execute("DELETE FROM run_workers WHERE run_id = ?1", [id])?;
    for w in &run.workers {
        tx.execute(
            "INSERT INTO run_workers(run_id, worker, kind, engine, model, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (id, w.id, &w.kind, &w.engine, &w.model, &w.status),
        )?;
    }
    tx.execute(
        "INSERT INTO run_high(id, high) VALUES (1, ?1)
         ON CONFLICT(id) DO UPDATE SET high = max(high, ?1)",
        [id],
    )?;
    let kept = i64::try_from(RUNS_KEPT).unwrap_or(i64::MAX);
    tx.execute(
        "DELETE FROM runs WHERE id IN (
             SELECT id FROM runs WHERE phase <> 'live'
             ORDER BY updated_at DESC, id DESC LIMIT -1 OFFSET ?1)",
        [kept],
    )?;
    tx.commit()
}

/// Marks every live run whose owner is not `me` and no longer `alive` as
/// interrupted and returns them, newest first. A run of another instance that
/// still runs is left alone.
pub(crate) fn recover_runs(
    conn: &mut Connection,
    me: u32,
    alive: impl Fn(u32) -> bool,
    now: i64,
) -> Result<Vec<RunRecord>, rusqlite::Error> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut found = Vec::new();
    {
        let mut live =
            tx.prepare("SELECT id, owner, goal FROM runs WHERE phase = 'live' ORDER BY id DESC")?;
        let rows = live.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, u32>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (id, owner, goal) = row?;
            if owner != me && !alive(owner) {
                found.push((id, goal));
            }
        }
    }
    let mut runs = Vec::new();
    for (id, goal) in found {
        tx.execute(
            "UPDATE runs SET phase = 'interrupted', updated_at = ?2 WHERE id = ?1",
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
        });
    }
    tx.commit()?;
    Ok(runs)
}

/// The highest run id the store has seen, or `None` when it has none or
/// cannot be read. Run ids start above it, so a clock set back across a
/// restart never reuses one.
pub fn highest_run(path: &Path) -> Option<u64> {
    let conn = open(path).ok()?;
    let high: i64 = conn
        .query_row(
            "SELECT max(COALESCE((SELECT high FROM run_high WHERE id = 1), 0),
                        COALESCE((SELECT max(id) FROM runs), 0))",
            [],
            |r| r.get(0),
        )
        .ok()?;
    u64::try_from(high).ok().filter(|high| *high > 0)
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
