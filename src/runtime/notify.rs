//! The desktop half of chat-notify: `notify-send`, run directly (no shell)
//! from the merged login `PATH`, detached from the UI thread.

use std::{
    ffi::{OsStr, OsString},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use tokio::process::Command;

use super::ai_detect::{find_on_path, run_capped};

/// The program that shows the notification.
const PROGRAM: &str = "notify-send";

/// The title of every notification.
const TITLE: &str = "lazarobox";

/// Longest `notify-send` may run before it is killed and reaped.
const TIMEOUT: Duration = Duration::from_secs(2);

/// More output than this is not a notification tool at all.
const OUTPUT_CAP: usize = 4 * 1024;

/// Longest body handed to the program, in characters.
const BODY_CAP: usize = 120;

/// Whether the session has a display to notify on.
pub fn display_set() -> bool {
    ["DISPLAY", "WAYLAND_DISPLAY"]
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|v| !v.is_empty()))
}

/// Whether a desktop notification can be sent: a display is set and
/// `notify-send` resolves on `path` (absolute entries only, never the cwd).
pub fn available(display: bool, path: &OsStr) -> bool {
    display && find_on_path(PROGRAM, path).is_some()
}

/// `text` as one short plain line: control characters become spaces and the
/// length is capped.
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(BODY_CAP)
        .collect()
}

/// Runs `notify-send` with `body`. `true` when it started and exited 0 inside
/// the timeout. The child has no stdin or stderr, runs in its own process
/// group, and is killed and reaped on every way out.
pub async fn send(path: &OsStr, body: &str) -> bool {
    let Some(program) = find_on_path(PROGRAM, path) else {
        return false;
    };
    let body = clean(body);
    run_capped(
        || {
            let mut command = Command::new(&program);
            // `--` so a body can never be read as an option.
            command
                .args(["--app-name", TITLE, "--", TITLE])
                .arg(&body)
                .env("PATH", path)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .process_group(0);
            command
        },
        TIMEOUT,
        OUTPUT_CAP,
    )
    .await
    .is_some()
}

/// Sends in the background and returns at once. When it fails and `fallback`
/// is set, the flag is raised so the next flush rings the bell instead.
pub fn spawn_send(path: OsString, body: String, fallback: Option<Arc<AtomicBool>>) {
    tokio::spawn(async move {
        if !send(&path, &body).await
            && let Some(flag) = fallback
        {
            flag.store(true, Ordering::Release);
        }
    });
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt, path::Path};

    use super::*;

    fn script(dir: &Path, body: &str) {
        let path = dir.join(PROGRAM);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn availability_needs_a_display_and_the_program_on_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().as_os_str().to_owned();
        assert!(!available(true, &path), "no program");
        script(dir.path(), "exit 0");
        assert!(available(true, &path));
        assert!(!available(false, &path), "no display");
    }

    #[test]
    fn a_relative_path_entry_never_resolves_the_program() {
        let dir = tempfile::tempdir().unwrap();
        script(dir.path(), "exit 0");
        assert!(!available(true, OsStr::new(".")));
        assert!(!available(true, OsStr::new("")));
    }

    #[test]
    fn the_body_is_one_plain_capped_line() {
        assert_eq!(clean("a\nb\x1b[31mc"), "a b [31mc");
        assert_eq!(clean(&"x".repeat(500)).len(), BODY_CAP);
    }

    #[tokio::test]
    async fn send_passes_the_title_and_body_as_arguments() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("args");
        script(
            dir.path(),
            &format!("printf '%s\\n' \"$@\" > '{}'", out.display()),
        );
        let path = dir.path().as_os_str().to_owned();
        assert!(send(&path, "Answer ready in pane 3; $(touch pwned)").await);
        assert_eq!(
            fs::read_to_string(&out).unwrap(),
            "--app-name\nlazarobox\n--\nlazarobox\nAnswer ready in pane 3; $(touch pwned)\n"
        );
        assert!(!dir.path().join("pwned").exists(), "no shell involved");
    }

    #[tokio::test]
    async fn a_missing_program_or_a_failing_exit_is_a_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().as_os_str().to_owned();
        assert!(!send(&path, "x").await);
        script(dir.path(), "exit 1");
        assert!(!send(&path, "x").await);
    }

    #[tokio::test]
    async fn a_hung_program_is_killed_after_the_timeout() {
        let dir = tempfile::tempdir().unwrap();
        script(dir.path(), "PATH=\"$PATH:/usr/bin:/bin\"\nsleep 30");
        let path = dir.path().as_os_str().to_owned();
        let started = std::time::Instant::now();
        assert!(!send(&path, "x").await);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn a_failed_background_send_raises_the_fallback_flag() {
        let dir = tempfile::tempdir().unwrap();
        script(dir.path(), "exit 1");
        let flag = Arc::new(AtomicBool::new(false));
        spawn_send(
            dir.path().as_os_str().to_owned(),
            "x".to_owned(),
            Some(Arc::clone(&flag)),
        );
        for _ in 0..100 {
            if flag.load(Ordering::Acquire) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("the fallback flag was never raised");
    }
}
