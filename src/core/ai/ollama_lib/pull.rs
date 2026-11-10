//! The reply of an Ollama pull (ollama-download): NDJSON lines of status,
//! per-digest `total`/`completed`, then `{"status":"success"}` or an
//! `error`. Pure: bytes in, [`Pulled`] events out, whatever the chunking.

use serde_json::Value;

use crate::core::ai::chat::stream::{Lines, SERVER_LINE_CAP};
use crate::core::ai::sanitize::clean;

/// Longest failure reason kept, in characters.
pub const REASON_CAP: usize = 200;

/// A stream that ended with neither `success` nor an `error`.
pub const CLOSED: &str = "Ollama closed the pull before it finished";

/// A line that is not the JSON object Ollama sends.
pub const MALFORMED: &str = "malformed pull reply";

/// A line over [`SERVER_LINE_CAP`].
pub const TOO_LONG: &str = "pull line over the size limit";

/// What a piece of the reply says.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Pulled {
    /// The bytes pulled so far and the bytes to pull, over every digest
    /// seen.
    Progress { done: u64, total: u64 },
    /// `{"status":"success"}`: the model is in Ollama's store.
    Done,
    /// The pull failed, with this reason: one clean line of at most
    /// [`REASON_CAP`] characters.
    Failed(String),
}

/// Reads a pull reply. After its [`Pulled::Done`] or [`Pulled::Failed`]
/// it reads nothing more.
pub struct PullDecoder {
    lines: Lines,
    /// `(digest, total, completed)`, in the order seen.
    digests: Vec<(String, u64, u64)>,
    ended: bool,
}

impl Default for PullDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl PullDecoder {
    pub fn new() -> Self {
        Self {
            lines: Lines::new(SERVER_LINE_CAP),
            digests: Vec::new(),
            ended: false,
        }
    }

    /// The events of `bytes`, up to and including the one that ends it.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Pulled> {
        let mut out = Vec::new();
        if self.ended {
            return out;
        }
        for line in self.lines.feed(bytes) {
            let event = match line {
                Ok(line) => self.line(&line),
                Err(_) => Some(Pulled::Failed(TOO_LONG.to_owned())),
            };
            if self.push(&mut out, event) {
                break;
            }
        }
        out
    }

    /// The reply ended: its last line, and a failure when it never said
    /// how the pull went.
    pub fn finish(&mut self) -> Vec<Pulled> {
        let mut out = Vec::new();
        if self.ended {
            return out;
        }
        let event = match self.lines.rest() {
            Ok(line) => self.line(&line),
            Err(_) => Some(Pulled::Failed(TOO_LONG.to_owned())),
        };
        if !self.push(&mut out, event) {
            self.ended = true;
            out.push(Pulled::Failed(CLOSED.to_owned()));
        }
        out
    }

    /// Adds `event` to `out`; true when it ends the pull.
    fn push(&mut self, out: &mut Vec<Pulled>, event: Option<Pulled>) -> bool {
        let Some(event) = event else {
            return false;
        };
        let ends = !matches!(event, Pulled::Progress { .. });
        self.ended |= ends;
        out.push(event);
        ends
    }

    /// The event of one line; `None` for a blank line or one that changes
    /// nothing shown.
    fn line(&mut self, line: &[u8]) -> Option<Pulled> {
        if line.iter().all(u8::is_ascii_whitespace) {
            return None;
        }
        let Ok(Value::Object(object)) = serde_json::from_slice::<Value>(line) else {
            return Some(Pulled::Failed(MALFORMED.to_owned()));
        };
        if let Some(reason) = object.get("error").and_then(error_text) {
            return Some(Pulled::Failed(reason));
        }
        if object.get("status").and_then(Value::as_str) == Some("success") {
            return Some(Pulled::Done);
        }
        let digest = object.get("digest").and_then(Value::as_str)?;
        let total = object.get("total").and_then(Value::as_u64)?;
        let completed = object
            .get("completed")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(total);
        match self.digests.iter_mut().find(|(d, ..)| d == digest) {
            Some(entry) if (entry.1, entry.2) == (total, completed) => return None,
            Some(entry) => (entry.1, entry.2) = (total, completed),
            None => self.digests.push((digest.to_owned(), total, completed)),
        }
        let (done, total) = self.digests.iter().fold((0u64, 0u64), |(d, t), e| {
            (d.saturating_add(e.2), t.saturating_add(e.1))
        });
        Some(Pulled::Progress { done, total })
    }
}

/// The reason of a reply that is not a 200: its `error`, else its text,
/// after the status.
pub fn status_reason(status: u16, body: &[u8]) -> String {
    let error = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| value.get("error").and_then(error_text));
    let text = error.unwrap_or_else(|| flat(&String::from_utf8_lossy(body)));
    if text.is_empty() {
        format!("HTTP {status}")
    } else {
        capped(&format!("HTTP {status}: {text}"))
    }
}

/// A non-empty `error` string, as one clean capped line.
fn error_text(value: &Value) -> Option<String> {
    let text = flat(value.as_str()?);
    (!text.is_empty()).then(|| capped(&text))
}

/// `text` sanitized onto one line: `clean` keeps `\n` and `\t`, which would
/// let it fake extra lines.
fn flat(text: &str) -> String {
    clean(text).split_whitespace().collect::<Vec<_>>().join(" ")
}

fn capped(text: &str) -> String {
    text.chars()
        .take(REASON_CAP)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

#[cfg(test)]
mod tests;
