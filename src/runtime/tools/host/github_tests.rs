//! Spec (github-tools, Availability): until the family is offered, a GitHub
//! call is an unknown tool and nothing runs. This slice has no host arm, so
//! even with the family on nothing starts `gh`.

use std::path::PathBuf;

use tokio::sync::mpsc;

use super::{exec_tests::world_with, files::gate, *};
use crate::core::ai::tools::{
    github::{GhCall, State},
    mcp::refusal_text,
    registry::{Family, FamilySet},
};

fn list() -> Call {
    Call::Github(GhCall::IssueList {
        state: State::Open,
        limit: 20,
    })
}

fn scope(families: FamilySet) -> TurnScope {
    let (tx, _rx) = mpsc::channel(1);
    let mut scope = TurnScope::new(tx, Locality::Local, PathBuf::from("/"));
    scope.families = families;
    scope
}

// "Model cannot enable them": the second check refuses what the parse did.
#[test]
fn the_gate_refuses_a_github_call_unless_the_family_is_offered() {
    let off = refusal_text(ToolError::UnknownTool);
    for families in [
        FamilySet::LEGACY,
        FamilySet::LEGACY.with(Family::Git).with(Family::Write),
    ] {
        assert_eq!(gate(&scope(families), &list()), Err(off));
    }
    let on = FamilySet::LEGACY.with(Family::Github);
    assert_eq!(gate(&scope(on), &list()), Ok(()));
    // The other families' calls keep their own answers.
    assert_eq!(gate(&scope(on), &Call::ListPanes), Ok(()));
}

#[tokio::test]
async fn a_github_call_with_the_family_off_returns_an_unknown_tool_error() {
    let w = world_with(|exec| exec);
    let (tx, _rx) = mpsc::channel(64);
    let scope = TurnScope::new(tx, Locality::Local, w.proj.clone());
    let out = w.host.dispatch(scope).call(list()).await;
    assert!(out.is_error, "{out:?}");
    assert_eq!(out.text, refusal_text(ToolError::UnknownTool));
}
