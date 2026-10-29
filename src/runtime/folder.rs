//! The folder probe (chat-folder, design D2): resolves a path on disk and
//! judges it with the same secret roots a tool call builds, so the chat never
//! accepts a folder the tools would refuse.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use tokio::{sync::mpsc, time::timeout};

use crate::{
    core::{
        ai::chat::folder::{Probed, Purpose, Refusal, judge},
        layout::PaneId,
    },
    runtime::tools::{ToolEnv, fs::load_roots},
};

/// How long a probe may take. A HOME on a hung network or FUSE mount would
/// block `canonicalize`: the probe answers `folder not found` instead and the
/// thread is left to finish on its own.
pub const TIMEOUT: Duration = Duration::from_secs(3);

/// What a probe answers: the pane, why it was asked, and the verdict.
pub type Answer = (PaneId, Purpose, Result<Probed, Refusal>);

/// Runs `work` on the blocking pool for at most `bound`; a probe that is too
/// slow, or whose thread dies, is `folder not found`.
pub async fn bounded(
    bound: Duration,
    work: impl FnOnce() -> Result<Probed, Refusal> + Send + 'static,
) -> Result<Probed, Refusal> {
    match timeout(bound, tokio::task::spawn_blocking(work)).await {
        Ok(Ok(result)) => result,
        _ => Err(Refusal::NotFound),
    }
}

/// Starts the probes of the app off the event loop and sends their answers.
pub struct Prober {
    env: ToolEnv,
    tx: mpsc::Sender<Answer>,
}

impl Prober {
    pub fn new(env: ToolEnv, tx: mpsc::Sender<Answer>) -> Self {
        Self { env, tx }
    }

    /// Probes `path` and sends exactly one answer. Returns at once; needs a
    /// tokio runtime.
    pub fn start(&self, pane: PaneId, path: PathBuf, purpose: Purpose) {
        let (env, tx) = (self.env.clone(), self.tx.clone());
        tokio::spawn(async move {
            let result = bounded(TIMEOUT, move || probe(&env, &path)).await;
            let _ = tx.send((pane, purpose, result)).await;
        });
    }
}

/// Resolves `path` (symlinks followed) and judges it. A missing path, a file
/// and a refused folder each have their reason; with HOME unusable everything
/// is `not allowed`, as for the tools.
pub fn probe(env: &ToolEnv, path: &Path) -> Result<Probed, Refusal> {
    // A relative path would read against the process cwd, not the chat.
    if !path.is_absolute() {
        return Err(Refusal::NotFound);
    }
    let real = std::fs::canonicalize(path).map_err(|_| Refusal::NotFound)?;
    if !real.is_dir() {
        return Err(Refusal::NotAFolder);
    }
    // `/` is refused whatever HOME says.
    if real.parent().is_none() {
        return Err(Refusal::Root);
    }
    let roots = load_roots(env).map_err(|_| Refusal::NotAllowed)?;
    let verdict = judge(&real, &roots)?;
    Ok(Probed {
        path: real,
        verdict,
    })
}

#[cfg(test)]
mod tests;
