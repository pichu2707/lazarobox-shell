//! A private folder that lives for one run and is removed after it: the
//! `TMPDIR` of the toolchain (read-write, never executable) and the `GOTMPDIR`
//! under the project root (`go run` executes what it builds there).

use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static RUNS: AtomicU64 = AtomicU64::new(0);

/// The folder is removed when this is dropped: the run ended, was cancelled or
/// failed to start.
#[derive(Debug)]
pub struct Scratch(PathBuf);

impl Scratch {
    /// A new private folder in `parent`, which is made (private too) when it
    /// does not exist. The name is `prefix`, the pid and a counter.
    pub fn unique(parent: &Path, prefix: &str) -> io::Result<Scratch> {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)?;
        // SAFETY: getpid has no preconditions and cannot fail. (`std::process`
        // is off limits to this file: it may write, not start a process.)
        let pid = unsafe { libc::getpid() };
        let name = format!("{prefix}-{pid}-{}", RUNS.fetch_add(1, Ordering::Relaxed));
        Scratch::exact(&fs::canonicalize(parent)?.join(name))
    }

    /// The folder `path`, which must not exist and whose parent must.
    pub fn exact(path: &Path) -> io::Result<Scratch> {
        DirBuilder::new().mode(0o700).create(path)?;
        Ok(Scratch(fs::canonicalize(path)?))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // A program that swapped the folder for a link loses only the link:
        // `remove_dir_all` does not follow it.
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests;
