use super::*;

const FILE: &str =
    "# mine\n[ga4]\nGOOGLE_CLOUD_PROJECT = \"p1\" # mine\n\n[clarity]\nCLARITY_API_TOKEN = \"a\"\n";

fn ok(text: &str, server: &str, name: &str, value: &str) -> String {
    edit(text, server, name, value).unwrap()
}

fn refused(text: &str, server: &str, name: &str, value: &str) -> String {
    edit(text, server, name, value).unwrap_err().notice()
}

// Spec Menu save: Only the value changes.
#[test]
fn only_the_value_changes() {
    let out = ok(FILE, "clarity", "CLARITY_API_TOKEN", "b");
    assert_eq!(out, FILE.replace("\"a\"", "\"b\""));
}

// Spec Menu save: Comment after the value kept.
#[test]
fn the_comment_after_the_value_is_kept() {
    let out = ok(FILE, "ga4", "GOOGLE_CLOUD_PROJECT", "p2");
    assert_eq!(out, FILE.replace("\"p1\"", "\"p2\""));
}

// Spec Menu save: New table appended (after one empty line).
#[test]
fn a_missing_table_is_appended_after_one_empty_line() {
    let before = "[clarity]\nCLARITY_API_TOKEN = \"a\"\n";
    let out = ok(before, "ga4", "GOOGLE_CLOUD_PROJECT", "p");
    assert_eq!(
        out,
        format!("{before}\n[ga4]\nGOOGLE_CLOUD_PROJECT = \"p\"\n")
    );
    // A file already ending in an empty line, or without a final newline.
    let out = ok(
        "[clarity]\nX = \"a\"\n\n",
        "ga4",
        "GOOGLE_CLOUD_PROJECT",
        "p",
    );
    assert_eq!(
        out,
        "[clarity]\nX = \"a\"\n\n[ga4]\nGOOGLE_CLOUD_PROJECT = \"p\"\n"
    );
    let out = ok("[clarity]\nX = \"a\"", "ga4", "GOOGLE_CLOUD_PROJECT", "p");
    assert_eq!(
        out,
        "[clarity]\nX = \"a\"\n\n[ga4]\nGOOGLE_CLOUD_PROJECT = \"p\"\n"
    );
    // An empty file holds only the table.
    assert_eq!(
        ok("", "ga4", "GOOGLE_CLOUD_PROJECT", "p"),
        "[ga4]\nGOOGLE_CLOUD_PROJECT = \"p\"\n"
    );
}

#[test]
fn a_missing_key_is_appended_at_the_end_of_its_table() {
    let before =
        "[gsc]\nGSC_CREDENTIALS_PATH = \"p\"\n# note\n\n[clarity]\nCLARITY_API_TOKEN = \"a\"\n";
    let out = ok(before, "gsc", "GSC_SKIP_OAUTH", "id");
    assert_eq!(
        out,
        "[gsc]\nGSC_CREDENTIALS_PATH = \"p\"\nGSC_SKIP_OAUTH = \"id\"\n# note\n\n[clarity]\nCLARITY_API_TOKEN = \"a\"\n"
    );
    // Last table, no final newline.
    let out = ok(
        "[gsc]\nGSC_CREDENTIALS_PATH = \"p\"",
        "gsc",
        "GSC_SKIP_OAUTH",
        "id",
    );
    assert_eq!(
        out,
        "[gsc]\nGSC_CREDENTIALS_PATH = \"p\"\nGSC_SKIP_OAUTH = \"id\"\n"
    );
}

// Spec Menu save: CRLF kept.
#[test]
fn crlf_is_kept_on_every_added_line() {
    let before = "[clarity]\r\nCLARITY_API_TOKEN = \"a\"\r\n";
    let out = ok(before, "gsc", "GSC_CREDENTIALS_PATH", "p");
    assert_eq!(
        out,
        format!("{before}\r\n[gsc]\r\nGSC_CREDENTIALS_PATH = \"p\"\r\n")
    );
    let before = "[gsc]\r\nGSC_CREDENTIALS_PATH = \"p\"\r\n";
    let out = ok(before, "gsc", "GSC_SKIP_OAUTH", "id");
    assert_eq!(
        out,
        "[gsc]\r\nGSC_CREDENTIALS_PATH = \"p\"\r\nGSC_SKIP_OAUTH = \"id\"\r\n"
    );
}

#[test]
fn the_value_is_a_basic_string_with_its_escapes() {
    let out = ok("", "clarity", "CLARITY_API_TOKEN", "a\"b\\c 'd' é");
    assert_eq!(
        out,
        "[clarity]\nCLARITY_API_TOKEN = \"a\\\"b\\\\c 'd' é\"\n"
    );
    // It reads back as the same value.
    let table: toml::Table = out.parse().unwrap();
    assert_eq!(
        table["clarity"]["CLARITY_API_TOKEN"].as_str(),
        Some("a\"b\\c 'd' é")
    );
}

#[test]
fn a_value_that_is_not_one_plain_line_is_refused() {
    for bad in ["a\nb", "a\r", "a\tb", "a\u{7f}", "a\u{0}b"] {
        assert_eq!(
            refused("", "clarity", "CLARITY_API_TOKEN", bad),
            "secrets.toml: the value must be one line without control characters"
        );
    }
}

// Spec Menu save: Inline table refused, plus the other shapes.
#[test]
fn a_table_that_is_not_plain_is_refused() {
    let want = "secrets.toml: [clarity] is not a plain table; edit it by hand";
    for text in [
        "clarity = { CLARITY_API_TOKEN = \"a\" }\n",
        "clarity.CLARITY_API_TOKEN = \"a\"\n",
        "[[clarity]]\nCLARITY_API_TOKEN = \"a\"\n",
        "clarity = \"x\"\n",
    ] {
        assert_eq!(
            refused(text, "clarity", "CLARITY_API_TOKEN", "b"),
            want,
            "{text}"
        );
    }
}

#[test]
fn an_existing_value_that_is_not_a_string_is_refused() {
    assert_eq!(
        refused(
            "[ga4]\nGOOGLE_CLOUD_PROJECT = 5\n",
            "ga4",
            "GOOGLE_CLOUD_PROJECT",
            "p"
        ),
        "secrets.toml: ga4.GOOGLE_CLOUD_PROJECT: expected a string; edit it by hand"
    );
}

// Spec Menu save: A name the entry does not list.
#[test]
fn a_name_the_entry_does_not_list_is_refused() {
    for (server, name) in [
        ("clarity", "PATH"),
        ("ga4", "CLARITY_API_TOKEN"),
        ("nope", "X"),
    ] {
        assert_eq!(
            refused("", server, name, "v"),
            format!("secrets.toml: {server}.{name} is not a value of this connector")
        );
    }
}

// Spec Menu save: Unparseable file never overwritten.
#[test]
fn an_unparseable_file_is_refused_by_line_only() {
    let text = "[clarity]\n\nCLARITY_API_TOKEN = \"sk-123\n";
    let notice = refused(text, "clarity", "CLARITY_API_TOKEN", "b");
    assert_eq!(
        notice,
        "secrets.toml: not valid TOML (line 3); fix it by hand, nothing was saved"
    );
    assert!(!notice.contains("sk-123"));
}

#[test]
fn a_server_name_is_never_written_unless_it_is_an_entry() {
    // The refusal comes before any parse, so a bad file cannot hide it.
    assert_eq!(
        refused("[x\n", "nope", "X", "v"),
        "secrets.toml: nope.X is not a value of this connector"
    );
}
