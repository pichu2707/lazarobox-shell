//! Tests of the `Usage (alpha)` row of Settings and its view (ai-cost,
//! config-menu "Usage row and view"): the two toggles and the billing mode of
//! the three CLI engines. The limits are in `usage_limit_tests`.

use super::account_tests::SAVED;
use super::ai_tests::{machine, menu, press, selected_label};
use super::picker_tests::{key, send};
use super::*;
use crate::core::ai::hf::{AccountAsk, AccountReply, REJECTED};
use crate::core::config_edit::{EditError, apply_edit};

const ROW: &str = "Usage (alpha)";

fn on_usage() -> MenuState {
    let mut m = menu(None, None, Some(machine()));
    m.goto(ROW);
    m
}

/// The view opened over the row.
fn opened() -> MenuState {
    let mut m = on_usage();
    send(&mut m, key(KeyCode::Enter));
    m
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

fn edit(table: &'static str, key: &'static str, value: &'static str) -> ConfigEdit {
    ConfigEdit {
        table,
        key,
        value: value.into(),
    }
}

fn hint(menu: &MenuState) -> String {
    match menu.footer() {
        Footer::Hint(text) => text.into_owned(),
        other => panic!("expected a hint, got {other:?}"),
    }
}

#[test]
fn the_row_follows_the_idle_row_in_settings_and_summarises_the_toggles() {
    let m = on_usage();
    let labels: Vec<_> = SECTIONS[0].items.iter().map(|i| i.label).collect();
    let at = labels.iter().position(|l| *l == ROW).unwrap();
    assert_eq!(
        (labels[at - 1], labels[at + 1]),
        ("llama-server idle", "Updates")
    );
    assert_eq!(selected_label(&m), ROW);
    assert_eq!(value_of(&m, ROW), "line on · bar on");
}

#[test]
fn a_menu_without_the_ai_section_has_no_usage_row() {
    let m = MenuState::open(&Config::default());
    assert!(
        !m.rows(&Config::default())
            .iter()
            .any(|row| matches!(row, Row::Item { label, .. } if *label == ROW))
    );
}

#[test]
fn enter_l_and_right_open_the_view_and_esc_returns_with_the_draft() {
    for open in [
        KeyCode::Enter,
        KeyCode::Char('l'),
        KeyCode::Right,
        KeyCode::Char(' '),
        KeyCode::Char('h'),
        KeyCode::Left,
    ] {
        let mut m = on_usage();
        assert_eq!(send(&mut m, key(open)), MenuCommand::None);
        assert!(m.in_sub_level(), "{open:?}");
        press(&mut m, 'l');
        assert_eq!(send(&mut m, key(KeyCode::Esc)), MenuCommand::None);
        assert!(!m.in_sub_level());
        assert_eq!(selected_label(&m), ROW);
        assert_eq!(value_of(&m, ROW), "line off · bar on", "{open:?}");
    }
}

#[test]
fn an_enter_repeat_does_not_open_the_view() {
    let mut m = on_usage();
    m.on_key(&key(KeyCode::Enter), true, &mut Config::default());
    assert!(!m.in_sub_level());
}

#[test]
fn the_view_lists_the_toggles_then_one_billing_row_per_cli_engine() {
    let m = opened();
    let labels: Vec<_> = m
        .rows(&Config::default())
        .into_iter()
        .filter_map(|row| match row {
            Row::Item { label, value, .. } => Some((label, value.unwrap().into_owned())),
            _ => None,
        })
        .collect();
    assert_eq!(
        labels,
        [
            ("Usage line", "on".to_owned()),
            ("Usage in bar", "on".to_owned()),
            ("claude billing", "subscription".to_owned()),
            ("codex billing", "subscription".to_owned()),
            ("gemini billing", "subscription".to_owned()),
            ("Turn limit (tokens)", "off".to_owned()),
            ("Day limit (tokens)", "off".to_owned()),
            ("Day limit ($)", "off".to_owned()),
        ]
    );
}

#[test]
fn a_toggle_cycles_off_and_on_without_a_save() {
    let mut m = opened();
    for expected in ["off", "on"] {
        assert_eq!(press(&mut m, 'l'), MenuCommand::None);
        assert_eq!(value_of(&m, "Usage line"), expected);
    }
}

#[test]
fn billing_cycles_both_ways_and_there_is_no_local_engine_row() {
    let mut m = opened();
    press(&mut m, 'j');
    press(&mut m, 'j');
    press(&mut m, 'l');
    assert_eq!(value_of(&m, "claude billing"), "per-token");
    press(&mut m, 'h');
    assert_eq!(value_of(&m, "claude billing"), "subscription");
    let rows = m.rows(&Config::default());
    assert!(
        !rows
            .iter()
            .any(|r| matches!(r, Row::Item { label, .. } if label.contains("ollama")))
    );
}

#[test]
fn enter_and_space_cycle_but_an_enter_repeat_is_ignored() {
    let mut m = opened();
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(value_of(&m, "Usage line"), "off");
    m.on_key(&key(KeyCode::Enter), true, &mut Config::default());
    assert_eq!(value_of(&m, "Usage line"), "off");
    press(&mut m, ' ');
    assert_eq!(value_of(&m, "Usage line"), "on");
}

#[test]
fn the_cursor_wraps_in_the_view() {
    let mut m = opened();
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "Day limit ($)");
    press(&mut m, 'j');
    assert_eq!(selected_label(&m), "Usage line");
}

#[test]
fn save_writes_only_the_changed_keys_in_rank_order() {
    let mut m = opened();
    press(&mut m, 'l');
    for _ in 0..2 {
        press(&mut m, 'j');
    }
    press(&mut m, 'l');
    assert_eq!(
        m.edits(&Config::default()),
        [
            edit("ai.usage", "line", "false"),
            edit("ai.usage.billing", "claude", "per-token"),
        ]
    );
}

#[test]
fn an_untouched_view_writes_nothing() {
    assert_eq!(opened().edits(&Config::default()), vec![]);
}

#[test]
fn s_inside_the_view_saves_instead_of_being_ignored() {
    let mut m = opened();
    assert_eq!(press(&mut m, 's'), MenuCommand::Save);
}

#[test]
fn esc_at_the_top_level_cancels_the_menu_and_the_view_never_does() {
    let mut m = opened();
    press(&mut m, 'l');
    assert_eq!(send(&mut m, key(KeyCode::Esc)), MenuCommand::None);
    assert_eq!(send(&mut m, key(KeyCode::Esc)), MenuCommand::Cancel);
}

#[test]
fn the_footer_names_the_keys_of_the_view() {
    assert_eq!(
        hint(&opened()),
        "j/k move · h/l change · Enter edit · s save · Esc back"
    );
}

#[test]
fn the_tab_keys_are_inert_in_the_view() {
    let mut m = opened();
    send(&mut m, key(KeyCode::Tab));
    press(&mut m, '3');
    assert!(m.in_sub_level());
}

#[test]
fn the_usage_keys_are_written_as_toml_booleans_and_strings() {
    let write = |e: ConfigEdit, text: &str| apply_edit(Some(text), &[e]).unwrap();
    let text = write(edit("ai.usage", "statusline", "false"), "");
    assert!(text.contains("statusline = false"), "{text}");
    let text = write(edit("ai.usage.billing", "gemini", "per-token"), &text);
    assert!(text.contains("gemini = \"per-token\""), "{text}");
}

#[test]
fn the_view_rows_have_unique_key_paths_and_ranks_apart_from_the_tabs() {
    let usage: Vec<_> = USAGE_SECTION
        .items
        .iter()
        .map(|item| match &item.kind {
            ItemKind::Draft(c) => (c.table(), c.key(), c.rank),
            _ => panic!("{} is not a draft", item.label),
        })
        .collect();
    assert_eq!(
        usage.iter().map(|(t, k, _)| (*t, *k)).collect::<Vec<_>>(),
        [
            ("ai.usage", "line"),
            ("ai.usage", "statusline"),
            ("ai.usage.billing", "claude"),
            ("ai.usage.billing", "codex"),
            ("ai.usage.billing", "gemini"),
            ("ai.usage", "turn_tokens"),
            ("ai.usage", "day_tokens"),
            ("ai.usage", "day_usd"),
        ]
    );
    let others: Vec<u8> = SECTIONS
        .iter()
        .chain([&MODELS_SECTION, &TOOLS_SECTION, &CHAT_SECTION])
        .flat_map(|s| s.items.iter())
        .filter_map(|item| match &item.kind {
            ItemKind::Draft(c) => Some(c.rank),
            _ => None,
        })
        .collect();
    let mut ranks: Vec<u8> = others
        .into_iter()
        .chain(usage.iter().map(|u| u.2))
        .collect();
    let count = ranks.len();
    ranks.sort_unstable();
    ranks.dedup();
    assert_eq!(ranks.len(), count);
}

#[test]
fn a_key_the_view_does_not_use_says_so_and_changes_nothing() {
    for code in [
        KeyCode::Tab,
        KeyCode::BackTab,
        KeyCode::Char('3'),
        KeyCode::Char('x'),
    ] {
        let mut m = opened();
        let tab = m.tab;
        assert_eq!(send(&mut m, key(code)), MenuCommand::None);
        assert!(m.in_sub_level(), "{code:?}");
        assert_eq!(m.tab, tab, "{code:?}");
        assert_eq!(hint(&m), "Esc first to leave Usage", "{code:?}");
        // The next press clears the notice.
        press(&mut m, 'j');
        assert_eq!(
            hint(&m),
            "j/k move · h/l change · Enter edit · s save · Esc back"
        );
    }
}

#[test]
fn a_refused_token_waits_for_the_usage_view_to_close() {
    let mut m = MenuState::open(&Config::default())
        .with_ai(AiConfig::default(), Some(machine()))
        .with_hf(SAVED, None);
    m.goto("Hugging Face");
    assert_eq!(
        send(&mut m, key(KeyCode::Enter)),
        MenuCommand::HfAccount(AccountAsk::Check)
    );
    m.goto(ROW);
    send(&mut m, key(KeyCode::Enter));
    m.account_reply(AccountReply::Checked(Err(REJECTED.to_owned())));
    assert_eq!(m.sub_view(), Some(SubView::Usage), "the view stays");
    assert!(m.account.field().is_none(), "the field did not open");
    send(&mut m, key(KeyCode::Esc));
    assert_eq!(m.sub_view(), Some(SubView::AccountField), "now it opens");
}

#[test]
fn a_billing_key_that_is_not_a_table_refuses_the_save() {
    let refused = apply_edit(
        Some("[ai.usage]\nbilling = 1\n"),
        &[edit("ai.usage.billing", "claude", "per-token")],
    );
    assert_eq!(refused, Err(EditError::NotATable("ai.usage.billing")));
}
