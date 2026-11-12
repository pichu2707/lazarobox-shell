use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use super::*;
use crate::{
    core::ai::mcp_catalog::{
        checks::Verdict,
        link::{Ask, Step},
    },
    runtime::{
        mcp::external::fake::{Lab, host_env, lab, timeouts},
        proc::wait_gone_blocking,
    },
};

const LONG: Duration = Duration::from_secs(10);

fn pid_of(lab: &Lab, name: &str) -> libc::pid_t {
    read_pid(&format!("{}.pid", lab.calls(name).display()))
}

/// The pid in `path`. The shell creates the file and then writes the pid: wait
/// for the text, not for the file.
fn read_pid(path: &str) -> libc::pid_t {
    let end = Instant::now() + Duration::from_secs(3);
    loop {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        if let Ok(pid) = text.trim().parse() {
            return pid;
        }
        assert!(Instant::now() < end, "the server wrote no pid");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Waits (on the async clock) until the shell has written its pid.
async fn wait_for_pid(path: &str) {
    while std::fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Blocks (without the async clock) until the server of `name` is stopped.
fn assert_reaped(lab: &Lab, name: &str) {
    let pid = pid_of(lab, name);
    assert!(wait_gone_blocking(pid), "the server is stopped");
}

async fn never() {
    std::future::pending::<()>().await;
}

fn secrets_file(lab: &Lab, text: &str, mode: u32) -> PathBuf {
    let path = lab.home.join("secrets.toml");
    std::fs::write(&path, text).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    path
}

async fn run(lab: &Lab, server: &McpServer, secrets: Option<&Path>) -> Probed {
    probe(
        server,
        &lab.home,
        host_env,
        secrets.map(Path::to_path_buf),
        timeouts(LONG),
        never(),
    )
    .await
}

#[tokio::test]
async fn a_server_that_answers_is_reported_and_stopped() {
    let lab = lab();
    let server = lab.listing("clarity", &["a", "b", "c"], "ok", "0");
    let report = run(&lab, &server, None).await;
    assert_eq!(report.verdict, Verdict::Started);
    assert!(report.lines[0].starts_with("mcp: started clarity (3 tools)"));
    assert!(report.problems.is_empty());
    assert_reaped(&lab, "clarity");
}

#[tokio::test]
async fn more_than_64_tools_is_one_more_line() {
    let lab = lab();
    let names: Vec<String> = (0..70).map(|i| format!("t{i}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let server = lab.listing("clarity", &refs, "ok", "0");
    let report = run(&lab, &server, None).await;
    assert_eq!(report.lines[0], "mcp: started clarity (64 tools)");
    // Every line of the checks reaches the user.
    let checks = checks::report("clarity", entry("clarity"), 70, &names[..64]);
    assert!(checks.len() >= 2, "{checks:?}");
    assert_eq!(report.lines[1..], checks[..]);
    assert_reaped(&lab, "clarity");
}

#[tokio::test(start_paused = true)]
async fn a_server_that_never_answers_times_out_after_20_seconds() {
    let lab = lab();
    let server = lab.mute("clarity");
    let report = probe(
        &server,
        &lab.home,
        host_env,
        None,
        Timeouts::default(),
        never(),
    )
    .await;
    assert_eq!(report.verdict, Verdict::Failed);
    assert_eq!(
        report.lines,
        ["mcp: clarity unavailable: timed out after 20 s"]
    );
    // The paused clock runs the 20 s out before the shell has written its
    // pid, so the reaping is checked by the Esc test and the success test.
}

#[tokio::test]
async fn esc_stops_the_probe_and_the_server_goes() {
    let lab = lab();
    let server = lab.mute("clarity");
    let pid_file = format!("{}.pid", lab.calls("clarity").display());
    // Esc comes once the shell has WRITTEN its pid (the file exists a moment
    // before): a stop before that leaves nothing to check.
    let esc = async {
        while std::fs::read_to_string(&pid_file)
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    let report = probe(&server, &lab.home, host_env, None, timeouts(LONG), esc).await;
    assert_eq!(report.verdict, Verdict::Stopped);
    assert_eq!(report.lines, ["mcp: clarity probe stopped"]);
    assert_reaped(&lab, "clarity");
}

/// The group guard is armed by `spawn` itself, before any await: a client
/// dropped right after it (Esc before the handshake even started) takes the
/// leader AND what the leader started with it.
#[tokio::test]
async fn a_client_dropped_right_after_spawn_kills_the_whole_group() {
    let lab = lab();
    let mut server = lab.mute("clarity");
    server.args[1] = "sleep 600 & echo $! > \"$4.child\"; echo $$ > \"$4.pid\"; wait".to_owned();
    let client =
        crate::runtime::mcp::client::McpClient::spawn(&server, &lab.home, host_env).unwrap();
    let base = lab.calls("clarity").display().to_string();
    wait_for_pid(&format!("{base}.pid")).await;
    wait_for_pid(&format!("{base}.child")).await;
    drop(client);
    let child = read_pid(&format!("{base}.child"));
    assert!(
        wait_gone_blocking(pid_of(&lab, "clarity")),
        "the leader is stopped"
    );
    assert!(
        wait_gone_blocking(child),
        "what the leader started is stopped"
    );
}

#[tokio::test]
async fn secrets_toml_values_reach_the_child_and_never_the_report() {
    let lab = lab();
    let server = lab.recorder("clarity", &["CLARITY_API_TOKEN"]);
    let path = secrets_file(&lab, "[clarity]\nCLARITY_API_TOKEN = \"sk-123\"\n", 0o600);
    let report = run(&lab, &server, Some(&path)).await;
    assert_eq!(report.verdict, Verdict::Started);
    assert!(
        lab.env_of("clarity")
            .contains(&"CLARITY_API_TOKEN=sk-123".to_owned())
    );
    assert!(!format!("{report:?}").contains("sk-123"));
    assert_reaped(&lab, "clarity");
}

// A tool name is server text: one equal to a value is redacted like the rest
// (collision lines name two tools).
#[tokio::test]
async fn a_tool_name_that_is_a_secret_value_is_redacted_in_every_line() {
    let lab = lab();
    let value = "SECRETSECRETSECRETSECRETSECRET";
    let both = [value, &format!("{value}_more")];
    let mut server = lab.listing("clarity", &both, "ok", "0");
    server.env = vec!["CLARITY_API_TOKEN".to_owned()];
    let path = secrets_file(
        &lab,
        &format!("[clarity]\nCLARITY_API_TOKEN = \"{value}\"\n"),
        0o600,
    );
    let report = run(&lab, &server, Some(&path)).await;
    assert_eq!(report.verdict, Verdict::Started);
    assert!(
        report.lines.iter().any(|l| l.contains("collide")),
        "{report:?}"
    );
    assert!(!format!("{report:?}").contains(value), "{report:?}");
    assert_reaped(&lab, "clarity");
}

#[tokio::test]
async fn the_prober_answers_with_one_event_and_esc_stops_it() {
    let lab = lab();
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let mut prober = Prober::new(lab.home.clone(), None, tx);
    prober.start(1, lab.mute("clarity"));
    wait_for_pid(&format!("{}.pid", lab.calls("clarity").display())).await;
    prober.cancel();
    let Some(AppEvent::McpProbe(1, report)) = rx.recv().await else {
        panic!("one McpProbe event");
    };
    assert_eq!(report.verdict, Verdict::Stopped);
    assert_reaped(&lab, "clarity");
}

#[tokio::test]
async fn a_new_probe_replaces_the_old_one_which_reports_nothing() {
    let lab = lab();
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let mut prober = Prober::new(lab.home.clone(), None, tx);
    prober.start(1, lab.mute("clarity"));
    wait_for_pid(&format!("{}.pid", lab.calls("clarity").display())).await;
    prober.start(2, lab.listing("ga4", &["a"], "ok", "0"));
    let Some(AppEvent::McpProbe(id, report)) = rx.recv().await else {
        panic!("one McpProbe event");
    };
    assert_eq!((id, report.verdict), (2, Verdict::Started));
    assert_reaped(&lab, "clarity");
    assert!(rx.try_recv().is_err(), "the replaced probe says nothing");
}

#[tokio::test]
async fn a_probe_that_panics_still_reports_a_failure() {
    let report = guarded("clarity", async { panic!("boom") }).await;
    assert_eq!(report.verdict, Verdict::Failed);
    assert_eq!(report.lines, ["mcp: clarity unavailable: the probe failed"]);
    assert!(!format!("{report:?}").contains("boom"));
}

#[tokio::test]
async fn a_secrets_problem_is_in_the_report_without_a_value() {
    let lab = lab();
    let server = lab.recorder("clarity", &["CLARITY_API_TOKEN"]);
    let path = secrets_file(&lab, "[clarity]\nCLARITY_API_TOKEN = \"sk-123\"\n", 0o644);
    let report = run(&lab, &server, Some(&path)).await;
    assert_eq!(report.problems.len(), 1, "{report:?}");
    assert!(!format!("{report:?}").contains("sk-123"));
    assert!(
        !lab.env_of("clarity")
            .contains(&"CLARITY_API_TOKEN=sk-123".to_owned()),
        "a file with a wrong mode is not used"
    );
    assert_reaped(&lab, "clarity");
}

// Spec mcp-catalog, Chat start without a token: the probe is refused too.
#[tokio::test]
async fn a_gsc_probe_without_a_token_is_refused_with_a_notice_and_no_spawn() {
    let lab = lab();
    let dir = lab.home.join(".config/mcp-gsc");
    let file = secrets_file(
        &lab,
        &format!(
            "[gsc]\nGSC_OAUTH_CLIENT_SECRETS_FILE = \"/c.json\"\nGSC_CONFIG_DIR = \"{}\"\nGSC_SKIP_OAUTH = \"false\"\n",
            dir.display()
        ),
        0o600,
    );
    let mut server = lab.recorder(
        "gsc",
        &[
            "GSC_OAUTH_CLIENT_SECRETS_FILE",
            "GSC_CONFIG_DIR",
            "GSC_SKIP_OAUTH",
        ],
    );
    server.name = "gsc".to_owned();
    let report = run(&lab, &server, Some(&file)).await;
    assert_eq!(report.verdict, Verdict::Failed);
    assert_eq!(
        report.lines,
        ["mcp: gsc unavailable: not linked; open Connectors and use Link with Google"]
    );
    assert!(lab.env_of("gsc").is_empty(), "the server was spawned");
}

fn link_answer() -> String {
    "https://accounts.google.com/o/oauth2/auth?client_id=the-id&redirect_uri=http%3A%2F%2Flocalhost%3A8000%2F"
        .to_owned()
}

fn ask() -> Ask {
    Ask {
        client_id: "the-id".to_owned(),
        email: "me@x.org".to_owned(),
    }
}

// S6b2: the link session takes the slot of the probe; Esc stops it.
#[tokio::test]
async fn a_link_says_its_link_then_its_end_and_esc_stops_it() {
    let lab = lab();
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let mut prober = Prober::new(lab.home.clone(), None, tx);
    prober.link(lab.server("google", "t", &link_answer()), ask());
    let Some(AppEvent::McpLink(Step::Waiting(url))) = rx.recv().await else {
        panic!("the link first");
    };
    assert_eq!(url, link_answer());
    prober.cancel();
    let Some(AppEvent::McpLink(Step::Ended(end))) = rx.recv().await else {
        panic!("then its end");
    };
    assert_eq!(end, Err("link cancelled".to_owned()));
    assert_reaped(&lab, "google");
}

#[tokio::test]
async fn a_probe_replaces_a_link_which_reports_nothing() {
    let lab = lab();
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let mut prober = Prober::new(lab.home.clone(), None, tx);
    prober.link(lab.server("google", "t", &link_answer()), ask());
    let Some(AppEvent::McpLink(Step::Waiting(_))) = rx.recv().await else {
        panic!("the link first");
    };
    prober.start(2, lab.listing("ga4", &["a"], "ok", "0"));
    let Some(AppEvent::McpProbe(2, report)) = rx.recv().await else {
        panic!("the probe answers");
    };
    assert_eq!(report.verdict, Verdict::Started);
    assert_reaped(&lab, "google");
    assert!(rx.try_recv().is_err(), "the replaced link says nothing");
}
