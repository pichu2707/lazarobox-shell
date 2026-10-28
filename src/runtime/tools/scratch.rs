//! A private folder that lives for one run and is removed after it: the
//! `TMPDIR` of the toolchain (read-write, never executable) and the `GOTMPDIR`
//! under the project root (`go run` executes what it builds there).

use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static RUNS: AtomicU64 = AtomicU64::new(0);

/// How many names [`Scratch::unique`] tries before it gives up.
const TRIES: u32 = 16;

/// The folder is removed when this is dropped: the run ended, was cancelled or
/// failed to start. [`Scratch::finish`] removes it and says what was left.
#[derive(Debug)]
pub struct Scratch(PathBuf);

impl Scratch {
    /// A new private folder in `parent`, which is made (private too) when it
    /// does not exist. The name is `prefix`, the pid, a counter and the clock,
    /// so a program in the project cannot plant it in advance; a name that is
    /// taken anyway is skipped, up to [`TRIES`] times.
    pub fn unique(parent: &Path, prefix: &str) -> io::Result<Scratch> {
        // SAFETY: getpid has no preconditions and cannot fail. (`std::process`
        // is off limits to this file: it may write, not start a process.)
        let pid = unsafe { libc::getpid() };
        Scratch::unique_with(parent, |_| {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let count = RUNS.fetch_add(1, Ordering::Relaxed);
            format!("{prefix}-{pid}-{count}-{nanos:x}")
        })
    }

    /// [`Scratch::unique`] with the names given by `name(attempt)`: an
    /// `AlreadyExists` moves on to the next attempt, any other error stops.
    fn unique_with(parent: &Path, mut name: impl FnMut(u32) -> String) -> io::Result<Scratch> {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)?;
        let parent = fs::canonicalize(parent)?;
        let mut taken = io::Error::from(io::ErrorKind::AlreadyExists);
        for attempt in 0..TRIES {
            match Scratch::exact(&parent.join(name(attempt))) {
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => taken = e,
                other => return other,
            }
        }
        Err(taken)
    }

    /// The folder `path`, which must not exist and whose parent must.
    pub fn exact(path: &Path) -> io::Result<Scratch> {
        DirBuilder::new().mode(0o700).create(path)?;
        Ok(Scratch(fs::canonicalize(path)?))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Removes the folder now and returns what is still there (the folder's
    /// own path when it could not go), so the caller can say so.
    pub fn finish(self) -> Vec<PathBuf> {
        remove(&self.0)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        remove(&self.0);
    }
}

/// Removes `path` and everything below it. A program leaves read-only folders
/// behind (a module cache is one), which `remove_dir_all` cannot empty: the
/// tree's folders (never a link) get `0700` and the removal is tried once
/// more. Returns `[path]` when something is still there.
fn remove(path: &Path) -> Vec<PathBuf> {
    // A program that swapped the folder for a link loses only the link:
    // `remove_dir_all` does not follow it.
    if fs::remove_dir_all(path).is_ok() {
        return Vec::new();
    }
    make_removable(path);
    match fs::remove_dir_all(path) {
        Ok(()) => Vec::new(),
        Err(_) if fs::symlink_metadata(path).is_err() => Vec::new(),
        Err(_) => vec![path.to_path_buf()],
    }
}

/// How deep and how many entries [`make_removable`] goes: a path cannot be
/// deeper than `PATH_MAX` allows anyway, and a program must not be able to
/// make this walk last.
const WALK_DEPTH: usize = 4096;
const WALK_ENTRIES: usize = 1_000_000;

/// `0700` on every folder under `path` and on `path` itself, with
/// `symlink_metadata`: a link is never entered and never changed.
fn make_removable(path: &Path) {
    walk(path, WALK_DEPTH, WALK_ENTRIES);
}

/// [`make_removable`] with its bounds. An explicit stack, no recursion: a
/// program decides how deep the tree is.
fn walk(path: &Path, max_depth: usize, max_entries: usize) {
    let mut stack = vec![(path.to_path_buf(), 0usize)];
    let mut seen = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(meta) = fs::symlink_metadata(&dir) else {
            continue;
        };
        if !meta.file_type().is_dir() {
            continue;
        }
        let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
        if depth >= max_depth {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > max_entries {
                return;
            }
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                stack.push((entry.path(), depth + 1));
            }
        }
    }
}

#[cfg(test)]
mod tests;
