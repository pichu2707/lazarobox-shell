//! The decoder of `gemini -o stream-json` output: one JSON event per line.
//! Pure: bytes in, `StreamEvent`s out, whatever the chunking.
//!
//! What the CLI prints (gemini 0.63.0, `tests/fixtures/gemini/`):
//!
//! - `init` carries the session id and the model; nothing we show.
//! - a `message` with `role: "user"` echoes the prompt, and is skipped. A
//!   `message` with `role: "assistant"` and `delta: true` is an answer
//!   fragment (a token can be cut in two fragments).
//! - `result` ends the turn: `status: "success"` is a stop, anything else is a
//!   failure whose `error.message` is shown, without the `[API Error: …]`
//!   wrapper the CLI puts around the API's text.
//! - **no reasoning**: no event carries thoughts with the pinned flags (also
//!   on a model asked to think), so nothing is shown as thinking.
//! - a `tool_use` means the policy that removes every tool did not hold: the
//!   turn ends with an error (the runtime then stops the process). The jail is
//!   what actually stops the tool. `tool_result` is ignored. The one exception
//!   is a wired turn (the lazarobox MCP server is on): a `tool_use` named
//!   `mcp_lazarobox_<tool>` passes, whatever else is refused. The qualified
//!   name is what tells our `read_file` from gemini's own built-in one; a
//!   text-only turn allows no tool, ours included.
//! - an `error` event is a non-fatal warning and is skipped. Any other kind of
//!   event ends the turn with an error: an allowlist, so a newer gemini that
//!   adds a kind (a thought, a tool of a new sort) fails closed.
//! - a line that does not start with `{` is ignored.

use serde_json::Value;

use super::engine::{ChatError, Finish, StreamEvent, Usage};
use super::stream::{
    CLI_LINE_CAP, Lines, StreamDecoder, TooLong, emit, error_message, finish_with, malformed,
    too_long,
};
use crate::core::ai::tools::wiring::GEMINI_TOOL_PREFIX;

/// The failure of a turn in which gemini started a tool.
pub const TOOL_REFUSED: &str = "gemini tried to use a tool";

/// The failure of a turn in which gemini sent an event kind we do not know.
/// The decoder fails closed: a newer gemini that adds a kind of event (a tool
/// of a new sort, a plan) is refused until lazarobox is updated.
pub const UNKNOWN_EVENT: &str =
    "gemini sent an event this version of lazarobox does not know; update lazarobox";

/// Decodes the stream-json output of the gemini CLI.
pub struct GeminiDecoder {
    lines: Lines,
    ended: bool,
    /// The turn has the lazarobox tools wired in: its `tool_use` events (and
    /// only those) are allowed.
    wired: bool,
}

impl GeminiDecoder {
    pub fn new() -> Self {
        Self {
            lines: Lines::new(CLI_LINE_CAP),
            ended: false,
            wired: false,
        }
    }

    /// For a turn whose gemini got the lazarobox MCP server.
    pub fn wired() -> Self {
        Self {
            wired: true,
            ..Self::new()
        }
    }

    /// Events of one line.
    fn line(&self, line: &[u8]) -> Vec<StreamEvent> {
        // Only a line that starts like a JSON object is an event. Anything
        // else (a warning a wrapper or runtime prints on stdout) is noise.
        if line.trim_ascii_start().first() != Some(&b'{') {
            return vec![];
        }
        let Ok(value) = serde_json::from_str::<Value>(&String::from_utf8_lossy(line)) else {
            return vec![malformed()];
        };
        match value["type"].as_str() {
            Some("message") => Self::message(&value),
            Some("tool_use") if self.wired && is_ours(&value) => vec![],
            Some("tool_use") => vec![StreamEvent::Error(ChatError::Failed(TOOL_REFUSED.into()))],
            Some("result") => Self::result(&value),
            // Known and harmless: the start of the session, the result of a
            // tool that was already refused, a non-fatal warning.
            Some("init" | "tool_result" | "error") => vec![],
            _ => vec![StreamEvent::Error(ChatError::Failed(UNKNOWN_EVENT.into()))],
        }
    }

    fn message(value: &Value) -> Vec<StreamEvent> {
        // Only an assistant fragment is the answer; a message that says it is
        // not a fragment (`delta: false`) would repeat text already sent.
        if value["role"].as_str() != Some("assistant") || value["delta"] == Value::Bool(false) {
            return vec![];
        }
        match value["content"].as_str().filter(|t| !t.is_empty()) {
            Some(text) => vec![StreamEvent::Delta(text.into())],
            None => vec![],
        }
    }

    fn result(value: &Value) -> Vec<StreamEvent> {
        if value["status"].as_str() == Some("success") {
            return usage(value)
                .map(StreamEvent::Usage)
                .into_iter()
                .chain([StreamEvent::Done(Finish::Stop)])
                .collect();
        }
        let text = value["error"]["message"].as_str().unwrap_or("");
        vec![StreamEvent::Error(ChatError::Failed(error_message(
            &unwrap_api_error(text),
        )))]
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

/// What the `result` line's `stats` report as used. `input_tokens` already
/// counts the cached ones. `None` unless both counts are there: a missing one is not zero.
fn usage(value: &Value) -> Option<Usage> {
    let stats = &value["stats"];
    let (input, output) = (
        stats["input_tokens"].as_u64(),
        stats["output_tokens"].as_u64(),
    );
    let (input, output) = input.zip(output)?;
    Some(Usage {
        input,
        output,
        cost_micros: None,
    })
}

/// Whether a `tool_use` is one of lazarobox's tools. gemini names an MCP tool
/// `mcp_{server}_{tool}`, so gemini's own `read_file` never matches ours; a
/// name that is missing, not text or not exactly that shape does not either.
fn is_ours(value: &Value) -> bool {
    value["tool_name"]
        .as_str()
        .and_then(|name| name.strip_prefix(GEMINI_TOOL_PREFIX))
        .is_some_and(|tool| !tool.is_empty())
}

/// The CLI wraps the API's text as `[API Error: <text>]` and may add its own
/// advice after the closing bracket. The wrapper adds nothing to the user.
fn unwrap_api_error(text: &str) -> String {
    match text.strip_prefix("[API Error: ") {
        // The wrapper's bracket is the last one; a `]` inside the text stays.
        Some(rest) => match rest.rfind(']') {
            Some(at) => format!("{}{}", &rest[..at], &rest[at + 1..]),
            None => rest.to_owned(),
        },
        None => text.to_owned(),
    }
}

impl Default for GeminiDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamDecoder for GeminiDecoder {
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
