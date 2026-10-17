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
            adapters::{Job, Wire},
            cli::{Exit, STDERR_CAP, verdict},
            engine::{ChatError, StreamEvent},
            stream::StreamDecoder,
        },
        jail::{
            Cli, CliLayout, ELF_HEAD_BYTES, Interpreter, JailArgs, JailOff, JailPolicy,
            JailRefusal, JailSpec, JailSupport, Launch, ShimGrant, Untrusted, elf_interp_at,
            interp_path, jail_spec_with, launch, parse_shebang,
            rpath::{homebrew_openssl_files, search_dirs},
        },
        tools::wiring,
    },
    runtime::{
        ai_detect::{self, find_on_path, wait_exit_unreaped},
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
    /// This program's canonical executable, resolved once at startup (inside a
    /// jailed CLI `/proc/self/exe` is node): what an engine runs as its MCP
    /// shim. `None` turns the tools off.
    pub shim: Option<PathBuf>,
    /// Variables set for the child before the scrub runs. Empty in the app;
    /// the tests use it to give the child something to scrub, because the
    /// test process cannot change its own environment safely.
    pub(crate) inject: Vec<(OsString, OsString)>,
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
            codex_home: std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute()),
            shim: std::env::current_exe()
                .and_then(fs::canonicalize)
                .ok()
                .filter(|p| p.is_absolute()),
            inject: Vec::new(),
        }
    }

    /// The same machine with another `PATH` (the process one extended with the
    /// login shell's). Never touches the process environment.
    pub fn with_path(mut self, path: OsString) -> Self {
        self.path = path;
        self
    }

    /// What the child gets set before the scrub: its `PATH` (the one the CLI
    /// was found on, so a script's `#!/usr/bin/env node` and the tools the CLI
    /// starts resolve the same way) and then `inject`, which wins.
    fn child_vars(&self) -> Vec<(OsString, OsString)> {
        let path = (OsString::from("PATH"), self.path.clone());
        std::iter::once(path)
            .chain(self.inject.iter().cloned())
            .collect()
    }
}

/// A private folder: created 0700, or an existing real folder of ours made
/// 0700. A symlink, a file or a foreign owner is refused.
pub(crate) fn ensure_private_dir(path: &Path) -> io::Result<()> {
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
pub(crate) fn read_comm(who: &str) -> Option<String> {
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

pub(crate) fn alive_by_comm(
    pid: u32,
    own: Option<&str>,
    read: &dyn Fn(u32) -> Option<String>,
) -> bool {
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
fn interpreter_trust(path: &Path) -> Result<PathBuf, (PathBuf, Untrusted)> {
    let Some(real) = resolve(path) else {
        return Err((path.to_path_buf(), Untrusted::Unresolvable));
    };
    if real.starts_with("/tmp") || real.starts_with("/var/tmp") {
        return Err((real, Untrusted::Scratch));
    }
    // A file or folder that cannot be read counts as writable, as before.
    let closed = |p: &Path| match fs::metadata(p) {
        Ok(meta) if meta.permissions().mode() & 0o022 == 0 => Ok(()),
        Ok(meta) => Err(Untrusted::Writable {
            path: p.to_path_buf(),
            // SAFETY: `geteuid` has no arguments and cannot fail.
            yours: meta.uid() == unsafe { libc::geteuid() },
        }),
        Err(_) => Err(Untrusted::Writable {
            path: p.to_path_buf(),
            yours: false,
        }),
    };
    if let Err(why) = closed(&real) {
        return Err((real, why));
    }
    match real.parent() {
        Some(dir) => match closed(dir) {
            Ok(()) => Ok(real),
            Err(why) => Err((real, why)),
        },
        None => Err((real, Untrusted::Unresolvable)),
    }
}

/// The real path of the program at `path` when lazarobox may run it on its
/// own account: the [`interpreter_trust`] rule. Run that path, not `path`, so
/// what was checked is what runs.
pub(crate) fn trusted_real_path(path: &Path) -> Option<PathBuf> {
    interpreter_trust(path).ok()
}

/// The interpreter a script asks for by bare name (`#!/usr/bin/env node`): the
/// one installed next to the CLI as it was found (npm, nvm, volta and the like
/// put `node` beside the global packages' `bin` links), else the first on
/// `PATH`. The first on `PATH` can be a different install whose libraries the
/// jail does not grant. A candidate that is not [trusted](interpreter_trust)
/// is skipped; when none is left the refusal names the first one skipped, or
/// the missing name.
fn interpreter_for(
    found: &Path,
    name: &str,
    path: &std::ffi::OsStr,
) -> Result<PathBuf, JailRefusal> {
    let beside = found.parent().into_iter().map(Path::to_path_buf);
    let listed = std::env::split_paths(path).filter(|dir| dir.is_absolute());
    let mut skipped = None;
    for candidate in beside
        .chain(listed)
        .filter_map(|dir| find_on_path(name, dir.as_os_str()))
    {
        match interpreter_trust(&candidate) {
            Ok(real) => return Ok(real),
            Err((path, why)) => {
                skipped.get_or_insert(JailRefusal::UntrustedInterpreter { path, why });
            }
        }
    }
    Err(skipped.unwrap_or_else(|| JailRefusal::NoInterpreter(name.to_owned())))
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

/// The layout of the CLI at `binary`, and the spec the jail would enforce for
/// it, or why the layout cannot be jailed (a path the rules refuse, a CLI
/// that cannot be resolved).
fn jail_plan(
    env: &CliEnv,
    cli: Cli,
    binary: &Path,
    dirs: &Dirs,
    shim: Option<(&Path, &Path)>,
) -> Result<(CliLayout, JailSpec), JailRefusal> {
    let home_dir = env.home.as_deref().ok_or(JailRefusal::NoHome)?;
    let home = resolve(home_dir).ok_or_else(|| JailRefusal::HomeMissing(home_dir.to_path_buf()))?;
    let bin = resolve(binary).ok_or_else(|| JailRefusal::Unreadable(binary.to_path_buf()))?;
    let head =
        read_head(&bin, ELF_HEAD_BYTES).ok_or_else(|| JailRefusal::Unreadable(bin.clone()))?;
    let interpreter = match parse_shebang(&head) {
        Some(Interpreter::Path(path)) => {
            Some(resolve(&path).ok_or(JailRefusal::InterpreterMissing(path))?)
        }
        // `interpreter_for` returns a resolved, trusted path.
        Some(Interpreter::OnPath(name)) => Some(interpreter_for(binary, &name, &env.path)?),
        None => None,
    };
    // The file the kernel starts is the interpreter of a script, else the CLI.
    // A loader the file names must resolve to a regular file; anything else
    // (a folder, a pipe, a missing path) refuses the layout. The spec then
    // refuses a broad place such as `/tmp`.
    let loaders = match read_loader(interpreter.as_deref().unwrap_or(&bin)) {
        None => Vec::new(),
        Some(loader) => {
            let real = resolve(&loader).ok_or_else(|| JailRefusal::BadLoader(loader.clone()))?;
            if !fs::metadata(&real).is_ok_and(|meta| meta.is_file()) {
                return Err(JailRefusal::BadLoader(real));
            }
            vec![real]
        }
    };
    // The folders the started program names as its library path, and the
    // OpenSSL files a Homebrew build reads, are read-only rules. Nothing else
    // of an install is opened.
    let named = elf_search_dirs(interpreter.as_deref().unwrap_or(&bin));
    let shim = shim
        .map(|(exe, socket)| -> Result<ShimGrant, JailRefusal> {
            // The shim and its loader run inside the jail with whatever the
            // CLI is allowed, so they pass the check an interpreter does.
            let untrusted = |(path, why)| JailRefusal::UntrustedInterpreter { path, why };
            interpreter_trust(exe).map_err(untrusted)?;
            let loader = read_loader(exe)
                .map(|l| resolve(&l).ok_or(JailRefusal::BadLoader(l)))
                .transpose()?;
            if let Some(loader) = &loader {
                interpreter_trust(loader).map_err(untrusted)?;
            }
            let socket =
                resolve(socket).ok_or_else(|| JailRefusal::Unreadable(socket.to_path_buf()))?;
            Ok(ShimGrant {
                binary: exe.to_path_buf(),
                loader,
                socket,
            })
        })
        .transpose()?;
    let layout = CliLayout {
        cli,
        home,
        codex_home: env.codex_home.as_deref().and_then(resolve),
        binary: bin.clone(),
        interpreter,
        loaders,
        lib_dirs: existing_dirs(&named),
        data_files: existing_files(&homebrew_openssl_files(&named)),
        helpers: Vec::new(),
        shim,
        cwd: dirs.cwd.clone(),
        tmp: dirs.tmp.clone(),
    };
    let spec =
        jail_spec_with(&layout, &resolve)?.retain_existing(|p| fs::symlink_metadata(p).is_ok());
    Ok((layout, spec))
}

/// Why the CLI at `binary` would run without its jail here, in a few words:
/// the system cannot jail, or its layout is refused. `None` when it can be
/// jailed (or cannot be told). Nothing is started.
pub(crate) fn jail_note(env: &CliEnv, cli: Cli, binary: &Path) -> Option<String> {
    let home = env.home.as_deref();
    if let JailSupport::Unavailable(off) = env.jail {
        return Some(JailRefusal::Unsupported(off).short(home));
    }
    let dirs = Dirs::create(env.data.as_deref()?).ok()?;
    let refusal = jail_plan(env, cli, binary, &dirs, None).err()?;
    Some(refusal.short(home))
}

/// The helper command line that runs `job` jailed, or why the layout cannot
/// be jailed.
fn jailed(
    env: &CliEnv,
    job: &Job,
    args: &[OsString],
    binary: &Path,
    dirs: &Dirs,
    shim: Option<&Wired>,
) -> Result<(PathBuf, Vec<OsString>), JailRefusal> {
    let grant = env.shim.as_deref().zip(shim.map(|w| w.socket.as_path()));
    let (layout, spec) = jail_plan(env, job.cli, binary, dirs, grant)?;
    let bin = layout.binary.clone();
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
    Ok((env.helper.clone(), args.render()))
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
    (cwd, tmp): (&Path, &Path),
    inject: &[(OsString, OsString)],
) -> io::Result<tokio::process::Child> {
    let mut attempt = 0;
    loop {
        match command(program, argv.to_vec(), (cwd, tmp), inject).spawn() {
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
    /// Said to the user before the turn starts, when the CLI runs without the
    /// jail it would normally have.
    notice: Option<String>,
    /// The variables the child gets before the scrub.
    vars: Vec<(OsString, OsString)>,
    /// The engine gets the tool server: the config, the flags and the jail
    /// grant are all in.
    wired: bool,
    /// The working folder when it is not the shared empty one.
    cwd: Option<PathBuf>,
}

impl Plan {
    /// The working folder and the `TMPDIR` the child starts with.
    fn folders(&self) -> (&Path, &Path) {
        let cwd = self.cwd.as_deref().unwrap_or(&self.dirs.cwd);
        (cwd, &self.dirs.tmp)
    }
}

/// The CLI's argv: the job's own, then the flag and path of each of the job's
/// files, in order. Each file is written into the turn's private temp folder
/// (0600, new), which the jail lets the CLI read and which is wiped after the
/// turn.
fn full_args(job: &Job, dirs: &Dirs) -> io::Result<Vec<OsString>> {
    let mut argv: Vec<OsString> = job.args.iter().map(OsString::from).collect();
    for file in &job.tmp_files {
        let path = dirs.tmp.join(file.name);
        write_private(&path, file.content.as_bytes())?;
        argv.extend([OsString::from(file.flag), path.into_os_string()]);
    }
    Ok(argv)
}

/// A new file, 0600, in the turn's private folder.
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?
        .write_all(bytes)
}

/// What the MCP tools add to a turn: flags, variables and the socket the jail
/// must let the engine reach.
struct Wired {
    args: Vec<OsString>,
    vars: Vec<(OsString, OsString)>,
    socket: PathBuf,
    /// Where the engine runs instead of the shared empty folder, when its
    /// config is read from the working folder (gemini).
    cwd: Option<PathBuf>,
}

/// The files and folders `wire_mcp` may have made in the turn's folder. The
/// token is in some of them, so a turn that does not use them removes them
/// (the folder is wiped after the turn anyway).
fn unwrite_mcp(dirs: &Dirs) {
    let _ = fs::remove_file(dirs.tmp.join("mcp.json"));
    let _ = fs::remove_file(dirs.tmp.join(GEMINI_POLICY_FILE));
    let _ = fs::remove_dir_all(dirs.tmp.join(GEMINI_WORK_DIR));
}

/// The private working folder of a wired gemini, in the turn's folder: its
/// `.gemini/settings.json` is the workspace config gemini reads.
const GEMINI_WORK_DIR: &str = "work";
const GEMINI_POLICY_FILE: &str = "gemini-mcp-policy.toml";

/// Writes gemini's workspace settings (0600, in 0700 folders) and the policy
/// that allows our server; the working folder is the one to run in.
fn write_gemini(
    shim: &Path,
    dirs: &Dirs,
    wire: &wiring::McpWire,
) -> Option<(Vec<String>, PathBuf)> {
    let work = dirs.tmp.join(GEMINI_WORK_DIR);
    let policy = dirs.tmp.join(GEMINI_POLICY_FILE);
    let wiring = wiring::gemini(shim, &policy, wire)?;
    let config = work.join(".gemini");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&config)
        .ok()?;
    write_private(&config.join("settings.json"), wiring.settings.as_bytes()).ok()?;
    write_private(&policy, wiring.policy.as_bytes()).ok()?;
    Some((wiring.args, work))
}

/// What the engine is told about the tools, and writes the config file where
/// the engine takes one (claude's holds the token: 0600, in the turn folder
/// that is wiped). `None` when the job has no wire, there is no shim program,
/// the engine has no wiring, or the file cannot be written: the turn then
/// runs as text.
fn wire_mcp(env: &CliEnv, job: &Job, dirs: &Dirs) -> Option<Wired> {
    let wire = job.mcp.as_ref()?;
    let shim = env.shim.as_deref()?;
    let mut cwd = None;
    let (args, vars) = match job.cli {
        Cli::Claude => {
            let path = dirs.tmp.join("mcp.json");
            let wiring = wiring::claude(shim, &path, wire)?;
            write_private(&path, wiring.config.as_bytes()).ok()?;
            let vars = wiring.env.into_iter().map(|(k, v)| (k.into(), v.into()));
            (wiring.args, vars.collect())
        }
        // The token is only in codex's environment: `env_vars` names it, so
        // codex hands it on to the shim.
        Cli::Codex => {
            let wiring = wiring::codex(shim, wire)?;
            let vars = wiring
                .env
                .into_iter()
                .map(|(k, v)| (k.into(), v.reveal().into()));
            (wiring.args, vars.collect())
        }
        // gemini reads the workspace settings of its working folder; the token
        // is in that file only (0600), and in none of its argv or env.
        Cli::Gemini => {
            let Some((args, work)) = write_gemini(shim, dirs, wire) else {
                unwrite_mcp(dirs);
                return None;
            };
            cwd = Some(work);
            (args, Vec::new())
        }
    };
    Some(Wired {
        args: args.into_iter().map(OsString::from).collect(),
        vars,
        socket: wire.socket.clone(),
        cwd,
    })
}

/// Decides how the CLI starts. `Err` is the final event of a turn that never
/// spawns anything.
fn plan(env: &CliEnv, job: &Job) -> Result<Plan, StreamEvent> {
    let how = launch(job.jail, env.jail);
    let home = env.home.as_deref();
    let refused = |why: &JailRefusal| StreamEvent::Error(ChatError::Jail(why.hint(home)));
    if how == Launch::Refuse {
        let JailSupport::Unavailable(off) = env.jail else {
            return Err(refused(&JailRefusal::Unsupported(JailOff::NotProbed)));
        };
        return Err(refused(&JailRefusal::Unsupported(off)));
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
    // The tools are wired only for a jailed run: a bare CLI would reach the
    // shim without the jail that confines what it can do.
    let mut wired = (how == Launch::Jailed)
        .then(|| wire_mcp(env, job, &dirs))
        .flatten();
    let vars = |wired: Option<&Wired>| {
        let mut vars = env.child_vars();
        vars.extend(wired.into_iter().flat_map(|w| w.vars.iter().cloned()));
        vars
    };
    // What the user is told when the job asked for tools and the turn has none.
    let tools_off = |notice: Option<String>| {
        let off = "tools are off for this turn: they could not be set up";
        match (job.mcp.is_some(), notice) {
            (false, notice) => notice,
            (true, None) => Some(off.to_owned()),
            (true, Some(notice)) => Some(format!("{notice}; {off}")),
        }
    };
    let reason = match (how, env.jail) {
        (Launch::Jailed, _) => {
            let mut result = None;
            // With the tools first; if the jail refuses that layout (say the
            // shim program sits in a place it never grants), again without
            // them. The jail is never widened to fit the tools.
            let tries = match wired.take() {
                Some(w) => vec![Some(w), None],
                None => vec![None],
            };
            for attempt in tries {
                let mut full = argv.clone();
                full.extend(attempt.iter().flat_map(|w| w.args.iter().cloned()));
                match jailed(env, job, &full, &binary, &dirs, attempt.as_ref()) {
                    Ok((program, argv)) => {
                        let notice = tools_off(None).filter(|_| attempt.is_none());
                        return Ok(Plan {
                            program,
                            argv,
                            vars: vars(attempt.as_ref()),
                            dirs,
                            notice,
                            wired: attempt.is_some(),
                            cwd: attempt.and_then(|w| w.cwd),
                        });
                    }
                    Err(why) => result = Some(why),
                }
                // The config holds the token: it does not stay for a turn that
                // was not told about it.
                unwrite_mcp(&dirs);
            }
            let why = result.expect("both attempts ran");
            // Layered: the flags alone are text-only, so a layout the jail
            // refuses runs bare. Required: no jail, no process.
            if job.jail == JailPolicy::Required {
                return Err(refused(&why));
            }
            why.hint(home)
        }
        (_, JailSupport::Unavailable(off)) => off.text(),
        (_, JailSupport::Available { .. }) => "the jail is off".to_owned(),
    };
    Ok(Plan {
        program: binary,
        argv,
        vars: vars(None),
        dirs,
        wired: false,
        cwd: None,
        notice: tools_off(Some(format!(
            "{} runs without the jail here: {reason}",
            job.binary
        ))),
    })
}

/// Runs `codex debug models` the way a codex turn starts: the same plan (the
/// jail helper with the codex layout, `Required`: no jail, no process), the
/// same private folders and scrubbed environment. stdin is closed. `Err` is
/// the reason on any failure; stderr is read only for its last line.
pub(crate) async fn run_listing(
    env: &CliEnv,
    limit: Duration,
    cap: usize,
) -> Result<Vec<u8>, String> {
    let job = Job {
        wire: Wire::CodexJsonl,
        binary: "codex",
        cli: Cli::Codex,
        args: vec!["debug".to_owned(), "models".to_owned()],
        stdin: Vec::new(),
        jail: JailPolicy::Required,
        tmp_files: Vec::new(),
        mcp: None,
    };
    let plan = match plan(env, &job) {
        Ok(plan) => plan,
        Err(StreamEvent::Error(ChatError::Jail(why))) => return Err(format!("jail: {why}")),
        Err(StreamEvent::Error(ChatError::Failed(text))) => return Err(text),
        Err(_) => return Err("could not start codex".to_owned()),
    };
    // Defence in depth: `Required` never plans a bare run (it refuses
    // instead), so this is unreachable today. If a future policy change let
    // the plan fall back to a bare run, the listing must not use it.
    if plan.notice.is_some() {
        return Err("jail: the jail is off".to_owned());
    }
    ai_detect::run_capped_why(
        || {
            let mut command = command(
                &plan.program,
                plan.argv.clone(),
                (&plan.dirs.cwd, &plan.dirs.tmp),
                &env.child_vars(),
            );
            command.stderr(Stdio::piped());
            command
        },
        limit,
        cap,
    )
    .await
    .map_err(|failure| failure.text("codex", limit, cap))
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

/// Tells the session of a turn that asked for tools that its engine runs as
/// text after all: nothing will connect, so the socket and token go now.
fn unwire(job: &Job) {
    if let Some(wire) = &job.mcp {
        wire.unwire.call();
    }
}

/// Runs one CLI turn and sends its events to `tx`. Exactly one `Done` or
/// `Error` ends it, unless it is cancelled: dropping the future (or the
/// receiver) kills the process group and sends nothing more.
pub async fn process_turn(env: &CliEnv, job: Job, tx: &mpsc::Sender<StreamEvent>) {
    let mut plan = match plan(env, &job) {
        Ok(plan) => plan,
        Err(event) => {
            unwire(&job);
            return send(tx, event).await;
        }
    };
    if !plan.wired {
        unwire(&job);
    }
    if let Some(line) = plan.notice.take() {
        send(tx, StreamEvent::Notice(line)).await;
    }
    // Declared in this order so the group guard drops before the child, and
    // the private folders last.
    let mut child = match spawn(&plan.program, &plan.argv, plan.folders(), &plan.vars).await {
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

    let mut decoder = fresh_decoder(job.wire, plan.wired);
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
    // The token of this turn's session is shown nowhere, not even if the CLI
    // echoes its config on stderr.
    let stderr = String::from_utf8_lossy(&tail);
    let stderr = match job.mcp.as_ref().map(|wire| wire.token.reveal()) {
        Some(token) if !token.is_empty() => stderr.replace(token, "(..)"),
        _ => stderr.into_owned(),
    };
    let last = verdict(job.binary, ended.last, ended.answered, exit, &stderr);
    send(tx, last).await;
}

#[cfg(test)]
mod tests;
