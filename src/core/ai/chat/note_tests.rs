use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::note::*;
use crate::core::ai::memory::{kind::MemKind, port::Note};

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn held(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
    KeyEvent::new_with_kind(code, mods, KeyEventKind::Repeat)
}

fn typed(ed: &mut NoteEditor, text: &str) {
    for c in text.chars() {
        ed.on_key(&press(KeyCode::Char(c)), false);
    }
}

fn valid() -> NoteEditor {
    let mut ed = NoteEditor::seeded("the answer");
    typed(&mut ed, "Use Axum");
    ed
}

// 9.3: the editor is seeded with the answer, redacted ---------------------

#[test]
fn it_opens_on_the_answer_with_the_default_kind_and_an_empty_title() {
    let ed = NoteEditor::seeded("line one\nline two");
    assert_eq!(ed.kind(), MemKind::Decision);
    assert_eq!(ed.title(), "");
    assert_eq!(ed.content(), "line one\nline two");
    assert_eq!(ed.field(), Field::Title, "the title is what the user types");
    assert_eq!(ed.hint(), None);
    assert_eq!(ed.notice(), None);
}

#[test]
fn the_seed_is_redacted_and_the_hint_counts_the_secrets() {
    let ed = NoteEditor::seeded("key sk-abcdefghijklmnopqrstuvwx and password=hunter2 ok");
    assert!(!ed.content().contains("sk-abcdefgh"), "{}", ed.content());
    assert!(!ed.content().contains("hunter2"), "{}", ed.content());
    assert_eq!(ed.hint().as_deref(), Some("2 secrets redacted"));
    let one = NoteEditor::seeded("password=hunter2");
    assert_eq!(one.hint().as_deref(), Some("1 secret redacted"));
}

#[test]
fn a_long_source_is_cut_and_says_so() {
    let ed = NoteEditor::seeded(&"a".repeat(20_000));
    assert_eq!(ed.content().chars().count(), 16_000);
    assert_eq!(ed.hint().as_deref(), Some("Cut to 16000 characters"));
    let both = NoteEditor::seeded(&format!("password=hunter2 {}", "a".repeat(20_000)));
    assert_eq!(
        both.hint().as_deref(),
        Some("1 secret redacted · Cut to 16000 characters")
    );
}

#[test]
fn a_source_exactly_at_the_cap_is_not_cut() {
    let ed = NoteEditor::seeded(&"a".repeat(16_000));
    assert_eq!(ed.hint(), None);
}

// 9.4: the keys of the editor ---------------------------------------------

#[test]
fn left_and_right_cycle_the_kind_over_the_closed_list() {
    let mut ed = NoteEditor::seeded("x");
    ed.on_key(&press(KeyCode::BackTab), false);
    assert_eq!(ed.field(), Field::Kind);
    ed.on_key(&press(KeyCode::Right), false);
    assert_eq!(ed.kind(), MemKind::Architecture);
    ed.on_key(&press(KeyCode::Left), false);
    ed.on_key(&press(KeyCode::Left), false);
    assert_eq!(ed.kind(), MemKind::Preference, "wraps to the last kind");
    ed.on_key(&press(KeyCode::Right), false);
    assert_eq!(ed.kind(), MemKind::Decision, "wraps to the first kind");
    typed(&mut ed, "abc");
    assert_eq!(
        (ed.title(), ed.content()),
        (String::new(), "x".to_owned()),
        "text does not type here"
    );
}

#[test]
fn tab_and_shift_tab_move_between_the_fields_in_a_ring() {
    let mut ed = NoteEditor::seeded("x");
    assert_eq!(ed.field(), Field::Title);
    ed.on_key(&press(KeyCode::Tab), false);
    assert_eq!(ed.field(), Field::Content);
    ed.on_key(&press(KeyCode::Tab), false);
    assert_eq!(ed.field(), Field::Kind);
    ed.on_key(&press(KeyCode::Tab), false);
    assert_eq!(ed.field(), Field::Title);
    ed.on_key(&press(KeyCode::BackTab), false);
    assert_eq!(ed.field(), Field::Kind);
    ed.on_key(&press(KeyCode::BackTab), false);
    assert_eq!(ed.field(), Field::Content);
}

#[test]
fn text_edits_the_focused_field_only() {
    let mut ed = NoteEditor::seeded("body");
    typed(&mut ed, "Ti");
    ed.on_key(&press(KeyCode::Tab), false);
    typed(&mut ed, "!");
    assert_eq!((ed.title(), ed.content()), ("Ti".into(), "body!".into()));
    ed.on_key(&press(KeyCode::Backspace), false);
    ed.on_key(&press(KeyCode::Home), false);
    ed.on_key(&press(KeyCode::Delete), false);
    assert_eq!(ed.content(), "ody");
    ed.on_key(&press(KeyCode::End), false);
    ed.on_key(&press(KeyCode::Left), false);
    typed(&mut ed, "-");
    assert_eq!(ed.content(), "od-y");
}

#[test]
fn enter_is_a_newline_in_the_content_and_a_save_in_the_title() {
    let mut ed = NoteEditor::seeded("a");
    ed.on_key(&press(KeyCode::Tab), false);
    assert_eq!(
        ed.on_key(&press(KeyCode::Enter), false),
        NoteOutcome::Handled
    );
    assert_eq!(ed.content(), "a\n");
    let mut ed = valid();
    assert_eq!(
        ed.on_key(&press(KeyCode::Enter), false),
        NoteOutcome::Save(Note {
            kind: MemKind::Decision,
            title: "Use Axum".into(),
            content: "the answer".into(),
        })
    );
}

#[test]
fn enter_on_the_kind_does_nothing() {
    let mut ed = valid();
    ed.on_key(&press(KeyCode::BackTab), false);
    assert_eq!(
        ed.on_key(&press(KeyCode::Enter), false),
        NoteOutcome::Handled
    );
    assert!(!ed.is_saving());
}

#[test]
fn up_and_down_move_between_the_lines_of_the_content() {
    let mut ed = NoteEditor::seeded("ab\ncd");
    ed.on_key(&press(KeyCode::Tab), false);
    ed.on_key(&press(KeyCode::Up), false);
    typed(&mut ed, "X");
    assert_eq!(ed.content(), "abX\ncd");
    ed.on_key(&press(KeyCode::Down), false);
    typed(&mut ed, "Y");
    assert_eq!(ed.content(), "abX\ncdY");
}

#[test]
fn ctrl_s_saves_and_esc_discards() {
    let mut ed = valid();
    assert!(matches!(ed.on_key(&ctrl('s'), false), NoteOutcome::Save(_)));
    let mut ed = valid();
    assert_eq!(ed.on_key(&press(KeyCode::Esc), false), NoteOutcome::Discard);
}

#[test]
fn repeat_is_ignored_on_ctrl_s_and_esc() {
    let mut ed = valid();
    assert_eq!(
        ed.on_key(&held(KeyCode::Char('s'), KeyModifiers::CONTROL), true),
        NoteOutcome::Handled
    );
    assert!(!ed.is_saving());
    assert_eq!(
        ed.on_key(&held(KeyCode::Esc, KeyModifiers::NONE), true),
        NoteOutcome::Handled
    );
}

#[test]
fn every_other_key_is_swallowed_and_changes_nothing() {
    let mut ed = valid();
    let before = ed.clone();
    let others = [
        ctrl('c'),
        ctrl('n'),
        ctrl('y'),
        ctrl('o'),
        ctrl('r'),
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT),
        press(KeyCode::PageUp),
        press(KeyCode::PageDown),
        press(KeyCode::F(5)),
        press(KeyCode::Insert),
    ];
    for key in others {
        assert_eq!(ed.on_key(&key, false), NoteOutcome::Handled, "{key:?}");
    }
    assert_eq!(ed, before);
}

#[test]
fn a_paste_goes_to_the_focused_field() {
    let mut ed = NoteEditor::seeded("a");
    ed.paste("T\nitle");
    assert_eq!(ed.title(), "T itle", "a title is one line");
    ed.on_key(&press(KeyCode::Tab), false);
    ed.paste("\nb");
    assert_eq!(ed.content(), "a\nb");
    ed.on_key(&press(KeyCode::Tab), false);
    ed.paste("zzz");
    assert_eq!((ed.title(), ed.content()), ("T itle".into(), "a\nb".into()));
}

#[test]
fn the_title_holds_120_characters_and_the_content_16000() {
    let mut ed = NoteEditor::seeded("");
    ed.paste(&"t".repeat(300));
    assert_eq!(ed.title().chars().count(), 120);
    ed.on_key(&press(KeyCode::Tab), false);
    ed.paste(&"c".repeat(20_000));
    assert_eq!(ed.content().chars().count(), 16_000);
}

// 9.5: the save checks ----------------------------------------------------

#[test]
fn an_empty_title_is_refused_naming_the_field() {
    let mut ed = NoteEditor::seeded("body");
    typed(&mut ed, "   ");
    assert_eq!(ed.on_key(&ctrl('s'), false), NoteOutcome::Handled);
    assert_eq!(ed.notice(), Some("Title is required"));
    assert!(!ed.is_saving());
    assert_eq!(ed.field(), Field::Title);
}

#[test]
fn an_empty_content_is_refused_naming_the_field() {
    let mut ed = NoteEditor::seeded("");
    typed(&mut ed, "A title");
    assert_eq!(ed.on_key(&ctrl('s'), false), NoteOutcome::Handled);
    assert_eq!(ed.notice(), Some("Content is required"));
    assert_eq!(ed.field(), Field::Content, "the focus goes to the field");
    assert!(!ed.is_saving());
}

#[test]
fn a_second_redaction_that_changes_the_text_keeps_the_editor_open() {
    let mut ed = NoteEditor::seeded("fine");
    typed(&mut ed, "Use Axum");
    ed.on_key(&press(KeyCode::Tab), false);
    typed(&mut ed, " password=hunter2");
    assert_eq!(ed.on_key(&ctrl('s'), false), NoteOutcome::Handled);
    assert_eq!(ed.notice(), Some("Redacted again; review and save"));
    assert!(!ed.content().contains("hunter2"), "{}", ed.content());
    assert!(!ed.is_saving());
    // The text the pass left is stable: the next save goes out.
    let NoteOutcome::Save(note) = ed.on_key(&ctrl('s'), false) else {
        panic!("the unchanged text must be sent");
    };
    assert!(!note.content.contains("hunter2"));
    assert!(ed.is_saving());
}

#[test]
fn a_secret_typed_into_the_title_is_redacted_too() {
    let mut ed = NoteEditor::seeded("fine");
    typed(&mut ed, "token=abcd1234efgh");
    assert_eq!(ed.on_key(&ctrl('s'), false), NoteOutcome::Handled);
    assert_eq!(ed.notice(), Some("Redacted again; review and save"));
    assert!(!ed.title().contains("abcd1234efgh"), "{}", ed.title());
}

#[test]
fn while_saving_only_esc_acts() {
    let mut ed = valid();
    assert!(matches!(ed.on_key(&ctrl('s'), false), NoteOutcome::Save(_)));
    assert!(ed.is_saving());
    let frozen = ed.clone();
    assert_eq!(
        ed.on_key(&ctrl('s'), false),
        NoteOutcome::Handled,
        "one save"
    );
    assert_eq!(
        ed.on_key(&press(KeyCode::Enter), false),
        NoteOutcome::Handled
    );
    typed(&mut ed, "zzz");
    assert_eq!(ed, frozen);
    assert_eq!(ed.on_key(&press(KeyCode::Esc), false), NoteOutcome::Discard);
}

#[test]
fn a_failed_save_keeps_the_edited_text_and_shows_the_failure() {
    let mut ed = valid();
    ed.on_key(&ctrl('s'), false);
    ed.save_failed("engram did not answer");
    assert!(!ed.is_saving());
    assert_eq!(ed.notice(), Some("engram did not answer"));
    assert_eq!(
        (ed.title(), ed.content()),
        ("Use Axum".into(), "the answer".into())
    );
    assert!(matches!(ed.on_key(&ctrl('s'), false), NoteOutcome::Save(_)));
}

#[test]
fn typing_clears_the_notice() {
    let mut ed = NoteEditor::seeded("body");
    ed.on_key(&ctrl('s'), false);
    assert_eq!(ed.notice(), Some("Title is required"));
    typed(&mut ed, "A");
    assert_eq!(ed.notice(), None);
}
