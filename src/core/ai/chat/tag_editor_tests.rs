//! The tag editor (chat-history: Edit tags): one comma-separated line, Enter
//! saves, Esc discards.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::tag_editor::*;
use super::tags::{EXTRA_MAX, Tags};

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn with(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, mods)
}

fn held(code: KeyCode) -> KeyEvent {
    KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Repeat)
}

fn typed(ed: &mut TagEditor, text: &str) {
    for c in text.chars() {
        assert_eq!(
            ed.on_key(&press(KeyCode::Char(c)), false),
            TagOutcome::Handled
        );
    }
}

fn enter(ed: &mut TagEditor) -> TagOutcome {
    ed.on_key(&press(KeyCode::Enter), false)
}

#[test]
fn it_opens_on_the_current_tags_as_one_line() {
    let ed = TagEditor::new(TagTarget::Pane, &Tags::parse("proj, sql, etl"));
    assert_eq!(ed.text(), "proj, sql, etl");
    assert_eq!(ed.target(), TagTarget::Pane);
}

#[test]
fn a_conversation_without_tags_opens_empty() {
    assert_eq!(
        TagEditor::new(TagTarget::Row(3), &Tags::default()).text(),
        ""
    );
}

#[test]
fn the_cursor_starts_at_the_end_of_the_line() {
    let mut ed = TagEditor::new(TagTarget::Pane, &Tags::parse("proj"));
    typed(&mut ed, ",a");
    assert_eq!(ed.text(), "proj,a");
}

#[test]
fn enter_saves_the_parsed_tags() {
    let mut ed = TagEditor::new(TagTarget::Pane, &Tags::parse("proj"));
    typed(&mut ed, ", sql");
    assert_eq!(enter(&mut ed), TagOutcome::Save(Tags::parse("proj, sql")));
}

#[test]
fn editing_the_project_tag_saves_the_new_one() {
    let mut ed = TagEditor::new(TagTarget::Pane, &Tags::parse("proj"));
    for _ in 0..4 {
        ed.on_key(&press(KeyCode::Backspace), false);
    }
    typed(&mut ed, "client-a");
    assert_eq!(enter(&mut ed), TagOutcome::Save(Tags::parse("client-a")));
}

#[test]
fn saving_an_emptied_line_removes_every_tag() {
    let mut ed = TagEditor::new(TagTarget::Pane, &Tags::parse("proj,a"));
    ed.on_key(&with(KeyCode::Char('u'), KeyModifiers::CONTROL), false);
    assert_eq!(enter(&mut ed), TagOutcome::Save(Tags::default()));
}

#[test]
fn the_saved_tags_follow_the_limits_of_the_spec() {
    let mut ed = TagEditor::new(TagTarget::Pane, &Tags::default());
    let many = (0..12)
        .map(|n| format!("t{n}"))
        .collect::<Vec<_>>()
        .join(",");
    typed(&mut ed, &format!("proj,sql,SQL,,{many}"));
    let TagOutcome::Save(tags) = enter(&mut ed) else {
        panic!("expected Save");
    };
    assert_eq!(tags.project.as_deref(), Some("proj"));
    assert_eq!(tags.extras.len(), EXTRA_MAX);
    assert_eq!(tags.extras[0], "sql");
}

#[test]
fn escape_discards() {
    let mut ed = TagEditor::new(TagTarget::Pane, &Tags::parse("proj"));
    typed(&mut ed, ",a");
    assert_eq!(ed.on_key(&press(KeyCode::Esc), false), TagOutcome::Discard);
}

#[test]
fn a_held_escape_or_enter_decides_nothing() {
    let mut ed = TagEditor::new(TagTarget::Pane, &Tags::parse("proj"));
    assert_eq!(ed.on_key(&held(KeyCode::Esc), true), TagOutcome::Handled);
    assert_eq!(ed.on_key(&held(KeyCode::Enter), true), TagOutcome::Handled);
    assert_eq!(ed.text(), "proj");
}

#[test]
fn a_held_character_keeps_typing() {
    let mut ed = TagEditor::new(TagTarget::Pane, &Tags::default());
    ed.on_key(&held(KeyCode::Char('x')), true);
    ed.on_key(&held(KeyCode::Char('x')), true);
    assert_eq!(ed.text(), "xx");
}

#[test]
fn the_cursor_keys_edit_in_place() {
    let mut ed = TagEditor::new(TagTarget::Pane, &Tags::parse("ac"));
    ed.on_key(&press(KeyCode::Left), false);
    typed(&mut ed, "b");
    ed.on_key(&press(KeyCode::Home), false);
    typed(&mut ed, "<");
    ed.on_key(&press(KeyCode::End), false);
    ed.on_key(&press(KeyCode::Delete), false);
    assert_eq!(ed.text(), "<abc");
}

#[test]
fn ctrl_c_and_other_chords_are_swallowed() {
    let mut ed = TagEditor::new(TagTarget::Pane, &Tags::parse("proj"));
    for key in [
        with(KeyCode::Char('c'), KeyModifiers::CONTROL),
        with(KeyCode::Char('y'), KeyModifiers::CONTROL),
        with(KeyCode::Char('j'), KeyModifiers::CONTROL),
        with(KeyCode::Enter, KeyModifiers::ALT),
        with(KeyCode::Char('x'), KeyModifiers::ALT),
        press(KeyCode::Tab),
        press(KeyCode::PageUp),
        press(KeyCode::Up),
    ] {
        assert_eq!(ed.on_key(&key, false), TagOutcome::Handled, "{key:?}");
    }
    assert_eq!(ed.text(), "proj");
}

#[test]
fn a_paste_is_one_line() {
    let mut ed = TagEditor::new(TagTarget::Pane, &Tags::default());
    ed.paste("proj,a\r\nb\tc");
    assert_eq!(ed.text(), "proj,a  b c");
}

#[test]
fn the_line_is_capped() {
    let mut ed = TagEditor::new(TagTarget::Pane, &Tags::default());
    ed.paste(&"x".repeat(LINE_CAP + 50));
    assert_eq!(ed.text().chars().count(), LINE_CAP);
}

#[test]
fn the_preview_counts_what_would_be_kept() {
    let mut ed = TagEditor::new(TagTarget::Pane, &Tags::default());
    typed(&mut ed, "proj, a, A, b,");
    assert_eq!(ed.preview(), Tags::parse("proj, a, b"));
}

// ---- the window on the line --------------------------------------------------

fn editor_with(text: &str) -> TagEditor {
    let mut ed = TagEditor::new(TagTarget::Pane, &Tags::default());
    ed.paste(text);
    ed
}

#[test]
fn a_line_that_fits_is_shown_whole_with_the_cursor_after_it() {
    let ed = editor_with("proj, sql");
    assert_eq!(ed.window(20), ("proj, sql".to_owned(), 9));
}

#[test]
fn a_line_that_exactly_fills_the_width_leaves_a_column_for_the_cursor() {
    let ed = editor_with("abcd");
    let (shown, col) = ed.window(4);
    assert_eq!((shown.as_str(), col), ("bcd", 3));
}

#[test]
fn a_long_line_shows_the_tail_up_to_the_cursor() {
    let ed = editor_with("0123456789");
    assert_eq!(ed.window(5), ("6789".to_owned(), 4));
}

#[test]
fn moving_left_keeps_the_cursor_in_the_window() {
    let mut ed = editor_with("0123456789");
    for _ in 0..8 {
        ed.on_key(&press(KeyCode::Left), false);
    }
    let (shown, col) = ed.window(5);
    assert_eq!((shown.as_str(), col), ("01234", 2));
}

#[test]
fn the_cursor_is_always_inside_the_window() {
    let mut ed = editor_with("a line of some length, with wide ＡＢ characters");
    for _ in 0..60 {
        for width in 0..=12 {
            let (shown, col) = ed.window(width);
            if width == 0 {
                assert_eq!((shown.as_str(), col), ("", 0));
            } else {
                assert!(col < width, "{col} in {width}");
            }
        }
        ed.on_key(&press(KeyCode::Left), false);
    }
}

#[test]
fn a_wide_character_never_overflows_the_window() {
    let ed = editor_with("ＡＢＣＤ");
    let (shown, col) = ed.window(5);
    assert!(super::wrap::text_width(&shown) <= 5, "{shown:?}");
    assert!(col < 5);
}
