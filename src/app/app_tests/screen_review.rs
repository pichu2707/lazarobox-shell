//! `read_pane` in the app: the screen is read and frozen when the ask arrives,
//! reviewed in the batch view behind its coverage gate, and sent only as the
//! frozen text (agent-tools and chat-pane: "read_pane").

use super::chat_live::plain;
use super::commit_review::{ms, paint, read_all, review};
use super::tool_approval::{announce_now, chat, decision, live, pending, shell_output};
use super::*;
use crate::app::tool::DECLINED;
use crate::core::ai::chat::{
    approval::ApprovalAsk,
    engine::{CallId, Decision, ScreenText, ToolStatus},
    review::ARM_DELAY,
};
use crate::core::ai::config::ApprovalMode;

fn screen_ask(pane: u32, scrollback: Option<u32>) -> ApprovalAsk {
    ApprovalAsk::Screen {
        pane,
        scrollback,
        review: None,
    }
}

/// The review the pending ask holds, with its frozen text.
fn frozen(a: &App) -> String {
    match pending(a) {
        Some((
            ApprovalAsk::Screen {
                review: Some(review),
                ..
            },
            _,
        )) => review.text().to_owned(),
        other => panic!("no screen review pending: {other:?}"),
    }
}

/// The lines of the transcript that mention `what`.
fn lines_with(a: &App, what: &str) -> Vec<String> {
    chat(a, id2())
        .lines(80)
        .iter()
        .map(|l| l.text.clone())
        .filter(|t| t.contains(what))
        .collect()
}

/// A live chat with `rows` numbered lines on the shell (pane 1), the pane
/// read with `scrollback` lines of history, and the first frame drawn.
fn asked(rows: usize, scrollback: Option<u32>) -> (App, u64) {
    let (mut a, turn) = live();
    let body: String = (0..rows).map(|n| format!("line {n}\r\n")).collect();
    shell_output(&mut a, body.as_bytes());
    announce_now(&mut a, turn, 1, screen_ask(1, scrollback));
    NOW_MS.with(|n| n.set(0));
    paint(&mut a);
    (a, turn)
}

fn sent(turn: u64, text: String) -> Effect {
    Effect::ToolScreen {
        pane: id2(),
        turn,
        call: CallId(1),
        text: ScreenText(text),
    }
}

#[test]
fn the_ask_opens_the_review_of_the_frozen_screen_in_every_mode() {
    for mode in [ApprovalMode::Ask, ApprovalMode::Auto, ApprovalMode::Run] {
        let (mut a, turn) = live();
        chat_mut(&mut a).set_approval_mode(mode);
        shell_output(&mut a, b"$ ls\r\nREADME.md\r\n");
        assert_eq!(announce_now(&mut a, turn, 1, screen_ask(1, None)), vec![]);
        assert_eq!(pending(&a).map(|p| p.1), Some(Some((turn, CallId(1)))));
        assert!(frozen(&a).contains("README.md"), "{mode:?}");
        assert!(review(&a).total() > 0, "{mode:?}");
        assert_eq!(a.approval_prompt(id2()), None);
    }
}

#[test]
fn y_is_locked_until_every_line_was_seen_then_sends_the_frozen_text() {
    let (mut a, turn) = asked(60, Some(100));
    assert!(review(&a).remaining() > 0);
    assert_eq!(at(&mut a, 60_000, key('y')), vec![]);
    assert!(pending(&a).is_some(), "still waiting, nothing sent");
    let text = frozen(&a);
    assert!(text.contains("line 59"), "{text}");
    read_all(&mut a);
    assert_eq!(
        at(&mut a, ms(ARM_DELAY) + 60_000, key('y')),
        vec![sent(turn, text)]
    );
    assert_eq!(pending(&a), None);
}

#[test]
fn what_is_sent_is_what_was_frozen_after_the_pane_writes_and_closes() {
    let (mut a, turn) = asked(5, None);
    let text = frozen(&a);
    shell_output(&mut a, b"changed after the freeze\r\n");
    assert_eq!(frozen(&a), text);
    a.panes.remove(&PaneId::FIRST);
    read_all(&mut a);
    let effects = at(&mut a, ms(ARM_DELAY) + 60_000, key('y'));
    assert_eq!(effects, vec![sent(turn, text.clone())]);
    assert!(!text.contains("changed after"), "{text}");
}

#[test]
fn n_and_esc_deny_and_say_so() {
    for rejecting in [key('n'), plain(KeyCode::Esc)] {
        let (mut a, turn) = asked(5, None);
        assert_eq!(
            at(&mut a, ms(ARM_DELAY), rejecting),
            vec![decision(turn, 1, Decision::Denied(DECLINED))]
        );
        assert_eq!(pending(&a), None);
        assert_eq!(
            lines_with(&a, "screen read"),
            ["• screen read denied: pane 1"]
        );
    }
}

#[test]
fn a_read_that_times_out_says_it_was_denied() {
    let (mut a, turn) = asked(5, None);
    super::tool_approval::result(&mut a, turn, 1, ToolStatus::Denied);
    assert_eq!(pending(&a), None);
    assert_eq!(
        lines_with(&a, "screen read"),
        ["• screen read denied: pane 1"]
    );
}

#[test]
fn the_chats_own_pane_and_an_unknown_one_are_denied_without_a_prompt() {
    for number in [2, 9] {
        let (mut a, turn) = live();
        assert_eq!(
            announce_now(&mut a, turn, 1, screen_ask(number, None)),
            vec![decision(turn, 1, Decision::Denied("the pane is not open"))],
            "pane {number}"
        );
        assert_eq!(pending(&a), None);
        assert_eq!(
            lines_with(&a, "is not open"),
            [format!("• pane {number} is not open")]
        );
    }
}

#[test]
fn a_blank_screen_is_denied_with_a_notice_and_no_prompt() {
    let (mut a, turn) = live();
    assert_eq!(
        announce_now(&mut a, turn, 1, screen_ask(1, None)),
        vec![decision(
            turn,
            1,
            Decision::Denied("the pane's screen is empty")
        )]
    );
    assert_eq!(pending(&a), None);
    assert_eq!(
        lines_with(&a, "is empty"),
        ["• pane 1: the screen is empty"]
    );
}

#[test]
fn a_cut_text_is_flagged_and_keeps_the_tail() {
    let (mut a, turn) = live();
    // Wide rows, so the 200 lines of history are worth more than the cap.
    a.update(AppEvent::Resize {
        cols: 400,
        rows: 25,
    });
    let body: String = (0..300)
        .map(|n| format!("{n:03} {}\r\n", "x".repeat(60)))
        .collect();
    shell_output(&mut a, body.as_bytes());
    announce_now(&mut a, turn, 1, screen_ask(1, Some(200)));
    let text = frozen(&a);
    assert!(text.starts_with("[earlier lines cut"), "{text}");
    assert!(text.contains("299 xxx"), "{text}");
    assert!(text.len() <= 8 * 1024, "{}", text.len());
}

#[test]
fn scrollback_on_a_full_screen_app_says_only_the_visible_screen() {
    let (mut a, turn) = live();
    shell_output(&mut a, b"\x1b[?1049hvim says hi\r\n");
    announce_now(&mut a, turn, 1, screen_ask(1, Some(50)));
    let text = frozen(&a);
    assert!(
        text.starts_with("full-screen app: visible screen only"),
        "{text}"
    );
    assert!(text.contains("vim says hi"), "{text}");
}

#[test]
fn a_generic_approve_never_answers_a_screen() {
    let (mut a, _) = asked(5, None);
    assert_eq!(at(&mut a, 60_000, plain(KeyCode::Enter)), vec![]);
    assert!(pending(&a).is_some());
}

fn chat_mut(a: &mut App) -> &mut ChatState {
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
}

#[test]
fn a_screen_ask_refused_before_prompting_says_so_in_the_chat() {
    use crate::core::ai::chat::approval::ANOTHER_PENDING;
    let (mut a, turn) = asked(5, None);
    assert_eq!(
        announce_now(&mut a, turn, 2, screen_ask(1, None)),
        vec![decision(turn, 2, Decision::Denied(ANOTHER_PENDING))]
    );
    assert_eq!(
        lines_with(&a, "screen read"),
        ["• screen read denied: pane 1 (another approval is pending)"]
    );
    // The first review is untouched and nothing else was said.
    assert_eq!(pending(&a).map(|p| p.1), Some(Some((turn, CallId(1)))));
}
