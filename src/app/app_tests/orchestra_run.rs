//! The run of an orchestrating pane as the app sees it (agent-orchestrator
//! D1, D5): the run manager's events, the failure alert and its keys, and
//! the run's lifetime; the alert is drawn above the composer.

use super::chat_live::{
    ask, chat_id, chat_of, code, composer, ctrl, delta, finish, plain, ready_app, repeat, type_text,
};
use super::orchestra_notice::NOTICE;
use super::orchestra_send::{orch_ai, orch_app_with, toggle};
use super::orchestra_toggle::screen;
use super::*;
use crate::app::orchestra::OrchHint;
use crate::core::ai::{
    chat::{Entry, OrchAlert},
    orchestra::{
        RunId, WorkerId,
        event::{OrchestraEvent, WorkerRow},
        run::UserChoice,
        worker::{ErrorClass, WorkerStatus},
    },
};

pub(super) const RUN: RunId = RunId(7);

/// An orchestrating pane whose notice was accepted earlier, its first
/// orchestrator turn in flight; returns the turn.
pub(super) fn running() -> (App, u64) {
    let mut ai = orch_ai();
    ai.orchestrator.acknowledged = true;
    let mut a = orch_app_with(ai);
    toggle(&mut a);
    let turn = match ask(&mut a, "plan it").as_slice() {
        [.., Effect::OrchestrateSend { turn, .. }] => *turn,
        other => panic!("an orchestrator turn: {other:?}"),
    };
    (a, turn)
}

pub(super) fn event(event: OrchestraEvent) -> AppEvent {
    AppEvent::Orchestra(chat_id(), event)
}

pub(super) fn row(run: RunId, id: u32, status: WorkerStatus) -> OrchestraEvent {
    OrchestraEvent::Worker(WorkerRow {
        run,
        id: WorkerId(id),
        kind: Task::Review,
        engine: "codex".into(),
        model: Some("gpt-5".into()),
        status,
        elapsed: None,
        usage: None,
        partial: false,
        overridden: false,
    })
}

// Never silent: a `y` that reached no wait (the worker ended first) is said.
#[test]
fn a_lost_answer_is_said_in_the_transcript() {
    let (mut a, _) = running();
    a.update(event(OrchestraEvent::AnswerLost));
    let said = a
        .panes
        .get(&chat_id())
        .and_then(PaneState::chat_state)
        .unwrap()
        .entries()
        .iter()
        .any(|entry| format!("{entry:?}").contains("your answer was not applied"));
    assert!(said);
}

// Spec (configuration): the run limits of `[ai.orchestrator]` travel with the
// orchestrator turn that starts the run.
#[test]
fn the_configured_run_limits_go_with_the_orchestrator_turn() {
    let mut ai = orch_ai();
    ai.orchestrator.max_parallel = 3;
    ai.orchestrator.max_workers = 5;
    ai.orchestrator.run_timeout_mins = 2;
    let mut a = orch_app_with(ai);
    toggle(&mut a);
    let effects = ask(&mut a, "plan it");
    let [.., Effect::OrchestrateSend { orch, .. }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    let limits = orch.limits;
    assert_eq!((limits.parallel, limits.workers), (3, 5));
    assert_eq!(limits.wall, std::time::Duration::from_secs(120));
}

// Spec (agent-orchestrator, Wall clock): the run that reached its limit says
// so, with the limit named; the stale run of another one stays quiet.
#[test]
fn a_run_that_reached_its_wall_clock_says_which_limit() {
    let said = |a: &App| {
        let chat = a.panes.get(&chat_id()).and_then(PaneState::chat_state);
        let text = format!("{:?}", chat.unwrap().entries());
        text.contains("run_timeout_mins (60)")
    };
    let (mut a, _) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    let after = std::time::Duration::from_secs(3600);
    a.update(event(OrchestraEvent::Expired {
        run: RunId(3),
        after,
    }));
    assert!(!said(&a));
    a.update(event(OrchestraEvent::Expired { run: RUN, after }));
    assert!(said(&a));
}

pub(super) fn halted(run: RunId, text: &str) -> OrchestraEvent {
    OrchestraEvent::Halted {
        run,
        worker: WorkerId(1),
        class: ErrorClass::Conflict,
        text: text.into(),
    }
}

/// A run whose worker 1 failed with `409 conflict: session busy`.
pub(super) fn alerted() -> (App, u64) {
    let (mut a, turn) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    a.update(event(row(RUN, 1, WorkerStatus::Failed)));
    assert_eq!(
        a.update(event(halted(RUN, "409 conflict: session busy"))),
        vec![]
    );
    (a, turn)
}

pub(super) fn decide(choice: UserChoice) -> Effect {
    Effect::OrchDecide {
        pane: chat_id(),
        run: RUN,
        choice,
    }
}

pub(super) fn alert(a: &App) -> Option<&OrchAlert> {
    chat_of(a).orch_alert()
}

// agent-orchestrator / Error shown as it came: the alert holds the provider
// message exactly as it came, with the worker and the class.
#[test]
fn a_failure_shows_its_alert_with_the_error_as_it_came() {
    let (a, _) = alerted();
    assert_eq!(
        alert(&a),
        Some(&OrchAlert {
            run: RUN,
            worker: WorkerId(1),
            class: ErrorClass::Conflict,
            text: "409 conflict: session busy".into(),
        })
    );
    assert_eq!(chat_of(&a).orch_run(), Some(RUN));
}

// chat-pane / Alert persists: time, sibling rows and the orchestrator's own
// output leave the alert as it was.
#[test]
fn the_alert_stays_through_time_and_other_output() {
    let (mut a, turn) = alerted();
    let before = alert(&a).cloned();
    a.update(AppEvent::AnimTick);
    a.update(event(row(RUN, 2, WorkerStatus::Running)));
    a.update(event(row(RUN, 2, WorkerStatus::Done)));
    a.update(delta(turn, "waiting for you"));
    a.update(AppEvent::MemoryTick);
    assert_eq!(alert(&a).cloned(), before);
    assert!(before.is_some());
}

// modal-input / Retry key: `r` with an empty composer asks the run manager
// to relaunch the worker; the alert is answered.
#[test]
fn r_retries_the_failed_worker() {
    let (mut a, _) = alerted();
    assert_eq!(a.update(key('r')), vec![decide(UserChoice::Retry)]);
    assert_eq!(alert(&a), None);
    assert_eq!(composer(&a), "", "the key was not typed");
    assert_eq!(chat_of(&a).orch_run(), Some(RUN), "the run goes on");
}

// modal-input / Cancel keys: `c` or Esc cancels the run.
#[test]
fn c_or_esc_cancels_the_run() {
    for cancel in [key('c'), plain(KeyCode::Esc)] {
        let (mut a, _) = alerted();
        assert_eq!(a.update(cancel), vec![decide(UserChoice::Cancel)]);
        assert_eq!(alert(&a), None);
        assert_eq!(composer(&a), "");
        // The run is over once its `Ended` comes.
        a.update(event(OrchestraEvent::Ended { run: RUN }));
        assert_eq!(chat_of(&a).orch_run(), None);
    }
}

// modal-input / Failure alert keys (key audit, 2026-10-29): with text in the
// composer the letters are typed, never taken as an answer.
#[test]
fn letters_typed_into_a_message_never_answer_the_alert() {
    let (mut a, _) = alerted();
    type_text(&mut a, "a");
    for typed in ['r', 'c'] {
        assert_eq!(a.update(key(typed)), vec![]);
    }
    assert_eq!(composer(&a), "arc");
    assert!(alert(&a).is_some(), "still shown");
}

// modal-input / Failure alert keys: other keys do not answer the alert, and
// a held or modified letter does not either.
#[test]
fn other_keys_do_not_answer_the_alert() {
    let (mut a, _) = alerted();
    let effects = a.update(key('x'));
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::OrchDecide { .. })),
        "{effects:?}"
    );
    a.update(plain(KeyCode::Backspace));
    a.update(repeat(KeyCode::Char('r'), KeyModifiers::NONE));
    a.update(code(KeyCode::Char('c'), KeyModifiers::ALT));
    assert!(alert(&a).is_some(), "still shown");
}

// modal-input / Ctrl+C: it cancels the orchestrator turn and the run, alert
// or not, as the chat cancel always did.
#[test]
fn ctrl_c_cancels_the_turn_and_the_run_and_answers_the_alert() {
    let (mut a, _) = alerted();
    let effects = a.update(ctrl('c'));
    assert!(
        effects.contains(&Effect::ChatCancel(chat_id())),
        "{effects:?}"
    );
    assert!(!effects.contains(&decide(UserChoice::Cancel)));
    assert_eq!(alert(&a), None);
}

// modal-input / Ctrl+C: a run outlives its turn, and Ctrl+C or Esc still
// ends it once the turn is over.
#[test]
fn ctrl_c_or_esc_ends_a_run_that_outlived_its_turn() {
    for cancel in [ctrl('c'), plain(KeyCode::Esc)] {
        let (mut a, turn) = running();
        a.update(event(row(RUN, 1, WorkerStatus::Running)));
        a.update(finish(turn));
        assert_eq!(a.update(cancel), vec![Effect::ChatCancel(chat_id())]);
    }
}

// chat-pane / Toggle blocked during a run: the run lasts until its `Ended`,
// past the orchestrator turn that started it (S1e-1a L5 closed).
#[test]
fn the_run_stays_active_after_its_turn_until_it_ends() {
    let (mut a, turn) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    a.update(finish(turn));
    assert_eq!(a.orch_hint(chat_id()), Some(OrchHint::CancelRunFirst));
    assert_eq!(toggle(&mut a), vec![]);
    assert!(chat_of(&a).orch(), "the mode stays on");
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    assert_eq!(a.orch_hint(chat_id()), Some(OrchHint::Toggle));
    toggle(&mut a);
    assert!(!chat_of(&a).orch(), "with the run over the key works again");
}

// A replaced run ends after the new one started: its `Ended` leaves the new
// run, and the new run's alert, in place.
#[test]
fn the_end_of_another_run_leaves_the_current_one() {
    let (mut a, _) = alerted();
    let old = RunId(3);
    a.update(event(OrchestraEvent::Ended { run: old }));
    assert_eq!(chat_of(&a).orch_run(), Some(RUN));
    assert!(alert(&a).is_some());
}

// S6: events for a pane that is not a chat, or no longer exists, change
// nothing.
#[test]
fn events_of_a_pane_that_is_not_a_chat_are_dropped() {
    let mut a = ready_app();
    let shell = PaneId::FIRST;
    assert_ne!(shell, chat_id());
    for gone in [shell, PaneId::for_test(99)] {
        let events = [
            row(RUN, 1, WorkerStatus::Running),
            halted(RUN, "boom"),
            OrchestraEvent::Ended { run: RUN },
        ];
        for e in events {
            assert_eq!(a.update(AppEvent::Orchestra(gone, e)), vec![]);
        }
    }
    assert_eq!(chat_of(&a).orch_run(), None);
    assert_eq!(alert(&a), None);
}

// S6: in a pane that does not orchestrate, Esc and Ctrl+C on an idle chat
// still do nothing, and `r`/`c` are typed.
#[test]
fn an_idle_chat_keeps_its_keys() {
    let mut a = ready_app();
    assert_eq!(a.update(plain(KeyCode::Esc)), vec![]);
    assert_eq!(a.update(ctrl('c')), vec![]);
    a.update(key('r'));
    a.update(key('c'));
    assert_eq!(composer(&a), "rc");
}

// agent-orchestrator / Row follows the status; Not in history: the run's rows
// land in the chat, one per worker, and the next orchestrator turn carries
// none of them.
#[test]
fn worker_rows_follow_their_status_and_stay_out_of_the_next_turn() {
    let (mut a, turn) = running();
    for status in [
        WorkerStatus::Queued,
        WorkerStatus::Running,
        WorkerStatus::Done,
    ] {
        a.update(event(row(RUN, 1, status)));
    }
    let rows: Vec<_> = chat_of(&a)
        .entries()
        .iter()
        .filter_map(|e| match e {
            Entry::Worker(row) => Some(row.status),
            _ => None,
        })
        .collect();
    assert_eq!(rows, [WorkerStatus::Done]);
    a.update(delta(turn, "all done"));
    a.update(finish(turn));
    a.update(event(OrchestraEvent::Ended { run: RUN }));
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

/// What the alert of `alerted` says.
const ALERT: &str = "run paused: worker review codex>gpt-5 failed: \
     409 conflict: session busy \u{b7} r retry \u{b7} m change model \u{b7} p change provider \u{b7} c/Esc cancel";

// chat-pane / Alert content (S1 part; the class and the change options are
// S2): the pane says the run is paused, names the worker, shows the error as
// it came and the keys that answer it.
#[test]
fn the_alert_is_drawn_with_the_worker_the_raw_error_and_its_keys() {
    let (mut a, turn) = alerted();
    assert_eq!(a.chat_guidance(chat_id()).as_deref(), Some(ALERT));
    insta::assert_snapshot!("orch_alert_on_screen", screen(&a));
    // Shown while the orchestrator still streams, and through other rows.
    a.update(delta(turn, "waiting for you"));
    a.update(event(row(RUN, 2, WorkerStatus::Running)));
    assert_eq!(a.chat_guidance(chat_id()).as_deref(), Some(ALERT));
    a.update(key('r'));
    assert_eq!(a.chat_guidance(chat_id()), None, "answered");
}

// The alert shows the status the worker ended with, and a line break in the
// error does not break the row.
#[test]
fn the_alert_names_how_the_worker_ended_on_one_paragraph() {
    let (mut a, _) = running();
    a.update(event(row(RUN, 1, WorkerStatus::TimedOut)));
    a.update(event(halted(RUN, "no answer\nin 600 s")));
    assert_eq!(
        a.chat_guidance(chat_id()).as_deref(),
        Some(
            "run paused: worker review codex>gpt-5 timed_out: no answer in 600 s \
             \u{b7} r retry \u{b7} m change model \u{b7} p change provider \u{b7} c/Esc cancel"
        )
    );
}

// With the notice still up, both are shown, in the order their keys answer:
// nothing that a key would answer is hidden.
#[test]
fn the_notice_and_the_alert_are_shown_together() {
    let mut ai = orch_ai();
    ai.orchestrator.acknowledged = false;
    let mut a = orch_app_with(ai);
    toggle(&mut a);
    ask(&mut a, "plan it");
    a.update(event(row(RUN, 1, WorkerStatus::Failed)));
    a.update(event(halted(RUN, "409 conflict: session busy")));
    assert_eq!(
        a.chat_guidance(chat_id()),
        Some(format!("{NOTICE} | {ALERT}"))
    );
}

// Review S2b: an answer names the run of the alert it answers, so the run
// manager never applies it to another run.
#[test]
fn the_answer_names_the_run_of_its_alert() {
    let (mut a, _) = running();
    a.update(event(row(RunId(9), 1, WorkerStatus::Failed)));
    a.update(event(halted(RunId(9), "boom")));
    assert_eq!(
        a.update(key('r')),
        vec![Effect::OrchDecide {
            pane: chat_id(),
            run: RunId(9),
            choice: UserChoice::Retry,
        }]
    );
}

// never fail silently: a retry the run manager could not apply brings the
// alert back, says why and leaves the keys working.
#[test]
fn a_declined_retry_shows_why_and_the_alert_answers_again() {
    let (mut a, _) = alerted();
    assert_eq!(a.update(key('r')), vec![decide(UserChoice::Retry)]);
    assert_eq!(alert(&a), None);
    a.update(event(OrchestraEvent::Declined {
        run: RUN,
        worker: WorkerId(1),
        class: ErrorClass::Conflict,
        text: "409 conflict: session busy".into(),
        reason: "could not seal the previous error".into(),
    }));
    let text = a.chat_guidance(chat_id()).unwrap_or_default();
    assert!(
        text.ends_with(
            "retry failed: could not seal the previous error \u{b7} r try again \u{b7} m change model \u{b7} p change provider \u{b7} c/Esc cancel"
        ),
        "{text}"
    );
    assert!(text.contains("409 conflict: session busy"), "{text}");
    let_go();
    assert_eq!(a.update(key('r')), vec![decide(UserChoice::Retry)]);
    assert_eq!(alert(&a), None);
    assert!(
        !a.chat_guidance(chat_id())
            .unwrap_or_default()
            .contains("retry failed")
    );
}

// An answer dropped by the run manager is said in the transcript when the
// pane has no other run; the alert of a run the pane has now stays.
#[test]
fn a_dropped_answer_is_said_and_touches_no_other_run() {
    let (mut a, _) = alerted();
    let before = alert(&a).cloned();
    a.update(event(OrchestraEvent::Dropped { run: RUN }));
    assert_eq!(alert(&a).cloned(), before, "the live alert stays");
    assert_eq!(chat_of(&a).orch_run(), Some(RUN));
    assert!(said(&a, MOVED_ON), "{:?}", chat_of(&a).entries());
}

// Without a run manager the answer is told in the transcript.
#[test]
fn an_unavailable_orchestration_is_said() {
    let (mut a, _) = alerted();
    a.update(event(OrchestraEvent::Unavailable));
    assert!(
        said(
            &a,
            "orchestration is not available: your answer was not applied"
        ),
        "{:?}",
        chat_of(&a).entries()
    );
}

fn said(a: &App, line: &str) -> bool {
    (chat_of(a).entries().iter()).any(|e| matches!(e, Entry::System(l) if l == line))
}

const MOVED_ON: &str = "an earlier run already moved on: your answer was not applied";

// No silent no-op: a drop naming another run than the pane's current one is
// said too; the neutral wording keeps it from being read as part of the new one.
#[test]
fn a_dropped_answer_of_an_older_run_is_said_neutrally_in_a_newer_one() {
    let (mut a, _) = alerted();
    a.update(event(OrchestraEvent::Dropped { run: RunId(3) }));
    assert!(said(&a, MOVED_ON), "{:?}", chat_of(&a).entries());
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    a.update(event(OrchestraEvent::Dropped { run: RUN }));
    assert_eq!(
        (chat_of(&a).entries().iter())
            .filter(|e| matches!(e, Entry::System(l) if l == MOVED_ON))
            .count(),
        2
    );
}

// Review fix 2: notes belong to the alert they were raised on. Ctrl+C and
// the run's end clear them, so the next alert does not say `retry failed`.
#[test]
fn a_note_does_not_leak_into_the_next_alert() {
    let declined = |a: &mut App| {
        a.update(key('r'));
        a.update(event(OrchestraEvent::Declined {
            run: RUN,
            worker: WorkerId(1),
            class: ErrorClass::Conflict,
            text: "409 conflict: session busy".into(),
            reason: "no nonce".into(),
        }));
        let_go();
    };
    // Ctrl+C
    let (mut a, _) = alerted();
    declined(&mut a);
    a.update(ctrl('c'));
    a.update(event(halted(RUN, "409 conflict: session busy")));
    assert_eq!(chat_of(&a).orch_note(), None, "ctrl+c cleared the note");
    // The run ends.
    let (mut a, _) = alerted();
    declined(&mut a);
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    assert_eq!(chat_of(&a).orch_note(), None, "the end cleared the note");
    a.update(event(halted(RunId(8), "409 conflict: session busy")));
    let text = a.chat_guidance(chat_id()).unwrap_or_default();
    assert!(!text.contains("retry failed"), "{text}");
}
