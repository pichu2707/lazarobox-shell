use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use super::*;
use crate::{
    core::ai::mcp_catalog::checks::Verdict,
    runtime::{
        mcp::external::fake::{Lab, host_env, lab, timeouts},
        proc::process_state,
    },
};

const LONG: Duration = Duration::from_secs(10);

fn pid_of(lab: &Lab, name: &str) -> libc::pid_t {
    let path = format!("{}.pid", lab.calls(name).display());
    // The shell creates the file and then writes the pid: wait for the text.
    let end = Instant::now() + Duration::from_secs(3);
    loop {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if let Ok(pid) = text.trim().parse() {
            return pid;
        }
        assert!(Instant::now() < end, "the server wrote no pid");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn gone(pid: libc::pid_t) -> bool {
    matches!(process_state(pid), None | Some('Z'))
}

/// Blocks (without the async clock) until the server of `name` is stopped.
fn assert_reaped(lab: &Lab, name: &str) {
    let pid = pid_of(lab, name);
    let end = Instant::now() + Duration::from_secs(3);
    while !gone(pid) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(gone(pid), "the server is stopped");
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
    let esc = async {
        while !Path::new(&pid_file).exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    let report = probe(&server, &lab.home, host_env, None, timeouts(LONG), esc).await;
    assert_eq!(report.verdict, Verdict::Stopped);
    assert_eq!(report.lines, ["mcp: clarity probe stopped"]);
    assert_reaped(&lab, "clarity");
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

#[tokio::test]
async fn the_prober_answers_with_one_event_and_esc_stops_it() {
    let lab = lab();
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let mut prober = Prober::new(lab.home.clone(), None, tx);
    prober.start(1, lab.mute("clarity"));
    let pid_file = format!("{}.pid", lab.calls("clarity").display());
    while !Path::new(&pid_file).exists() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
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
    let pid_file = format!("{}.pid", lab.calls("clarity").display());
    while !Path::new(&pid_file).exists() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
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
