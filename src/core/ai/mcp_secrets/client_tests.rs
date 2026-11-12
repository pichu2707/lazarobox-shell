use super::*;

const ID: &str = "123-abc.apps.googleusercontent.com";

#[test]
fn the_json_is_the_desktop_client_shape() {
    let text = client_json(ID, "GOCSPX-abc").unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"installed": {
            "client_id": ID,
            "client_secret": "GOCSPX-abc",
            "auth_uri": "https://accounts.google.com/o/oauth2/auth",
            "token_uri": "https://oauth2.googleapis.com/token",
            "redirect_uris": ["http://localhost"],
        }})
    );
    assert!(text.ends_with('\n'));
}

#[test]
fn the_serializer_escapes_quotes_and_backslashes() {
    let text = client_json(ID, r#"a"b\c"#).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["installed"]["client_secret"], r#"a"b\c"#);
}

#[test]
fn an_id_must_end_in_the_google_suffix_and_be_printable_ascii() {
    for bad in [
        "abc",
        "123-abc.apps.googleusercontent.com.evil",
        "12 3.apps.googleusercontent.com",
        "é.apps.googleusercontent.com",
        "1\n2.apps.googleusercontent.com",
        ".apps.googleusercontent.com",
    ] {
        assert_eq!(client_json(bad, "s"), Err(Refusal::BadClientId), "{bad:?}");
    }
}

#[test]
fn a_secret_must_be_one_line_of_printable_ascii() {
    for bad in ["", "a b", "é", "a\nb", "a\tb", "a\u{7f}"] {
        assert_eq!(
            client_json(ID, bad),
            Err(Refusal::BadClientSecret),
            "{bad:?}"
        );
    }
}

#[test]
fn the_notices_name_the_suffix_and_never_a_value() {
    assert!(
        Refusal::BadClientId
            .notice()
            .contains(".apps.googleusercontent.com")
    );
    assert!(!Refusal::BadClientSecret.notice().contains("secrets.toml"));
    assert_eq!(
        Refusal::NoFolder.notice_of(CLIENT_FILE),
        "google-oauth-client.json: no config folder"
    );
    assert!(
        Refusal::BackupUnsafe
            .notice_of(CLIENT_FILE)
            .starts_with("google-oauth-client.json.bak ")
    );
}
