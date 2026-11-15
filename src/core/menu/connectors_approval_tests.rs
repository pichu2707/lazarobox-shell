//! The approval level in the Connectors area (mcp-connect, Approval level of
//! an entry; config-menu, Connector approval level), at the `MenuState`
//! level: what the screen shows, the proposal and its `y`, the reply.

use super::{
    ai_tests::press,
    connectors_tests::{area, entry, presence, texts, whole},
    picker_tests::{key, send},
    *,
};
use crate::core::{
    ai::{
        config::McpApproval,
        mcp_catalog::{self, Class},
    },
    config_append::digest,
};

/// `config.toml` declaring `ga4` with the catalog's own command, plus `extra`
/// lines inside the entry.
fn ga4(extra: &str) -> String {
    format!(
        "[[ai.mcp.servers]]\nname = \"ga4\"\ncommand = \"{}\"\n{extra}",
        mcp_catalog::entry("ga4").unwrap().command
    )
}

/// The GA4 screen (entry 1) of a menu whose disk copy is `config`.
fn screen(config: &str) -> MenuState {
    let mut m = area(Vec::new());
    m.presence_reply(presence(config, ""));
    press(&mut m, 'j');
    press(&mut m, 'j');
    send(&mut m, key(KeyCode::Enter));
    m
}

/// Every row of the entry screen, read from its top.
fn said(m: &mut MenuState) -> String {
    for _ in 0..60 {
        press(m, 'k');
    }
    whole(m).join(" ")
}

// Spec: The level is shown with what it lets run
#[test]
fn the_entry_screen_shows_the_level_and_the_read_tools_that_run_unasked() {
    let mut m = screen(&ga4("approval = \"reads\"\n"));
    let all = said(&mut m);
    assert!(all.contains("approval: reads"), "{all}");
    let reads: Vec<&str> = mcp_catalog::entry("ga4")
        .unwrap()
        .tools
        .iter()
        .filter(|(_, class)| *class == Class::Read)
        .map(|(name, _)| *name)
        .collect();
    assert!(!reads.is_empty());
    for name in reads {
        assert!(all.contains(name), "{name} in {all}");
    }
    assert!(all.contains("every other tool asks"), "{all}");
    let mut m = screen(&ga4("approval = \"all\"\n"));
    let all = said(&mut m);
    assert!(all.contains("approval: all"), "{all}");
    assert!(all.contains("every tool the server registers"), "{all}");
}

// Spec: Missing key shows ask; Invalid value shows ask with the reason
#[test]
fn a_missing_key_or_an_invalid_value_shows_ask() {
    let all = said(&mut screen(&ga4("")));
    assert!(all.contains("approval: ask"), "{all}");
    assert!(all.contains("every call asks"), "{all}");
    let all = said(&mut screen(&ga4("approval = \"always\"\n")));
    assert!(all.contains("approval: ask"), "{all}");
    assert!(
        all.contains("expected \"ask\", \"reads\" or \"all\""),
        "{all}"
    );
}

// Spec: A custom server named like an entry asks at reads
#[test]
fn a_different_command_at_reads_says_every_call_asks() {
    let config = "[[ai.mcp.servers]]\nname = \"ga4\"\ncommand = \"mine\"\napproval = \"reads\"\n";
    let all = said(&mut screen(config));
    assert!(all.contains("approval: reads"), "{all}");
    assert!(all.contains("not the catalog's server"), "{all}");
    assert!(all.contains("every call asks"), "{all}");
}

// Spec: The list row shows a relaxed level
#[test]
fn the_list_suffixes_a_relaxed_level() {
    let config = format!(
        "{}\n[[ai.mcp.servers]]\nname = \"clarity\"\ncommand = \"x\"\napproval = \"all\"\n",
        ga4("approval = \"reads\"\n")
    );
    let secrets = "[clarity]\nCLARITY_API_TOKEN = \"t\"\n[ga4]\nGOOGLE_CLOUD_PROJECT = \"p\"\n";
    let mut m = area(Vec::new());
    m.presence_reply(presence(&config, secrets));
    let rows = texts(&m);
    for (label, state) in [("GA4", "declared · reads"), ("Clarity", "declared · all")] {
        assert!(
            rows.iter()
                .any(|r| r.starts_with(label) && r.contains(state)),
            "{label}: {rows:?}"
        );
    }
    // At `ask` the row carries no level.
    m.presence_reply(presence(&ga4(""), secrets));
    let rows = texts(&m);
    let row = rows.iter().find(|r| r.starts_with("GA4")).unwrap();
    assert_eq!(row, "GA4: declared");
}

// Spec: Not declared has no level; A project-limited entry is not edited
#[test]
fn an_undeclared_entry_has_no_level_and_a_project_entry_is_set_by_hand() {
    let mut m = screen("");
    let all = said(&mut m);
    assert!(!all.contains("approval:"), "{all}");
    assert_eq!(press(&mut m, 'a'), MenuCommand::None);
    assert_eq!(m.notice.as_deref(), Some(super::connectors::NOT_DECLARED));
    let project = ga4("project = \"/home/u/p\"\napproval = \"all\"\n");
    let mut m = screen(&project);
    let all = said(&mut m);
    assert!(all.contains("approval: all (project entry"), "{all}");
    assert!(all.contains("set it by hand"), "{all}");
    assert_eq!(press(&mut m, 'a'), MenuCommand::None);
    assert!(
        m.notice
            .as_deref()
            .unwrap()
            .contains("set approval by hand")
    );
    assert!(!texts(&m).concat().contains("(y/n)"));
}

// Spec: A proposal writes nothing; Nothing but the user's key writes it
#[test]
fn a_proposal_names_the_next_level_and_only_a_plain_y_confirms() {
    let config = ga4("");
    let mut m = screen(&config);
    assert_eq!(press(&mut m, 'a'), MenuCommand::None);
    assert_eq!(texts(&m)[0], "set ga4 to reads? (y/n)");
    assert_eq!(
        press(&mut m, 'y'),
        MenuCommand::EditApproval {
            name: "ga4".to_owned(),
            level: McpApproval::Reads,
            expected: Some(digest(config.as_bytes())),
        }
    );
    assert_eq!(m.edits(&Config::default()), Vec::new());
    // Each cancelling key leaves the level and sends nothing.
    let cancels = [
        KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::SHIFT),
        KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL),
        KeyEvent::new(KeyCode::Char('y'), KeyModifiers::ALT),
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    ];
    for cancel in cancels {
        let mut m = screen(&config);
        press(&mut m, 'a');
        assert_eq!(send(&mut m, cancel), MenuCommand::None, "{cancel:?}");
        assert_eq!(m.notice.as_deref(), Some("cancelled"), "{cancel:?}");
        assert!(said(&mut m).contains("approval: ask"));
        // The prompt is gone: a plain `y` now is an ordinary key.
        assert_eq!(press(&mut m, 'y'), MenuCommand::None);
    }
}

#[test]
fn the_order_is_ask_reads_all_ask_and_all_says_writes_run() {
    let mut m = screen(&ga4("approval = \"reads\"\n"));
    press(&mut m, 'a');
    let rows = texts(&m).join(" ");
    assert!(rows.starts_with("set ga4 to all? (y/n)"), "{rows}");
    assert!(
        rows.contains("every tool of ga4 runs without asking, writes included"),
        "{rows}"
    );
    let mut m = screen(&ga4("approval = \"all\"\n"));
    press(&mut m, 'a');
    assert_eq!(texts(&m)[0], "set ga4 to ask? (y/n)");
    let MenuCommand::EditApproval { level, .. } = press(&mut m, 'y') else {
        panic!("proposes");
    };
    assert_eq!(level, McpApproval::Ask);
}

// Spec (config-menu): a level change is not a draft; the reply reads again.
#[test]
fn the_reply_says_its_notice_and_reads_the_files_again() {
    let mut m = screen(&ga4(""));
    let done = "approval: ga4 → reads · restart · config.toml.bak";
    assert_eq!(
        m.approval_reply(Ok(done.to_owned())),
        MenuCommand::LoadPresence
    );
    assert_eq!(m.notice.as_deref(), Some(done));
    let refused = "approval: changed on disk \u{b7} nothing written";
    assert_eq!(
        m.approval_reply(Err(refused.to_owned())),
        MenuCommand::LoadPresence
    );
    assert_eq!(m.notice.as_deref(), Some(refused));
    assert_eq!(m.edits(&Config::default()), Vec::new());
}

// Spec: A running chat keeps its level; the area says to restart.
#[test]
fn the_re_read_shows_the_new_level_and_the_restart_line() {
    let startup = vec![crate::core::ai::config::McpServer {
        name: "ga4".to_owned(),
        command: mcp_catalog::entry("ga4").unwrap().command.to_owned(),
        args: Vec::new(),
        env: Vec::new(),
        project: None,
        enabled: true,
        approval: McpApproval::Ask,
    }];
    let mut m = entry(startup, 1);
    m.presence_reply(presence(&ga4(""), ""));
    assert!(!said(&mut m).contains("restart lazarobox"));
    m.presence_reply(presence(&ga4("approval = \"all\"\n"), ""));
    let all = said(&mut m);
    assert!(all.contains("approval: all"), "{all}");
    assert!(
        all.contains("restart lazarobox to use it in chats"),
        "{all}"
    );
}

// An unreadable or not yet read config.toml has no digest to compare: the
// proposal says so instead of failing later with `changed on disk`.
#[test]
fn an_unread_config_says_so_instead_of_changed_on_disk() {
    let startup = vec![crate::core::ai::config::McpServer {
        name: "ga4".to_owned(),
        command: "x".to_owned(),
        args: Vec::new(),
        env: Vec::new(),
        project: None,
        enabled: true,
        approval: McpApproval::Ask,
    }];
    let mut m = entry(startup, 1);
    assert_eq!(press(&mut m, 'a'), MenuCommand::None);
    assert_eq!(
        m.notice.as_deref(),
        Some("config.toml is not read yet; try again")
    );
    let unreadable = crate::core::ai::mcp_catalog::presence::Presence::read(
        Err(std::io::Error::other("too large")),
        &crate::core::ai::mcp_secrets::Loaded::default(),
    );
    m.presence_reply(unreadable);
    assert_eq!(press(&mut m, 'a'), MenuCommand::None);
    assert_eq!(
        m.notice.as_deref(),
        Some("config.toml could not be read (unreadable or over 256 KiB); set approval by hand")
    );
    assert!(!texts(&m).concat().contains("(y/n)"));
}
