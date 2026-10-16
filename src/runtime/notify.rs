//! The desktop half of chat-notify: `notify-send`, run directly (no shell)
//! from the merged login `PATH`, detached from the UI thread.
//!
//! Everything that can touch the disk or wait (the availability probe, the
//! trust check, the spawn) runs inside the spawned task, never in `apply`.

use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use tokio::{process::Command, sync::mpsc};

use super::{
    ai_detect::{find_on_path, run_capped},
    chat::is_trusted_program,
};
use crate::core::ai::{
    chat::notify::{bell_after_failed_desktop, describe_test, plays_sound, wants_desktop},
    config::NotifyMode,
};

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

/// The only variables the child inherits (besides `LC_*` and the merged
/// `PATH`, which is set apart): enough to find the session bus and the
/// display, nothing that could hold a secret.
const ALLOWED_ENV: [&str; 9] = [
    "HOME",
    "LANG",
    "DBUS_SESSION_BUS_ADDRESS",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XDG_RUNTIME_DIR",
    "XAUTHORITY",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_TYPE",
];

/// The facts of the session that decide whether a desktop notification can
/// show up at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Session {
    /// `DISPLAY` or `WAYLAND_DISPLAY` is set.
    pub display: bool,
    /// Running over SSH: the notification would pop up on a machine the user
    /// is not at (or nowhere).
    pub ssh: bool,
    /// A session bus address is set, or `$XDG_RUNTIME_DIR/bus` exists.
    pub bus: bool,
}

impl Session {
    /// Reads the session through `get` (an environment lookup) and `exists`
    /// (a path check); both injectable. Empty values count as unset.
    pub fn probe(get: impl Fn(&str) -> Option<OsString>, exists: impl Fn(&Path) -> bool) -> Self {
        let set = |name: &str| get(name).is_some_and(|v| !v.is_empty());
        let bus = set("DBUS_SESSION_BUS_ADDRESS")
            || get("XDG_RUNTIME_DIR")
                .filter(|dir| !dir.is_empty())
                .is_some_and(|dir| exists(&Path::new(&dir).join("bus")));
        Self {
            display: set("DISPLAY") || set("WAYLAND_DISPLAY"),
            ssh: set("SSH_CONNECTION") || set("SSH_TTY"),
            bus,
        }
    }

    /// The session of this process.
    pub fn current() -> Self {
        Self::probe(|name| std::env::var_os(name), |p| p.exists())
    }
}

/// The first program called `name` on `path` that is trusted: a regular
/// executable whose real path is not under a scratch folder and that neither it
/// nor its folder is writable by group or others (the rule the CLI
/// interpreters follow). An untrusted entry is skipped, not fatal. Absolute
/// entries only. The error says why none was usable.
fn program_state(name: &str, path: &OsStr) -> Result<PathBuf, String> {
    let mut found = false;
    for dir in std::env::split_paths(path) {
        let Some(file) = find_on_path(name, dir.as_os_str()) else {
            continue;
        };
        found = true;
        if is_trusted_program(&file) {
            return Ok(file);
        }
    }
    Err(if found {
        format!("{name} on PATH is untrusted (under /tmp or writable by others)")
    } else {
        format!("no {name} on PATH")
    })
}

fn trusted_program(path: &OsStr) -> Option<PathBuf> {
    program_state(PROGRAM, path).ok()
}

/// Why a desktop notification cannot be sent, in a few words; `None` when it
/// can: the session has a display and a bus, is not over SSH, and a trusted
/// `notify-send` is on `path`.
pub fn unavailable_reason(session: &Session, path: &OsStr) -> Option<String> {
    if session.ssh {
        return Some("SSH session".to_owned());
    }
    if !session.display {
        return Some("no display".to_owned());
    }
    if !session.bus {
        return Some("no session bus".to_owned());
    }
    program_state(PROGRAM, path).err()
}

/// Whether a desktop notification can be sent.
pub fn available(session: &Session, path: &OsStr) -> bool {
    unavailable_reason(session, path).is_none()
}

/// The part of `env` the child may see: the allow-list and every `LC_*`.
pub fn allowed_env(
    env: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    env.into_iter()
        .filter(|(key, _)| {
            let key = key.to_string_lossy();
            ALLOWED_ENV.contains(&key.as_ref()) || key.starts_with("LC_")
        })
        .collect()
}

/// `text` as one short plain line: control characters become spaces and the
/// length is capped.
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(BODY_CAP)
        .collect()
}

/// Runs `program` with `args`, with an empty environment but `PATH` and
/// `vars`. `true` when it started and exited 0 inside the timeout. The child
/// has no stdin or stderr, runs in its own process group, and is killed and
/// reaped on every way out.
async fn run_tool(
    program: &Path,
    args: &[OsString],
    path: &OsStr,
    vars: &[(OsString, OsString)],
) -> bool {
    run_capped(
        || {
            let mut command = Command::new(program);
            command
                .args(args)
                .env_clear()
                .envs(vars.iter().map(|(k, v)| (k, v)))
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

/// Runs the trusted `notify-send` on `path` with `body`.
pub async fn send(path: &OsStr, body: &str, vars: &[(OsString, OsString)]) -> bool {
    let Some(program) = trusted_program(path) else {
        return false;
    };
    // `--` so a body can never be read as an option.
    let args: Vec<OsString> = ["--app-name", TITLE, "--", TITLE]
        .iter()
        .map(OsString::from)
        .chain([OsString::from(clean(body))])
        .collect();
    run_tool(&program, &args, path, vars).await
}

/// The sound files, in order of preference.
const SOUND_FILES: [&str; 2] = [
    "/usr/share/sounds/freedesktop/stereo/message-new-instant.oga",
    "/usr/share/sounds/freedesktop/stereo/complete.oga",
];

/// The sound players, in order. `canberra-gtk-play` plays by event name; the
/// others need a file.
const PLAYERS: [&str; 3] = ["canberra-gtk-play", "pw-play", "paplay"];

/// The program and argument vector that play the sound: the first trusted
/// player on `path` that can run (`exists` says which sound files exist).
pub fn sound_plan(
    path: &OsStr,
    exists: impl Fn(&Path) -> bool,
) -> Result<(PathBuf, Vec<OsString>), String> {
    let file = SOUND_FILES.iter().map(Path::new).find(|f| exists(f));
    let mut found = false;
    let mut needed_file = false;
    for name in PLAYERS {
        let Ok(program) = program_state(name, path) else {
            continue;
        };
        found = true;
        if name == "canberra-gtk-play" {
            return Ok((program, vec!["-i".into(), "message-new-instant".into()]));
        }
        match file {
            Some(file) => return Ok((program, vec![file.as_os_str().to_owned()])),
            None => needed_file = true,
        }
    }
    Err(if found && needed_file {
        "no sound file found".to_owned()
    } else {
        "no sound player on PATH".to_owned()
    })
}

/// Plays the sound. The error says why not.
pub async fn play(
    path: &OsStr,
    vars: &[(OsString, OsString)],
    exists: impl Fn(&Path) -> bool,
) -> Result<(), String> {
    let (program, args) = sound_plan(path, exists)?;
    if run_tool(&program, &args, path, vars).await {
        Ok(())
    } else {
        Err("the sound player failed".to_owned())
    }
}

/// One desktop notification to send in the background.
pub struct Job {
    /// The merged login `PATH`.
    pub path: OsString,
    pub body: String,
    /// Raised when the notification cannot be sent or fails, so the next
    /// flush rings the bell instead.
    pub fallback: Option<Arc<AtomicBool>>,
    /// Set while a send is in flight: a job that finds it set is dropped.
    pub busy: Arc<AtomicBool>,
    /// Reads the session; run inside the task.
    pub session: fn() -> Session,
}

/// Clears the in-flight flag on every way out, a panic included.
struct Release(Arc<AtomicBool>);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Sends in the background and returns at once, without touching the disk.
/// At most one send is in flight: a burst of finished turns shows one
/// notification and the rest are dropped (the bell of `auto` already rang for
/// each). A desktop that is unavailable counts as a failed send.
pub fn spawn_send(job: Job) {
    if job.busy.swap(true, Ordering::AcqRel) {
        return;
    }
    tokio::spawn(async move {
        let _release = Release(Arc::clone(&job.busy));
        let sent = available_then_send(&job).await;
        if !sent && let Some(flag) = &job.fallback {
            flag.store(true, Ordering::Release);
        }
    });
}

async fn available_then_send(job: &Job) -> bool {
    let session = (job.session)();
    if !available(&session, &job.path) {
        return false;
    }
    send(&job.path, &job.body, &allowed_env(std::env::vars_os())).await
}

/// One sound to play in the background.
pub struct SoundJob {
    /// The merged login `PATH`.
    pub path: OsString,
    /// Set while a sound is in flight: a job that finds it set is dropped.
    pub busy: Arc<AtomicBool>,
    /// Reads the session; run inside the task.
    pub session: fn() -> Session,
}

/// Plays in the background and returns at once, with the same rules as
/// [`spawn_send`]: one in flight, nothing over SSH or without a session, and
/// nothing at all when no player is found.
pub fn spawn_sound(job: SoundJob) {
    if job.busy.swap(true, Ordering::AcqRel) {
        return;
    }
    tokio::spawn(async move {
        let _release = Release(Arc::clone(&job.busy));
        let session = (job.session)();
        if session.ssh || !session.display || !session.bus {
            return;
        }
        let _ = play(&job.path, &allowed_env(std::env::vars_os()), Path::exists).await;
    });
}

/// What the test action does and reports, with the focus rule ignored. The
/// bell is the caller's; this attempts the desktop and the sound the mode
/// asks for and describes the result.
pub async fn run_test(
    path: &OsStr,
    session: &Session,
    mode: NotifyMode,
    sound: bool,
    vars: &[(OsString, OsString)],
) -> String {
    let desktop = if wants_desktop(mode) {
        Some(match unavailable_reason(session, path) {
            Some(why) => Err(why),
            None if send(path, "Test notification", vars).await => Ok(()),
            None => Err("notify-send failed".to_owned()),
        })
    } else {
        None
    };
    let sound = if plays_sound(mode, sound) {
        Some(if session.ssh || !session.display || !session.bus {
            Err("no desktop session".to_owned())
        } else {
            play(path, vars, Path::exists).await
        })
    } else {
        None
    };
    describe_test(mode, desktop, sound)
}

/// One test action to run in the background.
pub struct TestJob {
    pub path: OsString,
    pub mode: NotifyMode,
    pub sound: bool,
    /// Raised when `desktop` mode could not show its notification, so the
    /// bell rings instead.
    pub fallback: Option<Arc<AtomicBool>>,
    pub report: mpsc::Sender<String>,
    pub session: fn() -> Session,
}

/// Runs the test in the background and sends the report line. It is never
/// coalesced: it answers a key press.
pub fn spawn_test(job: TestJob) {
    tokio::spawn(async move {
        let session = (job.session)();
        let text = run_test(
            &job.path,
            &session,
            job.mode,
            job.sound,
            &allowed_env(std::env::vars_os()),
        )
        .await;
        if text.contains("desktop unavailable")
            && bell_after_failed_desktop(job.mode)
            && let Some(flag) = &job.fallback
        {
            flag.store(true, Ordering::Release);
        }
        let _ = job.report.send(text).await;
    });
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt, path::Path};

    use super::*;
    use crate::core::ai::config::NotifyMode;

    /// A private folder outside `/tmp`, which is never trusted.
    fn lab() -> std::io::Result<tempfile::TempDir> {
        let dir = tempfile::tempdir_in(concat!(env!("CARGO_MANIFEST_DIR"), "/target"))?;
        // The umask may leave a new folder group-writable.
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755))?;
        Ok(dir)
    }

    fn here() -> Session {
        Session {
            display: true,
            ssh: false,
            bus: true,
        }
    }

    fn script(dir: &Path, body: &str) {
        let path = dir.join(PROGRAM);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn availability_needs_a_display_and_the_program_on_the_path() {
        let dir = lab().unwrap();
        let path = dir.path().as_os_str().to_owned();
        assert!(!available(&here(), &path), "no program");
        script(dir.path(), "exit 0");
        assert!(available(&here(), &path));
        for session in [
            Session {
                display: false,
                ..here()
            },
            Session {
                ssh: true,
                ..here()
            },
            Session {
                bus: false,
                ..here()
            },
        ] {
            assert!(!available(&session, &path), "{session:?}");
        }
    }

    #[test]
    fn a_relative_path_entry_never_resolves_the_program() {
        let dir = lab().unwrap();
        script(dir.path(), "exit 0");
        assert!(!available(&here(), OsStr::new(".")));
        assert!(!available(&here(), OsStr::new("")));
    }

    #[test]
    fn the_body_is_one_plain_capped_line() {
        assert_eq!(clean("a\nb\x1b[31mc"), "a b [31mc");
        assert_eq!(clean(&"x".repeat(500)).len(), BODY_CAP);
    }

    #[tokio::test]
    async fn send_passes_the_title_and_body_as_arguments() {
        let dir = lab().unwrap();
        let out = dir.path().join("args");
        script(
            dir.path(),
            &format!("printf '%s\\n' \"$@\" > '{}'", out.display()),
        );
        let path = dir.path().as_os_str().to_owned();
        assert!(send(&path, "Answer ready in pane 3; $(touch pwned)", &[]).await);
        assert_eq!(
            fs::read_to_string(&out).unwrap(),
            "--app-name\nlazarobox\n--\nlazarobox\nAnswer ready in pane 3; $(touch pwned)\n"
        );
        assert!(!dir.path().join("pwned").exists(), "no shell involved");
    }

    #[tokio::test]
    async fn a_missing_program_or_a_failing_exit_is_a_failure() {
        let dir = lab().unwrap();
        let path = dir.path().as_os_str().to_owned();
        assert!(!send(&path, "x", &[]).await);
        script(dir.path(), "exit 1");
        assert!(!send(&path, "x", &[]).await);
    }

    #[tokio::test]
    async fn a_hung_program_is_killed_after_the_timeout() {
        let dir = lab().unwrap();
        script(dir.path(), "PATH=\"$PATH:/usr/bin:/bin\"\nsleep 30");
        let path = dir.path().as_os_str().to_owned();
        let started = std::time::Instant::now();
        assert!(!send(&path, "x", &[]).await);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn a_failed_background_send_raises_the_fallback_flag() {
        let dir = lab().unwrap();
        script(dir.path(), "exit 1");
        let flag = Arc::new(AtomicBool::new(false));
        spawn_send(Job {
            path: dir.path().as_os_str().to_owned(),
            body: "x".to_owned(),
            fallback: Some(Arc::clone(&flag)),
            busy: Arc::default(),
            session: here,
        });
        for _ in 0..100 {
            if flag.load(Ordering::Acquire) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("the fallback flag was never raised");
    }

    // W1: the child gets an allow-list, never the whole environment.
    #[test]
    fn the_allow_list_keeps_the_session_and_drops_everything_else() {
        let env = [
            ("PATH", "/evil"),
            ("HOME", "/home/u"),
            ("LANG", "C.UTF-8"),
            ("LC_ALL", "C"),
            ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1/bus"),
            ("DISPLAY", ":0"),
            ("WAYLAND_DISPLAY", "wayland-0"),
            ("XDG_RUNTIME_DIR", "/run/user/1"),
            ("XAUTHORITY", "/home/u/.Xauthority"),
            ("XDG_CURRENT_DESKTOP", "GNOME"),
            ("XDG_SESSION_TYPE", "wayland"),
            ("ANTHROPIC_API_KEY", "secret"),
            ("GITHUB_TOKEN", "secret"),
            ("LCX", "not a locale"),
        ]
        .map(|(k, v)| (OsString::from(k), OsString::from(v)));
        let kept: Vec<String> = allowed_env(env)
            .into_iter()
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            kept,
            [
                "HOME",
                "LANG",
                "LC_ALL",
                "DBUS_SESSION_BUS_ADDRESS",
                "DISPLAY",
                "WAYLAND_DISPLAY",
                "XDG_RUNTIME_DIR",
                "XAUTHORITY",
                "XDG_CURRENT_DESKTOP",
                "XDG_SESSION_TYPE"
            ]
        );
    }

    #[tokio::test]
    async fn the_child_sees_the_merged_path_and_the_allow_list_only() {
        let dir = lab().unwrap();
        let out = dir.path().join("env");
        script(dir.path(), &format!("/usr/bin/env > '{}'", out.display()));
        let path = dir.path().as_os_str().to_owned();
        let vars = vec![(OsString::from("LANG"), OsString::from("C"))];
        assert!(send(&path, "x", &vars).await);
        let dump = fs::read_to_string(&out).unwrap();
        assert!(
            dump.contains(&format!("PATH={}", dir.path().display())),
            "{dump}"
        );
        assert!(dump.contains("LANG=C"), "{dump}");
        // The test process has CARGO_MANIFEST_DIR; the child must not.
        assert!(!dump.contains("CARGO_MANIFEST_DIR"), "{dump}");
        assert!(!dump.contains("CARGO_PKG"), "{dump}");
    }

    // W2: the PATH is not trusted blindly.
    #[test]
    fn a_program_under_tmp_or_writable_by_others_is_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        script(tmp.path(), "exit 0");
        assert!(!available(&here(), tmp.path().as_os_str()), "under /tmp");

        let dir = lab().unwrap();
        script(dir.path(), "exit 0");
        let path = dir.path().as_os_str();
        assert!(available(&here(), path));
        fs::set_permissions(dir.path().join(PROGRAM), fs::Permissions::from_mode(0o757)).unwrap();
        assert!(!available(&here(), path), "file writable by others");
        fs::set_permissions(dir.path().join(PROGRAM), fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o775)).unwrap();
        assert!(!available(&here(), path), "folder writable by group");
    }

    #[test]
    fn an_untrusted_entry_does_not_hide_a_trusted_one_later() {
        let bad = tempfile::tempdir().unwrap();
        script(bad.path(), "exit 0");
        let good = lab().unwrap();
        script(good.path(), "exit 0");
        let path = std::env::join_paths([bad.path(), good.path()]).unwrap();
        assert!(available(&here(), &path));
    }

    // W5: SSH and no session bus mean no desktop.
    #[test]
    fn the_session_reads_display_ssh_and_the_bus() {
        fn get(set: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
            let set: Vec<(String, OsString)> = set
                .iter()
                .map(|(k, v)| ((*k).to_owned(), OsString::from(*v)))
                .collect();
            move |name: &str| set.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
        }
        let none = |_: &Path| false;
        let s = Session::probe(
            get(&[("WAYLAND_DISPLAY", "w0"), ("DBUS_SESSION_BUS_ADDRESS", "u")]),
            none,
        );
        assert_eq!(
            s,
            Session {
                display: true,
                ssh: false,
                bus: true
            }
        );
        let s = Session::probe(
            get(&[("DISPLAY", ":0"), ("XDG_RUNTIME_DIR", "/run/user/1")]),
            |p| p == Path::new("/run/user/1/bus"),
        );
        assert!(s.display && s.bus && !s.ssh);
        let s = Session::probe(
            get(&[("DISPLAY", ":0"), ("XDG_RUNTIME_DIR", "/run/user/1")]),
            none,
        );
        assert!(!s.bus, "no address and no socket");
        for name in ["SSH_CONNECTION", "SSH_TTY"] {
            let s = Session::probe(
                get(&[
                    ("DISPLAY", ":0"),
                    ("DBUS_SESSION_BUS_ADDRESS", "u"),
                    (name, "x"),
                ]),
                none,
            );
            assert!(s.ssh, "{name}");
        }
        let s = Session::probe(
            get(&[("DISPLAY", ""), ("DBUS_SESSION_BUS_ADDRESS", "")]),
            none,
        );
        assert!(!s.display && !s.bus, "empty values are unset");
    }

    // W3: one send in flight; the rest of a burst is dropped.
    #[tokio::test]
    async fn a_burst_runs_one_notify_send_at_a_time() {
        let dir = lab().unwrap();
        let log = dir.path().join("log");
        script(
            dir.path(),
            &format!(
                "echo x >> '{}'\nPATH=\"$PATH:/usr/bin:/bin\"\nsleep 1",
                log.display()
            ),
        );
        let busy = Arc::new(AtomicBool::new(false));
        let job = |busy: &Arc<AtomicBool>| Job {
            path: dir.path().as_os_str().to_owned(),
            body: "x".to_owned(),
            fallback: None,
            busy: Arc::clone(busy),
            session: here,
        };
        spawn_send(job(&busy));
        spawn_send(job(&busy));
        spawn_send(job(&busy));
        for _ in 0..100 {
            if !busy.load(Ordering::Acquire) && log.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(!busy.load(Ordering::Acquire), "released when done");
        assert_eq!(
            fs::read_to_string(&log).unwrap(),
            "x\n",
            "one send for the burst"
        );
        spawn_send(job(&busy));
        for _ in 0..100 {
            if fs::read_to_string(&log).is_ok_and(|t| t.lines().count() == 2) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("the next send after the burst never ran");
    }

    // W4: the probe runs in the task; an unavailable desktop is a failure.
    #[tokio::test]
    async fn an_unavailable_desktop_raises_the_fallback_inside_the_task() {
        let dir = lab().unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        spawn_send(Job {
            path: dir.path().as_os_str().to_owned(),
            body: "x".to_owned(),
            fallback: Some(Arc::clone(&flag)),
            busy: Arc::default(),
            session: here,
        });
        for _ in 0..100 {
            if flag.load(Ordering::Acquire) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("no program on the path must raise the fallback");
    }

    // Why the desktop is unavailable, in words for the test action.
    #[test]
    fn the_unavailable_reasons_are_short_and_ordered() {
        let dir = lab().unwrap();
        let path = dir.path().as_os_str().to_owned();
        let why = |session: &Session| unavailable_reason(session, &path);
        assert_eq!(
            why(&Session {
                ssh: true,
                ..here()
            })
            .as_deref(),
            Some("SSH session")
        );
        assert_eq!(
            why(&Session {
                display: false,
                ..here()
            })
            .as_deref(),
            Some("no display")
        );
        assert_eq!(
            why(&Session {
                bus: false,
                ..here()
            })
            .as_deref(),
            Some("no session bus")
        );
        assert_eq!(why(&here()).as_deref(), Some("no notify-send on PATH"));
        script(dir.path(), "exit 0");
        assert_eq!(why(&here()), None);
        fs::set_permissions(dir.path().join(PROGRAM), fs::Permissions::from_mode(0o757)).unwrap();
        assert_eq!(
            why(&here()).as_deref(),
            Some("notify-send on PATH is untrusted (under /tmp or writable by others)")
        );
    }

    // The sound: three players in order, one argv each, no shell.
    fn tool(dir: &Path, name: &str, body: &str) {
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    const OGA: &str = "/usr/share/sounds/freedesktop/stereo/message-new-instant.oga";
    const COMPLETE: &str = "/usr/share/sounds/freedesktop/stereo/complete.oga";

    fn plan_in(
        dir: &Path,
        files: &'static [&'static str],
    ) -> Result<(String, Vec<String>), String> {
        sound_plan(dir.as_os_str(), |f| files.iter().any(|x| Path::new(x) == f)).map(|(p, a)| {
            (
                p.file_name().unwrap().to_string_lossy().into_owned(),
                a.iter().map(|x| x.to_string_lossy().into_owned()).collect(),
            )
        })
    }

    #[test]
    fn canberra_comes_first_and_needs_no_file() {
        let dir = lab().unwrap();
        for name in ["pw-play", "paplay", "canberra-gtk-play"] {
            tool(dir.path(), name, "exit 0");
        }
        assert_eq!(
            plan_in(dir.path(), &[]),
            Ok((
                "canberra-gtk-play".to_owned(),
                vec!["-i".to_owned(), "message-new-instant".to_owned()]
            ))
        );
    }

    #[test]
    fn pw_play_then_paplay_take_the_first_existing_sound_file() {
        let dir = lab().unwrap();
        tool(dir.path(), "paplay", "exit 0");
        assert_eq!(
            plan_in(dir.path(), &[OGA]),
            Ok(("paplay".to_owned(), vec![OGA.to_owned()]))
        );
        assert_eq!(
            plan_in(dir.path(), &[COMPLETE]),
            Ok(("paplay".to_owned(), vec![COMPLETE.to_owned()])),
            "complete.oga when the first is missing"
        );
        assert_eq!(
            plan_in(dir.path(), &[COMPLETE, OGA]).unwrap().1,
            vec![OGA.to_owned()],
            "the first of the two wins"
        );
        tool(dir.path(), "pw-play", "exit 0");
        assert_eq!(plan_in(dir.path(), &[OGA]).unwrap().0, "pw-play");
    }

    #[test]
    fn no_player_or_no_file_is_no_sound() {
        let dir = lab().unwrap();
        assert_eq!(
            plan_in(dir.path(), &[OGA]),
            Err("no sound player on PATH".to_owned())
        );
        tool(dir.path(), "pw-play", "exit 0");
        assert_eq!(
            plan_in(dir.path(), &[]),
            Err("no sound file found".to_owned())
        );
    }

    #[tokio::test]
    async fn play_runs_the_planned_argv_with_the_scrubbed_environment() {
        let dir = lab().unwrap();
        let out = dir.path().join("argv");
        tool(
            dir.path(),
            "canberra-gtk-play",
            &format!(
                "printf '%s\\n' \"$@\" > '{}'; /usr/bin/env >> '{}'",
                out.display(),
                out.display()
            ),
        );
        let path = dir.path().as_os_str().to_owned();
        assert_eq!(play(&path, &[], |_| false).await, Ok(()));
        let dump = fs::read_to_string(&out).unwrap();
        assert!(dump.starts_with("-i\nmessage-new-instant\n"), "{dump}");
        assert!(!dump.contains("CARGO_MANIFEST_DIR"), "{dump}");
    }

    #[tokio::test]
    async fn a_failing_player_is_reported() {
        let dir = lab().unwrap();
        tool(dir.path(), "canberra-gtk-play", "exit 1");
        let path = dir.path().as_os_str().to_owned();
        assert_eq!(
            play(&path, &[], |_| false).await,
            Err("the sound player failed".to_owned())
        );
    }

    fn sound_job(dir: &Path, session: fn() -> Session, busy: &Arc<AtomicBool>) -> SoundJob {
        SoundJob {
            path: dir.as_os_str().to_owned(),
            busy: Arc::clone(busy),
            session,
        }
    }

    async fn wait_lines(log: &Path, n: usize) -> bool {
        for _ in 0..100 {
            if fs::read_to_string(log).is_ok_and(|t| t.lines().count() == n) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    #[tokio::test]
    async fn a_burst_of_sounds_plays_one_at_a_time() {
        let dir = lab().unwrap();
        let log = dir.path().join("log");
        tool(
            dir.path(),
            "canberra-gtk-play",
            &format!(
                "echo x >> '{}'\nPATH=\"$PATH:/usr/bin:/bin\"\nsleep 1",
                log.display()
            ),
        );
        let busy = Arc::new(AtomicBool::new(false));
        for _ in 0..3 {
            spawn_sound(sound_job(dir.path(), here, &busy));
        }
        assert!(wait_lines(&log, 1).await);
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(fs::read_to_string(&log).unwrap(), "x\n");
    }

    #[tokio::test]
    async fn no_sound_over_ssh_or_without_a_session() {
        let dir = lab().unwrap();
        let log = dir.path().join("log");
        tool(
            dir.path(),
            "canberra-gtk-play",
            &format!("echo x >> '{}'", log.display()),
        );
        let busy = Arc::new(AtomicBool::new(false));
        spawn_sound(sound_job(
            dir.path(),
            || Session {
                ssh: true,
                ..here()
            },
            &busy,
        ));
        spawn_sound(sound_job(
            dir.path(),
            || Session {
                bus: false,
                ..here()
            },
            &busy,
        ));
        spawn_sound(sound_job(
            dir.path(),
            || Session {
                display: false,
                ..here()
            },
            &busy,
        ));
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(!log.exists());
    }

    // The test action's report, end to end with fake tools.
    #[tokio::test]
    async fn the_test_report_covers_bell_desktop_and_sound() {
        let dir = lab().unwrap();
        let path = dir.path().as_os_str().to_owned();
        let report = |mode, sound| {
            let path = path.clone();
            async move { run_test(&path, &here(), mode, sound, &[]).await }
        };
        assert_eq!(
            report(NotifyMode::Auto, true).await,
            "sent: bell · desktop unavailable: no notify-send on PATH · no sound: no sound player on PATH"
        );
        script(dir.path(), "exit 0");
        tool(dir.path(), "canberra-gtk-play", "exit 0");
        assert_eq!(
            report(NotifyMode::Auto, true).await,
            "sent: bell + desktop + sound"
        );
        assert_eq!(report(NotifyMode::Desktop, false).await, "sent: desktop");
        assert_eq!(report(NotifyMode::Bell, true).await, "sent: bell");
        assert_eq!(report(NotifyMode::Off, true).await, "notify is off");
    }
}
