//! Tests of the five family toggle rows of the AI section, at the
//! `MenuState` level.

use super::ai_tests::{ch, machine, press, selected_label};
use super::*;
use crate::core::ai::config::ToolToggle;

const ROWS: [(&str, ToolToggle); 5] = [
    ("Search", ToolToggle::Search),
    ("Write", ToolToggle::Write),
    ("Exec", ToolToggle::Exec),
    ("Git", ToolToggle::Git),
    ("Git write", ToolToggle::GitWrite),
];

fn open() -> MenuState {
    MenuState::open(&Config::default()).with_ai(AiConfig::default(), Some(machine()))
}

fn on(label: &str) -> MenuState {
    let mut m = open();
    m.goto(label);
    m
}

fn send(menu: &mut MenuState, code: KeyCode, repeat: bool) -> MenuCommand {
    let key = KeyEvent::new(code, KeyModifiers::NONE);
    menu.on_key(&key, repeat, &mut Config::default())
}

fn value_of(menu: &MenuState, wanted: &str) -> Option<String> {
    menu.all_rows(&Config::default())
        .into_iter()
        .find_map(|row| match row {
            Row::Item {
                label,
                value: Some(value),
                ..
            } if label == wanted => Some(value.into_owned()),
            _ => None,
        })
}

// Spec (config-menu): five static rows between Tools and Approval, each with
// its own kind and a distinct key path.
#[test]
fn the_five_rows_sit_between_tools_and_approval_in_order() {
    let labels: Vec<_> = TOOLS_SECTION.items.iter().map(|i| i.label).collect();
    let tools = labels.iter().position(|l| *l == "Tools").unwrap();
    let rows: Vec<_> = ROWS.iter().map(|(label, _)| *label).collect();
    assert_eq!(labels[tools + 1..tools + 6], rows[..]);
    assert_eq!(labels[tools + 6], "Approval");
}

#[test]
fn each_row_is_found_by_its_own_kind_and_key_paths_are_distinct() {
    let mut paths = Vec::new();
    for (label, toggle) in ROWS {
        let item = TOOLS_SECTION
            .items
            .iter()
            .find(|i| i.label == label)
            .unwrap();
        assert!(
            matches!(item.kind, ItemKind::Family(t) if t == toggle),
            "{label}"
        );
        paths.push(format!("ai.tools.{}", toggle.key()));
    }
    paths.sort();
    paths.dedup();
    assert_eq!(paths.len(), 5);
}

#[test]
fn navigation_goes_tools_then_the_five_rows_then_approval() {
    let mut m = on("Tools");
    for (label, _) in ROWS {
        press(&mut m, 'j');
        assert_eq!(selected_label(&m), label);
    }
    press(&mut m, 'j');
    assert_eq!(selected_label(&m), "Approval");
}

#[test]
fn every_row_reads_off_by_default_and_needs_the_ai_section() {
    let m = open();
    for (label, _) in ROWS {
        assert_eq!(value_of(&m, label).as_deref(), Some("off"), "{label}");
    }
    let plain = MenuState::open(&Config::default());
    assert_eq!(value_of(&plain, "Write"), None);
}

#[test]
fn every_cycle_key_toggles_the_draft_and_emits_nothing() {
    for (label, toggle) in ROWS {
        for code in [
            KeyCode::Char('l'),
            KeyCode::Right,
            KeyCode::Char(' '),
            KeyCode::Enter,
            KeyCode::Char('h'),
            KeyCode::Left,
        ] {
            let mut m = on(label);
            assert_eq!(send(&mut m, code, false), MenuCommand::None, "{code:?}");
            assert!(m.ai_draft().toggle(toggle), "{label} {code:?}");
            assert_eq!(value_of(&m, label).as_deref(), Some("on"));
            send(&mut m, code, false);
            assert!(!m.ai_draft().toggle(toggle), "{label} back {code:?}");
        }
    }
}

#[test]
fn toggling_one_row_leaves_the_others_and_the_live_config_alone() {
    let mut m = on("Write");
    let mut config = Config::default();
    m.on_key(&ch('l'), false, &mut config);
    assert_eq!(config, Config::default());
    for toggle in ToolToggle::ALL {
        assert_eq!(m.ai_draft().toggle(toggle), toggle == ToolToggle::Write);
    }
    assert!(m.ai_draft().tools_enabled());
}

#[test]
fn an_enter_repeat_is_ignored() {
    let mut m = on("Exec");
    assert_eq!(send(&mut m, KeyCode::Enter, true), MenuCommand::None);
    assert!(!m.ai_draft().toggle(ToolToggle::Exec));
}

// Spec (config-persistence): only the keys that differ are saved.
#[test]
fn only_a_changed_toggle_is_an_edit_and_it_is_that_key_alone() {
    let live = Config::default();
    let mut m = on("Git write");
    assert_eq!(m.edits(&live), vec![]);
    press(&mut m, 'l');
    assert_eq!(
        m.edits(&live),
        vec![ConfigEdit::toggle(ToolToggle::GitWrite, true)]
    );
    press(&mut m, 'l');
    assert_eq!(m.edits(&live), vec![], "back to the open value");
}

#[test]
fn several_changed_toggles_are_edits_in_row_order_before_approval() {
    let live = Config::default();
    let mut m = on("Exec");
    press(&mut m, 'l');
    m.goto("Search");
    press(&mut m, 'l');
    m.goto("Approval");
    press(&mut m, 'l');
    let keys: Vec<_> = m.edits(&live).iter().map(|e| e.key).collect();
    assert_eq!(keys, ["search", "exec", "approval"]);
}

// Spec (config-menu): Write, Exec and GitWrite show a one-line caution.
#[test]
fn only_the_rows_that_change_files_or_run_code_carry_a_caution() {
    for (label, toggle) in ROWS {
        let m = on(label);
        let risky = matches!(
            toggle,
            ToolToggle::Write | ToolToggle::Exec | ToolToggle::GitWrite
        );
        assert_eq!(m.caution().is_some(), risky, "{label}");
    }
    assert_eq!(on("Tools").caution(), None);
    assert_eq!(on("Approval").caution(), None);
    let line = on("Write").caution().unwrap();
    assert!(!line.contains('\n') && line.len() <= 44, "{line}");
}

// Design D18: the Exec row says why commands will be refused when the system
// has no Landlock jail. The reason is a short note next to the value.
fn exec_on(sandbox: Option<bool>) -> MenuState {
    let mut m = open();
    if let Some(sandbox) = sandbox {
        m = m.with_sandbox(sandbox);
    }
    m.goto("Exec");
    send(&mut m, KeyCode::Char(' '), false);
    m
}

#[test]
fn the_exec_row_says_it_needs_the_linux_sandbox_when_there_is_no_jail() {
    let m = exec_on(Some(false));
    assert_eq!(
        value_of(&m, "Exec").as_deref(),
        Some("on · needs Linux sandbox")
    );
}

#[test]
fn the_exec_row_is_plain_with_a_jail_or_before_the_system_is_known() {
    for sandbox in [Some(true), None] {
        assert_eq!(value_of(&exec_on(sandbox), "Exec").as_deref(), Some("on"));
    }
}

#[test]
fn the_reason_shows_only_while_exec_is_on_and_only_on_its_row() {
    let mut m = exec_on(Some(false));
    send(&mut m, KeyCode::Char(' '), false);
    assert_eq!(value_of(&m, "Exec").as_deref(), Some("off"));
    assert_eq!(value_of(&m, "Write").as_deref(), Some("off"));
    m.goto("Write");
    send(&mut m, KeyCode::Char(' '), false);
    assert_eq!(value_of(&m, "Write").as_deref(), Some("on"));
}
