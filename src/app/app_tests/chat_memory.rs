//! Memory in a chat view (engram-memory M6): the memory line and its key, the
//! context flow, the degradation lines.

use super::chat_notes::{CWD, PROJECT, ai, systems, type_text};
use super::*;
use crate::core::ai::{
    EngineState, Status,
    chat::{
        Entry,
        engine::{ChatRequest, Finish, StreamEvent},
        memory::Readiness,
        prompts::Resolved,
    },
    memory::{
        context::ContextBlock,
        port::{MemCmd, MemReply, Reason},
        reply::ProjectError,
    },
};

fn chat_id() -> PaneId {
    id2()
}

fn chat_of(a: &App) -> &ChatState {
    match a.pane_view(chat_id()) {
        Some(PaneView::Chat(chat)) => chat,
        _ => panic!("pane 2 is not a chat"),
    }
}

fn block(notes: usize) -> ContextBlock {
    let text = "<project-memory source=\"engram\" project=\"p\">\n- [decision] **Use Axum**\n</project-memory>"
        .to_owned();
    ContextBlock {
        bytes: text.len(),
        notes,
        text,
    }
}

/// A chat pane holding a memory line of `notes` notes.
fn with_line(notes: usize) -> App {
    let mut a = launched(80, 25);
    a.open_chat(None);
    a.panes
        .get_mut(&chat_id())
        .and_then(PaneState::chat_mut)
        .expect("a chat")
        .inject_memory(block(notes));
    a.take_dirty();
    a
}

fn head(a: &App) -> Option<String> {
    chat_of(a).entries().iter().find_map(|e| match e {
        Entry::Memory(line) => Some(line.head()),
        _ => None,
    })
}

// 10.4: the memory key ------------------------------------------------------

#[test]
fn prefix_o_unfolds_and_folds_the_memory_line_without_an_effect() {
    let mut a = with_line(3);
    assert_eq!(head(&a).as_deref(), Some("▸ memory (3 notes)"));
    assert_eq!(press(&mut a, "o"), vec![]);
    assert_eq!(head(&a).as_deref(), Some("▾ memory (3 notes)"));
    assert!(a.take_dirty());
    assert_eq!(press(&mut a, "o"), vec![]);
    assert_eq!(head(&a).as_deref(), Some("▸ memory (3 notes)"));
}

#[test]
fn prefix_o_with_no_memory_line_does_nothing() {
    let mut a = launched(80, 25);
    a.open_chat(None);
    let before = chat_of(&a).entries().to_vec();
    assert_eq!(press(&mut a, "o"), vec![]);
    assert_eq!(chat_of(&a).entries(), before);
    assert_eq!(a.notice(), None);
}

#[test]
fn prefix_o_on_a_shell_pane_says_it_is_for_chats() {
    let mut a = launched(80, 25);
    assert_eq!(press(&mut a, "o"), vec![]);
    assert_eq!(a.notice(), Some(MEMORY_CHAT_ONLY));
}

#[test]
fn ctrl_o_is_not_bound_in_a_chat() {
    let mut a = with_line(3);
    let effects = a.update(AppEvent::Key(KeyEvent::new(
        KeyCode::Char('o'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(effects, vec![], "swallowed, never a PTY write");
    assert_eq!(head(&a).as_deref(), Some("▸ memory (3 notes)"));
    assert_eq!(chat_of(&a).composer().text(), "", "typed nowhere");
}

// 10.3: the context flow --------------------------------------------------------

fn plain_key(code: KeyCode) -> AppEvent {
    AppEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

/// An app with a ready engine, a chat opened from `/srv/app` with `Prefix c`
/// and its prompt prepared; the effects of the opening come back too.
fn opened(memory: bool) -> (App, Vec<Effect>) {
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
    let effects = press(&mut a, "c");
    a.update(AppEvent::PromptReady(
        chat_id(),
        Resolved {
            text: "system".into(),
            notices: Vec::new(),
        },
    ));
    (a, effects)
}

fn memory_cmds(effects: &[Effect]) -> Vec<MemCmd> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Memory(cmd) => Some(cmd.clone()),
            _ => None,
        })
        .collect()
}

fn resolve() -> MemCmd {
    MemCmd::Resolve {
        pane: chat_id(),
        cwd: CWD.into(),
    }
}

fn context_cmd() -> MemCmd {
    MemCmd::Context {
        pane: chat_id(),
        project: PROJECT.into(),
    }
}

fn project(a: &mut App) -> Vec<Effect> {
    a.update(AppEvent::Memory(MemReply::Project {
        pane: chat_id(),
        result: Ok(PROJECT.into()),
    }))
}

fn context(a: &mut App, block: Option<ContextBlock>) -> Vec<Effect> {
    a.update(AppEvent::Memory(MemReply::Context {
        pane: chat_id(),
        block,
    }))
}

fn unavailable(a: &mut App, reason: Reason) -> Vec<Effect> {
    a.update(AppEvent::Memory(MemReply::Unavailable {
        pane: Some(chat_id()),
        reason,
    }))
}

/// Types `text`, presses Enter and returns the request the turn carries.
fn send(a: &mut App, text: &str) -> ChatRequest {
    type_text(a, text);
    match a.update(plain_key(KeyCode::Enter)).as_slice() {
        [Effect::ChatSend { request, .. }] => request.clone(),
        other => panic!("expected one ChatSend, got {other:?}"),
    }
}

/// Ends the turn in flight so the next send is allowed.
fn finish_turn(a: &mut App) {
    let turn = chat_of(a).turn();
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Delta("ok".into()),
    ));
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(Finish::Stop),
    ));
}

#[test]
fn opening_a_chat_with_memory_resolves_its_project_once() {
    let (a, effects) = opened(true);
    assert_eq!(memory_cmds(&effects), [resolve()]);
    assert_eq!(
        chat_of(&a).readiness(),
        &Readiness::Resolving { want_note: false }
    );
}

#[test]
fn a_known_project_asks_for_its_context_once_and_never_searches() {
    let (mut a, _) = opened(true);
    let effects = project(&mut a);
    assert_eq!(memory_cmds(&effects), [context_cmd()]);
    // A second project answer (a memory key probing again) asks for nothing.
    let again = project(&mut a);
    assert_eq!(memory_cmds(&again), []);
}

#[test]
fn memory_off_opens_a_chat_with_no_traffic_no_line_and_no_notice() {
    let (mut a, effects) = opened(false);
    assert_eq!(memory_cmds(&effects), []);
    assert_eq!(chat_of(&a).readiness(), &Readiness::Unknown);
    assert!(systems(&a).is_empty());
    let request = send(&mut a, "hi");
    assert_eq!(request.memory, None);
}

#[test]
fn a_send_never_waits_for_the_context() {
    let (mut a, _) = opened(true);
    project(&mut a);
    // The context is still in flight: the first turn goes out without it.
    let first = send(&mut a, "first");
    assert_eq!(first.memory, None);
    finish_turn(&mut a);
    context(&mut a, Some(block(3)));
    assert_eq!(head(&a).as_deref(), Some("▸ memory (3 notes)"));
    let second = send(&mut a, "second");
    assert_eq!(second.memory, Some(block(3).text));
    assert_eq!(second.system, "system", "the block is not in the prompt");
    assert!(
        second
            .messages
            .iter()
            .all(|m| !m.content.contains("project-memory")),
        "nor in the history"
    );
}

#[test]
fn an_empty_context_adds_no_block_and_no_line() {
    let (mut a, _) = opened(true);
    project(&mut a);
    context(&mut a, None);
    assert_eq!(head(&a), None);
    assert_eq!(send(&mut a, "hi").memory, None);
    assert!(systems(&a).is_empty());
}

#[test]
fn a_failed_fetch_adds_no_block_and_the_chat_streams() {
    let (mut a, _) = opened(true);
    project(&mut a);
    unavailable(&mut a, Reason::NoAnswer);
    assert_eq!(head(&a), None);
    assert_eq!(systems(&a), ["memory unavailable: engram did not answer"]);
    assert_eq!(send(&mut a, "hi").memory, None);
}

#[test]
fn a_context_nobody_asked_for_is_ignored() {
    let (mut a, _) = opened(true);
    context(&mut a, Some(block(3)));
    assert_eq!(head(&a), None, "no project yet, no fetch was sent");
    assert_eq!(send(&mut a, "hi").memory, None);
}

#[test]
fn a_second_context_reply_is_ignored() {
    let (mut a, _) = opened(true);
    project(&mut a);
    context(&mut a, Some(block(3)));
    context(&mut a, Some(block(7)));
    assert_eq!(head(&a).as_deref(), Some("▸ memory (3 notes)"));
}

#[test]
fn each_reason_gives_its_line_once_per_view() {
    let reasons = [
        (Reason::NotRunning, "engram is not running"),
        (
            Reason::OldServer { port: 7437 },
            "old engram server on :7437, restart it with 3.x",
        ),
        (Reason::Foreign { port: 9000 }, "port 9000 is not engram"),
        (Reason::NoAnswer, "engram did not answer"),
        (Reason::NoBinary, "engram not found on PATH"),
        (Reason::OldBinary, "engram on PATH is 1.x; upgrade it"),
    ];
    for (reason, text) in reasons {
        let (mut a, _) = opened(true);
        unavailable(&mut a, reason.clone());
        assert_eq!(systems(&a), [format!("memory unavailable: {text}")]);
        // A memory key looks again and fails again: still one line.
        send(&mut a, "q");
        finish_turn(&mut a);
        assert_eq!(memory_cmds(&press(&mut a, "n")), [resolve()], "{text}");
        unavailable(&mut a, reason);
        assert_eq!(systems(&a).len(), 1, "{text}");
        assert_eq!(head(&a), None);
    }
}

#[test]
fn a_chat_still_streams_with_engram_down() {
    let (mut a, _) = opened(true);
    unavailable(&mut a, Reason::NotRunning);
    let request = send(&mut a, "hi");
    assert_eq!(request.memory, None);
    assert_eq!(request.messages.len(), 1);
}

#[test]
fn a_directory_that_is_not_a_git_project_says_so_once() {
    for error in [ProjectError::NotGit, ProjectError::NotGit] {
        let (mut a, _) = opened(true);
        a.update(AppEvent::Memory(MemReply::Project {
            pane: chat_id(),
            result: Err(error),
        }));
        assert_eq!(systems(&a), ["memory needs a git project"]);
    }
}

#[test]
fn a_reply_the_app_cannot_read_gets_the_generic_reason_line() {
    let (mut a, _) = opened(true);
    a.update(AppEvent::Memory(MemReply::Project {
        pane: chat_id(),
        result: Err(ProjectError::Invalid),
    }));
    assert_eq!(
        systems(&a),
        ["memory unavailable: engram sent an unexpected reply"]
    );
}

#[test]
fn a_chat_without_a_directory_is_not_a_git_project_and_asks_for_nothing() {
    let mut a = launched(80, 25).with_ai(ai(true));
    a.open_chat_tab(None);
    let id = a.tab().focus;
    assert_eq!(a.begin_memory(id), vec![]);
    let chat = a
        .panes
        .get(&id)
        .and_then(PaneState::chat_state)
        .expect("chat");
    let lines: Vec<_> = chat
        .entries()
        .iter()
        .filter_map(|e| match e {
            Entry::System(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(lines, ["memory needs a git project"]);
}

#[test]
fn only_health_project_and_context_are_asked_while_chatting() {
    let (mut a, mut effects) = opened(true);
    effects.extend(project(&mut a));
    effects.extend(context(&mut a, Some(block(3))));
    for text in ["one", "two", "three"] {
        type_text(&mut a, text);
        effects.extend(a.update(plain_key(KeyCode::Enter)));
        finish_turn(&mut a);
    }
    assert_eq!(memory_cmds(&effects), [resolve(), context_cmd()]);
}

#[test]
fn a_reply_for_a_pane_that_is_gone_changes_nothing() {
    let (mut a, _) = opened(true);
    let gone = AppEvent::Memory(MemReply::Context {
        pane: PaneId::for_test(9),
        block: Some(block(3)),
    });
    assert_eq!(a.update(gone), vec![]);
    assert_eq!(head(&a), None);
}
