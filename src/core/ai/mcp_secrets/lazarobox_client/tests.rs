use super::*;
use crate::core::ai::mcp_secrets::{client_json, read_client};

const PIN: &str = "123456789012";
const ID: &str = "123456789012-abc.apps.googleusercontent.com";

// Spec (lazarobox-google-client, Integrity pin).
#[test]
fn an_id_of_the_pinned_project_is_accepted() {
    assert!(pinned(ID, Some(PIN)));
}

#[test]
fn a_foreign_project_is_refused() {
    assert!(!pinned(
        "999999999999-abc.apps.googleusercontent.com",
        Some(PIN)
    ));
}

#[test]
fn a_longer_number_that_starts_with_the_pin_is_refused() {
    assert!(!pinned(
        "1234567890123-abc.apps.googleusercontent.com",
        Some(PIN)
    ));
}

#[test]
fn an_id_that_is_not_a_client_id_is_refused_even_with_the_pin() {
    assert!(!pinned("123456789012-abc", Some(PIN)));
    assert!(!pinned(
        "123456789012-a bc.apps.googleusercontent.com",
        Some(PIN)
    ));
}

#[test]
fn an_id_with_nothing_between_the_pin_and_the_suffix_is_refused() {
    assert!(!pinned(
        "123456789012-.apps.googleusercontent.com",
        Some(PIN)
    ));
}

#[test]
fn an_empty_pin_is_not_configured() {
    let ok = body(ID, "s");
    assert_eq!(accept(200, &ok, Some("")).unwrap_err(), Why::NotConfigured);
}

#[test]
fn without_a_pin_nothing_is_accepted() {
    assert!(!pinned(ID, None));
    assert!(!pinned(ID, Some("")));
}

#[test]
fn the_pin_constant_is_digits_only() {
    if let Some(pin) = PROJECT_NUMBER {
        assert!(!pin.is_empty() && pin.bytes().all(|b| b.is_ascii_digit()));
    }
}

fn body(id: &str, secret: &str) -> String {
    serde_json::json!({"installed": {
        "client_id": id,
        "client_secret": secret,
        "auth_uri": "https://accounts.google.com/o/oauth2/auth",
        "token_uri": "https://oauth2.googleapis.com/token",
        "redirect_uris": ["http://localhost"],
    }})
    .to_string()
}

fn why(status: u16, text: &str) -> Why {
    accept(status, text, Some(PIN)).unwrap_err()
}

// Spec (Size and shape): a valid Desktop body of the pinned project.
#[test]
fn a_200_with_a_pinned_desktop_body_is_the_client() {
    let client = accept(200, &body(ID, "GOCSPX-s"), Some(PIN)).unwrap();
    assert_eq!(client.id, ID);
    assert_eq!(client.secret.expose(), "GOCSPX-s");
}

// Spec (Request shape): a redirect is a failure, never followed.
#[test]
fn other_statuses_are_failures_and_redirects_are_named() {
    let ok = body(ID, "s");
    assert_eq!(why(301, &ok), Why::Redirected(301));
    assert_eq!(why(302, &ok).text(), "redirected (HTTP 302)");
    assert_eq!(why(404, &ok).text(), "HTTP 404");
    assert_eq!(why(500, &ok), Why::Http(500));
    assert_eq!(why(204, &ok), Why::Http(204));
}

#[test]
fn a_body_over_16_kib_is_refused_without_parsing() {
    let big = format!("{}{}", body(ID, "s"), " ".repeat(16 * 1024));
    assert_eq!(why(200, &big), Why::TooLarge);
    assert_eq!(Why::TooLarge.text(), "larger than 16 KiB");
}

#[test]
fn a_body_that_is_not_a_desktop_client_is_refused() {
    let web = body(ID, "s").replace("installed", "web");
    let no_secret = r#"{"installed":{"client_id":"123456789012-a.apps.googleusercontent.com"}}"#;
    let bad_suffix = body("123456789012-abc.example.com", "s");
    let control = body(ID, "se\ncret");
    for text in [&web, no_secret, &bad_suffix, &control, "not json", "{}", ""] {
        assert_eq!(why(200, text), Why::NotDesktop, "{text:?}");
    }
    assert_eq!(Why::NotDesktop.text(), "not a Desktop client file");
}

#[test]
fn a_client_of_another_project_is_refused() {
    let other = body("999999999999-abc.apps.googleusercontent.com", "s");
    assert_eq!(why(200, &other), Why::Foreign);
    assert_eq!(Why::Foreign.text(), "not from the lazarobox project");
}

#[test]
fn without_a_pin_the_answer_is_not_configured_before_anything_else() {
    let ok = body(ID, "s");
    assert_eq!(accept(200, &ok, None).unwrap_err(), Why::NotConfigured);
    assert_eq!(accept(404, "", None).unwrap_err(), Why::NotConfigured);
}

// Design 3: the cache holds a rebuilt text, so a foreign `token_uri` of the
// fetched file never reaches the disk.
#[test]
fn the_cached_text_is_rebuilt_with_googles_endpoints() {
    let hostile = body(ID, "GOCSPX-s").replace(
        "https://oauth2.googleapis.com/token",
        "https://evil.example/token",
    );
    let client = accept(200, &hostile, Some(PIN)).unwrap();
    let text = client_json(&client.id, client.secret.expose()).unwrap();
    assert!(!text.contains("evil.example"), "{text}");
    assert!(text.contains("https://oauth2.googleapis.com/token"));
    // It reads back as the same client.
    let back = read_client(&text).unwrap();
    assert_eq!((back.id.as_str(), back.secret.expose()), (ID, "GOCSPX-s"));
}

#[test]
fn the_constants_are_the_design_names() {
    assert_eq!(CACHE_FILE, "lazarobox-google-client.json");
    assert_eq!(URL_PATH, "/lazarobox/google-client.json");
}

// Design, Notices: the exact texts.
#[test]
fn each_notice_is_the_design_text() {
    assert_eq!(
        failure_notice(&Why::NotConfigured),
        "own client: Esc, f on gsc, Enter, y, o if none, Esc, g · lazarobox: not configured in this build"
    );
    assert_eq!(
        failure_notice(&Why::Http(404)),
        "own client: Esc, f on gsc, Enter, y, o if none, Esc, g · lazarobox: not fetched (HTTP 404); link later"
    );
    assert_eq!(
        failure_notice(&Why::Redirected(301)),
        "own client: Esc, f on gsc, Enter, y, o if none, Esc, g · lazarobox: not fetched (redirected (HTTP 301)); link later"
    );
    assert_eq!(
        failure_notice(&Why::Curl(
            "javilazaro.es request failed: no reply".to_owned()
        )),
        "own client: Esc, f on gsc, Enter, y, o if none, Esc, g · lazarobox: not fetched (javilazaro.es request failed: no reply); link later"
    );
    assert_eq!(
        failure_notice(&Why::Foreign),
        "own client: Esc, f on gsc, Enter, y, o if none, Esc, g · lazarobox: refused (not from the lazarobox project); nothing saved"
    );
    assert_eq!(
        failure_notice(&Why::NotDesktop),
        "own client: Esc, f on gsc, Enter, y, o if none, Esc, g · lazarobox: refused (not a Desktop client file); nothing saved"
    );
    assert_eq!(
        failure_notice(&Why::TooLarge),
        "own client: Esc, f on gsc, Enter, y, o if none, Esc, g · lazarobox: refused (larger than 16 KiB); nothing saved"
    );
    assert_eq!(
        fallback_notice(&Why::Http(500)),
        "lazarobox client not refreshed (HTTP 500); linking with the saved copy"
    );
    assert_eq!(
        not_saved_notice("lazarobox-google-client.json: mode must be 0600 (chmod 600 /x)"),
        "lazarobox client not saved: lazarobox-google-client.json: mode must be 0600 (chmod 600 /x)"
    );
    assert_eq!(
        CLIENT_LINE,
        "client: lazarobox (fetched from www.javilazaro.es when you link)"
    );
}

#[test]
fn no_notice_holds_the_secret_or_the_body() {
    let secret = "GOCSPX-topsecret";
    let text = body(ID, secret);
    for status in [200, 301, 404] {
        if let Err(why) = accept(status, &text, Some("999")) {
            for notice in [failure_notice(&why), fallback_notice(&why)] {
                assert!(!notice.contains(secret) && !notice.contains("client_secret"));
            }
        }
    }
}

// Design 5: the cache sits next to `secrets.toml`, as an absolute path.
#[test]
fn the_cache_path_is_next_to_secrets_toml_and_absolute() {
    use std::path::Path;
    assert_eq!(
        cache_path(Some(Path::new("/h/.config/lazarobox/secrets.toml"))).as_deref(),
        Some("/h/.config/lazarobox/lazarobox-google-client.json")
    );
    assert_eq!(cache_path(Some(Path::new("secrets.toml"))), None);
    assert_eq!(cache_path(None), None);
}
