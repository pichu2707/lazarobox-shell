//! Tests of the `Memory` row and its privacy warning (engram-memory M4).

use super::*;
use crate::core::ai::config::MemoryConfig;
use crate::core::menu::{CHROME, MENU_BODY, MIN_WIDTH};

const ACKED: MemoryConfig = MemoryConfig {
    enabled: false,
    acknowledged: true,
    port: 7437,
    save_summaries: false,
};

fn menu_with(memory: MemoryConfig) -> MenuState {
    let mut ai = AiConfig::default();
    ai.memory = memory;
    MenuState::open(&Config::default()).with_ai(ai, None)
}

fn fresh() -> MenuState {
    menu_with(MemoryConfig::default())
}

fn press(menu: &mut MenuState, code: KeyCode) -> MenuCommand {
    menu.on_key(
        &KeyEvent::new(code, KeyModifiers::NONE),
        false,
        &mut Config::default(),
    )
}

fn press_with(menu: &mut MenuState, code: KeyCode, modifiers: KeyModifiers) -> MenuCommand {
    menu.on_key(
        &KeyEvent::new(code, modifiers),
        false,
        &mut Config::default(),
    )
}

fn repeat(menu: &mut MenuState, code: KeyCode) -> MenuCommand {
    menu.on_key(
        &KeyEvent::new(code, KeyModifiers::NONE),
        true,
        &mut Config::default(),
    )
}

fn selected_label(menu: &MenuState) -> &'static str {
    menu.rows(&Config::default())
        .iter()
        .find_map(|row| match row {
            Row::Item {
                label,
                selected: true,
                ..
            } => Some(*label),
            _ => None,
        })
        .expect("a selected row")
}

fn goto_memory(menu: &mut MenuState) {
    menu.goto("Memory");
}

/// The value shown on the `Memory` row.
fn shown_value(menu: &MenuState) -> String {
    menu.all_rows(&Config::default())
        .iter()
        .find_map(|row| match row {
            Row::Item {
                label: "Memory",
                value,
                ..
            } => value.as_deref().map(str::to_owned),
            _ => None,
        })
        .expect("a Memory row")
}

/// Every title and text line of the current view, one per line.
fn view_text(menu: &MenuState) -> String {
    menu.rows(&Config::default())
        .iter()
        .filter_map(|row| match row {
            Row::Title(text) | Row::Text(text) => Some(text.to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn warned() -> MenuState {
    let mut menu = fresh();
    goto_memory(&mut menu);
    press(&mut menu, KeyCode::Char('l'));
    assert!(menu.in_warning());
    menu
}

// Spec (config-menu): Sections and items are data, Descriptor integrity.
#[test]
fn the_memory_descriptor_is_well_formed_and_closes_the_chat_tab() {
    let memory = &CHAT_SECTION.items[CHAT_SECTION.items.len() - 1];
    assert_eq!(memory.label, "Memory");
    let ItemKind::Draft(AiChoice {
        value: DraftValue::Flag(row),
        ..
    }) = &memory.kind
    else {
        panic!("not a memory row");
    };
    assert_eq!((row.table, row.key), ("ai.memory", "enabled"));
    assert_eq!(row.values, ["off", "on"]);
    let labels: Vec<_> = CHAT_SECTION.items.iter().map(|i| i.label).collect();
    assert_eq!(labels, ["Profile", "Context", "Memory"]);
}

// Spec: Navigation, Task rows in order, Wrap.
#[test]
fn navigation_visits_memory_last_on_the_chat_tab_and_wraps() {
    let mut menu = fresh();
    menu.goto("Profile");
    for label in ["Context", "Memory", "Profile"] {
        press(&mut menu, KeyCode::Char('j'));
        assert_eq!(selected_label(&menu), label);
    }
    press(&mut menu, KeyCode::Up);
    assert_eq!(selected_label(&menu), "Memory");
    press(&mut menu, KeyCode::Up);
    assert_eq!(selected_label(&menu), "Context");
}

#[test]
fn memory_shows_off_by_default_and_on_when_enabled() {
    assert_eq!(shown_value(&fresh()), "off");
    let on = MemoryConfig {
        enabled: true,
        acknowledged: true,
        ..ACKED
    };
    assert_eq!(shown_value(&menu_with(on)), "on");
}

// Spec: Toggle when acknowledged.
#[test]
fn the_toggle_keys_flip_the_draft_when_acknowledged() {
    for code in [
        KeyCode::Char('l'),
        KeyCode::Char('h'),
        KeyCode::Right,
        KeyCode::Left,
        KeyCode::Char(' '),
        KeyCode::Enter,
    ] {
        let mut menu = menu_with(ACKED);
        goto_memory(&mut menu);
        assert_eq!(press(&mut menu, code), MenuCommand::None, "{code:?}");
        assert_eq!(shown_value(&menu), "on", "{code:?}");
        assert!(!menu.in_warning(), "{code:?}");
        press(&mut menu, code);
        assert_eq!(shown_value(&menu), "off", "{code:?}");
    }
}

#[test]
fn repeat_toggles_like_a_press_except_on_enter() {
    let mut menu = menu_with(ACKED);
    goto_memory(&mut menu);
    repeat(&mut menu, KeyCode::Char('l'));
    assert_eq!(shown_value(&menu), "on");
    repeat(&mut menu, KeyCode::Enter);
    assert_eq!(shown_value(&menu), "on", "Repeat of Enter is ignored");
}

// Spec: Footer and refresh.
#[test]
fn the_footer_is_the_settings_hint_and_r_does_nothing() {
    for memory in [MemoryConfig::default(), ACKED] {
        let mut menu = menu_with(memory);
        goto_memory(&mut menu);
        assert_eq!(
            menu.footer(),
            Footer::Hint(Cow::Borrowed("j/k move · h/l change · s save · Esc cancel"))
        );
        assert_eq!(press(&mut menu, KeyCode::Char('r')), MenuCommand::None);
    }
}

// Spec: No side effects on change.
#[test]
fn toggling_changes_no_live_config_and_asks_for_no_relayout() {
    let mut menu = menu_with(ACKED);
    goto_memory(&mut menu);
    let mut config = Config::default();
    let command = menu.on_key(
        &KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
        false,
        &mut config,
    );
    assert_eq!(command, MenuCommand::None);
    assert_eq!(config, Config::default());
}

// Spec: Memory privacy warning view, ruling 3 (draft flow).
#[test]
fn turning_on_without_the_acknowledgement_opens_the_warning() {
    let menu = warned();
    assert_eq!(
        shown_value_or_none(&menu),
        None,
        "the view replaces the rows"
    );
    assert!(view_text(&menu).contains("Turn memory on? (y/n)"));
    assert_eq!(menu.ai_draft().memory, MemoryConfig::default());
}

fn shown_value_or_none(menu: &MenuState) -> Option<String> {
    menu.all_rows(&Config::default())
        .iter()
        .find_map(|row| match row {
            Row::Item {
                label: "Memory",
                value,
                ..
            } => value.as_deref().map(str::to_owned),
            _ => None,
        })
}

#[test]
fn the_warning_states_the_three_facts() {
    let text = view_text(&warned());
    for needle in [
        "private",
        "0600",
        "every local process",
        "every agent that uses engram",
        "engram sync",
        "cannot restrict",
        "Turn memory on? (y/n)",
    ] {
        assert!(text.contains(needle), "{needle:?} missing in:\n{text}");
    }
    assert!(
        !view_text(&warned()).contains("Statusline"),
        "the warning replaces the list"
    );
}

#[test]
fn y_sets_the_drafts_and_writes_nothing() {
    let mut menu = warned();
    assert_eq!(press(&mut menu, KeyCode::Char('y')), MenuCommand::None);
    assert!(!menu.in_warning());
    assert_eq!(shown_value(&menu), "on");
    let memory = menu.ai_draft().memory;
    assert!(memory.enabled && memory.acknowledged);
    assert_eq!(selected_label(&menu), "Memory");
}

#[test]
fn any_other_key_declines_and_keeps_memory_off() {
    let keys = [
        (KeyCode::Char('n'), KeyModifiers::NONE),
        (KeyCode::Esc, KeyModifiers::NONE),
        (KeyCode::Enter, KeyModifiers::NONE),
        (KeyCode::Char('Y'), KeyModifiers::SHIFT),
        (KeyCode::Char('y'), KeyModifiers::CONTROL),
        (KeyCode::Char('s'), KeyModifiers::NONE),
        (KeyCode::Char('l'), KeyModifiers::NONE),
    ];
    for (code, modifiers) in keys {
        let mut menu = warned();
        let command = press_with(&mut menu, code, modifiers);
        assert_eq!(
            command,
            MenuCommand::None,
            "{code:?}: Esc/s must not leave the menu"
        );
        assert!(!menu.in_warning(), "{code:?}");
        assert_eq!(shown_value(&menu), "off", "{code:?}");
        assert_eq!(menu.ai_draft().memory, MemoryConfig::default(), "{code:?}");
        assert!(menu.edits(&Config::default()).is_empty(), "{code:?}");
    }
}

#[test]
fn repeat_events_are_ignored_in_the_warning() {
    for code in [KeyCode::Char('y'), KeyCode::Char('n'), KeyCode::Esc] {
        let mut menu = warned();
        let before = menu.clone();
        assert_eq!(repeat(&mut menu, code), MenuCommand::None);
        assert_eq!(menu, before, "{code:?}");
        assert!(menu.in_warning());
    }
}

#[test]
fn the_warning_footer_names_its_keys() {
    let menu = warned();
    let Footer::Hint(hint) = menu.footer() else {
        panic!("a hint expected");
    };
    assert_eq!(hint, "y accept (then s to save) · any other key declines");
}

// Review W4: y only drafts; the text must not imply it activates by itself.
#[test]
fn the_warning_says_when_it_stops_showing_and_what_it_cannot_promise() {
    let text = view_text(&warned());
    for needle in [
        "Once saved, this note is not shown again",
        "stay in engram until you delete them",
        "secret redaction is best effort",
    ] {
        assert!(text.contains(needle), "{needle:?} missing in:\n{text}");
    }
    assert!(!text.contains("accepting is remembered"), "{text}");
}

#[test]
fn a_second_enable_after_the_acknowledgement_shows_no_warning() {
    let mut menu = warned();
    press(&mut menu, KeyCode::Char('y'));
    press(&mut menu, KeyCode::Char('h'));
    assert_eq!(shown_value(&menu), "off");
    assert!(
        menu.ai_draft().memory.acknowledged,
        "off keeps the ack draft"
    );
    press(&mut menu, KeyCode::Char('l'));
    assert!(!menu.in_warning());
    assert_eq!(shown_value(&menu), "on");
}

#[test]
fn declining_then_asking_again_shows_the_warning_again() {
    let mut menu = warned();
    press(&mut menu, KeyCode::Char('n'));
    press(&mut menu, KeyCode::Char('l'));
    assert!(menu.in_warning());
}

// Spec (config-persistence): only what changed is edited.
fn flag(key: &'static str, on: bool) -> ConfigEdit {
    ConfigEdit::memory_flag(key, on)
}

#[test]
fn an_accepted_warning_edits_both_flags_in_order() {
    let mut menu = warned();
    press(&mut menu, KeyCode::Char('y'));
    assert_eq!(
        menu.edits(&Config::default()),
        vec![flag("enabled", true), flag("acknowledged", true)]
    );
}

#[test]
fn toggling_off_after_accepting_only_edits_the_acknowledgement() {
    let mut menu = warned();
    press(&mut menu, KeyCode::Char('y'));
    press(&mut menu, KeyCode::Char('h'));
    assert_eq!(
        menu.edits(&Config::default()),
        vec![flag("acknowledged", true)]
    );
}

#[test]
fn turning_off_an_enabled_memory_edits_only_enabled() {
    let on = MemoryConfig {
        enabled: true,
        acknowledged: true,
        port: 8123,
        save_summaries: false,
    };
    let mut menu = menu_with(on);
    assert!(menu.edits(&Config::default()).is_empty());
    goto_memory(&mut menu);
    press(&mut menu, KeyCode::Char('h'));
    assert_eq!(menu.edits(&Config::default()), vec![flag("enabled", false)]);
    assert_eq!(
        menu.ai_draft().memory.port,
        8123,
        "the port is never edited"
    );
}

#[test]
fn memory_edits_follow_the_task_edits_and_never_include_the_port() {
    let mut menu = warned();
    press(&mut menu, KeyCode::Char('y'));
    let edits = menu.edits(&Config::default());
    assert!(
        edits
            .iter()
            .all(|e| e.table == "ai.memory" && e.key != "port")
    );
    assert!(edits.iter().all(ConfigEdit::is_flag));
}

// Spec: Esc reverts (MenuState side: the app drops the draft on Cancel).
#[test]
fn esc_on_the_task_list_asks_to_cancel_after_an_accepted_warning() {
    let mut menu = warned();
    press(&mut menu, KeyCode::Char('y'));
    assert_eq!(press(&mut menu, KeyCode::Esc), MenuCommand::Cancel);
}

#[test]
fn a_warning_never_opens_without_the_ai_section() {
    let mut menu = MenuState::open(&Config::default());
    for _ in 0..4 {
        press(&mut menu, KeyCode::Char('l'));
        press(&mut menu, KeyCode::Char('j'));
    }
    assert!(!menu.in_warning());
}

// Review W3: a hand-edited `enabled = true` without the acknowledgement is
// inactive at runtime, so the row says `off` and the first toggle warns.
const HAND_EDITED: MemoryConfig = MemoryConfig {
    enabled: true,
    acknowledged: false,
    port: 7437,
    save_summaries: false,
};

#[test]
fn enabled_without_the_acknowledgement_shows_off() {
    assert!(!HAND_EDITED.active());
    assert_eq!(shown_value(&menu_with(HAND_EDITED)), "off");
}

#[test]
fn the_first_toggle_of_a_hand_edited_memory_opens_the_warning() {
    let mut menu = menu_with(HAND_EDITED);
    goto_memory(&mut menu);
    press(&mut menu, KeyCode::Char('l'));
    assert!(menu.in_warning());
    assert!(menu.edits(&Config::default()).is_empty());
}

#[test]
fn accepting_for_a_hand_edited_memory_edits_only_the_acknowledgement() {
    let mut menu = menu_with(HAND_EDITED);
    goto_memory(&mut menu);
    press(&mut menu, KeyCode::Char('l'));
    press(&mut menu, KeyCode::Char('y'));
    assert_eq!(shown_value(&menu), "on");
    assert_eq!(
        menu.edits(&Config::default()),
        vec![flag("acknowledged", true)]
    );
}

#[test]
fn declining_for_a_hand_edited_memory_keeps_it_off_and_edits_nothing() {
    let mut menu = menu_with(HAND_EDITED);
    goto_memory(&mut menu);
    press(&mut menu, KeyCode::Char('l'));
    press(&mut menu, KeyCode::Char('n'));
    assert_eq!(shown_value(&menu), "off");
    assert!(menu.edits(&Config::default()).is_empty());
}

// Review W2: consent needs the facts on screen. When the popup does not fit,
// the one-line fallback hides them, so no key accepts.
#[test]
fn y_does_not_accept_when_the_popup_does_not_fit() {
    for (cols, rows) in [(30, 5), (23, 40), (80, 14), (0, 0)] {
        let mut menu = warned();
        menu.set_term(cols, rows);
        assert!(!menu.fits_warning(), "{cols}x{rows}");
        assert_eq!(press(&mut menu, KeyCode::Char('y')), MenuCommand::None);
        assert!(!menu.in_warning(), "{cols}x{rows}: any key closes it");
        assert_eq!(shown_value(&menu), "off", "{cols}x{rows}");
        assert_eq!(menu.ai_draft().memory, MemoryConfig::default());
        assert!(menu.edits(&Config::default()).is_empty());
    }
}

#[test]
fn y_accepts_at_the_smallest_size_that_fits() {
    let mut menu = warned();
    menu.set_term(MIN_WIDTH, MENU_BODY as u16 + CHROME - 1);
    assert!(menu.fits_warning());
    press(&mut menu, KeyCode::Char('y'));
    assert_eq!(shown_value(&menu), "on");
}

#[test]
fn an_unknown_terminal_size_counts_as_fitting() {
    assert!(warned().fits_warning());
}
