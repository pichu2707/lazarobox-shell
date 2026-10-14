//! The conversation list drawn inside the chat pane (ai-chat C9).

use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    widgets::Widget,
};

use super::*;
use crate::core::ai::chat::{list::ListView, store::ConvRow};

fn theme() -> LazaroboxTheme {
    LazaroboxTheme::default()
}

fn row(id: i64, updated_at: i64, tag: &str, first: &str) -> ConvRow {
    ConvRow {
        id,
        updated_at,
        profile: "programmer".into(),
        tag: Some(tag.into()),
        first: first.into(),
    }
}

/// A chat opened in `proj` whose list holds two `proj` rows and one `other`.
fn listing() -> ChatState {
    let mut chat = ChatState::new(None);
    chat.set_tag(Some("proj".into()));
    chat.open_list();
    chat.list_mut().unwrap().set_rows(vec![
        row(3, 1_700_000_000, "proj", "How do I read a file in chunks?"),
        row(2, 1_690_000_000, "other", "Explain the borrow checker"),
        row(1, 1_680_000_000, "proj", "Write a\nretry loop"),
    ]);
    chat
}

fn draw(chat: &ChatState, width: u16, height: u16) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    ChatView::new(chat, &theme()).render(area, &mut buf);
    buf
}

fn rows(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

fn text(buf: &Buffer) -> String {
    rows(buf).join("\n")
}

fn press(chat: &mut ChatState, c: char) {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    chat.list_mut()
        .unwrap()
        .on_key(&KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), false);
}

#[test]
fn the_list_takes_the_place_of_the_conversation() {
    let mut chat = listing();
    chat.composer_mut().paste("my draft");
    let shown = text(&draw(&chat, 60, 12));
    assert!(shown.contains("Conversations · proj"), "{shown}");
    assert!(shown.contains("How do I read a file"), "{shown}");
    assert!(!shown.contains("my draft"), "the composer is not drawn");
    assert!(
        !shown.contains("borrow checker"),
        "other projects are hidden"
    );
}

#[test]
fn the_cursor_row_carries_the_marker() {
    let mut chat = listing();
    let before = rows(&draw(&chat, 60, 12));
    assert!(before[1].starts_with("▸ 2023-11-14"), "{before:?}");
    assert!(before[2].starts_with("  2023-03-"), "{before:?}");
    press(&mut chat, 'j');
    let after = rows(&draw(&chat, 60, 12));
    assert!(after[1].starts_with("  2023-11-14"), "{after:?}");
    assert!(after[2].starts_with("▸ "), "{after:?}");
}

#[test]
fn the_list_hides_the_real_cursor() {
    let chat = listing();
    let theme = theme();
    let view = ChatView::new(&chat, &theme);
    assert_eq!(view.cursor(Rect::new(0, 0, 60, 12)), None::<Position>);
}

#[test]
fn a_long_first_message_is_one_line_inside_the_pane() {
    let mut chat = ChatState::new(None);
    chat.open_list();
    chat.list_mut().unwrap().set_rows(vec![row(
        1,
        1_700_000_000,
        "proj",
        &format!("{}\n{}", "x".repeat(150), "y".repeat(150)),
    )]);
    // Drawn in a buffer wider than the pane: nothing leaks past column 40.
    let area = Rect::new(0, 0, 60, 8);
    let mut buf = Buffer::empty(area);
    ChatView::new(&chat, &theme()).render(Rect::new(0, 0, 40, 8), &mut buf);
    for y in 0..8 {
        for x in 40..60 {
            assert_eq!(buf[(x, y)].symbol(), " ", "({x},{y}) is outside the pane");
        }
    }
    let shown = rows(&buf);
    assert!(shown[1].ends_with('…'), "{shown:?}");
    assert_eq!(shown.iter().filter(|r| r.contains('…')).count(), 1);
}

#[test]
fn a_cursor_below_the_window_scrolls_the_rows() {
    let mut chat = ChatState::new(None);
    chat.open_list();
    let many: Vec<ConvRow> = (0..20)
        .map(|n| {
            row(
                100 - n,
                1_700_000_000 - n * 86_400,
                "p",
                &format!("message {n}"),
            )
        })
        .collect();
    chat.list_mut().unwrap().set_rows(many);
    for _ in 0..15 {
        press(&mut chat, 'j');
    }
    let shown = text(&draw(&chat, 50, 8));
    assert!(shown.contains("▸ "), "{shown}");
    assert!(
        shown.contains("message 15"),
        "the cursor row is in view: {shown}"
    );
    assert!(
        !shown.contains("message 0\n"),
        "the top scrolled away: {shown}"
    );
}

#[test]
fn the_delete_prompt_takes_the_last_row() {
    let mut chat = listing();
    press(&mut chat, 'd');
    let shown = rows(&draw(&chat, 60, 12));
    assert_eq!(shown[11], "Delete conversation? (y/n)");
}

#[test]
fn a_line_takes_the_last_row_until_the_next_key() {
    let mut chat = listing();
    chat.list_mut()
        .unwrap()
        .set_line("already open in another pane");
    let shown = rows(&draw(&chat, 60, 12));
    assert_eq!(shown[11], "already open in another pane");
    press(&mut chat, 'j');
    let shown = rows(&draw(&chat, 60, 12));
    assert!(
        shown[11].contains("open"),
        "the key hints are back: {shown:?}"
    );
    assert!(!shown[11].contains("already"));
}

#[test]
fn the_hints_list_the_keys_and_all_projects_only_with_a_project() {
    let chat = listing();
    let shown = rows(&draw(&chat, 70, 12));
    assert!(shown[11].contains("a all projects"), "{shown:?}");
    let mut none = ChatState::new(None);
    none.open_list();
    none.list_mut().unwrap().set_rows(Vec::new());
    let shown = rows(&draw(&none, 70, 12));
    assert!(!shown[11].contains("all projects"), "{shown:?}");
}

#[test]
fn snapshot_the_list_of_a_project() {
    insta::assert_snapshot!(text(&draw(&listing(), 60, 10)));
}

#[test]
fn snapshot_the_list_of_all_projects() {
    let mut chat = listing();
    press(&mut chat, 'a');
    press(&mut chat, 'j');
    insta::assert_snapshot!(text(&draw(&chat, 60, 10)));
}

#[test]
fn snapshot_an_empty_project() {
    let mut chat = ChatState::new(None);
    chat.set_tag(Some("proj".into()));
    chat.open_list();
    chat.list_mut()
        .unwrap()
        .set_rows(vec![row(2, 1_690_000_000, "other", "elsewhere")]);
    insta::assert_snapshot!(text(&draw(&chat, 60, 8)));
}

#[test]
fn snapshot_an_empty_store() {
    let mut chat = ChatState::new(None);
    chat.open_list();
    chat.list_mut().unwrap().set_rows(Vec::new());
    insta::assert_snapshot!(text(&draw(&chat, 60, 8)));
}

#[test]
fn snapshot_a_long_message_in_40_columns() {
    let mut chat = ChatState::new(None);
    chat.open_list();
    chat.list_mut().unwrap().set_rows(vec![row(
        1,
        1_700_000_000,
        "proj",
        &"a long first message with many words\n".repeat(8),
    )]);
    insta::assert_snapshot!(text(&draw(&chat, 40, 8)));
}

#[test]
fn snapshot_the_delete_prompt() {
    let mut chat = listing();
    press(&mut chat, 'd');
    insta::assert_snapshot!(text(&draw(&chat, 60, 10)));
}

#[test]
fn snapshot_the_list_in_a_tiny_pane() {
    let chat = listing();
    insta::assert_snapshot!(text(&draw(&chat, 14, 4)));
    insta::assert_snapshot!(text(&draw(&chat, 3, 2)));
    insta::assert_snapshot!(text(&draw(&chat, 1, 1)));
}

#[test]
fn a_list_view_never_panics_at_any_size() {
    let chat = listing();
    for w in 0..12 {
        for h in 0..6 {
            draw(&chat, w, h);
        }
    }
}

#[test]
fn a_list_is_listing_state_only_while_open() {
    let mut chat = listing();
    assert!(chat.list().is_some());
    chat.close_list();
    assert!(chat.list().is_none());
    let _ = ListView::new(None);
}
