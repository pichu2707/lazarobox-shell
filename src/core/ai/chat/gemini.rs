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
//!   what actually stops the tool. `tool_result` is ignored.
//! - an `error` event is a non-fatal warning and is skipped. Any other kind of
//!   event ends the turn with an error: an allowlist, so a newer gemini that
//!   adds a kind (a thought, a tool of a new sort) fails closed.
//! - a line that does not start with `{` is ignored.

use serde_json::Value;

use super::engine::{ChatError, Finish, StreamEvent};
use super::stream::{
    CLI_LINE_CAP, Lines, StreamDecoder, TooLong, emit, error_message, finish_with, malformed,
    too_long,
};

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
}

impl GeminiDecoder {
    pub fn new() -> Self {
        Self {
            lines: Lines::new(CLI_LINE_CAP),
            ended: false,
        }
    }

    /// Events of one line.
    fn line(line: &[u8]) -> Vec<StreamEvent> {
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
            Some("tool_use") => vec![StreamEvent::Error(ChatError::Failed(TOOL_REFUSED.into()))],
            Some("result") => vec![Self::result(&value)],
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

    fn result(value: &Value) -> StreamEvent {
        if value["status"].as_str() == Some("success") {
            return StreamEvent::Done(Finish::Stop);
        }
        let text = value["error"]["message"].as_str().unwrap_or("");
        StreamEvent::Error(ChatError::Failed(error_message(&unwrap_api_error(text))))
    }

    fn lines_events(&mut self, lines: Vec<Result<Vec<u8>, TooLong>>) -> Vec<StreamEvent> {
        let mut out = Vec::new();
        for line in lines {
            if self.ended {
                break;
            }
            let events = line.map_or_else(|TooLong| vec![too_long()], |l| Self::line(&l));
            emit(&mut self.ended, &mut out, events);
        }
        out
    }
}

/// The CLI wraps the API's text as `[API Error: <text>]` and may add its own
/// advice after the closing bracket. The wrapper adds nothing to the user.
fn unwrap_api_error(text: &str) -> String {
    match text.strip_prefix("[API Error: ") {
        Some(rest) => rest.replacen(']', "", 1),
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
