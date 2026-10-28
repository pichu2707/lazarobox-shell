//! The review of a commit: the batch view and its coverage gate, answered as
//! a `Commit` ask in every approval mode (git-tools: "commit").

use std::sync::Arc;

use super::chat_live::plain;
use super::tool_approval::{announce_now, chat, decision, live, pending};
use super::*;
use crate::app::tool::DECLINED;
use crate::core::ai::chat::{
    approval::ApprovalAsk,
    engine::{CallId, Decision},
    review::{ARM_DELAY, CommitReview},
    review_view::ReviewView,
};
use crate::core::ai::config::ApprovalMode;
use crate::ui::theme::LazaroboxTheme;
use ratatui::{Terminal, backend::TestBackend};

/// A diff of `rows` added lines, as `git diff --cached` prints it.
fn commit_of(rows: usize) -> ApprovalAsk {
    let body: String = (0..rows).map(|n| format!("+line {n}\n")).collect();
    let diff = format!("diff --git a/a.txt b/a.txt\n@@ -0,0 +1,{rows} @@\n{body}");
    ApprovalAsk::Commit(Arc::new(CommitReview::new(
        "feat: x",
        None,
        diff.as_bytes(),
    )))
}

fn review(a: &App) -> &ReviewView {
    chat(a, id2()).approvals().review().expect("a review view")
}

fn ms(d: std::time::Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap()
}

fn paint(a: &mut App) {
    let theme = LazaroboxTheme::default();
    let mut terminal = Terminal::new(TestBackend::new(80, 25)).unwrap();
    terminal
        .draw(|frame| crate::ui::render(frame, a, &theme))
        .unwrap();
    a.frame_drawn();
}

fn asked(rows: usize) -> (App, u64) {
    let (mut a, turn) = live();
    announce_now(&mut a, turn, 1, commit_of(rows));
    NOW_MS.with(|n| n.set(0));
    paint(&mut a);
    (a, turn)
}

fn read_all(a: &mut App) {
    for _ in 0..500 {
        if review(a).remaining() == 0 {
            return;
        }
        at(a, 100, plain(KeyCode::PageDown));
        paint(a);
    }
}

#[test]
fn a_commit_opens_the_review_in_every_approval_mode_and_never_runs_by_itself() {
    for mode in [ApprovalMode::Ask, ApprovalMode::Auto, ApprovalMode::Run] {
        let (mut a, turn) = live();
        a.panes
            .get_mut(&id2())
            .and_then(PaneState::chat_mut)
            .unwrap()
            .set_approval_mode(mode);
        assert_eq!(
            announce_now(&mut a, turn, 1, commit_of(5)),
            vec![],
            "{mode:?}"
        );
        assert_eq!(pending(&a).map(|p| p.1), Some(Some((turn, CallId(1)))));
        assert!(review(&a).total() > 0, "{mode:?}");
        assert_eq!(a.approval_prompt(id2()), None);
    }
}

// Scenario "Gate": a staged diff of 60 rows of which a few are shown.
#[test]
fn y_does_nothing_while_rows_of_the_diff_are_unseen() {
    let (mut a, _) = asked(60);
    assert!(review(&a).remaining() > 0);
    assert_eq!(at(&mut a, 60_000, key('y')), vec![]);
    assert!(pending(&a).is_some(), "still waiting, nothing committed");
}

#[test]
fn y_approves_only_after_every_row_was_seen_and_the_review_delay() {
    let (mut a, turn) = asked(60);
    read_all(&mut a);
    assert_eq!(at(&mut a, ms(ARM_DELAY) - 1, key('y')), vec![]);
    assert_eq!(
        at(&mut a, ms(ARM_DELAY) + 100, key('y')),
        vec![decision(turn, 1, Decision::Sent)]
    );
    assert_eq!(pending(&a), None);
}

// Scenario "Declined": `n` and Esc end it as denied.
#[test]
fn n_and_esc_deny_the_commit_and_the_transcript_says_so() {
    for rejecting in [key('n'), plain(KeyCode::Esc)] {
        let (mut a, turn) = asked(5);
        assert_eq!(
            at(&mut a, ms(ARM_DELAY), rejecting),
            vec![decision(turn, 1, Decision::Denied(DECLINED))]
        );
        assert_eq!(pending(&a), None);
        let lines: Vec<String> = chat(&a, id2())
            .lines(80)
            .iter()
            .map(|l| l.text.clone())
            .filter(|t| t.contains("rejected"))
            .collect();
        assert_eq!(lines, ["• commit rejected: 1 file, +5 -0"]);
    }
}

#[test]
fn a_generic_approve_never_answers_a_commit() {
    let (mut a, turn) = live();
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .approvals_mut()
        .request_call(commit_of(5), turn, CallId(1), None)
        .unwrap();
    assert_eq!(at(&mut a, 60_000, key('y')), vec![]);
    assert!(pending(&a).is_some(), "only the review answers a commit");
}
