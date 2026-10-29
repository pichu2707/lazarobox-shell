//! The requests of the Ollama download screens (ollama-download): the
//! library page on ollama.com, then a manifest and the license layers of a
//! model on the registry, HTTPS GETs through the system curl, as the Hub's.
//! Then the pull itself, through the local Ollama server.

use std::path::PathBuf;

use tokio::sync::{mpsc, oneshot};

use crate::app::AppEvent;
use crate::core::ai::hf::End;
use crate::core::ai::ollama_lib::{self, Card, Manifest, Pull};

use super::hf::{Curl, Fetch, REPLY_CAP};
use super::loopback::{PULL_PATH, Timeouts, pull_with};

/// The library page: HTML, 4 MiB, no redirects.
const PAGE: Fetch = Fetch {
    accept: "text/html",
    cap: REPLY_CAP,
    cut: false,
    follow: false,
};

/// A manifest: the registry's v2 JSON, 4 MiB, no redirects.
const MANIFEST: Fetch = Fetch {
    accept: "application/vnd.docker.distribution.manifest.v2+json",
    cap: REPLY_CAP,
    cut: false,
    follow: false,
};

/// A license blob: the registry sends it on to its storage, and only the
/// start is needed for its title.
const LICENSE: Fetch = Fetch {
    accept: "text/plain",
    cap: ollama_lib::LICENSE_CAP,
    cut: true,
    follow: true,
};

/// The production curls: the library site and the registry.
pub fn curls(path: std::ffi::OsString) -> (Curl, Curl) {
    (
        Curl::ollama(path.clone(), ollama_lib::LIBRARY),
        Curl::ollama(path, ollama_lib::REGISTRY),
    )
}

/// The cards of the library page; none when the page holds none.
pub async fn library(site: &Curl) -> Result<Vec<Card>, String> {
    match site.fetch(ollama_lib::library_path(), PAGE).await? {
        (200, body) => Ok(ollama_lib::parse_library(&body)),
        (code, _) => Err(site.failed(format!("HTTP {code}"))),
    }
}

/// The manifest of `model:tag`. A name that is not safe is never sent.
pub async fn manifest(registry: &Curl, model: &str, tag: &str) -> Result<Manifest, String> {
    if !ollama_lib::is_name(model) || !ollama_lib::is_name(tag) {
        return Err(registry.failed("not a model name"));
    }
    match registry
        .fetch(&ollama_lib::manifest_path(model, tag), MANIFEST)
        .await?
    {
        (200, body) => ollama_lib::parse_manifest(&body),
        (code, _) => Err(registry.failed(format!("HTTP {code}"))),
    }
}

/// The title of the license blob `digest` of `model`.
pub async fn license(registry: &Curl, model: &str, digest: &str) -> Result<String, String> {
    if !ollama_lib::is_name(model) {
        return Err(registry.failed("not a model name"));
    }
    match registry
        .fetch(&ollama_lib::blob_path(model, digest), LICENSE)
        .await?
    {
        (200, body) => Ok(ollama_lib::license_title(&body)),
        (code, _) => Err(registry.failed(format!("HTTP {code}"))),
    }
}

/// Pulls `pull` through the Ollama server on `127.0.0.1:port`, calling
/// `progress(done, total)` as it rises, until it ends or `cancel` fires (or
/// its sender is dropped). A cancel closes the connection; Ollama keeps the
/// parts it has, so the next pull of the same tag resumes.
pub async fn pull(
    timeouts: &Timeouts,
    port: u16,
    pull: &Pull,
    mut progress: impl FnMut(u64, u64) + Send,
    cancel: oneshot::Receiver<()>,
) -> End {
    let body = pull.body();
    let request = pull_with(timeouts, "127.0.0.1", port, PULL_PATH, &body, &mut progress);
    tokio::select! {
        ended = request => match ended {
            // Ollama keeps its own store: there is no file to name.
            Ok(()) => End::Done(PathBuf::new()),
            Err(reason) => End::Failed(format!("failed: {reason}")),
        },
        _ = cancel => End::Cancelled,
    }
}

/// Runs the one pull of the picker's Ollama screens in the background and
/// sends its progress and its end to the event loop, as the Hub does for a
/// Hugging Face download.
pub struct Ollama {
    /// The Ollama server's port, as detection uses it.
    port: u16,
    timeouts: Timeouts,
    events: mpsc::Sender<AppEvent>,
    /// The cancel of the running pull, open while it runs.
    cancel: Option<oneshot::Sender<()>>,
}

impl Ollama {
    pub fn new(port: u16, events: mpsc::Sender<AppEvent>) -> Self {
        Ollama {
            port,
            timeouts: Timeouts::PULL,
            events,
            cancel: None,
        }
    }

    /// Starts pulling `pull`, unless one runs already. Its progress comes as
    /// `AppEvent::DownloadProgress` (dropped when the loop is behind) and
    /// its end as one `AppEvent::DownloadEnd`.
    pub fn start(&mut self, job: Pull) {
        if self.cancel.as_ref().is_some_and(|tx| !tx.is_closed()) {
            return;
        }
        let (cancel, cancelled) = oneshot::channel();
        self.cancel = Some(cancel);
        let (port, timeouts, events) = (self.port, self.timeouts, self.events.clone());
        tokio::spawn(async move {
            let progress = |done, total| {
                let _ = events.try_send(AppEvent::DownloadProgress { done, total });
            };
            let end = pull(&timeouts, port, &job, progress, cancelled).await;
            let _ = events.send(AppEvent::DownloadEnd(end)).await;
        });
    }

    /// Stops the running pull by closing its connection.
    pub fn cancel(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
    }
}

#[cfg(test)]
mod pull_tests;
#[cfg(test)]
mod tests;
