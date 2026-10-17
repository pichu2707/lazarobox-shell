//! Tool rows in the chat state (spec chat-pane "Tool rows", chat-engines
//! "Tool events"): paired once, made only from host events, never stored or
//! sent.

use super::engine::{CallId, Finish, ToolStatus};
use super::state_tests::{SYSTEM, chat, ready, sent};
use super::transcript::{self, LineKind};
use super::*;

fn call(id: u64, name: &str, summary: &str) -> StreamEvent {
    StreamEvent::ToolCall {
        id: CallId(id),
        name: name.into(),
        summary: summary.into(),
        approval: None,
    }
}

fn result(id: u64, status: ToolStatus, summary: &str) -> StreamEvent {
    StreamEvent::ToolResult {
        id: CallId(id),
        status,
        summary: summary.into(),
    }
}

fn rows(state: &ChatState) -> Vec<&tool::ToolRow> {
    state
        .entries()
        .iter()
        .filter_map(|e| match e {
            Entry::Tool(row) => Some(row),
            _ => None,
        })
        .collect()
}

fn delta(text: &str) -> StreamEvent {
    StreamEvent::Delta(text.into())
}

#[test]
fn a_call_and_its_result_make_one_paired_row() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "find it");
    state.apply(turn, call(1, "find_dir", "curso-rust"));
    let pending = rows(&state);
    assert_eq!(pending.len(), 1);
    assert_eq!(
        (pending[0].name(), pending[0].summary()),
        ("find_dir", "curso-rust")
    );
    assert_eq!(pending[0].result(), None);
    state.apply(turn, result(1, ToolStatus::Ok, "1 result"));
    let done = rows(&state);
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].result(), Some((ToolStatus::Ok, "1 result")));
}

#[test]
fn a_result_is_paired_once_and_only_with_a_known_call() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    state.apply(turn, call(1, "list_dir", "~"));
    state.apply(turn, result(2, ToolStatus::Ok, "stranger"));
    assert_eq!(rows(&state)[0].result(), None);
    state.apply(turn, result(1, ToolStatus::Denied, "first"));
    state.apply(turn, result(1, ToolStatus::Ok, "second"));
    assert_eq!(
        rows(&state)[0].result(),
        Some((ToolStatus::Denied, "first"))
    );
    assert_eq!(rows(&state).len(), 1);
}

#[test]
fn a_call_announced_twice_is_one_row() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    state.apply(turn, call(7, "find_dir", "a"));
    state.apply(turn, call(7, "find_dir", "b"));
    let all = rows(&state);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].summary(), "a");
}

#[test]
fn a_tool_event_counts_as_activity_and_a_row_does_not_start_the_answer() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    state.apply(turn, StreamEvent::Slow);
    assert!(state.waiting_text().is_some());
    state.apply(turn, call(1, "find_dir", "x"));
    assert_eq!(state.phase(), Phase::Waiting { slow: false });
    assert_eq!(state.messages().count(), 1);
}

#[test]
fn a_row_sits_before_the_answer_it_belongs_to() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    state.apply(turn, call(1, "find_dir", "x"));
    state.apply(turn, result(1, ToolStatus::Ok, "ok"));
    state.apply(turn, delta("Found"));
    // A second call in the middle of the answer is still shown above it, and
    // the answer stays one message.
    state.apply(turn, call(2, "list_dir", "~"));
    state.apply(turn, delta(" it."));
    state.apply(turn, result(2, ToolStatus::Ok, "ok"));
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    let kinds: Vec<&str> = state
        .entries()
        .iter()
        .map(|e| match e {
            Entry::Message(m) if m.role == Role::User => "user",
            Entry::Message(_) => "assistant",
            Entry::Tool(_) => "tool",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["user", "tool", "tool", "assistant"]);
    let answer = state.messages().last().unwrap();
    assert_eq!(answer.text, "Found it.");
}

#[test]
fn a_call_that_never_got_its_result_ends_as_an_error_row() {
    for end in [
        StreamEvent::Done(Finish::Stop),
        StreamEvent::Error(ChatError::Interrupted),
    ] {
        let mut state = chat();
        let (turn, _) = sent(&mut state, "go");
        state.apply(turn, call(1, "find_dir", "x"));
        state.apply(turn, end);
        assert_eq!(
            rows(&state)[0].result(),
            Some((ToolStatus::Error, tool::NO_RESULT))
        );
    }
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    state.apply(turn, call(1, "find_dir", "x"));
    state.cancel();
    assert_eq!(
        rows(&state)[0].result(),
        Some((ToolStatus::Error, tool::NO_RESULT))
    );
}

#[test]
fn tool_events_of_another_turn_or_after_the_end_change_nothing() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    state.apply(turn, call(1, "find_dir", "x"));
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    let before = state.entries().to_vec();
    state.apply(turn, result(1, ToolStatus::Ok, "late"));
    state.apply(turn, call(2, "find_dir", "late"));
    state.apply(turn + 1, call(3, "find_dir", "other"));
    assert_eq!(state.entries(), before);
}

#[test]
fn rows_are_not_stored_not_sent_and_not_counted() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "first");
    state.apply(turn, call(1, "find_dir", "x"));
    state.apply(turn, result(1, ToolStatus::Ok, "ok"));
    state.apply(turn, delta("answer"));
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    let stored = std::mem::take(&mut state.outbox);
    let saved: Vec<_> = stored.iter().map(|m| (m.role, &*m.text)).collect();
    assert_eq!(saved, [(Role::User, "first"), (Role::Assistant, "answer")]);
    assert_eq!(state.messages().count(), 2);
    let (_, request) = sent(&mut state, "second");
    let roles: Vec<_> = request
        .messages
        .iter()
        .map(|m| (m.role, &*m.content))
        .collect();
    assert_eq!(
        roles,
        [
            (Role::User, "first"),
            (Role::Assistant, "answer"),
            (Role::User, "second")
        ]
    );
}

#[test]
fn text_that_imitates_a_row_is_plain_answer_text() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    let fake = "» find_dir curso-rust\n  ok: 1 result";
    state.apply(turn, delta(fake));
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    assert!(rows(&state).is_empty());
    let lines = transcript::build(&state, 60);
    let imitation: Vec<_> = lines
        .iter()
        .filter(|l| l.text.contains("find_dir"))
        .collect();
    assert_eq!(imitation.len(), 1);
    assert_eq!(imitation[0].kind, LineKind::Assistant);
    assert!(
        lines
            .iter()
            .all(|l| !matches!(l.kind, LineKind::Tool | LineKind::ToolAlert))
    );
}

fn texts_of(state: &ChatState, width: usize) -> Vec<(LineKind, String)> {
    transcript::build(state, width)
        .into_iter()
        .filter(|l| matches!(l.kind, LineKind::Tool | LineKind::ToolAlert))
        .map(|l| (l.kind, l.text))
        .collect()
}

#[test]
fn a_call_row_and_a_result_row_read_as_a_pair() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    state.apply(turn, call(1, "find_dir", "curso-rust"));
    assert_eq!(
        texts_of(&state, 40),
        [(LineKind::Tool, "» find_dir curso-rust".to_owned())]
    );
    state.apply(turn, result(1, ToolStatus::Ok, "1 result"));
    assert_eq!(
        texts_of(&state, 40),
        [
            (LineKind::Tool, "» find_dir curso-rust".to_owned()),
            (LineKind::Tool, "  ok: 1 result".to_owned()),
        ]
    );
}

#[test]
fn error_and_denied_results_draw_as_alerts() {
    for (status, word) in [(ToolStatus::Error, "error"), (ToolStatus::Denied, "denied")] {
        let mut state = chat();
        let (turn, _) = sent(&mut state, "go");
        state.apply(turn, call(1, "run_in_pane", "ls"));
        state.apply(turn, result(1, status, ""));
        assert_eq!(
            texts_of(&state, 40)[1],
            (LineKind::ToolAlert, format!("  {word}"))
        );
    }
}

#[test]
fn row_text_is_literal_wrapped_and_never_code() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "go");
    state.apply(turn, call(1, "read_file", "```\n# x"));
    state.apply(turn, result(1, ToolStatus::Ok, "```"));
    let lines = transcript::build(&state, 12);
    assert!(lines.iter().all(|l| l.kind != LineKind::Code));
    let rows = texts_of(&state, 12);
    assert!(
        rows.iter().all(|(_, t)| t.chars().count() <= 12),
        "{rows:?}"
    );
    assert!(rows[0].1.starts_with("» read_file"));
}

#[test]
fn the_locality_of_an_engine_comes_from_its_adapter_kind() {
    use crate::core::ai::tools::Locality;
    assert_eq!(engine::locality("ollama"), Some(Locality::Local));
    for cli in ["claude", "codex", "gemini", "opencode"] {
        assert_eq!(engine::locality(cli), Some(Locality::Cloud), "{cli}");
    }
    assert_eq!(engine::locality("nothing"), None);
    // The model name is not an input: a request for ollama with a cloud-like
    // model, or a CLI with a local-like one, keeps its engine's locality.
    let request = |engine: &str, model: &str| ChatRequest {
        engine: engine.into(),
        model: Some(model.into()),
        system: SYSTEM.into(),
        memory: None,
        tools: Tools::Off,
        messages: Vec::new(),
    };
    for (req, want) in [
        (request("ollama", "claude-sonnet"), Locality::Local),
        (request("claude", "llama3:8b"), Locality::Cloud),
    ] {
        assert_eq!(engine::locality(&req.engine), Some(want));
    }
    let _ = ready();
}
