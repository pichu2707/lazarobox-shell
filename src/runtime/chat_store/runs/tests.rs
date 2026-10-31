//! The runs tables against real SQLite files in temp dirs.

use tempfile::TempDir;

use super::*;
use crate::runtime::chat_store::open_as;

fn me() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

fn db(dir: &TempDir) -> std::path::PathBuf {
    dir.path().join("lazarobox").join("chat.db")
}

fn worker(id: u32, status: &str) -> WorkerRecord {
    WorkerRecord {
        id,
        kind: "explore".into(),
        engine: "claude".into(),
        model: Some("opus".into()),
        status: status.into(),
    }
}

fn run(id: u64, phase: RunPhase, workers: Vec<WorkerRecord>) -> RunRecord {
    RunRecord {
        id,
        goal: format!("goal {id}"),
        phase,
        workers,
        folder: None,
        supersedes: None,
    }
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

const DEAD: u32 = 4_000_000;

fn owner(pid: u32, token: i64) -> Owner {
    Owner { pid, token }
}

#[test]
fn a_database_from_before_the_runs_gains_empty_tables_and_keeps_its_version() {
    let dir = TempDir::new().unwrap();
    let path = db(&dir);
    {
        let conn = open_as(&path, me()).unwrap();
        conn.execute_batch(
            "INSERT INTO conversations(id, created_at, updated_at, profile, title)
             VALUES (7, 1, 1, 'programmer', 'old');
             DROP TABLE run_workers; DROP TABLE run_high; DROP TABLE runs;",
        )
        .unwrap();
    }
    let conn = open_as(&path, me()).unwrap();
    for table in ["runs", "run_workers", "run_high"] {
        assert_eq!(count(&conn, table), 0, "{table}");
    }
    assert_eq!(count(&conn, "conversations"), 1);
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 3);
}

#[test]
fn saving_a_run_again_updates_its_phase_and_the_workers_it_carries() {
    let dir = TempDir::new().unwrap();
    let mut conn = open_as(&db(&dir), me()).unwrap();
    let first = run(
        10,
        RunPhase::Live,
        vec![worker(1, "running"), worker(2, "queued")],
    );
    save_run(&mut conn, &first, owner(1, 1), 100).unwrap();
    let next = run(10, RunPhase::Ended, vec![worker(1, "done")]);
    save_run(&mut conn, &next, owner(1, 1), 200).unwrap();
    assert_eq!(count(&conn, "runs"), 1);
    assert_eq!(count(&conn, "run_workers"), 2, "worker 2 was not carried");
    let (phase, created, updated): (String, i64, i64) = conn
        .query_row("SELECT phase, created_at, updated_at FROM runs", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!((phase.as_str(), created, updated), ("ended", 100, 200));
}

#[test]
fn the_highest_id_survives_the_runs_that_carried_it() {
    let dir = TempDir::new().unwrap();
    let path = db(&dir);
    assert_eq!(highest_run(&path), Ok(0));
    let mut conn = open_as(&path, me()).unwrap();
    save_run(
        &mut conn,
        &run(900, RunPhase::Ended, vec![]),
        owner(1, 1),
        1,
    )
    .unwrap();
    save_run(
        &mut conn,
        &run(500, RunPhase::Ended, vec![]),
        owner(1, 1),
        2,
    )
    .unwrap();
    conn.execute("DELETE FROM runs", []).unwrap();
    drop(conn);
    assert_eq!(highest_run(&path), Ok(900));
}

#[test]
fn recovery_interrupts_only_live_runs_whose_owner_is_gone() {
    let dir = TempDir::new().unwrap();
    let mut conn = open_as(&db(&dir), me()).unwrap();
    let live = vec![worker(1, "running"), worker(2, "done")];
    save_run(&mut conn, &run(1, RunPhase::Live, live), owner(DEAD, 9), 10).unwrap();
    save_run(&mut conn, &run(2, RunPhase::Live, vec![]), owner(77, 9), 10).unwrap(); // owner runs
    save_run(&mut conn, &run(3, RunPhase::Live, vec![]), owner(55, 5), 10).unwrap(); // this process
    save_run(
        &mut conn,
        &run(4, RunPhase::Ended, vec![]),
        owner(DEAD, 9),
        10,
    )
    .unwrap();
    let found = recover_runs(&mut conn, owner(55, 5), |pid| pid == 77, 20).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].id, found[0].phase), (1, RunPhase::Interrupted));
    assert_eq!(found[0].goal, "goal 1");
    assert_eq!(
        found[0].workers,
        vec![worker(1, "running"), worker(2, "done")]
    );
    let phases: Vec<(i64, String)> = conn
        .prepare("SELECT id, phase FROM runs ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let want = [(1, "interrupted"), (2, "live"), (3, "live"), (4, "ended")];
    assert_eq!(phases, want.map(|(i, p)| (i, p.to_owned())));
    // Asked again while that owner still runs, only the run nobody answered
    // yet is named (see the next test); the live ones are left alone.
    let again = recover_runs(&mut conn, owner(55, 5), |pid| pid == 77, 30).unwrap();
    assert_eq!(again.iter().map(|r| r.id).collect::<Vec<_>>(), [1]);
}

// Spec: Restart shows an interrupted run (9.1c). A run nobody resumed or
// discarded is named again at the next restart; ending it (the resume or the
// discard) stops that.
#[test]
fn an_interrupted_run_nobody_answered_is_named_again_until_it_ends() {
    let dir = TempDir::new().unwrap();
    let mut conn = open_as(&db(&dir), me()).unwrap();
    let live = vec![worker(1, "running")];
    save_run(&mut conn, &run(1, RunPhase::Live, live), owner(DEAD, 9), 10).unwrap();
    let first = recover_runs(&mut conn, owner(55, 5), |_| false, 20).unwrap();
    assert_eq!(first.len(), 1);
    let second = recover_runs(&mut conn, owner(56, 6), |_| false, 30).unwrap();
    assert_eq!(second.len(), 1, "named again");
    assert_eq!(second[0].workers, vec![worker(1, "running")]);
    assert_eq!(second[0].phase, RunPhase::Interrupted);
    let ended = run(1, RunPhase::Ended, vec![]);
    save_run(&mut conn, &ended, owner(56, 6), 40).unwrap();
    let third = recover_runs(&mut conn, owner(57, 7), |_| false, 50).unwrap();
    assert!(third.is_empty(), "{third:?}");
}

#[test]
fn retention_keeps_the_newest_and_never_removes_a_live_run() {
    let dir = TempDir::new().unwrap();
    let mut conn = open_as(&db(&dir), me()).unwrap();
    save_run(
        &mut conn,
        &run(1, RunPhase::Live, vec![worker(1, "running")]),
        owner(1, 1),
        1,
    )
    .unwrap();
    for id in 2..=(RUNS_KEPT as u64 + 5) {
        let at = i64::try_from(id).unwrap();
        save_run(
            &mut conn,
            &run(id, RunPhase::Ended, vec![worker(1, "done")]),
            owner(1, 1),
            at,
        )
        .unwrap();
    }
    assert_eq!(count(&conn, "runs"), RUNS_KEPT as i64 + 1);
    let oldest_ended: i64 = conn
        .query_row("SELECT min(id) FROM runs WHERE phase = 'ended'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(oldest_ended, 6);
    assert_eq!(count(&conn, "runs WHERE id = 1"), 1);
    assert_eq!(count(&conn, "run_workers"), RUNS_KEPT as i64 + 1);
}

#[test]
fn a_run_id_beyond_the_database_range_is_refused() {
    let dir = TempDir::new().unwrap();
    let mut conn = open_as(&db(&dir), me()).unwrap();
    assert!(
        save_run(
            &mut conn,
            &run(u64::MAX, RunPhase::Live, vec![]),
            owner(1, 1),
            1
        )
        .is_err()
    );
    assert_eq!(count(&conn, "runs"), 0);
}

#[test]
fn this_process_and_a_nonsense_pid_are_alive_a_free_one_is_not() {
    assert!(pid_alive(std::process::id()));
    assert!(!pid_alive(DEAD));
}

// ---- review fixes: owner tokens, no flip back, one-worker upserts, the floor

fn phase_of(conn: &Connection, id: i64) -> String {
    conn.query_row("SELECT phase FROM runs WHERE id = ?1", [id], |r| r.get(0))
        .unwrap()
}

#[test]
fn a_reused_pid_with_another_token_is_interrupted_and_the_same_token_is_not() {
    let dir = TempDir::new().unwrap();
    let mut conn = open_as(&db(&dir), me()).unwrap();
    // After a reboot the dead owner's pid can be ours: only the token differs.
    save_run(&mut conn, &run(1, RunPhase::Live, vec![]), owner(55, 1), 1).unwrap();
    save_run(&mut conn, &run(2, RunPhase::Live, vec![]), owner(55, 5), 1).unwrap();
    // Another pid that is still alive, another token: another instance's run.
    save_run(&mut conn, &run(3, RunPhase::Live, vec![]), owner(77, 2), 1).unwrap();
    let found = recover_runs(&mut conn, owner(55, 5), |pid| pid == 77, 2).unwrap();
    assert_eq!(found.iter().map(|r| r.id).collect::<Vec<_>>(), [1]);
    assert_eq!(phase_of(&conn, 1), "interrupted");
    assert_eq!(phase_of(&conn, 2), "live");
    assert_eq!(phase_of(&conn, 3), "live");
}

#[test]
fn a_row_without_a_token_is_found_when_its_pid_is_ours_or_gone() {
    let dir = TempDir::new().unwrap();
    let mut conn = open_as(&db(&dir), me()).unwrap();
    for (id, pid) in [(1, 55), (2, DEAD), (3, 77)] {
        conn.execute(
            "INSERT INTO runs(id, owner, goal, phase, created_at, updated_at)
             VALUES (?1, ?2, 'g', 'live', 1, 1)",
            (id, pid),
        )
        .unwrap();
    }
    let found = recover_runs(&mut conn, owner(55, 5), |pid| pid == 77, 2).unwrap();
    let mut ids: Vec<_> = found.iter().map(|r| r.id).collect();
    ids.sort_unstable();
    assert_eq!(ids, [1, 2]);
}

#[test]
fn a_runs_table_without_the_token_column_gains_it_and_keeps_its_rows() {
    let dir = TempDir::new().unwrap();
    let path = db(&dir);
    {
        let conn = open_as(&path, me()).unwrap();
        conn.execute_batch(
            "DROP TABLE run_workers; DROP TABLE runs;
             CREATE TABLE runs (id INTEGER PRIMARY KEY, owner INTEGER NOT NULL,
                 goal TEXT NOT NULL, phase TEXT NOT NULL,
                 created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
             CREATE TABLE run_workers (
                 run_id INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
                 worker INTEGER NOT NULL, kind TEXT NOT NULL, engine TEXT NOT NULL,
                 model TEXT, status TEXT NOT NULL, PRIMARY KEY (run_id, worker)
             ) WITHOUT ROWID;
             INSERT INTO runs VALUES (4, 1, 'old', 'ended', 1, 1);",
        )
        .unwrap();
    }
    let mut conn = open_as(&path, me()).unwrap();
    let columns: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('runs')")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(columns.contains(&"token".to_owned()), "{columns:?}");
    assert_eq!(count(&conn, "runs"), 1);
    save_run(&mut conn, &run(5, RunPhase::Live, vec![]), owner(1, 1), 2).unwrap();
}

#[test]
fn a_saved_live_run_never_flips_a_run_another_instance_interrupted() {
    let dir = TempDir::new().unwrap();
    let mut conn = open_as(&db(&dir), me()).unwrap();
    save_run(
        &mut conn,
        &run(1, RunPhase::Live, vec![worker(1, "running")]),
        owner(1, 1),
        1,
    )
    .unwrap();
    conn.execute("UPDATE runs SET phase = 'interrupted'", [])
        .unwrap();
    let late = run(
        1,
        RunPhase::Live,
        vec![worker(1, "done"), worker(2, "queued")],
    );
    save_run(&mut conn, &late, owner(1, 1), 2).unwrap();
    assert_eq!(phase_of(&conn, 1), "interrupted");
    assert_eq!(
        count(&conn, "run_workers"),
        1,
        "its workers were not touched"
    );
    // The run's real end still closes it.
    save_run(&mut conn, &run(1, RunPhase::Ended, vec![]), owner(1, 1), 3).unwrap();
    assert_eq!(phase_of(&conn, 1), "ended");
}

#[test]
fn a_save_upserts_the_workers_it_carries_and_keeps_the_others() {
    let dir = TempDir::new().unwrap();
    let mut conn = open_as(&db(&dir), me()).unwrap();
    let live = |ws| run(1, RunPhase::Live, ws);
    save_run(&mut conn, &live(vec![worker(1, "running")]), owner(1, 1), 1).unwrap();
    save_run(&mut conn, &live(vec![worker(2, "queued")]), owner(1, 1), 2).unwrap();
    save_run(&mut conn, &live(vec![worker(1, "done")]), owner(1, 1), 3).unwrap();
    let rows: Vec<(u32, String)> = conn
        .prepare("SELECT worker, status FROM run_workers ORDER BY worker")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(rows, [(1, "done".to_owned()), (2, "queued".to_owned())]);
}

#[test]
fn the_floor_reads_a_file_without_the_tables_as_zero_and_never_migrates_it() {
    let dir = TempDir::new().unwrap();
    let path = db(&dir);
    {
        let conn = open_as(&path, me()).unwrap();
        conn.execute_batch("DROP TABLE run_workers; DROP TABLE run_high; DROP TABLE runs;")
            .unwrap();
    }
    assert_eq!(highest_run(&path), Ok(0));
    let conn = Connection::open(&path).unwrap();
    assert_eq!(count_tables(&conn, "runs"), 0, "reading created nothing");
}

fn count_tables(conn: &Connection, name: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE name = ?1",
        [name],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn a_locked_database_fails_the_floor_quickly_and_says_why() {
    let dir = TempDir::new().unwrap();
    let path = db(&dir);
    drop(open_as(&path, me()).unwrap());
    let holder = Connection::open(&path).unwrap();
    holder.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let started = std::time::Instant::now();
    let read = highest_run(&path);
    assert!(started.elapsed() < std::time::Duration::from_millis(1500));
    assert_eq!(read, Err(StoreError::Sqlite("database is locked")));
}

fn phase_of_run(conn: &Connection, id: i64) -> String {
    conn.query_row("SELECT phase FROM runs WHERE id = ?1", [id], |r| r.get(0))
        .unwrap()
}

fn in_folder(mut r: RunRecord, folder: &str) -> RunRecord {
    r.folder = Some(folder.into());
    r
}

// 9.1c review W1: a run keeps the folder it started in, and recovery gives it
// back; a run stored before the column has none.
#[test]
fn a_runs_folder_is_stored_and_recovered_and_a_run_without_one_has_none() {
    let dir = TempDir::new().unwrap();
    let mut conn = open_as(&db(&dir), me()).unwrap();
    let a = in_folder(run(1, RunPhase::Live, vec![]), "/work/a");
    save_run(&mut conn, &a, owner(DEAD, 9), 10).unwrap();
    save_run(
        &mut conn,
        &run(2, RunPhase::Live, vec![]),
        owner(DEAD, 9),
        10,
    )
    .unwrap();
    let found = recover_runs(&mut conn, owner(55, 5), |_| false, 20).unwrap();
    let folders: Vec<_> = found.iter().map(|r| (r.id, r.folder.clone())).collect();
    assert_eq!(folders, [(2, None), (1, Some("/work/a".into()))]);
    // A later save does not change where the run started.
    let moved = in_folder(run(1, RunPhase::Ended, vec![]), "/work/b");
    save_run(&mut conn, &moved, owner(DEAD, 9), 30).unwrap();
    let kept: String = conn
        .query_row("SELECT folder FROM runs WHERE id = 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(kept, "/work/a");
}

#[test]
fn a_runs_table_without_the_folder_column_gains_it_and_keeps_its_rows() {
    let dir = TempDir::new().unwrap();
    let path = db(&dir);
    {
        let conn = open_as(&path, me()).unwrap();
        conn.execute_batch(
            "DROP TABLE run_workers; DROP TABLE runs;
             CREATE TABLE runs (id INTEGER PRIMARY KEY, owner INTEGER NOT NULL,
                 token INTEGER, goal TEXT NOT NULL, phase TEXT NOT NULL,
                 created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
             CREATE TABLE run_workers (
                 run_id INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
                 worker INTEGER NOT NULL, kind TEXT NOT NULL, engine TEXT NOT NULL,
                 model TEXT, status TEXT NOT NULL, PRIMARY KEY (run_id, worker)
             ) WITHOUT ROWID;
             INSERT INTO runs VALUES (4, 1, NULL, 'old', 'interrupted', 1, 1);",
        )
        .unwrap();
    }
    let mut conn = open_as(&path, me()).unwrap();
    let found = recover_runs(&mut conn, owner(55, 5), |_| false, 2).unwrap();
    assert_eq!((found.len(), found[0].folder.clone()), (1, None));
    let a = in_folder(run(5, RunPhase::Live, vec![]), "/w");
    save_run(&mut conn, &a, owner(1, 1), 3).unwrap();
}

// 9.1c review W2: a resumed run's first save closes the run it supersedes, and
// only if that row is still interrupted (compare and set).
#[test]
fn a_run_that_supersedes_closes_the_old_row_only_while_it_is_interrupted() {
    let dir = TempDir::new().unwrap();
    let mut conn = open_as(&db(&dir), me()).unwrap();
    for id in [1, 2] {
        let r = run(id, RunPhase::Live, vec![worker(1, "running")]);
        save_run(&mut conn, &r, owner(DEAD, 9), 10).unwrap();
    }
    recover_runs(&mut conn, owner(55, 5), |_| false, 20).unwrap();
    let mut new = run(10, RunPhase::Live, vec![worker(1, "running")]);
    new.supersedes = Some(1);
    save_run(&mut conn, &new, owner(55, 5), 30).unwrap();
    assert_eq!(phase_of_run(&conn, 1), "ended");
    assert_eq!(phase_of_run(&conn, 2), "interrupted");
    // Another instance resuming the same row finds it closed already: no-op.
    let mut again = run(11, RunPhase::Live, vec![]);
    again.supersedes = Some(1);
    save_run(&mut conn, &again, owner(56, 6), 40).unwrap();
    assert_eq!(phase_of_run(&conn, 1), "ended");
    // A row that is not interrupted is never closed this way.
    let mut live = run(12, RunPhase::Live, vec![]);
    live.supersedes = Some(10);
    save_run(&mut conn, &live, owner(55, 5), 50).unwrap();
    assert_eq!(phase_of_run(&conn, 10), "live");
}

// 9.1c review S4: an interrupted run older than 30 days is closed at recovery
// and not named.
#[test]
fn an_interrupted_run_older_than_thirty_days_is_ended_not_named() {
    let dir = TempDir::new().unwrap();
    let mut conn = open_as(&db(&dir), me()).unwrap();
    save_run(
        &mut conn,
        &run(1, RunPhase::Live, vec![]),
        owner(DEAD, 9),
        1_000,
    )
    .unwrap();
    let first = recover_runs(&mut conn, owner(55, 5), |_| false, 1_000).unwrap();
    assert_eq!(first.len(), 1);
    let day = 24 * 60 * 60;
    let soon = recover_runs(&mut conn, owner(55, 5), |_| false, 1_000 + 29 * day).unwrap();
    assert_eq!(soon.len(), 1, "29 days");
    let late = recover_runs(&mut conn, owner(55, 5), |_| false, 1_000 + 31 * day).unwrap();
    assert!(late.is_empty(), "31 days");
    assert_eq!(phase_of_run(&conn, 1), "ended");
}
