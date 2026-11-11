//! The probe of the Connectors area: starts one declared server outside any
//! chat, the way a chat start does ([`external::connect`], the same spawn and
//! environment rules, `secrets.toml` values included), reads its tools,
//! reports, and stops it. It is started only after the user's `y`, writes no
//! file, and the `secrets.toml` problems name a table or a key at most. The
//! lines of a failure carry text the server said (its stderr tail, an RPC
//! error), which is redacted by shape only (`sanitize`, `redact_tokens`): a
//! follow-up replaces the exact values of that server.

use std::{
    ffi::OsString,
    future::Future,
    panic::AssertUnwindSafe,
    path::{Path, PathBuf},
};

use futures::FutureExt;
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

use super::{client::Timeouts, external::connect};
use crate::{
    app::AppEvent,
    core::ai::{
        config::McpServer,
        mcp_catalog::{
            checks::{self, Probed, Verdict},
            entry,
        },
        mcp_secrets::{redact_list, redact_values},
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
    let values = redact_list(server, &own);
    let connected = tokio::select! {
        result = connect(server, home, &own, &values, timeouts) => Some(result),
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
        problems: loaded
            .problems
            .iter()
            .map(|p| redact_values(p, &values))
            .collect(),
    }
}

/// `report` of a probe that may panic: a panic is a failed probe, so the area
/// never waits for a report that will not come. The panic text is not shown.
async fn guarded(server: &str, report: impl Future<Output = Probed>) -> Probed {
    match AssertUnwindSafe(report).catch_unwind().await {
        Ok(report) => report,
        Err(_) => Probed {
            server: clean(server),
            verdict: Verdict::Failed,
            lines: vec![format!(
                "mcp: {} unavailable: the probe failed",
                clean(server)
            )],
            problems: Vec::new(),
        },
    }
}

/// Runs the probes the app asks for, one at a time, and answers each with one
/// `AppEvent::McpProbe` carrying the number the app gave it.
pub struct Prober {
    home: PathBuf,
    secrets: Option<PathBuf>,
    events: mpsc::Sender<AppEvent>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl Prober {
    pub fn new(home: PathBuf, secrets: Option<PathBuf>, events: mpsc::Sender<AppEvent>) -> Self {
        Self {
            home,
            secrets,
            events,
            stop: None,
            task: None,
        }
    }

    /// Starts probe `id` of `server` on a task of its own. One that still runs
    /// is replaced: its task is aborted (dropping its server kills the
    /// process group) and it reports nothing, so two probes never overlap.
    pub fn start(&mut self, id: u64, server: McpServer) {
        self.stop = None;
        if let Some(old) = self.task.take() {
            old.abort();
        }
        let (stop, stopped) = oneshot::channel::<()>();
        self.stop = Some(stop);
        let (home, secrets, events) =
            (self.home.clone(), self.secrets.clone(), self.events.clone());
        self.task = Some(tokio::spawn(async move {
            let cancel = async {
                // A dropped sender is not an Esc.
                if stopped.await.is_err() {
                    std::future::pending::<()>().await;
                }
            };
            let run = probe(
                &server,
                &home,
                |name| std::env::var_os(name),
                secrets,
                Timeouts::default(),
                cancel,
            );
            let report = guarded(&server.name, run).await;
            let _ = events.send(AppEvent::McpProbe(id, report)).await;
        }));
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
