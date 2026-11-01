use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{
    ChatState, Entry, Follow, JAIL_ERROR, MsgStatus, Phase,
    engine::{ChatError, Finish, Role, StreamEvent},
    memory::*,
    state_tests::{chat, ready, sent},
    summary::*,
};

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn typed(draft: &mut SummaryDraft, text: &str) {
    for c in text.chars() {
        draft.on_key(&press(KeyCode::Char(c)), false);
    }
}

/// A chat with two answered turns.
fn talked() -> ChatState {
    let mut state = chat();
    for (q, a) in [("first q", "first a"), ("second q", "second a")] {
        let (turn, _) = sent(&mut state, q);
        state.apply(turn, StreamEvent::Delta(a.into()));
        state.apply(turn, StreamEvent::Done(Finish::Stop));
    }
    state
}

/// The summary run started on `state`: its turn.
fn started(state: &mut ChatState) -> u64 {
    state.begin_summary(&ready()).expect("a request").0
}

/// A run that produced `text`.
fn summarized(state: &mut ChatState, text: &str) {
    let turn = started(state);
    state.apply(turn, StreamEvent::Delta(text.into()));
    state.apply(turn, StreamEvent::Done(Finish::Stop));
}

// 11.1: the request ---------------------------------------------------------

#[test]
fn the_request_is_the_summary_prompt_and_the_message_texts_only() {
    let mut state = chat();
    let (turn, _) = sent(&mut state, "q1");
    state.apply(turn, StreamEvent::ThinkingDelta("secret reasoning".into()));
    state.apply(turn, StreamEvent::Delta("a1".into()));
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    state.inject_memory(crate::core::ai::memory::context::ContextBlock {
        text: "<project-memory>x</project-memory>".into(),
        notes: 1,
        bytes: 34,
    });
    let (_, request) = state.begin_summary(&ready()).expect("a request");
    assert_eq!(request.engine, "ollama");
    assert_eq!(request.model.as_deref(), Some("llama3:8b"));
    assert_eq!(request.system, SUMMARY_PROMPT.trim_end());
    assert_eq!(request.memory, None, "no memory block");
    let texts: Vec<(Role, &str)> = request
        .messages
        .iter()
        .map(|m| (m.role, m.content.as_str()))
        .collect();
    assert_eq!(
        texts,
        [
            (Role::User, "q1"),
            (Role::Assistant, "a1"),
            (Role::User, INSTRUCTION)
        ],
        "no reasoning, and a final instruction"
    );
}

#[test]
fn an_unanswered_question_is_left_out_of_the_history() {
    let mut state = talked();
    sent(&mut state, "never answered");
    state.cancel();
    let (_, request) = state.begin_summary(&ready()).expect("a request");
    let users = request
        .messages
        .iter()
        .filter(|m| m.role == Role::User)
        .count();
    assert_eq!(users, 3, "two questions and the instruction");
    assert!(
        request
            .messages
            .iter()
            .all(|m| m.content != "never answered")
    );
    // Roles alternate.
    assert!(
        request
            .messages
            .windows(2)
            .all(|pair| pair[0].role != pair[1].role)
    );
}

#[test]
fn the_history_is_capped_like_a_turn() {
    let mut state = chat();
    for n in 0..4 {
        let (turn, _) = sent(&mut state, &format!("{n}{}", "q".repeat(4_000)));
        state.apply(turn, StreamEvent::Delta("a".repeat(4_000)));
        state.apply(turn, StreamEvent::Done(Finish::Stop));
    }
    let (_, request) = state.begin_summary(&ready()).expect("a request");
    let chars: usize = request
        .messages
        .iter()
        .map(|m| m.content.chars().count())
        .sum();
    assert!(chars <= 12_000, "{chars}");
    assert_eq!(request.messages.last().unwrap().content, INSTRUCTION);
    assert_eq!(request.messages[0].role, Role::User);
}

#[test]
fn the_run_takes_the_stream_slot() {
    let mut state = talked();
    let before = state.turn();
    let (turn, _) = state.begin_summary(&ready()).unwrap();
    assert_eq!(turn, before + 1);
    assert_eq!(state.turn(), turn);
    assert!(matches!(state.phase(), Phase::Waiting { .. }));
    assert!(state.summarizing());
    assert!(state.stream_started().is_some());
}

#[test]
fn it_is_refused_while_a_stream_runs_and_a_second_one_too() {
    let mut state = talked();
    sent(&mut state, "third");
    assert!(state.begin_summary(&ready()).is_none());
    let mut other = talked();
    started(&mut other);
    assert!(other.begin_summary(&ready()).is_none());
    // And a send is refused while the summary runs.
    assert!(matches!(
        other.send("hi", &ready(), "sys"),
        super::SendOutcome::Refused
    ));
}

#[test]
fn it_needs_an_answer_and_a_ready_engine() {
    let mut empty = chat();
    assert!(empty.begin_summary(&ready()).is_none());
    sent(&mut empty, "asked, never answered");
    empty.cancel();
    assert!(empty.begin_summary(&ready()).is_none());
    let mut state = talked();
    assert!(
        state
            .begin_summary(&super::engine::Availability::Unassigned)
            .is_none()
    );
    assert!(!state.summarizing());
    assert_eq!(state.phase(), Phase::Idle);
}

// 11.1/11.2: the events -----------------------------------------------------

#[test]
fn thinking_is_dropped_and_deltas_stay_out_of_the_transcript() {
    let mut state = talked();
    let entries = state.entries().to_vec();
    let turn = started(&mut state);
    state.apply(turn, StreamEvent::ThinkingDelta("hmm".into()));
    state.apply(turn, StreamEvent::Delta("We ".into()));
    state.apply(turn, StreamEvent::Delta("chose Axum.".into()));
    assert_eq!(state.entries(), &entries[..], "nothing in the transcript");
    assert!(
        state.summary_draft().is_none(),
        "the draft opens at the end"
    );
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    assert_eq!(state.phase(), Phase::Idle);
    assert!(!state.summarizing());
    assert_eq!(state.entries(), &entries[..]);
    let draft = state.summary_draft().expect("the draft");
    assert_eq!(draft.content(), "We chose Axum.");
}

#[test]
fn the_draft_is_redacted_cut_and_clean() {
    let mut state = talked();
    summarized(&mut state, "we set password=hunter2 \u{1b}[31mred\u{7}");
    let text = state.summary_draft().unwrap().content();
    assert!(text.contains("password=[REDACTED]"), "{text}");
    assert!(!text.contains("hunter2"), "{text}");
    assert!(
        !text.contains('\u{1b}') && !text.contains('\u{7}'),
        "{text:?}"
    );
    assert_eq!(
        state.summary_draft().unwrap().hint().as_deref(),
        Some("1 secret redacted")
    );

    let mut long = talked();
    summarized(&mut long, &"a".repeat(5_000));
    let draft = long.summary_draft().unwrap();
    assert_eq!(draft.content().chars().count(), SUMMARY_CAP);
    assert_eq!(draft.hint().as_deref(), Some("Cut to 4000 characters"));
}

#[test]
fn an_error_or_an_empty_summary_opens_no_draft_and_says_why() {
    let mut state = talked();
    let turn = started(&mut state);
    state.apply(turn, StreamEvent::Delta("half".into()));
    state.apply(turn, StreamEvent::Error(ChatError::Failed("boom".into())));
    assert!(state.summary_draft().is_none());
    assert_eq!(state.phase(), Phase::Idle);
    assert_eq!(
        state.entries().last(),
        Some(&Entry::System("summary failed: boom".into()))
    );

    let mut empty = talked();
    let turn = started(&mut empty);
    empty.apply(turn, StreamEvent::Done(Finish::Stop));
    assert!(empty.summary_draft().is_none());
    assert_eq!(
        empty.entries().last(),
        Some(&Entry::System(NO_SUMMARY.into()))
    );

    let mut cut = talked();
    let turn = started(&mut cut);
    cut.apply(turn, StreamEvent::Delta("half".into()));
    cut.apply(turn, StreamEvent::Error(ChatError::Interrupted));
    assert!(cut.summary_draft().is_none());
}

#[test]
fn a_jail_error_ends_the_summary_run_with_its_line() {
    let mut state = talked();
    let turn = started(&mut state);
    state.apply(turn, StreamEvent::Error(ChatError::Jail("no node".into())));
    assert!(!state.summarizing());
    assert!(state.summary_draft().is_none());
    assert_eq!(
        state.entries().last(),
        Some(&Entry::System(format!(
            "summary failed: {JAIL_ERROR}: no node"
        )))
    );
}

#[test]
fn cancel_ends_the_run_without_a_draft_and_touches_no_answer() {
    let mut state = talked();
    let turn = started(&mut state);
    state.apply(turn, StreamEvent::Delta("part".into()));
    assert_eq!(state.cancel(), Follow::CancelEngine);
    assert_eq!(state.phase(), Phase::Idle);
    assert!(!state.summarizing());
    assert!(state.summary_draft().is_none());
    let last = state.messages().last().unwrap();
    assert_eq!(last.text, "second a");
    assert_eq!(last.status, MsgStatus::Complete, "the answer is untouched");
    // Late events of the cancelled run change nothing.
    state.apply(turn, StreamEvent::Delta("late".into()));
    state.apply(turn, StreamEvent::Done(Finish::Stop));
    assert!(state.summary_draft().is_none());
}

#[test]
fn a_run_that_overflows_its_collector_stops_the_engine() {
    let mut state = talked();
    let turn = started(&mut state);
    let follow = state.apply(turn, StreamEvent::Delta("x".repeat(100_000)));
    assert_eq!(follow, Follow::CancelEngine);
    assert_eq!(state.phase(), Phase::Idle);
    assert_eq!(
        state.summary_draft().unwrap().content().chars().count(),
        SUMMARY_CAP
    );
}

// 11.2/11.3: the draft's keys -----------------------------------------------

#[test]
fn ctrl_s_approves_the_text_and_esc_discards() {
    let mut draft = SummaryDraft::seeded("We chose Axum.");
    assert_eq!(
        draft.on_key(&ctrl('s'), false),
        SummaryOutcome::Approve("We chose Axum.".into())
    );
    let mut other = SummaryDraft::seeded("x");
    assert_eq!(
        other.on_key(&press(KeyCode::Esc), false),
        SummaryOutcome::Discard
    );
}

#[test]
fn held_ctrl_s_and_esc_do_nothing_and_ctrl_c_is_swallowed() {
    let mut draft = SummaryDraft::seeded("x");
    assert_eq!(draft.on_key(&ctrl('s'), true), SummaryOutcome::Handled);
    assert_eq!(
        draft.on_key(&press(KeyCode::Esc), true),
        SummaryOutcome::Handled
    );
    assert_eq!(draft.on_key(&ctrl('c'), false), SummaryOutcome::Handled);
    assert_eq!(draft.content(), "x");
    let held =
        KeyEvent::new_with_kind(KeyCode::Char('z'), KeyModifiers::NONE, KeyEventKind::Repeat);
    draft.on_key(&held, true);
    assert_eq!(draft.content(), "xz", "a held edit key edits");
}

#[test]
fn the_user_edits_the_text_with_newlines_and_a_paste() {
    let mut draft = SummaryDraft::seeded("ab");
    draft.on_key(&press(KeyCode::Enter), false);
    typed(&mut draft, "cd");
    draft.on_key(&press(KeyCode::Backspace), false);
    draft.paste("e\r\nf\u{1b}");
    assert_eq!(draft.content(), "ab\nce\nf");
}

#[test]
fn the_second_pass_changes_the_text_and_asks_to_review() {
    let mut draft = SummaryDraft::seeded("fine");
    typed(&mut draft, " password=hunter2");
    assert_eq!(draft.on_key(&ctrl('s'), false), SummaryOutcome::Handled);
    assert_eq!(draft.content(), "fine password=[REDACTED]");
    assert_eq!(draft.notice(), Some(REDACTED_AGAIN));
    assert_eq!(
        draft.on_key(&ctrl('s'), false),
        SummaryOutcome::Approve("fine password=[REDACTED]".into())
    );
}

#[test]
fn an_empty_summary_is_not_approved() {
    let mut draft = SummaryDraft::seeded("  \n ");
    assert_eq!(draft.on_key(&ctrl('s'), false), SummaryOutcome::Handled);
    assert_eq!(draft.notice(), Some(SUMMARY_REQUIRED));
}

#[test]
fn the_text_is_capped_while_typing() {
    let mut draft = SummaryDraft::seeded(&"a".repeat(SUMMARY_CAP));
    typed(&mut draft, "bbb");
    assert_eq!(draft.content().chars().count(), SUMMARY_CAP);
}

// 11.3: approval keeps, End carries -----------------------------------------

const SESSION: &str = "lazarobox-0123456789abcdef0123456789abcdef";

fn with_session(state: &mut ChatState) {
    state.adopt_session(SESSION, "proj");
    state.record_note("proj", "chat/decision/a", 1, "A");
}

#[test]
fn approving_keeps_the_summary_and_closes_the_draft() {
    let mut state = talked();
    summarized(&mut state, "first");
    state.approve_summary("first");
    assert!(state.summary_draft().is_none());
    assert_eq!(state.approved_summary(), Some("first"));
    // A second approval replaces it.
    summarized(&mut state, "second");
    state.approve_summary("second");
    assert_eq!(state.approved_summary(), Some("second"));
}

#[test]
fn discarding_keeps_nothing_new_and_the_earlier_approval_stays() {
    let mut state = talked();
    summarized(&mut state, "first");
    state.approve_summary("first");
    summarized(&mut state, "second");
    state.discard_summary();
    assert!(state.summary_draft().is_none());
    assert_eq!(state.approved_summary(), Some("first"));
}

#[test]
fn the_end_carries_the_latest_approved_summary_else_the_titles() {
    let mut titles = talked();
    with_session(&mut titles);
    assert_eq!(titles.take_end().unwrap().2, "Notes: A");

    let mut state = talked();
    with_session(&mut state);
    summarized(&mut state, "first");
    state.approve_summary("first");
    summarized(&mut state, "second");
    state.approve_summary("second");
    let end = state.take_end().expect("a session");
    assert_eq!(end.2, "second");
    assert_eq!(state.take_end(), None, "once");
}

#[test]
fn a_discarded_draft_is_not_sent() {
    let mut state = talked();
    with_session(&mut state);
    summarized(&mut state, "draft");
    state.discard_summary();
    assert_eq!(state.take_end().unwrap().2, "Notes: A");
}

#[test]
fn without_a_session_there_is_no_end_even_with_a_summary() {
    let mut state = talked();
    summarized(&mut state, "s");
    state.approve_summary("s");
    assert_eq!(state.take_end(), None);
}

#[test]
fn memory_off_drops_the_kept_summary() {
    let mut state = talked();
    with_session(&mut state);
    summarized(&mut state, "s");
    state.approve_summary("s");
    state.clear_memory_at(80, 10);
    assert_eq!(state.approved_summary(), None);
    assert_eq!(state.take_end().unwrap().2, "Notes: A");
}

#[test]
fn the_kept_summary_is_cut_for_the_end() {
    let mut state = talked();
    with_session(&mut state);
    state.approve_summary(&"é".repeat(5_000));
    assert_eq!(state.take_end().unwrap().2.chars().count(), SUMMARY_CAP);
}

// the readiness the key needs is the note's ------------------------------------

#[test]
fn the_new_notices_are_fixed_text() {
    assert_eq!(NOTHING_TO_SUMMARIZE, "Nothing to summarize");
    assert_eq!(
        SUMMARY_BUSY,
        "Still answering: wait for it or press Esc to cancel"
    );
    assert_eq!(cannot_summarize("x"), "Cannot summarize: x");
    let _ = Readiness::Unknown;
}

#[test]
fn a_yank_during_the_summary_run_copies_the_newest_answer() {
    let mut state = talked();
    let turn = started(&mut state);
    state.apply(turn, StreamEvent::Delta("the summary so far".into()));
    assert_eq!(state.phase(), Phase::Streaming);
    assert_eq!(
        super::yank::last_answer(&state).as_deref(),
        Some("second a"),
        "the summary text is not a message, so nothing is skipped"
    );
}

#[test]
fn a_cut_stream_opens_the_draft_with_an_incomplete_hint() {
    let mut state = talked();
    let turn = started(&mut state);
    state.apply(turn, StreamEvent::Delta("half a summ".into()));
    state.apply(turn, StreamEvent::Error(ChatError::Truncated));
    let draft = state.summary_draft().expect("the draft opens");
    assert_eq!(draft.content(), "half a summ");
    assert_eq!(draft.hint().as_deref(), Some(STREAM_CUT));

    // A run that ends well has no such hint.
    let mut whole = talked();
    let turn = started(&mut whole);
    whole.apply(turn, StreamEvent::Delta("all of it".into()));
    whole.apply(turn, StreamEvent::Done(Finish::Stop));
    assert_eq!(whole.summary_draft().expect("a draft").hint(), None);
}

#[test]
fn the_incomplete_hint_joins_the_other_hints() {
    let mut state = talked();
    let turn = started(&mut state);
    state.apply(turn, StreamEvent::Delta("password=hunter2".into()));
    state.apply(turn, StreamEvent::Error(ChatError::Truncated));
    let hint = state
        .summary_draft()
        .expect("a draft")
        .hint()
        .expect("hint");
    assert_eq!(hint, format!("1 secret redacted · {STREAM_CUT}"));
}

// ai-cost 1b.4: the summary run is a row with its role ------------------------

mod usage_rows {
    use super::*;
    use crate::core::ai::chat::{store::StoreCmd, usage::TurnUsage};
    use crate::core::layout::PaneId;

    fn used(output: u64) -> StreamEvent {
        StreamEvent::usage(TurnUsage {
            output: Some(output),
            ..TurnUsage::default()
        })
    }

    fn summary_rows(state: &mut ChatState) -> Vec<(String, bool, Option<u64>)> {
        state
            .store_cmds(PaneId::FIRST, &[])
            .into_iter()
            .filter_map(|cmd| match cmd {
                StoreCmd::RecordUsage { role, usage, .. } if role == "summary" => {
                    Some((role, usage.partial, usage.output))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_summary_run_that_reported_usage_is_a_summary_row() {
        let mut state = talked();
        let turn = started(&mut state);
        state.apply(turn, used(30));
        state.apply(turn, StreamEvent::Delta("text".into()));
        state.apply(turn, StreamEvent::Done(Finish::Stop));
        assert_eq!(
            summary_rows(&mut state),
            vec![("summary".into(), false, Some(30))]
        );
    }

    #[test]
    fn a_failed_or_cancelled_summary_run_is_partial_and_no_usage_is_no_row() {
        let mut state = talked();
        let turn = started(&mut state);
        state.apply(turn, used(5));
        state.apply(turn, StreamEvent::Error(ChatError::Interrupted));
        assert_eq!(
            summary_rows(&mut state),
            vec![("summary".into(), true, Some(5))]
        );

        let mut state = talked();
        let turn = started(&mut state);
        state.apply(turn, used(6));
        state.cancel();
        assert_eq!(
            summary_rows(&mut state),
            vec![("summary".into(), true, Some(6))]
        );

        let mut state = talked();
        summarized(&mut state, "text");
        assert!(summary_rows(&mut state).is_empty());
    }
}

#[test]
fn a_resume_during_a_run_records_the_reported_usage_as_partial() {
    use crate::core::ai::chat::{store::StoreCmd, tags::Tags, usage::TurnUsage};
    use crate::core::{ai::config::Profile, layout::PaneId};
    let mut state = talked();
    let turn = started(&mut state);
    state.apply(
        turn,
        StreamEvent::usage(TurnUsage {
            output: Some(4),
            ..TurnUsage::default()
        }),
    );
    state.resume_into(7, Profile::Programmer, Tags::default(), Vec::new());
    let partial: Vec<bool> = state
        .store_cmds(PaneId::FIRST, &[])
        .into_iter()
        .filter_map(|c| match c {
            StoreCmd::RecordUsage { role, usage, .. } if role == "summary" => Some(usage.partial),
            _ => None,
        })
        .collect();
    assert_eq!(partial, vec![true]);
}
