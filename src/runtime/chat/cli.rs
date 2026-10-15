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
    io::{self, Read, Seek, SeekFrom, Write},
    os::unix::fs::{DirBuilderExt, FileExt, MetadataExt, OpenOptionsExt, PermissionsExt},
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
            adapters::{EnvValue, Job},
            cli::{Exit, STDERR_CAP, verdict},
            engine::{ChatError, StreamEvent},
            stream::StreamDecoder,
        },
        jail::{
            CliLayout, ELF_HEAD_BYTES, Interpreter, JailArgs, JailPolicy, JailSupport, Launch,
            elf_interp_at, interp_path, jail_spec_with, launch, parse_shebang,
            rpath::{homebrew_openssl_files, search_dirs},
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
    /// `$CODEX_HOME`, when it is an absolute path: codex keeps its login and
    /// state there instead of in `~/.codex`.
    pub codex_home: Option<PathBuf>,
    /// Variables set for the child before the scrub runs. Empty in the app;
    /// the tests use it to give the child something to scrub, because the
    /// test process cannot change its own environment safely.
    pub(crate) inject: Vec<(OsString, OsString)>,
}

/// Variables the child never inherits: the agents it could be talked into
/// using (ssh, gpg, askpass helpers) and the state of an outer Claude Code
/// session, which would make the CLI think it runs inside one, and the
/// opencode variables that would load a config, a permission rule set or a
/// config folder of the user's into the chat. Settings
/// (`ANTHROPIC_*`, proxy, certificates, XDG, `HOME`, `PATH`, the D-Bus
/// address a keyring login needs) are kept.
const SCRUBBED: [&str; 11] = [
    "SSH_AUTH_SOCK",
    "SSH_AGENT_PID",
    "SSH_ASKPASS",
    "GIT_ASKPASS",
    "GPG_AGENT_INFO",
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_SSE_PORT",
    "OPENCODE_CONFIG",
    "OPENCODE_CONFIG_DIR",
    "OPENCODE_PERMISSION",
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
            codex_home: std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute()),
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

/// The `comm` of a process (`self` or a pid), if readable.
fn read_comm(who: &str) -> Option<String> {
    fs::read_to_string(format!("/proc/{who}/comm")).ok()
}

/// Whether `pid` is a live process of this program (same `comm` as ours). A
/// recycled pid that now belongs to something else counts as dead. Our own pid
/// is always alive. (An instance in another PID namespace that shares the data
/// folder looks dead from here.)
#[cfg(test)]
fn is_lazarobox(pid: u32) -> bool {
    alive_by_comm(pid, read_comm("self").as_deref(), &|p| {
        read_comm(&p.to_string())
    })
}

fn alive_by_comm(pid: u32, own: Option<&str>, read: &dyn Fn(u32) -> Option<String>) -> bool {
    pid == std::process::id() || own.is_some_and(|own| read(pid).as_deref() == Some(own))
}

/// [`sweep_stale`] given our own `comm` and a way to read others'. Without our
/// own `comm` nobody can be told apart, so nothing is swept.
fn sweep_with(scratch: &Path, own: Option<&str>, read: &dyn Fn(u32) -> Option<String>) {
    if own.is_some() {
        sweep_stale(scratch, &|pid| alive_by_comm(pid, own, read));
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
        sweep_with(&scratch, read_comm("self").as_deref(), &|p| {
            read_comm(&p.to_string())
        });
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

/// Whether the interpreter at `path` may run a CLI inside the jail. Its
/// canonical path must not be under a scratch folder (`/tmp`, `/var/tmp`, where
/// anyone can plant a program) and neither its folder nor the file may be
/// writable by group or others (they could swap it).
fn trusted_interpreter(path: &Path) -> bool {
    let Some(real) = resolve(path) else {
        return false;
    };
    if real.starts_with("/tmp") || real.starts_with("/var/tmp") {
        return false;
    }
    let closed =
        |p: &Path| fs::metadata(p).is_ok_and(|meta| meta.permissions().mode() & 0o022 == 0);
    closed(&real) && real.parent().is_some_and(closed)
}

/// The interpreter a script asks for by bare name (`#!/usr/bin/env node`): the
/// one installed next to the CLI as it was found (npm, nvm, volta and the like
/// put `node` beside the global packages' `bin` links), else the first on
/// `PATH`. The first on `PATH` can be a different install whose libraries the
/// jail does not grant. A candidate that is not [trusted](trusted_interpreter)
/// is skipped, so the answer can be none.
fn interpreter_for(found: &Path, name: &str, path: &std::ffi::OsStr) -> Option<PathBuf> {
    let beside = found.parent().into_iter().map(Path::to_path_buf);
    let listed = std::env::split_paths(path).filter(|dir| dir.is_absolute());
    beside
        .chain(listed)
        .filter_map(|dir| find_on_path(name, dir.as_os_str()))
        .find(|candidate| trusted_interpreter(candidate))
}

/// The ELF loader (`PT_INTERP`) of the executable at `path`. The program
/// headers are in the first bytes of the file; the string they point to can
/// be anywhere, so it is read from where they say. Only these two small reads
/// happen: the file is an executable, not a credential.
fn read_loader(path: &Path) -> Option<PathBuf> {
    let head = read_head(path, ELF_HEAD_BYTES)?;
    let (offset, size) = elf_interp_at(&head)?;
    let mut file = fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut raw = vec![0u8; size];
    file.read_exact(&mut raw).ok()?;
    interp_path(&raw)
}

/// The library folders the ELF executable at `path` names in its own search
/// path (`$ORIGIN` is its folder). Only small reads of the executable itself.
fn elf_search_dirs(path: &Path) -> Vec<PathBuf> {
    let (Some(head), Some(origin), Ok(file)) = (
        read_head(path, ELF_HEAD_BYTES),
        path.parent(),
        fs::File::open(path),
    ) else {
        return Vec::new();
    };
    search_dirs(&head, origin, &|offset, len| {
        let mut buf = vec![0u8; len];
        let mut filled = 0;
        while filled < len {
            let n = file
                .read_at(&mut buf[filled..], offset.checked_add(filled as u64)?)
                .ok()?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        buf.truncate(filled);
        Some(buf)
    })
}

/// The existing folders among `dirs`, resolved and without repeats.
fn existing_dirs(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for real in dirs.iter().filter_map(|d| resolve(d)) {
        if fs::metadata(&real).is_ok_and(|m| m.is_dir()) && !out.contains(&real) {
            out.push(real);
        }
    }
    out
}

/// The existing regular files among `files`, resolved and without repeats.
fn existing_files(files: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for real in files.iter().filter_map(|f| resolve(f)) {
        if fs::metadata(&real).is_ok_and(|m| m.is_file()) && !out.contains(&real) {
            out.push(real);
        }
    }
    out
}

/// The helper command line that runs `job` jailed, or `None` when the layout
/// cannot be jailed (a path the rules refuse, a CLI that cannot be resolved).
fn jailed(
    env: &CliEnv,
    job: &Job,
    args: &[OsString],
    binary: &Path,
    dirs: &Dirs,
) -> Option<(PathBuf, Vec<OsString>)> {
    let home = resolve(env.home.as_deref()?)?;
    let bin = resolve(binary)?;
    let head = read_head(&bin, ELF_HEAD_BYTES)?;
    let interpreter = match parse_shebang(&head) {
        Some(Interpreter::Path(path)) => Some(resolve(&path)?),
        Some(Interpreter::OnPath(name)) => {
            Some(resolve(&interpreter_for(binary, &name, &env.path)?)?)
        }
        None => None,
    };
    // The file the kernel starts is the interpreter of a script, else the CLI.
    // A loader the file names must resolve to a regular file; anything else
    // (a folder, a pipe, a missing path) refuses the layout. The spec then
    // refuses a broad place such as `/tmp`.
    let loaders = match read_loader(interpreter.as_deref().unwrap_or(&bin)) {
        None => Vec::new(),
        Some(loader) => {
            let real = resolve(&loader)?;
            if !fs::metadata(&real).is_ok_and(|meta| meta.is_file()) {
                return None;
            }
            vec![real]
        }
    };
    // The folders the started program names as its library path, and the
    // OpenSSL files a Homebrew build reads, are read-only rules. Nothing else
    // of an install is opened.
    let named = elf_search_dirs(interpreter.as_deref().unwrap_or(&bin));
    let layout = CliLayout {
        cli: job.cli,
        home,
        codex_home: env.codex_home.as_deref().and_then(resolve),
        binary: bin.clone(),
        interpreter,
        loaders,
        lib_dirs: existing_dirs(&named),
        data_files: existing_files(&homebrew_openssl_files(&named)),
        helpers: Vec::new(),
        cwd: dirs.cwd.clone(),
        tmp: dirs.tmp.clone(),
    };
    let spec = jail_spec_with(&layout, &resolve)
        .ok()?
        .retain_existing(|p| fs::symlink_metadata(p).is_ok());
    // A script starts through its interpreter, which is granted by exact
    // path. Left to the kernel, `#!/usr/bin/env node` would run `env`, which
    // the jail does not grant.
    let rest = args.iter().cloned();
    let (program, argv) = match layout.interpreter {
        Some(interpreter) => (
            interpreter,
            std::iter::once(bin.into_os_string()).chain(rest).collect(),
        ),
        None => (bin, rest.collect()),
    };
    let args = JailArgs {
        min_abi: None,
        spec,
        program,
        argv,
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
    // The jail ignores a relative `$CODEX_HOME`, so the child must not use it.
    let codex_home = inject
        .iter()
        .rev()
        .find(|(k, _)| k == "CODEX_HOME")
        .map(|(_, v)| v.clone())
        .or_else(|| std::env::var_os("CODEX_HOME"));
    if codex_home.is_some_and(|v| !Path::new(&v).is_absolute()) {
        command.env_remove("CODEX_HOME");
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
    /// The job's variables, resolved against the turn's folders.
    env: Vec<(OsString, OsString)>,
    /// Said to the user before the turn starts, when the CLI runs without the
    /// jail it would normally have.
    notice: Option<String>,
}

/// The CLI's argv: the job's own, then the flag and path of the job's file
/// when it has one. The file is written into the turn's private temp folder
/// (0600, new), which the jail lets the CLI read and which is wiped after the
/// turn.
fn full_args(job: &Job, dirs: &Dirs) -> io::Result<Vec<OsString>> {
    let mut argv: Vec<OsString> = job.args.iter().map(OsString::from).collect();
    if let Some(file) = &job.tmp_file {
        let path = dirs.tmp.join(file.name);
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?
            .write_all(file.content.as_bytes())?;
        argv.extend([OsString::from(file.flag), path.into_os_string()]);
    }
    Ok(argv)
}

/// The variables of `job`: constants as they are, and a temp value as a path
/// inside the turn's private temp folder (which the jail lets the CLI write).
fn job_env(job: &Job, dirs: &Dirs) -> Vec<(OsString, OsString)> {
    job.env
        .iter()
        .map(|var| {
            let value = match &var.value {
                EnvValue::Literal(text) => OsString::from(text),
                EnvValue::InTmp(name) => dirs.tmp.join(name).into_os_string(),
            };
            (OsString::from(var.name), value)
        })
        .collect()
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
    let argv = full_args(job, &dirs)
        .map_err(|_| StreamEvent::Error(ChatError::Failed(CANNOT_PREPARE.into())))?;
    let vars = job_env(job, &dirs);
    let reason = match (how, env.jail) {
        (Launch::Jailed, _) => match jailed(env, job, &argv, &binary, &dirs) {
            Some((program, argv)) => {
                return Ok(Plan {
                    program,
                    argv,
                    dirs,
                    env: vars,
                    notice: None,
                });
            }
            // Layered: the flags alone are text-only, so a layout the jail
            // refuses runs bare. Required: no jail, no process.
            None if job.jail == JailPolicy::Required => {
                return Err(StreamEvent::Error(ChatError::Jail));
            }
            None => "its folders cannot be restricted safely",
        },
        (_, JailSupport::Unavailable(why)) => why,
        (_, JailSupport::Available { .. }) => "the jail is off",
    };
    Ok(Plan {
        program: binary,
        argv,
        dirs,
        env: vars,
        notice: Some(format!(
            "{} runs without the jail here: {reason}",
            job.binary
        )),
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
    let mut plan = match plan(env, &job) {
        Ok(plan) => plan,
        Err(event) => return send(tx, event).await,
    };
    if let Some(line) = plan.notice.take() {
        send(tx, StreamEvent::Notice(line)).await;
    }
    // Declared in this order so the group guard drops before the child, and
    // the private folders last.
    // The job's variables come last, so they win over an inherited one.
    let vars: Vec<_> = env.inject.iter().chain(&plan.env).cloned().collect();
    let mut child = match spawn(&plan.program, &plan.argv, &plan.dirs, &vars).await {
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

    // The decoder's own error (a refused tool, malformed data, a failure the
    // CLI reported) is final: the process is stopped now, with no grace to
    // exit. A stream that just ended (`Interrupted`) still waits, because the
    // exit status or the jail marker explains it.
    let decoder_error =
        matches!(&ended.last, Some(StreamEvent::Error(e)) if *e != ChatError::Interrupted);
    let exit = if decoder_error {
        group.kill_and_reap(&mut child).await.ok();
        None
    } else {
        settle(&mut child, &mut group, pid, env.grace).await
    };
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
