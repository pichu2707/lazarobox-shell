use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use super::*;
use crate::core::ai::tools::mcp::PROTOCOL_VERSION;
use crate::runtime::proc::process_state;

/// Polling returns as soon as the condition holds, so a generous bound costs
/// nothing on a pass and keeps a loaded runner from failing the tests.
const BOUND: Duration = Duration::from_secs(20);

struct Lab {
    home: PathBuf,
    _root: tempfile::TempDir,
}

fn lab() -> Lab {
    let root = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let home = fs::canonicalize(root.path()).unwrap().join("home");
    fs::create_dir_all(&home).unwrap();
    Lab { home, _root: root }
}

impl Lab {
    fn file(&self, name: &str) -> PathBuf {
        self.home.join(name)
    }
}

/// `sh -c script sh args...`: the arguments are `$1`, `$2`, ...
fn sh(script: &str, args: &[&str]) -> McpServer {
    let mut argv = vec!["-c".to_owned(), script.to_owned(), "sh".to_owned()];
    argv.extend(args.iter().map(|a| (*a).to_owned()));
    McpServer {
        name: "fake".to_owned(),
        command: "/bin/sh".to_owned(),
        args: argv,
        env: Vec::new(),
        project: None,
        enabled: true,
    }
}

fn timeouts() -> Timeouts {
    Timeouts {
        handshake: Duration::from_secs(5),
        call: Duration::from_secs(5),
    }
}

fn host_env(name: &str) -> Option<OsString> {
    let value = match name {
        "PATH" => "/usr/bin:/bin",
        "HOME" => "/not/the/home",
        "USER" => "someone",
        "LANG" => "C",
        "LC_ALL" => "C",
        "TMPDIR" => "/tmp",
        "NAMED" => "named-value",
        "UNNAMED" => "unnamed-value",
        "LAZAROBOX_MCP_TOKEN" => "0123456789abcdef",
        _ => return None,
    };
    Some(OsString::from(value))
}

async fn appears(path: &Path) -> String {
    let end = Instant::now() + BOUND;
    loop {
        if let Ok(text) = fs::read_to_string(path)
            && !text.is_empty()
        {
            return text;
        }
        assert!(Instant::now() < end, "{} never appeared", path.display());
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn gone(pid: libc::pid_t) -> bool {
    matches!(process_state(pid), None | Some('Z'))
}

async fn wait_gone(pid: libc::pid_t) -> bool {
    let end = Instant::now() + BOUND;
    while !gone(pid) && Instant::now() < end {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    gone(pid)
}

#[tokio::test]
async fn only_the_named_variables_and_the_base_reach_the_server() {
    let lab = lab();
    let out = lab.file("env");
    let mut server = sh(
        "env > \"$1.tmp\" && mv \"$1.tmp\" \"$1\"; cat",
        &[out.to_str().unwrap()],
    );
    server.env = vec![
        "NAMED".to_owned(),
        "LAZAROBOX_MCP_TOKEN".to_owned(),
        "NOT_SET_ON_THE_HOST".to_owned(),
    ];
    let client = McpClient::spawn(&server, &lab.home, host_env).expect("spawns");
    let text = appears(&out).await;
    client.shutdown().await;
    let keys: Vec<&str> = text.lines().filter_map(|l| l.split('=').next()).collect();
    for want in ["PATH", "HOME", "USER", "LANG", "LC_ALL", "TMPDIR", "NAMED"] {
        assert!(keys.contains(&want), "{want} is passed: {keys:?}");
    }
    assert!(text.lines().any(|l| l == "NAMED=named-value"));
    // The shell adds a few of its own.
    let shell = ["PWD", "OLDPWD", "SHLVL", "_"];
    for key in &keys {
        assert!(
            ["PATH", "HOME", "USER", "LANG", "LC_ALL", "TMPDIR", "NAMED"].contains(key)
                || shell.contains(key),
            "{key} must not reach the server"
        );
    }
    assert!(!text.contains("unnamed-value") && !text.contains("0123456789abcdef"));
    // HOME is the one we resolved, not the host's.
    let home = format!("HOME={}", lab.home.display());
    assert!(text.lines().any(|l| l == home), "{text}");
}

#[tokio::test]
async fn a_variable_the_host_lacks_is_left_out_not_set_empty() {
    let lab = lab();
    let out = lab.file("env");
    let mut server = sh(
        "env > \"$1.tmp\" && mv \"$1.tmp\" \"$1\"; cat",
        &[out.to_str().unwrap()],
    );
    server.env = vec!["NOT_SET_ON_THE_HOST".to_owned()];
    let client = McpClient::spawn(&server, &lab.home, |_| None).expect("spawns");
    let text = appears(&out).await;
    client.shutdown().await;
    assert!(!text.contains("NOT_SET_ON_THE_HOST"), "{text}");
    assert!(!text.contains("PATH="), "no host base to pass: {text}");
}

#[tokio::test]
async fn an_argument_with_shell_syntax_stays_one_literal_argument() {
    let lab = lab();
    let out = lab.file("arg");
    let evil = "a; touch PWNED $(touch PWNED2)";
    let server = sh(
        "printf %s \"$1\" > \"$2.tmp\" && mv \"$2.tmp\" \"$2\"; cat",
        &[evil, out.to_str().unwrap()],
    );
    let client = McpClient::spawn(&server, &lab.home, host_env).expect("spawns");
    assert_eq!(appears(&out).await, evil);
    client.shutdown().await;
    assert!(!lab.file("PWNED").exists() && !lab.file("PWNED2").exists());
}

#[tokio::test]
async fn the_server_starts_in_the_home_directory() {
    let lab = lab();
    let out = lab.file("cwd");
    let server = sh(
        "pwd -P > \"$1.tmp\" && mv \"$1.tmp\" \"$1\"; cat",
        &[out.to_str().unwrap()],
    );
    let client = McpClient::spawn(&server, &lab.home, host_env).expect("spawns");
    let seen = appears(&out).await;
    client.shutdown().await;
    assert_eq!(Path::new(seen.trim()), lab.home);
}

#[tokio::test]
async fn a_program_that_cannot_start_is_one_unavailable_reason() {
    let lab = lab();
    let mut server = sh("", &[]);
    server.command = "/nonexistent/lazarobox-mcp".to_owned();
    let reason = McpClient::start(&server, &lab.home, host_env, timeouts())
        .await
        .err()
        .expect("unavailable");
    assert_eq!(reason, "could not start fake");
}

#[tokio::test]
async fn a_server_that_dies_at_once_is_an_error_not_a_hang() {
    let lab = lab();
    let server = sh("echo 'cannot open the database' >&2; exit 3", &[]);
    let reason = tokio::time::timeout(
        BOUND,
        McpClient::start(&server, &lab.home, host_env, timeouts()),
    )
    .await
    .expect("no hang")
    .err()
    .expect("unavailable");
    assert!(reason.contains("server closed the connection"), "{reason}");
    assert!(reason.contains("cannot open the database"), "{reason}");
}

#[tokio::test]
async fn the_stderr_reason_is_sanitized_and_capped() {
    let lab = lab();
    let server = sh(
        "printf 'old line\\n\\033[31mred\\033[0m \\033]0;evil\\007%s\\n' \"$(head -c 3000 /dev/zero | tr '\\0' a)\" >&2; exit 1",
        &[],
    );
    let reason = McpClient::start(&server, &lab.home, host_env, timeouts())
        .await
        .err()
        .expect("unavailable");
    assert!(!reason.chars().any(char::is_control), "{reason:?}");
    assert!(reason.contains("red"), "{reason}");
    assert!(!reason.contains("old line"), "only the last line");
    assert!(reason.len() < 300, "capped: {}", reason.len());
}

#[tokio::test]
async fn a_server_that_never_answers_is_killed_and_reported() {
    let lab = lab();
    let pid = lab.file("pid");
    let server = sh(
        "echo $$ > \"$1.tmp\" && mv \"$1.tmp\" \"$1\"; sleep 300",
        &[pid.to_str().unwrap()],
    );
    let quick = Timeouts {
        handshake: Duration::from_millis(300),
        call: Duration::from_secs(5),
    };
    let reason = McpClient::start(&server, &lab.home, host_env, quick)
        .await
        .err()
        .expect("unavailable");
    assert!(reason.starts_with("timed out"), "{reason}");
    let pid: libc::pid_t = appears(&pid).await.trim().parse().unwrap();
    assert!(wait_gone(pid).await, "the silent server is killed");
}

/// A server that answers `initialize`, then reads until its stdin closes.
const POLITE: &str = "read line; printf '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":\"%s\"}}\\n' \"$1\"; read line; cat";

#[tokio::test]
async fn start_returns_a_client_that_finished_the_handshake() {
    let lab = lab();
    let server = sh(&format!("{POLITE}\n"), &[PROTOCOL_VERSION]);
    let client = McpClient::start(&server, &lab.home, host_env, timeouts())
        .await
        .expect("handshake done");
    client.shutdown().await;
}

/// A leader with a background grandchild, whose pids it records.
const FAMILY: &str = "sleep 300 & echo $! > \"$1.g\"; echo $$ > \"$1.l\"; wait";

async fn family(lab: &Lab) -> (McpClient, libc::pid_t, libc::pid_t) {
    let base = lab.file("fam");
    let server = sh(FAMILY, &[base.to_str().unwrap()]);
    let client = McpClient::spawn(&server, &lab.home, host_env).expect("spawns");
    let grand = appears(&lab.file("fam.g")).await.trim().parse().unwrap();
    let leader = appears(&lab.file("fam.l")).await.trim().parse().unwrap();
    (client, leader, grand)
}

#[tokio::test]
async fn shutdown_leaves_no_orphan_grandchild() {
    let lab = lab();
    let (client, leader, grand) = family(&lab).await;
    assert!(!gone(leader) && !gone(grand), "the family runs");
    client.shutdown().await;
    assert!(wait_gone(leader).await, "the leader is reaped");
    assert!(wait_gone(grand).await, "the grandchild dies with the group");
}

#[tokio::test]
async fn dropping_the_client_leaves_no_orphan_grandchild() {
    let lab = lab();
    let (client, leader, grand) = family(&lab).await;
    drop(client);
    assert!(wait_gone(leader).await);
    assert!(wait_gone(grand).await);
}
