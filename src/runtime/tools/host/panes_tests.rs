//! `list_panes` through the host: free on a local engine, asked on a cloud
//! one with the exact text, wrapped as data, and offered only with the pane
//! family.

use super::{
    tests::{Scripted, events, fixture, scope},
    *,
};
use crate::core::ai::tools::registry::Family;

fn lines() -> Vec<String> {
    vec![
        "pane 1 (focused) cwd: /tmp program: nvim prueba.rs".to_owned(),
        "pane 2 cwd: /tmp program: zsh".to_owned(),
    ]
}

// A local engine keeps the text on the machine: no ask.
#[tokio::test]
async fn list_panes_runs_free_on_a_local_engine_and_wraps_the_lines_as_data() {
    let (_dir, home, host) = fixture();
    host.roster().set(lines());
    let (mut turn, mut rx) = scope(Locality::Local, &home);
    let asks = Scripted::new(Decision::Denied("no"));
    turn.approver = asks.clone();
    let out = host.dispatch(turn).call(Call::ListPanes).await;
    assert!(!out.is_error, "{out:?}");
    assert!(out.text.contains("<untrusted-tool-output"), "{out:?}");
    assert!(out.text.contains("program: nvim prueba.rs"), "{out:?}");
    assert!(asks.asked.lock().unwrap().is_empty());
    // The row shows what was sent.
    let shown = events(&mut rx);
    assert!(
        shown.iter().any(|e| matches!(e,
            StreamEvent::ToolResult { summary, .. } if summary.contains("nvim prueba.rs"))),
        "{shown:?}"
    );
}

// A cloud engine sends the text to its provider: the user is asked first and
// the prompt carries exactly the text that is sent.
#[tokio::test]
async fn a_cloud_engine_asks_with_the_exact_text_and_sends_it_after_y() {
    let (_dir, home, host) = fixture();
    host.roster().set(lines());
    let (mut turn, _rx) = scope(Locality::Cloud, &home);
    let asks = Scripted::new(Decision::Sent);
    turn.approver = asks.clone();
    let out = host.dispatch(turn).call(Call::ListPanes).await;
    let asked = asks.asked.lock().unwrap().clone();
    let [ApprovalAsk::Panes { text }] = asked.as_slice() else {
        panic!("{asked:?}");
    };
    assert_eq!(text, &lines().join("\n"));
    assert!(!out.is_error && out.text.contains(text.as_str()), "{out:?}");
}

#[tokio::test]
async fn a_denied_cloud_ask_sends_nothing() {
    let (_dir, home, host) = fixture();
    host.roster().set(lines());
    let (mut turn, _rx) = scope(Locality::Cloud, &home);
    turn.approver = Scripted::new(Decision::Denied("the user declined"));
    let out = host.dispatch(turn).call(Call::ListPanes).await;
    assert!(out.is_error && !out.text.contains("nvim"), "{out:?}");
}

// An empty roster (before the first poll) is said, not an error.
#[tokio::test]
async fn an_empty_roster_says_there_are_no_panes() {
    let (_dir, home, host) = fixture();
    let (turn, _rx) = scope(Locality::Local, &home);
    let out = host.dispatch(turn).call(Call::ListPanes).await;
    assert!(!out.is_error && out.text.contains("no panes"), "{out:?}");
}

// Without the pane family the call is an unknown tool, as for any family.
#[tokio::test]
async fn list_panes_is_refused_without_the_pane_family() {
    let (_dir, home, host) = fixture();
    host.roster().set(lines());
    let (mut turn, _rx) = scope(Locality::Local, &home);
    turn.families = FamilySet::EMPTY.with(Family::Read);
    let out = host.dispatch(turn).call(Call::ListPanes).await;
    assert!(out.is_error && !out.text.contains("nvim"), "{out:?}");
}

// A roster that has not been refreshed for a while says so, in the text the
// user approves and the model gets.
#[tokio::test]
async fn a_stale_roster_adds_its_age_to_the_ask_and_the_result() {
    let (_dir, home, host) = fixture();
    host.roster().set(lines());
    host.roster().backdate(Duration::from_secs(30));
    let (mut turn, _rx) = scope(Locality::Cloud, &home);
    let asks = Scripted::new(Decision::Sent);
    turn.approver = asks.clone();
    let out = host.dispatch(turn).call(Call::ListPanes).await;
    let asked = asks.asked.lock().unwrap().clone();
    let [ApprovalAsk::Panes { text }] = asked.as_slice() else {
        panic!("{asked:?}");
    };
    assert!(text.contains("s old; a lookup may be stuck)"), "{text}");
    assert!(out.text.contains(text.as_str()), "{out:?}");
}
