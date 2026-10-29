//! The requests of the Ollama download screens (ollama-download): the
//! library page on ollama.com, then a manifest and the license layers of a
//! model on the registry, HTTPS GETs through the system curl, as the Hub's.
//! Then the pull itself, through the local Ollama server.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};

use crate::app::AppEvent;
use crate::core::ai::hf::End;
use crate::core::ai::ollama_lib::{self, Answer, Ask, Card, Licenses, Manifest, Pull};

use super::hf::{Curl, Fetch, REPLY_CAP, free_space, total_ram};
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
    // The names came from the library page: never sent unless safe.
    if !ollama_lib::is_name(&pull.model) || !ollama_lib::is_name(&pull.tag) {
        return End::Failed("failed: not a model name".to_owned());
    }
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

/// Where Ollama keeps its models, as far as this process can tell: the
/// `OLLAMA_MODELS` folder, else the first of `~/.ollama/models` and the Linux
/// service's `/usr/share/ollama/.ollama/models` that exists. `None` when no
/// folder is known; the server may still use another.
pub fn models_folder(
    env: Option<std::ffi::OsString>,
    home: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    if let Some(folder) = env.filter(|f| !f.is_empty()) {
        return Some(PathBuf::from(folder));
    }
    let user = home.map(|home| home.join(".ollama/models"));
    user.into_iter()
        .chain([PathBuf::from("/usr/share/ollama/.ollama/models")])
        .find(|folder| exists(folder))
}

/// The cards of the Popular screen: the library page's, or the curated
/// catalog's when the page cannot be read or holds none.
async fn popular_cards(site: &Curl) -> (Vec<Card>, bool) {
    match library(site).await {
        Ok(cards) if !cards.is_empty() => (cards, false),
        _ => (ollama_lib::fallback_cards(), true),
    }
}

/// The answer to one request of the Ollama screens.
async fn answer(
    site: &Curl,
    registry: &Curl,
    folder: Option<&Path>,
    ask: &Ask,
) -> Result<Answer, String> {
    match ask {
        Ask::Popular => {
            let (cards, fallback) = popular_cards(site).await;
            Ok(Answer::Popular {
                cards,
                fallback,
                ram: total_ram(),
            })
        }
        Ask::Sizes { model, tags } => {
            let mut manifests = Vec::new();
            for tag in tags {
                manifests.push((tag.clone(), manifest(registry, model, tag).await.ok()));
            }
            Ok(Answer::Sizes {
                manifests,
                ram: total_ram(),
            })
        }
        Ask::Licenses {
            model,
            tag,
            digests,
        } => {
            let (size, digests) = match digests {
                Some(digests) => (None, digests.clone()),
                None => {
                    let manifest = manifest(registry, model, tag).await?;
                    (Some(manifest.size), manifest.licenses)
                }
            };
            let mut titles = Vec::new();
            for digest in &digests {
                titles.push(license(registry, model, digest).await?);
            }
            Ok(Answer::Licenses(Box::new(Licenses {
                size,
                titles,
                folder: folder.map(Path::to_owned),
                free: folder.and_then(free_space),
            })))
        }
    }
}

/// Answers the requests of the picker's Ollama screens and runs their one
/// pull in the background, sending the replies, the progress and the end
/// to the event loop, as the Hub does for a Hugging Face download.
pub struct Ollama {
    site: Arc<Curl>,
    registry: Arc<Curl>,
    /// Where Ollama keeps its models, when known.
    folder: Option<PathBuf>,
    timeouts: Timeouts,
    events: mpsc::Sender<AppEvent>,
    /// The cancel of the running pull, open while it runs.
    cancel: Option<oneshot::Sender<()>>,
}

impl Ollama {
    /// A worker running curl from `path` (the `PATH` it is looked up in).
    pub fn new(
        path: std::ffi::OsString,
        folder: Option<PathBuf>,
        events: mpsc::Sender<AppEvent>,
    ) -> Self {
        let (site, registry) = curls(path);
        Ollama {
            site: Arc::new(site),
            registry: Arc::new(registry),
            folder,
            timeouts: Timeouts::PULL,
            events,
            cancel: None,
        }
    }

    /// Tests: the library and the registry through these curls.
    #[cfg(test)]
    pub(crate) fn with_curls(mut self, site: Curl, registry: Curl) -> Self {
        self.site = Arc::new(site);
        self.registry = Arc::new(registry);
        self
    }

    /// Answers `ask` with one `AppEvent::OllamaReply`.
    pub fn ask(&self, ask: Ask) {
        let (site, registry) = (self.site.clone(), self.registry.clone());
        let (folder, events) = (self.folder.clone(), self.events.clone());
        tokio::spawn(async move {
            let answer = answer(&site, &registry, folder.as_deref(), &ask).await;
            let _ = events.send(AppEvent::OllamaReply(ask, answer)).await;
        });
    }

    /// Starts pulling `job` through the Ollama server on `127.0.0.1:port`,
    /// unless one runs already. Its progress comes as
    /// `AppEvent::DownloadProgress` (dropped when the loop is behind) and
    /// its end as one `AppEvent::DownloadEnd`.
    pub fn start(&mut self, port: u16, job: Pull) {
        if self.cancel.as_ref().is_some_and(|tx| !tx.is_closed()) {
            return;
        }
        let (cancel, cancelled) = oneshot::channel();
        self.cancel = Some(cancel);
        let (timeouts, events) = (self.timeouts, self.events.clone());
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
