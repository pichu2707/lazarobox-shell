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
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use tokio::{process::Command, sync::mpsc, time::timeout};

use super::{
    ai_detect::{RunFailure, find_on_path, run_capped_why},
    chat::trusted_real_path,
};
use crate::core::ai::{
    chat::notify::{
        SoundEnd, bell_after_failed_desktop, describe_test, plays_sound, wants_desktop,
    },
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
const ALLOWED_ENV: [&str; 20] = [
    "HOME",
    "LANG",
    "DBUS_SESSION_BUS_ADDRESS",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XDG_RUNTIME_DIR",
    "XAUTHORITY",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_TYPE",
    // The audio servers the sound players talk to.
    "PULSE_SERVER",
    "PULSE_RUNTIME_PATH",
    "PIPEWIRE_RUNTIME_DIR",
    "PIPEWIRE_REMOTE",
    // Where the sound themes and the locale data are looked up.
    "XDG_DATA_DIRS",
    "XDG_DATA_HOME",
    "XDG_CONFIG_HOME",
    "XDG_CONFIG_DIRS",
    "LANGUAGE",
    "USER",
    "LOGNAME",
];

/// The facts of the session that decide whether a desktop notification can
/// show up at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Session {
    /// `DISPLAY` or `WAYLAND_DISPLAY` is set.
    pub display: bool,
    /// Running over SSH (an SSH variable is set and the display is not a
    /// local one): the notification would pop up on a machine the user is not
    /// at (or nowhere).
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
        // A local X display is `:N` with no host. A stale SSH variable in a
        // long-lived local tmux server must not disable the desktop, and a
        // forwarded `localhost:N` display must not count as local.
        let local_display = set("WAYLAND_DISPLAY")
            || get("DISPLAY").is_some_and(|d| d.to_string_lossy().starts_with(':'));
        Self {
            display: set("DISPLAY") || set("WAYLAND_DISPLAY"),
            ssh: (set("SSH_CONNECTION") || set("SSH_TTY")) && !local_display,
            bus,
        }
    }

    /// The session of this process.
    pub fn current() -> Self {
        Self::probe(|name| std::env::var_os(name), |p| p.exists())
    }
}

/// The real path of the first program called `name` on `path` that is
/// trusted: a regular executable whose real path is not under a scratch folder and that neither it
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
        if let Some(real) = trusted_real_path(&file) {
            return Ok(real);
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
    desktop_plan(session, path).err()
}

/// The real path of the `notify-send` to run, or why there is none.
pub fn desktop_plan(session: &Session, path: &OsStr) -> Result<PathBuf, String> {
    if session.ssh {
        return Err("SSH session".to_owned());
    }
    if !session.display {
        return Err("no display".to_owned());
    }
    if !session.bus {
        return Err("no session bus".to_owned());
    }
    program_state(PROGRAM, path)
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

/// Longest a probe (session, trust, file checks) may take. A stalled disk
/// must not hold anything up.
const PROBE_TIMEOUT: Duration = Duration::from_secs(1);

/// Longest a whole desktop send may take, probe included (1 s probe, 2 s run).
const SEND_TIMEOUT: Duration = Duration::from_secs(4);

/// Longest the sound chain may take: at most five candidates (canberra, then
/// two files for each of `pw-play` and `paplay`), each bounded by [`TIMEOUT`].
const SOUND_STEP: Duration = Duration::from_secs(10);

/// Longest a whole background sound may take, probe included.
const SOUND_TIMEOUT: Duration = Duration::from_secs(12);

/// Longest the whole test action may take: the sum of its steps (desktop
/// probe and send, sound probe and chain) plus a margin. Every step has its
/// own bound, so this one only catches what they cannot.
const TEST_TIMEOUT: Duration = Duration::from_secs(16);

/// How long a good probe result is reused, so a normal turn does not stat the
/// `PATH` every time.
const PROBE_TTL: Duration = Duration::from_secs(60);

/// Raises its flag when the probe closure ends, a panic included.
struct Done(Arc<AtomicBool>);

impl Drop for Done {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

type Slot<T> = Mutex<Option<(Instant, OsString, T)>>;

/// The probes, with their guard rails. Healthy probes run side by side. A
/// probe that outlives [`PROBE_TIMEOUT`] marks the probes as stuck until its
/// thread really ends, and new probes then say so instead of piling up
/// blocking threads. Good results are cached for a minute, keyed by the
/// `PATH` they were probed with, and re-checked (the cheap trust check on the
/// cached real paths) before each use; failures are never cached.
pub struct Probe {
    /// The end flag of the probe thread that timed out, while it may still run.
    stuck: Mutex<Option<Arc<AtomicBool>>>,
    ttl: Duration,
    desktop: Slot<PathBuf>,
    sound: Slot<Vec<Candidate>>,
}

impl Default for Probe {
    fn default() -> Self {
        Self::with_ttl(PROBE_TTL)
    }
}

impl Probe {
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            stuck: Mutex::new(None),
            ttl,
            desktop: Mutex::new(None),
            sound: Mutex::new(None),
        }
    }

    fn is_stuck(&self) -> bool {
        self.stuck
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
            .is_some_and(|done| !done.load(Ordering::Acquire))
    }

    /// Runs `f` on a blocking thread and waits at most [`PROBE_TIMEOUT`].
    async fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, String> {
        if self.is_stuck() {
            return Err("PATH lookup still running".to_owned());
        }
        let done = Arc::new(AtomicBool::new(false));
        let guard = Done(Arc::clone(&done));
        let task = tokio::task::spawn_blocking(move || {
            let _guard = guard;
            f()
        });
        match timeout(PROBE_TIMEOUT, task).await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(_)) => Err("the check failed".to_owned()),
            Err(_) => {
                if let Ok(mut slot) = self.stuck.lock() {
                    *slot = Some(done);
                }
                Err("the check timed out".to_owned())
            }
        }
    }

    fn fresh<T: Clone>(&self, slot: &Slot<T>, path: &OsStr) -> Option<T> {
        let slot = slot.lock().ok()?;
        slot.as_ref()
            .filter(|(at, key, _)| at.elapsed() < self.ttl && key == path)
            .map(|(_, _, value)| value.clone())
    }

    fn store<T>(slot: &Slot<T>, path: &OsStr, value: T) {
        if let Ok(mut slot) = slot.lock() {
            *slot = Some((Instant::now(), path.to_owned(), value));
        }
    }

    /// The `notify-send` to run, or why there is none. `cached` reuses a good
    /// result from the last minute, after checking it is still trusted.
    pub async fn desktop(
        &self,
        session: fn() -> Session,
        path: &OsStr,
        cached: bool,
    ) -> Result<PathBuf, String> {
        let hit = cached.then(|| self.fresh(&self.desktop, path)).flatten();
        let owned = path.to_owned();
        let program = self
            .run(move || match hit {
                Some(p) if trusted_real_path(&p).as_ref() == Some(&p) => Ok(p),
                _ => desktop_plan(&session(), &owned),
            })
            .await??;
        Self::store(&self.desktop, path, program.clone());
        Ok(program)
    }

    /// The ways to play the sound, or why there are none (also: no desktop
    /// session). `cached` as in [`Probe::desktop`].
    pub async fn sound(
        &self,
        session: fn() -> Session,
        path: &OsStr,
        cached: bool,
    ) -> Result<Vec<Candidate>, String> {
        let hit = cached.then(|| self.fresh(&self.sound, path)).flatten();
        let owned = path.to_owned();
        let plan = self
            .run(move || {
                if let Some(plan) = hit
                    && plan
                        .iter()
                        .all(|c| trusted_real_path(&c.program).as_ref() == Some(&c.program))
                {
                    return Ok(plan);
                }
                let session = session();
                if session.ssh || !session.display || !session.bus {
                    return Err("no desktop session".to_owned());
                }
                sound_plan(&owned, |f| f.exists())
            })
            .await??;
        Self::store(&self.sound, path, plan.clone());
        Ok(plan)
    }
}

/// Runs `program` (a checked real path) as `name`, with `args`, with an empty
/// environment but `PATH` and `vars`. `Ok` when it exited 0 inside the timeout;
/// the failure says if it never started, failed, or was still running. The child has no stdin or stderr, runs in its own
/// process group, and is killed and reaped on every way out.
async fn run_tool(
    program: &Path,
    name: &str,
    args: &[OsString],
    path: &OsStr,
    vars: &[(OsString, OsString)],
) -> Result<(), RunFailure> {
    run_capped_why(
        || {
            let mut command = Command::new(program);
            // The name it was found under, for a multi-call binary.
            command
                .arg0(name)
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
    .map(|_| ())
}

/// Runs the checked `notify-send` at `program` with `body`.
async fn send_via(program: &Path, path: &OsStr, body: &str, vars: &[(OsString, OsString)]) -> bool {
    // `--` so a body can never be read as an option.
    let args: Vec<OsString> = ["--app-name", TITLE, "--", TITLE]
        .iter()
        .map(OsString::from)
        .chain([OsString::from(clean(body))])
        .collect();
    run_tool(program, PROGRAM, &args, path, vars).await.is_ok()
}

/// Runs the trusted `notify-send` on `path` with `body`.
pub async fn send(path: &OsStr, body: &str, vars: &[(OsString, OsString)]) -> bool {
    let Some(program) = trusted_program(path) else {
        return false;
    };
    send_via(&program, path, body, vars).await
}

/// The sound files, in order of preference.
const SOUND_FILES: [&str; 2] = [
    "/usr/share/sounds/freedesktop/stereo/message-new-instant.oga",
    "/usr/share/sounds/freedesktop/stereo/complete.oga",
];

/// The sound players, in order. `canberra-gtk-play` plays by event name; the
/// others need a file.
const PLAYERS: [&str; 3] = ["canberra-gtk-play", "pw-play", "paplay"];

/// One way to play the sound.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Candidate {
    /// The checked real path to run.
    pub program: PathBuf,
    /// The name it was found under (argv[0]).
    pub name: &'static str,
    pub args: Vec<OsString>,
}

/// Every way to play the sound, in the order to try them: canberra, then each
/// file-based player with each existing sound file. Only trusted players on
/// `path` appear (`exists` says which sound files exist).
pub fn sound_plan(path: &OsStr, exists: impl Fn(&Path) -> bool) -> Result<Vec<Candidate>, String> {
    let files: Vec<&Path> = SOUND_FILES
        .iter()
        .map(Path::new)
        .filter(|f| exists(f))
        .collect();
    let mut found = false;
    let mut needed_file = false;
    let mut plan = Vec::new();
    for name in PLAYERS {
        let Ok(program) = program_state(name, path) else {
            continue;
        };
        found = true;
        if name == "canberra-gtk-play" {
            plan.push(Candidate {
                program,
                name,
                args: vec!["-i".into(), "message-new-instant".into()],
            });
        } else if files.is_empty() {
            needed_file = true;
        } else {
            plan.extend(files.iter().map(|file| Candidate {
                program: program.clone(),
                name,
                args: vec![file.as_os_str().to_owned()],
            }));
        }
    }
    if !plan.is_empty() {
        return Ok(plan);
    }
    Err(if found && needed_file {
        "no sound file found".to_owned()
    } else {
        "no sound player on PATH".to_owned()
    })
}

/// Tries the candidates in order until one plays. A player that exits 0, or
/// is still running at the limit (it started, so it is playing), ends the
/// chain; only a spawn error or a failing exit moves on to the next one.
async fn play_plan(
    plan: &[Candidate],
    path: &OsStr,
    vars: &[(OsString, OsString)],
) -> Result<SoundEnd, String> {
    for candidate in plan {
        match run_tool(
            &candidate.program,
            candidate.name,
            &candidate.args,
            path,
            vars,
        )
        .await
        {
            Ok(()) => return Ok(SoundEnd::Played),
            Err(RunFailure::Timeout) => return Ok(SoundEnd::StillPlaying),
            Err(_) => {}
        }
    }
    Err("the sound player failed".to_owned())
}

/// Plays the sound. The error says why not.
pub async fn play(
    path: &OsStr,
    vars: &[(OsString, OsString)],
    exists: impl Fn(&Path) -> bool,
) -> Result<SoundEnd, String> {
    play_plan(&sound_plan(path, exists)?, path, vars).await
}

/// One desktop notification to send in the background.
pub struct Job {
    /// The merged login `PATH`.
    pub path: OsString,
    pub body: String,
    /// Raised when the notification cannot be sent, fails, or is dropped
    /// because another is in flight, so the next flush rings the bell.
    pub fallback: Option<Arc<AtomicBool>>,
    /// Set while a send is in flight: a job that finds it set is dropped.
    pub busy: Arc<AtomicBool>,
    /// Reads the session; run inside the task.
    pub session: fn() -> Session,
    /// The shared probe: one stuck thread at most, and a cache.
    pub probe: Arc<Probe>,
}

/// Clears the in-flight flag on every way out: a panic, a timeout, or a task
/// that is dropped before it ever ran.
struct Release(Arc<AtomicBool>);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Sends in the background and returns at once, without touching the disk.
/// At most one send is in flight: a burst of finished turns shows one
/// notification and the rest are dropped. A dropped send still raises the
/// fallback (so `desktop` mode rings; `auto` has none, its bell already
/// rang). An unavailable or hung desktop counts as a failed send. The whole
/// task is bounded, and the in-flight guard exists before the task does.
pub fn spawn_send(job: Job) {
    if job.busy.swap(true, Ordering::AcqRel) {
        if let Some(flag) = &job.fallback {
            flag.store(true, Ordering::Release);
        }
        return;
    }
    let release = Release(Arc::clone(&job.busy));
    tokio::spawn(async move {
        let _release = release;
        let sent = timeout(SEND_TIMEOUT, available_then_send(&job))
            .await
            .unwrap_or(false);
        if !sent && let Some(flag) = &job.fallback {
            flag.store(true, Ordering::Release);
        }
    });
}

async fn available_then_send(job: &Job) -> bool {
    let Ok(program) = job.probe.desktop(job.session, &job.path, true).await else {
        return false;
    };
    send_via(
        &program,
        &job.path,
        &job.body,
        &allowed_env(std::env::vars_os()),
    )
    .await
}

/// One sound to play in the background.
pub struct SoundJob {
    /// The merged login `PATH`.
    pub path: OsString,
    /// Set while a sound is in flight: a job that finds it set is dropped.
    pub busy: Arc<AtomicBool>,
    /// Reads the session; run inside the task.
    pub session: fn() -> Session,
    /// The shared probe: one stuck thread at most, and a cache.
    pub probe: Arc<Probe>,
}

/// Plays in the background and returns at once, with the same rules as
/// [`spawn_send`]: one in flight, bounded, nothing over SSH or without a
/// session, and nothing at all when no player works.
pub fn spawn_sound(job: SoundJob) {
    if job.busy.swap(true, Ordering::AcqRel) {
        return;
    }
    let release = Release(Arc::clone(&job.busy));
    tokio::spawn(async move {
        let _release = release;
        let _ = timeout(SOUND_TIMEOUT, async {
            let plan = job.probe.sound(job.session, &job.path, true).await.ok()?;
            let vars = allowed_env(std::env::vars_os());
            timeout(SOUND_STEP, play_plan(&plan, &job.path, &vars))
                .await
                .ok()?
                .ok()
        })
        .await;
    });
}

/// What the test action found.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TestReport {
    /// The footer line.
    pub text: String,
    /// The desktop notification was wanted and did not happen.
    pub desktop_failed: bool,
}

/// What the test action does and reports, with the focus rule ignored. The
/// bell is the caller's; this attempts the desktop and the sound the mode
/// asks for and describes the result. Each step has its own bound, so a slow
/// sound never hides the desktop outcome.
pub async fn run_test(
    path: &OsStr,
    probe: &Probe,
    session: fn() -> Session,
    mode: NotifyMode,
    sound: bool,
    vars: &[(OsString, OsString)],
    sound_step: Duration,
) -> TestReport {
    let desktop = if wants_desktop(mode) {
        Some(match probe.desktop(session, path, false).await {
            Err(why) => Err(why),
            Ok(program) => {
                match timeout(
                    TIMEOUT + PROBE_TIMEOUT,
                    send_via(&program, path, "Test notification", vars),
                )
                .await
                {
                    Ok(true) => Ok(()),
                    _ => Err("notify-send failed".to_owned()),
                }
            }
        })
    } else {
        None
    };
    let sound = if plays_sound(mode, sound) {
        Some(match probe.sound(session, path, false).await {
            Err(why) => Err(why),
            Ok(plan) => match timeout(sound_step, play_plan(&plan, path, vars)).await {
                Ok(result) => result,
                Err(_) => Err("the sound step timed out".to_owned()),
            },
        })
    } else {
        None
    };
    let desktop_failed = matches!(desktop, Some(Err(_)));
    TestReport {
        text: describe_test(mode, desktop, sound),
        desktop_failed,
    }
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
    /// Set while a test runs: a second one only says "test running".
    pub busy: Arc<AtomicBool>,
    /// The shared probe: one stuck thread at most, and a cache.
    pub probe: Arc<Probe>,
}

/// Runs the test in the background and sends the report line. One at a time;
/// the in-flight guard is dropped before the report goes out, so a report
/// never arrives while the flag still says "running".
pub fn spawn_test(job: TestJob) {
    if job.busy.swap(true, Ordering::AcqRel) {
        let _ = job.report.try_send("test running".to_owned());
        return;
    }
    let release = Release(Arc::clone(&job.busy));
    tokio::spawn(async move {
        let vars = allowed_env(std::env::vars_os());
        let run = run_test(
            &job.path,
            &job.probe,
            job.session,
            job.mode,
            job.sound,
            &vars,
            SOUND_STEP,
        );
        let (text, desktop_failed) = match timeout(TEST_TIMEOUT, run).await {
            Ok(report) => (report.text, report.desktop_failed),
            // The backstop: the steps bound themselves, so this is not
            // expected. Assume the worst for the desktop.
            Err(_) => ("the check timed out".to_owned(), true),
        };
        if desktop_failed
            && bell_after_failed_desktop(job.mode)
            && let Some(flag) = &job.fallback
        {
            flag.store(true, Ordering::Release);
        }
        drop(release);
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
            probe: Arc::default(),
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
            ("PULSE_SERVER", "unix:/run/pulse"),
            ("PULSE_RUNTIME_PATH", "/run/user/1/pulse"),
            ("PIPEWIRE_RUNTIME_DIR", "/run/user/1"),
            ("PIPEWIRE_REMOTE", "pipewire-0"),
            ("XDG_DATA_DIRS", "/usr/share"),
            ("XDG_DATA_HOME", "/home/u/.local/share"),
            ("XDG_CONFIG_HOME", "/home/u/.config"),
            ("XDG_CONFIG_DIRS", "/etc/xdg"),
            ("LANGUAGE", "en"),
            ("USER", "u"),
            ("LOGNAME", "u"),
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
                "XDG_SESSION_TYPE",
                "PULSE_SERVER",
                "PULSE_RUNTIME_PATH",
                "PIPEWIRE_RUNTIME_DIR",
                "PIPEWIRE_REMOTE",
                "XDG_DATA_DIRS",
                "XDG_DATA_HOME",
                "XDG_CONFIG_HOME",
                "XDG_CONFIG_DIRS",
                "LANGUAGE",
                "USER",
                "LOGNAME"
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
                    ("DISPLAY", "localhost:10.0"),
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
            probe: Arc::default(),
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
            probe: Arc::default(),
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

    type Plan = Vec<(String, Vec<String>)>;

    fn plan_in(dir: &Path, files: &'static [&'static str]) -> Result<Plan, String> {
        sound_plan(dir.as_os_str(), |f| files.iter().any(|x| Path::new(x) == f)).map(|list| {
            list.into_iter()
                .map(|c| {
                    (
                        c.name.to_owned(),
                        c.args
                            .iter()
                            .map(|x| x.to_string_lossy().into_owned())
                            .collect(),
                    )
                })
                .collect()
        })
    }

    fn cand(name: &str, args: &[&str]) -> (String, Vec<String>) {
        (
            name.to_owned(),
            args.iter().map(|a| (*a).to_owned()).collect(),
        )
    }

    #[test]
    fn the_candidates_run_canberra_then_pw_play_then_paplay_with_both_files() {
        let dir = lab().unwrap();
        for name in ["pw-play", "paplay", "canberra-gtk-play"] {
            tool(dir.path(), name, "exit 0");
        }
        assert_eq!(
            plan_in(dir.path(), &[COMPLETE, OGA]).unwrap(),
            vec![
                cand("canberra-gtk-play", &["-i", "message-new-instant"]),
                cand("pw-play", &[OGA]),
                cand("pw-play", &[COMPLETE]),
                cand("paplay", &[OGA]),
                cand("paplay", &[COMPLETE]),
            ]
        );
    }

    #[test]
    fn a_missing_file_or_player_drops_only_its_candidates() {
        let dir = lab().unwrap();
        tool(dir.path(), "paplay", "exit 0");
        assert_eq!(
            plan_in(dir.path(), &[OGA]).unwrap(),
            vec![cand("paplay", &[OGA])]
        );
        assert_eq!(
            plan_in(dir.path(), &[COMPLETE]).unwrap(),
            vec![cand("paplay", &[COMPLETE])],
            "complete.oga when the first is missing"
        );
        tool(dir.path(), "canberra-gtk-play", "exit 0");
        assert_eq!(
            plan_in(dir.path(), &[]).unwrap().len(),
            1,
            "canberra needs no file"
        );
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

    // W4: a failing player falls through to the next one; the first success stops.
    #[tokio::test]
    async fn a_failing_player_falls_through_and_the_first_success_stops_the_chain() {
        let dir = lab().unwrap();
        let log = dir.path().join("log");
        let note = |name: &str, code: u8| {
            tool(
                dir.path(),
                name,
                &format!("echo {name} >> '{}'; exit {code}", log.display()),
            );
        };
        note("canberra-gtk-play", 1);
        note("pw-play", 1);
        note("paplay", 0);
        let path = dir.path().as_os_str().to_owned();
        assert_eq!(
            play(&path, &[], |f| f == Path::new(OGA)).await,
            Ok(SoundEnd::Played)
        );
        assert_eq!(
            fs::read_to_string(&log).unwrap(),
            "canberra-gtk-play\npw-play\npaplay\n"
        );
        fs::remove_file(&log).unwrap();
        note("canberra-gtk-play", 0);
        assert_eq!(play(&path, &[], |_| true).await, Ok(SoundEnd::Played));
        assert_eq!(
            fs::read_to_string(&log).unwrap(),
            "canberra-gtk-play\n",
            "stops at the first success"
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
        assert_eq!(play(&path, &[], |_| false).await, Ok(SoundEnd::Played));
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
        tool(dir.path(), "pw-play", "exit 1");
        assert_eq!(
            play(&path, &[], |_| true).await,
            Err("the sound player failed".to_owned()),
            "every candidate failed"
        );
    }

    fn sound_job(dir: &Path, session: fn() -> Session, busy: &Arc<AtomicBool>) -> SoundJob {
        SoundJob {
            path: dir.as_os_str().to_owned(),
            busy: Arc::clone(busy),
            probe: Arc::default(),
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
            async move { run_test(&path, &Probe::default(), here, mode, sound, &[], SOUND_STEP).await }
        };
        let r = report(NotifyMode::Auto, true).await;
        assert_eq!(
            r.text,
            "sent: bell · desktop unavailable: no notify-send on PATH · no sound: no sound player on PATH"
        );
        assert!(r.desktop_failed);
        script(dir.path(), "exit 0");
        tool(dir.path(), "canberra-gtk-play", "exit 0");
        let r = report(NotifyMode::Auto, true).await;
        assert_eq!(r.text, "sent: bell + desktop + sound");
        assert!(!r.desktop_failed);
        assert_eq!(
            report(NotifyMode::Desktop, false).await.text,
            "sent: desktop"
        );
        assert_eq!(report(NotifyMode::Bell, true).await.text, "sent: bell");
        assert_eq!(report(NotifyMode::Off, true).await.text, "notify is off");
    }

    // S1: one test at a time; a second says so, structurally.
    #[tokio::test]
    async fn a_second_test_while_one_runs_reports_test_running() {
        let dir = lab().unwrap();
        script(dir.path(), "PATH=\"$PATH:/usr/bin:/bin\"\nsleep 1");
        let busy = Arc::new(AtomicBool::new(false));
        let (tx, mut rx) = mpsc::channel(4);
        let job = || TestJob {
            path: dir.path().as_os_str().to_owned(),
            mode: NotifyMode::Desktop,
            sound: false,
            fallback: None,
            report: tx.clone(),
            session: here,
            busy: Arc::clone(&busy),
            probe: Arc::default(),
        };
        spawn_test(job());
        spawn_test(job());
        assert_eq!(rx.recv().await.unwrap(), "test running");
        assert_eq!(rx.recv().await.unwrap(), "sent: desktop");
        assert!(!busy.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn a_failed_desktop_test_arms_the_fallback_without_parsing_text() {
        let dir = lab().unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        let (tx, mut rx) = mpsc::channel(4);
        spawn_test(TestJob {
            path: dir.path().as_os_str().to_owned(),
            mode: NotifyMode::Desktop,
            sound: false,
            fallback: Some(Arc::clone(&flag)),
            report: tx,
            session: here,
            busy: Arc::default(),
            probe: Arc::default(),
        });
        assert!(rx.recv().await.unwrap().contains("unavailable"));
        assert!(flag.load(Ordering::Acquire));
    }

    // W1: a probe that never returns must not stick the in-flight flag.
    #[tokio::test]
    async fn a_hung_probe_releases_the_flag_and_later_sends_still_work() {
        let dir = lab().unwrap();
        let log = dir.path().join("log");
        script(dir.path(), &format!("echo x >> '{}'", log.display()));
        let busy = Arc::new(AtomicBool::new(false));
        let flag = Arc::new(AtomicBool::new(false));
        let job = |session: fn() -> Session, fallback, probe: &Arc<Probe>| Job {
            path: dir.path().as_os_str().to_owned(),
            body: "x".to_owned(),
            fallback,
            busy: Arc::clone(&busy),
            probe: Arc::clone(probe),
            session,
        };
        spawn_send(job(
            || {
                std::thread::sleep(Duration::from_secs(3));
                here()
            },
            Some(Arc::clone(&flag)),
            &Arc::default(),
        ));
        for _ in 0..100 {
            if !busy.load(Ordering::Acquire) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            !busy.load(Ordering::Acquire),
            "released after the probe timeout"
        );
        assert!(
            flag.load(Ordering::Acquire),
            "a hung probe counts as unavailable"
        );
        spawn_send(job(here, None, &Arc::default()));
        assert!(wait_lines(&log, 1).await, "the next send went through");
    }

    // W1 (third review): a stuck probe blocks new probes instead of piling up threads.
    #[tokio::test]
    async fn a_stuck_probe_is_not_spawned_again_until_it_ends() {
        static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        fn slow() -> Session {
            CALLS.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(1800));
            here()
        }
        let dir = lab().unwrap();
        script(dir.path(), "exit 0");
        let path = dir.path().as_os_str().to_owned();
        let probe = Probe::default();
        assert_eq!(
            probe.desktop(slow, &path, true).await,
            Err("the check timed out".to_owned())
        );
        assert_eq!(
            probe.desktop(slow, &path, true).await,
            Err("PATH lookup still running".to_owned())
        );
        assert_eq!(
            probe.sound(slow, &path, true).await.unwrap_err(),
            "PATH lookup still running"
        );
        assert_eq!(CALLS.load(Ordering::SeqCst), 1, "no second thread");
        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert!(probe.desktop(here, &path, true).await.is_ok(), "free again");
    }

    // W1: a good probe is cached for the TTL; a bad one never is.
    #[tokio::test]
    async fn a_good_probe_is_cached_and_a_failure_is_not() {
        static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        fn counted() -> Session {
            CALLS.fetch_add(1, Ordering::SeqCst);
            here()
        }
        let dir = lab().unwrap();
        let path = dir.path().as_os_str().to_owned();
        let probe = Probe::with_ttl(Duration::from_secs(60));
        assert!(probe.desktop(counted, &path, true).await.is_err());
        script(dir.path(), "exit 0");
        assert!(
            probe.desktop(counted, &path, true).await.is_ok(),
            "a failure was not cached"
        );
        assert_eq!(CALLS.load(Ordering::SeqCst), 2);
        assert!(probe.desktop(counted, &path, true).await.is_ok());
        assert_eq!(
            CALLS.load(Ordering::SeqCst),
            2,
            "served from the cache: no new probe"
        );
        assert!(probe.desktop(counted, &path, false).await.is_ok());
        assert_eq!(
            CALLS.load(Ordering::SeqCst),
            3,
            "the test action looks again"
        );
        let cold = Probe::with_ttl(Duration::ZERO);
        assert!(cold.desktop(counted, &path, true).await.is_ok());
        assert!(cold.desktop(counted, &path, true).await.is_ok());
        assert_eq!(CALLS.load(Ordering::SeqCst), 5, "expired at once");
    }

    // W2 (fourth review): a cached path is re-checked before it runs.
    #[tokio::test]
    async fn a_cached_path_that_stopped_being_trusted_is_refreshed_away() {
        let dir = lab().unwrap();
        script(dir.path(), "exit 0");
        let path = dir.path().as_os_str().to_owned();
        let probe = Probe::default();
        assert!(probe.desktop(here, &path, true).await.is_ok());
        fs::set_permissions(dir.path().join(PROGRAM), fs::Permissions::from_mode(0o757)).unwrap();
        assert_eq!(
            probe.desktop(here, &path, true).await,
            Err("notify-send on PATH is untrusted (under /tmp or writable by others)".to_owned())
        );
        fs::set_permissions(dir.path().join(PROGRAM), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(probe.desktop(here, &path, true).await.is_ok());
        fs::remove_file(dir.path().join(PROGRAM)).unwrap();
        assert!(
            probe.desktop(here, &path, true).await.is_err(),
            "a removed file is not served"
        );
    }

    #[tokio::test]
    async fn a_cached_sound_plan_is_rechecked_too() {
        let dir = lab().unwrap();
        tool(dir.path(), "canberra-gtk-play", "exit 0");
        let path = dir.path().as_os_str().to_owned();
        let probe = Probe::default();
        assert!(probe.sound(here, &path, true).await.is_ok());
        fs::set_permissions(
            dir.path().join("canberra-gtk-play"),
            fs::Permissions::from_mode(0o757),
        )
        .unwrap();
        assert!(probe.sound(here, &path, true).await.is_err());
    }

    // W3 (fourth review): the cache belongs to the PATH it was probed with.
    #[tokio::test]
    async fn the_cache_is_keyed_by_the_path() {
        let one = lab().unwrap();
        let two = lab().unwrap();
        script(one.path(), "exit 0");
        let probe = Probe::default();
        assert!(
            probe
                .desktop(here, one.path().as_os_str(), true)
                .await
                .is_ok()
        );
        assert!(
            probe
                .desktop(here, two.path().as_os_str(), true)
                .await
                .is_err(),
            "another PATH is not served from the first one's cache"
        );
    }

    // W1 (fourth review): healthy concurrent probes both run.
    #[tokio::test]
    async fn desktop_and_sound_probed_together_on_a_cold_cache_both_succeed() {
        let dir = lab().unwrap();
        script(dir.path(), "exit 0");
        tool(dir.path(), "canberra-gtk-play", "exit 0");
        let path = dir.path().as_os_str().to_owned();
        let probe = Probe::default();
        let (desktop, sound) = tokio::join!(
            probe.desktop(here, &path, true),
            probe.sound(here, &path, true)
        );
        assert!(desktop.is_ok(), "{desktop:?}");
        assert!(sound.is_ok(), "{sound:?}");
    }

    // W6 (fourth review): the test report tells a player still running apart.
    #[tokio::test]
    async fn the_test_report_says_when_the_sound_is_still_playing() {
        let dir = lab().unwrap();
        script(dir.path(), "exit 0");
        tool(
            dir.path(),
            "canberra-gtk-play",
            "PATH=\"$PATH:/usr/bin:/bin\"\nsleep 30",
        );
        let path = dir.path().as_os_str().to_owned();
        let report = run_test(
            &path,
            &Probe::default(),
            here,
            NotifyMode::Auto,
            true,
            &[],
            SOUND_STEP,
        )
        .await;
        assert_eq!(
            report.text,
            "sent: bell + desktop · sound: still playing after 2 s"
        );
    }

    #[tokio::test]
    async fn the_sound_plan_is_cached_too() {
        static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        fn counted() -> Session {
            CALLS.fetch_add(1, Ordering::SeqCst);
            here()
        }
        let dir = lab().unwrap();
        let path = dir.path().as_os_str().to_owned();
        tool(dir.path(), "canberra-gtk-play", "exit 0");
        let probe = Probe::default();
        assert_eq!(probe.sound(counted, &path, true).await.unwrap().len(), 1);
        assert_eq!(probe.sound(counted, &path, true).await.unwrap().len(), 1);
        assert_eq!(
            CALLS.load(Ordering::SeqCst),
            1,
            "the second came from the cache"
        );
        fs::remove_file(dir.path().join("canberra-gtk-play")).unwrap();
        assert!(probe.sound(counted, &path, false).await.is_err());
    }

    // W2: the sound step times out on its own and keeps the desktop outcome.
    #[tokio::test]
    async fn a_sound_timeout_keeps_the_desktop_outcome_and_the_fallback_flag() {
        let dir = lab().unwrap();
        tool(
            dir.path(),
            "canberra-gtk-play",
            "PATH=\"$PATH:/usr/bin:/bin\"\nsleep 30",
        );
        let path = dir.path().as_os_str().to_owned();
        let report = run_test(
            &path,
            &Probe::default(),
            here,
            NotifyMode::Desktop,
            true,
            &[],
            Duration::from_millis(300),
        )
        .await;
        assert_eq!(
            report.text,
            "sent: bell · desktop unavailable: no notify-send on PATH · no sound: the sound step timed out"
        );
        assert!(report.desktop_failed);
    }

    #[test]
    fn the_test_bound_covers_the_sum_of_its_steps() {
        // desktop: probe + send; sound: probe + the chain; plus a margin.
        assert!(
            TEST_TIMEOUT
                >= PROBE_TIMEOUT + TIMEOUT + PROBE_TIMEOUT + SOUND_STEP + Duration::from_secs(1)
        );
        // At most five candidates, each bounded by the per-tool timeout.
        assert!(SOUND_STEP >= TIMEOUT * 5);
        assert!(SOUND_TIMEOUT >= PROBE_TIMEOUT + SOUND_STEP);
    }

    // W3 (third review): a player still running at the limit did start; do not play twice.
    #[tokio::test]
    async fn a_slow_player_counts_as_played_and_ends_the_chain() {
        let dir = lab().unwrap();
        let log = dir.path().join("log");
        tool(
            dir.path(),
            "canberra-gtk-play",
            "PATH=\"$PATH:/usr/bin:/bin\"\nsleep 30",
        );
        tool(
            dir.path(),
            "pw-play",
            &format!("echo pw >> '{}'", log.display()),
        );
        let path = dir.path().as_os_str().to_owned();
        assert_eq!(play(&path, &[], |_| true).await, Ok(SoundEnd::StillPlaying));
        assert!(!log.exists(), "the next candidate never ran");
    }

    #[tokio::test]
    async fn a_player_that_cannot_start_falls_through() {
        let dir = lab().unwrap();
        let log = dir.path().join("log");
        // Executable bit set but not a program the kernel can run.
        let bad = dir.path().join("canberra-gtk-play");
        fs::write(&bad, "\u{0}garbage").unwrap();
        fs::set_permissions(&bad, fs::Permissions::from_mode(0o755)).unwrap();
        tool(
            dir.path(),
            "pw-play",
            &format!("echo pw >> '{}'", log.display()),
        );
        let path = dir.path().as_os_str().to_owned();
        assert_eq!(play(&path, &[], |_| true).await, Ok(SoundEnd::Played));
        assert_eq!(fs::read_to_string(&log).unwrap(), "pw\n");
    }

    // W4 (third review): the guard is gone before the report is out.
    #[tokio::test]
    async fn the_test_flag_is_free_by_the_time_the_report_arrives() {
        let dir = lab().unwrap();
        script(dir.path(), "exit 0");
        let busy = Arc::new(AtomicBool::new(false));
        let (tx, mut rx) = mpsc::channel(4);
        spawn_test(TestJob {
            path: dir.path().as_os_str().to_owned(),
            mode: NotifyMode::Desktop,
            sound: false,
            fallback: None,
            report: tx,
            session: here,
            busy: Arc::clone(&busy),
            probe: Arc::default(),
        });
        assert_eq!(rx.recv().await.unwrap(), "sent: desktop");
        assert!(!busy.load(Ordering::Acquire), "no stale test running");
    }

    // W2: a dropped send in desktop mode still rings.
    #[tokio::test]
    async fn a_send_dropped_while_busy_raises_the_fallback() {
        let dir = lab().unwrap();
        let busy = Arc::new(AtomicBool::new(true));
        let flag = Arc::new(AtomicBool::new(false));
        let job = |fallback| Job {
            path: dir.path().as_os_str().to_owned(),
            body: "x".to_owned(),
            fallback,
            busy: Arc::clone(&busy),
            probe: Arc::default(),
            session: here,
        };
        spawn_send(job(None));
        assert!(!flag.load(Ordering::Acquire));
        spawn_send(job(Some(Arc::clone(&flag))));
        assert!(flag.load(Ordering::Acquire));
        assert!(
            busy.load(Ordering::Acquire),
            "the running one keeps its flag"
        );
    }

    // W3: the program that runs is the checked real path.
    #[tokio::test]
    async fn the_executed_program_is_the_real_path_not_the_found_symlink() {
        let dir = lab().unwrap();
        let out = dir.path().join("who");
        fs::write(
            dir.path().join("real-notify.sh"),
            format!("#!/bin/sh\necho \"$0\" > '{}'\n", out.display()),
        )
        .unwrap();
        fs::set_permissions(
            dir.path().join("real-notify.sh"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        std::os::unix::fs::symlink("real-notify.sh", dir.path().join(PROGRAM)).unwrap();
        let path = dir.path().as_os_str().to_owned();
        assert!(send(&path, "x", &[]).await);
        let ran = fs::read_to_string(&out).unwrap();
        assert!(ran.trim_end().ends_with("/real-notify.sh"), "{ran}");
    }

    // W5: SSH only counts when the desktop is not local.
    #[test]
    fn a_stale_ssh_variable_does_not_hide_a_local_desktop() {
        let get = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| OsString::from(*v))
            }
        };
        let none = |_: &Path| false;
        let ssh = |pairs| Session::probe(get(pairs), none).ssh;
        assert!(
            !ssh(&[("SSH_CONNECTION", "x"), ("DISPLAY", ":0")]),
            "local X"
        );
        assert!(
            !ssh(&[("SSH_TTY", "x"), ("DISPLAY", ":1.0")]),
            "local X with a screen"
        );
        assert!(!ssh(&[
            ("SSH_CONNECTION", "x"),
            ("WAYLAND_DISPLAY", "wayland-0")
        ]));
        assert!(
            ssh(&[("SSH_CONNECTION", "x"), ("DISPLAY", "localhost:10.0")]),
            "forwarded X"
        );
        assert!(ssh(&[("SSH_TTY", "x")]), "no display at all");
        assert!(!ssh(&[("DISPLAY", "localhost:10.0")]), "no SSH variable");
    }
}
