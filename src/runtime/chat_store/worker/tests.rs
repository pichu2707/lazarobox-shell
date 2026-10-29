//! The store thread against real SQLite files in temp dirs.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use rusqlite::Connection;
use tempfile::TempDir;
use tokio::sync::mpsc;

use super::*;
use crate::{
    core::{
        ai::chat::{
            MsgStatus,
            engine::Role,
            store::{ERROR_MAX, LinkKind, StoredMessage},
        },
        layout::PaneId,
    },
    runtime::chat_store::open_as,
};

fn me() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

fn db_path(dir: &TempDir) -> PathBuf {
    dir.path().join("lazarobox").join("chat.db")
}

fn pane() -> PaneId {
    PaneId::FIRST
}

fn msg(role: Role, text: &str, status: MsgStatus) -> StoredMessage {
    StoredMessage {
        role,
        text: text.into(),
        status,
        engine: "ollama".into(),
        model: Some("llama3".into()),
    }
}

fn user(text: &str) -> StoredMessage {
    msg(Role::User, text, MsgStatus::Complete)
}

fn reply_msg(text: &str, status: MsgStatus) -> StoredMessage {
    msg(Role::Assistant, text, status)
}

fn create(first: StoredMessage, open: Vec<i64>) -> StoreCmd {
    StoreCmd::Create {
        pane: pane(),
        profile: "programmer".into(),
        tag: Some("proj".into()),
        extras: vec![],
        first,
        origin: None,
        folder: None,
        open,
    }
}

fn append(conversation: i64, message: StoredMessage) -> StoreCmd {
    StoreCmd::Append {
        pane: pane(),
        conversation: Some(conversation),
        message,
    }
}

/// An append that names only the pane: the thread knows its conversation.
fn append_for_pane(message: StoredMessage) -> StoreCmd {
    StoreCmd::Append {
        pane: pane(),
        conversation: None,
        message,
    }
}

fn worker_at(db: &Path) -> Worker {
    let db = db.to_path_buf();
    Worker::new(move || open_as(&db, me()))
}

/// A worker on a fresh store, plus a second connection to look inside it.
fn fixture() -> (TempDir, Worker, Connection) {
    let dir = TempDir::new().unwrap();
    let db = db_path(&dir);
    let mut worker = worker_at(&db);
    // Open the store with a first, throwaway-free call: the schema must exist
    // before the inspector connects.
    let probe = worker.handle(create(user("probe"), vec![]));
    let Some(StoreReply::Created { conversation, .. }) = probe else {
        panic!("probe not created: {probe:?}");
    };
    let conn = Connection::open(&db).unwrap();
    conn.execute("DELETE FROM conversations WHERE id = ?1", [conversation])
        .unwrap();
    (dir, worker, conn)
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

fn created_id(reply: Option<StoreReply>) -> i64 {
    match reply {
        Some(StoreReply::Created { conversation, .. }) => conversation,
        other => panic!("expected Created, got {other:?}"),
    }
}

/// `(seq, role, content, status)` of a conversation, in order.
fn rows(conn: &Connection, id: i64) -> Vec<(i64, String, String, String)> {
    let mut stmt = conn
        .prepare(
            "SELECT seq, role, content, status FROM messages
             WHERE conversation_id = ?1 ORDER BY seq",
        )
        .unwrap();
    stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn seed(conn: &Connection, n: usize) {
    for i in 0..n {
        let stamp = 1_000 + i as i64;
        conn.execute(
            "INSERT INTO conversations(id, created_at, updated_at, profile, title)
             VALUES (?1, ?2, ?2, 'analyst', ?3)",
            (1_000 + i as i64, stamp, format!("t{i}")),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages(conversation_id, seq, role, content, engine, model, status, created_at)
             VALUES (?1, 1, 'user', 'hi', 'ollama', NULL, 'complete', ?2)",
            (1_000 + i as i64, stamp),
        )
        .unwrap();
    }
}

fn exists(conn: &Connection, id: i64) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM conversations WHERE id = ?1",
        [id],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
        == 1
}

// ---- saving (7.1) ----------------------------------------------------------

#[test]
fn the_first_send_creates_a_conversation_with_profile_tag_and_message() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("hello"), vec![])));
    assert_eq!(
        rows(&conn, id),
        vec![(1, "user".into(), "hello".into(), "complete".into())]
    );
    let (profile, tag, project): (String, String, i64) = conn
        .query_row(
            "SELECT c.profile, t.tag, t.is_project FROM conversations c
             JOIN tags t ON t.conversation_id = c.id WHERE c.id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        (profile.as_str(), tag.as_str(), project),
        ("programmer", "proj", 1)
    );
}

#[test]
fn the_title_is_the_first_line_of_the_first_message_cut_short() {
    let (_dir, mut worker, conn) = fixture();
    let long = format!("first line {}\nsecond", "x".repeat(200));
    let id = created_id(worker.handle(create(user(&long), vec![])));
    let title: String = conn
        .query_row("SELECT title FROM conversations WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(title.starts_with("first line x"));
    assert!(!title.contains('\n'));
    assert!(title.chars().count() <= 80);
}

#[test]
fn a_conversation_without_a_tag_stores_no_tag_row() {
    let (_dir, mut worker, conn) = fixture();
    let cmd = StoreCmd::Create {
        pane: pane(),
        profile: "analyst".into(),
        tag: None,
        extras: vec![],
        first: user("hi"),
        origin: None,
        folder: None,
        open: vec![],
    };
    created_id(worker.handle(cmd));
    assert_eq!(count(&conn, "tags"), 0);
}

#[test]
fn appends_land_in_order_with_their_status_engine_and_model() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("q1"), vec![])));
    for (text, status) in [
        ("a1", MsgStatus::Complete),
        ("Hel", MsgStatus::Cancelled),
        ("cut", MsgStatus::Interrupted),
        ("big", MsgStatus::Truncated),
    ] {
        assert_eq!(worker.handle(append(id, reply_msg(text, status))), None);
    }
    let got: Vec<_> = rows(&conn, id)
        .into_iter()
        .map(|(seq, _, text, status)| (seq, text, status))
        .collect();
    assert_eq!(
        got,
        vec![
            (1, "q1".into(), "complete".into()),
            (2, "a1".into(), "complete".into()),
            (3, "Hel".into(), "cancelled".into()),
            (4, "cut".into(), "interrupted".into()),
            (5, "big".into(), "truncated".into()),
        ]
    );
    let (engine, model): (String, Option<String>) = conn
        .query_row(
            "SELECT engine, model FROM messages WHERE conversation_id = ?1 AND seq = 2",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (engine.as_str(), model.as_deref()),
        ("ollama", Some("llama3"))
    );
}

#[test]
fn an_append_keeps_the_conversation_alive() {
    let (_dir, mut worker, conn) = fixture();
    seed(&conn, 1);
    worker.handle(append(1_000, reply_msg("late", MsgStatus::Complete)));
    let updated: i64 = conn
        .query_row(
            "SELECT updated_at FROM conversations WHERE id = 1000",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(updated > 1_000_000, "updated_at is the clock, not the seed");
}

// Spec (Saving conversations): an answer that failed after partial text is
// stored with status `error`.
#[test]
fn an_error_answer_is_stored_with_its_partial_text() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("q"), vec![])));
    assert_eq!(
        worker.handle(append(id, reply_msg("par", MsgStatus::Error))),
        None
    );
    let stored = rows(&conn, id);
    assert_eq!(stored.len(), 2);
    assert_eq!(
        (
            stored[1].1.as_str(),
            stored[1].2.as_str(),
            stored[1].3.as_str()
        ),
        ("assistant", "par", "error")
    );
}

// The thread maps a pane to the conversation it created, so a pane need not
// wait for the id before it appends.
#[test]
fn an_append_that_names_only_the_pane_lands_in_its_conversation() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("q"), vec![])));
    assert_eq!(
        worker.handle(append_for_pane(reply_msg("A", MsgStatus::Complete))),
        None
    );
    assert_eq!(rows(&conn, id).len(), 2);
    // Another pane's append is not misplaced.
    let other = StoreCmd::Append {
        pane: PaneId::for_test(9),
        conversation: None,
        message: user("stray"),
    };
    assert_eq!(worker.handle(other), None);
    assert_eq!(rows(&conn, id).len(), 2);
    assert_eq!(count(&conn, "messages"), 2);
}

#[test]
fn an_append_after_a_failed_create_is_dropped_silently() {
    let (_dir, mut worker, conn) = fixture();
    let other = PaneId::for_test(9);
    break_writes(&conn);
    let made = worker.handle(StoreCmd::Create {
        pane: other,
        profile: "programmer".into(),
        tag: None,
        extras: vec![],
        first: user("q"),
        origin: None,
        folder: None,
        open: vec![],
    });
    assert!(failed(made));
    mend_writes(&conn);
    let late = StoreCmd::Append {
        pane: other,
        conversation: None,
        message: reply_msg("A", MsgStatus::Complete),
    };
    assert_eq!(worker.handle(late), None);
    assert_eq!(count(&conn, "messages"), 0);
}

#[test]
fn stored_text_is_sanitized() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("x\u{1b}[31my"), vec![])));
    worker.handle(append(id, reply_msg("a\u{1b}[31mb", MsgStatus::Complete)));
    let got: Vec<_> = rows(&conn, id).into_iter().map(|r| r.2).collect();
    assert_eq!(got, vec!["xy".to_string(), "ab".to_string()]);
}

#[test]
fn commands_over_the_thread_run_in_the_order_sent() {
    let dir = TempDir::new().unwrap();
    let db = db_path(&dir);
    let (tx, mut rx) = mpsc::channel(8);
    let handle = {
        let db = db.clone();
        spawn(move || open_as(&db, me()), tx)
    };
    handle.send(create(user("q"), vec![]));
    let Some(StoreReply::Created { conversation, .. }) = rx.blocking_recv() else {
        panic!("no Created reply");
    };
    for i in 0..5 {
        handle.send(append(conversation, user(&format!("m{i}"))));
    }
    handle.shutdown(Duration::from_secs(2));
    let conn = Connection::open(&db).unwrap();
    let texts: Vec<_> = rows(&conn, conversation).into_iter().map(|r| r.2).collect();
    assert_eq!(texts, ["q", "m0", "m1", "m2", "m3", "m4"]);
    assert!(rx.try_recv().is_err(), "appends that succeed send no reply");
}

#[test]
fn a_session_that_never_chats_never_touches_the_data_directory() {
    let dir = TempDir::new().unwrap();
    let db = db_path(&dir);
    let (tx, _rx) = mpsc::channel(1);
    let handle = {
        let db = db.clone();
        spawn(move || open_as(&db, me()), tx)
    };
    handle.shutdown(Duration::from_secs(2));
    assert!(!db.exists() && !db.parent().unwrap().exists());
}

// ---- retention (7.2) -------------------------------------------------------

#[test]
fn the_201st_conversation_evicts_the_one_with_the_oldest_activity() {
    let (_dir, mut worker, conn) = fixture();
    seed(&conn, 200);
    let reply = worker.handle(create(user("new"), vec![]));
    let id = created_id(reply.clone());
    assert!(matches!(
        reply,
        Some(StoreReply::Created { evicted: true, .. })
    ));
    assert_eq!(count(&conn, "conversations"), 200);
    assert!(!exists(&conn, 1_000), "the oldest is gone");
    assert!(exists(&conn, 1_001));
    assert!(exists(&conn, id));
    assert_eq!(count(&conn, "messages"), 200, "its messages went with it");
}

#[test]
fn recent_activity_keeps_the_oldest_by_creation_alive() {
    let (_dir, mut worker, conn) = fixture();
    seed(&conn, 200);
    conn.execute(
        "UPDATE conversations SET updated_at = 9999999 WHERE id = 1000",
        [],
    )
    .unwrap();
    created_id(worker.handle(create(user("new"), vec![])));
    assert!(exists(&conn, 1_000));
    assert!(!exists(&conn, 1_001), "the next oldest by activity went");
}

#[test]
fn tags_do_not_protect_a_conversation() {
    let (_dir, mut worker, conn) = fixture();
    seed(&conn, 200);
    conn.execute(
        "INSERT INTO tags(conversation_id, tag, is_project) VALUES (1000, 'keep', 0)",
        [],
    )
    .unwrap();
    created_id(worker.handle(create(user("new"), vec![])));
    assert!(!exists(&conn, 1_000));
    let tags: i64 = conn
        .query_row("SELECT COUNT(*) FROM tags WHERE tag = 'keep'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(tags, 0);
}

#[test]
fn an_open_conversation_is_skipped() {
    let (_dir, mut worker, conn) = fixture();
    seed(&conn, 200);
    created_id(worker.handle(create(user("new"), vec![1_000])));
    assert!(exists(&conn, 1_000), "open in a pane, so it stays");
    assert!(!exists(&conn, 1_001), "the next oldest went");
}

#[test]
fn a_store_over_the_limit_is_trimmed_to_it() {
    let (_dir, mut worker, conn) = fixture();
    seed(&conn, 205);
    created_id(worker.handle(create(user("new"), vec![])));
    assert_eq!(count(&conn, "conversations"), 200);
    assert!(!exists(&conn, 1_005));
    assert!(exists(&conn, 1_006));
}

#[test]
fn below_the_limit_nothing_is_evicted_and_nothing_is_announced() {
    let (_dir, mut worker, conn) = fixture();
    seed(&conn, 199);
    let reply = worker.handle(create(user("new"), vec![]));
    assert!(matches!(
        reply,
        Some(StoreReply::Created { evicted: false, .. })
    ));
    assert_eq!(count(&conn, "conversations"), 200);
    assert!(exists(&conn, 1_000));
}

#[test]
fn an_evicted_conversation_leaves_no_trace_in_the_file() {
    let (dir, mut worker, conn) = fixture();
    seed(&conn, 200);
    conn.execute(
        "UPDATE messages SET content = 'ZZ-MARKER-ZZ' WHERE conversation_id = 1000",
        [],
    )
    .unwrap();
    created_id(worker.handle(create(user("new"), vec![])));
    let has = |p: &Path| {
        fs::read(p)
            .unwrap_or_default()
            .windows(12)
            .any(|w| w == b"ZZ-MARKER-ZZ")
    };
    let db = db_path(&dir);
    assert!(!has(&db));
    let mut journal = db.as_os_str().to_owned();
    journal.push("-journal");
    assert!(!has(Path::new(&journal)));
}

// ---- degrade (7.3) ---------------------------------------------------------

fn run_once(path: Option<PathBuf>, cmd: StoreCmd) -> StoreReply {
    let (tx, mut rx) = mpsc::channel(4);
    let handle = spawn_at(path, tx);
    handle.send(cmd);
    let reply = rx.blocking_recv().expect("a reply");
    handle.shutdown(Duration::from_secs(2));
    reply
}

fn assert_unavailable(reply: StoreReply) {
    let StoreReply::Unavailable { pane: from, reason } = reply else {
        panic!("expected Unavailable, got {reply:?}");
    };
    assert_eq!(from, pane());
    assert!(
        !reason.is_empty() && reason.chars().count() <= ERROR_MAX,
        "{reason}"
    );
    assert!(!reason.contains('\u{1b}'));
}

#[test]
fn a_corrupt_file_is_unavailable_and_its_bytes_are_untouched() {
    let dir = TempDir::new().unwrap();
    let db = db_path(&dir);
    fs::create_dir_all(db.parent().unwrap()).unwrap();
    let junk: Vec<u8> = (0..4096u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    fs::write(&db, &junk).unwrap();

    assert_unavailable(run_once(Some(db.clone()), create(user("hi"), vec![])));
    assert!(
        fs::read(&db).unwrap() == junk,
        "the corrupt file was changed"
    );
}

#[test]
fn a_data_directory_that_cannot_be_created_is_unavailable() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("lazarobox"), b"a file in the way").unwrap();
    assert_unavailable(run_once(Some(db_path(&dir)), create(user("hi"), vec![])));
}

#[test]
fn no_path_is_unavailable() {
    assert_unavailable(run_once(None, create(user("hi"), vec![])));
}

#[test]
fn an_unavailable_store_stays_unavailable_for_every_create() {
    let (tx, mut rx) = mpsc::channel(4);
    let handle = spawn_at(None, tx);
    handle.send(create(user("1"), vec![]));
    handle.send(create(user("2"), vec![]));
    assert_unavailable(rx.blocking_recv().unwrap());
    assert_unavailable(rx.blocking_recv().unwrap());
    handle.shutdown(Duration::from_secs(2));
}

fn break_writes(conn: &Connection) {
    conn.execute_batch(
        "CREATE TRIGGER no_writes BEFORE INSERT ON messages
         BEGIN SELECT RAISE(ABORT, 'disk on fire'); END;",
    )
    .unwrap();
}

fn mend_writes(conn: &Connection) {
    conn.execute_batch("DROP TRIGGER no_writes").unwrap();
}

fn failed(reply: Option<StoreReply>) -> bool {
    match reply {
        Some(StoreReply::Failed { notify, .. }) => notify,
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn a_failing_write_is_notified_once_per_run() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("q"), vec![])));
    break_writes(&conn);
    assert!(
        failed(worker.handle(append(id, user("two")))),
        "first failure"
    );
    assert!(
        !failed(worker.handle(append(id, user("three")))),
        "same run"
    );

    mend_writes(&conn);
    assert_eq!(worker.handle(append(id, user("four"))), None);
    break_writes(&conn);
    assert!(failed(worker.handle(append(id, user("five")))), "a new run");
}

#[test]
fn a_failed_create_leaves_no_half_conversation() {
    let (_dir, mut worker, conn) = fixture();
    break_writes(&conn);
    assert!(failed(worker.handle(create(user("q"), vec![]))));
    assert_eq!(count(&conn, "conversations"), 0);
    assert_eq!(count(&conn, "tags"), 0);
}

#[test]
fn an_append_to_a_missing_conversation_fails() {
    let (_dir, mut worker, _conn) = fixture();
    assert!(failed(worker.handle(append(424_242, user("x")))));
}

#[test]
fn a_blocked_store_does_not_block_the_sender() {
    let dir = TempDir::new().unwrap();
    let db = db_path(&dir);
    let (tx, mut rx) = mpsc::channel(4);
    let handle = {
        let db = db.clone();
        spawn(move || open_as(&db, me()), tx)
    };
    handle.send(create(user("q"), vec![]));
    let Some(StoreReply::Created { conversation, .. }) = rx.blocking_recv() else {
        panic!("no Created reply");
    };
    // Another process holds the write lock; the append waits on it.
    let holder = Connection::open(&db).unwrap();
    holder.execute_batch("BEGIN IMMEDIATE").unwrap();
    let started = Instant::now();
    for i in 0..50 {
        handle.send(append(conversation, user(&format!("m{i}"))));
    }
    // 50 sends take microseconds; the lock is held until released below, so
    // any wait on the database would run into the 2 s busy timeout. 1 s keeps
    // the assertion meaningful while leaving slow CI machines ample headroom.
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "send waited for the database"
    );
    holder.execute_batch("ROLLBACK").unwrap();
    handle.shutdown(Duration::from_secs(5));
}

#[test]
fn quit_flushes_what_was_queued_before_it() {
    let dir = TempDir::new().unwrap();
    let db = db_path(&dir);
    let (tx, mut rx) = mpsc::channel(4);
    let handle = {
        let db = db.clone();
        spawn(move || open_as(&db, me()), tx)
    };
    handle.send(create(user("q"), vec![]));
    let Some(StoreReply::Created { conversation, .. }) = rx.blocking_recv() else {
        panic!("no Created reply");
    };
    handle.send(append(conversation, reply_msg("Hel", MsgStatus::Cancelled)));
    handle.send(append(conversation, user("last")));
    handle.shutdown(Duration::from_secs(2));

    let conn = Connection::open(&db).unwrap();
    let got: Vec<_> = rows(&conn, conversation)
        .into_iter()
        .map(|r| (r.2, r.3))
        .collect();
    assert_eq!(
        got,
        vec![
            ("q".into(), "complete".into()),
            ("Hel".into(), "cancelled".into()),
            ("last".into(), "complete".into()),
        ]
    );
}

#[test]
fn shutdown_gives_up_at_its_bound() {
    let (tx, _rx) = mpsc::channel(1);
    let handle = spawn(
        || {
            std::thread::sleep(Duration::from_secs(30));
            Err(StoreError::NoPath)
        },
        tx,
    );
    handle.send(create(user("q"), vec![]));
    let started = Instant::now();
    handle.shutdown(Duration::from_millis(150));
    // The stuck opener sleeps 30 s; giving up well before that is the point.
    assert!(started.elapsed() < Duration::from_secs(10));
}

fn long(c: &str, n: usize) -> String {
    c.repeat(n)
}

#[test]
fn a_multibyte_title_is_cut_at_80_characters() {
    for glyph in ["é", "😀"] {
        let (_dir, mut worker, conn) = fixture();
        let id = created_id(worker.handle(create(user(&long(glyph, 200)), vec![])));
        let title: String = conn
            .query_row("SELECT title FROM conversations WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(title.chars().count(), 80, "glyph {glyph}");
        assert_eq!(title, long(glyph, 80));
    }
}

#[test]
fn a_multibyte_error_reason_is_cut_at_exactly_the_bound() {
    for glyph in ["é", "😀"] {
        let reason = bounded_error(&long(glyph, 400));
        assert_eq!(reason.chars().count(), ERROR_MAX, "glyph {glyph}");
        assert_eq!(ERROR_MAX, 120);
    }
}

#[test]
fn the_title_is_the_first_non_empty_line() {
    assert_eq!(title_of("\n\n  \n  hello world \nsecond"), "hello world");
    assert_eq!(title_of("first\nsecond"), "first");
}

#[test]
fn a_message_without_text_is_untitled() {
    assert_eq!(title_of(""), "(untitled)");
    assert_eq!(title_of(" \n\t\n"), "(untitled)");
}

#[test]
fn a_panicking_opener_still_gets_a_reply_and_shutdown_returns() {
    let (tx, mut rx) = mpsc::channel(4);
    let handle = spawn(|| panic!("opener blew up"), tx);
    handle.send(create(user("q"), vec![]));
    let Some(StoreReply::Unavailable { reason, .. }) = rx.blocking_recv() else {
        panic!("no Unavailable reply");
    };
    assert!(reason.chars().count() <= ERROR_MAX);
    // The store is unusable from now on, but the thread keeps answering.
    handle.send(create(user("again"), vec![]));
    assert!(matches!(
        rx.blocking_recv(),
        Some(StoreReply::Unavailable { .. })
    ));
    let started = Instant::now();
    handle.shutdown(Duration::from_secs(2));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn a_panicking_append_replies_failed_and_marks_the_store_unusable() {
    let (_dir, mut worker, _conn) = fixture();
    let id = created_id(worker.handle(create(user("q"), vec![])));
    let reply = worker.guarded_with(append(id, user("boom")), |_, _| panic!("handler bug"));
    assert!(matches!(
        reply,
        Some(StoreReply::Failed { notify: true, .. })
    ));
    let after = worker.guarded(create(user("next"), vec![]));
    assert!(matches!(after, Some(StoreReply::Unavailable { .. })));
    assert!(worker.guarded(append(id, user("late"))).is_none());
}

#[test]
fn a_panicking_create_replies_unavailable_with_a_bounded_reason() {
    let (_dir, mut worker, _conn) = fixture();
    let payload = long("x\n", 500);
    let reply = worker.guarded_with(create(user("q"), vec![]), |_, _| panic!("{payload}"));
    let Some(StoreReply::Unavailable { reason, .. }) = reply else {
        panic!("expected Unavailable, got {reply:?}");
    };
    assert!(reason.chars().count() <= ERROR_MAX);
    assert!(!reason.contains('\n'));
}

#[test]
fn a_handle_without_a_thread_answers_unavailable_to_a_create() {
    let (tx, mut rx) = mpsc::channel(4);
    let handle = spawn_with(
        |_| Err(std::io::Error::other("no threads left")),
        || Err(StoreError::NoPath),
        tx,
    );
    handle.send(append(1, user("silent")));
    handle.send(create(user("q"), vec![]));
    let Some(StoreReply::Unavailable { pane: p, reason }) = rx.blocking_recv() else {
        panic!("no Unavailable reply");
    };
    assert_eq!(p, pane());
    assert!(reason.chars().count() <= ERROR_MAX);
    assert!(rx.try_recv().is_err(), "the append must stay silent");
    handle.shutdown(Duration::from_millis(200));
}

// ---- memory links (8.3) ----------------------------------------------------

const SESSION_A: &str = "lazarobox-00112233445566778899aabbccddeeff";
const SESSION_B: &str = "lazarobox-ffeeddccbbaa99887766554433221100";

fn link_session(conversation: i64, session_id: &str) -> StoreCmd {
    StoreCmd::LinkSession {
        pane: pane(),
        conversation,
        session_id: session_id.into(),
        project: "proj".into(),
        replace: false,
    }
}

fn link_note(conversation: i64, topic_key: &str, observation_id: i64, title: &str) -> StoreCmd {
    StoreCmd::LinkNote {
        pane: pane(),
        conversation,
        project: "proj".into(),
        topic_key: topic_key.into(),
        observation_id,
        title: title.into(),
    }
}

fn session_of(reply: Option<StoreReply>) -> String {
    match reply {
        Some(StoreReply::SessionLinked {
            pane: from,
            session_id,
            ..
        }) => {
            assert_eq!(from, pane());
            session_id
        }
        other => panic!("expected SessionLinked, got {other:?}"),
    }
}

/// `(observation_id, conversation_id, title)` of a note row.
fn note(conn: &Connection, topic_key: &str) -> Vec<(i64, i64, String)> {
    let mut stmt = conn
        .prepare(
            "SELECT observation_id, conversation_id, title FROM memory_notes
             WHERE project = 'proj' AND topic_key = ?1",
        )
        .unwrap();
    stmt.query_map([topic_key], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn a_session_link_stores_the_id_and_the_project() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("hi"), vec![])));
    assert_eq!(
        session_of(worker.handle(link_session(id, SESSION_A))),
        SESSION_A
    );
    let row: (String, String) = conn
        .query_row(
            "SELECT session_id, project FROM memory_sessions WHERE conversation_id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(row, (SESSION_A.to_owned(), "proj".to_owned()));
}

#[test]
fn the_same_session_id_is_returned_on_resume() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("hi"), vec![])));
    session_of(worker.handle(link_session(id, SESSION_A)));
    // A resumed conversation proposes a fresh id and gets the stored one.
    assert_eq!(
        session_of(worker.handle(link_session(id, SESSION_B))),
        SESSION_A
    );
    assert_eq!(count(&conn, "memory_sessions"), 1);
}

#[test]
fn a_resume_returns_the_stored_project_even_when_another_was_proposed() {
    let (_dir, mut worker, _conn) = fixture();
    let id = created_id(worker.handle(create(user("hi"), vec![])));
    session_of(worker.handle(link_session(id, SESSION_A)));
    // The caller may resume from another directory: engram rejects a project
    // that differs from the registered one, so the stored pair wins.
    let reply = worker.handle(StoreCmd::LinkSession {
        pane: pane(),
        conversation: id,
        session_id: SESSION_B.into(),
        project: "other".into(),
        replace: false,
    });
    let Some(StoreReply::SessionLinked {
        session_id,
        project,
        ..
    }) = reply
    else {
        panic!("expected SessionLinked, got {reply:?}");
    };
    assert_eq!((session_id.as_str(), project.as_str()), (SESSION_A, "proj"));
}

// A folder change moves the conversation to another project: the new pair
// replaces the stored one (engram rejects a session under another project).
#[test]
fn a_replacing_link_overwrites_the_stored_pair() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("hi"), vec![])));
    session_of(worker.handle(link_session(id, SESSION_A)));
    let reply = worker.handle(StoreCmd::LinkSession {
        pane: pane(),
        conversation: id,
        session_id: SESSION_B.into(),
        project: "other".into(),
        replace: true,
    });
    let Some(StoreReply::SessionLinked {
        session_id,
        project,
        ..
    }) = reply
    else {
        panic!("expected SessionLinked, got {reply:?}");
    };
    assert_eq!(
        (session_id.as_str(), project.as_str()),
        (SESSION_B, "other")
    );
    assert_eq!(count(&conn, "memory_sessions"), 1);
}

#[test]
fn a_session_id_that_is_not_ours_is_not_stored() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("hi"), vec![])));
    for bad in [
        "",
        "lazarobox-ABCDEF",
        "other-00112233445566778899aabbccddeeff",
        "lazarobox-00112233445566778899aabbccddeeffaa",
        "lazarobox-0011223344556677 899aabbccddeeff",
    ] {
        assert_eq!(
            worker.handle(link_session(id, bad)),
            Some(StoreReply::LinkFailed {
                pane: pane(),
                kind: LinkKind::Session
            }),
            "{bad:?}"
        );
    }
    assert_eq!(count(&conn, "memory_sessions"), 0);
}

#[test]
fn two_conversations_cannot_share_a_session_id() {
    let (_dir, mut worker, conn) = fixture();
    let a = created_id(worker.handle(create(user("a"), vec![])));
    let b = created_id(worker.handle(create(user("b"), vec![])));
    session_of(worker.handle(link_session(a, SESSION_A)));
    assert_eq!(
        worker.handle(link_session(b, SESSION_A)),
        Some(StoreReply::LinkFailed {
            pane: pane(),
            kind: LinkKind::Session
        })
    );
    assert_eq!(count(&conn, "memory_sessions"), 1);
}

#[test]
fn a_successful_note_link_sends_no_reply() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("hi"), vec![])));
    assert!(
        worker
            .handle(link_note(id, "chat/decision/use-axum", 7, "Use Axum"))
            .is_none()
    );
    assert_eq!(
        note(&conn, "chat/decision/use-axum"),
        [(7, id, "Use Axum".to_owned())]
    );
}

#[test]
fn a_repeated_note_save_leaves_one_row() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("hi"), vec![])));
    worker.handle(link_note(id, "chat/decision/use-axum", 7, "Use Axum"));
    worker.handle(link_note(id, "chat/decision/use-axum", 7, "Use Axum v2"));
    assert_eq!(count(&conn, "memory_notes"), 1);
    assert_eq!(
        note(&conn, "chat/decision/use-axum"),
        [(7, id, "Use Axum v2".to_owned())]
    );
}

#[test]
fn a_second_conversation_saving_the_same_note_repoints_the_row() {
    let (_dir, mut worker, conn) = fixture();
    let a = created_id(worker.handle(create(user("a"), vec![])));
    let b = created_id(worker.handle(create(user("b"), vec![])));
    worker.handle(link_note(a, "chat/decision/use-axum", 7, "Use Axum"));
    worker.handle(link_note(b, "chat/decision/use-axum", 7, "Use Axum"));
    assert_eq!(count(&conn, "memory_notes"), 1);
    assert_eq!(
        note(&conn, "chat/decision/use-axum"),
        [(7, b, "Use Axum".to_owned())]
    );
}

#[test]
fn another_project_with_the_same_topic_key_is_another_row() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("hi"), vec![])));
    worker.handle(link_note(id, "chat/decision/x", 7, "X"));
    let other = StoreCmd::LinkNote {
        pane: pane(),
        conversation: id,
        project: "other".into(),
        topic_key: "chat/decision/x".into(),
        observation_id: 8,
        title: "X".into(),
    };
    worker.handle(other);
    assert_eq!(count(&conn, "memory_notes"), 2);
}

#[test]
fn a_note_title_is_clean_and_cut_to_120_characters() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("hi"), vec![])));
    let title = format!("a\u{1b}[31mb {}", "x".repeat(300));
    worker.handle(link_note(id, "chat/decision/t", 7, &title));
    let stored = &note(&conn, "chat/decision/t")[0].2;
    assert!(!stored.contains('\u{1b}'), "{stored:?}");
    assert!(stored.chars().count() <= 120, "{}", stored.chars().count());
    assert!(stored.starts_with("a"));
}

#[test]
fn no_note_content_reaches_the_database_or_its_side_files() {
    let (dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("hi"), vec![])));
    // The commands carry ids and a title only; the body of the note never
    // enters the store, whatever the title is cut from.
    session_of(worker.handle(link_session(id, SESSION_A)));
    worker.handle(link_note(id, "chat/decision/use-axum", 7, "Use Axum"));
    drop(conn);
    let db = db_path(&dir);
    for suffix in ["", "-journal", "-wal", "-shm"] {
        let mut name = db.as_os_str().to_owned();
        name.push(suffix);
        let bytes = fs::read(PathBuf::from(name)).unwrap_or_default();
        assert!(
            !bytes.windows(11).any(|w| w == b"SECRET-BODY"),
            "{suffix:?} holds the body"
        );
    }
    let linked: i64 = Connection::open(&db)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM memory_notes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(linked, 1);
}

#[test]
fn a_failed_note_link_reports_and_undoes_nothing() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("hi"), vec![])));
    worker.handle(link_note(id, "chat/decision/kept", 7, "Kept"));
    // No such conversation: the foreign key refuses the row.
    let reply = worker.handle(link_note(9_999, "chat/decision/ghost", 8, "Ghost"));
    assert_eq!(
        reply,
        Some(StoreReply::LinkFailed {
            pane: pane(),
            kind: LinkKind::Note
        })
    );
    assert_eq!(count(&conn, "memory_notes"), 1);
    assert_eq!(count(&conn, "conversations"), 1);
    assert_eq!(rows(&conn, id).len(), 1, "the messages are intact");
    // A write failure of a link is not the history failing: the next append
    // still notifies on its own first failure.
    assert!(worker.handle(append(id, user("more"))).is_none());
}

#[test]
fn links_on_an_unusable_store_report_a_link_failure() {
    let dir = TempDir::new().unwrap();
    let db = db_path(&dir);
    fs::create_dir_all(db.parent().unwrap()).unwrap();
    fs::write(&db, vec![7u8; 4096]).unwrap();
    let mut worker = worker_at(&db);
    assert_eq!(
        worker.handle(link_session(1, SESSION_A)),
        Some(StoreReply::LinkFailed {
            pane: pane(),
            kind: LinkKind::Session
        })
    );
    assert_eq!(
        worker.handle(link_note(1, "chat/decision/x", 7, "X")),
        Some(StoreReply::LinkFailed {
            pane: pane(),
            kind: LinkKind::Note
        })
    );
}

#[test]
fn a_panicking_link_replies_with_a_link_failure() {
    let (_dir, mut worker, _conn) = fixture();
    let reply = worker.guarded_with(link_note(1, "chat/decision/x", 7, "X"), |_, _| {
        panic!("handler bug")
    });
    assert_eq!(
        reply,
        Some(StoreReply::LinkFailed {
            pane: pane(),
            kind: LinkKind::Note
        })
    );
    let reply = worker.guarded_with(link_session(1, SESSION_A), |_, _| panic!("handler bug"));
    assert_eq!(
        reply,
        Some(StoreReply::LinkFailed {
            pane: pane(),
            kind: LinkKind::Session
        })
    );
}

#[test]
fn a_panicking_link_keeps_the_store_usable() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("hi"), vec![])));
    for cmd in [
        link_session(id, SESSION_A),
        link_note(id, "chat/decision/x", 7, "X"),
    ] {
        // The panic hits inside an open transaction, as a bug in a link would.
        let reply = worker.guarded_with(cmd, |w, _| {
            let conn = w.conn().unwrap();
            let tx = conn.transaction().unwrap();
            tx.execute(
                "INSERT INTO memory_sessions VALUES (?1, ?2, 'proj')",
                (id, SESSION_A),
            )
            .unwrap();
            panic!("handler bug")
        });
        assert!(matches!(reply, Some(StoreReply::LinkFailed { .. })));
    }
    // Read from a separate connection: the write was rolled back.
    assert_eq!(count(&conn, "memory_sessions"), 0);
    // History stays available: the next Create and Append succeed.
    let again = created_id(worker.guarded(create(user("still here"), vec![])));
    assert!(worker.guarded(append(again, user("more"))).is_none());
    assert_eq!(rows(&conn, again).len(), 2);
}

#[test]
fn a_link_panic_that_leaves_a_transaction_open_marks_the_store_unusable() {
    let (_dir, mut worker, _conn) = fixture();
    let id = created_id(worker.handle(create(user("q"), vec![])));
    let reply = worker.guarded_with(link_note(id, "chat/decision/x", 7, "X"), |w, _| {
        w.conn().unwrap().execute_batch("BEGIN").unwrap();
        panic!("handler bug")
    });
    assert!(matches!(reply, Some(StoreReply::LinkFailed { .. })));
    let after = worker.guarded(create(user("next"), vec![]));
    assert!(matches!(after, Some(StoreReply::Unavailable { .. })));
}

#[test]
fn links_travel_through_the_store_thread_in_order() {
    let dir = TempDir::new().unwrap();
    let db = db_path(&dir);
    let (tx, mut rx) = mpsc::channel(8);
    let handle = {
        let db = db.clone();
        spawn(move || open_as(&db, me()), tx)
    };
    handle.send(create(user("q"), vec![]));
    let Some(StoreReply::Created { conversation, .. }) = rx.blocking_recv() else {
        panic!("no Created");
    };
    handle.send(link_session(conversation, SESSION_A));
    handle.send(link_note(conversation, "chat/decision/x", 7, "X"));
    assert_eq!(session_of(rx.blocking_recv()), SESSION_A);
    handle.shutdown(Duration::from_secs(2));
    let conn = Connection::open(&db).unwrap();
    assert_eq!(count(&conn, "memory_notes"), 1);
}

// ---- eviction counts notes (8.5) -------------------------------------------

fn seed_notes(conn: &Connection, conversation: i64, n: usize) {
    for i in 0..n {
        conn.execute(
            "INSERT INTO memory_notes VALUES ('proj', ?1, ?2, ?3, 't')",
            (
                format!("chat/decision/c{conversation}-n{i}"),
                i as i64,
                conversation,
            ),
        )
        .unwrap();
    }
}

fn notes_of(reply: Option<StoreReply>) -> u32 {
    match reply {
        Some(StoreReply::Created { evicted_notes, .. }) => evicted_notes,
        other => panic!("expected Created, got {other:?}"),
    }
}

#[test]
fn eviction_counts_the_notes_of_the_evicted_conversation_and_removes_them() {
    let (_dir, mut worker, conn) = fixture();
    seed(&conn, 200);
    seed_notes(&conn, 1_000, 3);
    seed_notes(&conn, 1_001, 2); // survives, so it is not counted
    conn.execute(
        "INSERT INTO memory_sessions VALUES (1000, ?1, 'proj')",
        [SESSION_A],
    )
    .unwrap();

    let reply = worker.handle(create(user("new"), vec![]));
    assert!(matches!(
        reply,
        Some(StoreReply::Created { evicted: true, .. })
    ));
    assert_eq!(notes_of(reply), 3);
    assert!(!exists(&conn, 1_000));
    assert_eq!(count(&conn, "memory_notes"), 2, "only the survivor's rows");
    assert_eq!(count(&conn, "memory_sessions"), 0);
}

#[test]
fn one_note_counts_as_one() {
    let (_dir, mut worker, conn) = fixture();
    seed(&conn, 200);
    seed_notes(&conn, 1_000, 1);
    assert_eq!(notes_of(worker.handle(create(user("new"), vec![]))), 1);
}

#[test]
fn an_evicted_conversation_without_notes_counts_zero() {
    let (_dir, mut worker, conn) = fixture();
    seed(&conn, 200);
    seed_notes(&conn, 1_005, 4);
    let reply = worker.handle(create(user("new"), vec![]));
    assert!(matches!(
        reply,
        Some(StoreReply::Created { evicted: true, .. })
    ));
    assert_eq!(notes_of(reply), 0);
}

#[test]
fn below_the_limit_the_count_is_zero_and_no_note_goes() {
    let (_dir, mut worker, conn) = fixture();
    seed(&conn, 199);
    seed_notes(&conn, 1_000, 2);
    assert_eq!(notes_of(worker.handle(create(user("new"), vec![]))), 0);
    assert_eq!(count(&conn, "memory_notes"), 2);
}

#[test]
fn a_trim_over_the_limit_sums_the_notes_of_every_evicted_conversation() {
    let (_dir, mut worker, conn) = fixture();
    seed(&conn, 205);
    seed_notes(&conn, 1_000, 2);
    seed_notes(&conn, 1_003, 1);
    seed_notes(&conn, 1_006, 5); // the first to survive
    assert_eq!(notes_of(worker.handle(create(user("new"), vec![]))), 3);
    assert_eq!(count(&conn, "memory_notes"), 5);
}

#[test]
fn the_notes_of_an_open_conversation_are_not_counted() {
    let (_dir, mut worker, conn) = fixture();
    seed(&conn, 200);
    seed_notes(&conn, 1_000, 3); // open, so skipped
    seed_notes(&conn, 1_001, 2); // evicted instead
    assert_eq!(notes_of(worker.handle(create(user("new"), vec![1_000]))), 2);
    assert_eq!(count(&conn, "memory_notes"), 3);
}

// ---- list, resume, delete (ai-chat C9) ---------------------------------------

use crate::core::ai::chat::{
    Msg,
    store::{ConvRow, FIRST_MAX, NoteRow},
};

fn list() -> StoreCmd {
    StoreCmd::List {
        pane: pane(),
        origin: None,
    }
}

fn list_from(origin: &Path) -> StoreCmd {
    StoreCmd::List {
        pane: pane(),
        origin: Some(origin.to_path_buf()),
    }
}

fn listed(reply: Option<StoreReply>) -> Vec<ConvRow> {
    match reply {
        Some(StoreReply::Listed {
            pane: from, rows, ..
        }) => {
            assert_eq!(from, pane());
            rows
        }
        other => panic!("expected Listed, got {other:?}"),
    }
}

fn resume(conversation: i64) -> StoreCmd {
    StoreCmd::Resume {
        pane: pane(),
        conversation,
    }
}

fn delete(conversation: i64) -> StoreCmd {
    StoreCmd::Delete {
        pane: pane(),
        conversation,
    }
}

#[test]
fn the_list_is_newest_activity_first_with_the_first_user_message() {
    let (_dir, mut worker, conn) = fixture();
    let old = created_id(worker.handle(create(user("older question"), vec![])));
    let new = created_id(worker.handle(StoreCmd::Create {
        pane: pane(),
        profile: "analyst".into(),
        tag: None,
        extras: vec![],
        first: user("newer\nquestion"),
        origin: None,
        folder: None,
        open: vec![],
    }));
    worker.handle(append(old, reply_msg("an answer", MsgStatus::Complete)));
    conn.execute(
        "UPDATE conversations SET updated_at = 5000 WHERE id = ?1",
        [old],
    )
    .unwrap();
    conn.execute(
        "UPDATE conversations SET updated_at = 4000 WHERE id = ?1",
        [new],
    )
    .unwrap();
    let rows = listed(worker.handle(list()));
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (rows[0].id, rows[0].updated_at, rows[0].profile.as_str()),
        (old, 5000, "programmer")
    );
    assert_eq!(rows[0].tag.as_deref(), Some("proj"));
    // The first message of the conversation is the user's, not the answer.
    assert_eq!(rows[0].first, "older question");
    assert_eq!(
        (rows[1].id, rows[1].tag.clone(), rows[1].first.as_str()),
        (new, None, "newer\nquestion")
    );
}

#[test]
fn equal_activity_lists_the_higher_id_first() {
    let (_dir, mut worker, conn) = fixture();
    let a = created_id(worker.handle(create(user("a"), vec![])));
    let b = created_id(worker.handle(create(user("b"), vec![])));
    conn.execute("UPDATE conversations SET updated_at = 7", [])
        .unwrap();
    let ids: Vec<i64> = listed(worker.handle(list())).iter().map(|r| r.id).collect();
    assert_eq!(ids, vec![b, a]);
}

#[test]
fn a_long_first_message_is_cut_in_the_list() {
    let (_dir, mut worker, _conn) = fixture();
    created_id(worker.handle(create(user(&"é".repeat(FIRST_MAX + 50)), vec![])));
    let rows = listed(worker.handle(list()));
    assert_eq!(rows[0].first.chars().count(), FIRST_MAX);
}

#[test]
fn an_empty_store_lists_nothing() {
    let (_dir, mut worker, _conn) = fixture();
    assert_eq!(listed(worker.handle(list())), Vec::new());
}

#[test]
fn an_unusable_store_answers_unavailable_to_a_list() {
    let reply = run_once(None, list());
    assert_unavailable(reply);
}

#[test]
fn a_resume_returns_the_messages_in_order_with_their_status() {
    let (_dir, mut worker, _conn) = fixture();
    let id = created_id(worker.handle(create(user("q"), vec![])));
    worker.handle(append(id, reply_msg("full", MsgStatus::Complete)));
    worker.handle(append(id, user("q2")));
    worker.handle(append(id, reply_msg("Hel", MsgStatus::Cancelled)));
    let Some(StoreReply::Resumed {
        pane: from,
        conversation,
        profile,
        tag,
        messages,
        ..
    }) = worker.handle(resume(id))
    else {
        panic!("expected Resumed");
    };
    assert_eq!((from, conversation), (pane(), id));
    assert_eq!(
        (profile.as_str(), tag.as_deref()),
        ("programmer", Some("proj"))
    );
    let got: Vec<(Role, &str, MsgStatus)> = messages
        .iter()
        .map(|m: &Msg| (m.role, m.text.as_str(), m.status))
        .collect();
    assert_eq!(
        got,
        vec![
            (Role::User, "q", MsgStatus::Complete),
            (Role::Assistant, "full", MsgStatus::Complete),
            (Role::User, "q2", MsgStatus::Complete),
            (Role::Assistant, "Hel", MsgStatus::Cancelled),
        ]
    );
}

// Spec (Resume): a resumed conversation reuses the engram pair its notes were
// written under, read with the conversation and never written by the resume.
#[test]
fn a_resume_returns_the_stored_session_pair() {
    let (_dir, mut worker, conn) = fixture();
    let linked = created_id(worker.handle(create(user("linked"), vec![])));
    let plain = created_id(worker.handle(create(user("plain"), vec![])));
    session_of(worker.handle(link_session(linked, SESSION_A)));
    let before = count(&conn, "memory_sessions");
    let session = |reply: Option<StoreReply>| match reply {
        Some(StoreReply::Resumed { session, .. }) => session,
        other => panic!("expected Resumed, got {other:?}"),
    };
    assert_eq!(
        session(worker.handle(resume(linked))),
        Some((SESSION_A.to_owned(), "proj".to_owned()))
    );
    assert_eq!(session(worker.handle(resume(plain))), None);
    assert_eq!(count(&conn, "memory_sessions"), before, "a read only");
}

#[test]
fn resuming_a_missing_conversation_fails() {
    let (_dir, mut worker, _conn) = fixture();
    assert_eq!(
        worker.handle(resume(424_242)),
        Some(StoreReply::ResumeFailed { pane: pane() })
    );
}

#[test]
fn a_delete_removes_the_conversation_and_everything_that_hangs_from_it() {
    let (_dir, mut worker, conn) = fixture();
    let keep = created_id(worker.handle(create(user("keep"), vec![])));
    let id = created_id(worker.handle(create(user("gone"), vec![])));
    worker.handle(append(id, reply_msg("answer", MsgStatus::Complete)));
    session_of(worker.handle(link_session(id, SESSION_A)));
    worker.handle(link_note(id, "decision/a", 11, "A"));
    worker.handle(link_note(keep, "decision/b", 12, "B"));
    let reply = worker.handle(delete(id));
    // The note rows come back before the cascade removes them.
    assert_eq!(
        reply,
        Some(StoreReply::Deleted {
            pane: pane(),
            conversation: id,
            notes: vec![NoteRow {
                project: "proj".into(),
                observation_id: 11,
            }],
        })
    );
    assert!(!exists(&conn, id));
    assert!(exists(&conn, keep));
    assert_eq!(count(&conn, "messages"), 1);
    assert_eq!(count(&conn, "tags"), 1);
    assert_eq!(count(&conn, "memory_sessions"), 0);
    assert_eq!(count(&conn, "memory_notes"), 1);
}

// Spec (Delete): the prompt offers `its N notes`, so the list carries the
// count of each conversation's note links.
#[test]
fn the_list_counts_the_note_links_of_each_conversation() {
    let (_dir, mut worker, _conn) = fixture();
    let none = created_id(worker.handle(create(user("none"), vec![])));
    let three = created_id(worker.handle(create(user("three"), vec![])));
    let one = created_id(worker.handle(create(user("one"), vec![])));
    for (n, topic) in ["decision/a", "decision/b", "bugfix/c"].iter().enumerate() {
        worker.handle(link_note(three, topic, 20 + n as i64, "T"));
    }
    worker.handle(link_note(one, "pattern/d", 30, "D"));
    let rows = listed(worker.handle(list()));
    let notes = |id: i64| rows.iter().find(|r| r.id == id).map(|r| r.notes);
    assert_eq!(
        (notes(none), notes(three), notes(one)),
        (Some(0), Some(3), Some(1))
    );
}

// A note re-pointed to another conversation belongs to the one that wrote it
// last, so the older one no longer counts it.
#[test]
fn a_re_pointed_note_counts_for_its_last_conversation_only() {
    let (_dir, mut worker, _conn) = fixture();
    let first = created_id(worker.handle(create(user("first"), vec![])));
    let second = created_id(worker.handle(create(user("second"), vec![])));
    worker.handle(link_note(first, "decision/x", 41, "X"));
    worker.handle(link_note(second, "decision/x", 41, "X"));
    let rows = listed(worker.handle(list()));
    let notes = |id: i64| rows.iter().find(|r| r.id == id).map(|r| r.notes);
    assert_eq!((notes(first), notes(second)), (Some(0), Some(1)));
}

// Spec (Delete failure): nothing is returned for a delete that did not happen,
// so the app has no rows to send a request for.
#[test]
fn a_failed_delete_returns_no_note_rows() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("x"), vec![])));
    worker.handle(link_note(id, "decision/a", 11, "A"));
    conn.execute_batch(
        "CREATE TRIGGER no_deletes BEFORE DELETE ON conversations
         BEGIN SELECT RAISE(ABORT, 'disk on fire'); END;",
    )
    .unwrap();
    assert_eq!(
        worker.handle(delete(id)),
        Some(StoreReply::DeleteFailed { pane: pane() })
    );
    assert_eq!(count(&conn, "memory_notes"), 1, "the link stays too");
}

#[test]
fn a_delete_without_notes_returns_none() {
    let (_dir, mut worker, _conn) = fixture();
    let id = created_id(worker.handle(create(user("x"), vec![])));
    let Some(StoreReply::Deleted { notes, .. }) = worker.handle(delete(id)) else {
        panic!("expected Deleted");
    };
    assert_eq!(notes, Vec::new());
}

#[test]
fn deleting_a_missing_conversation_fails() {
    let (_dir, mut worker, _conn) = fixture();
    assert_eq!(
        worker.handle(delete(424_242)),
        Some(StoreReply::DeleteFailed { pane: pane() })
    );
}

#[test]
fn a_failing_delete_keeps_the_conversation() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("x"), vec![])));
    conn.execute_batch(
        "CREATE TRIGGER no_deletes BEFORE DELETE ON conversations
         BEGIN SELECT RAISE(ABORT, 'disk on fire'); END;",
    )
    .unwrap();
    assert_eq!(
        worker.handle(delete(id)),
        Some(StoreReply::DeleteFailed { pane: pane() })
    );
    assert!(exists(&conn, id));
    assert_eq!(count(&conn, "messages"), 1);
}

// Spec: Deleted text is gone from the file.
#[test]
fn deleted_text_is_gone_from_the_file() {
    let (dir, mut worker, _conn) = fixture();
    let id = created_id(worker.handle(create(user("ZZ-MARKER-ZZ"), vec![])));
    worker.handle(append(id, reply_msg("ZZ-MARKER-ZZ", MsgStatus::Complete)));
    let has = |p: &Path| {
        fs::read(p)
            .unwrap_or_default()
            .windows(12)
            .any(|w| w == b"ZZ-MARKER-ZZ")
    };
    let db = db_path(&dir);
    assert!(has(&db), "the marker is in the file before the delete");
    worker.handle(delete(id));
    assert!(!has(&db));
    let mut journal = db.as_os_str().to_owned();
    journal.push("-journal");
    assert!(!has(Path::new(&journal)));
}

#[test]
fn a_handle_without_a_thread_answers_every_history_read() {
    let (tx, mut rx) = mpsc::channel(8);
    let handle = spawn_with(
        |_| Err(std::io::Error::other("no threads left")),
        || Err(StoreError::NoPath),
        tx,
    );
    handle.send(list());
    handle.send(resume(1));
    handle.send(delete(1));
    assert!(matches!(
        rx.blocking_recv(),
        Some(StoreReply::Unavailable { .. })
    ));
    assert_eq!(
        rx.blocking_recv(),
        Some(StoreReply::ResumeFailed { pane: pane() })
    );
    assert_eq!(
        rx.blocking_recv(),
        Some(StoreReply::DeleteFailed { pane: pane() })
    );
    handle.shutdown(Duration::from_millis(200));
}

#[test]
fn a_panicking_history_command_still_gets_its_reply() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("q"), vec![])));
    // A delete or a resume that panics fails without taking the history down.
    let reply = worker.guarded_with(delete(id), |_, _| panic!("handler bug"));
    assert_eq!(reply, Some(StoreReply::DeleteFailed { pane: pane() }));
    let reply = worker.guarded_with(resume(id), |_, _| panic!("handler bug"));
    assert_eq!(reply, Some(StoreReply::ResumeFailed { pane: pane() }));
    assert!(exists(&conn, id));
    assert_eq!(listed(worker.guarded(list())).len(), 1);
    // A list that panics leaves the store unusable, like a create.
    let reply = worker.guarded_with(list(), |_, _| panic!("handler bug"));
    assert!(matches!(reply, Some(StoreReply::Unavailable { .. })));
}

// The project tag is resolved here, off the UI thread, from the origin cwd.
#[test]
fn the_list_carries_the_project_tag_of_the_origin() {
    let (dir, mut worker, _conn) = fixture();
    let proj = dir.path().join("proj");
    fs::create_dir_all(proj.join(".git")).unwrap();
    fs::create_dir_all(proj.join("src")).unwrap();
    let reply = worker.handle(list_from(&proj.join("src")));
    let Some(StoreReply::Listed { tag, .. }) = reply else {
        panic!("expected Listed, got {reply:?}");
    };
    assert_eq!(tag.as_deref(), Some("proj"));
}

#[test]
fn an_unknown_or_vanished_origin_lists_without_a_tag() {
    let (dir, mut worker, _conn) = fixture();
    for cmd in [list(), list_from(&dir.path().join("gone"))] {
        let reply = worker.handle(cmd);
        assert!(
            matches!(reply, Some(StoreReply::Listed { tag: None, .. })),
            "{reply:?}"
        );
    }
}

// A read that fails leaves the pane attached to the store.
#[test]
fn a_list_that_fails_to_read_is_not_an_unavailable_store() {
    let (dir, mut worker, conn) = fixture();
    conn.execute_batch("DROP TABLE tags").unwrap();
    let origin = dir.path().join("work");
    fs::create_dir_all(&origin).unwrap();
    let reply = worker.handle(list_from(&origin));
    assert_eq!(
        reply,
        Some(StoreReply::ListFailed {
            pane: pane(),
            tag: Some("work".into()),
        })
    );
}

// A conversation created before the list answered has no tag yet: the store
// thread resolves it from the origin the pane sent.
#[test]
fn a_create_without_a_tag_resolves_it_from_the_origin() {
    let (dir, mut worker, conn) = fixture();
    let proj = dir.path().join("proj");
    fs::create_dir_all(proj.join(".git")).unwrap();
    let cmd = |tag: Option<&str>, origin: Option<PathBuf>| StoreCmd::Create {
        pane: pane(),
        profile: "programmer".into(),
        tag: tag.map(str::to_owned),
        extras: vec![],
        first: user("hi"),
        origin,
        folder: None,
        open: vec![],
    };
    let id = created_id(worker.handle(cmd(None, Some(proj.clone()))));
    let tag = |id: i64| -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT tag FROM tags WHERE conversation_id = ?1 AND is_project = 1")
            .unwrap();
        stmt.query_map([id], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(tag(id), vec!["proj".to_owned()]);
    // A tag the pane knows wins over the origin.
    let id = created_id(worker.handle(cmd(Some("client"), Some(proj))));
    assert_eq!(tag(id), vec!["client".to_owned()]);
    // No tag and no origin: no tag row.
    let id = created_id(worker.handle(cmd(None, None)));
    assert_eq!(tag(id), Vec::<String>::new());
}

#[test]
fn a_deleted_conversation_is_forgotten_by_its_pane() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("q"), vec![])));
    assert!(matches!(
        worker.handle(delete(id)),
        Some(StoreReply::Deleted { .. })
    ));
    // The map no longer points the pane at a conversation that is gone: the
    // append is dropped quietly instead of failing.
    assert_eq!(
        worker.handle(append_for_pane(reply_msg("A", MsgStatus::Complete))),
        None
    );
    assert_eq!(count(&conn, "messages"), 0);
}

// ---- tags (10.5) -----------------------------------------------------------

fn set_tags(conversation: Option<i64>, tags: Tags) -> StoreCmd {
    StoreCmd::SetTags {
        pane: pane(),
        conversation,
        tags,
    }
}

/// `(tag, is_project)` of a conversation, project first, then by name.
fn tag_rows(conn: &Connection, id: i64) -> Vec<(String, i64)> {
    let mut stmt = conn
        .prepare(
            "SELECT tag, is_project FROM tags WHERE conversation_id = ?1
             ORDER BY is_project DESC, tag",
        )
        .unwrap();
    stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn updated_at(conn: &Connection, id: i64) -> i64 {
    conn.query_row(
        "SELECT updated_at FROM conversations WHERE id = ?1",
        [id],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn set_tags_replaces_the_project_tag_and_the_extras() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("q"), vec![])));
    let tags = Tags::parse("client-a, sql, etl");
    let reply = worker.handle(set_tags(Some(id), tags.clone()));
    assert_eq!(
        reply,
        Some(StoreReply::TagsSaved {
            pane: pane(),
            conversation: id,
            tags,
            updated_at: updated_at(&conn, id),
        })
    );
    assert_eq!(
        tag_rows(&conn, id),
        vec![("client-a".into(), 1), ("etl".into(), 0), ("sql".into(), 0)]
    );
}

#[test]
fn set_tags_with_nothing_removes_every_tag() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("q"), vec![])));
    worker.handle(set_tags(Some(id), Tags::parse("proj,a")));
    worker.handle(set_tags(Some(id), Tags::default()));
    assert_eq!(tag_rows(&conn, id), vec![]);
}

#[test]
fn set_tags_updates_the_last_activity() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("q"), vec![])));
    conn.execute(
        "UPDATE conversations SET updated_at = 5 WHERE id = ?1",
        [id],
    )
    .unwrap();
    let reply = worker.handle(set_tags(Some(id), Tags::parse("proj")));
    assert!(updated_at(&conn, id) > 5);
    // The reply carries the stamp the row got.
    let Some(StoreReply::TagsSaved {
        updated_at: stamp, ..
    }) = reply
    else {
        panic!("expected TagsSaved");
    };
    assert_eq!(stamp, updated_at(&conn, id));
}

#[test]
fn set_tags_normalizes_what_it_stores() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("q"), vec![])));
    // Built by hand, bypassing the parse: the thread cleans again.
    let raw = Tags {
        project: Some(" proj ".into()),
        extras: vec!["SQL".into(), "sql".into(), "a,b".into(), "".into()],
    };
    let Some(StoreReply::TagsSaved { tags, .. }) = worker.handle(set_tags(Some(id), raw)) else {
        panic!("expected TagsSaved");
    };
    assert_eq!(tags, Tags::parse("proj, SQL, a_b"));
    assert_eq!(
        tag_rows(&conn, id),
        vec![("proj".into(), 1), ("SQL".into(), 0), ("a_b".into(), 0)]
    );
}

#[test]
fn set_tags_is_all_or_nothing() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("q"), vec![])));
    // A trigger makes the second insert fail: the old tags must survive.
    conn.execute_batch(
        "CREATE TRIGGER no_boom BEFORE INSERT ON tags WHEN NEW.tag = 'boom'
         BEGIN SELECT RAISE(ABORT, 'no'); END;",
    )
    .unwrap();
    let reply = worker.handle(set_tags(Some(id), Tags::parse("new,boom")));
    assert_eq!(reply, Some(StoreReply::TagsFailed { pane: pane() }));
    assert_eq!(tag_rows(&conn, id), vec![("proj".into(), 1)]);
}

#[test]
fn set_tags_on_a_missing_conversation_fails_and_stores_nothing() {
    let (_dir, mut worker, conn) = fixture();
    let reply = worker.handle(set_tags(Some(777), Tags::parse("proj")));
    assert_eq!(reply, Some(StoreReply::TagsFailed { pane: pane() }));
    assert_eq!(count(&conn, "tags"), 0);
}

#[test]
fn set_tags_that_names_only_the_pane_lands_in_its_conversation() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create(user("q"), vec![])));
    let reply = worker.handle(set_tags(None, Tags::parse("other")));
    assert!(
        matches!(reply, Some(StoreReply::TagsSaved { conversation, .. }) if conversation == id)
    );
    assert_eq!(tag_rows(&conn, id), vec![("other".into(), 1)]);
}

#[test]
fn set_tags_after_a_failed_create_is_dropped_silently() {
    // The pane never created a conversation (the fixture's probe is gone).
    let (_dir, mut worker, conn) = fixture();
    worker.created.clear();
    assert_eq!(worker.handle(set_tags(None, Tags::parse("x"))), None);
    assert_eq!(count(&conn, "tags"), 0);
}

#[test]
fn a_create_stores_the_extra_tags_after_the_project_tag() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(StoreCmd::Create {
        pane: pane(),
        profile: "analyst".into(),
        tag: Some("proj".into()),
        extras: vec!["sql".into(), "SQL".into(), "etl".into()],
        first: user("q"),
        origin: None,
        folder: None,
        open: vec![],
    }));
    assert_eq!(
        tag_rows(&conn, id),
        vec![("proj".into(), 1), ("etl".into(), 0), ("sql".into(), 0)]
    );
}

#[test]
fn the_list_and_the_resume_carry_the_extra_tags() {
    let (_dir, mut worker, _conn) = fixture();
    let id = created_id(worker.handle(create(user("q"), vec![])));
    worker.handle(set_tags(Some(id), Tags::parse("proj, b, a")));
    let rows = listed(worker.handle(list()));
    assert_eq!(rows[0].tag.as_deref(), Some("proj"));
    assert_eq!(rows[0].extras, ["a", "b"]);
    let Some(StoreReply::Resumed { tag, extras, .. }) = worker.handle(resume(id)) else {
        panic!("expected Resumed");
    };
    assert_eq!(tag.as_deref(), Some("proj"));
    assert_eq!(extras, ["a", "b"]);
}

#[test]
fn the_list_filter_sees_a_retagged_conversation() {
    let (_dir, mut worker, _conn) = fixture();
    let id = created_id(worker.handle(create(user("q"), vec![])));
    worker.handle(set_tags(Some(id), Tags::parse("client-a")));
    let rows = listed(worker.handle(list()));
    assert_eq!(rows[0].tag.as_deref(), Some("client-a"));
}

#[test]
fn a_panicking_set_tags_replies_failed_and_keeps_an_idle_store() {
    let (_dir, mut worker, _conn) = fixture();
    let id = created_id(worker.handle(create(user("q"), vec![])));
    let reply = worker.guarded_with(set_tags(Some(id), Tags::parse("x")), |_, _| {
        panic!("handler bug")
    });
    assert_eq!(reply, Some(StoreReply::TagsFailed { pane: pane() }));
    assert!(worker.guarded(create(user("next"), vec![])).is_some());
}

#[test]
fn a_set_tags_with_no_store_thread_is_answered_failed() {
    let (tx, mut rx) = mpsc::channel(4);
    let handle = spawn_with(
        |_body| Err(io::Error::other("no thread")),
        || unreachable!(),
        tx,
    );
    handle.send(set_tags(Some(1), Tags::parse("x")));
    assert_eq!(
        rx.try_recv().unwrap(),
        StoreReply::TagsFailed { pane: pane() }
    );
}

// A failed `Create` must not leave the pane pointing at its earlier
// conversation: a later command that names only the pane would land there.
#[test]
fn a_failed_create_forgets_the_conversation_the_pane_had() {
    let (_dir, mut worker, conn) = fixture();
    let first = created_id(worker.handle(create(user("first"), vec![])));
    conn.execute_batch(
        "CREATE TRIGGER no_boom BEFORE INSERT ON conversations WHEN NEW.title = 'boom'
         BEGIN SELECT RAISE(ABORT, 'no'); END;",
    )
    .unwrap();
    let reply = worker.handle(create(user("boom"), vec![]));
    assert!(
        matches!(reply, Some(StoreReply::Failed { .. })),
        "{reply:?}"
    );
    assert_eq!(
        worker.handle(append_for_pane(reply_msg("late", MsgStatus::Complete))),
        None
    );
    assert_eq!(worker.handle(set_tags(None, Tags::parse("late"))), None);
    assert_eq!(rows(&conn, first).len(), 1, "nothing landed in the old one");
    assert_eq!(count(&conn, "tags"), 1, "the old conversation kept its tag");
}

// ---- the chat folder (chat-folder) -----------------------------------------

fn create_in(folder: &str) -> StoreCmd {
    StoreCmd::Create {
        pane: pane(),
        profile: "programmer".into(),
        tag: None,
        extras: vec![],
        first: user("q"),
        origin: None,
        folder: Some(PathBuf::from(folder)),
        open: vec![],
    }
}

fn set_folder(conversation: Option<i64>, folder: &str) -> StoreCmd {
    StoreCmd::SetFolder {
        pane: pane(),
        conversation,
        folder: PathBuf::from(folder),
    }
}

fn stored_folder(conn: &Connection, id: i64) -> Option<String> {
    conn.query_row(
        "SELECT folder FROM conversations WHERE id = ?1",
        [id],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn create_stores_the_folder_and_a_conversation_without_one_reads_null() {
    let (_dir, mut worker, conn) = fixture();
    let with = created_id(worker.handle(create_in("/work/a")));
    let without = created_id(worker.handle(create(user("q"), vec![])));
    assert_eq!(stored_folder(&conn, with).as_deref(), Some("/work/a"));
    assert_eq!(stored_folder(&conn, without), None);
}

#[test]
fn set_folder_replaces_the_folder_silently_and_bumps_the_activity() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create_in("/work/a")));
    conn.execute("UPDATE conversations SET updated_at = 1", [])
        .unwrap();
    assert_eq!(worker.handle(set_folder(Some(id), "/work/b")), None);
    assert_eq!(stored_folder(&conn, id).as_deref(), Some("/work/b"));
    assert!(updated_at(&conn, id) > 1);
}

#[test]
fn set_folder_without_an_id_resolves_the_conversation_of_the_pane() {
    let (_dir, mut worker, conn) = fixture();
    let id = created_id(worker.handle(create_in("/work/a")));
    assert_eq!(worker.handle(set_folder(None, "/work/c")), None);
    assert_eq!(stored_folder(&conn, id).as_deref(), Some("/work/c"));
}

#[test]
fn set_folder_for_a_conversation_that_is_gone_changes_nothing() {
    let (_dir, mut worker, conn) = fixture();
    assert_eq!(worker.handle(set_folder(Some(999), "/work/x")), None);
    assert_eq!(count(&conn, "conversations"), 0);
    assert_eq!(worker.handle(set_folder(None, "/work/x")), None);
}

#[test]
fn resume_returns_the_stored_folder_or_none() {
    let (_dir, mut worker, _conn) = fixture();
    let with = created_id(worker.handle(create_in("/work/a")));
    let without = created_id(worker.handle(create(user("q"), vec![])));
    let Some(StoreReply::Resumed { folder, .. }) = worker.handle(resume(with)) else {
        panic!("expected Resumed");
    };
    assert_eq!(folder, Some(PathBuf::from("/work/a")));
    let Some(StoreReply::Resumed { folder, .. }) = worker.handle(resume(without)) else {
        panic!("expected Resumed");
    };
    assert_eq!(folder, None);
}
