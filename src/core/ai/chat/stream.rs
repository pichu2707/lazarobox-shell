//! Incremental decoders for the two server stream formats: Ollama NDJSON and
//! OpenAI-style SSE. Pure: bytes in, `StreamEvent`s out, whatever the chunking.
//!
//! Deltas leave raw: the app feeds them to the stateful sanitizer, which has to
//! see them in order. Error text is sanitized and capped here, because it is
//! built from one whole line.

use serde_json::Value;

use super::engine::{ChatError, Finish, StreamEvent};
use crate::core::ai::sanitize::clean;

/// Line cap for server decoders, in bytes.
pub const SERVER_LINE_CAP: usize = 1 << 20;
/// Line cap for CLI decoders: the final `result` event repeats the answer.
pub const CLI_LINE_CAP: usize = 2 << 20;
/// Longest server error message kept, in characters.
pub const ERROR_CAP: usize = 200;

/// Turns bytes into events, one chunk at a time.
pub trait StreamDecoder {
    fn feed(&mut self, bytes: &[u8]) -> Vec<StreamEvent>;
    /// The stream ended: flushes what is pending, and reports a stream that
    /// never said it was done.
    fn finish(&mut self) -> Vec<StreamEvent>;
}

/// Splits bytes into lines at `\n`, dropping a trailing `\r`. A line over the
/// cap is reported once and the buffer is dropped.
pub(super) struct Lines {
    buf: Vec<u8>,
    /// Leading bytes of `buf` already known to hold no newline.
    scanned: usize,
    cap: usize,
}

/// A line was longer than the cap.
pub(super) struct TooLong;

impl Lines {
    pub(super) fn new(cap: usize) -> Self {
        Self {
            buf: Vec::new(),
            scanned: 0,
            cap,
        }
    }

    pub(super) fn feed(&mut self, bytes: &[u8]) -> Vec<Result<Vec<u8>, TooLong>> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        let mut start = 0;
        let mut from = self.scanned;
        while let Some(i) = self.buf[from..].iter().position(|&b| b == b'\n') {
            let line = trim_cr(&self.buf[start..from + i]);
            if line.len() > self.cap {
                self.buf.clear();
                self.scanned = 0;
                out.push(Err(TooLong));
                return out;
            }
            out.push(Ok(line.to_vec()));
            start = from + i + 1;
            from = start;
        }
        self.buf.drain(..start);
        self.scanned = self.buf.len();
        // One byte of slack for a `\r` whose `\n` has not arrived yet, so the
        // verdict never depends on where the chunks were cut.
        if self.buf.len() > self.cap + 1 {
            self.buf.clear();
            self.scanned = 0;
            out.push(Err(TooLong));
        }
        out
    }

    /// The last line, when the stream ended without a newline.
    pub(super) fn rest(&mut self) -> Result<Vec<u8>, TooLong> {
        self.scanned = 0;
        let buf = std::mem::take(&mut self.buf);
        let line = trim_cr(&buf);
        if line.len() > self.cap {
            Err(TooLong)
        } else {
            Ok(line.to_vec())
        }
    }
}

fn trim_cr(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

pub(super) fn too_long() -> StreamEvent {
    StreamEvent::Error(ChatError::Failed("stream line over the size limit".into()))
}

pub(super) fn malformed() -> StreamEvent {
    StreamEvent::Error(ChatError::Failed("malformed stream data".into()))
}

/// Appends `events` to `out` up to and including the first final one, which
/// also ends the stream.
pub(super) fn emit(ended: &mut bool, out: &mut Vec<StreamEvent>, events: Vec<StreamEvent>) {
    for event in events {
        let last = matches!(event, StreamEvent::Done(_) | StreamEvent::Error(_));
        out.push(event);
        if last {
            *ended = true;
            return;
        }
    }
}

pub(super) fn finish_with(ended: &mut bool, mut out: Vec<StreamEvent>) -> Vec<StreamEvent> {
    if !*ended {
        *ended = true;
        out.push(StreamEvent::Error(ChatError::Interrupted));
    }
    out
}

fn finish_reason(reason: Option<&str>) -> Finish {
    if reason == Some("length") {
        Finish::Length
    } else {
        Finish::Stop
    }
}

/// The error an engine reported in `value`, if any: `{"error":"..."}` or
/// `{"error":{"message":"..."}}`, flattened to one line and capped. Only a
/// non-empty string or an object counts; `null`, `false`, `""` and numbers
/// are values some servers send on healthy chunks.
fn server_error(value: &Value) -> Option<StreamEvent> {
    let error = value.get("error")?;
    let text = match error {
        Value::String(s) if !s.is_empty() => s.as_str(),
        Value::Object(_) => error.get("message").and_then(Value::as_str).unwrap_or(""),
        _ => return None,
    };
    let message = error_message(text);
    Some(StreamEvent::Error(ChatError::Failed(message)))
}

/// An engine's error text as one safe line: sanitized, whitespace collapsed,
/// at most `ERROR_CAP` characters, `engine error` when nothing is left.
pub(super) fn error_message(text: &str) -> String {
    // `clean` keeps `\n` and `\t`; both would let the text fake extra lines.
    let flat = clean(text).split_whitespace().collect::<Vec<_>>().join(" ");
    let capped: String = flat.chars().take(ERROR_CAP).collect();
    let capped = capped.trim_end();
    if capped.is_empty() {
        "engine error".to_owned()
    } else {
        capped.to_owned()
    }
}

fn delta(text: Option<&str>) -> Option<StreamEvent> {
    text.filter(|t| !t.is_empty())
        .map(|t| StreamEvent::Delta(t.into()))
}

fn thinking(text: Option<&str>) -> Option<StreamEvent> {
    text.filter(|t| !t.is_empty())
        .map(|t| StreamEvent::ThinkingDelta(t.into()))
}

/// Events of one Ollama line.
fn ndjson_line(line: &[u8]) -> Vec<StreamEvent> {
    if line.iter().all(u8::is_ascii_whitespace) {
        return vec![];
    }
    let Ok(value) = serde_json::from_str::<Value>(&String::from_utf8_lossy(line)) else {
        return vec![malformed()];
    };
    if let Some(error) = server_error(&value) {
        return vec![error];
    }
    // Reasoning first: Ollama sends `thinking` next to an empty `content`, and
    // a line holding both reads in that order.
    let message = &value["message"];
    let mut events: Vec<StreamEvent> = thinking(message["thinking"].as_str())
        .into_iter()
        .chain(delta(message["content"].as_str()))
        .collect();
    if value["done"].as_bool() == Some(true) {
        events.push(StreamEvent::Done(finish_reason(
            value["done_reason"].as_str(),
        )));
    }
    events
}

/// Events of one SSE `data` payload.
fn sse_event(data: &[u8]) -> Vec<StreamEvent> {
    if data.trim_ascii() == b"[DONE]" {
        return vec![StreamEvent::Done(Finish::Stop)];
    }
    let Ok(value) = serde_json::from_str::<Value>(&String::from_utf8_lossy(data)) else {
        return vec![malformed()];
    };
    if let Some(error) = server_error(&value) {
        return vec![error];
    }
    let choice = &value["choices"][0];
    let mut events: Vec<StreamEvent> = delta(choice["delta"]["content"].as_str())
        .into_iter()
        .collect();
    if let Some(reason) = choice["finish_reason"].as_str() {
        events.push(StreamEvent::Done(finish_reason(Some(reason))));
    }
    events
}

/// Decodes Ollama `/api/chat` replies, one JSON object per line.
pub struct NdjsonSplitter {
    lines: Lines,
    ended: bool,
}

impl NdjsonSplitter {
    pub fn new(line_cap: usize) -> Self {
        Self {
            lines: Lines::new(line_cap),
            ended: false,
        }
    }

    pub fn server() -> Self {
        Self::new(SERVER_LINE_CAP)
    }
}

impl StreamDecoder for NdjsonSplitter {
    fn feed(&mut self, bytes: &[u8]) -> Vec<StreamEvent> {
        let mut out = Vec::new();
        if self.ended {
            return out;
        }
        for line in self.lines.feed(bytes) {
            let events = line.map_or_else(|TooLong| vec![too_long()], |l| ndjson_line(&l));
            emit(&mut self.ended, &mut out, events);
            if self.ended {
                break;
            }
        }
        out
    }

    fn finish(&mut self) -> Vec<StreamEvent> {
        let mut out = Vec::new();
        if !self.ended {
            let events = self
                .lines
                .rest()
                .map_or_else(|TooLong| vec![too_long()], |l| ndjson_line(&l));
            emit(&mut self.ended, &mut out, events);
        }
        finish_with(&mut self.ended, out)
    }
}

/// Decodes OpenAI-compatible `/v1/chat/completions` replies (SSE).
pub struct SseParser {
    lines: Lines,
    /// The `data` lines of the event being read, joined by `\n`.
    data: Option<Vec<u8>>,
    cap: usize,
    ended: bool,
}

impl SseParser {
    pub fn new(event_cap: usize) -> Self {
        Self {
            lines: Lines::new(event_cap),
            data: None,
            cap: event_cap,
            ended: false,
        }
    }

    pub fn server() -> Self {
        Self::new(SERVER_LINE_CAP)
    }

    /// Reads one line; a blank one dispatches the event.
    fn line(&mut self, line: &[u8]) -> Vec<StreamEvent> {
        if line.is_empty() {
            return self.dispatch();
        }
        let (field, value) = match line.iter().position(|&b| b == b':') {
            Some(0) => return vec![],
            Some(i) => (&line[..i], &line[i + 1..]),
            None => (line, &line[line.len()..]),
        };
        if field == b"data" {
            let data = self.data.get_or_insert_with(Vec::new);
            if !data.is_empty() {
                data.push(b'\n');
            }
            data.extend_from_slice(value.strip_prefix(b" ").unwrap_or(value));
            if data.len() > self.cap {
                return vec![too_long()];
            }
        }
        vec![]
    }

    fn dispatch(&mut self) -> Vec<StreamEvent> {
        match self.data.take() {
            Some(data) if !data.is_empty() => sse_event(&data),
            _ => vec![],
        }
    }
}

impl StreamDecoder for SseParser {
    fn feed(&mut self, bytes: &[u8]) -> Vec<StreamEvent> {
        let mut out = Vec::new();
        if self.ended {
            return out;
        }
        for line in self.lines.feed(bytes) {
            let events = match line {
                Ok(l) => self.line(&l),
                Err(TooLong) => vec![too_long()],
            };
            emit(&mut self.ended, &mut out, events);
            if self.ended {
                break;
            }
        }
        out
    }

    fn finish(&mut self) -> Vec<StreamEvent> {
        let mut out = Vec::new();
        if !self.ended {
            let mut events = match self.lines.rest() {
                Ok(rest) => self.line(&rest),
                Err(TooLong) => vec![too_long()],
            };
            if events.is_empty() {
                events = self.dispatch();
            }
            emit(&mut self.ended, &mut out, events);
        }
        finish_with(&mut self.ended, out)
    }
}

#[cfg(test)]
mod tests;
