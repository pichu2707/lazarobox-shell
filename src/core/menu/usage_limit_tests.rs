//! Tests of the limit rows of the usage view (ai-cost, config-menu "Usage row
//! and view", slice 3): `Turn limit (tokens)`, `Day limit (tokens)` and
//! `Day limit ($)` are edited as text and `0` or empty means `off`.

use super::ai_tests::{machine, press, selected_label};
use super::picker_tests::{key, send};
use super::*;
use crate::core::config_edit::apply_edit;

const TURN: &str = "Turn limit (tokens)";
const DAY: &str = "Day limit (tokens)";
const USD: &str = "Day limit ($)";

fn view_with(ai: AiConfig) -> MenuState {
    let mut m = MenuState::open(&Config::default()).with_ai(ai, Some(machine()));
    m.goto("Usage (alpha)");
    send(&mut m, key(KeyCode::Enter));
    m
}

fn opened() -> MenuState {
    view_with(AiConfig::default())
}

fn value_of(menu: &MenuState, wanted: &str) -> String {
    menu.rows(&Config::default())
        .into_iter()
        .find_map(|row| match row {
            Row::Item {
                label,
                value: Some(value),
                ..
            } if label == wanted => Some(value.into_owned()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no row {wanted}"))
}

fn hint(menu: &MenuState) -> String {
    match menu.footer() {
        Footer::Hint(text) => text.into_owned(),
        other => panic!("expected a hint, got {other:?}"),
    }
}

fn go(menu: &mut MenuState, label: &str) {
    for _ in 0..8 {
        if selected_label(menu) == label {
            return;
        }
        press(menu, 'j');
    }
    panic!("no row {label}");
}

fn type_in(menu: &mut MenuState, text: &str) {
    for c in text.chars() {
        press(menu, c);
    }
}

fn edit_to(menu: &mut MenuState, label: &str, text: &str) {
    go(menu, label);
    send(menu, key(KeyCode::Enter));
    // Start from an empty field whatever the row held.
    for _ in 0..16 {
        send(menu, key(KeyCode::Backspace));
    }
    type_in(menu, text);
    send(menu, key(KeyCode::Enter));
}

fn edit(table: &'static str, key: &'static str, value: &'static str) -> ConfigEdit {
    ConfigEdit {
        table,
        key,
        value: value.into(),
    }
}

#[test]
fn the_limit_rows_follow_the_billing_rows_and_read_off_by_default() {
    let m = opened();
    for label in [TURN, DAY, USD] {
        assert_eq!(value_of(&m, label), "off");
    }
    let labels: Vec<_> = USAGE_SECTION.items.iter().map(|i| i.label).collect();
    assert_eq!(labels[5..], [TURN, DAY, USD]);
}

#[test]
fn typing_digits_edits_the_limit_and_a_letter_is_refused_with_a_notice() {
    let mut m = opened();
    go(&mut m, TURN);
    send(&mut m, key(KeyCode::Enter));
    type_in(&mut m, "40a");
    assert_eq!(hint(&m), "Digits only");
    type_in(&mut m, "00");
    assert_eq!(value_of(&m, TURN), "4000_");
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(value_of(&m, TURN), "4000");
    assert_eq!(
        m.edits(&Config::default()),
        [edit("ai.usage", "turn_tokens", "4000")]
    );
}

#[test]
fn esc_cancels_the_edit_and_keeps_the_view_open() {
    let mut ai = AiConfig::default();
    ai.usage_mut().set_turn_tokens(Some(4000));
    let mut m = view_with(ai);
    go(&mut m, TURN);
    send(&mut m, key(KeyCode::Enter));
    press(&mut m, '9');
    assert_eq!(send(&mut m, key(KeyCode::Esc)), MenuCommand::None);
    assert_eq!(value_of(&m, TURN), "4000");
    assert_eq!(m.sub_view(), Some(SubView::Usage));
    assert_eq!(m.edits(&Config::default()), vec![]);
}

#[test]
fn zero_and_empty_mean_off_and_removal_is_written() {
    for text in ["0", "", "000"] {
        let mut ai = AiConfig::default();
        ai.usage_mut().set_day_tokens(Some(4000));
        let mut m = view_with(ai);
        edit_to(&mut m, DAY, text);
        assert_eq!(value_of(&m, DAY), "off", "{text:?}");
        assert_eq!(
            m.edits(&Config::default()),
            [edit("ai.usage", "day_tokens", "")],
            "{text:?}"
        );
    }
}

#[test]
fn the_dollar_limit_takes_one_point_and_two_decimals() {
    let mut m = opened();
    go(&mut m, USD);
    send(&mut m, key(KeyCode::Enter));
    type_in(&mut m, "2.509");
    assert_eq!(hint(&m), "At most 2 decimals");
    assert_eq!(value_of(&m, USD), "2.50_");
    type_in(&mut m, ".");
    assert_eq!(hint(&m), "One decimal point only");
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(value_of(&m, USD), "2.5");
    assert_eq!(
        m.edits(&Config::default()),
        [edit("ai.usage", "day_usd", "2.5")]
    );
}

#[test]
fn a_point_is_refused_in_a_token_field_and_a_lone_point_is_not_a_number() {
    let mut m = opened();
    go(&mut m, DAY);
    send(&mut m, key(KeyCode::Enter));
    press(&mut m, '.');
    assert_eq!(hint(&m), "Digits only");
    send(&mut m, key(KeyCode::Esc));
    go(&mut m, USD);
    send(&mut m, key(KeyCode::Enter));
    press(&mut m, '.');
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(hint(&m), "Type a number, Esc cancels");
    assert_eq!(value_of(&m, USD), "._", "still editing");
}

#[test]
fn a_limit_has_at_most_twelve_digits() {
    let mut m = opened();
    go(&mut m, DAY);
    send(&mut m, key(KeyCode::Enter));
    type_in(&mut m, "1234567890123");
    assert_eq!(hint(&m), "At most 12 digits");
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(value_of(&m, DAY), "123456789012");
}

#[test]
fn backspace_deletes_and_the_field_starts_from_the_current_value() {
    let mut ai = AiConfig::default();
    ai.usage_mut().set_turn_tokens(Some(4000));
    let mut m = view_with(ai);
    go(&mut m, TURN);
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(value_of(&m, TURN), "4000_");
    send(&mut m, key(KeyCode::Backspace));
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(value_of(&m, TURN), "400");
}

#[test]
fn cycling_keys_on_a_limit_row_say_to_press_enter() {
    for code in [
        KeyCode::Char('l'),
        KeyCode::Char('h'),
        KeyCode::Right,
        KeyCode::Char(' '),
    ] {
        let mut m = opened();
        go(&mut m, TURN);
        send(&mut m, key(code));
        assert_eq!(hint(&m), "Enter edits this limit", "{code:?}");
        assert_eq!(value_of(&m, TURN), "off");
    }
}

#[test]
fn an_enter_repeat_does_not_start_an_edit() {
    let mut m = opened();
    go(&mut m, TURN);
    m.on_key(&key(KeyCode::Enter), true, &mut Config::default());
    press(&mut m, '5');
    assert_eq!(value_of(&m, TURN), "off");
}

#[test]
fn while_editing_every_other_key_says_so_and_nothing_leaks() {
    let mut m = opened();
    go(&mut m, TURN);
    send(&mut m, key(KeyCode::Enter));
    for code in [KeyCode::Tab, KeyCode::Char('s'), KeyCode::Char('j')] {
        assert_eq!(send(&mut m, key(code)), MenuCommand::None, "{code:?}");
        assert_eq!(hint(&m), "Digits only", "{code:?}");
    }
    assert_eq!(selected_label(&m), TURN);
}

#[test]
fn the_footer_names_enter_edit_and_the_edit_keys() {
    let mut m = opened();
    assert_eq!(
        hint(&m),
        "j/k move · h/l change · Enter edit · s save · Esc back"
    );
    go(&mut m, TURN);
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(hint(&m), "type digits · Enter ok · Esc cancel");
}

#[test]
fn the_limits_are_written_as_numbers_and_removed_when_cleared() {
    let write = |e: ConfigEdit, text: &str| apply_edit(Some(text), &[e]).unwrap();
    let text = write(edit("ai.usage", "turn_tokens", "4000"), "");
    assert!(text.contains("turn_tokens = 4000"), "{text}");
    let text = write(edit("ai.usage", "day_usd", "2.5"), &text);
    assert!(text.contains("day_usd = 2.5"), "{text}");
    let text = write(edit("ai.usage", "day_usd", "5"), &text);
    assert!(text.contains("day_usd = 5.0"), "{text}");
    let text = write(edit("ai.usage", "turn_tokens", ""), &text);
    assert!(!text.contains("turn_tokens"), "{text}");
    assert!(text.contains("day_usd"), "{text}");
}

#[test]
fn a_written_dollar_limit_parses_back() {
    let text = apply_edit(None, &[edit("ai.usage", "day_usd", "2.5")]).unwrap();
    let parsed = Config::parse(&text).unwrap();
    assert_eq!(parsed.ai.usage().day_usd_micros(), Some(2_500_000));
    assert!(parsed.problems.is_empty());
}
