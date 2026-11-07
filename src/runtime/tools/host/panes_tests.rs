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

fn read_pane(pane: u32) -> Call {
    Call::ReadPane {
        pane,
        scrollback: None,
    }
}

/// Answers every ask with the screen text the user approved.
struct Shown(&'static str);

impl Approver for Shown {
    fn ask(&self, _: CallId, _: &ApprovalAsk) -> BoxFuture<'static, Answer> {
        let text = Answer::Screen(ScreenText(self.0.to_owned()));
        async move { text }.boxed()
    }
}

// Every engine asks, with the pane and no text: the app reads the screen.
#[tokio::test]
async fn read_pane_asks_on_every_engine_without_a_text() {
    for locality in [Locality::Local, Locality::Cloud] {
        let (_dir, home, host) = fixture();
        let (mut turn, _rx) = scope(locality, &home);
        let asks = Scripted::new(Decision::Denied("no"));
        turn.approver = asks.clone();
        let _ = host
            .dispatch(turn)
            .call(Call::ReadPane {
                pane: 4,
                scrollback: Some(9),
            })
            .await;
        assert_eq!(
            asks.asked.lock().unwrap().as_slice(),
            [ApprovalAsk::Screen {
                pane: 4,
                scrollback: Some(9),
                review: None
            }],
            "{locality:?}"
        );
    }
}

// The frozen text the user approved is the result, wrapped as data.
#[tokio::test]
async fn an_approved_screen_is_wrapped_as_data_with_its_line_count() {
    let (_dir, home, host) = fixture();
    let (mut turn, mut rx) = scope(Locality::Cloud, &home);
    turn.approver = Arc::new(Shown("$ ls\nREADME.md\nsrc"));
    let out = host.dispatch(turn).call(read_pane(3)).await;
    assert!(!out.is_error, "{out:?}");
    assert!(out.text.contains("<untrusted-tool-output"), "{out:?}");
    assert!(out.text.contains("$ ls\nREADME.md\nsrc"), "{out:?}");
    let shown = events(&mut rx);
    assert!(
        shown.iter().any(|e| matches!(e,
            StreamEvent::ToolResult { summary, .. } if summary == "3 lines from pane 3")),
        "{shown:?}"
    );
}

// Only the frozen text answers: a mode answer or a plain `y` releases nothing.
#[tokio::test]
async fn a_read_pane_is_denied_unless_the_screen_answers() {
    for answer in [Decision::Sent, Decision::Unprompted, Decision::Typed(1)] {
        let (_dir, home, host) = fixture();
        let (mut turn, _rx) = scope(Locality::Cloud, &home);
        turn.approver = Scripted::new(answer);
        let out = host.dispatch(turn).call(read_pane(2)).await;
        assert!(
            out.is_error && out.text.contains(super::NOT_APPROVED),
            "{answer:?}: {out:?}"
        );
    }
}

// A screen can only answer a read_pane.
#[tokio::test]
async fn a_screen_answer_to_another_call_is_denied() {
    let (_dir, home, host) = fixture();
    let (mut turn, _rx) = scope(Locality::Cloud, &home);
    turn.approver = Arc::new(Shown("secret screen"));
    let out = host.dispatch(turn).call(Call::ListPanes).await;
    assert!(
        out.is_error && !out.text.contains("secret screen"),
        "{out:?}"
    );
}

// A forged call outside the pane family is refused before anything is asked.
#[tokio::test]
async fn read_pane_is_refused_without_the_pane_family() {
    let (_dir, home, host) = fixture();
    let (mut turn, _rx) = scope(Locality::Cloud, &home);
    turn.families = FamilySet::EMPTY.with(Family::Read);
    let asks = Scripted::new(Decision::Sent);
    turn.approver = asks.clone();
    let out = host.dispatch(turn).call(read_pane(2)).await;
    assert!(out.is_error, "{out:?}");
    assert!(asks.asked.lock().unwrap().is_empty());
}

// The second check refuses a screen read for every toolset of a worker or the
// orchestrator, which never get the pane family.
#[test]
fn the_gate_refuses_read_pane_in_a_worker_or_orchestrator_scope() {
    use crate::core::ai::orchestra::Toolset;
    use crate::core::ai::tools::mcp::refusal_text;
    let off = refusal_text(ToolError::UnknownTool);
    for toolset in [Toolset::ReadOnly, Toolset::Code, Toolset::Orchestrator] {
        let (mut turn, _rx) = scope(Locality::Cloud, std::path::Path::new("/"));
        turn.families = toolset.families();
        assert_eq!(files::gate(&turn, &read_pane(2)), Err(off), "{toolset:?}");
    }
}
