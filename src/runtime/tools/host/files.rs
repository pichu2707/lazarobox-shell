//! The calls the host runs on the files of the chat's root: the read tools,
//! now over the batch the chat has staged, and the write family. Kept out of
//! `host.rs`, which only dispatches.
//!
//! Staging fills an in-memory batch and touches no disk. `propose_batch` seals
//! the batch and asks the user; only an answer of `Sent` applies it, by the id
//! it was sealed with, through `Stages::apply`. What the model is told is
//! counts and paths: never the text of a staged file, a diff row or the disk
//! text an edit was made against (they can hold secrets the model never saw).

use std::{
    fmt::Display,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError, TryLockError},
};

use super::{Decision, NOT_APPROVED, Outcome, ToolHost, ToolStatus, TurnScope, ended};
use crate::{
    core::ai::{
        chat::{
            approval::Reach,
            review::{BatchReview, ReviewDoc, TOO_LARGE},
        },
        orchestra::{
            WorkerKey,
            changes::{Change, StagedText, diff_hash, render_diff},
        },
        tools::{
            Call, ToolError,
            mcp::refusal_text,
            paths::{PathVerdict, REFUSED, SecretRoots, resolve, resolve_write},
            registry::Family,
            stage::{
                BatchId, Entry, Existing, MAX_BYTES, MAX_ENTRIES, StageError, StageKey, Staged,
            },
            summary::clip,
        },
    },
    runtime::tools::{
        ToolEnv, WorkerScope,
        approver::ENDED,
        fs::{Within, load_roots},
        stage::{StageFailed, Stages, Unreadable, disk_bytes},
        staged::StagedView,
    },
};

/// What a sealed batch needs to be answered: its id, and whether the user can
/// approve it at all. It is also the guard of the batch: when it is dropped
/// the batch is discarded, so a call that never got to ask (a closed channel)
/// never leaves the pane's batch sealed. Once the ask is announced the batch
/// is the user's: a call dropped while it waits (a cancelled turn, a CLI
/// timeout) leaves a Proposed orphan for the user to review or discard. The
/// discard is generation-checked: after an answer has closed the batch, or
/// when a newer batch has taken its place, it finds nothing and does nothing.
pub(super) struct Sealed {
    id: BatchId,
    reviewable: bool,
    /// The paths of the batch as a worker's ledger names them
    /// ([`ledger_path`]): what an apply marks applied.
    paths: Vec<String>,
    stages: Arc<Mutex<Stages>>,
    /// The ask is open: dropping now leaves the batch as an orphan.
    keep: bool,
}

impl Sealed {
    /// The ask was announced: the user may now be looking at the batch.
    pub(super) fn announced(&mut self) {
        self.keep = true;
    }

    /// The wait ended with an answer, not with a drop: nothing is left open.
    pub(super) fn resumed(&mut self) {
        self.keep = false;
    }
}

impl Drop for Sealed {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        // A drop may run on an async worker, which must not wait for the
        // store while an apply holds it: then the discard waits on the
        // blocking pool instead.
        let id = self.id;
        match self.stages.try_lock() {
            Ok(mut stages) => drop(stages.discard(id)),
            Err(TryLockError::Poisoned(held)) => drop(held.into_inner().discard(id)),
            Err(TryLockError::WouldBlock) => {
                let stages = self.stages.clone();
                match tokio::runtime::Handle::try_current() {
                    Ok(runtime) => drop(runtime.spawn_blocking(move || lock(&stages).discard(id))),
                    Err(_) => drop(lock(&stages).discard(id)),
                }
            }
        }
    }
}

pub(super) fn lock(stages: &Mutex<Stages>) -> MutexGuard<'_, Stages> {
    stages.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(super) fn lock_set<T>(set: &Mutex<T>) -> MutexGuard<'_, T> {
    set.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The refusal of a write call the chat did not offer: the text of an unknown
/// tool. The parse already refuses it; this is the second check.
pub(super) fn gate(scope: &TurnScope, call: &Call) -> Result<(), &'static str> {
    let offered = scope.families.contains(Family::Write) && scope.stage_key.is_some();
    if (call.is_write() && !offered)
        || (call.is_exec() && !scope.families.contains(Family::Exec))
        || (call.is_git() && !scope.families.contains(Family::Git))
        || (call.is_git_write() && !scope.families.contains(Family::GitWrite))
    {
        return Err(refusal_text(ToolError::UnknownTool));
    }
    Ok(())
}

/// Seals the batch of the chat and builds the ask for it. An empty batch, a
/// batch over the caps or one already waiting is an error, and a refused
/// proposal leaves the batch as it was.
///
/// The store lock is taken on the blocking pool, not on the async worker: an
/// apply can hold it while it writes.
pub(super) async fn seal(
    host: &ToolHost,
    scope: &TurnScope,
) -> Result<(Sealed, Arc<BatchReview>), String> {
    let key = scope
        .stage_key
        .ok_or(refusal_text(ToolError::UnknownTool))?;
    let stages = host.stages.clone();
    // A worker's batch is headed by its label in the review (S7).
    let label = match scope.read_scope.as_ref() {
        None => None,
        Some(worker) => {
            // The receipt line of the review that covers the set being
            // sealed (S7b); a set that cannot be read has no line.
            let current = host.staged_changes(key, &scope.origin).await;
            let receipt =
                current.and_then(|set| worker.handle.receipt_line(worker.run, worker.worker, &set));
            Some((worker.ask_label(), worker.key(), receipt))
        }
    };
    let env = host.env(&scope.origin);
    tokio::task::spawn_blocking(move || seal_in(stages, key, label, &env))
        .await
        .unwrap_or_else(|_| Err(COULD_NOT.to_owned()))
}

fn seal_in(
    shared: Arc<Mutex<Stages>>,
    key: StageKey,
    label: Option<(String, WorkerKey, Option<String>)>,
    env: &ToolEnv,
) -> Result<(Sealed, Arc<BatchReview>), String> {
    let mut stages = lock(&shared);
    let id = stages
        .batch(key)
        .map(|batch| batch.id())
        .ok_or(StageError::Empty.to_string())?;
    stages.seal(id).map_err(|why| why.to_string())?;
    let batch = stages.batch(key).ok_or(StageError::Stale.to_string())?;
    let doc = ReviewDoc::build(batch);
    let review = Arc::new(match label {
        Some((label, key, receipt)) => BatchReview::labelled(doc, label, key).with_receipt(receipt),
        None => BatchReview::new(doc),
    });
    let reviewable = review.doc().reviewable();
    let paths = batch
        .entries()
        .iter()
        .map(|staged| ledger_path(env, &staged.target.real))
        .collect();
    drop(stages);
    Ok((
        Sealed {
            id,
            reviewable,
            paths,
            stages: shared,
            keep: false,
        },
        review,
    ))
}

/// Runs a call the user does not have to answer: a staging call, or a read
/// tool that sees the staged changes as pending.
pub(super) async fn run(
    host: &ToolHost,
    scope: &TurnScope,
    call: Call,
) -> (Outcome, ToolStatus, String) {
    run_within(host, scope, call, Within::Anywhere).await
}

/// [`run`] with a `read_file` kept [`Within`] a bound: a read the approval
/// mode let go because it stayed in the root is kept there by the read.
pub(super) async fn run_within(
    host: &ToolHost,
    scope: &TurnScope,
    call: Call,
    within: Within,
) -> (Outcome, ToolStatus, String) {
    let env = host.env(&scope.origin);
    if let (true, Some(key)) = (call.is_write(), scope.stage_key) {
        let stages = host.stages.clone();
        let worker = scope.read_scope.clone();
        let done =
            tokio::task::spawn_blocking(move || stage(&stages, key, &env, &call, worker.as_ref()))
                .await;
        return match done {
            Ok(Ok(text)) => ended(ToolStatus::Ok, text.clone(), &text),
            Ok(Err(why)) => ended(ToolStatus::Error, why.clone(), &why),
            Err(_) => ended(ToolStatus::Error, COULD_NOT.to_owned(), COULD_NOT),
        };
    }
    // The view whenever the family is on, so the marker cannot be forged.
    // A read-only review worker of a run sees the staged view too (S7b).
    let view = match scope
        .stage_key
        .filter(|_| scope.families.contains(Family::Write) || scope.read_scope.is_some())
    {
        None => None,
        Some(key) => {
            let stages = host.stages.clone();
            let taken = tokio::task::spawn_blocking(move || lock(&stages).view(key)).await;
            match taken {
                Ok(view) => Some(view),
                Err(_) => return ended(ToolStatus::Error, COULD_NOT.to_owned(), COULD_NOT),
            }
        }
    };
    read(call, env, host, view, within).await
}

const COULD_NOT: &str = "could not run";

/// The last line of an applied batch's result: a small model tends to stop
/// after the review, with the user's other steps undone (#439).
const CONTINUE: &str = "Continue with any step the user asked for that is not finished yet.";

/// Where a read of `input` leads, for its ask: the path guard's verdict, and
/// whether the real path (every link followed) is inside the real root of the
/// chat, and that root is concrete (see [`concrete_root`]). Read on the
/// blocking pool; a failure is [`Reach::Other`], which every mode that cares
/// about it prompts for. Only a fact for the ask: a read the mode lets go is
/// kept in the root by the read itself ([`Within::Root`]).
pub(super) async fn reach(host: &ToolHost, scope: &TurnScope, input: &str) -> Reach {
    let env = host.env(&scope.origin);
    let input = input.to_owned();
    tokio::task::spawn_blocking(move || {
        let Ok(roots) = load_roots(&env) else {
            return Reach::Refused;
        };
        match (
            resolve(&roots, &env.origin, &input),
            concrete_root(&env, &roots),
        ) {
            (PathVerdict::Refused, _) => Reach::Refused,
            (PathVerdict::Allowed(real), Some(root)) if real.starts_with(&root) => Reach::Root,
            (PathVerdict::Allowed(real), None) if in_root(&env, &real) => Reach::Broad,
            (PathVerdict::Allowed(_), _) => Reach::Other,
        }
    })
    .await
    .unwrap_or(Reach::Other)
}

/// Where a search of the chat's root leads, for its ask: [`Reach::Root`] for
/// a concrete root, [`Reach::Broad`] for HOME or a folder above it. Read on
/// the blocking pool; a failure is [`Reach::Other`].
pub(super) async fn search_reach(host: &ToolHost, scope: &TurnScope) -> Reach {
    let env = host.env(&scope.origin);
    tokio::task::spawn_blocking(move || {
        let Ok(roots) = load_roots(&env) else {
            return Reach::Other;
        };
        match concrete_root(&env, &roots) {
            Some(_) => Reach::Root,
            None if env.origin.canonicalize().is_ok() => Reach::Broad,
            None => Reach::Other,
        }
    })
    .await
    .unwrap_or(Reach::Other)
}

/// The real root of the chat when it is concrete enough to keep a read in:
/// not HOME and not a folder that holds HOME (`/` among them). `None` too
/// when the root cannot be resolved.
fn concrete_root(env: &ToolEnv, roots: &SecretRoots) -> Option<PathBuf> {
    let root = env.origin.canonicalize().ok()?;
    (!roots.home().starts_with(&root)).then_some(root)
}

/// Whether the resolved `real` path is the chat's root or under it, compared
/// with the root resolved too.
fn in_root(env: &ToolEnv, real: &Path) -> bool {
    env.origin
        .canonicalize()
        .is_ok_and(|root| real.starts_with(root))
}

async fn read(
    call: Call,
    env: ToolEnv,
    host: &ToolHost,
    view: Option<StagedView>,
    within: Within,
) -> (Outcome, ToolStatus, String) {
    match crate::runtime::tools::run_within(call, env, host.limits.clone(), view, within).await {
        Ok(text) => {
            let summary = format!("{} bytes", text.len());
            ended(ToolStatus::Ok, text, &summary)
        }
        Err(failure) => {
            let reason = failure.message();
            ended(ToolStatus::Error, reason.to_owned(), reason)
        }
    }
}

/// What the disk holds at `real`, for staging. A folder the batch makes holds
/// nothing yet.
///
/// `probed` is the disk read, made before the lock was taken. A folder the
/// batch makes wins over it. If the batch changed since the read, the apply
/// preflight catches the drift.
fn on_disk(
    stages: &Stages,
    key: StageKey,
    real: &Path,
    probed: Result<Option<Vec<u8>>, Unreadable>,
) -> Result<Option<Vec<u8>>, Unreadable> {
    let made = |batch: &crate::core::ai::tools::stage::Batch| {
        batch.entries().iter().any(|s| {
            s.entry == Entry::Dir && real != s.target.real && real.starts_with(&s.target.real)
        })
    };
    if stages.batch(key).is_some_and(made) {
        return Ok(None);
    }
    probed
}

/// The chat's folder as a write refusal names it: `$HOME` shown as `~`.
fn chat_folder(env: &ToolEnv) -> String {
    let home = env
        .home
        .as_deref()
        .map(Path::new)
        .filter(|home| home.parent().is_some());
    let shown = match home.and_then(|home| env.origin.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => env.origin.display().to_string(),
    };
    // A folder name is the user's, but a control character in it must not
    // reach the model's text as one.
    shown
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

/// The one text of every write refusal, with the chat's folder, so a model
/// learns where its paths start (local-model-writes D2). It is the same for
/// every reason: a model cannot tell an excluded path from a missing one.
fn refused_in(env: &ToolEnv) -> String {
    format!(
        "{REFUSED} (paths are relative to the chat's folder {})",
        chat_folder(env)
    )
}

/// The refusal of every write when the chat's folder is itself too broad
/// (HOME, a folder above it, `/`). The reason is about the folder, not a path,
/// so it leaks nothing about which paths are excluded, and it tells the model
/// to stop retrying and ask for a project folder.
fn broad_folder(env: &ToolEnv, roots: &SecretRoots) -> Option<String> {
    let root = env
        .origin
        .is_absolute()
        .then(|| env.origin.canonicalize().ok())??;
    roots.home().starts_with(&root).then(|| {
        format!(
            "{REFUSED}: the chat's folder is {}, where files cannot be written; ask the user to run /cd <project folder>",
            chat_folder(env)
        )
    })
}

/// The refusal of a write whose parent folder does not exist, for a path the
/// policy already allowed (so `list_dir` would show the same). `None` when
/// the parent is there or anything else is wrong with it.
fn missing_parent(env: &ToolEnv, path: &str, real: &Path) -> Option<String> {
    match real.parent()?.symlink_metadata() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        _ => return None,
    }
    let shown = Path::new(path).parent()?.to_str()?.trim_end_matches('/');
    (!shown.is_empty()).then(|| {
        format!(
            "{REFUSED}: the folder {shown}/ does not exist (paths are relative to the chat's folder {})",
            chat_folder(env)
        )
    })
}

fn failed<E: Display>(why: StageFailed<E>) -> String {
    match why {
        StageFailed::Exhausted => COULD_NOT.to_owned(),
        StageFailed::Batch(why) => why.to_string(),
    }
}

/// Stages one call. Blocking: it reads the disk to see what is there. A
/// worker's write must also stay in its run's read scope, and what it staged
/// goes to its change ledger (S7).
fn stage(
    stages: &Mutex<Stages>,
    key: StageKey,
    env: &ToolEnv,
    call: &Call,
    worker: Option<&WorkerScope>,
) -> Result<String, String> {
    let (text, change) = stage_as(stages, key, env, call, worker, disk_bytes)?;
    if let Some(worker) = worker {
        worker.handle.staged(worker.run, worker.worker, change);
    }
    Ok(text)
}

/// [`stage`] of a chat, with the disk read given, so a test can watch when it
/// happens. Only the Linux and macOS test uses it.
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
pub(super) fn stage_with_probe(
    stages: &Mutex<Stages>,
    key: StageKey,
    env: &ToolEnv,
    call: &Call,
    probe: impl FnOnce(&SecretRoots, &Path) -> Result<Option<Vec<u8>>, Unreadable>,
) -> Result<String, String> {
    stage_as(stages, key, env, call, None, probe).map(|(text, _)| text)
}

/// The path a ledger names: relative to the chat's real root, one sanitized
/// line; the real path itself (sanitized) when it is not under the root.
fn ledger_path(env: &ToolEnv, real: &Path) -> String {
    let root = env.origin.canonicalize().ok();
    let rel = root
        .as_deref()
        .and_then(|root| real.strip_prefix(root).ok());
    match rel {
        Some(rel) if rel.as_os_str().is_empty() => ".".to_owned(),
        Some(rel) => clip(&rel.to_string_lossy()),
        None => clip(&real.to_string_lossy()),
    }
}

/// What a worker's ledger records of a staged path: its size and diff hash.
fn ledger_change(env: &ToolEnv, staged: &Staged) -> Change {
    let (size, hash) = match &staged.entry {
        Entry::Dir => (None, diff_hash(None, "")),
        Entry::File { text, base } => (Some(text.len() as u64), diff_hash(base.as_deref(), text)),
    };
    Change {
        path: ledger_path(env, &staged.target.real),
        size,
        hash,
        applied: false,
    }
}

/// What a review worker's read of a path came to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum ReviewRead {
    Allowed,
    /// Refused, and not a file of the staged set: a probe (`.git`, a path
    /// outside the scope, an absolute path).
    Refused,
    /// Refused although it is a file of the staged set: the review cannot
    /// have read what it was asked to review.
    RefusedStaged,
}

/// Whether a review worker may read `input` through the staged view: its
/// write target (which exists on disk or only in the batch) resolves inside
/// the chat's root and under one of the run's scope roots, and no secret rule
/// refuses it. The read itself is kept in the chat root by the caller.
pub(super) async fn review_reach(host: &ToolHost, scope: &TurnScope, input: &str) -> ReviewRead {
    let Some(worker) = scope.read_scope.clone() else {
        return ReviewRead::Refused;
    };
    let (env, input) = (host.env(&scope.origin), input.to_owned());
    let (stages, key) = (host.stages.clone(), scope.stage_key);
    let check = tokio::task::spawn_blocking(move || {
        let roots = load_roots(&env).ok()?;
        let target = resolve_write(&roots, &env.origin, &input)?;
        if worker
            .handle
            .allows_write(worker.run, worker.worker, &target.real)
        {
            return Some(ReviewRead::Allowed);
        }
        let staged = key.is_some_and(|key| {
            lock(&stages).batch(key).is_some_and(|batch| {
                batch
                    .entries()
                    .iter()
                    .any(|staged| staged.target.real == target.real)
            })
        });
        Some(if staged {
            ReviewRead::RefusedStaged
        } else {
            ReviewRead::Refused
        })
    });
    check.await.ok().flatten().unwrap_or(ReviewRead::Refused)
}

/// The change set of the batch of `key`, as a worker's ledger names it.
pub(super) fn staged_changes(stages: &Mutex<Stages>, key: StageKey, env: &ToolEnv) -> Vec<Change> {
    let stages = lock(stages);
    let Some(batch) = stages.batch(key) else {
        return Vec::new();
    };
    batch
        .entries()
        .iter()
        .map(|staged| ledger_change(env, staged))
        .collect()
}

/// The diff text of the batch of `key` for a review prompt: the files `allow`
/// lets a review read, as a bounded unified diff, and the others named only.
/// Those are the files the review's read tools show it through the staged
/// view, so this hands the prompt nothing the tools would refuse.
pub(super) fn staged_diff(
    stages: &Mutex<Stages>,
    key: StageKey,
    env: &ToolEnv,
    allow: &dyn Fn(&Path) -> bool,
) -> String {
    let stages = lock(stages);
    let Some(batch) = stages.batch(key) else {
        return String::new();
    };
    let (mut shown, mut skipped) = (Vec::new(), Vec::new());
    for staged in batch.entries() {
        let Entry::File { text, base } = &staged.entry else {
            continue;
        };
        let path = ledger_path(env, &staged.target.real);
        if allow(&staged.target.real) {
            shown.push(StagedText {
                path,
                base: base.clone(),
                text: text.clone(),
            });
        } else {
            skipped.push(path);
        }
    }
    render_diff(&shown, &skipped)
}

/// [`stage`] with the disk read given: the text for the model and the change
/// staged. A worker's write outside its run's read scope is refused with the
/// one refusal text, before the disk is read.
fn stage_as(
    stages: &Mutex<Stages>,
    key: StageKey,
    env: &ToolEnv,
    call: &Call,
    worker: Option<&WorkerScope>,
    probe: impl FnOnce(&SecretRoots, &Path) -> Result<Option<Vec<u8>>, Unreadable>,
) -> Result<(String, Change), String> {
    let refused = || refused_in(env);
    let roots = load_roots(env).map_err(|_| refused())?;
    if let Some(broad) = broad_folder(env, &roots) {
        return Err(broad);
    }
    let path = match call {
        Call::CreateDir { path } | Call::WriteFile { path, .. } | Call::EditFile { path, .. } => {
            path
        }
        _ => return Err(refusal_text(ToolError::UnknownTool).to_owned()),
    };
    let target = resolve_write(&roots, &env.origin, path).ok_or_else(refused)?;
    // No scope pre-approves a write, but none outside the scope is staged.
    if let Some(worker) = worker
        && !worker
            .handle
            .allows_write(worker.run, worker.worker, &target.real)
    {
        return Err(refused());
    }
    // The disk is read before the lock is taken: the lock covers memory only.
    let probed = probe(&roots, &target.real);
    let mut stages = lock(stages);
    let disk = on_disk(&stages, key, &target.real, probed)
        .map_err(|_| missing_parent(env, path, &target.real).unwrap_or_else(refused))?;
    let existing = match &disk {
        None => Existing::Absent,
        Some(bytes) => Existing::Present {
            size: bytes.len() as u64,
            bytes,
        },
    };
    match call {
        // A folder that is already there can never be created by the batch.
        Call::CreateDir { .. } if disk.is_some() => return Err(refused()),
        Call::CreateDir { .. } => stages
            .stage_with(key, |b| b.stage_dir(&target))
            .map_err(failed),
        Call::WriteFile { content, .. } => stages
            .stage_with(key, |b| b.stage_file(&target, content, existing))
            .map_err(failed),
        Call::EditFile { old, new, .. } => stages
            .stage_with(key, |b| b.edit_file(&target, old, new, existing))
            .map_err(failed),
        _ => return Err(refusal_text(ToolError::UnknownTool).to_owned()),
    }?;
    let batch = stages.batch(key).ok_or_else(refused)?;
    let staged = batch
        .entries()
        .iter()
        .find(|staged| staged.target.real == target.real)
        .ok_or_else(refused)?;
    let change = ledger_change(env, staged);
    let text = format!(
        "staged {path} (batch: {} of {MAX_ENTRIES} entries, {} of {MAX_BYTES} bytes)",
        batch.entries().len(),
        batch.bytes()
    );
    Ok((text, change))
}

/// Ends the call of `propose_batch` with the user's answer. Only `Sent` on a
/// reviewable batch applies it, by the id it was sealed with; any other answer
/// drops the batch, and so does an apply that wrote nothing, so the model can
/// stage again.
pub(super) async fn answered(
    host: &ToolHost,
    scope: &TurnScope,
    sealed: Option<Sealed>,
    decision: Decision,
) -> (Outcome, ToolStatus, String) {
    let Some(mut sealed) = sealed else {
        return ended(ToolStatus::Error, COULD_NOT.to_owned(), COULD_NOT);
    };
    // Held to the end: any way out of here closes the batch (see `Sealed`),
    // except an answer that is no user's: the turn ended with the ask open.
    sealed.keep = matches!(decision, Decision::Denied(ENDED));
    let (id, reviewable) = (sealed.id, sealed.reviewable);
    let paths = std::mem::take(&mut sealed.paths);
    // `sealed` discards the batch when it goes out of scope.
    let denied = |reason: &str| ended(ToolStatus::Denied, format!("denied: {reason}"), reason);
    match decision {
        Decision::Sent if reviewable => {}
        Decision::Sent => return denied(TOO_LARGE),
        Decision::Denied(reason) => return denied(reason),
        // A batch is only ever sent or denied by the user, never typed, busy
        // or answered by the approval mode.
        Decision::Busy(_) | Decision::Typed(_) | Decision::Unprompted => {
            return denied(NOT_APPROVED);
        }
    }
    let env = host.env(&scope.origin);
    let stages = host.stages.clone();
    let done = tokio::task::spawn_blocking(move || {
        let roots = load_roots(&env).map_err(|_| refused_in(&env))?;
        lock(&stages)
            .apply(id, &roots)
            .map_err(|why| why.to_string())
    })
    .await;
    let result = match done {
        Ok(Ok(result)) => result,
        Ok(Err(why)) => return ended(ToolStatus::Error, why.clone(), &why),
        Err(_) => return ended(ToolStatus::Error, COULD_NOT.to_owned(), COULD_NOT),
    };
    let mut text = result.summary();
    // Only what the user approved and the apply wrote counts as applied.
    if let (true, Some(worker)) = (result.applied(), &scope.read_scope) {
        worker.handle.applied(worker.run, worker.worker, &paths);
    }
    let status = if result.applied() {
        text = format!("{text}\n{CONTINUE}");
        ToolStatus::Ok
    } else {
        // Nothing was written, or the user must look: `sealed` closes the batch.
        ToolStatus::Error
    };
    let first = text.lines().next().unwrap_or_default().to_owned();
    ended(status, text, &first)
}
