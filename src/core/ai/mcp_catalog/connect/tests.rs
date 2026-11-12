use super::*;
use crate::core::ai::mcp_secrets::{client_json, read_client};

const ID: &str = "123-abc.apps.googleusercontent.com";
const HOME: &str = "/home/ana";

fn client() -> Client {
    read_client(&client_json(ID, "GOCSPX-abc").unwrap()).unwrap()
}

fn server(name: &str, enabled: bool) -> McpServer {
    McpServer {
        name: name.to_owned(),
        command: "something-else".to_owned(),
        args: Vec::new(),
        env: Vec::new(),
        project: None,
        enabled,
    }
}

fn nothing_set(_: &str, _: &str) -> bool {
    false
}

fn input<'a>(
    servers: &'a [McpServer],
    has: &'a dyn Fn(&str, &str) -> bool,
    client: &'a Client,
    chosen: &'a [&'a str],
) -> Input<'a> {
    Input {
        servers,
        names: &[],
        has,
        home: Some(HOME),
        client: Some(client),
        client_file: None,
        route: ClientRoute::Own,
        email: "ana@example.com",
        chosen,
    }
}

/// The plan's values as (table, name, value) for the assertions.
fn values(plan: &Plan) -> Vec<(&str, &str, &str)> {
    plan.set
        .iter()
        .map(|s| (s.table, s.name, s.value.expose()))
        .collect()
}

#[test]
fn with_nothing_declared_both_services_are_appended_filled_and_installed() {
    let client = client();
    let plan = plan(&input(&[], &nothing_set, &client, &SERVICES)).unwrap();
    let ids: Vec<&str> = plan.append.iter().map(|e| e.id).collect();
    assert_eq!(ids, ["gsc", "google"]);
    assert_eq!(
        values(&plan),
        [
            (
                "gsc",
                "GSC_OAUTH_CLIENT_SECRETS_FILE",
                "/home/ana/.config/lazarobox/google-oauth-client.json"
            ),
            ("gsc", "GSC_CONFIG_DIR", "/home/ana/.config/mcp-gsc"),
            ("gsc", "GSC_SKIP_OAUTH", "false"),
            ("google", "GOOGLE_OAUTH_CLIENT_ID", ID),
            ("google", "GOOGLE_OAUTH_CLIENT_SECRET", "GOCSPX-abc"),
            ("google", "USER_GOOGLE_EMAIL", "ana@example.com"),
            ("google", "WORKSPACE_MCP_DISABLE_LOCAL_FILES", "true"),
        ]
    );
    assert!(plan.kept.is_empty());
    assert!(plan.declared.is_empty());
    assert_eq!(
        plan.installs,
        [
            "uv tool install mcp-search-console==0.4.2",
            "uv tool install workspace-mcp==2.4.1"
        ]
    );
}

#[test]
fn the_appended_text_is_the_catalog_entries_in_order() {
    let client = client();
    let plan = plan(&input(&[], &nothing_set, &client, &SERVICES)).unwrap();
    let gsc = snippet(entry("gsc").unwrap());
    let google = snippet(entry("google").unwrap());
    assert_eq!(plan.snippets(), format!("{gsc}\n{google}"));
}

#[test]
fn a_declared_service_is_left_alone_even_when_disabled() {
    let client = client();
    let servers = [server("gsc", false)];
    let plan = plan(&input(&servers, &nothing_set, &client, &SERVICES)).unwrap();
    let ids: Vec<&str> = plan.append.iter().map(|e| e.id).collect();
    assert_eq!(ids, ["google"]);
    assert!(plan.set.iter().all(|s| s.table == "google"));
    assert_eq!(plan.declared, ["gsc"]);
    assert_eq!(plan.installs, ["uv tool install workspace-mcp==2.4.1"]);
}

#[test]
fn with_every_service_declared_the_plan_does_nothing() {
    let client = client();
    let servers = [server("google", true), server("gsc", true)];
    let plan = plan(&input(&servers, &nothing_set, &client, &SERVICES)).unwrap();
    assert!(plan.append.is_empty() && plan.set.is_empty() && plan.installs.is_empty());
    assert_eq!(plan.declared, ["gsc", "google"]);
    assert_eq!(plan.snippets(), "");
}

#[test]
fn a_value_that_is_already_set_is_kept() {
    let client = client();
    let has = |table: &str, name: &str| table == "google" && name == "USER_GOOGLE_EMAIL";
    let plan = plan(&input(&[], &has, &client, &["google"])).unwrap();
    assert_eq!(plan.kept, [("google", "USER_GOOGLE_EMAIL")]);
    assert!(plan.set.iter().all(|s| s.name != "USER_GOOGLE_EMAIL"));
}

#[test]
fn search_console_alone_needs_no_e_mail() {
    let client = client();
    let mut input = input(&[], &nothing_set, &client, &["gsc"]);
    input.email = "";
    let plan = plan(&input).unwrap();
    assert!(plan.set.iter().all(|s| s.table == "gsc"));
}

#[test]
fn workspace_with_an_e_mail_that_is_not_plain_is_refused() {
    let client = client();
    for email in ["", "ana", "ana @example.com", "ana@exa\u{7}mple.com"] {
        let mut input = input(&[], &nothing_set, &client, &["google"]);
        input.email = email;
        assert_eq!(plan(&input).unwrap_err(), Refused::BadEmail, "{email:?}");
    }
}

#[test]
fn an_e_mail_is_not_asked_for_a_workspace_already_declared() {
    let client = client();
    let servers = [server("google", true)];
    let mut input = input(&servers, &nothing_set, &client, &["google"]);
    input.email = "";
    assert!(plan(&input).is_ok());
}

#[test]
fn nothing_chosen_or_a_service_outside_connect_google_is_refused() {
    let client = client();
    let none: [&str; 0] = [];
    assert_eq!(
        plan(&input(&[], &nothing_set, &client, &none)).unwrap_err(),
        Refused::NothingChosen
    );
    assert_eq!(
        plan(&input(&[], &nothing_set, &client, &["ga4"])).unwrap_err(),
        Refused::NotAService("ga4".to_owned())
    );
}

#[test]
fn search_console_without_an_absolute_home_is_refused() {
    let client = client();
    for home in [None, Some("relative/home")] {
        let mut input = input(&[], &nothing_set, &client, &["gsc"]);
        input.home = home;
        assert_eq!(plan(&input).unwrap_err(), Refused::NoHome, "{home:?}");
    }
}

#[test]
fn each_refusal_has_one_line_for_the_footer() {
    let notices = [
        Refused::NothingChosen.notice(),
        Refused::NotAService("ga4".to_owned()).notice(),
        Refused::BadEmail.notice(),
        Refused::NoHome.notice(),
        Refused::Uncovered {
            table: "google",
            name: "X",
        }
        .notice(),
    ];
    assert_eq!(
        notices,
        [
            "choose Search Console, Google Workspace or both",
            "ga4 is not connected by Connect Google",
            "type your Google e-mail address (name@domain, no spaces)",
            "no absolute home folder for the Search Console token",
            "google.X has no value Connect Google can fill; report this",
        ]
    );
}

#[test]
fn the_debug_of_a_plan_never_shows_a_value() {
    let client = client();
    let plan = plan(&input(&[], &nothing_set, &client, &SERVICES)).unwrap();
    let debug = format!("{plan:?}");
    for hidden in ["GOCSPX-abc", "ana@example.com", "/home/ana"] {
        assert!(!debug.contains(hidden), "{debug}");
    }
}
