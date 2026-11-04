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

// --- Ollama `*-cloud` while blocked (github-tools, Cloud consent; agent-tools,
// reads in a blocked conversation) ---

mod cloud {
    use std::sync::atomic::Ordering;

    use serde_json::json;

    use super::*;
    use crate::{
        core::ai::chat::approval::ApprovalAsk,
        runtime::tools::host::tests::{Scripted, fixture},
    };

    fn read(path: &str) -> Call {
        Call::ReadFile {
            path: path.into(),
            lines: None,
        }
    }

    fn grep() -> Call {
        crate::core::ai::tools::parse_call_in(
            "grep",
            &json!({"pattern": "hello"}),
            &Default::default(),
            FamilySet::LEGACY.with(Family::Search),
        )
        .unwrap()
    }

    /// A local-locality turn of an Ollama model; `cloud` is its name's verdict,
    /// `blocked` the conversation's flag at the start of the turn.
    async fn asks(cloud: bool, blocked: bool, call: Call) -> Vec<ApprovalAsk> {
        let (_dir, home, host) = fixture();
        let (tx, _rx) = mpsc::channel(64);
        let mut scope = TurnScope::new(tx, Locality::Local, home.clone());
        scope.ollama_cloud = cloud;
        scope.github_read.store(blocked, Ordering::Relaxed);
        let approver = Scripted::new(Decision::Sent);
        scope.approver = approver.clone();
        host.dispatch(scope).call(call).await;
        approver.asked.lock().unwrap().clone()
    }

    #[tokio::test]
    async fn a_blocked_ollama_cloud_chat_asks_for_a_read_and_a_search() {
        for call in [read("note.txt"), grep()] {
            let asked = asks(true, true, call.clone()).await;
            assert_eq!(asked.len(), 1, "{call:?}: {asked:?}");
            match &asked[0] {
                ApprovalAsk::Read { cloud, .. } => assert!(*cloud),
                ApprovalAsk::Search { .. } => {}
                other => panic!("{other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn without_the_block_or_the_cloud_name_nothing_is_asked() {
        for (cloud, blocked) in [(true, false), (false, true), (false, false)] {
            for call in [read("note.txt"), grep()] {
                let asked = asks(cloud, blocked, call.clone()).await;
                assert_eq!(
                    asked,
                    Vec::new(),
                    "cloud {cloud} blocked {blocked}: {call:?}"
                );
            }
        }
    }

    #[tokio::test]
    async fn names_only_tools_stay_free_for_a_blocked_ollama_cloud_chat() {
        let find = Call::FindDir {
            query: "curso".into(),
        };
        let list = Call::ListDir { path: ".".into() };
        for call in [find, list] {
            assert_eq!(asks(true, true, call.clone()).await, Vec::new(), "{call:?}");
        }
    }
}
