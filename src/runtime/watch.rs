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

use crate::runtime::proc::start_time;

/// The hidden first argument that runs the watcher.
pub const SUBCOMMAND: &str = "__watch";

/// How long the groups get to end after SIGHUP and SIGTERM.
pub const GRACE: Duration = Duration::from_secs(3);
/// How often the watcher asks whether the groups ended.
const POLL: Duration = Duration::from_millis(50);

/// The write end of the watcher's stdin, once started.
static WATCHER: OnceLock<Mutex<ChildStdin>> = OnceLock::new();

/// The watcher process: lists the groups lazarobox sends on stdin and stops
/// the ones still listed when stdin ends.
pub fn main(_args: &[std::ffi::OsString]) -> i32 {
    watch(io::stdin().lock(), GRACE);
    0
}

/// Starts the watcher for this process, once. Must run before any pane
/// exists. A watcher that cannot start leaves lazarobox as it was without it.
pub fn start() {
    if WATCHER.get().is_some() {
        return;
    }
    let Some(exe) = own_exe() else { return };
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
    let Ok(mut child) = command.spawn() else {
        return;
    };
    let Some(stdin) = child.stdin.take() else {
        return;
    };
    // SAFETY: fcntl on a pipe descriptor we own.
    unsafe {
        let fd = stdin.as_raw_fd();
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags == -1 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) == -1 {
            return;
        }
    }
    let _ = WATCHER.set(Mutex::new(stdin));
}

/// This program, as the jail helper finds it: `/proc/self/exe` on Linux
/// survives an upgrade of the binary on disk.
fn own_exe() -> Option<PathBuf> {
    if cfg!(target_os = "linux") {
        return Some(PathBuf::from("/proc/self/exe"));
    }
    std::env::current_exe().and_then(std::fs::canonicalize).ok()
}

/// Lists group `pgid` with the watcher, when there is one.
pub(crate) fn register(pgid: libc::pid_t) {
    let Some(watcher) = WATCHER.get() else { return };
    if let Some(start) = start_time(pgid) {
        send(watcher, &format!("+ {pgid} {start}\n"));
    }
}

/// Takes group `pgid` off the watcher's list, when there is one.
pub(crate) fn unregister(pgid: libc::pid_t) {
    if let Some(watcher) = WATCHER.get() {
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
    stop(&targets(&groups, own, start_time), grace);
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

/// The listed groups that may be signalled: never 0, 1 or `own`, and only
/// while the leader is the listed one or gone (`started` reads its start).
fn targets(
    groups: &BTreeMap<libc::pid_t, u64>,
    own: libc::pid_t,
    started: impl Fn(libc::pid_t) -> Option<u64>,
) -> Vec<libc::pid_t> {
    groups
        .iter()
        .filter(|&(&pgid, &start)| {
            pgid > 1 && pgid != own && started(pgid).is_none_or(|now| now == start)
        })
        .map(|(&pgid, _)| pgid)
        .collect()
}

/// SIGHUP and SIGTERM to each group, up to `grace` for them to end, then
/// SIGKILL to the ones left.
fn stop(groups: &[libc::pid_t], grace: Duration) {
    for &pgid in groups {
        signal(pgid, libc::SIGHUP);
        signal(pgid, libc::SIGTERM);
    }
    let mut left = groups.to_vec();
    let deadline = Instant::now() + grace;
    loop {
        // Signal 0 only asks whether the group still has a member.
        left.retain(|&pgid| signal(pgid, 0));
        if left.is_empty() || Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(POLL);
    }
    for pgid in left {
        signal(pgid, libc::SIGKILL);
    }
}

/// Sends `sig` to group `pgid`; false when the group has no member left.
fn signal(pgid: libc::pid_t, sig: libc::c_int) -> bool {
    // SAFETY: killpg only sends a signal; `targets` refused 0, 1 and our
    // own group.
    unsafe { libc::killpg(pgid, sig) == 0 }
}

#[cfg(test)]
mod tests;
