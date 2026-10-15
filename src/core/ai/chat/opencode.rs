//! The decoder of `opencode run --format json` output: one JSON event per
//! line. Pure: bytes in, `StreamEvent`s out, whatever the chunking.
//!
//! What the CLI prints (opencode 1.19.0, `tests/fixtures/opencode/`). Only the
//! `error` events were captured from a real run; the rest follow the event
//! schema of opencode's `run` command and are not verified against a
//! successful run:
//!
//! - `step_start` carries nothing we show.
//! - a `text` event holds a whole text part (there is no token streaming): it
//!   becomes one text delta. A turn can hold several; they are separated by a
//!   blank line.
//! - `step_finish` ends the turn. Its `part.reason` is `length` for a cut
//!   answer (a length stop), `tool-calls` when the model wanted a tool (the
//!   turn ends with an error) and anything else is a stop.
//! - an `error` event ends the turn with an error: the provider's message
//!   (`error.data.message`), else `error.message`, else the error's name.
//! - **no reasoning**: it is only printed with `--thinking`, which is not
//!   passed; a `reasoning` event is ignored if it shows up anyway.
//! - a `tool_use` means the permission rules that deny every tool did not
//!   hold: the turn ends with an error (the runtime then stops the process).
//!   The jail is what actually stops the tool.
//! - any other kind of event ends the turn with an error: an allowlist, so a
//!   newer opencode that adds a kind fails closed.
//! - a line that does not start with `{` is ignored.

use serde_json::Value;

use super::engine::{ChatError, Finish, StreamEvent};
use super::stream::{
    CLI_LINE_CAP, Lines, StreamDecoder, TooLong, emit, error_message, finish_with, malformed,
    too_long,
};

/// The failure of a turn in which opencode started a tool.
pub const TOOL_REFUSED: &str = "opencode tried to use a tool";

/// The failure of a turn in which opencode sent an event kind we do not know.
/// The decoder fails closed: a newer opencode that adds a kind of event is
/// refused until lazarobox is updated.
pub const UNKNOWN_EVENT: &str =
    "opencode sent an event this version of lazarobox does not know; update lazarobox";

/// Decodes the JSON events of `opencode run --format json`.
pub struct OpencodeDecoder {
    lines: Lines,
    ended: bool,
    /// A text went out: the next one starts after a blank line.
    answered: bool,
}

impl OpencodeDecoder {
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
            Some("text") => self.text(&value),
            Some("step_finish") => vec![Self::finish_step(&value)],
            Some("tool_use") => vec![StreamEvent::Error(ChatError::Failed(TOOL_REFUSED.into()))],
            Some("error") => vec![Self::failure(&value)],
            // Known and harmless: the start of a step, reasoning that was not
            // asked for.
            Some("step_start" | "reasoning") => vec![],
            _ => vec![StreamEvent::Error(ChatError::Failed(UNKNOWN_EVENT.into()))],
        }
    }

    fn text(&mut self, value: &Value) -> Vec<StreamEvent> {
        let Some(text) = value["part"]["text"].as_str().filter(|t| !t.is_empty()) else {
            return vec![];
        };
        let separator = if self.answered { "\n\n" } else { "" };
        self.answered = true;
        vec![StreamEvent::Delta(format!("{separator}{text}"))]
    }

    fn finish_step(value: &Value) -> StreamEvent {
        match value["part"]["reason"].as_str() {
            Some("length") => StreamEvent::Done(Finish::Length),
            Some("tool-calls") => StreamEvent::Error(ChatError::Failed(TOOL_REFUSED.into())),
            _ => StreamEvent::Done(Finish::Stop),
        }
    }

    fn failure(value: &Value) -> StreamEvent {
        let error = &value["error"];
        let text = error["data"]["message"]
            .as_str()
            .or(error["message"].as_str())
            .or(error["name"].as_str())
            .unwrap_or("");
        StreamEvent::Error(ChatError::Failed(error_message(text)))
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

impl Default for OpencodeDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamDecoder for OpencodeDecoder {
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
