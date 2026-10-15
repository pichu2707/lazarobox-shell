//! Runtime probes for AI engine detection: loopback HTTP, PATH lookup and
//! `--version`. The facts are classified by the pure `core::ai::reduce`.

use std::{
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

use futures::future::join_all;

use super::{chat::CliEnv, proc::GroupKill};
use crate::core::ai::{
    CATALOG, Detection, Engine, Kind, Probe,
    http::{MAX_BODY, parse_response},
    reduce,
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
    timeout(HTTP_TIMEOUT, exchange(port, path)).await.ok()?
}

async fn exchange(port: u16, path: &str) -> Option<(u16, Vec<u8>)> {
    // HTTP/1.0 with `Connection: close`: the reply ends at EOF. The host is
    // fixed, so no request can leave the machine.
    let mut stream = TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], port)))
        .await
        .ok()?;
    let request = format!(
        "GET {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
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

/// How long the output is read after the child has exited, for a pipe a
/// background process still holds open.
const EXIT_GRACE: Duration = Duration::from_millis(100);

/// Runs `<path> --version` and returns its stdout. `None` on a spawn
/// failure, a non-zero exit, a timeout or more than `VERSION_CAP` bytes of
/// output (it is dropped, not truncated). The child runs in its own process
/// group, which is killed on every way out so no grandchild survives.
pub async fn version(path: &Path) -> Option<Vec<u8>> {
    run_capped(
        || {
            let mut command = Command::new(path);
            command
                .arg("--version")
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
    let mut child = spawn_capped(&make).await?;
    // The child sees EOF on stdin at once.
    drop(child.stdin.take());
    let pid = child.id()? as libc::pid_t;
    // Declared after `child`, so it drops first: a dropped future still kills
    // the group while the child has not been reaped yet.
    // A refused pid (never a real fresh group) drops `child`, which kills it
    // through `kill_on_drop`; no group signal is ever sent.
    let mut group = GroupKill::new(pid)?;
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
    if timeout(Duration::from_millis(500), group.reap(&mut child))
        .await
        .is_err()
    {
        group.disarm();
    }
    output.ok()?
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
    /// listing while it is still under the TTL, else `None` (just `default`).
    pub async fn listing(&self, env: &CliEnv, force: bool) -> Option<Vec<u8>> {
        let now = (self.clock)();
        let cached = || {
            let last = self.last.lock().unwrap_or_else(|e| e.into_inner());
            last.as_ref()
                .filter(|(at, _)| now.saturating_duration_since(*at) < LISTING_TTL)
                .map(|(_, raw)| raw.clone())
        };
        if !force && let Some(raw) = cached() {
            return Some(raw);
        }
        match codex_listing(env).await {
            Some(raw) => {
                *self.last.lock().unwrap_or_else(|e| e.into_inner()) = Some((now, raw.clone()));
                Some(raw)
            }
            // A failure is never cached as a success.
            None => cached(),
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

/// Runs `codex debug models` and returns its stdout. `None` on any failure
/// (no jail, an old codex without the subcommand, a timeout, too much
/// output).
///
/// The command may refresh the catalog from OpenAI with the user's own
/// codex login and rewrite codex's cache (`models_cache.json`), exactly as
/// running codex does; that is how the list reflects the account. So it runs
/// the way a codex chat turn does: through the jail (`__jail-exec`, the codex
/// layout with its state folder writable), with the chat's scrubbed
/// environment, stdin closed and the private working folder. Like the chat,
/// codex requires the jail: without it nothing is started.
pub async fn codex_listing(env: &CliEnv) -> Option<Vec<u8>> {
    listing(env, LISTING_TIMEOUT, LISTING_CAP).await
}

async fn listing(env: &CliEnv, limit: Duration, cap: usize) -> Option<Vec<u8>> {
    super::chat::run_listing(env, limit, cap).await
}

/// Waits for `pid` to exit WITHOUT reaping it, so the pid stays reserved.
/// `Some(true)` for a clean exit 0.
pub(crate) async fn wait_exit_unreaped(pid: libc::pid_t) -> Option<bool> {
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
            Some(info.si_code == libc::CLD_EXITED && info.si_status() == 0)
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
) -> Option<Vec<u8>> {
    let mut stdout = child.stdout.take()?;
    let mut buf = Vec::new();
    let success = {
        let mut limited = (&mut stdout).take(cap as u64 + 1);
        let read = limited.read_to_end(&mut buf);
        tokio::pin!(read);
        tokio::select! {
            n = &mut read => {
                if n.ok()? > cap {
                    return None;
                }
                wait_exit_unreaped(pid).await?
            }
            status = wait_exit_unreaped(pid) => {
                let _ = timeout(EXIT_GRACE, &mut read).await;
                status?
            }
        }
    };
    (success && buf.len() <= cap).then_some(buf)
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

/// Detection over the real catalog and the process PATH.
pub async fn detect_all(env: &CliEnv, cache: &ListingCache, force: bool) -> Detection {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let lister = Lister { env, cache, force };
    detect_in(CATALOG, &path, Some(lister)).await
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
    force: bool,
) {
    spawn_detection_of(tx, async move { detect_all(&env, &cache, force).await });
}

async fn probe(target: &Target, path: &OsStr, lister: Option<&Lister<'_>>) -> Probe {
    let found = find_on_path(target.binary, path);
    match target.kind {
        Kind::Server { port, path, .. } => Probe::Server {
            id: target.id,
            on_path: found.is_some(),
            reply: http_get(port, path).await,
        },
        Kind::Cli => {
            // Only a resolved absolute path is ever run. Only codex has a
            // model listing, and it runs next to the version probe, so a slow
            // listing never adds to the version's time.
            let (stdout, models) = match &found {
                Some(file) => {
                    let list = async {
                        match (lister, target.id) {
                            (Some(l), "codex") => l.cache.listing(l.env, l.force).await,
                            _ => None,
                        }
                    };
                    tokio::join!(async { version(file).await }, list)
                }
                None => (None, None),
            };
            Probe::Cli {
                id: target.id,
                path: found,
                stdout,
                models,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ai::{
        EngineState, Status,
        jail::{Backend, JailSupport},
    };
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn spawn_detection_of_delivers_the_result() {
        let (tx, mut rx) = mpsc::channel(1);
        let one = Detection {
            engines: vec![state("codex", Status::Available, &["default"], None)],
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
        assert!(started.elapsed() < Duration::from_millis(100));
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
        // A bound socket that never listens holds the port until the end of
        // the test, so a parallel test cannot take it: the connect is refused.
        let held = tokio::net::TcpSocket::new_v4().unwrap();
        held.bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let port = held.local_addr().unwrap().port();
        let answer =
            tokio::time::timeout(Duration::from_millis(400), http_get(port, "/api/tags")).await;
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
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
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
            JailSupport::Unavailable("test")
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
        assert_eq!(
            codex_listing(&env).await,
            Some(b"{\"models\":[]}\n".to_vec())
        );
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
        assert!(codex_listing(&env).await.is_some());
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
        assert_eq!(codex_listing(&env).await, None);
        assert!(!lab.rec.join("ran").exists());
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
        assert_eq!(codex_listing(&env_of(&lab, true)).await, None);
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
            &format!("(sleep 1; echo x > {}) &\nsleep 30", marker.display()),
        );
        let started = std::time::Instant::now();
        let got = listing(&env_of(&lab, true), Duration::from_millis(300), LISTING_CAP).await;
        assert_eq!(got, None);
        assert!(started.elapsed() < Duration::from_secs(3));
        tokio::time::sleep(Duration::from_millis(1300)).await;
        assert!(!marker.exists());
    }

    #[tokio::test]
    async fn listing_drops_output_over_the_cap() {
        let lab = lab();
        fake_codex(&lab, "head -c 2000 /dev/zero | tr '\\0' x");
        let env = env_of(&lab, true);
        assert_eq!(listing(&env, LISTING_TIMEOUT, 1000).await, None);
        assert!(listing(&env, LISTING_TIMEOUT, 2000).await.is_some());
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
        assert!(first.is_some());
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
        cache.listing(&env, false).await;
        advance(&now, LISTING_TTL);
        cache.listing(&env, false).await;
        assert_eq!(runs(&lab), 2);
        // The new read restarts the window.
        advance(&now, Duration::from_secs(60));
        cache.listing(&env, false).await;
        assert_eq!(runs(&lab), 2);
    }

    #[tokio::test]
    async fn a_forced_refresh_always_re_lists() {
        let lab = lab();
        counting_codex(&lab, "echo '{\"models\":[]}'");
        let env = env_of(&lab, true);
        let (clock, _now) = fake_clock();
        let cache = ListingCache::new(clock);
        cache.listing(&env, false).await;
        cache.listing(&env, true).await;
        cache.listing(&env, true).await;
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
        assert!(good.is_some());

        // codex starts failing: a forced refresh keeps the cached list...
        counting_codex(&lab, "exit 2");
        advance(&now, Duration::from_secs(5 * 60));
        assert_eq!(cache.listing(&env, true).await, good);
        // ...and the failure did not extend the cache: it expires on schedule.
        advance(&now, Duration::from_secs(5 * 60));
        assert_eq!(cache.listing(&env, false).await, None);
        assert_eq!(runs(&lab), 3);
    }

    #[tokio::test]
    async fn a_failure_with_nothing_cached_is_just_default_and_is_retried() {
        let lab = lab();
        counting_codex(&lab, "exit 2");
        let env = env_of(&lab, true);
        let (clock, _now) = fake_clock();
        let cache = ListingCache::new(clock);
        assert_eq!(cache.listing(&env, false).await, None);
        assert_eq!(cache.listing(&env, false).await, None);
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
        assert_eq!(version(&script).await, Some(b"codex 0.5.0\n".to_vec()));
    }

    #[tokio::test]
    async fn version_gives_none_on_a_non_zero_exit() {
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(dir.path(), "claude", "echo oops\nexit 1", 0o755);
        assert_eq!(version(&script).await, None);
    }

    /// Dead for our purposes: gone, or a zombie waiting for a reaper.
    fn is_dead(pid: &str) -> bool {
        match fs::read_to_string(format!("/proc/{pid}/stat")) {
            Err(_) => true,
            Ok(stat) => stat
                .rsplit(')')
                .next()
                .is_some_and(|r| r.trim_start().starts_with('Z')),
        }
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
        let answer = version(&script).await;
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
        assert_eq!(version(&script).await, None);
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
        let cut = timeout(Duration::from_millis(500), version(&script)).await;
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
            async move { version(&path).await }
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
        let answer = version(&script).await;
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
        let answer = version(&script).await;
        assert_eq!(answer.map(|v| v.len()), Some(VERSION_CAP));
    }

    #[tokio::test]
    async fn version_waits_for_the_child_not_for_the_pipe() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("bg");
        let body = format!("sleep 30 &\necho $! > {}\necho v1.0", pid_file.display());
        let script = write_script(dir.path(), "bg", &body, 0o755);
        let started = std::time::Instant::now();
        let answer = version(&script).await;
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
            id,
            status,
            models: models.iter().map(ToString::to_string).collect(),
            version: v.map(ToString::to_string),
            model_labels: Vec::new(),
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

    fn expected(ollama: EngineState) -> Detection {
        Detection {
            engines: vec![
                ollama,
                state("lmstudio", Status::Available, &["gemma-3-4b"], None),
                state("llamacpp", Status::Available, &["m.gguf"], None),
                state("vllm", Status::NotRunning, &[], None),
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
            (expected(ollama), true)
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
                expected(state("ollama", Status::NotRunning, &[], None)),
                true
            )
        );
    }
}
