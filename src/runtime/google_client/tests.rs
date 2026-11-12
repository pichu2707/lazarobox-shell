//! The fetch, the cache and the fallback of the lazarobox client, against a
//! local server and a temp config folder (never the user's own).

use std::{
    ffi::OsString,
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::PathBuf,
};

use super::*;
use crate::core::ai::mcp_secrets::{CACHE_FILE, client_json};
use crate::runtime::hf::tests::{curl_available, ok, reply, serve};

const PIN: &str = "123456789012";
const ID: &str = "123456789012-abc.apps.googleusercontent.com";
const SECRET: &str = "GOCSPX-fetched";

macro_rules! need_curl {
    () => {
        if !curl_available() {
            eprintln!("SKIPPED: curl is not installed; the curl-backed checks did not run");
            return;
        }
    };
}

struct Lab {
    _root: tempfile::TempDir,
    dir: PathBuf,
}

fn lab() -> Lab {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("lazarobox");
    fs::create_dir(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    Lab { _root: root, dir }
}

impl Lab {
    fn secrets(&self) -> PathBuf {
        self.dir.join("secrets.toml")
    }

    fn cache(&self) -> PathBuf {
        self.dir.join(CACHE_FILE)
    }

    fn put_cache(&self, id: &str, mode: u32) {
        fs::write(self.cache(), client_json(id, "GOCSPX-saved").unwrap()).unwrap();
        fs::set_permissions(self.cache(), fs::Permissions::from_mode(mode)).unwrap();
    }

    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    async fn refresh(&self, curl: &Curl, pin: Option<&str>) -> Refresh {
        refresh(curl, pin, Some(&self.secrets())).await
    }
}

fn path() -> OsString {
    std::env::var_os("PATH").unwrap_or_default()
}

fn client_body(id: &str) -> String {
    serde_json::json!({"installed": {
        "client_id": id,
        "client_secret": SECRET,
        "token_uri": "https://evil.example/token",
    }})
    .to_string()
}

fn notice(refresh: Refresh) -> (&'static str, String) {
    match refresh {
        Refresh::Fresh => ("fresh", String::new()),
        Refresh::Stale(notice) => ("stale", notice),
        Refresh::Failed(notice) => ("failed", notice),
    }
}

// Spec (Fetch and cache): a 200 with a pinned client is cached 0600, rebuilt.
#[tokio::test]
async fn a_pinned_client_is_cached_private_and_rebuilt() {
    need_curl!();
    let lab = lab();
    let body = client_body(ID);
    let (base, seen) = serve(Box::new(move |_| ok(&body)));
    let curl = Curl::google_client(path()).local(&base);
    let done = lab.refresh(&curl, Some(PIN)).await;
    assert_eq!(notice(done).0, "fresh");
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(
        fs::read_to_string(lab.cache()).unwrap(),
        client_json(ID, SECRET).unwrap()
    );
    assert_eq!(fs::metadata(lab.cache()).unwrap().mode() & 0o7777, 0o600);
    assert_eq!(lab.names(), [CACHE_FILE]);
}

// Spec (Fallback): every refusal keeps the cache and says why.
#[tokio::test]
async fn a_refused_answer_keeps_a_valid_cache_and_says_why() {
    need_curl!();
    let answers: Vec<(&str, Vec<u8>)> = vec![
        (
            "301",
            b"HTTP/1.1 301 Moved\r\nLocation: http://127.0.0.1:1/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_vec(),
        ),
        ("404", reply("404 Not Found", b"")),
        (
            "foreign",
            ok(&client_body("999999999999-abc.apps.googleusercontent.com")),
        ),
        ("bad json", ok("{not json")),
        ("oversize", reply("200 OK", &vec![b' '; 20 * 1024])),
    ];
    for (name, answer) in answers {
        let lab = lab();
        lab.put_cache(ID, 0o600);
        let before = fs::read(lab.cache()).unwrap();
        let (base, _) = serve(Box::new(move |_| answer.clone()));
        let curl = Curl::google_client(path()).local(&base);
        let (kind, text) = notice(lab.refresh(&curl, Some(PIN)).await);
        assert_eq!(kind, "stale", "{name}: {text}");
        assert!(
            text.starts_with("lazarobox client not refreshed (")
                && text.ends_with("); linking with the saved copy"),
            "{name}: {text}"
        );
        assert_eq!(fs::read(lab.cache()).unwrap(), before, "{name}");
        assert_eq!(lab.names(), [CACHE_FILE], "{name}");
    }
}

#[tokio::test]
async fn a_refused_answer_without_a_cache_fails_and_writes_nothing() {
    need_curl!();
    let (base, _) = serve(Box::new(|_| {
        ok(&client_body("999999999999-a.apps.googleusercontent.com"))
    }));
    let lab = lab();
    let curl = Curl::google_client(path()).local(&base);
    let (kind, text) = notice(lab.refresh(&curl, Some(PIN)).await);
    assert_eq!(kind, "failed");
    assert_eq!(
        text,
        "lazarobox client refused (not from the lazarobox project); nothing saved; u uses your own client"
    );
    assert_eq!(lab.names(), Vec::<String>::new());

    let (base, _) = serve(Box::new(|_| reply("404 Not Found", b"")));
    let curl = Curl::google_client(path()).local(&base);
    let (kind, text) = notice(lab.refresh(&curl, Some(PIN)).await);
    assert_eq!(kind, "failed");
    assert_eq!(
        text,
        "lazarobox client not fetched (HTTP 404); link again later, or u uses your own client"
    );
    assert_eq!(lab.names(), Vec::<String>::new());
}

// Spec (Failure notices): unreachable host, curl missing.
#[tokio::test]
async fn an_unreachable_host_and_a_missing_curl_have_their_notice() {
    need_curl!();
    let lab = lab();
    // Nothing listens on port 1.
    let curl = Curl::google_client(path()).local("http://127.0.0.1:1");
    let (kind, text) = notice(lab.refresh(&curl, Some(PIN)).await);
    assert_eq!(kind, "failed");
    assert!(
        text.starts_with("lazarobox client not fetched (javilazaro.es request failed: "),
        "{text}"
    );
    // An empty PATH starts no process at all.
    let curl = Curl::google_client(OsString::new());
    let (kind, text) = notice(lab.refresh(&curl, Some(PIN)).await);
    assert_eq!(kind, "failed");
    assert_eq!(
        text,
        "lazarobox client not fetched (javilazaro.es needs curl on the PATH); link again later, or u uses your own client"
    );
    assert_eq!(lab.names(), Vec::<String>::new());
}

// Owner decision: no pin, no request.
#[tokio::test]
async fn without_a_pin_no_curl_runs_and_the_notice_says_so() {
    let lab = lab();
    lab.put_cache(ID, 0o600);
    let (base, seen) = serve(Box::new(|_| ok("{}")));
    let curl = Curl::google_client(path()).local(&base);
    let (kind, text) = notice(lab.refresh(&curl, None).await);
    assert_eq!(kind, "failed");
    assert_eq!(
        text,
        "lazarobox client not configured in this build; u uses your own client"
    );
    assert!(seen.lock().unwrap().is_empty(), "no request was made");
}

// Spec (Cache file): a cache that fails the handle checks is never the fallback.
#[tokio::test]
async fn a_loose_or_symlinked_cache_is_not_used_as_the_fallback() {
    need_curl!();
    let (base, _) = serve(Box::new(|_| reply("500 Error", b"")));
    let curl = Curl::google_client(path()).local(&base);

    let lab1 = lab();
    lab1.put_cache(ID, 0o644);
    let (kind, text) = notice(lab1.refresh(&curl, Some(PIN)).await);
    assert_eq!(kind, "failed");
    assert!(text.contains("HTTP 500"), "{text}");
    assert!(text.contains("mode must be 0600"), "{text}");

    let lab2 = lab();
    symlink(lab2.dir.join("elsewhere"), lab2.cache()).unwrap();
    let (kind, text) = notice(lab2.refresh(&curl, Some(PIN)).await);
    assert_eq!(kind, "failed");
    assert!(text.contains("is a symlink"), "{text}");
}

// Spec (Cache file): a refused write is a visible failure, never the secret.
#[tokio::test]
async fn a_cache_that_cannot_be_written_fails_with_the_save_notice() {
    need_curl!();
    let lab = lab();
    lab.put_cache(ID, 0o644);
    let body = client_body(ID);
    let (base, _) = serve(Box::new(move |_| ok(&body)));
    let curl = Curl::google_client(path()).local(&base);
    let (kind, text) = notice(lab.refresh(&curl, Some(PIN)).await);
    assert_eq!(kind, "failed");
    assert!(
        text.starts_with(
            "lazarobox client not saved: lazarobox-google-client.json: mode must be 0600"
        ),
        "{text}"
    );
    assert!(!text.contains(SECRET));
}
