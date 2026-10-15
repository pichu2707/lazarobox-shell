//! Runtime probes for AI engine detection: loopback HTTP, PATH lookup and
//! `--version`. The facts are classified by the pure `core::ai::reduce`.

use std::{
    ffi::{OsStr, OsString},
    fs,
    net::SocketAddr,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    process::Command,
    sync::mpsc,
    time::timeout,
};

use futures::future::join_all;

use super::proc::GroupKill;
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

/// Longest `opencode models` is allowed to run. It reads a cached list and
/// takes under a second; a slow or hung one only costs this much of the
/// detection, and the engine then offers just `default`.
pub const MODELS_TIMEOUT: Duration = Duration::from_secs(5);

/// Most `opencode models` output kept; more than this means the probe gave up
/// (the list is a few KiB).
const MODELS_CAP: usize = 256 * 1024;

/// One short-lived child whose stdout is read.
struct Capture<'a> {
    args: &'a [&'a str],
    env: &'a [(&'a str, OsString)],
    timeout: Duration,
    cap: usize,
}

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
    let capture = Capture {
        args: &["--version"],
        env: &[],
        timeout: VERSION_TIMEOUT,
        cap: VERSION_CAP,
    };
    run_capture(path, &capture).await
}

/// A private empty folder for one run, removed when dropped. Created `0700`
/// with a name no one can have planted (`mkdir` fails if it exists).
struct Scratch(PathBuf);

impl Scratch {
    fn create() -> Option<Self> {
        use std::os::unix::fs::DirBuilderExt;
        static RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "lazarobox-models-{}-{n}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos())
        ));
        fs::DirBuilder::new().mode(0o700).create(&dir).ok()?;
        Some(Self(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Runs `<path> models` and returns its stdout, the way the chat will see the
/// CLI: no external plugin, no project config, and the config, state and
/// session folders of the user left out (they live in a private empty folder
/// for this run), so the list holds the models a chat turn can use. `None`
/// when it fails, times out after `limit` or prints too much; detection then
/// offers just `default`.
pub async fn opencode_models(path: &Path, limit: Duration) -> Option<Vec<u8>> {
    let scratch = Scratch::create()?;
    let at = |name: &str| scratch.0.join(name).into_os_string();
    let env = [
        ("OPENCODE_PURE", OsString::from("1")),
        ("OPENCODE_DISABLE_PROJECT_CONFIG", OsString::from("1")),
        ("OPENCODE_DISABLE_AUTOUPDATE", OsString::from("1")),
        ("XDG_CONFIG_HOME", at("config")),
        ("XDG_STATE_HOME", at("state")),
        ("OPENCODE_DB", at("opencode.db")),
        ("TMPDIR", scratch.0.clone().into_os_string()),
    ];
    let capture = Capture {
        args: &["--pure", "models"],
        env: &env,
        timeout: limit,
        cap: MODELS_CAP,
    };
    run_capture(path, &capture).await
}

/// Runs a short-lived child and returns its stdout; see [`version`] for the
/// rules (own process group killed on every way out, bounded output).
async fn run_capture(path: &Path, capture: &Capture<'_>) -> Option<Vec<u8>> {
    let mut child = spawn_capture(path, capture).await?;
    let pid = child.id()? as libc::pid_t;
    // Declared after `child`, so it drops first: a dropped future still kills
    // the group while the child has not been reaped yet.
    // A refused pid (never a real fresh group) drops `child`, which kills it
    // through `kill_on_drop`; no group signal is ever sent.
    let mut group = GroupKill::new(pid)?;
    let output = timeout(capture.timeout, collect(&mut child, pid, capture.cap)).await;
    // INVARIANT: the group is signalled while the child is still unreaped.
    // A reaped pid can be recycled, and its pgid with it, so `collect_version`
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

/// Spawns `<path>` with the capture's args. A binary that is being written (an upgrade, or
/// a concurrent fork holding its write handle) fails with ETXTBSY for a
/// moment, so that one error is retried briefly.
async fn spawn_capture(path: &Path, capture: &Capture<'_>) -> Option<tokio::process::Child> {
    for attempt in 0..5 {
        // No shell: the resolved absolute path is the program.
        let spawned = Command::new(path)
            .args(capture.args)
            .envs(capture.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .process_group(0)
            .spawn();
        match spawned {
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
async fn collect(
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
    let probes = join_all(targets.iter().map(|target| probe(target, path))).await;
    reduce(&probes)
}

/// Detection over the real catalog and the process PATH.
pub async fn detect_all() -> Detection {
    let path = std::env::var_os("PATH").unwrap_or_default();
    detect_with(CATALOG, &path).await
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
pub fn spawn_detection(tx: mpsc::Sender<Detection>) {
    spawn_detection_of(tx, detect_all());
}

async fn probe(target: &Target, path: &OsStr) -> Probe {
    let found = find_on_path(target.binary, path);
    match target.kind {
        Kind::Server { port, path, .. } => Probe::Server {
            id: target.id,
            on_path: found.is_some(),
            reply: http_get(port, path).await,
        },
        Kind::Cli => {
            // Only a resolved absolute path is ever run.
            let stdout = match &found {
                Some(file) => version(file).await,
                None => None,
            };
            // Only opencode lists its models; the others have a fixed list.
            let models = match (&found, target.id) {
                (Some(file), "opencode") => opencode_models(file, MODELS_TIMEOUT).await,
                _ => None,
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
    use crate::core::ai::{EngineState, Status};
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

    /// An `opencode` that answers `--version`, and for `models` records how
    /// it was run and prints two ids.
    fn fake_opencode(dir: &Path) -> PathBuf {
        let rec = dir.join("rec");
        let body = format!(
            "for a; do case $a in --version) echo opencode 1.2.3; exit 0;; esac; done\n\
             printf '%s\\n' \"$@\" > {rec}.args\n\
             env > {rec}.env\n\
             echo openai/gpt-5.4\n\
             echo opencode/big-pickle",
            rec = rec.display()
        );
        write_script(dir, "opencode", &body, 0o755)
    }

    #[tokio::test]
    async fn opencode_models_runs_pure_in_a_private_folder_that_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let script = fake_opencode(dir.path());
        let listing = opencode_models(&script, MODELS_TIMEOUT).await;
        assert_eq!(
            listing,
            Some(b"openai/gpt-5.4\nopencode/big-pickle\n".to_vec())
        );
        let args = fs::read_to_string(dir.path().join("rec.args")).unwrap();
        assert_eq!(args, "--pure\nmodels\n");
        let env = fs::read_to_string(dir.path().join("rec.env")).unwrap();
        let var = |name: &str| {
            env.lines()
                .find_map(|l| l.strip_prefix(&format!("{name}=")))
                .map(str::to_owned)
        };
        assert_eq!(var("OPENCODE_PURE").as_deref(), Some("1"));
        assert_eq!(var("OPENCODE_DISABLE_PROJECT_CONFIG").as_deref(), Some("1"));
        let folder = PathBuf::from(var("TMPDIR").expect("TMPDIR"));
        assert!(
            folder.starts_with(std::env::temp_dir()) && folder != std::env::temp_dir(),
            "{folder:?}"
        );
        for name in ["XDG_CONFIG_HOME", "XDG_STATE_HOME", "OPENCODE_DB"] {
            let value = var(name).unwrap_or_default();
            assert!(PathBuf::from(&value).starts_with(&folder), "{name}={value}");
        }
        assert!(!folder.exists(), "the private folder is removed");
    }

    #[tokio::test]
    async fn opencode_models_gives_up_after_its_limit_and_leaves_no_process() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let body = format!("echo $$ > {}\nexec sleep 30", pid_file.display());
        let script = write_script(dir.path(), "opencode", &body, 0o755);
        let started = std::time::Instant::now();
        let listing = opencode_models(&script, Duration::from_millis(300)).await;
        assert_eq!(listing, None);
        assert!(started.elapsed() < Duration::from_secs(2));
        let pid = read_pid(&pid_file);
        assert!(
            dead_within(&pid, Duration::from_secs(5)).await,
            "process {pid} survived"
        );
    }

    #[tokio::test]
    async fn opencode_models_fails_soft_on_an_error_or_too_much_output() {
        let dir = tempfile::tempdir().unwrap();
        let failing = write_script(dir.path(), "fail", "echo oops\nexit 1", 0o755);
        assert_eq!(opencode_models(&failing, MODELS_TIMEOUT).await, None);
        let flood = write_script(dir.path(), "flood", "exec yes openai/gpt-5.4", 0o755);
        assert_eq!(opencode_models(&flood, MODELS_TIMEOUT).await, None);
        let missing = dir.path().join("missing");
        assert_eq!(opencode_models(&missing, MODELS_TIMEOUT).await, None);
    }

    #[tokio::test]
    async fn detection_lists_the_models_opencode_reports_after_default() {
        let dir = tempfile::tempdir().unwrap();
        fake_opencode(dir.path());
        let targets = [Engine {
            id: "opencode",
            kind: Kind::Cli,
            binary: "opencode",
        }];
        let detection = detect_with(&targets, &join(&[dir.path()])).await;
        let found = detection.engines.iter().find(|s| s.id == "opencode");
        assert_eq!(
            found,
            Some(&state(
                "opencode",
                Status::Available,
                &["default", "openai/gpt-5.4", "opencode/big-pickle"],
                Some("opencode 1.2.3")
            ))
        );
    }

    #[tokio::test]
    async fn detection_keeps_default_when_the_listing_fails() {
        let dir = tempfile::tempdir().unwrap();
        write_script(
            dir.path(),
            "opencode",
            "for a; do case $a in --version) echo opencode 1.2.3; exit 0;; esac; done\nexit 1",
            0o755,
        );
        let targets = [Engine {
            id: "opencode",
            kind: Kind::Cli,
            binary: "opencode",
        }];
        let detection = detect_with(&targets, &join(&[dir.path()])).await;
        let found = detection.engines.iter().find(|s| s.id == "opencode");
        assert_eq!(
            found,
            Some(&state(
                "opencode",
                Status::Available,
                &["default"],
                Some("opencode 1.2.3")
            ))
        );
    }

    #[tokio::test]
    async fn only_opencode_is_asked_for_its_models() {
        let dir = tempfile::tempdir().unwrap();
        let rec = dir.path().join("calls");
        let body = format!("echo \"$@\" >> {}\necho tool 1.0", rec.display());
        write_script(dir.path(), "codex", &body, 0o755);
        let targets = [Engine {
            id: "codex",
            kind: Kind::Cli,
            binary: "codex",
        }];
        detect_with(&targets, &join(&[dir.path()])).await;
        assert_eq!(fs::read_to_string(&rec).unwrap(), "--version\n");
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
