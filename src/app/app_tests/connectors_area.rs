//! The Connectors area through the app (mcp-connect, Entry list, Entry
//! screen, Install and login, Probe from the area, and the config re-read of
//! D20): the commands it emits, the replies it gets, and that nothing runs
//! without the user's `y`.

use super::*;
use crate::core::{
    ai::{
        mcp_catalog::{
            checks::{Probed, Verdict},
            entry,
            presence::Presence,
        },
        mcp_secrets::Secrets,
    },
    config::Config as FileConfig,
    menu::Footer,
};

const CLARITY: &str = "[[ai.mcp.servers]]\nname = \"clarity\"\ncommand = \"x\"\n";

fn rows(a: &App) -> Vec<String> {
    a.menu()
        .unwrap()
        .rows(&Config::default())
        .into_iter()
        .map(|row| match row {
            Row::Title(t) | Row::Text(t) => t.into_owned(),
            Row::Item { label, value, .. } => format!("{label}: {}", value.unwrap_or_default()),
            other => format!("{other:?}"),
        })
        .collect()
}

/// One key after the hold window of the last, so the same key twice is two
/// presses and not a held key.
fn tap(a: &mut App, event: AppEvent) -> Vec<Effect> {
    let_go();
    a.update(event)
}

fn spawned(effects: &[Effect]) -> Option<PaneId> {
    effects.iter().find_map(|e| match e {
        Effect::SpawnPane(id, _) => Some(*id),
        _ => None,
    })
}

fn writes(effects: &[Effect]) -> Vec<(PaneId, Vec<u8>)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::WritePty(id, bytes) => Some((*id, bytes.clone())),
            _ => None,
        })
        .collect()
}

/// The area open over an app that loaded `CLARITY` at startup.
fn in_area() -> App {
    let ai = FileConfig::parse(CLARITY).unwrap().ai;
    let mut a = launched(80, 25).with_ai(ai);
    tap(&mut a, ctrl_space());
    tap(&mut a, key('m'));
    tap(&mut a, key('4'));
    let effects = tap(&mut a, code(KeyCode::Enter));
    assert_eq!(effects, [Effect::LoadPresence], "opening reads the files");
    a
}

/// The same, on the Clarity entry screen.
fn on_entry() -> App {
    let mut a = in_area();
    tap(&mut a, code(KeyCode::Enter));
    a
}

fn failed() -> Probed {
    Probed {
        server: "clarity".to_owned(),
        verdict: Verdict::Failed,
        lines: vec!["mcp: clarity unavailable: boom".to_owned()],
        problems: Vec::new(),
    }
}

#[test]
fn the_load_presence_command_becomes_its_effect() {
    let mut a = launched(80, 25);
    assert_eq!(
        a.on_menu_command(MenuCommand::LoadPresence),
        [Effect::LoadPresence]
    );
}

#[test]
fn the_menu_opens_with_the_startup_servers() {
    assert_eq!(rows(&in_area())[1], "Clarity: declared");
}

#[test]
fn a_pasted_entry_asks_for_a_restart_on_the_list_and_the_entry() {
    let mut a = in_area();
    let none = Presence::read(Ok(String::new()), &Secrets::default());
    tap(&mut a, AppEvent::McpPresence(none));
    assert_eq!(rows(&a)[1], "Clarity: not declared");
    assert!(rows(&a).contains(&"restart lazarobox to use it in chats".to_owned()));
}

#[test]
fn a_presence_reply_after_the_menu_closed_changes_nothing() {
    let mut a = launched(80, 25);
    let none = Presence::read(Ok(String::new()), &Secrets::default());
    assert_eq!(tap(&mut a, AppEvent::McpPresence(none)), []);
}

#[test]
fn the_install_runs_only_after_y_and_with_the_shown_line() {
    let mut a = on_entry();
    let before = a.panes.len();
    let asked = tap(&mut a, key('i'));
    assert_eq!((spawned(&asked), writes(&asked)), (None, vec![]));
    let declined = tap(&mut a, key('n'));
    assert_eq!((spawned(&declined), writes(&declined)), (None, vec![]));
    assert_eq!(a.panes.len(), before);
    tap(&mut a, key('i'));
    let run = tap(&mut a, key('y'));
    let id = spawned(&run).expect("one new pane");
    let typed = tap(&mut a, AppEvent::Pty(id, PtyEvent::Output(b"$ ".to_vec())));
    let line = entry("clarity").unwrap().install;
    assert_eq!(writes(&typed), [(id, format!("{line}\r").into_bytes())]);
}

#[test]
fn the_probe_starts_only_after_y() {
    let mut a = on_entry();
    assert_eq!(tap(&mut a, key('p')), []);
    assert_eq!(tap(&mut a, key('n')), []);
    tap(&mut a, key('p'));
    let started = tap(&mut a, key('y'));
    assert!(matches!(started.as_slice(), [Effect::McpProbe(_, s)] if s.name == "clarity"));
}

#[test]
fn a_probe_reply_updates_the_area_even_a_failed_one() {
    let mut a = on_entry();
    tap(&mut a, key('p'));
    let started = tap(&mut a, key('y'));
    let Some(Effect::McpProbe(id, _)) = started.first() else {
        panic!("probe asked");
    };
    assert!(matches!(
        a.menu().unwrap().footer(),
        Footer::Hint(h) if h.starts_with("probing clarity")
    ));
    tap(&mut a, AppEvent::McpProbe(*id, failed()));
    assert!(matches!(
        a.menu().unwrap().footer(),
        Footer::Hint(h) if !h.starts_with("probing")
    ));
    assert!(rows(&a).iter().any(|l| l.contains("unavailable: boom")));
}

#[test]
fn esc_while_probing_cancels_the_probe() {
    let mut a = on_entry();
    tap(&mut a, key('p'));
    tap(&mut a, key('y'));
    assert_eq!(tap(&mut a, esc()), [Effect::CancelMcpProbe]);
}

// Spec (The area writes no configuration): no key of the area saves anything
// or types anything on its own.
#[test]
fn visiting_the_area_saves_and_types_nothing() {
    let mut a = in_area();
    let mut effects = Vec::new();
    for c in "jjkk".chars() {
        effects.extend(tap(&mut a, key(c)));
    }
    effects.extend(tap(&mut a, code(KeyCode::Enter)));
    for c in "jkctz12".chars() {
        effects.extend(tap(&mut a, key(c)));
    }
    effects.extend(tap(&mut a, esc()));
    effects.extend(tap(&mut a, esc()));
    assert!(
        !effects.iter().any(|e| matches!(
            e,
            Effect::SaveConfig(_) | Effect::WritePty(..) | Effect::McpProbe(..)
        )),
        "{effects:?}"
    );
    assert_eq!(spawned(&effects), None);
}
