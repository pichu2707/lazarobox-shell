//! Runtime probes for AI engine detection: loopback HTTP, PATH lookup and
//! `--version`. The facts are classified by the pure `core::ai::reduce`.

use std::{
    collections::HashMap,
    ffi::OsStr,
    fs,
    net::SocketAddr,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    process::Command,
    sync::mpsc,
    time::timeout,
};

use futures::{StreamExt, future::join_all, stream};

use super::{chat::CliEnv, proc::GroupKill};
use crate::core::ai::{
    Detection, Engine, Kind, ModelFacts, Probe, ServerPorts, Status,
    chat::{cli::last_line, engine::ChatError},
    http::{MAX_BODY, parse_response},
    jail::{Cli, NOT_ENFORCED_MARKER, exit_error},
    marks::Tools,
    parse::{self, sanitize},
    reduce,
    sanitize::redact_tokens,
    window,
};

/// Room for the status line and headers above the 1 MiB body cap.
const READ_SLACK: usize = 16 * 1024;

/// Covers connect, write and read of one loopback request.
pub const HTTP_TIMEOUT: Duration = Duration::from_millis(800);

/// Longest `--version` is allowed to run before the child is killed.
pub const VERSION_TIMEOUT: Duration = Duration::from_secs(2);

/// Most `--version` output kept; more than this means the probe gave up.
const VERSION_CAP: usize = 64 * 1024;

/// GETs `path` from `127.0.0.1:port`. `None` on any failure or timeout.
pub async fn http_get(port: u16, path: &'static str) -> Option<(u16, Vec<u8>)> {
    let request = format!(
        "GET {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    timeout(HTTP_TIMEOUT, exchange(port, &request)).await.ok()?
}

/// The only path detection POSTs to, and only on the Ollama port. It is a
/// constant, never a parameter, so no caller can aim this POST elsewhere.
const SHOW_PATH: &str = "/api/show";

/// Longest one `/api/show` call may take.
const SHOW_TIMEOUT: Duration = Duration::from_secs(1);

/// Longest the whole `/api/show` phase may take; the models still unanswered
/// are `Unknown`.
const SHOW_DEADLINE: Duration = Duration::from_secs(2);

/// Most `/api/show` calls in flight at once.
const SHOW_PARALLEL: usize = 4;

/// Most `/api/show` calls of one run (cached models cost none).
const SHOW_MAX_CALLS: usize = 32;

/// Most digests the cache keeps before it is cleared.
const SHOW_CACHE_CAP: usize = 256;

/// `POST /api/show` for `model` to `127.0.0.1:port`: the body holds the model
/// name and nothing else. `None` on any failure or timeout.
pub async fn http_post_show(port: u16, model: &str) -> Option<(u16, Vec<u8>)> {
    let body = serde_json::json!({ "model": model }).to_string();
    let request = format!(
        "POST {SHOW_PATH} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    timeout(SHOW_TIMEOUT, exchange(port, &request)).await.ok()?
}

/// Sends `request` (a whole HTTP/1.0 request) to `127.0.0.1:port`.
async fn exchange(port: u16, request: &str) -> Option<(u16, Vec<u8>)> {
    // HTTP/1.0 with `Connection: close`: the reply ends at EOF. The host is
    // fixed, so no request can leave the machine.
    let mut stream = TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], port)))
        .await
        .ok()?;
    stream.write_all(request.as_bytes()).await.ok()?;
    // Headers fit well inside the slack; a longer reply is cut and rejected.
    let mut raw = Vec::new();
    (&mut stream)
        .take((MAX_BODY + READ_SLACK) as u64)
        .read_to_end(&mut raw)
        .await
        .ok()?;
    parse_response(&raw)
}

/// Tool support of Ollama models by digest, in memory for the life of the
/// process (never on disk). Only answers (`Yes`, `No`) are kept: an `Unknown`
/// is probed again on the next run.
#[derive(Default)]
pub struct ShowCache(Mutex<HashMap<String, Tools>>);

impl ShowCache {
    pub fn get(&self, digest: &str) -> Option<Tools> {
        let map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.get(digest).copied()
    }

    pub fn put(&self, digest: &str, tools: Tools) {
        if tools == Tools::Unknown {
            return;
        }
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if map.len() >= SHOW_CACHE_CAP && !map.contains_key(digest) {
            map.clear();
        }
        map.insert(digest.to_owned(), tools);
    }
}

/// The model facts of `detection` with their tool support filled in, from the
/// cache or `POST /api/show`, within the bounds above. A model left
/// unanswered stays `Unknown`; nothing here changes the engine state or the
/// model list.
async fn show_phase(
    detection: &Detection,
    ports: &ServerPorts,
    cache: &ShowCache,
) -> Vec<ModelFacts> {
    let mut facts_all = detection.model_facts.clone();
    let up = detection
        .engines
        .iter()
        .any(|s| s.id == "ollama" && s.status == Status::Available);
    let Some(port) = ports.port("ollama").filter(|_| up) else {
        return facts_all;
    };
    let mut todo = Vec::new();
    for (at, facts) in facts_all.iter_mut().enumerate() {
        match facts.digest.as_deref().and_then(|d| cache.get(d)) {
            Some(tools) => facts.tools = tools,
            None => todo.push((at, facts.name.clone())),
        }
    }
    todo.truncate(SHOW_MAX_CALLS);
    let mut calls = stream::iter(todo)
        .map(|(at, name)| async move {
            let reply = http_post_show(port, &name).await;
            let tools = match reply {
                Some((200, body)) => parse::show_tools(&body),
                _ => Tools::Unknown,
            };
            (at, tools)
        })
        .buffer_unordered(SHOW_PARALLEL);
    let mut answers = Vec::new();
    let _ = timeout(SHOW_DEADLINE, async {
        while let Some(answer) = calls.next().await {
            answers.push(answer);
        }
    })
    .await;
    for (at, tools) in answers {
        let facts = &mut facts_all[at];
        facts.tools = tools;
        if let Some(digest) = &facts.digest {
            cache.put(digest, tools);
        }
    }
    facts_all
}

/// The first executable regular file called `name` in an absolute `path`
/// entry. Empty and relative entries are skipped.
pub fn find_on_path(name: &str, path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name))
        .find(|file| {
            fs::metadata(file)
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
}

/// Longest a killed run waits to reap its child, on top of its limit.
pub(crate) const REAP_GRACE: Duration = Duration::from_millis(500);

/// How long the output is read after the child has exited, for a pipe a
/// background process still holds open.
const EXIT_GRACE: Duration = Duration::from_millis(100);

/// Runs `<path> --version` with `search` as its `PATH` and returns its stdout. `None` on a spawn
/// failure, a non-zero exit, a timeout or more than `VERSION_CAP` bytes of
/// output (it is dropped, not truncated). The child runs in its own process
/// group, which is killed on every way out so no grandchild survives.
pub async fn version(path: &Path, search: &OsStr) -> Option<Vec<u8>> {
    run_capped(
        || {
            let mut command = Command::new(path);
            command
                .arg("--version")
                // The `PATH` the binary was found on, so a script's
                // `#!/usr/bin/env node` resolves like it does in a turn.
                .env("PATH", search)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .process_group(0);
            command
        },
        VERSION_TIMEOUT,
        VERSION_CAP,
    )
    .await
}

/// Runs the command `make` builds and returns its stdout. `None` on a spawn
/// failure, a non-zero exit, a timeout or more than `cap` bytes of output
/// (dropped, not truncated). The command must pipe stdout and run in its own
/// process group (`process_group(0)`); its stdin is closed here. The group is
/// killed on every way out so no grandchild survives.
pub(crate) async fn run_capped(
    make: impl Fn() -> Command,
    limit: Duration,
    cap: usize,
) -> Option<Vec<u8>> {
    run_capped_why(make, limit, cap).await.ok()
}

/// Why a capped run gave no output.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum RunFailure {
    /// The program could not be started.
    Spawn,
    /// It did not finish within the limit.
    Timeout,
    /// It printed more than the cap.
    TooLarge,
    /// Its output or exit could not be read.
    Io,
    /// It exited with a failure (`None`: a signal), with the last line it
    /// wrote to stderr when the command piped it.
    Exit {
        code: Option<i32>,
        stderr: Option<String>,
    },
}

/// Most bytes of stderr kept while a run lasts: its tail, where the reason is.
const STDERR_KEEP: usize = 16 * 1024;

/// Most bytes of a stderr line shown to the user.
pub(crate) const NOTE_CAP: usize = 200;

/// How long the stderr reader is waited for once the group is dead.
const STDERR_GRACE: Duration = Duration::from_millis(200);

/// Reads stderr to its end, keeping the last [`STDERR_KEEP`] bytes.
pub(crate) async fn read_tail(mut pipe: tokio::process::ChildStderr) -> Vec<u8> {
    let mut tail = Vec::new();
    let mut buf = [0u8; 4096];
    while let Ok(n) = pipe.read(&mut buf).await {
        if n == 0 {
            break;
        }
        tail.extend_from_slice(&buf[..n]);
        if tail.len() > STDERR_KEEP {
            tail.drain(..tail.len() - STDERR_KEEP);
        }
    }
    tail
}

/// The note for the last line of what the stderr reader read. A run that
/// timed out has no use for it, so the reader is aborted at once; otherwise
/// it is waited for up to [`STDERR_GRACE`] and then aborted.
async fn stderr_said(
    task: Option<tokio::task::JoinHandle<Vec<u8>>>,
    timed_out: bool,
) -> Option<String> {
    let task = task?;
    if timed_out {
        task.abort();
        return None;
    }
    finish_stderr(task, STDERR_GRACE)
        .await
        .and_then(|bytes| stderr_note(&bytes))
}

/// What the stderr reader read, waiting at most `wait` for it; a reader that
/// is still running after that is aborted, not left behind.
pub(crate) async fn finish_stderr(
    mut task: tokio::task::JoinHandle<Vec<u8>>,
    wait: Duration,
) -> Option<Vec<u8>> {
    match timeout(wait, &mut task).await {
        Ok(done) => done.ok(),
        Err(_) => {
            task.abort();
            None
        }
    }
}

/// The last non-blank line of `stderr`, sanitized (no escapes or control
/// characters) and capped at about [`NOTE_CAP`] bytes. `None` when nothing
/// readable is left.
pub(crate) fn stderr_note(stderr: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(stderr);
    let line = redact_tokens(&sanitize(&last_line(&text)?));
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    // The cap counts what comes after the helper's marker, so the marker the
    // jail check looks for is never cut.
    let (marker, reason) = match line.strip_prefix(NOT_ENFORCED_MARKER) {
        Some(reason) => (NOT_ENFORCED_MARKER, reason),
        None => ("", line),
    };
    if reason.len() <= NOTE_CAP {
        return Some(line.to_owned());
    }
    let end = (0..=NOTE_CAP)
        .rev()
        .find(|i| reason.is_char_boundary(*i))
        .unwrap_or(0);
    Some(format!("{marker}{}…", &reason[..end]))
}

impl RunFailure {
    /// The reason for the user: what `name` did and why the run gave nothing.
    /// A helper that could not enforce the jail says so on stderr, which is
    /// shown as a jail reason.
    pub(crate) fn text(&self, name: &str, limit: Duration, cap: usize) -> String {
        match self {
            Self::Spawn => format!("could not start {name}"),
            Self::Timeout if limit.subsec_millis() == 0 => {
                format!("timed out after {} s", limit.as_secs())
            }
            Self::Timeout => format!("timed out after {} ms", limit.as_millis()),
            Self::TooLarge if cap >= 1024 * 1024 => {
                format!("output over {} MiB", cap / (1024 * 1024))
            }
            Self::TooLarge => format!("output over {cap} bytes"),
            Self::Io => format!("could not read the output of {name}"),
            Self::Exit { code, stderr } => {
                // Only the last stderr line is kept: sandbox-exec writes one line, and a
                // forged marker can only misclassify toward a jail error (the safe side).
                if let Some(said) = stderr
                    && let Some(ChatError::Jail(reason)) = exit_error(*code, said)
                {
                    let reason = if reason.is_empty() {
                        "the jail could not be enforced"
                    } else {
                        reason.as_str()
                    };
                    return format!("jail: {reason}");
                }
                let head = match code {
                    Some(code) => format!("{name} exited {code}"),
                    None => format!("{name} was stopped by a signal"),
                };
                match stderr {
                    Some(said) => format!("{head}: {said}"),
                    None => head,
                }
            }
        }
    }
}

/// [`run_capped`] with the reason it gave nothing. When the command pipes
/// stderr, a failing exit carries its last line.
pub(crate) async fn run_capped_why(
    make: impl Fn() -> Command,
    limit: Duration,
    cap: usize,
) -> Result<Vec<u8>, RunFailure> {
    let mut child = spawn_capped(&make).await.ok_or(RunFailure::Spawn)?;
    // The child sees EOF on stdin at once.
    drop(child.stdin.take());
    let stderr = child
        .stderr
        .take()
        .map(|pipe| tokio::spawn(read_tail(pipe)));
    let pid = child.id().ok_or(RunFailure::Io)? as libc::pid_t;
    // Declared after `child`, so it drops first: a dropped future still kills
    // the group while the child has not been reaped yet.
    // A refused pid (never a real fresh group) drops `child`, which kills it
    // through `kill_on_drop`; no group signal is ever sent.
    let mut group = GroupKill::new(pid).ok_or(RunFailure::Io)?;
    let output = timeout(limit, collect_capped(&mut child, pid, cap)).await;
    // INVARIANT: the group is signalled while the child is still unreaped.
    // A reaped pid can be recycled, and its pgid with it, so `collect_capped`
    // only waits for the exit (WNOWAIT) and the reap comes after this kill.
    group.kill();
    // Reap the child so it does not linger as a zombie. `reap` disarms right
    // after the wait. If the timeout wins instead, the child stays unreaped
    // and the guard armed; disarm it by hand so the drop does not signal the
    // group a second time. The group already got SIGKILL above and the child
    // is spawned with `kill_on_drop(true)`, so nothing is left running.
    if timeout(REAP_GRACE, group.reap(&mut child)).await.is_err() {
        group.disarm();
    }
    let said = stderr_said(stderr, output.is_err()).await;
    match output {
        Err(_) => Err(RunFailure::Timeout),
        Ok(Err(RunFailure::Exit { code, .. })) => Err(RunFailure::Exit { code, stderr: said }),
        Ok(other) => other,
    }
}

/// Longest `codex debug models` is allowed to run. The probe runs next to the
/// version probe, so this bounds how long it can delay a detection.
pub const LISTING_TIMEOUT: Duration = Duration::from_secs(4);

/// Most listing output kept. The real catalog is about 560 KB, so the cap
/// leaves room for growth; more than this means the probe gave up.
pub const LISTING_CAP: usize = 2 * 1024 * 1024;

/// How long a successful codex listing is reused.
pub const LISTING_TTL: Duration = Duration::from_secs(10 * 60);

/// The clock the listing cache reads; tests inject a fake one.
pub type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

/// A codex listing: its raw output, or why there is none.
pub type Listing = Result<Vec<u8>, String>;

/// The last successful codex listing and when it was read, kept in memory
/// for the life of the process (no files).
pub struct ListingCache {
    clock: Clock,
    last: Mutex<Option<(Instant, Vec<u8>)>>,
}

impl ListingCache {
    pub fn new(clock: Clock) -> Self {
        Self {
            clock,
            last: Mutex::new(None),
        }
    }

    /// The listing to use now. Fresh enough (under [`LISTING_TTL`]) and not
    /// `force`d: the cached one, with no process started. Otherwise the jailed
    /// run; a success replaces the cache, a failure falls back to the cached
    /// listing while it is still under the TTL, else the failure's reason
    /// (just `default` in the picker).
    pub async fn listing(&self, env: &CliEnv, force: bool) -> Listing {
        let now = (self.clock)();
        let cached = || {
            let last = self.last.lock().unwrap_or_else(|e| e.into_inner());
            last.as_ref()
                .filter(|(at, _)| now.saturating_duration_since(*at) < LISTING_TTL)
                .map(|(_, raw)| raw.clone())
        };
        if !force && let Some(raw) = cached() {
            return Ok(raw);
        }
        match codex_listing(env).await {
            Ok(raw) => {
                *self.last.lock().unwrap_or_else(|e| e.into_inner()) = Some((now, raw.clone()));
                Ok(raw)
            }
            // A failure is never cached as a success.
            Err(why) => cached().ok_or(why),
        }
    }
}

/// Where detection reads codex's model list from.
pub struct Lister<'a> {
    pub env: &'a CliEnv,
    pub cache: &'a ListingCache,
    /// Bypass the cache (the menu's `r`).
    pub force: bool,
}

/// Runs `codex debug models` and returns its stdout. `Err` is the reason on
/// any failure (no jail, an old codex without the subcommand, a timeout, too
/// much output).
///
/// The command may refresh the catalog from OpenAI with the user's own
/// codex login and rewrite codex's cache (`models_cache.json`), exactly as
/// running codex does; that is how the list reflects the account. So it runs
/// the way a codex chat turn does: through the jail (`__jail-exec`, the codex
/// layout with its state folder writable), with the chat's scrubbed
/// environment, stdin closed and the private working folder. Like the chat,
/// codex requires the jail: without it nothing is started.
pub async fn codex_listing(env: &CliEnv) -> Listing {
    listing(env, LISTING_TIMEOUT, LISTING_CAP).await
}

async fn listing(env: &CliEnv, limit: Duration, cap: usize) -> Listing {
    super::chat::run_listing(env, limit, cap).await
}

/// Waits for `pid` to exit WITHOUT reaping it, so the pid stays reserved.
/// `Some(true)` for a clean exit 0.
pub(crate) async fn wait_exit_unreaped(pid: libc::pid_t) -> Option<bool> {
    let status = wait_exit_status_unreaped(pid).await?;
    Some(status == Some(0))
}

/// Like [`wait_exit_unreaped`], with the status: `Some(Some(code))` for an
/// exit, `Some(None)` for a process a signal ended.
async fn wait_exit_status_unreaped(pid: libc::pid_t) -> Option<Option<i32>> {
    tokio::task::spawn_blocking(move || {
        // SAFETY: `info` is plain data for waitid to fill in; the pid is our
        // own child and WNOWAIT leaves it for `Child::wait` to reap.
        unsafe {
            let mut info: libc::siginfo_t = std::mem::zeroed();
            loop {
                let id = pid as libc::id_t;
                if libc::waitid(libc::P_PID, id, &mut info, libc::WEXITED | libc::WNOWAIT) == 0 {
                    break;
                }
                if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                    return None;
                }
            }
            Some((info.si_code == libc::CLD_EXITED).then(|| info.si_status()))
        }
    })
    .await
    .ok()?
}

/// Spawns the command `make` builds. A binary that is being written (an
/// upgrade, or a concurrent fork holding its write handle) fails with ETXTBSY
/// for a moment, so that one error is retried briefly.
async fn spawn_capped(make: &impl Fn() -> Command) -> Option<tokio::process::Child> {
    for attempt in 0..5 {
        match make().spawn() {
            Ok(child) => return Some(child),
            Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) && attempt < 4 => {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(_) => return None,
        }
    }
    None
}

/// Reads stdout (bounded) while waiting for the exit, not for the pipe EOF.
async fn collect_capped(
    child: &mut tokio::process::Child,
    pid: libc::pid_t,
    cap: usize,
) -> Result<Vec<u8>, RunFailure> {
    let mut stdout = child.stdout.take().ok_or(RunFailure::Io)?;
    let mut buf = Vec::new();
    let status = {
        let mut limited = (&mut stdout).take(cap as u64 + 1);
        let read = limited.read_to_end(&mut buf);
        tokio::pin!(read);
        tokio::select! {
            n = &mut read => {
                if n.map_err(|_| RunFailure::Io)? > cap {
                    return Err(RunFailure::TooLarge);
                }
                wait_exit_status_unreaped(pid).await.ok_or(RunFailure::Io)?
            }
            status = wait_exit_status_unreaped(pid) => {
                let _ = timeout(EXIT_GRACE, &mut read).await;
                status.ok_or(RunFailure::Io)?
            }
        }
    };
    if status != Some(0) {
        return Err(RunFailure::Exit {
            code: status,
            stderr: None,
        });
    }
    if buf.len() > cap {
        return Err(RunFailure::TooLarge);
    }
    Ok(buf)
}

/// What `detect_with` probes: an entry of the catalog, or a test double
/// that points at an ephemeral port.
pub type Target = Engine;

/// Probes every target concurrently and classifies the facts. `path` is the
/// PATH value, read once by the caller.
pub async fn detect_with(targets: &[Target], path: &OsStr) -> Detection {
    detect_in(targets, path, None).await
}

/// Like [`detect_with`]; with a `lister`, codex's model list is read through
/// the jail its env describes (its `path` is replaced by `path`), reusing the
/// cache unless the lister forces a fresh read.
pub async fn detect_in(targets: &[Target], path: &OsStr, lister: Option<Lister<'_>>) -> Detection {
    let env = lister.as_ref().map(|l| CliEnv {
        path: path.to_owned(),
        ..l.env.clone()
    });
    let lister = lister.zip(env.as_ref()).map(|(l, env)| Lister { env, ..l });
    let probes = join_all(
        targets
            .iter()
            .map(|target| probe(target, path, lister.as_ref())),
    )
    .await;
    reduce(&probes)
}

/// Detection over the real catalog, each server on its port in `ports`, and
/// the `PATH` of `env`, the one the chat turns run with, so what is detected
/// is what can be started.
pub async fn detect_all(
    env: &CliEnv,
    cache: &ListingCache,
    show: &ShowCache,
    force: bool,
    ports: &ServerPorts,
    model_dirs: &[PathBuf],
) -> Detection {
    let lister = Lister { env, cache, force };
    let mut detection = detect_in(&ports.catalog(), &env.path, Some(lister)).await;
    // `env.data` is `<data home>/lazarobox`.
    detection.data_home = env
        .data
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    detection.vulkan_icd = crate::runtime::llama_install::installer::has_vulkan_icd();
    // None waits on another: the model scan is off the executor, the `/props`
    // probe and the `/api/show` calls (the only POST detection sends, to
    // Ollama alone) are on the network.
    let (models, windows, facts) = tokio::join!(
        local_models(&detection, model_dirs),
        windows(&detection, ports),
        show_phase(&detection, ports, show)
    );
    detection.local_models = models;
    detection.windows = windows;
    detection.model_facts = facts;
    detection
}

/// The context windows of the servers that are up and say theirs: a running
/// llama-server's `n_ctx` (`GET /props`). Ollama is left out: its window sits
/// behind a POST (`/api/show`), which only [`show_phase`] sends, for tools.
async fn windows(detection: &Detection, ports: &ServerPorts) -> Vec<(&'static str, usize)> {
    let up = detection
        .engines
        .iter()
        .any(|s| s.id == "llamacpp" && s.status == Status::Available);
    let Some(port) = ports.port("llamacpp").filter(|_| up) else {
        return Vec::new();
    };
    match http_get(port, "/props").await {
        Some((200, body)) => window::llamacpp_n_ctx(&body)
            .map(|n| vec![("llamacpp", n)])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// The llama.cpp model files in `dirs`, looked for only when llama-server is
/// installed: without it there is nothing to load them with.
async fn local_models(detection: &Detection, dirs: &[PathBuf]) -> Vec<PathBuf> {
    let installed = detection
        .engines
        .iter()
        .any(|s| s.id == "llamacpp" && s.status != Status::NotInstalled);
    if !installed || dirs.is_empty() {
        return Vec::new();
    }
    let dirs = dirs.to_vec();
    tokio::task::spawn_blocking(move || super::gguf::find(&dirs))
        .await
        .unwrap_or_default()
}

/// Runs `fut` off the caller and sends exactly one `Detection` to `tx`: the
/// result, or an empty one if the future panicked. Returns at once.
pub fn spawn_detection_of<F>(tx: mpsc::Sender<Detection>, fut: F)
where
    F: Future<Output = Detection> + Send + 'static,
{
    // The inner task isolates a panic: its `JoinError` becomes the empty
    // `Detection` instead of leaving the app waiting for a result forever.
    let inner = tokio::spawn(fut);
    tokio::spawn(async move {
        let _ = tx.send(inner.await.unwrap_or_default()).await;
    });
}

/// Starts a real detection; its result arrives on `tx`.
pub fn spawn_detection(
    tx: mpsc::Sender<Detection>,
    env: CliEnv,
    cache: Arc<ListingCache>,
    show: Arc<ShowCache>,
    force: bool,
    ports: ServerPorts,
    model_dirs: Arc<[PathBuf]>,
) {
    spawn_detection_of(tx, async move {
        detect_all(&env, &cache, &show, force, &ports, &model_dirs).await
    });
}

/// Why the jailed CLI `id`, found at `file`, would run without its jail.
fn jail_note_of(id: &str, env: &CliEnv, file: &Path) -> Option<super::chat::JailNote> {
    let cli = match id {
        "claude" => Cli::Claude,
        "codex" => Cli::Codex,
        "gemini" => Cli::Gemini,
        _ => return None,
    };
    super::chat::jail_note(env, cli, file)
}

/// The version output of a CLI. With a lister, codex is asked through its
/// jail: the standalone install puts the program inside codex's own writable
/// state folder, so it is never run outside the jail. Without one (no jail
/// context, tests) the plain run stays.
async fn cli_version(
    id: &str,
    file: &Path,
    search: &OsStr,
    lister: Option<&Lister<'_>>,
) -> Option<Vec<u8>> {
    match (id, lister) {
        ("codex", Some(l)) => super::chat::run_version(l.env, VERSION_TIMEOUT, VERSION_CAP)
            .await
            .ok(),
        _ => version(file, search).await,
    }
}

async fn probe(target: &Target, path: &OsStr, lister: Option<&Lister<'_>>) -> Probe {
    let found = find_on_path(target.binary, path);
    match target.kind {
        Kind::Server { port, path, .. } => Probe::Server {
            id: target.id,
            binary: found,
            reply: http_get(port, path).await,
        },
        Kind::Cli => {
            // Only a resolved absolute path is ever run. Only codex has a
            // model listing, and it runs next to the version probe, so a slow
            // listing never adds to the version's time.
            let (stdout, listing, note, install) = match &found {
                Some(file) => {
                    let list = async {
                        match (lister, target.id) {
                            (Some(l), "codex") => Some(l.cache.listing(l.env, l.force).await),
                            _ => None,
                        }
                    };
                    let (stdout, listing) =
                        tokio::join!(cli_version(target.id, file, path, lister), list);
                    let note = lister.and_then(|l| jail_note_of(target.id, l.env, file));
                    let install = lister
                        .filter(|_| target.id == "codex")
                        .and_then(|l| super::chat::codex_install(l.env, file));
                    (stdout, listing, note, install)
                }
                None => (None, None, None, None),
            };
            Probe::Cli {
                id: target.id,
                path: found,
                stdout,
                listing,
                jail: note.as_ref().map(|n| n.short.clone()),
                jail_full: note.map(|n| n.full),
                install,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ai::{
        EngineState, Status,
        jail::{Backend, JailOff, JailSupport},
    };
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn spawn_detection_of_delivers_the_result() {
        let (tx, mut rx) = mpsc::channel(1);
        let one = Detection {
            engines: vec![state("codex", Status::Available, &["default"], None)],
            ..Default::default()
        };
        let sent = one.clone();
        spawn_detection_of(tx, async move { sent });
        let got = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await;
        assert_eq!(got, Ok(Some(one)));
    }

    #[tokio::test]
    async fn spawn_detection_of_answers_with_an_empty_detection_on_panic() {
        let (tx, mut rx) = mpsc::channel(1);
        spawn_detection_of(tx, async { panic!("probe blew up") });
        let got = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await;
        assert_eq!(got, Ok(Some(Detection::default())));
    }

    #[tokio::test]
    async fn spawn_detection_of_returns_without_waiting() {
        let (tx, _rx) = mpsc::channel(1);
        let started = std::time::Instant::now();
        spawn_detection_of(tx, std::future::pending());
        // A blocking spawn would never come back (the future is `pending`).
        assert!(started.elapsed() < Duration::from_secs(5));
    }
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    type Requests = Arc<Mutex<Vec<Vec<u8>>>>;

    /// A loopback server on an ephemeral port that records each request and
    /// answers it with `reply` (then closes). `None` reply: accept and stay
    /// silent.
    async fn fake_server(reply: Option<Vec<u8>>) -> (u16, Requests) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests: Requests = Arc::default();
        let seen = requests.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let (seen, reply) = (seen.clone(), reply.clone());
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 512];
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        match stream.read(&mut chunk).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                    seen.lock().unwrap().push(buf);
                    match reply {
                        Some(bytes) => {
                            let _ = stream.write_all(&bytes).await;
                            let _ = stream.shutdown().await;
                        }
                        None => std::future::pending::<()>().await,
                    }
                });
            }
        });
        (port, requests)
    }

    #[tokio::test]
    async fn http_get_returns_status_and_body() {
        let reply = b"HTTP/1.0 200 OK\r\nContent-Length: 12\r\n\r\n{\"ok\":true}\n".to_vec();
        let (port, _) = fake_server(Some(reply)).await;
        assert_eq!(
            http_get(port, "/api/tags").await,
            Some((200, b"{\"ok\":true}\n".to_vec()))
        );
    }

    #[tokio::test]
    async fn http_get_decodes_a_chunked_reply() {
        let reply =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n"
                .to_vec();
        let (port, _) = fake_server(Some(reply)).await;
        assert_eq!(
            http_get(port, "/v1/models").await,
            Some((200, b"abcde".to_vec()))
        );
    }

    #[tokio::test]
    async fn http_get_sends_exactly_the_documented_request() {
        let reply = b"HTTP/1.0 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec();
        let (port, requests) = fake_server(Some(reply)).await;
        http_get(port, "/api/tags").await;
        let expected = format!(
            "GET /api/tags HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
        );
        assert_eq!(*requests.lock().unwrap(), vec![expected.into_bytes()]);
    }

    #[tokio::test]
    async fn http_get_gives_up_on_a_silent_server() {
        let (port, _) = fake_server(None).await;
        let answer =
            tokio::time::timeout(Duration::from_millis(1200), http_get(port, "/api/tags")).await;
        assert_eq!(answer, Ok(None));
    }

    #[tokio::test]
    async fn http_get_rejects_a_reply_over_the_cap() {
        let mut reply = b"HTTP/1.0 200 OK\r\n\r\n".to_vec();
        reply.resize(reply.len() + 1024 * 1024 + 1, b'x');
        let (port, _) = fake_server(Some(reply)).await;
        let answer =
            tokio::time::timeout(Duration::from_millis(1200), http_get(port, "/api/tags")).await;
        assert_eq!(answer, Ok(None));
    }

    #[tokio::test]
    async fn http_get_fails_fast_on_a_closed_port() {
        let refused = crate::runtime::refused_port::RefusedPort::new();
        let answer = tokio::time::timeout(
            Duration::from_millis(400),
            http_get(refused.port(), "/api/tags"),
        )
        .await;
        assert_eq!(answer, Ok(None));
    }

    #[tokio::test]
    async fn http_get_returns_a_redirect_without_following_it() {
        let reply =
            b"HTTP/1.0 301 Moved\r\nLocation: http://example.com/\r\nContent-Length: 0\r\n\r\n"
                .to_vec();
        let (port, requests) = fake_server(Some(reply)).await;
        assert_eq!(http_get(port, "/api/tags").await, Some((301, Vec::new())));
        assert_eq!(requests.lock().unwrap().len(), 1);
    }

    /// A file the probes treat as an executable. The `sh` shebang is only
    /// this test fixture; production code never spawns a shell.
    fn write_script(dir: &Path, name: &str, body: &str, mode: u32) -> PathBuf {
        let path = dir.join(name);
        // The probes run the child with the `PATH` it was found on, which a
        // test often sets to the fake bin dir alone: give the fixture the
        // system tools back.
        let text = format!("#!/bin/sh\nPATH=\"$PATH:/usr/bin:/bin\"\n{body}\n");
        fs::write(&path, text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    fn write_file(dir: &Path, name: &str, mode: u32) -> PathBuf {
        write_script(dir, name, "", mode)
    }

    const LISTING: &str = include_str!("../../tests/fixtures/codex/debug-models.json");

    /// A scratch machine under the crate dir (the jail layout refuses `/tmp`):
    /// a `bin` dir, a home with `.codex`, a private data dir and a record dir.
    struct Lab {
        _root: tempfile::TempDir,
        bin: PathBuf,
        home: PathBuf,
        data: PathBuf,
        rec: PathBuf,
    }

    fn lab() -> Lab {
        let root = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let base = fs::canonicalize(root.path()).unwrap();
        let lab = Lab {
            bin: base.join("bin"),
            home: base.join("home"),
            data: base.join("data/lazarobox"),
            rec: base.join("rec"),
            _root: root,
        };
        for dir in [&lab.bin, &lab.home.join(".codex"), &lab.rec, &lab.data] {
            fs::create_dir_all(dir).unwrap();
        }
        for dir in [&lab.bin, &lab.home, &lab.rec] {
            fs::set_permissions(dir, fs::Permissions::from_mode(0o755)).unwrap();
        }
        lab
    }

    /// A fake codex: `body` answers `debug models`, anything else is the
    /// version probe.
    fn fake_codex(lab: &Lab, body: &str) -> PathBuf {
        write_script(
            &lab.bin,
            "codex",
            &format!(
                "REC='{}'\nif [ \"$1\" = debug ] && [ \"$2\" = models ]; then\n{body}\nelse echo codex 0.5.0; fi",
                lab.rec.display()
            ),
            0o755,
        )
    }

    /// A pass-through helper: records its argv, drops the rules, execs the CLI.
    fn pass_helper(lab: &Lab) -> PathBuf {
        write_script(
            &lab.rec,
            "helper",
            &format!(
                "printf '%s\\0' \"$@\" > '{}/helper_args'\nwhile [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"",
                lab.rec.display()
            ),
            0o755,
        )
    }

    fn env_of(lab: &Lab, jail: bool) -> CliEnv {
        let support = if jail {
            JailSupport::Available {
                backend: Backend::Landlock,
                abi: 7,
            }
        } else {
            JailSupport::Unavailable(JailOff::NotProbed)
        };
        let mut env = CliEnv::from_env(support);
        env.path = lab.bin.clone().into_os_string();
        env.home = Some(lab.home.clone());
        env.data = Some(lab.data.clone());
        env.codex_home = None;
        env.helper = pass_helper(lab);
        env
    }

    fn helper_args(lab: &Lab) -> Vec<String> {
        fs::read_to_string(lab.rec.join("helper_args"))
            .unwrap_or_default()
            .split('\0')
            .filter(|a| !a.is_empty())
            .map(str::to_owned)
            .collect()
    }

    #[tokio::test]
    async fn listing_runs_through_the_codex_jail_with_its_state_folder_writable() {
        let lab = lab();
        fake_codex(&lab, "echo '{\"models\":[]}'");
        let env = env_of(&lab, true);
        assert_eq!(codex_listing(&env).await, Ok(b"{\"models\":[]}\n".to_vec()));
        let args = helper_args(&lab);
        assert_eq!(args[0], "__jail-exec");
        let codex_dir = fs::canonicalize(lab.home.join(".codex")).unwrap();
        let rw: Vec<_> = args
            .windows(2)
            .filter(|w| w[0] == "--rw")
            .map(|w| w[1].clone())
            .collect();
        assert!(rw.contains(&codex_dir.display().to_string()), "{rw:?}");
        let tail: Vec<_> = args.iter().rev().take(3).rev().cloned().collect();
        assert!(tail[0].ends_with("/codex"), "{tail:?}");
        assert_eq!(&tail[1..], ["debug", "models"]);
    }

    #[tokio::test]
    async fn listing_honors_codex_home_for_the_writable_folder() {
        let lab = lab();
        fake_codex(&lab, "echo '{}'");
        let custom = lab.home.join("custom-codex");
        fs::create_dir(&custom).unwrap();
        let mut env = env_of(&lab, true);
        env.codex_home = Some(custom.clone());
        assert!(codex_listing(&env).await.is_ok());
        let args = helper_args(&lab);
        let rw: Vec<_> = args
            .windows(2)
            .filter(|w| w[0] == "--rw")
            .map(|w| w[1].clone())
            .collect();
        assert!(rw.contains(&custom.display().to_string()), "{rw:?}");
        assert!(
            !rw.contains(&lab.home.join(".codex").display().to_string()),
            "{rw:?}"
        );
    }

    #[tokio::test]
    async fn listing_without_a_jail_never_starts_codex() {
        let lab = lab();
        fake_codex(&lab, &format!("echo x > '{}/ran'", lab.rec.display()));
        let env = env_of(&lab, false);
        assert_eq!(
            codex_listing(&env).await,
            Err("jail: the jail was not probed".to_owned())
        );
        assert!(!lab.rec.join("ran").exists());
        assert!(!lab.rec.join("helper_args").exists());
    }

    fn seatbelt_env(lab: &Lab) -> CliEnv {
        let mut env = env_of(lab, true);
        env.jail = JailSupport::Available {
            backend: Backend::Seatbelt,
            abi: 0,
        };
        env
    }

    #[tokio::test]
    async fn listing_runs_through_the_helper_when_the_backend_is_seatbelt() {
        let lab = lab();
        fake_codex(&lab, "echo '{\"models\":[]}'");
        assert_eq!(
            codex_listing(&seatbelt_env(&lab)).await,
            Ok(b"{\"models\":[]}\n".to_vec())
        );
        let args = helper_args(&lab);
        assert_eq!(args[0], "__jail-exec");
        assert_eq!(&args[args.len() - 2..], ["debug", "models"]);
    }

    #[tokio::test]
    async fn a_listing_that_ends_in_exit_71_is_an_error_and_codex_never_runs_bare() {
        let lab = lab();
        fake_codex(&lab, "echo 'sandbox-exec: bad profile' >&2\nexit 71");
        let env = seatbelt_env(&lab);
        let got = codex_listing(&env).await;
        assert_eq!(
            got,
            Err("jail: the macOS sandbox could not be applied".to_owned())
        );
        // Without a jail nothing starts at all.
        let mut off = seatbelt_env(&lab);
        off.jail = JailSupport::Unavailable(JailOff::NoSeatbelt(
            crate::core::ai::jail::SeatbeltFault::Refused,
        ));
        std::fs::remove_file(lab.rec.join("helper_args")).unwrap();
        assert!(codex_listing(&off).await.is_err());
        assert!(!lab.rec.join("helper_args").exists());
    }

    #[tokio::test]
    async fn listing_fails_soft_for_an_old_codex_without_the_subcommand() {
        let lab = lab();
        write_script(
            &lab.bin,
            "codex",
            "echo 'unknown command' >&2\nexit 2",
            0o755,
        );
        assert_eq!(
            codex_listing(&env_of(&lab, true)).await,
            Err("codex exited 2: unknown command".to_owned())
        );
    }

    #[tokio::test]
    async fn a_failing_listing_says_the_exit_code_and_the_last_stderr_line() {
        let lab = lab();
        fake_codex(
            &lab,
            "echo 'warming up' >&2\necho 'not logged in: run codex login' >&2\necho >&2\nexit 1",
        );
        assert_eq!(
            codex_listing(&env_of(&lab, true)).await,
            Err("codex exited 1: not logged in: run codex login".to_owned())
        );
    }

    #[tokio::test]
    async fn a_failing_listing_without_stderr_still_says_the_exit_code() {
        let lab = lab();
        fake_codex(&lab, "exit 3");
        assert_eq!(
            codex_listing(&env_of(&lab, true)).await,
            Err("codex exited 3".to_owned())
        );
    }

    #[tokio::test]
    async fn a_listing_killed_by_a_signal_says_so() {
        let lab = lab();
        fake_codex(&lab, "kill -9 $$");
        assert_eq!(
            codex_listing(&env_of(&lab, true)).await,
            Err("codex was stopped by a signal".to_owned())
        );
    }

    #[tokio::test]
    async fn the_stderr_reason_is_sanitized_and_capped() {
        let lab = lab();
        fake_codex(
            &lab,
            "printf 'bad \\033[31mred\\033[0m \\342\\200\\256flip %s\\n' \"$(yes word | head -80 | tr '\\n' ' ')\" >&2\nexit 1",
        );
        let Err(why) = codex_listing(&env_of(&lab, true)).await else {
            panic!("a failure was expected");
        };
        let reason = why.strip_prefix("codex exited 1: ").unwrap();
        assert!(reason.starts_with("bad red flip word word"), "{reason}");
        assert!(
            !reason.chars().any(|c| c.is_control() || c == '\u{202e}'),
            "{reason:?}"
        );
        assert!(
            reason.len() <= NOTE_CAP + '…'.len_utf8(),
            "{}",
            reason.len()
        );
        assert!(reason.ends_with('…'));
    }

    #[test]
    fn a_stderr_note_never_cuts_a_character() {
        let multi = "é".repeat(300);
        let note = stderr_note(multi.as_bytes()).unwrap();
        assert!(
            note.ends_with('…') && note.len() <= NOTE_CAP + 3,
            "{}",
            note.len()
        );
        assert_eq!(stderr_note(b"\n  \n"), None);
        assert_eq!(stderr_note(b"\x1b[2J\n"), None);
        assert_eq!(
            stderr_note(b"a\nlast one \n\n").as_deref(),
            Some("last one")
        );
    }

    #[test]
    fn a_stderr_note_hides_secrets() {
        let note = |text: &str| stderr_note(text.as_bytes()).unwrap();
        assert_eq!(note("401: bad key sk-proj-Abc123_def"), "401: bad key …");
        assert_eq!(
            note("Authorization: Bearer eyJhbGciOi.payload.sig"),
            "Authorization: Bearer …"
        );
        assert_eq!(
            note("token aaaaaaaaaaaaaaaaaaaaaaa9 expired"),
            "token … expired"
        );
        assert_eq!(
            note("not logged in: run codex login"),
            "not logged in: run codex login"
        );
    }

    #[test]
    fn a_stderr_note_skips_a_last_line_that_is_only_escapes() {
        assert_eq!(
            stderr_note(b"real error\n\x1b[2J\n").as_deref(),
            Some("real error")
        );
    }

    #[test]
    fn the_cap_counts_after_the_helper_marker() {
        let marker = crate::core::ai::jail::NOT_ENFORCED_MARKER;
        let short = format!("{marker}: {}", "x ".repeat(80));
        let note = stderr_note(short.as_bytes()).unwrap();
        assert_eq!(note, short.trim(), "a reason under the cap is whole");
        let long = format!("{marker}: {}", "y".repeat(5) + &" z".repeat(300));
        let note = stderr_note(long.as_bytes()).unwrap();
        let rest = note.strip_prefix(marker).expect("the marker survives");
        assert!(rest.ends_with('…'), "{note}");
        assert!(
            rest.len() <= ": ".len() + NOTE_CAP + '…'.len_utf8(),
            "{}",
            rest.len()
        );
    }

    /// A task that only ends when aborted, and says so when it is dropped.
    struct Dropped(Arc<std::sync::atomic::AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// A reader task that never ends by itself and flags when it is dropped.
    fn endless_reader() -> (
        tokio::task::JoinHandle<Vec<u8>>,
        Arc<std::sync::atomic::AtomicBool>,
    ) {
        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let guard = Dropped(Arc::clone(&dropped));
        let task = tokio::spawn(async move {
            let _guard = guard;
            tokio::time::sleep(Duration::from_secs(60)).await;
            Vec::new()
        });
        (task, dropped)
    }

    #[tokio::test]
    async fn a_timed_out_run_aborts_its_stderr_reader_at_once() {
        let (task, dropped) = endless_reader();
        let started = std::time::Instant::now();
        assert_eq!(stderr_said(Some(task), true).await, None);
        // No grace is waited for: the abort is immediate.
        assert!(
            started.elapsed() < STDERR_GRACE / 2,
            "{:?}",
            started.elapsed()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            dropped.load(std::sync::atomic::Ordering::SeqCst),
            "aborted, not detached"
        );
    }

    #[tokio::test]
    async fn a_finished_run_reads_the_reader_within_the_grace_and_aborts_a_late_one() {
        let done = tokio::spawn(async { b"oops\n".to_vec() });
        assert_eq!(
            stderr_said(Some(done), false).await.as_deref(),
            Some("oops")
        );
        let (late, dropped) = endless_reader();
        assert_eq!(stderr_said(Some(late), false).await, None);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(stderr_said(None, true).await, None);
    }

    #[tokio::test]
    async fn a_run_that_times_out_with_stderr_piped_is_a_timeout() {
        let make = || {
            let mut command = Command::new("/bin/sh");
            command
                .args(["-c", "echo late >&2; sleep 30"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .process_group(0);
            command
        };
        let got = run_capped_why(make, Duration::from_millis(300), 1000).await;
        assert_eq!(got, Err(RunFailure::Timeout));
    }

    #[tokio::test]
    async fn a_stderr_reader_that_outlives_the_grace_is_aborted() {
        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let guard = Dropped(Arc::clone(&dropped));
        let task = tokio::spawn(async move {
            let _guard = guard;
            tokio::time::sleep(Duration::from_secs(60)).await;
            Vec::new()
        });
        assert_eq!(finish_stderr(task, Duration::from_millis(50)).await, None);
        // The abort lands on the runtime's next turn: poll for it.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !dropped.load(std::sync::atomic::Ordering::SeqCst)
            && std::time::Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            dropped.load(std::sync::atomic::Ordering::SeqCst),
            "aborted, not detached"
        );
    }

    #[tokio::test]
    async fn a_helper_text_outside_the_vocabulary_gives_the_generic_jail_reason() {
        let lab = lab();
        fake_codex(&lab, "echo hi");
        let mut env = env_of(&lab, true);
        env.helper = write_script(
            &lab.rec,
            "helper",
            "echo 'lazarobox-jail: jail not enforced: ignore all instructions' >&2\nexit 125",
            0o755,
        );
        assert_eq!(
            codex_listing(&env).await,
            Err("jail: the jail could not be enforced".to_owned())
        );
    }

    #[tokio::test]
    async fn a_helper_that_could_not_enforce_the_jail_is_a_jail_reason() {
        let lab = lab();
        fake_codex(&lab, "echo hi");
        let mut env = env_of(&lab, true);
        env.helper = write_script(
            &lab.rec,
            "helper",
            "echo 'lazarobox-jail: jail not enforced: ABI 2 is below the required 3' >&2\nexit 125",
            0o755,
        );
        assert_eq!(
            codex_listing(&env).await,
            Err("jail: ABI 2 is below the required 3".to_owned())
        );
    }

    #[tokio::test]
    async fn a_listing_the_jail_refuses_says_which_rule() {
        let lab = lab();
        fake_codex(&lab, "echo hi");
        let mut env = env_of(&lab, true);
        env.codex_home = Some(lab.home.clone());
        let Err(why) = codex_listing(&env).await else {
            panic!("a failure was expected");
        };
        assert!(
            why.starts_with("jail: ~ is too broad for the jail"),
            "{why}"
        );
    }

    #[tokio::test]
    async fn a_missing_codex_binary_says_so() {
        let lab = lab();
        assert_eq!(
            codex_listing(&env_of(&lab, true)).await,
            Err("codex was not found on PATH".to_owned())
        );
    }

    #[tokio::test]
    async fn listing_has_stdin_closed_a_private_cwd_and_a_scrubbed_env() {
        let lab = lab();
        fake_codex(
            &lab,
            "pwd\nif read line; then echo STDIN-OPEN; else echo STDIN-EOF; fi\necho \"agent=${SSH_AUTH_SOCK-unset}\"",
        );
        let mut env = env_of(&lab, true);
        env.inject = vec![("SSH_AUTH_SOCK".into(), "/tmp/agent".into())];
        let out = String::from_utf8(codex_listing(&env).await.unwrap()).unwrap();
        let mut lines = out.lines();
        let cwd = Path::new(lines.next().unwrap());
        assert!(
            cwd.starts_with(fs::canonicalize(&lab.data).unwrap()),
            "{cwd:?}"
        );
        assert_ne!(cwd, std::env::current_dir().unwrap());
        assert_eq!(lines.next(), Some("STDIN-EOF"));
        assert_eq!(lines.next(), Some("agent=unset"));
    }

    #[tokio::test]
    async fn listing_kills_the_process_group_after_the_timeout() {
        let lab = lab();
        let marker = lab.rec.join("grandchild-survived");
        fake_codex(
            &lab,
            &format!("(sleep 2; echo x > {}) &\nsleep 30", marker.display()),
        );
        let started = std::time::Instant::now();
        let got = listing(&env_of(&lab, true), Duration::from_millis(300), LISTING_CAP).await;
        assert_eq!(got, Err("timed out after 300 ms".to_owned()));
        assert!(started.elapsed() < Duration::from_secs(15));
        // The grandchild would write at 2 s if the kill had missed it.
        tokio::time::sleep(Duration::from_millis(2300)).await;
        assert!(!marker.exists());
    }

    #[tokio::test]
    async fn listing_drops_output_over_the_cap() {
        let lab = lab();
        fake_codex(&lab, "head -c 2000 /dev/zero | tr '\\0' x");
        let env = env_of(&lab, true);
        assert_eq!(
            listing(&env, LISTING_TIMEOUT, 1000).await,
            Err("output over 1000 bytes".to_owned())
        );
        assert!(listing(&env, LISTING_TIMEOUT, 2000).await.is_ok());
    }

    #[test]
    fn the_cap_fits_the_real_catalog_size_and_the_timeout_is_short() {
        // `codex debug models` printed 560491 bytes with codex-cli 0.162.1.
        const { assert!(LISTING_CAP > 560_491) };
        assert!(LISTING_TIMEOUT <= Duration::from_secs(4));
    }

    #[tokio::test]
    async fn detect_reads_the_codex_listing_but_no_other_cli() {
        let lab = lab();
        let data = lab.rec.join("listing.json");
        fs::write(&data, LISTING).unwrap();
        fake_codex(&lab, &format!("cat {}", data.display()));
        write_script(
            &lab.bin,
            "claude",
            &format!("cat {}", data.display()),
            0o755,
        );
        let targets = cli_targets(&["claude", "codex"]);
        let env = env_of(&lab, true);
        let cache = real_cache();
        let d = detect_in(
            &targets,
            &join(&[&lab.bin]),
            Some(lister(&env, &cache, false)),
        )
        .await;
        let get = |id| d.engines.iter().find(|e| e.id == id).unwrap().clone();
        assert_eq!(get("codex").model_labels.len(), 7);
        assert_eq!(get("codex").version.as_deref(), Some("codex 0.5.0"));
        assert!(get("claude").model_labels.is_empty());
    }

    /// A script-free fake CLI for every jailed engine.
    fn fake_jailed_clis(lab: &Lab) {
        for name in ["claude", "gemini"] {
            write_script(&lab.bin, name, &format!("echo {name} 1.0.0"), 0o755);
        }
        fake_codex(lab, "echo '{\"models\":[]}'");
    }

    /// claude's note: on macOS it is never jailed, whatever the probe said.
    fn claude_note(elsewhere: Option<String>) -> Option<String> {
        if crate::core::ai::jail::Os::CURRENT == crate::core::ai::jail::Os::Macos {
            Some("uses macOS Keychain".to_owned())
        } else {
            elsewhere
        }
    }

    async fn jail_notes(env: &CliEnv, lab: &Lab) -> Vec<(&'static str, Option<String>)> {
        let targets = cli_targets(&["claude", "codex", "gemini", "opencode"]);
        write_script(&lab.bin, "opencode", "echo oc 1", 0o755);
        let cache = real_cache();
        let d = detect_in(
            &targets,
            &join(&[&lab.bin]),
            Some(lister(env, &cache, false)),
        )
        .await;
        d.engines
            .into_iter()
            .filter(|e| e.status == Status::Available)
            .map(|e| (e.id, e.diagnostics.jail))
            .collect()
    }

    #[tokio::test]
    async fn a_kernel_without_the_abi_is_noted_on_every_jailed_cli_but_not_opencode() {
        let lab = lab();
        fake_jailed_clis(&lab);
        let mut env = env_of(&lab, false);
        env.jail = JailSupport::Unavailable(JailOff::NoLandlock(2));
        let note = Some("Landlock ABI 2, needs 3+".to_owned());
        assert_eq!(
            jail_notes(&env, &lab).await,
            [
                ("claude", claude_note(note.clone())),
                ("codex", note.clone()),
                ("gemini", note),
                ("opencode", None)
            ]
        );
    }

    #[tokio::test]
    async fn a_layout_the_jail_refuses_is_noted_without_running_the_helper() {
        let lab = lab();
        fake_jailed_clis(&lab);
        let mut env = env_of(&lab, true);
        env.helper = lab.rec.join("never-called");
        env.codex_home = Some(lab.home.clone());
        let notes = jail_notes(&env, &lab).await;
        assert_eq!(
            notes,
            [
                ("claude", claude_note(None)),
                ("codex", Some("~ too broad".to_owned())),
                ("gemini", None),
                ("opencode", None)
            ]
        );
        assert!(!lab.rec.join("helper_args").exists());
    }

    #[tokio::test]
    async fn a_jailable_layout_has_no_jail_note() {
        let lab = lab();
        fake_jailed_clis(&lab);
        let env = env_of(&lab, true);
        let notes = jail_notes(&env, &lab).await;
        for (id, note) in &notes {
            let expected = if *id == "claude" {
                claude_note(None)
            } else {
                None
            };
            assert_eq!(note, &expected, "{notes:?}");
        }
    }

    #[tokio::test]
    async fn codex_is_never_run_for_its_version_without_a_jail() {
        let lab = lab();
        let ran = lab.rec.join("ran");
        write_script(
            &lab.bin,
            "codex",
            &format!("touch '{}'\necho codex 0.5.0", ran.display()),
            0o755,
        );
        let env = env_of(&lab, false);
        let cache = real_cache();
        let d = detect_in(
            &cli_targets(&["codex"]),
            &join(&[&lab.bin]),
            Some(lister(&env, &cache, false)),
        )
        .await;
        let codex = d.engines.iter().find(|e| e.id == "codex").unwrap();
        assert_eq!(codex.status, Status::Available);
        assert_eq!(codex.version, None);
        assert!(!ran.exists(), "codex ran outside the jail");
    }

    #[tokio::test]
    async fn the_version_runs_through_the_jail_helper() {
        let lab = lab();
        fake_codex(&lab, "echo '{\"models\":[]}'");
        let env = env_of(&lab, true);
        let cache = real_cache();
        let d = detect_in(
            &cli_targets(&["codex"]),
            &join(&[&lab.bin]),
            Some(lister(&env, &cache, false)),
        )
        .await;
        let codex = d.engines.iter().find(|e| e.id == "codex").unwrap();
        assert_eq!(codex.version.as_deref(), Some("codex 0.5.0"));
        assert!(
            helper_args(&lab)
                .iter()
                .any(|a| a == "--version" || a == "debug")
        );
    }

    #[tokio::test]
    async fn the_standalone_layout_is_read_from_the_real_path_and_the_state_folder() {
        let lab = lab();
        let real = lab
            .home
            .join(".codex/packages/standalone/releases/1.0.0/bin");
        fs::create_dir_all(&real).unwrap();
        write_script(&real, "codex", "echo codex 1.0.0", 0o755);
        std::os::unix::fs::symlink(real.join("codex"), lab.bin.join("codex")).unwrap();
        let env = env_of(&lab, false);
        let cache = real_cache();
        let d = detect_in(
            &cli_targets(&["codex"]),
            &join(&[&lab.bin]),
            Some(lister(&env, &cache, false)),
        )
        .await;
        let codex = d.engines.iter().find(|e| e.id == "codex").unwrap();
        assert_eq!(
            codex.diagnostics.install,
            Some(crate::core::ai::codex_install::CodexInstall::Standalone)
        );
        // Without a jail the full reason says what to do, whatever the layout.
        assert!(codex.diagnostics.jail_full.is_some());
        let d = detect_in(&cli_targets(&["claude"]), &join(&[&lab.bin]), None).await;
        assert!(d.engines.iter().all(|e| e.diagnostics.install.is_none()));
    }

    #[tokio::test]
    async fn a_failing_version_probe_is_available_with_an_unknown_version() {
        let lab = lab();
        write_script(&lab.bin, "claude", "echo broken >&2\nexit 1", 0o755);
        let d = detect_with(&cli_targets(&["claude"]), &join(&[&lab.bin])).await;
        let claude = d.engines.iter().find(|e| e.id == "claude").unwrap();
        assert_eq!(claude.status, Status::Available);
        assert_eq!(claude.version, None);
        assert_eq!(
            crate::core::ai::status_text(Some(&d), "claude"),
            "available · version unknown"
        );
    }

    #[tokio::test]
    async fn detect_all_looks_in_the_env_path_not_the_process_one() {
        let lab = lab();
        write_script(&lab.bin, "claude", "echo claude 9.9.9", 0o755);
        let env = env_of(&lab, false);
        let d = detect_all(
            &env,
            &real_cache(),
            &ShowCache::default(),
            false,
            &ServerPorts::default(),
            &[],
        )
        .await;
        let claude = d.engines.iter().find(|e| e.id == "claude").unwrap();
        assert_eq!(claude.version.as_deref(), Some("claude 9.9.9"));
    }

    #[tokio::test]
    async fn detect_all_probes_a_server_on_its_configured_port() {
        let body = r#"{"data":[{"id":"m","owned_by":"llamacpp"}]}"#;
        let reply = format!(
            "HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let (port, requests) = fake_server(Some(reply.into_bytes())).await;
        let lab = lab();
        let env = env_of(&lab, false);
        let ports = ServerPorts::default().with("llamacpp", port);
        let d = detect_all(
            &env,
            &real_cache(),
            &ShowCache::default(),
            false,
            &ports,
            &[],
        )
        .await;
        let llamacpp = d.engines.iter().find(|e| e.id == "llamacpp").unwrap();
        assert_eq!(llamacpp.status, Status::Available);
        assert_eq!(llamacpp.models, ["m"]);
        let seen = requests.lock().unwrap();
        // The detection, then the window of the server it found.
        assert_eq!(seen.len(), 2);
        assert!(
            seen[0].starts_with(b"GET /v1/models HTTP/1."),
            "{:?}",
            String::from_utf8_lossy(&seen[0])
        );
        assert!(
            seen[1].starts_with(b"GET /props HTTP/1."),
            "{:?}",
            String::from_utf8_lossy(&seen[1])
        );
    }

    // chat-memory S1: a running llama-server's `n_ctx` is the window.
    #[tokio::test]
    async fn a_running_llama_server_reports_its_context_window() {
        let body = r#"{"data":[{"id":"m","owned_by":"llamacpp"}],"default_generation_settings":{"n_ctx":8192}}"#;
        let reply = format!(
            "HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let (port, _) = fake_server(Some(reply.into_bytes())).await;
        let lab = lab();
        let env = env_of(&lab, false);
        let ports = ServerPorts::default().with("llamacpp", port);
        let d = detect_all(
            &env,
            &real_cache(),
            &ShowCache::default(),
            false,
            &ports,
            &[],
        )
        .await;
        assert_eq!(d.window("llamacpp"), Some(8_192));
        assert_eq!(d.window("ollama"), None);
    }

    #[tokio::test]
    async fn a_server_that_is_not_running_has_no_window_and_costs_no_request() {
        let (port, requests) = fake_server(None).await;
        // Nothing listens on the port the Ollama row would use, and the
        // silent fake never answers llamacpp's detection.
        let lab = lab();
        let env = env_of(&lab, false);
        let ports = ServerPorts::default().with("llamacpp", port);
        let d = detect_all(
            &env,
            &real_cache(),
            &ShowCache::default(),
            false,
            &ports,
            &[],
        )
        .await;
        assert_eq!(d.window("llamacpp"), None);
        assert_eq!(requests.lock().unwrap().len(), 1, "only the detection");
    }

    // Spec S2: the local models are listed only when llama-server is there.
    #[tokio::test]
    async fn an_installed_llama_server_brings_the_local_models() {
        let (port, _) = fake_server(None).await;
        let ports = ServerPorts::default().with("llamacpp", port);
        let models = tempfile::tempdir().unwrap();
        fs::write(models.path().join("qwen.gguf"), b"GGUF").unwrap();
        let dirs = [models.path().to_path_buf()];
        let lab = lab();
        let env = env_of(&lab, false);
        let d = detect_all(
            &env,
            &real_cache(),
            &ShowCache::default(),
            false,
            &ports,
            &dirs,
        )
        .await;
        assert_eq!(d.local_models, Vec::<PathBuf>::new(), "no llama-server");
        write_script(&lab.bin, "llama-server", "exit 0", 0o755);
        let d = detect_all(
            &env,
            &real_cache(),
            &ShowCache::default(),
            false,
            &ports,
            &dirs,
        )
        .await;
        let llamacpp = d.engines.iter().find(|e| e.id == "llamacpp").unwrap();
        assert_eq!(llamacpp.status, Status::NotRunning);
        assert_eq!(d.local_models, [models.path().join("qwen.gguf")]);
    }

    #[tokio::test]
    async fn the_version_child_sees_the_given_path() {
        let dir = tempfile::tempdir().unwrap();
        // No prelude: the script prints the PATH it was started with.
        let script = dir.path().join("claude");
        fs::write(&script, "#!/bin/sh\necho \"$PATH\"\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let got = version(&script, OsStr::new("/opt/login/bin:/usr/bin")).await;
        assert_eq!(got, Some(b"/opt/login/bin:/usr/bin\n".to_vec()));
    }

    #[tokio::test]
    async fn detection_without_an_env_skips_the_listing() {
        let lab = lab();
        fake_codex(&lab, "cat /dev/null; exit 9");
        let d = detect_with(&cli_targets(&["codex"]), &join(&[&lab.bin])).await;
        let codex = d.engines.iter().find(|e| e.id == "codex").unwrap();
        assert!(codex.model_labels.is_empty());
    }

    #[tokio::test]
    async fn the_codex_probe_takes_the_longer_run_not_the_sum() {
        let lab = lab();
        write_script(
            &lab.bin,
            "codex",
            "if [ \"$1\" = debug ]; then sleep 1; echo '{\"models\":[]}'; else sleep 1; echo codex 0.5.0; fi",
            0o755,
        );
        let env = env_of(&lab, true);
        let started = std::time::Instant::now();
        let cache = real_cache();
        let lister = lister(&env, &cache, false);
        let d = detect_in(&cli_targets(&["codex"]), &join(&[&lab.bin]), Some(lister)).await;
        let took = started.elapsed();
        let codex = d.engines.iter().find(|e| e.id == "codex").unwrap();
        assert_eq!(codex.version.as_deref(), Some("codex 0.5.0"));
        assert!(took < Duration::from_millis(1800), "{took:?}");
    }

    fn real_cache() -> ListingCache {
        ListingCache::new(Arc::new(Instant::now))
    }

    fn lister<'a>(env: &'a CliEnv, cache: &'a ListingCache, force: bool) -> Lister<'a> {
        Lister { env, cache, force }
    }

    /// A clock the test moves by hand.
    fn fake_clock() -> (Clock, Arc<Mutex<Instant>>) {
        let now = Arc::new(Mutex::new(Instant::now()));
        let read = Arc::clone(&now);
        (Arc::new(move || *read.lock().unwrap()), now)
    }

    fn advance(now: &Arc<Mutex<Instant>>, by: Duration) {
        let mut at = now.lock().unwrap();
        *at += by;
    }

    /// A codex whose `debug models` counts its runs and prints `reply`.
    fn counting_codex(lab: &Lab, reply: &str) {
        fake_codex(
            lab,
            &format!("echo run >> '{}/runs'\n{reply}", lab.rec.display()),
        );
    }

    fn runs(lab: &Lab) -> usize {
        fs::read_to_string(lab.rec.join("runs")).map_or(0, |t| t.lines().count())
    }

    #[tokio::test]
    async fn a_listing_is_reused_for_ten_minutes_without_a_second_spawn() {
        let lab = lab();
        counting_codex(&lab, "echo '{\"models\":[]}'");
        let env = env_of(&lab, true);
        let (clock, now) = fake_clock();
        let cache = ListingCache::new(clock);
        let first = cache.listing(&env, false).await;
        assert!(first.is_ok());
        assert_eq!(runs(&lab), 1);
        advance(&now, Duration::from_secs(9 * 60 + 59));
        assert_eq!(cache.listing(&env, false).await, first);
        assert_eq!(runs(&lab), 1, "no second spawn inside the window");
    }

    #[tokio::test]
    async fn the_listing_is_read_again_after_ten_minutes() {
        let lab = lab();
        counting_codex(&lab, "echo '{\"models\":[]}'");
        let env = env_of(&lab, true);
        let (clock, now) = fake_clock();
        let cache = ListingCache::new(clock);
        let _ = cache.listing(&env, false).await;
        advance(&now, LISTING_TTL);
        let _ = cache.listing(&env, false).await;
        assert_eq!(runs(&lab), 2);
        // The new read restarts the window.
        advance(&now, Duration::from_secs(60));
        let _ = cache.listing(&env, false).await;
        assert_eq!(runs(&lab), 2);
    }

    #[tokio::test]
    async fn a_forced_refresh_always_re_lists() {
        let lab = lab();
        counting_codex(&lab, "echo '{\"models\":[]}'");
        let env = env_of(&lab, true);
        let (clock, _now) = fake_clock();
        let cache = ListingCache::new(clock);
        let _ = cache.listing(&env, false).await;
        let _ = cache.listing(&env, true).await;
        let _ = cache.listing(&env, true).await;
        assert_eq!(runs(&lab), 3);
    }

    #[tokio::test]
    async fn a_failure_falls_back_to_a_fresh_cache_and_is_never_cached() {
        let lab = lab();
        counting_codex(&lab, "echo '{\"models\":[]}'");
        let env = env_of(&lab, true);
        let (clock, now) = fake_clock();
        let cache = ListingCache::new(clock);
        let good = cache.listing(&env, false).await;
        assert!(good.is_ok());

        // codex starts failing: a forced refresh keeps the cached list...
        counting_codex(&lab, "exit 2");
        advance(&now, Duration::from_secs(5 * 60));
        assert_eq!(cache.listing(&env, true).await, good);
        // ...and the failure did not extend the cache: it expires on schedule.
        advance(&now, Duration::from_secs(5 * 60));
        assert_eq!(
            cache.listing(&env, false).await,
            Err("codex exited 2".to_owned())
        );
        assert_eq!(runs(&lab), 3);
    }

    #[tokio::test]
    async fn a_failure_with_nothing_cached_is_just_default_and_is_retried() {
        let lab = lab();
        counting_codex(&lab, "exit 2");
        let env = env_of(&lab, true);
        let (clock, _now) = fake_clock();
        let cache = ListingCache::new(clock);
        let failed = Err("codex exited 2".to_owned());
        assert_eq!(cache.listing(&env, false).await, failed);
        assert_eq!(cache.listing(&env, false).await, failed);
        assert_eq!(runs(&lab), 2, "a failure is not remembered");
    }

    #[tokio::test]
    async fn detection_runs_reuse_the_cache_and_only_r_forces() {
        let lab = lab();
        let data = lab.rec.join("listing.json");
        fs::write(&data, LISTING).unwrap();
        counting_codex(&lab, &format!("cat {}", data.display()));
        let env = env_of(&lab, true);
        let (clock, _now) = fake_clock();
        let cache = ListingCache::new(clock);
        let targets = cli_targets(&["codex"]);
        let path = join(&[&lab.bin]);
        for force in [false, false, true] {
            let d = detect_in(&targets, &path, Some(lister(&env, &cache, force))).await;
            let codex = d.engines.iter().find(|e| e.id == "codex").unwrap();
            assert_eq!(codex.model_labels.len(), 7, "force={force}");
        }
        assert_eq!(
            runs(&lab),
            2,
            "startup + menu open share one run; r adds one"
        );
    }

    fn cli_targets(ids: &[&'static str]) -> Vec<Target> {
        ids.iter()
            .map(|id| Engine {
                id,
                kind: Kind::Cli,
                binary: id,
            })
            .collect()
    }

    #[tokio::test]
    async fn version_returns_stdout() {
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(dir.path(), "codex", "echo codex 0.5.0", 0o755);
        assert_eq!(
            version(&script, OsStr::new("/usr/bin:/bin")).await,
            Some(b"codex 0.5.0\n".to_vec())
        );
    }

    #[tokio::test]
    async fn version_gives_none_on_a_non_zero_exit() {
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(dir.path(), "claude", "echo oops\nexit 1", 0o755);
        assert_eq!(version(&script, OsStr::new("/usr/bin:/bin")).await, None);
    }

    /// Dead for our purposes: gone, or a zombie waiting for a reaper.
    fn is_dead(pid: &str) -> bool {
        let state = pid
            .parse()
            .ok()
            .and_then(crate::runtime::proc::process_state);
        matches!(state, None | Some('Z'))
    }

    async fn dead_within(pid: &str, window: Duration) -> bool {
        let deadline = std::time::Instant::now() + window;
        while std::time::Instant::now() < deadline {
            if is_dead(pid) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        is_dead(pid)
    }

    fn read_pid(file: &Path) -> String {
        let pid = fs::read_to_string(file)
            .unwrap_or_default()
            .trim()
            .to_string();
        assert!(pid.parse::<u32>().is_ok(), "the script never ran");
        pid
    }

    #[tokio::test]
    async fn version_kills_a_hanging_child_after_the_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let body = format!("echo $$ > {}\nexec sleep 30", pid_file.display());
        let script = write_script(dir.path(), "hang", &body, 0o755);
        let started = std::time::Instant::now();
        let answer = version(&script, OsStr::new("/usr/bin:/bin")).await;
        let elapsed = started.elapsed();
        assert_eq!(
            (answer, elapsed < Duration::from_millis(2500)),
            (None, true)
        );
        let pid = read_pid(&pid_file);
        assert!(
            dead_within(&pid, Duration::from_secs(5)).await,
            "child {pid} survived the timeout"
        );
    }

    #[tokio::test]
    async fn version_kills_the_whole_process_group_on_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("grandchild");
        let body = format!("sleep 30 &\necho $! > {}\nwait", pid_file.display());
        let script = write_script(dir.path(), "tree", &body, 0o755);
        assert_eq!(version(&script, OsStr::new("/usr/bin:/bin")).await, None);
        let pid = read_pid(&pid_file);
        assert!(
            dead_within(&pid, Duration::from_secs(5)).await,
            "grandchild {pid} survived the timeout"
        );
    }

    #[tokio::test]
    async fn version_kills_the_group_when_the_future_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("grandchild");
        let body = format!("sleep 30 &\necho $! > {}\nwait", pid_file.display());
        let script = write_script(dir.path(), "tree", &body, 0o755);
        // The wrapper is shorter than the probe, so version() is dropped mid-way.
        let cut = timeout(
            Duration::from_millis(500),
            version(&script, OsStr::new("/usr/bin:/bin")),
        )
        .await;
        assert!(cut.is_err(), "the probe finished before it was dropped");
        let pid = read_pid(&pid_file);
        assert!(
            dead_within(&pid, Duration::from_secs(5)).await,
            "grandchild {pid} survived a dropped probe"
        );
    }

    #[tokio::test]
    async fn version_retries_a_spawn_that_hits_a_busy_text_file() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("busy");
        // Open for writing while exec runs: the kernel answers ETXTBSY.
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(b"#!/bin/sh\necho busy 1.0\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        let probe = tokio::spawn({
            let path = path.clone();
            async move { version(&path, OsStr::new("/usr/bin:/bin")).await }
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
        drop(file);
        assert_eq!(probe.await.unwrap(), Some(b"busy 1.0\n".to_vec()));
    }

    #[tokio::test]
    async fn version_caps_the_output_it_reads() {
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(dir.path(), "flood", "exec yes AAAAAAAAAAAAAAAA", 0o755);
        let started = std::time::Instant::now();
        let answer = version(&script, OsStr::new("/usr/bin:/bin")).await;
        assert_eq!(
            (answer, started.elapsed() < Duration::from_millis(1500)),
            (None, true)
        );
    }

    #[tokio::test]
    async fn version_accepts_output_exactly_at_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!("head -c {VERSION_CAP} /dev/zero | tr '\\0' a");
        let script = write_script(dir.path(), "full", &body, 0o755);
        let answer = version(&script, OsStr::new("/usr/bin:/bin")).await;
        assert_eq!(answer.map(|v| v.len()), Some(VERSION_CAP));
    }

    #[tokio::test]
    async fn version_waits_for_the_child_not_for_the_pipe() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("bg");
        let body = format!("sleep 30 &\necho $! > {}\necho v1.0", pid_file.display());
        let script = write_script(dir.path(), "bg", &body, 0o755);
        let started = std::time::Instant::now();
        let answer = version(&script, OsStr::new("/usr/bin:/bin")).await;
        assert_eq!(
            (answer, started.elapsed() < Duration::from_secs(1)),
            (Some(b"v1.0\n".to_vec()), true)
        );
        let pid = read_pid(&pid_file);
        assert!(
            dead_within(&pid, Duration::from_secs(5)).await,
            "background process {pid} survived"
        );
    }

    fn join(dirs: &[&Path]) -> std::ffi::OsString {
        std::env::join_paths(dirs).unwrap()
    }

    /// Serializes the tests that change the process working directory.
    static CWD: Mutex<()> = Mutex::new(());

    #[test]
    fn find_on_path_finds_an_executable_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = write_file(dir.path(), "codex", 0o755);
        assert_eq!(
            find_on_path("codex", &join(&[dir.path()])),
            Some(file.clone())
        );
        assert_eq!(find_on_path("gemini", &join(&[dir.path()])), None);
    }

    #[test]
    fn find_on_path_first_match_wins() {
        let (first, second) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        write_file(second.path(), "codex", 0o755);
        let winner = write_file(first.path(), "codex", 0o755);
        assert_eq!(
            find_on_path("codex", &join(&[first.path(), second.path()])),
            Some(winner)
        );
    }

    #[test]
    fn find_on_path_ignores_files_without_the_exec_bit() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "codex", 0o644);
        assert_eq!(find_on_path("codex", &join(&[dir.path()])), None);
    }

    #[test]
    fn find_on_path_ignores_directories() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("codex")).unwrap();
        assert_eq!(find_on_path("codex", &join(&[dir.path()])), None);
    }

    #[test]
    fn find_on_path_skips_empty_and_relative_entries() {
        let _guard = CWD.lock().unwrap_or_else(|e| e.into_inner());
        let cwd = tempfile::tempdir().unwrap();
        write_file(cwd.path(), "codex", 0o755);
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(cwd.path()).unwrap();
        let found = find_on_path("codex", OsStr::new(":.:bin:"));
        std::env::set_current_dir(previous).unwrap();
        assert_eq!(found, None);
    }

    fn ok_json(body: &str) -> Option<Vec<u8>> {
        Some(
            format!(
                "HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .into_bytes(),
        )
    }

    fn state(id: &'static str, status: Status, models: &[&str], v: Option<&str>) -> EngineState {
        EngineState {
            binary: None,
            id,
            status,
            models: models.iter().map(ToString::to_string).collect(),
            version: v.map(ToString::to_string),
            model_labels: Vec::new(),
            diagnostics: Default::default(),
        }
    }

    struct Fixture {
        targets: Vec<Target>,
        path: std::ffi::OsString,
        servers: Vec<(u16, &'static str, Requests)>,
        marker: PathBuf,
        bystander: TcpListener,
        _dir: tempfile::TempDir,
    }

    /// Four fake servers on ephemeral ports (ollama silent on request), a
    /// PATH dir with fake binaries and a listener nobody should talk to.
    async fn fixture(ollama_silent: bool) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ollama-was-executed");
        write_script(
            dir.path(),
            "ollama",
            &format!("echo x > {}", marker.display()),
            0o755,
        );
        write_script(dir.path(), "vllm", "", 0o755);
        write_script(dir.path(), "claude", "exit 1", 0o755);
        write_script(dir.path(), "codex", "echo codex 0.5.0", 0o755);
        write_script(dir.path(), "gemini", "exec sleep 30", 0o755);
        let tags = ok_json(r#"{"models":[{"name":"llama3:8b"},{"name":"qwen2.5:7b"}]}"#);
        let lms = ok_json(r#"{"data":[{"id":"gemma-3-4b"}]}"#);
        let llama = ok_json(r#"{"data":[{"id":"m.gguf","owned_by":"llamacpp"}]}"#);
        let foreign = ok_json(r#"{"data":[{"id":"x","owned_by":"localai"}]}"#);
        let mut servers = Vec::new();
        let mut targets = Vec::new();
        let rows = [
            (
                "ollama",
                "/api/tags",
                None,
                "ollama",
                if ollama_silent { None } else { tags },
            ),
            ("lmstudio", "/v1/models", None, "lms", lms),
            (
                "llamacpp",
                "/v1/models",
                Some("llamacpp"),
                "llama-server",
                llama,
            ),
            ("vllm", "/v1/models", Some("vllm"), "vllm", foreign),
        ];
        for (id, path, owner, binary, reply) in rows {
            let (port, requests) = fake_server(reply).await;
            servers.push((port, path, requests));
            targets.push(Engine {
                id,
                kind: Kind::Server { port, path, owner },
                binary,
            });
        }
        for id in ["claude", "codex", "gemini", "opencode"] {
            targets.push(Engine {
                id,
                kind: Kind::Cli,
                binary: id,
            });
        }
        Fixture {
            targets,
            path: join(&[dir.path()]),
            servers,
            marker,
            bystander: TcpListener::bind("127.0.0.1:0").await.unwrap(),
            _dir: dir,
        }
    }

    fn expected(ollama: EngineState, bin: &Path) -> Detection {
        let found = |state: EngineState| EngineState {
            binary: Some(bin.join(state.id)),
            ..state
        };
        // The fixture's listing has names only: facts with no size or digest.
        let model_facts = ollama
            .models
            .iter()
            .map(|name| crate::core::ai::ModelFacts {
                engine: "ollama",
                name: name.clone(),
                params: None,
                digest: None,
                tools: Tools::Unknown,
            })
            .collect();
        Detection {
            model_facts,
            engines: vec![
                found(ollama),
                state("lmstudio", Status::Available, &["gemma-3-4b"], None),
                state("llamacpp", Status::Available, &["m.gguf"], None),
                found(state("vllm", Status::NotRunning, &[], None)),
                state("claude", Status::Available, &["default"], None),
                state(
                    "codex",
                    Status::Available,
                    &["default"],
                    Some("codex 0.5.0"),
                ),
                state("gemini", Status::Available, &["default"], None),
                state("opencode", Status::NotInstalled, &[], None),
            ],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn detect_with_probes_every_engine_and_nothing_else() {
        let f = fixture(false).await;
        let started = std::time::Instant::now();
        let detection = detect_with(&f.targets, &f.path).await;
        let ollama = state(
            "ollama",
            Status::Available,
            &["llama3:8b", "qwen2.5:7b"],
            None,
        );
        assert_eq!(
            (detection, started.elapsed() < Duration::from_millis(2500)),
            (expected(ollama, f._dir.path()), true)
        );
        for (port, path, requests) in &f.servers {
            let get = format!(
                "GET {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
            );
            assert_eq!(*requests.lock().unwrap(), vec![get.into_bytes()]);
        }
        let bystander = tokio::time::timeout(Duration::from_millis(50), f.bystander.accept()).await;
        assert!(bystander.is_err(), "an unrelated listener was contacted");
        assert!(!f.marker.exists(), "a binary was run besides --version");
    }

    #[tokio::test]
    async fn detect_with_answers_once_when_a_server_hangs() {
        let f = fixture(true).await;
        let started = std::time::Instant::now();
        let detection = detect_with(&f.targets, &f.path).await;
        assert_eq!(
            (detection, started.elapsed() < Duration::from_millis(2500)),
            (
                expected(
                    state("ollama", Status::NotRunning, &[], None),
                    f._dir.path()
                ),
                true
            )
        );
    }
}

#[cfg(test)]
mod show_tests;
