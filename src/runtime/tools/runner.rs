//! The spawn core of `run_command` (design D14). It starts one program with a
//! given environment, with stdout and stderr on a single pipe (so the order is
//! kept), stdin on `/dev/null` and the program leading its own process group,
//! and reads what it prints into a [`Capture`].
//!
//! It sits below the allow-list: it runs whatever it is given, so only code
//! that has already judged the program may call it. This is the one tool file
//! the audit lets start a process.

use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::io::AsyncReadExt;
use tokio::net::unix::pipe::Receiver;
use tokio::process::Command;

use crate::core::ai::tools::run::{Capture, Ended};
use crate::runtime::ai_detect::wait_exit_unreaped;
use crate::runtime::proc::GroupKill;

/// One program to start. The environment is cleared first: `env` is all the
/// program sees.
#[derive(Clone, Debug)]
pub struct Run {
    pub program: PathBuf,
    pub argv: Vec<OsString>,
    pub cwd: PathBuf,
    pub env: Vec<(OsString, OsString)>,
    pub timeout: Duration,
}

/// How a run went.
#[derive(Debug)]
pub struct Done {
    pub ended: Ended,
    pub capture: Capture,
    pub duration: Duration,
}

/// Bytes read from the pipe at a time.
const CHUNK: usize = 16 * 1024;

/// Starts `run`, reads its output until it ends and reaps it. Dropping the
/// future kills the whole process group.
pub async fn spawn_core(run: Run) -> io::Result<Done> {
    let started = Instant::now();
    let (reader, writer) = io::pipe()?;
    let mut command = Command::new(&run.program);
    command
        .args(&run.argv)
        .current_dir(&run.cwd)
        .env_clear()
        .envs(run.env.iter().map(|(name, value)| (name, value)))
        .stdin(Stdio::null())
        .stdout(writer.try_clone()?)
        .stderr(writer)
        .kill_on_drop(true)
        .process_group(0);
    let mut child = spawn(&mut command).await?;
    // The command holds the write ends: until it is gone the pipe never ends.
    drop(command);
    let pid = child.id().and_then(|id| libc::pid_t::try_from(id).ok());
    // Declared after `child`, so it drops first and kills the group.
    let Some(mut group) = pid.and_then(GroupKill::new) else {
        return Err(io::Error::other("could not own the process group"));
    };
    let pid = pid.expect("a group exists only for a known pid");
    let mut pipe = Receiver::from_file(File::from(OwnedFd::from(reader)))?;

    let mut capture = Capture::new();
    let mut buf = vec![0u8; CHUNK];
    let mut open = true;
    let exit = wait_exit_unreaped(pid);
    let deadline = tokio::time::sleep(run.timeout);
    tokio::pin!(exit, deadline);
    let mut timed_out = false;
    loop {
        tokio::select! {
            read = pipe.read(&mut buf), if open => match read {
                Ok(n) if n > 0 => capture.push(&buf[..n]),
                _ => open = false,
            },
            _ = &mut exit => break,
            _ = &mut deadline => {
                timed_out = true;
                break;
            }
        }
    }
    // The leader is still unreaped, so its group is still ours to kill: on a
    // timeout, and on a normal exit too, for what the leader left behind.
    let status = group.kill_and_reap(&mut child).await?;
    if open {
        drain(&mut pipe, &mut capture, &mut buf).await;
    }
    Ok(Done {
        ended: Ended {
            exit: status.code(),
            signal: status.signal(),
            timed_out,
            cancelled: false,
        },
        capture,
        duration: started.elapsed(),
    })
}

/// How long the pipe is read after the group is killed. What the program wrote
/// last is still in the pipe; a grandchild that left the group and kept the
/// write end open must not hold the run (a documented limit).
const DRAIN: Duration = Duration::from_millis(500);

/// Reads what is left in the pipe, up to its end or [`DRAIN`].
async fn drain(pipe: &mut Receiver, capture: &mut Capture, buf: &mut [u8]) {
    let rest = async {
        loop {
            match pipe.read(buf).await {
                Ok(n) if n > 0 => capture.push(&buf[..n]),
                _ => break,
            }
        }
    };
    let _ = tokio::time::timeout(DRAIN, rest).await;
}

/// A binary that was written a moment ago fails with ETXTBSY while another
/// fork still holds its write handle: that one error is retried briefly.
async fn spawn(command: &mut Command) -> io::Result<tokio::process::Child> {
    let mut attempt = 0;
    loop {
        match command.spawn() {
            Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) && attempt < 4 => {
                attempt += 1;
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            other => return other,
        }
    }
}

#[cfg(test)]
mod tests;
