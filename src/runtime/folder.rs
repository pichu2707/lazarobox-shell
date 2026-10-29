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
        ai::chat::{
            folder::{Probed, Purpose, Refusal, judge},
            folder_picker::Source,
        },
        layout::PaneId,
    },
    runtime::tools::{ToolEnv, fs::load_roots},
};

/// How long a probe may take. A HOME on a hung network or FUSE mount would
/// block `canonicalize`: the probe answers `folder not found` instead and the
/// thread is left to finish on its own.
pub const TIMEOUT: Duration = Duration::from_secs(3);

/// What the folder worker answers.
#[derive(Debug)]
pub enum Answer {
    /// One folder: the pane, why it was asked, and the verdict.
    Probed(PaneId, Purpose, Result<Probed, Refusal>),
    /// The picker candidates, each judged.
    Many(PaneId, Vec<(Source, Result<Probed, Refusal>)>),
    /// The directory names that complete the field text the pane sent.
    Completed(PaneId, String, Vec<String>),
}

/// Runs `work` on the blocking pool for at most `bound`; `None` when it is
/// too slow or its thread dies (the thread is left to finish on its own).
pub async fn within<T: Send + 'static>(
    bound: Duration,
    work: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    timeout(bound, tokio::task::spawn_blocking(work))
        .await
        .ok()?
        .ok()
}

/// [`within`] for a probe: too slow, or a dead thread, is `folder not found`.
pub async fn bounded(
    bound: Duration,
    work: impl FnOnce() -> Result<Probed, Refusal> + Send + 'static,
) -> Result<Probed, Refusal> {
    within(bound, work).await.unwrap_or(Err(Refusal::NotFound))
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
            let _ = tx.send(Answer::Probed(pane, purpose, result)).await;
        });
    }

    /// Judges the picker candidates (see [`probe_many`]) and sends exactly one
    /// answer; too slow is an empty list.
    pub fn start_many(&self, pane: PaneId, paths: Vec<(Source, PathBuf)>, git: bool) {
        let (env, tx) = (self.env.clone(), self.tx.clone());
        tokio::spawn(async move {
            let found = within(TIMEOUT, move || probe_many(&env, &paths, git))
                .await
                .unwrap_or_default();
            let _ = tx.send(Answer::Many(pane, found)).await;
        });
    }

    /// Lists the completions of `text` (see [`complete`]) and sends exactly
    /// one answer; too slow is an empty list.
    pub fn start_completion(&self, pane: PaneId, text: String, dir: PathBuf, prefix: String) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let names = within(TIMEOUT, move || complete(&dir, &prefix))
                .await
                .unwrap_or_default();
            let _ = tx.send(Answer::Completed(pane, text, names)).await;
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

/// The git root that holds `start`: the nearest folder up from it with a
/// `.git` (a folder, or the file of a worktree). The walk stops below HOME and
/// `/`, so HOME itself, a dotfiles repo there, is never a root.
pub fn walk_git(home: &Path, start: &Path) -> Option<PathBuf> {
    let (home, start) = (
        std::fs::canonicalize(home).ok()?,
        std::fs::canonicalize(start).ok()?,
    );
    start
        .ancestors()
        .take_while(|dir| *dir != home && dir.starts_with(&home))
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf)
}

/// Probes every candidate. With `git`, a `PaneRoot` candidate is first
/// moved up to its git root and dropped when it has none.
pub fn probe_many(
    env: &ToolEnv,
    paths: &[(Source, PathBuf)],
    git: bool,
) -> Vec<(Source, Result<Probed, Refusal>)> {
    let home = env.home.as_deref().map(Path::new);
    paths
        .iter()
        .filter_map(|(source, path)| {
            let path = if git && *source == Source::PaneRoot {
                walk_git(home?, path)?
            } else {
                path.clone()
            };
            Some((*source, probe(env, &path)))
        })
        .collect()
}

/// The most completions listed.
pub const COMPLETIONS_MAX: usize = 50;

/// The names of the directories in `dir` that start with `prefix`, sorted, at
/// most [`COMPLETIONS_MAX`]. Hidden ones only when `prefix` starts with `.`.
pub fn complete(dir: &Path, prefix: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let hidden = prefix.starts_with('.');
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with(prefix) && (hidden || !name.starts_with('.')))
        .filter(|name| dir.join(name).is_dir())
        .collect();
    names.sort();
    names.truncate(COMPLETIONS_MAX);
    names
}

#[cfg(test)]
mod tests;
