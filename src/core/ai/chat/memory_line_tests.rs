//! The folded memory line at the top of the transcript (engram-memory 10.4).

use super::{
    engine::{Finish, StreamEvent},
    state_tests::{chat, sent},
    transcript::{LineKind, Scroll},
    *,
};
use crate::core::ai::memory::context::ContextBlock;

fn block(notes: usize, data: &str) -> ContextBlock {
    let text =
        format!("<project-memory source=\"engram\" project=\"p\">\n{data}\n</project-memory>");
    ContextBlock {
        bytes: text.len(),
        notes,
        text,
    }
}

fn rows(state: &ChatState) -> Vec<(LineKind, String)> {
    state
        .lines(60)
        .iter()
        .map(|l| (l.kind, l.text.clone()))
        .collect()
}

fn first(state: &ChatState) -> (LineKind, String) {
    rows(state).remove(0)
}

fn answered(state: &mut ChatState, text: &str) {
    let (turn, _) = sent(state, "q");
    state.apply(turn, StreamEvent::Delta(text.into()));
    state.apply(turn, StreamEvent::Done(Finish::Stop));
}

#[test]
fn a_block_with_notes_shows_a_folded_line_before_the_messages() {
    let mut state = chat();
    answered(&mut state, "hello");
    state.inject_memory(block(3, "- [decision] **Use Axum**"));
    assert_eq!(
        first(&state),
        (LineKind::ReasoningHead, "▸ memory (3 notes)".to_owned())
    );
    assert!(matches!(state.entries()[0], Entry::Memory(_)));
    assert!(matches!(state.entries()[1], Entry::Message(_)));
}

#[test]
fn one_note_is_singular() {
    let mut state = chat();
    state.inject_memory(block(1, "- [decision] **Use Axum**"));
    assert_eq!(first(&state).1, "▸ memory (1 note)");
}

#[test]
fn a_block_without_notes_is_injected_but_draws_no_line() {
    let mut state = chat();
    state.inject_memory(block(0, "### Recent Sessions\n- **s** ended"));
    assert!(state.entries().is_empty());
    assert!(rows(&state).is_empty());
    let request = match state.send("hi", &state_tests::ready(), "") {
        SendOutcome::Sent { request, .. } => request,
        SendOutcome::Refused => panic!("refused"),
    };
    assert!(request.memory.is_some(), "the model still gets it");
}

#[test]
fn no_block_no_line() {
    let mut state = chat();
    answered(&mut state, "hello");
    assert!(
        !state
            .entries()
            .iter()
            .any(|e| matches!(e, Entry::Memory(_)))
    );
}

#[test]
fn the_toggle_expands_to_the_injected_text_behind_the_gutter_and_folds_again() {
    let mut state = chat();
    state.inject_memory(block(3, "- decision: use Axum"));
    assert!(state.toggle_memory());
    let expanded = rows(&state);
    assert_eq!(
        expanded[0],
        (LineKind::ReasoningHead, "▾ memory (3 notes)".to_owned())
    );
    assert_eq!(
        expanded[1],
        (
            LineKind::Reasoning,
            "┊ <project-memory source=\"engram\" project=\"p\">".to_owned()
        )
    );
    assert!(
        expanded
            .iter()
            .any(|r| *r == (LineKind::Reasoning, "┊ - decision: use Axum".to_owned()))
    );
    assert!(state.toggle_memory());
    assert_eq!(
        rows(&state),
        [(LineKind::ReasoningHead, "▸ memory (3 notes)".to_owned())]
    );
}

#[test]
fn the_injected_text_is_shown_literally() {
    let mut state = chat();
    state.inject_memory(block(1, "**not bold** `code` # not a heading"));
    state.toggle_memory();
    let shown = rows(&state);
    assert!(
        shown
            .iter()
            .any(|r| r.1 == "┊ **not bold** `code` # not a heading" && r.0 == LineKind::Reasoning),
        "{shown:?}"
    );
}

#[test]
fn the_toggle_with_no_line_changes_nothing() {
    let mut state = chat();
    answered(&mut state, "hello");
    let before = (state.entries().to_vec(), state.generation());
    assert!(!state.toggle_memory());
    assert!(!state.toggle_memory_at(40, 6));
    assert_eq!((state.entries().to_vec(), state.generation()), before);
}

#[test]
fn the_toggle_never_touches_the_reasoning_block_and_the_other_way_round() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "q");
    state.apply(turn, StreamEvent::ThinkingDelta("deep thought".into()));
    state.apply(turn, StreamEvent::Delta("answer".into()));
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    state.inject_memory(block(2, "- a"));
    assert!(state.toggle_memory());
    let reasoning = state.entries().iter().find_map(|e| match e {
        Entry::Reasoning(r) => Some(r.state()),
        _ => None,
    });
    assert_eq!(reasoning, Some(ReasoningState::Collapsed));
    assert!(state.toggle_reasoning());
    let memory_open = state.entries().iter().any(|e| match e {
        Entry::Memory(line) => line.expanded(),
        _ => false,
    });
    assert!(memory_open, "Ctrl+R left the memory line as it was");
}

#[test]
fn the_line_is_not_a_message() {
    let mut state = chat();
    answered(&mut state, "hello");
    let before = state.messages().count();
    state.inject_memory(block(3, "- a"));
    assert_eq!(state.messages().count(), before);
    assert_eq!(yank::last_answer(&state).as_deref(), Some("hello"));
    // Nor does it take room from the 400 messages.
    let (turn, request) = sent(&mut state, "again");
    assert_eq!(request.messages.len(), 3, "q, hello, again");
    state.apply(turn, StreamEvent::Done(Finish::Stop));
}

#[test]
fn a_second_block_replaces_the_line() {
    let mut state = chat();
    state.inject_memory(block(2, "- a"));
    state.inject_memory(block(4, "- a\n- b"));
    let lines: Vec<_> = state
        .entries()
        .iter()
        .filter(|e| matches!(e, Entry::Memory(_)))
        .collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(first(&state).1, "▸ memory (4 notes)");
}

#[test]
fn a_late_line_keeps_a_pinned_view_on_the_same_content() {
    let mut state = chat();
    for i in 0..30 {
        answered(&mut state, &format!("answer {i}"));
    }
    let lines = state.lines(40);
    let row = lines
        .iter()
        .position(|l| l.text == "answer 10")
        .expect("the line exists");
    state.set_scroll(Scroll::Pinned(row));
    state.inject_memory_at(block(3, "- a"), 40, 6);
    let lines = state.lines(40);
    let top = state.scroll().top(lines.len(), 6);
    assert_eq!(lines[top].text, "answer 10");
}
