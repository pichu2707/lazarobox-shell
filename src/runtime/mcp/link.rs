//! The checks behind Link with Google for `gsc` (S6b1): the client file and
//! `uv`, run by lazarobox as direct argv (no shell) with a 5-second limit,
//! then the login line built from the resolved values. The line is only
//! returned: the menu shows it at the `y`, and the user's shell runs it.
//! Nothing here spawns the server or opens a token file.

use std::{
    ffi::OsStr,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::mpsc;

use crate::{
    app::AppEvent,
    core::ai::{
        mcp_catalog::gsc::{self, Plan},
        mcp_secrets::cache_path,
    },
    runtime::{
        google_client::{Refresh, refresh},
        hf::Curl,
    },
};

/// Why one `uv` call gave no output.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fail {
    NotFound,
    Other(&'static str),
}

pub const NO_CLIENT: &str = "add your Desktop OAuth client first (o)";
const LIMIT: Duration = Duration::from_secs(5);
/// The most output read from one call.
const OUT_MAX: u64 = 64 * 1024;

/// Runs `program` with `args`, no shell, and gives its standard output.
pub fn run_with(program: &OsStr, args: &[&str], limit: Duration) -> Result<String, Fail> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| match e.kind() {
            ErrorKind::NotFound => Fail::NotFound,
            _ => Fail::Other("uv failed"),
        })?;
    let end = Instant::now() + limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Fail::Other("uv did not answer in 5 s"));
            }
        }
    };
    let mut out = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        let _ = stdout.take(OUT_MAX).read_to_end(&mut out);
    }
    match status.success() {
        true => Ok(String::from_utf8_lossy(&out).into_owned()),
        false => Err(Fail::Other("uv failed")),
    }
}

/// The real runner: `uv` from `PATH`, five seconds at most.
pub fn uv(args: &[&str]) -> Result<String, Fail> {
    run_with(OsStr::new("uv"), args, LIMIT)
}

/// The line to show, or the notice of the first check that fails.
pub fn check(
    plan: &Plan,
    run: &impl Fn(&[&str]) -> Result<String, Fail>,
    exists: impl Fn(&Path) -> bool,
    opener: Option<&str>,
) -> Result<String, String> {
    if !exists(Path::new(&plan.file)) {
        return Err(NO_CLIENT.to_owned());
    }
    let said = |fail: Fail| match fail {
        Fail::NotFound => gsc::NO_UV.to_owned(),
        Fail::Other(why) => why.to_owned(),
    };
    gsc::pinned(&run(&["tool", "list"]).map_err(said)?)?;
    let dir = run(&["tool", "dir"]).map_err(said)?;
    let dir = gsc::tool_dir(&dir).ok_or_else(|| gsc::NO_TOOL_DIR.to_owned())?;
    gsc::login_line(&dir, plan, opener).map_err(str::to_owned)
}

/// What Link needs to fetch the lazarobox client: curl, where `secrets.toml`
/// lives (the cache sits next to it) and the pin.
pub struct Fetch {
    curl: Curl,
    secrets: Option<PathBuf>,
    pin: Option<String>,
}

impl Fetch {
    pub fn new(curl: Curl, secrets: Option<PathBuf>, pin: Option<String>) -> Self {
        Self { curl, secrets, pin }
    }
}

/// The answer to one Link: the line or the notice that stops it, and the
/// fallback notice when the saved copy was used. The client is fetched, by
/// value, only when the plan holds the cache path; any other path is checked
/// as it always was, with no request.
pub async fn answer(
    plan: Plan,
    fetch: &Fetch,
    check: impl FnOnce(Plan) -> Result<String, String> + Send + 'static,
) -> (Result<String, String>, Option<String>) {
    let mut notice = None;
    if cache_path(fetch.secrets.as_deref()).as_deref() == Some(plan.file.as_str()) {
        match refresh(&fetch.curl, fetch.pin.as_deref(), fetch.secrets.as_deref()).await {
            Refresh::Fresh => {}
            Refresh::Stale(said) => notice = Some(said),
            Refresh::Failed(said) => return (Err(said), None),
        }
    }
    let line = tokio::task::spawn_blocking(move || check(plan))
        .await
        .unwrap_or_else(|_| Err("the check failed".to_owned()));
    (line, notice)
}

/// Answers each `Effect::LinkGsc` with one `AppEvent::GscLine`.
pub struct Linker {
    events: mpsc::Sender<AppEvent>,
    /// The name of the program lazarobox opens links with, handed to the
    /// login line as `BROWSER`; `None` when the host has none.
    opener: Option<String>,
    fetch: Arc<Fetch>,
}

impl Linker {
    pub fn new(events: mpsc::Sender<AppEvent>, opener: Option<String>, fetch: Fetch) -> Self {
        Self {
            events,
            opener,
            fetch: Arc::new(fetch),
        }
    }

    /// Fetches the client when the plan asks for it, runs the checks on a
    /// blocking thread and sends the answer. A stat of the client file, then
    /// `uv`: nothing is spawned but `uv`, and `curl` for the cache path.
    pub fn check(&self, plan: Plan) {
        let (events, opener, fetch) = (
            self.events.clone(),
            self.opener.clone(),
            Arc::clone(&self.fetch),
        );
        tokio::spawn(async move {
            let run = move |plan: Plan| {
                check(
                    &plan,
                    &uv,
                    |file| file.metadata().is_ok(),
                    opener.as_deref(),
                )
            };
            let (line, notice) = answer(plan, &fetch, run).await;
            let _ = events.send(AppEvent::GscLine(line, notice)).await;
        });
    }
}

#[cfg(test)]
mod fetch_tests;
#[cfg(test)]
mod tests;
