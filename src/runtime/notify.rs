//! The desktop half of chat-notify: `notify-send`, run directly (no shell)
//! from the merged login `PATH`, detached from the UI thread.
//!
//! The machine is probed once at startup and again by the `t` test action
//! (display, session bus, SSH, the trusted `notify-send`, the sound players);
//! the result lives in a shared [`PlanSlot`]. A send or a sound only reads that
//! slot and re-checks the chosen path just before it runs, so nothing here
//! ever waits for the disk on the caller's thread.

use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use tokio::{process::Command, sync::mpsc, time::timeout};

use super::{
    ai_detect::{RunFailure, find_on_path, run_capped_why},
    chat::trusted_real_path,
};
use crate::core::ai::{
    chat::notify::{describe_test, plays_sound, wants_desktop},
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

/// Longest a whole desktop send may take (a trust re-check and the 2 s run).
const SEND_TIMEOUT: Duration = Duration::from_secs(4);

/// Longest the sound chain may take: at most five candidates (canberra, then
/// two files for each of `pw-play` and `paplay`), each bounded by [`TIMEOUT`],
/// plus headroom for the trust re-checks, so every player's reason is reported.
const SOUND_STEP: Duration = Duration::from_secs(12);

/// Longest the test action waits for its fresh probe.
const PROBE_BOUND: Duration = Duration::from_secs(3);

/// What one probe of the machine found: the session, the trusted
/// `notify-send` and the ways to play the sound, each with its reason when
/// there is none.
#[derive(Clone, Debug)]
pub struct Plan {
    pub desktop: Result<PathBuf, String>,
    pub sound: Result<Vec<Candidate>, String>,
}

/// Probes the machine: reads the session and looks for the tools on `path`.
/// It touches the disk, so it runs on a blocking thread, never on a send.
pub fn probe_plan(session: fn() -> Session, path: &OsStr) -> Plan {
    let session = session();
    let usable = !session.ssh && session.display && session.bus;
    Plan {
        desktop: desktop_plan(&session, path),
        sound: if usable {
            sound_plan(path, |f| f.exists())
        } else {
            Err("no desktop session".to_owned())
        },
    }
}

thread_local! {
    /// Set while a probe runs on this thread: its panic is caught and reported,
    /// so the panic hook must not tear the terminal down for it.
    static IN_PROBE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the current thread is running a probe (see the panic hook).
pub fn in_probe() -> bool {
    IN_PROBE.with(std::cell::Cell::get)
}

/// Runs the probe with the marker set and its panic caught.
fn probe_caught(session: fn() -> Session, path: &OsStr) -> Option<Plan> {
    IN_PROBE.with(|flag| flag.set(true));
    let plan = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| probe_plan(session, path)));
    IN_PROBE.with(|flag| flag.set(false));
    plan.ok()
}

struct Slot {
    plan: Option<Plan>,
    /// The ticket of the probe that wrote last.
    stamp: u64,
    /// The newest probe panicked and left no plan.
    failed: bool,
}

struct SlotInner {
    state: Mutex<Slot>,
    next: AtomicU64,
}

/// The shared slot of the last [`Plan`]. Empty until the first probe ends; a
/// send or a sound that finds it empty treats the desktop as unavailable.
/// Every probe takes a ticket when it starts, and a write only counts if its
/// ticket is newer than the stored one, so a slow probe never overwrites a
/// newer plan.
#[derive(Clone)]
pub struct PlanSlot(Arc<SlotInner>);

impl Default for PlanSlot {
    fn default() -> Self {
        Self(Arc::new(SlotInner {
            state: Mutex::new(Slot {
                plan: None,
                stamp: 0,
                failed: false,
            }),
            next: AtomicU64::new(0),
        }))
    }
}

impl PlanSlot {
    fn lock(&self) -> std::sync::MutexGuard<'_, Slot> {
        self.0.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The ticket of a probe that is about to start.
    fn ticket(&self) -> u64 {
        self.0.next.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub fn get(&self) -> Option<Plan> {
        self.lock().plan.clone()
    }

    /// Whether the newest probe panicked and left no plan.
    pub fn failed(&self) -> bool {
        self.lock().failed
    }

    fn fill(&self, ticket: u64, plan: Plan) {
        let mut slot = self.lock();
        if ticket > slot.stamp {
            *slot = Slot {
                plan: Some(plan),
                stamp: ticket,
                failed: false,
            };
        }
    }

    fn fail(&self, ticket: u64) {
        let mut slot = self.lock();
        if ticket > slot.stamp {
            *slot = Slot {
                plan: None,
                stamp: ticket,
                failed: true,
            };
        }
    }
}

/// Probes once in the background, at startup. The closure fills the slot
/// itself when it ends, so a slow probe still fills it for later and nothing
/// ever waits for it. A probe that panics is recorded as failed.
pub fn probe_in_background(slot: &PlanSlot, session: fn() -> Session, path: OsString) {
    let (slot, ticket) = (slot.clone(), slot.ticket());
    drop(tokio::task::spawn_blocking(move || {
        match probe_caught(session, &path) {
            Some(plan) => slot.fill(ticket, plan),
            None => slot.fail(ticket),
        }
    }));
}

/// A fresh probe for the test action, waited for at most [`PROBE_BOUND`].
/// The closure fills the slot when it ends, even after the bound.
async fn probe_now(
    slot: &PlanSlot,
    session: fn() -> Session,
    path: OsString,
) -> Result<Plan, String> {
    let (slot, ticket) = (slot.clone(), slot.ticket());
    let task = tokio::task::spawn_blocking(move || match probe_caught(session, &path) {
        Some(plan) => {
            slot.fill(ticket, plan.clone());
            Some(plan)
        }
        None => {
            slot.fail(ticket);
            None
        }
    });
    match timeout(PROBE_BOUND, task).await {
        Ok(Ok(Some(plan))) => Ok(plan),
        Ok(_) => Err("the check failed".to_owned()),
        Err(_) => Err("the check timed out".to_owned()),
    }
}

/// Whether `program` (a real path from a probe) is still trusted right now:
/// the cheap check just before an exec.
async fn still_trusted(program: &Path) -> bool {
    let program = program.to_owned();
    tokio::task::spawn_blocking(move || trusted_real_path(&program).as_ref() == Some(&program))
        .await
        .unwrap_or(false)
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

/// Why a candidate failed, in a few words.
fn why(failure: &RunFailure) -> &'static str {
    match failure {
        RunFailure::Timeout => "still running after 2 s, killed",
        RunFailure::Spawn => "could not start",
        _ => "failed",
    }
}

/// Tries the candidates in order and returns the name of the first player
/// that exits 0 within its limit. A player still running at the limit is
/// killed and counts as failed, like a spawn error or a failing exit; so does
/// one whose file stopped being trusted since the probe. The error lists why
/// each failed.
async fn play_plan(
    plan: &[Candidate],
    path: &OsStr,
    vars: &[(OsString, OsString)],
) -> Result<&'static str, String> {
    let mut failures: Vec<String> = Vec::new();
    for candidate in plan {
        let reason = if !still_trusted(&candidate.program).await {
            "no longer trusted"
        } else {
            match run_tool(
                &candidate.program,
                candidate.name,
                &candidate.args,
                path,
                vars,
            )
            .await
            {
                Ok(()) => return Ok(candidate.name),
                Err(failure) => why(&failure),
            }
        };
        let entry = format!("{}: {reason}", candidate.name);
        if !failures.contains(&entry) {
            failures.push(entry);
        }
    }
    Err(format!("no player worked ({})", failures.join(", ")))
}

/// Raises the bell fallback when dropped, unless settled as fine first: so
/// every way out of a send (an early return, a timeout, a panic) rings in
/// `desktop` mode.
struct FallbackGuard {
    flag: Option<Arc<AtomicBool>>,
    armed: bool,
}

impl FallbackGuard {
    fn new(flag: Option<Arc<AtomicBool>>) -> Self {
        Self { flag, armed: true }
    }

    /// Says whether the event needs the fallback after all.
    fn settle(&mut self, failed: bool) {
        self.armed = failed;
    }
}

impl Drop for FallbackGuard {
    fn drop(&mut self) {
        if self.armed
            && let Some(flag) = &self.flag
        {
            flag.store(true, Ordering::Release);
        }
    }
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
    /// What the last probe found. Empty means unavailable.
    pub plan: PlanSlot,
}

/// Clears the in-flight flag on every way out: a panic, a timeout, or a task
/// that is dropped before it ever ran.
struct Release(Arc<AtomicBool>);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Sends in the background and returns at once; nothing here touches the disk
/// on the caller's thread. At most one send is in flight: a burst of finished
/// turns shows one notification and the rest are dropped. A dropped send, an
/// empty or negative plan, a path that is no longer trusted and a failed send
/// all raise the fallback (so `desktop` mode rings; `auto` has none, its bell
/// already rang).
pub fn spawn_send(job: Job) {
    let mut fallback = FallbackGuard::new(job.fallback.clone());
    if job.busy.swap(true, Ordering::AcqRel) {
        return; // the guard rings
    }
    let release = Release(Arc::clone(&job.busy));
    tokio::spawn(async move {
        let _release = release;
        let sent = timeout(SEND_TIMEOUT, send_planned(&job))
            .await
            .unwrap_or(false);
        fallback.settle(!sent);
    });
}

async fn send_planned(job: &Job) -> bool {
    let Some(Ok(program)) = job.plan.get().map(|plan| plan.desktop) else {
        return false;
    };
    if !still_trusted(&program).await {
        return false;
    }
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
    /// What the last probe found. Empty means no sound.
    pub plan: PlanSlot,
}

/// Plays in the background and returns at once, with the same rules as
/// [`spawn_send`]: one in flight, nothing over SSH or without a session, and
/// nothing at all when the plan is empty or no player works.
pub fn spawn_sound(job: SoundJob) {
    if job.busy.swap(true, Ordering::AcqRel) {
        return;
    }
    let release = Release(Arc::clone(&job.busy));
    tokio::spawn(async move {
        let _release = release;
        let Some(Ok(plan)) = job.plan.get().map(|plan| plan.sound) else {
            return;
        };
        let vars = allowed_env(std::env::vars_os());
        let _ = timeout(SOUND_STEP, play_plan(&plan, &job.path, &vars)).await;
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

/// What the test action does and reports for a fresh `plan`, with the focus
/// rule ignored. The bell is the caller's; this attempts the desktop and the
/// sound the mode asks for and describes the result. Each step has its own
/// bound, so a slow sound never hides the desktop outcome.
pub async fn run_test(
    path: &OsStr,
    plan: &Plan,
    mode: NotifyMode,
    sound: bool,
    vars: &[(OsString, OsString)],
    sound_step: Duration,
) -> TestReport {
    let desktop = if wants_desktop(mode) {
        Some(match &plan.desktop {
            Err(why) => Err(why.clone()),
            Ok(program) => {
                if send_via(program, path, "Test notification", vars).await {
                    Ok(())
                } else {
                    Err("notify-send failed".to_owned())
                }
            }
        })
    } else {
        None
    };
    let sound = if plays_sound(mode, sound) {
        Some(match &plan.sound {
            Err(why) => Err(why.clone()),
            Ok(candidates) => match timeout(sound_step, play_plan(candidates, path, vars)).await {
                Ok(result) => result.map(str::to_owned),
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
    /// Reads the session for the fresh probe.
    pub session: fn() -> Session,
    /// Set while a test runs: a second one only says "test running".
    pub busy: Arc<AtomicBool>,
    /// The slot the fresh probe refreshes.
    pub plan: PlanSlot,
}

/// Runs the test in the background and sends the report line. It probes the
/// machine again first (waiting at most [`PROBE_BOUND`]), so a tool installed
/// since startup is seen. One at a time; the in-flight guard and the fallback
/// are settled before the report goes out.
pub fn spawn_test(job: TestJob) {
    if job.busy.swap(true, Ordering::AcqRel) {
        let _ = job.report.try_send("test running".to_owned());
        return;
    }
    let release = Release(Arc::clone(&job.busy));
    let mut fallback = FallbackGuard::new(job.fallback.clone());
    tokio::spawn(async move {
        let text = match probe_now(&job.plan, job.session, job.path.clone()).await {
            Ok(plan) => {
                let vars = allowed_env(std::env::vars_os());
                let report =
                    run_test(&job.path, &plan, job.mode, job.sound, &vars, SOUND_STEP).await;
                fallback.settle(report.desktop_failed);
                report.text
            }
            Err(why) => {
                fallback.settle(wants_desktop(job.mode));
                why
            }
        };
        drop(fallback);
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

    // Thin helpers over the production path (`desktop_plan`, `sound_plan`,
    // `send_via`, `play_plan`), for tests that only need a yes or a reason.
    async fn send(path: &OsStr, body: &str, vars: &[(OsString, OsString)]) -> bool {
        match probe_plan(here, path).desktop {
            Ok(program) => send_via(&program, path, body, vars).await,
            Err(_) => false,
        }
    }

    fn unavailable_reason(session: &Session, path: &OsStr) -> Option<String> {
        desktop_plan(session, path).err()
    }

    fn available(session: &Session, path: &OsStr) -> bool {
        unavailable_reason(session, path).is_none()
    }

    async fn play(
        path: &OsStr,
        vars: &[(OsString, OsString)],
        exists: impl Fn(&Path) -> bool,
    ) -> Result<&'static str, String> {
        play_plan(&sound_plan(path, exists)?, path, vars).await
    }

    fn plan_with(dir: &Path, session: fn() -> Session) -> PlanSlot {
        let slot = PlanSlot::default();
        slot.fill(slot.ticket(), probe_plan(session, dir.as_os_str()));
        slot
    }

    fn plan_for(dir: &Path) -> PlanSlot {
        plan_with(dir, here)
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
            plan: plan_for(dir.path()),
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
            plan: plan_for(dir.path()),
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

    // A plan without a desktop (here: no notify-send on the PATH) is a failed
    // send, so `desktop` mode rings.
    #[tokio::test]
    async fn a_plan_without_a_desktop_raises_the_fallback() {
        let dir = lab().unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        spawn_send(Job {
            path: dir.path().as_os_str().to_owned(),
            body: "x".to_owned(),
            fallback: Some(Arc::clone(&flag)),
            busy: Arc::default(),
            plan: plan_for(dir.path()),
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
            Ok("paplay")
        );
        assert_eq!(
            fs::read_to_string(&log).unwrap(),
            "canberra-gtk-play\npw-play\npaplay\n"
        );
        fs::remove_file(&log).unwrap();
        note("canberra-gtk-play", 0);
        assert_eq!(play(&path, &[], |_| true).await, Ok("canberra-gtk-play"));
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
        assert_eq!(play(&path, &[], |_| false).await, Ok("canberra-gtk-play"));
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
            Err("no player worked (canberra-gtk-play: failed)".to_owned())
        );
        tool(dir.path(), "pw-play", "exit 1");
        assert_eq!(
            play(&path, &[], |_| true).await,
            Err("no player worked (canberra-gtk-play: failed, pw-play: failed)".to_owned()),
            "every candidate failed"
        );
    }

    fn sound_job(dir: &Path, session: fn() -> Session, busy: &Arc<AtomicBool>) -> SoundJob {
        SoundJob {
            path: dir.as_os_str().to_owned(),
            busy: Arc::clone(busy),
            plan: plan_with(dir, session),
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
            async move {
                run_test(
                    &path,
                    &probe_plan(here, &path),
                    mode,
                    sound,
                    &[],
                    SOUND_STEP,
                )
                .await
            }
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
        assert_eq!(r.text, "sent: bell + desktop + sound (canberra-gtk-play)");
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
            plan: PlanSlot::default(),
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
            plan: PlanSlot::default(),
        });
        assert!(rx.recv().await.unwrap().contains("unavailable"));
        assert!(flag.load(Ordering::Acquire));
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
            &probe_plan(here, &path),
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
        assert_eq!(play(&path, &[], |_| true).await, Ok("pw-play"));
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
            plan: PlanSlot::default(),
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
            plan: plan_for(dir.path()),
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

    // ---- the startup plan ----

    // A slow startup probe never delays anything and fills the slot later.
    #[tokio::test]
    async fn a_slow_startup_probe_fills_the_slot_later_without_blocking() {
        fn slow() -> Session {
            std::thread::sleep(Duration::from_millis(400));
            here()
        }
        let dir = lab().unwrap();
        script(dir.path(), "exit 0");
        let slot = PlanSlot::default();
        let started = std::time::Instant::now();
        probe_in_background(&slot, slow, dir.path().as_os_str().to_owned());
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "returns at once"
        );
        assert!(slot.get().is_none(), "not filled yet");
        for _ in 0..100 {
            if slot.get().is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let plan = slot.get().expect("filled by the closure itself");
        assert!(plan.desktop.is_ok(), "{:?}", plan.desktop);
    }

    // An empty slot (probe not done) is unavailable, with the fallback.
    #[tokio::test]
    async fn an_empty_slot_means_no_desktop_and_the_fallback_applies() {
        let flag = Arc::new(AtomicBool::new(false));
        spawn_send(Job {
            path: OsString::new(),
            body: "x".to_owned(),
            fallback: Some(Arc::clone(&flag)),
            busy: Arc::default(),
            plan: PlanSlot::default(),
        });
        for _ in 0..100 {
            if flag.load(Ordering::Acquire) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("an empty slot must raise the fallback");
    }

    #[tokio::test]
    async fn an_empty_slot_plays_no_sound() {
        let dir = lab().unwrap();
        let log = dir.path().join("log");
        tool(
            dir.path(),
            "canberra-gtk-play",
            &format!("echo x >> '{}'", log.display()),
        );
        spawn_sound(SoundJob {
            path: dir.path().as_os_str().to_owned(),
            busy: Arc::default(),
            plan: PlanSlot::default(),
        });
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(!log.exists());
    }

    // The path is re-checked just before it runs.
    #[tokio::test]
    async fn a_path_that_stopped_being_trusted_is_not_run_and_the_fallback_applies() {
        let dir = lab().unwrap();
        let log = dir.path().join("log");
        script(dir.path(), &format!("echo x >> '{}'", log.display()));
        let plan = plan_for(dir.path());
        assert!(plan.get().unwrap().desktop.is_ok());
        fs::set_permissions(dir.path().join(PROGRAM), fs::Permissions::from_mode(0o757)).unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        spawn_send(Job {
            path: dir.path().as_os_str().to_owned(),
            body: "x".to_owned(),
            fallback: Some(Arc::clone(&flag)),
            busy: Arc::default(),
            plan,
        });
        for _ in 0..100 {
            if flag.load(Ordering::Acquire) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(flag.load(Ordering::Acquire));
        assert!(!log.exists(), "the swapped file never ran");
    }

    #[tokio::test]
    async fn a_candidate_that_stopped_being_trusted_is_skipped() {
        let dir = lab().unwrap();
        let log = dir.path().join("log");
        tool(
            dir.path(),
            "canberra-gtk-play",
            &format!("echo c >> '{}'", log.display()),
        );
        tool(
            dir.path(),
            "pw-play",
            &format!("echo pw >> '{}'", log.display()),
        );
        let path = dir.path().as_os_str().to_owned();
        let plan = sound_plan(&path, |_| true).unwrap();
        fs::set_permissions(
            dir.path().join("canberra-gtk-play"),
            fs::Permissions::from_mode(0o757),
        )
        .unwrap();
        assert_eq!(play_plan(&plan, &path, &[]).await, Ok("pw-play"));
        assert_eq!(fs::read_to_string(&log).unwrap(), "pw\n");
    }

    // Option (b): a player still running at its limit is killed and fails.
    #[tokio::test]
    async fn a_hung_player_is_killed_and_the_next_candidate_plays() {
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
        assert_eq!(play(&path, &[], |_| true).await, Ok("pw-play"));
        assert_eq!(fs::read_to_string(&log).unwrap(), "pw\n");
    }

    #[tokio::test]
    async fn when_every_player_fails_the_reasons_are_listed() {
        let dir = lab().unwrap();
        tool(
            dir.path(),
            "canberra-gtk-play",
            "PATH=\"$PATH:/usr/bin:/bin\"\nsleep 30",
        );
        tool(dir.path(), "pw-play", "exit 3");
        let path = dir.path().as_os_str().to_owned();
        assert_eq!(
            play(&path, &[], |f| f == Path::new(OGA)).await,
            Err("no player worked (canberra-gtk-play: still running after 2 s, killed, pw-play: failed)".to_owned())
        );
    }

    // The test action probes again, so a newly installed tool is seen.
    #[tokio::test]
    async fn the_test_action_probes_again_and_refreshes_the_slot() {
        let dir = lab().unwrap();
        let slot = plan_for(dir.path());
        assert!(
            slot.get().unwrap().desktop.is_err(),
            "stale: nothing installed yet"
        );
        script(dir.path(), "exit 0");
        let (tx, mut rx) = mpsc::channel(4);
        spawn_test(TestJob {
            path: dir.path().as_os_str().to_owned(),
            mode: NotifyMode::Desktop,
            sound: false,
            fallback: None,
            report: tx,
            session: here,
            busy: Arc::default(),
            plan: slot.clone(),
        });
        assert_eq!(rx.recv().await.unwrap(), "sent: desktop");
        assert!(
            slot.get().unwrap().desktop.is_ok(),
            "the slot was refreshed"
        );
    }

    #[tokio::test]
    async fn a_slow_probe_in_the_test_action_times_out_and_still_fills_the_slot_later() {
        fn slow() -> Session {
            std::thread::sleep(Duration::from_millis(3600));
            here()
        }
        let dir = lab().unwrap();
        script(dir.path(), "exit 0");
        let slot = PlanSlot::default();
        let flag = Arc::new(AtomicBool::new(false));
        let (tx, mut rx) = mpsc::channel(4);
        spawn_test(TestJob {
            path: dir.path().as_os_str().to_owned(),
            mode: NotifyMode::Desktop,
            sound: false,
            fallback: Some(Arc::clone(&flag)),
            report: tx,
            session: slow,
            busy: Arc::default(),
            plan: slot.clone(),
        });
        assert_eq!(rx.recv().await.unwrap(), "the check timed out");
        assert!(flag.load(Ordering::Acquire), "desktop mode rings");
        for _ in 0..40 {
            if slot.get().is_some() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("the slow probe never filled the slot");
    }

    // The fallback is raised on every way out, a panic included.
    #[test]
    fn the_fallback_guard_raises_on_drop_unless_settled() {
        let flag = Arc::new(AtomicBool::new(false));
        drop(FallbackGuard::new(Some(Arc::clone(&flag))));
        assert!(flag.load(Ordering::Acquire), "dropped armed");
        flag.store(false, Ordering::Release);
        let mut guard = FallbackGuard::new(Some(Arc::clone(&flag)));
        guard.settle(false);
        drop(guard);
        assert!(!flag.load(Ordering::Acquire), "settled as fine");
        let flag2 = Arc::clone(&flag);
        let _ = std::panic::catch_unwind(move || {
            let _guard = FallbackGuard::new(Some(flag2));
            panic!("the send task died");
        });
        assert!(flag.load(Ordering::Acquire), "a panic still raises it");
        drop(FallbackGuard::new(None));
    }

    // A desktop success is never marked as failed by what comes after.
    #[tokio::test]
    async fn a_slow_sound_after_a_desktop_success_does_not_fail_the_desktop() {
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
            &probe_plan(here, &path),
            NotifyMode::Desktop,
            true,
            &[],
            Duration::from_millis(300),
        )
        .await;
        assert!(!report.desktop_failed);
        assert!(report.text.starts_with("sent: desktop"), "{}", report.text);
    }

    // ---- the slot: generations, a failed probe, poisoning ----

    #[test]
    fn an_older_probe_never_overwrites_a_newer_plan() {
        let dir = lab().unwrap();
        let slot = PlanSlot::default();
        let (older, newer) = (slot.ticket(), slot.ticket());
        script(dir.path(), "exit 0");
        let good = probe_plan(here, dir.path().as_os_str());
        let bad = probe_plan(
            || Session {
                ssh: true,
                ..here()
            },
            dir.path().as_os_str(),
        );
        slot.fill(newer, good);
        slot.fill(older, bad);
        assert!(slot.get().unwrap().desktop.is_ok(), "the newer plan stays");
    }

    #[tokio::test]
    async fn a_slow_startup_probe_cannot_overwrite_the_test_probe() {
        fn slow_and_remote() -> Session {
            std::thread::sleep(Duration::from_millis(600));
            Session {
                ssh: true,
                ..here()
            }
        }
        let dir = lab().unwrap();
        script(dir.path(), "exit 0");
        let path = dir.path().as_os_str().to_owned();
        let slot = PlanSlot::default();
        probe_in_background(&slot, slow_and_remote, path.clone());
        let (tx, mut rx) = mpsc::channel(4);
        spawn_test(TestJob {
            path,
            mode: NotifyMode::Desktop,
            sound: false,
            fallback: None,
            report: tx,
            session: here,
            busy: Arc::default(),
            plan: slot.clone(),
        });
        assert_eq!(rx.recv().await.unwrap(), "sent: desktop");
        tokio::time::sleep(Duration::from_millis(900)).await;
        assert!(
            slot.get().unwrap().desktop.is_ok(),
            "the late startup probe lost"
        );
    }

    #[tokio::test]
    async fn a_panicking_startup_probe_is_recorded_and_the_test_still_probes() {
        fn boom() -> Session {
            panic!("the probe died");
        }
        let dir = lab().unwrap();
        script(dir.path(), "exit 0");
        let slot = PlanSlot::default();
        probe_in_background(&slot, boom, dir.path().as_os_str().to_owned());
        for _ in 0..100 {
            if slot.failed() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(slot.failed());
        assert!(slot.get().is_none());
        let (tx, mut rx) = mpsc::channel(4);
        spawn_test(TestJob {
            path: dir.path().as_os_str().to_owned(),
            mode: NotifyMode::Desktop,
            sound: false,
            fallback: None,
            report: tx,
            session: here,
            busy: Arc::default(),
            plan: slot.clone(),
        });
        assert_eq!(rx.recv().await.unwrap(), "sent: desktop");
        assert!(!slot.failed(), "a good plan clears the failure");
    }

    #[tokio::test]
    async fn a_panicking_test_probe_says_the_check_failed_and_rings_in_desktop_mode() {
        fn boom() -> Session {
            panic!("the probe died");
        }
        let flag = Arc::new(AtomicBool::new(false));
        let (tx, mut rx) = mpsc::channel(4);
        spawn_test(TestJob {
            path: OsString::new(),
            mode: NotifyMode::Desktop,
            sound: false,
            fallback: Some(Arc::clone(&flag)),
            report: tx,
            session: boom,
            busy: Arc::default(),
            plan: PlanSlot::default(),
        });
        assert_eq!(rx.recv().await.unwrap(), "the check failed");
        assert!(flag.load(Ordering::Acquire));
    }

    #[test]
    fn a_poisoned_slot_still_reads_and_writes() {
        let dir = lab().unwrap();
        let slot = PlanSlot::default();
        let held = slot.clone();
        let _ = std::thread::spawn(move || {
            let _guard = held.0.state.lock().unwrap();
            panic!("poison it");
        })
        .join();
        assert!(slot.get().is_none());
        slot.fill(slot.ticket(), probe_plan(here, dir.path().as_os_str()));
        assert!(slot.get().is_some());
    }

    #[test]
    fn the_sound_chain_has_headroom_for_every_candidate() {
        // Five candidates at the per-program limit, plus the trust checks.
        assert!(SOUND_STEP > TIMEOUT * 5);
    }

    // A probe panic is caught and must not tear the terminal down.
    #[tokio::test]
    async fn the_probe_thread_is_marked_so_the_panic_hook_leaves_the_terminal_alone() {
        static MARKED: AtomicBool = AtomicBool::new(false);
        fn spy() -> Session {
            MARKED.store(in_probe(), Ordering::SeqCst);
            here()
        }
        assert!(!in_probe());
        let dir = lab().unwrap();
        let slot = PlanSlot::default();
        probe_in_background(&slot, spy, dir.path().as_os_str().to_owned());
        for _ in 0..100 {
            if slot.get().is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(MARKED.load(Ordering::SeqCst));
    }
}
