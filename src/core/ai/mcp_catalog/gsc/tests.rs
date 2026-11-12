use super::*;

const ALL: [&str; 4] = [
    "GSC_OAUTH_CLIENT_SECRETS_FILE",
    "GSC_CONFIG_DIR",
    "GSC_CREDENTIALS_PATH",
    "GSC_SKIP_OAUTH",
];

fn server(name: &str, env: &[&str]) -> McpServer {
    McpServer {
        name: name.to_owned(),
        command: "mcp-search-console".to_owned(),
        args: Vec::new(),
        env: env.iter().map(|n| (*n).to_owned()).collect(),
        project: None,
        enabled: true,
    }
}

fn values<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |name| {
        pairs
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| (*v).to_owned())
    }
}

const OAUTH: [(&str, &str); 3] = [
    (
        "GSC_OAUTH_CLIENT_SECRETS_FILE",
        "/h/.config/lazarobox/google-oauth-client.json",
    ),
    ("GSC_CONFIG_DIR", "/h/.config/mcp-gsc"),
    ("GSC_SKIP_OAUTH", "false"),
];

// 6b1.5: the start refusal

#[test]
fn oauth_values_without_a_token_are_refused_as_not_linked() {
    let s = server("gsc", &ALL);
    let seen = std::cell::RefCell::new(Vec::new());
    let refusal = start_refusal(&s, &values(&OAUTH), |p| {
        seen.borrow_mut().push(p.to_owned());
        false
    });
    assert_eq!(refusal, Some(NOT_LINKED));
    assert_eq!(
        NOT_LINKED,
        "not linked; open Connectors and use Link with Google"
    );
    assert_eq!(
        *seen.borrow(),
        [Path::new("/h/.config/mcp-gsc/token.json").to_owned()]
    );
}

#[test]
fn a_token_lets_the_start_through() {
    let s = server("gsc", &ALL);
    assert_eq!(start_refusal(&s, &values(&OAUTH), |_| true), None);
}

#[test]
fn a_missing_or_relative_config_dir_is_refused_without_looking_for_a_token() {
    let s = server("gsc", &ALL);
    for dir in [None, Some(""), Some("~/.config/mcp-gsc"), Some("mcp-gsc")] {
        let mut pairs = vec![OAUTH[0], OAUTH[2]];
        if let Some(dir) = &dir {
            pairs.push(("GSC_CONFIG_DIR", dir));
        }
        let refusal = start_refusal(&s, &values(&pairs), |_| panic!("no stat without a folder"));
        assert_eq!(refusal, Some(UNFILLED), "{dir:?}");
    }
    assert_eq!(UNFILLED, "fill the OAuth values in Connectors (f)");
}

#[test]
fn the_service_account_way_and_old_entries_are_never_refused() {
    let key = [
        ("GSC_CREDENTIALS_PATH", "/k.json"),
        ("GSC_SKIP_OAUTH", "true"),
    ];
    let new = server("gsc", &ALL);
    // An S5 entry pasted before the OAuth way: it names neither OAuth value.
    let s5 = server("gsc", &["GSC_CREDENTIALS_PATH", "GSC_SKIP_OAUTH"]);
    let host = [OAUTH[0], OAUTH[1], ("GSC_SKIP_OAUTH", "true")];
    for skip in ["true", "1", "yes", "TRUE", " Yes "] {
        let pairs = [OAUTH[0], OAUTH[1], ("GSC_SKIP_OAUTH", skip)];
        assert_eq!(
            start_refusal(&new, &values(&pairs), |_| false),
            None,
            "{skip}"
        );
    }
    assert_eq!(start_refusal(&new, &values(&key), |_| false), None);
    assert_eq!(start_refusal(&s5, &values(&key), |_| false), None);
    // The server only gets the names it lists: a host value of the others
    // does not select OAuth.
    assert_eq!(start_refusal(&s5, &values(&host), |_| false), None);
    let host_oauth = [OAUTH[0], OAUTH[1]];
    assert_eq!(start_refusal(&s5, &values(&host_oauth), |_| false), None);
}

#[test]
fn another_name_is_not_covered() {
    let s = server("search", &ALL);
    assert_eq!(start_refusal(&s, &values(&OAUTH), |_| false), None);
}

#[test]
fn linked_is_the_token_file_for_the_oauth_way_only() {
    let s = server("gsc", &ALL);
    assert_eq!(linked(&s, &values(&OAUTH), |_| true), Some(true));
    assert_eq!(linked(&s, &values(&OAUTH), |_| false), Some(false));
    let key = [
        ("GSC_CREDENTIALS_PATH", "/k.json"),
        ("GSC_SKIP_OAUTH", "true"),
    ];
    assert_eq!(linked(&s, &values(&key), |_| true), None);
    assert_eq!(linked(&server("x", &ALL), &values(&OAUTH), |_| true), None);
    // A relative folder is not linked: nothing is looked for.
    let rel = [OAUTH[0], ("GSC_CONFIG_DIR", "rel")];
    assert_eq!(
        linked(&s, &values(&rel), |_| panic!("no stat")),
        Some(false)
    );
}

// 6b1.3: the values of the line are the values the server gets

#[test]
fn the_plan_holds_what_the_server_would_get() {
    let s = server("gsc", &ALL);
    assert_eq!(
        plan(&s, &values(&OAUTH)),
        Ok(Plan {
            dir: "/h/.config/mcp-gsc".to_owned(),
            file: "/h/.config/lazarobox/google-oauth-client.json".to_owned(),
        })
    );
}

#[test]
fn a_declared_entry_that_does_not_pass_a_value_is_refused_naming_it() {
    let no_dir = server("gsc", &["GSC_OAUTH_CLIENT_SECRETS_FILE", "GSC_SKIP_OAUTH"]);
    assert_eq!(plan(&no_dir, &values(&OAUTH)), Err(PASS_DIR));
    let no_file = server("gsc", &["GSC_CONFIG_DIR", "GSC_SKIP_OAUTH"]);
    assert_eq!(plan(&no_file, &values(&OAUTH)), Err(PASS_FILE));
    let s5 = server("gsc", &["GSC_CREDENTIALS_PATH", "GSC_SKIP_OAUTH"]);
    assert!(plan(&s5, &values(&OAUTH)).is_err_and(|n| n.contains("paste the current entry")));
}

#[test]
fn unfilled_relative_or_skipped_values_say_to_fill_them_first() {
    let s = server("gsc", &ALL);
    let rel = [OAUTH[0], ("GSC_CONFIG_DIR", "~/.config/mcp-gsc")];
    let empty = [("GSC_OAUTH_CLIENT_SECRETS_FILE", ""), OAUTH[1]];
    let relative_file = [("GSC_OAUTH_CLIENT_SECRETS_FILE", "client.json"), OAUTH[1]];
    let skip = [OAUTH[0], OAUTH[1], ("GSC_SKIP_OAUTH", "true")];
    for pairs in [&rel[..], &empty, &relative_file, &skip, &[]] {
        assert_eq!(plan(&s, &values(pairs)), Err(FILL_FIRST), "{pairs:?}");
    }
}

// 6b1.1: the uv checks, parsed

#[test]
fn the_pinned_version_in_uv_tool_list_passes() {
    let list = "mcp-search-console v0.4.2\n- mcp-gsc\n- mcp-search-console\nruff v0.1.0\n";
    assert_eq!(pinned(list), Ok(()));
    assert_eq!(pinned("mcp-search-console v0.4.2"), Ok(()));
}

#[test]
fn another_version_or_none_names_what_was_found_and_the_fix() {
    assert_eq!(
        pinned("mcp-search-console v0.4.3\n- mcp-gsc\n"),
        Err(
            "mcp-search-console 0.4.3; run: uv tool install --force mcp-search-console==0.4.2"
                .to_owned()
        )
    );
    assert_eq!(
        pinned("ruff v0.1.0\n"),
        Err("mcp-search-console not installed; run: uv tool install --force mcp-search-console==0.4.2"
            .to_owned())
    );
    // A longer version is not the pinned one.
    assert!(pinned("mcp-search-console v0.4.20").is_err());
}

#[test]
fn the_version_found_is_sanitized_and_short() {
    let long = format!("mcp-search-console v1.\u{1b}[31m{}\n", "9".repeat(60));
    let notice = pinned(&long).unwrap_err();
    assert!(!notice.contains('\u{1b}'), "{notice:?}");
    let found = notice.strip_prefix("mcp-search-console ").unwrap();
    let found = found.split(';').next().unwrap();
    assert!(found.chars().count() <= 20, "{found}");
}

#[test]
fn the_tool_dir_is_one_absolute_folder() {
    assert_eq!(
        tool_dir("/h/.local/share/uv/tools\n"),
        Some("/h/.local/share/uv/tools".to_owned())
    );
    for bad in ["", "\n", "relative/dir\n", "/a\n/b\n", "/a\u{7}b\n", "$(x)"] {
        assert_eq!(tool_dir(bad), None, "{bad:?}");
    }
}

// 6b1.2: the line

fn plan_of(dir: &str, file: &str) -> Plan {
    Plan {
        dir: dir.to_owned(),
        file: file.to_owned(),
    }
}

#[test]
fn the_line_is_exactly_the_spec_text() {
    let plan = plan_of(
        "/h/.config/mcp-gsc",
        "/h/.config/lazarobox/google-oauth-client.json",
    );
    assert_eq!(
        login_line("/h/.local/share/uv/tools", &plan).unwrap(),
        "env -u XDG_CONFIG_HOME -u XDG_DATA_HOME -u GSC_ALLOW_DESTRUCTIVE -u GSC_SKIP_OAUTH GSC_CONFIG_DIR='/h/.config/mcp-gsc' GSC_OAUTH_CLIENT_SECRETS_FILE='/h/.config/lazarobox/google-oauth-client.json' '/h/.local/share/uv/tools/mcp-search-console/bin/python' -I -c 'import gsc_server as g; g.get_gsc_service_oauth(); print(\"Search Console linked\")'"
    );
}

#[test]
fn a_quote_is_escaped_and_a_control_character_or_substitution_refused() {
    let line = login_line("/t", &plan_of("/d", "/it's here/c.json")).unwrap();
    assert!(
        line.contains("GSC_OAUTH_CLIENT_SECRETS_FILE='/it'\\''s here/c.json'"),
        "{line}"
    );
    for bad in ["/a\nb", "/a\u{1b}b", "/a\tb", "/a$(x)", "/a\u{7f}"] {
        assert_eq!(
            login_line("/t", &plan_of("/d", bad)),
            Err(BAD_PATH),
            "{bad:?}"
        );
        assert_eq!(
            login_line("/t", &plan_of(bad, "/f")),
            Err(BAD_PATH),
            "{bad:?}"
        );
        assert_eq!(
            login_line(bad, &plan_of("/d", "/f")),
            Err(BAD_PATH),
            "{bad:?}"
        );
    }
}

#[test]
fn the_line_stays_outside_mcp_and_names_no_confidential_value() {
    let line = login_line("/t", &plan_of("/h/.config/mcp-gsc", "/f")).unwrap();
    for word in ["uvx", "npx", "pipx run", "dlx", "$(", "`"] {
        assert!(!line.contains(word), "{word}");
    }
    for unset in [
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "GSC_ALLOW_DESTRUCTIVE",
        "GSC_SKIP_OAUTH",
    ] {
        assert!(line.contains(&format!("-u {unset} ")), "{unset}");
    }
    assert!(line.contains("GSC_CONFIG_DIR='/h/.config/mcp-gsc'"));
    assert!(line.chars().all(|c| !c.is_control()));
}
