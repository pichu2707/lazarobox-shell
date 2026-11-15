//! The Connect Google screen's checks before any write (openspec change
//! `google-connect`, S5): the plan with a dry run of both writes, and the
//! checklist links.

use super::*;
use crate::core::{
    ai::mcp_secrets::{client_json, read_client},
    config_append::digest,
    opener::is_web_url,
};

const ID: &str = "123-abc.apps.googleusercontent.com";

fn client() -> Client {
    read_client(&client_json(ID, "GOCSPX-abc").unwrap()).unwrap()
}

fn nothing_set(_: &str, _: &str) -> bool {
    false
}

fn input<'a>(
    servers: &'a [McpServer],
    has: &'a dyn Fn(&str, &str) -> bool,
    client: &'a Client,
) -> Input<'a> {
    Input {
        servers,
        names: &[],
        has,
        home: Some("/home/ana"),
        client: Some(client),
        client_file: None,
        route: ClientRoute::Own,
        email: "ana@example.com",
        chosen: &SERVICES,
    }
}

const OLD: &str =
    "mouse = false\n\n[[ai.mcp.servers]]\nname = \"docs\"\ncommand = \"/usr/bin/docs\"\n";

#[test]
fn a_preview_carries_the_write_and_what_the_screen_lists() {
    let client = client();
    let shown = preview(&input(&[], &nothing_set, &client), Some(OLD.as_bytes()), "").unwrap();
    assert_eq!(shown.write.names, ["gsc", "google"]);
    assert_eq!(shown.write.expected, Some(digest(OLD.as_bytes())));
    assert_eq!(shown.write.set.len(), 7);
    assert_eq!(
        shown.installs,
        [
            "uv tool install mcp-search-console==0.4.2",
            "uv tool install workspace-mcp==2.4.1"
        ]
    );
    assert!(shown.kept.is_empty() && shown.declared.is_empty());
    assert_eq!(shown.blocked, None);
}

#[test]
fn no_config_file_is_planned_as_a_new_one() {
    let client = client();
    let shown = preview(&input(&[], &nothing_set, &client), None, "").unwrap();
    assert_eq!(shown.write.expected, None);
    assert_eq!(shown.blocked, None);
}

// Refusals show before any write: the append's dry run.
#[test]
fn a_config_the_entries_cannot_follow_is_blocked_and_says_why() {
    let client = client();
    let inline = "[ai.mcp]\nservers = [{ name = \"docs\", command = \"x\" }]\n";
    let shown = preview(
        &input(&[], &nothing_set, &client),
        Some(inline.as_bytes()),
        "",
    )
    .unwrap();
    let blocked = shown.blocked.unwrap();
    assert!(blocked.contains("inline ai.mcp.servers"), "{blocked}");
    assert_eq!(shown.write.set.len(), 7, "y can still save the values");
}

#[test]
fn a_config_that_is_not_utf8_is_blocked_with_its_line() {
    let client = client();
    let bytes = b"mouse = false\n\xff\n";
    let shown = preview(&input(&[], &nothing_set, &client), Some(bytes), "").unwrap();
    assert_eq!(
        shown.blocked.unwrap(),
        "config.toml: not valid TOML (line 2)"
    );
}

// Refusals show before any write: the secrets save's dry run.
#[test]
fn a_secrets_file_the_values_cannot_go_into_refuses_the_plan() {
    let client = client();
    let err = preview(
        &input(&[], &nothing_set, &client),
        None,
        "[gsc]\nGSC_CONFIG_DIR = 3\n",
    )
    .unwrap_err();
    assert_eq!(
        err,
        "connect: secrets.toml: gsc.GSC_CONFIG_DIR: expected a string; edit it by hand"
    );
}

#[test]
fn a_refused_plan_is_its_one_line_notice() {
    let client = client();
    let mut input = input(&[], &nothing_set, &client);
    input.email = "not an e-mail";
    assert_eq!(
        preview(&input, None, "").unwrap_err(),
        Refused::BadEmail.notice()
    );
}

#[test]
fn every_service_declared_is_nothing_to_write() {
    let client = client();
    let server = |name: &str| McpServer {
        name: name.to_owned(),
        command: "x".to_owned(),
        args: Vec::new(),
        env: Vec::new(),
        project: None,
        enabled: true,
    };
    let servers = [server("gsc"), server("google")];
    assert_eq!(
        preview(&input(&servers, &nothing_set, &client), None, "").unwrap_err(),
        "connect: gsc, google are already declared; nothing to write"
    );
}

#[test]
fn kept_values_are_listed_and_not_written() {
    let client = client();
    let has = |table: &str, _: &str| table == "gsc";
    let shown = preview(&input(&[], &has, &client), None, "").unwrap();
    assert_eq!(
        shown.kept,
        [
            ("gsc", "GSC_OAUTH_CLIENT_SECRETS_FILE"),
            ("gsc", "GSC_CONFIG_DIR"),
            ("gsc", "GSC_SKIP_OAUTH")
        ]
    );
    assert!(shown.write.set.iter().all(|(table, ..)| *table == "google"));
}

#[test]
fn the_debug_of_a_preview_never_shows_a_value() {
    let client = client();
    let shown = preview(&input(&[], &nothing_set, &client), None, "").unwrap();
    let debug = format!("{shown:?}");
    for hidden in ["GOCSPX-abc", "ana@example.com"] {
        assert!(!debug.contains(hidden), "{debug}");
    }
}

// Spec: the checklist, one line per step, each with its page.
#[test]
fn the_checklist_links_every_page_of_the_one_time_setup() {
    let links: Vec<&str> = CHECKLIST.iter().map(|(_, link)| *link).collect();
    assert_eq!(
        links,
        [
            "https://console.cloud.google.com/projectcreate",
            "https://console.cloud.google.com/apis/library/searchconsole.googleapis.com",
            "https://console.cloud.google.com/apis/library/gmail.googleapis.com",
            "https://console.cloud.google.com/apis/library/docs.googleapis.com",
            "https://console.cloud.google.com/apis/library/sheets.googleapis.com",
            "https://console.cloud.google.com/apis/library/drive.googleapis.com",
            "https://console.cloud.google.com/auth/audience",
            "https://console.cloud.google.com/apis/credentials",
            super::super::TUTORIAL,
        ]
    );
    assert!(links.iter().all(|link| is_web_url(link)));
    let steps: String = CHECKLIST.iter().map(|(step, _)| *step).collect();
    assert!(steps.contains("Error 403: access_denied"), "{steps}");
    assert!(steps.contains("7 days"), "{steps}");
}
