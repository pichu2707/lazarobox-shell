//! The read tools: `read_file`, `list_dir` and `find_dir`. Read only: no
//! process, no socket, no write, and no path that the policy refuses.
//!
//! Known limit: a regular file with more than one hard link is refused by
//! `read_file`, because a hard link to a secret has an ordinary path. Files
//! that are legitimately hard-linked (pnpm and nix stores, git objects) are
//! refused with it. Listings still show their names.

use std::{
    collections::VecDeque,
    ffi::OsStr,
    ffi::OsString,
    fs::FileType,
    io::Read,
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::fs::MetadataExt,
    },
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use crate::{
    core::{
        ai::{
            memory::redact::redact,
            sanitize::clean,
            tools::{
                Call,
                paths::{PathVerdict, REFUSED, SecretRoots, is_excluded_name, resolve},
            },
        },
        config::config_path,
    },
    runtime::{
        chat_store::resolve_path,
        tools::open::{fd_path, open_beneath},
    },
};

/// The caps pinned by the design.
#[derive(Clone, Debug)]
pub struct Limits {
    /// `read_file`: bytes of text.
    pub read_bytes: usize,
    /// `list_dir`: entries returned.
    pub list_entries: usize,
    /// `find_dir`: folder levels below HOME.
    pub depth: usize,
    /// `find_dir`: entries looked at (also bounds one `list_dir` scan).
    pub visited: usize,
    /// `find_dir`: wall time.
    pub time: Duration,
    /// `find_dir`: paths returned.
    pub results: usize,
    /// A test seam that makes every looked-at entry cost this long, as a slow
    /// filesystem would.
    #[cfg(test)]
    pub(crate) entry_delay: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            read_bytes: 64 * 1024,
            list_entries: 200,
            depth: 8,
            visited: 20_000,
            time: Duration::from_secs(2),
            results: 20,
            #[cfg(test)]
            entry_delay: Duration::ZERO,
        }
    }
}

/// What a call may see: the environment values it needs, read by the caller.
#[derive(Clone, Debug)]
pub struct ToolEnv {
    pub home: Option<OsString>,
    pub xdg_config_home: Option<OsString>,
    pub xdg_data_home: Option<OsString>,
    /// The chat's origin cwd, against which a relative path is read.
    pub origin: PathBuf,
}

/// Why a call failed. The text is short and never holds file content or a
/// path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Failure {
    Refused,
    NotText,
    Unreadable,
    TimedOut,
    EmptyQuery,
    NotReadTool,
}

impl Failure {
    /// The text that goes back to the model.
    pub fn message(self) -> &'static str {
        match self {
            Failure::Refused => REFUSED,
            Failure::NotText => "the file is not text",
            Failure::Unreadable => "could not read",
            Failure::TimedOut => "timed out",
            Failure::EmptyQuery => "the query is empty",
            Failure::NotReadTool => "not a read tool",
        }
    }
}

/// Bytes read past the cap so redaction sees a whole secret.
const REDACT_MARGIN: usize = 1024;

/// Appended when `list_dir` cut the list.
pub const LIST_MARKER: &str = "[truncated: more entries than the list cap]";

/// Appended when `find_dir` hit a depth, visit, time or result bound.
pub const FIND_MARKER: &str = "[truncated: the search was bounded]";

/// Appended when `read_file` cut the text at the cap.
pub const READ_MARKER: &str = "[truncated: the file is larger than the read cap]";

/// When a walk must stop: its wall-time deadline or the caller's cancel flag.
/// Checked per entry and before each `canonicalize`, so a cancelled or late
/// call stops touching the disk. A syscall already blocked (a hung open on a
/// dead mount) cannot be interrupted and can still pin one blocking thread.
struct Budget<'a> {
    deadline: Instant,
    cancel: &'a AtomicBool,
    #[cfg(test)]
    entry_delay: Duration,
}

impl Budget<'_> {
    fn spent(&self) -> bool {
        self.cancel.load(Ordering::Relaxed) || Instant::now() >= self.deadline
    }
}

#[cfg(test)]
thread_local! {
    /// Folders `find_dir` opened on this thread, so a test can see a spent
    /// budget stop the walk.
    static OPENS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// What one folder open costs on this thread, as a slow mount would.
    static OPEN_DELAY: std::cell::Cell<Duration> = const { std::cell::Cell::new(Duration::ZERO) };
}

/// Runs one read tool, synchronously. Builds the secret roots ONCE for the
/// whole call and refuses the call when they cannot be built.
pub fn execute(call: &Call, env: &ToolEnv, limits: &Limits) -> Result<String, Failure> {
    execute_with(call, env, limits, &AtomicBool::new(false))
}

/// [`execute`] that stops early when `cancel` is set or `limits.time` passes.
pub fn execute_with(
    call: &Call,
    env: &ToolEnv,
    limits: &Limits,
    cancel: &AtomicBool,
) -> Result<String, Failure> {
    let roots = load_roots(env)?;
    let budget = Budget {
        deadline: Instant::now() + limits.time,
        cancel,
        #[cfg(test)]
        entry_delay: limits.entry_delay,
    };
    match call {
        Call::ReadFile { path } => read_file(&roots, env, path, limits),
        Call::ListDir { path } => list_dir(&roots, &budget, env, path, limits),
        Call::FindDir { query } => find_dir(&roots, &budget, query, limits),
        Call::RunInPane { .. } => Err(Failure::NotReadTool),
    }
}

/// The secret roots for one call. HOME unset, empty or relative refuses, and
/// so does a root that cannot be read; there is no fallback to `/`.
fn load_roots(env: &ToolEnv) -> Result<SecretRoots, Failure> {
    let home = env
        .home
        .as_ref()
        .filter(|home| !home.is_empty())
        .ok_or(Failure::Refused)?;
    let parent = |path: Option<PathBuf>| path.and_then(|p| p.parent().map(Path::to_path_buf));
    let extra: Vec<PathBuf> = [
        parent(config_path(env.xdg_config_home.clone(), Some(home.clone()))),
        parent(resolve_path(
            env.xdg_data_home.as_deref(),
            Some(home.as_os_str()),
        )),
    ]
    .into_iter()
    .flatten()
    .collect();
    SecretRoots::load(Path::new(home), &extra).ok_or(Failure::Refused)
}

/// Opens the already resolved `real` path and proves the descriptor is that
/// path and still allowed. No symlink is followed in any component, so a link
/// swapped in after the judgment fails the open; the descriptor's own path is
/// read back and judged against the same snapshot of roots.
pub fn open_checked(roots: &SecretRoots, real: &Path, dir: bool) -> Result<OwnedFd, Failure> {
    if !roots.judge_real(real) {
        return Err(Failure::Refused);
    }
    let rel = real
        .strip_prefix(roots.home())
        .map_err(|_| Failure::Refused)?;
    let fd = open_beneath(roots.home(), rel, dir).map_err(|error| match error.kind() {
        // No safe open on this platform: the tool cannot work at all.
        std::io::ErrorKind::Unsupported => Failure::Unreadable,
        _ => Failure::Refused,
    })?;
    match fd_path(&fd) {
        Ok(opened) if opened == real && roots.judge_real(&opened) => Ok(fd),
        _ => Err(Failure::Refused),
    }
}

fn read_file(
    roots: &SecretRoots,
    env: &ToolEnv,
    path: &str,
    limits: &Limits,
) -> Result<String, Failure> {
    let PathVerdict::Allowed(real) = resolve(roots, &env.origin, path) else {
        return Err(Failure::Refused);
    };
    let file = std::fs::File::from(open_checked(roots, &real, false)?);
    let meta = file.metadata().map_err(|_| Failure::Unreadable)?;
    // A directory, FIFO, socket or device: refused like any hidden path.
    if !meta.is_file() {
        return Err(Failure::Refused);
    }
    // A hard link to a secret has an ordinary path. Files that really are
    // hard-linked (pnpm and nix stores, git objects) are refused too.
    if meta.nlink() > 1 {
        return Err(Failure::Refused);
    }
    // Read a margin past the cap: a secret that straddles the cap must be
    // redacted whole, and only then is the text cut.
    let window = limits.read_bytes + REDACT_MARGIN;
    let mut bytes = Vec::new();
    file.take(window as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Failure::Unreadable)?;
    if bytes.contains(&0) {
        return Err(Failure::NotText);
    }
    let more = bytes.len() > window;
    bytes.truncate(window);
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        // A character split by the window is the only decoding error allowed.
        Err(error) if more && error.utf8_error().error_len().is_none() => {
            let valid = error.utf8_error().valid_up_to();
            let mut bytes = error.into_bytes();
            bytes.truncate(valid);
            String::from_utf8(bytes).map_err(|_| Failure::NotText)?
        }
        Err(_) => return Err(Failure::NotText),
    };
    // Clean first: a zero-width or control character inside a token would
    // otherwise hide it from the redaction.
    let mut out = redact(&clean(&text)).text;
    // The window edge may have cut a token in two, and redaction cannot see
    // a secret it only half has: drop the trailing partial token, even when
    // redaction left the text under the cap.
    if more {
        out.truncate(out.rfind(char::is_whitespace).unwrap_or(0));
    }
    let cut = more || out.len() > limits.read_bytes;
    if out.len() > limits.read_bytes {
        let mut end = limits.read_bytes;
        while !out.is_char_boundary(end) {
            end -= 1;
        }
        out.truncate(end);
    }
    if cut {
        out.push('\n');
        out.push_str(READ_MARKER);
    }
    Ok(out)
}

/// The entries of an open directory, at most `max` of them, and whether more
/// were left. Read through `/proc/self/fd`, so it is the opened directory and
/// not a path that is listed. A symlink is reported as such, never followed.
fn entries(
    fd: &OwnedFd,
    max: usize,
    budget: &Budget,
) -> Result<(Vec<(OsString, FileType)>, bool), Failure> {
    let dir = std::fs::read_dir(format!("/proc/self/fd/{}", fd.as_raw_fd()))
        .map_err(|_| Failure::Unreadable)?;
    let mut out = Vec::new();
    for entry in dir {
        if out.len() == max || budget.spent() {
            return Ok((out, true));
        }
        #[cfg(test)]
        std::thread::sleep(budget.entry_delay);
        let entry = entry.map_err(|_| Failure::Unreadable)?;
        out.push((
            entry.file_name(),
            entry.file_type().map_err(|_| Failure::Unreadable)?,
        ));
    }
    Ok((out, false))
}

/// Whether `dir/name` may be shown. A link is judged by where it leads, and a
/// dangling one is hidden.
fn visible(roots: &SecretRoots, budget: &Budget, dir: &Path, name: &OsStr, kind: FileType) -> bool {
    if is_excluded_name(&name.to_string_lossy()) {
        return false;
    }
    let full = dir.join(name);
    if kind.is_symlink() {
        !budget.spent()
            && full
                .canonicalize()
                .is_ok_and(|real| roots.judge_real(&real))
    } else {
        roots.judge_real(&full)
    }
}

/// Whether redaction would change the name. Such a name is skipped: neither
/// listed nor matched, and a folder with one is not entered. Showing it as
/// `[REDACTED]`, or matching a query against the raw name, would tell the
/// model which prefixes of a key exist.
fn redaction_changes(name: &OsStr) -> bool {
    let line = one_line(name);
    redact(&line).text != line
}

/// A name that fits on one line: escapes and bidi marks are gone, and so is
/// every remaining control character.
fn one_line(name: &OsStr) -> String {
    clean(&name.to_string_lossy())
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

fn list_dir(
    roots: &SecretRoots,
    budget: &Budget,
    env: &ToolEnv,
    path: &str,
    limits: &Limits,
) -> Result<String, Failure> {
    let PathVerdict::Allowed(real) = resolve(roots, &env.origin, path) else {
        return Err(Failure::Refused);
    };
    let fd = open_checked(roots, &real, true)?;
    let (found, scan_cut) = entries(&fd, limits.visited, budget)?;
    let mut shown: Vec<(String, char)> = found
        .iter()
        .filter(|(name, kind)| {
            visible(roots, budget, &real, name, *kind) && !redaction_changes(name)
        })
        .map(|(name, kind)| {
            let tag = if kind.is_dir() {
                'd'
            } else if kind.is_file() {
                'f'
            } else if kind.is_symlink() {
                'l'
            } else {
                '?'
            };
            (one_line(name), tag)
        })
        .collect();
    shown.sort();
    let cut = scan_cut || shown.len() > limits.list_entries;
    shown.truncate(limits.list_entries);
    let mut lines: Vec<String> = shown
        .into_iter()
        .map(|(name, tag)| format!("{tag} {name}"))
        .collect();
    if lines.is_empty() && !cut {
        lines.push("(empty)".into());
    }
    if cut {
        lines.push(LIST_MARKER.into());
    }
    Ok(lines.join("\n"))
}

/// Breadth-first search below HOME for folders whose name contains `query`.
/// Each folder is opened without following a link and judged again; a link is
/// never entered; excluded folders are skipped, never entered. The bounds are
/// depth, entries looked at, wall time and results.
fn find_dir(
    roots: &SecretRoots,
    budget: &Budget,
    query: &str,
    limits: &Limits,
) -> Result<String, Failure> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Err(Failure::EmptyQuery);
    }
    let mut queue = VecDeque::from([PathBuf::new()]);
    let (mut visited, mut cut) = (0usize, false);
    let mut found: Vec<String> = Vec::new();
    'search: while let Some(rel) = queue.pop_front() {
        // A spent budget stops the walk before the next open, even when the
        // folders left in the queue have no entries to trip the per-entry check.
        if budget.spent() {
            cut = true;
            break;
        }
        let level = rel.components().count();
        let dir = roots.home().join(&rel);
        #[cfg(test)]
        {
            OPENS.with(|c| c.set(c.get() + 1));
            std::thread::sleep(OPEN_DELAY.with(|c| c.get()));
        }
        let Ok(fd) = open_checked(roots, &dir, true) else {
            continue;
        };
        let Ok((mut children, more)) = entries(&fd, limits.visited, budget) else {
            continue;
        };
        cut |= more;
        children.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, kind) in children {
            visited += 1;
            if budget.spent() || visited > limits.visited {
                cut = true;
                break 'search;
            }
            if !kind.is_dir()
                || !visible(roots, budget, &dir, &name, kind)
                || redaction_changes(&name)
            {
                continue;
            }
            if level >= limits.depth {
                // Frontier: nothing here is reported, only noted as unexplored.
                cut = true;
                break;
            }
            let child = rel.join(&name);
            if name.to_string_lossy().to_lowercase().contains(&needle) {
                found.push(format!("~/{}", one_line(child.as_os_str())));
                if found.len() == limits.results {
                    cut = true;
                    break 'search;
                }
            }
            queue.push_back(child);
        }
    }
    let mut out = if found.is_empty() {
        "no match".to_owned()
    } else {
        found.join("\n")
    };
    if cut {
        out.push('\n');
        out.push_str(FIND_MARKER);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};

    pub(super) struct Fixture {
        pub dir: tempfile::TempDir,
        pub home: PathBuf,
    }

    pub(super) fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let home = root.join("home");
        for sub in ["home/dev/app", "home/.ssh", "home/outside-link", "outside"] {
            fs::create_dir_all(root.join(sub)).unwrap();
        }
        fs::remove_dir(root.join("home/outside-link")).unwrap();
        fs::write(home.join("dev/app/main.rs"), "fn main() {}\n").unwrap();
        fs::write(home.join(".ssh/id"), "SECRET-TOKEN-123").unwrap();
        fs::write(root.join("outside/note.txt"), "out").unwrap();
        Fixture { dir, home }
    }

    pub(super) fn env(home: &Path) -> ToolEnv {
        ToolEnv {
            home: Some(home.into()),
            xdg_config_home: None,
            xdg_data_home: None,
            origin: home.to_path_buf(),
        }
    }

    fn read(env: &ToolEnv, path: &str) -> Result<String, Failure> {
        execute(
            &Call::ReadFile { path: path.into() },
            env,
            &Limits::default(),
        )
    }

    #[test]
    fn reads_a_text_file_by_tilde_absolute_and_relative_path() {
        let f = fixture();
        let mut e = env(&f.home);
        let abs = f.home.join("dev/app/main.rs");
        assert_eq!(read(&e, "~/dev/app/main.rs").unwrap(), "fn main() {}\n");
        assert_eq!(read(&e, abs.to_str().unwrap()).unwrap(), "fn main() {}\n");
        e.origin = f.home.join("dev");
        assert_eq!(read(&e, "app/main.rs").unwrap(), "fn main() {}\n");
    }

    #[test]
    fn home_unset_empty_or_relative_refuses_every_call_and_never_uses_root() {
        let f = fixture();
        for home in [
            None,
            Some(OsString::new()),
            Some("relative/home".into()),
            Some("/".into()),
        ] {
            let mut e = env(&f.home);
            e.home = home;
            e.origin = f.home.clone();
            for call in [
                Call::ReadFile {
                    path: "~/dev/app/main.rs".into(),
                },
                Call::ReadFile {
                    path: "/etc/hostname".into(),
                },
                Call::ListDir { path: "~".into() },
                Call::FindDir {
                    query: "app".into(),
                },
            ] {
                assert_eq!(
                    execute(&call, &e, &Limits::default()),
                    Err(Failure::Refused),
                    "{:?} {call:?}",
                    e.home
                );
            }
        }
    }

    #[test]
    fn an_unreadable_secret_root_refuses_the_whole_call() {
        let f = fixture();
        fs::remove_dir_all(f.home.join(".ssh")).unwrap();
        let looped = f.home.join(".ssh");
        symlink(&looped, &looped).unwrap();
        assert_eq!(
            read(&env(&f.home), "~/dev/app/main.rs"),
            Err(Failure::Refused)
        );
    }

    #[test]
    fn the_lazarobox_config_and_data_dirs_are_extra_roots_even_when_moved_by_xdg() {
        let f = fixture();
        let data = f.home.join("custom-data/lazarobox");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("chat.db"), "rows").unwrap();
        let config = f.home.join("custom-config/lazarobox");
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("config.toml"), "token").unwrap();
        let mut e = env(&f.home);
        assert!(read(&e, "~/custom-data/lazarobox/chat.db").is_ok());
        e.xdg_data_home = Some(f.home.join("custom-data").into());
        e.xdg_config_home = Some(f.home.join("custom-config").into());
        assert_eq!(
            read(&e, "~/custom-data/lazarobox/chat.db"),
            Err(Failure::Refused)
        );
        assert_eq!(
            read(&e, "~/custom-config/lazarobox/config.toml"),
            Err(Failure::Refused)
        );
    }

    #[test]
    fn excluded_outside_and_missing_paths_give_the_same_refusal() {
        let f = fixture();
        symlink(f.home.join("../outside/note.txt"), f.home.join("link")).unwrap();
        let e = env(&f.home);
        for path in [
            "~/.ssh/id",
            "../outside/note.txt",
            "~/link",
            "~/nope",
            "~root/x",
        ] {
            let err = read(&e, path).unwrap_err();
            assert_eq!(err, Failure::Refused, "{path}");
            assert_eq!(err.message(), REFUSED);
        }
    }

    #[test]
    fn nul_bytes_and_invalid_utf8_are_not_text_and_the_error_holds_no_content() {
        let f = fixture();
        fs::write(f.home.join("bin"), b"SECRET-TOKEN-123\0more").unwrap();
        fs::write(f.home.join("latin"), b"SECRET-TOKEN-123 \xe9 caf\xe9").unwrap();
        let e = env(&f.home);
        for path in ["~/bin", "~/latin"] {
            let err = read(&e, path).unwrap_err();
            assert_eq!(err, Failure::NotText, "{path}");
            assert!(!err.message().contains("SECRET-TOKEN-123"));
        }
    }

    #[test]
    fn a_big_file_is_cut_at_the_cap_on_a_char_boundary_and_says_so() {
        let f = fixture();
        let limits = Limits {
            read_bytes: 9,
            ..Limits::default()
        };
        fs::write(f.home.join("big"), "aaaaaaaaé-tail").unwrap();
        let out = execute(
            &Call::ReadFile {
                path: "~/big".into(),
            },
            &env(&f.home),
            &limits,
        )
        .unwrap();
        assert_eq!(out, format!("aaaaaaaa\n{READ_MARKER}"));
        fs::write(f.home.join("exact"), "012345678").unwrap();
        let out = execute(
            &Call::ReadFile {
                path: "~/exact".into(),
            },
            &env(&f.home),
            &limits,
        )
        .unwrap();
        assert_eq!(out, "012345678");
    }

    #[test]
    fn recognizable_secrets_are_redacted() {
        let f = fixture();
        fs::write(f.home.join("conf"), "key = sk-abcDEF1234567890\nname = x\n").unwrap();
        let out = read(&env(&f.home), "~/conf").unwrap();
        assert!(!out.contains("sk-abcDEF1234567890"), "{out}");
        assert!(out.contains("name = x"));
    }

    #[test]
    fn a_hard_link_to_a_secret_is_refused() {
        let f = fixture();
        fs::hard_link(f.home.join(".ssh/id"), f.home.join("innocent.txt")).unwrap();
        assert_eq!(read(&env(&f.home), "~/innocent.txt"), Err(Failure::Refused));
        // The same file with one link is fine.
        fs::write(f.home.join("solo.txt"), "ok").unwrap();
        assert_eq!(read(&env(&f.home), "~/solo.txt").unwrap(), "ok");
    }

    #[test]
    fn a_fifo_a_directory_and_a_device_are_not_read_and_never_block() {
        let f = fixture();
        let fifo = std::ffi::CString::new(f.home.join("pipe").to_str().unwrap()).unwrap();
        // SAFETY: a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let e = env(&f.home);
        let _socket = std::os::unix::net::UnixListener::bind(f.home.join("sock")).unwrap();
        for path in ["~/pipe", "~/dev", "~/sock"] {
            assert_eq!(read(&e, path), Err(Failure::Refused), "{path}");
        }
    }

    #[test]
    fn a_token_split_by_a_zero_width_char_is_redacted() {
        let f = fixture();
        fs::write(f.home.join("conf"), "k = sk-abcDEF\u{200b}1234567890\n").unwrap();
        let out = read(&env(&f.home), "~/conf").unwrap();
        assert!(!out.contains("1234567890"), "{out:?}");
        assert!(out.contains("[REDACTED]"), "{out:?}");
    }

    #[test]
    fn a_secret_straddling_the_cap_is_redacted_whole_before_the_cut() {
        let f = fixture();
        fs::write(f.home.join("conf"), "xxxxxxxxx sk-abcDEF1234567890 tail").unwrap();
        let limits = Limits {
            read_bytes: 20,
            ..Limits::default()
        };
        let out = execute(
            &Call::ReadFile {
                path: "~/conf".into(),
            },
            &env(&f.home),
            &limits,
        )
        .unwrap();
        assert_eq!(out, format!("xxxxxxxxx [REDACTED]\n{READ_MARKER}"));
    }

    #[test]
    fn a_secret_cut_by_the_window_edge_never_survives_even_when_redaction_shrank_the_text() {
        let f = fixture();
        let secret = format!("sk-{}", "a1B2".repeat(50));
        let mut text = String::new();
        for _ in 0..9 {
            text.push_str(&secret);
            text.push(' ');
        }
        // The window is read_bytes + REDACT_MARGIN = 2024 bytes: the last
        // secret starts at 2016 and is cut by the edge.
        text.push_str(&"p".repeat(2016 - text.len() - 1));
        text.push(' ');
        assert_eq!(text.len(), 2016);
        text.push_str("sk-QWERTYUIOPASDF1234567890 tail");
        fs::write(f.home.join("conf"), &text).unwrap();
        let limits = Limits {
            read_bytes: 1000,
            ..Limits::default()
        };
        let out = execute(
            &Call::ReadFile {
                path: "~/conf".into(),
            },
            &env(&f.home),
            &limits,
        )
        .unwrap();
        assert!(!out.contains("sk-"), "{out:?}");
        assert!(out.ends_with(READ_MARKER), "{out:?}");
        assert!(out.contains("[REDACTED]"), "{out:?}");
    }

    #[test]
    fn a_link_swapped_in_after_the_judgment_is_not_followed() {
        let f = fixture();
        let roots = SecretRoots::load(&f.home, &[]).unwrap();
        let real = f.home.join("dev/app/main.rs");
        assert!(open_checked(&roots, &real, false).is_ok());
        // The file becomes a link to a secret.
        fs::remove_file(&real).unwrap();
        symlink(f.home.join(".ssh/id"), &real).unwrap();
        assert_eq!(
            open_checked(&roots, &real, false).unwrap_err(),
            Failure::Refused
        );
        // A parent folder becomes a link to another folder.
        fs::remove_file(&real).unwrap();
        fs::rename(f.home.join("dev/app"), f.home.join("dev/app-moved")).unwrap();
        symlink(f.home.join("../outside"), f.home.join("dev/app")).unwrap();
        assert_eq!(
            open_checked(&roots, &real, false).unwrap_err(),
            Failure::Refused
        );
    }

    #[test]
    fn the_opened_descriptor_is_judged_again_against_the_snapshot() {
        let f = fixture();
        let roots = SecretRoots::load(&f.home, &[]).unwrap();
        // Opens fine, but the policy refuses what it is.
        for secret in [f.home.join(".ssh/id"), f.home.join("../outside/note.txt")] {
            assert!(open_checked(&roots, &secret, false).is_err(), "{secret:?}");
        }
    }

    fn list(env: &ToolEnv, path: &str, limits: &Limits) -> Result<String, Failure> {
        execute(&Call::ListDir { path: path.into() }, env, limits)
    }

    fn find(env: &ToolEnv, query: &str, limits: &Limits) -> Result<String, Failure> {
        execute(
            &Call::FindDir {
                query: query.into(),
            },
            env,
            limits,
        )
    }

    #[test]
    fn list_dir_returns_sorted_entries_with_kinds_and_omits_excluded_ones() {
        let f = fixture();
        let dev = f.home.join("dev");
        fs::create_dir_all(dev.join("zeta")).unwrap();
        fs::write(dev.join("a.txt"), "x").unwrap();
        fs::write(dev.join(".env"), "x").unwrap();
        fs::write(dev.join(".NPMRC"), "x").unwrap();
        fs::create_dir_all(dev.join(".env.d")).unwrap();
        symlink(dev.join("a.txt"), dev.join("ok-link")).unwrap();
        symlink(f.home.join(".ssh/id"), dev.join("secret-link")).unwrap();
        symlink(f.home.join("nowhere"), dev.join("dangling")).unwrap();
        let out = list(&env(&f.home), "~/dev", &Limits::default()).unwrap();
        assert_eq!(out, "f a.txt\nd app\nl ok-link\nd zeta");
    }

    #[test]
    fn list_dir_of_home_hides_the_secret_folders() {
        let f = fixture();
        fs::create_dir_all(f.home.join(".claude")).unwrap();
        let out = list(&env(&f.home), "~", &Limits::default()).unwrap();
        assert_eq!(out, "d dev");
    }

    #[test]
    fn list_dir_cleans_control_and_bidi_names_and_cannot_forge_a_line() {
        let f = fixture();
        fs::write(f.home.join("a\nf fake\u{202e}b\x1b[31m"), "x").unwrap();
        let out = list(&env(&f.home), "~", &Limits::default()).unwrap();
        assert_eq!(out.lines().count(), 2, "{out:?}");
        assert!(out.chars().all(|c| !c.is_control() || c == '\n'), "{out:?}");
        assert!(!out.contains('\u{202e}'));
    }

    #[test]
    fn list_dir_stops_at_the_cap_and_says_so() {
        let f = fixture();
        for n in 0..5 {
            fs::write(f.home.join(format!("f{n}")), "x").unwrap();
        }
        let limits = Limits {
            list_entries: 3,
            ..Limits::default()
        };
        let out = list(&env(&f.home), "~", &limits).unwrap();
        assert_eq!(out.lines().count(), 4, "{out}");
        assert!(out.ends_with(LIST_MARKER), "{out}");
        let out = list(&env(&f.home), "~/dev", &limits).unwrap();
        assert!(!out.contains("truncated"), "{out}");
    }

    #[test]
    fn list_dir_refuses_excluded_outside_missing_and_file_targets() {
        let f = fixture();
        let e = env(&f.home);
        symlink(f.home.join("../outside"), f.home.join("dev/out")).unwrap();
        for path in [
            "~/.ssh",
            "~/dev/out",
            "..",
            "~/missing",
            "~/dev/app/main.rs",
        ] {
            assert_eq!(
                list(&e, path, &Limits::default()),
                Err(Failure::Refused),
                "{path}"
            );
        }
    }

    #[test]
    fn find_dir_finds_nested_folders_case_insensitively_as_tilde_paths() {
        let f = fixture();
        fs::create_dir_all(f.home.join("dev/Curso-Rust/src")).unwrap();
        fs::create_dir_all(f.home.join("old/deep/curso-rust-2")).unwrap();
        let out = find(&env(&f.home), "curso-rust", &Limits::default()).unwrap();
        assert_eq!(out, "~/dev/Curso-Rust\n~/old/deep/curso-rust-2");
        assert_eq!(
            find(&env(&f.home), "zzz", &Limits::default()).unwrap(),
            "no match"
        );
        assert_eq!(
            find(&env(&f.home), "  ", &Limits::default()),
            Err(Failure::EmptyQuery)
        );
    }

    #[test]
    fn find_dir_skips_excluded_folders_and_does_not_descend_into_them() {
        let f = fixture();
        for dir in [
            ".ssh/curso-rust",
            ".claude/curso-rust",
            "x/.env.d/curso-rust",
            ".aws",
        ] {
            fs::create_dir_all(f.home.join(dir)).unwrap();
        }
        assert_eq!(
            find(&env(&f.home), "curso-rust", &Limits::default()).unwrap(),
            "no match"
        );
        assert_eq!(
            find(&env(&f.home), "aws", &Limits::default()).unwrap(),
            "no match"
        );
        assert_eq!(
            find(&env(&f.home), ".env", &Limits::default()).unwrap(),
            "no match"
        );
    }

    #[test]
    fn find_dir_never_follows_a_symlink() {
        let f = fixture();
        fs::create_dir_all(f.home.join("../outside/curso-rust")).unwrap();
        symlink(f.home.join("../outside"), f.home.join("dev/gate")).unwrap();
        symlink(f.home.join("dev/app"), f.home.join("curso-link")).unwrap();
        assert_eq!(
            find(&env(&f.home), "curso", &Limits::default()).unwrap(),
            "no match"
        );
    }

    #[test]
    fn find_dir_obeys_the_depth_and_result_caps_and_says_so() {
        let f = fixture();
        fs::create_dir_all(f.home.join("a/b/c/target")).unwrap();
        let shallow = Limits {
            depth: 3,
            ..Limits::default()
        };
        let out = find(&env(&f.home), "target", &shallow).unwrap();
        assert_eq!(out, format!("no match\n{FIND_MARKER}"));
        let deep = Limits {
            depth: 4,
            ..Limits::default()
        };
        assert_eq!(
            find(&env(&f.home), "target", &deep).unwrap(),
            "~/a/b/c/target"
        );
        for n in 0..3 {
            fs::create_dir_all(f.home.join(format!("t{n}-hit"))).unwrap();
        }
        let few = Limits {
            results: 2,
            ..Limits::default()
        };
        let out = find(&env(&f.home), "hit", &few).unwrap();
        assert_eq!(out.lines().count(), 3, "{out}");
        assert!(out.ends_with(FIND_MARKER));
    }

    #[test]
    fn find_dir_obeys_the_visited_cap() {
        let f = fixture();
        for n in 0..10 {
            fs::create_dir_all(f.home.join(format!("d{n}"))).unwrap();
        }
        let limits = Limits {
            visited: 4,
            ..Limits::default()
        };
        let out = find(&env(&f.home), "d9", &limits).unwrap();
        assert!(out.ends_with(FIND_MARKER), "{out}");
    }

    #[test]
    fn find_dir_ends_at_its_time_bound_on_a_slow_filesystem() {
        let f = fixture();
        for n in 0..200 {
            fs::create_dir_all(f.home.join(format!("d{n}"))).unwrap();
        }
        let limits = Limits {
            time: Duration::from_millis(40),
            entry_delay: Duration::from_millis(5),
            ..Limits::default()
        };
        let start = Instant::now();
        let out = find(&env(&f.home), "nothing", &limits).unwrap();
        assert!(
            start.elapsed() < Duration::from_millis(400),
            "{:?}",
            start.elapsed()
        );
        assert!(out.ends_with(FIND_MARKER), "{out}");
    }

    #[test]
    fn a_cancelled_find_dir_stops_at_once_and_says_it_was_bounded() {
        let f = fixture();
        for n in 0..200 {
            fs::create_dir_all(f.home.join(format!("d{n}"))).unwrap();
        }
        let limits = Limits {
            entry_delay: Duration::from_millis(20),
            ..Limits::default()
        };
        let cancel = AtomicBool::new(true);
        let call = Call::FindDir { query: "d1".into() };
        let start = Instant::now();
        let out = execute_with(&call, &env(&f.home), &limits, &cancel).unwrap();
        assert!(
            start.elapsed() < Duration::from_millis(150),
            "{:?}",
            start.elapsed()
        );
        assert_eq!(out, format!("no match\n{FIND_MARKER}"));
    }

    #[test]
    fn a_cancelled_list_dir_does_not_resolve_links_and_says_it_was_cut() {
        let f = fixture();
        symlink(f.home.join("dev"), f.home.join("a-link")).unwrap();
        let cancel = AtomicBool::new(true);
        let call = Call::ListDir { path: "~".into() };
        let out = execute_with(&call, &env(&f.home), &Limits::default(), &cancel).unwrap();
        assert_eq!(out, LIST_MARKER);
        let live = AtomicBool::new(false);
        let out = execute_with(&call, &env(&f.home), &Limits::default(), &live).unwrap();
        assert_eq!(out, "l a-link\nd dev");
        assert!(!live.load(Ordering::Relaxed));
    }

    #[test]
    fn the_visited_cap_is_exact() {
        // dev, .ssh, dev/app and app/main.rs: four entries in all.
        let f = fixture();
        let exact = Limits {
            visited: 4,
            ..Limits::default()
        };
        assert_eq!(find(&env(&f.home), "zzz", &exact).unwrap(), "no match");
        let short = Limits {
            visited: 3,
            ..Limits::default()
        };
        let out = find(&env(&f.home), "zzz", &short).unwrap();
        assert_eq!(out, format!("no match\n{FIND_MARKER}"));
    }

    #[test]
    fn a_name_that_redaction_would_change_is_neither_listed_nor_found() {
        let f = fixture();
        fs::write(f.home.join("sk-abcDEF1234567890"), "x").unwrap();
        fs::create_dir_all(f.home.join("dev/sk-ghiJKL1234567890/inner-hit")).unwrap();
        fs::create_dir_all(f.home.join("dev/plain-hit")).unwrap();
        let out = list(&env(&f.home), "~", &Limits::default()).unwrap();
        assert_eq!(out, "d dev");
        let out = list(&env(&f.home), "~/dev", &Limits::default()).unwrap();
        assert_eq!(out, "d app\nd plain-hit");
        // A prefix of the key must not confirm that such a name exists.
        for query in ["sk-g", "sk-ghiJKL", "ghiJKL1234567890", "REDACTED"] {
            let out = find(&env(&f.home), query, &Limits::default()).unwrap();
            assert_eq!(out, "no match", "{query}");
        }
        // Nor is the folder entered: its children would carry the name.
        assert_eq!(
            find(&env(&f.home), "inner-hit", &Limits::default()).unwrap(),
            "no match"
        );
        assert_eq!(
            find(&env(&f.home), "plain-hit", &Limits::default()).unwrap(),
            "~/dev/plain-hit"
        );
    }

    #[test]
    fn a_spent_budget_stops_find_dir_before_it_opens_the_next_folder() {
        let f = fixture();
        for n in 0..200 {
            fs::create_dir_all(f.home.join(format!("d{n}"))).unwrap();
        }
        // Every open costs 10 ms; the call has 60 ms.
        let limits = Limits {
            time: Duration::from_millis(60),
            ..Limits::default()
        };
        OPEN_DELAY.with(|c| c.set(Duration::from_millis(10)));
        OPENS.with(|c| c.set(0));
        let out = find(&env(&f.home), "nothing", &limits).unwrap();
        let opens = OPENS.with(|c| c.get());
        assert!(opens < 20, "{opens} folders were opened");
        assert!(out.ends_with(FIND_MARKER), "{out}");
    }

    #[test]
    fn a_hard_linked_file_is_listed_by_name_but_not_readable() {
        let f = fixture();
        fs::hard_link(f.home.join(".ssh/id"), f.home.join("linked.txt")).unwrap();
        let e = env(&f.home);
        let out = list(&e, "~", &Limits::default()).unwrap();
        assert!(out.contains("f linked.txt"), "{out}");
        assert!(!out.contains("SECRET"));
        assert_eq!(read(&e, "~/linked.txt"), Err(Failure::Refused));
    }

    #[test]
    fn the_tools_leave_the_tree_untouched() {
        let f = fixture();
        let snapshot = |root: &Path| {
            let mut all = Vec::new();
            let mut stack = vec![root.to_path_buf()];
            while let Some(dir) = stack.pop() {
                for entry in fs::read_dir(&dir).unwrap() {
                    let p = entry.unwrap().path();
                    let meta = fs::symlink_metadata(&p).unwrap();
                    all.push((p.clone(), meta.len(), meta.modified().unwrap()));
                    if meta.is_dir() {
                        stack.push(p);
                    }
                }
            }
            all.sort();
            all
        };
        let before = snapshot(f.dir.path());
        let e = env(&f.home);
        let _ = read(&e, "~/dev/app/main.rs");
        let _ = read(&e, "~/.ssh/id");
        let _ = list(&e, "~/dev", &Limits::default());
        let _ = find(&e, "app", &Limits::default());
        assert_eq!(snapshot(f.dir.path()), before);
    }
}
