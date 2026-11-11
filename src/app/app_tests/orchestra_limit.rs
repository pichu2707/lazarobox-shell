//! The limit ask of a `simple` worker (agent-orchestrator W9b, D14, decision
//! K2): an inline prompt of the chat pane, a worker ask like the others,
//! answered only by the user's fresh keys: `y` keeps the batch whole, `n` or
//! Esc splits the work and discards the staged batch.

use super::chat_live::plain;
use super::tool_approval::{announce, announce_now, live, pending, worker_decision};
use super::*;
use crate::core::ai::chat::{
    approval::{ApprovalAsk, LimitFile, answers_itself},
    engine::Decision,
};
use crate::core::ai::config::ApprovalMode;
use crate::core::ai::orchestra::WorkerKey;
use ratatui::{Terminal, backend::TestBackend};

fn limit(worker: u32, label: &str) -> ApprovalAsk {
    ApprovalAsk::Limit {
        worker: WorkerKey::for_test(1, worker),
        label: label.to_owned(),
        over: vec![
            LimitFile {
                path: "src/a.rs".to_owned(),
                lines: 450,
                every_line: false,
            },
            LimitFile {
                path: "notes.xyz".to_owned(),
                lines: 500,
                every_line: true,
            },
        ],
        outside: vec!["src/b.rs".to_owned()],
    }
}

// 22.6: the prompt shows each path, its counted lines, the limit 400,
// `every line counted` for an unknown language and the outside files.
#[test]
fn the_prompt_shows_each_file_its_lines_the_limit_and_the_outside_files() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, limit(1, "simple ollama>llama3"));
    let prompt = a.approval_prompt(id2()).unwrap();
    assert!(prompt.target.contains("simple ollama>llama3"), "{prompt:?}");
    assert!(
        prompt.command.contains("src/a.rs: 450 counted lines"),
        "{prompt:?}"
    );
    assert!(
        prompt
            .command
            .contains("notes.xyz: 500 lines, every line counted"),
        "{prompt:?}"
    );
    assert!(
        prompt.command.contains("outside the brief: src/b.rs"),
        "{prompt:?}"
    );
    let notice = prompt.notice.clone().unwrap_or_default();
    assert!(notice.contains("400"), "{prompt:?}");
    assert!(prompt.keys.contains("y keep"), "{prompt:?}");
    assert!(prompt.keys.contains("n or Esc split"), "{prompt:?}");
}

// K2: `y` keeps, `n` and Esc split; the answers are addressed to the worker.
#[test]
fn y_keeps_and_n_or_esc_split_to_the_workers_session() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, limit(1, "simple x>y"));
    assert_eq!(
        a.update(key('y')),
        vec![worker_decision(turn, 1, Decision::Sent)]
    );
    let declined = Decision::Denied("the user declined");
    announce(&mut a, turn, 2, limit(1, "simple x>y"));
    assert_eq!(a.update(key('n')), vec![worker_decision(turn, 2, declined)]);
    announce(&mut a, turn, 3, limit(1, "simple x>y"));
    assert_eq!(
        a.update(plain(KeyCode::Esc)),
        vec![worker_decision(turn, 3, declined)]
    );
    assert_eq!(pending(&a), None);
}

// Review fix 5: whatever the approval mode of the chat, the ask is shown and
// only the user's `y` keeps and `n` splits.
#[test]
fn y_and_n_answer_it_in_every_approval_mode() {
    for mode in [ApprovalMode::Ask, ApprovalMode::Auto, ApprovalMode::Run] {
        let (mut a, turn) = live();
        super::tool_run::chat_mut(&mut a).set_approval_mode(mode);
        announce(&mut a, turn, 1, limit(1, "simple x>y"));
        assert!(pending(&a).is_some(), "{mode:?}: it opens a prompt");
        let sent = vec![worker_decision(turn, 1, Decision::Sent)];
        assert_eq!(a.update(key('y')), sent, "{mode:?}");
        announce(&mut a, turn, 2, limit(1, "simple x>y"));
        let declined = Decision::Denied("the user declined");
        let split = vec![worker_decision(turn, 2, declined)];
        assert_eq!(a.update(key('n')), split, "{mode:?}");
    }
}

// An early key does not count: the prompt arms after its delay.
#[test]
fn a_key_typed_before_the_prompt_is_armed_answers_nothing() {
    let (mut a, turn) = live();
    announce_now(&mut a, turn, 1, limit(1, "simple x>y"));
    assert_eq!(a.update(key('y')), vec![]);
    assert!(pending(&a).is_some(), "still waiting");
}

// 22.2: no approval mode answers it (`route` is pinned in `approval.rs`).
#[test]
fn no_approval_mode_answers_a_limit_ask() {
    let ask = limit(1, "simple x>y");
    for mode in [ApprovalMode::Ask, ApprovalMode::Run, ApprovalMode::Auto] {
        assert_eq!(answers_itself(&ask, mode, false), None, "{mode:?}");
    }
}

// It queues behind an ask on screen like any worker ask.
#[test]
fn a_limit_ask_waits_its_turn_in_the_queue() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, limit(1, "simple a>b"));
    assert!(announce(&mut a, turn, 2, limit(2, "simple c>d")).is_empty());
    let first = a.approval_prompt(id2()).unwrap();
    assert!(first.target.contains("simple a>b"), "{first:?}");
    assert!(first.target.contains("1 waiting"), "{first:?}");
}

// 22.6: the prompt at a narrow width.
#[test]
fn the_limit_prompt_at_a_narrow_width() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, limit(1, "simple ollama>llama3"));
    let theme = crate::ui::theme::LazaroboxTheme::default();
    let mut terminal = Terminal::new(TestBackend::new(80, 25)).unwrap();
    terminal
        .draw(|frame| crate::ui::render(frame, &a, &theme))
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    let screen = (0..buf.area.height)
        .map(|y| {
            let row: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
            row.trim_end().to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(screen.contains("src/a.rs"), "{screen}");
    insta::assert_snapshot!("limit_prompt_80x25", screen);
}
