//! A llama-server that lazarobox starts itself (`[ai.servers.llamacpp]` with
//! `managed = true`).
//!
//! Trust model:
//!
//! - Nothing starts on config load or on detection. Only a chat turn whose
//!   engine is llamacpp starts it, at most one at a time.
//! - The program is `llama-server` from the `PATH`, run directly with one argv
//!   element per argument: `--host 127.0.0.1 --port <port> -m <model>`. No
//!   shell parses the model path.
//! - Before a start, the port must be silent. A server that already answers
//!   there was not started by us and is never treated as ours: the turn fails
//!   and nothing is started.
//! - It runs in its own process group, so stopping it takes its children too.
//!   Quitting lazarobox stops it ([`Managed::shutdown`]); dropping it kills the
//!   group as a backstop, so a cancelled start leaves nothing running.
//! - Its output goes to a log file in lazarobox's state folder, emptied on
//!   each start and never followed through a symlink. A failed start shows its
//!   last line, sanitized.

use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};

use futures::future::BoxFuture;
use tokio::{process::Command, sync::Mutex, time::sleep};

use crate::runtime::{
    ai_detect::{http_get, stderr_note},
    proc::GroupKill,
};

/// How long a start waits for the model to load and the server to answer.
const START_WAIT: Duration = Duration::from_secs(90);
/// How often a start asks whether the server answers.
const START_POLL: Duration = Duration::from_millis(250);
/// How long stopping the server may take.
const SHUTDOWN: Duration = Duration::from_secs(3);
/// How much of the log's end is read for the failure line.
const TAIL: usize = 4096;

/// The status of `GET /v1/models` on a port, `None` when nothing answers. A
/// seam so the start logic is tested without a real server.
pub type Probe = Arc<dyn Fn(u16) -> BoxFuture<'static, Option<u16>> + Send + Sync>;

/// The real probe: the detection's loopback GET.
pub fn models_probe() -> Probe {
    Arc::new(|port| Box::pin(async move { http_get(port, "/v1/models").await.map(|(s, _)| s) }))
}

/// The OS side of a started server. `group` is declared before `child` so it
/// drops first: a dropped server still kills the whole group while the leader
/// has not been reaped, and `kill_on_drop` covers a refused group.
struct Process {
    group: Option<GroupKill>,
    child: tokio::process::Child,
}

pub struct Managed {
    program: OsString,
    model: PathBuf,
    port: u16,
    log: Option<PathBuf>,
    probe: Probe,
    wait: Duration,
    running: Mutex<Option<Process>>,
}

impl Managed {
    /// The server lazarobox starts on `port` with `model`, writing to `log`.
    pub fn new(model: PathBuf, port: u16, log: Option<PathBuf>) -> Self {
        Self::with(
            OsString::from("llama-server"),
            model,
            port,
            log,
            models_probe(),
            START_WAIT,
        )
    }

    /// [`Managed::new`] with the program, the probe and the start wait given.
    pub(crate) fn with(
        program: OsString,
        model: PathBuf,
        port: u16,
        log: Option<PathBuf>,
        probe: Probe,
        wait: Duration,
    ) -> Self {
        Self {
            program,
            model,
            port,
            log,
            probe,
            wait,
            running: Mutex::new(None),
        }
    }

    /// Ok once the server we started answers; it is started first when it is
    /// not running. The error is the one reason, already safe to show.
    pub async fn ensure(&self) -> Result<(), String> {
        let mut running = self.running.lock().await;
        if let Some(process) = running.as_mut() {
            match process.child.try_wait() {
                // Ours and still running.
                Ok(None) => return Ok(()),
                // It ended (reaped now): its pid must not be signalled again.
                _ => {
                    if let Some(group) = process.group.as_mut() {
                        group.disarm();
                    }
                    *running = None;
                }
            }
        }
        if (self.probe)(self.port).await.is_some() {
            return Err(format!(
                "llama-server not started: another program answers on 127.0.0.1:{}",
                self.port
            ));
        }
        // Held here until it answers: a cancelled turn drops it, which kills it.
        let mut process = self.spawn()?;
        let deadline = Instant::now() + self.wait;
        loop {
            sleep(START_POLL).await;
            if let Ok(Some(_)) = process.child.try_wait() {
                if let Some(group) = process.group.as_mut() {
                    group.disarm();
                }
                return Err(self.failure("llama-server stopped"));
            }
            if (self.probe)(self.port).await == Some(200) {
                *running = Some(process);
                return Ok(());
            }
            if Instant::now() >= deadline {
                stop(process).await;
                return Err(self.failure(&format!(
                    "llama-server did not answer within {} s",
                    self.wait.as_secs()
                )));
            }
        }
    }

    fn spawn(&self) -> Result<Process, String> {
        let (stdout, stderr) = match self.log.as_deref().map(open_log) {
            Some(Ok(file)) => match file.try_clone() {
                Ok(copy) => (Stdio::from(file), Stdio::from(copy)),
                Err(_) => (Stdio::null(), Stdio::null()),
            },
            _ => (Stdio::null(), Stdio::null()),
        };
        let mut command = Command::new(&self.program);
        command
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(self.port.to_string())
            .arg("-m")
            .arg(&self.model)
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .kill_on_drop(true)
            .process_group(0);
        let child = command
            .spawn()
            .map_err(|_| "could not start llama-server: is it on the PATH?".to_owned())?;
        let group = child
            .id()
            .and_then(|pid| GroupKill::new(pid.try_into().ok()?));
        Ok(Process { group, child })
    }

    /// `what`, with the last line of the log when it has one.
    fn failure(&self, what: &str) -> String {
        match self
            .log
            .as_deref()
            .and_then(log_tail)
            .and_then(|t| stderr_note(&t))
        {
            Some(line) => format!("{what}: {line}"),
            None => what.to_owned(),
        }
    }

    /// Stops the server we started, whole group, in at most 3 s.
    pub async fn shutdown(&self) {
        if let Some(process) = self.running.lock().await.take() {
            stop(process).await;
        }
    }

    /// The pid of the running server, for tests.
    #[cfg(test)]
    pub(crate) async fn pid(&self) -> Option<u32> {
        self.running.lock().await.as_ref()?.child.id()
    }
}

/// Kills the group and reaps the leader; on a timeout the armed guard kills
/// again as it drops.
async fn stop(mut process: Process) {
    let Process { group, child } = &mut process;
    let kill = async {
        match group {
            Some(group) => {
                let _ = group.kill_and_reap(child).await;
            }
            None => {
                let _ = child.kill().await;
            }
        }
    };
    let _ = tokio::time::timeout(SHUTDOWN, kill).await;
}

/// The log, emptied, private to the user and never through a symlink.
fn open_log(path: &Path) -> io::Result<fs::File> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

/// The end of the log, read without following a symlink.
fn log_tail(path: &Path) -> Option<Vec<u8>> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    Some(bytes[bytes.len().saturating_sub(TAIL)..].to_vec())
}

#[cfg(test)]
mod tests;
