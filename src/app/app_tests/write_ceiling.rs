//! The `/write` grant reaches the next spawn (modal-input: Write on from the
//! chat is a user action; agent-orchestrator: Menu ceiling for worker
//! toolsets). The effective menu is the saved one plus Write while the grant
//! holds, and nothing else.

use std::path::PathBuf;

use super::chat_live::{ask, chat_of};
use super::github_grant::{finish, send, tools_app};
use super::orchestra_run::alerted;
use super::orchestra_send::{orch_app, orchestrated, prompt_ready, toggle};
use super::write_grant::{dialog, granted, hold, lines, submit, wait_for_arming};
use super::*;
use crate::core::ai::{
    chat::approval::WRITE_GRANTED_LINE,
    config::ToolToggle,
    orchestra::Toolset,
    tools::registry::{Family, FamilySet},
};

/// The `OrchCeiling` of `effects` for the chat: exactly one.
fn ceiling_of(effects: &[Effect]) -> FamilySet {
    let all: Vec<FamilySet> = (effects.iter())
        .filter_map(|e| match e {
            Effect::OrchCeiling { pane, ceiling } if *pane == id2() => Some(*ceiling),
            _ => None,
        })
        .collect();
    match all.as_slice() {
        [one] => *one,
        _ => panic!("one OrchCeiling: {effects:?}"),
    }
}

/// An orchestrating chat that holds the grant.
fn orch_with_grant() -> App {
    let mut a = orch_app();
    toggle(&mut a);
    assert!(a.orchestrating(id2()));
    hold(&mut a);
    a
}

// Security: the effective menu is the saved menu plus Write and nothing else,
// and only while tools are on.
#[test]
fn the_effective_menu_is_the_saved_menu_plus_write_and_nothing_else() {
    let mut a = orch_app();
    assert_eq!(a.orch_ceiling(id2()), a.ai.families());
    hold(&mut a);
    let held = a.orch_ceiling(id2());
    assert_eq!(held, a.ai.families().with(Family::Write));
    for other in [Family::Exec, Family::Git, Family::GitWrite, Family::Github] {
        assert!(!held.contains(other), "{other:?}");
    }
    // The role still decides: a read-only toolset never widens.
    for toolset in [Toolset::ReadOnly, Toolset::Orchestrator] {
        assert!(!toolset.capped(held).contains(Family::Write), "{toolset:?}");
    }
    assert!(Toolset::Code.capped(held).contains(Family::Write));
    // Tools off: the grant is over.
    a.ai.set_tools_enabled(false);
    assert_eq!(a.orch_ceiling(id2()), a.ai.families());
    // A pane that is gone has no grant.
    assert_eq!(a.orch_ceiling(PaneId::FIRST), a.ai.families());
}

// Spec ("Turning Write on lets the spawn through", /write variant): y in an
// orchestrating chat tells the run, with the menu plus Write.
#[test]
fn y_in_an_orchestrating_chat_updates_the_live_run() {
    let mut a = orch_app();
    toggle(&mut a);
    dialog(&mut a);
    wait_for_arming();
    let effects = a.update(key('y'));
    assert_eq!(
        ceiling_of(&effects),
        a.ai.families().with(Family::Write),
        "{effects:?}"
    );
    // A decline changes nothing for the run.
    let mut a = orch_app();
    toggle(&mut a);
    dialog(&mut a);
    wait_for_arming();
    assert_eq!(a.update(key('n')), vec![]);
}

// Guard: a plain chat has no run to tell.
#[test]
fn y_in_a_plain_chat_sends_no_effect() {
    let mut a = tools_app();
    dialog(&mut a);
    wait_for_arming();
    assert_eq!(a.update(key('y')), vec![]);
    assert!(granted(&a));
}

// Spec: every end of the grant lowers the run's menu again.
#[test]
fn every_end_of_the_grant_lowers_the_run_menu() {
    let saved = |a: &App| a.ai.families();
    // A folder change.
    let mut a = orch_with_grant();
    let effects = a.apply_folder(id2(), PathBuf::from("/tmp"));
    assert_eq!(ceiling_of(&effects), saved(&a));
    // Tools off.
    let mut a = orch_with_grant();
    a.ai.set_tools_enabled(false);
    let effects = a.sync_tools();
    assert_eq!(ceiling_of(&effects), saved(&a));
    // No grant, nothing to lower.
    let mut a = orch_app();
    toggle(&mut a);
    let effects = a.apply_folder(id2(), PathBuf::from("/tmp"));
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::OrchCeiling { .. })),
        "{effects:?}"
    );
}

// The saved Write toggle coming on ends the grant, and the run is told the
// menu it has now (Write is in it).
#[test]
fn a_save_that_turns_write_on_lowers_nothing_below_the_menu() {
    let mut a = orch_with_grant();
    let mut draft = a.ai.clone();
    draft.set_toggle(ToolToggle::Write, true);
    a.menu = Some(MenuState::open(&a.config).with_ai(draft, a.engines.clone()));
    a.input = InputMode::Menu;
    let effects = a.update(AppEvent::ConfigSaved);
    let ceiling = effects
        .iter()
        .find_map(|e| match e {
            Effect::OrchCeiling { ceiling, .. } => Some(*ceiling),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{effects:?}"));
    assert!(ceiling.contains(Family::Write), "the menu has Write now");
    assert!(!granted(&a));
}

// Spec: a turn carries the effective menu, and the coordinator keeps its own
// toolset whatever the grant says.
#[test]
fn an_orchestrator_turn_carries_the_menu_plus_the_grant() {
    let mut a = orch_with_grant();
    let effects = ask(&mut a, "plan it");
    let (_, _, request, orch) = orchestrated(&effects).expect("a turn");
    assert_eq!(orch.ceiling, a.ai.families().with(Family::Write));
    assert_eq!(request.families, Toolset::Orchestrator.families());
    assert!(!a.ai.toggle(ToolToggle::Write), "the saved toggle is off");
}

// A retry or a relaunch answered under a grant follows the grant.
#[test]
fn the_answer_to_a_failure_carries_the_grant() {
    let (mut a, _) = alerted();
    a.ai.set_tools_enabled(true);
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .set_write_granted(true);
    let effects = a.update(key('r'));
    let [Effect::OrchDecide { ceiling, .. }] = effects.as_slice() else {
        panic!("{effects:?}")
    };
    assert_eq!(*ceiling, a.ai.families().with(Family::Write));
}

// Spec (chat-pane): ORCH on says the step from the chat while Write is off,
// and says nothing while the chat holds the grant.
#[test]
fn turning_orch_on_names_the_chat_step_unless_the_chat_holds_the_grant() {
    let mut off = orch_app();
    toggle(&mut off);
    let said = lines(&off);
    assert!(said.iter().any(|l| l.contains("/write")), "{said:?}");
    assert!(said.iter().all(|l| l.is_ascii()), "{said:?}");
    let mut on = orch_app();
    hold(&mut on);
    toggle(&mut on);
    let said = lines(&on);
    assert!(!said.iter().any(|l| l.contains("/write")), "{said:?}");
    assert!(!said.iter().any(|l| l.contains("need Write")), "{said:?}");
}

// Spec (9.4): a plain chat is widened for its next turn, by the user's own y,
// not for the one that runs, and loses it where the grant ends.
#[test]
fn a_plain_chat_next_turn_is_widened_by_the_grant() {
    let mut a = tools_app();
    let (turn, running) = send(&mut a);
    assert!(!running.families.contains(Family::Write));
    hold(&mut a);
    finish(&mut a, turn);
    let (turn, next) = send(&mut a);
    assert_eq!(next.families, a.ai.families().with(Family::Write));
    assert_eq!(next.families, a.offered_families(id2()));
    assert!(!a.ai.toggle(ToolToggle::Write), "the saved toggle is off");
    finish(&mut a, turn);
    a.apply_folder(id2(), PathBuf::from("/tmp"));
    a.update(prompt_ready(id2()));
    let (_, last) = send(&mut a);
    assert!(!last.families.contains(Family::Write));
    assert!(lines(&a).contains(&WRITE_GRANTED_LINE.to_owned()));
}

// Guard: the grant does not make a plain chat orchestrate, and the command in
// the composer is not sent as text.
#[test]
fn the_command_text_never_reaches_the_request() {
    let mut a = tools_app();
    hold(&mut a);
    let effects = submit(&mut a, "hello");
    let [Effect::ChatSend { request, .. }] = effects.as_slice() else {
        panic!("{effects:?}")
    };
    assert!(
        request
            .messages
            .iter()
            .all(|m| !m.content.contains("/write"))
    );
    assert!(!chat_of(&a).orch());
}
