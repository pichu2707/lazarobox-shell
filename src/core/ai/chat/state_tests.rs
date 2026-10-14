use super::engine::{ChatMessage, Finish};
use super::*;

pub(super) const SYSTEM: &str = "You are helpful.";

pub(super) fn ready() -> Availability {
    Availability::Ready {
        engine: "ollama".into(),
        model: Some("llama3:8b".into()),
    }
}

pub(super) fn chat() -> ChatState {
    ChatState::new(None)
}

pub(super) fn sent(state: &mut ChatState, text: &str) -> (u64, ChatRequest) {
    match state.send(text, &ready(), SYSTEM) {
        SendOutcome::Sent { turn, request } => (turn, request),
        SendOutcome::Refused => panic!("send of {text:?} was refused"),
    }
}

/// The engine answered `text` in full and the turn is over.
fn answered(state: &mut ChatState, text: &str) {
    state.entries.push(Entry::Message(Msg {
        role: Role::Assistant,
        text: text.to_owned(),
        status: MsgStatus::Complete,
    }));
    state.phase = Phase::Idle;
}

/// The turn ended before any text: nothing was stored but the user message.
fn abandon(state: &mut ChatState) {
    state.phase = Phase::Idle;
}

pub(super) fn texts(state: &ChatState) -> Vec<(Role, String, MsgStatus)> {
    state
        .messages()
        .map(|m| (m.role, m.text.clone(), m.status))
        .collect()
}

pub(super) fn systems(state: &ChatState) -> Vec<&str> {
    state
        .entries()
        .iter()
        .filter_map(|e| match e {
            Entry::System(s) => Some(s.as_str()),
            Entry::Message(_) | Entry::Reasoning(_) | Entry::Memory(_) => None,
        })
        .collect()
}

#[test]
fn send_appends_the_user_message_and_builds_the_request() {
    let mut state = chat();
    let (turn, request) = sent(&mut state, "hello");
    assert_eq!(turn, state.turn());
    assert_eq!(
        texts(&state),
        [(Role::User, "hello".into(), MsgStatus::Complete)]
    );
    assert_eq!(request.engine, "ollama");
    assert_eq!(request.model.as_deref(), Some("llama3:8b"));
    assert_eq!(request.system, SYSTEM);
    assert_eq!(
        request.messages,
        [ChatMessage {
            role: Role::User,
            content: "hello".into()
        }]
    );
    assert_eq!(state.phase(), Phase::Waiting { slow: false });
    assert_eq!(state.waiting_text(), Some(WAITING));
}
#[test]
fn blank_and_whitespace_sends_are_refused_silently() {
    let mut state = chat();
    for text in ["", "   ", "\n\t \n"] {
        assert_eq!(state.send(text, &ready(), SYSTEM), SendOutcome::Refused);
    }
    assert!(state.entries().is_empty());
    assert_eq!(state.phase(), Phase::Idle);
    assert_eq!(state.turn(), 0);
}
#[test]
fn a_send_that_is_not_ready_changes_nothing() {
    let mut state = chat();
    let guidance = [
        Availability::Unassigned,
        Availability::Checking,
        Availability::Unsupported {
            engine: "opencode".into(),
        },
    ];
    for availability in guidance {
        assert_eq!(
            state.send("hello", &availability, SYSTEM),
            SendOutcome::Refused
        );
    }
    assert!(state.entries().is_empty());
    assert_eq!(state.phase(), Phase::Idle);
}
#[test]
fn every_send_gets_a_new_turn() {
    let mut state = chat();
    let (first, _) = sent(&mut state, "a");
    answered(&mut state, "ok");
    let (second, _) = sent(&mut state, "b");
    assert!(second > first);
}
#[test]
fn only_complete_is_not_partial() {
    use MsgStatus::*;
    let all = [Complete, Cancelled, Interrupted, Truncated, Error];
    let partial: Vec<bool> = all.iter().map(|s| s.is_partial()).collect();
    assert_eq!(partial, [false, true, true, true, true]);
    let names: Vec<&str> = all.iter().map(|s| s.as_str()).collect();
    assert_eq!(
        names,
        ["complete", "cancelled", "interrupted", "truncated", "error"]
    );
}
#[test]
fn the_conversation_holds_at_most_400_messages() {
    let mut state = chat();
    for i in 0..(MAX_MESSAGES / 2) {
        sent(&mut state, &format!("q{i}"));
        answered(&mut state, "a");
    }
    assert_eq!(state.messages().count(), MAX_MESSAGES);
    assert_eq!(
        state.send("one more", &ready(), SYSTEM),
        SendOutcome::Refused
    );
    assert_eq!(state.messages().count(), MAX_MESSAGES);
    assert_eq!(systems(&state), [TRUNCATION_MARKER, FULL]);
}
#[test]
fn the_last_exchange_always_fits_under_the_cap() {
    let mut state = chat();
    for i in 0..(MAX_MESSAGES / 2 - 1) {
        sent(&mut state, &format!("q{i}"));
        answered(&mut state, "a");
    }
    sent(&mut state, "last");
    answered(&mut state, "a");
    assert_eq!(state.messages().count(), MAX_MESSAGES);
}
#[test]
fn the_system_prompt_travels_apart_and_stays_pinned() {
    let mut state = chat();
    for i in 0..30 {
        let (_, request) = sent(&mut state, &format!("q{i}"));
        assert_eq!(request.system, SYSTEM);
        answered(&mut state, "a");
    }
    let (_, request) = sent(&mut state, "last");
    assert_eq!(request.system, SYSTEM);
    // 61 messages: the 40-message window would start on an assistant message,
    // which is dropped, so 39 are sent and the first is a user message.
    assert_eq!(request.messages.len(), 39);
    assert_eq!(request.messages[0].role, Role::User);
    assert_eq!(
        request.messages.last().map(|m| m.content.as_str()),
        Some("last")
    );
}
#[test]
fn the_marker_is_shown_once_before_the_oldest_sent_message() {
    let mut state = chat();
    for i in 0..20 {
        sent(&mut state, &format!("q{i}"));
        answered(&mut state, &format!("a{i}"));
    }
    assert!(systems(&state).is_empty(), "40 messages fit, no marker");
    let (_, request) = sent(&mut state, "q20");
    // The window of 40 would start on a0; it starts on q1 instead.
    assert_eq!(request.messages.len(), 39);
    answered(&mut state, "a20");
    let markers = |s: &ChatState| {
        s.entries()
            .iter()
            .enumerate()
            .filter(|(_, e)| **e == Entry::System(TRUNCATION_MARKER.into()))
            .map(|(i, _)| i)
            .collect::<Vec<_>>()
    };
    let at = markers(&state);
    assert_eq!(at.len(), 1);
    // 41 messages so far: q0 is trimmed and a0 would lead the window, so
    // neither is sent and the marker sits before q1.
    match &state.entries()[at[0] + 1] {
        Entry::Message(m) => assert_eq!(m.text, "q1"),
        other => panic!("marker not followed by a message: {other:?}"),
    }
    // The next send moves the marker; it is never duplicated.
    sent(&mut state, "q21");
    answered(&mut state, "a");
    let at = markers(&state);
    assert_eq!(at.len(), 1);
    match &state.entries()[at[0] + 1] {
        Entry::Message(m) => assert_eq!(m.text, "q2"),
        other => panic!("marker not followed by a message: {other:?}"),
    }
}
#[test]
fn a_short_conversation_has_no_marker() {
    let mut state = chat();
    sent(&mut state, "hi");
    answered(&mut state, "a");
    sent(&mut state, "again");
    assert!(systems(&state).is_empty());
}
#[test]
fn a_cli_engine_gets_the_large_character_cap() {
    let mut state = chat();
    let cli = Availability::Ready {
        engine: "claude".into(),
        model: None,
    };
    let big = "x".repeat(8_000);
    for _ in 0..3 {
        let SendOutcome::Sent { .. } = state.send(&big, &cli, SYSTEM) else {
            panic!("refused")
        };
        answered(&mut state, &big);
    }
    let SendOutcome::Sent { request, .. } = state.send("q", &cli, SYSTEM) else {
        panic!("refused")
    };
    assert_eq!(request.messages.len(), 7);
    assert!(!systems(&state).contains(&TRUNCATION_MARKER));
    assert_eq!(request.model, None);
}
#[test]
fn a_user_message_cancelled_while_waiting_is_not_sent_again() {
    let mut state = chat();
    sent(&mut state, "first");
    abandon(&mut state);
    let (_, request) = sent(&mut state, "second");
    let sent_texts: Vec<_> = request
        .messages
        .iter()
        .map(|m| m.content.as_str())
        .collect();
    assert_eq!(sent_texts, ["second"]);
    assert!(request.messages.iter().all(|m| m.role == Role::User));
    let kept: Vec<_> = state.messages().map(|m| m.text.as_str()).collect();
    assert_eq!(kept, ["first", "second"], "still in the transcript");
}
#[test]
fn the_marker_stays_right_when_an_orphan_is_left_out() {
    let mut state = chat();
    sent(&mut state, "orphan");
    abandon(&mut state);
    for i in 0..20 {
        sent(&mut state, &format!("q{i}"));
        answered(&mut state, &format!("a{i}"));
    }
    let (_, request) = sent(&mut state, "last");
    assert_eq!(request.messages.len(), 39);
    let at = state
        .entries()
        .iter()
        .position(|e| *e == Entry::System(TRUNCATION_MARKER.into()))
        .expect("marker");
    // 20 exchanges plus "last" make 41 sendable messages: q0 is left out, and
    // so is a0, which would lead the window.
    match &state.entries()[at + 1] {
        Entry::Message(m) => assert_eq!(m.text, "q1"),
        other => panic!("marker not followed by a message: {other:?}"),
    }
}
#[test]
fn repeated_refusals_do_not_stack_system_lines() {
    let mut state = chat();
    sent(&mut state, "one");
    for _ in 0..3 {
        assert_eq!(state.send("two", &ready(), SYSTEM), SendOutcome::Refused);
    }
    assert_eq!(systems(&state), [BUSY]);
}
#[test]
fn repeated_sends_into_a_full_conversation_do_not_stack_lines() {
    let mut state = chat();
    for i in 0..(MAX_MESSAGES / 2) {
        sent(&mut state, &format!("q{i}"));
        answered(&mut state, "a");
    }
    for _ in 0..3 {
        state.send("more", &ready(), SYSTEM);
    }
    assert_eq!(systems(&state), [TRUNCATION_MARKER, FULL]);
}
#[test]
fn an_odd_message_count_is_full_when_the_exchange_does_not_fit() {
    let mut state = chat();
    for i in 0..(MAX_MESSAGES / 2 - 1) {
        sent(&mut state, &format!("q{i}"));
        answered(&mut state, "a");
    }
    sent(&mut state, "cancelled");
    abandon(&mut state);
    assert_eq!(state.messages().count(), MAX_MESSAGES - 1);
    assert_eq!(state.send("more", &ready(), SYSTEM), SendOutcome::Refused);
    assert_eq!(state.messages().count(), MAX_MESSAGES - 1);
    assert_eq!(systems(&state).last(), Some(&FULL));
}
#[test]
fn a_refused_send_leaves_the_turn_message_last_among_the_messages() {
    // The in-flight message is the last `Entry::Message`, not the last entry:
    // a refused send appends a system line after it.
    let mut state = chat();
    sent(&mut state, "one");
    assert_eq!(state.send("two", &ready(), SYSTEM), SendOutcome::Refused);
    assert_eq!(state.entries().last(), Some(&Entry::System(BUSY.into())));
    assert_eq!(state.phase(), Phase::Waiting { slow: false });
    let last_message = state.messages().last().map(|m| m.text.as_str());
    assert_eq!(last_message, Some("one"));
    assert_eq!(state.messages().count(), 1);
}

#[test]
fn each_chat_owns_its_composer() {
    let mut a = chat();
    let b = chat();
    assert!(a.composer().is_empty());
    a.composer_mut().paste("draft");
    assert_eq!(a.composer().text(), "draft");
    assert!(b.composer().is_empty(), "another chat is unaffected");
}

#[test]
fn the_composer_survives_a_send_of_other_text() {
    let mut state = chat();
    state.composer_mut().paste("next");
    sent(&mut state, "hello");
    assert_eq!(
        state.composer().text(),
        "next",
        "send takes text, not the box"
    );
}

#[test]
fn a_status_name_round_trips_and_an_unknown_one_is_none() {
    for status in [
        MsgStatus::Complete,
        MsgStatus::Cancelled,
        MsgStatus::Interrupted,
        MsgStatus::Truncated,
        MsgStatus::Error,
    ] {
        assert_eq!(MsgStatus::from_name(status.as_str()), Some(status));
    }
    assert_eq!(MsgStatus::from_name("done"), None);
}

#[test]
fn a_notice_is_one_system_line_per_pane_however_many_turns_say_it() {
    let mut state = chat();
    let line = "claude runs without the jail here: no Landlock";
    let (turn, _) = sent(&mut state, "one");
    state.apply(turn, StreamEvent::Notice(line.into()));
    state.apply(turn, StreamEvent::Notice(line.into()));
    state.apply(turn, StreamEvent::Delta("a".into()));
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    let (turn, _) = sent(&mut state, "two");
    state.apply(turn, StreamEvent::Notice(line.into()));
    state.apply(turn, StreamEvent::Delta("b".into()));
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    assert_eq!(systems(&state), [line]);
}

#[test]
fn a_notice_does_not_start_the_answer_or_end_the_turn() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "hi");
    state.apply(turn, StreamEvent::Notice("x".into()));
    assert_eq!(state.phase(), Phase::Waiting { slow: false });
    assert_eq!(texts(&state).len(), 1, "only the user message");
}
