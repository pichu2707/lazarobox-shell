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

use rusqlite::{Connection, OpenFlags};

/// The schema version this build writes and understands.
const SCHEMA_VERSION: i64 = 1;

/// How long a write waits for another instance before it gives up.
const BUSY_TIMEOUT: Duration = Duration::from_millis(2000);

/// Longest SQLite or OS message kept in an error.
const MAX_DETAIL: usize = 60;

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
    Io(String),
    Sqlite(String),
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

impl From<io::Error> for StoreError {
    fn from(err: io::Error) -> Self {
        Self::Io(short(&err.kind().to_string()))
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(err: rusqlite::Error) -> Self {
        let text = match &err {
            rusqlite::Error::SqliteFailure(code, msg) => {
                msg.clone().unwrap_or_else(|| code.to_string())
            }
            other => other.to_string(),
        };
        Self::Sqlite(short(&text))
    }
}

/// One line, no control characters, at most [`MAX_DETAIL`] characters.
fn short(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() || c == '/' { ' ' } else { c })
        .take(MAX_DETAIL)
        .collect::<String>()
        .trim()
        .to_owned()
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
    if let Some(dir) = path.parent() {
        prepare_dir(dir, uid)?;
    }
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
    conn.pragma_update(None, "trusted_schema", false)?;
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.pragma_update(None, "secure_delete", true)?;
    // The rollback journal goes away after each transaction, so no file keeps
    // deleted text around.
    let mode: String =
        conn.pragma_update_and_check(None, "journal_mode", "delete", |r| r.get(0))?;
    if !mode.eq_ignore_ascii_case("delete") {
        return Err(StoreError::Sqlite(short("journal mode not available")));
    }
    migrate(&mut conn)?;
    harden_side_files(path, uid)?;
    Ok(conn)
}

/// Creates the data directory 0700, or checks and corrects an existing one.
fn prepare_dir(dir: &Path, uid: u32) -> Result<(), StoreError> {
    match fs::symlink_metadata(dir) {
        Ok(meta) => {
            if !meta.is_dir() {
                return Err(StoreError::Unsafe("data directory is not a directory"));
            }
            if meta.uid() != uid {
                return Err(StoreError::Unsafe("data directory has a foreign owner"));
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

/// Creates schema v1 on a new database; a newer schema is refused.
fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(StoreError::Newer(version));
    }
    if version < 1 {
        let tx = conn.transaction()?;
        tx.execute_batch(SCHEMA_V1)?;
        tx.pragma_update(None, "user_version", 1)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
