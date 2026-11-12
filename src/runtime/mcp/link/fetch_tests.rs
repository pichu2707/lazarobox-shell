//! Link fetches the lazarobox client by value (openspec change
//! `gsc-lazarobox-client`, spec: When the fetch may run, Fallback), against a
//! local server and a temp config folder.

use std::{
    ffi::OsString,
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use super::*;
use crate::core::ai::mcp_secrets::{cache_path, client_json};
use crate::runtime::hf::{
    Curl,
    tests::{curl_available, ok, reply, serve},
};

const PIN: &str = "123456789012";
const ID: &str = "123456789012-abc.apps.googleusercontent.com";

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
    secrets: PathBuf,
}

fn lab() -> Lab {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("lazarobox");
    fs::create_dir(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    Lab {
        secrets: dir.join("secrets.toml"),
        _root: root,
    }
}

impl Lab {
    fn plan(&self, file: &str) -> Plan {
        Plan {
            dir: "/h/.config/mcp-gsc".to_owned(),
            file: file.to_owned(),
        }
    }

    fn cache(&self) -> String {
        cache_path(Some(&self.secrets)).unwrap()
    }

    fn fetch(&self, curl: Curl) -> Fetch {
        Fetch::new(curl, Some(self.secrets.clone()), Some(PIN.to_owned()))
    }

    fn put_cache(&self) {
        let file = self.cache();
        fs::write(&file, client_json(ID, "GOCSPX-saved").unwrap()).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    }
}

fn body() -> String {
    serde_json::json!({"installed": {
        "client_id": ID,
        "client_secret": "GOCSPX-fetched",
    }})
    .to_string()
}

fn path() -> OsString {
    std::env::var_os("PATH").unwrap_or_default()
}

type Seen = Arc<Mutex<Vec<String>>>;
type Calls = Arc<Mutex<Vec<usize>>>;

/// A check that records how many requests the server had seen when it ran.
fn checking(
    seen: &Seen,
    calls: &Calls,
) -> impl FnOnce(Plan) -> Result<String, String> + Send + 'static {
    let (seen, calls) = (Arc::clone(seen), Arc::clone(calls));
    move |_| {
        calls.lock().unwrap().push(seen.lock().unwrap().len());
        Ok("LINE".to_owned())
    }
}

#[tokio::test]
async fn the_cache_path_fetches_on_every_link_before_any_check() {
    need_curl!();
    let lab = lab();
    let text = body();
    let (base, seen) = serve(Box::new(move |_| ok(&text)));
    let fetch = lab.fetch(Curl::google_client(path()).local(&base));
    let calls = Calls::default();
    for _ in 0..2 {
        let done = answer(lab.plan(&lab.cache()), &fetch, checking(&seen, &calls)).await;
        assert_eq!(done, (Ok("LINE".to_owned()), None));
    }
    assert_eq!(seen.lock().unwrap().len(), 2);
    assert_eq!(*calls.lock().unwrap(), [1, 2]);
    assert!(fs::metadata(lab.cache()).is_ok());
}

#[tokio::test]
async fn any_other_path_spawns_no_curl() {
    let lab = lab();
    // An empty PATH: a curl run would end in a notice.
    let fetch = lab.fetch(Curl::google_client(OsString::new()));
    let calls = Calls::default();
    let own = "/h/.config/lazarobox/google-oauth-client.json";
    let done = answer(lab.plan(own), &fetch, checking(&Seen::default(), &calls)).await;
    assert_eq!(done, (Ok("LINE".to_owned()), None));
    assert!(fs::metadata(lab.cache()).is_err());
}

#[tokio::test]
async fn a_failed_fetch_without_a_cache_stops_before_any_check() {
    need_curl!();
    let lab = lab();
    let (base, seen) = serve(Box::new(|_| reply("404 Not Found", b"")));
    let fetch = lab.fetch(Curl::google_client(path()).local(&base));
    let calls = Calls::default();
    let (line, notice) = answer(lab.plan(&lab.cache()), &fetch, checking(&seen, &calls)).await;
    assert_eq!(
        line,
        Err(
            "lazarobox client not fetched (HTTP 404); link again later, or u uses your own client"
                .to_owned()
        )
    );
    assert_eq!(notice, None);
    assert!(calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_failed_fetch_with_a_saved_copy_links_and_carries_the_notice() {
    need_curl!();
    let lab = lab();
    lab.put_cache();
    let (base, seen) = serve(Box::new(|_| reply("500 Oops", b"")));
    let fetch = lab.fetch(Curl::google_client(path()).local(&base));
    let calls = Calls::default();
    let (line, notice) = answer(lab.plan(&lab.cache()), &fetch, checking(&seen, &calls)).await;
    assert_eq!(line, Ok("LINE".to_owned()));
    assert_eq!(
        notice.as_deref(),
        Some("lazarobox client not refreshed (HTTP 500); linking with the saved copy")
    );
    assert_eq!(*calls.lock().unwrap(), [1]);
}

// Spec (Nothing else fetches): the Linker is the only caller of `refresh`.
#[test]
fn the_linker_is_the_only_caller_of_refresh() {
    let callers: Vec<&str> = [
        "src/runtime.rs",
        "src/app.rs",
        "src/runtime/mcp/presence.rs",
    ]
    .into_iter()
    .filter(|file| {
        let text = fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file))
            .unwrap();
        text.contains("google_client::refresh") || text.contains("refresh(")
    })
    .collect();
    assert_eq!(callers, Vec::<&str>::new());
}
