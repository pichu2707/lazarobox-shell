use super::*;

#[test]
fn key_value_pairs_with_a_sensitive_name_lose_the_value() {
    assert_eq!(scrub("PASSWORD=abc123 run"), "PASSWORD=*** run");
    assert_eq!(scrub("--password=hunter2"), "--password=***");
    assert_eq!(
        scrub("API_KEY=k1 DB_SECRET=s2"),
        "API_KEY=*** DB_SECRET=***"
    );
}

#[test]
fn a_flag_with_a_sensitive_name_loses_the_next_word() {
    assert_eq!(
        scrub("login --password hunter2 now"),
        "login --password *** now"
    );
    assert_eq!(
        scrub("cmd --token abc --verbose"),
        "cmd --token *** --verbose"
    );
    assert_eq!(scrub("cmd --token --verbose"), "cmd --token --verbose");
}

#[test]
fn url_query_parameters_are_scrubbed() {
    assert_eq!(
        scrub("curl https://h/x?token=short&page=2"),
        "curl https://h/x?token=***&page=2"
    );
}

#[test]
fn a_header_with_a_sensitive_name_loses_its_value() {
    assert_eq!(
        scrub("curl -H \"X-Api-Key: abcd\" url"),
        "curl -H \"X-Api-Key: ***\" url"
    );
    assert_eq!(
        scrub("Authorization: Bearer abc.def x"),
        "Authorization: *** *** x"
    );
}

#[test]
fn mysql_attached_password_is_scrubbed() {
    assert_eq!(
        scrub("mysql -u root -pSecret12 db"),
        "mysql -u root -p*** db"
    );
    assert_eq!(scrub("ls -pa"), "ls -pa");
}

#[test]
fn ordinary_arguments_stay_untouched() {
    for text in [
        "read_file src/main.rs",
        "cargo test --lib -- --nocapture",
        "grep path=/a/b limit=5",
        "git commit -m \"fix: handle tokens\"",
    ] {
        assert_eq!(scrub(text), text);
    }
}

#[test]
fn json_values_of_secret_names_are_masked_and_the_braces_stay() {
    assert_eq!(
        scrub(r#"{"password":"abc def","n":1}"#),
        r#"{"password":"***","n":1}"#
    );
    assert_eq!(scrub(r#"{"api_key": "k1"}"#), r#"{"api_key": "***"}"#);
    assert_eq!(scrub(r#"{"token":12345}"#), r#"{"token":***}"#);
    assert_eq!(scrub(r#"{"name":"bob"}"#), r#"{"name":"bob"}"#);
}

#[test]
fn curl_user_sshpass_and_a_header_without_space_are_masked() {
    assert_eq!(scrub("curl -u bob:s3cret url"), "curl -u bob:*** url");
    assert_eq!(
        scrub("sshpass -p hunter2 ssh host"),
        "sshpass -p *** ssh host"
    );
    assert_eq!(
        scrub("Authorization:Bearer abc.def"),
        "Authorization:*** ***"
    );
    assert_eq!(scrub("Authorization:abc"), "Authorization:***");
}

#[test]
fn a_quoted_value_with_spaces_is_masked_whole() {
    assert_eq!(scrub("API_KEY='my secret value' run"), "API_KEY=*** run");
    assert_eq!(
        scrub("cmd --password 'two words' x"),
        "cmd --password *** x"
    );
    assert_eq!(
        scrub("export PASSWORD=\"a b c\" && ls"),
        "export PASSWORD=*** && ls"
    );
}

#[test]
fn only_whole_name_segments_count() {
    for name in [
        "api_key",
        "x-api-key",
        "db.password",
        "ACCESS_TOKEN",
        "apikey",
    ] {
        assert_eq!(scrub(&format!("{name}=v")), format!("{name}=***"), "{name}");
    }
    for name in ["keyboard", "monkey", "author", "passthrough", "tokenizer"] {
        assert_eq!(scrub(&format!("{name}=v")), format!("{name}=v"), "{name}");
    }
}

#[test]
fn the_mysql_flag_ends_with_its_command() {
    assert_eq!(
        scrub("mysql -pSecret12 db && ls -pa"),
        "mysql -p*** db && ls -pa"
    );
    assert_eq!(scrub("mysql db; ls -pa"), "mysql db; ls -pa");
    assert_eq!(scrub("mysql -p db"), "mysql -p db");
}

#[test]
fn an_unbalanced_quote_never_duplicates_or_leaks_text() {
    // A clipped command: an odd quote count, no secret in it.
    let clipped = "exec git commit -m \"fix: handle the case where";
    assert_eq!(scrub(clipped), clipped);
    assert_eq!(scrub("a \" b \" c \""), "a \" b \" c \"");
    // A stray quote before JSON does not flip name and value.
    assert_eq!(
        scrub("5\" disk {\"password\":\"abc\"}"),
        "5\" disk {\"password\":\"***\"}"
    );
    assert_eq!(
        scrub("say \"hi {\"token\": \"t\"}"),
        "say \"hi {\"token\": \"***\"}"
    );
}

#[test]
fn a_json_array_value_is_masked_whole() {
    assert_eq!(
        scrub(r#"{"token": ["abc","def"], "n": 1}"#),
        r#"{"token": ***, "n": 1}"#
    );
}

#[test]
fn text_without_a_secret_comes_out_as_it_went_in() {
    for text in [
        "",
        "\"",
        "a:b \"c\": d",
        "{\"name\": \"bob\", \"n\": [1, 2]}",
        "url http://host:80/x?a=b&c=d",
        "it's a \"quote",
    ] {
        assert_eq!(scrub(text), text);
    }
}

#[test]
fn a_flag_that_names_no_value_does_not_take_the_next_word() {
    assert_eq!(
        scrub("docker login --password-stdin host"),
        "docker login --password-stdin host"
    );
    assert_eq!(scrub("cmd --no-auth host"), "cmd --no-auth host");
    assert_eq!(scrub("cmd --key-file a.pem"), "cmd --key-file a.pem");
}
