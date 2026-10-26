//! The watcher: a small process lazarobox starts at launch (`lazarobox
//! __watch`) that outlives it, so nothing lazarobox started is left running
//! once it ends, whatever the way (quit, crash, SIGKILL, a closed terminal).
//!
//! - lazarobox writes to the watcher's stdin each process group it starts
//!   (`+ <pgid> <start>`, from [`GroupKill::new`]) and each one it has reaped
//!   (`- <pgid>`, from [`GroupKill::disarm`]). Anything else is ignored.
//! - When lazarobox ends, the kernel closes the pipe. The watcher then sends
//!   SIGHUP and SIGTERM to every group still listed, gives them [`GRACE`] to
//!   end, sends SIGKILL to what is left, and exits.
//! - A group is only signalled while its leader is still the process that was
//!   listed (same start time) or has no leader left: a pid is not reused while
//!   a group with that id has members, so a recycled pid is never hit. Group
//!   1, group 0 and the watcher's own group are never signalled.
//! - It runs in its own session, so a closing terminal does not reach it, and
//!   it is started before any pane exists, so it holds no terminal.
//! - `engram serve` is never listed: it is the user's shared memory server
//!   and keeps running on purpose.
//! - Writes never block lazarobox: the pipe is non-blocking and a line that
//!   does not fit is dropped (that group would then only be left behind by a
//!   crash, as before the watcher).
//!
//! [`GroupKill::new`]: crate::runtime::proc::GroupKill::new
//! [`GroupKill::disarm`]: crate::runtime::proc::GroupKill::disarm

use std::{
    collections::BTreeMap,
    io::{self, BufRead, Write},
    os::{fd::AsRawFd, unix::process::CommandExt},
    path::PathBuf,
    process::{ChildStdin, Command, Stdio},
    sync::{Mutex, OnceLock, PoisonError},
    time::{Duration, Instant},
};

use crate::runtime::proc::{running, start_time};

/// The hidden first argument that runs the watcher.
pub const SUBCOMMAND: &str = "__watch";

/// How long the groups get to end after SIGHUP and SIGTERM.
pub const GRACE: Duration = Duration::from_secs(3);
/// How often the watcher asks whether the groups ended.
const POLL: Duration = Duration::from_millis(50);

/// How long quitting gives the groups after SIGHUP and SIGTERM: less than
/// the quit bound, so what was killed is still reaped inside it.
pub const QUIT_GRACE: Duration = Duration::from_secs(2);

/// What [`start`] sets up: the groups lazarobox has not reaped yet, and the
/// write end of the watcher's stdin when it started.
struct State {
    listed: Listed,
    watcher: Option<Mutex<ChildStdin>>,
}

static STATE: OnceLock<State> = OnceLock::new();

/// The watcher process: lists the groups lazarobox sends on stdin and stops
/// the ones still listed when stdin ends.
pub fn main(_args: &[std::ffi::OsString]) -> i32 {
    watch(io::stdin().lock(), GRACE);
    0
}

/// Starts listing the groups lazarobox starts, and the watcher, once. Must
/// run before any pane exists. A watcher that cannot start leaves only the
/// list, which [`stop_listed`] still uses when quitting.
pub fn start() {
    if STATE.get().is_some() {
        return;
    }
    let _ = STATE.set(State {
        listed: Listed::default(),
        watcher: spawn_watcher().map(Mutex::new),
    });
}

/// SIGHUP and SIGTERM to every group lazarobox started and has not reaped,
/// up to `grace` for them to end, then SIGKILL to the ones still listed.
/// Blocking: quitting runs it on its own thread.
pub fn stop_listed(grace: Duration) {
    if let Some(state) = STATE.get() {
        state.listed.stop(grace);
    }
}

/// `lazarobox-shell __watch` in its own session, with a non-blocking pipe
/// to its stdin.
fn spawn_watcher() -> Option<ChildStdin> {
    let exe = own_exe()?;
    let mut command = Command::new(exe);
    command
        .arg(SUBCOMMAND)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: the closure runs in the child between fork and exec and only
    // calls setsid, which is async-signal-safe.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    // Never waited on: it ends after lazarobox does.
    let stdin = command.spawn().ok()?.stdin.take()?;
    // SAFETY: fcntl on a pipe descriptor we own.
    unsafe {
        let fd = stdin.as_raw_fd();
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags == -1 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) == -1 {
            return None;
        }
    }
    Some(stdin)
}

/// This program, as the jail helper finds it: `/proc/self/exe` on Linux
/// survives an upgrade of the binary on disk.
fn own_exe() -> Option<PathBuf> {
    if cfg!(target_os = "linux") {
        return Some(PathBuf::from("/proc/self/exe"));
    }
    std::env::current_exe().and_then(std::fs::canonicalize).ok()
}

/// Lists group `pgid`, and with the watcher when there is one. Nothing
/// before [`start`], so tests never list anything.
pub(crate) fn register(pgid: libc::pid_t) {
    let Some(state) = STATE.get() else { return };
    let Some(start) = start_time(pgid) else {
        return;
    };
    state.listed.add(pgid, start);
    if let Some(watcher) = &state.watcher {
        send(watcher, &format!("+ {pgid} {start}\n"));
    }
}

/// Takes group `pgid` off the list, and off the watcher's.
pub(crate) fn unregister(pgid: libc::pid_t) {
    let Some(state) = STATE.get() else { return };
    state.listed.remove(pgid);
    if let Some(watcher) = &state.watcher {
        send(watcher, &format!("- {pgid}\n"));
    }
}

/// One whole line or nothing: a line this short is written atomically.
fn send(watcher: &Mutex<ChildStdin>, line: &str) {
    let mut stdin = watcher.lock().unwrap_or_else(PoisonError::into_inner);
    let _ = stdin.write(line.as_bytes());
}

/// Reads the list from `input` until it ends, then stops what is listed.
pub fn watch(input: impl BufRead, grace: Duration) {
    let mut groups = BTreeMap::new();
    for line in input.lines() {
        let Ok(line) = line else { break };
        apply(&mut groups, &line);
    }
    // SAFETY: getpgrp has no preconditions and cannot fail.
    let own = unsafe { libc::getpgrp() };
    // Lazarobox is gone, so the watcher is nobody's parent here: a group
    // lives while it has a member, and what a leader left behind gets the
    // grace too.
    stop(
        &targets(&groups, own, start_time),
        grace,
        |pgid, _| signal(pgid, 0),
        |_| true,
    );
}

/// One line of the list: `+ <pgid> <start>` or `- <pgid>`.
fn apply(groups: &mut BTreeMap<libc::pid_t, u64>, line: &str) {
    let words: Vec<&str> = line.split(' ').collect();
    match words.as_slice() {
        ["+", pgid, start] => {
            if let (Ok(pgid), Ok(start)) = (pgid.parse(), start.parse()) {
                groups.insert(pgid, start);
            }
        }
        ["-", pgid] => {
            if let Ok(pgid) = pgid.parse() {
                groups.remove(&pgid);
            }
        }
        _ => {}
    }
}

/// The listed groups that may be signalled, with their leader's start: never
/// 0, 1 or `own`, and only while the leader is the listed one or gone
/// (`started` reads its start).
fn targets(
    groups: &BTreeMap<libc::pid_t, u64>,
    own: libc::pid_t,
    started: impl Fn(libc::pid_t) -> Option<u64>,
) -> Vec<(libc::pid_t, u64)> {
    groups
        .iter()
        .filter(|&(&pgid, &start)| {
            pgid > 1 && pgid != own && started(pgid).is_none_or(|now| now == start)
        })
        .map(|(&pgid, &start)| (pgid, start))
        .collect()
}

/// SIGHUP and SIGTERM to each group, then up to `grace` while it is
/// `alive`. One that stops being alive gets SIGKILL at once for whatever is
/// left in it; one still alive after `grace` gets it then. SIGKILL only goes
/// to groups `still` listed.
fn stop(
    groups: &[(libc::pid_t, u64)],
    grace: Duration,
    alive: impl Fn(libc::pid_t, u64) -> bool,
    still: impl Fn(libc::pid_t) -> bool,
) {
    for &(pgid, _) in groups {
        signal(pgid, libc::SIGHUP);
        signal(pgid, libc::SIGTERM);
    }
    let kill = |pgid| {
        if still(pgid) {
            signal(pgid, libc::SIGKILL);
        }
    };
    let mut left = groups.to_vec();
    let deadline = Instant::now() + grace;
    loop {
        left.retain(|&(pgid, start)| {
            if alive(pgid, start) {
                return true;
            }
            kill(pgid);
            false
        });
        if left.is_empty() || Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(POLL);
    }
    for (pgid, _) in left {
        kill(pgid);
    }
}

/// Sends `sig` to group `pgid`; false when the group has no member left.
fn signal(pgid: libc::pid_t, sig: libc::c_int) -> bool {
    // SAFETY: killpg only sends a signal; `targets` refused 0, 1 and our
    // own group.
    unsafe { libc::killpg(pgid, sig) == 0 }
}

/// The groups lazarobox started and has not reaped yet, with their leader's
/// start time: what quitting stops politely.
#[derive(Default)]
pub(crate) struct Listed(Mutex<BTreeMap<libc::pid_t, u64>>);

impl Listed {
    pub(crate) fn add(&self, pgid: libc::pid_t, start: u64) {
        self.lock().insert(pgid, start);
    }

    pub(crate) fn remove(&self, pgid: libc::pid_t) {
        self.lock().remove(&pgid);
    }

    /// [`stop`] on a copy of the list. A group lives while its leader runs: a
    /// leader that ended is a zombie until lazarobox reaps it, which keeps its
    /// group, so what it left behind gets SIGKILL at once, like a pane's
    /// shell. A group lazarobox reaps meanwhile leaves the list and gets no
    /// SIGKILL: its pid may already be reused.
    pub(crate) fn stop(&self, grace: Duration) {
        let groups = self.lock().clone();
        // SAFETY: getpgrp has no preconditions and cannot fail.
        let own = unsafe { libc::getpgrp() };
        stop(&targets(&groups, own, start_time), grace, running, |pgid| {
            self.lock().contains_key(&pgid)
        });
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<libc::pid_t, u64>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests;
