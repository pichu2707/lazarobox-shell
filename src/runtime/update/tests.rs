use std::{
    cell::Cell,
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use super::*;

const DAY: u64 = 24 * 60 * 60;

struct Fake {
    reply: Result<String, Failed>,
    calls: Cell<u32>,
}

impl Fake {
    fn ok(latest: &str) -> Self {
        Self::body(format!(
            r#"{{"crate":{{"max_stable_version":"{latest}"}}}}"#
        ))
    }
    fn body(body: String) -> Self {
        Self {
            reply: Ok(body),
            calls: Cell::new(0),
        }
    }
    fn failing() -> Self {
        Self {
            reply: Err(Failed),
            calls: Cell::new(0),
        }
    }
}

impl Fetcher for Fake {
    fn fetch(&self) -> Result<String, Failed> {
        self.calls.set(self.calls.get() + 1);
        self.reply.clone()
    }
}

fn cache_file() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lazarobox").join("update-check.json");
    (dir, path)
}

fn stored(path: &Path) -> Cache {
    read_cache(path).expect("a cache file")
}

#[test]
fn the_cache_lives_next_to_the_chat_store() {
    let xdg = OsStr::new("/data");
    let home = OsStr::new("/home/u");
    assert_eq!(
        cache_path(Some(xdg), Some(home)),
        Some(PathBuf::from("/data/lazarobox/update-check.json"))
    );
    assert_eq!(
        cache_path(None, Some(home)),
        Some(PathBuf::from(
            "/home/u/.local/share/lazarobox/update-check.json"
        ))
    );
}

#[test]
fn a_relative_or_missing_location_gives_no_cache_path() {
    assert_eq!(
        cache_path(Some(OsStr::new("rel")), Some(OsStr::new("also/rel"))),
        None
    );
    assert_eq!(cache_path(None, None), None);
}

#[test]
fn the_cache_file_round_trips_and_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, path) = cache_file();
    let cache = Cache {
        checked_at: 42,
        ok: true,
        latest: Some("0.16.0".into()),
    };
    write_cache(&path, &cache).unwrap();
    assert_eq!(read_cache(&path), Some(cache));
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn a_missing_oversized_or_damaged_cache_reads_as_none() {
    let (_dir, path) = cache_file();
    assert_eq!(read_cache(&path), None);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "{ not json").unwrap();
    assert_eq!(read_cache(&path), None);
    fs::write(&path, " ".repeat(10_000)).unwrap();
    assert_eq!(read_cache(&path), None);
}

#[test]
fn disabled_shows_nothing_and_checks_nothing() {
    let (_dir, path) = cache_file();
    let newer = Cache {
        checked_at: 1,
        ok: true,
        latest: Some("0.16.0".into()),
    };
    write_cache(&path, &newer).unwrap();
    let s = startup(false, Some(&path), 2, "0.15.1");
    assert_eq!(
        s,
        Startup {
            announce: None,
            due: false
        }
    );
}

#[test]
fn without_a_cache_the_check_is_due_and_nothing_is_shown() {
    let (_dir, path) = cache_file();
    let s = startup(true, Some(&path), 100, "0.15.1");
    assert_eq!(
        s,
        Startup {
            announce: None,
            due: true
        }
    );
}

#[test]
fn a_fresh_cache_announces_without_a_check() {
    let (_dir, path) = cache_file();
    let c = Cache {
        checked_at: 1_000,
        ok: true,
        latest: Some("0.16.0".into()),
    };
    write_cache(&path, &c).unwrap();
    let s = startup(true, Some(&path), 1_000 + DAY - 1, "0.15.1");
    assert_eq!(
        s,
        Startup {
            announce: Some("0.16.0".into()),
            due: false
        }
    );
}

#[test]
fn a_stale_cache_still_announces_while_the_check_runs() {
    let (_dir, path) = cache_file();
    let c = Cache {
        checked_at: 1_000,
        ok: true,
        latest: Some("0.16.0".into()),
    };
    write_cache(&path, &c).unwrap();
    let s = startup(true, Some(&path), 1_000 + DAY, "0.15.1");
    assert_eq!(s.announce.as_deref(), Some("0.16.0"));
    assert!(s.due);
}

#[test]
fn an_installed_version_in_the_cache_is_due_at_once_and_announces_nothing() {
    let (_dir, path) = cache_file();
    let c = Cache {
        checked_at: 1_000,
        ok: true,
        latest: Some("0.16.0".into()),
    };
    write_cache(&path, &c).unwrap();
    // One minute later the user runs 0.16.0: the cached answer is old news.
    let s = startup(true, Some(&path), 1_060, "0.16.0");
    assert_eq!(
        s,
        Startup {
            announce: None,
            due: true
        }
    );
}

#[test]
fn a_quick_restart_after_the_stamp_does_not_ask_again() {
    let (_dir, path) = cache_file();
    // What `begin` leaves while a check is in flight: failed, latest kept.
    let c = Cache {
        checked_at: 1_000,
        ok: false,
        latest: Some("0.16.0".into()),
    };
    write_cache(&path, &c).unwrap();
    assert!(!startup(true, Some(&path), 1_060, "0.16.0").due);
}

#[test]
fn no_cache_location_means_no_check() {
    let s = startup(true, None, 100, "0.15.1");
    assert_eq!(
        s,
        Startup {
            announce: None,
            due: false
        }
    );
}

#[test]
fn a_refresh_stores_the_new_version_and_announces_it() {
    let (_dir, path) = cache_file();
    let fake = Fake::ok("0.16.0");
    assert_eq!(
        refresh(&path, 500, "0.15.1", &fake).as_deref(),
        Some("0.16.0")
    );
    assert_eq!(fake.calls.get(), 1);
    assert_eq!(
        stored(&path),
        Cache {
            checked_at: 500,
            ok: true,
            latest: Some("0.16.0".into())
        }
    );
}

#[test]
fn a_refresh_at_the_current_version_announces_nothing() {
    let (_dir, path) = cache_file();
    let fake = Fake::ok("0.15.1");
    assert_eq!(refresh(&path, 500, "0.15.1", &fake), None);
    assert_eq!(stored(&path).latest.as_deref(), Some("0.15.1"));
}

#[test]
fn a_failed_refresh_keeps_the_cached_newer_version_and_moves_the_clock() {
    let (_dir, path) = cache_file();
    write_cache(
        &path,
        &Cache {
            checked_at: 1,
            ok: true,
            latest: Some("0.16.0".into()),
        },
    )
    .unwrap();
    let fake = Fake::failing();
    assert_eq!(
        refresh(&path, 900, "0.15.1", &fake).as_deref(),
        Some("0.16.0")
    );
    assert_eq!(
        stored(&path),
        Cache {
            checked_at: 900,
            ok: false,
            latest: Some("0.16.0".into())
        }
    );
}

#[test]
fn a_failed_first_refresh_still_stamps_the_clock() {
    let (_dir, path) = cache_file();
    assert_eq!(refresh(&path, 900, "0.15.1", &Fake::failing()), None);
    let c = stored(&path);
    assert_eq!((c.checked_at, c.ok, c.latest), (900, false, None));
}

#[test]
fn a_bad_reply_counts_as_a_failure() {
    for body in [
        "",
        "<html>",
        "{}",
        r#"{"crate":{"max_stable_version":"x.y"}}"#,
    ] {
        let (_dir, path) = cache_file();
        let fake = Fake::body(body.to_owned());
        assert_eq!(refresh(&path, 900, "0.15.1", &fake), None, "{body:?}");
        assert!(!stored(&path).ok, "{body:?}");
    }
}

#[test]
fn an_unwritable_cache_never_panics() {
    let (dir, _) = cache_file();
    // The parent "directory" is a file: create_dir_all and the write fail.
    let blocker = dir.path().join("blocker");
    fs::write(&blocker, "x").unwrap();
    let path = blocker.join("sub").join("update-check.json");
    let fake = Fake::ok("0.16.0");
    assert_eq!(
        refresh(&path, 1, "0.15.1", &fake).as_deref(),
        Some("0.16.0")
    );
}

#[test]
fn the_spawned_refresh_sends_its_result_and_survives_a_closed_channel() {
    let (_dir, path) = cache_file();
    let (tx, mut rx) = mpsc::channel(1);
    spawn_refresh(path.clone(), 500, "0.15.1", Fake::ok("0.16.0"), tx);
    let got = loop {
        match rx.try_recv() {
            Ok(v) => break v,
            Err(mpsc::error::TryRecvError::Empty) => thread::sleep(Duration::from_millis(5)),
            Err(e) => panic!("{e:?}"),
        }
    };
    assert_eq!(got.as_deref(), Some("0.16.0"));

    let (tx, rx) = mpsc::channel(1);
    drop(rx);
    spawn_refresh(path, 600, "0.15.1", Fake::ok("0.16.0"), tx);
    thread::sleep(Duration::from_millis(100));
}

#[test]
fn the_user_agent_names_the_app_the_version_and_the_repository() {
    assert_eq!(
        user_agent("0.15.1"),
        "lazarobox-shell/0.15.1 (+https://github.com/pichu2707/lazarobox-shell)"
    );
}

fn request(url: &str, https_only: bool) -> CurlFetcher {
    CurlFetcher {
        program: "curl".into(),
        path: "/usr/bin".into(),
        url: url.to_owned(),
        user_agent: "ua/1".into(),
        https_only,
        noproxy: false,
        cwd: None,
        read_cap: REPLY_CAP,
        file_cap: REPLY_CAP,
    }
}

#[test]
fn the_request_is_one_quiet_https_get_with_no_redirects_and_no_download() {
    let args = request("https://example.test/x", true).args();
    assert_eq!(args[0], "-q", "ignore ~/.curlrc");
    let has = |a: &str| args.iter().any(|x| x == a);
    assert!(has("--proto") && has("=https"));
    assert!(has("--max-time") && has("5"));
    assert!(has("--max-filesize"));
    for banned in ["-L", "--location", "-o", "-O", "-X", "-d", "--data", "-T"] {
        assert!(!has(banned), "{banned}");
    }
    let n = args.len();
    assert_eq!(&args[n - 2..], ["--", "https://example.test/x"]);
    let ua = args.iter().position(|a| a == "--user-agent").unwrap();
    assert_eq!(args[ua + 1], "ua/1");
}

#[test]
fn plain_http_is_only_allowed_when_asked_for() {
    assert!(
        !request("http://x", false)
            .args()
            .iter()
            .any(|a| a == "=https")
    );
    assert!(
        request("http://x", true)
            .args()
            .iter()
            .any(|a| a == "=https")
    );
}

#[test]
fn the_production_request_honours_the_users_proxy() {
    // Corporate proxies and CA bundles come from the environment curl reads.
    let args = CurlFetcher::crates_io("/usr/bin".into()).args();
    assert!(!args.iter().any(|a| a.contains("noproxy")));
    assert!(!args.iter().any(|a| a == "--insecure" || a == "-k"));
}

// --- the real curl against a local fake server -------------------------

/// Serves one connection per entry of `replies` and records each request.
fn serve(replies: Vec<&'static str>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://{}/api/v1/crates/lazarobox-shell",
        listener.local_addr().unwrap()
    );
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    thread::spawn(move || {
        for reply in replies {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            log.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf[..n]).into_owned());
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    (url, seen)
}

fn curl_available() -> bool {
    Command::new("curl")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Ends the test early, saying so, when the machine has no curl.
macro_rules! need_curl {
    () => {
        if !curl_available() {
            eprintln!("SKIPPED: curl is not installed; the curl-backed checks did not run");
            return;
        }
    };
}

/// A test request: plain HTTP to a local server, and never through the
/// developer's proxy.
fn fetcher(url: String) -> CurlFetcher {
    CurlFetcher {
        program: "curl".into(),
        path: std::env::var_os("PATH").unwrap_or_default(),
        url,
        user_agent: user_agent("9.9.9"),
        https_only: false,
        noproxy: true,
        cwd: None,
        read_cap: REPLY_CAP,
        file_cap: REPLY_CAP,
    }
}

const OK: &str = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 17\r\n\r\n{\"crate\":{\"a\":1}}";

#[test]
fn a_200_gives_the_body_and_sends_the_user_agent() {
    need_curl!();
    let (url, seen) = serve(vec![OK]);
    assert_eq!(fetcher(url).fetch().as_deref(), Ok("{\"crate\":{\"a\":1}}"));
    let request = seen.lock().unwrap()[0].clone();
    assert!(
        request.starts_with("GET /api/v1/crates/lazarobox-shell HTTP/1.1"),
        "{request}"
    );
    assert!(
        request.contains("lazarobox-shell/9.9.9 (+https://github.com/pichu2707/lazarobox-shell)"),
        "{request}"
    );
    assert!(!request.to_lowercase().contains("cookie"));
    assert!(!request.to_lowercase().contains("authorization"));
}

#[test]
fn any_non_200_is_a_failure() {
    need_curl!();
    for reply in [
        "HTTP/1.1 404 Not Found\r\nConnection: close\r\nContent-Length: 2\r\n\r\n{}",
        "HTTP/1.1 500 Oops\r\nConnection: close\r\nContent-Length: 2\r\n\r\n{}",
        "HTTP/1.1 301 Moved\r\nLocation: http://127.0.0.1:1/\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n",
    ] {
        let (url, _) = serve(vec![reply]);
        assert_eq!(fetcher(url).fetch(), Err(Failed), "{reply}");
    }
}

#[test]
fn a_refused_connection_is_a_failure() {
    need_curl!();
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    assert_eq!(
        fetcher(format!("http://127.0.0.1:{port}/")).fetch(),
        Err(Failed)
    );
}

#[test]
fn a_server_that_never_answers_times_out() {
    need_curl!();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let started = Instant::now();
    assert_eq!(fetcher(url).fetch(), Err(Failed));
    assert!(started.elapsed() < Duration::from_secs(TIMEOUT_SECS + 3));
    drop(listener);
}

#[test]
fn a_missing_curl_is_a_failure_not_a_panic() {
    let mut f = fetcher("http://127.0.0.1:1/".into());
    f.program = "lazarobox-no-such-program".into();
    assert_eq!(f.fetch(), Err(Failed));
}

#[test]
fn the_production_request_is_https_only_and_targets_crates_io() {
    let f = CurlFetcher::crates_io("/usr/bin".into());
    assert!(f.https_only);
    assert_eq!(f.url, "https://crates.io/api/v1/crates/lazarobox-shell");
    assert!(f.user_agent.starts_with("lazarobox-shell/"));
}

// --- the start-up entry ---------------------------------------------------

#[derive(Clone)]
struct Counting {
    calls: Arc<std::sync::atomic::AtomicU32>,
    latest: &'static str,
}

impl Fetcher for Counting {
    fn fetch(&self) -> Result<String, Failed> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(format!(
            r#"{{"crate":{{"max_stable_version":"{}"}}}}"#,
            self.latest
        ))
    }
}

fn counting(latest: &'static str) -> (Counting, Arc<std::sync::atomic::AtomicU32>) {
    let calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
    (
        Counting {
            calls: Arc::clone(&calls),
            latest,
        },
        calls,
    )
}

fn settle() {
    thread::sleep(Duration::from_millis(150));
}

#[test]
fn begin_with_the_check_off_makes_no_request_and_shows_nothing() {
    let (_dir, path) = cache_file();
    write_cache(
        &path,
        &Cache {
            checked_at: 1,
            ok: true,
            latest: Some("0.16.0".into()),
        },
    )
    .unwrap();
    let (fetcher, calls) = counting("0.17.0");
    let (tx, mut rx) = mpsc::channel(1);
    assert_eq!(
        begin(false, Some(path), 1 + 2 * DAY, "0.15.1", fetcher, tx),
        None
    );
    settle();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(rx.try_recv().is_err());
}

#[test]
fn begin_within_24_hours_uses_the_cache_and_makes_no_request() {
    let (_dir, path) = cache_file();
    write_cache(
        &path,
        &Cache {
            checked_at: 1_000,
            ok: true,
            latest: Some("0.16.0".into()),
        },
    )
    .unwrap();
    let (fetcher, calls) = counting("0.17.0");
    let (tx, mut rx) = mpsc::channel(1);
    let shown = begin(true, Some(path), 1_000 + DAY - 1, "0.15.1", fetcher, tx);
    assert_eq!(shown.as_deref(), Some("0.16.0"));
    settle();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(rx.try_recv().is_err());
}

#[test]
fn begin_when_due_returns_at_once_and_reports_later() {
    let (_dir, path) = cache_file();
    let (fetcher, calls) = counting("0.16.0");
    let (tx, mut rx) = mpsc::channel(1);
    assert_eq!(
        begin(true, Some(path.clone()), 5_000, "0.15.1", fetcher, tx),
        None
    );
    let got = loop {
        match rx.try_recv() {
            Ok(v) => break v,
            Err(mpsc::error::TryRecvError::Empty) => thread::sleep(Duration::from_millis(5)),
            Err(e) => panic!("{e:?}"),
        }
    };
    assert_eq!(got.as_deref(), Some("0.16.0"));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(stored(&path).latest.as_deref(), Some("0.16.0"));
}

#[test]
fn begin_without_a_cache_location_makes_no_request() {
    let (fetcher, calls) = counting("0.16.0");
    let (tx, _rx) = mpsc::channel(1);
    assert_eq!(begin(true, None, 5_000, "0.15.1", fetcher, tx), None);
    settle();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn the_clock_reads_unix_seconds() {
    assert!(now_secs() > 1_700_000_000);
}

// --- review fixes -----------------------------------------------------------

#[test]
fn an_endless_reply_is_cut_at_the_cap_and_curl_is_killed() {
    need_curl!();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n");
        let chunk = format!("{:x}\r\n{}\r\n", 4096, "a".repeat(4096));
        // Until the client goes away.
        let mut sent = 0usize;
        while stream.write_all(chunk.as_bytes()).is_ok() {
            sent += chunk.len();
        }
        sent
    });
    let mut f = fetcher(url);
    f.read_cap = 10_000;
    f.file_cap = 1_000_000_000;
    assert_eq!(f.fetch(), Err(Failed));
    // Killed at the cap: the server could push only what the sockets buffer,
    // not the 1 GB that curl's own limit would have let through.
    let sent = server.join().unwrap();
    assert!(sent < 32 * 1024 * 1024, "{sent} bytes were sent");
}

#[test]
fn a_reply_within_the_cap_is_kept() {
    need_curl!();
    let (url, _) = serve(vec![OK]);
    let mut f = fetcher(url);
    f.read_cap = 64;
    assert_eq!(f.fetch().as_deref(), Ok("{\"crate\":{\"a\":1}}"));
}

#[test]
fn a_fifo_at_the_cache_path_does_not_block() {
    let (_dir, path) = cache_file();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let c_path = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    // SAFETY: a valid NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
    let (tx, rx) = std::sync::mpsc::channel();
    let probe = path.clone();
    thread::spawn(move || tx.send(read_cache(&probe)).unwrap());
    let got = rx
        .recv_timeout(Duration::from_secs(3))
        .expect("read_cache blocked");
    assert_eq!(got, None);
}

#[test]
fn a_symlink_at_the_cache_path_is_not_followed() {
    let (dir, path) = cache_file();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let real = dir.path().join("real.json");
    fs::write(
        &real,
        Cache {
            checked_at: 1,
            ok: true,
            latest: None,
        }
        .to_json(),
    )
    .unwrap();
    std::os::unix::fs::symlink(&real, &path).unwrap();
    assert_eq!(read_cache(&path), None);
}

#[test]
fn the_temp_file_is_private_unique_and_never_left_behind() {
    let (_dir, path) = cache_file();
    write_cache(&path, &Cache::default()).unwrap();
    let names: Vec<_> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, ["update-check.json"]);
    assert!(
        temp_path(&path)
            .to_string_lossy()
            .contains(&std::process::id().to_string())
    );
}

#[test]
fn a_planted_temp_symlink_is_not_written_through() {
    let (dir, path) = cache_file();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let victim = dir.path().join("victim");
    fs::write(&victim, "keep").unwrap();
    std::os::unix::fs::symlink(&victim, temp_path(&path)).unwrap();
    write_cache(&path, &Cache::default()).unwrap();
    assert_eq!(fs::read_to_string(&victim).unwrap(), "keep");
    assert!(read_cache(&path).is_some());
}

#[test]
fn a_failed_write_removes_its_temp_file() {
    let (_dir, path) = cache_file();
    fs::create_dir_all(&path).unwrap(); // the target is a directory: rename fails
    assert!(write_cache(&path, &Cache::default()).is_err());
    assert!(!temp_path(&path).exists());
}

#[test]
fn begin_stamps_the_clock_before_the_request() {
    let (_dir, path) = cache_file();
    // A fetcher that never answers in the test's lifetime.
    struct Slow;
    impl Fetcher for Slow {
        fn fetch(&self) -> Result<String, Failed> {
            thread::sleep(Duration::from_secs(2));
            Err(Failed)
        }
    }
    let (tx, rx) = mpsc::channel(1);
    assert_eq!(
        begin(true, Some(path.clone()), 7_000, "0.15.1", Slow, tx),
        None
    );
    // Right away, while the request is in flight, a quit-and-restart finds it.
    let c = stored(&path);
    assert_eq!(c.checked_at, 7_000);
    let again = startup(true, Some(&path), 7_000 + 60, "0.15.1");
    assert!(!again.due, "a quick restart must not ask again");
    // Let the request thread finish before the temp directory goes away, so
    // its write cannot recreate it.
    let mut rx = rx;
    let deadline = Instant::now() + Duration::from_secs(10);
    while rx.try_recv().is_err() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn an_empty_or_relative_path_never_runs_a_planted_curl() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("ran");
    let planted = dir.path().join("curl");
    fs::write(
        &planted,
        format!("#!/bin/sh\ntouch {}\nprintf 'x\\n200'\n", marker.display()),
    )
    .unwrap();
    fs::set_permissions(&planted, fs::Permissions::from_mode(0o755)).unwrap();
    for path in ["", ":", "relative:/usr/bin", "."] {
        let mut f = fetcher("http://127.0.0.1:1/".into());
        f.path = path.into();
        f.cwd = Some(dir.path().to_owned());
        assert_eq!(f.fetch(), Err(Failed), "{path:?}");
        assert!(!marker.exists(), "{path:?} ran the planted curl");
    }
}

#[test]
fn begin_does_not_ask_when_the_cache_cannot_be_written() {
    let (dir, _) = cache_file();
    let blocker = dir.path().join("blocker");
    fs::write(&blocker, "x").unwrap();
    let path = blocker.join("sub").join("update-check.json");
    let (fetcher, calls) = counting("0.16.0");
    let (tx, mut rx) = mpsc::channel(1);
    assert_eq!(begin(true, Some(path), 5_000, "0.15.1", fetcher, tx), None);
    settle();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(rx.try_recv().is_err());
}

#[test]
fn begin_does_not_ask_on_a_read_only_cache_directory() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, path) = cache_file();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::set_permissions(path.parent().unwrap(), fs::Permissions::from_mode(0o500)).unwrap();
    // A superuser ignores the mode: nothing to prove then.
    // SAFETY: geteuid cannot fail.
    let writable_anyway = unsafe { libc::geteuid() } == 0;
    let (fetcher, calls) = counting("0.16.0");
    let (tx, _rx) = mpsc::channel(1);
    begin(true, Some(path.clone()), 5_000, "0.15.1", fetcher, tx);
    settle();
    let parent = path.parent().unwrap();
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();
    if !writable_anyway {
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }
}
