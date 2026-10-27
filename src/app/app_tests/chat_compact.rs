//! Rolling compaction in the app (chat-memory S3, S4, S8): a run goes out
//! after a turn ends, with the `summarize` task's engine, and never holds the
//! user's next send.

use super::chat_notes::{CWD, PROJECT, ai, plain};
use super::*;
use crate::core::ai::{
    EngineState, Status,
    chat::{
        Entry,
        compact::COMPACT_PROMPT,
        engine::{ChatError, ChatRequest, Finish, StreamEvent},
    },
    config::{Assignment, CompactionMode, Task},
    memory::port::MemReply,
};

fn chat_id() -> PaneId {
    id2()
}

fn chat_of(a: &App) -> &ChatState {
    match a.pane_view(chat_id()) {
        Some(PaneView::Chat(chat)) => chat,
        _ => panic!("pane 2 is not a chat"),
    }
}

/// An open chat on a ready ollama; `summarize` is assigned `model` when given.
fn app(memory: bool, summarize: Option<&str>) -> App {
    let mut config = ai(memory);
    if let Some(model) = summarize {
        config.set(
            Task::Summarize,
            Assignment {
                engine: Some("ollama".into()),
                model: Some(model.into()),
            },
        );
    }
    let mut a = launched(80, 25).with_ai(config);
    a.update(cwd_event(CWD));
    a.update(AppEvent::EnginesDetected(Detection {
        engines: vec![EngineState {
            id: "ollama",
            status: Status::Available,
            models: vec!["llama3:8b".to_owned(), "tiny".to_owned()],
            version: None,
            model_labels: Vec::new(),
            diagnostics: Default::default(),
        }],
        ..Default::default()
    }));
    press(&mut a, "c");
    a.update(AppEvent::PromptReady(
        chat_id(),
        crate::core::ai::chat::prompts::Resolved {
            text: "system".into(),
            notices: Vec::new(),
        },
    ));
    if memory {
        a.update(AppEvent::Memory(MemReply::Project {
            pane: chat_id(),
            result: Ok(PROJECT.into()),
        }));
    }
    a.take_dirty();
    a
}

/// The effects of one exchange: the question sent, then the answer streamed.
/// Returns the effects of the final `Done` and the request of the question.
fn exchange(a: &mut App, size: usize) -> (Vec<Effect>, ChatRequest) {
    a.chat_type(chat_id(), &"q".repeat(size));
    let sent = a.update(plain(KeyCode::Enter));
    let Some(Effect::ChatSend { turn, request, .. }) = sent
        .iter()
        .find(|e| matches!(e, Effect::ChatSend { .. }))
        .cloned()
    else {
        panic!("expected a ChatSend, got {sent:?}");
    };
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Delta("a".repeat(size)),
    ));
    let end = a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(Finish::Stop),
    ));
    (end, request)
}

fn compaction_of(effects: &[Effect]) -> Option<(u64, ChatRequest)> {
    effects.iter().find_map(|e| match e {
        Effect::ChatCompact { turn, request, .. } => Some((*turn, request.clone())),
        _ => None,
    })
}

/// Drives exchanges of 1000 characters until a run goes out.
fn until_compaction(a: &mut App) -> (u64, ChatRequest) {
    for _ in 0..14 {
        let (end, _) = exchange(a, 1_000);
        if let Some(run) = compaction_of(&end) {
            return run;
        }
    }
    panic!("no compaction run went out");
}

#[test]
fn a_run_goes_out_after_a_turn_ends_with_the_summarize_engine() {
    let mut a = app(true, Some("tiny"));
    let (turn, request) = until_compaction(&mut a);
    assert_eq!(request.engine, "ollama");
    assert_eq!(request.model.as_deref(), Some("tiny"));
    assert_eq!(turn, chat_of(&a).turn());
    assert_eq!(chat_of(&a).phase(), Phase::Idle, "the pane stays free");
}

#[test]
fn an_unassigned_summarize_task_uses_the_chat_engine_never_the_general_one() {
    let mut config = ai(true);
    config.set(
        Task::General,
        Assignment {
            engine: Some("claude".into()),
            model: Some("opus".into()),
        },
    );
    let mut a = app(true, None).with_ai(config);
    let (_, request) = until_compaction(&mut a);
    assert_eq!(request.engine, "ollama");
    assert_eq!(request.model.as_deref(), Some("llama3:8b"));
}

#[test]
fn an_explicit_summarize_task_wins_over_the_chat_engine() {
    let mut config = ai(true);
    config.set(
        Task::General,
        Assignment {
            engine: Some("claude".into()),
            model: Some("opus".into()),
        },
    );
    config.set(
        Task::Summarize,
        Assignment {
            engine: Some("ollama".into()),
            model: Some("tiny".into()),
        },
    );
    let mut a = app(true, None).with_ai(config);
    let (_, request) = until_compaction(&mut a);
    assert_eq!(request.model.as_deref(), Some("tiny"));
}

#[test]
fn compaction_off_sends_no_run_and_keeps_the_marker() {
    let mut config = ai(true);
    config.set_compaction(CompactionMode::Off);
    let mut a = app(true, Some("tiny")).with_ai(config);
    for _ in 0..14 {
        let (end, _) = exchange(&mut a, 1_000);
        assert_eq!(compaction_of(&end), None);
    }
    assert!(
        chat_of(&a).entries().iter().any(
            |e| matches!(e, Entry::System(s) if s == crate::core::ai::chat::TRUNCATION_MARKER)
        )
    );
}

#[test]
fn a_short_chat_sends_no_run() {
    let mut a = app(true, Some("tiny"));
    for _ in 0..8 {
        let (end, _) = exchange(&mut a, 10);
        assert_eq!(compaction_of(&end), None);
    }
}

#[test]
fn memory_off_still_compacts_and_the_summary_goes_with_the_next_send() {
    let mut a = app(false, Some("tiny"));
    let (turn, _) = until_compaction(&mut a);
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Delta("- the decision".into()),
    ));
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(Finish::Stop),
    ));
    assert!(a.take_dirty(), "the transcript changed");
    assert!(
        chat_of(&a)
            .entries()
            .iter()
            .any(|e| matches!(e, Entry::System(s) if s.ends_with("messages summarized")))
    );
    let (_, request) = exchange(&mut a, 1_000);
    assert!(request.memory.unwrap().contains("- the decision"));
}

#[test]
fn a_failed_run_changes_nothing_and_does_not_block_the_next_send() {
    let mut a = app(true, Some("tiny"));
    let (turn, _) = until_compaction(&mut a);
    let before = chat_of(&a).entries().len();
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Error(ChatError::Failed("down".into())),
    ));
    assert_eq!(chat_of(&a).entries().len(), before);
    let (_, request) = exchange(&mut a, 1_000);
    assert_eq!(request.memory, None);
}

#[test]
fn a_send_while_the_run_is_in_flight_goes_at_once_and_the_late_summary_is_dropped() {
    let mut a = app(true, Some("tiny"));
    let (turn, _) = until_compaction(&mut a);
    a.chat_type(chat_id(), "next question");
    let sent = a.update(plain(KeyCode::Enter));
    assert!(
        sent.iter()
            .any(|e| matches!(e, Effect::ChatSend { request, .. } if request.system != COMPACT_PROMPT.trim_end())),
        "{sent:?}"
    );
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Delta("- late".into()),
    ));
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(Finish::Stop),
    ));
    assert_eq!(chat_of(&a).summary_block(), None);
}

#[test]
fn a_summarize_model_that_is_missing_sends_no_run() {
    let mut a = app(true, Some("gone"));
    for _ in 0..14 {
        let (end, _) = exchange(&mut a, 1_000);
        assert_eq!(compaction_of(&end), None);
    }
}
