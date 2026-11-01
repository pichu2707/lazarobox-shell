//! Tests of the menu tabs: descriptors, keys, cursor, sub-levels, labels,
//! the update note gate and the fixed sizes.

use super::ai_tests::{ch, detection, engine, machine, menu, press, selected_label};
use super::picker_tests::{cursor_label, key, open, send};
use super::*;
use crate::core::ai::{Status, config::MemoryConfig};

const SETTINGS: [&str; 9] = [
    "Statusline position",
    "Tab bar position",
    "Mouse",
    "Notify",
    "Sound",
    "llama-server idle",
    "Usage (alpha)",
    "Updates",
    "Feedback",
];

const MODELS: [&str; 8] = [
    "Set all tasks",
    "general",
    "code",
    "chat",
    "review",
    "summarize",
    "orchestrate",
    "Hugging Face",
];

const TOOLS: [&str; 8] = [
    "Tools",
    "Search",
    "Write",
    "Exec",
    "Git",
    "Git write",
    "Approval",
    "Orchestrator",
];

const CHAT: [&str; 3] = ["Profile", "Context", "Memory"];

fn with_mods(m: &mut MenuState, code: KeyCode, mods: KeyModifiers, repeat: bool) -> MenuCommand {
    m.on_key(&KeyEvent::new(code, mods), repeat, &mut Config::default())
}

fn labels(m: &MenuState) -> Vec<&'static str> {
    m.tabs().map(|t| t.label).collect()
}

fn tab(m: &mut MenuState) -> MenuCommand {
    send(m, key(KeyCode::Tab))
}

fn press_digit(m: &mut MenuState, digit: char) -> MenuCommand {
    send(m, ch(digit))
}

/// The footer notice, when the footer shows one of the menu's own.
fn notice(m: &MenuState) -> Option<String> {
    m.notice.clone()
}

// ---- 3.1 descriptors ------------------------------------------------------

#[test]
fn tabs_are_well_formed_in_the_five_tab_order() {
    let order: Vec<_> = TABS.iter().map(|t| t.label).collect();
    assert_eq!(order, ["Settings", "Models", "Tools", "Connectors", "Chat"]);
    assert_eq!(TABS.len(), TAB_COUNT);
    assert_eq!(TAB_COUNT, 5);
    let mut unique = order.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), TAB_COUNT, "unique labels");
    assert!(!TABS[0].needs_ai);
    assert!(TABS[1..].iter().all(|t| t.needs_ai));
    assert!(TABS[0].note && TABS[1..].iter().all(|t| !t.note));
    assert!(TABS[2].caution && !TABS[1].caution && !TABS[4].caution);
    assert!(TABS[3].sections.is_empty(), "no connector rows yet");
}

#[test]
fn every_section_is_in_exactly_one_tab() {
    let ai = [&MODELS_SECTION, &TOOLS_SECTION, &CHAT_SECTION];
    for section in SECTIONS.iter().chain(ai) {
        let homes = TABS
            .iter()
            .flat_map(|t| t.sections.iter())
            .filter(|s| std::ptr::eq(*s, section))
            .count();
        assert_eq!(homes, 1, "{}", section.title);
    }
    let total: usize = TABS.iter().map(|t| t.sections.len()).sum();
    assert_eq!(total, SECTIONS.len() + 3, "no foreign section");
}

#[test]
fn the_rows_sit_on_their_tab_in_the_tab_map_order() {
    let mut m = menu(None, None, Some(machine()));
    let mut seen = Vec::new();
    for _ in 0..m.tabs().count() {
        let rows = m.rows(&Config::default());
        seen.push(rows.len());
        tab(&mut m);
    }
    assert_eq!(
        seen,
        [SETTINGS.len(), MODELS.len(), TOOLS.len(), CHAT.len()]
    );
    let labels = |s: &Section| s.items.iter().map(|i| i.label).collect::<Vec<_>>();
    assert_eq!(labels(&MODELS_SECTION), MODELS);
    assert_eq!(labels(&TOOLS_SECTION), TOOLS);
    assert_eq!(labels(&CHAT_SECTION), CHAT);
}

/// A tab table whose Connectors tab has one row, for the scenarios where
/// MCP has landed.
static MCP_ROWS: Section = Section {
    title: "Connectors",
    items: &[Item {
        label: "server",
        kind: ItemKind::Feedback,
    }],
};

static WITH_CONNECTORS: [Tab; TAB_COUNT] = [
    Tab {
        label: "Settings",
        sections: SECTIONS,
        needs_ai: false,
        note: true,
        caution: false,
    },
    Tab {
        label: "Models",
        sections: std::slice::from_ref(&MODELS_SECTION),
        needs_ai: true,
        note: false,
        caution: false,
    },
    Tab {
        label: "Tools",
        sections: std::slice::from_ref(&TOOLS_SECTION),
        needs_ai: true,
        note: false,
        caution: true,
    },
    Tab {
        label: "Connectors",
        sections: std::slice::from_ref(&MCP_ROWS),
        needs_ai: true,
        note: false,
        caution: false,
    },
    Tab {
        label: "Chat",
        sections: std::slice::from_ref(&CHAT_SECTION),
        needs_ai: true,
        note: false,
        caution: false,
    },
];

/// Swaps the tab table for [`WITH_CONNECTORS`] until it drops.
struct Mcp;

impl Mcp {
    fn lands() -> Self {
        TEST_TABS.with(|t| t.set(Some(&WITH_CONNECTORS)));
        Self
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        TEST_TABS.with(|t| t.set(None));
    }
}

#[test]
fn the_ai_tabs_are_listed_only_when_the_ai_section_is_attached() {
    assert_eq!(labels(&MenuState::open(&Config::default())), ["Settings"]);
    assert_eq!(
        labels(&menu(None, None, None)),
        ["Settings", "Models", "Tools", "Chat"],
        "Connectors is hidden while it has no rows"
    );
}

#[test]
fn connectors_appears_between_tools_and_chat_with_its_first_row() {
    let _mcp = Mcp::lands();
    let mut m = menu(None, None, None);
    assert_eq!(
        labels(&m),
        ["Settings", "Models", "Tools", "Connectors", "Chat"]
    );
    assert_eq!(MenuState::open(&Config::default()).tabs().count(), 1);
    press_digit(&mut m, '3');
    tab(&mut m);
    assert_eq!(m.active_tab(), 3, "Connectors");
    tab(&mut m);
    assert_eq!(m.active_tab(), 4, "Chat");
}

#[test]
fn a_tab_appearing_keeps_every_cursor() {
    let mut m = menu(None, None, None);
    press_digit(&mut m, '5');
    m.goto("Memory");
    press_digit(&mut m, '1');
    press(&mut m, 'j');
    let _mcp = Mcp::lands();
    press_digit(&mut m, '5');
    assert_eq!(selected_label(&m), "Memory");
    press_digit(&mut m, '1');
    assert_eq!(selected_label(&m), "Tab bar position");
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
        let forward = code == KeyCode::Tab && mods == KeyModifiers::NONE;
        let mut m = menu(None, None, Some(machine()));
        assert_eq!(m.active_tab(), 0);
        assert_eq!(with_mods(&mut m, code, mods, false), MenuCommand::None);
        let (first, second) = if forward { (1, 2) } else { (3, 2) };
        assert_eq!(m.active_tab(), first, "{code:?}+{mods:?}");
        with_mods(&mut m, code, mods, false);
        assert_eq!(m.active_tab(), second, "{code:?}+{mods:?}");
        for _ in 0..2 {
            with_mods(&mut m, code, mods, false);
        }
        assert_eq!(m.active_tab(), 0, "wraps {code:?}+{mods:?}");
        assert_eq!(selected_label(&m), "Statusline position");
    }
}

#[test]
fn tab_skips_the_hidden_connectors_in_both_directions() {
    let mut m = menu(None, None, Some(machine()));
    let mut seen = Vec::new();
    for _ in 0..4 {
        tab(&mut m);
        seen.push(selected_label(&m));
    }
    assert_eq!(
        seen,
        ["Set all tasks", "Tools", "Profile", "Statusline position"]
    );
    send(&mut m, key(KeyCode::BackTab));
    assert_eq!(selected_label(&m), "Profile");
    send(&mut m, key(KeyCode::BackTab));
    assert_eq!(selected_label(&m), "Tools");
    assert_eq!(m.tab, 2, "the stored index is the TABS position");
}

#[test]
fn the_stored_tab_is_the_tabs_position_even_with_connectors_hidden() {
    let mut m = menu(None, None, None);
    press_digit(&mut m, '5');
    assert_eq!(m.tab, 4);
    assert_eq!(m.active_tab(), 3, "the bar position among the visible");
    assert_eq!(selected_label(&m), "Profile");
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
        assert_eq!(m.active_tab(), if code == KeyCode::Tab { 1 } else { 3 });
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
fn with_a_single_tab_tab_only_shows_a_notice() {
    for code in [KeyCode::Tab, KeyCode::BackTab] {
        let mut m = MenuState::open(&Config::default());
        let mut before = m.clone();
        assert_eq!(send(&mut m, key(code)), MenuCommand::None);
        assert_eq!(notice(&m).as_deref(), Some("No other tabs"));
        assert_eq!(m.footer(), Footer::Hint("No other tabs".into()));
        before.notice = m.notice.clone();
        assert_eq!(m, before, "nothing else changes");
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
fn j_and_k_wrap_inside_models_tools_and_chat() {
    for (digit, rows, tab_index) in [
        ('2', &MODELS[..], 1),
        ('3', &TOOLS[..], 2),
        ('5', &CHAT[..], 3),
    ] {
        let mut m = menu(None, None, Some(machine()));
        press_digit(&mut m, digit);
        let mut seen = vec![selected_label(&m)];
        for _ in 0..rows.len() - 1 {
            press(&mut m, 'j');
            seen.push(selected_label(&m));
        }
        assert_eq!(seen, rows);
        press(&mut m, 'j');
        assert_eq!(selected_label(&m), rows[0], "wraps, no crossing");
        assert_eq!(m.active_tab(), tab_index);
        press(&mut m, 'k');
        assert_eq!(selected_label(&m), rows[rows.len() - 1]);
        assert_eq!(m.active_tab(), tab_index);
    }
}

#[test]
fn each_tab_keeps_its_own_cursor() {
    let mut m = menu(None, None, Some(machine()));
    m.goto("Sound");
    tab(&mut m);
    assert_eq!(selected_label(&m), "Set all tasks", "a tab starts on row 0");
    m.goto("code");
    press_digit(&mut m, '3');
    press(&mut m, 'j');
    assert_eq!(selected_label(&m), "Search");
    press_digit(&mut m, '1');
    assert_eq!(selected_label(&m), "Sound", "Settings kept its row");
    press_digit(&mut m, '2');
    assert_eq!(selected_label(&m), "code", "Models kept its row");
    press_digit(&mut m, '3');
    assert_eq!(selected_label(&m), "Search", "Tools kept its row");
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

/// The picker lists swallow the digits: no jump, no notice.
fn assert_digits_inert(m: &mut MenuState) {
    let before = m.clone();
    for digit in ['1', '3', '4', '5', '0', '9'] {
        assert_eq!(press_digit(m, digit), MenuCommand::None);
        assert_eq!(*m, before, "digit {digit}");
        assert_eq!(m.notice, None, "no notice in a sub-level");
    }
}

#[test]
fn tab_is_inert_in_the_engines_list() {
    let mut m = menu(None, None, Some(machine()));
    open(&mut m, "code");
    assert!(m.in_sub_level());
    assert_inert(&mut m);
    assert_digits_inert(&mut m);
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
    assert_digits_inert(&mut m);
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
fn tab_in_the_privacy_warning_declines_and_stays_on_the_chat_tab() {
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
        assert_eq!(m.active_tab(), 3, "still the Chat tab");
        assert_eq!(selected_label(&m), "Memory");
    }
}

// ---- 3.5 labels and the note gate -----------------------------------------

#[test]
fn the_settings_label_carries_a_star_while_an_update_waits() {
    let m = menu(None, None, None);
    assert_eq!(m.tab_labels(), ["Settings", "Models", "Tools", "Chat"]);
    let m = m.with_update(Some("9.9.9".into()));
    assert_eq!(m.tab_labels(), ["Settings*", "Models", "Tools", "Chat"]);
    let installed = m.with_installed(Some("9.9.9".into()));
    assert_eq!(
        installed.tab_labels(),
        ["Settings", "Models", "Tools", "Chat"],
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
    for digit in ['2', '3', '5'] {
        press_digit(&mut m, digit);
        assert_eq!(m.update_note(), None, "tab {digit}");
    }
    press_digit(&mut m, '1');
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
    for digit in ['2', '3', '5'] {
        press_digit(&mut m, digit);
        let caution = if digit == '3' { CAUTION_ROWS } else { 0 };
        heights.push(m.rows(&cfg).len() + caution);
    }
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
    assert_eq!(VIEW_BODY, 12, "the Settings tab binds");
    assert_eq!(
        VIEW_BODY,
        SECTIONS[0].items.len() + 1 + NOTE_ROWS,
        "Settings with its note"
    );
    assert_eq!(
        1 + crate::core::ai::picker::VIEW,
        VIEW_BODY - 1,
        "the picker window is one row under"
    );
}

#[test]
fn the_menu_body_is_the_bar_plus_the_tallest_view() {
    assert_eq!(MENU_BODY, TAB_BAR + VIEW_BODY);
    assert_eq!(MENU_BODY, 13);
}

#[test]
fn the_warning_has_warning_rows_lines() {
    assert_eq!(MenuState::warning_rows().len(), WARNING_ROWS);
    assert_eq!(WARNING_ROWS, 10);
}

#[test]
fn the_warning_fits_from_24_by_15() {
    let mut m = warned();
    let least = MENU_BODY as u16 + CHROME - 1;
    m.set_term(MIN_WIDTH, least);
    assert!(m.fits_warning());
    m.set_term(MIN_WIDTH, least - 1);
    assert!(!m.fits_warning());
    m.set_term(MIN_WIDTH - 1, least);
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
    for _ in 0..4 {
        let rows = m.rows(&Config::default());
        assert!(
            rows.iter().all(|r| matches!(r, Row::Item { .. })),
            "{rows:?}"
        );
        tab(&mut m);
    }
}

// ---- 1.3 jump keys and notices --------------------------------------------

fn footer_text(m: &MenuState) -> String {
    match m.footer() {
        Footer::Hint(text) => text.into_owned(),
        Footer::Error(text) => text.to_owned(),
    }
}

#[test]
fn the_digits_jump_to_the_fixed_positions() {
    let mut m = menu(None, None, Some(machine()));
    let mut seen = Vec::new();
    for digit in ['2', '3', '5', '1'] {
        assert_eq!(press_digit(&mut m, digit), MenuCommand::None);
        seen.push((m.tab, selected_label(&m)));
    }
    assert_eq!(
        seen,
        [
            (1, "Set all tasks"),
            (2, "Tools"),
            (4, "Profile"),
            (0, "Statusline position")
        ]
    );
}

#[test]
fn every_digit_reaches_its_tab_when_connectors_has_rows() {
    let _mcp = Mcp::lands();
    let mut m = menu(None, None, Some(machine()));
    let mut seen = Vec::new();
    for digit in ['1', '2', '3', '4', '5'] {
        press_digit(&mut m, digit);
        seen.push(m.tab);
        assert_eq!(
            notice(&m).is_some(),
            digit == '1',
            "only the active tab says so"
        );
    }
    assert_eq!(seen, [0, 1, 2, 3, 4]);
}

#[test]
fn a_repeat_of_a_digit_jumps_like_a_press() {
    let mut m = menu(None, None, Some(machine()));
    with_mods(&mut m, KeyCode::Char('3'), KeyModifiers::NONE, true);
    assert_eq!(m.tab, 2);
}

#[test]
fn a_jump_to_the_active_tab_changes_nothing_and_says_so() {
    let mut m = menu(None, None, Some(machine()));
    press_digit(&mut m, '2');
    press(&mut m, 'j');
    let mut before = m.clone();
    press_digit(&mut m, '2');
    assert_eq!(footer_text(&m), "Already on Models");
    before.notice = m.notice.clone();
    assert_eq!(m, before);
    press_digit(&mut m, '1');
    assert_eq!(m.tab, 0);
    assert_eq!(notice(&m), None);
    press_digit(&mut m, '1');
    assert_eq!(footer_text(&m), "Already on Settings");
}

#[test]
fn four_while_connectors_is_hidden_shows_a_notice_and_changes_nothing() {
    let mut m = menu(None, None, Some(machine()));
    press_digit(&mut m, '2');
    m.goto("code");
    let mut before = m.clone();
    assert_eq!(press_digit(&mut m, '4'), MenuCommand::None);
    assert_eq!(notice(&m).as_deref(), Some("No connectors yet"));
    assert_eq!(footer_text(&m), "No connectors yet");
    assert_eq!(selected_label(&m), "code");
    before.notice = m.notice.clone();
    assert_eq!(m, before);
    // The next press clears it and then acts.
    press(&mut m, 'j');
    assert_eq!(notice(&m), None);
    assert_eq!(selected_label(&m), "chat");
}

#[test]
fn without_the_ai_section_two_to_five_say_it_is_not_available() {
    for digit in ['2', '3', '4', '5'] {
        let mut m = MenuState::open(&Config::default());
        let mut before = m.clone();
        assert_eq!(press_digit(&mut m, digit), MenuCommand::None);
        assert_eq!(footer_text(&m), "No AI section", "digit {digit}");
        before.notice = m.notice.clone();
        assert_eq!(m, before);
    }
}

#[test]
fn zero_and_six_to_nine_say_the_range_and_change_nothing_else() {
    for digit in ['0', '6', '7', '8', '9'] {
        let mut m = menu(None, None, Some(machine()));
        let mut before = m.clone();
        assert_eq!(press_digit(&mut m, digit), MenuCommand::None);
        assert_eq!(footer_text(&m), "Tabs are 1-5", "digit {digit}");
        before.notice = m.notice.clone();
        assert_eq!(m, before);
    }
}

#[test]
fn ctrl_and_alt_digits_are_swallowed_without_a_notice() {
    for mods in [KeyModifiers::CONTROL, KeyModifiers::ALT] {
        for digit in ['0', '2', '4', '9'] {
            let mut m = menu(None, None, Some(machine()));
            let before = m.clone();
            with_mods(&mut m, KeyCode::Char(digit), mods, false);
            assert_eq!(m, before, "{mods:?}+{digit}");
        }
    }
}

#[test]
fn a_notice_is_cleared_by_the_next_press_which_then_acts() {
    for first in ['4', '0'] {
        let mut m = menu(None, None, Some(machine()));
        press_digit(&mut m, first);
        assert!(notice(&m).is_some());
        press_digit(&mut m, '3');
        assert_eq!(notice(&m), None);
        assert_eq!(m.tab, 2, "the second digit jumped");
    }
    let mut m = MenuState::open(&Config::default());
    tab(&mut m);
    assert!(notice(&m).is_some());
    press(&mut m, 'j');
    assert_eq!(notice(&m), None);
}

#[test]
fn the_notice_blocks_neither_save_nor_escape() {
    let mut m = menu(None, None, Some(machine()));
    press_digit(&mut m, '4');
    assert_eq!(press(&mut m, 's'), MenuCommand::Save);
    let mut m = menu(None, None, Some(machine()));
    press_digit(&mut m, '4');
    assert_eq!(send(&mut m, key(KeyCode::Esc)), MenuCommand::Cancel);
}

#[test]
fn an_error_wins_over_a_notice_in_the_footer() {
    let mut m = menu(None, None, Some(machine()));
    press_digit(&mut m, '4');
    m.set_error("boom".to_owned());
    assert_eq!(m.footer(), Footer::Error("boom"));
}

#[test]
fn digits_stay_inert_over_the_warning_and_decline_it() {
    for digit in ['1', '5', '4'] {
        let mut m = warned();
        press_digit(&mut m, digit);
        assert!(!m.in_warning(), "declined");
        assert_eq!(m.tab, 4, "still Chat");
        assert_eq!(notice(&m), None);
    }
}

#[test]
fn the_custom_field_types_digits_and_the_tab_stays() {
    let mut m = custom_field();
    for c in ['a', 'b', '1'] {
        send(&mut m, ch(c));
    }
    assert_eq!(
        m.rows(&Config::default()).last(),
        Some(&Row::Field("ab1".into()))
    );
    assert_eq!(m.active_tab(), 1);
}

#[test]
fn the_no_ai_notice_fits_the_smallest_popup() {
    let mut m = MenuState::open(&Config::default());
    press_digit(&mut m, '2');
    assert!(cells_of(&footer_text(&m)) <= usize::from(MIN_WIDTH) - 4);
}

fn cells_of(text: &str) -> usize {
    text.chars().count()
}

#[test]
fn shift_digits_are_not_jumps_and_follow_the_modified_key_path() {
    let mut m = menu(None, None, Some(machine()));
    let before = m.clone();
    with_mods(&mut m, KeyCode::Char('3'), KeyModifiers::SHIFT, false);
    assert_eq!(m, before, "no jump and no notice");
}

#[test]
fn the_tab_count_matches_the_one_to_five_keys_and_the_hint() {
    assert_eq!(TAB_COUNT, 5);
    assert_eq!(NOTICE_RANGE, format!("Tabs are 1-{TAB_COUNT}"));
}
