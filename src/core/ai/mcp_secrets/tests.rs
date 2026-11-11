use std::ffi::OsString;

use super::*;

fn server(name: &str, env: &[&str]) -> McpServer {
    McpServer {
        name: name.to_owned(),
        command: "/usr/bin/x".to_owned(),
        args: Vec::new(),
        env: env.iter().map(|n| (*n).to_owned()).collect(),
        project: None,
        enabled: true,
    }
}

fn parse(text: &str) -> Loaded {
    Secrets::parse(text.as_bytes())
}

#[test]
fn redact_list_holds_what_the_child_gets_from_the_file_and_from_the_host() {
    let loaded =
        parse("[clarity]\nCLARITY_API_TOKEN = \"sk-1\"\nOTHER = \"unlisted\"\nEMPTY = \"\"\n");
    let s = server(
        "clarity",
        &["CLARITY_API_TOKEN", "EMPTY", "GITHUB_TOKEN", "NONE"],
    );
    let host = |name: &str| host(name);
    let own = loaded.secrets.lookup_for(&s, &host);
    let list = redact_list(&s, &own);
    // The file's value (not the host's `host`, which the file wins over), the
    // host fallback `gh`; never an empty value, an unlisted name or a name
    // nobody has.
    assert!(list.contains(&"sk-1".to_owned()), "{list:?}");
    assert!(list.contains(&"gh".to_owned()), "{list:?}");
    assert!(
        !list
            .iter()
            .any(|v| v.is_empty() || v == "unlisted" || v == "host")
    );
}

#[test]
fn redact_list_adds_the_percent_and_json_escaped_forms() {
    let s = server("x", &["A", "B"]);
    let lookup = |name: &str| match name {
        "A" => Some(OsString::from("p@ss w0rd/x")),
        "B" => Some(OsString::from("a\"b\\c")),
        _ => None,
    };
    let list = redact_list(&s, &lookup);
    for form in [
        "p@ss w0rd/x",
        "p%40ss%20w0rd%2Fx",
        "p%40ss%20w0rd%2fx",
        "a\"b\\c",
        "a\\\"b\\\\c",
    ] {
        assert!(list.contains(&form.to_owned()), "{form}: {list:?}");
    }
    let said = "dsn=u:p%40ss%20w0rd%2Fx@h json={\"k\":\"a\\\"b\\\\c\"}";
    let shown = redact_values(said, &list);
    assert!(
        !shown.contains("p%40") && !shown.contains("a\\\"b"),
        "{shown}"
    );
}

#[test]
fn redact_values_replaces_every_occurrence_longest_first_and_skips_empty() {
    let values = ["ab".to_owned(), String::new(), "abcd".to_owned()];
    assert_eq!(
        redact_values("x abcd y ab z abcd", &values),
        "x [redacted] y [redacted] z [redacted]"
    );
    assert_eq!(redact_values("nothing here", &values), "nothing here");
    assert_eq!(redact_values("keep", &[]), "keep");
    // A value that is a piece of the placeholder is not matched in it.
    let odd = ["ed".to_owned()];
    assert_eq!(redact_values("ed ed", &odd), "[redacted] [redacted]");
}

/// The host of these tests: `CLARITY_API_TOKEN=host`, `GITHUB_TOKEN=gh` and a
/// `PATH`, nothing else.
fn host(name: &str) -> Option<OsString> {
    match name {
        "CLARITY_API_TOKEN" => Some("host".into()),
        "GITHUB_TOKEN" => Some("gh".into()),
        "PATH" => Some("/usr/bin".into()),
        _ => None,
    }
}

fn no_host(_: &str) -> Option<OsString> {
    None
}

const EXAMPLE: &str =
    "[clarity]\nCLARITY_API_TOKEN = \"tok\"\n\n[ga4]\nGOOGLE_CLOUD_PROJECT = \"my-project\"\n";

// Spec mcp-secrets, Location and ownership: default place.
#[test]
fn the_default_place_is_under_home_dot_config() {
    assert_eq!(
        secrets_path(None, Some("/h".into())),
        Some(PathBuf::from("/h/.config/lazarobox/secrets.toml"))
    );
}

// Spec mcp-secrets, Location and ownership: XDG place.
#[test]
fn the_xdg_place_wins_over_home() {
    assert_eq!(
        secrets_path(Some("/x".into()), Some("/h".into())),
        Some(PathBuf::from("/x/lazarobox/secrets.toml"))
    );
    // A relative XDG folder is ignored, as for `config.toml`.
    assert_eq!(
        secrets_path(Some("x".into()), Some("/h".into())),
        Some(PathBuf::from("/h/.config/lazarobox/secrets.toml"))
    );
    assert_eq!(secrets_path(None, None), None);
}

// Spec mcp-secrets, Format: two tables.
#[test]
fn each_table_holds_the_values_of_its_own_server() {
    let loaded = parse(EXAMPLE);
    assert_eq!(loaded.problems, Vec::<String>::new());
    let s = &loaded.secrets;
    assert_eq!(s.value("clarity", "CLARITY_API_TOKEN"), Some("tok"));
    assert_eq!(s.value("ga4", "GOOGLE_CLOUD_PROJECT"), Some("my-project"));
    assert_eq!(s.value("ga4", "CLARITY_API_TOKEN"), None);
    assert_eq!(s.value("clarity", "GOOGLE_CLOUD_PROJECT"), None);
}

// Spec mcp-secrets, Format: wrong value type.
#[test]
fn a_value_that_is_not_a_string_is_skipped_alone_and_reported() {
    let loaded = parse("[ga4]\nGOOGLE_CLOUD_PROJECT = 5\nX = \"ok\"\nY = [\"sk-1\"]\n");
    assert_eq!(loaded.secrets.value("ga4", "X"), Some("ok"));
    assert_eq!(loaded.secrets.value("ga4", "GOOGLE_CLOUD_PROJECT"), None);
    assert_eq!(
        loaded.problems,
        [
            "secrets.toml: ga4.GOOGLE_CLOUD_PROJECT: expected a string",
            "secrets.toml: ga4.Y: expected a string",
        ]
    );
}

// Spec mcp-secrets, Format: bad name.
#[test]
fn a_key_that_is_not_a_variable_name_is_skipped_and_reported_quoted() {
    let loaded = parse("[ga4]\n\"BAD NAME\" = \"sk-v\"\nGOOD = \"g\"\n");
    assert_eq!(
        loaded.problems,
        ["secrets.toml: ga4.\"BAD NAME\": expected a variable name"]
    );
    assert_eq!(loaded.secrets.value("ga4", "BAD NAME"), None);
    assert_eq!(loaded.secrets.value("ga4", "GOOD"), Some("g"));
    // Too long for an env name (65 bytes), and a control character in a key.
    let long = "A".repeat(65);
    let loaded = parse(&format!("[a]\n{long} = \"v\"\n\"x\\u001b[31m\" = \"v\"\n"));
    assert_eq!(loaded.problems.len(), 2, "{:?}", loaded.problems);
    assert!(loaded.problems.iter().all(|p| !p.contains('\u{1b}')));
}

// Spec mcp-secrets, Format: top-level value.
#[test]
fn a_top_level_value_is_reported_and_the_tables_are_kept() {
    let loaded = parse("TOKEN = \"sk-top\"\n[clarity]\nCLARITY_API_TOKEN = \"tok\"\n");
    assert_eq!(loaded.problems, ["secrets.toml: TOKEN: expected a table"]);
    assert_eq!(
        loaded.secrets.value("clarity", "CLARITY_API_TOKEN"),
        Some("tok")
    );
}

// Spec mcp-secrets, Format: unknown table.
#[test]
fn an_unknown_table_is_no_problem_and_reaches_no_server() {
    let loaded = parse("[old-server]\nCLARITY_API_TOKEN = \"old\"\n");
    assert_eq!(loaded.problems, Vec::<String>::new());
    let clarity = server("clarity", &["CLARITY_API_TOKEN"]);
    let lookup = loaded.secrets.lookup_for(&clarity, &no_host);
    assert_eq!(lookup("CLARITY_API_TOKEN"), None);
}

// Spec mcp-secrets, Problems without values: syntax error hides the line.
#[test]
fn a_syntax_error_gives_its_line_and_no_text_of_it() {
    let text = "# my values\n[clarity]\nCLARITY_API_TOKEN = \"sk-123\nOTHER = \"o\"\n";
    let loaded = parse(text);
    assert_eq!(loaded.problems, ["secrets.toml: not valid TOML (line 3)"]);
    assert_eq!(loaded.secrets, Secrets::default());
    assert!(!format!("{loaded:?}").contains("sk-123"));
}

#[test]
fn bytes_that_are_not_utf8_are_reported_by_line_with_no_values() {
    let loaded = Secrets::parse(b"[a]\nX = \"ok\"\nY = \"\xff\"\n");
    assert_eq!(loaded.problems, ["secrets.toml: not valid TOML (line 3)"]);
    assert_eq!(loaded.secrets, Secrets::default());
}

#[test]
fn debug_shows_tables_and_counts_never_names_or_values() {
    let loaded = parse(EXAMPLE);
    let shown = format!("{loaded:?}");
    assert!(
        shown.contains("clarity") && shown.contains("ga4"),
        "{shown}"
    );
    for hidden in [
        "tok",
        "my-project",
        "CLARITY_API_TOKEN",
        "GOOGLE_CLOUD_PROJECT",
    ] {
        assert!(!shown.contains(hidden), "{hidden} in {shown}");
    }
}

// Spec mcp-servers, Lifecycle: value from the secrets file, and it wins
// over the host variable.
#[test]
fn a_named_variable_takes_the_file_value_over_the_host() {
    let loaded = parse(EXAMPLE);
    let clarity = server("clarity", &["CLARITY_API_TOKEN"]);
    let with_host = loaded.secrets.lookup_for(&clarity, &host);
    assert_eq!(with_host("CLARITY_API_TOKEN"), Some("tok".into()));
    let bare = loaded.secrets.lookup_for(&clarity, &no_host);
    assert_eq!(bare("CLARITY_API_TOKEN"), Some("tok".into()));
}

// Spec mcp-servers, Lifecycle: host variable when the file lacks the name.
#[test]
fn a_name_the_table_lacks_falls_back_to_the_host() {
    let loaded = parse(EXAMPLE);
    let clarity = server("clarity", &["GITHUB_TOKEN"]);
    let lookup = loaded.secrets.lookup_for(&clarity, &host);
    assert_eq!(lookup("GITHUB_TOKEN"), Some("gh".into()));
    // With no file at all, every name is the host's, as before.
    let none = Secrets::default();
    let lookup = none.lookup_for(&clarity, &host);
    assert_eq!(lookup("GITHUB_TOKEN"), Some("gh".into()));
    assert_eq!(lookup("CLARITY_API_TOKEN"), Some("host".into()));
}

// Spec mcp-servers, Secrets and privacy: a value reaches only its own server,
// and an unnamed key is not passed.
#[test]
fn a_value_reaches_only_its_own_server_and_only_when_named() {
    let loaded = parse("[clarity]\nCLARITY_API_TOKEN = \"tok\"\nEXTRA = \"e\"\nPATH = \"/evil\"\n");
    let ga4 = server("ga4", &["CLARITY_API_TOKEN"]);
    let lookup = loaded.secrets.lookup_for(&ga4, &no_host);
    assert_eq!(lookup("CLARITY_API_TOKEN"), None);
    let clarity = server("clarity", &["CLARITY_API_TOKEN"]);
    let lookup = loaded.secrets.lookup_for(&clarity, &host);
    assert_eq!(lookup("EXTRA"), None);
    // A base variable the entry does not name stays the host's.
    assert_eq!(lookup("PATH"), Some("/usr/bin".into()));
}

// Spec mcp-secrets, Problems without values: a token pasted as a key name
// is redacted in the problem, like any secret in a tool result.
#[test]
fn a_token_pasted_as_a_key_name_is_redacted_in_the_problem() {
    let loaded = parse("[ga4]\n\"sk-proj-abcdef123456\" = \"x\"\n");
    assert_eq!(loaded.problems.len(), 1, "{:?}", loaded.problems);
    let problem = &loaded.problems[0];
    assert!(problem.starts_with("secrets.toml: ga4."), "{problem}");
    assert!(problem.ends_with(": expected a variable name"), "{problem}");
    assert!(!problem.contains("abcdef123456"), "{problem}");
}

// A token pasted as a table name is redacted in the problem, like a key name.
#[test]
fn a_token_used_as_a_table_name_is_not_shown_in_a_problem() {
    let loaded = parse("sk-abcdef123456 = \"x\"\n[sk-abcdef654321]\nGOOD = 5\n");
    assert_eq!(loaded.problems.len(), 2, "{:?}", loaded.problems);
    for problem in &loaded.problems {
        assert!(!problem.contains("abcdef123456"), "{problem}");
        assert!(!problem.contains("abcdef654321"), "{problem}");
    }
}
