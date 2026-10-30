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
    "billing",
];
const RATE_LIMIT: &[&str] = &["rate limit", "rate_limit", "too many requests"];
const CONFLICT: &[&str] = &["conflict"];
const AUTH: &[&str] = &[
    "unauthorized",
    "unauthenticated",
    "forbidden",
    "permission denied",
    "invalid api key",
    "invalid x-api-key",
    "api key not valid",
    "not logged in",
    "authentication",
    "/login",
];
const QUOTA: &[&str] = &["quota", "usage limit"];
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
    if any(QUOTA_STRONG) {
        ErrorClass::Quota
    } else if has_code(&lower, "429") || any(RATE_LIMIT) {
        ErrorClass::RateLimit
    } else if has_code(&lower, "409") || any(CONFLICT) {
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

/// Whether `code` stands alone in `text`: not part of a longer number.
fn has_code(text: &str, code: &str) -> bool {
    text.match_indices(code).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + code.len()..].chars().next();
        !before.is_some_and(|c| c.is_ascii_digit()) && !after.is_some_and(|c| c.is_ascii_digit())
    })
}

#[cfg(test)]
mod tests;
