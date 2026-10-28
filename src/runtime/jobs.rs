//! What still runs in the panes: the jobs (process groups) of each pane's
//! session other than its shell's own group. lazarobox never stops them (a
//! pane behaves like any terminal: what survives a hangup keeps running); it
//! only names them, in the quit prompt and after it closes.
//!
//! - A pane's shell is the leader of its own session (portable-pty calls
//!   `setsid`), so its jobs are the process groups of that session. Jobs
//!   started with `&`, `nohup` or `disown` stay in it; a daemon that called
//!   `setsid` left it and is not named.
//! - A session counts only while its leader is the recorded shell (same start
//!   time) or gone: a session id is not reused while the session has members.
//! - Each job is named by its group leader's command line (on macOS, its
//!   program name), or by the lowest pid left in it when the leader ended.

#[cfg(target_os = "linux")]
use std::fs;
use std::{collections::BTreeMap, fmt::Write, thread, time::Duration};

/// How long the jobs a pane's hangup is ending get before they are named
/// as left running.
pub const HANGUP_SETTLE: Duration = Duration::from_millis(150);

/// A pane's session: its shell's pid (= session id) and start time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Session {
    pub sid: libc::pid_t,
    pub start: u64,
}

/// One job still running: its process group and what it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub pgid: libc::pid_t,
    pub command: String,
}

/// The jobs that run in `sessions`, ordered by process group.
pub fn running(sessions: &[Session]) -> Vec<Job> {
    if sessions.is_empty() {
        return Vec::new();
    }
    let sids: Vec<libc::pid_t> = sessions.iter().map(|session| session.sid).collect();
    jobs_of(sessions, &processes(&sids))
}

/// What still runs in `sessions` once the jobs ending on their hangup had
/// `settle` to go; read once more only when something was running.
pub fn left_running(sessions: &[Session], settle: Duration) -> Vec<Job> {
    if running(sessions).is_empty() {
        return Vec::new();
    }
    thread::sleep(settle);
    running(sessions)
}

/// The lines naming `jobs` after lazarobox closed, `None` when nothing runs.
pub fn report(jobs: &[Job]) -> Option<String> {
    if jobs.is_empty() {
        return None;
    }
    let mut out = String::from("lazarobox-shell: still running from your panes:\n");
    for job in jobs {
        let _ = writeln!(out, "  {} (process group {})", job.command, job.pgid);
    }
    Some(out)
}

/// One process as the system lists it.
#[derive(Clone, Debug)]
struct Proc {
    pid: libc::pid_t,
    pgid: libc::pid_t,
    sid: libc::pid_t,
    start: u64,
    zombie: bool,
    command: String,
}

/// [`running`] over a given process list.
fn jobs_of(sessions: &[Session], procs: &[Proc]) -> Vec<Job> {
    let ours: Vec<libc::pid_t> = sessions
        .iter()
        .filter(|session| {
            procs
                .iter()
                .find(|p| p.pid == session.sid)
                .is_none_or(|leader| leader.start == session.start)
        })
        .map(|session| session.sid)
        .collect();
    let mut groups: BTreeMap<libc::pid_t, Vec<&Proc>> = BTreeMap::new();
    for p in procs {
        if !p.zombie && p.pgid != p.sid && ours.contains(&p.sid) {
            groups.entry(p.pgid).or_default().push(p);
        }
    }
    groups
        .into_iter()
        .filter_map(|(pgid, members)| {
            let named = members
                .iter()
                .find(|p| p.pid == pgid)
                .or_else(|| members.iter().min_by_key(|p| p.pid))?;
            Some(Job {
                pgid,
                command: named.command.clone(),
            })
        })
        .collect()
}

/// Every process of the sessions `sids`, read from `/proc`.
#[cfg(target_os = "linux")]
fn processes(sids: &[libc::pid_t]) -> Vec<Proc> {
    let Ok(dir) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    dir.filter_map(|entry| {
        let pid: libc::pid_t = entry.ok()?.file_name().to_str()?.parse().ok()?;
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let (head, tail) = stat.rsplit_once(')')?;
        let comm = head.split_once('(')?.1;
        // state ppid pgrp session ... starttime (field 22).
        let fields: Vec<&str> = tail.split_whitespace().collect();
        let sid = fields.get(3)?.parse().ok()?;
        if !sids.contains(&sid) {
            return None;
        }
        let args = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        let args: Vec<String> = args
            .split(|&b| b == 0)
            .filter(|arg| !arg.is_empty())
            .map(|arg| String::from_utf8_lossy(arg).into_owned())
            .collect();
        Some(Proc {
            pid,
            pgid: fields.get(2)?.parse().ok()?,
            sid,
            start: fields.get(19)?.parse().ok()?,
            zombie: matches!(*fields.first()?, "Z" | "X"),
            command: if args.is_empty() {
                comm.to_owned()
            } else {
                args.join(" ")
            },
        })
    })
    .collect()
}

/// Every process of the sessions `sids`, from `proc_listallpids`; only the
/// program name is read.
#[cfg(target_os = "macos")]
fn processes(sids: &[libc::pid_t]) -> Vec<Proc> {
    // SAFETY: a null buffer asks for the count only.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    let Ok(count) = usize::try_from(count) else {
        return Vec::new();
    };
    // Room for processes started since the count.
    let mut pids: Vec<libc::pid_t> = vec![0; count + 64];
    let bytes = (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
    // SAFETY: `pids` is a writable buffer of `bytes` bytes.
    let listed = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    let Ok(listed) = usize::try_from(listed) else {
        return Vec::new();
    };
    pids.truncate(listed.min(pids.len()));
    pids.into_iter()
        .filter(|&pid| pid > 0)
        .filter_map(|pid| {
            // SAFETY: getsid has no memory arguments.
            let sid = unsafe { libc::getsid(pid) };
            if sid == -1 || !sids.contains(&sid) {
                return None;
            }
            let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
            let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
            // SAFETY: `info` is a writable buffer of exactly `size` bytes.
            let read = unsafe {
                libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size)
            };
            if read != size {
                return None;
            }
            let name = if info.pbi_name[0] != 0 {
                &info.pbi_name[..]
            } else {
                &info.pbi_comm[..]
            };
            let name: Vec<u8> = name
                .iter()
                .take_while(|&&c| c != 0)
                .map(|&c| c as u8)
                .collect();
            Some(Proc {
                pid,
                pgid: info.pbi_pgid as libc::pid_t,
                sid,
                start: info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec,
                zombie: info.pbi_status == libc::SZOMB,
                command: String::from_utf8_lossy(&name).into_owned(),
            })
        })
        .collect()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn processes(_sids: &[libc::pid_t]) -> Vec<Proc> {
    Vec::new()
}

#[cfg(test)]
mod tests;
