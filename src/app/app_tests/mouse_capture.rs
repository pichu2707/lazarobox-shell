//! The host mouse capture follows `config.mouse` (pane-scroll S3): one effect
//! per actual change, live preview in the menu, Esc reverts, save keeps the
//! previewed state.

use super::*;

fn off() -> Config {
    Config {
        mouse: false,
        ..Config::default()
    }
}

/// Two tabs (opening the menu never resizes) with the startup sync done.
fn synced(config: &Config) -> App {
    let mut a = launched(80, 25).with_config(config);
    a.sync_mouse();
    press(&mut a, "tn");
    a
}

/// Opens the menu and moves to the `Mouse` row.
fn on_the_mouse_row(a: &mut App) {
    open_menu(a);
    a.update(key('j'));
    a.update(key('j'));
}

fn capture(on: bool) -> Vec<Effect> {
    vec![Effect::MouseCapture(on)]
}

// Spec: one write per actual change, none when the value is unchanged.
#[test]
fn the_first_sync_turns_the_capture_on_by_default_and_the_next_is_silent() {
    let mut a = launched(80, 25);
    assert_eq!(a.sync_mouse(), Some(Effect::MouseCapture(true)));
    assert_eq!(a.sync_mouse(), None);
}

// Spec: `mouse = false` never writes a capture sequence at startup.
#[test]
fn a_config_with_the_mouse_off_never_asks_for_the_capture() {
    let mut a = launched(80, 25).with_config(&off());
    assert_eq!(a.sync_mouse(), None);
    assert_eq!(a.sync_mouse(), None);
}

// Spec: Mouse cycles and toggles capture live.
#[test]
fn cycling_the_mouse_row_toggles_the_capture_once_per_change_with_no_pty_effect() {
    let mut a = synced(&Config::default());
    assert_eq!(open_menu(&mut a), vec![Effect::DetectEngines]);
    a.update(key('j'));
    a.update(key('j'));
    assert_eq!(a.update(key('l')), capture(false));
    assert_eq!(a.update(key('l')), capture(true));
    assert_eq!(a.update(key('h')), capture(false));
}

#[test]
fn moving_over_the_other_rows_never_touches_the_capture() {
    let mut a = synced(&Config::default());
    open_menu(&mut a);
    for k in ['j', 'k', 'j'] {
        assert_eq!(a.update(key(k)), Vec::<Effect>::new());
    }
}

// Spec: Esc reverts mouse capture.
#[test]
fn esc_after_turning_the_mouse_off_enables_the_capture_again_and_writes_nothing() {
    let mut a = synced(&Config::default());
    on_the_mouse_row(&mut a);
    a.update(key('l'));
    assert!(!a.config.mouse);
    assert_eq!(a.update(esc()), capture(true));
    assert!(a.config.mouse);
    assert!(a.menu().is_none());
    assert_eq!(a.input(), InputMode::Terminal);
}

#[test]
fn esc_with_the_mouse_untouched_emits_no_capture() {
    let mut a = synced(&Config::default());
    open_menu(&mut a);
    assert_eq!(a.update(esc()), Vec::<Effect>::new());
}

#[test]
fn esc_after_turning_the_mouse_on_from_a_config_with_it_off_disables_the_capture() {
    let mut a = synced(&off());
    on_the_mouse_row(&mut a);
    assert_eq!(a.update(key('l')), capture(true));
    assert_eq!(a.update(esc()), capture(false));
}

// Spec: Save keeps the previewed state.
#[test]
fn saving_keeps_the_previewed_capture_and_writes_the_root_key() {
    let mut a = synced(&Config::default());
    on_the_mouse_row(&mut a);
    a.update(key('l'));
    let edit = ConfigEdit {
        table: "",
        key: "mouse",
        value: "false".into(),
    };
    assert_eq!(a.update(key('s')), vec![Effect::SaveConfig(vec![edit])]);
    assert_eq!(a.update(AppEvent::ConfigSaved), Vec::<Effect>::new());
    assert!(a.menu().is_none());
    assert!(!a.config.mouse);
    assert_eq!(a.sync_mouse(), None, "capture stays off, nothing to resend");
}

// Spec: Failed save keeps the preview.
#[test]
fn a_failed_save_keeps_the_menu_and_the_previewed_capture_until_esc() {
    let mut a = synced(&Config::default());
    on_the_mouse_row(&mut a);
    a.update(key('l'));
    a.update(key('s'));
    assert_eq!(
        a.update(AppEvent::ConfigSaveFailed("save failed: x".into())),
        Vec::<Effect>::new()
    );
    assert!(a.menu().is_some());
    assert!(!a.config.mouse);
    assert_eq!(a.sync_mouse(), None);
    assert_eq!(a.update(esc()), capture(true));
}

// An unchanged draft closes without a save and without a capture effect.
#[test]
fn a_menu_without_a_mouse_change_saves_nothing_about_the_mouse() {
    let mut a = synced(&Config::default());
    open_menu(&mut a);
    assert_eq!(a.update(key('s')), Vec::<Effect>::new());
}

// A toggled-back draft is a no-op for the file, and the capture is back on.
#[test]
fn toggling_off_and_on_again_writes_no_mouse_key() {
    let mut a = synced(&Config::default());
    on_the_mouse_row(&mut a);
    a.update(key('l'));
    a.update(key('l'));
    assert_eq!(a.update(key('s')), Vec::<Effect>::new());
}
