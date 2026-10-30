//! The folder picker drawn inside the chat pane (chat-folder B2).

use ratatui::{
    buffer::Buffer,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
    layout::Rect,
    widgets::Widget,
};

use super::*;
use crate::core::ai::chat::{
    folder::Refusal,
    folder_picker::{Row, Source},
};

const ID: u64 = 7;

fn theme() -> LazaroboxTheme {
    LazaroboxTheme::default()
}

fn row(path: &str, source: Source) -> Row {
    Row {
        path: path.into(),
        source,
    }
}

/// A chat with its picker open over a draft in the composer.
fn picking() -> ChatState {
    let mut chat = ChatState::new(Some("/h".into()));
    chat.composer_mut().paste("a draft in the composer");
    chat.open_folder_picker(ID, "/h".into());
    chat
}

fn with_rows(mut chat: ChatState, rows: Vec<Row>) -> ChatState {
    chat.folder_picker_mut().unwrap().set_rows(ID, rows);
    chat
}

fn three() -> ChatState {
    with_rows(
        picking(),
        vec![
            row("/w/client-a", Source::LastUsed),
            row("/w/lazarobox", Source::PaneRoot),
            row("/w/notes", Source::Recent),
        ],
    )
}

fn press(chat: &mut ChatState, code: KeyCode) {
    chat.folder_picker_mut()
        .unwrap()
        .on_key(&KeyEvent::new(code, KeyModifiers::NONE), false);
}

fn draw(chat: &ChatState, width: u16, height: u16) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    ChatView::new(chat, &theme()).render(area, &mut buf);
    buf
}

fn text(buf: &Buffer) -> String {
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn snapshot_the_picker_with_three_candidates() {
    insta::assert_snapshot!(text(&draw(&three(), 60, 10)));
}

#[test]
fn snapshot_the_picker_with_the_second_row_selected_and_a_typed_path() {
    let mut chat = three();
    press(&mut chat, KeyCode::Down);
    chat.folder_picker_mut().unwrap().paste("~/co");
    insta::assert_snapshot!(text(&draw(&chat, 60, 10)));
}

#[test]
fn snapshot_the_picker_without_candidates_shows_the_hint() {
    insta::assert_snapshot!(text(&draw(&with_rows(picking(), Vec::new()), 60, 10)));
}

#[test]
fn snapshot_the_picker_while_gathering() {
    insta::assert_snapshot!(text(&draw(&picking(), 60, 10)));
}

#[test]
fn snapshot_the_picker_with_a_refusal() {
    let mut chat = three();
    chat.folder_picker_mut().unwrap().paste("/nope");
    press(&mut chat, KeyCode::Enter);
    chat.folder_picker_mut()
        .unwrap()
        .on_probed(ID, Err(Refusal::NotFound));
    insta::assert_snapshot!(text(&draw(&chat, 60, 10)));
}

#[test]
fn the_picker_never_panics_and_the_cursor_stays_inside() {
    let mut chat = three();
    chat.folder_picker_mut().unwrap().paste(&"x".repeat(300));
    let th = theme();
    for width in 0..=14u16 {
        for height in 0..=8u16 {
            let area = Rect::new(2, 3, width, height);
            let mut buf = Buffer::empty(area);
            let view = ChatView::new(&chat, &th);
            if let Some(pos) = view.cursor(area) {
                assert!(area.contains(pos), "{pos:?} outside {area:?}");
            }
            view.render(area, &mut buf);
        }
    }
}

#[test]
fn a_hostile_folder_name_draws_no_control_or_bidi_character() {
    let chat = with_rows(
        picking(),
        vec![row("/w/\u{1b}[31mred\u{202e}evil\u{1b}[0m", Source::Recent)],
    );
    let buf = draw(&chat, 60, 10);
    for y in 0..10 {
        for x in 0..60 {
            let symbol = buf[(x, y)].symbol();
            assert!(
                symbol.chars().all(|c| !c.is_control() && c != '\u{202e}'),
                "{symbol:?} at {x},{y}"
            );
        }
    }
}

#[test]
fn the_composer_draft_is_not_drawn_and_is_kept() {
    let chat = three();
    assert!(!text(&draw(&chat, 60, 10)).contains("a draft"));
    assert_eq!(chat.composer().text(), "a draft in the composer");
}

#[test]
fn a_long_path_keeps_its_source_in_view() {
    let long = format!("/w/{}/project", "deep/".repeat(20));
    let chat = with_rows(picking(), vec![row(&long, Source::PaneRoot)]);
    let drawn = text(&draw(&chat, 30, 10));
    assert!(drawn.contains("pane"), "{drawn}");
}
