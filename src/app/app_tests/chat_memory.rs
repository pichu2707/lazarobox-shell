//! Memory in a chat view (engram-memory M6): the memory line and its key, the
//! context flow, the degradation lines.

use super::chat_notes::{
    CWD, PROJECT, ai, ctrl, pause, ready, saved_reply, systems, type_text, with_answer,
};
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
        offer::Offer,
        port::{MemCmd, MemReply, Reason, ServerOutcome},
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

// 10.9: no wait without a bound ---------------------------------------------

const NO_ANSWER_LINE: &str = "memory unavailable: engram did not answer";

fn tick(a: &mut App, ms: u64) -> Vec<Effect> {
    at(a, ms, AppEvent::MemoryTick)
}

/// A chat opened at t = 0 whose project request is never answered.
fn waiting_for_the_project() -> App {
    let mut a = timed(launched(80, 25).with_ai(ai(true)));
    at(&mut a, 0, cwd_event(CWD));
    at(&mut a, 0, ctrl_space());
    let effects = at(&mut a, 0, key('c'));
    assert_eq!(memory_cmds(&effects), [resolve()]);
    a
}

#[test]
fn a_view_waiting_for_its_project_gives_up_after_five_seconds() {
    let mut a = waiting_for_the_project();
    assert!(a.memory_waiting());
    assert_eq!(tick(&mut a, 4_999), vec![]);
    assert!(systems(&a).is_empty(), "not yet");
    assert!(a.memory_waiting());
    assert_eq!(tick(&mut a, 5_000), vec![]);
    assert_eq!(systems(&a), [NO_ANSWER_LINE]);
    assert!(!a.memory_waiting(), "the timer can be disarmed");
    // Once: later ticks add nothing.
    tick(&mut a, 60_000);
    assert_eq!(systems(&a), [NO_ANSWER_LINE]);
}

#[test]
fn after_giving_up_the_chat_streams_and_the_late_project_reply_is_ignored() {
    let mut a = waiting_for_the_project();
    tick(&mut a, 5_000);
    let late = a.update(AppEvent::Memory(MemReply::Project {
        pane: chat_id(),
        result: Ok(PROJECT.into()),
    }));
    assert_eq!(memory_cmds(&late), [], "no context is asked for any more");
    assert_eq!(
        chat_of(&a).readiness(),
        &Readiness::Unavailable("engram did not answer".into())
    );
    a.update(AppEvent::PromptReady(
        chat_id(),
        Resolved {
            text: "system".into(),
            notices: Vec::new(),
        },
    ));
    a.update(AppEvent::EnginesDetected(Detection {
        engines: vec![EngineState {
            id: "ollama",
            status: Status::Available,
            models: vec!["llama3:8b".to_owned()],
            version: None,
        }],
    }));
    assert_eq!(send(&mut a, "hi").memory, None);
}

#[test]
fn a_lost_context_reply_gives_up_after_five_seconds_from_its_request() {
    let mut a = waiting_for_the_project();
    at(
        &mut a,
        1_000,
        AppEvent::Memory(MemReply::Project {
            pane: chat_id(),
            result: Ok(PROJECT.into()),
        }),
    );
    assert!(a.memory_waiting());
    tick(&mut a, 5_999);
    assert!(systems(&a).is_empty());
    tick(&mut a, 6_000);
    assert_eq!(systems(&a), [NO_ANSWER_LINE]);
    assert!(!a.memory_waiting());
    // The reply that finally comes finds nobody waiting.
    at(
        &mut a,
        7_000,
        AppEvent::Memory(MemReply::Context {
            pane: chat_id(),
            block: Some(block(3)),
        }),
    );
    assert_eq!(head(&a), None);
}

#[test]
fn an_answer_in_time_means_nothing_to_give_up() {
    let mut a = waiting_for_the_project();
    at(
        &mut a,
        100,
        AppEvent::Memory(MemReply::Project {
            pane: chat_id(),
            result: Ok(PROJECT.into()),
        }),
    );
    at(
        &mut a,
        200,
        AppEvent::Memory(MemReply::Context {
            pane: chat_id(),
            block: Some(block(3)),
        }),
    );
    assert!(!a.memory_waiting());
    a.take_dirty();
    tick(&mut a, 60_000);
    assert!(systems(&a).is_empty());
    assert!(
        !a.take_dirty(),
        "a tick with nothing to give up on redraws nothing"
    );
}

#[test]
fn a_save_key_waiting_for_its_project_gives_up_with_the_not_ready_notice() {
    let mut a = timed(with_answer(false, "use Axum"));
    a.ai.memory = ai(true).memory;
    at(&mut a, 0, ctrl_space());
    let effects = at(&mut a, 0, key('n'));
    assert_eq!(memory_cmds(&effects), [resolve()]);
    assert_eq!(a.notice(), Some("Checking memory…"));
    tick(&mut a, 5_000);
    assert_eq!(
        a.notice(),
        Some("Memory is not ready: engram did not answer")
    );
    assert!(chat_of(&a).note_editor().is_none());
    assert_eq!(systems(&a), [NO_ANSWER_LINE]);
}

#[test]
fn a_note_editor_waiting_for_its_reply_stops_waiting_and_keeps_the_text() {
    let mut a = ready("use Axum");
    let mut a = timed({
        type_text(&mut a, "Use Axum");
        a
    });
    let sent = at(&mut a, 0, ctrl('s'));
    assert!(matches!(
        sent.as_slice(),
        [Effect::Memory(MemCmd::Save { .. })]
    ));
    assert!(chat_of(&a).note_editor().unwrap().is_saving());
    assert!(a.memory_waiting());
    let (token, _) = super::chat_notes::oldest_save(&a);
    tick(&mut a, 4_999);
    assert!(chat_of(&a).note_editor().unwrap().is_saving());
    tick(&mut a, 5_000);
    let editor = chat_of(&a).note_editor().expect("the editor stays open");
    assert!(!editor.is_saving(), "free to save again");
    assert_eq!(editor.title(), "Use Axum");
    assert_eq!(editor.notice(), Some("engram did not answer"));
    assert!(!a.memory_waiting());
    // The reply that was only late: the note exists in engram, so it is
    // recorded and linked; the editor the user may have reopened is untouched.
    let late = AppEvent::Memory(MemReply::Saved {
        pane: chat_id(),
        id: token,
        project: PROJECT.into(),
        session: "lazarobox-0123456789abcdef0123456789abcdef".into(),
        observation_id: 7,
        topic_key: "chat/decision/use-axum".into(),
        title: "Use Axum".into(),
    });
    at(&mut a, 8_000, late);
    assert!(
        chat_of(&a).note_editor().is_some(),
        "not closed by a stale reply"
    );
    assert!(systems(&a).contains(&"saved note decision: Use Axum".to_owned()));
}

#[test]
fn a_save_given_up_on_after_esc_is_a_system_line() {
    let mut a = ready("use Axum");
    let mut a = timed({
        type_text(&mut a, "Use Axum");
        a
    });
    at(&mut a, 0, ctrl('s'));
    at(&mut a, 100, plain_key(KeyCode::Esc));
    assert!(chat_of(&a).note_editor().is_none());
    tick(&mut a, 5_000);
    assert!(systems(&a).contains(&"note not saved: engram did not answer".to_owned()));
}

#[test]
fn a_lost_save_reply_does_not_stop_a_retry() {
    let mut a = ready("use Axum");
    let mut a = timed({
        type_text(&mut a, "Use Axum");
        a
    });
    at(&mut a, 0, ctrl('s'));
    tick(&mut a, 5_000);
    pause(&mut a);
    let again = at(&mut a, 6_000, ctrl('s'));
    assert!(matches!(
        again.as_slice(),
        [Effect::Memory(MemCmd::Save { .. })]
    ));
    // This one is answered in time.
    let reply = saved_reply(
        &a,
        "lazarobox-0123456789abcdef0123456789abcdef",
        7,
        "chat/decision/use-axum",
        "Use Axum",
    );
    at(&mut a, 6_100, reply);
    assert!(
        chat_of(&a).note_editor().is_none(),
        "closed by its own reply"
    );
    tick(&mut a, 60_000);
    assert!(!systems(&a).iter().any(|l| l.starts_with("note not saved")));
}

// 10.6: the `engram serve` offer -------------------------------------------------

const OFFER_BIN: &str = "/usr/local/bin/engram";
const OFFER_PROMPT: &str =
    "engram is not running. Start \"/usr/local/bin/engram serve 7437\"? y start · n not now";

fn offer() -> AppEvent {
    AppEvent::Memory(MemReply::ServerOffer(Offer {
        path: OFFER_BIN.into(),
        port: 7437,
    }))
}

/// A chat whose engram is down, with the worker's one offer received.
fn offered() -> App {
    let (mut a, _) = opened(true);
    unavailable(&mut a, Reason::NotRunning);
    assert_eq!(a.update(offer()), vec![]);
    a
}

fn start_server() -> Vec<Effect> {
    vec![Effect::Memory(MemCmd::StartServer)]
}

fn unmodified(c: char) -> AppEvent {
    AppEvent::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
}

#[test]
fn an_offer_is_shown_in_the_chat_view_with_the_absolute_path() {
    let a = offered();
    assert_eq!(a.chat_guidance(chat_id()).as_deref(), Some(OFFER_PROMPT));
}

#[test]
fn an_unmodified_y_starts_the_server_and_is_not_typed() {
    let mut a = offered();
    assert_eq!(a.update(unmodified('y')), start_server());
    assert_eq!(a.chat_guidance(chat_id()), None, "answered");
    assert_eq!(chat_of(&a).composer().text(), "");
}

#[test]
fn any_other_key_declines_and_is_not_typed() {
    let others = [
        unmodified('n'),
        unmodified('x'),
        AppEvent::Key(KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::SHIFT)),
        AppEvent::Key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL)),
        AppEvent::Key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::ALT)),
        plain_key(KeyCode::Esc),
        plain_key(KeyCode::Enter),
        plain_key(KeyCode::Left),
        AppEvent::Key(KeyEvent::new_with_kind(
            KeyCode::Char('y'),
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        )),
    ];
    for key in others {
        let mut a = offered();
        assert_eq!(a.update(key.clone()), vec![], "{key:?}");
        assert_eq!(a.chat_guidance(chat_id()), None, "{key:?}");
        assert_eq!(chat_of(&a).composer().text(), "", "{key:?}");
    }
}

#[test]
fn the_offer_is_made_once_per_run() {
    let mut a = offered();
    a.update(unmodified('n'));
    a.update(offer());
    assert_eq!(
        a.chat_guidance(chat_id()),
        None,
        "declined, not asked again"
    );
    // A memory key does not bring it back either.
    send(&mut a, "q");
    finish_turn(&mut a);
    press(&mut a, "n");
    assert_eq!(a.chat_guidance(chat_id()), None);
    // And after a start nothing is asked again.
    let mut a = offered();
    a.update(unmodified('y'));
    a.update(offer());
    assert_eq!(a.chat_guidance(chat_id()), None);
}

#[test]
fn the_offer_waits_while_no_chat_is_focused_and_shows_on_a_chat_open() {
    let mut a = launched(80, 25).with_ai(ai(true));
    assert_eq!(a.update(offer()), vec![]);
    assert_eq!(a.chat_guidance(chat_id()), None, "no chat view yet");
    a.update(cwd_event(CWD));
    press(&mut a, "c");
    assert_eq!(a.chat_guidance(chat_id()).as_deref(), Some(OFFER_PROMPT));
    assert_eq!(a.update(unmodified('y')), start_server());
}

#[test]
fn a_memory_key_shows_an_offer_held_while_the_chat_was_not_focused() {
    let (mut a, _) = opened(true);
    unavailable(&mut a, Reason::NotRunning);
    send(&mut a, "q");
    finish_turn(&mut a);
    press(&mut a, "wv");
    assert!(!a.focused_is_chat(), "the new pane is a shell");
    a.update(offer());
    assert_eq!(a.chat_guidance(chat_id()), None, "held");
    press(&mut a, "h");
    assert!(a.focused_is_chat());
    assert_eq!(
        a.chat_guidance(chat_id()),
        None,
        "focus alone does not show it"
    );
    press(&mut a, "n");
    assert_eq!(a.chat_guidance(chat_id()).as_deref(), Some(OFFER_PROMPT));
}

#[test]
fn an_offer_with_memory_off_is_ignored() {
    let (mut a, _) = opened(false);
    a.update(offer());
    assert_eq!(a.chat_guidance(chat_id()), None);
    assert_eq!(a.update(unmodified('y')).len(), 0);
}

#[test]
fn the_offer_is_not_a_stream_cancel_but_a_stream_still_runs_under_it() {
    let mut a = offered();
    assert_eq!(a.chat_guidance(chat_id()).as_deref(), Some(OFFER_PROMPT));
    let request = send(&mut a, "hi");
    assert_eq!(request.memory, None);
}

#[test]
fn a_started_server_is_a_system_line_and_the_down_views_look_again() {
    let mut a = offered();
    a.update(unmodified('y'));
    let effects = a.update(AppEvent::Memory(MemReply::Server(ServerOutcome::Started {
        pid: 4242,
    })));
    assert!(
        systems(&a).contains(
            &"started engram serve (pid 4242); it keeps running after lazarobox quits".to_owned()
        ),
        "{:?}",
        systems(&a)
    );
    assert_eq!(
        memory_cmds(&effects),
        [resolve()],
        "the project is asked again"
    );
    // The answer fetches the context as for any view.
    let effects = project(&mut a);
    assert_eq!(memory_cmds(&effects), [context_cmd()]);
    assert_eq!(
        systems(&a).len(),
        2,
        "no second unavailable line: {:?}",
        systems(&a)
    );
}

#[test]
fn an_adopted_server_says_nothing_but_the_views_look_again() {
    let mut a = offered();
    a.update(unmodified('y'));
    let before = systems(&a);
    let effects = a.update(AppEvent::Memory(MemReply::Server(ServerOutcome::Adopted)));
    assert_eq!(systems(&a), before);
    assert_eq!(memory_cmds(&effects), [resolve()]);
}

#[test]
fn a_failed_start_gives_its_reason_line_and_looks_nowhere() {
    let mut a = offered();
    a.update(unmodified('y'));
    let effects = a.update(AppEvent::Memory(MemReply::Server(ServerOutcome::Failed(
        Reason::StartFailed(Some("address already in use".into())),
    ))));
    assert_eq!(effects, vec![]);
    assert!(
        systems(&a).contains(
            &"memory unavailable: could not start engram: address already in use".to_owned()
        ),
        "{:?}",
        systems(&a)
    );
}

#[test]
fn a_server_outcome_with_no_chat_open_is_a_notice() {
    let mut a = launched(80, 25).with_ai(ai(true));
    a.update(AppEvent::Memory(MemReply::Server(ServerOutcome::Started {
        pid: 7,
    })));
    assert_eq!(
        a.notice(),
        Some("started engram serve (pid 7); it keeps running after lazarobox quits")
    );
}

// 10.5: a missing or old binary is the reason, not 'not running' ------------------

fn global_notice(reason: Reason) -> AppEvent {
    AppEvent::Memory(MemReply::Unavailable { pane: None, reason })
}

#[test]
fn a_missing_or_old_binary_names_itself_as_the_reason() {
    for (reason, text) in [
        (Reason::NoBinary, "engram not found on PATH"),
        (Reason::OldBinary, "engram on PATH is 1.x; upgrade it"),
    ] {
        // The worker tells the notice first, then answers the pane's command.
        let (mut a, _) = opened(true);
        assert_eq!(a.update(global_notice(reason)), vec![]);
        unavailable(&mut a, Reason::NotRunning);
        assert_eq!(systems(&a), [format!("memory unavailable: {text}")]);
        assert_eq!(
            chat_of(&a).readiness(),
            &Readiness::Unavailable(text.into()),
            "the memory keys say the same"
        );
    }
}

#[test]
fn a_view_opened_after_the_notice_gets_the_same_reason() {
    let mut a = launched(80, 25).with_ai(ai(true));
    a.update(global_notice(Reason::NoBinary));
    a.update(cwd_event(CWD));
    press(&mut a, "c");
    unavailable(&mut a, Reason::NotRunning);
    assert_eq!(
        systems(&a),
        ["memory unavailable: engram not found on PATH"]
    );
}

#[test]
fn only_the_not_running_reason_is_replaced() {
    let (mut a, _) = opened(true);
    a.update(global_notice(Reason::NoBinary));
    unavailable(&mut a, Reason::OldServer { port: 7437 });
    assert_eq!(
        systems(&a),
        ["memory unavailable: old engram server on :7437, restart it with 3.x"]
    );
}

#[test]
fn a_started_server_forgets_the_binary_notice() {
    let mut a = launched(80, 25).with_ai(ai(true));
    a.update(global_notice(Reason::NoBinary));
    a.update(AppEvent::Memory(MemReply::Server(ServerOutcome::Adopted)));
    a.update(cwd_event(CWD));
    press(&mut a, "c");
    unavailable(&mut a, Reason::NotRunning);
    assert_eq!(systems(&a), ["memory unavailable: engram is not running"]);
}

#[test]
fn a_global_notice_with_memory_off_changes_nothing() {
    let (mut a, _) = opened(false);
    assert_eq!(a.update(global_notice(Reason::NoBinary)), vec![]);
    assert!(systems(&a).is_empty());
}
