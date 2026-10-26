use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use super::*;
use crate::core::ai::hf::{Choice, HubFile, Job, License};

const COMMIT: &str = "9217f5db79a29953eb74d5343926648285ec7e67";

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

type Handler = dyn Fn(&str) -> Vec<u8> + Send + Sync;

/// A local HTTP server answering every request with `handler(request)`,
/// recording each request's head.
fn serve(handler: Box<Handler>) -> (String, Arc<Mutex<Vec<String>>>) {
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

fn reply(status: &str, body: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

fn ok(body: &str) -> Vec<u8> {
    reply("200 OK", body.as_bytes())
}

/// A test curl: plain HTTP to the local server, never through a proxy.
fn curl(base: &str, token: Option<&str>) -> Curl {
    Curl {
        program: "curl".into(),
        path: std::env::var_os("PATH").unwrap_or_default(),
        base: base.to_owned(),
        https_only: false,
        noproxy: true,
        token: token.map(str::to_owned),
        user_agent: "lazarobox-shell/9.9.9".to_owned(),
    }
}

/// The sha256 of `bytes` in hex: the object id the Hub gives an LFS file.
fn sha(bytes: &[u8]) -> String {
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

fn header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    request.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

// Spec (model-download, Hub requests): Only HTTPS to the Hub.
#[test]
fn the_hub_requests_are_https_only_and_end_with_the_url() {
    let hub = Curl::hub("/usr/bin".into(), Some("secret-123".into()));
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
        hub.stdin_config().as_deref(),
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
            Curl::hub("/usr/bin".into(), Some(bad.into())).stdin_config(),
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
fn file_server(body: &'static [u8]) -> (String, Arc<Mutex<Vec<String>>>) {
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

fn no_cancel() -> oneshot::Receiver<()> {
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
