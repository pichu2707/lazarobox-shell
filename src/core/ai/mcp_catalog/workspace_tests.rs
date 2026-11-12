//! S6c: the Workspace entries, the `gsc` ways, the Gmail filter and the
//! confidential-by-default flag.

use super::*;

fn declared(name: &str) -> McpServer {
    McpServer {
        name: name.to_owned(),
        command: "x".to_owned(),
        args: Vec::new(),
        env: Vec::new(),
        project: None,
        enabled: true,
    }
}

/// A two-way entry for the machinery S6b1 will use for `gsc`.
fn two_ways() -> Entry {
    const OAUTH: &[Var] = &[
        Var {
            name: "CLIENT_FILE",
            preset: Some(".config/x/client.json"),
            under_home: true,
            ..Var::SECRET
        },
        Var {
            name: "SKIP",
            preset: Some("false"),
            ..Var::SECRET
        },
        Var {
            name: "TOKEN_DIR",
            ..Var::SECRET
        },
    ];
    const KEY: &[Var] = &[
        Var {
            name: "KEY_PATH",
            ..Var::SECRET
        },
        Var {
            name: "SKIP",
            preset: Some("true"),
            ..Var::SECRET
        },
    ];
    const ENV: &[Var] = &[OAUTH[0], OAUTH[2], KEY[0], OAUTH[1]];
    Entry {
        env: ENV,
        ways: &[
            Way {
                title: "OAuth",
                vars: OAUTH,
                login: None,
            },
            Way {
                title: "Key",
                vars: KEY,
                login: None,
            },
        ],
        ..entry("ga4").copied().unwrap()
    }
}

fn all_vars() -> impl Iterator<Item = &'static Var> {
    entries()
        .iter()
        .flat_map(|e| e.env.iter().chain(e.ways.iter().flat_map(|w| w.vars)))
}

// 6c.1 flags and presets

#[test]
fn a_secret_is_confidential_unless_it_opts_out_and_the_shown_set_is_listed() {
    let mut shown: Vec<&str> = all_vars().filter(|v| v.shown).map(|v| v.name).collect();
    shown.sort_unstable();
    shown.dedup();
    assert_eq!(
        shown,
        [
            "GOOGLE_CLOUD_PROJECT",
            "GOOGLE_OAUTH_CLIENT_ID",
            "GSC_CONFIG_DIR",
            "GSC_CREDENTIALS_PATH",
            "GSC_OAUTH_CLIENT_SECRETS_FILE",
            "GSC_SKIP_OAUTH",
            "USER_GOOGLE_EMAIL",
            "WORKSPACE_MCP_DISABLE_LOCAL_FILES",
        ]
    );
    for v in all_vars() {
        assert!(!v.shown || v.secret, "{}", v.name);
        assert_eq!(v.confidential(), v.secret && !v.shown, "{}", v.name);
    }
    for name in ["CLARITY_API_TOKEN", "GOOGLE_OAUTH_CLIENT_SECRET"] {
        let v = all_vars().find(|v| v.name == name).unwrap();
        assert!(v.confidential(), "{name}");
    }
}

#[test]
fn a_path_preset_is_written_absolute_and_never_with_a_tilde() {
    let v = Var {
        name: "P",
        preset: Some(".config/x"),
        under_home: true,
        ..Var::SECRET
    };
    assert_eq!(v.preset_for("/h").as_deref(), Some("/h/.config/x"));
    assert_eq!(v.preset_for("/h/").as_deref(), Some("/h/.config/x"));
    assert_eq!(v.preset_for("~"), None);
    assert_eq!(v.preset_for("rel"), None);
    let plain = Var {
        preset: Some("true"),
        ..v
    };
    assert_eq!(
        Var {
            under_home: false,
            ..plain
        }
        .preset_for("")
        .as_deref(),
        Some("true")
    );
}

#[test]
fn every_entry_env_is_the_union_of_its_ways_and_links_match() {
    for e in entries() {
        if !e.ways.is_empty() {
            let mut union: Vec<&str> = Vec::new();
            for way in e.ways {
                for v in way.vars {
                    if !union.contains(&v.name) {
                        union.push(v.name);
                    }
                }
            }
            let mut env: Vec<&str> = e.env.iter().map(|v| v.name).collect();
            env.sort_unstable();
            union.sort_unstable();
            assert_eq!(env, union, "{}", e.id);
        }
        if e.link == Link::Login {
            assert!(
                e.login.is_some() || !e.ways.is_empty(),
                "{}: a login link needs a line or ways",
                e.id
            );
        }
        for hidden in e.hidden {
            assert!(
                e.tools.iter().any(|(n, _)| n == hidden),
                "{}: {hidden}",
                e.id
            );
        }
    }
}

// 6c.2 gsc ways

#[test]
fn ways_resolve_their_own_presets_with_an_absolute_path_and_no_tilde() {
    let e = two_ways();
    assert_eq!(
        e.ways[0].values("/h"),
        [
            ("CLIENT_FILE", "/h/.config/x/client.json".to_owned()),
            ("SKIP", "false".to_owned()),
            ("TOKEN_DIR", String::new()),
        ]
    );
    assert_eq!(
        e.ways[1].values("/h"),
        [("KEY_PATH", String::new()), ("SKIP", "true".to_owned())]
    );
    assert!(e.ways[0].values("~")[0].1.is_empty());
}

#[test]
fn gsc_has_an_oauth_way_first_and_a_service_account_way() {
    let e = entry("gsc").unwrap();
    assert_eq!(e.ways.len(), 2);
    assert_eq!(e.ways[0].title, "OAuth");
    assert_eq!(
        e.ways[0].values("/h"),
        [
            (
                "GSC_OAUTH_CLIENT_SECRETS_FILE",
                "/h/.config/lazarobox/google-oauth-client.json".to_owned()
            ),
            ("GSC_CONFIG_DIR", "/h/.config/mcp-gsc".to_owned()),
            ("GSC_SKIP_OAUTH", "false".to_owned()),
        ]
    );
    assert_eq!(
        e.ways[1].values("/h"),
        [
            ("GSC_CREDENTIALS_PATH", String::new()),
            ("GSC_SKIP_OAUTH", "true".to_owned()),
        ]
    );
    for way in e.ways {
        for (name, value) in way.values("/h") {
            assert!(!value.contains('~'), "{name}");
        }
    }
    // The OAuth way comes back only with the Link action.
    assert_eq!(e.link, Link::Login);
    assert!(e.ways[0].login.is_none() && e.login.is_none());
}

#[test]
fn the_gsc_template_keeps_the_service_account_values_of_s5() {
    let e = entry("gsc").unwrap();
    assert_eq!(
        secrets_template(e).unwrap(),
        "[gsc]\nGSC_OAUTH_CLIENT_SECRETS_FILE = \"\"\nGSC_CONFIG_DIR = \"\"\nGSC_CREDENTIALS_PATH = \"\"\nGSC_SKIP_OAUTH = \"true\"\n"
    );
}

#[test]
fn gsc_hides_reauthenticate_and_names_no_destructive_flag() {
    let e = entry("gsc").unwrap();
    assert_eq!(e.hidden, ["reauthenticate"]);
    assert_eq!(e.tools.len(), 21);
    assert!(!format!("{e:?}{}", snippet(e)).contains("GSC_ALLOW_DESTRUCTIVE"));
}

#[test]
fn a_table_with_only_the_key_path_is_complete_as_on_main() {
    let gsc = entry("gsc").unwrap();
    assert!(gsc.missing(|n| n == "GSC_CREDENTIALS_PATH").is_empty());
    // Nothing set: the OAuth way, the default, names what is missing. Its
    // path presets are under `$HOME`, so the template cannot fill them.
    assert_eq!(
        gsc.missing(|_| false),
        ["GSC_OAUTH_CLIENT_SECRETS_FILE", "GSC_CONFIG_DIR"]
    );
    // With ways: the first complete way wins, else the first way names it.
    let e = two_ways();
    assert!(e.missing(|n| n == "KEY_PATH").is_empty());
    assert_eq!(e.missing(|_| false), ["CLIENT_FILE", "TOKEN_DIR"]);
    let ga4 = entry("ga4").unwrap();
    assert_eq!(ga4.missing(|_| false), ["GOOGLE_CLOUD_PROJECT"]);
    assert!(entry("apps-script").unwrap().missing(|_| false).is_empty());
}

#[test]
fn the_way_is_the_one_the_table_fills_completely_else_the_first() {
    let gsc = entry("gsc").unwrap();
    assert_eq!(gsc.way_for(|_| false), 0);
    assert_eq!(gsc.way_for(|n| n == "GSC_CREDENTIALS_PATH"), 1);
    let oauth = |n: &str| ["GSC_OAUTH_CLIENT_SECRETS_FILE", "GSC_CONFIG_DIR"].contains(&n);
    assert_eq!(gsc.way_for(oauth), 0);
    // Both complete: the first.
    assert_eq!(gsc.way_for(|_| true), 0);
    assert_eq!(entry("ga4").unwrap().way_for(|_| true), 0);
}

// 6c.3 google

#[test]
fn google_installs_pinned_and_runs_the_permissions_it_verified() {
    let e = entry("google").unwrap();
    assert_eq!(e.install, "uv tool install workspace-mcp==2.4.1");
    assert_eq!(e.command, "workspace-mcp");
    assert_eq!(
        e.args,
        [
            "--single-user",
            "--permissions",
            "gmail:readonly+drafts",
            "docs:full",
            "sheets:full",
            "--disabled-tools",
            "send_gmail_message",
        ]
    );
    let server = {
        let config = crate::core::config::Config::parse(&snippet(e)).unwrap();
        config.ai.mcp_servers()[0].clone()
    };
    assert_eq!(
        server.env,
        [
            "GOOGLE_OAUTH_CLIENT_ID",
            "GOOGLE_OAUTH_CLIENT_SECRET",
            "USER_GOOGLE_EMAIL",
            "WORKSPACE_MCP_DISABLE_LOCAL_FILES"
        ]
    );
    let text = format!("{e:?}");
    assert!(!text.contains("WORKSPACE_MCP_CREDENTIALS_DIR"));
    assert!(
        !e.args
            .iter()
            .any(|a| *a == "--tools" || *a == "--read-only")
    );
    assert_eq!((e.login.is_none(), e.link), (true, Link::GoogleAuth));
    assert!(e.gmail_allowlist);
    assert_eq!(e.hidden, ["start_google_auth"]);
}

#[test]
fn google_turns_local_files_off_by_default() {
    let e = entry("google").unwrap();
    assert_eq!(
        secrets_template(e).unwrap(),
        "[google]\nGOOGLE_OAUTH_CLIENT_ID = \"\"\nGOOGLE_OAUTH_CLIENT_SECRET = \"\"\nUSER_GOOGLE_EMAIL = \"\"\nWORKSPACE_MCP_DISABLE_LOCAL_FILES = \"true\"\n"
    );
}

#[test]
fn google_pins_the_42_tools_with_their_classes() {
    let e = entry("google").unwrap();
    assert_eq!(e.tools.len(), 42);
    let mut names: Vec<_> = e.tools.iter().map(|(n, _)| *n).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 42);
    for tool in [
        "draft_gmail_message",
        "get_gmail_attachment_content",
        "start_google_auth",
        "modify_sheet_values",
        "batch_update_doc",
        "unknown_tool",
    ] {
        assert_eq!(class("google", tool), Class::Write, "{tool}");
    }
    for tool in [
        "search_gmail_messages",
        "get_gmail_thread_content",
        "list_gmail_labels",
        "get_doc_content",
        "read_sheet_values",
        "list_spreadsheet_comments",
    ] {
        assert_eq!(class("google", tool), Class::Read, "{tool}");
    }
    let reads = e.tools.iter().filter(|(_, c)| *c == Class::Read).count();
    assert_eq!(reads, 6 + 7 + 5);
    // Every pinned Gmail tool is one the allowlist admits.
    for (name, _) in e.tools.iter().filter(|(n, _)| n.contains("gmail")) {
        assert!(GMAIL_ALLOWLIST.contains(name), "{name}");
    }
}

#[test]
fn no_two_google_tools_collide_after_the_name_cut() {
    for e in entries() {
        let names: Vec<String> = e.tools.iter().map(|(n, _)| (*n).to_owned()).collect();
        assert_eq!(checks::collisions(e.id, &names), Vec::<String>::new());
    }
}

#[test]
fn the_google_guide_states_the_risks_and_the_steps() {
    let guide = entry("google").unwrap().guide.join("\n");
    for word in [
        "name = \"google\"",
        "Gmail, Docs, Sheets and Drive",
        "Desktop",
        "expire after 7 days",
        "drafts",
        "send",
        "gmail.compose",
        "plaintext",
        "~/.google_workspace_mcp",
        "drive.readonly",
        "get_gmail_attachment_content",
        "WORKSPACE_MCP_DISABLE_LOCAL_FILES",
        "~/.workspace-mcp/attachments",
    ] {
        assert!(guide.contains(word), "{word}: {guide}");
    }
}

// 6c.4 the filter

#[test]
fn only_the_eight_gmail_names_pass_for_google() {
    let g = declared("google");
    for tool in GMAIL_ALLOWLIST {
        assert!(admits(&g, tool), "{tool}");
    }
    for tool in [
        "send_gmail_message",
        "delete_gmail_label_x",
        "Modify_GMAIL_Labels",
        "manage_gmail_filter",
        "SEARCH_GMAIL_MESSAGES",
    ] {
        assert!(!admits(&g, tool), "{tool}");
    }
    assert!(admits(&g, "search_docs") && admits(&g, "read_sheet_values"));
}

#[test]
fn hidden_tools_are_dropped_by_server_name_only() {
    assert!(!admits(&declared("google"), "start_google_auth"));
    assert!(!admits(&declared("gsc"), "reauthenticate"));
    assert!(admits(&declared("gsc"), "get_search_analytics"));
    assert!(admits(&declared("google"), "reauthenticate"));
    assert!(!admits(&declared("google"), "Start_Google_Auth"));
    assert!(!admits(&declared("gsc"), "REAUTHENTICATE"));
    for name in ["mail", "search", "Google"] {
        let s = declared(name);
        for tool in ["send_gmail_message", "start_google_auth", "reauthenticate"] {
            assert!(admits(&s, tool), "{name} {tool}");
        }
    }
}

#[test]
fn the_filter_runs_on_the_whole_list_so_the_cap_keeps_the_admitted_tools() {
    let g = declared("google");
    let mut listed = vec![
        "send_gmail_message".to_owned(),
        "start_google_auth".to_owned(),
    ];
    listed.extend((0..6).map(|i| format!("unknown_gmail_{i}")));
    listed.extend((0..62).map(|i| format!("tool_{i}")));
    assert_eq!(listed.len(), 70);
    let admitted = listed.iter().filter(|t| admits(&g, t)).count();
    assert_eq!(admitted, 62);
    assert!(checks::too_many("google", admitted).is_none());
}

// 6c.7 apps-script

#[test]
fn apps_script_pins_clasp_and_has_no_secrets() {
    let e = entry("apps-script").unwrap();
    assert_eq!(e.install, "npm i -g @google/clasp@3.4.1");
    assert_eq!((e.command, e.args), ("clasp", &["mcp"][..]));
    assert!(e.env.is_empty() && secrets_template(e).is_none());
    assert!(!snippet(e).contains("env"));
    assert_eq!(e.login.unwrap().line, "clasp login");
    assert_eq!(e.link, Link::Login);
    assert!(!e.gmail_allowlist && e.hidden.is_empty());
    assert_eq!(e.tools.len(), 5);
    assert_eq!(class("apps-script", "list_projects"), Class::Read);
    for tool in [
        "push_files",
        "create_project",
        "pull_files",
        "clone_project",
    ] {
        assert_eq!(class("apps-script", tool), Class::Write, "{tool}");
    }
}

#[test]
fn the_apps_script_guide_names_the_switch_and_the_local_writes() {
    let guide = entry("apps-script").unwrap().guide.join("\n");
    for word in [
        "script.google.com/home/usersettings",
        "push_files",
        "scopes",
        "pull_files",
        "clone_project",
        "overwrite",
    ] {
        assert!(guide.contains(word), "{word}: {guide}");
    }
}
