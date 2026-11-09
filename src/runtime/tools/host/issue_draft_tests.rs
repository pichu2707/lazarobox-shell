//! Spec (issue-draft): the host validates nothing more, asks the user on every
//! call, and runs no `gh`: the draft is handed over by the app on `y`.

use super::{
    files::gate,
    tests::{Scripted, events, fixture, scope},
    *,
};
use crate::core::{
    ai::{
        chat::{
            approval::ApprovalAsk,
            engine::{Decision, StreamEvent, ToolStatus},
        },
        tools::{
            Call, Locality,
            issue_draft::IssueDraft,
            mcp::refusal_text,
            registry::{Family, FamilySet},
        },
    },
    menu::feedback::Kind,
};

fn draft(title: &str) -> IssueDraft {
    IssueDraft {
        kind: Kind::Idea,
        title: title.into(),
        description: "the why and the what".into(),
    }
}

fn on() -> FamilySet {
    FamilySet::LEGACY.with(Family::IssueDraft)
}

fn tools(
    host: &Arc<ToolHost>,
    home: &std::path::Path,
    approver: Arc<dyn Approver>,
) -> (Arc<dyn Dispatch>, tokio::sync::mpsc::Receiver<StreamEvent>) {
    let (mut scope, rx) = scope(Locality::Cloud, home);
    scope.families = on();
    scope.approver = approver;
    (host.dispatch(scope), rx)
}

fn statuses(seen: &[StreamEvent]) -> Vec<ToolStatus> {
    seen.iter()
        .filter_map(|event| match event {
            StreamEvent::ToolResult { status, .. } => Some(*status),
            _ => None,
        })
        .collect()
}

// `y`: the tool returns `ok` at once (D2), saying what happens next, and no
// `gh` was started.
#[tokio::test]
async fn y_returns_ok_with_what_happens_next_and_runs_no_gh() {
    let (_dir, home, host) = fixture();
    let approver = Scripted::new(Decision::Sent);
    let (tools, mut rx) = tools(&host, &home, approver.clone());
    let out = tools.call(Call::IssueDraft(draft("a goal"))).await;
    assert!(!out.is_error, "{out:?}");
    assert_eq!(out.text, READY);
    assert!(out.text.contains("review"), "{}", out.text);
    assert!(out.text.contains("send"), "{}", out.text);
    assert_eq!(
        approver.asked.lock().unwrap().clone(),
        vec![ApprovalAsk::IssueDraft(draft("a goal"))]
    );
    let seen = events(&mut rx);
    assert_eq!(statuses(&seen), vec![ToolStatus::Ok]);
    assert!(
        !seen.iter().any(|e| matches!(e, StreamEvent::GithubStarted)),
        "the draft starts no gh"
    );
}

// `n`, a cancel, a closed chat: `denied` with the reason given.
#[tokio::test]
async fn a_denied_answer_is_denied_and_reaches_the_model() {
    for reason in [
        crate::app::CANCELLED,
        crate::runtime::tools::approver::ENDED,
        "the user declined",
    ] {
        let (_dir, home, host) = fixture();
        let (tools, mut rx) = tools(&host, &home, Scripted::new(Decision::Denied(reason)));
        let out = tools.call(Call::IssueDraft(draft("a goal"))).await;
        assert_eq!(out.text, format!("denied: {reason}"));
        assert_eq!(
            statuses(&events(&mut rx)),
            vec![ToolStatus::Denied],
            "{reason}"
        );
    }
}

// One decline per turn: a later call of the same turn is denied without a
// prompt, and a different title does not reopen the question.
#[tokio::test]
async fn after_a_denied_later_calls_of_the_turn_are_denied_without_asking() {
    let (_dir, home, host) = fixture();
    let approver = Scripted::new(Decision::Denied("the user declined"));
    let (tools, mut rx) = tools(&host, &home, approver.clone());
    let first = tools.call(Call::IssueDraft(draft("one"))).await;
    assert_eq!(first.text, "denied: the user declined");
    let later = tools.call(Call::IssueDraft(draft("two"))).await;
    assert!(later.text.starts_with("denied: "), "{}", later.text);
    assert!(later.text.contains("draft"), "{}", later.text);
    assert_eq!(approver.asked.lock().unwrap().len(), 1, "no second prompt");
    assert_eq!(
        statuses(&events(&mut rx)),
        vec![ToolStatus::Denied, ToolStatus::Denied]
    );
}

// Another turn starts clean: the decline does not outlive its turn.
#[tokio::test]
async fn a_new_turn_asks_again_after_a_decline() {
    let (_dir, home, host) = fixture();
    let approver = Scripted::new(Decision::Denied("the user declined"));
    let (first, _rx) = tools(&host, &home, approver.clone());
    first.call(Call::IssueDraft(draft("one"))).await;
    let (second, _rx2) = tools(&host, &home, approver.clone());
    second.call(Call::IssueDraft(draft("two"))).await;
    assert_eq!(approver.asked.lock().unwrap().len(), 2);
}

// Family gate (second check after the parse): off is an unknown tool.
#[test]
fn the_gate_refuses_a_draft_unless_the_family_is_offered() {
    let off = refusal_text(crate::core::ai::tools::ToolError::UnknownTool);
    let (_dir, home, _host) = fixture();
    let (mut scope, _rx) = scope(Locality::Cloud, &home);
    assert_eq!(
        gate(&scope, &Call::IssueDraft(draft("t"))),
        Err(off),
        "default scope (LEGACY)"
    );
    scope.families = on();
    assert_eq!(gate(&scope, &Call::IssueDraft(draft("t"))), Ok(()));
}

// A worker scope never gets the family, and a draft it could name is an
// unknown tool.
#[tokio::test]
async fn a_worker_scope_cannot_call_the_draft() {
    let (_dir, home, host) = fixture();
    let approver = Scripted::new(Decision::Sent);
    let (mut scope, _rx) = scope(Locality::Cloud, &home);
    scope.approver = approver.clone();
    // A worker's scope is the toolset's, with no draft family.
    scope.families = crate::core::ai::orchestra::Toolset::Code.families();
    let tools = host.dispatch(scope);
    let out = tools.call(Call::IssueDraft(draft("t"))).await;
    assert_eq!(
        out.text,
        refusal_text(crate::core::ai::tools::ToolError::UnknownTool)
    );
    assert!(approver.asked.lock().unwrap().is_empty());
}
