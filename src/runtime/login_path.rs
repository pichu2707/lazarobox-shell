//! The user's login-shell `PATH`, for a lazarobox started from a desktop
//! launcher or rofi. Those sessions give the app a bare `PATH` (for example
//! `~/.cargo/bin:/usr/local/bin`), so the AI CLIs installed under
//! `~/.local/bin`, linuxbrew or nvm are neither detected nor runnable.
//!
//! The pure parts (reading the marked output, merging two lists) are apart
//! from the probes that run a shell. The shell starts with the process `PATH`
//! plus the system folders, so its rc files find their tools; when `$SHELL`
//! gives nothing, `bash -l` is tried once within the same deadline. The login
//! entries then come first in the merged `PATH`. A failed lookup changes
//! nothing but dropping the relative entries.

use std::{
    ffi::{OsStr, OsString},
    os::unix::ffi::OsStringExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};

use tokio::process::Command;

use super::ai_detect::{REAP_GRACE, run_capped};

/// Longest the whole lookup may take, rc files included, before it is killed.
/// The `$SHELL` probe and the bash fallback share it.
pub const LOGIN_TIMEOUT: Duration = Duration::from_millis(3000);

/// Most output kept; more than this is not a `PATH`.
const OUTPUT_CAP: usize = 64 * 1024;

/// Fences the value in the shell's output, so whatever the rc files print
/// around it is ignored.
const MARK: &str = "__LAZAROBOX_LOGIN_PATH__";

/// Fixed text: nothing user-controlled is ever put in it.
const SCRIPT: &str = "printf '__LAZAROBOX_LOGIN_PATH__%s__LAZAROBOX_LOGIN_PATH__' \"$PATH\"";

/// The shell to ask: `$SHELL` when it is an absolute path, else `/bin/sh`.
pub fn shell_program(shell: Option<&OsStr>) -> PathBuf {
    shell
        .map(Path::new)
        .filter(|p| p.is_absolute())
        .map_or_else(|| PathBuf::from("/bin/sh"), Path::to_path_buf)
}

/// The last well-formed value fenced by two marks in `output`: absolute (it
/// starts with `/`), not empty, no newline or NUL. Every mark is tried as an
/// opening one, so a fake pair printed earlier (an rc file or xtrace echoing
/// the script, with a literal `%s` between its marks) cannot hide the real
/// one.
pub fn parse_marked(output: &[u8]) -> Option<OsString> {
    let mark = MARK.as_bytes();
    let mut best = None;
    let mut from = 0;
    while let Some(at) = find(&output[from..], mark) {
        let start = from + at + mark.len();
        let Some(len) = find(&output[start..], mark) else {
            break;
        };
        let value = &output[start..start + len];
        if value.first() == Some(&b'/') && !value.iter().any(|b| matches!(b, b'\n' | 0)) {
            best = Some(value);
        }
        from = start;
    }
    best.map(|value| OsString::from_vec(value.to_vec()))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// What a `PATH` falls back to when nothing absolute is left. Never empty:
/// `execvp` reads an empty `PATH` as the current directory.
pub const DEFAULT_PATH: &str = "/usr/local/bin:/usr/bin:/bin";

/// The absolute, distinct entries of the given lists, in order.
fn union(lists: &[&OsStr]) -> Vec<PathBuf> {
    let mut seen: Vec<PathBuf> = Vec::new();
    for dir in lists.iter().flat_map(|l| std::env::split_paths(l)) {
        if dir.is_absolute() && !seen.contains(&dir) {
            seen.push(dir);
        }
    }
    seen
}

/// Joins absolute directories; [`DEFAULT_PATH`] when there are none.
fn join_or_default(dirs: Vec<PathBuf>) -> OsString {
    if dirs.is_empty() {
        return OsString::from(DEFAULT_PATH);
    }
    // Absolute directories never contain the separator, so this cannot fail.
    std::env::join_paths(dirs).unwrap_or_else(|_| OsString::from(DEFAULT_PATH))
}

/// The entries of `login` first, then those of `process` that are missing.
/// The login shell's order is the one the user's terminal resolves programs
/// with, so a stale copy of a CLI in a system folder of the bare launcher
/// `PATH` cannot hide the user's own. Only absolute entries stay; empty and
/// repeated ones are dropped. When none is left the result is
/// [`DEFAULT_PATH`], so every caller (the merge and the fallback alike) gets a
/// `PATH` that cannot reach the current directory.
pub fn merge_paths(process: &OsStr, login: &OsStr) -> OsString {
    join_or_default(union(&[login, process]))
}

/// The process `PATH` alone, held to the same rule as a merged one: only
/// absolute entries stay. Used when the login lookup gave nothing.
pub fn sanitize(process: &OsStr) -> OsString {
    merge_paths(process, OsStr::new(""))
}

/// The `PATH` the probe shell starts with: the process entries, then the
/// system folders of [`DEFAULT_PATH`] that are missing. A launcher's bare
/// `PATH` has no `/usr/bin`, and the rc files (`brew shellenv`, nvm) need
/// `tr`, `readlink` and the like from it, or they fail before exporting the
/// user's folders.
pub fn probe_path(process: &OsStr) -> OsString {
    join_or_default(union(&[process, OsStr::new(DEFAULT_PATH)]))
}

/// One probe: runs `shell` with `args` and `path` as its `PATH`, and reads the
/// marked value. `None` on a spawn failure, a non-zero exit, a timeout or
/// output without the marks.
async fn probe(shell: &Path, args: &[&str], path: &OsStr, limit: Duration) -> Option<OsString> {
    let output = run_capped(
        || {
            let mut command = Command::new(shell);
            command
                .args(args)
                .arg(SCRIPT)
                .env("PATH", path)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            // SAFETY: the closure only calls `setsid`, which is
            // async-signal-safe. Its own session leaves the interactive shell
            // without a terminal to fight over, and the group is still ours
            // to kill.
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            command
        },
        limit,
        OUTPUT_CAP,
    )
    .await?;
    parse_marked(&output)
}

/// Asks `shell` (a login, interactive one) for its `PATH`, starting it with
/// `path`. `None` on a spawn failure, a non-zero exit, a timeout or output
/// without the marks.
pub async fn login_shell_path(shell: &Path, path: &OsStr, limit: Duration) -> Option<OsString> {
    probe(shell, &["-l", "-i", "-c"], path, limit).await
}

/// The fallback shell, tried once when `$SHELL` gave nothing.
const FALLBACK_SHELL: &str = "/bin/bash";

/// Time kept for the fallback out of the overall deadline.
const FALLBACK_RESERVE: Duration = Duration::from_millis(1000);

/// What the fallback may run for: what is left of `total` after `elapsed`,
/// less the time a timed-out run may take to reap its child, so the whole
/// lookup stays within `total`. `None` when nothing is left.
fn fallback_budget(total: Duration, elapsed: Duration) -> Option<Duration> {
    let left = total.saturating_sub(elapsed).saturating_sub(REAP_GRACE);
    (!left.is_zero()).then_some(left)
}

/// [`login_shell_path`], then `bash` (a login, non-interactive one: `-l -c`
/// does not read `~/.bashrc`, only the login files, so a `PATH` set only
/// there is not seen; the same script and `PATH`) once when that gave nothing and `shell` is not already
/// bash. Both share the one `total` deadline: the first attempt leaves
/// [`FALLBACK_RESERVE`] of it to the second, which also gives up the time a
/// timed-out run may spend reaping, so the total never passes `total`. `bash` is never run when `shell`
/// succeeded.
async fn login_with_fallback(
    shell: &Path,
    bash: &Path,
    process: &OsStr,
    total: Duration,
) -> Option<OsString> {
    let started = Instant::now();
    let path = probe_path(process);
    let first = total.saturating_sub(FALLBACK_RESERVE).max(total / 2);
    if let Some(found) = login_shell_path(shell, &path, first).await {
        return Some(found);
    }
    if shell.file_name() == bash.file_name() {
        return None;
    }
    let left = fallback_budget(total, started.elapsed())?;
    probe(bash, &["-l", "-c"], &path, left).await
}

async fn resolve_with(
    process: OsString,
    shell: Option<OsString>,
    bash: &Path,
    total: Duration,
) -> OsString {
    match login_with_fallback(&shell_program(shell.as_deref()), bash, &process, total).await {
        Some(login) => merge_paths(&process, &login),
        None => sanitize(&process),
    }
}

/// `process` extended with the login shell's entries, which come first. On
/// any failure it is returned with only its absolute entries.
pub async fn resolve(process: OsString, shell: Option<OsString>, limit: Duration) -> OsString {
    resolve_with(process, shell, Path::new(FALLBACK_SHELL), limit).await
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt, time::Instant};

    use super::*;

    fn marked(value: &str) -> Vec<u8> {
        format!("{MARK}{value}{MARK}").into_bytes()
    }

    fn os(s: &str) -> OsString {
        OsString::from(s)
    }

    fn fake_shell(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn parses_the_value_between_the_marks() {
        assert_eq!(parse_marked(&marked("/a:/b")), Some(os("/a:/b")));
    }

    #[test]
    fn ignores_noise_before_and_after_the_marks() {
        let mut out = b"welcome\nmotd /x:/y\n".to_vec();
        out.extend(marked("/a:/b"));
        out.extend(b"\nbye\n");
        assert_eq!(parse_marked(&out), Some(os("/a:/b")));
    }

    #[test]
    fn an_echoed_script_cannot_mask_the_real_value() {
        // xtrace or an rc file that echoes the script prints a fake pair
        // around a literal `%s` before the real one.
        let mut out = format!("+ printf '{MARK}%s{MARK}' \"$PATH\"\n").into_bytes();
        out.extend(marked("/a:/b"));
        assert_eq!(parse_marked(&out), Some(os("/a:/b")));
        let mut traced = format!("+ printf '{MARK}%s{MARK}' /x:/y\n").into_bytes();
        traced.extend(marked("/a:/b"));
        assert_eq!(parse_marked(&traced), Some(os("/a:/b")));
    }

    #[test]
    fn the_last_well_formed_value_wins_and_it_must_be_absolute() {
        let mut out = marked("/first");
        out.extend(marked("/second"));
        assert_eq!(parse_marked(&out), Some(os("/second")));
        assert_eq!(parse_marked(&marked("bin:/a")), None);
        assert_eq!(parse_marked(&marked("%s")), None);
        let mut tail = marked("/real");
        tail.extend(marked("junk"));
        assert_eq!(parse_marked(&tail), Some(os("/real")));
    }

    #[test]
    fn rejects_missing_empty_and_garbage_values() {
        assert_eq!(parse_marked(b""), None);
        assert_eq!(parse_marked(b"/a:/b"), None);
        assert_eq!(parse_marked(format!("{MARK}/a:/b").as_bytes()), None);
        assert_eq!(parse_marked(&marked("")), None);
        assert_eq!(parse_marked(&marked("/a\n/b")), None);
        assert_eq!(parse_marked(&marked("/a\0/b")), None);
    }

    #[test]
    fn merge_puts_the_login_entries_first() {
        // The launcher must pick what the user's terminal picks: a stale
        // `/usr/local/bin/codex` must not hide the login shell's own.
        let merged = merge_paths(&os("/p1:/p2"), &os("/l1:/p2:/l2"));
        assert_eq!(merged, os("/l1:/p2:/l2:/p1"));
    }

    #[test]
    fn merge_drops_relative_empty_and_repeated_entries() {
        let merged = merge_paths(&os("/p::rel:/p"), &os("bin:/l:/l:"));
        assert_eq!(merged, os("/l:/p"));
    }

    #[test]
    fn merge_with_an_empty_login_keeps_the_process_path() {
        assert_eq!(merge_paths(&os("/p1:/p2"), &os("")), os("/p1:/p2"));
    }

    #[test]
    fn shell_is_sh_unless_it_is_absolute() {
        assert_eq!(shell_program(None), PathBuf::from("/bin/sh"));
        assert_eq!(shell_program(Some(&os("zsh"))), PathBuf::from("/bin/sh"));
        assert_eq!(shell_program(Some(&os(""))), PathBuf::from("/bin/sh"));
        assert_eq!(
            shell_program(Some(&os("/usr/bin/zsh"))),
            PathBuf::from("/usr/bin/zsh")
        );
    }

    #[tokio::test]
    async fn reads_the_path_through_noise_in_the_rc_files() {
        let dir = tempfile::tempdir().unwrap();
        let shell = fake_shell(
            dir.path(),
            "noisy",
            &format!(
                "echo 'rc says hi'\nprintf '{MARK}/home/u/.local/bin:/opt/brew/bin{MARK}'\necho tail"
            ),
        );
        let got = login_shell_path(&shell, &os("/p"), Duration::from_secs(5)).await;
        assert_eq!(got, Some(os("/home/u/.local/bin:/opt/brew/bin")));
    }

    #[tokio::test]
    async fn passes_login_and_interactive_flags_and_the_fixed_script() {
        let dir = tempfile::tempdir().unwrap();
        let seen = dir.path().join("seen");
        let shell = fake_shell(
            dir.path(),
            "argv",
            &format!(
                "printf '%s\\n' \"$#\" \"$1\" \"$2\" \"$3\" \"$4\" > '{}'\nprintf '{MARK}/x{MARK}'",
                seen.display()
            ),
        );
        login_shell_path(&shell, &os("/p"), Duration::from_secs(5))
            .await
            .unwrap();
        let lines: Vec<String> = fs::read_to_string(seen)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        assert_eq!(lines, ["4", "-l", "-i", "-c", SCRIPT]);
    }

    #[tokio::test]
    async fn a_hanging_shell_is_killed_at_the_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let shell = fake_shell(dir.path(), "hang", "sleep 30");
        let started = Instant::now();
        let got = login_shell_path(&shell, &os("/p"), Duration::from_millis(300)).await;
        assert_eq!(got, None);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn a_failing_shell_gives_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let shell = fake_shell(
            dir.path(),
            "fail",
            &format!("printf '{MARK}/x{MARK}'\nexit 1"),
        );
        assert_eq!(
            login_shell_path(&shell, &os("/p"), Duration::from_secs(5)).await,
            None
        );
    }

    #[tokio::test]
    async fn a_missing_shell_gives_nothing() {
        let got = login_shell_path(
            Path::new("/nonexistent/shell"),
            &os("/p"),
            Duration::from_secs(1),
        )
        .await;
        assert_eq!(got, None);
    }

    #[tokio::test]
    async fn resolve_merges_on_success_and_keeps_the_path_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let good = fake_shell(dir.path(), "good", &format!("printf '{MARK}/l:/p{MARK}'"));
        let bad = fake_shell(dir.path(), "bad", "exit 1");
        let t = Duration::from_secs(5);
        let nobash = fake_shell(dir.path(), "nobash", "exit 1");
        assert_eq!(
            resolve_with(os("/p"), Some(good.clone().into_os_string()), &nobash, t).await,
            os("/l:/p")
        );
        assert_eq!(
            resolve_with(os("/p"), Some(bad.clone().into_os_string()), &nobash, t).await,
            os("/p")
        );
    }

    #[tokio::test]
    async fn a_failed_lookup_still_drops_relative_and_empty_entries() {
        let dir = tempfile::tempdir().unwrap();
        let bad = fake_shell(dir.path(), "bad", "exit 1");
        let t = Duration::from_secs(5);
        let nobash = fake_shell(dir.path(), "nobash", "exit 1");
        assert_eq!(
            resolve_with(os(":rel:/p:bin:/p"), Some(bad.into_os_string()), &nobash, t).await,
            os("/p")
        );
    }

    #[test]
    fn the_probe_path_is_the_process_one_plus_the_system_dirs() {
        assert_eq!(
            probe_path(&os("/a:/usr/bin:rel::/b")),
            os("/a:/usr/bin:/b:/usr/local/bin:/bin")
        );
        assert_eq!(probe_path(&os("")), os(DEFAULT_PATH));
        assert_eq!(probe_path(&os("rel")), os(DEFAULT_PATH));
    }

    #[tokio::test]
    async fn the_shell_starts_with_the_system_dirs_on_its_path() {
        // The rc files of a real shell call `tr`, `readlink`... from
        // `/usr/bin`, which a launcher's bare PATH lacks.
        let dir = tempfile::tempdir().unwrap();
        let shell = fake_shell(
            dir.path(),
            "needs-tr",
            &format!("echo x | tr x y >/dev/null || exit 1\nprintf '{MARK}%s{MARK}' \"$PATH\""),
        );
        let bare = os("/nonexistent:rel");
        let got = login_shell_path(&shell, &probe_path(&bare), Duration::from_secs(5)).await;
        assert_eq!(got, Some(os("/nonexistent:/usr/local/bin:/usr/bin:/bin")));
    }

    #[tokio::test]
    async fn a_failing_shell_makes_bash_run_with_the_same_script_and_path() {
        let dir = tempfile::tempdir().unwrap();
        let bad = fake_shell(dir.path(), "zsh", "exit 1");
        let seen = dir.path().join("seen");
        let bash = fake_shell(
            dir.path(),
            "fakebash",
            &format!(
                "printf '%s\\n' \"$#\" \"$1\" \"$2\" \"$3\" > '{}'\nprintf '{MARK}%s{MARK}' \"$PATH\"",
                seen.display()
            ),
        );
        let got = login_with_fallback(&bad, &bash, &os("/p"), Duration::from_secs(5)).await;
        assert_eq!(got, Some(os("/p:/usr/local/bin:/usr/bin:/bin")));
        let lines: Vec<String> = fs::read_to_string(seen)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        assert_eq!(lines, ["3", "-l", "-c", SCRIPT]);
    }

    #[tokio::test]
    async fn a_shell_without_a_value_also_makes_bash_run() {
        let dir = tempfile::tempdir().unwrap();
        let silent = fake_shell(dir.path(), "zsh", "echo nothing here");
        let bash = fake_shell(dir.path(), "fakebash", &format!("printf '{MARK}/b{MARK}'"));
        let got = login_with_fallback(&silent, &bash, &os("/p"), Duration::from_secs(5)).await;
        assert_eq!(got, Some(os("/b")));
    }

    #[tokio::test]
    async fn bash_is_not_run_when_the_shell_succeeded() {
        let dir = tempfile::tempdir().unwrap();
        let good = fake_shell(dir.path(), "zsh", &format!("printf '{MARK}/z{MARK}'"));
        let ran = dir.path().join("ran");
        let bash = fake_shell(
            dir.path(),
            "fakebash",
            &format!("touch '{}'\nprintf '{MARK}/b{MARK}'", ran.display()),
        );
        let got = login_with_fallback(&good, &bash, &os("/p"), Duration::from_secs(5)).await;
        assert_eq!(got, Some(os("/z")));
        assert!(!ran.exists());
    }

    #[tokio::test]
    async fn bash_is_not_retried_when_the_shell_already_is_bash() {
        let dir = tempfile::tempdir().unwrap();
        let ran = dir.path().join("ran");
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        // `$SHELL` is a bash that fails; the fallback is the same program.
        let bash = fake_shell(
            &sub,
            "bash",
            &format!("echo x >> '{}'\nexit 1", ran.display()),
        );
        let got = login_with_fallback(&bash, &bash, &os("/p"), Duration::from_secs(5)).await;
        assert_eq!(got, None);
        assert_eq!(fs::read_to_string(&ran).unwrap().lines().count(), 1);
    }

    #[test]
    fn the_fallback_budget_leaves_room_to_reap() {
        let ms = Duration::from_millis;
        assert_eq!(fallback_budget(ms(3000), ms(2000)), Some(ms(500)));
        assert_eq!(fallback_budget(ms(3000), ms(2500)), None);
        assert_eq!(fallback_budget(ms(3000), ms(2600)), None);
        assert_eq!(fallback_budget(ms(3000), ms(0)), Some(ms(2500)));
    }

    #[tokio::test]
    async fn both_attempts_share_one_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let one = dir.path().join("one");
        let two = dir.path().join("two");
        let hang = |name: &str, mark: &Path| {
            fake_shell(
                dir.path(),
                name,
                &format!("touch '{}'\nsleep 30", mark.display()),
            )
        };
        let (zsh, bash) = (hang("zsh", &one), hang("fakebash", &two));
        let started = Instant::now();
        let got = login_with_fallback(&zsh, &bash, &os("/p"), Duration::from_millis(2000)).await;
        assert_eq!(got, None);
        assert!(one.exists() && two.exists(), "bash got no time to run");
        // Not 2 x 1.2 s: one budget for both.
        assert!(
            started.elapsed() < Duration::from_millis(2000),
            "{:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn resolve_falls_back_to_bash_and_puts_the_login_entries_first() {
        let dir = tempfile::tempdir().unwrap();
        let bad = fake_shell(dir.path(), "zsh", "exit 1");
        let bash = fake_shell(
            dir.path(),
            "fakebash",
            &format!("printf '{MARK}/l:/p{MARK}'"),
        );
        let got = resolve_with(
            os("/p:/q"),
            Some(bad.into_os_string()),
            &bash,
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(got, os("/l:/p:/q"));
    }

    #[test]
    fn the_fallback_path_keeps_only_absolute_entries() {
        assert_eq!(sanitize(&os(":rel:/a:./b:/c")), os("/a:/c"));
    }

    #[test]
    fn an_empty_or_all_relative_path_gives_the_fixed_default() {
        // An empty PATH makes execvp search the current directory.
        for process in ["", ":", "rel", ":rel:./bin"] {
            assert_eq!(sanitize(&os(process)), os(DEFAULT_PATH), "{process:?}");
            assert_eq!(merge_paths(&os(process), &os("")), os(DEFAULT_PATH));
            assert_eq!(merge_paths(&os(process), &os("also/rel")), os(DEFAULT_PATH));
        }
        assert_eq!(merge_paths(&os(""), &os("/l")), os("/l"));
    }

    #[tokio::test]
    async fn a_failed_lookup_with_no_usable_path_gives_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let bad = fake_shell(dir.path(), "bad", "exit 1");
        let t = Duration::from_secs(5);
        let nobash = fake_shell(dir.path(), "nobash", "exit 1");
        assert_eq!(
            resolve_with(os(""), Some(bad.into_os_string()), &nobash, t).await,
            os(DEFAULT_PATH)
        );
    }
}
