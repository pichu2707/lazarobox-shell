//! Summary draft and approval (engram-memory M8): the `Prefix s` key, the
//! request, the draft view and its keys, and the summary carried by the
//! session end.

use super::chat_notes::{
    CWD, PROJECT, ai, ctrl, pause, plain, saved_reply, systems, type_text, with_answer,
};
use super::*;
use crate::core::ai::{
    EngineState, Status,
    chat::{
        Phase,
        engine::{ChatError, ChatRequest, Finish, Role, StreamEvent},
        memory::Readiness,
        note::{MEMORY_CHECKING, MEMORY_OFF, not_ready},
        store::{StoreCmd, StoreReply},
        summary::{
            INSTRUCTION, KEPT, NOTHING_TO_SUMMARIZE, REDACTED_AGAIN, SUMMARY_BUSY, SUMMARY_PROMPT,
        },
    },
    memory::port::{MemCmd, MemReply, Reason},
};

const SESSION: &str = "lazarobox-0123456789abcdef0123456789abcdef";

fn chat_id() -> PaneId {
    id2()
}

fn chat_of(a: &App) -> &ChatState {
    match a.pane_view(chat_id()) {
        Some(PaneView::Chat(chat)) => chat,
        _ => panic!("pane 2 is not a chat"),
    }
}

/// A chat with one answered exchange and its project known.
fn ready_chat() -> App {
    let mut a = with_answer(true, "We use Axum.");
    a.update(AppEvent::Memory(MemReply::Project {
        pane: chat_id(),
        result: Ok(PROJECT.into()),
    }));
    a.take_dirty();
    a
}

/// The summary key; the turn and the request it sent.
fn summarize(a: &mut App) -> (u64, ChatRequest) {
    let effects = press(a, "s");
    match effects.as_slice() {
        [
            Effect::ChatSend {
                pane,
                turn,
                request,
            },
        ] if *pane == chat_id() => (*turn, request.clone()),
        other => panic!("expected one ChatSend, got {other:?}"),
    }
}

fn delta(a: &mut App, turn: u64, text: &str) {
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Delta(text.into()),
    ));
}

fn done(a: &mut App, turn: u64) {
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(Finish::Stop),
    ));
}

/// A chat whose summary draft is open on `text`.
pub(super) fn with_draft(text: &str) -> App {
    let mut a = ready_chat();
    let (turn, _) = summarize(&mut a);
    delta(&mut a, turn, text);
    done(&mut a, turn);
    assert!(chat_of(&a).summary_draft().is_some(), "the draft opened");
    a
}

fn draft_text(a: &App) -> String {
    chat_of(a).summary_draft().expect("a draft").content()
}

// 11.1: the key and the request -----------------------------------------------

#[test]
fn the_key_sends_one_chat_request_with_the_summary_prompt_and_the_texts_only() {
    let mut a = ready_chat();
    let (turn, request) = summarize(&mut a);
    assert_eq!(turn, chat_of(&a).turn());
    assert_eq!(request.system, SUMMARY_PROMPT.trim_end());
    assert_eq!(request.memory, None);
    let texts: Vec<(Role, &str)> = request
        .messages
        .iter()
        .map(|m| (m.role, m.content.as_str()))
        .collect();
    assert_eq!(
        texts,
        [
            (Role::User, "q"),
            (Role::Assistant, "We use Axum."),
            (Role::User, INSTRUCTION)
        ]
    );
    assert!(matches!(chat_of(&a).phase(), Phase::Waiting { .. }));
}

#[test]
fn the_key_never_writes_to_a_pty_or_the_composer() {
    let mut a = ready_chat();
    type_text(&mut a, "draft");
    let effects = press(&mut a, "s");
    assert!(
        effects.iter().all(|e| !matches!(e, Effect::WritePty(..))),
        "{effects:?}"
    );
    assert_eq!(chat_of(&a).composer().text(), "draft");
}

#[test]
fn it_is_refused_while_a_stream_runs() {
    let mut a = ready_chat();
    type_text(&mut a, "more");
    a.update(plain(KeyCode::Enter));
    assert!(matches!(chat_of(&a).phase(), Phase::Waiting { .. }));
    assert_eq!(press(&mut a, "s"), vec![]);
    assert_eq!(a.notice(), Some(SUMMARY_BUSY));
}

#[test]
fn a_second_press_while_the_summary_runs_sends_nothing() {
    let mut a = ready_chat();
    summarize(&mut a);
    assert_eq!(press(&mut a, "s"), vec![]);
    assert_eq!(a.notice(), Some(SUMMARY_BUSY));
}

#[test]
fn a_send_is_refused_while_the_summary_runs() {
    let mut a = ready_chat();
    summarize(&mut a);
    type_text(&mut a, "hi");
    assert_eq!(a.update(plain(KeyCode::Enter)), vec![]);
    assert_eq!(chat_of(&a).composer().text(), "hi", "the text stays");
}

#[test]
fn without_an_answer_there_is_nothing_to_summarize() {
    let mut a = launched(80, 25).with_ai(ai(true));
    a.update(cwd_event(CWD));
    press(&mut a, "c");
    a.update(AppEvent::Memory(MemReply::Project {
        pane: chat_id(),
        result: Ok(PROJECT.into()),
    }));
    assert_eq!(press(&mut a, "s"), vec![]);
    assert_eq!(a.notice(), Some(NOTHING_TO_SUMMARIZE));
}

/// A chat whose first question is out and whose first answer is streaming.
fn first_answer_streaming() -> App {
    let mut a = launched(80, 25).with_ai(ai(true));
    a.update(cwd_event(CWD));
    a.update(AppEvent::EnginesDetected(Detection {
        engines: vec![EngineState {
            id: "ollama",
            status: Status::Available,
            models: vec!["llama3:8b".to_owned()],
            version: None,
            model_labels: Vec::new(),
            diagnostics: Default::default(),
        }],
        ..Default::default()
    }));
    press(&mut a, "c");
    a.update(AppEvent::PromptReady(
        chat_id(),
        crate::core::ai::chat::prompts::Resolved {
            text: "system".into(),
            notices: Vec::new(),
        },
    ));
    a.update(AppEvent::Memory(MemReply::Project {
        pane: chat_id(),
        result: Ok(PROJECT.into()),
    }));
    type_text(&mut a, "q");
    a.update(plain(KeyCode::Enter));
    let turn = chat_of(&a).turn();
    delta(&mut a, turn, "partial");
    assert_eq!(chat_of(&a).phase(), Phase::Streaming);
    a
}

#[test]
fn while_the_first_answer_streams_the_key_says_busy_not_nothing() {
    let mut a = first_answer_streaming();
    assert_eq!(press(&mut a, "s"), vec![]);
    assert_eq!(a.notice(), Some(SUMMARY_BUSY));
}

#[test]
fn memory_off_shows_only_its_notice() {
    let mut a = with_answer(false, "x");
    assert_eq!(press(&mut a, "s"), vec![]);
    assert_eq!(a.notice(), Some(MEMORY_OFF));
}

#[test]
fn memory_not_ready_shows_why_and_sends_no_request() {
    let mut a = with_answer(true, "x");
    a.update(AppEvent::Memory(MemReply::Unavailable {
        pane: Some(chat_id()),
        reason: Reason::NotRunning,
    }));
    let effects = press(&mut a, "s");
    assert!(
        effects
            .iter()
            .all(|e| !matches!(e, Effect::ChatSend { .. })),
        "{effects:?}"
    );
    assert_eq!(
        a.notice(),
        Some(not_ready(&Reason::NotRunning.text()).as_str())
    );

    let mut checking = with_answer(true, "x");
    let effects = press(&mut checking, "s");
    assert!(
        effects
            .iter()
            .all(|e| !matches!(e, Effect::ChatSend { .. }))
    );
    assert!(matches!(
        chat_of(&checking).readiness(),
        Readiness::Resolving { .. } | Readiness::Unknown
    ));
    assert_eq!(checking.notice(), Some(MEMORY_CHECKING));
}

#[test]
fn an_engine_that_is_not_ready_says_cannot_summarize() {
    let mut a = ready_chat();
    a.update(AppEvent::EnginesDetected(Detection {
        engines: vec![],
        ..Default::default()
    }));
    assert_eq!(press(&mut a, "s"), vec![]);
    let notice = a.notice().expect("a notice");
    assert!(notice.starts_with("Cannot summarize: "), "{notice}");
}

#[test]
fn the_key_on_a_shell_pane_only_says_it_is_for_chats() {
    let mut a = launched(80, 25).with_ai(ai(true));
    assert_eq!(press(&mut a, "s"), vec![]);
    assert_eq!(
        a.notice(),
        Some("The summary is for chat panes; open one with Ctrl+Space c")
    );
}

#[test]
fn another_memory_view_blocks_the_key_and_the_other_way_round() {
    let mut a = ready_chat();
    press(&mut a, "n");
    assert!(chat_of(&a).note_editor().is_some());
    assert_eq!(press(&mut a, "s"), vec![]);
    assert!(chat_of(&a).summary_draft().is_none());
    assert!(!chat_of(&a).summarizing());

    let mut b = with_draft("text");
    press(&mut b, "n");
    assert!(chat_of(&b).note_editor().is_none(), "one view at a time");
    assert_eq!(press(&mut b, "s"), vec![]);
}

// 11.1: cancel -------------------------------------------------------------------

#[test]
fn esc_cancels_the_run_and_leaves_the_pane_idle_without_a_draft() {
    let mut a = ready_chat();
    let (turn, _) = summarize(&mut a);
    delta(&mut a, turn, "part");
    pause(&mut a);
    let effects = a.update(plain(KeyCode::Esc));
    assert_eq!(effects, vec![Effect::ChatCancel(chat_id())]);
    assert_eq!(chat_of(&a).phase(), Phase::Idle);
    assert!(chat_of(&a).summary_draft().is_none());
    delta(&mut a, turn, "late");
    done(&mut a, turn);
    assert!(chat_of(&a).summary_draft().is_none(), "late events dropped");
}

// 11.2: the draft ---------------------------------------------------------------

#[test]
fn deltas_fill_the_draft_not_the_transcript_and_thinking_is_dropped() {
    let mut a = ready_chat();
    let before = chat_of(&a).entries().to_vec();
    let (turn, _) = summarize(&mut a);
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::ThinkingDelta("reasoning".into()),
    ));
    delta(&mut a, turn, "We ");
    delta(&mut a, turn, "chose Axum.");
    done(&mut a, turn);
    assert_eq!(chat_of(&a).entries(), &before[..]);
    assert_eq!(draft_text(&a), "We chose Axum.");
    assert_eq!(chat_of(&a).phase(), Phase::Idle);
}

#[test]
fn the_draft_is_redacted() {
    let a = with_draft("we set password=hunter2");
    assert_eq!(draft_text(&a), "we set password=[REDACTED]");
}

#[test]
fn a_failed_run_says_so_in_the_transcript() {
    let mut a = ready_chat();
    let (turn, _) = summarize(&mut a);
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Error(ChatError::Failed("boom".into())),
    ));
    assert!(chat_of(&a).summary_draft().is_none());
    assert!(systems(&a).contains(&"summary failed: boom".to_owned()));
}

#[test]
fn the_draft_takes_the_keys_and_the_composer_keeps_its_text() {
    let mut a = ready_chat();
    type_text(&mut a, "keep");
    let (turn, _) = summarize(&mut a);
    delta(&mut a, turn, "ab");
    done(&mut a, turn);
    type_text(&mut a, "cd");
    a.update(plain(KeyCode::Enter));
    assert_eq!(draft_text(&a), "abcd\n");
    assert_eq!(chat_of(&a).composer().text(), "keep");
    a.update(AppEvent::Paste("e\r\nf".into()));
    assert_eq!(draft_text(&a), "abcd\ne\nf");
    assert_eq!(chat_of(&a).composer().text(), "keep");
}

#[test]
fn keys_in_the_draft_reach_no_pty_and_ctrl_c_never_quits() {
    let mut a = with_draft("x");
    let effects = a.update(ctrl('c'));
    assert_eq!(effects, vec![]);
    assert!(chat_of(&a).summary_draft().is_some());
    assert_eq!(a.update(plain(KeyCode::PageUp)), vec![]);
}

#[test]
fn the_prefix_works_inside_the_draft() {
    let mut a = with_draft("x");
    a.update(ctrl_space());
    assert_eq!(a.input(), InputMode::Prefix);
}

// 11.3: approval ---------------------------------------------------------------

#[test]
fn ctrl_s_keeps_the_summary_and_sends_nothing() {
    let mut a = with_draft("We chose Axum.");
    let effects = a.update(ctrl('s'));
    assert_eq!(effects, vec![], "no engram request at approval");
    assert!(chat_of(&a).summary_draft().is_none());
    assert_eq!(chat_of(&a).approved_summary(), Some("We chose Axum."));
    assert_eq!(a.notice(), Some(KEPT));
}

#[test]
fn approving_again_replaces_the_summary() {
    let mut a = with_draft("first");
    a.update(ctrl('s'));
    let (turn, _) = summarize(&mut a);
    delta(&mut a, turn, "second");
    done(&mut a, turn);
    pause(&mut a);
    assert_eq!(a.update(ctrl('s')), vec![]);
    assert_eq!(chat_of(&a).approved_summary(), Some("second"));
}

#[test]
fn esc_discards_and_keeps_nothing() {
    let mut a = with_draft("text");
    assert_eq!(a.update(plain(KeyCode::Esc)), vec![]);
    assert!(chat_of(&a).summary_draft().is_none());
    assert_eq!(chat_of(&a).approved_summary(), None);
}

#[test]
fn the_second_redaction_pass_keeps_the_draft_open() {
    let mut a = with_draft("fine");
    type_text(&mut a, " password=hunter2");
    assert_eq!(a.update(ctrl('s')), vec![]);
    assert_eq!(chat_of(&a).approved_summary(), None);
    assert_eq!(draft_text(&a), "fine password=[REDACTED]");
    assert_eq!(
        chat_of(&a).summary_draft().unwrap().notice(),
        Some(REDACTED_AGAIN)
    );
    pause(&mut a);
    a.update(ctrl('s'));
    assert_eq!(
        chat_of(&a).approved_summary(),
        Some("fine password=[REDACTED]")
    );
}

#[test]
fn memory_turned_off_with_the_draft_open_refuses_the_approval() {
    let a = with_draft("text");
    let mut a = a.with_ai(ai(false));
    assert_eq!(a.update(ctrl('s')), vec![]);
    assert_eq!(a.notice(), Some(MEMORY_OFF));
    assert!(chat_of(&a).summary_draft().is_some(), "the text stays");
    assert_eq!(chat_of(&a).approved_summary(), None);
}

// 11.3: the session end ------------------------------------------------------

/// The end of a chat that wrote notes and approved no summary.
fn end(summary: &str) -> Effect {
    Effect::Memory(MemCmd::End {
        session: SESSION.into(),
        project: PROJECT.into(),
        summary: summary.into(),
        cwd: None,
    })
}

/// The end that carries an approved summary: the worker may register the
/// session first.
fn end_summary(summary: &str) -> Effect {
    Effect::Memory(MemCmd::End {
        session: SESSION.into(),
        project: PROJECT.into(),
        summary: summary.into(),
        cwd: Some(CWD.into()),
    })
}

fn fixed_session() -> Option<String> {
    Some(SESSION.to_owned())
}

/// A chat that saved the note `A` into the session.
fn wrote_one() -> App {
    let mut a = ready_chat();
    press(&mut a, "n");
    type_text(&mut a, "A");
    a.update(ctrl('s'));
    a.update(saved_reply(&a, SESSION, 1, "chat/decision/a", "A"));
    a
}

fn approve_summary(a: &mut App, text: &str) {
    let (turn, _) = summarize(a);
    delta(a, turn, text);
    done(a, turn);
    pause(a);
    a.update(ctrl('s'));
    assert_eq!(chat_of(a).approved_summary(), Some(text));
}

#[test]
fn the_end_at_close_carries_the_latest_approved_summary_once() {
    let mut a = wrote_one();
    approve_summary(&mut a, "first");
    approve_summary(&mut a, "second");
    press(&mut a, "wq");
    let effects = a.update(key('y'));
    let ends: Vec<&Effect> = effects
        .iter()
        .filter(|e| matches!(e, Effect::Memory(MemCmd::End { .. })))
        .collect();
    assert_eq!(ends, [&end_summary("second")], "{effects:?}");
}

#[test]
fn quit_carries_the_summary_too() {
    let mut a = wrote_one();
    approve_summary(&mut a, "We chose Axum.");
    press(&mut a, "q");
    assert_eq!(
        a.update(key('y')),
        vec![end_summary("We chose Axum."), Effect::Quit]
    );
}

#[test]
fn a_discarded_summary_leaves_the_titles_at_close() {
    let mut a = wrote_one();
    let (turn, _) = summarize(&mut a);
    delta(&mut a, turn, "draft");
    done(&mut a, turn);
    a.update(plain(KeyCode::Esc));
    press(&mut a, "q");
    assert_eq!(a.update(key('y')), vec![end("Notes: A"), Effect::Quit]);
}

// An approved summary in a chat with no note still reaches engram: the session
// is registered at close, as part of the End.

#[test]
fn a_summary_alone_registers_a_session_and_ends_it_at_close() {
    let mut a = ready_chat().with_session_ids(fixed_session);
    approve_summary(&mut a, "alone");
    // Approval sends nothing and takes no session yet.
    assert_eq!(chat_of(&a).session(), None);
    press(&mut a, "q");
    assert_eq!(a.update(key('y')), vec![end_summary("alone"), Effect::Quit]);
}

#[test]
fn a_summary_alone_links_the_new_session_once_the_conversation_exists() {
    let mut a = ready_chat().with_session_ids(fixed_session);
    a.update(AppEvent::Store(StoreReply::Created {
        pane: chat_id(),
        conversation: 7,
        evicted: false,
        evicted_notes: 0,
    }));
    approve_summary(&mut a, "alone");
    press(&mut a, "q");
    assert_eq!(
        a.update(key('y')),
        vec![
            Effect::Store(StoreCmd::LinkSession {
                pane: chat_id(),
                conversation: 7,
                session_id: SESSION.into(),
                project: PROJECT.into(),
            }),
            end_summary("alone"),
            Effect::Quit
        ]
    );
}

#[test]
fn a_summary_alone_in_a_closed_pane_ends_once() {
    let mut a = ready_chat().with_session_ids(fixed_session);
    approve_summary(&mut a, "alone");
    press(&mut a, "wq");
    let effects = a.update(key('y'));
    assert_eq!(ends_in(&effects), 1, "{effects:?}");
    assert!(effects.contains(&end_summary("alone")), "{effects:?}");
}

#[test]
fn a_summary_alone_keeps_the_pair_the_store_holds() {
    let mut a =
        ready_chat().with_session_ids(|| Some("lazarobox-ffffffffffffffffffffffffffffffff".into()));
    a.update(AppEvent::Store(StoreReply::SessionLinked {
        pane: chat_id(),
        conversation: 7,
        session_id: SESSION.into(),
        project: PROJECT.into(),
    }));
    approve_summary(&mut a, "alone");
    press(&mut a, "q");
    assert_eq!(a.update(key('y')), vec![end_summary("alone"), Effect::Quit]);
}

#[test]
fn a_summary_alone_with_memory_off_before_close_sends_nothing() {
    let a = ready_chat().with_session_ids(fixed_session);
    let mut a = a.with_ai(ai(true));
    approve_summary(&mut a, "alone");
    let mut a = a.with_ai(ai(false));
    press(&mut a, "q");
    assert_eq!(a.update(key('y')), vec![Effect::Quit]);

    let mut b = ready_chat().with_session_ids(fixed_session);
    approve_summary(&mut b, "alone");
    b.memory_turned_off();
    press(&mut b, "q");
    assert_eq!(b.update(key('y')), vec![Effect::Quit]);
}

#[test]
fn a_summary_alone_without_a_session_id_sends_nothing() {
    let mut a = ready_chat();
    approve_summary(&mut a, "alone");
    press(&mut a, "q");
    assert_eq!(a.update(key('y')), vec![Effect::Quit]);
}

#[test]
fn a_chat_with_notes_and_no_summary_never_asks_for_a_session_id() {
    let mut a = wrote_one().with_session_ids(|| panic!("no new id is needed"));
    press(&mut a, "q");
    assert_eq!(a.update(key('y')), vec![end("Notes: A"), Effect::Quit]);
}

#[test]
fn turning_memory_off_drops_the_kept_summary() {
    let mut a = wrote_one();
    approve_summary(&mut a, "private");
    a.memory_turned_off();
    assert_eq!(chat_of(&a).approved_summary(), None);
    press(&mut a, "q");
    let effects = a.update(key('y'));
    assert!(!format!("{effects:?}").contains("private"), "{effects:?}");
}

#[test]
fn memory_found_off_at_close_sends_the_titles_not_the_summary() {
    let mut a = wrote_one();
    approve_summary(&mut a, "private");
    // Off without the usual turn-off path: the end still must not carry it.
    let mut a = a.with_ai(ai(false));
    press(&mut a, "q");
    let effects = a.update(key('y'));
    assert!(!format!("{effects:?}").contains("private"), "{effects:?}");
    assert_eq!(ends_in(&effects), 1, "{effects:?}");
}

/// How many `End` commands `effects` holds.
fn ends_in(effects: &[Effect]) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::Memory(MemCmd::End { .. })))
        .count()
}

#[test]
fn the_reasoning_of_the_conversation_never_reaches_the_summary_request() {
    let mut a = ready_chat();
    type_text(&mut a, "again");
    let turn = match a.update(plain(KeyCode::Enter)).as_slice() {
        [Effect::ChatSend { turn, .. }] => *turn,
        other => panic!("expected one ChatSend, got {other:?}"),
    };
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::ThinkingDelta("secret plan".into()),
    ));
    delta(&mut a, turn, "second answer");
    done(&mut a, turn);
    let (_, request) = summarize(&mut a);
    assert!(
        !format!("{request:?}").contains("secret plan"),
        "{request:?}"
    );
    assert_eq!(
        request.messages.len(),
        5,
        "four messages and the instruction"
    );
}

// parked sends and the memory views -------------------------------------------

fn detected() -> AppEvent {
    AppEvent::EnginesDetected(Detection {
        engines: vec![EngineState {
            id: "ollama",
            status: Status::Available,
            models: vec!["llama3:8b".to_owned()],
            version: None,
            model_labels: Vec::new(),
            diagnostics: Default::default(),
        }],
        ..Default::default()
    })
}

/// An answered chat whose next send is parked (a detection is running again)
/// with the note editor open over it.
fn parked_under_the_editor() -> App {
    let mut a = ready_chat();
    // The detection runs again: sends wait for its result.
    a.engines = None;
    type_text(&mut a, "hi");
    assert_eq!(a.update(plain(KeyCode::Enter)), vec![], "parked");
    press(&mut a, "n");
    assert!(chat_of(&a).note_editor().is_some());
    a
}

fn sends(effects: &[Effect]) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::ChatSend { .. }))
        .count()
}

#[test]
fn a_parked_send_waits_while_the_note_editor_is_open_and_goes_when_it_closes() {
    let mut a = parked_under_the_editor();
    let effects = a.update(detected());
    assert_eq!(sends(&effects), 0, "{effects:?}");
    assert_eq!(chat_of(&a).phase(), Phase::Idle);
    let effects = a.update(plain(KeyCode::Esc));
    assert_eq!(sends(&effects), 1, "{effects:?}");
}

#[test]
fn a_parked_send_goes_when_the_note_is_saved() {
    let mut a = parked_under_the_editor();
    type_text(&mut a, "A");
    a.update(ctrl('s'));
    let effects = a.update(detected());
    assert_eq!(sends(&effects), 0, "{effects:?}");
    let effects = a.update(saved_reply(&a, SESSION, 1, "chat/decision/a", "A"));
    assert_eq!(sends(&effects), 1, "{effects:?}");
}
