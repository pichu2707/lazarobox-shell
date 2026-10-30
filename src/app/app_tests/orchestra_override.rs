//! The run-scoped override of a failure alert (agent-orchestrator, Failure
//! options and relaunch; config-menu, Picker opened from a failure notice):
//! `m` and `p` open the detached picker, a choice relaunches the worker and
//! reaches later rosters, and nothing is ever saved. Also the visible answer
//! to a retry the run manager could not apply. State asserts only.

use super::chat_live::{ask, chat_id, chat_of, composer, ctrl, finish, plain, type_text};
use super::orchestra_run::{RUN, alert, alerted, decide, event, halted, row};
use super::*;
use crate::core::ai::{
    chat::engine::{Availability, OrchSend},
    orchestra::{
        RunId, WorkerId,
        event::OrchestraEvent,
        run::UserChoice,
        worker::{ErrorClass, WorkerStatus},
    },
};

fn guidance(a: &App) -> String {
    a.chat_guidance(chat_id()).unwrap_or_default()
}

/// Moves the picker's cursor down until `[label]` is under it.
fn to(a: &mut App, label: &str) {
    for _ in 0..16 {
        if guidance(a).contains(&format!("[{label}]")) {
            return;
        }
        a.update(key('j'));
    }
    panic!("no `[{label}]` in {}", guidance(a));
}

/// The roster the effects carry, either the relaunch's or the next turn's.
fn relaunch_roster(effects: &[Effect]) -> crate::core::ai::orchestra::run::Roster {
    match effects {
        [
            Effect::OrchDecide {
                choice: UserChoice::Relaunch(roster),
                ..
            },
        ] => roster.clone(),
        other => panic!("one relaunch: {other:?}"),
    }
}

fn next_turn_roster(a: &mut App, turn: u64) -> crate::core::ai::orchestra::run::Roster {
    a.update(finish(turn));
    match ask(a, "next").as_slice() {
        [.., Effect::OrchestrateSend { orch, .. }] => {
            let OrchSend { roster, .. } = orch.as_ref();
            roster.clone()
        }
        other => panic!("an orchestrator turn: {other:?}"),
    }
}

fn claude_ready(roster: &crate::core::ai::orchestra::run::Roster, task: Task) -> bool {
    matches!(roster.get(task), Availability::Ready { engine, .. } if engine == "claude")
}

/// The failed worker is a `review` on codex; the menu has no `review`.
/// `p`, then claude, then its first model.
fn override_review_to_claude(a: &mut App) -> Vec<Effect> {
    a.update(key('p'));
    to(a, "claude");
    a.update(plain(KeyCode::Enter));
    // The same key again within the hold window would be a held key.
    let_go();
    a.update(plain(KeyCode::Enter))
}

// config-menu / Change model opens level 2; modal-input / Change keys.
#[test]
fn m_opens_the_models_of_the_engine_in_use_and_hides_the_alert_keys() {
    let (mut a, _) = alerted();
    assert_eq!(a.update(key('m')), vec![]);
    let text = guidance(&a);
    assert!(text.starts_with("Model for review (codex)"), "{text}");
    assert!(
        text.contains("[gpt-5]"),
        "the cursor is on the model in use"
    );
    assert_eq!(composer(&a), "", "the key was not typed");
}

// config-menu / Change provider opens level 1.
#[test]
fn p_opens_the_engines() {
    let (mut a, _) = alerted();
    assert_eq!(a.update(key('p')), vec![]);
    let text = guidance(&a);
    assert!(text.starts_with("Engine for review"), "{text}");
    assert!(text.contains("[codex]"), "{text}");
}

// modal-input / Change keys: `m`, Esc, `p` opens the model picker, returns
// the alert, then opens the engine picker; config-menu / Esc creates no
// override.
#[test]
fn esc_returns_to_the_alert_without_an_override() {
    let (mut a, _) = alerted();
    let before = a.ai().clone();
    a.update(key('m'));
    assert_eq!(a.update(plain(KeyCode::Esc)), vec![], "nothing relaunches");
    assert!(guidance(&a).starts_with("run paused:"), "{}", guidance(&a));
    assert!(alert(&a).is_some());
    assert!(chat_of(&a).orch_overrides().is_none());
    a.update(key('p'));
    assert!(guidance(&a).starts_with("Engine for review"));
    assert_eq!(a.update(plain(KeyCode::Esc)), vec![]);
    assert_eq!(a.ai(), &before);
}

// modal-input / Failure alert keys: the letters act only with an empty
// composer.
#[test]
fn m_and_p_are_typed_into_a_message() {
    let (mut a, _) = alerted();
    type_text(&mut a, "a");
    a.update(key('m'));
    a.update(key('p'));
    assert_eq!(composer(&a), "amp");
    assert!(guidance(&a).starts_with("run paused:"));
}

// While the picker is open the run stays paused: its keys are the picker's,
// and the alert's `r` and `c` do not answer it.
#[test]
fn the_picker_takes_the_keys_while_it_is_open() {
    let (mut a, _) = alerted();
    a.update(key('m'));
    for ignored in [key('r'), key('c'), key('x')] {
        assert_eq!(a.update(ignored), vec![]);
    }
    assert!(guidance(&a).starts_with("Model for review"));
    assert_eq!(composer(&a), "");
    assert!(alert(&a).is_some(), "the run is still paused");
}

// Ctrl+C cancels the turn and the run through the picker too.
#[test]
fn ctrl_c_still_cancels_with_the_picker_open() {
    let (mut a, _) = alerted();
    a.update(key('p'));
    let effects = a.update(ctrl('c'));
    assert!(
        effects.contains(&Effect::ChatCancel(chat_id())),
        "{effects:?}"
    );
}

// agent-orchestrator / Change provider relaunches on the new one; config-menu
// / Choice is an override, not a save; config-persistence / A failure picker
// choice is never persisted.
#[test]
fn a_choice_relaunches_the_worker_on_the_override_and_saves_nothing() {
    let (mut a, _) = alerted();
    let before = a.ai().clone();
    let effects = override_review_to_claude(&mut a);
    assert!(claude_ready(&relaunch_roster(&effects), Task::Review));
    assert_eq!(alert(&a), None, "answered");
    assert_eq!(a.ai(), &before, "the menu configuration is untouched");
    assert!(!guidance(&a).contains("Model for"), "the picker is closed");
    let over = chat_of(&a).orch_overrides().expect("an override");
    assert_eq!(
        over.get(Task::Review).and_then(|x| x.engine.as_deref()),
        Some("claude")
    );
}

// agent-orchestrator / Override reaches later workers of the task: the next
// roster carries it for `review` only.
#[test]
fn later_rosters_carry_the_override_for_its_task_only() {
    let (mut a, turn) = alerted();
    override_review_to_claude(&mut a);
    let roster = next_turn_roster(&mut a, turn);
    assert!(claude_ready(&roster, Task::Review));
    assert_eq!(roster.get(Task::Code), &Availability::Unassigned);
}

// agent-orchestrator / Override ends with the run: the next run starts from
// the menu values.
#[test]
fn the_override_ends_with_the_run() {
    let (mut a, turn) = alerted();
    override_review_to_claude(&mut a);
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    assert!(chat_of(&a).orch_overrides().is_none());
    let roster = next_turn_roster(&mut a, turn);
    assert_eq!(roster.get(Task::Review), &Availability::Unassigned);
}

// An `Ended` of another run leaves the override of the current one.
#[test]
fn the_end_of_another_run_keeps_the_override() {
    let (mut a, _) = alerted();
    override_review_to_claude(&mut a);
    a.update(event(OrchestraEvent::Ended { run: RunId(3) }));
    assert!(chat_of(&a).orch_overrides().is_some());
}

// agent-orchestrator / Override shown: the rows of the affected workers carry
// the marker, an earlier row does not, and the alert names the override.
#[test]
fn the_override_is_marked_on_later_rows_and_named_in_the_alert() {
    let (mut a, _) = alerted();
    override_review_to_claude(&mut a);
    a.update(event(row(RUN, 2, WorkerStatus::Running)));
    a.update(event(halted(RUN, "boom")));
    let text = guidance(&a);
    assert!(text.contains("override: review claude>"), "{text}");
    let marked: Vec<bool> = chat_of(&a)
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            crate::core::ai::chat::Entry::Worker(row) => Some(row.overridden),
            _ => None,
        })
        .collect();
    assert_eq!(marked, [false, true], "worker 1 ran before the override");
}
