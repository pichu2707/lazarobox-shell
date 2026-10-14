//! Tests of the `Mouse` toggle row at the `MenuState` level.

use super::ai_tests::{machine, menu, press, selected_label};
use super::*;

fn mouse_edit(value: &'static str) -> ConfigEdit {
    ConfigEdit {
        table: "",
        key: "mouse",
        value: value.into(),
    }
}

fn config(mouse: bool) -> Config {
    Config {
        mouse,
        ..Config::default()
    }
}

fn select_mouse(menu: &mut MenuState) {
    for _ in 0..13 {
        if selected_label(menu) == "Mouse" {
            return;
        }
        press(menu, 'j');
    }
    panic!("the cursor never reaches the Mouse row");
}

// Spec: Descriptor integrity (Mouse has the key path `mouse`).
#[test]
fn the_mouse_descriptor_is_the_third_settings_toggle() {
    let item = &SECTIONS[0].items[2];
    assert_eq!(item.label, "Mouse");
    let ItemKind::Toggle(toggle) = &item.kind else {
        panic!("Mouse is not a toggle");
    };
    assert_eq!((toggle.table, toggle.key), ("", "mouse"));
    assert_eq!(toggle.values, ["off", "on"]);
    for mouse in [false, true] {
        let mut c = config(!mouse);
        (toggle.set)(&mut c, usize::from(mouse));
        assert_eq!(c.mouse, mouse);
        assert_eq!((toggle.get)(&c), usize::from(mouse));
    }
}

// Spec: Settings rows, Reflects the config.
#[test]
fn the_row_shows_the_live_value_and_is_not_muted() {
    for (mouse, text) in [(true, "on"), (false, "off")] {
        let rows = MenuState::open(&config(mouse)).rows(&config(mouse));
        assert_eq!(
            rows[3],
            Row::Item {
                label: "Mouse",
                value: Some(text.into()),
                selected: false,
                enabled: true,
            }
        );
    }
}

// Spec: Mouse row is selectable.
#[test]
fn the_cursor_goes_through_mouse_between_the_bar_row_and_the_ai_rows() {
    let mut m = menu(None, None, Some(machine()));
    press(&mut m, 'j');
    assert_eq!(selected_label(&m), "Tab bar position");
    press(&mut m, 'j');
    assert_eq!(selected_label(&m), "Mouse");
    press(&mut m, 'j');
    assert_eq!(selected_label(&m), "Set all tasks");
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "Mouse");
}

#[test]
fn without_the_ai_section_the_cursor_wraps_over_three_rows() {
    let mut m = MenuState::open(&Config::default());
    let mut live = Config::default();
    for expected in ["Tab bar position", "Mouse", "Statusline position"] {
        m.on_key(
            &KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
            false,
            &mut live,
        );
        assert_eq!(selected_label(&m), expected);
    }
}

// Spec: Mouse cycles.
#[test]
fn cycling_the_row_flips_the_live_flag_and_reports_a_change() {
    let mut m = menu(None, None, Some(machine()));
    select_mouse(&mut m);
    let mut live = Config::default();
    for (key, expected) in [('l', false), ('l', true), ('h', false), ('h', true)] {
        let event = KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE);
        assert_eq!(m.on_key(&event, false, &mut live), MenuCommand::Changed);
        assert_eq!(live.mouse, expected);
    }
    assert_eq!(live.bars, Config::default().bars, "no other key moved");
}

// Spec: edits emit `mouse` only when the draft differs from open.
#[test]
fn edits_write_mouse_as_a_root_boolean_only_when_it_changed() {
    let m = MenuState::open(&config(true));
    assert_eq!(m.edits(&config(true)), vec![]);
    assert_eq!(m.edits(&config(false)), vec![mouse_edit("false")]);
    let m = MenuState::open(&config(false));
    assert_eq!(m.edits(&config(false)), vec![]);
    assert_eq!(m.edits(&config(true)), vec![mouse_edit("true")]);
}

#[test]
fn a_toggled_back_mouse_adds_no_edit_and_comes_after_the_bar_edits() {
    let m = MenuState::open(&config(true));
    let mut live = config(false);
    live.mouse = true;
    assert_eq!(m.edits(&live), vec![]);
    live.mouse = false;
    live.bars.tabbar = BarPosition::Bottom;
    let keys: Vec<_> = m.edits(&live).iter().map(|e| (e.table, e.key)).collect();
    assert_eq!(keys, [("tabbar", "position"), ("", "mouse")]);
}
