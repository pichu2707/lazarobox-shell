//! A left click in the open menu (open-links S2): the link line of the
//! Hugging Face token field opens like Ctrl+O; every other click does
//! nothing. The click lands where the popup is drawn because the renderer
//! and the hit-test share one geometry.

use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use super::*;
use crate::core::ai::hf::TokenFacts;
use crate::core::layout::Rect;
use crate::core::menu::{CHROME, Footer, MENU_BODY};

const TOKEN_URL: &str = "https://huggingface.co/settings/tokens/new?tokenType=read";

fn special(code: KeyCode) -> AppEvent {
    AppEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn no_tokens() -> TokenFacts {
    TokenFacts::default()
}

fn press_at(kind: MouseEventKind, (column, row): (u16, u16)) -> AppEvent {
    AppEvent::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

fn click(a: &mut App, at: (u16, u16)) -> Vec<Effect> {
    a.update(press_at(MouseEventKind::Down(MouseButton::Left), at))
}

/// A `cols` x `rows` app with the menu open on the token field.
fn token_field(cols: u16, rows: u16, opener: bool) -> App {
    let mut a = App::new(cols, rows)
        .with_opener(opener)
        .with_hf_tokens(no_tokens);
    a.update(ctrl_space());
    a.update(key('m'));
    goto_row(&mut a, "Hugging Face");
    a.update(special(KeyCode::Enter));
    assert!(row_of(&a, |row| matches!(row, Row::Field(_))).is_some());
    a
}

/// The body index of the first row that `pick` accepts.
fn row_of(a: &App, pick: impl Fn(&Row) -> bool) -> Option<usize> {
    a.menu().unwrap().rows(&a.config).iter().position(pick)
}

/// The screen cell inside body row `index` of the drawn popup, at `dx`
/// columns from its left edge.
fn cell_of(a: &App, index: usize, dx: u16) -> (u16, u16) {
    let (cols, rows) = a.term;
    let screen = Rect {
        x: 0,
        y: 0,
        width: cols,
        height: rows,
    };
    let layout = a
        .menu()
        .unwrap()
        .popup_layout(&a.config, screen)
        .expect("the full popup is drawn");
    (layout.inner.x + dx, layout.first_row + index as u16)
}

fn link_cell(a: &App) -> (u16, u16) {
    let index = row_of(a, |row| matches!(row, Row::Text(text) if text == TOKEN_URL))
        .expect("the link line is drawn");
    cell_of(a, index, 2)
}

/// Everything a click that does nothing must leave as it was.
fn snapshot(a: &App) -> (InputMode, Vec<Row>, String) {
    let menu = a.menu().unwrap();
    (
        a.input(),
        menu.rows(&a.config),
        format!("{:?}", menu.footer()),
    )
}

// Spec (open-links S2): a click on the link line does what Ctrl+O does: it
// opens the link through the host's opener and says so, the field stays.
#[test]
fn a_click_on_the_token_link_opens_it_in_the_browser() {
    // A roomy terminal, and a narrow one just tall enough for the popup.
    let least = MENU_BODY as u16 + CHROME - 1;
    for (cols, rows) in [(80, 24), (40, least)] {
        let mut a = token_field(cols, rows, true);
        a.take_dirty();
        let at = link_cell(&a);
        assert_eq!(
            click(&mut a, at),
            vec![Effect::OpenUrl(TOKEN_URL.to_owned())],
            "{cols}x{rows}"
        );
        assert_eq!(
            a.menu().unwrap().footer(),
            Footer::Hint("Opened in your browser".into()),
            "{cols}x{rows}"
        );
        assert_eq!(a.input(), InputMode::Menu);
        assert!(row_of(&a, |row| matches!(row, Row::Field(_))).is_some());
        assert!(a.take_dirty(), "the footer is redrawn");
    }
}

// Spec (open-links S2, S3): with no opener the click copies the link, as
// Ctrl+O does.
#[test]
fn a_click_on_the_token_link_copies_it_without_an_opener() {
    let mut a = token_field(80, 24, false);
    let at = link_cell(&a);
    let effects = click(&mut a, at);
    assert!(
        matches!(effects.as_slice(), [Effect::SetClipboard(_)]),
        "{effects:?}"
    );
    assert_eq!(a.input(), InputMode::Menu);
    assert!(row_of(&a, |row| matches!(row, Row::Field(_))).is_some());
}

// Spec (open-links S2): only the link line is clickable: the other rows of
// the field, the tab bar, the border and the screen outside do nothing.
#[test]
fn other_clicks_in_the_token_field_do_nothing() {
    let mut a = token_field(80, 24, true);
    let link = row_of(
        &a,
        |row| matches!(row, Row::Text(text) if text == TOKEN_URL),
    )
    .unwrap();
    let (x, y) = link_cell(&a);
    let mut cells: Vec<(u16, u16)> = (0..link).map(|index| cell_of(&a, index, 2)).collect();
    cells.extend([
        (x, y - link as u16 - 1), // the tab bar
        (x - 3, y),               // the border, left of the link line
        (x, y + 1),               // under the link line
        (0, 0),
        (79, 23),
    ]);
    a.take_dirty();
    let before = snapshot(&a);
    for at in cells {
        assert_eq!(click(&mut a, at), vec![], "{at:?}");
        assert_eq!(snapshot(&a), before, "{at:?}");
    }
    assert!(!a.take_dirty());
}

// Spec (open-links S2): only a left press opens the link.
#[test]
fn other_buttons_on_the_link_line_do_nothing() {
    let mut a = token_field(80, 24, true);
    let at = link_cell(&a);
    a.take_dirty();
    let before = snapshot(&a);
    for kind in [
        MouseEventKind::Down(MouseButton::Right),
        MouseEventKind::Down(MouseButton::Middle),
        MouseEventKind::Up(MouseButton::Left),
        MouseEventKind::Drag(MouseButton::Left),
        MouseEventKind::Moved,
    ] {
        assert_eq!(a.update(press_at(kind, at)), vec![], "{kind:?}");
        assert_eq!(snapshot(&a), before, "{kind:?}");
    }
    assert!(!a.take_dirty());
}

// Spec (open-links S2): with `Mouse` off the app takes no click.
#[test]
fn a_click_with_the_mouse_off_does_nothing() {
    let mut a = token_field(80, 24, true);
    a.config.mouse = false;
    let at = link_cell(&a);
    let before = snapshot(&a);
    assert_eq!(click(&mut a, at), vec![]);
    assert_eq!(snapshot(&a), before);
}

// Spec (open-links S2): a click on a menu row does not select it.
#[test]
fn a_click_on_a_menu_row_does_nothing() {
    let mut a = app();
    a.update(ctrl_space());
    a.update(key('m'));
    let rows = a.menu().unwrap().rows(&a.config).len();
    a.take_dirty();
    let before = snapshot(&a);
    for index in 0..rows {
        let at = cell_of(&a, index, 2);
        assert_eq!(click(&mut a, at), vec![], "row {index}");
        assert_eq!(snapshot(&a), before, "row {index}");
    }
    assert!(!a.take_dirty());
}

// Spec (open-links S2): the Feedback form shows no link, so no row of it
// is clickable.
#[test]
fn a_click_in_the_feedback_form_does_nothing() {
    let mut a = app().with_opener(true);
    a.update(ctrl_space());
    a.update(key('m'));
    goto_row(&mut a, "Feedback");
    a.update(special(KeyCode::Enter));
    let rows = a.menu().unwrap().rows(&a.config).len();
    let before = snapshot(&a);
    for index in 0..rows {
        let at = cell_of(&a, index, 2);
        assert_eq!(click(&mut a, at), vec![], "row {index}");
        assert_eq!(snapshot(&a), before, "row {index}");
    }
}

// Spec (open-links S2): on the one-line fallback nothing is clickable.
#[test]
fn a_click_on_the_fallback_line_does_nothing() {
    let mut a = token_field(80, 24, true);
    a.update(AppEvent::Resize { cols: 30, rows: 6 });
    let before = snapshot(&a);
    for row in 0..6 {
        for column in [0, 5, 15, 29] {
            assert_eq!(click(&mut a, (column, row)), vec![], "{column},{row}");
        }
    }
    assert_eq!(snapshot(&a), before);
}

// Spec (mouse-input): outside MENU a click is still ignored.
#[test]
fn a_click_outside_the_menu_does_nothing() {
    let mut a = app();
    a.take_dirty();
    for at in [(0, 0), (10, 5), (79, 23)] {
        assert_eq!(click(&mut a, at), vec![], "{at:?}");
    }
    assert_eq!(a.input(), InputMode::Terminal);
    assert!(!a.take_dirty());
}
