//! The probe of the Connectors area: starts one declared server outside any
//! chat, the way a chat start does ([`external::connect`], the same spawn and
//! environment rules, `secrets.toml` values included), reads its tools,
//! reports, and stops it. It is started only after the user's `y`, writes no
//! file, and its report never holds a `secrets.toml` value: the lines are made
//! from the server's name, counts and tool names, and the problems name a
//! table or a key at most.

use std::{
    ffi::OsString,
    future::Future,
    path::{Path, PathBuf},
};

use tokio::sync::{mpsc, oneshot};

use super::{client::Timeouts, external::connect};
use crate::{
    app::AppEvent,
    core::ai::{
        config::McpServer,
        mcp_catalog::{
            checks::{self, Probed, Verdict},
            entry,
        },
        sanitize::clean,
    },
    runtime::mcp_secrets::load_for_start,
};

/// Starts `server`, lists its tools and stops it. `cancel` ending (Esc) stops
/// the probe: the half-started server is dropped, which kills its process
/// group.
pub async fn probe(
    server: &McpServer,
    home: &Path,
    host: impl Fn(&str) -> Option<OsString>,
    secrets: Option<PathBuf>,
    timeouts: Timeouts,
    cancel: impl Future<Output = ()>,
) -> Probed {
    let loaded = load_for_start(secrets).await;
    // This server's own values only, as in a chat start.
    let own = loaded.secrets.lookup_for(server, &host);
    let connected = tokio::select! {
        result = connect(server, home, &own, timeouts) => Some(result),
        () = cancel => None,
    };
    let (verdict, lines) = match connected {
        None => (
            Verdict::Stopped,
            vec![format!("mcp: {} probe stopped", clean(&server.name))],
        ),
        Some(Err(reason)) => (
            Verdict::Failed,
            vec![checks::failed(
                &server.name,
                &reason,
                timeouts.handshake.as_secs(),
            )],
        ),
        Some(Ok((client, listed))) => {
            let names: Vec<String> = listed.tools.into_iter().map(|t| t.name).collect();
            let lines = checks::started(&server.name, entry(&server.name), listed.admitted, &names);
            client.shutdown().await;
            (Verdict::Started, lines)
        }
    };
    Probed {
        server: clean(&server.name),
        verdict,
        lines,
        problems: loaded.problems,
    }
}

/// Runs the probes the app asks for, one at a time, and answers each with one
/// `AppEvent::McpProbe`.
pub struct Prober {
    home: PathBuf,
    secrets: Option<PathBuf>,
    events: mpsc::Sender<AppEvent>,
    stop: Option<oneshot::Sender<()>>,
}

impl Prober {
    pub fn new(home: PathBuf, secrets: Option<PathBuf>, events: mpsc::Sender<AppEvent>) -> Self {
        Self {
            home,
            secrets,
            events,
            stop: None,
        }
    }

    /// Starts the probe of `server` on a task of its own; one already running
    /// is stopped first.
    pub fn start(&mut self, server: McpServer) {
        self.cancel();
        let (stop, stopped) = oneshot::channel::<()>();
        self.stop = Some(stop);
        let (home, secrets, events) =
            (self.home.clone(), self.secrets.clone(), self.events.clone());
        tokio::spawn(async move {
            let cancel = async {
                // A dropped sender is not an Esc.
                if stopped.await.is_err() {
                    std::future::pending::<()>().await;
                }
            };
            let report = probe(
                &server,
                &home,
                |name| std::env::var_os(name),
                secrets,
                Timeouts::default(),
                cancel,
            )
            .await;
            let _ = events.send(AppEvent::McpProbe(report)).await;
        });
    }

    /// Esc: stops the probe that runs, if any.
    pub fn cancel(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

#[cfg(test)]
mod tests;
