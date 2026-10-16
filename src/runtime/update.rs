//! The edge of the update notice: the cache file, the one HTTPS request and
//! the background thread. The rules live in `core::update`.
//!
//! The request goes through the system `curl`: the project's own HTTP client
//! is plain HTTP to loopback by design, and a TLS stack would be a large new
//! dependency for one GET a day. Every failure (no curl, no network, timeout,
//! non-200, bad reply) ends as "nothing to announce": silent, never a panic,
//! never on the UI thread.

use std::{
    ffi::{OsStr, OsString},
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
};

use tokio::sync::mpsc;

use crate::core::update::{self, Cache};

/// The whole request is given this long, connect included.
pub const TIMEOUT_SECS: u64 = 5;

/// Most bytes of reply kept. The crate document is about 100 KiB.
const REPLY_CAP: usize = 4 * 1024 * 1024;

/// Most bytes read of the cache file; it holds three short fields.
const CACHE_CAP: u64 = 4 * 1024;

/// One failed request. Why does not matter: the user is told nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Failed;

/// Asks for the crate document. The one seam tests replace.
pub trait Fetcher: Send + 'static {
    /// The reply body of a 200, else `Failed`.
    fn fetch(&self) -> Result<String, Failed>;
}

/// `$XDG_DATA_HOME/lazarobox/update-check.json`, else `$HOME/.local/share/...`:
/// the directory the chat store already lives in.
pub fn cache_path(xdg: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    let absolute = |v: Option<&OsStr>| {
        v.map(Path::new)
            .filter(|p| p.is_absolute())
            .map(Path::to_path_buf)
    };
    let base = absolute(xdg).or_else(|| absolute(home).map(|h| h.join(".local/share")))?;
    Some(base.join("lazarobox").join("update-check.json"))
}

/// The cache, or `None` when it is missing, too big or damaged.
pub fn read_cache(path: &Path) -> Option<Cache> {
    let mut text = String::new();
    fs::File::open(path)
        .ok()?
        .take(CACHE_CAP + 1)
        .read_to_string(&mut text)
        .ok()?;
    if text.len() as u64 > CACHE_CAP {
        return None;
    }
    Cache::parse(&text)
}

/// Writes the cache through a temporary file and a rename, mode 0600.
pub fn write_cache(path: &Path, cache: &Cache) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    let written = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temp)
        .and_then(|mut file| file.write_all(cache.to_json().as_bytes()))
        .and_then(|()| fs::rename(&temp, path));
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

/// What the start-up knows without the network.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Startup {
    /// The version to announce from the cache.
    pub announce: Option<String>,
    /// Whether a background check must run.
    pub due: bool,
}

/// The start-up decision. Disabled: nothing is shown and nothing is due. No
/// cache location: nothing is due either, since the schedule could not be kept.
pub fn startup(enabled: bool, path: Option<&Path>, now: u64, current: &str) -> Startup {
    let (true, Some(path)) = (enabled, path) else {
        return Startup {
            announce: None,
            due: false,
        };
    };
    let cache = read_cache(path);
    Startup {
        announce: update::announced(cache.as_ref(), current),
        due: update::is_due(cache.as_ref(), now),
    }
}

/// One check: asks, stores the outcome and returns what to announce. A failed
/// ask keeps the version already known and only moves the clock.
pub fn refresh(path: &Path, now: u64, current: &str, fetcher: &dyn Fetcher) -> Option<String> {
    let known = read_cache(path).unwrap_or_default();
    let result = fetcher
        .fetch()
        .ok()
        .and_then(|body| update::parse_response(&body))
        .ok_or(());
    let cache = known.after(now, result);
    // A cache that cannot be written only costs a check on the next start.
    let _ = write_cache(path, &cache);
    update::announced(Some(&cache), current)
}

/// The arguments of the request. The URL comes last, after `--`.
pub fn curl_args(url: &str, user_agent: &str, https_only: bool) -> Vec<String> {
    let mut args: Vec<String> = [
        // Not the user's ~/.curlrc: it could add redirects, output files...
        "-q",
        "--silent",
        "--max-time",
        &TIMEOUT_SECS.to_string(),
        "--connect-timeout",
        &TIMEOUT_SECS.to_string(),
        "--max-filesize",
        &REPLY_CAP.to_string(),
        "--user-agent",
        user_agent,
        "--header",
        "Accept: application/json",
        // The status code goes after the body, on its own line.
        "--write-out",
        "\n%{http_code}",
    ]
    .map(str::to_owned)
    .into();
    if https_only {
        args.extend(["--proto".to_owned(), "=https".to_owned()]);
    }
    args.extend(["--".to_owned(), url.to_owned()]);
    args
}

/// The descriptive `User-Agent` crates.io asks for.
pub fn user_agent(version: &str) -> String {
    format!("lazarobox-shell/{version} (+https://github.com/pichu2707/lazarobox-shell)")
}

/// Runs the system `curl`.
pub struct CurlFetcher {
    program: OsString,
    path: OsString,
    url: String,
    user_agent: String,
    https_only: bool,
}

impl CurlFetcher {
    /// The production request: crates.io over HTTPS only, with `path` as the
    /// `PATH` to find `curl` in.
    pub fn crates_io(path: OsString) -> Self {
        Self {
            program: "curl".into(),
            path,
            url: update::API_URL.to_owned(),
            user_agent: user_agent(env!("CARGO_PKG_VERSION")),
            https_only: true,
        }
    }
}

impl Fetcher for CurlFetcher {
    fn fetch(&self) -> Result<String, Failed> {
        let output = Command::new(&self.program)
            .args(curl_args(&self.url, &self.user_agent, self.https_only))
            .env("PATH", &self.path)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|_| Failed)?;
        if !output.status.success() || output.stdout.len() > REPLY_CAP + 16 {
            return Err(Failed);
        }
        let text = String::from_utf8(output.stdout).map_err(|_| Failed)?;
        match text.rsplit_once('\n') {
            Some((body, "200")) => Ok(body.to_owned()),
            _ => Err(Failed),
        }
    }
}

/// Runs the check on its own thread and sends what to announce. The UI never
/// waits for it; a closed channel (the app quit) is ignored.
pub fn spawn_refresh(
    path: PathBuf,
    now: u64,
    current: &'static str,
    fetcher: impl Fetcher,
    reply: mpsc::Sender<Option<String>>,
) {
    let job = move || {
        let announce = refresh(&path, now, current, &fetcher);
        let _ = reply.blocking_send(announce);
    };
    // Not `spawn_blocking`: the runtime would wait for it at quit.
    let _ = thread::Builder::new()
        .name("update-check".into())
        .spawn(job);
}

#[cfg(test)]
mod tests;
