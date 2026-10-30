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
    /// The picker candidates (pane, picker id), each judged, and whether an
    /// earlier request was still on the disk, so none was made.
    Many(PaneId, u64, Vec<(Source, Result<Probed, Refusal>)>, bool),
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
    /// How long a request may take; [`TIMEOUT`] outside tests.
    bound: Duration,
}

impl Prober {
    pub fn new(env: ToolEnv, tx: mpsc::Sender<Answer>) -> Self {
        Self {
            env,
            tx,
            many: Flight::default(),
            completions: Flight::default(),
            bound: TIMEOUT,
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
    /// is still stuck on the disk the answer is empty and says it is busy.
    pub fn start_many(&self, pane: PaneId, id: u64, paths: Vec<(Source, PathBuf)>, git: bool) {
        self.many_with(pane, id, paths, move |roots| {
            move |source, path: &Path| candidate(&roots, source, path, git)
        });
    }

    /// [`Self::start_many`] with the judging of one candidate chosen by
    /// `make` from the loaded roots.
    fn many_with<W>(
        &self,
        pane: PaneId,
        id: u64,
        paths: Vec<(Source, PathBuf)>,
        make: impl FnOnce(SecretRoots) -> W + Send + 'static,
    ) where
        W: Fn(Source, &Path) -> Option<Result<Probed, Refusal>> + Send + Sync + 'static,
    {
        let (env, tx, held) = (self.env.clone(), self.tx.clone(), self.many.start());
        let bound = self.bound;
        tokio::spawn(async move {
            let busy = held.is_none();
            let mut found = Vec::new();
            if let Some(held) = held {
                let kept = held.clone();
                let roots = within(bound, move || {
                    let _held = kept;
                    load_roots(&env).ok()
                })
                .await
                .flatten();
                if let Some(roots) = roots {
                    let work = make(roots);
                    found = each(paths, bound, move |source, path| {
                        let _held = &held;
                        work(source, path)
                    })
                    .await;
                }
            }
            let _ = tx.send(Answer::Many(pane, id, found, busy)).await;
        });
    }

    /// Lists the completions of `text` (see [`complete`]) and sends exactly
    /// one answer; too slow is an empty list, and one still running is an
    /// empty list marked busy.
    pub fn start_completion(
        &self,
        pane: PaneId,
        id: u64,
        text: String,
        dir: PathBuf,
        prefix: String,
    ) {
        self.completion_with(pane, id, text, move |roots| complete(roots, &dir, &prefix));
    }

    /// [`Self::start_completion`] with the listing chosen by `work`.
    fn completion_with(
        &self,
        pane: PaneId,
        id: u64,
        text: String,
        work: impl FnOnce(&SecretRoots) -> Completion + Send + 'static,
    ) {
        let (env, tx, held) = (self.env.clone(), self.tx.clone(), self.completions.start());
        let bound = self.bound;
        tokio::spawn(async move {
            let names = match held {
                Some(held) => within(bound, move || {
                    let _held = held;
                    load_roots(&env)
                        .map(|roots| work(&roots))
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default(),
                None => Completion {
                    busy: true,
                    ..Completion::default()
                },
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
    let path = if git && source == Source::PaneRoot {
        walk_git(roots.home(), path)?
    } else {
        path.to_path_buf()
    };
    Some(resolve(&path).and_then(|real| judged(roots, real)))
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

/// The most entries a completion checks against the disk.
const EXAMINED_MAX: usize = COMPLETIONS_MAX * 4;

/// A control character, or a format one (zero width, bidi override, BOM)
/// that would make a name read as something else.
fn unsafe_char(c: char) -> bool {
    let (soft_hyphen, bom) = ('\u{ad}', '\u{feff}');
    // Zero width and direction marks, bidi embeddings and overrides, joiners.
    let format = matches!(c as u32, 0x200b..=0x200f | 0x202a..=0x202e | 0x2060..=0x2069);
    c.is_control() || format || c == soft_hyphen || c == bom
}

/// The names of the directories in `dir` that start with `prefix`, sorted, at
/// most [`COMPLETIONS_MAX`]. Hidden ones only when `prefix` starts with `.`.
/// Nothing is listed unless `dir` is a folder a chat may work in or HOME, and
/// an entry whose real path the tools would refuse (a link into `~/.ssh`) is
/// left out, as is a name with a control or invisible format character. A
/// folder above HOME or outside it (`/home`, `/usr`) lists nothing. At most
/// [`EXAMINED_MAX`] entries are checked; reaching it marks the list truncated.
pub fn complete(roots: &SecretRoots, dir: &Path, prefix: &str) -> Completion {
    let none = Completion::default();
    let Ok(real) = std::fs::canonicalize(dir) else {
        return none;
    };
    let verdict = judged(roots, real.clone()).map(|probed| probed.verdict);
    if !matches!(verdict, Ok(Verdict::Concrete | Verdict::Home)) {
        return none;
    }
    let Ok(entries) = std::fs::read_dir(&real) else {
        return none;
    };
    let hidden = prefix.starts_with('.');
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with(prefix) && (hidden || !name.starts_with('.')))
        .filter(|name| !name.chars().any(unsafe_char))
        .collect();
    names.sort();
    let mut found = Completion::default();
    for (seen, name) in names.into_iter().enumerate() {
        if seen == EXAMINED_MAX {
            found.truncated = true;
            break;
        }
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
