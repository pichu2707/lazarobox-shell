use std::time::{Duration, Instant};

use serde_json::json;

use super::{
    fake::{host_env, lab, timeouts},
    *,
};
use crate::runtime::proc::process_state;

const LONG: Duration = Duration::from_secs(10);

async fn never() {
    std::future::pending::<()>().await;
}

fn gone(pid: libc::pid_t) -> bool {
    matches!(process_state(pid), None | Some('Z'))
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
    assert!(notices[0].starts_with("mcp: broken:"), "{notices:?}");
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
    assert!(pids.iter().all(|p| !gone(*p)), "both run");
    // Through an `Arc`, as the host holds it; a call after it is refused.
    let set = std::sync::Arc::new(set);
    set.shutdown().await;
    let after = set.call("one__a", json!({}), never()).await;
    assert!(after.is_error, "{after:?}");
    let end = Instant::now() + LONG;
    while !pids.iter().all(|p| gone(*p)) && Instant::now() < end {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(pids.iter().all(|p| gone(*p)), "both are stopped");
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
