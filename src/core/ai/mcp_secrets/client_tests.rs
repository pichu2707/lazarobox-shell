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

#[test]
fn a_saved_client_file_reads_back_its_id_and_secret() {
    let client = read_client(&client_json(ID, "GOCSPX-abc").unwrap()).unwrap();
    assert_eq!(client.id, ID);
    assert_eq!(client.secret.expose(), "GOCSPX-abc");
}

#[test]
fn the_file_google_downloads_reads_too() {
    let text = format!(
        r#"{{"installed":{{"client_id":"{ID}","project_id":"demo-123","auth_uri":"https://accounts.google.com/o/oauth2/auth","token_uri":"https://oauth2.googleapis.com/token","auth_provider_x509_cert_url":"https://www.googleapis.com/oauth2/v1/certs","client_secret":"GOCSPX-xyz","redirect_uris":["http://localhost"]}}}}"#
    );
    let client = read_client(&text).unwrap();
    assert_eq!(client.id, ID);
    assert_eq!(client.secret.expose(), "GOCSPX-xyz");
}

#[test]
fn a_file_that_is_not_a_desktop_client_is_refused() {
    let web = format!(r#"{{"web":{{"client_id":"{ID}","client_secret":"s"}}}}"#);
    let no_secret = format!(r#"{{"installed":{{"client_id":"{ID}"}}}}"#);
    let number = format!(r#"{{"installed":{{"client_id":"{ID}","client_secret":7}}}}"#);
    for text in ["", "not json", "[]", &web, &no_secret, &number] {
        assert!(
            matches!(read_client(text), Err(Refusal::BadClientFile)),
            "{text:?}"
        );
    }
}

#[test]
fn a_file_with_a_bad_id_or_secret_is_refused_as_the_form_would() {
    let id = r#"{"installed":{"client_id":"123.example.com","client_secret":"s"}}"#;
    let secret = format!(r#"{{"installed":{{"client_id":"{ID}","client_secret":"a b"}}}}"#);
    assert!(matches!(read_client(id), Err(Refusal::BadClientId)));
    assert!(matches!(
        read_client(&secret),
        Err(Refusal::BadClientSecret)
    ));
}

#[test]
fn an_oversized_client_file_is_refused() {
    let padding = " ".repeat(CLIENT_MAX);
    let text = format!("{}{padding}", client_json(ID, "s").unwrap());
    assert!(matches!(read_client(&text), Err(Refusal::BadClientFile)));
}

#[test]
fn the_debug_of_a_client_never_shows_the_secret() {
    let client = read_client(&client_json(ID, "GOCSPX-hidden").unwrap()).unwrap();
    let debug = format!("{client:?}");
    assert!(debug.contains(ID), "{debug}");
    assert!(!debug.contains("GOCSPX-hidden"), "{debug}");
}

#[test]
fn a_bad_client_file_notice_names_the_form() {
    assert_eq!(
        Refusal::BadClientFile.notice(),
        "google-oauth-client.json is not a Desktop OAuth client; save it again (o)"
    );
}
