//! The environment of a toolchain run: cleared, then rebuilt from a short
//! list. Pure: `read` stands in for the parent's environment. Nothing outside
//! the lists below gets through, so the session token, `SSH_AUTH_SOCK`,
//! `GPG_*`, the display and bus sockets and every `*_TOKEN` or `AWS_*` stay
//! out without being named.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Copied from the parent when set.
const PASSED: [&str; 10] = [
    "HOME",
    "USER",
    "LANG",
    "LC_ALL",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "GOPATH",
    "GOROOT",
    "GOCACHE",
    "GOMODCACHE",
];

/// Set to these values whatever the parent holds (`TMPDIR` is the run's own).
const FIXED: [(&str, &str); 7] = [
    ("TERM", "dumb"),
    ("CI", "1"),
    ("NO_COLOR", "1"),
    ("CARGO_TERM_COLOR", "never"),
    ("CARGO_TERM_PROGRESS_WHEN", "never"),
    ("PYTHONUNBUFFERED", "1"),
    ("GIT_TERMINAL_PROMPT", "0"),
];

/// The environment for the child, in a fixed order. `PATH` keeps only the
/// absolute entries outside `root`: a relative entry follows the child's cwd,
/// and a binary the project holds is one the project could have written.
pub fn scrub(
    read: &dyn Fn(&str) -> Option<OsString>,
    root: &Path,
    tmp: &Path,
) -> Vec<(OsString, OsString)> {
    let mut env = Vec::new();
    if let Some(path) = read("PATH").and_then(|raw| safe_path(&raw, root)) {
        env.push((OsString::from("PATH"), path));
    }
    for name in PASSED {
        if let Some(value) = read(name) {
            env.push((OsString::from(name), value));
        }
    }
    env.push((OsString::from("TMPDIR"), tmp.as_os_str().to_owned()));
    env.extend(FIXED.map(|(name, value)| (OsString::from(name), OsString::from(value))));
    env
}

/// `raw` without its relative and project entries; `None` when none is left.
fn safe_path(raw: &OsString, root: &Path) -> Option<OsString> {
    let kept: Vec<PathBuf> = std::env::split_paths(raw)
        .filter(|entry| entry.is_absolute() && !entry.starts_with(root))
        .collect();
    if kept.is_empty() {
        return None;
    }
    std::env::join_paths(kept).ok()
}

#[cfg(test)]
mod tests;
