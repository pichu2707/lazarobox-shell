//! Enter on the Updates row: a cargo install runs in a new tab, anything else
//! shows the command to copy.

use std::{ffi::OsString, path::PathBuf};

use super::*;
use crate::core::update::{INSTALL_COMMAND_LOCKED, InstallEnv};

const CARGO: &str = "/home/ana/.cargo/bin/cargo";

fn cargo_env() -> InstallEnv {
    InstallEnv {
        exe: Some("/home/ana/.cargo/bin/lazarobox".into()),
        cargo_home: None,
        home: Some("/home/ana".into()),
        cargo: Some(CARGO.into()),
        path: OsString::from("/home/ana/.cargo/bin:/usr/bin"),
    }
}

fn distro_env() -> InstallEnv {
    InstallEnv {
        exe: Some("/usr/bin/lazarobox".into()),
        ..cargo_env()
    }
}

fn enter() -> AppEvent {
    AppEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
}

/// A two-tab app (so the menu never resizes) with the menu on the Updates row.
fn on_updates(update: Option<&str>, env: Option<InstallEnv>) -> App {
    let mut a = launched(80, 25)
        .with_update(update.map(str::to_owned))
        .with_install_env(env);
    a.sync_mouse();
    press(&mut a, "tn");
    a.update(ctrl_space());
    a.update(key('m'));
    for _ in 0..5 {
        a.update(key('j'));
    }
    a
}

fn spawned(effects: &[Effect]) -> Vec<(&PaneId, &SpawnSpec)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::SpawnPane(id, spec) => Some((id, spec)),
            _ => None,
        })
        .collect()
}

fn pane_text(a: &App, id: PaneId) -> String {
    let pty = a.panes[&id].pty().expect("a pty pane");
    (0..25)
        .map(|row| {
            (0..80)
                .map(|col| pty.emu.cell(row, col).map_or("", |c| c.text))
                .map(|t| if t.is_empty() { " " } else { t })
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn footer(a: &App) -> String {
    match a.menu().unwrap().footer() {
        crate::core::menu::Footer::Hint(t) => t.into_owned(),
        crate::core::menu::Footer::Error(t) => t.to_owned(),
    }
}

/// Starts the install and returns the update pane.
fn started() -> (App, PaneId) {
    let mut a = on_updates(Some("0.15.4"), Some(cargo_env()));
    let effects = a.update(enter());
    let (id, _) = spawned(&effects)[0];
    let id = *id;
    (a, id)
}

#[test]
fn a_cargo_install_runs_cargo_directly_in_a_new_visible_tab() {
    let mut a = on_updates(Some("0.15.4"), Some(cargo_env()));
    let tabs = a.tabs.len();
    let effects = a.update(enter());
    let spawns = spawned(&effects);
    assert_eq!(spawns.len(), 1, "{effects:?}");
    let (id, spec) = spawns[0];
    assert_eq!(spec.program, PathBuf::from(CARGO));
    assert_eq!(spec.args, ["install", "lazarobox-shell", "--locked"]);
    assert!(spec.report_status);
    assert!(
        spec.env.contains(&(
            "PATH".to_owned(),
            "/home/ana/.cargo/bin:/usr/bin".to_owned()
        )),
        "cargo runs on the login PATH: {:?}",
        spec.env
    );
    assert_eq!(spec.cwd, PathBuf::from("/"), "never the user's cwd");
    assert_eq!(a.tabs.len(), tabs + 1);
    assert_eq!(a.tab().focus, *id, "the new tab is the one on screen");
    assert_eq!(a.input(), InputMode::Menu, "the menu draft is kept");
}

#[test]
fn a_second_enter_while_it_runs_starts_nothing() {
    let (mut a, _) = started();
    let effects = a.update(enter());
    assert!(spawned(&effects).is_empty(), "{effects:?}");
    assert!(footer(&a).contains("already running"), "{}", footer(&a));
}

#[test]
fn a_binary_outside_cargo_only_shows_the_command_and_copies_it() {
    for code in [KeyCode::Enter, KeyCode::Char('y')] {
        let mut a = on_updates(Some("0.15.4"), Some(distro_env()));
        assert!(a.update(enter()).is_empty(), "nothing runs");
        assert!(
            footer(&a).starts_with(INSTALL_COMMAND_LOCKED),
            "{}",
            footer(&a)
        );
        let copy = a.update(AppEvent::Key(KeyEvent::new(code, KeyModifiers::NONE)));
        let sequence = crate::core::copy::osc52(INSTALL_COMMAND_LOCKED).unwrap();
        assert_eq!(copy, vec![Effect::SetClipboard(sequence)], "{code:?}");
    }
}

#[test]
fn without_cargo_on_the_path_the_command_is_shown_not_run() {
    let env = InstallEnv {
        cargo: None,
        ..cargo_env()
    };
    let mut a = on_updates(Some("0.15.4"), Some(env));
    assert!(a.update(enter()).is_empty());
    assert!(footer(&a).starts_with(INSTALL_COMMAND_LOCKED));
}

#[test]
fn without_an_environment_the_command_is_shown_not_run() {
    let mut a = on_updates(Some("0.15.4"), None);
    assert!(a.update(enter()).is_empty());
    assert!(footer(&a).starts_with(INSTALL_COMMAND_LOCKED));
}

#[test]
fn without_an_update_enter_runs_and_shows_nothing() {
    let mut a = on_updates(None, Some(cargo_env()));
    assert!(a.update(enter()).is_empty());
    assert!(footer(&a).starts_with("up to date"), "{}", footer(&a));
}

#[test]
fn exit_zero_closes_the_pane_and_announces_the_install() {
    let (mut a, id) = started();
    let tabs = a.tabs.len();
    a.update(AppEvent::Pty(id, PtyEvent::Status(0)));
    assert_eq!(
        a.notice(),
        Some("lazarobox 0.15.4 installed: restart lazarobox to use it")
    );
    assert_eq!(a.status_update(), None, "nothing left to announce");
    a.update(AppEvent::Pty(id, PtyEvent::Exited));
    assert_eq!(a.tabs.len(), tabs - 1, "the tab closes with its pane");
    assert!(!a.panes.contains_key(&id));
    let rows = a.menu().unwrap().rows(&a.config);
    assert!(
        rows.iter().any(|r| matches!(r,
            Row::Item { label: "Updates", value: Some(v), .. } if v == "0.15.4 installed: restart")),
        "{rows:?}"
    );
}

#[test]
fn a_late_check_does_not_bring_the_update_back_after_an_install() {
    let (mut a, id) = started();
    a.update(AppEvent::Pty(id, PtyEvent::Status(0)));
    a.update(AppEvent::Pty(id, PtyEvent::Exited));
    a.update(AppEvent::UpdateChecked(Some("0.15.4".into())));
    assert_eq!(a.status_update(), None);
}

#[test]
fn a_menu_opened_after_the_install_says_installed() {
    let (mut a, id) = started();
    a.update(AppEvent::Pty(id, PtyEvent::Status(0)));
    a.update(AppEvent::Pty(id, PtyEvent::Exited));
    a.update(esc());
    a.update(ctrl_space());
    a.update(key('m'));
    let rows = a.menu().unwrap().rows(&a.config);
    assert!(rows.iter().any(|r| matches!(r,
        Row::Item { label: "Updates", value: Some(v), .. } if v == "0.15.4 installed: restart")));
}

#[test]
fn a_failure_keeps_the_pane_open_with_the_final_line() {
    let (mut a, id) = started();
    let tabs = a.tabs.len();
    a.update(AppEvent::Pty(
        id,
        PtyEvent::Output(b"error: could not compile\r\n".to_vec()),
    ));
    a.update(AppEvent::Pty(id, PtyEvent::Status(101)));
    assert_eq!(a.notice(), Some("update failed (exit 101)"));
    let effects = a.update(AppEvent::Pty(id, PtyEvent::Exited));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(a.tabs.len(), tabs, "the pane stays");
    let text = pane_text(&a, id);
    assert!(text.contains("error: could not compile"), "{text}");
    assert!(text.contains("update failed (exit 101)"), "{text}");
    assert_eq!(a.status_update(), Some("0.15.4"), "still not installed");
}

#[test]
fn a_failed_pane_can_be_started_again_after_it_is_closed() {
    let (mut a, id) = started();
    a.update(AppEvent::Pty(id, PtyEvent::Status(1)));
    a.update(AppEvent::Pty(id, PtyEvent::Exited));
    let effects = a.update(enter());
    assert_eq!(
        spawned(&effects).len(),
        1,
        "a finished run does not block a retry"
    );
}

#[test]
fn a_spawn_error_keeps_the_pane_open_with_a_failure_line() {
    let (mut a, id) = started();
    let tabs = a.tabs.len();
    a.update(AppEvent::SpawnFailed(id, "no such file".into()));
    assert_eq!(a.tabs.len(), tabs, "the pane stays");
    let text = pane_text(&a, id);
    assert!(
        text.contains("update failed (cannot start cargo)"),
        "{text}"
    );
    assert_eq!(a.notice(), Some("update failed (cannot start cargo)"));
}

#[test]
fn a_success_in_the_only_pane_never_quits_the_app() {
    let (mut a, id) = started();
    // The user closed the original tab while cargo ran.
    a.drop_tab(0);
    a.drop_tab(0);
    assert_eq!(a.tabs.len(), 1);
    a.update(AppEvent::Pty(id, PtyEvent::Status(0)));
    let effects = a.update(AppEvent::Pty(id, PtyEvent::Exited));
    assert!(!effects.contains(&Effect::Quit), "{effects:?}");
    assert!(a.panes.contains_key(&id), "the finished pane stays");
    assert!(pane_text(&a, id).contains("lazarobox 0.15.4 installed: restart lazarobox to use it"));
}

#[test]
fn a_shell_exit_still_closes_its_pane_without_a_status() {
    let mut a = launched(80, 25);
    press(&mut a, "tn");
    let id = a.tab().focus;
    a.update(AppEvent::Pty(id, PtyEvent::Exited));
    assert!(!a.panes.contains_key(&id));
}
