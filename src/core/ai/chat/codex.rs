//! The decoder of `codex exec --json` output: one JSON event per line. Pure:
//! bytes in, `StreamEvent`s out, whatever the chunking.
//!
//! What the CLI prints (codex 0.142.5, `tests/fixtures/codex/`):
//!
//! - `thread.started` and `turn.started` carry nothing we show.
//! - an `item.completed` whose item is an `agent_message` holds a whole
//!   message (there is no token streaming): it becomes one text delta. A turn
//!   can hold several messages (a short note before the answer); they are
//!   separated by a blank line. `item.started` and `item.updated` repeat a
//!   message that is not final yet, so they are skipped.
//! - `turn.completed` ends the turn. `turn.failed` ends it with an error whose
//!   message is usually the API's JSON error body; the text inside is shown.
//! - a bare `error` event is a notice (a reconnect, or the first copy of the
//!   error a `turn.failed` repeats) and an `error` item is a warning: neither
//!   ends the turn.
//! - **no reasoning**: the usage counts reasoning tokens, but no event
//!   carries reasoning text with the pinned flags, so nothing is shown as
//!   thinking.
//! - an item that is a tool call (`command_execution`, `file_change`,
//!   `mcp_tool_call`, `collab_tool_call`) means the flags that remove the
//!   tools did not hold: the turn ends with an error (the runtime then stops
//!   the process). The jail is what actually stops the tool.
//! - everything else, including any line that does not start with `{`, is
//!   ignored.

use serde_json::Value;

use super::engine::{ChatError, Finish, StreamEvent};
use super::stream::{
    CLI_LINE_CAP, Lines, StreamDecoder, TooLong, emit, error_message, finish_with, malformed,
    too_long,
};

/// The failure of a turn in which codex started a tool.
pub const TOOL_REFUSED: &str = "codex tried to use a tool";

/// Items that run something or change something.
const TOOL_ITEMS: [&str; 4] = [
    "command_execution",
    "file_change",
    "mcp_tool_call",
    "collab_tool_call",
];

/// Decodes the JSONL output of `codex exec --json`.
pub struct CodexDecoder {
    lines: Lines,
    ended: bool,
    /// A message went out: the next one starts after a blank line.
    answered: bool,
}

impl CodexDecoder {
    pub fn new() -> Self {
        Self {
            lines: Lines::new(CLI_LINE_CAP),
            ended: false,
            answered: false,
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
        match value["type"].as_str() {
            Some("item.started" | "item.updated") => Self::tool_check(&value),
            Some("item.completed") => self.completed(&value),
            Some("turn.completed") => vec![StreamEvent::Done(Finish::Stop)],
            Some("turn.failed") => vec![StreamEvent::Error(ChatError::Failed(failure(&value)))],
            _ => vec![],
        }
    }

    /// The refusal when `value` is a tool item, else nothing.
    fn tool_check(value: &Value) -> Vec<StreamEvent> {
        let kind = value["item"]["type"].as_str();
        if kind.is_some_and(|k| TOOL_ITEMS.contains(&k)) {
            vec![StreamEvent::Error(ChatError::Failed(TOOL_REFUSED.into()))]
        } else {
            vec![]
        }
    }

    fn completed(&mut self, value: &Value) -> Vec<StreamEvent> {
        if value["item"]["type"].as_str() != Some("agent_message") {
            return Self::tool_check(value);
        }
        let Some(text) = value["item"]["text"].as_str().filter(|t| !t.is_empty()) else {
            return vec![];
        };
        let separator = if self.answered { "\n\n" } else { "" };
        self.answered = true;
        vec![StreamEvent::Delta(format!("{separator}{text}"))]
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

/// The text of a `turn.failed`: the `error.message` inside the API's JSON
/// body when the message is one, else the message as it is.
fn failure(value: &Value) -> String {
    let message = value["error"]["message"].as_str().unwrap_or("");
    let inner = serde_json::from_str::<Value>(message).ok();
    let text = inner
        .as_ref()
        .and_then(|body| {
            body["error"]["message"]
                .as_str()
                .or(body["message"].as_str())
        })
        .unwrap_or(message);
    error_message(text)
}

impl Default for CodexDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamDecoder for CodexDecoder {
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
