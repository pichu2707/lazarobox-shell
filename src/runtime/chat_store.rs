//! The chat history database: where it lives, how it is opened and what its
//! schema looks like. Opening is the whole privacy story, so it is kept apart
//! from the store thread that will own the connection (C6b).

use std::{
    ffi::OsStr,
    fmt, fs, io,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{Connection, OpenFlags, TransactionBehavior};

pub mod runs;
pub mod worker;
pub use runs::highest_run;
pub use worker::{StoreHandle, spawn, spawn_at};

/// The schema version this build writes and understands.
const SCHEMA_VERSION: i64 = 3;

/// How long a write waits for another instance before it gives up.
const BUSY_TIMEOUT: Duration = Duration::from_millis(2000);

/// The files SQLite may keep next to the database.
const SIDE_SUFFIXES: [&str; 3] = ["-journal", "-wal", "-shm"];

const SCHEMA_V1: &str = "
CREATE TABLE conversations (
    id         INTEGER PRIMARY KEY,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    profile    TEXT NOT NULL CHECK (profile IN ('analyst', 'programmer')),
    title      TEXT NOT NULL
);
CREATE TABLE messages (
    conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    seq             INTEGER NOT NULL,
    role            TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    content         TEXT NOT NULL,
    engine          TEXT NOT NULL,
    model           TEXT,
    status          TEXT NOT NULL
        CHECK (status IN ('complete', 'cancelled', 'interrupted', 'truncated', 'error')),
    created_at      INTEGER NOT NULL,
    UNIQUE (conversation_id, seq)
);
CREATE TABLE tags (
    conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    tag             TEXT NOT NULL,
    is_project      INTEGER NOT NULL CHECK (is_project IN (0, 1)),
    PRIMARY KEY (conversation_id, tag)
) WITHOUT ROWID;
CREATE INDEX tags_by_tag ON tags(tag);
CREATE INDEX conversations_by_activity ON conversations(updated_at);
";

/// Schema v2 only adds: the link between a conversation and the engram
/// session and notes it wrote. No note content is kept here, only ids and the
/// title, and both tables go with their conversation.
const SCHEMA_V2: &str = "
CREATE TABLE memory_sessions (
    conversation_id INTEGER PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
    session_id      TEXT NOT NULL UNIQUE,
    project         TEXT NOT NULL
);
CREATE TABLE memory_notes (
    project         TEXT NOT NULL,
    topic_key       TEXT NOT NULL,
    observation_id  INTEGER NOT NULL,
    conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    title           TEXT NOT NULL,
    PRIMARY KEY (project, topic_key)
) WITHOUT ROWID;
CREATE INDEX memory_notes_by_conversation ON memory_notes(conversation_id);
";

/// Schema v3 (1.0 freeze) drops the closed-list CHECKs on `profile`, `role` and
/// `status`, so a new profile, role (`tool`) or status needs no table rebuild
/// later. The allowed values are validated in Rust when a row is written. Done
/// with SQLite's documented 12-step ALTER procedure for `conversations`,
/// `messages` and `tags`; the caller turns foreign keys off around it. Rowids,
/// ids, the `folder` column and every other constraint are carried over.
/// The usage table (ai-cost D7). Idempotent, so it runs on every open.
const SCHEMA_USAGE: &str = "
    CREATE TABLE IF NOT EXISTS turn_usage (
        id INTEGER PRIMARY KEY,
        ts INTEGER NOT NULL,
        conversation_id INTEGER REFERENCES conversations(id) ON DELETE SET NULL,
        engine TEXT NOT NULL,
        model TEXT,
        role TEXT NOT NULL DEFAULT 'chat',
        worker TEXT,
        input INTEGER,
        output INTEGER,
        cache_read INTEGER,
        cache_write INTEGER,
        reasoning INTEGER,
        cost_micros INTEGER,
        cost_source TEXT,
        duration_ms INTEGER,
        partial INTEGER NOT NULL DEFAULT 0 CHECK (partial IN (0, 1))
    );
    CREATE INDEX IF NOT EXISTS turn_usage_by_ts ON turn_usage(ts);
    CREATE INDEX IF NOT EXISTS turn_usage_by_engine ON turn_usage(engine, ts);
    CREATE INDEX IF NOT EXISTS turn_usage_by_conversation ON turn_usage(conversation_id);
";

const SCHEMA_V3: &str = "
CREATE TABLE new_conversations (
    id         INTEGER PRIMARY KEY,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    profile    TEXT NOT NULL,
    title      TEXT NOT NULL,
    folder     TEXT
);
INSERT INTO new_conversations(id, created_at, updated_at, profile, title, folder)
    SELECT id, created_at, updated_at, profile, title, folder FROM conversations;
CREATE TABLE new_messages (
    conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    seq             INTEGER NOT NULL,
    role            TEXT NOT NULL,
    content         TEXT NOT NULL,
    engine          TEXT NOT NULL,
    model           TEXT,
    status          TEXT NOT NULL,
    created_at      INTEGER NOT NULL,
    UNIQUE (conversation_id, seq)
);
INSERT INTO new_messages(rowid, conversation_id, seq, role, content, engine, model, status, created_at)
    SELECT rowid, conversation_id, seq, role, content, engine, model, status, created_at FROM messages;
CREATE TABLE new_tags (
    conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    tag             TEXT NOT NULL,
    is_project      INTEGER NOT NULL CHECK (is_project IN (0, 1)),
    PRIMARY KEY (conversation_id, tag)
) WITHOUT ROWID;
INSERT INTO new_tags(conversation_id, tag, is_project)
    SELECT conversation_id, tag, is_project FROM tags;
DROP TABLE messages;
DROP TABLE tags;
DROP TABLE conversations;
ALTER TABLE new_conversations RENAME TO conversations;
ALTER TABLE new_messages RENAME TO messages;
ALTER TABLE new_tags RENAME TO tags;
CREATE INDEX tags_by_tag ON tags(tag);
CREATE INDEX conversations_by_activity ON conversations(updated_at);
";

/// Why the store cannot be used. The text is short and free of paths and
/// message content, so it can go into the `history unavailable:` line.
#[derive(Debug, PartialEq, Eq)]
pub enum StoreError {
    /// Neither `XDG_DATA_HOME` nor `HOME` gives a usable base.
    NoPath,
    /// A symlink, a foreign owner or a file of the wrong kind.
    Unsafe(&'static str),
    /// The schema was written by a newer build.
    Newer(i64),
    Io(&'static str),
    Sqlite(&'static str),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPath => f.write_str("no data directory"),
            Self::Unsafe(what) => write!(f, "unsafe store: {what}"),
            Self::Newer(v) => write!(f, "store schema {v} is newer than this build"),
            Self::Io(text) | Self::Sqlite(text) => f.write_str(text),
        }
    }
}

// Errors carry fixed texts chosen from the error kind or code, never the OS or
// SQLite message: those embed the full path of the database.
impl From<io::Error> for StoreError {
    fn from(err: io::Error) -> Self {
        Self::Io(match err.kind() {
            io::ErrorKind::NotFound => "not found",
            io::ErrorKind::PermissionDenied => "permission denied",
            _ => "i/o error",
        })
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(err: rusqlite::Error) -> Self {
        use rusqlite::ErrorCode as C;
        Self::Sqlite(match &err {
            rusqlite::Error::SqliteFailure(code, _) => match code.code {
                C::CannotOpen => "cannot open database",
                C::DatabaseBusy | C::DatabaseLocked => "database is locked",
                C::NotADatabase => "not a database",
                C::DatabaseCorrupt => "database is corrupt",
                C::SystemIoFailure => "disk I/O error",
                C::DiskFull => "disk full",
                C::ReadOnly => "database is read-only",
                C::PermissionDenied => "permission denied",
                _ => "database error",
            },
            _ => "database error",
        })
    }
}

/// The database path for the given environment values: an absolute
/// `XDG_DATA_HOME`, else an absolute `HOME` plus `.local/share`.
pub fn resolve_path(xdg: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    let absolute = |v: Option<&OsStr>| {
        v.map(Path::new)
            .filter(|p| p.is_absolute())
            .map(Path::to_path_buf)
    };
    let base = absolute(xdg).or_else(|| absolute(home).map(|h| h.join(".local/share")))?;
    Some(base.join("lazarobox").join("chat.db"))
}

/// Opens the store at `path` as the current user.
pub fn open(path: &Path) -> Result<Connection, StoreError> {
    // SAFETY: geteuid has no preconditions and cannot fail.
    open_as(path, unsafe { libc::geteuid() })
}

/// Opens the store at `path`, treating `uid` as the only acceptable owner.
///
/// The umask is never touched (PTY children inherit it): the directory and the
/// file are created with explicit modes instead.
pub(crate) fn open_as(path: &Path, uid: u32) -> Result<Connection, StoreError> {
    open_with(path, uid, SCHEMA_VERSION)
}

/// [`open_as`] for a build that understands schemas up to `supported`: a newer
/// file is refused untouched. Lets the tests play an older build.
pub(crate) fn open_with(path: &Path, uid: u32, supported: i64) -> Result<Connection, StoreError> {
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        Some(_) => Path::new("."),
        None => return Err(StoreError::NoPath),
    };
    let name = path.file_name().ok_or(StoreError::NoPath)?;
    prepare_dir(dir, uid)?;
    // SQLite applies NOFOLLOW to every path component, so a symlinked ancestor
    // (/home, ~/.local, XDG_DATA_HOME) would fail. The directory itself was just
    // checked to be a real one; resolving it keeps the flag guarding the leaf.
    let path = fs::canonicalize(dir)?.join(name);
    let path = path.as_path();
    check_file(path, uid)?;
    harden_side_files(path, uid)?;
    precreate(path, uid)?;

    let mut conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    conn.busy_timeout(BUSY_TIMEOUT)?;
    // A newer schema is refused before any pragma that could write to the file.
    refuse_newer(&conn, supported)?;
    conn.pragma_update(None, "trusted_schema", false)?;
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.pragma_update(None, "secure_delete", true)?;
    // The rollback journal goes away after each transaction, so no file keeps
    // deleted text around.
    let mode: String =
        conn.pragma_update_and_check(None, "journal_mode", "delete", |r| r.get(0))?;
    if !mode.eq_ignore_ascii_case("delete") {
        return Err(StoreError::Sqlite("journal mode not available"));
    }
    migrate_to(&mut conn, supported, || {})?;
    if supported >= 3 {
        // Usage is never worth the history: a table that cannot be made
        // surfaces again, with its reason, when a usage row is written.
        let _ = ensure_usage(&mut conn);
    }
    harden_side_files(path, uid)?;
    Ok(conn)
}

/// Why a private directory cannot be used.
#[derive(Debug)]
pub(crate) enum DirProblem {
    /// A symlink or a file of the wrong kind.
    NotADirectory,
    /// Owned by someone other than the expected user.
    ForeignOwner,
    Io(io::Error),
}

impl From<io::Error> for DirProblem {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

/// Creates a directory 0700, or checks and corrects an existing one: a
/// symlink, a non-directory or a foreign owner is refused, a looser mode is
/// tightened. Shared by every file lazarobox keeps privately.
pub(crate) fn secure_dir(dir: &Path, uid: u32) -> Result<(), DirProblem> {
    match fs::symlink_metadata(dir) {
        Ok(meta) => {
            if !meta.is_dir() {
                return Err(DirProblem::NotADirectory);
            }
            if meta.uid() != uid {
                return Err(DirProblem::ForeignOwner);
            }
            if meta.mode() & 0o7777 != 0o700 {
                fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
            }
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = dir.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::DirBuilder::new().mode(0o700).create(dir)?;
        }
        Err(err) => return Err(err.into()),
    }
    Ok(())
}

/// Creates the data directory 0700, or checks and corrects an existing one.
fn prepare_dir(dir: &Path, uid: u32) -> Result<(), StoreError> {
    secure_dir(dir, uid).map_err(|problem| match problem {
        DirProblem::NotADirectory => StoreError::Unsafe("data directory is not a directory"),
        DirProblem::ForeignOwner => StoreError::Unsafe("data directory has a foreign owner"),
        DirProblem::Io(err) => err.into(),
    })
}

/// Refuses a symlink, a non-regular file or a foreign owner, and corrects a
/// wider mode. A missing file is fine.
fn check_file(path: &Path, uid: u32) -> Result<(), StoreError> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };
    if meta.file_type().is_symlink() {
        return Err(StoreError::Unsafe("store file is a symlink"));
    }
    if !meta.is_file() {
        return Err(StoreError::Unsafe("store file is not a regular file"));
    }
    if meta.uid() != uid {
        return Err(StoreError::Unsafe("store file has a foreign owner"));
    }
    if meta.mode() & 0o7777 != 0o600 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Creates the database file 0600 without following a symlink, then checks
/// what was actually opened.
fn precreate(path: &Path, uid: u32) -> Result<(), StoreError> {
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|err| match err.raw_os_error() {
            Some(libc::ELOOP) => StoreError::Unsafe("store file is a symlink"),
            _ => err.into(),
        })?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(StoreError::Unsafe("store file is not a regular file"));
    }
    if meta.uid() != uid {
        return Err(StoreError::Unsafe("store file has a foreign owner"));
    }
    if meta.mode() & 0o7777 != 0o600 {
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Brings every side file that exists to mode 0600; a symlink or a foreign
/// owner among them is refused.
pub(crate) fn harden_side_files(db: &Path, uid: u32) -> Result<(), StoreError> {
    for suffix in SIDE_SUFFIXES {
        let mut name = db.as_os_str().to_owned();
        name.push(suffix);
        check_file(Path::new(&name), uid)?;
    }
    Ok(())
}

fn user_version(conn: &Connection) -> Result<i64, StoreError> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

fn refuse_newer(conn: &Connection, supported: i64) -> Result<(), StoreError> {
    match user_version(conn)? {
        v if v > supported => Err(StoreError::Newer(v)),
        _ => Ok(()),
    }
}

/// Brings the database to the current schema; a newer one is refused. Returns
/// how many orphan rows the v3 rebuild dropped.
#[cfg(test)]
fn migrate(conn: &mut Connection) -> Result<usize, StoreError> {
    migrate_with(conn, || {})
}

/// [`migrate`] with a hook just before the write lock is taken, where another
/// instance could get in first.
#[cfg(test)]
fn migrate_with(conn: &mut Connection, before_lock: impl FnOnce()) -> Result<usize, StoreError> {
    migrate_to(conn, SCHEMA_VERSION, before_lock)
}

/// Adds the nullable `conversations.folder` column when the file lacks it. It
/// is not a schema version: an older build never selects it and its inserts
/// name their columns, so it keeps working on the same file (chat-folder D8).
/// It runs on every open with `target >= 2`, so a later schema step (v3) may
/// assume `conversations.folder` exists.
fn ensure_folder(tx: &rusqlite::Transaction<'_>) -> Result<(), StoreError> {
    let present: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('conversations') WHERE name = 'folder')",
        [],
        |r| r.get(0),
    )?;
    if !present {
        tx.execute_batch("ALTER TABLE conversations ADD COLUMN folder TEXT")?;
    }
    Ok(())
}

/// Creates the usage table when the file lacks it. Like `ensure_folder` it is
/// not a schema version: `refuse_newer` would lock a v3 build out of the file,
/// and a v3 build never names the table. It holds counts, ids and timestamps
/// only, never text, and outlives its conversation (the link is set to NULL).
/// Later changes to it must stay additive.
pub(crate) fn ensure_usage(conn: &mut Connection) -> Result<(), rusqlite::Error> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(SCHEMA_USAGE)?;
    tx.commit()
}

/// Creates or upgrades the schema up to `target`, every step in one
/// transaction: a fresh database gets every version together, an older one only
/// the additions. A schema newer than `target` is refused.
///
/// The v3 rebuild drops tables other tables point at, so foreign keys are off
/// for the whole migration (SQLite ignores that pragma inside a transaction)
/// and put back as they were afterwards, whatever the outcome. If putting them
/// back fails after a committed migration, the open still succeeds: that is a
/// fact about this connection only, and the store is correct on disk.
///
/// Returns how many orphan rows (messages or tags of a conversation that no
/// longer exists) the v3 rebuild dropped. After a successful rebuild of an
/// existing store the file is vacuumed, best effort: the rebuild leaves the old
/// pages on the freelist and the file would stay twice its size.
fn migrate_to(
    conn: &mut Connection,
    target: i64,
    before_lock: impl FnOnce(),
) -> Result<usize, StoreError> {
    let keys: bool = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
    if keys {
        conn.pragma_update(None, "foreign_keys", false)?;
    }
    let migrated = migrate_locked(conn, target, before_lock);
    if keys {
        let _ = conn.pragma_update(None, "foreign_keys", true);
    }
    let (dropped, rebuilt) = migrated?;
    if rebuilt {
        vacuum(conn);
    }
    Ok(dropped)
}

/// Gives the freed pages back to the file. The scratch copy SQLite builds
/// stays in memory, so no chat text lands in a temporary file. A failure is
/// ignored: the store is fine, only bigger.
fn vacuum(conn: &Connection) {
    let _ = conn.pragma_update(None, "temp_store", "MEMORY");
    let _ = conn.execute_batch("VACUUM");
    let _ = conn.pragma_update(None, "temp_store", "DEFAULT");
}

/// The migration proper: the orphans it dropped and whether it rebuilt an
/// existing store.
fn migrate_locked(
    conn: &mut Connection,
    target: i64,
    before_lock: impl FnOnce(),
) -> Result<(usize, bool), StoreError> {
    before_lock();
    // The version is read under the write lock, so two instances on a fresh
    // store cannot both decide to create the schema.
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let version = user_version(&tx)?;
    if version > target {
        return Err(StoreError::Newer(version));
    }
    if version < 1 {
        tx.execute_batch(SCHEMA_V1)?;
    }
    if version < 2 && target >= 2 {
        tx.execute_batch(SCHEMA_V2)?;
    }
    if target >= 2 {
        ensure_folder(&tx)?;
    }
    if target >= 3 {
        runs::ensure(&tx)?;
    }
    let mut dropped = 0;
    let rebuilt = (1..3).contains(&version) && target >= 3;
    if version < 3 && target >= 3 {
        // A row whose conversation is gone can never be listed or resumed, and
        // would fail the check below, locking the whole history out. It is
        // dropped before the copy and counted. Only the rebuilt tables are
        // checked: the others are not touched by this step.
        for table in ["messages", "tags"] {
            dropped += tx.execute(
                &format!(
                    "DELETE FROM {table} WHERE conversation_id NOT IN (SELECT id FROM conversations)"
                ),
                [],
            )?;
        }
        tx.execute_batch(SCHEMA_V3)?;
        for table in ["messages", "tags"] {
            let broken: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check(?1))",
                [table],
                |r| r.get(0),
            )?;
            if broken {
                return Err(StoreError::Sqlite("foreign key check failed"));
            }
        }
    }
    if version < target {
        tx.pragma_update(None, "user_version", target)?;
    }
    tx.commit()?;
    Ok((dropped, rebuilt))
}

#[cfg(test)]
mod tests;
