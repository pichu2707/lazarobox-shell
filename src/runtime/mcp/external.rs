//! The user's MCP servers of one chat, started together: what the model may
//! call besides the built-in tools.
//!
//! Trust model, in addition to the one of [`super::client`]:
//!
//! - Nothing here decides whether a call may run. The host calls
//!   [`ExternalSet::call`] only after the user pressed `y` for that exact call
//!   (design D13), and wraps what comes back as untrusted data (D14).
//! - A call is routed by looking the qualified name up in the registry the
//!   model was offered (`Qualified::raw_of`), never by splitting the name. A
//!   name that is not in it reaches no server.
//! - A server that cannot start or list its tools is ONE notice and is left
//!   out; the others keep working. A server runs one request at a time.
//! - [`ExternalSet::shutdown`] stops every server and its process group.

use std::{
    ffi::OsString,
    future::Future,
    path::Path,
    sync::{Arc, OnceLock},
};

use futures::{StreamExt, future::join_all, stream};
use serde_json::Value;
use tokio::{sync::Mutex, time::timeout};

use super::{
    Outcome,
    client::{ClientError, McpClient, Timeouts},
};
use crate::core::ai::{
    config::McpServer,
    sanitize::clean,
    tools::{
        ExternalTool,
        servers::{Qualified, qualify},
    },
};

/// What the model is told when a name reaches no running server.
const NOT_AVAILABLE: &str = "mcp error: that tool is not available";

/// Most servers started at once. The config does not cap the count, and each
/// start spawns a process and waits out a handshake.
const START_AT_ONCE: usize = 8;

/// How long [`ExternalSet::shutdown`] waits for a call in flight to let go of
/// its server; after that the drop of the set kills it.
const STOP_WAIT: std::time::Duration = std::time::Duration::from_secs(4);

/// Where a turn's set is put once it is started. A turn opens its session (the
/// tool socket, the host's scope) before its servers are up, so the session
/// reads the set from here when a call or a `tools/list` arrives.
pub type Slot = Arc<OnceLock<Arc<ExternalSet>>>;

/// The running servers and the tools they offer.
pub struct ExternalSet {
    /// `None` once stopped.
    clients: Vec<(String, Mutex<Option<McpClient>>)>,
    qualified: Qualified,
    /// Why a server is missing, one line each.
    unavailable: Vec<String>,
}

/// Starts one server and reads its tools, or says why it cannot.
async fn connect(
    server: &McpServer,
    home: &Path,
    lookup: &impl Fn(&str) -> Option<OsString>,
    timeouts: Timeouts,
) -> Result<(McpClient, Vec<ExternalTool>), String> {
    let mut client = McpClient::start(server, home, lookup, timeouts).await?;
    match client.list_tools().await {
        Ok(tools) => Ok((client, tools)),
        Err(error) => {
            client.shutdown().await;
            Err(error.reason())
        }
    }
}

/// [`connect`] under the name the chat shows.
async fn start_one(
    server: &McpServer,
    home: &Path,
    lookup: &impl Fn(&str) -> Option<OsString>,
    timeouts: Timeouts,
) -> (String, Result<(McpClient, Vec<ExternalTool>), String>) {
    (
        clean(&server.name),
        connect(server, home, lookup, timeouts).await,
    )
}

impl ExternalSet {
    /// Starts every server at once. Must be called inside a tokio runtime.
    pub async fn start(
        servers: &[&McpServer],
        home: &Path,
        lookup: impl Fn(&str) -> Option<OsString>,
        timeouts: Timeouts,
    ) -> Self {
        // In order, at most `START_AT_ONCE` at a time.
        // The futures are built first: a `StreamExt::map` closure over borrows
        // makes the future fail the `Send` check of a spawned turn.
        let starts: Vec<_> = servers
            .iter()
            .map(|server| start_one(server, home, &lookup, timeouts))
            .collect();
        let started: Vec<_> = stream::iter(starts).buffered(START_AT_ONCE).collect().await;
        let mut clients = Vec::new();
        let mut listed = Vec::new();
        let mut unavailable = Vec::new();
        for (name, result) in started {
            match result {
                Ok((client, tools)) => {
                    listed.push((name.clone(), tools));
                    clients.push((name, Mutex::new(Some(client))));
                }
                Err(reason) => unavailable.push(format!("mcp: {name} unavailable: {reason}")),
            }
        }
        let pairs: Vec<(&str, &[_])> = listed
            .iter()
            .map(|(name, tools)| (name.as_str(), tools.as_slice()))
            .collect();
        Self {
            qualified: qualify(&pairs),
            clients,
            unavailable,
        }
    }

    /// The tools offered, under their qualified names.
    pub fn qualified(&self) -> &Qualified {
        &self.qualified
    }

    /// What the chat is told when the servers are up: one `mcp: started <name>`
    /// per server that runs, then the notices.
    pub fn announcements(&self) -> Vec<String> {
        let started = self
            .clients
            .iter()
            .map(|(name, _)| format!("mcp: started {name}"));
        started.chain(self.notices()).collect()
    }

    /// One line per server that is missing and per tool that was dropped.
    pub fn notices(&self) -> Vec<String> {
        let dropped = self.qualified.dropped.iter().map(|d| d.notice());
        self.unavailable.iter().cloned().chain(dropped).collect()
    }

    /// Calls the tool `qualified` with `arguments`. The caller has the user's
    /// approval. Never fails: a name nobody owns, a cancel, a timeout or a
    /// dead server is an error result.
    pub async fn call(
        &self,
        qualified: &str,
        arguments: Value,
        cancel: impl Future<Output = ()>,
    ) -> Outcome {
        let owner = self.qualified.raw_of(qualified).and_then(|(server, raw)| {
            let (_, client) = self.clients.iter().find(|(name, _)| name == server)?;
            Some((client, raw))
        });
        let Some((client, raw)) = owner else {
            return Outcome {
                text: NOT_AVAILABLE.to_owned(),
                is_error: true,
            };
        };
        let mut cancel = std::pin::pin!(cancel);
        // A server answers one request at a time: a second call waits.
        let mut guard = tokio::select! {
            guard = client.lock() => guard,
            () = &mut cancel => {
                return Outcome {
                    text: format!("mcp error: {}", ClientError::Cancelled.reason()),
                    is_error: true,
                };
            }
        };
        match guard.as_mut() {
            Some(client) => client.call(raw, arguments, &mut cancel).await,
            None => Outcome {
                text: NOT_AVAILABLE.to_owned(),
                is_error: true,
            },
        }
    }

    /// Stops every server and its process group. Works through an `Arc`; a
    /// call after it is refused. A server whose call is still in flight is
    /// waited for at most [`STOP_WAIT`]; one that is missed (or a set that is
    /// just dropped) is killed when it drops, by `Process`'s `Drop` and
    /// `kill_on_drop`.
    pub async fn shutdown(&self) {
        join_all(self.clients.iter().map(|(_, slot)| async {
            let Ok(mut guard) = timeout(STOP_WAIT, slot.lock()).await else {
                return;
            };
            if let Some(client) = guard.take() {
                client.shutdown().await;
            }
        }))
        .await;
    }
}

#[cfg(test)]
pub(crate) mod fake;
#[cfg(test)]
mod tests;
