use super::*;
use crate::core::{
    ai::{
        chat::approval::ask_for,
        tools::{Call, Locality},
    },
    opener::is_web_url,
};

fn declared(name: &str) -> McpServer {
    McpServer {
        name: name.to_owned(),
        command: "whatever".to_owned(),
        args: Vec::new(),
        env: Vec::new(),
        project: None,
        enabled: true,
    }
}

fn parse_snippet(entry: &Entry) -> McpServer {
    let config = crate::core::config::Config::parse(&snippet(entry)).unwrap();
    assert_eq!(config.problems, Vec::<String>::new());
    let servers = config.ai.mcp_servers();
    assert_eq!(servers.len(), 1);
    servers[0].clone()
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|n| (*n).to_owned()).collect()
}

fn bare() -> Entry {
    Entry {
        id: "bare",
        title: "Bare",
        group: Group::Workspace,
        project: "p",
        license: "l",
        install: "npm i -g x",
        command: "x",
        args: &[],
        env: &[],
        ways: &[],
        login: None,
        link: Link::None,
        hidden: &[],
        gmail_allowlist: false,
        guide: &["g"],
        links: &[],
        token_location: "t",
        tools: &[],
    }
}

// 2.1 integrity

#[test]
fn the_catalog_lists_the_analytics_then_the_workspace_entries() {
    let ids: Vec<_> = entries().iter().map(|e| e.id).collect();
    assert_eq!(ids, ["clarity", "ga4", "gsc", "google", "apps-script"]);
    let groups: Vec<_> = entries().iter().map(|e| e.group).collect();
    assert_eq!(
        groups,
        [
            Group::Analytics,
            Group::Analytics,
            Group::Analytics,
            Group::Workspace,
            Group::Workspace
        ]
    );
}

#[test]
fn every_entry_is_complete_and_its_links_pass_the_opener_check() {
    assert!(!entries().is_empty());
    for e in entries() {
        assert!(!e.install.is_empty() && !e.command.is_empty(), "{}", e.id);
        assert!(!e.project.is_empty() && !e.license.is_empty(), "{}", e.id);
        assert!(
            !e.token_location.is_empty() && !e.guide.is_empty(),
            "{}",
            e.id
        );
        assert!(!e.links.is_empty(), "{}", e.id);
        for link in e.links {
            assert!(is_web_url(link), "{}: {link}", e.id);
        }
        // The id is a valid server name: the snippet parses back with no problem.
        assert_eq!(parse_snippet(e).name, e.id);
    }
}

#[test]
fn ids_are_unique() {
    let mut ids: Vec<_> = entries().iter().map(|e| e.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), entries().len());
}

// 2.2 match by name

#[test]
fn a_declared_entry_finds_itself_by_name() {
    assert_eq!(entry("clarity").map(|e| e.id), Some("clarity"));
    assert_eq!(entry("ga4").map(|e| e.id), Some("ga4"));
}

#[test]
fn only_an_exact_name_matches_and_other_servers_are_untouched() {
    for name in ["fs", "Clarity", "clarity ", "clarity-2", "", "clar"] {
        assert!(entry(name).is_none(), "{name:?}");
        assert!(admits(&declared(name), "any_tool"));
    }
}

// 2.3 class, recorded only

#[test]
fn a_known_tool_has_its_class_and_an_unknown_one_is_write() {
    assert_eq!(class("ga4", "run_report"), Class::Read);
    assert_eq!(class("ga4", "create_property_annotation"), Class::Write);
    assert_eq!(class("clarity", "new_tool"), Class::Write);
    assert_eq!(class("fs", "run_report"), Class::Write);
}

#[test]
fn a_read_tool_still_asks_for_a_y_on_every_locality() {
    let call = Call::External {
        name: "ga4__run_report".to_owned(),
        server: "ga4".to_owned(),
        tool: "run_report".to_owned(),
        args: serde_json::json!({}),
    };
    assert_eq!(class("ga4", "run_report"), Class::Read);
    for locality in [Locality::Local, Locality::Cloud] {
        assert!(ask_for(&call, locality).is_some(), "{locality:?}");
    }
}

// 2.4 no first-run download

#[test]
fn no_run_or_install_command_downloads_on_first_run() {
    assert!(!entries().is_empty());
    for e in entries() {
        assert!(
            !["uvx", "npx", "pipx", "pnpm"].contains(&e.command),
            "{}",
            e.id
        );
        assert!(!e.command.contains('/'), "{}", e.id);
        for line in std::iter::once(e.install).chain(e.login.map(|l| l.line)) {
            let head = line.split_whitespace().next().unwrap_or("");
            assert!(!["uvx", "npx", "pnpm"].contains(&head), "{line}");
            assert!(
                !line.contains("pipx run") && !line.contains("dlx"),
                "{line}"
            );
        }
    }
}

// 2.5 entries

#[test]
fn clarity_builds_its_snippet_and_template() {
    let e = entry("clarity").unwrap();
    assert_eq!(e.install, "npm i -g @microsoft/clarity-mcp-server");
    assert!(e.login.is_none());
    let server = parse_snippet(e);
    assert_eq!(server.command, "clarity-mcp-server");
    assert_eq!(server.env, ["CLARITY_API_TOKEN"]);
    assert!(server.args.is_empty());
    assert_eq!(
        secrets_template(e).unwrap(),
        "[clarity]\nCLARITY_API_TOKEN = \"\"\n"
    );
}

#[test]
fn clarity_knows_its_three_tools_as_read() {
    let e = entry("clarity").unwrap();
    assert_eq!(e.tools.len(), 3);
    assert!(e.tools.iter().all(|(_, c)| *c == Class::Read));
}

#[test]
fn the_clarity_guide_states_the_limits() {
    let guide = entry("clarity").unwrap().guide.join("\n");
    assert!(guide.contains("10 requests per project per day"), "{guide}");
    assert!(guide.contains("1 to 3 days"));
    assert!(guide.contains("1,000 rows"));
    assert!(guide.contains("project settings"));
}

#[test]
fn ga4_runs_read_only_and_names_the_cloud_project() {
    let e = entry("ga4").unwrap();
    assert_eq!(e.install, "pipx install analytics-mcp");
    let server = parse_snippet(e);
    assert_eq!(server.command, "analytics-mcp");
    assert_eq!(server.env, ["GOOGLE_CLOUD_PROJECT"]);
    assert!(!snippet(e).contains("--allow-write"));
    assert!(server.args.iter().all(|a| a != "--allow-write"));
    assert_eq!(
        secrets_template(e).unwrap(),
        "[ga4]\nGOOGLE_CLOUD_PROJECT = \"\"\n"
    );
}

#[test]
fn ga4_logs_in_with_the_user_own_client_file() {
    let line = entry("ga4").unwrap().login.unwrap().line;
    assert!(line.starts_with("gcloud auth application-default login --scopes "));
    assert!(line.contains("analytics.readonly"));
    assert!(line.contains("--client-id-file=~/.config/lazarobox/google-oauth-client.json"));
}

#[test]
fn ga4_classes_are_read_and_an_unpinned_write_tool_is_write() {
    for tool in ["run_report", "run_funnel_report", "run_realtime_report"] {
        assert_eq!(class("ga4", tool), Class::Read, "{tool}");
    }
    assert_eq!(class("ga4", "create_property_annotation"), Class::Write);
}

#[test]
fn the_ga4_guide_warns_about_testing_clients() {
    let guide = entry("ga4").unwrap().guide.join("\n");
    assert!(guide.contains("Testing"), "{guide}");
    assert!(guide.contains("expire after 7 days"));
    assert!(guide.contains("Desktop"));
    assert!(guide.contains("Google Analytics"));
    assert!(guide.contains("google-oauth-client.json"));
}

// 5.1 gsc with a service account

#[test]
fn gsc_uses_a_service_account_and_never_names_the_destructive_flag() {
    let e = entry("gsc").unwrap();
    assert_eq!(e.install, "uv tool install mcp-search-console==0.4.2");
    assert_eq!(e.command, "mcp-search-console");
    assert!(e.login.is_none());
    let server = parse_snippet(e);
    assert_eq!(server.env, ["GSC_CREDENTIALS_PATH", "GSC_SKIP_OAUTH"]);
    let all = format!("{}{:?}", snippet(e), e);
    assert!(!all.contains("GSC_ALLOW_DESTRUCTIVE"), "{all}");
}

#[test]
fn the_gsc_guide_walks_through_the_service_account() {
    let guide = entry("gsc").unwrap().guide.join("\n");
    for word in [
        "service account",
        "Search Console API",
        "JSON",
        "0600",
        "gsc-service-account.json",
        "user of the property",
        "name = \"gsc\"",
    ] {
        assert!(guide.contains(word), "{word}: {guide}");
    }
}

#[test]
fn gsc_classes_split_queries_from_sitemap_and_site_changes() {
    for tool in [
        "get_search_analytics",
        "inspect_url_enhanced",
        "list_sitemaps_enhanced",
        "list_properties",
    ] {
        assert_eq!(class("gsc", tool), Class::Read, "{tool}");
    }
    for tool in [
        "manage_sitemaps",
        "submit_sitemap",
        "delete_site",
        "add_site",
    ] {
        assert_eq!(class("gsc", tool), Class::Write, "{tool}");
    }
    assert!(entry("gsc").unwrap().tools.len() < 64);
}

#[test]
fn a_bare_entry_has_no_template_and_a_minimal_snippet() {
    assert!(secrets_template(&bare()).is_none());
    assert_eq!(
        snippet(&bare()),
        "[[ai.mcp.servers]]\nname = \"bare\"\ncommand = \"x\"\n"
    );
}

#[test]
fn a_preset_fills_the_template() {
    let preset = Entry {
        id: "p",
        title: "P",
        args: &["--a"],
        env: &[
            Var {
                name: "A_PATH",
                ..Var::SECRET
            },
            Var {
                name: "A_SKIP",
                preset: Some("true"),
                ..Var::SECRET
            },
        ],
        ..bare()
    };
    assert_eq!(
        secrets_template(&preset).unwrap(),
        "[p]\nA_PATH = \"\"\nA_SKIP = \"true\"\n"
    );
    assert!(snippet(&preset).contains("args = [\"--a\"]\nenv = [\"A_PATH\", \"A_SKIP\"]\n"));
}

// 2.6 checks

#[test]
fn more_than_64_tools_warn_and_64_do_not() {
    assert_eq!(
        checks::too_many("clarity", 70).unwrap(),
        "clarity: lists 70 tools; only the first 64 are kept"
    );
    assert!(checks::too_many("clarity", 64).is_none());
}

#[test]
fn the_cap_is_the_one_the_client_keeps() {
    assert_eq!(crate::core::ai::tools::servers::MAX_TOOLS, 64);
}

#[test]
fn two_tools_that_collide_after_the_cut_name_the_second_as_dropped() {
    let lines = checks::collisions(
        "google",
        &names(&["get_gmail_thread_content_a", "get_gmail_thread_content_b"]),
    );
    assert_eq!(
        lines,
        [
            "google: get_gmail_thread_content_a and get_gmail_thread_content_b collide after the 24-character cut; get_gmail_thread_content_b is dropped"
        ]
    );
}

#[test]
fn collisions_fold_case_and_symbols_and_distinct_names_are_quiet() {
    assert_eq!(
        checks::collisions("s", &names(&["Run.Report", "run_report"])).len(),
        1
    );
    assert!(checks::collisions("s", &names(&["run_report", "run_funnel"])).is_empty());
}

#[test]
fn tools_the_entry_does_not_know_are_counted() {
    let e = entry("ga4").unwrap();
    assert_eq!(
        checks::unknown("ga4", e, &names(&["run_report", "a", "b"])).unwrap(),
        "ga4: 2 tools not in the catalog (class write)"
    );
    assert!(checks::unknown("ga4", e, &names(&["run_report"])).is_none());
}

#[test]
fn report_joins_the_checks_and_a_plain_server_has_no_catalog_line() {
    let many: Vec<String> = (0..64).map(|i| format!("t{i}")).collect();
    let lines = checks::report("clarity", entry("clarity"), 70, &many);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].contains("lists 70 tools"));
    assert!(lines[1].contains("64 tools not in the catalog"));
    assert!(checks::report("fs", None, 2, &names(&["a", "b"])).is_empty());
}

#[test]
fn ga4_pins_only_its_ten_read_tools() {
    let e = entry("ga4").unwrap();
    assert_eq!(e.tools.len(), 10);
    assert!(e.tools.iter().all(|(_, c)| *c == Class::Read));
    for tool in [
        "list_key_events",
        "run_conversions_report",
        "list_google_ads_links",
    ] {
        assert!(e.tools.iter().any(|(n, _)| *n == tool), "{tool}");
    }
    for tool in [
        "create_property_annotation",
        "create_custom_dimension",
        "create_key_event",
    ] {
        assert!(!e.tools.iter().any(|(n, _)| *n == tool), "{tool}");
    }
}

#[test]
fn a_tool_with_no_usable_name_is_reported_as_dropped() {
    let lines = checks::collisions("s", &names(&["ok", "", "fine"]));
    assert_eq!(lines, ["s: tool with an empty or invalid name dropped"]);
}

#[test]
fn names_from_a_server_are_cleaned_in_the_lines() {
    let long = "x".repeat(24);
    let a = format!("{long}\u{1b}[31ma");
    let b = format!("{long}\u{1b}[31mb");
    let lines = checks::collisions("s", &[a, b]);
    assert_eq!(lines.len(), 1);
    assert!(!lines[0].contains('\u{1b}'));
}

#[test]
fn a_started_probe_says_how_many_tools_and_keeps_every_check_line() {
    let kept = names(&["run_report", "x", "y"]);
    let lines = checks::started("ga4", entry("ga4"), 3, &kept);
    assert_eq!(lines[0], "mcp: started ga4 (3 tools)");
    assert_eq!(
        lines[1..],
        checks::report("ga4", entry("ga4"), 3, &kept)[..]
    );
    assert_eq!(lines.len(), 2, "{lines:?}");
}

#[test]
fn a_failed_probe_names_the_handshake_in_seconds_for_a_timeout_only() {
    assert_eq!(
        checks::failed("clarity", "timed out", 20),
        "mcp: clarity unavailable: timed out after 20 s"
    );
    assert_eq!(
        checks::failed("clarity", "timed out: no token", 20),
        "mcp: clarity unavailable: timed out after 20 s: no token"
    );
    assert_eq!(
        checks::failed("clarity", "could not start clarity", 20),
        "mcp: clarity unavailable: could not start clarity"
    );
}

#[test]
fn every_install_and_login_line_is_one_plain_line() {
    use crate::core::ai::sanitize::is_format;
    for e in entries() {
        let lines = [Some(e.install), e.login.map(|l| l.line)];
        for line in lines.into_iter().flatten() {
            assert!(!line.trim().is_empty(), "{}: empty line", e.id);
            // Typed into a shell and sent with Enter: one line, no control
            // character, no bidi or other format character.
            assert!(
                !line.chars().any(|c| c.is_control() || is_format(c)),
                "{}: {line:?}",
                e.id
            );
        }
    }
}
