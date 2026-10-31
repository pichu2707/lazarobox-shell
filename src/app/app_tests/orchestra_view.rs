//! The full-pane transcript of a worker (agent-orchestrator S9, Run view and
//! persistence; modal-input, Worker transcript key): `Ctrl+Space W` opens it
//! for the selected worker, Esc or `q` closes it, it follows new output and
//! never takes the keys of an approval.

use super::chat_live::{chat_of, composer, ctrl, plain};
use super::orchestra_cancel::{next_worker, select};
use super::orchestra_run::{RUN, event, row, running};
use super::orchestra_toggle::screen;
use super::tool_approval::{announce, pending, read};
use super::*;
use crate::core::ai::{
    chat::engine::CallId,
    orchestra::{
        WorkerId,
        event::{OrchestraEvent, TraceItem},
        worker::{ErrorClass, WorkerStatus},
    },
};

fn trace(worker: u32, item: TraceItem) -> AppEvent {
    event(OrchestraEvent::Trace {
        run: RUN,
        worker: WorkerId(worker),
        item,
    })
}

fn say(worker: u32, text: &str) -> AppEvent {
    trace(worker, TraceItem::Text(text.to_owned()))
}

/// The prefix and `W`, after a pause so no key is taken for a hold.
fn open(a: &mut App) -> Vec<Effect> {
    NOW_MS.with(|n| n.set(n.get() + 1_000));
    press(a, "W")
}

/// Workers 1 and 2 running, worker 1 selected and having said `hello`.
fn selected_one() -> App {
    let (mut a, _) = running();
    for id in [1, 2] {
        a.update(event(row(RUN, id, WorkerStatus::Running)));
    }
    for id in [1, 2] {
        a.update(trace(id, TraceItem::Begin));
    }
    a.update(say(1, "hello from one"));
    a.update(say(2, "hello from two"));
    select(&mut a, next_worker());
    a
}

fn viewing() -> App {
    let mut a = selected_one();
    assert_eq!(open(&mut a), vec![]);
    assert!(chat_of(&a).run_view().is_some(), "the view opens");
    a
}

fn tapped(a: &mut App, ev: AppEvent) -> Vec<Effect> {
    NOW_MS.with(|n| n.set(n.get() + 600));
    a.update(ev)
}

// Spec (Run view and persistence): a key opens a full-pane transcript of a
// worker: its output, in its own pane.
#[test]
fn prefix_w_opens_the_transcript_of_the_selected_worker() {
    let a = viewing();
    let view = chat_of(&a).run_view().expect("the view is open");
    assert_eq!((view.run, view.worker), (RUN, WorkerId(1)));
    let shown = screen(&a);
    assert!(shown.contains("worker 1"), "{shown}");
    assert!(shown.contains("hello from one"), "{shown}");
    assert!(!shown.contains("hello from two"), "{shown}");
    assert_eq!(a.input(), InputMode::Terminal);
}

// Never silent: no selection says how to select one.
#[test]
fn without_a_selected_worker_it_says_how_to_select() {
    let (mut a, _) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    assert_eq!(open(&mut a), vec![]);
    assert!(chat_of(&a).run_view().is_none());
    assert_eq!(a.notice(), Some("Select a worker with Alt+Up/Down"));
}

// Never silent: with the gate off the key says why it does nothing.
#[test]
fn with_the_gate_off_it_says_so() {
    let (mut a, _) = running();
    a.ai.orchestrator.enabled = false;
    assert_eq!(open(&mut a), vec![]);
    assert!(chat_of(&a).run_view().is_none());
    assert_eq!(
        a.notice(),
        Some("The orchestrator is off; enable it in the menu (Ctrl+Space m)")
    );
}

// Never silent: a shell pane says the view is for chats.
#[test]
fn on_a_shell_pane_it_says_the_view_is_for_chats() {
    let mut a = selected_one();
    press(&mut a, "h");
    assert_eq!(a.focused(), PaneId::FIRST);
    open(&mut a);
    assert_eq!(
        a.notice(),
        Some("The worker transcript is for chat panes; open one with Ctrl+Space c")
    );
}

// Esc and q close it, back to the chat; neither cancels the run or types.
#[test]
fn esc_and_q_close_the_view_without_touching_the_run_or_the_composer() {
    for closing in [plain(KeyCode::Esc), plain(KeyCode::Char('q'))] {
        let mut a = viewing();
        assert_eq!(tapped(&mut a, closing), vec![]);
        assert!(chat_of(&a).run_view().is_none());
        assert_eq!(composer(&a), "");
        assert!(!screen(&a).contains("hello from one"));
    }
}

// The view is read-only: nothing typed reaches the composer or sends.
#[test]
fn other_keys_do_nothing_while_the_view_is_open() {
    let mut a = viewing();
    for ev in [
        plain(KeyCode::Char('x')),
        plain(KeyCode::Enter),
        ctrl('o'),
        ctrl('u'),
    ] {
        assert_eq!(tapped(&mut a, ev), vec![]);
    }
    assert_eq!(composer(&a), "");
    assert!(chat_of(&a).run_view().is_some());
}

// The view follows new output while open; scrolled up it holds still until
// the end is asked for again.
#[test]
fn the_view_follows_new_output_unless_scrolled() {
    let mut a = viewing();
    let lines: String = (0..60).map(|n| format!("line {n}\n")).collect();
    a.update(say(1, &lines));
    let shown = screen(&a);
    assert!(
        shown.contains("line 59") && !shown.contains("line 0\n"),
        "{shown}"
    );
    tapped(&mut a, plain(KeyCode::PageUp));
    a.update(say(1, "newest\n"));
    assert!(!screen(&a).contains("newest"));
    tapped(&mut a, plain(KeyCode::End));
    assert!(screen(&a).contains("newest"));
}

// The view scrolls with the arrows and the pages, to the top and back.
#[test]
fn the_view_scrolls_by_line_page_and_to_the_ends() {
    let mut a = viewing();
    let lines: String = (0..60).map(|n| format!("line {n}\n")).collect();
    a.update(say(1, &lines));
    tapped(&mut a, plain(KeyCode::Home));
    assert!(screen(&a).contains("hello from one"));
    tapped(&mut a, plain(KeyCode::Down));
    tapped(&mut a, plain(KeyCode::Char('j')));
    assert!(!screen(&a).contains("hello from one"));
    tapped(&mut a, plain(KeyCode::Char('k')));
    tapped(&mut a, plain(KeyCode::Up));
    assert!(screen(&a).contains("hello from one"));
    tapped(&mut a, plain(KeyCode::PageDown));
    let paged = screen(&a);
    assert!(
        paged.contains("line 30") && !paged.contains("hello from one"),
        "{paged}"
    );
    tapped(&mut a, plain(KeyCode::End));
    assert!(screen(&a).contains("line 59"));
}

// Untrusted worker text is shown clean.
#[test]
fn worker_text_is_shown_sanitized() {
    let mut a = viewing();
    a.update(say(1, "\u{1b}[2Jwipe\u{7}d\u{1b}]0;title\u{7} ok"));
    a.update(event(OrchestraEvent::Trace {
        run: RUN,
        worker: WorkerId(1),
        item: TraceItem::Call {
            id: CallId(1),
            name: "read_file\u{1b}[31m".into(),
            summary: "a.rs".into(),
        },
    }));
    let shown = screen(&a);
    assert!(shown.contains("wiped ok"), "{shown}");
    assert!(shown.contains("read_file a.rs"), "{shown}");
    assert!(!shown.contains('\u{1b}'));
}

// The worker may end while the view is open: it stays, with the final status.
#[test]
fn the_view_stays_open_when_its_worker_ends() {
    let mut a = viewing();
    a.update(event(row(RUN, 1, WorkerStatus::Done)));
    assert_eq!(chat_of(&a).selected_worker(), None);
    assert!(chat_of(&a).run_view().is_some());
    let shown = screen(&a);
    assert!(
        shown.contains("done") && shown.contains("hello from one"),
        "{shown}"
    );
}

// The run ends: the view keeps what it shows until it is closed, and then
// the transcript is gone.
#[test]
fn a_run_that_ends_leaves_the_view_until_it_is_closed() {
    let mut a = viewing();
    a.update(event(OrchestraEvent::Ended { run: RUN }));
    assert!(screen(&a).contains("hello from one"));
    tapped(&mut a, plain(KeyCode::Esc));
    assert!(chat_of(&a).run_view().is_none());
}

// Ctrl+C keeps cancelling the selected worker: the view never traps the user.
#[test]
fn ctrl_c_still_cancels_the_selected_worker_from_the_view() {
    let mut a = viewing();
    let effects = tapped(&mut a, ctrl('c'));
    assert!(
        effects.iter().any(|e| matches!(
            e,
            Effect::OrchCancelWorker { worker, .. } if *worker == WorkerId(1)
        )),
        "{effects:?}"
    );
}

// A failure alert is the user's to answer: the view gives way to it.
#[test]
fn a_failure_alert_closes_the_view() {
    let mut a = viewing();
    a.update(event(row(RUN, 1, WorkerStatus::Failed)));
    a.update(event(OrchestraEvent::Halted {
        run: RUN,
        worker: WorkerId(1),
        class: ErrorClass::Other,
        text: "boom".into(),
    }));
    assert!(chat_of(&a).run_view().is_none());
    assert!(chat_of(&a).orch_alert().is_some());
}

// An approval that arrives while the view is open is shown under it and
// answered by its own keys; the view does not take them.
#[test]
fn an_approval_arriving_in_the_view_is_shown_and_answered() {
    let (mut a, turn) = {
        let (mut a, turn) = running();
        for id in [1, 2] {
            a.update(event(row(RUN, id, WorkerStatus::Running)));
        }
        a.update(say(1, "hello from one"));
        select(&mut a, next_worker());
        (a, turn)
    };
    assert_eq!(open(&mut a), vec![]);
    announce(&mut a, turn, 1, read("~/notes.txt"));
    assert!(pending(&a).is_some());
    let shown = screen(&a);
    assert!(shown.contains("notes.txt"), "{shown}");
    assert!(shown.contains("hello from one"), "{shown}");
    // `q` is not the view's while a prompt waits.
    tapped(&mut a, plain(KeyCode::Char('q')));
    assert!(chat_of(&a).run_view().is_some());
    let effects = tapped(&mut a, plain(KeyCode::Char('y')));
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::ToolDecision { .. })),
        "{effects:?}"
    );
}

// The other views of the pane would replace the transcript under the user.
#[test]
fn the_prefix_views_wait_while_the_transcript_is_open() {
    let mut a = viewing();
    for leaf in ["T", "n", "s"] {
        press(&mut a, leaf);
        assert_eq!(a.notice(), Some("Close the worker transcript first (Esc)"));
        assert!(chat_of(&a).run_view().is_some());
        assert!(chat_of(&a).tag_editor().is_none());
    }
}

// Never silent: another view of the chat in the way says so.
#[test]
fn another_open_view_says_to_close_it_first() {
    let mut a = selected_one();
    a.focused_state_mut().chat_mut().unwrap().open_list();
    assert!(chat_of(&a).list().is_some());
    open(&mut a);
    assert!(chat_of(&a).run_view().is_none());
    assert_eq!(a.notice(), Some("Close this view first"));
}

// The footer names the key while a worker is selected.
#[test]
fn the_footer_names_the_key_with_a_worker_selected() {
    let a = selected_one();
    let shown = screen(&a);
    assert!(shown.contains("Ctrl+Space W"), "{shown}");
}
