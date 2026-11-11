//! Tests of the `Connectors` row and the area it opens (config-menu,
//! Connectors row; mcp-connect, Entry list and the config re-read), at the
//! `MenuState` level.

use std::io;

use super::ai_tests::{menu, press};
use super::picker_tests::{key, send};
use super::*;
use crate::core::ai::{
    config::McpServer,
    mcp_catalog::{
        checks::{Probed, Verdict},
        presence::Presence,
    },
    mcp_secrets::Secrets,
};

pub(super) fn declared(name: &str, enabled: bool) -> McpServer {
    McpServer {
        name: name.to_owned(),
        command: "x".to_owned(),
        args: Vec::new(),
        env: Vec::new(),
        project: None,
        enabled,
    }
}

/// A menu with the AI section and these servers loaded at startup.
pub(super) fn with(servers: Vec<McpServer>) -> MenuState {
    menu(None, None, None).with_connectors(servers, Vec::new())
}

/// The Connectors row selected (its tab is the fourth).
pub(super) fn on_row(servers: Vec<McpServer>) -> MenuState {
    let mut m = with(servers);
    press(&mut m, '4');
    m
}

/// The area open, its answer to `LoadPresence` not yet given.
pub(super) fn area(servers: Vec<McpServer>) -> MenuState {
    let mut m = on_row(servers);
    assert_eq!(send(&mut m, key(KeyCode::Enter)), MenuCommand::LoadPresence);
    m
}

/// What `config.toml` and `secrets.toml` hold, as the runtime reads them.
pub(super) fn presence(config: &str, secrets: &str) -> Presence {
    Presence::read(
        Ok(config.to_owned()),
        &Secrets::parse(secrets.as_bytes()).secrets,
    )
}

pub(super) fn texts(m: &MenuState) -> Vec<String> {
    m.rows(&Config::default())
        .into_iter()
        .map(|row| match row {
            Row::Title(t) | Row::Text(t) => t.into_owned(),
            Row::Item { label, value, .. } => match value {
                Some(value) => format!("{label}: {value}"),
                None => label.to_owned(),
            },
            other => format!("{other:?}"),
        })
        .collect()
}

const BOTH: &str = "[[ai.mcp.servers]]\nname = \"ga4\"\ncommand = \"x\"\n\
                    [[ai.mcp.servers]]\nname = \"clarity\"\ncommand = \"x\"\n";
const CLARITY: &str = "[[ai.mcp.servers]]\nname = \"clarity\"\ncommand = \"x\"\n";

// ---- 4.1 the row ----------------------------------------------------------

// Spec (config-menu, Connectors row): The Connectors tab.
#[test]
fn the_connectors_tab_holds_one_row_with_none() {
    let m = on_row(Vec::new());
    assert_eq!(
        m.tab_labels(),
        ["Settings", "Models", "Tools", "Connectors", "Chat"]
    );
    assert_eq!(texts(&m), ["Connectors: none"]);
}

// Spec: Count of declared entries (gsc does not exist yet: ga4 is disabled).
#[test]
fn the_value_counts_declared_and_enabled_catalog_entries() {
    let servers = vec![
        declared("clarity", true),
        declared("fs", true),
        declared("ga4", false),
    ];
    assert_eq!(texts(&on_row(servers)), ["Connectors: 1 declared"]);
}

// Spec: Opens the area.
#[test]
fn enter_opens_the_area_and_esc_comes_back_to_the_row() {
    let mut m = on_row(Vec::new());
    assert_eq!(send(&mut m, key(KeyCode::Enter)), MenuCommand::LoadPresence);
    assert!(m.in_sub_level());
    assert_eq!(send(&mut m, key(KeyCode::Esc)), MenuCommand::None);
    assert!(!m.in_sub_level());
    assert_eq!(texts(&m), ["Connectors: none"]);
    assert_eq!(m.edits(&Config::default()), Vec::new());
}

// Spec: the row is no draft value: `l`, `h` and Space change nothing.
#[test]
fn cycling_keys_on_the_row_change_nothing() {
    let mut m = on_row(Vec::new());
    for code in ['l', 'h', ' '] {
        assert_eq!(press(&mut m, code), MenuCommand::None);
    }
    assert_eq!(texts(&m), ["Connectors: none"]);
}

// ---- 4.2 the list ---------------------------------------------------------

// Spec (mcp-connect, Entry list): Groups and order.
#[test]
fn the_list_groups_the_entries_and_marks_them_not_declared() {
    let m = area(Vec::new());
    assert_eq!(
        texts(&m),
        ["Analytics", "Clarity: not declared", "GA4: not declared"]
    );
}

#[test]
fn a_disabled_entry_says_so_and_an_enabled_one_is_declared() {
    let m = area(vec![declared("clarity", false), declared("ga4", true)]);
    assert_eq!(texts(&m)[1..], ["Clarity: disabled", "GA4: declared"]);
}

// Spec: Missing values; the list uses the copy read from the disk.
#[test]
fn a_declared_entry_without_its_secret_says_which_values_are_missing() {
    let mut m = area(Vec::new());
    m.presence_reply(presence(
        BOTH,
        "[clarity]\nCLARITY_API_TOKEN = \"sk-123\"\n",
    ));
    assert_eq!(
        texts(&m)[1..3],
        [
            "Clarity: declared",
            "GA4: values missing: GOOGLE_CLOUD_PROJECT"
        ]
    );
}

// Spec: an empty value is a missing one, and no value is ever drawn.
#[test]
fn an_empty_value_counts_as_missing_and_no_value_is_drawn() {
    let mut m = area(Vec::new());
    m.presence_reply(presence(CLARITY, "[clarity]\nCLARITY_API_TOKEN = \"\"\n"));
    assert_eq!(texts(&m)[1], "Clarity: values missing: CLARITY_API_TOKEN");
    m.presence_reply(presence(
        CLARITY,
        "[clarity]\nCLARITY_API_TOKEN = \"sk-123\"\n",
    ));
    assert_eq!(texts(&m)[1], "Clarity: declared");
    assert!(!format!("{m:?}").contains("sk-123"));
    assert!(!texts(&m).concat().contains("sk-123"));
}

// Spec: the last probe result of this session.
#[test]
fn a_probed_entry_shows_the_last_result() {
    let probed = Probed {
        server: "clarity".to_owned(),
        verdict: Verdict::Started,
        lines: vec!["mcp: started clarity (3 tools)".to_owned()],
        problems: Vec::new(),
    };
    let mut m =
        menu(None, None, None).with_connectors(vec![declared("clarity", true)], vec![probed]);
    press(&mut m, '4');
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(texts(&m)[1], "Clarity: declared · probe: started");
    m.probe_reply(Probed {
        server: "clarity".to_owned(),
        verdict: Verdict::Failed,
        lines: Vec::new(),
        problems: Vec::new(),
    });
    assert_eq!(texts(&m)[1], "Clarity: declared · probe: failed");
}

fn selected_label(m: &MenuState) -> Option<&'static str> {
    m.rows(&Config::default())
        .into_iter()
        .find_map(|row| match row {
            Row::Item {
                label,
                selected: true,
                ..
            } => Some(label),
            _ => None,
        })
}

#[test]
fn j_and_k_move_the_cursor_and_wrap() {
    let mut m = area(Vec::new());
    assert_eq!(selected_label(&m), Some("Clarity"));
    press(&mut m, 'j');
    assert_eq!(selected_label(&m), Some("GA4"));
    press(&mut m, 'j');
    assert_eq!(selected_label(&m), Some("Clarity"), "wraps");
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), Some("GA4"));
}

// Spec: a config that cannot be read is said, never a silent fallback.
#[test]
fn a_config_that_cannot_be_read_keeps_the_startup_list_and_says_so() {
    let mut m = area(vec![declared("clarity", true)]);
    m.presence_reply(Presence::read(
        Err(io::Error::from(io::ErrorKind::PermissionDenied)),
        &Secrets::parse(b"[clarity]\nCLARITY_API_TOKEN = \"t\"\n").secrets,
    ));
    assert_eq!(texts(&m)[1], "Clarity: declared");
    assert!(
        texts(&m)
            .iter()
            .any(|line| line.contains("config.toml could not be read"))
    );
}

// No key is swallowed in silence.
#[test]
fn an_unbound_key_in_the_list_shows_a_notice() {
    let mut m = area(Vec::new());
    assert_eq!(press(&mut m, 'z'), MenuCommand::None);
    assert_eq!(m.notice.as_deref(), Some("no such key here"));
}

// ---- 4.4b config re-read (D20) ------------------------------------------------

#[test]
fn a_disk_copy_that_differs_asks_for_a_restart_and_one_that_matches_does_not() {
    let mut same = area(vec![declared("clarity", true)]);
    same.presence_reply(presence(CLARITY, ""));
    assert!(!texts(&same).iter().any(|l| l.contains("restart lazarobox")));

    let mut m = area(Vec::new());
    m.presence_reply(presence(CLARITY, ""));
    assert_eq!(texts(&m)[1], "Clarity: values missing: CLARITY_API_TOKEN");
    assert!(texts(&m).contains(&"restart lazarobox to use it in chats".to_owned()));
}

// Spec: Entry pasted after startup: the entry is declared.
#[test]
fn an_entry_pasted_after_startup_is_declared() {
    let mut m = area(Vec::new());
    m.presence_reply(presence(CLARITY, "[clarity]\nCLARITY_API_TOKEN = \"t\"\n"));
    assert_eq!(texts(&m)[1], "Clarity: declared");
}

#[test]
fn opening_the_area_again_asks_the_disk_again() {
    let mut m = area(Vec::new());
    m.presence_reply(presence(CLARITY, ""));
    send(&mut m, key(KeyCode::Esc));
    assert_eq!(send(&mut m, key(KeyCode::Enter)), MenuCommand::LoadPresence);
    assert_eq!(
        texts(&m)[1],
        "Clarity: not declared",
        "the old copy is dropped"
    );
}
