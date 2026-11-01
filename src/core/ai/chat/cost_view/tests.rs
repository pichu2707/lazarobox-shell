use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn lines(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("line {i}")).collect()
}

#[test]
fn esc_closes_and_other_keys_do_not() {
    let mut view = CostView::with(lines(3));
    assert!(!view.on_key(&key(KeyCode::Char('x'))));
    assert!(view.on_key(&key(KeyCode::Esc)));
}

#[test]
fn scrolling_is_bounded_by_what_the_window_hides() {
    let mut view = CostView::with(lines(11));
    // Ten body rows in a window of four: six rows are hidden.
    view.set_layout(4, 10);
    for _ in 0..50 {
        view.on_key(&key(KeyCode::Down));
    }
    assert_eq!(view.scroll(), 6);
    // No dead zone: one Up moves at once.
    view.on_key(&key(KeyCode::Up));
    assert_eq!(view.scroll(), 5);
    view.on_key(&key(KeyCode::PageDown));
    assert_eq!(view.scroll(), 6);
    view.on_key(&key(KeyCode::PageUp));
    assert_eq!(view.scroll(), 0);
}

#[test]
fn a_view_that_fits_does_not_scroll() {
    let mut view = CostView::with(lines(3));
    view.set_layout(8, 2);
    view.on_key(&key(KeyCode::Down));
    assert_eq!(view.scroll(), 0);
}

#[test]
fn a_shrinking_window_pulls_the_scroll_back() {
    let mut view = CostView::with(lines(11));
    view.set_layout(4, 10);
    for _ in 0..6 {
        view.on_key(&key(KeyCode::Down));
    }
    view.set_layout(9, 10);
    assert_eq!(view.scroll(), 1);
}

#[test]
fn ctrl_c_is_not_swallowed_silently() {
    let mut view = CostView::with(lines(3));
    let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(!view.on_key(&ctrl_c));
    assert_eq!(view.note(), Some(CLOSE_FIRST));
    view.on_key(&key(KeyCode::Down));
    assert_eq!(view.note(), None, "the next key clears it");
}

#[test]
fn long_lines_wrap_and_keep_their_tail() {
    let long =
        "  claude › a-very-long-model-name: in 1 · out 2 · $0.0049 list-price equivalent · partial";
    let view = CostView::with(vec!["AI usage (alpha)".into(), long.into()]);
    let rows = view.body_rows(24);
    assert!(rows.len() > 1);
    assert!(
        rows.iter().all(|(row, _)| row.chars().count() <= 24),
        "{rows:?}"
    );
    let joined: String = rows
        .iter()
        .map(|(r, _)| r.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(joined.contains("list-price equivalent"), "{joined}");
    assert!(joined.ends_with("partial"), "{joined}");
}

#[test]
fn this_chat_rows_are_flagged_for_highlight() {
    let view = CostView::with(vec!["AI usage (alpha)".into(), "this chat: no data".into()]);
    assert_eq!(
        view.body_rows(40),
        vec![("this chat: no data".to_owned(), true)]
    );
}
