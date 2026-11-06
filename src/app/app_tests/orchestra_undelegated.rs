//! The undelegated-reply notice (agent-orchestrator, "An undelegated
//! coordinator reply is disclosed [W1]"; chat-pane, "Notice repeats"): the run
//! manager says a turn ended with no worker; the pane shows one neutral line
//! once it is idle, and drops it when the turn failed or was cancelled.

use super::chat_live::{ask, chat_id, chat_of, ctrl, delta, finish};
use super::orchestra_run::{RUN, event, row, running};
use super::*;
use crate::core::ai::chat::{
    Entry,
    engine::{ChatError, StreamEvent},
};
use crate::core::ai::orchestra::{RunId, worker::WorkerStatus};

const NOTICE: &str = "no worker ran, nothing written";

fn undelegated() -> AppEvent {
    event(OrchestraEvent::Undelegated { run: RUN })
}

fn shown(a: &App) -> usize {
    (chat_of(a).entries().iter())
        .filter(|e| matches!(e, Entry::System(l) if l.starts_with("no worker ran")))
        .count()
}

fn fail(turn: u64) -> AppEvent {
    let error = StreamEvent::Error(ChatError::Failed("401 unauthorized".into()));
    AppEvent::Chat(chat_id(), turn, error)
}

fn starts_a_turn(effects: &[Effect]) -> bool {
    effects
        .iter()
        .any(|e| matches!(e, Effect::OrchestrateSend { .. } | Effect::ChatSend { .. }))
}

// Spec "Text-only turn gets the notice": the event comes while the turn's last
// words are still on screen, so it waits; the line shows once the pane is idle,
// and no further turn starts by itself.
#[test]
fn a_text_only_turn_gets_the_notice_when_the_pane_goes_idle() {
    let (mut a, turn) = running();
    a.update(delta(turn, "here is the answer"));
    a.update(undelegated());
    assert_eq!(shown(&a), 0, "parked while the turn runs");
    let effects = a.update(finish(turn));
    assert_eq!(shown(&a), 1);
    assert!(!starts_a_turn(&effects), "{effects:?}");
}

// The turn's end may come first: the pane is idle when the event arrives.
#[test]
fn the_notice_shows_at_once_when_the_pane_is_already_idle() {
    let (mut a, turn) = running();
    a.update(finish(turn));
    let effects = a.update(undelegated());
    assert_eq!(shown(&a), 1);
    assert!(!starts_a_turn(&effects), "{effects:?}");
}

// Spec "A legitimate answer gets the same neutral notice": plain ASCII, no
// accusation, and the information only.
#[test]
fn the_notice_is_neutral_ascii_and_names_no_key() {
    let (mut a, turn) = running();
    a.update(finish(turn));
    a.update(undelegated());
    let text = (chat_of(&a).entries().iter())
        .find_map(|e| match e {
            Entry::System(l) if l.starts_with("no worker ran") => Some(l.clone()),
            _ => None,
        })
        .expect("the notice");
    assert!(text.is_ascii(), "{text}");
    assert_eq!(text, NOTICE, "no key is advertised before the key exists");
    assert!(!text.contains("Enter") && !text.contains("Esc"), "{text}");
    for accusation in ["should", "wrong", "fail", "refus", "ignored", "did not"] {
        assert!(!text.contains(accusation), "{text}");
    }
}

// Spec "Notice repeats": the condition in two turns shows the line twice.
#[test]
fn the_notice_is_shown_after_each_such_turn() {
    let (mut a, turn) = running();
    a.update(undelegated());
    a.update(finish(turn));
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    let next = match ask(&mut a, "and now?").as_slice() {
        [.., Effect::OrchestrateSend { turn, .. }] => *turn,
        other => panic!("an orchestrator turn: {other:?}"),
    };
    a.update(undelegated());
    a.update(finish(next));
    assert_eq!(shown(&a), 2);
}

// A failed turn shows no notice, whichever of the two events came first.
#[test]
fn a_failed_turn_drops_the_notice() {
    let (mut a, turn) = running();
    a.update(undelegated());
    a.update(fail(turn));
    assert_eq!(shown(&a), 0, "the parked notice is dropped");

    let (mut a, turn) = running();
    a.update(fail(turn));
    a.update(undelegated());
    assert_eq!(shown(&a), 0, "a late notice is dropped");
}

// Ctrl+C cancels the run: its parked notice goes with it.
#[test]
fn a_cancelled_run_drops_the_notice() {
    let (mut a, turn) = running();
    a.update(undelegated());
    a.update(ctrl('c'));
    a.update(finish(turn));
    assert_eq!(shown(&a), 0);
}

// chat-pane, "Notice is a system line, not history": the next orchestrator
// turn carries the user's words and the coordinator's, not the notice.
#[test]
fn the_notice_stays_out_of_the_next_orchestrator_turn() {
    let (mut a, turn) = running();
    a.update(delta(turn, "all done"));
    a.update(undelegated());
    a.update(finish(turn));
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    assert_eq!(shown(&a), 1, "still shown");
    let request = match ask(&mut a, "next").as_slice() {
        [.., Effect::OrchestrateSend { request, .. }] => request.clone(),
        other => panic!("an orchestrator turn: {other:?}"),
    };
    let sent: Vec<&str> = request
        .messages
        .iter()
        .map(|m| m.content.as_str())
        .collect();
    assert_eq!(sent, ["plan it", "all done", "next"]);
}

fn interrupt(turn: u64) -> AppEvent {
    AppEvent::Chat(chat_id(), turn, StreamEvent::Error(ChatError::Interrupted))
}

// An interrupted turn is not a clean end, whichever event the app hears first.
#[test]
fn an_interrupted_turn_drops_the_notice_in_either_order() {
    let (mut a, turn) = running();
    a.update(undelegated());
    a.update(interrupt(turn));
    assert_eq!(shown(&a), 0, "parked first");

    let (mut a, turn) = running();
    a.update(interrupt(turn));
    a.update(undelegated());
    assert_eq!(shown(&a), 0, "the end first");
}

// The user sends the next turn before the last one's notice lands, and that
// turn spawns a worker: the old turn's notice must not show after it.
#[test]
fn a_notice_that_lands_during_the_next_turn_is_dropped_when_it_has_workers() {
    let (mut a, turn) = running();
    a.update(finish(turn));
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    let next = match ask(&mut a, "and now?").as_slice() {
        [.., Effect::OrchestrateSend { turn, .. }] => *turn,
        other => panic!("an orchestrator turn: {other:?}"),
    };
    a.update(undelegated());
    a.update(event(row(RunId(8), 1, WorkerStatus::Running)));
    a.update(finish(next));
    assert_eq!(shown(&a), 0);
}

// The pane is idle and already shows a newer run when an older run's notice
// lands late: the notice belongs to that older run and is dropped.
#[test]
fn a_late_notice_of_an_older_run_is_dropped_on_an_idle_pane() {
    let (mut a, turn) = running();
    a.update(event(row(RunId(8), 1, WorkerStatus::Running)));
    a.update(finish(turn));
    a.update(undelegated());
    assert_eq!(shown(&a), 0);
}

// A parked notice does not outlive the turn it was parked for: a new
// orchestrator turn clears it.
#[test]
fn a_new_orchestrator_turn_clears_a_stale_parked_notice() {
    let (mut a, turn) = running();
    a.update(undelegated());
    a.panes
        .get_mut(&chat_id())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .set_orch_turn(turn + 1);
    assert_eq!(
        (a.panes.get_mut(&chat_id()))
            .and_then(PaneState::chat_mut)
            .and_then(ChatState::take_undelegated),
        None
    );
}
