use rusqlite::Connection;

use super::*;
use crate::core::ai::chat::dashboard::{HourCell, Span};
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
    assert_eq!(data.matches("{DAY_SQL}").count(), 5);
    assert_eq!(data.matches("julianday(").count(), 2, "only the const");
}

/// The local weekday (0 Sunday) and hour SQLite reads for `ts`.
fn local(conn: &Connection, ts: i64) -> (u8, u8) {
    conn.query_row(
        "SELECT CAST(strftime('%w', ?1, 'unixepoch', 'localtime') AS INTEGER),
                CAST(strftime('%H', ?1, 'unixepoch', 'localtime') AS INTEGER)",
        [ts],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap()
}

#[test]
fn hours_are_placed_by_the_local_end_time_with_monday_first() {
    let conn = store();
    put(&conn, chat(AT, Some(1), 90));
    let data = usage_data(&conn, None, AT).unwrap();
    let (w, h) = local(&conn, AT);
    // Sunday is the last row: `(w + 6) % 7`.
    let want = HourCell {
        days_ago: 0,
        weekday: (w + 6) % 7,
        hour: h,
        // The row holds input 10 and the output given.
        tokens: 100,
    };
    assert_eq!(data.hours, [want]);
}

#[test]
fn records_of_one_hour_add_up_and_another_hour_or_day_is_its_own_cell() {
    let conn = store();
    // The same hour twice, the next hour, and the same hour a week back.
    put(&conn, chat(AT, Some(1), 10));
    put(&conn, chat(AT, Some(1), 20));
    put(&conn, chat(AT + 3_600, Some(1), 5));
    put(&conn, chat(AT - 7 * DAY, Some(1), 1));
    let data = usage_data(&conn, None, AT + 3_600).unwrap();
    // Each record holds input 10: 20 + 30 in one cell, 15, and 11 a week back.
    let mut got: Vec<u64> = data.hours.iter().map(|c| c.tokens).collect();
    got.sort_unstable();
    assert_eq!(got, [11, 15, 50]);
    assert_eq!(data.hours.len(), 3);
}

#[test]
fn a_record_with_no_known_tokens_still_makes_its_cell() {
    let conn = store();
    conn.execute(
        "INSERT INTO turn_usage(ts, engine, role, input, output)
         VALUES (?1, 'ollama', 'chat', NULL, NULL)",
        [AT],
    )
    .unwrap();
    let data = usage_data(&conn, None, AT).unwrap();
    assert_eq!(data.hours.len(), 1);
    assert_eq!(data.hours[0].tokens, 0);
}

#[test]
fn a_store_without_the_table_has_no_hours() {
    let conn = Connection::open_in_memory().unwrap();
    assert!(usage_data(&conn, None, AT).unwrap().hours.is_empty());
}

/// The usage of Friday 20:00 UTC read in a zone 5 h 30 min ahead (POSIX
/// `IST-5:30`, no tz database needed) is Saturday 01:30. The zone is set on a
/// child run of this test, so no other test sees the change; without
/// `localtime` the cell would be Friday 20:00.
#[test]
fn hours_follow_the_local_zone_not_utc() {
    const FRIDAY_2000_UTC: i64 = 1_800_043_200;
    if std::env::var_os("LAZAROBOX_HEAT_TZ_CHILD").is_none() {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "--nocapture"])
            .arg("runtime::chat_store::usage_data::tests::hours_follow_the_local_zone_not_utc")
            .env("TZ", "IST-5:30")
            .env("LAZAROBOX_HEAT_TZ_CHILD", "1")
            .output()
            .unwrap();
        let log = String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{log}");
        assert!(log.contains("1 passed"), "the child ran nothing: {log}");
        return;
    }
    let conn = store();
    put(&conn, chat(FRIDAY_2000_UTC, Some(1), 90));
    let data = usage_data(&conn, None, FRIDAY_2000_UTC).unwrap();
    // Saturday is the sixth row (Monday first), at hour 1.
    let want = HourCell {
        days_ago: 0,
        weekday: 5,
        hour: 1,
        tokens: 100,
    };
    assert_eq!(data.hours, [want]);
}

fn started(conn: &Connection, secs: i64) {
    conn.execute(
        "UPDATE turn_usage SET started_at = ts - ?1 WHERE id = (SELECT MAX(id) FROM turn_usage)",
        [secs],
    )
    .unwrap();
}

#[test]
fn spans_are_the_worker_runs_with_a_start_and_leave_chat_turns_out() {
    let conn = store();
    put(&conn, worker(AT, "1.a", Some(10_000)));
    started(&conn, 10);
    put(&conn, worker(AT - DAY, "1.b", Some(5_000)));
    started(&conn, 5);
    // No start (an older record), a chat turn and a compaction run: none is a span.
    put(&conn, worker(AT, "1.c", None));
    put(&conn, chat(AT, Some(1), 1));
    started(&conn, 3);
    let data = usage_data(&conn, None, AT).unwrap();
    assert_eq!(
        data.spans,
        [
            Span {
                days_ago: 1,
                start: AT - DAY - 5,
                end: AT - DAY
            },
            Span {
                days_ago: 0,
                start: AT - 10,
                end: AT
            },
        ]
    );
}

#[test]
fn agent_days_sum_the_worker_rows_of_each_engine_with_their_wall_time() {
    let conn = store();
    put(&conn, worker(AT, "1.a", Some(2_000)));
    put(&conn, worker(AT, "1.b", Some(500)));
    put(&conn, worker(AT, "1.c", None));
    put(&conn, chat(AT, Some(1), 7));
    let data = usage_data(&conn, None, AT).unwrap();
    let [day] = data.agents.as_slice() else {
        panic!("{:?}", data.agents)
    };
    assert_eq!(
        (day.days_ago, day.engine.as_str(), day.workers),
        (0, "claude", 3)
    );
    assert_eq!((day.input, day.output), (Some(30), Some(3)));
    assert_eq!((day.wall_ms, day.timed), (Some(2_500), 2));
    assert!(!day.partial);
}

#[test]
fn a_store_without_the_timing_columns_still_reads_the_agents() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE turn_usage(id INTEGER PRIMARY KEY, ts INTEGER, conversation_id INTEGER,
             engine TEXT, model TEXT, role TEXT, worker TEXT, input INTEGER, output INTEGER,
             cache_read INTEGER, cache_write INTEGER, cost_micros INTEGER, partial INTEGER);
         INSERT INTO turn_usage(ts, engine, role, worker, input, output, partial)
             VALUES (1800000000, 'claude', 'worker', '1.a', 5, 6, 0);",
    )
    .unwrap();
    let data = usage_data(&conn, None, AT).unwrap();
    assert!(data.spans.is_empty());
    assert_eq!(data.agents.len(), 1);
    assert_eq!((data.agents[0].wall_ms, data.agents[0].timed), (None, 0));
}

#[test]
fn token_sums_past_i64_saturate_instead_of_failing_the_read() {
    let conn = store();
    for _ in 0..2 {
        put(&conn, chat(AT, Some(1), i64::MAX));
        put(&conn, worker(AT, "9.a", Some(1_000)));
    }
    conn.execute(
        "UPDATE turn_usage SET input = ?1, cache_read = ?1, cost_micros = ?1",
        [i64::MAX],
    )
    .unwrap();
    let data = usage_data(&conn, Some(1), AT).unwrap();
    let big = u64::MAX / 2;
    let row = data.rows.iter().find(|r| r.in_chat).unwrap();
    assert_eq!(row.output, Some(big));
    assert_eq!(row.input, Some(big));
    assert_eq!(row.cache_read, Some(big));
    assert_eq!(row.cost_micros, Some(big));
    assert_eq!(row.total, Some(big));
    let agent = &data.agents[0];
    assert_eq!(agent.input, Some(big));
    assert!(data.hours.iter().all(|h| h.tokens == big));
}

#[test]
fn a_column_with_no_value_stays_none_under_the_saturating_sum() {
    let conn = store();
    conn.execute(
        "INSERT INTO turn_usage(ts, engine, model, role, output) VALUES (?1, 'claude', 'm', 'chat', 5)",
        [AT],
    )
    .unwrap();
    let data = usage_data(&conn, None, AT).unwrap();
    assert_eq!(data.rows[0].input, None);
    assert_eq!(data.rows[0].output, Some(5));
}
