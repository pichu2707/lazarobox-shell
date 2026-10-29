//! The one-time provider notice of an orchestrating pane (agent-orchestrator
//! D12): shown when a run starts, never blocking it; Enter accepts and
//! saves, Esc dismisses, only with an empty composer. State asserts only.

use super::chat_live::{ask, chat_id, chat_of, composer, finish, plain, repeat, type_text};
use super::orchestra_send::{orch_ai, orch_app, orch_app_with, toggle};
use super::*;
use crate::core::config::ConfigEdit;

fn acceptance() -> Effect {
    Effect::SaveConfig(vec![ConfigEdit::orchestrator_flag("acknowledged", true)])
}

fn orchestrates(effects: &[Effect]) -> bool {
    effects
        .iter()
        .any(|e| matches!(e, Effect::OrchestrateSend { .. }))
}

/// An orchestrating pane whose first run just started, the notice not yet
/// accepted.
fn noticed() -> App {
    let mut a = orch_app();
    toggle(&mut a);
    let effects = ask(&mut a, "plan it");
    assert!(orchestrates(&effects), "{effects:?}");
    a
}

/// Ends the run in flight and starts the next one.
fn next_run(a: &mut App) -> Vec<Effect> {
    a.update(finish(chat_of(a).turn()));
    ask(a, "again")
}

// agent-orchestrator / Shown once, never blocks: the run starts without
// waiting for the key.
#[test]
fn the_notice_is_shown_when_the_first_run_starts_and_does_not_hold_it() {
    let a = noticed();
    assert!(a.orch_notice(chat_id()));
    assert!(chat_of(&a).orch_turn_in_flight(), "the run started");
    assert!(!a.ai().orchestrator.acknowledged);
}

// agent-orchestrator / Shown once, never blocks; modal-input / Notice keys;
// config-menu / Accept persists.
#[test]
fn enter_with_an_empty_composer_accepts_and_saves_the_notice() {
    {
        let mut a = noticed();
        let effects = a.update(plain(KeyCode::Enter));
        assert_eq!(effects, vec![acceptance()]);
        assert!(!a.orch_notice(chat_id()), "the notice is gone");
        assert!(a.ai().orchestrator.acknowledged);
        assert_eq!(composer(&a), "", "nothing was typed");
        assert!(chat_of(&a).orch_turn_in_flight(), "the run goes on");
        assert_eq!(a.update(AppEvent::ConfigSaved), Vec::new());
        // agent-orchestrator / Not shown again.
        assert!(orchestrates(&next_run(&mut a)));
        assert!(!a.orch_notice(chat_id()));
    }
}

// modal-input / Notice keys (user decision, 2026-10-29: Enter only): `y`
// with an empty composer is typed, and the notice stays unaccepted.
#[test]
fn y_is_typed_and_does_not_accept_the_notice() {
    let mut a = noticed();
    let effects = a.update(key('y'));
    assert!(!effects.contains(&acceptance()), "{effects:?}");
    assert_eq!(composer(&a), "y", "`y` went to the composer");
    assert!(a.orch_notice(chat_id()), "still shown");
    assert!(!a.ai().orchestrator.acknowledged);
}

// agent-orchestrator / Not shown again.
#[test]
fn an_accepted_notice_is_not_shown_at_a_run() {
    let mut ai = orch_ai();
    ai.orchestrator.acknowledged = true;
    let mut a = orch_app_with(ai);
    toggle(&mut a);
    assert!(orchestrates(&ask(&mut a, "plan it")));
    assert!(!a.orch_notice(chat_id()));
}

// modal-input / Notice keys; config-menu / Dismiss does not accept;
// agent-orchestrator / Not accepted is shown again.
#[test]
fn esc_dismisses_without_accepting_and_the_next_run_shows_it_again() {
    let mut a = noticed();
    assert_eq!(a.update(plain(KeyCode::Esc)), Vec::new());
    assert!(!a.orch_notice(chat_id()));
    assert!(!a.ai().orchestrator.acknowledged);
    assert!(chat_of(&a).orch_turn_in_flight(), "Esc answered the notice");
    assert!(orchestrates(&next_run(&mut a)));
    assert!(a.orch_notice(chat_id()), "shown again");
}

// modal-input / Typed Enter does not accept: with text in the composer every
// key goes to the composer and its usual action.
#[test]
fn keys_typed_into_a_message_never_answer_the_notice() {
    let mut a = noticed();
    type_text(&mut a, "say yes");
    assert_eq!(composer(&a), "say yes", "`y` was typed, not taken");
    let effects = a.update(plain(KeyCode::Enter));
    assert!(!effects.contains(&acceptance()), "{effects:?}");
    a.update(plain(KeyCode::Esc));
    assert!(a.orch_notice(chat_id()), "still shown");
    assert!(!a.ai().orchestrator.acknowledged);
}

// modal-input / Notice keys: a held key does not answer it.
#[test]
fn a_held_enter_does_not_answer_the_notice() {
    let mut a = noticed();
    assert_eq!(
        a.update(repeat(KeyCode::Enter, KeyModifiers::NONE)),
        Vec::new()
    );
    assert!(a.orch_notice(chat_id()));
    assert!(!a.ai().orchestrator.acknowledged);
}

// S7: a pane that does not orchestrate never shows the notice, and its Esc
// still cancels its turn.
#[test]
fn a_chat_turn_shows_no_notice_and_esc_still_cancels_it() {
    let mut a = orch_app();
    let effects = ask(&mut a, "hello");
    assert!(!orchestrates(&effects), "{effects:?}");
    assert!(!a.orch_notice(chat_id()));
    let effects = a.update(plain(KeyCode::Esc));
    assert!(
        effects.contains(&Effect::ChatCancel(chat_id())),
        "{effects:?}"
    );
    assert!(!a.ai().orchestrator.acknowledged);
}

// The notice's save is answered to the notice: it never closes, or shows its
// error in, a menu opened while it was written.
#[test]
fn the_notice_save_result_leaves_a_menu_opened_meanwhile_open() {
    let mut a = noticed();
    a.update(plain(KeyCode::Enter));
    open_menu(&mut a);
    assert!(a.menu().is_some());
    assert_eq!(a.update(AppEvent::ConfigSaved), Vec::new());
    assert!(a.menu().is_some(), "the menu is still open");
}

// config-menu / Accept persists: a failed write did not persist, so the
// notice is not accepted and is shown at the next run; the error is told.
#[test]
fn a_failed_notice_save_leaves_it_unaccepted_and_says_why() {
    let mut a = noticed();
    a.update(plain(KeyCode::Enter));
    a.update(AppEvent::ConfigSaveFailed("save failed: disk full".into()));
    assert!(!a.ai().orchestrator.acknowledged);
    assert_eq!(a.notice(), Some("save failed: disk full"));
    assert!(orchestrates(&next_run(&mut a)));
    assert!(a.orch_notice(chat_id()));
}
