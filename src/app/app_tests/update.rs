//! The update notice in the app: the cached start-up value, the late result
//! of the background check, and what the statusline and the menu get.

use super::*;

fn open_menu(a: &mut App) {
    a.update(ctrl_space());
    a.update(key('m'));
}

#[test]
fn no_update_by_default() {
    assert_eq!(app().status_update(), None);
}

#[test]
fn the_startup_value_is_kept() {
    let a = app().with_update(Some("0.16.0".into()));
    assert_eq!(a.status_update(), Some("0.16.0"));
}

#[test]
fn the_late_result_sets_and_clears_the_notice_and_redraws() {
    let mut a = app();
    a.take_dirty();
    assert_eq!(
        a.update(AppEvent::UpdateChecked(Some("0.16.0".into()))),
        Vec::new()
    );
    assert_eq!(a.status_update(), Some("0.16.0"));
    assert!(a.take_dirty());
    a.update(AppEvent::UpdateChecked(None));
    assert_eq!(a.status_update(), None);
}

#[test]
fn a_menu_opened_later_carries_the_note() {
    let mut a = app().with_update(Some("0.16.0".into()));
    open_menu(&mut a);
    assert!(a.menu().unwrap().update_note().is_some());
}

#[test]
fn a_result_that_arrives_with_the_menu_open_reaches_it() {
    let mut a = app();
    open_menu(&mut a);
    assert!(a.menu().unwrap().update_note().is_none());
    a.update(AppEvent::UpdateChecked(Some("0.16.0".into())));
    assert!(a.menu().unwrap().update_note().is_some());
}

#[test]
fn the_notice_never_asks_for_an_effect() {
    // Notify only: nothing the app emits for an update can start a pane or a process.
    let mut a = app().with_update(Some("0.16.0".into()));
    assert_eq!(
        a.update(AppEvent::UpdateChecked(Some("0.17.0".into()))),
        Vec::new()
    );
}
