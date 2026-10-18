//! Starting and stopping the process behind an [`McpClient`].
//!
//! Trust model, in addition to the one of the parent module:
//!
//! - The command and its arguments are the user's own config. The program is
//!   executed directly, with one argv element per configured argument: no
//!   shell ever parses them, so `;` or `$(..)` in an argument is just text.
//! - The server is trusted by the user but NOT jailed (design D8). What it
//!   gets is minimal on purpose: the environment is cleared, then a small base
//!   ([`BASE_ENV`]), then ONLY the variables the config names. The session
//!   token ([`TOKEN_ENV`]) is never passed, even if the config names it, and
//!   neither is anything else the user did not list.
//! - A config entry must not be able to inject code into the server's dynamic
//!   loader: `LD_*` and `DYLD_*` names are silently dropped, whatever the
//!   config says. `HOME` is the resolved home and is not overridable either.
//! - A relative `command` is looked up on the CHILD's scrubbed `PATH`, and one
//!   containing a `/` resolves against `home`, because the working directory
//!   is applied first.
//! - The group kill does not reach a descendant that daemonizes (`setsid` or
//!   `setpgid`): it leaves the group and outlives the server. That is the
//!   limit of a process-group guard, and the server is trusted not to do it.
//! - It starts in `$HOME`, not in the repository, so a project cannot make a
//!   server read it by accident, and in its own process group so that stopping
//!   it takes its grandchildren too.
//! - Nothing here is called on config load or repository open: a caller starts
//!   a server on demand only.

use std::{ffi::OsString, path::Path, process::Stdio, time::Duration};

use tokio::{process::Command, task::JoinHandle, time::timeout};

use super::{McpClient, Timeouts};
use crate::{
    core::ai::{config::McpServer, sanitize::clean},
    runtime::{
        ai_detect::{finish_stderr, read_tail, stderr_note},
        mcp::shim::TOKEN_ENV,
        proc::GroupKill,
    },
};

/// The variables every server gets from the host, when the host has them.
/// `HOME` is not looked up: it is the directory the server starts in.
const BASE_ENV: [&str; 5] = ["PATH", "USER", "LANG", "LC_ALL", "TMPDIR"];

/// How long a failed start waits for the tail of stderr: longer than the
/// probes' grace so a slow runner still reports why, and still bounded.
const START_GRACE: Duration = Duration::from_secs(1);

/// How long stopping a server may take.
const SHUTDOWN: Duration = Duration::from_secs(3);

/// The OS side of a started server. `group` is declared before `child` so it
/// drops first: a dropped client still kills the whole group while the leader
/// has not been reaped, and `kill_on_drop` covers a refused group.
pub(super) struct Process {
    group: Option<GroupKill>,
    child: tokio::process::Child,
    /// Keeps the last of stderr, so a full pipe never blocks the server and a
    /// failure can say why.
    stderr: Option<JoinHandle<Vec<u8>>>,
}

/// Whether `name` would load code into the server's dynamic loader.
fn is_loader_var(name: &str) -> bool {
    name.starts_with("LD_") || name.starts_with("DYLD_")
}

/// The environment of a server: the base, then the names in `named` that the
/// host has, then `HOME`. [`TOKEN_ENV`], `HOME` and loader variables named in
/// the config are never taken from it.
fn child_env(
    named: &[String],
    home: &Path,
    lookup: impl Fn(&str) -> Option<OsString>,
) -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> = BASE_ENV
        .iter()
        .copied()
        .chain(
            named
                .iter()
                .map(String::as_str)
                .filter(|name| *name != TOKEN_ENV && *name != "HOME" && !is_loader_var(name)),
        )
        .filter_map(|name| Some((OsString::from(name), lookup(name)?)))
        .collect();
    env.push((OsString::from("HOME"), home.as_os_str().to_owned()));
    env
}

impl Drop for Process {
    fn drop(&mut self) {
        // Fields drop after this, `group` first. A descendant that left the
        // group may still hold the pipe: the reader must not wait for it.
        if let Some(task) = &self.stderr {
            task.abort();
        }
    }
}

impl McpClient {
    /// Starts the server and connects to its pipes; the handshake is not done.
    /// The error is the one reason, already safe to show. Must be called
    /// inside a tokio runtime (it spawns the stderr reader).
    pub fn spawn(
        server: &McpServer,
        home: &Path,
        env_lookup: impl Fn(&str) -> Option<OsString>,
    ) -> Result<Self, String> {
        let mut command = Command::new(&server.command);
        command
            .args(&server.args)
            .env_clear()
            .envs(child_env(&server.env, home, env_lookup))
            .current_dir(home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .process_group(0);
        let fail = || format!("could not start {}", clean(&server.name));
        let mut child = command.spawn().map_err(|_| fail())?;
        let group = child
            .id()
            .and_then(|pid| GroupKill::new(pid.try_into().ok()?));
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(fail());
        };
        let mut client = McpClient::over(stdout, stdin, Timeouts::default());
        client.process = Some(Process {
            group,
            child,
            stderr: Some(tokio::spawn(read_tail(stderr))),
        });
        Ok(client)
    }

    /// [`McpClient::spawn`] and the handshake. A server that cannot be started,
    /// dies or does not answer is ONE reason: what went wrong, and the last
    /// line it said on stderr (sanitized and capped). It is stopped then. Must
    /// be called inside a tokio runtime.
    pub async fn start(
        server: &McpServer,
        home: &Path,
        env_lookup: impl Fn(&str) -> Option<OsString>,
        timeouts: Timeouts,
    ) -> Result<Self, String> {
        let mut client = Self::spawn(server, home, env_lookup)?;
        client.timeouts = timeouts;
        let error = match client.handshake().await {
            Ok(()) => return Ok(client),
            Err(error) => error,
        };
        let stderr = client.process.as_mut().and_then(|p| p.stderr.take());
        client.shutdown().await;
        let said = match stderr {
            Some(task) => finish_stderr(task, START_GRACE).await,
            None => None,
        };
        Err(match said.and_then(|bytes| stderr_note(&bytes)) {
            Some(note) => format!("{}: {note}", error.reason()),
            None => error.reason(),
        })
    }

    /// Kills the server's whole process group and reaps it, in at most 3 s.
    /// Nothing is left running, grandchildren included.
    pub async fn shutdown(mut self) {
        let Some(mut process) = self.process.take() else {
            return;
        };
        let Process {
            group,
            child,
            stderr,
        } = &mut process;
        let stop = async {
            match group {
                Some(group) => {
                    let _ = group.kill_and_reap(child).await;
                }
                None => {
                    let _ = child.kill().await;
                }
            }
        };
        // On a timeout the armed guard kills again as it drops.
        let _ = timeout(SHUTDOWN, stop).await;
        if let Some(task) = stderr {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests;
