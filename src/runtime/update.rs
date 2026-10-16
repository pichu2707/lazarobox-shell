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
    use std::os::unix::fs::OpenOptionsExt;
    // Never follow a link, never wait on a FIFO or a device: only a plain
    // file is read.
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY)
        .open(path)
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    file.take(CACHE_CAP + 1).read_to_string(&mut text).ok()?;
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
    let temp = temp_path(path);
    // A stale file of a past process with this pid is replaced; a link
    // planted there is removed, never written through.
    let _ = fs::remove_file(&temp);
    let written = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
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
        due: update::is_due(cache.as_ref(), now, current),
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
    let cache = known.after(now, result, current);
    // A cache that cannot be written only costs a check on the next start.
    let _ = write_cache(path, &cache);
    update::announced(Some(&cache), current)
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
    /// Test requests must not go through the developer's proxy.
    noproxy: bool,
    /// Working directory of the child; `None` keeps ours. Tests only.
    cwd: Option<PathBuf>,
    /// Most reply bytes read before curl is killed.
    read_cap: usize,
    /// The `--max-filesize` given to curl.
    file_cap: usize,
}

/// The temporary file `write_cache` fills before the rename.
fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{}.tmp", std::process::id()));
    PathBuf::from(name)
}

impl CurlFetcher {
    /// The arguments of the request. The URL comes last, after `--`.
    pub fn args(&self) -> Vec<String> {
        let mut args: Vec<String> = [
            // Not the user's ~/.curlrc: it could add redirects, output files...
            "-q",
            "--silent",
            "--max-time",
            &TIMEOUT_SECS.to_string(),
            "--connect-timeout",
            &TIMEOUT_SECS.to_string(),
            "--max-filesize",
            &self.file_cap.to_string(),
            "--user-agent",
            &self.user_agent,
            "--header",
            "Accept: application/json",
            // The status code goes after the body, on its own line.
            "--write-out",
            "\n%{http_code}",
        ]
        .map(str::to_owned)
        .into();
        if self.https_only {
            args.extend(["--proto".to_owned(), "=https".to_owned()]);
        }
        if self.noproxy {
            args.extend(["--noproxy".to_owned(), "*".to_owned()]);
        }
        args.extend(["--".to_owned(), self.url.clone()]);
        args
    }

    /// The production request: crates.io over HTTPS only, with `path` as the
    /// `PATH` to find `curl` in.
    pub fn crates_io(path: OsString) -> Self {
        Self {
            program: "curl".into(),
            path,
            url: update::API_URL.to_owned(),
            user_agent: user_agent(env!("CARGO_PKG_VERSION")),
            https_only: true,
            noproxy: false,
            cwd: None,
            read_cap: REPLY_CAP,
            file_cap: REPLY_CAP,
        }
    }
}

impl Fetcher for CurlFetcher {
    fn fetch(&self) -> Result<String, Failed> {
        // An empty or relative entry makes the lookup search the current
        // directory, where a planted `curl` could be: refuse such a `PATH`.
        let safe =
            !self.path.is_empty() && std::env::split_paths(&self.path).all(|dir| dir.is_absolute());
        if !safe {
            return Err(Failed);
        }
        let mut command = Command::new(&self.program);
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        let mut child = command
            .args(self.args())
            .env("PATH", &self.path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| Failed)?;
        let mut out = Vec::new();
        // The status line is 4 bytes; a reply longer than the cap is cut off.
        let limit = self.read_cap as u64 + 8;
        let read = child
            .stdout
            .take()
            .ok_or(Failed)
            .and_then(|stdout| stdout.take(limit).read_to_end(&mut out).map_err(|_| Failed));
        if read.is_err() || out.len() as u64 >= limit {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Failed);
        }
        let status = child.wait().map_err(|_| Failed)?;
        if !status.success() {
            return Err(Failed);
        }
        let text = String::from_utf8(out).map_err(|_| Failed)?;
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

/// The start-up entry: what to show now, and, when a check is due, the
/// background request whose answer comes through `reply`. Disabled or without
/// a cache location it does nothing and shows nothing.
pub fn begin(
    enabled: bool,
    cache: Option<PathBuf>,
    now: u64,
    current: &'static str,
    fetcher: impl Fetcher,
    reply: mpsc::Sender<Option<String>>,
) -> Option<String> {
    let started = startup(enabled, cache.as_deref(), now, current);
    if let (true, Some(path)) = (started.due, cache) {
        // Stamp the clock before asking. It proves the cache can be written
        // (otherwise every start would ask again) and makes a quit within the
        // timeout leave a record; the answer overwrites it.
        let known = read_cache(&path).unwrap_or_default();
        if write_cache(&path, &known.after(now, Err(()), current)).is_ok() {
            spawn_refresh(path, now, current, fetcher, reply);
        }
    }
    started.announce
}

/// How this process was installed, read from the real machine: the running
/// binary, `CARGO_HOME`, `$HOME` and `cargo` on the login `path`.
pub fn install_env(path: &OsStr) -> update::InstallEnv {
    install_env_from(
        std::env::current_exe().ok(),
        std::env::var_os("CARGO_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
        path,
    )
}

/// [`install_env`] with the facts given. Paths that exist are canonicalized,
/// so a link to the binary or to the cargo home compares by where it points;
/// one that does not exist stays as given.
pub fn install_env_from(
    exe: Option<PathBuf>,
    cargo_home: Option<PathBuf>,
    home: Option<PathBuf>,
    path: &OsStr,
) -> update::InstallEnv {
    let real = |p: PathBuf| fs::canonicalize(&p).unwrap_or(p);
    update::InstallEnv {
        exe: exe.map(real),
        // A relative CARGO_HOME would mean "under whatever cwd we have".
        cargo_home: cargo_home.filter(|p| p.is_absolute()).map(real),
        home: home.map(real),
        cargo: crate::runtime::desktop::which("cargo", Some(path)),
        path: path.to_owned(),
    }
}

/// Now, in Unix seconds. A clock before 1970 reads as 0.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests;
