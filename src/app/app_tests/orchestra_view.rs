//! The full-pane transcript of a worker (agent-orchestrator S9, Run view and
//! persistence; modal-input, Worker transcript key): `Ctrl+Space W` opens it
//! for the selected worker, Esc or `q` closes it, it follows new output and
//! never takes the keys of an approval.

use super::chat_live::{chat_id, chat_of, composer, ctrl, plain};
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

// A trace is a redraw only for the worker on view: a fast stream must not wake
// the screen for nothing.
#[test]
fn a_trace_redraws_only_when_its_worker_is_on_view() {
    let mut a = selected_one();
    a.take_dirty();
    a.update(say(1, "no view open"));
    assert!(!a.take_dirty(), "no view");
    open(&mut a);
    a.take_dirty();
    a.update(say(2, "another worker"));
    assert!(!a.take_dirty(), "another worker");
    a.update(say(1, "\nfresh"));
    assert!(a.take_dirty(), "the worker on view");
    let shown = screen(&a);
    assert!(shown.contains("fresh"), "{shown}");
}

/// A wheel tick over the chat pane (the mouse on).
fn wheel(a: &mut App, kind: ratatui::crossterm::event::MouseEventKind) {
    a.config.mouse = true;
    let rect = a
        .tiling()
        .panes
        .iter()
        .find(|(id, _)| *id == chat_id())
        .map(|(_, rect)| *rect)
        .unwrap();
    a.update(AppEvent::Mouse(ratatui::crossterm::event::MouseEvent {
        kind,
        column: rect.x + 2,
        row: rect.y + 2,
        modifiers: KeyModifiers::NONE,
    }));
}

/// The `line N` numbers on screen.
fn visible(screen: &str) -> Vec<u32> {
    screen
        .split("line ")
        .skip(1)
        .filter_map(|rest| {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
        .collect()
}

fn sixty_lines() -> String {
    (0..60).map(|n| format!("line {n}\n")).collect()
}

// The wheel moves the view, not the conversation hidden under it.
#[test]
fn the_wheel_scrolls_the_transcript_and_not_the_hidden_conversation() {
    use ratatui::crossterm::event::MouseEventKind::{ScrollDown, ScrollUp};
    let mut a = viewing();
    a.update(say(1, &sixty_lines()));
    let conversation = chat_of(&a).scroll();
    let end = visible(&screen(&a)).into_iter().max().unwrap();
    wheel(&mut a, ScrollUp);
    let up = visible(&screen(&a)).into_iter().max().unwrap();
    assert!(up < end, "{up} < {end}");
    wheel(&mut a, ScrollDown);
    assert_eq!(visible(&screen(&a)).into_iter().max(), Some(end));
    assert_eq!(chat_of(&a).scroll(), conversation);
}

// The keys and the wheel scroll by the rows the frame really drew, also when
// an approval prompt takes the bottom of the pane.
#[test]
fn scrolling_uses_the_rows_drawn_with_an_approval_pending() {
    use ratatui::crossterm::event::MouseEventKind::ScrollUp;
    let (mut a, turn) = running();
    a.update(event(row(RUN, 1, WorkerStatus::Running)));
    a.update(trace(1, TraceItem::Begin));
    a.update(say(1, &sixty_lines()));
    select(&mut a, next_worker());
    assert_eq!(open(&mut a), vec![]);
    announce(&mut a, turn, 1, read("~/notes.txt"));
    let end = visible(&screen(&a)).into_iter().max().unwrap();
    assert_eq!(end, 59, "following the end");
    wheel(&mut a, ScrollUp);
    let up = visible(&screen(&a)).into_iter().max().unwrap();
    assert_eq!(end - up, 3, "one tick moves three rows, not a jump");
}

// A paste never reaches the composer hidden under the view, and says so.
#[test]
fn a_paste_is_refused_while_the_transcript_is_open() {
    let mut a = viewing();
    a.update(AppEvent::Paste("pasted".into()));
    assert_eq!(composer(&a), "");
    assert_eq!(a.notice(), Some("Close the worker transcript first (Esc)"));
}

// Never silent: a key the view does not use says how to leave it.
#[test]
fn an_unused_key_says_how_to_close_the_view() {
    let mut a = viewing();
    tapped(&mut a, plain(KeyCode::Char('x')));
    assert_eq!(a.notice(), Some("Esc closes the transcript"));
}

// Memory and approval-mode keys would act on the hidden conversation.
#[test]
fn the_memory_and_approval_keys_wait_while_the_transcript_is_open() {
    for leaf in ["o", "a"] {
        let mut a = viewing();
        press(&mut a, leaf);
        assert_eq!(
            a.notice(),
            Some("Close the worker transcript first (Esc)"),
            "{leaf}"
        );
    }
}

// The key stays whole when the hint wraps in a narrow pane.
#[test]
fn the_footer_key_is_not_split_by_the_wrap() {
    let a = selected_one();
    let shown = screen(&a);
    assert!(shown.contains("Ctrl+Space W transcript"), "{shown}");
}
