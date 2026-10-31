//! The step before a `code` worker's batch review (agent-orchestrator S7b,
//! D16): assess the staged change set and, for a `high` one, ask the user for
//! consent on a wait of its own, then hand the answer to the run manager,
//! which runs the review. The ask is built here from the assessment and the
//! staged ledger, never from text the worker supplied. Nothing in this step
//! approves the batch, and no answer to the consent reaches the worker's own
//! tool call: the consent is announced as a call of its own, with its own id,
//! before `propose_batch` is announced.

use std::sync::atomic::Ordering;

use super::{Owed, ToolHost, ToolStatus, TurnScope};
use crate::{
    core::ai::{
        chat::engine::{Answer, CallId, Decision, StreamEvent},
        orchestra::{
            changes::Change,
            rdd::{Consent, consent_ask},
            review::Tier,
        },
        tools::{stage::StageKey, summary::clip},
    },
    runtime::tools::approver::ENDED,
};
/// The row of the consent, a call of its own.
const ROW: &str = "review";

/// Reviews the staged change set of `scope`'s worker when it is `high`.
/// Returns when the review ended or was skipped; the caller then seals the
/// batch for the user's review whatever happened here.
pub(super) async fn before_batch(host: &ToolHost, scope: &TurnScope) {
    let (Some(worker), Some(key)) = (&scope.read_scope, scope.stage_key) else {
        return;
    };
    let changes = staged_changes(host, scope, key).await;
    if changes.is_empty() {
        return;
    }
    let assessment = worker.handle.assess(worker.run, &changes);
    if assessment.tier < Tier::High {
        return;
    }
    let ask = consent_ask(worker.key(), &worker.ask_label(), &assessment, &changes);
    let id = CallId(host.next.fetch_add(1, Ordering::Relaxed));
    let pending = scope.approver.ask(id, &ask);
    let Ok(slot) = scope.events.clone().reserve_owned().await else {
        return;
    };
    if scope.approver.is_over() {
        return;
    }
    let announce = StreamEvent::ToolCall {
        id,
        name: ROW.to_owned(),
        summary: clip("review a high-risk change?"),
        approval: Some(Box::new(ask)),
    };
    if scope.events.send(announce).await.is_err() {
        return;
    }
    let owed = Owed {
        id,
        slot: Some(slot),
    };
    let consent = match pending.await {
        Answer::Decided(Decision::Sent) => Consent::Granted,
        // The turn ended with the ask open: nobody answered, nothing runs.
        Answer::Decided(Decision::Denied(ENDED)) => {
            owed.answer(ToolStatus::Denied, "cancelled");
            return;
        }
        // The user's `n`, and anything no user typed: the review does not
        // run and the batch goes on to the user's review, unreviewed.
        _ => Consent::Declined,
    };
    let (status, row) = match consent {
        Consent::Granted => (ToolStatus::Ok, "review started"),
        Consent::Declined => (ToolStatus::Denied, "not reviewed"),
    };
    owed.answer(status, row);
    worker
        .handle
        .review(worker.run, worker.worker, changes, assessment, consent)
        .await;
}

/// The change set of the pane's batch, as a worker's ledger names it.
async fn staged_changes(host: &ToolHost, scope: &TurnScope, key: StageKey) -> Vec<Change> {
    host.staged_changes(key, &scope.origin).await
}
