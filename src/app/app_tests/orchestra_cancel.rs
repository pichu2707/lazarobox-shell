//! Selecting a worker row and the escalating Ctrl+C (agent-orchestrator S3,
//! modal-input / Cancel keys for workers): the first Ctrl+C cancels the
//! selected worker, a second one within two seconds (or one with no worker
//! selected) cancels the turn and the whole run.

use std::cell::Cell;

use super::chat_live::{chat_id, chat_of, code, ctrl, plain};
use super::orchestra_run::{RUN, alerted, decide, event, row, running};
use super::*;
use crate::core::ai::orchestra::{
    WorkerId, event::OrchestraEvent, run::UserChoice, worker::WorkerStatus,
};

thread_local! {
    static NOW_MS: Cell<u64> = const { Cell::new(0) };
    static BASE: Instant = Instant::now();
}

fn mock_now() -> Instant {
    BASE.with(|b| *b + Duration::from_millis(NOW_MS.with(Cell::get)))
}

/// A key `after_ms` after the last one. The same key twice within the hold
/// window counts as held, so presses are spaced out.
fn press(a: &mut App, after_ms: u64, ev: AppEvent) -> Vec<Effect> {
    NOW_MS.with(|n| n.set(n.get() + after_ms));
    a.update(ev)
}

fn select(a: &mut App, ev: AppEvent) {
    press(a, 600, ev);
}

fn whole_run(effects: &[Effect]) -> bool {
    effects.contains(&Effect::ChatCancel(chat_id()))
        && !effects
            .iter()
            .any(|e| matches!(e, Effect::OrchCancelWorker { .. }))
}

/// A run with workers 1 and 2 running, on the mock clock.
fn two_running() -> App {
    NOW_MS.with(|n| n.set(0));
    let (a, _) = running();
    let mut a = a.with_clock(mock_now);
    for id in [1, 2] {
        a.update(event(row(RUN, id, WorkerStatus::Running)));
    }
    a
}

fn next_worker() -> AppEvent {
    code(KeyCode::Down, KeyModifiers::ALT)
}

fn previous_worker() -> AppEvent {
    code(KeyCode::Up, KeyModifiers::ALT)
}

fn selected(a: &App) -> Option<WorkerId> {
    chat_of(a).selected_worker()
}

fn cancel_worker(worker: u32) -> Effect {
    Effect::OrchCancelWorker {
        pane: chat_id(),
        run: RUN,
        worker: WorkerId(worker),
    }
}

// modal-input / Cancel the selected worker: Alt+Down and Alt+Up walk the
// workers that have not ended, and wrap.
#[test]
fn alt_arrows_select_the_live_workers_in_turn() {
    let mut a = two_running();
    assert_eq!(selected(&a), None);
    assert_eq!(press(&mut a, 600, next_worker()), vec![]);
    assert_eq!(selected(&a), Some(WorkerId(1)));
    select(&mut a, next_worker());
    assert_eq!(selected(&a), Some(WorkerId(2)));
    select(&mut a, next_worker());
    assert_eq!(selected(&a), Some(WorkerId(1)), "wraps");
    select(&mut a, previous_worker());
    assert_eq!(selected(&a), Some(WorkerId(2)), "and back");
    a.update(event(row(RUN, 1, WorkerStatus::Done)));
    select(&mut a, next_worker());
    assert_eq!(
        selected(&a),
        Some(WorkerId(2)),
        "an ended worker is skipped"
    );
}

// Never silent: nothing to select says so.
#[test]
fn selecting_with_no_live_worker_says_so() {
    let (mut a, _) = running();
    a.update(next_worker());
    assert_eq!(selected(&a), None);
    assert_eq!(a.notice(), Some("No worker to select"));
}

// The selection goes with its worker and with its run.
#[test]
fn the_selection_ends_with_the_worker_and_with_the_run() {
    let mut a = two_running();
    select(&mut a, next_worker());
    a.update(event(row(RUN, 1, WorkerStatus::Cancelled)));
    assert_eq!(selected(&a), None);
    select(&mut a, next_worker());
    assert_eq!(selected(&a), Some(WorkerId(2)));
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    assert_eq!(selected(&a), None);
}

// The selected row is drawn with its own prefix.
#[test]
fn the_selected_row_is_marked_in_the_transcript() {
    let mut a = two_running();
    let marked = |a: &App| {
        let lines = chat_of(a).lines(80);
        lines
            .iter()
            .filter(|l| format!("{l:?}").contains("> "))
            .count()
    };
    assert_eq!(marked(&a), 0);
    select(&mut a, next_worker());
    assert_eq!(marked(&a), 1);
}

// modal-input / Cancel the selected worker: the first Ctrl+C cancels that
// worker only and says how to cancel the run.
#[test]
fn the_first_ctrl_c_cancels_only_the_selected_worker() {
    let mut a = two_running();
    select(&mut a, next_worker());
    select(&mut a, next_worker());
    let effects = press(&mut a, 600, ctrl('c'));
    assert_eq!(effects, vec![cancel_worker(2)]);
    assert_eq!(a.notice(), Some("Ctrl+C again cancels the run"));
    assert_eq!(selected(&a), None);
}

// modal-input / Ctrl+C: a second press within two seconds ends the turn and
// the run, a late one cancels the newly selected worker again.
#[test]
fn a_second_ctrl_c_within_two_seconds_cancels_the_run() {
    let mut a = two_running();
    select(&mut a, next_worker());
    assert_eq!(press(&mut a, 600, ctrl('c')), vec![cancel_worker(1)]);
    a.update(event(row(RUN, 1, WorkerStatus::Cancelled)));
    select(&mut a, next_worker());
    let effects = press(&mut a, 600, ctrl('c'));
    assert!(whole_run(&effects), "{effects:?}");

    let mut late = two_running();
    select(&mut late, next_worker());
    press(&mut late, 600, ctrl('c'));
    late.update(event(row(RUN, 1, WorkerStatus::Cancelled)));
    select(&mut late, next_worker());
    assert_eq!(press(&mut late, 1_500, ctrl('c')), vec![cancel_worker(2)]);
}

// modal-input / Ctrl+C: the escalation belongs to its run. A new run started
// within the two seconds is not cancelled whole by its first Ctrl+C.
#[test]
fn a_new_run_drops_the_armed_second_ctrl_c() {
    let mut a = two_running();
    select(&mut a, next_worker());
    assert_eq!(press(&mut a, 600, ctrl('c')), vec![cancel_worker(1)]);
    let next = RunId(RUN.0 + 1);
    for id in [1, 2] {
        a.update(event(row(next, id, WorkerStatus::Running)));
    }
    select(&mut a, next_worker());
    let effects = press(&mut a, 600, ctrl('c'));
    let one = Effect::OrchCancelWorker {
        pane: chat_id(),
        run: next,
        worker: WorkerId(1),
    };
    assert_eq!(effects, vec![one]);
}

// modal-input / Ctrl+C: with no worker selected it cancels the turn and the
// run, as it always did; so does Esc, selected or not.
#[test]
fn ctrl_c_with_no_selection_and_esc_cancel_the_whole_run() {
    let mut a = two_running();
    let effects = press(&mut a, 600, ctrl('c'));
    assert!(whole_run(&effects), "{effects:?}");
    let mut b = two_running();
    select(&mut b, next_worker());
    let effects = press(&mut b, 600, plain(KeyCode::Esc));
    assert!(whole_run(&effects), "{effects:?}");
}

// modal-input / Ctrl+C: with a failure alert up and a sibling selected, Ctrl+C
// cancels that sibling and leaves the alert; Esc still ends the run.
#[test]
fn with_an_alert_up_ctrl_c_cancels_the_selected_sibling_and_esc_the_run() {
    let (mut a, _) = alerted();
    a.update(event(row(RUN, 2, WorkerStatus::Running)));
    NOW_MS.with(|n| n.set(0));
    let mut a = a.with_clock(mock_now);
    select(&mut a, next_worker());
    assert_eq!(press(&mut a, 600, ctrl('c')), vec![cancel_worker(2)]);
    assert!(chat_of(&a).orch_alert().is_some());
    let effects = press(&mut a, 600, plain(KeyCode::Esc));
    assert_eq!(effects, vec![decide(UserChoice::Cancel)]);
}

// The footer hint names the keys while a run has a worker that is not over.
#[test]
fn the_guidance_names_the_cancel_keys_while_a_worker_is_live() {
    let mut a = two_running();
    let hint = a.orch_guidance(chat_id()).unwrap();
    assert!(hint.contains("Alt+Up/Down select a worker"), "{hint}");
    a.update(next_worker());
    let hint = a.orch_guidance(chat_id()).unwrap();
    assert!(
        hint.contains("worker 1") && hint.contains("Ctrl+C cancels it"),
        "{hint}"
    );
    for id in [1, 2] {
        a.update(event(row(RUN, id, WorkerStatus::Done)));
    }
    assert_eq!(a.orch_guidance(chat_id()), None);
}

// Gate off: the keys do nothing and Ctrl+C is the chat cancel.
#[test]
fn with_the_gate_off_the_keys_are_ignored() {
    let (mut a, _) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    a.ai.orchestrator.enabled = false;
    assert_eq!(a.update(next_worker()), vec![]);
    assert_eq!(selected(&a), None);
    assert_eq!(a.orch_guidance(chat_id()), None);
}
