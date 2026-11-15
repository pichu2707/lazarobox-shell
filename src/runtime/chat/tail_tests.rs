//! What the end of an orchestrator turn tells its run (agent-orchestrator,
//! "Gates on step state are visible and bounded"): the turn ended in `Done`,
//! and its last round of text wrote a call as JSON.

use super::tail::{CAP, Tail};
use crate::core::ai::chat::engine::{CallId, ChatError, Finish, StreamEvent};
use crate::core::ai::orchestra::run::TurnEnd;

const CALL: &str = r#"{"name": "spawn_agent", "arguments": {"kind": "code"}}"#;

fn delta(text: &str) -> StreamEvent {
    StreamEvent::Delta(text.to_owned())
}

fn tool_call() -> StreamEvent {
    StreamEvent::ToolCall {
        id: CallId(1),
        name: "wait".to_owned(),
        summary: "wait".to_owned(),
        approval: None,
    }
}

fn ended(events: impl IntoIterator<Item = StreamEvent>) -> TurnEnd {
    let mut tail = Tail::default();
    for event in events {
        tail.see(&event);
    }
    tail.end()
}

#[test]
fn a_turn_that_ended_in_done_is_answered_and_a_written_call_is_named() {
    let done = || StreamEvent::Done(Finish::Stop);
    assert_eq!(
        ended([delta("I will "), delta(CALL), done()]),
        TurnEnd {
            answered: true,
            written: Some("spawn_agent")
        }
    );
    assert_eq!(
        ended([delta("Plain answer, naming spawn_agent."), done()]),
        TurnEnd {
            answered: true,
            written: None
        }
    );
}

#[test]
fn an_errored_or_interrupted_turn_is_not_answered() {
    let error = StreamEvent::Error(ChatError::Interrupted);
    assert_eq!(
        ended([delta(CALL), error]),
        TurnEnd {
            answered: false,
            written: Some("spawn_agent")
        }
    );
    assert_eq!(ended([delta("text")]), TurnEnd::default());
}

#[test]
fn a_tool_call_or_a_notice_drops_the_text_before_it() {
    let done = || StreamEvent::Done(Finish::Stop);
    let after_call = ended([delta(CALL), tool_call(), delta("done"), done()]);
    assert_eq!(after_call.written, None);
    let after_notice = ended([
        delta(CALL),
        StreamEvent::Notice("the model wrote a tool call as text".into()),
        done(),
    ]);
    assert_eq!(after_notice.written, None);
    assert!(after_notice.answered);
}

#[test]
fn the_text_is_capped_and_keeps_its_end() {
    let mut tail = Tail::default();
    tail.see(&delta(&"a".repeat(CAP * 2)));
    tail.see(&delta(CALL));
    tail.see(&StreamEvent::Done(Finish::Stop));
    assert_eq!(tail.end().written, Some("spawn_agent"));
    assert!(tail.len() <= CAP, "{}", tail.len());
}
