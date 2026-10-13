//! Save as note (engram-memory M7b): Ctrl+N, the note editor and its keys,
//! the save flow against the memory worker's replies.

use super::*;
use crate::core::ai::{
    EngineState, Status,
    chat::{
        Entry,
        engine::{Finish, StreamEvent},
        memory::Readiness,
        note::{Field, MEMORY_OFF, NOTHING_TO_SAVE},
    },
    config::{Assignment, MemoryConfig, Task},
    memory::{
        kind::MemKind,
        port::{MemCmd, MemReply, Note, Reason},
        reply::ProjectError,
    },
};

const PROJECT: &str = "lazarobox-shell";
const CWD: &str = "/srv/app";

fn chat_id() -> PaneId {
    id2()
}

fn ai(memory: bool) -> AiConfig {
    let mut ai = AiConfig::default();
    ai.set(
        Task::Chat,
        Assignment {
            engine: Some("ollama".into()),
            model: Some("llama3:8b".into()),
        },
    );
    ai.memory = MemoryConfig {
        enabled: memory,
        acknowledged: memory,
        ..MemoryConfig::default()
    };
    ai
}

fn code(code: KeyCode, mods: KeyModifiers) -> AppEvent {
    AppEvent::Key(KeyEvent::new(code, mods))
}

fn ctrl(c: char) -> AppEvent {
    code(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn plain(code_: KeyCode) -> AppEvent {
    code(code_, KeyModifiers::NONE)
}

fn held(code: KeyCode, modifiers: KeyModifiers) -> AppEvent {
    AppEvent::Key(KeyEvent::new_with_kind(
        code,
        modifiers,
        KeyEventKind::Repeat,
    ))
}

fn chat_of(a: &App) -> &ChatState {
    match a.pane_view(chat_id()) {
        Some(PaneView::Chat(chat)) => chat,
        _ => panic!("pane 2 is not a chat"),
    }
}

fn type_text(a: &mut App, text: &str) {
    for c in text.chars() {
        a.update(key(c));
    }
}

/// A chat opened from `/srv/app` whose first exchange answered `answer`.
fn with_answer(memory: bool, answer: &str) -> App {
    let mut a = launched(80, 25).with_ai(ai(memory));
    a.update(cwd_event(CWD));
    a.update(AppEvent::EnginesDetected(Detection {
        engines: vec![EngineState {
            id: "ollama",
            status: Status::Available,
            models: vec!["llama3:8b".to_owned()],
            version: None,
        }],
    }));
    press(&mut a, "c");
    a.update(AppEvent::PromptReady(
        chat_id(),
        crate::core::ai::chat::prompts::Resolved {
            text: "system".into(),
            notices: Vec::new(),
        },
    ));
    type_text(&mut a, "q");
    let turn = match a.update(plain(KeyCode::Enter)).as_slice() {
        [Effect::ChatSend { turn, .. }] => *turn,
        other => panic!("expected one ChatSend, got {other:?}"),
    };
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Delta(answer.into()),
    ));
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(Finish::Stop),
    ));
    a.take_dirty();
    a
}

/// The same chat once its project is known.
fn ready(answer: &str) -> App {
    let mut a = with_answer(true, answer);
    a.update(ctrl('n'));
    a.update(AppEvent::Memory(MemReply::Project {
        pane: chat_id(),
        result: Ok(PROJECT.into()),
    }));
    assert!(chat_of(&a).note_editor().is_some(), "the editor opened");
    a
}

fn resolve() -> Effect {
    Effect::Memory(MemCmd::Resolve {
        pane: chat_id(),
        cwd: CWD.into(),
    })
}

fn project_reply(result: Result<String, ProjectError>) -> AppEvent {
    AppEvent::Memory(MemReply::Project {
        pane: chat_id(),
        result,
    })
}

fn saved_reply(session: &str, id: i64, topic_key: &str, title: &str) -> AppEvent {
    AppEvent::Memory(MemReply::Saved {
        pane: chat_id(),
        session: session.into(),
        observation_id: id,
        topic_key: topic_key.into(),
        title: title.into(),
    })
}

const SESSION: &str = "lazarobox-0123456789abcdef0123456789abcdef";

/// A key that does nothing anywhere, between two presses of one key: a key
/// pressed again within half a second counts as held, and is ignored.
fn pause(a: &mut App) {
    a.update(plain(KeyCode::PageUp));
}

fn systems(a: &App) -> Vec<String> {
    chat_of(a)
        .entries()
        .iter()
        .filter_map(|e| match e {
            Entry::System(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

// 9.3: Ctrl+N ----------------------------------------------------------------

#[test]
fn memory_off_shows_the_notice_and_asks_for_nothing() {
    let mut a = with_answer(false, "an answer");
    assert_eq!(a.update(ctrl('n')), vec![]);
    assert_eq!(a.notice(), Some(MEMORY_OFF));
    assert!(chat_of(&a).note_editor().is_none());
}

#[test]
fn without_an_answer_there_is_nothing_to_save() {
    let mut a = launched(80, 25).with_ai(ai(true));
    press(&mut a, "c");
    assert_eq!(a.update(ctrl('n')), vec![]);
    assert_eq!(a.notice(), Some(NOTHING_TO_SAVE));
    assert!(chat_of(&a).note_editor().is_none());
}

#[test]
fn the_first_ctrl_n_asks_for_the_project_and_opens_the_editor_on_the_reply() {
    let mut a = with_answer(true, "use Axum");
    assert_eq!(a.update(ctrl('n')), vec![resolve()]);
    assert!(chat_of(&a).note_editor().is_none(), "waits for the project");
    assert_eq!(a.update(project_reply(Ok(PROJECT.into()))), vec![]);
    let editor = chat_of(&a).note_editor().expect("open");
    assert_eq!(editor.content(), "use Axum");
    assert_eq!(editor.kind(), MemKind::Decision);
    assert_eq!(editor.title(), "");
}

#[test]
fn a_known_project_opens_the_editor_at_once() {
    let mut a = ready("use Axum");
    a.update(plain(KeyCode::Esc));
    assert!(chat_of(&a).note_editor().is_none());
    pause(&mut a);
    assert_eq!(a.update(ctrl('n')), vec![], "no second resolve");
    assert!(chat_of(&a).note_editor().is_some());
}

#[test]
fn ctrl_n_does_not_touch_the_composer_or_reach_a_pty() {
    let mut a = with_answer(true, "x");
    type_text(&mut a, "draft");
    let effects = a.update(ctrl('n'));
    assert!(
        effects.iter().all(|e| !matches!(e, Effect::WritePty(..))),
        "{effects:?}"
    );
    assert_eq!(chat_of(&a).composer().text(), "draft");
}

#[test]
fn an_unavailable_memory_says_why_and_probes_again_on_the_next_try() {
    let mut a = with_answer(true, "x");
    a.update(ctrl('n'));
    a.update(AppEvent::Memory(MemReply::Unavailable {
        pane: Some(chat_id()),
        reason: Reason::NotRunning,
    }));
    assert_eq!(
        a.notice(),
        Some("Memory is not ready: engram is not running")
    );
    assert!(chat_of(&a).note_editor().is_none());
    pause(&mut a);
    assert_eq!(a.update(ctrl('n')), vec![resolve()], "a re-probe");
    assert_eq!(
        a.notice(),
        Some("Memory is not ready: engram is not running")
    );
}

#[test]
fn a_directory_that_is_not_git_is_not_ready() {
    let mut a = with_answer(true, "x");
    a.update(ctrl('n'));
    a.update(project_reply(Err(ProjectError::NotGit)));
    assert_eq!(
        a.notice(),
        Some("Memory is not ready: memory needs a git project")
    );
    assert!(chat_of(&a).note_editor().is_none());
}

#[test]
fn a_second_ctrl_n_while_waiting_asks_nothing_more() {
    let mut a = with_answer(true, "x");
    a.update(ctrl('n'));
    pause(&mut a);
    assert_eq!(a.update(ctrl('n')), vec![]);
    assert_eq!(a.notice(), Some("Checking memory…"));
}

#[test]
fn a_reply_for_a_pane_that_is_gone_changes_nothing() {
    let mut a = with_answer(true, "x");
    let gone = AppEvent::Memory(MemReply::Project {
        pane: PaneId::for_test(9),
        result: Ok(PROJECT.into()),
    });
    assert_eq!(a.update(gone), vec![]);
    assert_eq!(chat_of(&a).project(), None);
}

#[test]
fn the_reply_of_a_check_nobody_wanted_does_not_open_the_editor() {
    let mut a = with_answer(true, "x");
    a.update(project_reply(Ok(PROJECT.into())));
    assert!(chat_of(&a).note_editor().is_none());
    assert_eq!(chat_of(&a).project(), Some(PROJECT));
}

#[test]
fn repeat_of_ctrl_n_is_ignored() {
    let mut a = with_answer(true, "x");
    assert_eq!(
        a.update(held(KeyCode::Char('n'), KeyModifiers::CONTROL)),
        vec![]
    );
    assert_eq!(a.notice(), None);
    assert_eq!(chat_of(&a).readiness(), &Readiness::Unknown);
}

// 9.4: the editor keys ---------------------------------------------------------

#[test]
fn keys_in_the_editor_edit_the_editor_and_never_the_composer() {
    let mut a = ready("body");
    type_text(&mut a, "Use Axum");
    assert_eq!(chat_of(&a).note_editor().unwrap().title(), "Use Axum");
    assert_eq!(chat_of(&a).composer().text(), "");
    a.update(plain(KeyCode::Tab));
    assert_eq!(chat_of(&a).note_editor().unwrap().field(), Field::Content);
    a.update(plain(KeyCode::Enter));
    assert_eq!(chat_of(&a).note_editor().unwrap().content(), "body\n");
}

#[test]
fn the_editor_swallows_everything_else_and_ctrl_c_never_quits() {
    let mut a = ready("body");
    for event in [
        ctrl('c'),
        ctrl('y'),
        ctrl('r'),
        ctrl('b'),
        plain(KeyCode::PageUp),
        plain(KeyCode::F(2)),
    ] {
        assert_eq!(a.update(event.clone()), vec![], "{event:?}");
    }
    assert!(chat_of(&a).note_editor().is_some());
    assert_eq!(a.notice(), None);
}

#[test]
fn esc_discards_with_no_request_and_keeps_the_composer() {
    let mut a = with_answer(true, "body");
    type_text(&mut a, "draft");
    a.update(ctrl('n'));
    a.update(project_reply(Ok(PROJECT.into())));
    type_text(&mut a, "typed in the editor");
    assert_eq!(a.update(plain(KeyCode::Esc)), vec![]);
    assert!(chat_of(&a).note_editor().is_none());
    assert_eq!(chat_of(&a).composer().text(), "draft");
}

#[test]
fn repeat_of_ctrl_s_and_esc_is_ignored() {
    let mut a = ready("body");
    type_text(&mut a, "T");
    assert_eq!(
        a.update(held(KeyCode::Char('s'), KeyModifiers::CONTROL)),
        vec![]
    );
    assert!(!chat_of(&a).note_editor().unwrap().is_saving());
    assert_eq!(a.update(held(KeyCode::Esc, KeyModifiers::NONE)), vec![]);
    assert!(chat_of(&a).note_editor().is_some());
}

#[test]
fn a_paste_goes_to_the_editor() {
    let mut a = ready("body");
    a.update(AppEvent::Paste("A\ntitle".into()));
    assert_eq!(chat_of(&a).note_editor().unwrap().title(), "A title");
    assert_eq!(chat_of(&a).composer().text(), "");
}

#[test]
fn the_stream_keeps_updating_under_the_editor() {
    let mut a = with_answer(true, "first");
    type_text(&mut a, "again");
    let turn = match a.update(plain(KeyCode::Enter)).as_slice() {
        [Effect::ChatSend { turn, .. }] => *turn,
        other => panic!("{other:?}"),
    };
    a.update(ctrl('n'));
    a.update(project_reply(Ok(PROJECT.into())));
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Delta("second".into()),
    ));
    assert_eq!(chat_of(&a).messages().last().unwrap().text, "second");
    assert_eq!(chat_of(&a).note_editor().unwrap().content(), "first");
}

// 9.5: the save flow ----------------------------------------------------------

fn save_cmd(session: Option<&str>, title: &str, content: &str) -> Effect {
    Effect::Memory(MemCmd::Save {
        pane: chat_id(),
        session: session.map(str::to_owned),
        project: PROJECT.into(),
        cwd: CWD.into(),
        note: Note {
            kind: MemKind::Decision,
            title: title.into(),
            content: content.into(),
        },
    })
}

#[test]
fn ctrl_s_sends_one_save_without_a_session_the_first_time() {
    let mut a = ready("use Axum");
    type_text(&mut a, "Use Axum");
    assert_eq!(
        a.update(ctrl('s')),
        vec![save_cmd(None, "Use Axum", "use Axum")]
    );
    assert!(chat_of(&a).note_editor().unwrap().is_saving());
    pause(&mut a);
    assert_eq!(a.update(ctrl('s')), vec![], "a second save is not sent");
}

#[test]
fn enter_in_the_title_saves_too() {
    let mut a = ready("body");
    type_text(&mut a, "T");
    assert_eq!(
        a.update(plain(KeyCode::Enter)),
        vec![save_cmd(None, "T", "body")]
    );
}

#[test]
fn an_empty_title_or_content_is_refused_naming_the_field() {
    let mut a = ready("body");
    assert_eq!(a.update(ctrl('s')), vec![]);
    assert_eq!(
        chat_of(&a).note_editor().unwrap().notice(),
        Some("Title is required")
    );
    let mut a = ready("x");
    type_text(&mut a, "T");
    a.update(plain(KeyCode::Tab));
    a.update(plain(KeyCode::Backspace));
    assert_eq!(a.update(ctrl('s')), vec![]);
    assert_eq!(
        chat_of(&a).note_editor().unwrap().notice(),
        Some("Content is required")
    );
}

#[test]
fn text_the_second_pass_changes_is_not_sent() {
    let mut a = ready("fine");
    type_text(&mut a, "T");
    a.update(plain(KeyCode::Tab));
    type_text(&mut a, " password=hunter2");
    assert_eq!(a.update(ctrl('s')), vec![]);
    let editor = chat_of(&a).note_editor().unwrap();
    assert_eq!(editor.notice(), Some("Redacted again; review and save"));
    assert!(!editor.content().contains("hunter2"));
    pause(&mut a);
    let sent = a.update(ctrl('s'));
    assert!(matches!(
        sent.as_slice(),
        [Effect::Memory(MemCmd::Save { .. })]
    ));
}

#[test]
fn a_saved_note_closes_the_editor_and_adds_the_system_line() {
    let mut a = ready("use Axum");
    type_text(&mut a, "Use Axum");
    a.update(ctrl('s'));
    let before = chat_of(&a).messages().count();
    let effects = a.update(saved_reply(
        SESSION,
        41,
        "chat/decision/use-axum",
        "Use Axum",
    ));
    assert_eq!(
        effects,
        vec![],
        "no model call, no link without a conversation"
    );
    assert!(chat_of(&a).note_editor().is_none());
    assert_eq!(systems(&a), ["saved note decision: Use Axum"]);
    assert_eq!(chat_of(&a).messages().count(), before, "no message added");
    assert_eq!(chat_of(&a).session().unwrap().id, SESSION);
    assert_eq!(chat_of(&a).session().unwrap().project, PROJECT);
}

#[test]
fn the_second_note_goes_into_the_same_session() {
    let mut a = ready("use Axum");
    type_text(&mut a, "One");
    a.update(ctrl('s'));
    a.update(saved_reply(SESSION, 1, "chat/decision/one", "One"));
    a.update(ctrl('n'));
    type_text(&mut a, "Two");
    assert_eq!(
        a.update(ctrl('s')),
        vec![save_cmd(Some(SESSION), "Two", "use Axum")]
    );
}

#[test]
fn the_same_note_again_upserts_with_the_same_line() {
    let mut a = ready("use Axum");
    type_text(&mut a, "Use Axum");
    a.update(ctrl('s'));
    a.update(saved_reply(
        SESSION,
        1,
        "chat/decision/use-axum",
        "Use Axum",
    ));
    a.update(ctrl('n'));
    type_text(&mut a, "Use Axum");
    a.update(ctrl('s'));
    a.update(saved_reply(
        SESSION,
        1,
        "chat/decision/use-axum",
        "Use Axum",
    ));
    assert_eq!(
        systems(&a),
        [
            "saved note decision: Use Axum",
            "saved note decision: Use Axum"
        ]
    );
}

#[test]
fn a_failed_save_keeps_the_editor_with_its_text() {
    let mut a = ready("use Axum");
    type_text(&mut a, "Use Axum");
    a.update(ctrl('s'));
    let failed = AppEvent::Memory(MemReply::SaveFailed {
        pane: chat_id(),
        reason: "engram refused the note".into(),
    });
    assert_eq!(a.update(failed), vec![]);
    let editor = chat_of(&a).note_editor().expect("still open");
    assert_eq!(editor.title(), "Use Axum");
    assert_eq!(editor.notice(), Some("engram refused the note"));
    assert!(!editor.is_saving());
    assert!(systems(&a).is_empty());
}

#[test]
fn an_unavailable_memory_during_a_save_is_a_failed_save() {
    let mut a = ready("use Axum");
    type_text(&mut a, "T");
    a.update(ctrl('s'));
    a.update(AppEvent::Memory(MemReply::Unavailable {
        pane: Some(chat_id()),
        reason: Reason::NoAnswer,
    }));
    let editor = chat_of(&a).note_editor().expect("still open");
    assert_eq!(editor.notice(), Some("engram did not answer"));
    assert!(!editor.is_saving());
    assert_eq!(
        chat_of(&a).project(),
        Some(PROJECT),
        "the project stays known"
    );
}

#[test]
fn a_note_saved_after_the_editor_was_discarded_is_still_recorded() {
    let mut a = ready("use Axum");
    type_text(&mut a, "T");
    a.update(ctrl('s'));
    a.update(plain(KeyCode::Esc));
    a.update(ctrl('n'));
    type_text(&mut a, "Other");
    a.update(saved_reply(SESSION, 1, "chat/decision/t", "T"));
    assert_eq!(systems(&a), ["saved note decision: T"]);
    let editor = chat_of(&a).note_editor().expect("the new editor stays");
    assert_eq!(editor.title(), "Other");
    assert!(chat_of(&a).session().is_some());
}

#[test]
fn a_saved_reply_for_a_pane_that_is_gone_is_dropped() {
    let mut a = ready("x");
    let stale = AppEvent::Memory(MemReply::Saved {
        pane: PaneId::for_test(9),
        session: SESSION.into(),
        observation_id: 1,
        topic_key: "chat/decision/x".into(),
        title: "x".into(),
    });
    assert_eq!(a.update(stale), vec![]);
}

#[test]
fn a_secret_in_the_reasoning_never_reaches_the_traffic() {
    let mut a = launched(80, 25).with_ai(ai(true));
    a.update(cwd_event(CWD));
    a.update(AppEvent::EnginesDetected(Detection {
        engines: vec![EngineState {
            id: "ollama",
            status: Status::Available,
            models: vec!["llama3:8b".to_owned()],
            version: None,
        }],
    }));
    press(&mut a, "c");
    a.update(AppEvent::PromptReady(
        chat_id(),
        crate::core::ai::chat::prompts::Resolved {
            text: "s".into(),
            notices: Vec::new(),
        },
    ));
    type_text(&mut a, "q");
    let turn = match a.update(plain(KeyCode::Enter)).as_slice() {
        [Effect::ChatSend { turn, .. }] => *turn,
        other => panic!("{other:?}"),
    };
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::ThinkingDelta("secret plan".into()),
    ));
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Delta("the answer".into()),
    ));
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(Finish::Stop),
    ));
    let mut traffic = a.update(ctrl('n'));
    traffic.extend(a.update(project_reply(Ok(PROJECT.into()))));
    type_text(&mut a, "T");
    traffic.extend(a.update(ctrl('s')));
    assert!(
        !format!("{traffic:?}").contains("secret plan"),
        "{traffic:?}"
    );
}
