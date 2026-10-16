//! The user's login-shell `PATH`, for a lazarobox started from a desktop
//! launcher or rofi. Those sessions give the app a bare `PATH` (for example
//! `~/.cargo/bin:/usr/local/bin`), so the AI CLIs installed under
//! `~/.local/bin`, linuxbrew or nvm are neither detected nor runnable.
//!
//! The pure parts (reading the marked output, merging two lists) are apart
//! from the one probe that runs the shell. A failed probe changes nothing.

use std::{
    ffi::{OsStr, OsString},
    os::unix::ffi::OsStringExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use tokio::process::Command;

use super::ai_detect::run_capped;

/// Longest the login shell may take, rc files included, before it is killed.
pub const LOGIN_TIMEOUT: Duration = Duration::from_millis(2500);

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

/// The value between the first two marks of `output`. `None` when the marks
/// are missing, the value is empty or it has a newline or NUL in it.
pub fn parse_marked(output: &[u8]) -> Option<OsString> {
    let mark = MARK.as_bytes();
    let start = find(output, mark)? + mark.len();
    let len = find(&output[start..], mark)?;
    let value = &output[start..start + len];
    if value.is_empty() || value.iter().any(|b| matches!(b, b'\n' | 0)) {
        return None;
    }
    Some(OsString::from_vec(value.to_vec()))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// The entries of `process` first, then those of `login` that are missing.
/// Only absolute entries stay; empty and repeated ones are dropped.
pub fn merge_paths(process: &OsStr, login: &OsStr) -> OsString {
    let mut seen: Vec<PathBuf> = Vec::new();
    for dir in std::env::split_paths(process).chain(std::env::split_paths(login)) {
        if dir.is_absolute() && !seen.contains(&dir) {
            seen.push(dir);
        }
    }
    std::env::join_paths(seen).unwrap_or_else(|_| process.to_owned())
}

/// Asks `shell` (a login, interactive one) for its `PATH`. `None` on a
/// spawn failure, a non-zero exit, a timeout or output without the marks.
pub async fn login_shell_path(shell: &Path, limit: Duration) -> Option<OsString> {
    let output = run_capped(
        || {
            let mut command = Command::new(shell);
            command
                .args(["-l", "-i", "-c", SCRIPT])
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

/// `process` extended with the login shell's entries. On any failure it is
/// returned unchanged.
pub async fn resolve(process: OsString, shell: Option<OsString>, limit: Duration) -> OsString {
    match login_shell_path(&shell_program(shell.as_deref()), limit).await {
        Some(login) => merge_paths(&process, &login),
        None => process,
    }
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
    fn rejects_missing_empty_and_garbage_values() {
        assert_eq!(parse_marked(b""), None);
        assert_eq!(parse_marked(b"/a:/b"), None);
        assert_eq!(parse_marked(format!("{MARK}/a:/b").as_bytes()), None);
        assert_eq!(parse_marked(&marked("")), None);
        assert_eq!(parse_marked(&marked("/a\n/b")), None);
        assert_eq!(parse_marked(&marked("/a\0/b")), None);
    }

    #[test]
    fn merge_keeps_the_process_entries_first() {
        let merged = merge_paths(&os("/p1:/p2"), &os("/l1:/p2:/l2"));
        assert_eq!(merged, os("/p1:/p2:/l1:/l2"));
    }

    #[test]
    fn merge_drops_relative_empty_and_repeated_entries() {
        let merged = merge_paths(&os("/p::rel:/p"), &os("bin:/l:/l:"));
        assert_eq!(merged, os("/p:/l"));
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
        let got = login_shell_path(&shell, Duration::from_secs(5)).await;
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
        login_shell_path(&shell, Duration::from_secs(5))
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
        let got = login_shell_path(&shell, Duration::from_millis(300)).await;
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
        assert_eq!(login_shell_path(&shell, Duration::from_secs(5)).await, None);
    }

    #[tokio::test]
    async fn a_missing_shell_gives_nothing() {
        let got = login_shell_path(Path::new("/nonexistent/shell"), Duration::from_secs(1)).await;
        assert_eq!(got, None);
    }

    #[tokio::test]
    async fn resolve_merges_on_success_and_keeps_the_path_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let good = fake_shell(dir.path(), "good", &format!("printf '{MARK}/l:/p{MARK}'"));
        let bad = fake_shell(dir.path(), "bad", "exit 1");
        let t = Duration::from_secs(5);
        assert_eq!(
            resolve(os("/p"), Some(good.clone().into_os_string()), t).await,
            os("/p:/l")
        );
        assert_eq!(
            resolve(os("/p"), Some(bad.clone().into_os_string()), t).await,
            os("/p")
        );
    }
}
