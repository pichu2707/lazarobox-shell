//! The Connectors area through the app (mcp-connect, Entry list, Entry
//! screen, Install and login, Probe from the area, and the config re-read of
//! D20): the commands it emits, the replies it gets, and that nothing runs
//! without the user's `y`.

use super::*;
use crate::core::ai::mcp_secrets::Secrets;
use crate::core::{
    ai::{
        mcp_catalog::{
            checks::{Probed, Verdict},
            entry,
            presence::Presence,
        },
        mcp_secrets::Loaded,
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
    let none = Presence::read(Ok(String::new()), &Loaded::default());
    tap(&mut a, AppEvent::McpPresence(none));
    assert_eq!(rows(&a)[1], "Clarity: not declared");
    assert!(rows(&a).contains(&"restart lazarobox to use it in chats".to_owned()));
}

#[test]
fn a_presence_reply_after_the_menu_closed_changes_nothing() {
    let mut a = launched(80, 25);
    let none = Presence::read(Ok(String::new()), &Loaded::default());
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

// A resize while the menu is open re-wraps the area at once, not at the next key.
#[test]
fn a_resize_rewraps_an_open_prompt_without_a_key() {
    let mut a = on_entry();
    tap(&mut a, key('i'));
    assert!(rows(&a).iter().any(|l| l.contains("npm i -g")));
    tap(&mut a, AppEvent::Resize { cols: 20, rows: 25 });
    assert_eq!(rows(&a), ["widen the terminal to read the command"]);
    tap(&mut a, AppEvent::Resize { cols: 80, rows: 25 });
    assert!(rows(&a).iter().any(|l| l.contains("npm i -g")));
}

// ---- S6a2: the secret field through the app ---------------------------------

const MATCHING: &str = "[[ai.mcp.servers]]\nname = \"clarity\"\ncommand = \"clarity-mcp-server\"\n";

fn secret(text: &str) -> crate::core::ai::mcp_secrets::SecretText {
    let mut s = crate::core::ai::mcp_secrets::SecretText::new();
    s.push_str(text);
    s
}

fn footer(a: &App) -> String {
    match a.menu().unwrap().footer() {
        Footer::Hint(h) => h.into_owned(),
        Footer::Error(e) => e.to_owned(),
    }
}

/// The Clarity field open over a chat that declared the catalog command.
fn field_in_a_chat() -> App {
    let ai = FileConfig::parse(MATCHING).unwrap().ai;
    let mut a = super::run_pane::one_shell_and_a_chat("hello").with_ai(ai);
    tap(&mut a, ctrl_space());
    tap(&mut a, key('m'));
    tap(&mut a, key('4'));
    tap(&mut a, code(KeyCode::Enter));
    tap(&mut a, code(KeyCode::Enter));
    tap(&mut a, key('f'));
    tap(&mut a, code(KeyCode::Enter));
    a
}

// Spec: Save a new value (the command becomes the effect, once).
#[test]
fn a_confirmed_field_becomes_one_save_effect_with_the_value() {
    let mut a = field_in_a_chat();
    for c in "tok".chars() {
        assert_eq!(tap(&mut a, key(c)), []);
    }
    assert_eq!(
        tap(&mut a, code(KeyCode::Enter)),
        [Effect::SaveSecret {
            server: "clarity",
            name: "CLARITY_API_TOKEN",
            value: secret("tok")
        }]
    );
}

// Spec: Paste goes to the field only.
#[test]
fn a_paste_goes_to_the_open_field_and_not_to_the_chat() {
    let mut a = field_in_a_chat();
    assert_eq!(tap(&mut a, AppEvent::Paste("sk-123".to_owned())), []);
    assert_eq!(super::run_pane::chat(&a, id2()).composer().text(), "");
    let rows = rows(&a);
    assert!(rows.iter().any(|r| r.trim() == "••••••••"), "{rows:?}");
    assert_eq!(
        tap(&mut a, code(KeyCode::Enter)),
        [Effect::SaveSecret {
            server: "clarity",
            name: "CLARITY_API_TOKEN",
            value: secret("sk-123")
        }]
    );
    assert_eq!(super::run_pane::chat(&a, id2()).composer().text(), "");
}

#[test]
fn the_reply_of_a_save_shows_its_notice_and_reads_the_files_again() {
    let mut a = field_in_a_chat();
    let said = "saved CLARITY_API_TOKEN".to_owned();
    assert_eq!(
        tap(&mut a, AppEvent::SecretSaved(Ok(said.clone()))),
        [Effect::LoadPresence]
    );
    assert_eq!(footer(&a), said);
    let refusal = "secrets.toml is a symlink; edit it by hand".to_owned();
    assert_eq!(tap(&mut a, AppEvent::SecretSaved(Err(refusal.clone()))), []);
    assert_eq!(footer(&a), refusal);
}

// A reply that arrives after the menu closed is still said, never dropped.
#[test]
fn a_reply_after_the_menu_closed_is_said_in_the_notice() {
    let mut a = launched(80, 25);
    assert_eq!(
        tap(&mut a, AppEvent::SecretSaved(Ok("saved X".to_owned()))),
        []
    );
    assert_eq!(a.notice.as_deref(), Some("saved X"));
    tap(
        &mut a,
        AppEvent::BackupDeleted(Err("secrets.toml.bak: there is no backup".to_owned())),
    );
    assert_eq!(
        a.notice.as_deref(),
        Some("secrets.toml.bak: there is no backup")
    );
}

// Spec: the backup action is a command, then an effect, only after `y`.
#[test]
fn deleting_the_backup_is_an_effect_only_after_y() {
    let mut a = on_entry();
    let present = Presence::read(Ok(CLARITY.to_owned()), &Loaded::default()).with_backup(true);
    tap(&mut a, AppEvent::McpPresence(present));
    tap(&mut a, key('f'));
    assert_eq!(tap(&mut a, key('d')), []);
    assert_eq!(tap(&mut a, key('y')), [Effect::DeleteBackup]);
    assert_eq!(
        tap(
            &mut a,
            AppEvent::BackupDeleted(Ok("deleted secrets.toml.bak".to_owned()))
        ),
        [Effect::LoadPresence]
    );
}

// Spec: The value appears nowhere else. Every surface the app has to show or
// format what happened, from the paste to the reply.
#[test]
fn a_saved_confidential_value_reaches_no_surface_of_the_app() {
    let value = "tok-7f3a9c1e2b4d6f80";
    let mut a = field_in_a_chat();
    let mut seen = vec![format!(
        "{:?}",
        tap(&mut a, AppEvent::Paste(value.to_owned()))
    )];
    seen.push(rows(&a).concat());
    let effects = tap(&mut a, code(KeyCode::Enter));
    seen.push(format!("{effects:?}"));
    let reply = AppEvent::SecretSaved(Ok("saved CLARITY_API_TOKEN".to_owned()));
    seen.push(format!("{reply:?} {:?}", tap(&mut a, reply.clone())));
    let loaded = format!("[clarity]\nCLARITY_API_TOKEN = \"{value}\"\n");
    let presence = Presence::read(Ok(MATCHING.to_owned()), &Secrets::parse(loaded.as_bytes()));
    tap(&mut a, AppEvent::McpPresence(presence));
    seen.extend([
        rows(&a).concat(),
        footer(&a),
        format!("{:?}", a.menu().unwrap()),
    ]);
    seen.push(super::run_pane::chat(&a, id2()).composer().text());
    seen.push(a.notice.clone().unwrap_or_default());
    let all = seen.join("\n");
    for at in 0..=value.len() - 4 {
        assert!(
            !all.contains(&value[at..at + 4]),
            "{} leaked",
            &value[at..at + 4]
        );
    }
}

// Spec (config-menu, Connectors row): a secret save is not a draft, and every
// key goes to the open field.
#[test]
fn a_secret_field_is_not_a_draft_and_takes_every_key() {
    let mut a = field_in_a_chat();
    for c in "s4q".chars() {
        assert_eq!(tap(&mut a, key(c)), []);
    }
    assert_eq!(
        rows(&a).len(),
        2,
        "the field stays one screen: {:?}",
        rows(&a)
    );
    tap(
        &mut a,
        AppEvent::SecretSaved(Ok("saved CLARITY_API_TOKEN".to_owned())),
    );
    assert!(a.menu().unwrap().edits(&Config::default()).is_empty());
    let effects = [esc(), esc(), esc(), esc()]
        .map(|e| tap(&mut a, e))
        .concat();
    assert!(
        !effects.iter().any(|e| matches!(
            e,
            Effect::SaveConfig(_) | Effect::SaveSecret { .. } | Effect::DeleteBackup
        )),
        "{effects:?}"
    );
}

// Spec: every other action of the area writes nothing; only a confirmed field
// or `y` on the backup prompt produces a write effect.
#[test]
fn nothing_but_a_confirmed_field_or_a_confirmed_backup_prompt_writes() {
    let mut a = on_entry();
    let mut effects = Vec::new();
    for e in [
        key('f'),
        code(KeyCode::Enter),
        key('t'),
        key('o'),
        key('k'),
        esc(),
        key('d'),
        key('n'),
        esc(),
    ] {
        effects.extend(tap(&mut a, e));
    }
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::SaveSecret { .. } | Effect::DeleteBackup)),
        "{effects:?}"
    );
}
