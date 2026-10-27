//! Tests of the menu tabs: descriptors, keys, cursor, sub-levels, labels,
//! the update note gate and the fixed sizes.

use super::ai_tests::{ch, detection, engine, machine, menu, press, selected_label};
use super::picker_tests::{cursor_label, key, open, send};
use super::*;
use crate::core::ai::{Status, config::MemoryConfig};

const SETTINGS: [&str; 8] = [
    "Statusline position",
    "Tab bar position",
    "Mouse",
    "Notify",
    "Sound",
    "llama-server idle",
    "Updates",
    "Feedback",
];

const AI: [&str; 17] = [
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
    "Search",
    "Write",
    "Exec",
    "Git",
    "Git write",
    "Approval",
    "Hugging Face",
];

fn with_mods(m: &mut MenuState, code: KeyCode, mods: KeyModifiers, repeat: bool) -> MenuCommand {
    m.on_key(&KeyEvent::new(code, mods), repeat, &mut Config::default())
}

fn labels(m: &MenuState) -> Vec<&'static str> {
    m.tabs().map(|t| t.label).collect()
}

fn tab(m: &mut MenuState) -> MenuCommand {
    send(m, key(KeyCode::Tab))
}

// ---- 3.1 descriptors ------------------------------------------------------

#[test]
fn tabs_are_well_formed_settings_first_ai_second_and_no_mcp() {
    assert_eq!(TABS[0].label, "Settings");
    assert_eq!(TABS[1].label, "AI");
    assert_eq!(TABS.len(), TAB_COUNT);
    let mut unique: Vec<_> = TABS.iter().map(|t| t.label).collect();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), TAB_COUNT, "unique labels");
    assert!(TABS.iter().all(|t| t.label != "MCP"));
    assert!(!TABS[0].needs_ai);
    assert!(TABS[1].needs_ai);
    assert!(TABS[0].note && !TABS[1].note, "the note lives on Settings");
}

#[test]
fn every_section_is_in_exactly_one_tab() {
    for section in SECTIONS.iter().chain([&AI_SECTION]) {
        let homes = TABS
            .iter()
            .flat_map(|t| t.sections.iter())
            .filter(|s| std::ptr::eq(*s, section))
            .count();
        assert_eq!(homes, 1, "{}", section.title);
    }
    let total: usize = TABS.iter().map(|t| t.sections.len()).sum();
    assert_eq!(total, SECTIONS.len() + 1, "no foreign section");
}

#[test]
fn the_ai_tab_is_listed_only_when_the_ai_section_is_attached() {
    assert_eq!(labels(&MenuState::open(&Config::default())), ["Settings"]);
    assert_eq!(labels(&menu(None, None, None)), ["Settings", "AI"]);
}

// ---- 3.2 keys -------------------------------------------------------------

#[test]
fn tab_and_back_tab_cycle_the_tabs_with_wrap_in_every_encoding() {
    let encodings = [
        (KeyCode::Tab, KeyModifiers::NONE),
        (KeyCode::Tab, KeyModifiers::SHIFT),
        (KeyCode::BackTab, KeyModifiers::NONE),
        (KeyCode::BackTab, KeyModifiers::SHIFT),
    ];
    for (code, mods) in encodings {
        let mut m = menu(None, None, Some(machine()));
        assert_eq!(m.active_tab(), 0);
        assert_eq!(with_mods(&mut m, code, mods, false), MenuCommand::None);
        assert_eq!(m.active_tab(), 1, "{code:?}+{mods:?}");
        assert_eq!(selected_label(&m), "Set all tasks");
        with_mods(&mut m, code, mods, false);
        assert_eq!(m.active_tab(), 0, "wraps {code:?}+{mods:?}");
        assert_eq!(selected_label(&m), "Statusline position");
    }
}

#[test]
fn ctrl_and_alt_tab_are_swallowed() {
    for mods in [KeyModifiers::CONTROL, KeyModifiers::ALT] {
        for code in [KeyCode::Tab, KeyCode::BackTab] {
            let mut m = menu(None, None, Some(machine()));
            let before = m.clone();
            assert_eq!(with_mods(&mut m, code, mods, false), MenuCommand::None);
            assert_eq!(m, before, "{code:?}+{mods:?}");
            assert_eq!(m.active_tab(), 0);
        }
    }
}

#[test]
fn a_repeat_of_tab_or_back_tab_switches_like_a_press() {
    for code in [KeyCode::Tab, KeyCode::BackTab] {
        let mut m = menu(None, None, Some(machine()));
        with_mods(&mut m, code, KeyModifiers::NONE, true);
        assert_eq!(m.active_tab(), 1, "{code:?}");
    }
}

#[test]
fn a_release_of_tab_does_nothing() {
    let mut m = menu(None, None, Some(machine()));
    let mut release = key(KeyCode::Tab);
    release.kind = KeyEventKind::Release;
    m.on_key(&release, false, &mut Config::default());
    assert_eq!(m.active_tab(), 0);
}

#[test]
fn with_a_single_tab_tab_does_nothing() {
    for code in [KeyCode::Tab, KeyCode::BackTab] {
        let mut m = MenuState::open(&Config::default());
        let before = m.clone();
        assert_eq!(send(&mut m, key(code)), MenuCommand::None);
        assert_eq!(m, before);
    }
}

#[test]
fn switching_tabs_has_no_side_effect_and_leaves_the_draft_alone() {
    let mut m = menu(Some("ollama"), Some("llama3:8b"), Some(machine()));
    let draft = m.ai_draft();
    let mut live = Config::default();
    for code in [KeyCode::Tab, KeyCode::BackTab, KeyCode::Tab] {
        let command = m.on_key(&key(code), false, &mut live);
        assert_eq!(
            command,
            MenuCommand::None,
            "no Resize, Write, Refresh or Save"
        );
    }
    assert_eq!(live, Config::default());
    assert_eq!(m.ai_draft(), draft);
    assert!(m.edits(&live).is_empty());
}

// ---- 3.3 cursor and navigation --------------------------------------------

#[test]
fn j_and_k_wrap_inside_the_settings_tab() {
    let mut m = menu(None, None, Some(machine()));
    let mut seen = vec![selected_label(&m)];
    for _ in 0..SETTINGS.len() - 1 {
        press(&mut m, 'j');
        seen.push(selected_label(&m));
    }
    assert_eq!(seen, SETTINGS);
    press(&mut m, 'j');
    assert_eq!(
        selected_label(&m),
        "Statusline position",
        "wraps, no crossing"
    );
    assert_eq!(m.active_tab(), 0);
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "Feedback", "wraps back");
    let up = KeyCode::Up;
    let down = KeyCode::Down;
    send(&mut m, key(down));
    assert_eq!(selected_label(&m), "Statusline position");
    send(&mut m, key(up));
    assert_eq!(selected_label(&m), "Feedback");
}

#[test]
fn j_and_k_wrap_inside_the_ai_tab() {
    let mut m = menu(None, None, Some(machine()));
    tab(&mut m);
    let mut seen = vec![selected_label(&m)];
    for _ in 0..AI.len() - 1 {
        press(&mut m, 'j');
        seen.push(selected_label(&m));
    }
    assert_eq!(seen, AI);
    press(&mut m, 'j');
    assert_eq!(selected_label(&m), "Set all tasks", "wraps, no crossing");
    assert_eq!(m.active_tab(), 1);
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "Hugging Face");
    assert_eq!(m.active_tab(), 1);
}

#[test]
fn each_tab_keeps_its_own_cursor() {
    let mut m = menu(None, None, Some(machine()));
    m.goto("Sound");
    tab(&mut m);
    assert_eq!(selected_label(&m), "Set all tasks", "a tab starts on row 0");
    m.goto("code");
    tab(&mut m);
    assert_eq!(selected_label(&m), "Sound", "Settings kept its row");
    tab(&mut m);
    assert_eq!(selected_label(&m), "code", "AI kept its row");
}

#[test]
fn a_new_menu_opens_on_settings_row_zero() {
    let m = menu(None, None, Some(machine()));
    assert_eq!(m.active_tab(), 0);
    assert_eq!(selected_label(&m), "Statusline position");
    assert_eq!(m.tab_cursor, [0; TAB_COUNT]);
}

// ---- 3.4 sub-levels -------------------------------------------------------

const TAB_KEYS: [(KeyCode, KeyModifiers); 3] = [
    (KeyCode::Tab, KeyModifiers::NONE),
    (KeyCode::BackTab, KeyModifiers::NONE),
    (KeyCode::Tab, KeyModifiers::SHIFT),
];

fn assert_inert(m: &mut MenuState) {
    let before = m.clone();
    for (code, mods) in TAB_KEYS {
        assert_eq!(with_mods(m, code, mods, false), MenuCommand::None);
        assert_eq!(*m, before, "{code:?}+{mods:?}");
        assert_eq!(m.active_tab(), 1);
    }
}

#[test]
fn tab_is_inert_in_the_engines_list() {
    let mut m = menu(None, None, Some(machine()));
    open(&mut m, "code");
    assert!(m.in_sub_level());
    assert_inert(&mut m);
}

#[test]
fn tab_is_inert_in_the_models_list() {
    let mut m = menu(None, None, Some(machine()));
    open(&mut m, "code");
    press(&mut m, 'j');
    press(&mut m, 'j');
    send(&mut m, key(KeyCode::Enter));
    assert!(m.in_sub_level());
    assert_inert(&mut m);
}

fn custom_field() -> MenuState {
    let mut m = menu(None, None, Some(machine()));
    open(&mut m, "code");
    press(&mut m, 'j');
    press(&mut m, 'j');
    send(&mut m, key(KeyCode::Enter));
    while cursor_label(&m) != "custom…" {
        press(&mut m, 'j');
    }
    send(&mut m, key(KeyCode::Enter));
    m
}

#[test]
fn tab_is_inert_in_the_custom_field_and_the_buffer_survives() {
    let mut m = custom_field();
    for c in ['a', 'b'] {
        send(&mut m, ch(c));
    }
    assert!(m.in_sub_level());
    assert_inert(&mut m);
    assert_eq!(
        m.rows(&Config::default()).last(),
        Some(&Row::Field("ab".into()))
    );
}

fn warned() -> MenuState {
    let mut m = menu(None, None, None);
    m.goto("Memory");
    press(&mut m, 'l');
    assert!(m.in_warning());
    m
}

#[test]
fn tab_in_the_privacy_warning_declines_and_stays_on_the_ai_tab() {
    for (code, mods) in TAB_KEYS {
        let mut m = warned();
        assert!(m.in_sub_level());
        assert_eq!(with_mods(&mut m, code, mods, false), MenuCommand::None);
        assert!(!m.in_warning(), "declined");
        assert_eq!(
            m.ai_draft().memory,
            MemoryConfig::default(),
            "draft stays off"
        );
        assert!(
            m.edits(&Config::default()).is_empty(),
            "nothing acknowledged"
        );
        assert_eq!(m.active_tab(), 1, "still the AI tab");
        assert_eq!(selected_label(&m), "Memory");
    }
}

// ---- 3.5 labels and the note gate -----------------------------------------

#[test]
fn the_settings_label_carries_a_star_while_an_update_waits() {
    let m = menu(None, None, None);
    assert_eq!(m.tab_labels(), ["Settings", "AI"]);
    let m = m.with_update(Some("9.9.9".into()));
    assert_eq!(m.tab_labels(), ["Settings*", "AI"]);
    let installed = m.with_installed(Some("9.9.9".into()));
    assert_eq!(
        installed.tab_labels(),
        ["Settings", "AI"],
        "after the install"
    );
}

#[test]
fn a_late_update_and_an_install_move_the_star() {
    let mut m = menu(None, None, None);
    m.set_update(Some("9.9.9".into()));
    assert_eq!(m.tab_labels()[0], "Settings*");
    m.set_installed(Some("9.9.9".into()));
    assert_eq!(m.tab_labels()[0], "Settings");
}

#[test]
fn without_the_ai_section_there_is_one_label() {
    let m = MenuState::open(&Config::default()).with_update(Some("9.9.9".into()));
    assert_eq!(m.tab_labels(), ["Settings*"]);
}

#[test]
fn the_update_note_shows_on_settings_only() {
    let mut m = menu(None, None, Some(machine())).with_update(Some("9.9.9".into()));
    assert!(m.update_note().is_some(), "Settings");
    tab(&mut m);
    assert_eq!(m.update_note(), None, "AI");
    tab(&mut m);
    assert!(m.update_note().is_some(), "back on Settings");
}

#[test]
fn the_update_note_is_hidden_over_the_sub_levels() {
    let mut picker = menu(None, None, Some(machine())).with_update(Some("9.9.9".into()));
    open(&mut picker, "code");
    assert_eq!(picker.update_note(), None);
    let warning = warned().with_update(Some("9.9.9".into()));
    assert_eq!(warning.update_note(), None);
}

#[test]
fn the_note_has_note_rows_lines() {
    let m = menu(None, None, None).with_update(Some("9.9.9".into()));
    assert_eq!(m.update_note().map(|n| n.len()), Some(NOTE_ROWS));
}

#[test]
fn in_sub_level_is_true_for_the_warning_the_picker_and_the_custom_field() {
    let mut m = menu(None, None, Some(machine()));
    assert!(!m.in_sub_level());
    tab(&mut m);
    assert!(!m.in_sub_level());
    assert!(warned().in_sub_level());
    let mut picker = menu(None, None, Some(machine()));
    open(&mut picker, "code");
    assert!(picker.in_sub_level());
    assert!(custom_field().in_sub_level());
}

// ---- 3.6 sizing -----------------------------------------------------------

/// Every view of an AI menu, as the body rows it takes under the bar.
fn view_heights() -> Vec<usize> {
    let cfg = Config::default();
    let models: Vec<String> = (1..=12).map(|n| format!("model{n}:7b")).collect();
    let refs: Vec<&str> = models.iter().map(String::as_str).collect();
    let big = detection(&[engine("ollama", Status::Available, &refs, None)]);
    let mut heights = Vec::new();
    // Settings with the update note: rows, a gap and the note.
    let mut m = menu(None, None, Some(machine())).with_update(Some("9.9.9".into()));
    heights.push(m.rows(&cfg).len() + 1 + NOTE_ROWS);
    tab(&mut m);
    heights.push(m.rows(&cfg).len() + CAUTION_ROWS);
    // The picker: engines and models of a long list.
    let mut p = menu(Some("ollama"), None, Some(big));
    open(&mut p, "code");
    heights.push(p.rows(&cfg).len());
    for _ in 0..3 {
        press(&mut p, 'j');
    }
    send(&mut p, key(KeyCode::Enter));
    heights.push(p.rows(&cfg).len());
    heights.push(custom_field().rows(&cfg).len());
    heights.push(warned().rows(&cfg).len());
    heights
}

#[test]
fn view_body_is_the_tallest_view_and_no_view_exceeds_it() {
    let heights = view_heights();
    assert!(
        heights.iter().all(|h| *h <= VIEW_BODY),
        "{heights:?} > {VIEW_BODY}"
    );
    assert_eq!(
        heights.iter().max().copied(),
        Some(VIEW_BODY),
        "{heights:?}"
    );
    assert_eq!(
        VIEW_BODY,
        AI_SECTION.items.len() + CAUTION_ROWS,
        "the AI tab and its caution row bind"
    );
}

#[test]
fn the_menu_body_is_the_bar_plus_the_tallest_view() {
    assert_eq!(MENU_BODY, TAB_BAR + VIEW_BODY);
    assert_eq!(MENU_BODY, 19);
}

#[test]
fn the_warning_has_warning_rows_lines() {
    assert_eq!(MenuState::warning_rows().len(), WARNING_ROWS);
    assert_eq!(WARNING_ROWS, 10);
}

#[test]
fn the_warning_fits_from_24_by_22() {
    let mut m = warned();
    m.set_term(MIN_WIDTH, 22);
    assert!(m.fits_warning());
    m.set_term(MIN_WIDTH, 21);
    assert!(!m.fits_warning());
    m.set_term(MIN_WIDTH - 1, 22);
    assert!(!m.fits_warning());
}

#[test]
fn the_no_ai_menu_keeps_its_single_tab_layout() {
    let m = MenuState::open(&Config::default());
    let rows = m.rows(&Config::default());
    assert_eq!(
        rows[0],
        Row::Title("Settings".into()),
        "titles stay with one tab"
    );
    assert_eq!(rows.len(), 6);
}

#[test]
fn titles_are_gone_when_the_menu_is_tabbed() {
    let mut m = menu(None, None, Some(machine()));
    for _ in 0..2 {
        let rows = m.rows(&Config::default());
        assert!(
            rows.iter().all(|r| matches!(r, Row::Item { .. })),
            "{rows:?}"
        );
        tab(&mut m);
    }
}
