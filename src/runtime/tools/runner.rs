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
use std::future::Future;
use std::io;
use std::io::Read;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::AsyncReadExt;
use tokio::net::unix::pipe::Receiver;
use tokio::process::Command;

use crate::core::ai::jail::{
    JailArgs, JailSpec, NOT_ENFORCED_MARKER, NetBind, Rlimits, STATUS_FD, STATUS_FD_VAR,
};
use crate::core::ai::toolchain::env::scrub;
use crate::core::ai::tools::allow::Program;
use crate::core::ai::tools::run::{Capture, Ended};
use crate::runtime::ai_detect::wait_exit_unreaped;
use crate::runtime::proc::GroupKill;
use crate::runtime::tools::integrity::{self, Change};
use crate::runtime::tools::scratch::Scratch;

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

/// A program the allow-list let through, ready to run in the toolchain jail.
#[derive(Debug)]
pub struct Job {
    /// The `lazarobox` binary that serves `__jail-exec`.
    pub helper: PathBuf,
    /// The rules for the run (`toolchain_spec`); [`profile`] adds the rest.
    pub spec: JailSpec,
    pub tool: Program,
    /// The canonical program and its argv (the allow-list already judged them).
    pub program: PathBuf,
    pub argv: Vec<OsString>,
    /// The canonical project root: the one `rwx` folder.
    pub root: PathBuf,
    pub cwd: PathBuf,
    /// The private `TMPDIR`, already in `spec.rw`. Removed when the run ends.
    pub scratch: Scratch,
    pub timeout: Duration,
    /// Where the changes of a run that is dropped (cancelled) before it ends
    /// go. Keep a clone and [`Unreported::drain`] it after each call.
    pub unreported: Unreported,
}

/// The watched-place changes of runs that were dropped before they could
/// return a [`Ran`]. A cancelled `run_jailed` takes its after-snapshot as it
/// is dropped and pushes the diff here; X-d1 drains it (it is a shared handle:
/// clone it into the [`Job`], keep the clone) and treats what it finds like
/// the `changes` of a run that finished. A run that finishes never pushes.
///
/// The drop-time snapshot reads the file system, so it runs on its own
/// detached thread, never on the runtime's: the sink may fill shortly after
/// the drop, not at it. Drain it again later (before a commit or a push, say)
/// and not only right after the cancellation. If that thread cannot start, a
/// [`Kind::Unchecked`] change is pushed instead.
#[derive(Clone, Debug, Default)]
pub struct Unreported(Arc<Mutex<Vec<Change>>>);

impl Unreported {
    /// Takes every change handed over so far, oldest first.
    pub fn drain(&self) -> Vec<Change> {
        std::mem::take(&mut *self.lock())
    }

    fn push(&self, changes: Vec<Change>) {
        self.lock().extend(changes);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Change>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Holds the "before" of a run. If the run is dropped (or fails) before it
/// took its own "after", this takes it as it drops, on a detached thread (the
/// drop may run on a runtime thread, which must not block on the file
/// system): the group is already signalled by then (the spawn future drops
/// first), so what a program wrote before it died is seen; a write in the
/// instant between the signal and the death can still be missed (a
/// documented limit). The result reaches [`Job::unreported`] when the thread
/// is done.
pub(crate) struct AfterOnDrop {
    root: PathBuf,
    /// Git dirs outside the root that are watched too (a linked worktree's).
    extra: Vec<PathBuf>,
    before: Option<integrity::Snapshot>,
    unreported: Unreported,
}

impl AfterOnDrop {
    pub(crate) fn new(
        root: PathBuf,
        extra: Vec<PathBuf>,
        before: integrity::Snapshot,
        unreported: Unreported,
    ) -> Self {
        Self {
            root,
            extra,
            before: Some(before),
            unreported,
        }
    }

    /// The "before", for a run that takes its own "after".
    pub(crate) fn disarm(&mut self) -> integrity::Snapshot {
        self.before.take().unwrap_or_default()
    }
}

impl Drop for AfterOnDrop {
    fn drop(&mut self) {
        let Some(before) = self.before.take() else {
            return;
        };
        let (root, extra) = (self.root.clone(), self.extra.clone());
        let sink = self.unreported.clone();
        let spawned = std::thread::Builder::new()
            .name("lbx-after-snapshot".to_owned())
            .spawn(move || {
                let changes =
                    integrity::compare(&before, &integrity::snapshot_with_git_dirs(&root, &extra));
                if !changes.is_empty() {
                    sink.push(changes);
                }
            });
        if spawned.is_err() {
            // Fail closed: nothing was compared.
            self.unreported.push(vec![Change {
                path: PathBuf::from(".git"),
                kind: integrity::Kind::Unchecked,
                before: None,
                after: Some(b"could not take the after-snapshot of a dropped run".to_vec()),
                mode: (None, None),
            }]);
        }
    }
}

/// The private folders of a run. Whatever way the run ends they are removed:
/// by [`Cleanup::finish`] when it ended normally (the caller runs it off the
/// runtime), and by a detached thread when the guard is dropped (the run was
/// cancelled or failed to start), so no runtime thread waits on the removal.
struct Cleanup(Vec<Scratch>);

impl Cleanup {
    fn take(&mut self) -> Vec<Scratch> {
        std::mem::take(&mut self.0)
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        let folders = self.take();
        if folders.is_empty() {
            return;
        }
        // If no thread can start, the closure (and the folders) drop here:
        // the removal then runs inline rather than not at all.
        let _ = std::thread::Builder::new()
            .name("lbx-scratch-cleanup".to_owned())
            .spawn(move || drop(folders));
    }
}

/// A finished jailed run: how it went and which watched places in the root it
/// changed (X-d1 shows them with a diff and offers to put them back).
#[derive(Debug)]
pub struct Ran {
    pub done: Done,
    pub changes: Vec<Change>,
    /// What of the private folders could not be removed (their paths): say
    /// so, do not assume they are gone.
    pub leftovers: Vec<PathBuf>,
    /// What the helper said on its status channel when it could not apply the
    /// rules. The program cannot write there, so unlike its output this can
    /// be believed.
    pub not_enforced: Option<String>,
}

/// The two settings every toolchain run gets, whatever the spec says: TCP
/// listening open (a dev server) and the core, file-size and CPU limits.
pub fn profile(spec: JailSpec) -> JailSpec {
    JailSpec {
        net_bind: NetBind::Open,
        rlimits: Rlimits::Toolchain,
        ..spec
    }
}

/// Runs `job` through the jail helper with a scrubbed environment. `read`
/// stands in for the parent's environment. The private folders are made
/// before the program starts and removed after it (or if the future is
/// dropped); the watched places of the root are read before and after.
pub async fn run_jailed(
    job: Job,
    read: &(dyn Fn(&str) -> Option<OsString> + Sync),
) -> io::Result<Ran> {
    let tmp = job.scratch.path().to_path_buf();
    let mut cleanup = Cleanup(vec![job.scratch]);
    let mut env = scrub(read, &job.root, &tmp);
    // `go run` and `go test` execute what they build, and the private TMPDIR
    // never executes: they get a folder under the `rwx` root.
    if job.tool == Program::Go {
        let gotmp = Scratch::unique(&job.root, ".lbx-gotmp")?;
        env.push((OsString::from("GOTMPDIR"), gotmp.path().into()));
        cleanup.0.push(gotmp);
    }
    let args = JailArgs {
        min_abi: None,
        // The proxies (`cargo`, `go`) run under their own name, whatever they
        // canonicalize to.
        arg0: Some(OsString::from(job.tool.name())),
        spec: profile(job.spec),
        program: job.program,
        argv: job.argv,
    };
    let mut guard = AfterOnDrop::new(
        job.root.clone(),
        Vec::new(),
        watched(&job.root, &[]).await,
        job.unreported,
    );
    // The helper's own channel: the program is started with it closed.
    let (status, report) = io::pipe()?;
    let done = spawn_with(
        Run {
            program: job.helper,
            argv: args.render(),
            cwd: job.cwd,
            env,
            timeout: job.timeout,
        },
        Some(report),
    )
    .await?;
    let not_enforced = said(status);
    let after = watched(&job.root, &[]).await;
    let changes = integrity::compare(&guard.disarm(), &after);
    let folders = cleanup.take();
    let leftovers = tokio::task::spawn_blocking(move || {
        folders
            .into_iter()
            .flat_map(Scratch::finish)
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    Ok(Ran {
        done,
        changes,
        leftovers,
        not_enforced,
    })
}

/// What the helper wrote on its status pipe, when it is the "not enforced"
/// line. Called after the run ended, so every writer is gone and the read ends
/// at once; it never waits.
fn said(reader: io::PipeReader) -> Option<String> {
    crate::runtime::proc::nonblocking(&reader);
    let mut text = Vec::new();
    // At most a line is read; an error or "would block" is "nothing said".
    let _ = reader.take(4096).read_to_end(&mut text);
    String::from_utf8_lossy(&text)
        .lines()
        .find(|line| line.starts_with(NOT_ENFORCED_MARKER))
        .map(str::to_owned)
}

/// [`integrity::snapshot_with_git_dirs`] off the event loop.
pub(crate) async fn watched(root: &Path, extra: &[PathBuf]) -> integrity::Snapshot {
    let (root, extra) = (root.to_path_buf(), extra.to_vec());
    tokio::task::spawn_blocking(move || integrity::snapshot_with_git_dirs(&root, &extra))
        .await
        .unwrap_or_default()
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
    spawn_with(run, None).await
}

/// [`spawn_core`], handing the program `status` as descriptor [`STATUS_FD`]
/// (its number in [`STATUS_FD_VAR`]) for the jail helper to report on.
async fn spawn_with(run: Run, status: Option<io::PipeWriter>) -> io::Result<Done> {
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
    if let Some(status) = &status {
        command.env(STATUS_FD_VAR, STATUS_FD.to_string());
        crate::runtime::proc::hand_over(&mut command, status.as_raw_fd(), STATUS_FD);
    }
    let mut child = spawn(&mut command).await?;
    // The command holds the write ends: until it is gone the pipe never ends.
    drop(command);
    drop(status);
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
    let status = bounded(group.kill_and_reap(&mut child), REAP).await?;
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

/// How long the final kill-and-reap may take. After SIGKILL it is immediate;
/// a process stuck in the kernel is not, and must not hold the caller.
const REAP: Duration = Duration::from_secs(5);

/// `wait`, or an error when it takes more than `limit`.
async fn bounded<T>(wait: impl Future<Output = io::Result<T>>, limit: Duration) -> io::Result<T> {
    match tokio::time::timeout(limit, wait).await {
        Ok(done) => done,
        Err(_) => Err(io::Error::other(
            "the program did not end after it was killed",
        )),
    }
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
