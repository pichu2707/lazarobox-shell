//! Enter on the Updates row: a cargo install runs in a new tab, anything else
//! shows the command to copy.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use super::*;
use crate::core::update::{
    Fallback, INSTALL_COMMAND, InstallEnv, failed_text, no_status_text, not_started_text,
};

const CARGO: &str = "/home/ana/.cargo/bin/cargo";

fn cargo_env() -> InstallEnv {
    InstallEnv {
        exe: Some("/home/ana/.cargo/bin/lazarobox".into()),
        bin_dir: Some("/home/ana/.cargo/bin".into()),
        cargo: Some(CARGO.into()),
        cargo_refused: false,
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
        .with_install_env(env)
        .with_cargo_check(|_| true);
    a.sync_mouse();
    press(&mut a, "tn");
    to_updates_row(&mut a);
    a
}

/// Opens the menu and moves to the Updates row.
fn to_updates_row(a: &mut App) {
    a.update(ctrl_space());
    a.update(key('m'));
    for _ in 0..5 {
        a.update(key('j'));
    }
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

/// The pane's text with the terminal's hard wrapping removed.
fn flat(a: &App, id: PaneId) -> String {
    pane_text(a, id).replace('\n', "")
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
    assert_eq!(
        spec.args,
        [
            "install",
            "lazarobox-shell",
            "--locked",
            "--version",
            "0.15.4",
            "--root",
            "/home/ana/.cargo"
        ],
        "the announced version is exact and the root is named, so neither a newer \
         release nor CARGO_INSTALL_ROOT can change what is installed"
    );
    assert!(spec.report_status);
    assert!(
        spec.env_os.contains(&(
            OsString::from("PATH"),
            OsString::from("/home/ana/.cargo/bin:/usr/bin")
        )),
        "cargo runs on the login PATH: {:?}",
        spec.env_os
    );
    assert_eq!(spec.cwd, PathBuf::from("/"), "never the user's cwd");
    assert_eq!(a.tabs.len(), tabs + 1);
    assert_eq!(a.tab().focus, *id, "the new tab is the one on screen");
}

#[test]
fn enter_closes_the_menu_and_lands_on_the_update_pane() {
    let (a, id) = started();
    assert!(a.menu().is_none(), "the menu is closed");
    assert_eq!(a.input(), InputMode::Terminal, "keys go to the pane");
    assert_eq!(a.tab().focus, id, "the focused pane is the update pane");
    assert!(
        a.panes[&id].pty().is_some_and(|pty| pty.update.is_some()),
        "and it is the update pane"
    );
}

#[test]
fn closing_for_the_install_treats_an_unsaved_draft_as_esc_does() {
    // Esc at the top level discards what was not saved. Closing for the
    // install must neither keep it nor save it behind the user's back.
    let fresh = || {
        let mut a = launched(80, 25)
            .with_update(Some("0.15.4".to_owned()))
            .with_install_env(Some(cargo_env()))
            .with_cargo_check(|_| true);
        press(&mut a, "tn");
        a.update(ctrl_space());
        a.update(key('m'));
        a
    };
    let mut esc_app = fresh();
    let original = *esc_app.config();
    esc_app.update(key('l'));
    assert_ne!(*esc_app.config(), original, "the draft really changed");
    esc_app.update(esc());
    let mut enter_app = fresh();
    enter_app.update(key('l'));
    assert_ne!(*enter_app.config(), original);
    for _ in 0..5 {
        enter_app.update(key('j'));
    }
    let effects = enter_app.update(enter());
    assert_eq!(spawned(&effects).len(), 1, "{effects:?}");
    assert_eq!(*enter_app.config(), *esc_app.config(), "same as Esc");
    assert_eq!(*enter_app.config(), original, "and nothing was saved");
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::SaveConfig(..))),
        "{effects:?}"
    );
}

#[test]
fn starting_tells_the_user_where_it_runs_how_long_and_not_to_close() {
    let (a, id) = started();
    assert_eq!(a.tab().focus, id, "the new tab is on screen");
    let text = flat(&a, id);
    assert!(text.contains("Updating lazarobox to 0.15.4."), "{text}");
    assert!(text.contains("a few minutes"), "{text}");
    assert!(text.contains("Keep lazarobox open"), "{text}");
    // The technical line still follows, for whoever wants it.
    assert!(
        text.contains("running /home/ana/.cargo/bin/cargo"),
        "{text}"
    );
}

#[test]
fn a_failure_names_the_command_to_run_by_hand_in_the_pane() {
    let (mut a, id) = started();
    a.update(AppEvent::Pty(id, PtyEvent::Status(101)));
    let text = flat(&a, id)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(text.contains("The update did not finish"), "{text}");
    assert!(text.contains(INSTALL_COMMAND), "{text}");
}

#[test]
fn a_second_enter_while_it_runs_starts_nothing() {
    let (mut a, _) = started();
    to_updates_row(&mut a);
    let effects = a.update(enter());
    assert!(spawned(&effects).is_empty(), "{effects:?}");
    assert!(
        footer(&a).starts_with("An update is already running"),
        "{}",
        footer(&a)
    );
}

#[test]
fn a_binary_outside_cargo_only_shows_the_command_and_copies_it() {
    for code in [KeyCode::Enter, KeyCode::Char('y')] {
        let mut a = on_updates(Some("0.15.4"), Some(distro_env()));
        assert!(a.update(enter()).is_empty(), "nothing runs");
        assert_eq!(footer(&a), Fallback::NotCargoInstall.footer());
        let copy = a.update(AppEvent::Key(KeyEvent::new(code, KeyModifiers::NONE)));
        let sequence = crate::core::copy::osc52(INSTALL_COMMAND).unwrap();
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
    assert_eq!(footer(&a), Fallback::NoCargo.footer());
    assert_eq!(
        footer(&a),
        "Enter or y copies it \u{b7} Rust is not installed"
    );
    // The command alone will not work without Rust: the note says so.
    let note = a.menu().unwrap().update_note().unwrap();
    assert_eq!(note[0], "Install Rust first (rustup.rs), then run:");
    assert_eq!(note[1], INSTALL_COMMAND);
}

#[test]
fn a_refused_cargo_says_so_instead_of_looking_like_nothing_happened() {
    let env = InstallEnv {
        cargo: None,
        cargo_refused: true,
        ..cargo_env()
    };
    let mut a = on_updates(Some("0.15.4"), Some(env));
    assert!(a.update(enter()).is_empty(), "nothing runs");
    assert_eq!(footer(&a), Fallback::UntrustedCargo.footer());
    assert!(
        footer(&a).starts_with("Enter or y copies"),
        "{}",
        footer(&a)
    );
    assert!(footer(&a).contains("too open"), "{}", footer(&a));
}

#[test]
fn a_cargo_that_stopped_being_trusted_since_start_up_is_refused_at_enter() {
    // Start-up found and trusted the cargo; by Enter it no longer passes (a
    // folder above it was opened up). The check runs again, on that path.
    let mut a = on_updates(Some("0.15.4"), Some(cargo_env())).with_cargo_check(|path| {
        assert_eq!(path, Path::new(CARGO), "the path that was found is checked");
        false
    });
    let tabs = a.tabs.len();
    let effects = a.update(enter());
    assert!(spawned(&effects).is_empty(), "nothing runs: {effects:?}");
    assert_eq!(a.tabs.len(), tabs, "no tab opens");
    assert_eq!(footer(&a), Fallback::UntrustedCargo.footer());
    // Still trusted: the same app runs it, so the refusal is the check's.
    let mut b = on_updates(Some("0.15.4"), Some(cargo_env())).with_cargo_check(|_| true);
    assert_eq!(spawned(&b.update(enter())).len(), 1);
}

#[test]
fn without_an_environment_the_command_is_shown_not_run() {
    let mut a = on_updates(Some("0.15.4"), None);
    assert!(a.update(enter()).is_empty());
    assert_eq!(footer(&a), Fallback::NotCargoInstall.footer());
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
        Some("Updated to 0.15.4. Restart lazarobox to use it.")
    );
    assert_eq!(a.status_update(), None, "nothing left to announce");
    a.update(AppEvent::Pty(id, PtyEvent::Exited));
    assert_eq!(a.tabs.len(), tabs - 1, "the tab closes with its pane");
    assert!(!a.panes.contains_key(&id));
    to_updates_row(&mut a);
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
    assert_eq!(a.notice(), Some(failed_text(101).as_str()));
    let effects = a.update(AppEvent::Pty(id, PtyEvent::Exited));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(a.tabs.len(), tabs, "the pane stays");
    let text = flat(&a, id);
    assert!(text.contains("error: could not compile"), "{text}");
    assert!(text.contains(&failed_text(101)), "{text}");
    assert_eq!(a.status_update(), Some("0.15.4"), "still not installed");
}

#[test]
fn a_failed_pane_can_be_started_again_after_it_is_closed() {
    let (mut a, id) = started();
    a.update(AppEvent::Pty(id, PtyEvent::Status(1)));
    a.update(AppEvent::Pty(id, PtyEvent::Exited));
    to_updates_row(&mut a);
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
    let text = flat(&a, id);
    assert!(text.contains(&not_started_text()), "{text}");
    assert_eq!(a.notice(), Some(not_started_text().as_str()));
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
    assert!(flat(&a, id).contains("Updated to 0.15.4. Restart lazarobox to use it."));
}

#[test]
fn a_shell_exit_still_closes_its_pane_without_a_status() {
    let mut a = launched(80, 25);
    press(&mut a, "tn");
    let id = a.tab().focus;
    a.update(AppEvent::Pty(id, PtyEvent::Exited));
    assert!(!a.panes.contains_key(&id));
}

#[test]
fn a_path_that_is_not_utf8_reaches_cargo_unchanged() {
    use std::os::unix::ffi::OsStringExt;
    let path = OsString::from_vec(b"/home/an\xff/bin:/usr/bin".to_vec());
    let env = InstallEnv {
        path: path.clone(),
        ..cargo_env()
    };
    let mut a = on_updates(Some("0.15.4"), Some(env));
    let effects = a.update(enter());
    let (_, spec) = spawned(&effects)[0];
    assert!(spec.env_os.contains(&(OsString::from("PATH"), path)));
}

#[test]
fn the_pane_then_names_what_runs() {
    let (a, id) = started();
    // The lines are longer than the pane is wide, so they wrap.
    let text = flat(&a, id);
    assert!(
        text.contains(
            "running /home/ana/.cargo/bin/cargo install lazarobox-shell --locked --version 0.15.4 --root /home/ana/.cargo"
        ),
        "{text}"
    );
}

#[test]
fn a_child_that_ends_without_a_status_is_a_failure_and_the_pane_stays() {
    let (mut a, id) = started();
    let tabs = a.tabs.len();
    let effects = a.update(AppEvent::Pty(id, PtyEvent::Exited));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(a.tabs.len(), tabs, "the pane stays");
    assert!(flat(&a, id).contains("The update stopped unexpectedly"));
    assert_eq!(a.notice(), Some(no_status_text().as_str()));
    assert_eq!(a.status_update(), Some("0.15.4"), "still not installed");
}
