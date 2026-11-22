use rusqlite::Connection;

use super::*;
use crate::runtime::chat_store::ensure_usage;

const AT: i64 = 1_800_000_000;
const DAY: i64 = 86_400;

fn store() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    // The usage table links to it; only the link matters here.
    conn.execute_batch(
        "CREATE TABLE conversations (id INTEGER PRIMARY KEY);
         INSERT INTO conversations(id) VALUES (1), (2);",
    )
    .unwrap();
    ensure_usage(&mut conn).unwrap();
    conn
}

struct Rec<'a> {
    ts: i64,
    conv: Option<i64>,
    role: &'a str,
    worker: Option<&'a str>,
    output: i64,
    wall_ms: Option<i64>,
}

fn put(conn: &Connection, r: Rec) {
    conn.execute(
        "INSERT INTO turn_usage(ts, conversation_id, engine, model, role, worker, input, output,
             wall_ms)
         VALUES (?1, ?2, 'claude', 'm', ?3, ?4, 10, ?5, ?6)",
        rusqlite::params![r.ts, r.conv, r.role, r.worker, r.output, r.wall_ms],
    )
    .unwrap();
}

fn chat(ts: i64, conv: Option<i64>, output: i64) -> Rec<'static> {
    Rec {
        ts,
        conv,
        role: "chat",
        worker: None,
        output,
        wall_ms: None,
    }
}

fn linked(ts: i64, conv: i64, name: &'static str) -> Rec<'static> {
    Rec {
        conv: Some(conv),
        ..worker(ts, name, Some(1_000))
    }
}

fn worker(ts: i64, name: &'static str, wall_ms: Option<i64>) -> Rec<'static> {
    Rec {
        ts,
        conv: None,
        role: "worker",
        worker: Some(name),
        output: 1,
        wall_ms,
    }
}

fn keys(data: &UsageData, day: u32) -> Vec<&str> {
    let mut keys: Vec<&str> = data
        .tasks
        .iter()
        .filter(|(d, _)| *d == day)
        .map(|(_, k)| k.as_str())
        .collect();
    keys.sort_unstable();
    keys
}

#[test]
fn tasks_are_distinct_conversations_and_runs_with_deleted_chats_left_out() {
    let conn = store();
    for _ in 0..5 {
        put(&conn, chat(AT, Some(1), 10));
    }
    for _ in 0..2 {
        put(&conn, chat(AT, Some(2), 10));
    }
    for w in ["9.a", "9.b", "9.c"] {
        put(&conn, worker(AT, w, Some(1_000)));
    }
    // A chat deleted since: its tokens stay, its link is NULL.
    put(&conn, chat(AT, None, 10));
    // A chat that went on two days is one pair per day.
    put(&conn, chat(AT - 2 * DAY, Some(1), 10));
    let data = usage_data(&conn, None, AT).unwrap();
    assert_eq!(keys(&data, 0), ["c1", "c2", "r9"]);
    assert_eq!(keys(&data, 2), ["c1"]);
    assert_eq!(data.tasks.len(), 4);
}

#[test]
fn a_run_launched_from_a_chat_is_a_task_of_its_own() {
    let conn = store();
    // Chat A x5 and chat B x2, plus 3 worker rows of one run linked to A.
    for _ in 0..5 {
        put(&conn, chat(AT, Some(1), 10));
    }
    for _ in 0..2 {
        put(&conn, chat(AT, Some(2), 10));
    }
    for w in ["9.a", "9.b", "9.c"] {
        put(&conn, linked(AT, 1, w));
    }
    let data = usage_data(&conn, None, AT).unwrap();
    assert_eq!(keys(&data, 0), ["c1", "c2", "r9"]);
}

#[test]
fn rows_carry_the_chat_output_and_the_wall_time_of_chat_and_worker_records() {
    let conn = store();
    put(
        &conn,
        Rec {
            wall_ms: Some(4_000),
            ..chat(AT, Some(1), 100)
        },
    );
    put(&conn, chat(AT, Some(1), 50));
    put(&conn, worker(AT, "1.a", Some(2_000)));
    put(&conn, worker(AT, "1.b", Some(500)));
    put(&conn, worker(AT, "1.c", None));
    // A compaction run is not agent time, even with a wall time.
    put(
        &conn,
        Rec {
            role: "compaction",
            wall_ms: Some(9_000),
            ..chat(AT, Some(1), 7)
        },
    );
    let data = usage_data(&conn, None, AT).unwrap();
    let [row] = data.rows.as_slice() else {
        panic!("one row, got {:?}", data.rows);
    };
    // R1: the average divides this by the chat turns that have an output.
    assert_eq!(row.turns, 2);
    assert_eq!(row.chat_output, Some(150));
    assert_eq!(row.chat_outputs, 2);
    assert_eq!(row.output, Some(160));
    assert_eq!(row.workers, 3);
    assert_eq!(row.wall_ms, Some(6_500));
    assert_eq!(row.wall_timed, 3);
}

#[test]
fn an_old_file_without_the_timing_columns_reads_no_wall_time() {
    let conn = store();
    put(&conn, chat(AT, Some(1), 10));
    put(&conn, worker(AT, "1.a", None));
    conn.execute_batch(
        "ALTER TABLE turn_usage DROP COLUMN started_at;
         ALTER TABLE turn_usage DROP COLUMN wall_ms;",
    )
    .unwrap();
    let data = usage_data(&conn, None, AT).unwrap();
    let [row] = data.rows.as_slice() else {
        panic!("one row");
    };
    assert_eq!((row.wall_ms, row.wall_timed), (None, 0));
    assert_eq!(row.workers, 1);
    assert_eq!(keys(&data, 0), ["c1", "r1"]);
}

#[test]
fn a_store_without_usage_reads_nothing() {
    let conn = Connection::open_in_memory().unwrap();
    assert_eq!(usage_data(&conn, None, AT).unwrap(), UsageData::default());
}

#[test]
fn rows_and_tasks_agree_on_the_day_across_local_midnight() {
    let conn = store();
    let local = |at: &str| -> i64 {
        conn.query_row(
            &format!("SELECT CAST(strftime('%s', '{at}', 'utc') AS INTEGER)"),
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    put(&conn, chat(local("2026-06-14 23:59:00"), Some(1), 10));
    put(&conn, chat(local("2026-06-15 00:01:00"), Some(2), 10));
    let data = usage_data(&conn, None, local("2026-06-15 12:00:00")).unwrap();
    let days: Vec<u32> = data.rows.iter().map(|r| r.days_ago).collect();
    assert_eq!(days, [0, 1], "one row per local day");
    assert_eq!(keys(&data, 1), ["c1"]);
    assert_eq!(keys(&data, 0), ["c2"]);
}

#[test]
fn a_worker_name_without_a_run_makes_no_task() {
    let conn = store();
    put(&conn, worker(AT, ".a", Some(1)));
    put(&conn, worker(AT, "nodot", Some(1)));
    let data = usage_data(&conn, None, AT).unwrap();
    assert!(data.tasks.is_empty(), "{:?}", data.tasks);
}

// The test above runs in whatever zone the machine has, and under UTC a UTC
// day and a local day are the same: it cannot tell two day formulas apart.
// So the shared fragment is pinned structurally: only `DAY_SQL` computes it.
#[test]
fn both_queries_take_the_day_from_the_shared_fragment() {
    let worker = include_str!("../worker.rs");
    let data = include_str!("../usage_data.rs");
    assert!(
        !worker.contains("julianday("),
        "worker.rs has its own day SQL"
    );
    assert!(worker.contains("{DAY_SQL}"));
    assert_eq!(data.matches("{DAY_SQL}").count(), 2);
    assert_eq!(data.matches("julianday(").count(), 2, "only the const");
}
