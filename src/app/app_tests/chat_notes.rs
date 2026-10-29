//! Save as note (engram-memory M7b): the Prefix n key, the note editor and its keys,
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
        port::{MemCmd, MemReply, Note, Reason, SaveId},
        reply::ProjectError,
    },
};

pub(super) const PROJECT: &str = "lazarobox-shell";
pub(super) const CWD: &str = "/srv/app";

fn chat_id() -> PaneId {
    id2()
}

pub(super) fn ai(memory: bool) -> AiConfig {
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

pub(super) fn ctrl(c: char) -> AppEvent {
    code(KeyCode::Char(c), KeyModifiers::CONTROL)
}

pub(super) fn plain(code_: KeyCode) -> AppEvent {
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

pub(super) fn type_text(a: &mut App, text: &str) {
    for c in text.chars() {
        a.update(key(c));
    }
}

/// A chat opened from `/srv/app` whose first exchange answered `answer`.
pub(super) fn with_answer(memory: bool, answer: &str) -> App {
    answered(memory, answer, true)
}

/// `with_answer`, with or without a known origin directory.
fn answered(memory: bool, answer: &str, known_cwd: bool) -> App {
    let mut a = launched(80, 25).with_ai(ai(memory));
    if known_cwd {
        a.update(cwd_event(CWD));
    }
    a.update(AppEvent::EnginesDetected(Detection {
        engines: vec![EngineState {
            binary: None,
            id: "ollama",
            status: Status::Available,
            models: vec!["llama3:8b".to_owned()],
            version: None,
            model_labels: Vec::new(),
            diagnostics: Default::default(),
        }],
        ..Default::default()
    }));
    if known_cwd {
        press(&mut a, "c");
    } else {
        a.open_chat_tab(None);
    }
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
pub(super) fn ready(answer: &str) -> App {
    let mut a = with_answer(true, answer);
    press(&mut a, "n");
    a.update(AppEvent::Memory(MemReply::Project {
        pane: chat_id(),
        epoch: 0,
        result: Ok(PROJECT.into()),
    }));
    assert!(chat_of(&a).note_editor().is_some(), "the editor opened");
    a
}

fn resolve() -> Effect {
    Effect::Memory(MemCmd::Resolve {
        pane: chat_id(),
        epoch: 0,
        cwd: CWD.into(),
    })
}

fn project_reply(result: Result<String, ProjectError>) -> AppEvent {
    AppEvent::Memory(MemReply::Project {
        pane: chat_id(),
        epoch: 0,
        result,
    })
}

/// The oldest save the chat still awaits: its token and the project it was
/// sent under, as the worker would echo them.
pub(super) fn oldest_save(a: &App) -> (SaveId, String) {
    let save = chat_of(a)
        .pending_save_ids()
        .first()
        .and_then(|id| chat_of(a).pending_save(*id))
        .expect("a pending save");
    (save.id, save.project)
}

/// The worker's `Saved` for the oldest pending save.
pub(super) fn saved_reply(
    a: &App,
    session: &str,
    id: i64,
    topic_key: &str,
    title: &str,
) -> AppEvent {
    let (token, project) = oldest_save(a);
    AppEvent::Memory(MemReply::Saved {
        pane: chat_id(),
        id: token,
        project,
        session: session.into(),
        observation_id: id,
        topic_key: topic_key.into(),
        title: title.into(),
    })
}

const SESSION: &str = "lazarobox-0123456789abcdef0123456789abcdef";

/// A key that does nothing anywhere, between two presses of one key: a key
/// pressed again within half a second counts as held, and is ignored.
pub(super) fn pause(a: &mut App) {
    a.update(plain(KeyCode::PageUp));
}

pub(super) fn systems(a: &App) -> Vec<String> {
    chat_of(a)
        .entries()
        .iter()
        .filter_map(|e| match e {
            Entry::System(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

// 9.3: Prefix n ----------------------------------------------------------------

#[test]
fn memory_off_shows_the_notice_and_asks_for_nothing() {
    let mut a = with_answer(false, "an answer");
    assert_eq!(press(&mut a, "n"), vec![]);
    assert_eq!(a.notice(), Some(MEMORY_OFF));
    assert!(chat_of(&a).note_editor().is_none());
}

#[test]
fn without_an_answer_there_is_nothing_to_save() {
    let mut a = launched(80, 25).with_ai(ai(true));
    press(&mut a, "c");
    assert_eq!(press(&mut a, "n"), vec![]);
    assert_eq!(a.notice(), Some(NOTHING_TO_SAVE));
    assert!(chat_of(&a).note_editor().is_none());
}

/// A chat opened while memory was off, which was then turned on in the menu:
/// nobody asked for its project yet.
fn opened_before_memory_was_on(answer: &str) -> App {
    let mut a = with_answer(false, answer);
    a.ai.memory = ai(true).memory;
    a
}

#[test]
fn the_first_save_key_asks_for_the_project_and_opens_the_editor_on_the_reply() {
    let mut a = opened_before_memory_was_on("use Axum");
    assert_eq!(chat_of(&a).readiness(), &Readiness::Unknown);
    assert_eq!(press(&mut a, "n"), vec![resolve()]);
    assert!(chat_of(&a).note_editor().is_none(), "waits for the project");
    let effects = a.update(project_reply(Ok(PROJECT.into())));
    assert_eq!(
        effects,
        [Effect::Memory(MemCmd::Context {
            pane: chat_id(),
            epoch: 0,
            project: PROJECT.into()
        })],
        "the view had no context yet, so it asks for it"
    );
    let editor = chat_of(&a).note_editor().expect("open");
    assert_eq!(editor.content(), "use Axum");
    assert_eq!(editor.kind(), MemKind::Decision);
    assert_eq!(editor.title(), "");
}

#[test]
fn a_save_key_pressed_while_the_open_check_runs_opens_the_editor_on_its_reply() {
    let mut a = with_answer(true, "use Axum");
    assert_eq!(
        chat_of(&a).readiness(),
        &Readiness::Resolving { want_note: false },
        "the view asked for its project when it opened"
    );
    assert_eq!(press(&mut a, "n"), vec![], "no second resolve");
    assert!(chat_of(&a).note_editor().is_none(), "waits for the project");
    a.update(project_reply(Ok(PROJECT.into())));
    assert!(chat_of(&a).note_editor().is_some());
}

#[test]
fn a_known_project_opens_the_editor_at_once() {
    let mut a = ready("use Axum");
    a.update(plain(KeyCode::Esc));
    assert!(chat_of(&a).note_editor().is_none());
    pause(&mut a);
    assert_eq!(press(&mut a, "n"), vec![], "no second resolve");
    assert!(chat_of(&a).note_editor().is_some());
}

#[test]
fn ctrl_n_does_not_touch_the_composer_or_reach_a_pty() {
    let mut a = with_answer(true, "x");
    type_text(&mut a, "draft");
    let effects = press(&mut a, "n");
    assert!(
        effects.iter().all(|e| !matches!(e, Effect::WritePty(..))),
        "{effects:?}"
    );
    assert_eq!(chat_of(&a).composer().text(), "draft");
}

#[test]
fn an_unavailable_memory_says_why_and_probes_again_on_the_next_try() {
    let mut a = with_answer(true, "x");
    press(&mut a, "n");
    a.update(AppEvent::Memory(MemReply::Unavailable {
        epoch: None,
        pane: Some(chat_id()),
        reason: Reason::NotRunning,
    }));
    assert_eq!(
        a.notice(),
        Some("Memory is not ready: engram is not running")
    );
    assert!(chat_of(&a).note_editor().is_none());
    pause(&mut a);
    assert_eq!(press(&mut a, "n"), vec![resolve()], "a re-probe");
    assert_eq!(
        a.notice(),
        Some("Memory is not ready: engram is not running")
    );
}

#[test]
fn a_directory_that_is_not_git_is_not_ready() {
    let mut a = with_answer(true, "x");
    press(&mut a, "n");
    a.update(project_reply(Err(ProjectError::NotGit)));
    assert_eq!(
        a.notice(),
        Some("Memory is not ready: memory needs a git project")
    );
    assert!(chat_of(&a).note_editor().is_none());
}

#[test]
fn without_a_known_directory_memory_is_not_ready_and_nothing_is_asked() {
    let mut a = answered(true, "x", false);
    assert_eq!(press(&mut a, "n"), vec![], "no directory to resolve");
    assert_eq!(
        a.notice(),
        Some("Memory is not ready: memory needs a git project")
    );
    assert_eq!(
        chat_of(&a).readiness(),
        &Readiness::Unavailable("memory needs a git project".into()),
        "not stuck in Resolving"
    );
    // A later try says the same, not "Checking memory…" forever.
    pause(&mut a);
    assert_eq!(press(&mut a, "n"), vec![]);
    assert_eq!(
        a.notice(),
        Some("Memory is not ready: memory needs a git project")
    );
}

#[test]
fn a_second_ctrl_n_while_waiting_asks_nothing_more() {
    let mut a = with_answer(true, "x");
    press(&mut a, "n");
    pause(&mut a);
    assert_eq!(press(&mut a, "n"), vec![]);
    assert_eq!(a.notice(), Some("Checking memory…"));
}

#[test]
fn a_reply_for_a_pane_that_is_gone_changes_nothing() {
    let mut a = with_answer(true, "x");
    let gone = AppEvent::Memory(MemReply::Project {
        pane: PaneId::for_test(9),
        epoch: 0,
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
fn ctrl_n_alone_does_nothing_in_a_chat() {
    let mut a = opened_before_memory_was_on("x");
    for event in [ctrl('n'), held(KeyCode::Char('n'), KeyModifiers::CONTROL)] {
        assert_eq!(a.update(event), vec![]);
    }
    assert_eq!(a.notice(), None);
    assert_eq!(chat_of(&a).readiness(), &Readiness::Unknown);
    assert!(chat_of(&a).note_editor().is_none());
}

#[test]
fn the_save_note_key_on_a_shell_pane_only_says_it_is_for_chats() {
    let mut a = launched(80, 25);
    assert_eq!(press(&mut a, "n"), vec![]);
    assert_eq!(a.notice(), Some(NOTE_CHAT_ONLY));
    assert_eq!(a.input(), InputMode::Terminal);
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

// A terminal that sends ^H for Backspace delivers Ctrl+H.
#[test]
fn ctrl_h_deletes_like_backspace_in_the_note_editor() {
    let mut a = ready("body");
    type_text(&mut a, "Ab");
    a.update(ctrl('h'));
    assert_eq!(chat_of(&a).note_editor().unwrap().title(), "A");
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
    press(&mut a, "n");
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
    press(&mut a, "n");
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

fn save_cmd(n: u64, session: Option<&str>, title: &str, content: &str) -> Effect {
    Effect::Memory(MemCmd::Save {
        pane: chat_id(),
        id: SaveId(n),
        session: session.map(str::to_owned),
        project: PROJECT.into(),
        cwd: CWD.into(),
        note: Note {
            kind: MemKind::Decision,
            title: title.into(),
            content: content.into(),
        },
        topic_key: None,
    })
}

#[test]
fn ctrl_s_sends_one_save_without_a_session_the_first_time() {
    let mut a = ready("use Axum");
    type_text(&mut a, "Use Axum");
    assert_eq!(
        a.update(ctrl('s')),
        vec![save_cmd(1, None, "Use Axum", "use Axum")]
    );
    assert!(chat_of(&a).note_editor().unwrap().is_saving());
    pause(&mut a);
    assert_eq!(a.update(ctrl('s')), vec![], "a second save is not sent");
}

#[test]
fn memory_turned_off_after_the_editor_opened_sends_no_save() {
    let mut a = ready("use Axum");
    type_text(&mut a, "Use Axum");
    // The menu applied a draft with memory off while the editor was open.
    let mut a = a.with_ai(ai(false));
    assert_eq!(a.update(ctrl('s')), vec![], "no Save effect");
    assert_eq!(a.notice(), Some(MEMORY_OFF));
    let editor = chat_of(&a).note_editor().expect("the editor stays open");
    assert_eq!(editor.title(), "Use Axum");
    assert_eq!(editor.notice(), Some(MEMORY_OFF));
    assert!(!editor.is_saving(), "free to save once memory is back");
    // Back on, the same editor saves.
    let mut a = a.with_ai(ai(true));
    pause(&mut a);
    assert!(matches!(
        a.update(ctrl('s')).as_slice(),
        [Effect::Memory(MemCmd::Save { .. })]
    ));
}

#[test]
fn enter_in_the_title_saves_too() {
    let mut a = ready("body");
    type_text(&mut a, "T");
    assert_eq!(
        a.update(plain(KeyCode::Enter)),
        vec![save_cmd(1, None, "T", "body")]
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
        &a,
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
    a.update(saved_reply(&a, SESSION, 1, "chat/decision/one", "One"));
    press(&mut a, "n");
    type_text(&mut a, "Two");
    assert_eq!(
        a.update(ctrl('s')),
        vec![save_cmd(2, Some(SESSION), "Two", "use Axum")]
    );
}

#[test]
fn the_same_note_again_upserts_with_the_same_line() {
    let mut a = ready("use Axum");
    type_text(&mut a, "Use Axum");
    a.update(ctrl('s'));
    a.update(saved_reply(
        &a,
        SESSION,
        1,
        "chat/decision/use-axum",
        "Use Axum",
    ));
    press(&mut a, "n");
    type_text(&mut a, "Use Axum");
    a.update(ctrl('s'));
    a.update(saved_reply(
        &a,
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
        id: oldest_save(&a).0,
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
    a.update(AppEvent::Memory(MemReply::SaveUnavailable {
        pane: chat_id(),
        id: oldest_save(&a).0,
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
    press(&mut a, "n");
    type_text(&mut a, "Other");
    a.update(saved_reply(&a, SESSION, 1, "chat/decision/t", "T"));
    assert_eq!(systems(&a), ["saved note decision: T"]);
    let editor = chat_of(&a).note_editor().expect("the new editor stays");
    assert_eq!(editor.title(), "Other");
    assert!(chat_of(&a).session().is_some());
}

fn save_failed(a: &App, reason: &str) -> AppEvent {
    AppEvent::Memory(MemReply::SaveFailed {
        pane: chat_id(),
        id: oldest_save(a).0,
        reason: reason.into(),
    })
}

#[test]
fn a_failure_that_comes_after_esc_is_said_in_the_transcript() {
    let mut a = ready("use Axum");
    type_text(&mut a, "T");
    a.update(ctrl('s'));
    a.update(plain(KeyCode::Esc));
    assert_eq!(a.update(save_failed(&a, "engram refused the note")), vec![]);
    assert_eq!(systems(&a), ["note not saved: engram refused the note"]);
    assert!(chat_of(&a).note_editor().is_none());
}

#[test]
fn a_late_unavailable_after_esc_is_said_too() {
    let mut a = ready("use Axum");
    type_text(&mut a, "T");
    a.update(ctrl('s'));
    a.update(plain(KeyCode::Esc));
    a.update(AppEvent::Memory(MemReply::SaveUnavailable {
        pane: chat_id(),
        id: oldest_save(&a).0,
        reason: Reason::NoAnswer,
    }));
    assert_eq!(systems(&a), ["note not saved: engram did not answer"]);
}

#[test]
fn an_old_reply_never_closes_or_fails_a_newer_editor() {
    let mut a = ready("use Axum");
    type_text(&mut a, "One");
    a.update(ctrl('s'));
    a.update(plain(KeyCode::Esc));
    pause(&mut a);
    press(&mut a, "n");
    type_text(&mut a, "Two");
    a.update(ctrl('s'));
    // The answer to the first save: the second editor goes on waiting.
    a.update(save_failed(&a, "engram refused the note"));
    let editor = chat_of(&a).note_editor().expect("the newer editor stays");
    assert!(editor.is_saving(), "still waiting for its own answer");
    assert_eq!(editor.notice(), None);
    assert_eq!(systems(&a), ["note not saved: engram refused the note"]);
    // The answer to the second one closes it.
    a.update(saved_reply(&a, SESSION, 2, "chat/decision/two", "Two"));
    assert!(chat_of(&a).note_editor().is_none());
}

#[test]
fn the_saved_reply_of_an_old_save_leaves_a_newer_saving_editor_open() {
    let mut a = ready("use Axum");
    type_text(&mut a, "One");
    a.update(ctrl('s'));
    a.update(plain(KeyCode::Esc));
    pause(&mut a);
    press(&mut a, "n");
    type_text(&mut a, "Two");
    a.update(ctrl('s'));
    a.update(saved_reply(&a, SESSION, 1, "chat/decision/one", "One"));
    let editor = chat_of(&a).note_editor().expect("the newer editor stays");
    assert!(editor.is_saving());
    a.update(saved_reply(&a, SESSION, 2, "chat/decision/two", "Two"));
    assert!(chat_of(&a).note_editor().is_none());
}

#[test]
fn a_saved_reply_for_a_pane_that_is_gone_is_dropped() {
    let mut a = ready("x");
    let stale = AppEvent::Memory(MemReply::Saved {
        pane: PaneId::for_test(9),
        id: SaveId(1),
        project: PROJECT.into(),
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
            binary: None,
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
    let mut traffic = press(&mut a, "n");
    traffic.extend(a.update(project_reply(Ok(PROJECT.into()))));
    type_text(&mut a, "T");
    traffic.extend(a.update(ctrl('s')));
    assert!(
        !format!("{traffic:?}").contains("secret plan"),
        "{traffic:?}"
    );
}

// 9.6: links to the conversation (chat.db) ------------------------------------

use crate::core::ai::chat::store::{LinkKind, StoreCmd, StoreReply};

fn created(conversation: i64, evicted: bool, evicted_notes: u32) -> AppEvent {
    AppEvent::Store(StoreReply::Created {
        pane: chat_id(),
        conversation,
        evicted,
        evicted_notes,
        created_at: 1_000,
    })
}

fn link_session(conversation: i64) -> Effect {
    Effect::Store(StoreCmd::LinkSession {
        pane: chat_id(),
        conversation,
        session_id: SESSION.into(),
        project: PROJECT.into(),
        replace: false,
    })
}

fn link_note(conversation: i64, id: i64, topic_key: &str, title: &str) -> Effect {
    Effect::Store(StoreCmd::LinkNote {
        pane: chat_id(),
        conversation,
        project: PROJECT.into(),
        topic_key: topic_key.into(),
        observation_id: id,
        title: title.into(),
    })
}

/// Writes a note called `title` and delivers the worker's `Saved`.
fn write_note(a: &mut App, title: &str, id: i64) -> Vec<Effect> {
    press(a, "n");
    type_text(a, title);
    a.update(ctrl('s'));
    let key = format!("chat/decision/{}", title.to_lowercase());
    a.update(saved_reply(a, SESSION, id, &key, title))
}

#[test]
fn a_saved_note_links_the_session_and_itself_once_the_conversation_exists() {
    let mut a = ready("body");
    a.update(created(7, false, 0));
    type_text(&mut a, "One");
    a.update(ctrl('s'));
    let effects = a.update(saved_reply(&a, SESSION, 41, "chat/decision/one", "One"));
    assert_eq!(
        effects,
        vec![
            link_session(7),
            link_note(7, 41, "chat/decision/one", "One")
        ]
    );
    // The session is linked once; later notes link themselves only.
    let effects = write_note(&mut a, "Two", 42);
    assert_eq!(effects, vec![link_note(7, 42, "chat/decision/two", "Two")]);
}

#[test]
fn links_wait_for_the_conversation_and_follow_it() {
    let mut a = ready("body");
    type_text(&mut a, "One");
    a.update(ctrl('s'));
    let effects = a.update(saved_reply(&a, SESSION, 41, "chat/decision/one", "One"));
    assert_eq!(effects, vec![], "no conversation to link to yet");
    assert_eq!(
        a.update(created(7, false, 0)),
        vec![
            link_session(7),
            link_note(7, 41, "chat/decision/one", "One")
        ]
    );
}

const OTHER_SESSION: &str = "lazarobox-ffffffffffffffffffffffffffffffff";

// chat-folder D6: the project follows the folder, so the session does too.
#[test]
fn a_folder_change_ends_the_session_and_links_the_next_one_in_the_new_project() {
    let mut a = ready("body");
    a.update(created(7, false, 0));
    type_text(&mut a, "One");
    a.update(ctrl('s'));
    a.update(saved_reply(&a, SESSION, 41, "chat/decision/one", "One"));

    let effects = a.apply_folder(chat_id(), "/srv/other".into());
    assert!(
        effects.contains(&Effect::Memory(MemCmd::End {
            session: SESSION.into(),
            project: PROJECT.into(),
            summary: "Notes: One".into(),
            cwd: None,
        })),
        "{effects:?}"
    );
    assert!(effects.contains(&Effect::Memory(MemCmd::Resolve {
        pane: chat_id(),
        cwd: "/srv/other".into(),
        epoch: 1,
    })));
    assert!(chat_of(&a).session().is_none());

    // The project of the new folder answers; the next note opens a session
    // there and the link replaces the stored one.
    a.update(AppEvent::Memory(MemReply::Project {
        pane: chat_id(),
        epoch: 1,
        result: Ok("other".into()),
    }));
    pause(&mut a);
    press(&mut a, "n");
    type_text(&mut a, "Two");
    a.update(ctrl('s'));
    let effects = a.update(saved_reply(
        &a,
        OTHER_SESSION,
        42,
        "chat/decision/two",
        "Two",
    ));
    assert_eq!(
        effects,
        vec![
            Effect::Store(StoreCmd::LinkSession {
                pane: chat_id(),
                conversation: 7,
                session_id: OTHER_SESSION.into(),
                project: "other".into(),
                replace: true,
            }),
            Effect::Store(StoreCmd::LinkNote {
                pane: chat_id(),
                conversation: 7,
                project: "other".into(),
                topic_key: "chat/decision/two".into(),
                observation_id: 42,
                title: "Two".into(),
            }),
        ]
    );
}

// A save answered after the folder changed (its wait ran out first) belongs to
// the old project: it must not adopt the old session or record its note.
#[test]
fn a_late_save_answer_of_the_old_folder_is_ignored() {
    let mut a = ready("body");
    a.update(created(7, false, 0));
    type_text(&mut a, "One");
    a.update(ctrl('s'));
    let late = saved_reply(&a, SESSION, 41, "chat/decision/one", "One");
    a.apply_folder(chat_id(), "/srv/other".into());
    a.take_dirty();
    assert_eq!(a.update(late), vec![]);
    assert!(chat_of(&a).session().is_none());
    assert!(!systems(&a).iter().any(|l| l.contains("saved note")));

    for reply in [
        MemReply::SaveFailed {
            pane: chat_id(),
            id: crate::core::ai::memory::port::SaveId(1),
            reason: "boom".into(),
        },
        MemReply::SaveUnavailable {
            pane: chat_id(),
            id: crate::core::ai::memory::port::SaveId(1),
            reason: Reason::NotRunning,
        },
    ] {
        let before = systems(&a);
        assert_eq!(a.update(AppEvent::Memory(reply)), vec![]);
        assert_eq!(systems(&a), before);
    }
}

#[test]
fn the_stored_pair_of_a_resumed_conversation_wins() {
    let mut a = ready("body");
    a.update(created(7, false, 0));
    type_text(&mut a, "One");
    a.update(ctrl('s'));
    a.update(saved_reply(&a, SESSION, 41, "chat/decision/one", "One"));
    let stored = "lazarobox-ffffffffffffffffffffffffffffffff";
    a.update(AppEvent::Store(StoreReply::SessionLinked {
        pane: chat_id(),
        conversation: 7,
        session_id: stored.into(),
        project: "other-project".into(),
    }));
    press(&mut a, "n");
    type_text(&mut a, "Two");
    let save = a.update(ctrl('s'));
    let [
        Effect::Memory(MemCmd::Save {
            session, project, ..
        }),
    ] = save.as_slice()
    else {
        panic!("one save, got {save:?}");
    };
    assert_eq!(session.as_deref(), Some(stored));
    assert_eq!(
        project, "other-project",
        "the project the session was made with"
    );
    // What the save was sent under is what gets recorded and linked, even
    // though the chat's own project differs.
    let saved = a.update(saved_reply(&a, stored, 42, "chat/decision/two", "Two"));
    let [Effect::Store(StoreCmd::LinkNote { project, .. })] = saved.as_slice() else {
        panic!("one note link, got {saved:?}");
    };
    assert_eq!(project, "other-project");
}

#[test]
fn a_link_failure_is_said_once_and_the_note_stays_saved() {
    let mut a = ready("body");
    a.update(created(7, false, 0));
    type_text(&mut a, "One");
    a.update(ctrl('s'));
    a.update(saved_reply(&a, SESSION, 41, "chat/decision/one", "One"));
    for kind in [LinkKind::Note, LinkKind::Session, LinkKind::Note] {
        let failed = AppEvent::Store(StoreReply::LinkFailed {
            pane: chat_id(),
            kind,
        });
        assert_eq!(a.update(failed), vec![]);
    }
    assert_eq!(
        systems(&a),
        ["saved note decision: One", "note saved but not linked"]
    );
    // The chat keeps working.
    assert!(chat_of(&a).note_editor().is_none());
    write_note(&mut a, "Two", 42);
    assert_eq!(systems(&a).len(), 3);
}

#[test]
fn store_replies_for_a_pane_that_is_gone_change_nothing() {
    let mut a = ready("body");
    let gone = PaneId::for_test(9);
    for reply in [
        StoreReply::Created {
            pane: gone,
            conversation: 1,
            evicted: true,
            evicted_notes: 3,
            created_at: 1_000,
        },
        StoreReply::LinkFailed {
            pane: gone,
            kind: LinkKind::Note,
        },
        StoreReply::SessionLinked {
            pane: gone,
            conversation: 1,
            session_id: SESSION.into(),
            project: PROJECT.into(),
        },
    ] {
        assert_eq!(a.update(AppEvent::Store(reply)), vec![]);
    }
    assert!(systems(&a).is_empty());
}

// A write that failed for a pane that never saved is nobody's news. (An
// `Unavailable` store does earn its line: see `chat_history`.)
#[test]
fn a_failed_write_for_a_pane_that_never_saved_is_ignored() {
    let mut a = ready("body");
    let reply = StoreReply::Failed {
        pane: chat_id(),
        notify: true,
    };
    assert_eq!(a.update(AppEvent::Store(reply)), vec![]);
    assert!(systems(&a).is_empty());
}

// 8.6 / 8.8: the eviction lines reach the transcript ----------------------------

#[test]
fn an_eviction_adds_its_line_and_the_note_count() {
    let mut a = with_answer(true, "x");
    assert_eq!(a.update(created(7, true, 2)), vec![]);
    assert_eq!(
        systems(&a),
        [
            "oldest conversation removed (limit 200)",
            "2 notes of removed conversations remain in engram"
        ]
    );
}

#[test]
fn one_note_is_singular_and_zero_adds_no_line() {
    let mut a = with_answer(true, "x");
    a.update(created(7, true, 1));
    assert_eq!(
        systems(&a),
        [
            "oldest conversation removed (limit 200)",
            "1 note of a removed conversation remains in engram"
        ]
    );
    let mut b = with_answer(true, "x");
    b.update(created(7, true, 0));
    assert_eq!(systems(&b), ["oldest conversation removed (limit 200)"]);
}

#[test]
fn nothing_evicted_adds_no_line() {
    let mut a = with_answer(true, "x");
    assert_eq!(a.update(created(7, false, 5)), vec![]);
    assert!(systems(&a).is_empty());
}

#[test]
fn the_eviction_lines_show_with_memory_off_and_ask_nothing() {
    let mut a = with_answer(false, "x");
    assert_eq!(a.update(created(7, true, 3)), vec![]);
    assert_eq!(systems(&a).len(), 2);
}

// 9.6: the session ends once --------------------------------------------------

fn end(summary: &str) -> Effect {
    Effect::Memory(MemCmd::End {
        session: SESSION.into(),
        project: PROJECT.into(),
        summary: summary.into(),
        cwd: None,
    })
}

/// A chat that saved the notes `A` and `B` into the session.
fn wrote_two() -> App {
    let mut a = ready("body");
    type_text(&mut a, "A");
    a.update(ctrl('s'));
    a.update(saved_reply(&a, SESSION, 1, "chat/decision/a", "A"));
    write_note(&mut a, "B", 2);
    a
}

#[test]
fn closing_the_pane_ends_the_session_with_the_note_titles() {
    let mut a = wrote_two();
    press(&mut a, "wq");
    let effects = a.update(key('y'));
    assert!(effects.contains(&end("Notes: A; B")), "{effects:?}");
    assert_eq!(
        effects
            .iter()
            .filter(|e| matches!(e, Effect::Memory(MemCmd::End { .. })))
            .count(),
        1
    );
}

#[test]
fn quitting_ends_the_session_before_the_quit() {
    let mut a = wrote_two();
    press(&mut a, "q");
    assert_eq!(
        a.update(key('y')),
        vec![end("Notes: A; B"), Effect::BatchClose(id2()), Effect::Quit]
    );
}

#[test]
fn closing_a_tab_ends_the_sessions_of_its_chats() {
    let mut a = wrote_two();
    // A second tab keeps the app alive; the chat's tab is closed.
    press(&mut a, "tn");
    press(&mut a, "b1");
    press(&mut a, "tc");
    let effects = a.update(key('y'));
    assert_eq!(a.tab_count(), 1, "{effects:?}");
    assert!(effects.contains(&end("Notes: A; B")), "{effects:?}");
}

#[test]
fn a_chat_that_never_wrote_sends_no_end() {
    let mut a = with_answer(true, "x");
    press(&mut a, "wq");
    let effects = a.update(key('y'));
    assert!(
        effects.iter().all(|e| !matches!(e, Effect::Memory(_))),
        "{effects:?}"
    );
    let mut b = with_answer(true, "x");
    press(&mut b, "q");
    assert_eq!(
        b.update(key('y')),
        vec![Effect::BatchClose(id2()), Effect::Quit]
    );
}

#[test]
fn the_end_is_not_sent_twice() {
    let mut a = wrote_two();
    press(&mut a, "wq");
    let first = a.update(key('y'));
    assert!(first.contains(&end("Notes: A; B")));
    press(&mut a, "q");
    assert_eq!(a.update(key('y')), vec![Effect::Quit], "the pane is gone");
}

#[test]
fn the_reasoning_never_reaches_the_end_summary() {
    let mut a = wrote_two();
    press(&mut a, "q");
    let effects = a.update(key('y'));
    assert!(!format!("{effects:?}").contains("secret plan"));
}

// Replies are matched to their save by token, not by queue order -------------

/// Two saves in flight: the first under `PROJECT`, the second under
/// `OTHER_PROJECT` (the pair of a resumed conversation arrived in between).
/// Returns the tokens of both.
const OTHER_PROJECT: &str = "other-project";

fn two_saves_in_flight() -> (App, SaveId, SaveId) {
    let mut a = ready("body");
    a.update(created(7, false, 0));
    type_text(&mut a, "One");
    a.update(ctrl('s'));
    a.update(plain(KeyCode::Esc));
    a.update(AppEvent::Store(StoreReply::SessionLinked {
        pane: chat_id(),
        conversation: 7,
        session_id: "lazarobox-ffffffffffffffffffffffffffffffff".into(),
        project: OTHER_PROJECT.into(),
    }));
    pause(&mut a);
    press(&mut a, "n");
    type_text(&mut a, "Two");
    a.update(ctrl('s'));
    let ids = chat_of(&a).pending_save_ids();
    assert_eq!(ids.len(), 2, "both saves await their answer");
    (a, ids[0], ids[1])
}

fn saved_as(id: SaveId, project: &str, obs: i64, key: &str, title: &str) -> AppEvent {
    AppEvent::Memory(MemReply::Saved {
        pane: chat_id(),
        id,
        project: project.into(),
        session: SESSION.into(),
        observation_id: obs,
        topic_key: key.into(),
        title: title.into(),
    })
}

fn note_link_project(effects: &[Effect]) -> String {
    let [.., Effect::Store(StoreCmd::LinkNote { project, .. })] = effects else {
        panic!("a note link, got {effects:?}");
    };
    project.clone()
}

#[test]
fn a_busy_for_the_newest_save_does_not_swallow_the_oldest_one() {
    let (mut a, first, second) = two_saves_in_flight();
    // The link refused the second save at once: it was never queued.
    a.update(AppEvent::Memory(MemReply::SaveUnavailable {
        pane: chat_id(),
        id: second,
        reason: Reason::Busy,
    }));
    let editor = chat_of(&a).note_editor().expect("the second editor stays");
    assert_eq!(editor.notice(), Some("engram is busy"), "its own failure");
    assert_eq!(chat_of(&a).pending_save_ids(), vec![first]);
    // The first save was really queued: its answer belongs to the first project.
    let saved = a.update(saved_as(first, PROJECT, 1, "chat/decision/one", "One"));
    assert_eq!(note_link_project(&saved), PROJECT);
    assert!(chat_of(&a).pending_save_ids().is_empty());
}

#[test]
fn replies_in_the_reverse_order_still_find_their_own_save() {
    let (mut a, first, second) = two_saves_in_flight();
    let saved = a.update(saved_as(
        second,
        OTHER_PROJECT,
        2,
        "chat/decision/two",
        "Two",
    ));
    assert_eq!(note_link_project(&saved), OTHER_PROJECT);
    assert!(
        chat_of(&a).note_editor().is_none(),
        "the editor of the second save closes"
    );
    assert_eq!(chat_of(&a).pending_save_ids(), vec![first]);
    let saved = a.update(saved_as(first, PROJECT, 1, "chat/decision/one", "One"));
    assert_eq!(note_link_project(&saved), PROJECT);
}

#[test]
fn a_saved_reply_with_an_unknown_token_is_recorded_under_its_own_project() {
    let (mut a, first, _second) = two_saves_in_flight();
    let saved = a.update(saved_as(SaveId(99), "echoed", 5, "chat/decision/x", "X"));
    assert_eq!(note_link_project(&saved), "echoed", "the note is not lost");
    assert_eq!(chat_of(&a).pending_save_ids().len(), 2, "no save consumed");
    assert!(
        chat_of(&a).note_editor().unwrap().is_saving(),
        "no editor is touched by an answer nobody awaits"
    );
    assert!(chat_of(&a).pending_save_ids().contains(&first));
}

#[test]
fn a_failure_with_an_unknown_token_is_said_and_touches_nothing() {
    let (mut a, _first, _second) = two_saves_in_flight();
    a.update(AppEvent::Memory(MemReply::SaveFailed {
        pane: chat_id(),
        id: SaveId(99),
        reason: "engram refused the note".into(),
    }));
    a.update(AppEvent::Memory(MemReply::SaveUnavailable {
        pane: chat_id(),
        id: SaveId(98),
        reason: Reason::NoAnswer,
    }));
    assert_eq!(
        systems(&a),
        [
            "note not saved: engram refused the note",
            "note not saved: engram did not answer"
        ]
    );
    assert!(chat_of(&a).note_editor().unwrap().is_saving());
    assert_eq!(chat_of(&a).pending_save_ids().len(), 2);
}

#[test]
fn a_restart_needed_answer_fails_the_save_it_belongs_to() {
    let mut a = ready("body");
    type_text(&mut a, "One");
    let effects = a.update(ctrl('s'));
    let [Effect::Memory(cmd)] = effects.as_slice() else {
        panic!("one save, got {effects:?}");
    };
    // What a memory without a worker answers for that very command.
    let reply = cmd.unavailable(Reason::RestartNeeded);
    a.update(AppEvent::Memory(reply));
    let editor = chat_of(&a).note_editor().expect("still open");
    assert_eq!(editor.notice(), Some("memory applies after a restart"));
    assert!(!editor.is_saving());
    assert!(chat_of(&a).pending_save_ids().is_empty());
}
