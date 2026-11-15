//! The client route of Connect Google (openspec change
//! `gsc-lazarobox-client`): Search Console alone can use the lazarobox
//! client, and everything else keeps the user's own client as in #736.

use super::*;
use crate::core::ai::mcp_secrets::{client_json, read_client};

const ID: &str = "123-abc.apps.googleusercontent.com";
const CACHE: &str = "/home/ana/.config/lazarobox/lazarobox-google-client.json";
const OWN: &str = "/home/ana/.config/lazarobox/google-oauth-client.json";

fn own() -> Client {
    read_client(&client_json(ID, "GOCSPX-own").unwrap()).unwrap()
}

fn nothing_set(_: &str, _: &str) -> bool {
    false
}

fn input<'a>(
    route: ClientRoute,
    client: Option<&'a Client>,
    chosen: &'a [&'a str],
    servers: &'a [McpServer],
    has: &'a dyn Fn(&str, &str) -> bool,
    cache: &'a str,
) -> Input<'a> {
    Input {
        servers,
        names: &[],
        has,
        home: Some("/home/ana"),
        client,
        client_file: Some(cache),
        route,
        email: "ana@example.com",
        chosen,
    }
}

fn gsc_file(plan: &Plan) -> Option<&str> {
    plan.set
        .iter()
        .find(|s| s.name == "GSC_OAUTH_CLIENT_SECRETS_FILE")
        .map(|s| s.value.expose())
}

// Spec (config-menu, Search Console only): no own client needed, the plan
// writes the absolute cache path and nothing of the own client.
#[test]
fn search_console_alone_on_the_lazarobox_route_needs_no_own_client() {
    for cache in [CACHE, "/x/cfg/lazarobox/lazarobox-google-client.json"] {
        let plan = plan(&input(
            ClientRoute::Lazarobox,
            None,
            &["gsc"],
            &[],
            &nothing_set,
            cache,
        ))
        .unwrap();
        assert_eq!(gsc_file(&plan), Some(cache));
        assert!(plan.set.iter().all(|s| s.table == "gsc"));
        let all = format!(
            "{:?}",
            plan.set
                .iter()
                .map(|s| s.value.expose())
                .collect::<Vec<_>>()
        );
        assert!(!all.contains(ID) && !all.contains("GOCSPX"), "{all}");
    }
}

#[test]
fn a_readable_own_client_does_not_change_the_lazarobox_route() {
    let client = own();
    let plan = plan(&input(
        ClientRoute::Lazarobox,
        Some(&client),
        &["gsc"],
        &[],
        &nothing_set,
        CACHE,
    ))
    .unwrap();
    assert_eq!(gsc_file(&plan), Some(CACHE));
}

#[test]
fn the_own_route_is_as_in_issue_736() {
    let client = own();
    let plan = plan(&input(
        ClientRoute::Own,
        Some(&client),
        &["gsc"],
        &[],
        &nothing_set,
        CACHE,
    ))
    .unwrap();
    assert_eq!(gsc_file(&plan), Some(OWN));
    assert_eq!(plan_without_client().unwrap_err(), Refused::NeedsOwnClient);
}

fn plan_without_client() -> Result<Plan, Refused> {
    plan(&input(
        ClientRoute::Own,
        None,
        &["google"],
        &[],
        &nothing_set,
        CACHE,
    ))
}

// Owner decision 2026-11-12: Workspace always uses the own client.
#[test]
fn workspace_on_the_lazarobox_route_is_refused() {
    let client = own();
    for chosen in [&["google"][..], &SERVICES[..]] {
        let refused = plan(&input(
            ClientRoute::Lazarobox,
            Some(&client),
            chosen,
            &[],
            &nothing_set,
            CACHE,
        ))
        .unwrap_err();
        assert_eq!(refused, Refused::NeedsOwnClient);
        assert!(
            refused.notice().contains("own client"),
            "{}",
            refused.notice()
        );
    }
}

#[test]
fn both_ticked_use_the_own_client_for_both() {
    let client = own();
    let plan = plan(&input(
        ClientRoute::Own,
        Some(&client),
        &SERVICES,
        &[],
        &nothing_set,
        CACHE,
    ))
    .unwrap();
    assert_eq!(gsc_file(&plan), Some(OWN));
    assert!(plan.set.iter().any(|s| s.value.expose() == ID));
}

// Spec (A declared gsc is left alone) and a held value is kept.
#[test]
fn a_declared_gsc_is_left_alone_and_a_held_value_is_kept() {
    let declared = [McpServer {
        name: "gsc".to_owned(),
        command: "x".to_owned(),
        args: Vec::new(),
        env: Vec::new(),
        project: None,
        enabled: true,
    }];
    let plan_declared = plan(&input(
        ClientRoute::Lazarobox,
        None,
        &["gsc"],
        &declared,
        &nothing_set,
        CACHE,
    ))
    .unwrap();
    assert_eq!(plan_declared.declared, ["gsc"]);
    assert!(plan_declared.set.is_empty());
    let held = |table: &str, name: &str| table == "gsc" && name == "GSC_OAUTH_CLIENT_SECRETS_FILE";
    let plan_held = plan(&input(
        ClientRoute::Lazarobox,
        None,
        &["gsc"],
        &[],
        &held,
        CACHE,
    ))
    .unwrap();
    assert_eq!(gsc_file(&plan_held), None);
    assert!(
        plan_held
            .kept
            .contains(&("gsc", "GSC_OAUTH_CLIENT_SECRETS_FILE"))
    );
}
