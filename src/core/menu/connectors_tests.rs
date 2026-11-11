//! Tests of the `Connectors` row and the area it opens (config-menu,
//! Connectors row; mcp-connect, Entry list and Entry screen), at the
//! `MenuState` level.

use std::io;

use super::ai_tests::{menu, press};
use super::footer_tests::hint;
use super::picker_tests::{key, send};
use super::*;
use crate::core::ai::{
    config::McpServer,
    mcp_catalog::{
        checks::{Probed, Verdict},
        presence::Presence,
    },
    mcp_secrets::{Loaded, Secrets, mode_problem},
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
    menu(None, None, None).with_connectors(servers, Vec::new(), None)
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
    Presence::read(Ok(config.to_owned()), &Secrets::parse(secrets.as_bytes()))
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
        menu(None, None, None).with_connectors(vec![declared("clarity", true)], vec![probed], None);
    press(&mut m, '4');
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(texts(&m)[1], "Clarity: declared · probe: started");
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
fn j_and_k_move_the_cursor_and_enter_opens_an_entry() {
    let mut m = area(Vec::new());
    assert_eq!(selected_label(&m), Some("Clarity"));
    press(&mut m, 'j');
    assert_eq!(selected_label(&m), Some("GA4"));
    press(&mut m, 'j');
    assert_eq!(selected_label(&m), Some("Clarity"), "wraps");
    press(&mut m, 'k');
    send(&mut m, key(KeyCode::Enter));
    assert!(texts(&m)[0].starts_with("GA4"));
    send(&mut m, key(KeyCode::Esc));
    assert_eq!(selected_label(&m), Some("GA4"), "Esc goes back to the list");
}

// Spec: a config that cannot be read is said, never a silent fallback.
#[test]
fn a_config_that_cannot_be_read_keeps_the_startup_list_and_says_so() {
    let mut m = area(vec![declared("clarity", true)]);
    m.presence_reply(Presence::read(
        Err(io::Error::from(io::ErrorKind::PermissionDenied)),
        &Secrets::parse(b"[clarity]\nCLARITY_API_TOKEN = \"t\"\n"),
    ));
    assert_eq!(texts(&m)[1], "Clarity: declared");
    assert!(
        texts(&m)
            .iter()
            .any(|line| line.contains("config.toml could not be read"))
    );
}

// ---- 4.3 the entry screen ---------------------------------------------------

/// The area on the entry `at` (0 clarity, 1 ga4), `servers` declared.
fn entry(servers: Vec<McpServer>, at: usize) -> MenuState {
    let mut m = area(servers);
    for _ in 0..at {
        press(&mut m, 'j');
    }
    send(&mut m, key(KeyCode::Enter));
    m
}

/// Every row of the entry screen, whatever the scroll.
fn whole(m: &mut MenuState) -> Vec<String> {
    let mut seen = texts(m);
    for _ in 0..60 {
        press(m, 'j');
        for line in texts(m) {
            if !seen.contains(&line) {
                seen.push(line);
            }
        }
    }
    seen
}

// Spec (Entry screen): the snippet is the catalog's, and parses back.
#[test]
fn the_entry_screen_shows_the_snippet_the_template_and_the_files() {
    let path = std::path::PathBuf::from("/home/ana/.config/lazarobox/config.toml");
    let mut m = menu(None, None, None).with_connectors(Vec::new(), Vec::new(), Some(path));
    press(&mut m, '4');
    send(&mut m, key(KeyCode::Enter));
    press(&mut m, 'j');
    send(&mut m, key(KeyCode::Enter));
    let all = whole(&mut m).join("\n");
    assert!(
        all.contains("[[ai.mcp.servers]]\n  name = \"ga4\""),
        "{all}"
    );
    assert!(all.contains("GOOGLE_CLOUD_PROJECT = \"\""));
    assert!(all.contains("config.toml: /home/ana/.config/lazarobox/config.toml"));
    assert!(all.contains("secrets.toml: /home/ana/.config/lazarobox/secrets.toml"));
    assert!(all.contains("7 days"), "the Testing warning of C7");
    // The lines are the catalog's own snippet, which parses back (mcp-catalog).
    let entry = crate::core::ai::mcp_catalog::entry("ga4").unwrap();
    for line in crate::core::ai::mcp_catalog::snippet(entry).lines() {
        assert!(all.contains(&format!("  {line}")), "{line}");
    }
}

// Spec: Probe only when declared.
#[test]
fn the_probe_is_offered_only_when_declared() {
    let mut m = entry(Vec::new(), 0);
    let all = whole(&mut m);
    assert!(
        all.iter()
            .any(|l| l.contains("paste the entry into config.toml first"))
    );
    assert!(!all.iter().any(|l| l == "  p probe"));
    assert_eq!(press(&mut m, 'p'), MenuCommand::None);
    assert!(m.notice.is_some(), "a visible notice");
    assert!(!format!("{:?}", m.rows(&Config::default())).contains("probe clarity?"));
    let mut declared = entry(vec![declared("clarity", true)], 0);
    assert!(whole(&mut declared).iter().any(|l| l == "  p probe"));
}

// Spec: Values never drawn.
#[test]
fn a_secret_value_is_never_drawn_and_shows_as_set() {
    let mut m = entry(vec![declared("clarity", true)], 0);
    m.presence_reply(presence(
        CLARITY,
        "[clarity]\nCLARITY_API_TOKEN = \"sk-123\"\n",
    ));
    let all = whole(&mut m).join("\n");
    assert!(all.contains("CLARITY_API_TOKEN: set"), "{all}");
    assert!(!all.contains("sk-123"));
    assert!(!format!("{m:?}").contains("sk-123"));
}

// Spec: the guide links open through the opener, or are copied without one.
#[test]
fn a_link_key_opens_the_link_and_copies_it_without_an_opener() {
    let mut m = entry(Vec::new(), 1).with_opener(true);
    assert_eq!(
        press(&mut m, '1'),
        MenuCommand::Open("https://console.cloud.google.com/apis/credentials".to_owned())
    );
    let mut m = entry(Vec::new(), 1);
    assert!(matches!(press(&mut m, '2'), MenuCommand::Copy(_)));
    assert_eq!(press(&mut m, '9'), MenuCommand::None);
    assert!(m.notice.is_some());
}

#[test]
fn c_and_t_copy_the_entry_and_the_template() {
    let mut m = entry(Vec::new(), 0);
    let MenuCommand::Copy(snippet) = press(&mut m, 'c') else {
        panic!("copies");
    };
    assert!(snippet.starts_with("[[ai.mcp.servers]]\nname = \"clarity\""));
    assert_eq!(
        press(&mut m, 't'),
        MenuCommand::Copy("[clarity]\nCLARITY_API_TOKEN = \"\"\n".to_owned())
    );
}

#[test]
fn a_long_screen_scrolls_and_never_grows_past_the_body() {
    let mut m = entry(Vec::new(), 1);
    assert_eq!(m.rows(&Config::default()).len(), VIEW_BODY);
    let first = texts(&m);
    for _ in 0..80 {
        press(&mut m, 'j');
        assert!(m.rows(&Config::default()).len() <= VIEW_BODY);
    }
    assert_ne!(texts(&m), first);
    for _ in 0..80 {
        press(&mut m, 'k');
    }
    assert_eq!(texts(&m), first, "k comes back to the top at once");
}

// ---- 4.4 prompts -------------------------------------------------------------

// Spec: Nothing without y, for every key but `y` (C11).
#[test]
fn only_y_after_the_prompt_runs_anything() {
    let keys: Vec<KeyCode> = (0x20u8..0x7f)
        .map(|b| KeyCode::Char(b as char))
        .chain([
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Tab,
            KeyCode::Backspace,
        ])
        .filter(|k| *k != KeyCode::Char('y'))
        .collect();
    for ask in ['i', 'l', 'p'] {
        for k in &keys {
            let mut m = entry(vec![declared("ga4", true)], 1);
            press(&mut m, ask);
            assert_eq!(send(&mut m, key(*k)), MenuCommand::None, "{ask} then {k:?}");
            // The prompt is gone: the same key again is an ordinary key.
            let again = send(&mut m, key(*k));
            assert!(
                !matches!(again, MenuCommand::Connector(_) | MenuCommand::Probe(_)),
                "{ask} {k:?} then {k:?}"
            );
        }
    }
}

// Spec: no key of the area, without a prompt, runs a connector or a probe.
#[test]
fn no_key_runs_a_connector_or_a_probe_on_its_own() {
    let mut m = entry(vec![declared("ga4", true)], 1);
    for b in 0x20u8..0x7f {
        let c = b as char;
        if matches!(c, 'i' | 'l' | 'p') {
            continue;
        }
        let command = press(&mut m, c);
        assert!(
            !matches!(command, MenuCommand::Connector(_) | MenuCommand::Probe(_)),
            "{c}"
        );
    }
}

// Spec: Visible command; the prompt shows the full line and sends Enter's line.
#[test]
fn the_prompt_shows_the_full_command_and_y_sends_that_line() {
    let mut m = entry(Vec::new(), 1);
    press(&mut m, 'l');
    let shown: String = texts(&m).concat().replace(' ', "");
    let login = crate::core::ai::mcp_catalog::entry("ga4")
        .unwrap()
        .login
        .unwrap()
        .line;
    assert!(shown.contains(&login.replace(' ', "")), "{shown}");
    assert!(hint(&m).contains("y yes"));
    assert_eq!(press(&mut m, 'y'), MenuCommand::Connector(login.to_owned()));
    assert!(texts(&m)[0].starts_with("GA4"), "no second prompt");
}

#[test]
fn install_asks_with_its_own_line_and_login_needs_a_login() {
    let mut m = entry(Vec::new(), 0);
    assert_eq!(press(&mut m, 'l'), MenuCommand::None);
    assert!(m.notice.is_some(), "clarity has no login");
    press(&mut m, 'i');
    assert!(texts(&m)[0].starts_with("install clarity? (y/n)"));
    assert_eq!(
        press(&mut m, 'y'),
        MenuCommand::Connector("npm i -g @microsoft/clarity-mcp-server".to_owned())
    );
}

// Spec: the prompt says unsaved menu changes are discarded when dirty.
#[test]
fn a_dirty_draft_is_named_in_the_prompt_and_a_clean_one_is_not() {
    let mut clean = entry(Vec::new(), 0);
    press(&mut clean, 'i');
    assert!(!texts(&clean).concat().contains("Unsaved menu changes"));

    let mut config = Config::default();
    let mut m = with(Vec::new());
    let step = |m: &mut MenuState, config: &mut Config, code| {
        m.on_key(&KeyEvent::new(code, KeyModifiers::NONE), false, config)
    };
    m.goto("Mouse");
    step(&mut m, &mut config, KeyCode::Char('l'));
    assert!(!m.edits(&config).is_empty());
    m.goto("Connectors");
    step(&mut m, &mut config, KeyCode::Enter);
    step(&mut m, &mut config, KeyCode::Enter);
    step(&mut m, &mut config, KeyCode::Char('i'));
    assert!(
        texts(&m)
            .concat()
            .contains("Unsaved menu changes are discarded.")
    );
}

fn key_with(m: &mut MenuState, code: KeyCode, mods: KeyModifiers) -> MenuCommand {
    m.on_key(&KeyEvent::new(code, mods), false, &mut Config::default())
}

// Only a plain `y` confirms: Ctrl+y and Alt+y are other keys.
#[test]
fn ctrl_y_and_alt_y_confirm_nothing() {
    for ask in ['i', 'l', 'p'] {
        for mods in [
            KeyModifiers::CONTROL,
            KeyModifiers::ALT,
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            KeyModifiers::SUPER,
        ] {
            let mut m = entry(vec![declared("ga4", true)], 1);
            press(&mut m, ask);
            let command = key_with(&mut m, KeyCode::Char('y'), mods);
            assert_eq!(command, MenuCommand::None, "{ask} {mods:?}");
            assert_eq!(m.notice.as_deref(), Some("cancelled"));
            // The prompt is gone: a plain `y` now is an ordinary key.
            assert_eq!(press(&mut m, 'y'), MenuCommand::None);
        }
    }
}

// A key that closes a prompt says so.
#[test]
fn any_other_key_cancels_with_a_visible_notice() {
    let mut m = entry(Vec::new(), 0);
    press(&mut m, 'i');
    assert_eq!(press(&mut m, 'n'), MenuCommand::None);
    assert_eq!(m.notice.as_deref(), Some("cancelled"));
    assert!(texts(&m)[0].starts_with("State:") || texts(&m)[0].starts_with("Clarity"));
}

/// The command rows of the open prompt: the text rows indented by two spaces,
/// without the leading space the renderer adds.
fn command_rows(m: &MenuState) -> String {
    m.rows(&Config::default())
        .into_iter()
        .skip(2)
        .filter_map(|row| match row {
            Row::Text(t) => t.strip_prefix("  ").map(str::to_owned),
            _ => None,
        })
        .collect()
}

// Every catalog install and login line is shown whole, spaces kept.
#[test]
fn every_catalog_line_renders_verbatim_in_its_prompt() {
    for cols in [62u16, 80, 40] {
        for (at, entry_) in crate::core::ai::mcp_catalog::entries().iter().enumerate() {
            let lines = [
                Some(('i', entry_.install)),
                entry_.login.map(|l| ('l', l.line)),
            ];
            for (ask, line) in lines.into_iter().flatten() {
                let mut m = entry(Vec::new(), at);
                m.set_term(cols, 40);
                press(&mut m, ask);
                let shown = m.rows(&Config::default());
                assert!(shown.len() <= VIEW_BODY, "{cols} {line}");
                assert_eq!(command_rows(&m), line, "{cols} {line}");
                assert_eq!(
                    press(&mut m, 'y'),
                    MenuCommand::Connector(line.to_owned()),
                    "{cols}"
                );
            }
        }
    }
}

// Spec (Install and login): a command that cannot be read whole is not asked.
#[test]
fn a_narrow_terminal_refuses_the_prompt_with_a_notice() {
    let mut m = entry(Vec::new(), 1);
    m.set_term(20, 40);
    assert_eq!(press(&mut m, 'l'), MenuCommand::None);
    assert_eq!(
        m.notice.as_deref(),
        Some("widen the terminal to read the command")
    );
    assert!(!texts(&m).iter().any(|l| l.contains("(y/n)")));
    // Asked while wide, then the terminal shrinks: `y` runs nothing.
    let mut m = entry(Vec::new(), 1);
    press(&mut m, 'l');
    m.set_term(20, 40);
    assert_eq!(texts(&m), ["widen the terminal to read the command"]);
    assert_eq!(press(&mut m, 'y'), MenuCommand::None);
    assert_eq!(
        m.notice.as_deref(),
        Some("widen the terminal to read the command")
    );
}

// A command whose rows would not fit the body is refused, never clipped.
#[test]
fn a_command_taller_than_the_body_is_refused() {
    let mut server = declared("clarity", true);
    server.args = vec!["x".repeat(900)];
    let mut m = entry(vec![server], 0);
    press(&mut m, 'p');
    assert_eq!(
        m.notice.as_deref(),
        Some("widen the terminal to read the command")
    );
    assert!(!texts(&m).iter().any(|l| l.contains("(y/n)")));
}

// The probe prompt names what it will spawn.
#[test]
fn the_probe_prompt_shows_the_command_and_its_arguments() {
    let mut server = declared("clarity", true);
    server.command = "clarity-mcp-server".to_owned();
    server.args = vec!["--flag".to_owned(), "a b".to_owned()];
    let mut m = entry(vec![server], 0);
    press(&mut m, 'p');
    assert_eq!(texts(&m)[0], "probe clarity? (y/n)");
    assert_eq!(command_rows(&m), "clarity-mcp-server --flag a b");
}

// Spec (Probe from the area): `probe <name>? (y/n)`, then footer and Esc.
#[test]
fn the_probe_asks_then_runs_then_esc_stops_it() {
    let mut m = entry(vec![declared("clarity", true)], 0);
    press(&mut m, 'p');
    assert_eq!(texts(&m)[0], "probe clarity? (y/n)");
    let MenuCommand::Probe(server) = press(&mut m, 'y') else {
        panic!("probes");
    };
    assert_eq!(server.name, "clarity");
    assert_eq!(hint(&m), "probing clarity… · Esc stops it");
    assert_eq!(press(&mut m, 'p'), MenuCommand::None);
    assert_eq!(m.notice.as_deref(), Some("a probe is already running"));
    assert_eq!(send(&mut m, key(KeyCode::Esc)), MenuCommand::CancelProbe);
    m.probe_reply(Probed {
        server: "clarity".to_owned(),
        verdict: Verdict::Stopped,
        lines: vec!["mcp: clarity probe stopped".to_owned()],
        problems: Vec::new(),
    });
    assert!(!hint(&m).starts_with("probing"));
    assert!(whole(&mut m).iter().any(|l| l.contains("probe stopped")));
}

// Spec (Probe from the area): the report lines and the problems are shown.
#[test]
fn the_report_and_its_problems_are_shown_on_the_entry_screen() {
    let mut m = entry(vec![declared("clarity", true)], 0);
    m.probe_reply(Probed {
        server: "clarity".to_owned(),
        verdict: Verdict::Started,
        lines: vec!["mcp: started clarity (3 tools)".to_owned()],
        problems: vec!["secrets.toml: [clarity]: expected a table".to_owned()],
    });
    let all = whole(&mut m);
    assert!(all.iter().any(|l| l.contains("started clarity (3 tools)")));
    assert!(all.iter().any(|l| l.contains("expected a table")));
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
    send(&mut m, key(KeyCode::Enter));
    assert!(whole(&mut m).contains(&"restart lazarobox to use it in chats".to_owned()));
}

// Spec: Entry pasted after startup: declared, probe offered.
#[test]
fn an_entry_pasted_after_startup_is_declared_and_can_be_probed() {
    let mut m = area(Vec::new());
    m.presence_reply(presence(CLARITY, "[clarity]\nCLARITY_API_TOKEN = \"t\"\n"));
    assert_eq!(texts(&m)[1], "Clarity: declared");
    send(&mut m, key(KeyCode::Enter));
    press(&mut m, 'p');
    assert_eq!(texts(&m)[0], "probe clarity? (y/n)");
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

// No key is swallowed in silence.
#[test]
fn an_unbound_key_in_the_list_shows_a_notice() {
    let mut m = area(Vec::new());
    assert_eq!(press(&mut m, 'z'), MenuCommand::None);
    assert_eq!(m.notice.as_deref(), Some("no such key here"));
}

// A reordered file is the same list: no false restart. A changed entry is not.
#[test]
fn reordering_the_entries_asks_for_no_restart_and_changing_one_does() {
    let startup = vec![declared("clarity", true), declared("ga4", true)];
    let mut m = area(startup.clone());
    let reordered = "[[ai.mcp.servers]]\nname = \"ga4\"\ncommand = \"x\"\n\
                     [[ai.mcp.servers]]\nname = \"clarity\"\ncommand = \"x\"\n";
    m.presence_reply(presence(reordered, ""));
    assert!(!texts(&m).iter().any(|l| l.contains("restart lazarobox")));
    let changed = reordered.replacen("command = \"x\"", "command = \"y\"", 1);
    m.presence_reply(presence(&changed, ""));
    assert!(texts(&m).iter().any(|l| l.contains("restart lazarobox")));
}

// Spec (visible notices): `values missing` says why when the file is not used.
#[test]
fn a_secrets_file_that_is_not_used_is_named_next_to_the_missing_values() {
    let mut m = area(Vec::new());
    let loaded = Loaded::problem(mode_problem(std::path::Path::new("/h/secrets.toml")));
    m.presence_reply(Presence::read(Ok(CLARITY.to_owned()), &loaded));
    assert_eq!(texts(&m)[1], "Clarity: values missing: CLARITY_API_TOKEN");
    let all = texts(&m).join(" ");
    assert!(all.contains("secrets.toml: mode must be 0600"), "{all}");
    assert!(all.contains("chmod 600 /h/secrets.toml"), "{all}");
    assert!(!format!("{m:?}").contains("sk-"));
}
