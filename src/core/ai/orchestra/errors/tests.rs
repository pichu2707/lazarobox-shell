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
fn synthetic_429_fixtures_classify_per_adapter() {
    // Synthetic fixtures (see tests/fixtures/orchestra/README.md). Gemini's
    // 429 words an empty daily quota, which is a Quota, not a burst.
    let cases = [
        (
            "ollama",
            fixture!("orchestra/ollama-429.ndjson"),
            ErrorClass::RateLimit,
        ),
        (
            "claude",
            fixture!("orchestra/claude-429.jsonl"),
            ErrorClass::RateLimit,
        ),
        (
            "codex",
            fixture!("orchestra/codex-429.jsonl"),
            ErrorClass::RateLimit,
        ),
        (
            "gemini",
            fixture!("orchestra/gemini-429.jsonl"),
            ErrorClass::Quota,
        ),
    ];
    for (engine, text, class) in cases {
        let got = classify(engine, &decoded(engine, text));
        assert_eq!(got.class, class, "{engine}: {}", got.message);
        assert!(got.message.contains("429") || got.message.contains("Rate limit"));
    }
}

#[test]
fn a_plain_429_on_gemini_is_a_rate_limit() {
    let text = ChatError::Failed("[API Error: status 429 Too Many Requests]".into());
    assert_eq!(classify("gemini", &text).class, ErrorClass::RateLimit);
}

#[test]
fn synthetic_409_fixtures_give_conflict_for_every_adapter() {
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

fn class_of_text(text: &str) -> ErrorClass {
    classify("codex", &ChatError::Failed(text.into())).class
}

#[test]
fn a_bare_number_is_not_a_status_code() {
    for text in [
        "no such file /data/429/x",
        "bad key sk-ab429cd",
        "took 1.429s",
        "panic at src/main.rs:403:5",
        "timed out after 401 ms",
        "wrote 409 bytes",
    ] {
        assert_ne!(class_of_text(text), ErrorClass::RateLimit, "{text}");
        assert_ne!(class_of_text(text), ErrorClass::Auth, "{text}");
        assert_ne!(class_of_text(text), ErrorClass::Conflict, "{text}");
    }
    assert_eq!(class_of_text("timed out after 401 ms"), ErrorClass::Timeout);
    assert_eq!(class_of_text("took 1.429s"), ErrorClass::Other);
}

#[test]
fn a_status_code_in_http_context_still_counts() {
    for (text, class) in [
        ("status 429", ErrorClass::RateLimit),
        ("HTTP 429.", ErrorClass::RateLimit),
        ("API Error: 401 nope", ErrorClass::Auth),
        ("{\"status\":409,\"x\":1}", ErrorClass::Conflict),
        ("code: 403", ErrorClass::Auth),
        ("429 Too Many Requests: slow", ErrorClass::RateLimit),
        ("409 Conflict: busy", ErrorClass::Conflict),
        ("401 Unauthorized", ErrorClass::Auth),
    ] {
        assert_eq!(class_of_text(text), class, "{text}");
    }
}

#[test]
fn gemini_daily_quota_wording_on_a_429_is_a_quota() {
    for text in [
        "Resource has been exhausted (e.g. check quota). status 429",
        "status 429: you exceeded your current quota",
        "429 Too Many Requests: check quota",
    ] {
        assert_eq!(class_of_text(text), ErrorClass::Quota, "{text}");
    }
}

#[test]
fn unrelated_conflict_and_permission_text_is_other() {
    for text in [
        "merge conflict in src/a.rs",
        "permission denied: /tmp/x",
        "file is forbidden to read",
    ] {
        assert_eq!(class_of_text(text), ErrorClass::Other, "{text}");
    }
    assert_eq!(class_of_text("conflict error: busy"), ErrorClass::Conflict);
    assert_eq!(class_of_text("billing issue"), ErrorClass::Quota);
}

#[test]
fn a_rejected_long_window_from_the_claude_decoder_is_a_quota() {
    for window in ["five_hour", "seven_day", "seven_day_opus", "weekly"] {
        let event = format!(
            "{{\"type\":\"rate_limit_event\",\"rate_limit_info\":{{\"status\":\"rejected\",\"rateLimitType\":\"{window}\"}}}}\n"
        );
        for reason in ["Claude AI usage limit reached", "boom"] {
            let result = format!(
                "{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":true,\"result\":\"{reason}\"}}\n"
            );
            let got = classify("claude", &decoded("claude", &format!("{event}{result}")));
            assert_eq!(got.class, ErrorClass::Quota, "{window}: {}", got.message);
        }
    }
}

#[test]
fn a_rejected_short_window_is_a_rate_limit_not_a_quota() {
    for window in ["one_minute", "one_hour", "ten_second"] {
        let text = format!(
            "{{\"type\":\"rate_limit_event\",\"rate_limit_info\":{{\"status\":\"rejected\",\"rateLimitType\":\"{window}\"}}}}\n\
             {{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":true,\"result\":\"boom\"}}\n"
        );
        assert_eq!(class_of("claude", &text), ErrorClass::RateLimit, "{window}");
    }
}

#[test]
fn a_rejected_window_survives_a_stream_that_ends_without_a_result() {
    let text = "{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"status\":\"rejected\",\"rateLimitType\":\"five_hour\"}}\n";
    assert_eq!(class_of("claude", text), ErrorClass::Quota);
}

// A window mark does not pre-empt explicit evidence of another class.
#[test]
fn a_window_mark_beside_an_auth_status_is_auth() {
    let text = "API Error: 401 Invalid API key (limit window: one_minute)";
    assert_eq!(class_of_text(text), ErrorClass::Auth);
    let bare = "rejected (limit window: one_minute)";
    assert_eq!(class_of_text(bare), ErrorClass::RateLimit);
}
