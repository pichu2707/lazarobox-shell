//! Tests of the `MenuState::goto` test helper.

use super::ai_tests::{machine, menu, selected_label};
use super::*;

const LABELS: [&str; 16] = [
    "Statusline position",
    "Tab bar position",
    "Mouse",
    "Notify",
    "Sound",
    "Updates",
    "Set all tasks",
    "general",
    "code",
    "chat",
    "review",
    "summarize",
    "Profile",
    "Context",
    "Memory",
    "Tools",
];

#[test]
fn goto_reaches_every_row_from_every_start() {
    for start in LABELS {
        for target in LABELS {
            let mut m = menu(None, None, Some(machine()));
            m.goto(start);
            m.goto(target);
            assert_eq!(selected_label(&m), target, "{start} -> {target}");
        }
    }
}

#[test]
fn goto_stays_when_the_row_is_already_selected() {
    let mut m = menu(None, None, Some(machine()));
    m.goto("Statusline position");
    assert_eq!(selected_label(&m), "Statusline position");
}

#[test]
fn goto_finds_the_updates_row_when_an_update_marks_it() {
    let mut m = menu(None, None, Some(machine())).with_update(Some("9.9.9".into()));
    m.goto("Updates");
    assert!(selected_label(&m).ends_with("Updates"));
}

#[test]
#[should_panic(expected = "no row Nope")]
fn goto_panics_on_an_unknown_label() {
    menu(None, None, Some(machine())).goto("Nope");
}

#[test]
#[should_panic(expected = "no row Nope")]
fn goto_panics_without_ai_rows_instead_of_hanging() {
    MenuState::open(&Config::default()).goto("Nope");
}

#[test]
#[should_panic(expected = "no row code")]
fn goto_cannot_reach_an_ai_row_without_the_ai_section() {
    MenuState::open(&Config::default()).goto("code");
}
