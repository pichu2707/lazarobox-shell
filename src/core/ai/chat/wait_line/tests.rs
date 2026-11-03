//! The wait line names what the turn waits for and, for a server turn whose
//! clock said `Slow`, how long and until when.

use std::time::{Duration, Instant};

use super::super::{
    approval::{ApprovalAsk, Origin},
    engine::{CallId, Finish, ToolStatus},
    state_tests::{SYSTEM, chat, sent},
    transcript::{self, LineKind},
    *,
};
use super::*;

fn t0() -> Instant {
    use std::sync::OnceLock;
    static BASE: OnceLock<Instant> = OnceLock::new();
    *BASE.get_or_init(Instant::now)
}

fn at(secs: u64) -> Instant {
    t0() + Duration::from_secs(secs)
}

fn call(id: u64, name: &str) -> StreamEvent {
    StreamEvent::ToolCall {
        id: CallId(id),
        name: name.into(),
        summary: "x".into(),
        approval: None,
    }
}

fn done(id: u64) -> StreamEvent {
    StreamEvent::ToolResult {
        id: CallId(id),
        status: ToolStatus::Ok,
        summary: "ok".into(),
    }
}

fn text(state: &ChatState) -> Option<String> {
    state.waiting_text().map(|t| t.into_owned())
}

/// A turn on `engine`, whose clock said `Slow` at second `slow_at`.
fn slow_turn(engine: &str, slow_at: u64) -> (ChatState, u64) {
    let mut state = chat();
    let availability = Availability::Ready {
        engine: engine.into(),
        model: Some("m".into()),
    };
    let turn = match state.send("hi", &availability, SYSTEM) {
        SendOutcome::Sent { turn, .. } => turn,
        SendOutcome::Refused => panic!("refused"),
    };
    state.apply_at(turn, StreamEvent::Slow, at(slow_at));
    (state, turn)
}

#[test]
fn the_limit_is_the_constant_the_runtime_clock_reads() {
    assert_eq!(FIRST_BYTE_LIMIT, Duration::from_secs(120));
    assert_eq!(SLOW_AFTER, Duration::from_secs(3));
}

#[test]
fn the_model_wait_is_the_default() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "hi");
    assert_eq!(text(&state).as_deref(), Some(WAITING));
    state.apply(turn, StreamEvent::Slow);
    assert_eq!(text(&state).as_deref(), Some(STILL_WAITING));
}

#[test]
fn a_pending_approval_says_it_waits_for_the_user() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "hi");
    state
        .approvals_mut()
        .request_call(ApprovalAsk::read("/x"), turn, CallId(1), None)
        .unwrap();
    assert_eq!(text(&state).as_deref(), Some(WAITING_USER));
    let id = state.approvals().pending().unwrap().id();
    state.approvals_mut().withdraw(id);
    assert_eq!(text(&state).as_deref(), Some(WAITING));
}

#[test]
fn a_running_tool_is_named_until_its_result() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "hi");
    state.apply(turn, call(1, "read_file"));
    assert_eq!(text(&state).as_deref(), Some("running read_file…"));
    state.apply(turn, done(1));
    assert_eq!(text(&state).as_deref(), Some(WAITING));
}

#[test]
fn the_approval_wins_over_the_tool_that_waits_for_it() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "hi");
    state.apply(turn, call(1, "read_file"));
    state
        .approvals_mut()
        .request_call(ApprovalAsk::read("/x"), turn, CallId(1), None)
        .unwrap();
    assert_eq!(text(&state).as_deref(), Some(WAITING_USER));
}

#[test]
fn a_label_change_rebuilds_the_cached_lines() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "hi");
    let before = state.generation();
    state
        .approvals_mut()
        .request_call(ApprovalAsk::read("/x"), turn, CallId(1), None)
        .unwrap();
    assert_ne!(state.generation(), before);
    let last = state.lines(80).last().cloned().unwrap();
    assert_eq!(
        (last.kind, last.text.as_str()),
        (LineKind::Waiting, WAITING_USER)
    );
}

#[test]
fn there_are_no_seconds_before_the_clock_says_slow() {
    let mut state = chat();
    sent(&mut state, "hi");
    assert_eq!(state.wait_elapsed(at(2)), None);
}

#[test]
fn the_seconds_count_from_the_slow_event_minus_its_delay() {
    let (state, _) = slow_turn("ollama", 10);
    // The clock said Slow at 3 s: it started at second 7.
    assert_eq!(state.wait_elapsed(at(10)), Some(Duration::from_secs(3)));
    assert_eq!(state.wait_elapsed(at(52)), Some(Duration::from_secs(45)));
}

#[test]
fn a_tool_call_ends_the_count_and_the_next_slow_starts_it_again() {
    let (mut state, turn) = slow_turn("ollama", 10);
    state.apply_at(turn, call(1, "list_dir"), at(20));
    assert_eq!(state.wait_elapsed(at(21)), None);
    state.apply_at(turn, done(1), at(22));
    assert_eq!(state.wait_elapsed(at(23)), None);
    state.apply_at(turn, StreamEvent::Slow, at(40));
    assert_eq!(state.wait_elapsed(at(41)), Some(Duration::from_secs(4)));
}

#[test]
fn the_time_held_for_the_user_is_not_counted() {
    let (mut state, turn) = slow_turn("ollama", 10);
    state.observe_hold(at(20));
    state
        .approvals_mut()
        .request_call(ApprovalAsk::read("/x"), turn, CallId(1), None)
        .unwrap();
    state.observe_hold(at(30));
    assert_eq!(
        state.wait_elapsed(at(35)),
        None,
        "no seconds under a prompt"
    );
    let id = state.approvals().pending().unwrap().id();
    state.approvals_mut().withdraw(id);
    state.observe_hold(at(50));
    // Started at 7, held from 30 to 50: at 60 the clock shows 60 - 7 - 20.
    assert_eq!(state.wait_elapsed(at(60)), Some(Duration::from_secs(33)));
}

#[test]
fn an_approval_of_the_user_does_not_pause_the_clock() {
    let (mut state, _) = slow_turn("ollama", 10);
    state
        .approvals_mut()
        .request(ApprovalAsk::read("/x"), Origin::Block)
        .unwrap();
    state.observe_hold(at(20));
    let id = state.approvals().pending().unwrap().id();
    state.approvals_mut().withdraw(id);
    state.observe_hold(at(40));
    assert_eq!(state.wait_elapsed(at(52)), Some(Duration::from_secs(45)));
}

#[test]
fn a_cli_turn_states_no_seconds_and_no_limit() {
    for engine in ["claude", "codex", "gemini"] {
        let (state, _) = slow_turn(engine, 10);
        assert_eq!(text(&state).as_deref(), Some(STILL_WAITING), "{engine}");
        assert_eq!(state.wait_elapsed(at(30)), None, "{engine}");
    }
}

#[test]
fn the_suffix_gives_what_fits() {
    let secs = Duration::from_secs(45);
    assert_eq!(
        suffix(secs, FIRST_BYTE_LIMIT, 40).as_deref(),
        Some(" 45s (gives up at 120s)")
    );
    assert_eq!(suffix(secs, FIRST_BYTE_LIMIT, 22).as_deref(), Some(" 45s"));
    assert_eq!(suffix(secs, FIRST_BYTE_LIMIT, 4).as_deref(), Some(" 45s"));
    assert_eq!(suffix(secs, FIRST_BYTE_LIMIT, 3), None);
}

#[test]
fn the_wait_row_keeps_its_height_whatever_the_width() {
    let (state, _) = slow_turn("ollama", 10);
    for width in [12, 20, 28, 40, 80] {
        let rows: Vec<_> = transcript::build(&state, width)
            .into_iter()
            .filter(|l| l.kind == LineKind::Waiting)
            .collect();
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|r| r.text.chars().count() <= width));
    }
}

/// A chat whose first turn on `turn_engine` said `Slow` at second 10 and then
/// ended with an answer, with a summary of it begun at second 500 on
/// `summary_engine`.
fn summary_after_slow_turn(turn_engine: &str, summary_engine: &str) -> (ChatState, u64) {
    let (mut state, turn) = slow_turn(turn_engine, 10);
    state.apply_at(turn, StreamEvent::Delta("a".into()), at(20));
    state.apply_at(turn, StreamEvent::Done(Finish::Stop), at(21));
    let availability = Availability::Ready {
        engine: summary_engine.into(),
        model: Some("m".into()),
    };
    let (turn, _) = state
        .begin_summary_at(&availability, at(500))
        .expect("a summary request");
    (state, turn)
}

#[test]
fn a_summary_does_not_inherit_the_anchor_of_an_earlier_turn() {
    let (mut state, turn) = summary_after_slow_turn("ollama", "ollama");
    state.apply_at(turn, StreamEvent::Slow, at(510));
    // The summary's clock said Slow at 510 (started at 507), not at 7.
    assert_eq!(state.wait_elapsed(at(512)), Some(Duration::from_secs(5)));
}

#[test]
fn the_seconds_of_a_summary_count_from_its_own_slow_event() {
    let (mut state, turn) = summary_after_slow_turn("ollama", "llamacpp");
    assert_eq!(state.wait_elapsed(at(505)), None, "not slow yet");
    state.apply_at(turn, StreamEvent::Slow, at(530));
    assert_eq!(state.wait_elapsed(at(530)), Some(Duration::from_secs(3)));
    assert_eq!(state.wait_elapsed(at(575)), Some(Duration::from_secs(48)));
}

#[test]
fn a_summary_on_a_cli_engine_states_no_seconds() {
    // The turn engine is a server, so a stale request engine would not hide them.
    for engine in ["claude", "codex", "gemini"] {
        let (mut state, turn) = summary_after_slow_turn("ollama", engine);
        state.apply_at(turn, StreamEvent::Slow, at(510));
        assert_eq!(text(&state).as_deref(), Some(STILL_WAITING), "{engine}");
        assert_eq!(state.wait_elapsed(at(520)), None, "{engine}");
    }
}

#[test]
fn a_server_summary_after_a_cli_turn_states_its_seconds() {
    let (mut state, turn) = summary_after_slow_turn("claude", "ollama");
    state.apply_at(turn, StreamEvent::Slow, at(510));
    assert_eq!(state.wait_elapsed(at(512)), Some(Duration::from_secs(5)));
}

#[test]
fn no_run_leaves_its_clock_behind() {
    // The end of a turn.
    let (mut state, turn) = slow_turn("ollama", 10);
    state.apply_at(turn, StreamEvent::Done(Finish::Stop), at(20));
    assert_eq!(state.wait_since, None, "turn end");
    // The end of a summary and its cancellation.
    let (mut state, turn) = summary_after_slow_turn("ollama", "ollama");
    state.apply_at(turn, StreamEvent::Slow, at(510));
    state.apply_at(turn, StreamEvent::Done(Finish::Stop), at(520));
    assert_eq!(state.wait_since, None, "summary end");
    let (mut state, turn) = summary_after_slow_turn("ollama", "ollama");
    state.apply_at(turn, StreamEvent::Slow, at(510));
    state.cancel_at(at(520));
    assert_eq!(state.wait_since, None, "summary cancelled");
}

#[test]
fn the_limit_is_the_first_byte_one_until_a_read_restarts_the_idle_clock() {
    let (state, _) = slow_turn("ollama", 10);
    assert_eq!(state.wait_limit(), FIRST_BYTE_LIMIT);
}

#[test]
fn a_read_after_slow_moves_the_limit_to_the_idle_clock_from_that_read() {
    let (mut state, turn) = slow_turn("ollama", 10);
    // Anchor at 7; the headers arrive at 20: the turn dies at 20 + 60.
    state.apply_at(turn, StreamEvent::Reading, at(20));
    assert_eq!(state.wait_limit(), Duration::from_secs(13 + 60));
    // A keep-alive at 50 restarts it.
    state.apply_at(turn, StreamEvent::Reading, at(50));
    assert_eq!(state.wait_limit(), Duration::from_secs(43 + 60));
    // The seconds keep counting from the same anchor and the line is the same.
    assert_eq!(state.wait_elapsed(at(52)), Some(Duration::from_secs(45)));
    assert_eq!(text(&state).as_deref(), Some(STILL_WAITING));
}

#[test]
fn a_read_is_not_a_byte_for_the_model() {
    let (mut state, turn) = slow_turn("ollama", 10);
    let generation = state.generation();
    state.apply_at(turn, StreamEvent::Reading, at(20));
    assert_eq!(state.phase(), Phase::Waiting { slow: true });
    assert_eq!(state.generation(), generation);
    assert!(state.wait_elapsed(at(21)).is_some());
}

#[test]
fn a_read_before_slow_changes_nothing() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "hi");
    state.apply_at(turn, StreamEvent::Reading, at(1));
    state.apply_at(turn, StreamEvent::Slow, at(10));
    assert_eq!(state.wait_limit(), FIRST_BYTE_LIMIT);
}

#[test]
fn a_new_request_starts_from_the_first_byte_limit_again() {
    let (mut state, turn) = slow_turn("ollama", 10);
    state.apply_at(turn, StreamEvent::Reading, at(20));
    // A tool round, then the next request that is slow in its turn.
    state.apply_at(turn, call(1, "list_dir"), at(30));
    assert_eq!(state.wait_limit(), FIRST_BYTE_LIMIT);
    state.apply_at(turn, done(1), at(31));
    state.apply_at(turn, StreamEvent::Slow, at(40));
    state.apply_at(turn, StreamEvent::Reading, at(45));
    assert_eq!(state.wait_limit(), Duration::from_secs(8 + 60));
    state.apply_at(turn, call(2, "list_dir"), at(50));
    state.apply_at(turn, done(2), at(51));
    state.apply_at(turn, StreamEvent::Slow, at(60));
    assert_eq!(state.wait_limit(), FIRST_BYTE_LIMIT);
}

#[test]
fn the_limit_of_a_summary_follows_its_own_reads() {
    let (mut state, turn) = summary_after_slow_turn("ollama", "ollama");
    state.apply_at(turn, StreamEvent::Slow, at(510));
    state.apply_at(turn, StreamEvent::Reading, at(520));
    assert_eq!(state.wait_limit(), Duration::from_secs(13 + 60));
    // A read in the summary of another run never leaks: the clock resets.
    state.apply_at(turn, StreamEvent::Done(Finish::Stop), at(530));
    assert_eq!(state.wait_limit(), FIRST_BYTE_LIMIT);
}

#[test]
fn a_cli_turn_still_states_no_seconds_after_a_read() {
    let (mut state, turn) = slow_turn("claude", 10);
    state.apply_at(turn, StreamEvent::Reading, at(20));
    assert_eq!(state.wait_elapsed(at(30)), None);
}
