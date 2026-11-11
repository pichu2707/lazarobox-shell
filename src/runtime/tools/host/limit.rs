//! The step before a `simple` worker's batch is reviewed or sealed (W9b, D14,
//! decision K2): a staged file over the line limit asks the user to keep the
//! batch whole (`y`) or split the work (`n` or Esc), which discards the staged
//! batch. The ask is a call of its own, announced before `propose_batch` is,
//! and nothing is reviewed or proposed until it is answered. No approval mode
//! and no timer answers it, and the worker's own clock stops while it waits.
//! The counts come from the staged texts, never from text the worker said.

use std::sync::atomic::Ordering;

use super::{Owed, ToolHost, ToolStatus, TurnScope, files};
use crate::{
    core::ai::{
        chat::engine::{Answer, CallId, Decision, StreamEvent},
        orchestra::limit::limit_ask,
        tools::summary::clip,
    },
    runtime::tools::approver::CANCELLED,
};

/// The row of the ask, a call of its own.
const ROW: &str = "limit";

/// What a batch whose lines cannot be counted is never proposed with.
const NO_COUNT: &str = "could not count the staged lines: nothing is proposed";

/// What the step decides for the batch.
pub(super) enum Limited {
    /// Review and seal the batch whole. The files the user was asked about,
    /// kept or not: a later check does not ask about them again.
    Go(Vec<String>),
    /// The call ends here with this text and nothing is proposed.
    Stop(String),
}

/// Asks about the files of the worker's staged batch that are over the limit
/// and not in `asked`. It runs before the review and again before the seal:
/// a correction or a sibling stages into the batch in between, and a batch
/// that cannot be counted is never proposed unasked.
pub(super) async fn check(host: &ToolHost, scope: &TurnScope, asked: &[String]) -> Limited {
    let (Some(worker), Some(key)) = (&scope.read_scope, scope.stage_key) else {
        return Limited::Go(Vec::new());
    };
    let (stages, env) = (host.stages.clone(), host.env(&scope.origin));
    let taken = tokio::task::spawn_blocking(move || files::staged_counts(&stages, key, &env));
    let Ok(counts) = taken.await else {
        return Limited::Stop(NO_COUNT.to_owned());
    };
    // The staged files the brief does not list ride in the same ask (C).
    let paths = counts.iter().map(|(path, _)| path.clone()).collect();
    let outside = worker.handle.outside_brief(worker.run, worker.worker, paths);
    let Some(listed) = worker.handle.over_limit(worker.run, worker.worker, counts) else {
        return Limited::Stop(NO_COUNT.to_owned());
    };
    let fresh = |path: &String| !asked.contains(path);
    let over: Vec<_> = listed.into_iter().filter(|(p, _)| fresh(p)).collect();
    let outside: Vec<String> = outside.into_iter().filter(fresh).collect();
    if over.is_empty() && outside.is_empty() {
        return Limited::Go(asked.to_vec());
    }
    let asked_now: Vec<String> = asked
        .iter()
        .cloned()
        .chain(over.iter().map(|(path, _)| path.clone()))
        .chain(outside.iter().cloned())
        .collect();
    let label = worker.ask_label();
    let ask = limit_ask(worker.key(), &label, &over, &outside);
    let id = CallId(host.next.fetch_add(1, Ordering::Relaxed));
    // The wait is not the worker's own work: its deadline stops (PR15).
    let _paused = worker.handle.pause(worker.run, worker.worker);
    let pending = scope.approver.ask(id, &ask);
    let Ok(slot) = scope.events.clone().reserve_owned().await else {
        return Limited::Stop(CANCELLED.to_owned());
    };
    if scope.approver.is_over() {
        return Limited::Stop(CANCELLED.to_owned());
    }
    let announce = StreamEvent::ToolCall {
        id,
        name: ROW.to_owned(),
        summary: clip(if over.is_empty() {
            "outside the brief?"
        } else {
            "line limit?"
        }),
        approval: Some(Box::new(ask)),
    };
    if scope.events.send(announce).await.is_err() {
        return Limited::Stop(CANCELLED.to_owned());
    }
    let owed = Owed {
        id,
        slot: Some(slot),
    };
    let not_answered =
        |why: &str| format!("the limit ask was not answered ({why}): nothing is proposed");
    match pending.await {
        Answer::Decided(Decision::Sent) => {
            owed.answer(ToolStatus::Ok, "kept whole");
            Limited::Go(asked_now)
        }
        Answer::Decided(Decision::Denied(crate::app::DECLINED)) => {
            let lost = files::discard_files(&host.stages, key);
            // Only a file over the limit is a split; a decline of files
            // outside the brief is neither a split nor a round (K2).
            let said = if over.is_empty() {
                format!("writes outside the brief declined: {lost} staged files discarded")
            } else {
                let files: Vec<String> = over.into_iter().map(|(path, _)| path).collect();
                worker.handle.split(worker.run, worker.worker, files);
                format!("split chosen: {lost} staged files discarded")
            };
            let line = format!("worker {label}: {said}");
            let _ = scope.events.send(StreamEvent::Notice(line)).await;
            owed.answer(ToolStatus::Denied, &clip(&said));
            Limited::Stop(format!(
                "{said}; write nothing more and end with a short text"
            ))
        }
        // Cancelled or expired: nobody split, and nothing is discarded.
        Answer::Decided(Decision::Denied(why)) => {
            let text = not_answered(why);
            owed.answer(ToolStatus::Denied, &clip(&text));
            Limited::Stop(text)
        }
        _ => {
            let text = not_answered("no user answer");
            owed.answer(ToolStatus::Denied, &clip(&text));
            Limited::Stop(text)
        }
    }
}
