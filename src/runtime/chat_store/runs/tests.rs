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
    }
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

const DEAD: u32 = 4_000_000;

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
fn saving_a_run_again_replaces_its_workers_and_phase() {
    let dir = TempDir::new().unwrap();
    let mut conn = open_as(&db(&dir), me()).unwrap();
    let first = run(
        10,
        RunPhase::Live,
        vec![worker(1, "running"), worker(2, "queued")],
    );
    save_run(&mut conn, &first, 1, 100).unwrap();
    let next = run(10, RunPhase::Ended, vec![worker(1, "done")]);
    save_run(&mut conn, &next, 1, 200).unwrap();
    assert_eq!(count(&conn, "runs"), 1);
    assert_eq!(count(&conn, "run_workers"), 1);
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
    assert_eq!(highest_run(&path), None);
    let mut conn = open_as(&path, me()).unwrap();
    save_run(&mut conn, &run(900, RunPhase::Ended, vec![]), 1, 1).unwrap();
    save_run(&mut conn, &run(500, RunPhase::Ended, vec![]), 1, 2).unwrap();
    conn.execute("DELETE FROM runs", []).unwrap();
    drop(conn);
    assert_eq!(highest_run(&path), Some(900));
}

#[test]
fn recovery_interrupts_only_live_runs_whose_owner_is_gone() {
    let dir = TempDir::new().unwrap();
    let mut conn = open_as(&db(&dir), me()).unwrap();
    let live = vec![worker(1, "running"), worker(2, "done")];
    save_run(&mut conn, &run(1, RunPhase::Live, live), DEAD, 10).unwrap();
    save_run(&mut conn, &run(2, RunPhase::Live, vec![]), 77, 10).unwrap(); // owner runs
    save_run(&mut conn, &run(3, RunPhase::Live, vec![]), 55, 10).unwrap(); // this process
    save_run(&mut conn, &run(4, RunPhase::Ended, vec![]), DEAD, 10).unwrap();
    let found = recover_runs(&mut conn, 55, |pid| pid == 77, 20).unwrap();
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
    // Asked again while that owner still runs, nothing is new.
    assert!(
        recover_runs(&mut conn, 55, |pid| pid == 77, 30)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn retention_keeps_the_newest_and_never_removes_a_live_run() {
    let dir = TempDir::new().unwrap();
    let mut conn = open_as(&db(&dir), me()).unwrap();
    save_run(
        &mut conn,
        &run(1, RunPhase::Live, vec![worker(1, "running")]),
        1,
        1,
    )
    .unwrap();
    for id in 2..=(RUNS_KEPT as u64 + 5) {
        let at = i64::try_from(id).unwrap();
        save_run(
            &mut conn,
            &run(id, RunPhase::Ended, vec![worker(1, "done")]),
            1,
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
    assert!(save_run(&mut conn, &run(u64::MAX, RunPhase::Live, vec![]), 1, 1).is_err());
    assert_eq!(count(&conn, "runs"), 0);
}

#[test]
fn this_process_and_a_nonsense_pid_are_alive_a_free_one_is_not() {
    assert!(pid_alive(std::process::id()));
    assert!(!pid_alive(DEAD));
}
