//! The review of a proposed write batch: a full-pane view that opens for a
//! `Batch` ask in every approval mode, a coverage gate on `y`, and `n` or Esc
//! that rejects the whole batch.

use super::chat_live::{ctrl, plain};
use super::tool_approval::{announce_now, chat, decision, live, pending, result};
use super::*;
use crate::app::tool::DECLINED;
use crate::core::ai::chat::{
    approval::ApprovalAsk,
    engine::{CallId, Decision, ToolStatus},
    review::{ARM_DELAY, TOO_LARGE},
    review_view::{ReviewView, testing::numbered},
};
use crate::core::ai::config::ApprovalMode;
use crate::ui::theme::LazaroboxTheme;
use ratatui::{Terminal, backend::TestBackend};

fn batch(n: usize) -> ApprovalAsk {
    ApprovalAsk::Batch(numbered(n))
}

fn review(a: &App) -> &ReviewView {
    chat(a, id2()).approvals().review().expect("a review view")
}

fn ms(d: std::time::Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap()
}

/// Draws the app as the event loop does, then lets the review take the rows
/// the frame showed.
fn paint(a: &mut App) {
    let theme = LazaroboxTheme::default();
    let mut terminal = Terminal::new(TestBackend::new(80, 25)).unwrap();
    terminal
        .draw(|frame| crate::ui::render(frame, a, &theme))
        .unwrap();
    a.frame_drawn();
}

fn paint_at(a: &mut App, t: u64) {
    NOW_MS.with(|n| n.set(t));
    paint(a);
}

/// An event at `t`, then the frame it causes.
fn at_drawn(a: &mut App, t: u64, ev: AppEvent) -> Vec<Effect> {
    let effects = at(a, t, ev);
    paint(a);
    effects
}

/// A batch of `n` rows asked at t = 0, not drawn yet.
fn asked_unseen(n: usize) -> (App, u64) {
    let (mut a, turn) = live();
    announce_now(&mut a, turn, 1, batch(n));
    (a, turn)
}

/// A batch of `n` rows asked and drawn at t = 0.
fn asked(n: usize) -> (App, u64) {
    let (mut a, turn) = asked_unseen(n);
    paint_at(&mut a, 0);
    (a, turn)
}

/// Pages down at `t`, drawing each frame, until nothing is unseen or the end
/// is on screen.
fn read_all(a: &mut App, t: u64) {
    for _ in 0..500 {
        if review(a).remaining() == 0 {
            return;
        }
        at_drawn(a, t, plain(KeyCode::PageDown));
    }
}

fn system_lines(a: &App) -> Vec<String> {
    chat(a, id2())
        .lines(80)
        .iter()
        .map(|l| l.text.clone())
        .filter(|t| t.contains("batch"))
        .collect()
}

#[test]
fn a_batch_opens_the_review_in_every_approval_mode_and_never_runs_by_itself() {
    for mode in [ApprovalMode::Ask, ApprovalMode::Auto, ApprovalMode::Run] {
        let (mut a, turn) = live();
        a.panes
            .get_mut(&id2())
            .and_then(PaneState::chat_mut)
            .unwrap()
            .set_approval_mode(mode);
        assert_eq!(announce_now(&mut a, turn, 1, batch(5)), vec![], "{mode:?}");
        assert_eq!(pending(&a).map(|p| p.1), Some(Some((turn, CallId(1)))));
        assert!(review(&a).total() > 0, "{mode:?}");
        // The overlay of the other asks is not drawn for it.
        assert_eq!(a.approval_prompt(id2()), None);
    }
}

#[test]
fn y_does_nothing_while_lines_are_unseen_however_late() {
    let (mut a, _) = asked(200);
    assert!(review(&a).remaining() > 0);
    assert_eq!(at(&mut a, 60_000, key('y')), vec![]);
    assert!(pending(&a).is_some(), "still waiting");
}

#[test]
fn y_on_a_locked_gate_hints_for_two_seconds_and_changes_nothing() {
    let (mut a, _) = asked(200);
    let (remaining, top) = (review(&a).remaining(), review(&a).top());
    assert_eq!(at_drawn(&mut a, 5_000, key('y')), vec![]);
    assert!(pending(&a).is_some());
    assert_eq!(review(&a).remaining(), remaining);
    assert_eq!(review(&a).top(), top);
    let hint = |a: &App, t: u64| {
        NOW_MS.with(|n| n.set(t));
        review(a).locked_hint((a.clock)())
    };
    assert_eq!(hint(&a, 6_999), Some(remaining));
    assert_eq!(hint(&a, 7_000), None);
}

#[test]
fn y_during_the_arm_delay_with_every_line_seen_stays_silent() {
    let (mut a, _) = asked(2);
    read_all(&mut a, 100);
    assert_eq!(review(&a).remaining(), 0);
    assert_eq!(at(&mut a, 200, key('y')), vec![]);
    NOW_MS.with(|n| n.set(300));
    assert_eq!(review(&a).locked_hint((a.clock)()), None);
}

#[test]
fn a_hint_never_blocks_the_approval_nor_the_ready_wake() {
    let (mut a, turn) = asked(60);
    assert_eq!(at_drawn(&mut a, 100, key('y')), vec![], "locked");
    read_all(&mut a, 900);
    assert_eq!(review(&a).remaining(), 0);
    // Still inside the 2 s of the hint, once the delay is over.
    NOW_MS.with(|n| n.set(1_000));
    assert!(review(&a).locked_hint((a.clock)()).is_none(), "all seen");
    a.take_dirty();
    a.wake_review();
    assert!(a.take_dirty(), "the frame that shows `y ready`");
    assert_eq!(
        at(&mut a, 1_100, key('y')),
        vec![decision(turn, 1, Decision::Sent)]
    );
}

#[test]
fn y_on_a_batch_that_cannot_be_reviewed_says_so_for_two_seconds() {
    let (mut a, _) = asked(2100);
    assert_eq!(at(&mut a, 5_000, key('y')), vec![]);
    NOW_MS.with(|n| n.set(6_999));
    assert!(review(&a).refused_hint((a.clock)()));
    NOW_MS.with(|n| n.set(7_000));
    assert!(!review(&a).refused_hint((a.clock)()));
    assert!(pending(&a).is_some(), "nothing was answered");
}

#[test]
fn y_arms_at_full_coverage_plus_the_review_delay() {
    let (mut a, turn) = asked(60);
    read_all(&mut a, 100);
    assert_eq!(review(&a).remaining(), 0);
    assert_eq!(
        early_held_then_new(&mut a, ms(ARM_DELAY), &key('y')),
        vec![decision(turn, 1, Decision::Sent)]
    );
    assert_eq!(pending(&a), None);
    assert!(chat(&a, id2()).approvals().review().is_none());
    // A first press at exactly the delay approves.
    let (mut b, turn) = asked(60);
    read_all(&mut b, 100);
    assert_eq!(
        at(&mut b, ms(ARM_DELAY), key('y')),
        vec![decision(turn, 1, Decision::Sent)]
    );
}

#[test]
fn a_held_y_never_approves() {
    let (mut a, _) = asked(2);
    read_all(&mut a, 100);
    let held = super::chat_live::repeat(KeyCode::Char('y'), KeyModifiers::NONE);
    assert_eq!(at(&mut a, 5_000, held), vec![]);
    assert!(pending(&a).is_some());
}

// A held key sent as Presses: the decision is never taken by the hold.
#[test]
fn a_held_answer_key_sent_as_presses_never_answers() {
    for answer in [key('y'), key('n'), plain(KeyCode::Esc)] {
        let (mut a, _) = asked(2);
        read_all(&mut a, 100);
        let end = ms(ARM_DELAY) + 1_000;
        for t in (100..=end).step_by(100) {
            assert_eq!(at(&mut a, t, answer.clone()), vec![], "{answer:?} at {t}");
        }
        assert!(pending(&a).is_some(), "{answer:?}");
    }
}

#[test]
fn scrolling_keys_mark_rows_seen_and_other_keys_do_nothing() {
    let (mut a, _) = asked(100);
    let before = review(&a).remaining();
    at_drawn(&mut a, 1, plain(KeyCode::Down));
    assert_eq!((review(&a).top(), review(&a).remaining()), (1, before - 1));
    at(&mut a, 2, key('j'));
    assert_eq!(review(&a).top(), 2);
    at(&mut a, 3, key('k'));
    at(&mut a, 4, plain(KeyCode::Up));
    assert_eq!(review(&a).top(), 0);
    at(&mut a, 5, plain(KeyCode::PageDown));
    assert!(review(&a).top() > 2);
    at(&mut a, 6, plain(KeyCode::End));
    let end = review(&a).top();
    at(&mut a, 7, plain(KeyCode::Home));
    assert_eq!(review(&a).top(), 0);
    assert!(end > 50);
    for other in [key('a'), plain(KeyCode::Enter), key('Y'), key(' ')] {
        assert_eq!(at(&mut a, 8, other), vec![]);
    }
    assert_eq!(review(&a).top(), 0);
    // A held arrow keeps scrolling.
    let held = super::chat_live::repeat(KeyCode::Down, KeyModifiers::NONE);
    at(&mut a, 9, held);
    assert_eq!(review(&a).top(), 1);
}

#[test]
fn n_and_esc_reject_the_whole_batch_with_a_system_line() {
    for rejecting in [key('n'), plain(KeyCode::Esc)] {
        let (mut a, turn) = asked(5);
        assert_eq!(
            at(&mut a, ms(ARM_DELAY), rejecting),
            vec![decision(turn, 1, Decision::Denied(DECLINED))]
        );
        assert_eq!(pending(&a), None);
        assert_eq!(system_lines(&a), ["• batch rejected: 1 file, +5 -0"]);
    }
}

#[test]
fn ctrl_c_cancels_the_turn_and_denies_the_batch() {
    let (mut a, turn) = asked(5);
    let effects = at(&mut a, 0, ctrl('c'));
    assert_eq!(
        effects,
        vec![
            decision(turn, 1, Decision::Denied("cancelled")),
            Effect::ChatCancel(id2())
        ]
    );
}

#[test]
fn a_batch_too_large_to_review_never_arms_and_tells_the_model_to_split() {
    let (mut a, turn) = asked(2100);
    assert!(!review(&a).reviewable());
    read_all(&mut a, 100);
    assert_eq!(review(&a).remaining(), 0);
    assert_eq!(at(&mut a, 600_000, key('y')), vec![]);
    assert!(pending(&a).is_some());
    assert_eq!(
        at(&mut a, 600_001, key('n')),
        vec![decision(turn, 1, Decision::Denied(TOO_LARGE))]
    );
    assert!(TOO_LARGE.contains("too large") && TOO_LARGE.contains("smaller batches"));
}

#[test]
fn an_empty_batch_is_rejected_with_the_same_answer() {
    let (mut a, turn) = live();
    let empty = ApprovalAsk::Batch(std::sync::Arc::new(
        crate::core::ai::chat::review::BatchReview::new(
            crate::core::ai::chat::review::ReviewDoc::build(
                &crate::core::ai::tools::stage::Batch::new(
                    crate::core::ai::tools::stage::BatchId {
                        key: crate::core::ai::tools::stage::StageKey::Pane(id2()),
                        generation: 1,
                    },
                ),
            ),
        ),
    ));
    announce_now(&mut a, turn, 1, empty);
    assert_eq!(at(&mut a, 600_000, key('y')), vec![]);
    assert_eq!(
        at(&mut a, 600_001, key('n')),
        vec![decision(turn, 1, Decision::Denied(TOO_LARGE))]
    );
}

#[test]
fn a_call_that_ends_takes_the_review_with_it() {
    let (mut a, turn) = asked(5);
    result(&mut a, turn, 1, ToolStatus::Denied);
    assert_eq!(pending(&a), None);
    assert!(chat(&a, id2()).approvals().review().is_none());
}

#[test]
fn a_resize_keeps_what_was_seen_and_counts_the_new_screen() {
    let (mut a, _) = asked(60);
    read_all(&mut a, 100);
    assert_eq!(review(&a).remaining(), 0);
    a.update(AppEvent::Resize { cols: 50, rows: 12 });
    assert_eq!(review(&a).remaining(), 0, "no line is lost");
}

#[test]
fn n_and_esc_wait_for_the_review_delay_too() {
    for rejecting in [key('n'), plain(KeyCode::Esc)] {
        let (mut a, turn) = asked(5);
        let wait = ms(ARM_DELAY);
        assert_eq!(
            early_held_then_new(&mut a, wait, &rejecting),
            vec![decision(turn, 1, Decision::Denied(DECLINED))]
        );
        let (mut b, turn) = asked(5);
        assert_eq!(
            at(&mut b, wait, rejecting),
            vec![decision(turn, 1, Decision::Denied(DECLINED))]
        );
    }
}

#[test]
fn the_other_asks_answer_after_the_short_delay() {
    use crate::core::ai::chat::approval::ASK_ARM_DELAY;
    let asks = [
        ApprovalAsk::Search {
            pattern: "x".into(),
            dir: ".".into(),
            reach: crate::core::ai::chat::approval::Reach::Other,
        },
        ApprovalAsk::External {
            server: "s".into(),
            tool: "t".into(),
            args: "{}".into(),
        },
    ];
    for ask in asks {
        let wait = ms(ASK_ARM_DELAY);
        let (mut a, turn) = live();
        announce_now(&mut a, turn, 1, ask.clone());
        assert_eq!(
            early_held_then_new(&mut a, wait, &key('y')),
            vec![decision(turn, 1, Decision::Sent)]
        );
        let (mut b, turn) = live();
        announce_now(&mut b, turn, 1, ask);
        assert_eq!(
            at(&mut b, wait, key('y')),
            vec![decision(turn, 1, Decision::Sent)]
        );
    }
}

#[test]
fn a_review_counts_nothing_as_seen_until_it_is_drawn() {
    let (mut a, _) = asked_unseen(60);
    let (total, now) = (review(&a).total(), review(&a).remaining());
    assert_eq!(now, total, "opening is not drawing");
    paint_at(&mut a, 0);
    assert!(review(&a).remaining() < total);
}

#[test]
fn a_resize_while_the_review_is_not_drawn_counts_nothing() {
    let (mut a, _) = asked_unseen(60);
    a.update(AppEvent::Resize { cols: 50, rows: 12 });
    let v = review(&a);
    assert_eq!(v.remaining(), v.total());
}

#[test]
fn a_review_in_a_background_tab_counts_and_arms_only_once_it_is_shown() {
    let (mut a, turn) = live();
    press(&mut a, "tn");
    announce_now(&mut a, turn, 1, batch(60));
    paint_at(&mut a, 5_000);
    let (total, left) = (review(&a).total(), review(&a).arm_left(mock_now()));
    assert_eq!(review(&a).remaining(), total, "its pane was not drawn");
    assert_eq!(left, ARM_DELAY);
    press(&mut a, "gB");
    paint_at(&mut a, 6_000);
    assert!(review(&a).remaining() < total);
    NOW_MS.with(|n| n.set(6_000 + ms(ARM_DELAY) - 1));
    assert!(review(&a).arm_left(mock_now()) > std::time::Duration::ZERO);
    NOW_MS.with(|n| n.set(6_000 + ms(ARM_DELAY)));
    assert_eq!(review(&a).arm_left(mock_now()), std::time::Duration::ZERO);
}

#[test]
fn the_delay_starts_at_the_first_draw_not_at_the_ask() {
    let drawn_late = || {
        let (mut a, turn) = asked_unseen(2);
        paint_at(&mut a, 500);
        read_all(&mut a, 500);
        assert_eq!(review(&a).remaining(), 0);
        (a, turn)
    };
    let (mut a, _) = drawn_late();
    assert_eq!(at(&mut a, ms(ARM_DELAY), key('y')), vec![], "ask + delay");
    let (mut b, turn) = drawn_late();
    assert_eq!(
        early_held_then_new(&mut b, 500 + ms(ARM_DELAY), &key('y')),
        vec![decision(turn, 1, Decision::Sent)]
    );
    let (mut c, turn) = drawn_late();
    assert_eq!(
        at(&mut c, 500 + ms(ARM_DELAY), key('y')),
        vec![decision(turn, 1, Decision::Sent)]
    );
}

#[test]
fn n_and_esc_wait_for_the_delay_from_the_first_draw_on_a_reviewable_batch() {
    for rejecting in [key('n'), plain(KeyCode::Esc)] {
        let (mut a, turn) = asked_unseen(5);
        assert_eq!(at(&mut a, 60_000, rejecting.clone()), vec![], "not drawn");
        paint_at(&mut a, 60_000);
        assert_eq!(
            early_held_then_new(&mut a, 60_000 + ms(ARM_DELAY), &rejecting),
            vec![decision(turn, 1, Decision::Denied(DECLINED))]
        );
        let (mut b, turn) = asked_unseen(5);
        paint_at(&mut b, 60_000);
        assert_eq!(
            at(&mut b, 60_000 + ms(ARM_DELAY), rejecting),
            vec![decision(turn, 1, Decision::Denied(DECLINED))]
        );
    }
}

#[test]
fn n_and_esc_are_immediate_on_a_batch_that_cannot_be_reviewed() {
    for rejecting in [key('n'), plain(KeyCode::Esc)] {
        let (mut a, turn) = asked_unseen(2100);
        assert_eq!(
            at(&mut a, 0, rejecting),
            vec![decision(turn, 1, Decision::Denied(TOO_LARGE))]
        );
    }
}

#[test]
fn the_screen_redraws_when_the_delay_ends() {
    let (mut a, _) = asked(5);
    NOW_MS.with(|n| n.set(100));
    a.take_dirty();
    a.wake_review();
    assert!(a.take_dirty(), "the countdown runs");
    NOW_MS.with(|n| n.set(ms(ARM_DELAY)));
    a.wake_review();
    assert!(a.take_dirty(), "one more frame for `y ready`");
    a.wake_review();
    assert!(!a.take_dirty(), "then it rests");
}

#[test]
fn a_review_that_was_never_drawn_does_not_wake_the_screen() {
    let (mut a, _) = asked_unseen(5);
    a.take_dirty();
    NOW_MS.with(|n| n.set(100));
    a.wake_review();
    assert!(!a.take_dirty());
}

/// A review ready for `y`: drawn at 0, read to the end.
fn ready() -> (App, u64) {
    let (mut a, turn) = asked(30);
    read_all(&mut a, 0);
    assert_eq!(review(&a).remaining(), 0);
    (a, turn)
}

#[test]
fn a_generic_approve_never_answers_a_batch() {
    // The state that must not exist: a batch ask without its review view.
    let (mut a, turn) = live();
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .approvals_mut()
        .request_call(batch(5), turn, CallId(1), None)
        .unwrap();
    assert!(!a.batch_pending(id2()));
    assert_eq!(at(&mut a, 60_000, key('y')), vec![]);
    assert!(pending(&a).is_some(), "only the review answers a batch");
}

fn chat_mut(a: &mut App) -> &mut crate::core::ai::chat::ChatState {
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
}

fn mouse(kind: ratatui::crossterm::event::MouseEventKind, a: &App) -> AppEvent {
    let rect = a
        .tiling()
        .panes
        .iter()
        .find(|(id, _)| *id == id2())
        .map(|(_, rect)| *rect)
        .unwrap();
    AppEvent::Mouse(ratatui::crossterm::event::MouseEvent {
        kind,
        column: rect.x + rect.width / 2,
        row: rect.y + rect.height / 2,
        modifiers: KeyModifiers::NONE,
    })
}

#[test]
fn the_wheel_scrolls_the_review_and_never_the_hidden_transcript() {
    use crate::core::ai::chat::transcript::Scroll;
    use ratatui::crossterm::event::MouseEventKind;
    let (mut a, turn) = live();
    for _ in 0..100 {
        chat_mut(&mut a).push_system_line("filler".into());
    }
    chat_mut(&mut a).set_scroll(Scroll::Pinned(10));
    announce_now(&mut a, turn, 1, batch(100));
    paint_at(&mut a, 0);
    a.config.mouse = true;
    a.take_dirty();
    let down = mouse(MouseEventKind::ScrollDown, &a);
    assert_eq!(a.update(down), vec![]);
    assert_eq!(review(&a).top(), 1);
    assert!(a.take_dirty());
    let up = mouse(MouseEventKind::ScrollUp, &a);
    a.update(up.clone());
    a.update(up);
    assert_eq!(review(&a).top(), 0);
    assert_eq!(chat(&a, id2()).scroll(), Scroll::Pinned(10), "untouched");
}

#[test]
fn a_click_over_a_review_does_nothing() {
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    let (mut a, _) = asked(100);
    a.config.mouse = true;
    a.take_dirty();
    let before = (review(&a).top(), review(&a).remaining());
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
        MouseEventKind::Down(MouseButton::Right),
        MouseEventKind::Drag(MouseButton::Left),
    ] {
        let event = mouse(kind, &a);
        assert_eq!(a.update(event), vec![], "{kind:?}");
    }
    assert_eq!((review(&a).top(), review(&a).remaining()), before);
    assert!(pending(&a).is_some());
}

#[test]
fn a_release_event_does_nothing() {
    use ratatui::crossterm::event::{KeyEvent, KeyEventKind};
    let (mut a, _) = ready();
    for code in [KeyCode::Char('y'), KeyCode::Char('n'), KeyCode::Esc] {
        let release = AppEvent::Key(KeyEvent::new_with_kind(
            code,
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        assert_eq!(at(&mut a, 60_000, release), vec![], "{code:?}");
    }
    assert!(pending(&a).is_some());
}

#[test]
fn a_review_in_an_unfocused_pane_takes_no_keys() {
    let (mut a, turn) = ready();
    press(&mut a, "h");
    assert_ne!(a.focused(), id2());
    for answer in [key('y'), key('n'), plain(KeyCode::Esc)] {
        let effects = at(&mut a, 60_000, answer);
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::ToolDecision { .. })),
            "{effects:?}"
        );
    }
    assert!(pending(&a).is_some(), "still waiting for turn {turn}");
}

#[test]
fn turning_tools_off_withdraws_the_review() {
    let (mut a, turn) = asked(5);
    a.ai.set_tools_enabled(false);
    let effects = a.sync_tools();
    assert!(
        effects.contains(&decision(turn, 1, Decision::Denied("tools are off"))),
        "{effects:?}"
    );
    assert_eq!(pending(&a), None);
    assert!(chat(&a, id2()).approvals().review().is_none());
}

#[test]
fn a_second_ask_while_a_review_is_pending_is_refused_and_the_review_stays() {
    use crate::core::ai::chat::approval::ANOTHER_PENDING;
    let (mut a, turn) = asked(5);
    let (total, top) = (review(&a).total(), review(&a).top());
    assert_eq!(
        announce_now(&mut a, turn, 2, batch(7)),
        vec![decision(turn, 2, Decision::Denied(ANOTHER_PENDING))]
    );
    assert_eq!(pending(&a).map(|p| p.1), Some(Some((turn, CallId(1)))));
    assert_eq!((review(&a).total(), review(&a).top()), (total, top));
}

#[test]
fn the_scroll_badge_does_not_cover_the_review_counter() {
    use crate::core::ai::chat::transcript::Scroll;
    let (mut a, turn) = live();
    for _ in 0..100 {
        chat_mut(&mut a).push_system_line("filler".into());
    }
    chat_mut(&mut a).set_scroll(Scroll::Pinned(0));
    announce_now(&mut a, turn, 1, batch(5));
    assert!(a.scrolled_by(id2()).is_some(), "the transcript is scrolled");
    let theme = LazaroboxTheme::default();
    let mut terminal = Terminal::new(TestBackend::new(80, 25)).unwrap();
    terminal
        .draw(|frame| crate::ui::render(frame, &a, &theme))
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    let rect = a
        .tiling()
        .panes
        .iter()
        .find(|(id, _)| *id == id2())
        .map(|(_, rect)| *rect)
        .unwrap();
    let row: String = (rect.x..rect.x + rect.width)
        .map(|x| buf[(x, rect.y)].symbol())
        .collect();
    assert!(row.contains("seen") && !row.contains("[+"), "{row:?}");
}

// The orphan review (chat-pane "Staged batch indicator", code-write "Orphan,
// close and quit"): the badge comes from the runtime's poll, Ctrl+G asks the
// runtime for the review, and the same view and gate answer with an id.

mod orphan {
    use super::*;
    use crate::core::ai::{
        chat::review::{Badge, BatchEvent},
        tools::stage::{BatchId, StageKey},
    };
    use std::collections::HashMap;

    fn batch_id() -> BatchId {
        BatchId {
            key: StageKey::Pane(id2()),
            generation: 7,
        }
    }

    fn badges(badge: Option<Badge>) -> HashMap<PaneId, Badge> {
        badge.map(|b| (id2(), b)).into_iter().collect()
    }

    fn badge(a: &App) -> Option<Badge> {
        chat(a, id2()).batch_badge()
    }

    /// A chat that holds an orphan of `n` rows, its turn over.
    fn orphaned(n: usize) -> App {
        let (mut a, _) = live();
        a.update(ctrl('c'));
        a.sync_batches(&badges(Some(Badge::Orphan(n))));
        a
    }

    fn review_event(n: usize) -> AppEvent {
        AppEvent::Batch(id2(), BatchEvent::Review(batch_id(), numbered(n)))
    }

    fn done(shown: &str) -> AppEvent {
        AppEvent::Batch(
            id2(),
            BatchEvent::Done {
                shown: shown.into(),
                told: None,
            },
        )
    }

    #[test]
    fn the_poll_sets_and_clears_the_badge_and_redraws_only_on_a_change() {
        let (mut a, _) = live();
        assert_eq!(badge(&a), None);
        a.take_dirty();
        a.sync_batches(&badges(Some(Badge::Staged(2))));
        assert_eq!(badge(&a), Some(Badge::Staged(2)));
        assert!(a.take_dirty());
        a.sync_batches(&badges(Some(Badge::Staged(2))));
        assert!(!a.take_dirty(), "the same poll draws nothing");
        a.sync_batches(&badges(Some(Badge::Orphan(2))));
        assert_eq!(badge(&a), Some(Badge::Orphan(2)));
        a.sync_batches(&badges(None));
        assert_eq!(badge(&a), None);
        // A pane that is not a chat is not a place for a badge.
        a.sync_batches(&HashMap::from([(PaneId::FIRST, Badge::Orphan(1))]));
        assert_eq!(badge(&a), None);
    }

    #[test]
    fn ctrl_g_asks_for_the_review_of_an_orphan_and_nothing_else() {
        let mut a = orphaned(3);
        assert_eq!(a.update(ctrl('g')), vec![Effect::BatchReview(id2())]);
        // A batch the model is still staging is not the user's to review.
        a.sync_batches(&badges(Some(Badge::Staged(3))));
        assert_eq!(at(&mut a, 5_000, ctrl('g')), vec![]);
        assert!(a.notice().is_some_and(|n| n.contains("orphan")));
        a.sync_batches(&badges(None));
        assert_eq!(at(&mut a, 10_000, ctrl('g')), vec![]);
    }

    #[test]
    fn ctrl_g_never_types_and_a_held_one_asks_once() {
        let mut a = orphaned(3);
        let held = super::super::chat_live::repeat(KeyCode::Char('g'), KeyModifiers::CONTROL);
        assert_eq!(a.update(held), vec![]);
        assert_eq!(chat(&a, id2()).composer().text(), "");
        assert_eq!(
            at(&mut a, 5_000, ctrl('g')),
            vec![Effect::BatchReview(id2())]
        );
    }

    #[test]
    fn the_review_event_opens_the_same_view_for_the_orphan() {
        let mut a = orphaned(5);
        assert_eq!(a.update(review_event(5)), vec![]);
        assert!(review(&a).total() > 0);
        assert_eq!(pending(&a).map(|p| p.1), Some(None), "no call waits");
        // A second Ctrl+G is swallowed by the open view.
        assert_eq!(a.update(ctrl('g')), vec![]);
        paint_at(&mut a, 0);
        assert!(chat(&a, id2()).orphan().is_some());
    }

    #[test]
    fn y_applies_the_orphan_behind_the_same_gate() {
        let mut a = orphaned(60);
        a.update(review_event(60));
        paint_at(&mut a, 0);
        assert_eq!(at(&mut a, 60_000, key('y')), vec![], "rows unseen");
        read_all(&mut a, 100);
        assert_eq!(
            early_held_then_new(&mut a, ms(ARM_DELAY), &key('y')),
            vec![Effect::BatchApply {
                pane: id2(),
                id: batch_id()
            }]
        );
        assert_eq!(pending(&a), None);
        assert!(chat(&a, id2()).approvals().review().is_none());
        assert_eq!(chat(&a, id2()).orphan(), None);
        // A first press at exactly the delay applies it.
        let mut b = orphaned(60);
        b.update(review_event(60));
        paint_at(&mut b, 0);
        read_all(&mut b, 100);
        assert_eq!(
            at(&mut b, ms(ARM_DELAY), key('y')),
            vec![Effect::BatchApply {
                pane: id2(),
                id: batch_id()
            }]
        );
    }

    #[test]
    fn y_on_an_orphan_review_that_cannot_be_approved_hints() {
        let mut a = orphaned(2100);
        a.update(review_event(2100));
        paint_at(&mut a, 0);
        assert_eq!(at(&mut a, 5_000, key('y')), vec![]);
        NOW_MS.with(|n| n.set(6_000));
        assert!(review(&a).refused_hint((a.clock)()));
    }

    #[test]
    fn n_and_esc_discard_the_orphan_after_the_delay() {
        for rejecting in [key('n'), plain(KeyCode::Esc)] {
            let mut a = orphaned(5);
            a.update(review_event(5));
            paint_at(&mut a, 0);
            let wait = ms(ARM_DELAY);
            assert_eq!(
                early_held_then_new(&mut a, wait, &rejecting),
                vec![Effect::BatchDiscard {
                    pane: id2(),
                    id: batch_id()
                }]
            );
            assert_eq!(pending(&a), None);
            assert_eq!(system_lines(&a), Vec::<String>::new(), "the notice speaks");
            let mut b = orphaned(5);
            b.update(review_event(5));
            paint_at(&mut b, 0);
            assert_eq!(
                at(&mut b, wait, rejecting),
                vec![Effect::BatchDiscard {
                    pane: id2(),
                    id: batch_id()
                }]
            );
        }
    }

    #[test]
    fn ctrl_c_closes_the_view_and_leaves_the_orphan_alone() {
        let mut a = orphaned(5);
        a.update(review_event(5));
        // A new Ctrl+C, not the one that cancelled the turn still held.
        assert_eq!(at(&mut a, 1_000, ctrl('c')), vec![]);
        assert_eq!(pending(&a), None);
        assert_eq!(badge(&a), Some(Badge::Orphan(5)));
        let again = at(&mut a, 5_000, ctrl('g'));
        assert_eq!(again, vec![Effect::BatchReview(id2())]);
    }

    #[test]
    fn a_review_event_is_refused_while_another_ask_waits_or_the_pane_is_gone() {
        let (mut a, turn) = live();
        announce_now(&mut a, turn, 1, super::super::tool_approval::run("ls"));
        a.update(review_event(2));
        assert!(matches!(pending(&a), Some((ApprovalAsk::Run { .. }, _))));
        assert_eq!(chat(&a, id2()).orphan(), None);
        assert!(a.notice().is_some());
        // The event of a pane that is gone does nothing.
        let gone = BatchEvent::Review(batch_id(), numbered(2));
        a.update(AppEvent::Batch(PaneId::for_test(99), gone));
    }

    // A stale orphan id (the view went some other way) never turns the `y` of
    // a model's batch into an apply by id.
    #[test]
    fn a_tool_batch_never_answers_through_a_stale_orphan_id() {
        let (mut a, turn) = asked(2);
        a.panes
            .get_mut(&id2())
            .and_then(PaneState::chat_mut)
            .unwrap()
            .set_orphan(Some(batch_id()));
        assert_eq!(
            at(&mut a, ms(ARM_DELAY), key('n')),
            vec![decision(turn, 1, Decision::Denied(DECLINED))]
        );
        assert_eq!(chat(&a, id2()).orphan(), None);
    }

    // Cancel at the batch ask, the orphan badge, Ctrl+G, the review, `y`, the
    // answer: the badge goes and the pane can stage again.
    #[test]
    fn cancel_at_the_ask_then_review_apply_and_the_badge_clears() {
        let (mut a, turn) = asked_unseen(60);
        paint_at(&mut a, 0);
        assert_eq!(
            at(&mut a, 0, ctrl('c')),
            vec![
                decision(turn, 1, Decision::Denied("cancelled")),
                Effect::ChatCancel(id2())
            ]
        );
        // The runtime kept the batch as an orphan and the poll says so.
        a.sync_batches(&badges(Some(Badge::Orphan(1))));
        assert_eq!(badge(&a), Some(Badge::Orphan(1)));
        assert_eq!(a.update(ctrl('g')), vec![Effect::BatchReview(id2())]);
        a.update(review_event(60));
        paint_at(&mut a, 1);
        read_all(&mut a, 100);
        let apply = Effect::BatchApply {
            pane: id2(),
            id: batch_id(),
        };
        assert_eq!(at(&mut a, 100 + ms(ARM_DELAY), key('y')), vec![apply]);
        a.update(done("applied 1 paths"));
        assert_eq!(a.notice(), Some("applied 1 paths"));
        a.sync_batches(&badges(None));
        assert_eq!(badge(&a), None);
        assert_eq!(a.update(ctrl('g')), vec![], "nothing left to review");
    }

    #[test]
    fn cancel_then_discard_clears_the_badge_too() {
        let mut a = orphaned(4);
        a.update(ctrl('g'));
        a.update(review_event(4));
        paint_at(&mut a, 0);
        let discard = Effect::BatchDiscard {
            pane: id2(),
            id: batch_id(),
        };
        assert_eq!(at(&mut a, ms(ARM_DELAY), key('n')), vec![discard]);
        a.update(done("discarded 4 staged entries"));
        a.sync_batches(&badges(None));
        assert_eq!(badge(&a), None);
    }

    // Spec (chat-pane, "Closing discards a batch"): quitting discards the
    // batch of every chat that holds one, and says how many entries.
    #[test]
    fn quitting_closes_the_batch_of_each_chat_before_the_quit() {
        let (mut a, _) = live();
        a.update(ctrl('c'));
        // A batch staged since the last poll has no badge yet; it goes too.
        let plain = a.quit_effects();
        let close = plain.iter().position(|e| *e == Effect::BatchClose(id2()));
        let quit = plain.iter().position(|e| *e == Effect::Quit);
        assert!(close.is_some() && close < quit, "{plain:?}");
        for badge in [Badge::Staged(2), Badge::Orphan(1)] {
            a.sync_batches(&badges(Some(badge)));
            let effects = a.quit_effects();
            let close = effects.iter().position(|e| *e == Effect::BatchClose(id2()));
            let quit = effects.iter().position(|e| *e == Effect::Quit);
            assert!(close.is_some() && close < quit, "{effects:?}");
        }
    }
}
