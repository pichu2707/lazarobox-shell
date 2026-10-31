//! Failure classification (agent-orchestrator D10): a pure text classifier
//! over the `ChatError` a decoder reports. The adapters already flatten a
//! provider error to one sanitized line, so the wording is what is matched;
//! `tests/fixtures/` pins it per adapter and each CLI bump re-checks it.

use super::worker::ErrorClass;
use crate::core::ai::chat::engine::ChatError;
use crate::core::ai::sanitize::redact_tokens;

/// The text shown for a stream that ended without a final event.
const INTERRUPTED: &str = "the engine's stream ended without a result";

/// A failure with its class. The message is the provider's, with any token
/// redacted.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Classified {
    pub class: ErrorClass,
    pub message: String,
}

/// Wording of an exhausted allowance, checked before a 429: some providers
/// answer 429 for both a burst and an empty account.
const QUOTA_STRONG: &[&str] = &[
    "insufficient_quota",
    "exhausted your",
    "credit balance",
    "check quota",
    "exceeded your current quota",
    "resource has been exhausted",
];
const RATE_LIMIT: &[&str] = &["rate limit", "rate_limit", "too many requests"];
const CONFLICT: &[&str] = &["conflict error", "conflict_error"];
const AUTH: &[&str] = &[
    "unauthorized",
    "unauthenticated",
    "invalid api key",
    "invalid x-api-key",
    "api key not valid",
    "not logged in",
    "authentication",
    "/login",
];
const QUOTA: &[&str] = &["quota", "usage limit", "billing"];
/// Words before a status code that make it an HTTP status.
const CODE_CONTEXT: &[&str] = &["status", "http", "error", "code"];
/// Reason phrases after a status code.
const REASONS: &[&str] = &["too many requests", "unauthorized", "forbidden", "conflict"];
/// Marks the window the claude decoder appends for a rejected limit.
const WINDOW_MARK: &str = "limit window: ";
const TIMEOUT: &[&str] = &["timed out", "timeout", "deadline exceeded"];

/// Classifies a failure of `engine`'s worker turn. The wording is matched
/// across adapters (each flattens to the provider's own text), so `engine`
/// does not change the table today; it is kept for a wording that proves
/// adapter-specific.
pub fn classify(_engine: &str, error: &ChatError) -> Classified {
    let (class, text) = match error {
        ChatError::Jail(reason) => (ErrorClass::JailRefusal, reason.as_str()),
        ChatError::Interrupted | ChatError::Truncated => (ErrorClass::Other, INTERRUPTED),
        ChatError::Failed(text) => (class_of(text), text.as_str()),
    };
    Classified {
        class,
        message: redact_tokens(text),
    }
}

fn class_of(text: &str) -> ErrorClass {
    let lower = text.to_ascii_lowercase();
    let any = |words: &[&str]| words.iter().any(|word| lower.contains(word));
    if any(QUOTA_STRONG) || long_window(&lower) {
        ErrorClass::Quota
    } else if has_code(&lower, "429") || any(RATE_LIMIT) {
        ErrorClass::RateLimit
    } else if has_code(&lower, "409") || any(CONFLICT) || lower.starts_with("conflict:") {
        ErrorClass::Conflict
    } else if has_code(&lower, "401") || has_code(&lower, "403") || any(AUTH) {
        ErrorClass::Auth
    } else if any(QUOTA) {
        ErrorClass::Quota
    } else if any(TIMEOUT) {
        ErrorClass::Timeout
    } else {
        ErrorClass::Other
    }
}

/// Whether the claude decoder's `(limit window: X)` names a window of hours
/// or days: a spent allowance, not a burst.
fn long_window(text: &str) -> bool {
    text.split_once(WINDOW_MARK).is_some_and(|(_, rest)| {
        let window = rest.split(')').next().unwrap_or("");
        window == "five_hour" || window.contains("day") || window.contains("week")
    })
}

/// Whether `code` is an HTTP status in `text`: it stands alone (no
/// neighbour that makes it a path, a version, a key or a longer number) and
/// opens the text, follows `status`/`HTTP`/`error`/`code` or precedes a reason phrase.
fn has_code(text: &str, code: &str) -> bool {
    let glued = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '/' | '.' | '-');
    text.match_indices(code).any(|(at, _)| {
        let head = &text[..at];
        let tail = &text[at + code.len()..];
        let mut after = tail.chars();
        let ends_ok = match after.next() {
            Some(c) if glued(c) && !matches!(c, '.' | '-') => false,
            Some('.' | '-' | ':') => !after.next().is_some_and(|c| c.is_ascii_alphanumeric()),
            _ => true,
        };
        let starts_ok = !head.chars().next_back().is_some_and(glued);
        let before = head.trim_end_matches(|c: char| c == ':' || c == '"' || c.is_whitespace());
        // A message that opens with the code is a status line.
        let named = before.is_empty() || CODE_CONTEXT.iter().any(|word| before.ends_with(word));
        let reason = REASONS.iter().any(|r| tail.trim_start().starts_with(r));
        ends_ok && starts_ok && (named || reason)
    })
}

#[cfg(test)]
mod tests;
