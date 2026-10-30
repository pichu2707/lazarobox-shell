use super::*;
use crate::core::ai::chat::claude::ClaudeDecoder;
use crate::core::ai::chat::codex::CodexDecoder;
use crate::core::ai::chat::engine::StreamEvent;
use crate::core::ai::chat::gemini::GeminiDecoder;
use crate::core::ai::chat::stream::{NdjsonSplitter, SERVER_LINE_CAP, StreamDecoder};

/// The failure a decoder reports for a whole fixture.
fn failure(mut decoder: impl StreamDecoder, fixture: &str) -> ChatError {
    let mut events = decoder.feed(fixture.as_bytes());
    events.extend(decoder.finish());
    events
        .into_iter()
        .find_map(|event| match event {
            StreamEvent::Error(error) => Some(error),
            _ => None,
        })
        .expect("the fixture fails the turn")
}

/// Decodes `fixture` with the adapter of `engine`.
fn decoded(engine: &str, fixture: &str) -> ChatError {
    match engine {
        "claude" => failure(ClaudeDecoder::new(), fixture),
        "codex" => failure(CodexDecoder::new(), fixture),
        "gemini" => failure(GeminiDecoder::new(), fixture),
        _ => failure(NdjsonSplitter::new(SERVER_LINE_CAP), fixture),
    }
}

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!("../../../../../tests/fixtures/", $name))
    };
}

fn class_of(engine: &str, fixture: &str) -> ErrorClass {
    classify(engine, &decoded(engine, fixture)).class
}

#[test]
fn every_adapter_gives_rate_limit_for_a_429() {
    let cases = [
        ("ollama", fixture!("orchestra/ollama-429.ndjson")),
        ("claude", fixture!("orchestra/claude-429.jsonl")),
        ("codex", fixture!("orchestra/codex-429.jsonl")),
        ("gemini", fixture!("orchestra/gemini-429.jsonl")),
    ];
    for (engine, text) in cases {
        let got = classify(engine, &decoded(engine, text));
        assert_eq!(
            got.class,
            ErrorClass::RateLimit,
            "{engine}: {}",
            got.message
        );
        assert!(got.message.contains("429") || got.message.contains("Rate limit"));
    }
}

#[test]
fn every_adapter_gives_conflict_for_a_409() {
    let cases = [
        ("ollama", fixture!("orchestra/ollama-409.ndjson")),
        ("claude", fixture!("orchestra/claude-409.jsonl")),
        ("codex", fixture!("orchestra/codex-409.jsonl")),
        ("gemini", fixture!("orchestra/gemini-409.jsonl")),
    ];
    for (engine, text) in cases {
        let got = classify(engine, &decoded(engine, text));
        assert_eq!(got.class, ErrorClass::Conflict, "{engine}: {}", got.message);
    }
}

#[test]
fn the_provider_message_is_kept_beside_the_class() {
    let text = fixture!("orchestra/claude-429.jsonl");
    let got = classify("claude", &decoded("claude", text));
    assert!(
        got.message.contains("organization's rate limit"),
        "{}",
        got.message
    );
}

#[test]
fn recorded_gemini_failures_classify_from_the_real_wording() {
    let quota = fixture!("gemini/quota-error.jsonl");
    let model = fixture!("gemini/model-error.jsonl");
    assert_eq!(class_of("gemini", quota), ErrorClass::Quota);
    assert_eq!(class_of("gemini", model), ErrorClass::Other);
}

#[test]
fn recorded_model_errors_of_the_other_cli_are_other() {
    assert_eq!(
        class_of("claude", fixture!("claude/model-error.jsonl")),
        ErrorClass::Other
    );
    assert_eq!(
        class_of("codex", fixture!("codex/model-error.jsonl")),
        ErrorClass::Other
    );
}

#[test]
fn the_class_table_covers_auth_quota_timeout_jail_and_other() {
    let failed = |text: &str| ChatError::Failed(text.to_owned());
    let cases = [
        (failed("API Error: 401 Invalid API key"), ErrorClass::Auth),
        (failed("Not logged in. Please run /login"), ErrorClass::Auth),
        (failed("403 Forbidden: permission denied"), ErrorClass::Auth),
        (
            failed("insufficient_quota: you exceeded your current quota"),
            ErrorClass::Quota,
        ),
        (failed("Your credit balance is too low"), ErrorClass::Quota),
        (
            failed("the request timed out after 60s"),
            ErrorClass::Timeout,
        ),
        (
            ChatError::Jail("the sandbox exited with 71".into()),
            ErrorClass::JailRefusal,
        ),
        (failed("something odd happened"), ErrorClass::Other),
        (ChatError::Interrupted, ErrorClass::Other),
    ];
    for (error, class) in cases {
        assert_eq!(classify("codex", &error).class, class, "{error:?}");
    }
}

#[test]
fn a_status_code_needs_its_own_digits() {
    let got = classify("ollama", &ChatError::Failed("request 14290 failed".into()));
    assert_eq!(got.class, ErrorClass::Other);
    let got = classify("ollama", &ChatError::Failed("HTTP 429.".into()));
    assert_eq!(got.class, ErrorClass::RateLimit);
}

#[test]
fn a_secret_in_the_message_is_redacted_and_the_class_survives() {
    let token = "sk-abcdefghijklmnopqrstuvwxyz0123456789";
    let text = format!("401 unauthorized for key {token}");
    let got = classify("claude", &ChatError::Failed(text));
    assert_eq!(got.class, ErrorClass::Auth);
    assert!(!got.message.contains(token), "{}", got.message);
    assert!(got.message.contains("401"));
    let jail = classify("codex", &ChatError::Jail(format!("bad {token}")));
    assert!(!jail.message.contains(token), "{}", jail.message);
}
