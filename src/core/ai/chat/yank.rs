//! What a yank copies out of a chat: the last finished answer, or the last
//! fenced code block of it. Reasoning and system lines are never part of it.

use super::{ChatState, Phase, engine::Role};

/// The text of the last assistant message that is not still being written.
/// `None` when there is none or it is empty.
pub fn last_answer(chat: &ChatState) -> Option<String> {
    let answers: Vec<_> = chat
        .messages()
        .filter(|m| m.role == Role::Assistant)
        .collect();
    // While a stream writes the newest answer, the one before it is the
    // last finished one.
    let skip = usize::from(chat.phase() == Phase::Streaming);
    let finished = answers.iter().rev().nth(skip)?;
    (!finished.text.is_empty()).then(|| finished.text.clone())
}

/// The code lines (without the fence lines) of the last fenced block of the
/// last assistant message. An unclosed fence runs to the end of the message.
pub fn last_code_block(chat: &ChatState) -> Option<String> {
    let text = last_answer(chat)?;
    let mut block: Option<Vec<&str>> = None;
    let mut open: Option<Vec<&str>> = None;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            match open.take() {
                Some(done) => block = Some(done),
                None => open = Some(Vec::new()),
            }
        } else if let Some(lines) = open.as_mut() {
            lines.push(line);
        }
    }
    let last = open.or(block)?;
    (!last.is_empty()).then(|| last.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ai::chat::{
        Entry, MsgStatus,
        engine::{Finish, StreamEvent},
        state_tests::{chat, sent},
    };

    /// A chat whose last turn answered `body`.
    fn answered(body: &str) -> ChatState {
        let mut state = chat();
        let (turn, _) = sent(&mut state, "q");
        state.apply(turn, StreamEvent::Delta(body.into()));
        state.apply(turn, StreamEvent::Done(Finish::Stop));
        state
    }

    #[test]
    fn the_last_answer_is_its_exact_text() {
        assert_eq!(last_answer(&answered("hello")).as_deref(), Some("hello"));
    }

    #[test]
    fn no_assistant_message_gives_nothing() {
        let mut state = chat();
        assert_eq!(last_answer(&state), None);
        sent(&mut state, "q");
        assert_eq!(last_answer(&state), None);
        assert_eq!(last_code_block(&state), None);
    }

    #[test]
    fn the_newest_answer_wins() {
        let mut state = answered("one");
        let (turn, _) = sent(&mut state, "again");
        state.apply(turn, StreamEvent::Delta("two".into()));
        state.apply(turn, StreamEvent::Done(Finish::Stop));
        assert_eq!(last_answer(&state).as_deref(), Some("two"));
    }

    #[test]
    fn a_cancelled_answer_can_be_yanked() {
        let mut state = chat();
        let (turn, _) = sent(&mut state, "q");
        state.apply(turn, StreamEvent::Delta("Hel".into()));
        state.cancel();
        assert_eq!(last_answer(&state).as_deref(), Some("Hel"));
    }

    #[test]
    fn an_answer_still_streaming_is_not_finished() {
        let mut state = answered("old");
        let (turn, _) = sent(&mut state, "again");
        state.apply(turn, StreamEvent::Delta("new, so far".into()));
        assert_eq!(last_answer(&state).as_deref(), Some("old"));
    }

    #[test]
    fn reasoning_and_system_lines_are_never_yanked() {
        let mut state = chat();
        let (turn, _) = sent(&mut state, "q");
        state.apply(turn, StreamEvent::ThinkingDelta("secret plan".into()));
        state.apply(turn, StreamEvent::Delta("hello".into()));
        state.apply(turn, StreamEvent::Done(Finish::Stop));
        state.entries.push(Entry::System("a notice".into()));
        assert!(
            state
                .entries()
                .iter()
                .any(|e| matches!(e, Entry::Reasoning(_)))
        );
        assert_eq!(last_answer(&state).as_deref(), Some("hello"));
        assert_eq!(last_code_block(&state), None);
    }

    #[test]
    fn the_last_code_block_loses_its_fences() {
        let state = answered("text\n```rust\nlet x = 1;\nlet y = 2;\n```\nmore");
        assert_eq!(
            last_code_block(&state).as_deref(),
            Some("let x = 1;\nlet y = 2;")
        );
    }

    #[test]
    fn the_last_of_several_blocks_wins() {
        let state = answered("```\na\n```\ntext\n```\nb\n```");
        assert_eq!(last_code_block(&state).as_deref(), Some("b"));
    }

    #[test]
    fn an_unclosed_fence_runs_to_the_end() {
        let state = answered("ok\n```\nlet x = 1;\nlet y = 2;");
        assert_eq!(
            last_code_block(&state).as_deref(),
            Some("let x = 1;\nlet y = 2;")
        );
    }

    #[test]
    fn no_fence_or_an_empty_block_gives_nothing() {
        assert_eq!(last_code_block(&answered("plain")), None);
        assert_eq!(last_code_block(&answered("```\n```")), None);
    }

    #[test]
    fn a_truncated_answer_is_still_an_answer() {
        let mut state = answered("hello");
        if let Some(Entry::Message(m)) = state.entries.last_mut() {
            m.status = MsgStatus::Truncated;
        }
        assert_eq!(last_answer(&state).as_deref(), Some("hello"));
    }
}
