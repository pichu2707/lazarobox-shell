//! Memory in a chat view (engram-memory M6): the memory line and its key, the
//! context flow, the degradation lines.

use super::*;
use crate::core::ai::{chat::Entry, memory::context::ContextBlock};

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
