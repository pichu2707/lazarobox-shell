//! The decoder of `claude -p --output-format stream-json` output: one JSON
//! event per line. Pure: bytes in, `StreamEvent`s out, whatever the chunking.
//!
//! What the CLI prints (claude 2.1.299, `tests/fixtures/claude/`):
//!
//! - `stream_event` lines wrap the API stream. A `text_delta` is an answer
//!   fragment and a `thinking_delta` is reasoning; `signature_delta`, block
//!   starts and stops, and `message_*` events carry nothing we show.
//! - an `assistant` line repeats the whole message once the block is complete,
//!   so it is skipped: the answer is the sum of the deltas.
//! - the `result` line ends the turn. It repeats the whole answer too, which is
//!   why CLI lines may be 2 MiB. A failure arrives as `is_error: true` (with
//!   `subtype: "success"`, as the CLI reports an API error that way).
//! - a tool block that is not `mcp__lazarobox__*` fails the turn closed; one that
//!   is ours adds nothing (the host announces the call). The shape is the one
//!   claude 2.1.x documents; the manual check records the real one.
//! - `system`, `rate_limit_event` and any unknown type are ignored, and so is
//!   any line that does not start with `{` (stray stdout text).

use serde_json::Value;

use super::engine::{ChatError, Finish, StreamEvent};
use super::stream::{
    CLI_LINE_CAP, Lines, StreamDecoder, TooLong, emit, error_message, finish_with, malformed,
    too_long,
};
use crate::core::ai::tools::wiring::CLAUDE_TOOL_PREFIX;

/// The failure of a turn in which claude started a tool that is not ours.
pub const TOOL_REFUSED: &str = "claude tried to use a tool";

/// Whether this line starts a tool that is not one of lazarobox's own. The
/// only tool claude may use is `mcp__lazarobox__*` (what the app serves; its
/// rows come from the host, not from this stream), so anything else, in the
/// stream or in a repeated message, sub-agents included, means the flags that
/// remove the other tools did not hold. A content block whose type ends in
/// `tool_use` (`server_tool_use` too) counts, and so does one without a name.
fn started_foreign_tool(value: &Value) -> bool {
    let blocks: Vec<&Value> = match value["type"].as_str() {
        Some("stream_event") if value["event"]["type"].as_str() == Some("content_block_start") => {
            vec![&value["event"]["content_block"]]
        }
        Some("assistant") => value["message"]["content"]
            .as_array()
            .map(|items| items.iter().collect())
            .unwrap_or_default(),
        _ => return false,
    };
    blocks
        .into_iter()
        .any(|block| match block["type"].as_str() {
            Some("tool_use") => !block["name"]
                .as_str()
                .is_some_and(|name| name.starts_with(CLAUDE_TOOL_PREFIX)),
            Some(other) => other.ends_with("tool_use"),
            None => false,
        })
}

/// Decodes the stream-json output of the claude CLI.
pub struct ClaudeDecoder {
    lines: Lines,
    ended: bool,
    /// An answer fragment went out: the `result` text is a repeat.
    streamed: bool,
}

impl ClaudeDecoder {
    pub fn new() -> Self {
        Self {
            lines: Lines::new(CLI_LINE_CAP),
            ended: false,
            streamed: false,
        }
    }

    /// Events of one line.
    fn line(&mut self, line: &[u8]) -> Vec<StreamEvent> {
        // Only a line that starts like a JSON object is an event. Anything
        // else (a warning a wrapper or runtime prints on stdout) is noise.
        if line.trim_ascii_start().first() != Some(&b'{') {
            return vec![];
        }
        let Ok(value) = serde_json::from_str::<Value>(&String::from_utf8_lossy(line)) else {
            return vec![malformed()];
        };
        if started_foreign_tool(&value) {
            return vec![StreamEvent::Error(ChatError::Failed(TOOL_REFUSED.into()))];
        }
        match value["type"].as_str() {
            Some("stream_event") => self.stream_event(&value),
            Some("result") => self.result(&value),
            _ => vec![],
        }
    }

    fn stream_event(&mut self, value: &Value) -> Vec<StreamEvent> {
        // A sub-agent's stream is not this chat's answer.
        if !value["parent_tool_use_id"].is_null() {
            return vec![];
        }
        let event = &value["event"];
        if event["type"].as_str() != Some("content_block_delta") {
            return vec![];
        }
        let delta = &event["delta"];
        match delta["type"].as_str() {
            Some("text_delta") => {
                let text = delta["text"].as_str().filter(|t| !t.is_empty());
                self.streamed |= text.is_some();
                text.map(|t| StreamEvent::Delta(t.into()))
                    .into_iter()
                    .collect()
            }
            Some("thinking_delta") => delta["thinking"]
                .as_str()
                .filter(|t| !t.is_empty())
                .map(|t| StreamEvent::ThinkingDelta(t.into()))
                .into_iter()
                .collect(),
            _ => vec![],
        }
    }

    fn result(&self, value: &Value) -> Vec<StreamEvent> {
        let text = value["result"].as_str().unwrap_or("");
        let failed = value["is_error"].as_bool() == Some(true)
            || value["subtype"]
                .as_str()
                .is_some_and(|s| s.starts_with("error"));
        if failed {
            return vec![StreamEvent::Error(ChatError::Failed(error_message(text)))];
        }
        let mut events = Vec::new();
        // Without partial messages the result is the only copy of the answer.
        if !self.streamed && !text.is_empty() {
            events.push(StreamEvent::Delta(text.into()));
        }
        let finish = if value["stop_reason"].as_str() == Some("max_tokens") {
            Finish::Length
        } else {
            Finish::Stop
        };
        events.push(StreamEvent::Done(finish));
        events
    }

    fn lines_events(&mut self, lines: Vec<Result<Vec<u8>, TooLong>>) -> Vec<StreamEvent> {
        let mut out = Vec::new();
        for line in lines {
            if self.ended {
                break;
            }
            let events = line.map_or_else(|TooLong| vec![too_long()], |l| self.line(&l));
            emit(&mut self.ended, &mut out, events);
        }
        out
    }
}

impl Default for ClaudeDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamDecoder for ClaudeDecoder {
    fn feed(&mut self, bytes: &[u8]) -> Vec<StreamEvent> {
        if self.ended {
            return Vec::new();
        }
        let lines = self.lines.feed(bytes);
        self.lines_events(lines)
    }

    fn finish(&mut self) -> Vec<StreamEvent> {
        let mut out = Vec::new();
        if !self.ended {
            let rest = self.lines.rest();
            out = self.lines_events(vec![rest]);
        }
        finish_with(&mut self.ended, out)
    }
}

#[cfg(test)]
mod tests;
