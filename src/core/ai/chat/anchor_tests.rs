//! A pinned view stays on the same content when rows appear or vanish above
//! it: a reasoning block toggled, or a width change.

use super::engine::{Finish, StreamEvent};
use super::state_tests::{chat, sent};
use super::transcript::Scroll;
use super::*;

const HEIGHT: usize = 6;

/// A finished turn: a reasoning block of many short lines, then an answer of
/// numbered lines.
fn reasoned_chat() -> ChatState {
    let mut state = chat();
    let (turn, _) = sent(
        &mut state,
        "a question that is rather long and will wrap at narrow widths",
    );
    state.apply(turn, StreamEvent::ThinkingDelta("think ".repeat(60)));
    let answer: Vec<String> = (0..60).map(|i| format!("answer line {i}")).collect();
    state.apply(turn, StreamEvent::Delta(answer.join("\n")));
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    state
}

fn top_text(state: &ChatState, width: usize) -> String {
    let lines = state.lines(width);
    let top = state.scroll().top(lines.len(), HEIGHT);
    lines[top].text.clone()
}

fn pin_on(state: &mut ChatState, width: usize, text: &str) {
    let lines = state.lines(width);
    let row = lines
        .iter()
        .position(|l| l.text == text)
        .expect("the line exists");
    state.set_scroll(Scroll::Pinned(row));
}

#[test]
fn expanding_the_reasoning_above_keeps_the_same_line_on_top() {
    let mut state = reasoned_chat();
    pin_on(&mut state, 40, "answer line 30");
    assert!(state.toggle_reasoning_at(40, HEIGHT));
    assert_eq!(top_text(&state, 40), "answer line 30");
    assert!(state.toggle_reasoning_at(40, HEIGHT));
    assert_eq!(top_text(&state, 40), "answer line 30");
}

#[test]
fn a_followed_view_stays_followed_through_a_toggle() {
    let mut state = reasoned_chat();
    assert!(state.toggle_reasoning_at(40, HEIGHT));
    assert_eq!(state.scroll(), Scroll::Follow);
}

#[test]
fn a_width_change_above_the_viewport_keeps_the_same_line_on_top() {
    let mut state = reasoned_chat();
    assert!(state.toggle_reasoning());
    pin_on(&mut state, 60, "answer line 30");
    state.rewrap(60, 24, HEIGHT);
    assert_eq!(top_text(&state, 24), "answer line 30");
    state.rewrap(24, 80, HEIGHT);
    assert_eq!(top_text(&state, 80), "answer line 30");
}

#[test]
fn a_followed_view_stays_followed_through_a_width_change() {
    let mut state = reasoned_chat();
    state.rewrap(60, 24, HEIGHT);
    assert_eq!(state.scroll(), Scroll::Follow);
}

#[test]
fn toggling_without_a_block_changes_nothing() {
    let mut state = chat();
    state.set_scroll(Scroll::Pinned(3));
    assert!(!state.toggle_reasoning_at(40, HEIGHT));
    assert_eq!(state.scroll(), Scroll::Pinned(3));
}
