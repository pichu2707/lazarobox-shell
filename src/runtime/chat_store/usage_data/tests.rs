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
fn rows_carry_the_chat_output_and_the_worker_wall_time() {
    let conn = store();
    put(&conn, chat(AT, Some(1), 100));
    put(&conn, chat(AT, Some(1), 50));
    put(&conn, worker(AT, "1.a", Some(2_000)));
    put(&conn, worker(AT, "1.b", Some(500)));
    put(&conn, worker(AT, "1.c", None));
    let data = usage_data(&conn, None, AT).unwrap();
    let [row] = data.rows.as_slice() else {
        panic!("one row, got {:?}", data.rows);
    };
    // R1: the average divides this by the chat turns, which is what `turns` is.
    assert_eq!(row.turns, 2);
    assert_eq!(row.chat_output, Some(150));
    assert_eq!(row.output, Some(153));
    assert_eq!(row.workers, 3);
    assert_eq!(row.worker_wall_ms, Some(2_500));
    assert_eq!(row.worker_timed, 2);
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
    assert_eq!((row.worker_wall_ms, row.worker_timed), (None, 0));
    assert_eq!(row.workers, 1);
    assert_eq!(keys(&data, 0), ["c1", "r1"]);
}

#[test]
fn a_store_without_usage_reads_nothing() {
    let conn = Connection::open_in_memory().unwrap();
    assert_eq!(usage_data(&conn, None, AT).unwrap(), UsageData::default());
}
