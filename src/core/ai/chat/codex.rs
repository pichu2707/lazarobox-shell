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
//! - an item of any kind but `agent_message`, `reasoning`, `error` and
//!   `todo_list` (a command, a file change, a tool call, web search, or a kind
//!   a newer codex adds) means the flags that remove the tools did not hold.
//!   The one exception is a wired turn (the lazarobox MCP server is on): an
//!   `mcp_tool_call` whose `server` is exactly `lazarobox` passes, any other
//!   server or a missing one does not. Unlike claude's, the text-only mode
//!   stays an allowlist too (every unknown kind ends the turn), because the
//!   shell is the tool a missed flag would leave open. The turn then ends with
//!   an error and the runtime stops the process. The allowlist means nothing
//!   unknown runs unseen; the jail is what actually stops the tool.
//! - a failure that says the model is not supported for the login gets a hint
//!   to pick another model.
//! - everything else, including any line that does not start with `{`, is
//!   ignored.

use serde_json::Value;

use super::engine::{ChatError, Finish, StreamEvent, Usage};
use super::stream::{
    CLI_LINE_CAP, ERROR_CAP, Lines, StreamDecoder, TooLong, emit, error_message, finish_with,
    malformed, too_long,
};
use crate::core::ai::tools::wiring::SERVER;

/// The failure of a turn in which codex started a tool.
pub const TOOL_REFUSED: &str = "codex tried to use a tool";

/// The only item kinds a text-only turn may hold. Any other kind (a command,
/// a file change, a tool call, web search, or a kind a newer codex adds) ends
/// the turn: an allowlist, so nothing unknown runs unseen.
const ALLOWED_ITEMS: [&str; 4] = ["agent_message", "reasoning", "error", "todo_list"];

/// Added to a failure that says the model is not available to the login.
pub const MODEL_HINT: &str = " - pick a model your account supports (custom…)";

/// What a failure text holds when the account cannot use the model.
const MODEL_UNSUPPORTED: &str = "not supported when using Codex";

/// Decodes the JSONL output of `codex exec --json`.
pub struct CodexDecoder {
    lines: Lines,
    ended: bool,
    /// A message went out: the next one starts after a blank line.
    answered: bool,
    /// The turn has the lazarobox tools wired in: its `mcp_tool_call` items
    /// (and only those) are allowed.
    wired: bool,
}

/// What `turn.completed` reports as used. `input_tokens` already counts the
/// cached ones. `None` when the line holds no token count at all.
fn usage(value: &Value) -> Option<Usage> {
    let counts = &value["usage"];
    let (input, output) = (
        counts["input_tokens"].as_u64(),
        counts["output_tokens"].as_u64(),
    );
    if input.is_none() && output.is_none() {
        return None;
    }
    Some(Usage {
        input: input.unwrap_or(0),
        output: output.unwrap_or(0),
        cost_micros: None,
    })
}

impl CodexDecoder {
    pub fn new() -> Self {
        Self {
            lines: Lines::new(CLI_LINE_CAP),
            ended: false,
            answered: false,
            wired: false,
        }
    }

    /// For a turn whose codex got the lazarobox MCP server.
    pub fn wired() -> Self {
        Self {
            wired: true,
            ..Self::new()
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
            Some("item.started" | "item.updated") => self.tool_check(&value),
            Some("item.completed") => self.completed(&value),
            Some("turn.completed") => usage(&value)
                .map(StreamEvent::Usage)
                .into_iter()
                .chain([StreamEvent::Done(Finish::Stop)])
                .collect(),
            Some("turn.failed") => vec![StreamEvent::Error(ChatError::Failed(failure(&value)))],
            _ => vec![],
        }
    }

    /// The refusal when the item of `value` is not an allowed kind. A wired
    /// turn also allows an `mcp_tool_call` whose `server` is exactly ours (the
    /// host announces the call, so it adds nothing here); a text-only turn
    /// allows no tool at all, ours included.
    fn tool_check(&self, value: &Value) -> Vec<StreamEvent> {
        let item = &value["item"];
        let kind = item["type"].as_str();
        let ours =
            self.wired && kind == Some("mcp_tool_call") && item["server"].as_str() == Some(SERVER);
        if ours || kind.is_some_and(|k| ALLOWED_ITEMS.contains(&k)) {
            vec![]
        } else {
            vec![StreamEvent::Error(ChatError::Failed(TOOL_REFUSED.into()))]
        }
    }

    fn completed(&mut self, value: &Value) -> Vec<StreamEvent> {
        if value["item"]["type"].as_str() != Some("agent_message") {
            return self.tool_check(value);
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
    let shown = error_message(text);
    if !text.contains(MODEL_UNSUPPORTED) {
        return shown;
    }
    // Keep the hint whole: cut the message, not the hint.
    let room = ERROR_CAP - MODEL_HINT.chars().count();
    let head: String = shown.chars().take(room).collect();
    format!("{}{MODEL_HINT}", head.trim_end())
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
