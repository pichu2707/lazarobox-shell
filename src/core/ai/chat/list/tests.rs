use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;

fn row(id: i64, updated_at: i64, tag: Option<&str>, first: &str) -> ConvRow {
    ConvRow {
        id,
        updated_at,
        profile: "programmer".into(),
        tag: tag.map(str::to_owned),
        first: first.into(),
    }
}

/// Two `proj` conversations and one `other`, newest first as the store sends.
fn rows() -> Vec<ConvRow> {
    vec![
        row(3, 1_700_000_000, Some("proj"), "newest in proj"),
        row(2, 1_690_000_000, Some("other"), "somewhere else"),
        row(1, 1_680_000_000, Some("proj"), "oldest in proj"),
    ]
}

fn loaded(project: Option<&str>) -> ListView {
    let mut list = ListView::new(project.map(str::to_owned));
    list.set_rows(rows());
    list
}

fn press(list: &mut ListView, code: KeyCode) -> ListOutcome {
    list.on_key(&KeyEvent::new(code, KeyModifiers::NONE), false)
}

fn ch(list: &mut ListView, c: char) -> ListOutcome {
    press(list, KeyCode::Char(c))
}

fn ids(list: &ListView) -> Vec<i64> {
    list.visible().iter().map(|r| r.id).collect()
}

// Spec (Conversation list): Filtered by project.
#[test]
fn the_list_shows_the_conversations_of_the_project_newest_first() {
    let list = loaded(Some("proj"));
    assert_eq!(ids(&list), vec![3, 1]);
    assert!(!list.showing_all());
}

#[test]
fn another_project_shows_its_own_rows() {
    let list = loaded(Some("other"));
    assert_eq!(ids(&list), vec![2]);
}

// Spec: Toggle all projects.
#[test]
fn a_toggles_between_the_project_and_all_projects() {
    let mut list = loaded(Some("proj"));
    assert_eq!(ch(&mut list, 'a'), ListOutcome::Handled);
    assert_eq!(ids(&list), vec![3, 2, 1]);
    assert!(list.showing_all());
    ch(&mut list, 'a');
    assert_eq!(ids(&list), vec![3, 1]);
}

// Spec: No project.
#[test]
fn without_a_project_every_conversation_shows_and_a_does_nothing() {
    let mut list = loaded(None);
    assert_eq!(ids(&list), vec![3, 2, 1]);
    assert!(list.showing_all());
    assert_eq!(list.title(), "Conversations · all projects");
    ch(&mut list, 'a');
    assert_eq!(ids(&list), vec![3, 2, 1]);
}

#[test]
fn the_title_names_the_project() {
    assert_eq!(loaded(Some("proj")).title(), "Conversations · proj");
    let mut list = loaded(Some("proj"));
    ch(&mut list, 'a');
    assert_eq!(list.title(), "Conversations · all projects");
}

// Spec: Empty project / Empty store.
#[test]
fn an_empty_project_says_so() {
    let mut list = ListView::new(Some("proj".into()));
    list.set_rows(vec![row(2, 1, Some("other"), "x")]);
    assert_eq!(
        list.empty_text().as_deref(),
        Some("No conversations for proj. Press n to start one.")
    );
}

#[test]
fn an_empty_store_says_so_and_a_full_list_says_nothing() {
    let mut list = ListView::new(Some("proj".into()));
    list.set_rows(Vec::new());
    assert_eq!(
        list.empty_text().as_deref(),
        Some("No conversations yet. Press n to start one.")
    );
    assert_eq!(loaded(Some("proj")).empty_text(), None);
    // Without a project the same empty store reads the same.
    let mut none = ListView::new(None);
    none.set_rows(Vec::new());
    assert_eq!(
        none.empty_text().as_deref(),
        Some("No conversations yet. Press n to start one.")
    );
}

#[test]
fn before_the_rows_arrive_the_list_is_loading() {
    let list = ListView::new(Some("proj".into()));
    assert!(!list.loaded());
    assert_eq!(list.empty_text().as_deref(), Some("Loading conversations…"));
    assert!(loaded(None).loaded());
}

// Spec: Cursor clamps.
#[test]
fn the_cursor_clamps_at_both_ends() {
    let mut list = loaded(Some("proj"));
    assert_eq!(list.cursor(), 0);
    ch(&mut list, 'k');
    assert_eq!(list.cursor(), 0);
    ch(&mut list, 'j');
    assert_eq!(list.cursor(), 1);
    ch(&mut list, 'j');
    assert_eq!(list.cursor(), 1, "stays on the last row");
    press(&mut list, KeyCode::Up);
    assert_eq!(list.cursor(), 0);
    press(&mut list, KeyCode::Down);
    assert_eq!(list.cursor(), 1);
}

#[test]
fn a_held_cursor_key_keeps_moving() {
    let mut list = loaded(None);
    list.on_key(&KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE), true);
    assert_eq!(list.cursor(), 1);
}

#[test]
fn toggling_resets_the_cursor_and_removing_a_row_clamps_it() {
    let mut list = loaded(None);
    ch(&mut list, 'j');
    ch(&mut list, 'j');
    assert_eq!(list.cursor(), 2);
    list.remove(1);
    assert_eq!(list.cursor(), 1, "the last row is gone: clamp");
    assert_eq!(ids(&list), vec![3, 2]);
}

// Spec: New conversation.
#[test]
fn n_and_esc_start_a_new_conversation() {
    let mut list = loaded(Some("proj"));
    assert_eq!(ch(&mut list, 'n'), ListOutcome::New);
    assert_eq!(press(&mut list, KeyCode::Esc), ListOutcome::New);
    // Also over an empty list.
    let mut empty = ListView::new(None);
    empty.set_rows(Vec::new());
    assert_eq!(ch(&mut empty, 'n'), ListOutcome::New);
}

#[test]
fn enter_resumes_the_row_under_the_cursor() {
    let mut list = loaded(Some("proj"));
    ch(&mut list, 'j');
    assert_eq!(press(&mut list, KeyCode::Enter), ListOutcome::Resume(1));
    let mut empty = ListView::new(None);
    empty.set_rows(Vec::new());
    assert_eq!(press(&mut empty, KeyCode::Enter), ListOutcome::Handled);
}

// Spec: Delete confirmed / declined.
#[test]
fn d_then_y_deletes_the_row_under_the_cursor() {
    let mut list = loaded(None);
    ch(&mut list, 'j');
    assert_eq!(ch(&mut list, 'd'), ListOutcome::Handled);
    assert!(list.confirming());
    assert_eq!(list.prompt().as_deref(), Some("Delete conversation? (y/n)"));
    assert_eq!(ch(&mut list, 'y'), ListOutcome::Delete(2));
    assert!(!list.confirming());
}

#[test]
fn anything_but_a_plain_lowercase_y_declines() {
    for key in [
        KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::SHIFT),
        KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL),
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    ] {
        let mut list = loaded(None);
        ch(&mut list, 'd');
        assert_eq!(list.on_key(&key, false), ListOutcome::Handled, "{key:?}");
        assert!(!list.confirming(), "{key:?} cancels the prompt");
        assert_eq!(ids(&list), vec![3, 2, 1], "nothing is removed");
    }
}

#[test]
fn a_declined_prompt_does_not_act_on_the_key_that_declined_it() {
    let mut list = loaded(None);
    ch(&mut list, 'd');
    // `n` declines the prompt; it does not also start a new conversation.
    assert_eq!(ch(&mut list, 'n'), ListOutcome::Handled);
}

#[test]
fn a_held_y_does_not_confirm() {
    let mut list = loaded(None);
    ch(&mut list, 'd');
    let held = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE);
    assert_eq!(list.on_key(&held, true), ListOutcome::Handled);
    assert!(list.confirming(), "the prompt is still there");
}

#[test]
fn d_on_an_empty_list_asks_nothing() {
    let mut list = ListView::new(None);
    list.set_rows(Vec::new());
    ch(&mut list, 'd');
    assert!(!list.confirming());
}

#[test]
fn one_shot_keys_ignore_a_hold() {
    let mut list = loaded(None);
    for code in [
        KeyCode::Enter,
        KeyCode::Esc,
        KeyCode::Char('n'),
        KeyCode::Char('d'),
        KeyCode::Char('a'),
    ] {
        let out = list.on_key(&KeyEvent::new(code, KeyModifiers::NONE), true);
        assert_eq!(out, ListOutcome::Handled, "{code:?}");
    }
    assert!(!list.confirming());
    // A hold of `a` does not toggle either.
    let mut with_project = loaded(Some("proj"));
    with_project.on_key(&KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE), true);
    assert_eq!(ids(&with_project), vec![3, 1]);
}

// Spec modal-input: Ctrl+R in the list view, unlisted keys swallowed.
#[test]
fn other_keys_are_swallowed_and_type_nothing() {
    let mut list = loaded(Some("proj"));
    for key in [
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
        KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE),
    ] {
        assert_eq!(list.on_key(&key, false), ListOutcome::Handled, "{key:?}");
    }
    assert_eq!(list.cursor(), 0);
    assert!(!list.confirming());
}

#[test]
fn a_line_stays_until_the_next_key() {
    let mut list = loaded(None);
    list.set_line("already open in another pane");
    assert_eq!(list.line(), Some("already open in another pane"));
    ch(&mut list, 'j');
    assert_eq!(list.line(), None);
}

// Spec: rows are `YYYY-MM-DD · profile · first message…`.
#[test]
fn a_date_is_the_utc_day_of_the_last_activity() {
    assert_eq!(date_of(0), "1970-01-01");
    assert_eq!(date_of(1_700_000_000), "2023-11-14");
    // Leap day, and the second before a new year.
    assert_eq!(date_of(1_709_164_800), "2024-02-29");
    assert_eq!(date_of(1_704_067_199), "2023-12-31");
}

#[test]
fn a_row_reads_date_profile_and_first_message() {
    let r = row(1, 1_700_000_000, None, "hello there");
    assert_eq!(row_text(&r, 80), "2023-11-14 · programmer · hello there");
}

#[test]
fn newlines_and_tabs_in_the_first_message_turn_into_spaces() {
    let r = row(1, 0, None, "one\ntwo\r\n\tthree   four");
    assert_eq!(
        row_text(&r, 80),
        "1970-01-01 · programmer · one two three four"
    );
}

#[test]
fn control_characters_are_dropped_from_a_row() {
    let r = row(1, 0, None, "a\u{1b}[31mb\u{202e}c");
    assert_eq!(row_text(&r, 80), "1970-01-01 · programmer · abc");
}

// Spec: Long first message.
#[test]
fn a_long_row_is_cut_with_an_ellipsis_to_the_width() {
    let r = row(1, 0, None, &"word ".repeat(60));
    let line = row_text(&r, 40);
    assert_eq!(text_width(&line), 40);
    assert!(line.ends_with('…'), "{line}");
    assert!(line.starts_with("1970-01-01 · programmer · word"));
}

#[test]
fn a_row_that_fits_is_not_cut() {
    let r = row(1, 0, None, "short");
    let line = row_text(&r, 38);
    assert!(!line.contains('…'), "{line}");
}

#[test]
fn a_wide_character_never_overflows_the_width() {
    let r = row(1, 0, None, &"漢".repeat(40));
    let line = row_text(&r, 41);
    assert!(text_width(&line) <= 41, "{line}");
    assert!(line.ends_with('…'));
}

#[test]
fn a_zero_width_row_is_empty() {
    assert_eq!(row_text(&row(1, 0, None, "x"), 0), "");
}

#[test]
fn an_empty_first_message_leaves_the_row_without_text() {
    let r = row(1, 0, None, "  \n ");
    assert_eq!(row_text(&r, 80), "1970-01-01 · programmer ·");
}
