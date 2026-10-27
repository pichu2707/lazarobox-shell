use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use super::*;
use crate::core::ai::hf::{Choice, HubFile, Job, License};

const COMMIT: &str = "9217f5db79a29953eb74d5343926648285ec7e67";

pub(in crate::runtime) fn curl_available() -> bool {
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

pub(in crate::runtime) type Handler = dyn Fn(&str) -> Vec<u8> + Send + Sync;

/// A local HTTP server answering every request with `handler(request)`,
/// recording each request's head.
pub(in crate::runtime) fn serve(handler: Box<Handler>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let handler: Arc<Handler> = Arc::from(handler);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let handler = Arc::clone(&handler);
            let log = Arc::clone(&log);
            thread::spawn(move || {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut chunk) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                let request = String::from_utf8_lossy(&buf).into_owned();
                log.lock().unwrap().push(request.clone());
                let _ = stream.write_all(&handler(&request));
            });
        }
    });
    (base, seen)
}

pub(in crate::runtime) fn reply(status: &str, body: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

pub(in crate::runtime) fn ok(body: &str) -> Vec<u8> {
    reply("200 OK", body.as_bytes())
}

/// A test curl: plain HTTP to the local server, never through a proxy.
fn curl(base: &str, token: Option<&str>) -> Curl {
    Curl {
        label: "Hugging Face",
        program: "curl".into(),
        path: std::env::var_os("PATH").unwrap_or_default(),
        base: base.to_owned(),
        https_only: false,
        noproxy: true,
        token: token.map(|t| Secret::new(t.to_owned())),
        user_agent: "lazarobox-shell/9.9.9".to_owned(),
    }
}

/// The sha256 of `bytes` in hex: the object id the Hub gives an LFS file.
pub(in crate::runtime) fn sha(bytes: &[u8]) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(bytes))
}

fn oid(n: u8) -> String {
    format!("{n:02x}").repeat(32)
}

fn job(files: &[(&str, u64, u8)]) -> Job {
    let files: Vec<HubFile> = files
        .iter()
        .map(|(path, size, n)| HubFile {
            path: (*path).to_owned(),
            size: *size,
            oid: oid(*n),
        })
        .collect();
    Job {
        id: "o/r".into(),
        commit: COMMIT.into(),
        choice: Choice {
            quant: "Q4_K_M".into(),
            size: files.iter().map(|f| f.size).sum(),
            files,
        },
        license: License {
            id: "mit".into(),
            link: "https://huggingface.co/o/r".into(),
        },
    }
}

pub(in crate::runtime) fn header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    request.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

// Spec (model-download, Hub requests): Only HTTPS to the Hub.
#[test]
fn the_hub_requests_are_https_only_and_end_with_the_url() {
    let hub = Curl::hub("/usr/bin".into(), Some(Secret::new("secret-123".into())));
    let api = hub.api_args("/api/models/o/r");
    assert_eq!(api[0], "-q");
    for flag in [["--proto", "=https"], ["--proto-redir", "=https"]] {
        assert!(api.windows(2).any(|w| w == flag), "{api:?}");
    }
    assert_eq!(
        api[api.len() - 2..],
        ["--", "https://huggingface.co/api/models/o/r"]
    );
    assert!(!api.iter().any(|a| a == "--location"), "{api:?}");
    let download = hub.download_args("/o/r/resolve/c/a.gguf", Path::new("/x/a.incomplete"));
    for flag in ["--location", "--fail"] {
        assert!(download.iter().any(|a| a == flag), "{download:?}");
    }
    assert!(
        download.windows(2).any(|w| w == ["--continue-at", "-"]),
        "{download:?}"
    );
    assert_eq!(
        download[download.len() - 2..],
        ["--", "https://huggingface.co/o/r/resolve/c/a.gguf"]
    );
    // Spec (Token): Token never in argv.
    for args in [api, download] {
        assert!(!args.iter().any(|a| a.contains("secret-123")), "{args:?}");
        assert!(args.windows(2).any(|w| w == ["--config", "-"]), "{args:?}");
    }
    assert_eq!(
        hub.stdin_config().as_ref().map(Secret::expose),
        Some("header = \"Authorization: Bearer secret-123\"\n")
    );
    // Without a token, nothing is read from stdin.
    let anonymous = Curl::hub("/usr/bin".into(), None);
    assert!(!anonymous.api_args("/x").iter().any(|a| a == "--config"));
    assert_eq!(anonymous.stdin_config(), None);
}

#[test]
fn a_token_that_cannot_be_a_header_is_not_sent() {
    for bad in ["a\"b", "a\\b", "a b", "a\nb"] {
        assert_eq!(
            Curl::hub("/usr/bin".into(), Some(Secret::new(bad.into()))).stdin_config(),
            None,
            "{bad:?}"
        );
    }
}

#[tokio::test]
async fn a_search_lists_repos_and_sends_the_token_once() {
    need_curl!();
    let (base, seen) = serve(Box::new(|_| {
        ok(r#"[{"id":"Qwen/Qwen2.5-0.5B-Instruct-GGUF","downloads":335444}]"#)
    }));
    let hits = search(&curl(&base, Some("tok-1")), "qwen 0.5b")
        .await
        .unwrap();
    assert_eq!(hits[0].id, "Qwen/Qwen2.5-0.5B-Instruct-GGUF");
    let request = seen.lock().unwrap()[0].clone();
    assert!(
        request.starts_with("GET /api/models?search=qwen%200.5b&filter=gguf"),
        "{request}"
    );
    assert_eq!(header(&request, "authorization"), Some("Bearer tok-1"));
    assert_eq!(
        header(&request, "user-agent"),
        Some("lazarobox-shell/9.9.9")
    );
}

#[tokio::test]
async fn a_repo_brings_its_license_and_choices() {
    need_curl!();
    let info = format!(
        r#"{{"id":"o/r","sha":"{COMMIT}","cardData":{{"license":"apache-2.0"}},"siblings":[{{"rfilename":"LICENSE"}}]}}"#
    );
    let tree = format!(
        r#"[{{"type":"file","oid":"{}","size":5,"lfs":{{"oid":"{}","size":5}},"path":"a-Q4_K_M.gguf"}}]"#,
        "1".repeat(40),
        oid(1)
    );
    let (base, seen) = serve(Box::new(move |request| {
        if request.starts_with("GET /api/models/o/r/tree/") {
            ok(&tree)
        } else {
            ok(&info)
        }
    }));
    let files = repo_files(&curl(&base, None), "o/r").await.unwrap();
    assert_eq!(files.license.id, "apache-2.0");
    assert_eq!(
        files.license.link,
        format!("https://huggingface.co/o/r/blob/{COMMIT}/LICENSE")
    );
    assert_eq!(files.choices.len(), 1);
    let requests = seen.lock().unwrap();
    assert!(requests[1].starts_with(&format!("GET /api/models/o/r/tree/{COMMIT}?recursive=true")));
    assert_eq!(header(&requests[0], "authorization"), None);
}

// Spec (model-download, Hub requests): Failures by status.
#[tokio::test]
async fn failures_name_the_repo_and_the_status() {
    need_curl!();
    let (base, _) = serve(Box::new(|request| {
        if request.contains("/gated/") {
            reply("401 Unauthorized", b"{}")
        } else if request.contains("/gone/") {
            reply("404 Not Found", b"{}")
        } else {
            reply("500 Oops", b"{}")
        }
    }));
    let curl = curl(&base, None);
    assert_eq!(
        repo_files(&curl, "gated/m").await,
        Err("gated/m needs a Hugging Face token: set HF_TOKEN".to_owned())
    );
    assert_eq!(
        repo_files(&curl, "gone/m").await,
        Err("gone/m not found on Hugging Face".to_owned())
    );
    assert_eq!(
        search(&curl, "x").await,
        Err("Hugging Face request failed: HTTP 500".to_owned())
    );
    let mut missing = curl;
    missing.program = "lazarobox-no-such-curl".into();
    assert_eq!(search(&missing, "x").await, Err(NO_CURL.to_owned()));
}

/// A server for the files of `o/r` at COMMIT: `a.gguf` holds `body`; a
/// Range request gets the rest from that offset.
pub(in crate::runtime) fn file_server(body: &'static [u8]) -> (String, Arc<Mutex<Vec<String>>>) {
    serve(Box::new(move |request| {
        let from = header(request, "range")
            .and_then(|r| r.strip_prefix("bytes="))
            .and_then(|r| r.trim_end_matches('-').parse::<usize>().ok());
        match from {
            Some(from) => {
                let rest = &body[from..];
                let mut out = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {from}-{}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len() - 1,
                    body.len(),
                    rest.len()
                )
                .into_bytes();
                out.extend_from_slice(rest);
                out
            }
            None => reply("200 OK", body),
        }
    }))
}

pub(in crate::runtime) fn no_cancel() -> oneshot::Receiver<()> {
    let (tx, rx) = oneshot::channel();
    std::mem::forget(tx);
    rx
}

// Spec (model-download, Download into the Hugging Face cache): Layout after
// a download.
#[tokio::test]
async fn a_download_lands_in_the_hub_layout() {
    need_curl!();
    let (base, _) = file_server(b"GGUF-model-bytes");
    let hub = tempfile::tempdir().unwrap();
    let mut job = job(&[("a-Q4_K_M.gguf", 16, 1)]);
    let id = sha(b"GGUF-model-bytes");
    job.choice.files[0].oid = id.clone();
    let mut seen = Vec::new();
    let end = download(
        &curl(&base, None),
        hub.path(),
        &job,
        |d, t| seen.push((d, t)),
        no_cancel(),
    )
    .await;
    let repo = hub.path().join("models--o--r");
    let snapshot = repo.join(format!("snapshots/{COMMIT}/a-Q4_K_M.gguf"));
    assert_eq!(end, End::Done(snapshot.clone()));
    assert_eq!(std::fs::read(&snapshot).unwrap(), b"GGUF-model-bytes");
    assert_eq!(
        std::fs::read_link(&snapshot).unwrap(),
        PathBuf::from(format!("../../blobs/{}", id))
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("refs/main")).unwrap(),
        COMMIT
    );
    assert!(!repo.join(format!("blobs/{}.incomplete", id)).exists());
    assert_eq!(seen.last(), Some(&(16, 16)));
    // The local model scan lists it.
    let found = crate::runtime::gguf::find(&[hub.path().to_path_buf()]);
    assert_eq!(found, [snapshot]);
}

// Spec: Resume and cancel.
#[tokio::test]
async fn a_partial_download_resumes_from_where_it_stopped() {
    need_curl!();
    let (base, seen) = file_server(b"0123456789");
    let hub = tempfile::tempdir().unwrap();
    let blobs = hub.path().join("models--o--r/blobs");
    std::fs::create_dir_all(&blobs).unwrap();
    let mut job = job(&[("a.gguf", 10, 1)]);
    let id = sha(b"0123456789");
    job.choice.files[0].oid = id.clone();
    std::fs::write(blobs.join(format!("{id}.incomplete")), b"0123").unwrap();
    let end = download(&curl(&base, None), hub.path(), &job, |_, _| {}, no_cancel()).await;
    assert!(matches!(end, End::Done(_)), "{end:?}");
    assert_eq!(std::fs::read(blobs.join(id)).unwrap(), b"0123456789");
    let request = seen.lock().unwrap()[0].clone();
    assert_eq!(header(&request, "range"), Some("bytes=4-"));
}

// A partial file longer than the file cannot be resumed: it is removed and
// the download starts over.
#[tokio::test]
async fn an_oversized_partial_file_is_removed_and_fetched_again() {
    need_curl!();
    let (base, seen) = file_server(b"0123456789");
    let hub = tempfile::tempdir().unwrap();
    let blobs = hub.path().join("models--o--r/blobs");
    std::fs::create_dir_all(&blobs).unwrap();
    let mut job = job(&[("a.gguf", 10, 1)]);
    let id = sha(b"0123456789");
    job.choice.files[0].oid = id.clone();
    std::fs::write(blobs.join(format!("{id}.incomplete")), b"0123456789xx").unwrap();
    let end = download(&curl(&base, None), hub.path(), &job, |_, _| {}, no_cancel()).await;
    assert!(matches!(end, End::Done(_)), "{end:?}");
    assert_eq!(std::fs::read(blobs.join(&id)).unwrap(), b"0123456789");
    assert!(!blobs.join(format!("{id}.incomplete")).exists());
    let request = seen.lock().unwrap()[0].clone();
    assert_eq!(header(&request, "range"), None);
}

#[tokio::test]
async fn a_cancelled_download_keeps_its_partial_file_and_no_link() {
    need_curl!();
    // A server that sends a few bytes and then stalls.
    let (base, _) = serve(Box::new(|_| {
        thread::sleep(std::time::Duration::from_secs(30));
        Vec::new()
    }));
    let hub = tempfile::tempdir().unwrap();
    let blobs = hub.path().join("models--o--r/blobs");
    std::fs::create_dir_all(&blobs).unwrap();
    std::fs::write(blobs.join(format!("{}.incomplete", oid(1))), b"0123").unwrap();
    let (tx, rx) = oneshot::channel();
    let job = job(&[("a.gguf", 10, 1)]);
    let curl = curl(&base, None);
    let task = download(&curl, hub.path(), &job, |_, _| {}, rx);
    let cancel = async {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        tx.send(()).unwrap();
    };
    let (end, ()) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(task, cancel)
    })
    .await
    .expect("the cancel stops curl");
    assert_eq!(end, End::Cancelled);
    assert_eq!(
        std::fs::read(blobs.join(format!("{}.incomplete", oid(1)))).unwrap(),
        b"0123"
    );
    assert!(
        !hub.path()
            .join(format!("models--o--r/snapshots/{COMMIT}"))
            .exists()
    );
}

#[tokio::test]
async fn a_corrupt_download_is_removed_and_never_linked() {
    need_curl!();
    let (base, _) = file_server(b"0123456789");
    let hub = tempfile::tempdir().unwrap();
    let mut job = job(&[("a.gguf", 10, 1)]);
    job.choice.files[0].oid = sha(b"9876543210");
    let end = download(&curl(&base, None), hub.path(), &job, |_, _| {}, no_cancel()).await;
    assert_eq!(
        end,
        End::Failed("download failed: checksum mismatch".into())
    );
    let repo = hub.path().join("models--o--r");
    assert!(
        !repo
            .join(format!("blobs/{}.incomplete", sha(b"9876543210")))
            .exists()
    );
    assert!(!repo.join(format!("blobs/{}", sha(b"9876543210"))).exists());
    assert!(!repo.join("snapshots").exists());
}

#[tokio::test]
async fn a_size_mismatch_removes_the_partial_file() {
    need_curl!();
    let (base, _) = file_server(b"short");
    let hub = tempfile::tempdir().unwrap();
    let job = job(&[("a.gguf", 10, 1)]);
    let end = download(&curl(&base, None), hub.path(), &job, |_, _| {}, no_cancel()).await;
    assert_eq!(end, End::Failed("download failed: size mismatch".into()));
    assert!(
        !hub.path()
            .join(format!("models--o--r/blobs/{}.incomplete", oid(1)))
            .exists()
    );
}

#[tokio::test]
async fn a_failed_download_says_it_can_resume() {
    need_curl!();
    let (base, _) = serve(Box::new(|_| reply("500 Oops", b"")));
    let hub = tempfile::tempdir().unwrap();
    let job = job(&[("a.gguf", 10, 1)]);
    let End::Failed(message) =
        download(&curl(&base, None), hub.path(), &job, |_, _| {}, no_cancel()).await
    else {
        panic!("not a failure");
    };
    assert!(message.starts_with("download failed: "), "{message}");
    assert!(message.ends_with(" (run it again to resume)"), "{message}");
}

// A blob already in the cache with the right size is not fetched again: a
// split model's parts are linked under one commit.
#[tokio::test]
async fn present_blobs_are_reused_and_every_part_is_linked() {
    let hub = tempfile::tempdir().unwrap();
    let blobs = hub.path().join("models--o--r/blobs");
    std::fs::create_dir_all(&blobs).unwrap();
    std::fs::write(blobs.join(oid(1)), b"aaaa").unwrap();
    std::fs::write(blobs.join(oid(2)), b"bbb").unwrap();
    let job = job(&[
        ("m-00001-of-00002.gguf", 4, 1),
        ("m-00002-of-00002.gguf", 3, 2),
    ]);
    // A curl that cannot run proves nothing is fetched.
    let mut curl = curl("http://127.0.0.1:9", None);
    curl.program = "lazarobox-no-such-curl".into();
    let end = download(&curl, hub.path(), &job, |_, _| {}, no_cancel()).await;
    let snapshots = hub.path().join(format!("models--o--r/snapshots/{COMMIT}"));
    assert_eq!(end, End::Done(snapshots.join("m-00001-of-00002.gguf")));
    assert_eq!(
        std::fs::read(snapshots.join("m-00002-of-00002.gguf")).unwrap(),
        b"bbb"
    );
}

// Spec: No space.
#[test]
fn the_missing_bytes_must_fit_in_the_free_space() {
    let hub = tempfile::tempdir().unwrap();
    let blobs = hub.path().join("models--o--r/blobs");
    std::fs::create_dir_all(&blobs).unwrap();
    std::fs::write(blobs.join(format!("{}.incomplete", oid(1))), vec![0; 400]).unwrap();
    let job = job(&[("a.gguf", 1_000, 1), ("b.gguf", 1_000_000_000, 2)]);
    assert_eq!(missing_bytes(hub.path(), &job), 1_000_000_600);
    assert_eq!(
        space_check(1_000_000_000, Some(500_000_000)),
        Err("not enough disk space: 1.0 GB needed, 500 MB free".to_owned())
    );
    assert_eq!(space_check(10, Some(10)), Ok(()));
    assert_eq!(
        space_check(10, None),
        Ok(()),
        "an unknown free space never blocks"
    );
    assert!(free_space(hub.path()).is_some_and(|free| free > 0));
    // A folder that does not exist yet is measured on its nearest parent.
    assert!(free_space(&hub.path().join("not/yet")).is_some());
}

/// The token lookup of a test: only `$HF_HOME`, plus `HF_TOKEN` when given.
fn env_at(hf_home: &Path, hf_token: Option<&str>) -> TokenEnv {
    TokenEnv {
        hf_token: hf_token.map(str::to_owned),
        hf_token_path: None,
        hf_home: Some(hf_home.to_owned()),
        xdg_cache_home: None,
        home: None,
    }
}

/// A worker whose requests go to the local server at `base`, with the token
/// file under `hf_home`.
fn account_worker(
    base: &str,
    hf_home: &Path,
    hf_token: Option<&'static str>,
) -> (Hub, tokio::sync::mpsc::Receiver<AppEvent>) {
    let (tx, rx) = tokio::sync::mpsc::channel(8);
    let (base, home) = (base.to_owned(), hf_home.to_owned());
    let hub = Hub::new(OsString::new(), None, move || env_at(&home, hf_token), tx)
        .with_curl(move |token| curl(&base, token.as_ref().map(Secret::expose)));
    (hub, rx)
}

async fn account_reply(
    hub: &mut Hub,
    ask: AccountAsk,
    rx: &mut mpsc::Receiver<AppEvent>,
) -> AccountReply {
    hub.account(ask);
    match tokio::time::timeout(Duration::from_secs(20), rx.recv()).await {
        Ok(Some(AppEvent::HfAccount(reply))) => reply,
        other => panic!("no account reply: {other:?}"),
    }
}

fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

// Esc in the token field stops a connect: its check is dropped and nothing
// is saved.
#[tokio::test]
async fn a_cancelled_connect_saves_nothing() {
    need_curl!();
    let (base, _) = serve(Box::new(|_| {
        thread::sleep(Duration::from_secs(5));
        ok(r#"{"type":"user","name":"ana"}"#)
    }));
    let dir = tempfile::tempdir().unwrap();
    let (mut hub, mut rx) = account_worker(&base, dir.path(), None);
    hub.account(AccountAsk::Connect(Secret::new("hf_abc".into())));
    tokio::time::sleep(Duration::from_millis(300)).await;
    let reply = account_reply(&mut hub, AccountAsk::CancelConnect, &mut rx).await;
    assert_eq!(
        reply,
        AccountReply::Connected(Err("Hugging Face token not saved: cancelled".to_owned()))
    );
    assert!(!dir.path().join("token").exists());
}

// Spec (model-download, Connect a Hugging Face account): Connect.
#[tokio::test]
async fn a_connect_saves_the_accepted_token_0600_and_sends_it_only_on_stdin() {
    need_curl!();
    let (base, seen) = serve(Box::new(|_| ok(r#"{"type":"user","name":"ana"}"#)));
    let dir = tempfile::tempdir().unwrap();
    // A missing folder is made, with mode 0700.
    let hf_home = dir.path().join("cache/huggingface");
    let (mut hub, mut rx) = account_worker(&base, &hf_home, None);
    let reply = account_reply(
        &mut hub,
        AccountAsk::Connect(Secret::new("hf_abc".into())),
        &mut rx,
    )
    .await;
    assert_eq!(reply, AccountReply::Connected(Ok("ana".to_owned())));
    let file = hf_home.join("token");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hf_abc");
    assert_eq!(mode(&file), 0o600);
    assert_eq!(mode(&hf_home), 0o700);
    assert_eq!(mode(&dir.path().join("cache")), 0o700);
    // Only the token file is left: the temporary file was renamed.
    let names: Vec<_> = std::fs::read_dir(&hf_home)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["token"]);
    // The request carried it once, as curl's stdin config made it.
    let requests = seen.lock().unwrap().clone();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].starts_with("GET /api/whoami-v2 "),
        "{}",
        requests[0]
    );
    assert_eq!(header(&requests[0], "authorization"), Some("Bearer hf_abc"));
    // Spec (Token): never in argv.
    let args = curl(&base, Some("hf_abc")).api_args(hf::WHOAMI_PATH);
    assert!(!args.iter().any(|a| a.contains("hf_abc")), "{args:?}");
    assert!(args.windows(2).any(|w| w == ["--config", "-"]), "{args:?}");
    let production =
        Curl::hub("/usr/bin".into(), Some(Secret::new("hf_abc".into()))).api_args(hf::WHOAMI_PATH);
    assert_eq!(
        production.last().map(String::as_str),
        Some("https://huggingface.co/api/whoami-v2")
    );
    assert!(!production.iter().any(|a| a.contains("hf_abc")));
}

// Spec (Connect a Hugging Face account): Rejected token.
#[tokio::test]
async fn a_rejected_token_writes_nothing() {
    need_curl!();
    let (base, _) = serve(Box::new(|_| {
        reply(
            "401 Unauthorized",
            br#"{"error":"Invalid username or password."}"#,
        )
    }));
    let dir = tempfile::tempdir().unwrap();
    let hf_home = dir.path().join("hf");
    let (mut hub, mut rx) = account_worker(&base, &hf_home, None);
    let reply = account_reply(
        &mut hub,
        AccountAsk::Connect(Secret::new("hf_bad".into())),
        &mut rx,
    )
    .await;
    assert_eq!(
        reply,
        AccountReply::Connected(Err("Hugging Face did not accept the token".to_owned()))
    );
    assert!(!hf_home.exists(), "nothing is written");
}

#[tokio::test]
async fn a_connect_replaces_a_saved_token_and_writes_through_a_symlink() {
    need_curl!();
    use std::os::unix::fs::PermissionsExt;
    let (base, _) = serve(Box::new(|_| ok(r#"{"name":"ana"}"#)));
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real-token");
    std::fs::write(&real, "old").unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o644)).unwrap();
    let hf_home = dir.path().join("hf");
    std::fs::create_dir(&hf_home).unwrap();
    std::os::unix::fs::symlink(&real, hf_home.join("token")).unwrap();
    let (mut hub, mut rx) = account_worker(&base, &hf_home, None);
    let reply = account_reply(
        &mut hub,
        AccountAsk::Connect(Secret::new("hf_new".into())),
        &mut rx,
    )
    .await;
    assert_eq!(reply, AccountReply::Connected(Ok("ana".to_owned())));
    assert!(
        std::fs::symlink_metadata(hf_home.join("token"))
            .unwrap()
            .is_symlink()
    );
    assert_eq!(std::fs::read_to_string(&real).unwrap(), "hf_new");
    assert_eq!(mode(&real), 0o600);
}

// Spec (Connect a Hugging Face account): Disconnect.
#[tokio::test]
async fn a_disconnect_deletes_the_token_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("token");
    std::fs::write(&file, "hf_abc").unwrap();
    let (mut hub, mut rx) = account_worker("http://127.0.0.1:9", dir.path(), None);
    let reply = account_reply(&mut hub, AccountAsk::Disconnect, &mut rx).await;
    assert_eq!(reply, AccountReply::Disconnected(Ok(())));
    assert!(!file.exists());
    // A file already gone is no failure.
    let again = account_reply(&mut hub, AccountAsk::Disconnect, &mut rx).await;
    assert_eq!(again, AccountReply::Disconnected(Ok(())));
}

// Spec (config-menu, Hugging Face account row): the check made when the row
// is opened asks with the saved token, not `HF_TOKEN`.
#[tokio::test]
async fn a_check_asks_who_the_saved_token_belongs_to() {
    need_curl!();
    let (base, seen) = serve(Box::new(|_| ok(r#"{"name":"ana"}"#)));
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("token"), "hf_saved\n").unwrap();
    let (mut hub, mut rx) = account_worker(&base, dir.path(), Some("hf_env"));
    let reply = account_reply(&mut hub, AccountAsk::Check, &mut rx).await;
    assert_eq!(reply, AccountReply::Checked(Ok("ana".to_owned())));
    let request = seen.lock().unwrap()[0].clone();
    assert_eq!(header(&request, "authorization"), Some("Bearer hf_saved"));
}

#[test]
fn the_facts_say_whether_hf_token_is_set_and_a_token_is_saved() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(facts(&env_at(dir.path(), None)), TokenFacts::default());
    assert_eq!(
        facts(&env_at(dir.path(), Some(" "))),
        TokenFacts::default(),
        "a blank HF_TOKEN is none"
    );
    std::fs::write(dir.path().join("token"), "hf_abc").unwrap();
    assert_eq!(
        facts(&env_at(dir.path(), Some("t0"))),
        TokenFacts {
            env: true,
            saved: true
        }
    );
}

// A relative `HF_TOKEN_PATH` is read from the working directory, as
// `huggingface_hub` reads it; the other paths stay as they are.
#[test]
fn a_relative_token_path_resolves_against_the_working_directory() {
    let vars = |name: &str| match name {
        "HF_TOKEN_PATH" => Some(OsString::from("tok/en")),
        "HF_HOME" => Some(OsString::from("/hf")),
        _ => None,
    };
    let env = token_env_from(vars, Path::new("/work"));
    assert_eq!(env.hf_token_path, Some(PathBuf::from("/work/tok/en")));
    assert_eq!(env.hf_home, Some(PathBuf::from("/hf")));
    assert_eq!(env.hf_token, None);
    let absolute = token_env_from(
        |name: &str| (name == "HF_TOKEN_PATH").then(|| OsString::from("/t")),
        Path::new("/work"),
    );
    assert_eq!(absolute.hf_token_path, Some(PathBuf::from("/t")));
}

// Spec (llama-server-install, Release resolution): GitHub only over HTTPS,
// with the URL built by the caller and no token.
#[test]
fn the_github_requests_are_https_only_and_end_with_the_url() {
    let api = Curl::github_api("/usr/bin".into())
        .api_args("/repos/ggml-org/llama.cpp/releases?per_page=10");
    assert_eq!(api[0], "-q");
    for flag in [["--proto", "=https"], ["--proto-redir", "=https"]] {
        assert!(api.windows(2).any(|w| w == flag), "{api:?}");
    }
    assert_eq!(
        api[api.len() - 2..],
        [
            "--",
            "https://api.github.com/repos/ggml-org/llama.cpp/releases?per_page=10"
        ]
    );
    let github = Curl::github("/usr/bin".into());
    let download = github.download_args(
        "/ggml-org/llama.cpp/releases/download/b9000/a.tar.gz",
        Path::new("/x/a.tar.gz"),
    );
    assert_eq!(
        download[download.len() - 2..],
        [
            "--",
            "https://github.com/ggml-org/llama.cpp/releases/download/b9000/a.tar.gz"
        ]
    );
    assert!(
        download.windows(2).any(|w| w == ["--continue-at", "-"]),
        "{download:?}"
    );
    // The GitHub download is bounded: its size, its redirects, a stall.
    let bounded = github.download_args_within(
        "/ggml-org/llama.cpp/releases/download/b9000/a.tar.gz",
        Path::new("/x/a.tar.gz"),
        Some(1234),
    );
    for flag in [
        ["--max-filesize", "1234"],
        ["--max-redirs", "5"],
        ["--speed-limit", "1"],
        ["--speed-time", "60"],
    ] {
        assert!(bounded.windows(2).any(|w| w == flag), "{bounded:?}");
    }
    // The Hub download keeps its own arguments.
    assert!(
        !download
            .iter()
            .any(|a| a.starts_with("--max-") || a.starts_with("--speed-"))
    );
    // No GitHub token is ever read or sent.
    for args in [api, download] {
        assert!(!args.iter().any(|a| a == "--config"), "{args:?}");
    }
    assert_eq!(github.stdin_config(), None);
}

// Spec (llama-server-install, Errors): tool-missing reads with the service.
#[tokio::test]
async fn a_github_request_without_curl_names_github() {
    let missing = Curl::github_api(OsString::new());
    assert_eq!(
        missing.get("/x").await,
        Err("GitHub needs curl on the PATH".to_owned())
    );
    // The Hub keeps its own words.
    assert_eq!(NO_CURL, "Hugging Face needs curl on the PATH");
}

#[tokio::test]
async fn a_failed_github_request_carries_the_github_label() {
    need_curl!();
    let path = std::env::var_os("PATH").unwrap_or_default();
    // Nothing listens on port 1.
    let refused = Curl::github_api(path.clone()).local("http://127.0.0.1:1");
    let message = refused.get("/x").await.unwrap_err();
    assert!(message.starts_with("GitHub request failed: "), "{message}");
    // A reply that is not text.
    let (base, _) = serve(Box::new(|_| reply("200 OK", &[0xff, 0xfe])));
    let unreadable = Curl::github_api(path).local(&base).get("/x").await;
    assert_eq!(
        unreadable,
        Err("GitHub request failed: unreadable reply".to_owned())
    );
    // The Hub label is untouched.
    let (base, _) = serve(Box::new(|_| reply("200 OK", &[0xff, 0xfe])));
    assert_eq!(
        curl(&base, None).get("/x").await,
        Err(hf::UNREADABLE.to_owned())
    );
}
