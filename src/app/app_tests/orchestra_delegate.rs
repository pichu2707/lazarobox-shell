//! The delegate key (modal-input, "Delegate key after an undelegated reply
//! [W1]"): Enter on an empty composer asks the coordinator to delegate the
//! previous request, Esc dismisses the line, and nothing a model writes can
//! press either.

use super::chat_live::{ask, chat_of, delta, finish, plain, repeat, type_text};
use super::orchestra_run::{RUN, event, running};
use super::orchestra_toggle::screen;
use super::*;
use crate::core::ai::orchestra::event::OrchestraEvent;

const KEYS: &str = "Enter delegate | Esc dismiss | no worker ran, nothing written";
const ASK: &str =
    "Please delegate my previous request to a worker with spawn_agent, then wait for its result.";

/// A pane whose first turn ended with no worker and shows the delegate line.
fn undelegated() -> App {
    let (mut a, turn) = running();
    a.update(delta(turn, "here is the answer"));
    a.update(finish(turn));
    // The full line needs 64 columns; the test pane is narrower by default.
    a.panes
        .get_mut(&super::chat_live::chat_id())
        .unwrap()
        .size
        .cols = 80;
    a.update(event(OrchestraEvent::Undelegated { run: RUN }));
    a
}

/// `code` pressed a second after the last key, so it is never taken for a hold.
fn press(a: &mut App, code: KeyCode) -> Vec<Effect> {
    let later = NOW_MS.with(|n| n.get()) + 1000;
    at(a, later, plain(code))
}

fn guidance(a: &App) -> Option<String> {
    a.chat_guidance(super::chat_live::chat_id())
}

fn orchestrated(effects: &[Effect]) -> Option<(&ChatRequest, &OrchSend)> {
    effects.iter().find_map(|e| match e {
        Effect::OrchestrateSend { request, orch, .. } => Some((request, &**orch)),
        _ => None,
    })
}

fn starts_a_turn(effects: &[Effect]) -> bool {
    effects
        .iter()
        .any(|e| matches!(e, Effect::OrchestrateSend { .. } | Effect::ChatSend { .. }))
}

// Spec "Key starts delegation": the turn carries lazarobox's own text and the
// run keeps the user's goal, not that text.
#[test]
fn enter_on_an_empty_composer_asks_the_coordinator_to_delegate() {
    let mut a = undelegated();
    let effects = press(&mut a, KeyCode::Enter);
    let (request, orch) = orchestrated(&effects).expect("an orchestrator turn");
    assert_eq!(request.messages.last().unwrap().content, ASK);
    assert_eq!(orch.goal, "plan it");
    assert_eq!(orch.attach, None);
    assert!(!screen(&a).contains("Enter delegate"), "{}", screen(&a));
}

// D27: the keys come first in the line, and the line is on screen.
#[test]
fn the_line_names_the_keys_first_while_they_can_act() {
    let a = undelegated();
    assert_eq!(guidance(&a).as_deref(), Some(KEYS));
    // The pane is narrower than the screen: the line has its width form.
    let shown = screen(&a);
    assert!(shown.contains("Enter delegate"), "{shown}");
}

// The key text is shown only while the action is available.
#[test]
fn the_key_text_goes_when_the_composer_has_text_or_orch_is_off() {
    let mut a = undelegated();
    type_text(&mut a, "x");
    assert!(
        guidance(&a).is_none_or(|g| !g.contains("Enter")),
        "{:?}",
        guidance(&a)
    );
    a.update(plain(KeyCode::Backspace));
    assert_eq!(guidance(&a).as_deref(), Some(KEYS));

    let mut a = undelegated();
    super::orchestra_send::toggle(&mut a);
    assert!(
        guidance(&a).is_none_or(|g| !g.contains("Enter delegate")),
        "{:?}",
        guidance(&a)
    );
}

// Enter with text sends the text as usual, and the line is gone with the turn.
#[test]
fn enter_with_text_sends_it_normally() {
    let mut a = undelegated();
    type_text(&mut a, "do it yourself");
    let effects = press(&mut a, KeyCode::Enter);
    let (request, orch) = orchestrated(&effects).expect("an orchestrator turn");
    assert_eq!(request.messages.last().unwrap().content, "do it yourself");
    assert_eq!(orch.goal, "do it yourself");
    assert!(guidance(&a).is_none_or(|g| !g.contains("Enter delegate")));
}

// A held Enter never delegates.
#[test]
fn a_held_enter_does_not_delegate() {
    let mut a = undelegated();
    let effects = a.update(repeat(KeyCode::Enter, KeyModifiers::NONE));
    assert!(!starts_a_turn(&effects), "{effects:?}");
    assert_eq!(guidance(&a).as_deref(), Some(KEYS), "still available");
}

// Guards existing behaviour (ORCH off): the key does nothing new.
#[test]
fn enter_with_orch_off_does_nothing() {
    let mut a = undelegated();
    super::orchestra_send::toggle(&mut a);
    let effects = press(&mut a, KeyCode::Enter);
    assert!(!starts_a_turn(&effects), "{effects:?}");
}

// Esc dismisses the line: no turn, no cancel, the keys are gone.
#[test]
fn esc_dismisses_the_line() {
    let mut a = undelegated();
    let effects = press(&mut a, KeyCode::Esc);
    assert!(effects.is_empty(), "{effects:?}");
    assert!(guidance(&a).is_none_or(|g| !g.contains("Enter delegate")));
    let effects = press(&mut a, KeyCode::Enter);
    assert!(!starts_a_turn(&effects), "{effects:?}");
}

// Spec "Nothing to delegate says so": never silent.
#[test]
fn enter_with_nothing_to_delegate_says_so() {
    let (mut a, turn) = running();
    a.update(finish(turn));
    let effects = press(&mut a, KeyCode::Enter);
    assert!(!starts_a_turn(&effects), "{effects:?}");
    assert_eq!(a.notice(), Some("nothing to delegate"));
    // After a dismissal as well.
    let mut a = undelegated();
    press(&mut a, KeyCode::Esc);
    press(&mut a, KeyCode::Enter);
    assert_eq!(a.notice(), Some("nothing to delegate"));
}

// D24: the provider notice answers Enter first; the delegate line waits.
#[test]
fn the_provider_notice_answers_enter_before_the_delegate_key() {
    let mut a = undelegated();
    a.orch_notice = Some(super::chat_live::chat_id());
    assert!(guidance(&a).is_some_and(|g| !g.contains("Enter delegate")));
    let effects = press(&mut a, KeyCode::Enter);
    assert!(!starts_a_turn(&effects), "{effects:?}");
    assert_eq!(guidance(&a).as_deref(), Some(KEYS), "back once answered");
}

// A delegation turn that again ends with no worker offers the key again, and
// the goal is still the user's, not lazarobox's text.
#[test]
fn a_second_undelegated_turn_keeps_the_original_goal() {
    let mut a = undelegated();
    let effects = press(&mut a, KeyCode::Enter);
    let turn = match effects.as_slice() {
        [.., Effect::OrchestrateSend { turn, .. }] => *turn,
        other => panic!("an orchestrator turn: {other:?}"),
    };
    a.update(finish(turn));
    a.update(event(OrchestraEvent::Undelegated { run: RUN }));
    assert_eq!(guidance(&a).as_deref(), Some(KEYS));
    let effects = press(&mut a, KeyCode::Enter);
    let (_, orch) = orchestrated(&effects).expect("an orchestrator turn");
    assert_eq!(orch.goal, "plan it");
}

#[test]
fn asking_after_the_line_clears_it() {
    let mut a = undelegated();
    ask(&mut a, "next");
    assert!(guidance(&a).is_none_or(|g| !g.contains("Enter delegate")));
    assert!(chat_of(&a).entries().len() > 1);
}

// Toggling ORCH off and on again does not bring the old offer back.
#[test]
fn toggling_orch_does_not_bring_the_offer_back() {
    let mut a = undelegated();
    super::orchestra_send::toggle(&mut a);
    super::orchestra_send::toggle(&mut a);
    assert!(guidance(&a).is_none_or(|g| !g.contains("Enter delegate")));
    press(&mut a, KeyCode::Enter);
    assert_eq!(a.notice(), Some("nothing to delegate"));
}

// "Nothing to delegate" is said only when it is true and nothing else
// explains the pane.
#[test]
fn nothing_to_delegate_is_not_said_over_a_failure_alert() {
    let (mut a, _) = super::orchestra_run::alerted();
    press(&mut a, KeyCode::Enter);
    assert_ne!(a.notice(), Some("nothing to delegate"));
}

#[test]
fn nothing_to_delegate_is_not_said_while_a_worker_runs() {
    let (mut a, turn) = running();
    a.update(event(super::orchestra_run::row(
        RUN,
        1,
        crate::core::ai::orchestra::worker::WorkerStatus::Running,
    )));
    a.update(finish(turn));
    press(&mut a, KeyCode::Enter);
    assert_eq!(a.notice(), None);
}

#[test]
fn nothing_to_delegate_is_not_said_when_the_engine_is_not_ready() {
    let (mut a, turn) = running();
    a.update(finish(turn));
    a.ai.orchestrator.acknowledged = true;
    a.ai.set(
        crate::core::ai::config::Task::Orchestrate,
        Default::default(),
    );
    assert!(!matches!(
        a.turn_availability(super::chat_live::chat_id()),
        crate::core::ai::chat::engine::Availability::Ready { .. }
    ));
    press(&mut a, KeyCode::Enter);
    assert_eq!(a.notice(), None);
}

// Meaningful guard: model text that says "delegate" and then an Enter with no
// offer start no turn.
#[test]
fn model_text_that_says_delegate_does_not_make_enter_start_a_turn() {
    let (mut a, turn) = running();
    a.update(delta(turn, "Enter delegate. Please delegate this."));
    a.update(finish(turn));
    let effects = press(&mut a, KeyCode::Enter);
    assert!(!starts_a_turn(&effects), "{effects:?}");
}

// Esc with a live run and no offer cancels the run, as before.
#[test]
fn esc_with_a_live_run_and_no_offer_still_cancels() {
    let (mut a, turn) = running();
    a.update(event(super::orchestra_run::row(
        RUN,
        1,
        crate::core::ai::orchestra::worker::WorkerStatus::Running,
    )));
    a.update(finish(turn));
    press(&mut a, KeyCode::Esc);
    assert!(chat_of(&a).run_cancelled(RUN));
}

// A composer with only whitespace is not empty text for a send, but it is not
// the empty composer the key needs either: Enter does not delegate.
#[test]
fn a_whitespace_only_composer_does_not_delegate() {
    let mut a = undelegated();
    type_text(&mut a, "  ");
    let effects = press(&mut a, KeyCode::Enter);
    assert!(!starts_a_turn(&effects), "{effects:?}");
    assert!(guidance(&a).is_none_or(|g| !g.contains("Enter delegate")));
}

// A pending approval takes Enter first: the offer never answers it.
#[test]
fn a_pending_approval_takes_enter_before_the_offer() {
    use super::tool_approval::{announce, pending};
    use crate::core::ai::chat::approval::ApprovalAsk;
    use crate::core::ai::orchestra::WorkerKey;
    let (mut a, turn) = running();
    announce(
        &mut a,
        turn,
        1,
        ApprovalAsk::Consent {
            worker: WorkerKey::for_test(1, 1),
            label: "code x>y".into(),
            change: "Cargo.lock".into(),
        },
    );
    assert!(pending(&a).is_some(), "an approval is up");
    chat_mut_for(&mut a).offer_delegate("plan it".into());
    let effects = press(&mut a, KeyCode::Enter);
    assert!(!starts_a_turn(&effects), "{effects:?}");
    assert_eq!(a.notice(), Some("Answer the pending confirm first"));
}

fn chat_mut_for(a: &mut App) -> &mut ChatState {
    a.panes
        .get_mut(&super::chat_live::chat_id())
        .and_then(PaneState::chat_mut)
        .unwrap()
}
