//! The wording rules the macOS real-codex test applies to a turn's outcome.
//! They are plain string matching, so they run on every system.

#[path = "support/codex_outcome.rs"]
mod codex_outcome;

use codex_outcome::{Outcome, classify};

/// The error text codex 0.153.4 produced through the jail on the CI runners.
const REAL_401: &str = "unexpected status 401 Unauthorized: Incorrect API key provided: sk-laz*****-key. You can find your API key at https://platform.openai.com/account/api-keys., url: https://api.openai.com/v1/responses";

#[test]
fn the_real_401_event_text_is_an_authentication_failure() {
    assert_eq!(classify(REAL_401), Outcome::Auth);
    assert_eq!(classify("Incorrect API key provided"), Outcome::Auth);
}

#[test]
fn loose_words_are_not_an_authentication_failure() {
    for text in [
        "please login",
        "not logged in",
        "authentication required",
        "check your api key",
        "error 401",
        "unauthorized",
    ] {
        assert_eq!(classify(text), Outcome::Other, "{text}");
    }
}

#[test]
fn a_broken_jail_is_named_by_its_marker() {
    for marker in [
        "UnknownIssuer",
        "invalid peer certificate: UnknownIssuer",
        "Operation not permitted",
        "sandbox_apply: Operation not permitted",
        "the macOS sandbox could not be applied",
        "Connection refused",
        "Network is unreachable",
        "dns error: failed to lookup address information",
    ] {
        assert!(
            matches!(classify(marker), Outcome::Broken(_)),
            "{marker} should be Broken"
        );
    }
}

#[test]
fn a_broken_marker_wins_over_a_401() {
    let text = format!("{REAL_401} invalid peer certificate: UnknownIssuer");
    assert!(matches!(classify(&text), Outcome::Broken(_)));
}

#[test]
fn words_that_only_look_like_failures_are_not_broken() {
    for text in [
        "tls handshake ok",
        "dns",
        "could not resolve the model",
        "certificate pinned",
        "sandbox-exec ran",
    ] {
        assert_eq!(classify(text), Outcome::Other, "{text}");
    }
}

#[test]
fn an_external_service_answer_is_a_skip() {
    for text in [
        "unexpected status 403 Forbidden: <html>Just a moment...</html>",
        "unexpected status 429 Too Many Requests: rate limited",
        "{\"error\":{\"code\":\"unsupported_country_region_territory\"}}",
        "Attention Required! | Cloudflare",
    ] {
        assert!(
            matches!(classify(text), Outcome::External(_)),
            "{text} should be External"
        );
    }
}
