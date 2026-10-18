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

use std::{ffi::OsString, future::Future, path::Path};

use futures::future::join_all;
use serde_json::Value;
use tokio::sync::Mutex;

use super::{
    Outcome,
    client::{ClientError, McpClient, Timeouts},
};
use crate::core::ai::{
    config::McpServer,
    sanitize::clean,
    tools::servers::{Qualified, qualify},
};

/// What the model is told when a name reaches no running server.
const NOT_AVAILABLE: &str = "mcp error: that tool is not available";

/// The running servers and the tools they offer.
pub struct ExternalSet {
    clients: Vec<(String, Mutex<McpClient>)>,
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
) -> Result<(McpClient, Vec<crate::core::ai::tools::ExternalTool>), String> {
    let mut client = McpClient::start(server, home, lookup, timeouts).await?;
    match client.list_tools().await {
        Ok(tools) => Ok((client, tools)),
        Err(error) => {
            client.shutdown().await;
            Err(error.reason())
        }
    }
}

impl ExternalSet {
    /// Starts every server at once. Must be called inside a tokio runtime.
    pub async fn start(
        servers: &[&McpServer],
        home: &Path,
        lookup: impl Fn(&str) -> Option<OsString>,
        timeouts: Timeouts,
    ) -> Self {
        let started = join_all(servers.iter().map(|server| async {
            (
                clean(&server.name),
                connect(server, home, &lookup, timeouts).await,
            )
        }))
        .await;
        let mut clients = Vec::new();
        let mut listed = Vec::new();
        let mut unavailable = Vec::new();
        for (name, result) in started {
            match result {
                Ok((client, tools)) => {
                    listed.push((name.clone(), tools));
                    clients.push((name, Mutex::new(client)));
                }
                Err(reason) => unavailable.push(format!("mcp: {name}: {reason}")),
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
        let mut client = tokio::select! {
            guard = client.lock() => guard,
            () = &mut cancel => {
                return Outcome {
                    text: format!("mcp error: {}", ClientError::Cancelled.reason()),
                    is_error: true,
                };
            }
        };
        client.call(raw, arguments, &mut cancel).await
    }

    /// Stops every server.
    pub async fn shutdown(self) {
        join_all(
            self.clients
                .into_iter()
                .map(|(_, client)| client.into_inner().shutdown()),
        )
        .await;
    }
}

#[cfg(test)]
mod fake;
#[cfg(test)]
mod tests;
