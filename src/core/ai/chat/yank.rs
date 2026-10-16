//! What a yank copies out of a chat: the last finished answer, or the last
//! fenced code block of it. Reasoning and system lines are never part of it.

use super::{ChatState, Entry, MsgStatus, Phase, engine::Role, transcript::is_fence};

/// The text of the last assistant message that is not still being written.
/// `None` when there is none; an empty one is passed over for the one before.
pub fn last_answer(chat: &ChatState) -> Option<String> {
    let answers: Vec<_> = chat
        .messages()
        .filter(|m| m.role == Role::Assistant)
        .collect();
    // While a stream writes the newest answer, the one before it is the
    // last finished one. A summary run streams into the draft, not into a
    // message: nothing is being written, so nothing is skipped.
    let skip = usize::from(chat.phase() == Phase::Streaming && !chat.summarizing());
    let finished = answers
        .iter()
        .rev()
        .skip(skip)
        .find(|m| !m.text.is_empty())?;
    Some(finished.text.clone())
}

/// One fenced block of a message.
struct Fence<'a> {
    /// The text after the opening fence, trimmed.
    info: &'a str,
    /// The lines between the fences.
    body: Vec<&'a str>,
    /// The closing fence was seen. Only the last block can be unclosed.
    closed: bool,
}

/// The fenced blocks of `text`, in order. An unclosed fence runs to the end.
/// This is the one fence parser, and it asks the renderer's own predicate
/// (`transcript::is_fence`) on the renderer's own lines, so what is selected
/// is what the user sees drawn as code.
fn fences(text: &str) -> Vec<Fence<'_>> {
    let mut done = Vec::new();
    let mut open: Option<Fence<'_>> = None;
    for line in text.split('\n') {
        if is_fence(line, open.is_some()) {
            match open.take() {
                Some(mut fence) => {
                    fence.closed = true;
                    done.push(fence);
                }
                None => {
                    let info = line.trim_start().trim_start_matches('`').trim();
                    open = Some(Fence {
                        info,
                        body: Vec::new(),
                        closed: false,
                    });
                }
            }
        } else if let Some(fence) = open.as_mut() {
            fence.body.push(line);
        }
    }
    done.extend(open);
    done
}

/// The code lines (without the fence lines) of the last fenced block of the
/// last assistant message. An unclosed fence runs to the end of the message.
pub fn last_code_block(chat: &ChatState) -> Option<String> {
    let text = last_answer(chat)?;
    let last = fences(&text).pop()?;
    (!last.body.is_empty()).then(|| last.body.join("\n"))
}

/// The body of the last code block of the last finished answer, when that
/// block is closed and its language is a shell: `bash`, `sh`, `shell` or
/// `zsh`, any case (the first word after the fence). Only the last block
/// counts, the one the user sees at the bottom: if it is another language or
/// still open, nothing is offered, and an earlier shell block is never
/// reached back for. `None` too when the block is empty, while a turn is in
/// flight, and when the answer was cancelled, cut or failed.
pub fn last_shell_block(chat: &ChatState) -> Option<String> {
    // The bottom block of the transcript is the one the user sees. While a
    // turn is in flight (waiting or streaming) that block is, or is about to
    // be, the new answer: offer nothing until it has finished.
    if chat.phase() != Phase::Idle {
        return None;
    }
    // Only the answer of the last turn counts. The last message of any role
    // must be a finished, successful, non-empty assistant answer, with no
    // system line (an error or a notice) after it: a turn that produced no
    // answer must not bring an older answer back.
    let (index, msg) = chat
        .entries
        .iter()
        .enumerate()
        .rev()
        .find_map(|(i, e)| match e {
            Entry::Message(m) => Some((i, m)),
            _ => None,
        })?;
    if msg.role != Role::Assistant || msg.status != MsgStatus::Complete || msg.text.is_empty() {
        return None;
    }
    if chat.entries[index + 1..]
        .iter()
        .any(|e| matches!(e, Entry::System(_)))
    {
        return None;
    }
    let last = fences(&msg.text).pop()?;
    let shell = last.info.split_whitespace().next().is_some_and(|lang| {
        ["bash", "sh", "shell", "zsh"]
            .iter()
            .any(|s| lang.eq_ignore_ascii_case(s))
    });
    (last.closed && shell && !last.body.is_empty()).then(|| last.body.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ai::chat::{
        Entry, Msg, MsgStatus,
        engine::{Finish, StreamEvent},
        state_tests::{chat, sent},
        transcript::{LineKind, assistant_rows},
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
    fn an_empty_finished_answer_falls_back_to_the_one_before() {
        let mut state = answered("```\nold code\n```");
        // A stored or truncated answer can be empty.
        state.entries.push(Entry::Message(Msg {
            role: Role::Assistant,
            text: String::new(),
            status: MsgStatus::Truncated,
        }));
        assert_eq!(
            state
                .messages()
                .filter(|m| m.role == Role::Assistant)
                .count(),
            2,
            "the empty answer is a message"
        );
        assert_eq!(last_answer(&state).as_deref(), Some("```\nold code\n```"));
        assert_eq!(last_code_block(&state).as_deref(), Some("old code"));
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

    #[test]
    fn shell_languages_are_found_in_any_case() {
        for lang in [
            "bash",
            "sh",
            "shell",
            "zsh",
            "BASH",
            "Sh",
            "Zsh",
            "bash title=x",
        ] {
            let state = answered(&format!("text\n```{lang}\nls -la\n```\n"));
            assert_eq!(
                last_shell_block(&state).as_deref(),
                Some("ls -la"),
                "{lang}"
            );
        }
    }

    #[test]
    fn other_or_no_language_gives_nothing() {
        for body in [
            "```rust\nlet x = 1;\n```",
            "```\nls\n```",
            "```bashful\nls\n```",
            "```python\nprint(1)\n```",
            "```bash\nls\n",
            "plain",
            "```bash\n```",
        ] {
            assert_eq!(last_shell_block(&answered(body)), None, "{body:?}");
        }
    }

    #[test]
    fn only_the_last_block_counts_and_it_must_be_a_closed_shell_block() {
        let state = answered("```bash\none\n```\n```sh\ntwo\nthree\n```");
        assert_eq!(last_shell_block(&state).as_deref(), Some("two\nthree"));
        // A later block of another language hides the earlier shell block.
        let state = answered("```bash\none\n```\ntext\n```rust\nx\n```");
        assert_eq!(last_shell_block(&state), None);
        // So does a later block that is still open.
        let state = answered("```bash\none\n```\ntext\n```bash\ntwo");
        assert_eq!(last_shell_block(&state), None);
    }

    /// The last code block the renderer shows, as a shell command would be
    /// read from it: `Some(body)` only for a closed shell block. Blocks are
    /// split at closing fences, so adjacent blocks stay apart.
    fn rendered_shell_block(text: &str) -> Option<String> {
        let mut rows = Vec::new();
        assistant_rows(text, 10_000, &mut rows);
        // (opening row, body rows, closed)
        let mut blocks: Vec<(String, Vec<String>, bool)> = Vec::new();
        let mut open: Option<(String, Vec<String>)> = None;
        for row in &rows {
            if row.kind != LineKind::Code {
                assert!(open.is_none(), "prose inside a block: {row:?}");
                continue;
            }
            match open.take() {
                None => open = Some((row.text.clone(), Vec::new())),
                Some((first, body)) if row.text.trim_start().starts_with("```") => {
                    blocks.push((first, body, true));
                }
                Some((first, mut body)) => {
                    body.push(row.text.clone());
                    open = Some((first, body));
                }
            }
        }
        if let Some((first, body)) = open {
            blocks.push((first, body, false));
        }
        let (first, body, closed) = blocks.pop()?;
        let lang = first
            .trim_start()
            .trim_start_matches('`')
            .split_whitespace()
            .next()?;
        let shell = ["bash", "sh", "shell", "zsh"]
            .iter()
            .any(|s| lang.eq_ignore_ascii_case(s));
        (closed && shell && !body.is_empty()).then(|| body.join("\n"))
    }

    /// Every sequence of 1 to 4 lines drawn from `lines`.
    fn sequences(lines: &[&str], depth: usize, stack: &mut Vec<usize>, out: &mut Vec<String>) {
        if !stack.is_empty() {
            out.push(
                stack
                    .iter()
                    .map(|&i| lines[i])
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
        if depth == 0 {
            return;
        }
        for i in 0..lines.len() {
            stack.push(i);
            sequences(lines, depth - 1, stack, out);
            stack.pop();
        }
    }

    #[test]
    fn every_small_fence_combination_agrees_with_the_renderer() {
        let lines = [
            "```bash",
            "```sh",
            "```",
            "```a```",
            "````bash",
            "  ```bash",
            "  ```",
            "echo x",
            "text",
            "```rust",
        ];
        let mut all = Vec::new();
        sequences(&lines, 4, &mut Vec::new(), &mut all);
        assert_eq!(all.len(), 10 + 100 + 1000 + 10_000);
        for body in all {
            assert_eq!(
                last_shell_block(&answered(&body)),
                rendered_shell_block(&body),
                "{body:?}"
            );
        }
    }

    #[test]
    fn the_selected_block_is_the_one_the_renderer_shows() {
        for body in [
            "```bash\nrm -rf ~\n```\n```a```\nprose\n```bash\necho safe\n```",
            "```bash\nrm -rf ~\n```\ntext\n```a```\necho safe\n```",
            "```bash\necho one\n```\ntext ```a``` more\n```bash\necho two\n```",
            "````bash\necho four\n```",
            "```bash\necho a\n```\ntext\n````bash\necho four\n````",
            "  ```bash\n  echo indented\n  ```",
            "```bash`x\necho tick\n```",
            "text\n```bash\necho x\n```rust\nafter close",
            "```bash\necho x\n```rust\n",
            "```bash\necho one\n```\ntext\n```bash\necho unclosed",
            "```bash\necho one\n```\ntext\n```rust\nnot shell\n```",
            "```bash\nrm -rf ~\n```\n```a```b\ntext\n```sh\necho safe\n```",
            "```bash\r\necho crlf\n```",
            "```bash\na\n```\n```sh\nb\n```",
            "```sh\nb\n```\n```rust\nr\n```",
            "```rust\nr\n```\n```sh\nb\n```",
            "```bash\na\n```\n```sh\nb",
            "```bash\na\n```\n```sh",
            "```bash\n  ```\nafter\n```",
            "```bash\n  ```bash\nx\n```\n```sh\nb\n```",
        ] {
            let state = answered(body);
            assert_eq!(
                last_shell_block(&state),
                rendered_shell_block(body),
                "{body:?}"
            );
        }
    }

    #[test]
    fn nothing_is_offered_while_an_answer_streams_or_is_awaited() {
        let mut state = answered("```bash\nold\n```");
        let (turn, _) = sent(&mut state, "again");
        // Waiting: the new message does not exist yet.
        assert_eq!(last_shell_block(&state), None);
        // Streaming: the bottom block is the one being written.
        state.apply(turn, StreamEvent::Delta("```bash\nnew\n```".into()));
        assert_eq!(last_shell_block(&state), None);
        state.apply(turn, StreamEvent::Done(Finish::Stop));
        assert_eq!(last_shell_block(&state).as_deref(), Some("new"));
    }

    #[test]
    fn a_cancelled_or_failed_answer_offers_nothing() {
        let mut state = chat();
        let (turn, _) = sent(&mut state, "q");
        state.apply(turn, StreamEvent::Delta("```bash\nrm -rf ~\n```".into()));
        state.cancel();
        assert_eq!(last_shell_block(&state), None);
        for status in [
            MsgStatus::Interrupted,
            MsgStatus::Truncated,
            MsgStatus::Error,
        ] {
            let mut state = answered("```bash\nls\n```");
            assert_eq!(last_shell_block(&state).as_deref(), Some("ls"));
            if let Some(Entry::Message(m)) = state.entries.last_mut() {
                m.status = status;
            }
            assert_eq!(last_shell_block(&state), None, "{status:?}");
        }
    }

    #[test]
    fn a_newer_turn_without_an_answer_never_reaches_back() {
        use crate::core::ai::chat::engine::ChatError;
        type End = fn(&mut ChatState, u64);
        let ends: [(&str, End); 5] = [
            ("done with no delta", |s, t| {
                s.apply(t, StreamEvent::Done(Finish::Stop));
            }),
            ("failed", |s, t| {
                s.apply(t, StreamEvent::Error(ChatError::Failed("boom".into())));
            }),
            ("truncated", |s, t| {
                s.apply(t, StreamEvent::Error(ChatError::Truncated));
            }),
            ("interrupted", |s, t| {
                s.apply(t, StreamEvent::Error(ChatError::Interrupted));
            }),
            ("cancelled while waiting", |s, _| {
                s.cancel();
            }),
        ];
        for (name, end) in ends {
            let mut state = answered("```bash\nrm -rf ~\n```");
            assert_eq!(last_shell_block(&state).as_deref(), Some("rm -rf ~"));
            let (turn, _) = sent(&mut state, "again");
            end(&mut state, turn);
            assert_eq!(state.phase(), Phase::Idle, "{name}");
            assert_eq!(last_shell_block(&state), None, "{name}");
        }
    }

    #[test]
    fn a_user_message_after_the_answer_hides_it() {
        let mut state = answered("```bash\nls\n```");
        sent(&mut state, "again");
        assert_eq!(last_shell_block(&state), None);
    }

    #[test]
    fn a_notice_after_the_answer_hides_it() {
        let mut state = answered("```bash\nls\n```");
        state.entries.push(Entry::System("a notice".into()));
        assert_eq!(last_shell_block(&state), None);
    }
}
