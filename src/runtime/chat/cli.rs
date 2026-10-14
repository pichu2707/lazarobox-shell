//! One turn of a CLI engine as a child process.
//!
//! - No shell: the program is the absolute path `find_on_path` found (or the
//!   jail helper) and every argument is one `argv` element.
//! - stdin gets the transcript and is then closed. The history never goes into
//!   `argv`, which `ps` shows.
//! - The working directory is a private empty folder and `TMPDIR` a private
//!   per-turn one, so a project file (`CLAUDE.md`, `AGENTS.md`) cannot steer
//!   the CLI and a jailed CLI has somewhere to write.
//! - The child leads its own process group. Dropping the turn (a cancel) or
//!   any way out of it kills the whole group.
//! - stdout is decoded as it arrives; the final event is held back until the
//!   process has exited, because a failing exit or the jail marker overrules a
//!   `Done` (see `core::ai::chat::cli::verdict`).
//! - Nothing here opens a credential file. The jail layout is built from
//!   paths and `symlink_metadata`/`canonicalize` only; the only file content
//!   read is the first bytes of the CLI itself (its shebang or ELF header).

use std::{
    ffi::OsString,
    fs,
    io::{self, Read},
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use futures::FutureExt;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{ChildStderr, ChildStdin, ChildStdout, Command},
    sync::mpsc,
    time::timeout,
};

use super::fresh_decoder;
use crate::{
    core::ai::{
        chat::{
            adapters::Job,
            cli::{Exit, STDERR_CAP, verdict},
            engine::{ChatError, StreamEvent},
            stream::StreamDecoder,
        },
        jail::{
            CliLayout, ELF_HEAD_BYTES, Interpreter, JailArgs, JailPolicy, JailSupport, Launch,
            elf_loader, jail_spec_with, launch, parse_shebang,
        },
    },
    runtime::{
        ai_detect::{find_on_path, wait_exit_unreaped},
        chat_store,
        jail::resolve,
        loopback::{Stop, TOO_LONG, Timeouts, Wait},
        proc::GroupKill,
    },
};

/// Size of one read from the child's pipes.
const READ_SIZE: usize = 16 * 1024;
/// How long a CLI gets to exit once it has said it is done (or closed stdout)
/// before its group is killed.
pub const EXIT_GRACE: Duration = Duration::from_secs(2);

pub const CANNOT_PREPARE: &str = "cannot prepare the private working directory";

/// Everything a CLI turn needs from the machine. Built once at startup; the
/// tests build their own.
#[derive(Clone, Debug)]
pub struct CliEnv {
    /// The `PATH` the CLI is looked up in.
    pub path: OsString,
    /// `$HOME`, when it is an absolute path.
    pub home: Option<PathBuf>,
    /// `<data home>/lazarobox`, where the private folders live.
    pub data: Option<PathBuf>,
    pub jail: JailSupport,
    /// The program that applies the jail and execs the CLI.
    pub helper: PathBuf,
    pub timeouts: Timeouts,
    /// See [`EXIT_GRACE`].
    pub grace: Duration,
    /// Variables set for the child before the scrub runs. Empty in the app;
    /// the tests use it to give the child something to scrub, because the
    /// test process cannot change its own environment safely.
    pub inject: Vec<(OsString, OsString)>,
}

/// Variables the child never inherits: the agents it could be talked into
/// using (ssh, gpg, askpass helpers) and the state of an outer Claude Code
/// session, which would make the CLI think it runs inside one. Settings
/// (`ANTHROPIC_*`, proxy, certificates, XDG, `HOME`, `PATH`, the D-Bus
/// address a keyring login needs) are kept.
const SCRUBBED: [&str; 8] = [
    "SSH_AUTH_SOCK",
    "SSH_AGENT_PID",
    "SSH_ASKPASS",
    "GIT_ASKPASS",
    "GPG_AGENT_INFO",
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_SSE_PORT",
];

impl CliEnv {
    /// The real machine: the process environment and `/proc/self/exe` as the
    /// jail helper (it survives an upgrade of the binary on disk).
    pub fn from_env(jail: JailSupport) -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|h| h.is_absolute());
        let data = chat_store::resolve_path(
            std::env::var_os("XDG_DATA_HOME").as_deref(),
            std::env::var_os("HOME").as_deref(),
        )
        .and_then(|db| db.parent().map(Path::to_path_buf));
        Self {
            path: std::env::var_os("PATH").unwrap_or_default(),
            home,
            data,
            jail,
            helper: PathBuf::from("/proc/self/exe"),
            timeouts: Timeouts::CLI,
            grace: EXIT_GRACE,
            inject: Vec::new(),
        }
    }
}

/// A private folder: created 0700, or an existing real folder of ours made
/// 0700. A symlink, a file or a foreign owner is refused.
fn ensure_private_dir(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            // SAFETY: geteuid has no preconditions and cannot fail.
            let me = unsafe { libc::geteuid() };
            if !meta.is_dir() || meta.uid() != me {
                return Err(io::Error::other("not a private folder"));
            }
            if meta.permissions().mode() & 0o777 != 0o700 {
                fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
            }
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path),
        Err(e) => Err(e),
    }
}

/// Removes its folder when dropped: the turn ended, was cancelled or failed.
struct TmpDir(PathBuf);

impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Whether `pid` is a live process of this program (same `comm` as ours). A
/// recycled pid that now belongs to something else counts as dead.
fn is_lazarobox(pid: u32) -> bool {
    let comm = |who: &str| fs::read_to_string(format!("/proc/{who}/comm")).ok();
    match (comm("self"), comm(&pid.to_string())) {
        (Some(ours), Some(theirs)) => ours == theirs,
        _ => false,
    }
}

/// Best effort: removes the turn folders (`<pid>-<n>`) of runs that are gone
/// (crashed or killed before they could wipe their own). A folder whose pid
/// `alive` accepts, or whose name is not of that shape, stays.
fn sweep_stale(scratch: &Path, alive: &dyn Fn(u32) -> bool) {
    let Ok(entries) = fs::read_dir(scratch) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let pid = name
            .to_str()
            .and_then(|n| n.split_once('-'))
            .and_then(|(pid, turn)| turn.bytes().all(|b| b.is_ascii_digit()).then_some(pid))
            .and_then(|pid| pid.parse::<u32>().ok());
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        if pid.is_some_and(|pid| is_dir && !alive(pid)) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

static TURNS: AtomicU64 = AtomicU64::new(0);

/// The private folders of one turn.
struct Dirs {
    cwd: PathBuf,
    tmp: PathBuf,
    _guard: TmpDir,
}

impl Dirs {
    fn create(data: &Path) -> io::Result<Self> {
        ensure_private_dir(data)?;
        let cwd = data.join("cli-cwd");
        let scratch = data.join("cli-tmp");
        ensure_private_dir(&cwd)?;
        ensure_private_dir(&scratch)?;
        sweep_stale(&scratch, &is_lazarobox);
        let own = scratch.join(format!(
            "{}-{}",
            std::process::id(),
            TURNS.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&own)?;
        let guard = TmpDir(own.clone());
        Ok(Self {
            cwd: fs::canonicalize(cwd)?,
            tmp: fs::canonicalize(own)?,
            _guard: guard,
        })
    }
}

/// Reads at most `max` bytes from the start of a file.
fn read_head(path: &Path, max: usize) -> Option<Vec<u8>> {
    let mut head = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(max as u64)
        .read_to_end(&mut head)
        .ok()?;
    Some(head)
}

/// The helper command line that runs `job` jailed, or `None` when the layout
/// cannot be jailed (a path the rules refuse, a CLI that cannot be resolved).
fn jailed(env: &CliEnv, job: &Job, binary: &Path, dirs: &Dirs) -> Option<(PathBuf, Vec<OsString>)> {
    let home = resolve(env.home.as_deref()?)?;
    let bin = resolve(binary)?;
    let head = read_head(&bin, ELF_HEAD_BYTES)?;
    let interpreter = match parse_shebang(&head) {
        Some(Interpreter::Path(path)) => Some(resolve(&path)?),
        Some(Interpreter::OnPath(name)) => Some(resolve(&find_on_path(&name, &env.path)?)?),
        None => None,
    };
    // The file the kernel starts is the interpreter of a script, else the CLI.
    let starter = read_head(interpreter.as_deref().unwrap_or(&bin), ELF_HEAD_BYTES)?;
    let loaders = elf_loader(&starter)
        .and_then(|loader| resolve(&loader))
        .into_iter()
        .collect();
    let layout = CliLayout {
        cli: job.cli,
        home,
        codex_home: None,
        binary: bin.clone(),
        interpreter,
        loaders,
        helpers: Vec::new(),
        cwd: dirs.cwd.clone(),
        tmp: dirs.tmp.clone(),
    };
    let spec = jail_spec_with(&layout, &resolve)
        .ok()?
        .retain_existing(|p| fs::symlink_metadata(p).is_ok());
    let args = JailArgs {
        min_abi: None,
        spec,
        program: bin,
        argv: job.args.iter().map(OsString::from).collect(),
    };
    Some((env.helper.clone(), args.render()))
}

/// The child process: no shell, the private folders, a scrubbed environment
/// and a process group of its own.
fn command(
    program: &Path,
    argv: Vec<OsString>,
    (cwd, tmp): (&Path, &Path),
    inject: &[(OsString, OsString)],
) -> Command {
    let mut command = Command::new(program);
    command
        .args(argv)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.envs(inject.iter().map(|(k, v)| (k, v)));
    for name in SCRUBBED {
        command.env_remove(name);
    }
    command
        .env("NO_COLOR", "1")
        .env("TMPDIR", tmp)
        .kill_on_drop(true)
        .process_group(0);
    command
}

/// Spawns, retrying the one error a binary that is being written gives.
async fn spawn(
    program: &Path,
    argv: &[OsString],
    dirs: &Dirs,
    inject: &[(OsString, OsString)],
) -> io::Result<tokio::process::Child> {
    let mut attempt = 0;
    loop {
        match command(program, argv.to_vec(), (&dirs.cwd, &dirs.tmp), inject).spawn() {
            Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) && attempt < 4 => {
                attempt += 1;
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            other => return other,
        }
    }
}

async fn send(tx: &mpsc::Sender<StreamEvent>, event: StreamEvent) {
    let _ = tx.send(event).await;
}

async fn fail(tx: &mpsc::Sender<StreamEvent>, text: String) {
    send(tx, StreamEvent::Error(ChatError::Failed(text))).await;
}

/// What the turn runs: the program and its argv, and the private folders.
struct Plan {
    program: PathBuf,
    argv: Vec<OsString>,
    dirs: Dirs,
}

/// Decides how the CLI starts. `Err` is the final event of a turn that never
/// spawns anything.
fn plan(env: &CliEnv, job: &Job) -> Result<Plan, StreamEvent> {
    let how = launch(job.jail, env.jail);
    if how == Launch::Refuse {
        return Err(StreamEvent::Error(ChatError::Jail));
    }
    let Some(binary) = find_on_path(job.binary, &env.path) else {
        let text = format!("{} was not found on PATH", job.binary);
        return Err(StreamEvent::Error(ChatError::Failed(text)));
    };
    let dirs = env
        .data
        .as_deref()
        .and_then(|data| Dirs::create(data).ok())
        .ok_or_else(|| StreamEvent::Error(ChatError::Failed(CANNOT_PREPARE.into())))?;
    let argv: Vec<OsString> = job.args.iter().map(OsString::from).collect();
    if how == Launch::Jailed {
        match jailed(env, job, &binary, &dirs) {
            Some((program, argv)) => {
                return Ok(Plan {
                    program,
                    argv,
                    dirs,
                });
            }
            // Layered: the flags alone are text-only, so a layout the jail
            // refuses runs bare. Required: no jail, no process.
            None if job.jail == JailPolicy::Required => {
                return Err(StreamEvent::Error(ChatError::Jail));
            }
            None => {}
        }
    }
    Ok(Plan {
        program: binary,
        argv,
        dirs,
    })
}

/// Writes the transcript, then drops the pipe: the CLI sees EOF.
async fn write_stdin(mut stdin: ChildStdin, bytes: &[u8]) {
    // A CLI that exits without reading closes the pipe: not an error here, the
    // exit status tells.
    let _ = stdin.write_all(bytes).await;
    let _ = stdin.shutdown().await;
}

/// Reads stderr to its end, keeping the last `STDERR_CAP` bytes.
async fn read_stderr(mut stderr: ChildStderr) -> Vec<u8> {
    let mut tail = Vec::new();
    let mut buf = vec![0u8; READ_SIZE];
    while let Ok(n) = stderr.read(&mut buf).await {
        if n == 0 {
            break;
        }
        tail.extend_from_slice(&buf[..n]);
        if tail.len() > STDERR_CAP {
            tail.drain(..tail.len() - STDERR_CAP);
        }
    }
    tail
}

/// How the output ended: the decoder's final event (held back) and whether
/// any answer text went out.
struct Relayed {
    last: Option<StreamEvent>,
    answered: bool,
}

/// Decodes stdout and forwards every event but the final one.
async fn relay(
    stdout: &mut ChildStdout,
    timeouts: &Timeouts,
    decoder: &mut (dyn StreamDecoder + Send),
    tx: &mpsc::Sender<StreamEvent>,
) -> Result<Relayed, Stop> {
    let mut wait = Wait::new(timeouts);
    let mut buf = vec![0u8; READ_SIZE];
    let mut answered = false;
    loop {
        let n = wait.read(stdout, &mut buf, tx).await?;
        let events = if n == 0 {
            decoder.finish()
        } else {
            decoder.feed(&buf[..n])
        };
        for event in events {
            if matches!(event, StreamEvent::Done(_) | StreamEvent::Error(_)) {
                return Ok(Relayed {
                    last: Some(event),
                    answered,
                });
            }
            answered |= matches!(event, StreamEvent::Delta(_));
            tx.send(event).await.map_err(|_| Stop::Cancelled)?;
        }
        if n == 0 {
            return Ok(Relayed {
                last: None,
                answered,
            });
        }
    }
}

/// Lets the leader exit by itself for up to `grace`, then kills the group and
/// reaps. `Some` only for a leader that exited on its own.
async fn settle(
    child: &mut tokio::process::Child,
    group: &mut GroupKill,
    pid: libc::pid_t,
    grace: Duration,
) -> Option<Exit> {
    let exited = matches!(timeout(grace, wait_exit_unreaped(pid)).await, Ok(Some(_)));
    // The leader is still unreaped here, so its group id is still ours: kill
    // what it left behind before reaping.
    let status = group.kill_and_reap(child).await.ok();
    status.filter(|_| exited).map(|status| Exit {
        code: status.code(),
    })
}

/// Runs one CLI turn and sends its events to `tx`. Exactly one `Done` or
/// `Error` ends it, unless it is cancelled: dropping the future (or the
/// receiver) kills the process group and sends nothing more.
pub async fn process_turn(env: &CliEnv, job: Job, tx: &mpsc::Sender<StreamEvent>) {
    let plan = match plan(env, &job) {
        Ok(plan) => plan,
        Err(event) => return send(tx, event).await,
    };
    // Declared in this order so the group guard drops before the child, and
    // the private folders last.
    let mut child = match spawn(&plan.program, &plan.argv, &plan.dirs, &env.inject).await {
        Ok(child) => child,
        Err(e) => {
            let text = format!("could not start {}: {}", job.binary, e.kind());
            return fail(tx, text).await;
        }
    };
    let pid = child.id().map(|p| p as libc::pid_t);
    let (Some(mut group), Some(stdin), Some(stdout), Some(stderr)) = (
        pid.and_then(GroupKill::new),
        child.stdin.take(),
        child.stdout.take(),
        child.stderr.take(),
    ) else {
        // `kill_on_drop` takes the child down.
        return fail(tx, format!("could not start {}", job.binary)).await;
    };
    let pid = pid.expect("a group exists only for a known pid");
    let mut stdout = stdout;

    let mut decoder = fresh_decoder(job.wire);
    let mut stdin_task = std::pin::pin!(write_stdin(stdin, &job.stdin).fuse());
    let mut stderr_task = std::pin::pin!(read_stderr(stderr).fuse());
    let mut stderr_tail = None;
    let relayed = {
        let mut relaying = std::pin::pin!(timeout(
            env.timeouts.total,
            relay(&mut stdout, &env.timeouts, &mut *decoder, tx)
        ));
        loop {
            tokio::select! {
                done = &mut relaying => break done,
                () = &mut stdin_task => {}
                tail = &mut stderr_task => stderr_tail = Some(tail),
            }
        }
    };
    let ended = match relayed {
        Ok(Ok(relayed)) => relayed,
        Ok(Err(Stop::Cancelled)) => {
            group.kill_and_reap(&mut child).await.ok();
            return;
        }
        Ok(Err(stop)) => {
            group.kill_and_reap(&mut child).await.ok();
            let error = match stop {
                Stop::Failed(text) => ChatError::Failed(text.into()),
                Stop::Error(error) => error,
                Stop::Cancelled => return,
            };
            return send(tx, StreamEvent::Error(error)).await;
        }
        Err(_) => {
            group.kill_and_reap(&mut child).await.ok();
            return fail(tx, TOO_LONG.into()).await;
        }
    };

    let exit = settle(&mut child, &mut group, pid, env.grace).await;
    // The group is dead and reaped: the pipes reach EOF at once.
    let tail = match stderr_tail {
        Some(tail) => tail,
        None => timeout(Duration::from_millis(500), &mut stderr_task)
            .await
            .unwrap_or_default(),
    };
    let stderr = String::from_utf8_lossy(&tail);
    let last = verdict(job.binary, ended.last, ended.answered, exit, &stderr);
    send(tx, last).await;
}

#[cfg(test)]
mod tests;
