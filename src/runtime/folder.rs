//! The folder probe (chat-folder, design D2): resolves a path on disk and
//! judges it with the same secret roots a tool call builds, so the chat never
//! accepts a folder the tools would refuse.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use futures::future::join_all;
use tokio::{sync::mpsc, time::timeout};

use crate::{
    core::{
        ai::{
            chat::{
                folder::{Probed, Purpose, Refusal, Verdict, judge},
                folder_picker::{Completion, Source},
            },
            tools::paths::SecretRoots,
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
    /// The picker candidates (pane, picker id), each judged.
    Many(PaneId, u64, Vec<(Source, Result<Probed, Refusal>)>),
    /// The directory names that complete the field text the picker sent
    /// (pane, picker id, text).
    Completed(PaneId, u64, String, Completion),
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

/// At most one request of a kind at a time: a hung filesystem leaves its
/// blocking threads behind, and repeated asks must not fill the pool.
#[derive(Clone, Default)]
struct Flight(Arc<AtomicBool>);

/// Held by the blocking work of a request; frees the [`Flight`] when the last
/// clone is dropped, that is, when the work really ended.
struct Held(Arc<AtomicBool>);

impl Drop for Held {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl Flight {
    fn start(&self) -> Option<Arc<Held>> {
        self.0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Arc::new(Held(self.0.clone())))
    }
}

/// Starts the probes of the app off the event loop and sends their answers.
pub struct Prober {
    env: ToolEnv,
    tx: mpsc::Sender<Answer>,
    many: Flight,
    completions: Flight,
}

impl Prober {
    pub fn new(env: ToolEnv, tx: mpsc::Sender<Answer>) -> Self {
        Self {
            env,
            tx,
            many: Flight::default(),
            completions: Flight::default(),
        }
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

    /// Judges the picker candidates, each bounded on its own (see
    /// [`candidate`]), and sends exactly one answer. While an earlier request
    /// is still stuck on the disk the answer is empty.
    pub fn start_many(&self, pane: PaneId, id: u64, paths: Vec<(Source, PathBuf)>, git: bool) {
        let (env, tx, held) = (self.env.clone(), self.tx.clone(), self.many.start());
        tokio::spawn(async move {
            let found = match held {
                Some(held) => {
                    let kept = held.clone();
                    let roots = within(TIMEOUT, move || {
                        let _held = kept;
                        load_roots(&env).ok()
                    })
                    .await
                    .flatten();
                    match roots {
                        Some(roots) => {
                            each(paths, TIMEOUT, move |source, path| {
                                let _held = &held;
                                candidate(&roots, source, path, git)
                            })
                            .await
                        }
                        None => Vec::new(),
                    }
                }
                None => Vec::new(),
            };
            let _ = tx.send(Answer::Many(pane, id, found)).await;
        });
    }

    /// Lists the completions of `text` (see [`complete`]) and sends exactly
    /// one answer; too slow, or one still running, is an empty list.
    pub fn start_completion(
        &self,
        pane: PaneId,
        id: u64,
        text: String,
        dir: PathBuf,
        prefix: String,
    ) {
        let (env, tx, held) = (self.env.clone(), self.tx.clone(), self.completions.start());
        tokio::spawn(async move {
            let names = match held {
                Some(held) => within(TIMEOUT, move || {
                    let _held = held;
                    load_roots(&env)
                        .map(|roots| complete(&roots, &dir, &prefix))
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default(),
                None => Completion::default(),
            };
            let _ = tx.send(Answer::Completed(pane, id, text, names)).await;
        });
    }
}

/// Resolves `path` (symlinks followed) and judges it. A missing path, a file
/// and a refused folder each have their reason; with HOME unusable everything
/// is `not allowed`, as for the tools.
pub fn probe(env: &ToolEnv, path: &Path) -> Result<Probed, Refusal> {
    let real = resolve(path)?;
    // `/` is refused whatever HOME says.
    if real.parent().is_none() {
        return Err(Refusal::Root);
    }
    let roots = load_roots(env).map_err(|_| Refusal::NotAllowed)?;
    judged(&roots, real)
}

/// [`probe`] with the roots already loaded.
fn probe_with(roots: &SecretRoots, path: &Path) -> Result<Probed, Refusal> {
    judged(roots, resolve(path)?)
}

/// The real path of an existing folder.
fn resolve(path: &Path) -> Result<PathBuf, Refusal> {
    // A relative path would read against the process cwd, not the chat.
    if !path.is_absolute() {
        return Err(Refusal::NotFound);
    }
    let real = std::fs::canonicalize(path).map_err(|_| Refusal::NotFound)?;
    if real.is_dir() {
        Ok(real)
    } else {
        Err(Refusal::NotAFolder)
    }
}

fn judged(roots: &SecretRoots, real: PathBuf) -> Result<Probed, Refusal> {
    let verdict = judge(&real, roots)?;
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

/// Judges one picker candidate. With `git`, a `PaneRoot` candidate is first
/// moved up to its git root and dropped (`None`) when it has none.
pub fn candidate(
    roots: &SecretRoots,
    source: Source,
    path: &Path,
    git: bool,
) -> Option<Result<Probed, Refusal>> {
    if git && source == Source::PaneRoot {
        let root = walk_git(roots.home(), path)?;
        return Some(probe_with(roots, &root));
    }
    Some(probe_with(roots, path))
}

/// Runs `work` for every candidate at once, each on the blocking pool and
/// bounded on its own: one that stalls is `folder not found` and never takes
/// the others with it. Candidates `work` drops (`None`) are left out.
pub async fn each(
    items: Vec<(Source, PathBuf)>,
    bound: Duration,
    work: impl Fn(Source, &Path) -> Option<Result<Probed, Refusal>> + Send + Sync + 'static,
) -> Vec<(Source, Result<Probed, Refusal>)> {
    let work = Arc::new(work);
    let tasks = items.into_iter().map(|(source, path)| {
        let work = work.clone();
        async move {
            match within(bound, move || work(source, &path)).await {
                Some(done) => done.map(|result| (source, result)),
                None => Some((source, Err(Refusal::NotFound))),
            }
        }
    });
    join_all(tasks).await.into_iter().flatten().collect()
}

/// The most completions listed.
pub const COMPLETIONS_MAX: usize = 50;

/// The names of the directories in `dir` that start with `prefix`, sorted, at
/// most [`COMPLETIONS_MAX`]. Hidden ones only when `prefix` starts with `.`.
/// Nothing is listed unless `dir` is a folder a chat may work in or HOME, and
/// an entry whose real path the tools would refuse (a link into `~/.ssh`) is
/// left out, as is a name with a control character.
pub fn complete(roots: &SecretRoots, dir: &Path, prefix: &str) -> Completion {
    let none = Completion::default();
    let Ok(real) = std::fs::canonicalize(dir) else {
        return none;
    };
    match probe_with(roots, &real) {
        Ok(Probed {
            verdict: Verdict::Concrete | Verdict::Home,
            ..
        }) => {}
        _ => return none,
    }
    let Ok(entries) = std::fs::read_dir(&real) else {
        return none;
    };
    let hidden = prefix.starts_with('.');
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with(prefix) && (hidden || !name.starts_with('.')))
        .filter(|name| !name.chars().any(char::is_control))
        .collect();
    names.sort();
    let mut found = Completion::default();
    for name in names {
        let allowed = std::fs::canonicalize(real.join(&name))
            .is_ok_and(|entry| entry.is_dir() && roots.judge_real(&entry));
        if !allowed {
            continue;
        }
        if found.names.len() == COMPLETIONS_MAX {
            found.truncated = true;
            break;
        }
        found.names.push(name);
    }
    found
}

#[cfg(test)]
mod tests;
