//! The step before a `code` worker's batch review (agent-orchestrator S7b,
//! D16): assess the staged change set and, for a `high` one, ask the user for
//! consent on a wait of its own, then hand the answer to the run manager,
//! which runs the review. The ask is built here from the assessment and the
//! staged ledger, never from text the worker supplied. Nothing in this step
//! approves the batch, and no answer to the consent reaches the worker's own
//! tool call: the consent is announced as a call of its own, with its own id,
//! before `propose_batch` is announced.
//!
//! Never silent: a set that cannot be read, a review that cannot start and a
//! path staged after the review each show a row of their own, and the batch
//! still goes to the user's review.

use std::future::Future;
use std::sync::atomic::Ordering;

use super::{Owed, ToolHost, ToolStatus, TurnScope};
use crate::{
    core::ai::{
        chat::engine::{Answer, CallId, Decision, StreamEvent},
        orchestra::{
            changes::Change,
            rdd::consent_ask,
            review::{Assessment, Tier},
        },
        tools::summary::clip,
    },
    runtime::tools::{Answered, WorkerScope},
};

/// The row of the consent, a call of its own.
const ROW: &str = "review";

/// What the user's `n` to the consent denies with.
const USER_DECLINED: &str = "the user declined";

/// A staged set read on the blocking pool: `None` when the read failed, which
/// is never an empty (and so passive) set.
pub(super) fn joined(taken: Result<Vec<Change>, tokio::task::JoinError>) -> Option<Vec<Change>> {
    taken.ok()
}

/// Reviews the staged change set of `scope`'s worker when it is `high`.
/// Returns when the review ended or was skipped; the caller then seals the
/// batch for the user's review whatever happened here.
pub(super) async fn before_batch(host: &ToolHost, scope: &TurnScope) {
    let Some(key) = scope.stage_key else { return };
    before_batch_with(host, scope, host.staged_changes(key, &scope.origin)).await;
}

/// [`before_batch`] with the first read of the staged set given.
pub(super) async fn before_batch_with(
    host: &ToolHost,
    scope: &TurnScope,
    read: impl Future<Output = Option<Vec<Change>>>,
) {
    let (Some(worker), Some(key)) = (&scope.read_scope, scope.stage_key) else {
        return;
    };
    let Some(changes) = read.await else {
        let why = "could not read the staged changes: they are not reviewed";
        note(host, scope, ToolStatus::Error, why).await;
        return;
    };
    if changes.is_empty() {
        return;
    }
    let assessment = worker.handle.assess(worker.run, &changes);
    let mut reviewed = false;
    if assessment.tier >= Tier::High {
        let Some(answered) = consent(host, scope, worker, &assessment, &changes).await else {
            return;
        };
        let granted = answered == Answered::Granted;
        let started = worker
            .handle
            .review(
                worker.run,
                worker.worker,
                changes.clone(),
                assessment,
                answered,
            )
            .await;
        match started {
            Ok(()) => reviewed = granted,
            Err(why) => {
                let why = format!("review could not start: {why}; the change is not reviewed");
                note(host, scope, ToolStatus::Error, &why).await;
            }
        }
    }
    // What the review looked at is the set it was given: a path that got in
    // since (a sibling worker, a correction past its blockers) was not.
    let Some(now) = host.staged_changes(key, &scope.origin).await else {
        let why = "could not re-read the staged changes: later paths are not reviewed";
        note(host, scope, ToolStatus::Error, why).await;
        return;
    };
    let late: Vec<Change> = now
        .into_iter()
        .filter(|change| !changes.iter().any(|seen| seen.path == change.path))
        .collect();
    if late.is_empty() {
        return;
    }
    let assessment = worker.handle.assess(worker.run, &late);
    if reviewed || assessment.tier >= Tier::High {
        worker
            .handle
            .late(worker.run, worker.worker, late, assessment, reviewed);
    }
}

/// Asks the user, on a call of its own. `None` when nobody can be asked.
async fn consent(
    host: &ToolHost,
    scope: &TurnScope,
    worker: &WorkerScope,
    assessment: &Assessment,
    changes: &[Change],
) -> Option<Answered> {
    let ask = consent_ask(worker.key(), &worker.ask_label(), assessment, changes);
    let id = CallId(host.next.fetch_add(1, Ordering::Relaxed));
    let pending = scope.approver.ask(id, &ask);
    let slot = scope.events.clone().reserve_owned().await.ok()?;
    if scope.approver.is_over() {
        return None;
    }
    let announce = StreamEvent::ToolCall {
        id,
        name: ROW.to_owned(),
        summary: clip("review a high-risk change?"),
        approval: Some(Box::new(ask)),
    };
    scope.events.send(announce).await.ok()?;
    let owed = Owed {
        id,
        slot: Some(slot),
    };
    let answered = match pending.await {
        Answer::Decided(Decision::Sent) => Answered::Granted,
        Answer::Decided(Decision::Denied(USER_DECLINED)) => Answered::Declined,
        // Cancelled or expired: nobody declined, and it is not recorded so.
        Answer::Decided(Decision::Denied(reason)) => Answered::Unanswered(reason.to_owned()),
        Answer::Decided(Decision::Busy(_)) => Answered::Unanswered("busy".to_owned()),
        Answer::Decided(Decision::Typed(_) | Decision::Unprompted) | Answer::Ran(_) => {
            Answered::Unanswered("no user answer".to_owned())
        }
    };
    match &answered {
        Answered::Granted => owed.answer(ToolStatus::Ok, "review started"),
        Answered::Declined => owed.answer(ToolStatus::Denied, "not reviewed"),
        Answered::Unanswered(why) => {
            owed.answer(
                ToolStatus::Denied,
                &clip(&format!("consent not answered ({why})")),
            );
        }
    }
    Some(answered)
}

/// A row of its own that says `summary`, for what the user must see.
async fn note(host: &ToolHost, scope: &TurnScope, status: ToolStatus, summary: &str) {
    let id = CallId(host.next.fetch_add(1, Ordering::Relaxed));
    let Ok(slot) = scope.events.clone().reserve_owned().await else {
        return;
    };
    let announce = StreamEvent::ToolCall {
        id,
        name: ROW.to_owned(),
        summary: clip(summary),
        approval: None,
    };
    if scope.events.send(announce).await.is_err() {
        return;
    }
    let owed = Owed {
        id,
        slot: Some(slot),
    };
    owed.answer(status, &clip(summary));
}
