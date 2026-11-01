//! Store opening tests. Every path lives in a temp dir and every environment
//! value is passed in, so nothing here touches the real data directory.

use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
};

use tempfile::TempDir;

use super::*;

fn me() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().mode() & 0o7777
}

fn store_path(dir: &TempDir) -> PathBuf {
    dir.path().join("lazarobox").join("chat.db")
}

fn os(s: &str) -> Option<&OsStr> {
    Some(OsStr::new(s))
}

fn pragma_text(conn: &Connection, name: &str) -> String {
    conn.query_row(&format!("PRAGMA {name}"), [], |r| {
        r.get::<_, rusqlite::types::Value>(0)
    })
    .map(|v| match v {
        rusqlite::types::Value::Integer(i) => i.to_string(),
        rusqlite::types::Value::Text(t) => t,
        other => format!("{other:?}"),
    })
    .unwrap()
}

fn side(db: &Path, suffix: &str) -> PathBuf {
    let mut name = db.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

// ---- path resolution -------------------------------------------------------

#[test]
fn an_absolute_xdg_value_wins() {
    assert_eq!(
        resolve_path(os("/x"), os("/h")),
        Some(PathBuf::from("/x/lazarobox/chat.db"))
    );
}

#[test]
fn an_empty_or_relative_xdg_value_falls_back_to_home() {
    let want = Some(PathBuf::from("/h/.local/share/lazarobox/chat.db"));
    assert_eq!(resolve_path(os(""), os("/h")), want);
    assert_eq!(resolve_path(os("rel/dir"), os("/h")), want);
    assert_eq!(resolve_path(None, os("/h")), want);
}

#[test]
fn without_a_usable_base_there_is_no_store() {
    assert_eq!(resolve_path(None, None), None);
    assert_eq!(resolve_path(os(""), os("")), None);
    assert_eq!(resolve_path(os("rel"), os("relhome")), None);
    assert_eq!(resolve_path(None, os("relhome")), None);
}

#[test]
fn an_absolute_xdg_value_needs_no_home() {
    assert_eq!(
        resolve_path(os("/x"), None),
        Some(PathBuf::from("/x/lazarobox/chat.db"))
    );
}

// ---- directory and file modes ----------------------------------------------

#[test]
fn the_data_directory_is_created_0700() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    open_as(&db, me()).unwrap();
    assert_eq!(mode(db.parent().unwrap()), 0o700);
}

#[test]
fn a_wide_data_directory_is_corrected() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    fs::create_dir(db.parent().unwrap()).unwrap();
    fs::set_permissions(db.parent().unwrap(), fs::Permissions::from_mode(0o755)).unwrap();
    open_as(&db, me()).unwrap();
    assert_eq!(mode(db.parent().unwrap()), 0o700);
}

#[test]
fn a_fresh_store_and_its_journal_are_0600() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    let conn = open_as(&db, me()).unwrap();
    assert_eq!(mode(&db), 0o600);

    // The rollback journal exists only inside a write transaction.
    conn.execute_batch(
        "BEGIN; INSERT INTO conversations(created_at, updated_at, profile, title)
         VALUES (1, 1, 'programmer', 't');",
    )
    .unwrap();
    let journal = side(&db, "-journal");
    assert!(
        journal.exists(),
        "expected a rollback journal mid-transaction"
    );
    assert_eq!(mode(&journal), 0o600);
    conn.execute_batch("COMMIT").unwrap();
    assert!(!journal.exists(), "the journal is removed after the commit");
    assert_eq!(mode(&db), 0o600);
}

#[test]
fn a_wide_database_mode_is_corrected_on_open() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    drop(open_as(&db, me()).unwrap());
    fs::set_permissions(&db, fs::Permissions::from_mode(0o644)).unwrap();
    drop(open_as(&db, me()).unwrap());
    assert_eq!(mode(&db), 0o600);
}

#[test]
fn every_side_file_that_exists_is_brought_to_0600() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    drop(open_as(&db, me()).unwrap());
    for suffix in ["-journal", "-wal", "-shm"] {
        let path = side(&db, suffix);
        fs::write(&path, b"").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
    }
    harden_side_files(&db, me()).unwrap();
    for suffix in ["-journal", "-wal", "-shm"] {
        assert_eq!(mode(&side(&db, suffix)), 0o600, "{suffix}");
    }
}

// ---- refusals --------------------------------------------------------------

#[test]
fn a_symlinked_database_is_unavailable_and_untouched() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    fs::create_dir(db.parent().unwrap()).unwrap();
    let target = dir.path().join("target");
    fs::write(&target, b"precious").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
    symlink(&target, &db).unwrap();

    assert!(matches!(open_as(&db, me()), Err(StoreError::Unsafe(_))));
    assert_eq!(fs::read(&target).unwrap(), b"precious");
    assert_eq!(mode(&target), 0o644);
}

#[test]
fn a_symlinked_side_file_is_unavailable() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    drop(open_as(&db, me()).unwrap());
    let target = dir.path().join("target");
    fs::write(&target, b"precious").unwrap();
    symlink(&target, side(&db, "-journal")).unwrap();

    assert!(matches!(open_as(&db, me()), Err(StoreError::Unsafe(_))));
    assert_eq!(fs::read(&target).unwrap(), b"precious");
}

#[test]
fn a_symlinked_data_directory_is_unavailable() {
    let dir = TempDir::new().unwrap();
    let real = dir.path().join("real");
    fs::create_dir(&real).unwrap();
    symlink(&real, dir.path().join("lazarobox")).unwrap();

    assert!(matches!(
        open_as(&store_path(&dir), me()),
        Err(StoreError::Unsafe(_))
    ));
    assert_eq!(fs::read_dir(&real).unwrap().count(), 0);
}

#[test]
fn a_database_of_a_foreign_owner_is_unavailable_and_untouched() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    drop(open_as(&db, me()).unwrap());
    fs::set_permissions(&db, fs::Permissions::from_mode(0o644)).unwrap();
    let before = fs::read(&db).unwrap();

    // A different expected uid stands in for a file somebody else owns.
    assert!(matches!(open_as(&db, me() + 1), Err(StoreError::Unsafe(_))));
    assert_eq!(fs::read(&db).unwrap(), before);
    assert_eq!(mode(&db), 0o644);
}

#[test]
fn a_directory_in_place_of_the_database_is_unavailable() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    fs::create_dir_all(&db).unwrap();
    assert!(matches!(open_as(&db, me()), Err(StoreError::Unsafe(_))));
}

// ---- pragmas ---------------------------------------------------------------

#[test]
fn the_connection_pragmas_are_set() {
    let dir = TempDir::new().unwrap();
    let conn = open_as(&store_path(&dir), me()).unwrap();
    assert_eq!(pragma_text(&conn, "secure_delete"), "1");
    assert_eq!(pragma_text(&conn, "foreign_keys"), "1");
    assert_eq!(pragma_text(&conn, "trusted_schema"), "0");
    assert_eq!(pragma_text(&conn, "busy_timeout"), "2000");
    assert_eq!(pragma_text(&conn, "journal_mode"), "delete");
}

#[test]
fn a_database_left_in_wal_mode_is_switched_back_to_delete() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    fs::create_dir(db.parent().unwrap()).unwrap();
    {
        let conn = Connection::open(&db).unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode = wal", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }
    let conn = open_as(&db, me()).unwrap();
    assert_eq!(pragma_text(&conn, "journal_mode"), "delete");
    assert!(!side(&db, "-wal").exists());
}

#[test]
fn the_store_never_checkpoints() {
    for source in [include_str!("../chat_store.rs"), include_str!("worker.rs")] {
        assert!(!source.contains("wal_checkpoint"));
    }
}

#[test]
fn a_deleted_conversation_leaves_no_trace_in_any_file() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    let conn = open_as(&db, me()).unwrap();
    conn.execute_batch(
        "INSERT INTO conversations(id, created_at, updated_at, profile, title)
           VALUES (1, 1, 1, 'analyst', 'ZZ-MARKER-ZZ');
         INSERT INTO messages(conversation_id, seq, role, content, engine, model, status, created_at)
           VALUES (1, 0, 'user', 'ZZ-MARKER-ZZ hello', 'ollama', 'm', 'complete', 1);
         INSERT INTO tags(conversation_id, tag, is_project) VALUES (1, 'ZZ-MARKER-ZZ', 0);",
    )
    .unwrap();
    let bytes = |p: &Path| fs::read(p).unwrap_or_default();
    let has = |p: &Path| bytes(p).windows(12).any(|w| w == b"ZZ-MARKER-ZZ");
    assert!(
        has(&db),
        "the marker should be in the file before the delete"
    );

    conn.execute("DELETE FROM conversations WHERE id = 1", [])
        .unwrap();
    for path in [
        db.clone(),
        side(&db, "-journal"),
        side(&db, "-wal"),
        side(&db, "-shm"),
    ] {
        assert!(!has(&path), "{path:?} still holds the marker");
    }
}

// ---- schema ----------------------------------------------------------------

fn names(conn: &Connection, kind: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT name FROM sqlite_master WHERE type = '{kind}' AND name NOT LIKE 'sqlite_%' ORDER BY name"
        ))
        .unwrap();
    stmt.query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn conversation(conn: &Connection, id: i64) {
    conn.execute(
        "INSERT INTO conversations(id, created_at, updated_at, profile, title)
         VALUES (?1, 1, 1, 'programmer', 't')",
        [id],
    )
    .unwrap();
}

fn message(conn: &Connection, conv: i64, seq: i64, status: &str) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO messages(conversation_id, seq, role, content, engine, model, status, created_at)
         VALUES (?1, ?2, 'assistant', 'c', 'e', NULL, ?3, 1)",
        rusqlite::params![conv, seq, status],
    )
}

#[test]
fn a_fresh_store_gets_schema_v3() {
    let dir = TempDir::new().unwrap();
    let conn = open_as(&store_path(&dir), me()).unwrap();
    assert_eq!(pragma_text(&conn, "user_version"), "3");
    assert_eq!(
        names(&conn, "table"),
        [
            "conversations",
            "memory_notes",
            "memory_sessions",
            "messages",
            "tags",
            "turn_usage"
        ]
    );
    let indexes = names(&conn, "index");
    assert!(indexes.iter().any(|n| n == "tags_by_tag"), "{indexes:?}");
    assert!(
        indexes.iter().any(|n| n == "conversations_by_activity"),
        "{indexes:?}"
    );
}

#[test]
fn every_current_status_is_stored() {
    let dir = TempDir::new().unwrap();
    let conn = open_as(&store_path(&dir), me()).unwrap();
    conversation(&conn, 1);
    for (seq, ok) in ["complete", "cancelled", "interrupted", "truncated", "error"]
        .into_iter()
        .enumerate()
    {
        message(&conn, 1, seq as i64, ok).unwrap();
    }
}

#[test]
fn a_sequence_number_is_unique_within_a_conversation() {
    let dir = TempDir::new().unwrap();
    let conn = open_as(&store_path(&dir), me()).unwrap();
    conversation(&conn, 1);
    conversation(&conn, 2);
    message(&conn, 1, 0, "complete").unwrap();
    assert!(message(&conn, 1, 0, "complete").is_err());
    message(&conn, 2, 0, "complete").unwrap();
}

#[test]
fn deleting_a_conversation_removes_its_messages_and_tags() {
    let dir = TempDir::new().unwrap();
    let conn = open_as(&store_path(&dir), me()).unwrap();
    conversation(&conn, 1);
    message(&conn, 1, 0, "complete").unwrap();
    conn.execute(
        "INSERT INTO tags(conversation_id, tag, is_project) VALUES (1, 'a', 1)",
        [],
    )
    .unwrap();
    // A message of a conversation that does not exist is refused.
    assert!(message(&conn, 9, 0, "complete").is_err());

    conn.execute("DELETE FROM conversations WHERE id = 1", [])
        .unwrap();
    for table in ["messages", "tags"] {
        let n: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "{table}");
    }
}

#[test]
fn reopening_keeps_the_data_and_the_version() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    let conn = open_as(&db, me()).unwrap();
    conversation(&conn, 1);
    drop(conn);

    let conn = open_as(&db, me()).unwrap();
    assert_eq!(pragma_text(&conn, "user_version"), "3");
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM conversations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn an_empty_database_at_version_zero_is_migrated() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    fs::create_dir(db.parent().unwrap()).unwrap();
    drop(Connection::open(&db).unwrap());
    let conn = open_as(&db, me()).unwrap();
    assert_eq!(pragma_text(&conn, "user_version"), "3");
    assert_eq!(names(&conn, "table").len(), 6);
}

#[test]
fn a_newer_schema_makes_the_store_unavailable_and_is_left_alone() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    drop(open_as(&db, me()).unwrap());
    Connection::open(&db)
        .unwrap()
        .execute_batch("PRAGMA user_version = 4")
        .unwrap();
    let before = fs::read(&db).unwrap();

    assert_eq!(open_as(&db, me()).err(), Some(StoreError::Newer(4)));
    assert_eq!(fs::read(&db).unwrap(), before);
}

#[test]
fn a_corrupt_file_is_unavailable_and_left_alone() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    fs::create_dir(db.parent().unwrap()).unwrap();
    fs::write(
        &db,
        b"this is not a database, just some text padding it out",
    )
    .unwrap();
    let before = fs::read(&db).unwrap();

    assert!(matches!(open_as(&db, me()), Err(StoreError::Sqlite(_))));
    assert_eq!(fs::read(&db).unwrap(), before);
}

#[test]
fn errors_read_as_short_lines_without_paths() {
    for err in [
        StoreError::NoPath,
        StoreError::Unsafe("symlink"),
        StoreError::Newer(7),
        StoreError::Io("denied"),
        StoreError::Sqlite("not a database"),
    ] {
        let text = err.to_string();
        assert!(!text.is_empty() && text.len() <= 80, "{text}");
        assert!(!text.contains('/'), "{text}");
    }
}

// ---- review fixes ----------------------------------------------------------

#[test]
fn a_symlinked_ancestor_of_the_data_directory_is_fine() {
    let dir = TempDir::new().unwrap();
    let real = dir.path().join("real");
    fs::create_dir(&real).unwrap();
    symlink(&real, dir.path().join("alias")).unwrap();
    let db = dir.path().join("alias").join("lazarobox").join("chat.db");

    let conn = open_as(&db, me()).unwrap();
    assert_eq!(pragma_text(&conn, "user_version"), "3");
    assert!(real.join("lazarobox").join("chat.db").is_file());
}

#[test]
fn two_instances_migrating_a_fresh_store_both_succeed() {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("chat.db");
    let mut b = Connection::open(&db).unwrap();
    b.busy_timeout(BUSY_TIMEOUT).unwrap();

    // The other instance gets in right after this one decided what to do.
    migrate_with(&mut b, || {
        let mut a = Connection::open(&db).unwrap();
        migrate(&mut a).unwrap();
    })
    .unwrap();
    assert_eq!(pragma_text(&b, "user_version"), "3");
}

#[test]
fn a_newer_wal_database_is_not_rewritten_before_the_refusal() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    fs::create_dir(db.parent().unwrap()).unwrap();
    {
        let conn = Connection::open(&db).unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode = wal", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        conn.execute_batch("PRAGMA user_version = 4").unwrap();
    }
    let before = fs::read(&db).unwrap();
    let wal_before = side(&db, "-wal").exists();

    assert_eq!(open_as(&db, me()).err(), Some(StoreError::Newer(4)));
    assert!(fs::read(&db).unwrap() == before, "the main file changed");
    assert_eq!(side(&db, "-wal").exists(), wal_before);
}

#[test]
fn a_failed_open_names_no_path() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("secret-dir-name").join("x.db");
    let err = Connection::open_with_flags(&missing, OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap_err();
    let text = StoreError::from(err).to_string();
    let tmp_name = dir
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(!text.contains("secret-dir-name"), "{text}");
    assert!(!text.contains(&tmp_name), "{text}");
    assert!(!text.contains("tmp"), "{text}");
    assert_eq!(text, "cannot open database");
}

// ---- schema v2 (8.1) -------------------------------------------------------

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

/// A database exactly as a v1-only build leaves it, with some rows.
fn v1_store(db: &Path) {
    let conn = open_with(db, me(), 1).unwrap();
    assert_eq!(pragma_text(&conn, "user_version"), "1");
    assert_eq!(names(&conn, "table"), ["conversations", "messages", "tags"]);
    conversation(&conn, 1);
    conversation(&conn, 2);
    message(&conn, 1, 1, "complete").unwrap();
    message(&conn, 1, 2, "complete").unwrap();
    conn.execute(
        "INSERT INTO tags(conversation_id, tag, is_project) VALUES (2, 'keep', 0)",
        [],
    )
    .unwrap();
}

#[test]
fn a_v1_database_opens_as_v3_with_every_row_intact() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    v1_store(&db);

    let conn = open_as(&db, me()).unwrap();
    assert_eq!(pragma_text(&conn, "user_version"), "3");
    assert_eq!(count(&conn, "conversations"), 2);
    assert_eq!(count(&conn, "messages"), 2);
    assert_eq!(count(&conn, "tags"), 1);
    assert_eq!(count(&conn, "memory_sessions"), 0);
    assert_eq!(count(&conn, "memory_notes"), 0);
}

#[test]
fn the_v2_migration_is_one_transaction() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    v1_store(&db);
    // A pre-existing table of the same name makes the second statement of the
    // v2 batch fail: nothing of the batch may survive.
    Connection::open(&db)
        .unwrap()
        .execute_batch("CREATE TABLE memory_notes(x)")
        .unwrap();

    assert!(open_as(&db, me()).is_err());
    let conn = Connection::open(&db).unwrap();
    assert_eq!(pragma_text(&conn, "user_version"), "1");
    assert!(!names(&conn, "table").contains(&"memory_sessions".to_owned()));
}

#[test]
fn a_v1_only_opener_refuses_a_v2_file_and_leaves_its_bytes() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    drop(open_as(&db, me()).unwrap());
    let before = fs::read(&db).unwrap();

    assert_eq!(open_with(&db, me(), 1).err(), Some(StoreError::Newer(3)));
    assert!(fs::read(&db).unwrap() == before, "the v2 file changed");
    for suffix in ["-journal", "-wal", "-shm"] {
        assert!(!side(&db, suffix).exists());
    }
}

#[test]
fn the_link_tables_have_the_designed_shape() {
    let dir = TempDir::new().unwrap();
    let conn = open_as(&store_path(&dir), me()).unwrap();
    let cols = |table: &str| -> Vec<(String, i64)> {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT name, pk FROM pragma_table_info('{table}')"
            ))
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(
        cols("memory_sessions"),
        [
            ("conversation_id".to_owned(), 1),
            ("session_id".to_owned(), 0),
            ("project".to_owned(), 0)
        ]
    );
    assert_eq!(
        cols("memory_notes"),
        [
            ("project".to_owned(), 1),
            ("topic_key".to_owned(), 2),
            ("observation_id".to_owned(), 0),
            ("conversation_id".to_owned(), 0),
            ("title".to_owned(), 0)
        ]
    );
    let without_rowid: i64 = conn
        .query_row(
            "SELECT wr FROM pragma_table_list('memory_notes')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(without_rowid, 1);

    // The session id is unique, and both tables follow their conversation.
    conversation(&conn, 1);
    conversation(&conn, 2);
    let session = |conv: i64, id: &str| {
        conn.execute(
            "INSERT INTO memory_sessions(conversation_id, session_id, project) VALUES (?1, ?2, 'p')",
            rusqlite::params![conv, id],
        )
    };
    session(1, "lazarobox-aa").unwrap();
    assert!(
        session(2, "lazarobox-aa").is_err(),
        "session ids are unique"
    );
    conn.execute(
        "INSERT INTO memory_notes VALUES ('p', 'chat/decision/x', 7, 1, 't')",
        [],
    )
    .unwrap();
    conn.execute("DELETE FROM conversations WHERE id = 1", [])
        .unwrap();
    assert_eq!(count(&conn, "memory_sessions"), 0);
    assert_eq!(count(&conn, "memory_notes"), 0);
}

#[test]
fn the_link_tables_are_covered_by_secure_delete_and_the_file_modes() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    let conn = open_as(&db, me()).unwrap();
    assert_eq!(pragma_text(&conn, "secure_delete"), "1");
    conversation(&conn, 1);
    conn.execute(
        "INSERT INTO memory_sessions VALUES (1, 'lazarobox-ZZ-MARKER-ZZ', 'p')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO memory_notes VALUES ('p', 'chat/decision/ZZ-MARKER-ZZ', 7, 1, 'ZZ-MARKER-ZZ')",
        [],
    )
    .unwrap();
    let has = |p: &Path| {
        fs::read(p)
            .unwrap_or_default()
            .windows(12)
            .any(|w| w == b"ZZ-MARKER-ZZ")
    };
    assert!(
        has(&db),
        "the marker should be in the file before the delete"
    );
    assert_eq!(mode(&db), 0o600);

    conn.execute("DELETE FROM conversations WHERE id = 1", [])
        .unwrap();
    for suffix in ["", "-journal", "-wal", "-shm"] {
        assert!(!has(&side(&db, suffix)), "{suffix} still holds the marker");
    }
    assert_eq!(mode(&db), 0o600);
}

// ---- the folder column (chat-folder) ---------------------------------------

fn columns(conn: &Connection) -> Vec<String> {
    conn.prepare("SELECT name FROM pragma_table_info('conversations')")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn a_fresh_store_has_a_nullable_folder_column() {
    let dir = TempDir::new().unwrap();
    let conn = open_as(&store_path(&dir), me()).unwrap();
    assert!(columns(&conn).contains(&"folder".to_owned()));
    conversation(&conn, 1);
    let folder: Option<String> = conn
        .query_row("SELECT folder FROM conversations WHERE id = 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(folder, None);
}

#[test]
fn a_v2_file_without_the_column_gets_it_and_keeps_its_rows() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    {
        let conn = open_as(&db, me()).unwrap();
        conversation(&conn, 1);
        conn.execute_batch("ALTER TABLE conversations DROP COLUMN folder")
            .unwrap();
        assert!(!columns(&conn).contains(&"folder".to_owned()));
    }
    let conn = open_as(&db, me()).unwrap();
    assert!(columns(&conn).contains(&"folder".to_owned()));
    assert_eq!(count(&conn, "conversations"), 1);
    assert_eq!(pragma_text(&conn, "user_version"), "3");
}

#[test]
fn the_folder_column_is_added_once() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    drop(open_as(&db, me()).unwrap());
    let conn = open_as(&db, me()).unwrap();
    let folders = columns(&conn).iter().filter(|c| *c == "folder").count();
    assert_eq!(folders, 1);
}

#[test]
fn a_v1_database_gets_the_folder_column_when_it_becomes_v2() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    v1_store(&db);
    let conn = open_as(&db, me()).unwrap();
    assert!(columns(&conn).contains(&"folder".to_owned()));
}

#[test]
fn an_older_build_still_opens_a_file_that_has_the_column() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    drop(open_with(&db, me(), 2).unwrap());
    let conn = open_with(&db, me(), 2).unwrap();
    assert!(columns(&conn).contains(&"folder".to_owned()));
    assert_eq!(pragma_text(&conn, "user_version"), "2");
    // The old build's INSERT names its columns and ignores the new one.
    conn.execute(
        "INSERT INTO conversations(created_at, updated_at, profile, title)
         VALUES (1, 1, 'analyst', 't')",
        [],
    )
    .unwrap();
}

// ---- schema v3 and the fixture files (1.0 storage freeze) -------------------

const FIXTURE_V1: &str = include_str!("../../../tests/fixtures/chat_store/v1.sql");
const FIXTURE_V2: &str = include_str!("../../../tests/fixtures/chat_store/v2.sql");

/// Builds `chat.db` from a checked-in fixture script, as an old build left it.
fn from_fixture(dir: &TempDir, script: &str) -> PathBuf {
    let db = store_path(dir);
    fs::create_dir(db.parent().unwrap()).unwrap();
    Connection::open(&db)
        .unwrap()
        .execute_batch(script)
        .unwrap();
    db
}

/// Every row of `select`, one line each, values debug-quoted.
fn dump(conn: &Connection, select: &str) -> Vec<String> {
    let mut stmt = conn.prepare(select).unwrap();
    let cols = stmt.column_count();
    stmt.query_map([], |r| {
        let mut line = Vec::new();
        for i in 0..cols {
            let v: rusqlite::types::Value = r.get(i)?;
            line.push(format!("{v:?}"));
        }
        Ok(line.join("|"))
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

const TABLE_DUMPS: [&str; 6] = [
    "SELECT id, created_at, updated_at, profile, title FROM conversations ORDER BY id",
    "SELECT rowid, * FROM messages ORDER BY rowid",
    "SELECT * FROM tags ORDER BY conversation_id, tag",
    "SELECT * FROM memory_sessions ORDER BY conversation_id",
    "SELECT * FROM memory_notes ORDER BY project, topic_key",
    "SELECT id, folder FROM conversations ORDER BY id",
];

/// The v1 fixture has no memory tables and no folder column yet.
fn snapshot(db: &Path, v1: bool) -> Vec<Vec<String>> {
    let conn = Connection::open(db).unwrap();
    TABLE_DUMPS
        .iter()
        .enumerate()
        .map(|(i, s)| {
            if v1 && i >= 3 {
                Vec::new()
            } else {
                dump(&conn, s)
            }
        })
        .collect()
}

fn assert_v3_intact(db: &Path, before: &[Vec<String>], v1: bool) {
    let conn = open_as(db, me()).unwrap();
    assert_eq!(pragma_text(&conn, "user_version"), "3");
    for (i, select) in TABLE_DUMPS.iter().enumerate() {
        if v1 && i >= 3 {
            continue;
        }
        assert_eq!(&dump(&conn, select), &before[i], "{select}");
    }
    let broken = dump(&conn, "SELECT * FROM pragma_foreign_key_check");
    assert!(broken.is_empty(), "{broken:?}");
    let indexes = names(&conn, "index");
    for want in [
        "tags_by_tag",
        "conversations_by_activity",
        "memory_notes_by_conversation",
    ] {
        assert!(indexes.iter().any(|n| n == want), "{want} in {indexes:?}");
    }
    // Foreign keys are back on and still cascade after the rebuild.
    assert_eq!(pragma_text(&conn, "foreign_keys"), "1");
    conn.execute("DELETE FROM conversations WHERE id = 1", [])
        .unwrap();
    assert_eq!(
        count(&conn, "messages"),
        before[1].len() as i64 - 3,
        "the cascade removed conversation 1's three messages"
    );
    // The tables that were not rebuilt still point at the new `conversations`
    // and cascade with it.
    if !v1 {
        assert_eq!(count(&conn, "memory_sessions"), 1);
        assert_eq!(count(&conn, "memory_notes"), 1);
    }
    for table in ["messages", "tags", "memory_sessions", "memory_notes"] {
        let sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = ?1",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            sql.contains("REFERENCES conversations(id)"),
            "{table}: {sql}"
        );
    }
}

#[test]
fn a_v2_fixture_migrates_to_v3_keeping_every_row_id_and_folder() {
    let dir = TempDir::new().unwrap();
    let db = from_fixture(&dir, FIXTURE_V2);
    let before = snapshot(&db, false);
    assert_eq!(before[1].len(), 5);
    // The fixture has a hole in its rowids (rowid 4 was deleted).
    assert!(!before[1].iter().any(|l| l.starts_with("Integer(4)|")));
    assert!(before[5][0].contains("/home/u/work/parser"));
    assert_v3_intact(&db, &before, false);
}

#[test]
fn a_v1_fixture_migrates_to_v3_with_every_row_intact_and_a_folder_column() {
    let dir = TempDir::new().unwrap();
    let db = from_fixture(&dir, FIXTURE_V1);
    let before = snapshot(&db, true);
    assert_v3_intact(&db, &before, true);
    let conn = open_as(&db, me()).unwrap();
    assert!(columns(&conn).contains(&"folder".to_owned()));
    assert_eq!(count(&conn, "memory_sessions"), 0);
}

#[test]
fn v3_has_no_enum_checks_so_a_new_role_profile_or_status_is_a_plain_insert() {
    let dir = TempDir::new().unwrap();
    let conn = open_as(&store_path(&dir), me()).unwrap();
    conn.execute(
        "INSERT INTO conversations(id, created_at, updated_at, profile, title)
         VALUES (1, 1, 1, 'reviewer', 't')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO messages(conversation_id, seq, role, content, engine, model, status, created_at)
         VALUES (1, 1, 'tool', 'c', 'e', NULL, 'partial', 1)",
        [],
    )
    .unwrap();
    // What stays enforced: uniqueness, the foreign key and not-null.
    assert!(message(&conn, 1, 1, "complete").is_err());
    assert!(message(&conn, 99, 1, "complete").is_err());
    assert!(
        conn.execute(
            "INSERT INTO conversations(created_at, updated_at, profile, title)
             VALUES (1, 1, NULL, 't')",
            [],
        )
        .is_err()
    );
}

#[test]
fn reopening_a_v3_store_changes_nothing() {
    let dir = TempDir::new().unwrap();
    let db = from_fixture(&dir, FIXTURE_V2);
    drop(open_as(&db, me()).unwrap());
    let first = snapshot(&db, false);
    let conn = open_as(&db, me()).unwrap();
    assert_eq!(pragma_text(&conn, "user_version"), "3");
    drop(conn);
    assert_eq!(snapshot(&db, false), first);
}

#[test]
fn a_v2_build_refuses_a_v3_file_and_leaves_its_bytes() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    drop(open_as(&db, me()).unwrap());
    let before = fs::read(&db).unwrap();

    assert_eq!(open_with(&db, me(), 2).err(), Some(StoreError::Newer(3)));
    assert!(fs::read(&db).unwrap() == before, "the v3 file changed");
    for suffix in ["-journal", "-wal", "-shm"] {
        assert!(!side(&db, suffix).exists());
    }
}

#[test]
fn two_instances_migrating_a_v2_store_migrate_it_once() {
    let dir = TempDir::new().unwrap();
    let db = from_fixture(&dir, FIXTURE_V2);
    let before = snapshot(&db, false);
    let mut b = Connection::open(&db).unwrap();
    b.busy_timeout(BUSY_TIMEOUT).unwrap();

    migrate_with(&mut b, || {
        let mut a = Connection::open(&db).unwrap();
        migrate(&mut a).unwrap();
    })
    .unwrap();
    assert_eq!(pragma_text(&b, "user_version"), "3");
    drop(b);
    assert_v3_intact(&db, &before, false);
}

#[test]
fn a_failed_v3_rebuild_rolls_back_to_the_untouched_v2_file() {
    let dir = TempDir::new().unwrap();
    let db = from_fixture(&dir, FIXTURE_V2);
    // A leftover table of the scratch name makes the rebuild fail midway.
    Connection::open(&db)
        .unwrap()
        .execute_batch("CREATE TABLE new_messages(x)")
        .unwrap();
    let before = snapshot(&db, false);

    assert!(open_as(&db, me()).is_err());
    let conn = Connection::open(&db).unwrap();
    assert_eq!(pragma_text(&conn, "user_version"), "2");
    assert_eq!(snapshot(&db, false), before);
}

#[test]
fn an_orphan_row_is_dropped_and_counted_instead_of_blocking_the_migration() {
    let dir = TempDir::new().unwrap();
    let db = from_fixture(&dir, FIXTURE_V2);
    {
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = OFF;
             INSERT INTO messages(conversation_id, seq, role, content, engine, model, status, created_at)
                 VALUES (99, 1, 'user', 'orphan', 'e', NULL, 'complete', 1);
             INSERT INTO tags(conversation_id, tag, is_project) VALUES (99, 'x', 0);",
        )
        .unwrap();
    }
    let mut conn = Connection::open(&db).unwrap();
    assert_eq!(migrate(&mut conn).unwrap(), 2);
    assert_eq!(pragma_text(&conn, "user_version"), "3");
    assert_eq!(count(&conn, "messages"), 5);
    assert_eq!(count(&conn, "tags"), 3);
}

#[test]
fn foreign_keys_are_restored_as_they_were_even_when_the_migration_fails() {
    for was_on in [true, false] {
        let dir = TempDir::new().unwrap();
        let db = from_fixture(&dir, FIXTURE_V2);
        Connection::open(&db)
            .unwrap()
            .execute_batch("CREATE TABLE new_messages(x)")
            .unwrap();
        let mut conn = Connection::open(&db).unwrap();
        conn.pragma_update(None, "foreign_keys", was_on).unwrap();
        assert!(migrate(&mut conn).is_err());
        assert_eq!(
            pragma_text(&conn, "foreign_keys"),
            if was_on { "1" } else { "0" }
        );
    }
}

#[test]
fn a_migrated_store_shrinks_back_and_stays_private() {
    let dir = TempDir::new().unwrap();
    let db = from_fixture(&dir, FIXTURE_V2);
    {
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 4000)
             INSERT INTO messages(conversation_id, seq, role, content, engine, model, status, created_at)
                 SELECT 1, i + 10, 'user', hex(randomblob(1000)), 'e', NULL, 'complete', 1 FROM n;",
        )
        .unwrap();
    }
    let before = fs::metadata(&db).unwrap().len();
    let conn = open_as(&db, me()).unwrap();
    let after = fs::metadata(&db).unwrap().len();
    assert!(after * 5 <= before * 6, "{before} -> {after}");
    assert_eq!(count(&conn, "messages"), 4005);
    assert_eq!(pragma_text(&conn, "secure_delete"), "1");
    assert_eq!(mode(&db), 0o600);
    for suffix in ["-journal", "-wal", "-shm"] {
        assert!(!side(&db, suffix).exists());
    }
}

// ---- usage table (ai-cost 1b.1) --------------------------------------------

fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")
        .unwrap();
    stmt.query_map([table], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn a_v3_file_gains_the_usage_table_without_a_version_bump() {
    let dir = TempDir::new().unwrap();
    let db = from_fixture(&dir, FIXTURE_V2);
    drop(open_as(&db, me()).unwrap());
    // Back to a file as a v3 build left it: no usage table.
    let raw = Connection::open(&db).unwrap();
    raw.execute_batch("DROP TABLE turn_usage").unwrap();
    drop(raw);
    let conn = open_as(&db, me()).unwrap();
    assert_eq!(pragma_text(&conn, "user_version"), "3");
    assert!(names(&conn, "table").contains(&"turn_usage".to_owned()));
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM turn_usage", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    // The same file stays open to a build that only knows v3.
    drop(conn);
    assert!(open_with(&db, me(), 3).is_ok());
}

#[test]
fn the_usage_table_holds_only_counts_ids_and_timestamps() {
    let dir = TempDir::new().unwrap();
    let conn = open_as(&store_path(&dir), me()).unwrap();
    assert_eq!(
        table_columns(&conn, "turn_usage"),
        [
            "id",
            "ts",
            "conversation_id",
            "engine",
            "model",
            "role",
            "worker",
            "input",
            "output",
            "cache_read",
            "cache_write",
            "reasoning",
            "cost_micros",
            "cost_source",
            "duration_ms",
            "partial"
        ]
    );
}

#[test]
fn deleting_a_conversation_keeps_its_usage_with_the_link_cleared() {
    let dir = TempDir::new().unwrap();
    let conn = open_as(&store_path(&dir), me()).unwrap();
    conversation(&conn, 1);
    for ts in [10, 20, 30] {
        conn.execute(
            "INSERT INTO turn_usage(ts, conversation_id, engine, output) VALUES (?1, 1, 'ollama', 5)",
            [ts],
        )
        .unwrap();
    }
    conn.execute("DELETE FROM conversations WHERE id = 1", [])
        .unwrap();
    let kept: (i64, i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), COUNT(conversation_id), SUM(output) FROM turn_usage",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(kept, (3, 0, 15));
}

#[test]
fn a_v2_build_still_refuses_and_a_usage_table_does_not_move_the_version() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    drop(open_as(&db, me()).unwrap());
    assert_eq!(open_with(&db, me(), 2).err(), Some(StoreError::Newer(3)));
}
