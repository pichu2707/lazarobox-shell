//! The Connectors area through the app (mcp-connect, Entry list, and the
//! config re-read of D20): the command it emits when it opens, the replies it
//! gets, and that visiting it writes nothing.

use super::*;
use crate::core::{
    ai::{
        mcp_catalog::{
            checks::{Probed, Verdict},
            presence::Presence,
        },
        mcp_secrets::Secrets,
    },
    config::Config as FileConfig,
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
fn a_pasted_entry_asks_for_a_restart() {
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
fn a_probe_reply_updates_the_open_area() {
    let mut a = in_area();
    tap(
        &mut a,
        AppEvent::McpProbe(Probed {
            server: "clarity".to_owned(),
            verdict: Verdict::Failed,
            lines: vec!["mcp: clarity unavailable: boom".to_owned()],
            problems: Vec::new(),
        }),
    );
    assert_eq!(rows(&a)[1], "Clarity: declared · probe: failed");
}

// Spec (The area writes no configuration): no key of the area saves anything
// or types anything on its own.
#[test]
fn visiting_the_area_saves_and_types_nothing() {
    let mut a = in_area();
    let mut effects = Vec::new();
    for c in "jjkkz".chars() {
        effects.extend(tap(&mut a, key(c)));
    }
    effects.extend(tap(&mut a, esc()));
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::SaveConfig(_) | Effect::WritePty(..))),
        "{effects:?}"
    );
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::SpawnPane(..) | Effect::McpProbe(_)))
    );
}
