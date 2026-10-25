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
    path::Path,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use super::{Decision, NOT_APPROVED, Outcome, ToolHost, ToolStatus, TurnScope, ended};
use crate::{
    core::ai::{
        chat::review::{BatchReview, ReviewDoc, TOO_LARGE},
        tools::{
            Call, ToolError,
            mcp::refusal_text,
            paths::{REFUSED, SecretRoots, resolve_write},
            registry::Family,
            stage::{BatchId, Entry, Existing, MAX_BYTES, MAX_ENTRIES, StageError, StageKey},
        },
    },
    runtime::tools::{
        ToolEnv,
        fs::load_roots,
        run_over,
        stage::{StageFailed, Stages, disk_bytes},
        staged::StagedView,
    },
};

/// What a sealed batch needs to be answered: its id, and whether the user can
/// approve it at all.
pub(super) struct Sealed {
    id: BatchId,
    reviewable: bool,
}

pub(super) fn lock(stages: &Mutex<Stages>) -> MutexGuard<'_, Stages> {
    stages.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The refusal of a write call the chat did not offer: the text of an unknown
/// tool. The parse already refuses it; this is the second check.
pub(super) fn gate(scope: &TurnScope, call: &Call) -> Result<(), &'static str> {
    let offered = scope.families.contains(Family::Write) && scope.stage_key.is_some();
    if call.is_write() && !offered {
        return Err(refusal_text(ToolError::UnknownTool));
    }
    Ok(())
}

/// Seals the batch of the chat and builds the ask for it. An empty batch, a
/// batch over the caps or one already waiting is an error, and a refused
/// proposal leaves the batch as it was.
pub(super) fn seal(
    host: &ToolHost,
    scope: &TurnScope,
) -> Result<(Sealed, Arc<BatchReview>), String> {
    let key = scope
        .stage_key
        .ok_or(refusal_text(ToolError::UnknownTool))?;
    let mut stages = lock(&host.stages);
    let id = stages
        .batch(key)
        .map(|batch| batch.id())
        .ok_or(StageError::Empty.to_string())?;
    stages.seal(id).map_err(|why| why.to_string())?;
    let batch = stages.batch(key).ok_or(StageError::Stale.to_string())?;
    let review = Arc::new(BatchReview::new(ReviewDoc::build(batch)));
    let reviewable = review.doc().reviewable();
    Ok((Sealed { id, reviewable }, review))
}

/// Runs a call the user does not have to answer: a staging call, or a read
/// tool that sees the staged changes as pending.
pub(super) async fn run(
    host: &ToolHost,
    scope: &TurnScope,
    call: Call,
) -> (Outcome, ToolStatus, String) {
    let env = host.env(&scope.origin);
    if let (true, Some(key)) = (call.is_write(), scope.stage_key) {
        let stages = host.stages.clone();
        let done = tokio::task::spawn_blocking(move || stage(&stages, key, &env, &call)).await;
        return match done {
            Ok(Ok(text)) => ended(ToolStatus::Ok, text.clone(), &text),
            Ok(Err(why)) => ended(ToolStatus::Error, why.clone(), &why),
            Err(_) => ended(ToolStatus::Error, COULD_NOT.to_owned(), COULD_NOT),
        };
    }
    // The view whenever the family is on, so the marker cannot be forged.
    let view = scope
        .stage_key
        .filter(|_| scope.families.contains(Family::Write))
        .map(|key| lock(&host.stages).view(key));
    read(call, env, host, view).await
}

const COULD_NOT: &str = "could not run";

async fn read(
    call: Call,
    env: ToolEnv,
    host: &ToolHost,
    view: Option<StagedView>,
) -> (Outcome, ToolStatus, String) {
    match run_over(call, env, host.limits.clone(), view).await {
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
/// nothing yet. Every refusal is the one refusal text.
fn on_disk(
    stages: &Stages,
    key: StageKey,
    roots: &SecretRoots,
    real: &Path,
) -> Result<Option<Vec<u8>>, String> {
    let made = |batch: &crate::core::ai::tools::stage::Batch| {
        batch.entries().iter().any(|s| {
            s.entry == Entry::Dir && real != s.target.real && real.starts_with(&s.target.real)
        })
    };
    if stages.batch(key).is_some_and(made) {
        return Ok(None);
    }
    disk_bytes(roots, real).map_err(|_| REFUSED.to_owned())
}

fn failed<E: Display>(why: StageFailed<E>) -> String {
    match why {
        StageFailed::Exhausted => COULD_NOT.to_owned(),
        StageFailed::Batch(why) => why.to_string(),
    }
}

/// Stages one call. Blocking: it reads the disk to see what is there.
fn stage(
    stages: &Mutex<Stages>,
    key: StageKey,
    env: &ToolEnv,
    call: &Call,
) -> Result<String, String> {
    let refused = || REFUSED.to_owned();
    let roots = load_roots(env).map_err(|_| refused())?;
    let path = match call {
        Call::CreateDir { path } | Call::WriteFile { path, .. } | Call::EditFile { path, .. } => {
            path
        }
        _ => return Err(refusal_text(ToolError::UnknownTool).to_owned()),
    };
    let target = resolve_write(&roots, &env.origin, path).ok_or_else(refused)?;
    let mut stages = lock(stages);
    let disk = on_disk(&stages, key, &roots, &target.real)?;
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
    Ok(format!(
        "staged {path} (batch: {} of {MAX_ENTRIES} entries, {} of {MAX_BYTES} bytes)",
        batch.entries().len(),
        batch.bytes()
    ))
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
    let Some(Sealed { id, reviewable }) = sealed else {
        return ended(ToolStatus::Error, COULD_NOT.to_owned(), COULD_NOT);
    };
    let denied = |reason: &str| {
        // A batch that is gone already is not an error here.
        let _ = lock(&host.stages).discard(id);
        ended(ToolStatus::Denied, format!("denied: {reason}"), reason)
    };
    match decision {
        Decision::Sent if reviewable => {}
        Decision::Sent => return denied(TOO_LARGE),
        Decision::Denied(reason) => return denied(reason),
        // A batch is only ever sent or denied, never typed or busy.
        Decision::Busy(_) | Decision::Typed(_) => return denied(NOT_APPROVED),
    }
    let env = host.env(&scope.origin);
    let stages = host.stages.clone();
    let done = tokio::task::spawn_blocking(move || {
        let roots = load_roots(&env).map_err(|_| REFUSED.to_owned())?;
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
    let text = result.summary();
    let status = if result.applied() {
        ToolStatus::Ok
    } else {
        // Nothing was written, or the user must look: the batch is closed.
        let _ = lock(&host.stages).discard(id);
        ToolStatus::Error
    };
    let first = text.lines().next().unwrap_or_default().to_owned();
    ended(status, text, &first)
}
