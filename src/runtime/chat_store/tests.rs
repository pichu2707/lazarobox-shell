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
fn a_fresh_store_gets_schema_v2() {
    let dir = TempDir::new().unwrap();
    let conn = open_as(&store_path(&dir), me()).unwrap();
    assert_eq!(pragma_text(&conn, "user_version"), "2");
    assert_eq!(
        names(&conn, "table"),
        [
            "conversations",
            "memory_notes",
            "memory_sessions",
            "messages",
            "tags"
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
fn the_status_enum_and_the_profile_are_checked() {
    let dir = TempDir::new().unwrap();
    let conn = open_as(&store_path(&dir), me()).unwrap();
    conversation(&conn, 1);
    for (seq, ok) in ["complete", "cancelled", "interrupted", "truncated", "error"]
        .into_iter()
        .enumerate()
    {
        message(&conn, 1, seq as i64, ok).unwrap();
    }
    assert!(message(&conn, 1, 100, "partial").is_err());
    let bad_profile = conn.execute(
        "INSERT INTO conversations(created_at, updated_at, profile, title) VALUES (1, 1, 'x', 't')",
        [],
    );
    assert!(bad_profile.is_err());
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
    assert_eq!(pragma_text(&conn, "user_version"), "2");
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
    assert_eq!(pragma_text(&conn, "user_version"), "2");
    assert_eq!(names(&conn, "table").len(), 5);
}

#[test]
fn a_newer_schema_makes_the_store_unavailable_and_is_left_alone() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    drop(open_as(&db, me()).unwrap());
    Connection::open(&db)
        .unwrap()
        .execute_batch("PRAGMA user_version = 3")
        .unwrap();
    let before = fs::read(&db).unwrap();

    assert_eq!(open_as(&db, me()).err(), Some(StoreError::Newer(3)));
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
    assert_eq!(pragma_text(&conn, "user_version"), "2");
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
    assert_eq!(pragma_text(&b, "user_version"), "2");
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
        conn.execute_batch("PRAGMA user_version = 3").unwrap();
    }
    let before = fs::read(&db).unwrap();
    let wal_before = side(&db, "-wal").exists();

    assert_eq!(open_as(&db, me()).err(), Some(StoreError::Newer(3)));
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
fn a_v1_database_opens_as_v2_with_every_row_intact() {
    let dir = TempDir::new().unwrap();
    let db = store_path(&dir);
    v1_store(&db);

    let conn = open_as(&db, me()).unwrap();
    assert_eq!(pragma_text(&conn, "user_version"), "2");
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

    assert_eq!(open_with(&db, me(), 1).err(), Some(StoreError::Newer(2)));
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
    assert_eq!(pragma_text(&conn, "user_version"), "2");
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
    drop(open_as(&db, me()).unwrap());
    let conn = open_with(&db, me(), 2).unwrap();
    assert_eq!(pragma_text(&conn, "user_version"), "2");
    // The old build's INSERT names its columns and ignores the new one.
    conn.execute(
        "INSERT INTO conversations(created_at, updated_at, profile, title)
         VALUES (1, 1, 'analyst', 't')",
        [],
    )
    .unwrap();
}
