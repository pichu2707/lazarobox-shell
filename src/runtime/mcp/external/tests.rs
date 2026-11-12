use std::time::Duration;

use serde_json::json;

use super::{
    fake::{host_env, lab, timeouts},
    *,
};
use crate::runtime::proc::{stopped, wait_gone};

const LONG: Duration = Duration::from_secs(10);

async fn never() {
    std::future::pending::<()>().await;
}

#[tokio::test]
async fn every_server_is_started_and_a_call_goes_to_the_one_that_owns_the_tool() {
    let lab = lab();
    let servers = [
        lab.server("alpha", "search", "from alpha"),
        lab.server("beta", "lookup", "from beta"),
    ];
    let refs: Vec<&McpServer> = servers.iter().collect();
    let set = ExternalSet::start(&refs, &lab.home, host_env, timeouts(LONG)).await;
    let names: Vec<String> = set
        .qualified()
        .external()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(names, ["alpha__search", "beta__lookup"]);
    let a = set.call("alpha__search", json!({"q": 1}), never()).await;
    let b = set.call("beta__lookup", json!({}), never()).await;
    assert_eq!((a.text.as_str(), a.is_error), ("from alpha", false));
    assert_eq!((b.text.as_str(), b.is_error), ("from beta", false));
    // The server got the raw name and the arguments, not the qualified name.
    let got = lab.received("alpha");
    assert!(
        got.contains("\"name\":\"search\"") && got.contains("\"q\":1"),
        "{got}"
    );
    assert!(!got.contains("alpha__"), "{got}");
    assert!(set.notices().is_empty());
    set.shutdown().await;
}

#[tokio::test]
async fn a_server_that_cannot_start_is_one_notice_and_the_others_still_work() {
    let lab = lab();
    let mut broken = lab.server("broken", "x", "never");
    broken.command = "/nonexistent/lazarobox-fake".to_owned();
    let servers = [broken, lab.server("fine", "ping", "pong")];
    let refs: Vec<&McpServer> = servers.iter().collect();
    let set = ExternalSet::start(&refs, &lab.home, host_env, timeouts(LONG)).await;
    let notices = set.notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(
        notices[0].starts_with("mcp: broken unavailable: "),
        "{notices:?}"
    );
    let out = set.call("fine__ping", json!({}), never()).await;
    assert_eq!(out.text, "pong");
    let missing = set.call("broken__x", json!({}), never()).await;
    assert!(missing.is_error);
    set.shutdown().await;
}

#[tokio::test]
async fn a_dropped_tool_is_surfaced_as_a_notice() {
    let lab = lab();
    // `x` + `a__b` and `x__a` + `b` both qualify to `x__a__b`.
    let servers = [
        lab.server("x", "a__b", "first"),
        lab.server("x__a", "b", "second"),
    ];
    let refs: Vec<&McpServer> = servers.iter().collect();
    let set = ExternalSet::start(&refs, &lab.home, host_env, timeouts(LONG)).await;
    assert_eq!(set.qualified().external().len(), 1);
    assert_eq!(set.notices(), ["mcp: x__a: b dropped, same name as x/a__b"]);
    let out = set.call("x__a__b", json!({}), never()).await;
    assert_eq!(out.text, "first");
    set.shutdown().await;
}

#[tokio::test]
async fn an_unknown_name_is_an_error_result_and_a_cancel_ends_a_call() {
    let lab = lab();
    let servers = [lab.slow("slow", "wait", "late", "30")];
    let refs: Vec<&McpServer> = servers.iter().collect();
    let set = ExternalSet::start(&refs, &lab.home, host_env, timeouts(LONG)).await;
    let unknown = set.call("slow__nothing", json!({}), never()).await;
    assert!(unknown.is_error, "{unknown:?}");
    let cancelled = set
        .call(
            "slow__wait",
            json!({}),
            tokio::time::sleep(Duration::from_millis(100)),
        )
        .await;
    assert_eq!(cancelled.text, "mcp error: cancelled");
    assert!(cancelled.is_error);
    set.shutdown().await;
}

#[tokio::test]
async fn shutdown_stops_every_server() {
    let lab = lab();
    let servers = [lab.server("one", "a", "x"), lab.server("two", "b", "y")];
    let refs: Vec<&McpServer> = servers.iter().collect();
    let set = ExternalSet::start(&refs, &lab.home, host_env, timeouts(LONG)).await;
    let pids: Vec<libc::pid_t> = ["one", "two"]
        .iter()
        .map(|n| {
            let path = format!("{}.pid", lab.calls(n).display());
            std::fs::read_to_string(path)
                .unwrap()
                .trim()
                .parse()
                .unwrap()
        })
        .collect();
    assert!(pids.iter().all(|p| !stopped(*p)), "both run");
    // Through an `Arc`, as the host holds it; a call after it is refused.
    let set = std::sync::Arc::new(set);
    set.shutdown().await;
    let after = set.call("one__a", json!({}), never()).await;
    assert!(after.is_error, "{after:?}");
    for pid in &pids {
        assert!(wait_gone(*pid).await, "both are stopped");
    }
}

#[tokio::test]
async fn more_servers_than_the_start_cap_all_come_up_in_order() {
    let lab = lab();
    let servers: Vec<McpServer> = (0..12)
        .map(|n| lab.server(&format!("s{n}"), "t", &format!("from {n}")))
        .collect();
    let refs: Vec<&McpServer> = servers.iter().collect();
    let set = ExternalSet::start(&refs, &lab.home, host_env, timeouts(LONG)).await;
    let names: Vec<String> = set
        .qualified()
        .external()
        .into_iter()
        .map(|t| t.name)
        .collect();
    let want: Vec<String> = (0..12).map(|n| format!("s{n}__t")).collect();
    assert_eq!(names, want);
    assert_eq!(set.call("s11__t", json!({}), never()).await.text, "from 11");
    set.shutdown().await;
}

#[tokio::test]
async fn the_chat_is_told_which_servers_started_and_which_did_not() {
    let lab = lab();
    let mut broken = lab.server("broken", "x", "never");
    broken.command = "/nonexistent/lazarobox-fake".to_owned();
    let servers = [broken, lab.server("fine", "ping", "pong")];
    let refs: Vec<&McpServer> = servers.iter().collect();
    let set = ExternalSet::start(&refs, &lab.home, host_env, timeouts(LONG)).await;
    let lines = set.announcements();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0], "mcp: started fine");
    assert!(
        lines[1].starts_with("mcp: broken unavailable: "),
        "{lines:?}"
    );
    set.shutdown().await;
}

/// The host of the `secrets.toml` tests: a `PATH`, plus the pairs given.
fn host_with(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
    move |name| {
        pairs
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| OsString::from(value))
            .or_else(|| host_env(name))
    }
}

fn secrets(text: &str) -> Secrets {
    let loaded = Secrets::parse(text.as_bytes());
    assert_eq!(loaded.problems, Vec::<String>::new());
    loaded.secrets
}

/// Starts `servers` with `secrets` and `host`, and gives each one's recorded
/// environment, in order.
async fn recorded(
    lab: &super::fake::Lab,
    servers: &[McpServer],
    secrets: &Secrets,
    host: impl Fn(&str) -> Option<OsString>,
) -> Vec<Vec<String>> {
    let refs: Vec<&McpServer> = servers.iter().collect();
    let set = ExternalSet::start_with(&refs, &lab.home, host, secrets, timeouts(LONG)).await;
    assert!(set.notices().is_empty(), "{:?}", set.notices());
    set.shutdown().await;
    servers.iter().map(|s| lab.env_of(&s.name)).collect()
}

fn has(env: &[String], line: &str) -> bool {
    env.iter().any(|l| l == line)
}

fn names(env: &[String]) -> Vec<&str> {
    env.iter().filter_map(|l| l.split('=').next()).collect()
}

// Spec mcp-servers, Lifecycle: value from the secrets file.
#[tokio::test]
async fn a_named_variable_gets_its_value_from_the_servers_table() {
    let lab = lab();
    let file = secrets("[clarity]\nCLARITY_API_TOKEN = \"tok\"\nEXTRA = \"e\"\nPATH = \"/evil\"\n");
    let servers = [lab.recorder("clarity", &["CLARITY_API_TOKEN"])];
    let env = recorded(&lab, &servers, &file, host_env).await.remove(0);
    assert!(has(&env, "CLARITY_API_TOKEN=tok"), "{env:?}");
    // Spec mcp-servers, Secrets and privacy: unnamed key is not passed, and
    // the base keeps the host's values.
    assert!(!names(&env).contains(&"EXTRA"), "{env:?}");
    assert!(has(&env, "PATH=/usr/bin:/bin"), "{env:?}");
}

// Spec mcp-servers, Lifecycle: secrets file wins over the host variable.
#[tokio::test]
async fn the_file_value_wins_over_the_host_variable() {
    let lab = lab();
    let file = secrets("[clarity]\nCLARITY_API_TOKEN = \"tok\"\n");
    let servers = [lab.recorder("clarity", &["CLARITY_API_TOKEN"])];
    let host = host_with(&[("CLARITY_API_TOKEN", "host")]);
    let env = recorded(&lab, &servers, &file, host).await.remove(0);
    assert!(has(&env, "CLARITY_API_TOKEN=tok"), "{env:?}");
    assert!(!has(&env, "CLARITY_API_TOKEN=host"), "{env:?}");
}

// Spec mcp-servers, Lifecycle: host variable when the file lacks the name;
// task 1.6, characterization: with no `secrets.toml` it is as before.
#[tokio::test]
async fn a_name_the_table_lacks_and_no_file_at_all_take_the_host_value() {
    let lab = lab();
    let host = host_with(&[("GITHUB_TOKEN", "gh")]);
    let file = secrets("[clarity]\nCLARITY_API_TOKEN = \"tok\"\n");
    let servers = [lab.recorder("clarity", &["GITHUB_TOKEN"])];
    let env = recorded(&lab, &servers, &file, &host).await.remove(0);
    assert!(has(&env, "GITHUB_TOKEN=gh"), "{env:?}");
    let refs: Vec<&McpServer> = servers.iter().collect();
    let set = ExternalSet::start(&refs, &lab.home, &host, timeouts(LONG)).await;
    set.shutdown().await;
    let env = lab.env_of("clarity");
    assert!(has(&env, "GITHUB_TOKEN=gh"), "{env:?}");
    assert!(!names(&env).contains(&"CLARITY_API_TOKEN"), "{env:?}");
}

// Spec mcp-servers, Lifecycle: forbidden names stay out.
#[tokio::test]
async fn loader_home_and_token_names_are_never_taken_from_the_file() {
    let lab = lab();
    let file = secrets(
        "[fake]\nLD_PRELOAD = \"/tmp/evil.so\"\nHOME = \"/not/home\"\nLAZAROBOX_MCP_TOKEN = \"0123\"\n",
    );
    let servers = [lab.recorder("fake", &["LD_PRELOAD", "HOME", "LAZAROBOX_MCP_TOKEN"])];
    let env = recorded(&lab, &servers, &file, host_env).await.remove(0);
    let keys = names(&env);
    assert!(!keys.contains(&"LD_PRELOAD"), "{env:?}");
    assert!(!keys.contains(&"LAZAROBOX_MCP_TOKEN"), "{env:?}");
    assert!(
        has(&env, &format!("HOME={}", lab.home.display())),
        "{env:?}"
    );
}

// Spec mcp-secrets, Format: two tables; spec mcp-servers, Secrets and
// privacy: a value reaches only its own server.
#[tokio::test]
async fn each_server_gets_only_its_own_tables_values() {
    let lab = lab();
    let file = secrets(
        "[clarity]\nCLARITY_API_TOKEN = \"tok\"\n\n[ga4]\nGOOGLE_CLOUD_PROJECT = \"my-project\"\n",
    );
    let servers = [
        lab.recorder("clarity", &["CLARITY_API_TOKEN", "GOOGLE_CLOUD_PROJECT"]),
        lab.recorder("ga4", &["CLARITY_API_TOKEN", "GOOGLE_CLOUD_PROJECT"]),
    ];
    let envs = recorded(&lab, &servers, &file, host_env).await;
    assert!(has(&envs[0], "CLARITY_API_TOKEN=tok"), "{envs:?}");
    assert!(
        !names(&envs[0]).contains(&"GOOGLE_CLOUD_PROJECT"),
        "{envs:?}"
    );
    assert!(has(&envs[1], "GOOGLE_CLOUD_PROJECT=my-project"), "{envs:?}");
    assert!(!names(&envs[1]).contains(&"CLARITY_API_TOKEN"), "{envs:?}");
}

// Spec mcp-catalog, Not linked is refused at chat start; OAuth way without a
// usable GSC_CONFIG_DIR (6b1.5).
const GSC_ENV: [&str; 4] = [
    "GSC_OAUTH_CLIENT_SECRETS_FILE",
    "GSC_CONFIG_DIR",
    "GSC_CREDENTIALS_PATH",
    "GSC_SKIP_OAUTH",
];

fn gsc_values(dir: &str) -> Secrets {
    secrets(&format!(
        "[gsc]\nGSC_OAUTH_CLIENT_SECRETS_FILE = \"/c.json\"\nGSC_CONFIG_DIR = \"{dir}\"\nGSC_SKIP_OAUTH = \"false\"\n"
    ))
}

async fn gsc_start(lab: &super::fake::Lab, name: &str, file: &Secrets) -> ExternalSet {
    let server = lab.recorder(name, &GSC_ENV);
    let set = ExternalSet::start_with(&[&server], &lab.home, host_env, file, timeouts(LONG)).await;
    set.shutdown().await;
    set
}

#[tokio::test]
async fn gsc_with_oauth_values_and_no_token_is_not_spawned_and_says_so_once() {
    let lab = lab();
    let dir = lab.home.join("mcp-gsc");
    let set = gsc_start(&lab, "gsc", &gsc_values(dir.to_str().unwrap())).await;
    assert_eq!(
        set.notices(),
        ["mcp: gsc unavailable: not linked; open Connectors and use Link with Google"]
    );
    assert!(lab.env_of("gsc").is_empty(), "the server was spawned");
}

#[tokio::test]
async fn gsc_with_a_token_starts_normally() {
    let lab = lab();
    let dir = lab.home.join("mcp-gsc");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("token.json"), "{}").unwrap();
    let set = gsc_start(&lab, "gsc", &gsc_values(dir.to_str().unwrap())).await;
    assert!(set.notices().is_empty(), "{:?}", set.notices());
    assert!(has(
        &lab.env_of("gsc"),
        &format!("GSC_CONFIG_DIR={}", dir.display())
    ));
}

#[tokio::test]
async fn gsc_without_a_usable_config_dir_is_not_spawned() {
    let lab = lab();
    for dir in ["", "~/.config/mcp-gsc", "relative"] {
        let set = gsc_start(&lab, "gsc", &gsc_values(dir)).await;
        assert_eq!(
            set.notices(),
            ["mcp: gsc unavailable: fill the OAuth values in Connectors (f)"],
            "{dir:?}"
        );
        assert!(lab.env_of("gsc").is_empty(), "{dir:?}");
    }
}

#[tokio::test]
async fn the_service_account_way_and_another_name_start_without_a_token() {
    let lab = lab();
    let key = secrets("[gsc]\nGSC_CREDENTIALS_PATH = \"/k.json\"\nGSC_SKIP_OAUTH = \"true\"\n");
    let set = gsc_start(&lab, "gsc", &key).await;
    assert!(set.notices().is_empty(), "{:?}", set.notices());
    assert!(!lab.env_of("gsc").is_empty());
    // Another name gets neither the hidden tool nor the check (documented).
    let other = secrets(
        "[search]\nGSC_OAUTH_CLIENT_SECRETS_FILE = \"/c.json\"\nGSC_SKIP_OAUTH = \"false\"\n",
    );
    let set = gsc_start(&lab, "search", &other).await;
    assert!(set.notices().is_empty(), "{:?}", set.notices());
    assert!(!lab.env_of("search").is_empty());
}
