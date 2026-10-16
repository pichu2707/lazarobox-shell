//! Run in pane (pane-commands, modal-input): `Prefix e` opens a confirm that
//! types the last shell block of an answer into a shell pane. It never
//! sends Enter.

use super::chat_live::{detection, ollama};
use super::*;
use crate::core::ai::chat::{
    approval::ApprovalAsk,
    engine::{Finish, StreamEvent},
};

pub(super) const BLOCK: &str = "Try this:\n```bash\ncd ~/dev/curso-rust\n```";

/// Presses Enter in a chat with text and returns the turn it sent.
pub(super) fn enter_turn(a: &mut App) -> u64 {
    let effects = a.update(AppEvent::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));
    match effects.as_slice() {
        [Effect::ChatSend { turn, .. }] => *turn,
        other => panic!("expected one ChatSend, got {other:?}"),
    }
}

/// Gives chat pane `chat` a finished answer. The app must be built with
/// `ollama()` as its AI config.
pub(super) fn answer(a: &mut App, chat: PaneId, text: &str) {
    a.update(AppEvent::EnginesDetected(detection(&["llama3:8b"])));
    a.update(AppEvent::PromptReady(
        chat,
        crate::core::ai::chat::prompts::Resolved {
            text: "system".to_owned(),
            notices: Vec::new(),
        },
    ));
    a.update(key('q'));
    let turn = enter_turn(a);
    a.update(AppEvent::Chat(
        chat,
        turn,
        StreamEvent::Delta(text.to_owned()),
    ));
    a.update(AppEvent::Chat(chat, turn, StreamEvent::Done(Finish::Stop)));
    a.take_dirty();
}

/// A shell (pane 1) and a focused chat (pane 2) that answered `text`.
pub(super) fn one_shell_and_a_chat(text: &str) -> App {
    let mut a = launched(80, 25).with_ai(ollama());
    press(&mut a, "c");
    assert_eq!(a.focused(), id2());
    answer(&mut a, id2(), text);
    a
}

pub(super) fn chat(a: &App, id: PaneId) -> &ChatState {
    match a.pane_view(id) {
        Some(PaneView::Chat(chat)) => chat,
        _ => panic!("pane {id:?} is not a chat"),
    }
}

fn pending(a: &App, id: PaneId) -> Option<ApprovalAsk> {
    chat(a, id).approvals().pending().map(|p| p.ask().clone())
}

fn run(command: &str) -> ApprovalAsk {
    ApprovalAsk::Run {
        command: command.to_owned(),
    }
}

// 1b.1: the leaf and its refusals ------------------------------------------

#[test]
fn prefix_e_opens_a_pending_confirm_and_writes_nothing() {
    let mut a = one_shell_and_a_chat(BLOCK);
    let effects = press(&mut a, "e");
    assert_eq!(effects, vec![]);
    assert_eq!(a.input(), InputMode::Terminal);
    assert_eq!(pending(&a, id2()), Some(run("cd ~/dev/curso-rust")));
    assert_eq!(chat(&a, id2()).run_target(), Some(PaneId::FIRST));
}

#[test]
fn prefix_e_on_a_shell_pane_only_shows_a_notice() {
    let mut a = launched(80, 25).with_ai(ollama());
    let effects = press(&mut a, "e");
    assert_eq!(effects, vec![]);
    assert!(
        a.notice().is_some_and(|n| n.contains("chat")),
        "{:?}",
        a.notice()
    );
}

#[test]
fn prefix_e_on_the_conversation_list_only_shows_a_notice() {
    let mut a = one_shell_and_a_chat(BLOCK);
    a.focused_state_mut().chat_mut().unwrap().open_list();
    let effects = press(&mut a, "e");
    assert_eq!(effects, vec![]);
    assert!(a.notice().is_some());
    assert_eq!(pending(&a, id2()), None);
}

#[test]
fn prefix_e_with_tools_off_only_shows_a_notice() {
    let mut ai = ollama();
    ai.set_tools_enabled(false);
    let mut a = launched(80, 25).with_ai(ai);
    press(&mut a, "c");
    answer(&mut a, id2(), BLOCK);
    let effects = press(&mut a, "e");
    assert_eq!(effects, vec![]);
    assert_eq!(
        a.notice(),
        Some("Tools are off: nothing can run from the chat")
    );
    assert_eq!(pending(&a, id2()), None);
}

#[test]
fn no_runnable_block_gives_a_notice_and_no_confirm() {
    for text in [
        "```rust\nfn main() {}\n```",
        "```\nls\n```",
        "```bash\nls",
        "no code at all",
    ] {
        let mut a = one_shell_and_a_chat(text);
        assert_eq!(press(&mut a, "e"), vec![], "{text:?}");
        assert_eq!(
            a.notice(),
            Some("No runnable command in the last answer"),
            "{text:?}"
        );
        assert_eq!(pending(&a, id2()), None, "{text:?}");
    }
}

#[test]
fn a_streaming_message_offers_no_confirm() {
    let mut a = one_shell_and_a_chat("hello");
    a.update(key('x'));
    let turn = enter_turn(&mut a);
    a.update(AppEvent::Chat(
        id2(),
        turn,
        StreamEvent::Delta(BLOCK.to_owned()),
    ));
    assert_eq!(press(&mut a, "e"), vec![]);
    assert_eq!(pending(&a, id2()), None);
}

#[test]
fn a_command_that_does_not_validate_is_refused_with_its_reason() {
    let mut a = one_shell_and_a_chat("```bash\nls \u{1b}[31m\n```");
    assert_eq!(press(&mut a, "e"), vec![]);
    assert_eq!(a.notice(), Some("Command refused: escape character"));
    assert_eq!(pending(&a, id2()), None);
}

#[test]
fn the_confirm_does_not_depend_on_the_engine() {
    for engine in ["ollama", "claude", "codex", "gemini"] {
        let mut ai = ollama();
        ai.set(
            crate::core::ai::config::Task::Chat,
            crate::core::ai::config::Assignment {
                engine: Some(engine.into()),
                model: None,
            },
        );
        // The answer is injected as events, so no engine is spoken to.
        let mut a = launched(80, 25).with_ai(ai);
        press(&mut a, "c");
        let id = id2();
        a.focused_state_mut().chat_mut().unwrap();
        let turn = {
            let chat = a.focused_state_mut().chat_mut().unwrap();
            chat.composer_mut().paste("q");
            let ready = crate::core::ai::chat::engine::Availability::Ready {
                engine: engine.into(),
                model: None,
            };
            match chat.send("q", &ready, "system") {
                crate::core::ai::chat::SendOutcome::Sent { turn, .. } => turn,
                other => panic!("{other:?}"),
            }
        };
        a.update(AppEvent::Chat(
            id,
            turn,
            StreamEvent::Delta(BLOCK.to_owned()),
        ));
        a.update(AppEvent::Chat(id, turn, StreamEvent::Done(Finish::Stop)));
        press(&mut a, "e");
        assert_eq!(
            pending(&a, id),
            Some(run("cd ~/dev/curso-rust")),
            "{engine}"
        );
    }
}

// 1b.2: the target pane ------------------------------------------------------

/// Pane numbers as the prompt names them.
fn label(a: &App, id: PaneId) -> String {
    a.approval_prompt(id).expect("a pending confirm").target
}

#[test]
fn the_target_is_the_last_focused_shell_not_the_first() {
    let mut a = launched(80, 25).with_ai(ollama());
    press(&mut a, "wv"); // shell pane 2, focused
    let second = a.focused();
    press(&mut a, "c"); // chat opened from pane 2
    let chat_id = a.focused();
    answer(&mut a, chat_id, BLOCK);
    press(&mut a, "e");
    assert_eq!(chat(&a, chat_id).run_target(), Some(second));
}

#[test]
fn a_chat_is_never_a_target_and_a_tab_without_shell_refuses() {
    let mut a = one_shell_and_a_chat(BLOCK);
    a.update(AppEvent::Pty(PaneId::FIRST, PtyEvent::Exited));
    assert_eq!(a.focused(), id2());
    assert_eq!(press(&mut a, "e"), vec![]);
    assert_eq!(a.notice(), Some("No shell pane in this tab"));
    assert_eq!(pending(&a, id2()), None);
}

#[test]
fn a_shell_in_another_tab_is_not_a_target() {
    let mut a = one_shell_and_a_chat(BLOCK);
    press(&mut a, "tn"); // tab 2 with its own shell
    press(&mut a, "b1"); // back to tab 1
    assert_eq!(a.focused(), id2());
    a.update(AppEvent::Pty(PaneId::FIRST, PtyEvent::Exited));
    assert_eq!(press(&mut a, "e"), vec![]);
    assert_eq!(a.notice(), Some("No shell pane in this tab"));
    assert_eq!(pending(&a, id2()), None);
}

#[test]
fn p_cycles_the_target_with_wrap_and_writes_nothing() {
    let mut a = launched(80, 25).with_ai(ollama());
    press(&mut a, "wv");
    press(&mut a, "wh");
    press(&mut a, "c");
    let chat_id = a.focused();
    answer(&mut a, chat_id, BLOCK);
    press(&mut a, "e");
    let mut seen = vec![chat(&a, chat_id).run_target().unwrap()];
    let mut labels = vec![label(&a, chat_id)];
    for _ in 0..3 {
        assert_eq!(a.update(key('p')), vec![]);
        seen.push(chat(&a, chat_id).run_target().unwrap());
        labels.push(label(&a, chat_id));
    }
    assert_eq!(seen[0], seen[3], "three presses wrap around: {seen:?}");
    assert_eq!(labels[0], labels[3]);
    assert!(seen[0] != seen[1] && seen[1] != seen[2] && seen[0] != seen[2]);
    assert!(
        seen.iter().all(|t| *t != chat_id),
        "a chat is never a target"
    );
    assert!(pending(&a, chat_id).is_some(), "p never answers");
}

#[test]
fn the_label_names_the_pane_number_and_a_tilde_cwd() {
    let mut a = launched(80, 25)
        .with_env(
            Some("/bin/zsh"),
            Some("/home/ana".into()),
            "/home/ana/dev".into(),
        )
        .with_ai(ollama());
    press(&mut a, "c");
    answer(&mut a, id2(), BLOCK);
    press(&mut a, "e");
    assert_eq!(label(&a, id2()), "run in pane 1 (~/dev)");
}

#[test]
fn the_label_has_no_control_characters() {
    let mut a = launched(80, 25)
        .with_env(Some("/bin/zsh"), None, "/launch".into())
        .with_ai(ollama());
    a.update(AppEvent::Cwd(
        PaneId::FIRST,
        "/tmp/\u{1b}[31mred\u{202e}x".into(),
    ));
    press(&mut a, "c");
    answer(&mut a, id2(), BLOCK);
    press(&mut a, "e");
    let text = label(&a, id2());
    assert!(text.starts_with("run in pane 1 (/tmp/"), "{text:?}");
    assert!(
        !text.chars().any(|c| c.is_control() || c == '\u{202e}'),
        "{text:?}"
    );
}
