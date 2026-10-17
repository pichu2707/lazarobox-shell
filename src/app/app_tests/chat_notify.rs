//! The bell for an answer nobody watches (chat-notify): which chat events
//! ask for `Effect::Bell`, and what the window focus event changes.

use super::chat_notes::{PROJECT, with_answer};
use super::*;
use crate::core::ai::{
    chat::{
        ANSWER_CAP,
        engine::{ChatError, Finish, StreamEvent},
    },
    config::NotifyMode,
    memory::port::MemReply,
};

fn chat_id() -> PaneId {
    id2()
}

fn shell_id() -> PaneId {
    PaneId::FIRST
}

fn event(turn: u64, event: StreamEvent) -> AppEvent {
    AppEvent::Chat(chat_id(), turn, event)
}

fn done(turn: u64) -> AppEvent {
    event(turn, StreamEvent::Done(Finish::Stop))
}

fn error(turn: u64) -> AppEvent {
    event(turn, StreamEvent::Error(ChatError::Failed("boom".into())))
}

/// A shell and a chat (focused) whose turn 1 is streaming; `notify` is the
/// `[ai] notify` mode.
fn streaming(notify: NotifyMode) -> App {
    let mut ai = AiConfig::default();
    ai.set_notify(notify);
    let mut a = launched(80, 25).with_ai(ai);
    a.open_chat(None);
    assert_eq!(a.focused(), chat_id());
    match a.chat_send(chat_id(), "hi").as_slice() {
        [Effect::ChatSend { turn: 1, .. }] => {}
        other => panic!("expected the first ChatSend, got {other:?}"),
    }
    a.update(event(1, StreamEvent::Delta("Hel".into())));
    a
}

/// The same, with the user looking at the shell pane.
fn unwatched() -> App {
    let mut a = streaming(NotifyMode::Bell);
    a.set_focus(shell_id());
    a
}

const BELL: [Effect; 1] = [Effect::Bell];

// Spec (chat-notify): Answer finishes in an unfocused pane.
#[test]
fn an_answer_that_finishes_in_another_pane_rings_once() {
    let mut a = unwatched();
    assert_eq!(a.update(done(1)), BELL);
}

// Spec: Answer finishes in the focused pane of a focused window.
#[test]
fn an_answer_in_the_focused_pane_of_a_focused_window_is_silent() {
    let mut a = streaming(NotifyMode::Bell);
    assert_eq!(a.update(done(1)), vec![]);
}

// Spec: Window blurred.
#[test]
fn a_blurred_window_rings_and_a_focused_one_does_not() {
    let mut a = streaming(NotifyMode::Bell);
    assert_eq!(a.update(AppEvent::WindowFocus(false)), vec![]);
    assert_eq!(a.update(done(1)), BELL);
    a.chat_send(chat_id(), "again");
    a.update(AppEvent::WindowFocus(true));
    assert_eq!(a.update(done(2)), vec![]);
}

// Spec: Window focus is tracked; it never draws.
#[test]
fn window_focus_events_emit_nothing_and_do_not_redraw() {
    let mut a = streaming(NotifyMode::Bell);
    a.take_dirty();
    for focused in [false, true, false] {
        assert_eq!(a.update(AppEvent::WindowFocus(focused)), vec![]);
        assert!(!a.take_dirty(), "{focused}");
    }
}

// Spec: Unsupported reporting: no focus event ever arrives.
#[test]
fn without_focus_events_the_window_counts_as_focused() {
    let mut a = streaming(NotifyMode::Bell);
    assert_eq!(a.update(done(1)), vec![]);
}

// Spec: Notify off.
#[test]
fn notify_off_never_rings() {
    let mut a = streaming(NotifyMode::Off);
    a.set_focus(shell_id());
    a.update(AppEvent::WindowFocus(false));
    assert_eq!(a.update(done(1)), vec![]);
}

// Spec: Error and summary: an error ends the answer too.
#[test]
fn an_error_rings() {
    let mut a = unwatched();
    assert_eq!(a.update(error(1)), BELL);
}

#[test]
fn the_interrupted_and_truncated_ends_ring() {
    for end in [ChatError::Interrupted, ChatError::Truncated] {
        let mut a = unwatched();
        assert_eq!(a.update(event(1, StreamEvent::Error(end))), BELL);
    }
}

#[test]
fn the_answer_cap_cancels_the_engine_and_rings() {
    let mut a = unwatched();
    let big = "a".repeat(ANSWER_CAP + 1);
    assert_eq!(
        a.update(event(1, StreamEvent::Delta(big))),
        vec![Effect::ChatCancel(chat_id()), Effect::Bell]
    );
}

// Spec: Cancel and stale events.
#[test]
fn a_cancel_and_the_events_after_it_never_ring() {
    let mut a = unwatched();
    assert_eq!(
        a.chat_cancel(chat_id()),
        vec![Effect::ChatCancel(chat_id())]
    );
    for late in [done(1), error(1), event(1, StreamEvent::Delta("x".into()))] {
        assert_eq!(a.update(late), vec![]);
    }
}

#[test]
fn events_of_another_turn_do_not_ring() {
    let mut a = unwatched();
    assert_eq!(a.update(done(9)), vec![]);
    assert_eq!(a.update(error(0)), vec![]);
    assert_eq!(a.update(done(1)), BELL, "the live turn still rings");
}

// Spec: Duplicate final event.
#[test]
fn a_second_final_event_of_the_same_turn_does_not_ring_again() {
    let mut a = unwatched();
    assert_eq!(a.update(done(1)), BELL);
    assert_eq!(a.update(done(1)), vec![]);
    assert_eq!(a.update(error(1)), vec![]);
}

#[test]
fn events_that_do_not_end_the_turn_never_ring() {
    let mut a = unwatched();
    for mid in [
        StreamEvent::Slow,
        StreamEvent::Delta("lo".into()),
        StreamEvent::ThinkingDelta("hm".into()),
    ] {
        assert_eq!(a.update(event(1, mid)), vec![]);
    }
}

#[test]
fn each_answer_of_a_pane_rings_once() {
    let mut a = unwatched();
    assert_eq!(a.update(done(1)), BELL);
    a.chat_send(chat_id(), "more");
    assert_eq!(a.update(done(2)), BELL);
}

#[test]
fn a_chat_in_another_tab_rings_when_it_finishes() {
    let mut a = streaming(NotifyMode::Bell);
    a.open_chat_tab(None);
    assert_ne!(a.focused(), chat_id());
    assert_eq!(a.update(done(1)), BELL);
}

// Spec: Error and summary: a finished summary run rings under the same rule.
fn summarizing(watched: bool) -> (App, u64) {
    let mut a = with_answer(true, "We use Axum.");
    a.ai.set_notify(NotifyMode::Bell);
    a.update(AppEvent::Memory(MemReply::Project {
        pane: chat_id(),
        result: Ok(PROJECT.into()),
    }));
    let turn = match press(&mut a, "s").as_slice() {
        [Effect::ChatSend { turn, .. }] => *turn,
        other => panic!("expected one ChatSend, got {other:?}"),
    };
    a.update(event(turn, StreamEvent::Delta("- decided".into())));
    if !watched {
        a.set_focus(shell_id());
    }
    (a, turn)
}

#[test]
fn a_finished_summary_run_rings_when_unwatched() {
    let (mut a, turn) = summarizing(false);
    assert_eq!(a.update(done(turn)), BELL);
}

#[test]
fn a_failed_summary_run_rings_when_unwatched() {
    let (mut a, turn) = summarizing(false);
    assert_eq!(a.update(error(turn)), BELL);
}

#[test]
fn a_watched_summary_run_is_silent() {
    let (mut a, turn) = summarizing(true);
    assert_eq!(a.update(done(turn)), vec![]);
}

// Spec (desktop notify): auto and desktop ask the runtime to decide; the pane
// is named and the answer text never travels with the effect.
fn notice(mode: NotifyMode) -> Effect {
    Effect::Notify {
        mode,
        body: "Answer ready in pane 2".to_owned(),
        sound: true,
    }
}

#[test]
fn auto_and_desktop_emit_one_notify_and_no_bell() {
    for mode in [NotifyMode::Auto, NotifyMode::Desktop] {
        let mut a = streaming(mode);
        a.set_focus(shell_id());
        assert_eq!(a.update(done(1)), vec![notice(mode)], "{mode:?}");
    }
}

#[test]
fn a_failed_turn_says_so_and_a_watched_or_cancelled_one_is_silent() {
    let mut a = streaming(NotifyMode::Auto);
    a.set_focus(shell_id());
    assert_eq!(
        a.update(error(1)),
        vec![Effect::Notify {
            mode: NotifyMode::Auto,
            body: "Turn failed in pane 2".to_owned(),
            sound: true
        }]
    );
    let mut a = streaming(NotifyMode::Desktop);
    assert_eq!(a.update(done(1)), vec![], "watched");
}

#[test]
fn the_notify_body_never_carries_the_answer() {
    let mut a = streaming(NotifyMode::Auto);
    a.set_focus(shell_id());
    a.update(event(1, StreamEvent::Delta("SECRET-ANSWER".into())));
    match a.update(done(1)).as_slice() {
        [Effect::Notify { body, .. }] => assert!(!body.contains("SECRET"), "{body}"),
        other => panic!("expected one Notify, got {other:?}"),
    }
}

// Regression: a blurred window over the focused chat pane notifies (the case
// of switching to another OS window while the answer is still coming).
#[test]
fn a_blurred_window_over_the_focused_chat_emits_a_notify() {
    for mode in [NotifyMode::Auto, NotifyMode::Desktop] {
        let mut a = streaming(mode);
        assert_eq!(a.focused(), chat_id());
        assert_eq!(a.update(AppEvent::WindowFocus(false)), vec![]);
        assert_eq!(a.update(done(1)), vec![notice(mode)], "{mode:?}");
    }
}

#[test]
fn the_sound_flag_rides_the_effect_only_in_auto_and_desktop() {
    let mut a = streaming(NotifyMode::Auto);
    a.ai.set_notify_sound(false);
    a.set_focus(shell_id());
    assert_eq!(
        a.update(done(1)),
        vec![Effect::Notify {
            mode: NotifyMode::Auto,
            body: "Answer ready in pane 2".to_owned(),
            sound: false
        }]
    );
}
