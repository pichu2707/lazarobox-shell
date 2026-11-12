use std::{
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, Instant},
};

use tokio::sync::oneshot;

use super::*;
use crate::{
    core::ai::{
        mcp_catalog::link::{NO_LINK, Refused, Step},
        tools::mcp::PROTOCOL_VERSION,
    },
    runtime::{
        mcp::external::fake::{Lab, host_env, lab},
        proc::wait_gone_blocking,
    },
};

const ID: &str = "123-abc.apps.googleusercontent.com";
const EMAIL: &str = "me@x.org";
const FILE: &str = "me@x.org.json";

fn ask() -> Ask {
    Ask {
        client_id: ID.to_owned(),
        email: EMAIL.to_owned(),
    }
}

/// Short limits so a test waits in real time, not for 5 minutes.
fn quick() -> Limits {
    Limits {
        handshake: Duration::from_secs(10),
        call: Duration::from_millis(400),
        wait: Duration::from_millis(1500),
        poll: Duration::from_millis(50),
    }
}

fn link() -> String {
    format!(
        "https://accounts.google.com/o/oauth2/auth?client_id={ID}&redirect_uri=http%3A%2F%2Flocalhost%3A8000%2Foauth2callback&state=s"
    )
}

fn answer() -> String {
    format!("**ACTION REQUIRED**\\n   Authorization URL: {}\\n", link())
}

/// A fake `google`: it answers the handshake and the list, then every call
/// with the text `$3` (and records the line in `$4`); `$5` seconds after a
/// call it writes the credentials file `$6` when that is named; with `$7`
/// set it exits after answering; `$8` is a delay before each answer.
const SCRIPT: &str = "echo $$ > \"$4.pid\"; env > \"$4.env\"; \
    read l; printf '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":\"%s\"}}\\n' \"$1\"; \
    read l; read l; \
    printf '{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[%s]}}\\n' \"$2\"; \
    n=3; while read l; do echo \"$l\" >> \"$4\"; sleep \"$8\"; \
    printf '{\"jsonrpc\":\"2.0\",\"id\":%s,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"%s\"}]}}\\n' \"$n\" \"$3\"; \
    if [ -n \"$6\" ]; then (sleep \"$5\"; mkdir -p \"$HOME/.google_workspace_mcp/credentials\"; f=\"$HOME/.google_workspace_mcp/credentials/$6\"; rm -f \"$f\"; echo x > \"$f\") & fi; \
    if [ -n \"$7\" ]; then exit 0; fi; \
    n=$((n+1)); done";

fn google(lab: &Lab, text: &str, writes: &str, exits: bool) -> McpServer {
    slow(lab, text, writes, exits, "0")
}

fn slow(lab: &Lab, text: &str, writes: &str, exits: bool, delay: &str) -> McpServer {
    let mut server = lab.server("google", "search_gmail_messages", text);
    let calls = lab.calls("google");
    server.args = [
        "-c",
        SCRIPT,
        "sh",
        PROTOCOL_VERSION,
        r#"{"name":"search_gmail_messages","inputSchema":{"type":"object"}}"#,
        text,
        calls.to_str().unwrap(),
        "1",
        writes,
        if exits { "x" } else { "" },
        delay,
    ]
    .iter()
    .map(|a| (*a).to_owned())
    .collect();
    server.env = vec![
        "GOOGLE_OAUTH_CLIENT_ID".to_owned(),
        "USER_GOOGLE_EMAIL".to_owned(),
    ];
    server
}

fn creds(lab: &Lab) -> std::path::PathBuf {
    lab.home.join(".google_workspace_mcp/credentials")
}

fn old_file(lab: &Lab) -> std::path::PathBuf {
    std::fs::create_dir_all(creds(lab)).unwrap();
    let path = creds(lab).join(FILE);
    std::fs::write(&path, "old").unwrap();
    let file = std::fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000))
        .unwrap();
    path
}

fn real_stat(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

#[derive(Debug)]
struct Run {
    result: Result<String, String>,
    steps: Vec<Step>,
}

async fn run_with(
    lab: &Lab,
    server: &McpServer,
    limits: Limits,
    cancel_after_link: bool,
    secrets: Option<std::path::PathBuf>,
) -> Run {
    let (events, mut seen) = mpsc::channel(8);
    let (stop, stopped) = oneshot::channel::<()>();
    let cancel = async move {
        if stopped.await.is_err() {
            std::future::pending::<()>().await;
        }
    };
    let mut stop = Some(stop);
    // Reads the steps as they come; the first one may press Esc.
    let collector = tokio::spawn(async move {
        let mut steps = Vec::new();
        while let Some(AppEvent::McpLink(step)) = seen.recv().await {
            steps.push(step);
            if cancel_after_link && let Some(stop) = stop.take() {
                let _ = stop.send(());
            }
        }
        steps
    });
    let ctx = Ctx {
        home: &lab.home,
        secrets,
        events: &events,
        limits,
    };
    let result = session(server, &ask(), ctx, host_env, real_stat, cancel).await;
    drop(events);
    Run {
        result,
        steps: collector.await.unwrap(),
    }
}

async fn run(lab: &Lab, server: &McpServer, limits: Limits) -> Run {
    run_with(lab, server, limits, false, None).await
}

fn pid_of(lab: &Lab) -> libc::pid_t {
    let path = format!("{}.pid", lab.calls("google").display());
    let end = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(pid) = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .trim()
            .parse()
        {
            return pid;
        }
        assert!(Instant::now() < end, "the server wrote no pid");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn assert_reaped(lab: &Lab) {
    assert!(wait_gone_blocking(pid_of(lab)), "the server is stopped");
}

// Spec mcp-connect, google link succeeds.
#[tokio::test]
async fn a_link_that_rewrites_the_credentials_file_succeeds_and_stops_the_server() {
    let lab = lab();
    old_file(&lab);
    let server = google(&lab, &answer(), FILE, false);
    let run = run(&lab, &server, quick()).await;
    assert_eq!(run.result, Ok("linked google".to_owned()));
    assert_eq!(run.steps, [Step::Waiting(link())]);
    assert_reaped(&lab);
    // The call is by the raw name, with the account.
    let sent = lab.received("google");
    assert!(sent.contains(r#""name":"start_google_auth""#), "{sent}");
    assert!(sent.contains(r#""user_google_email":"me@x.org""#), "{sent}");
    assert!(sent.contains(r#""service_name""#), "{sent}");
}

#[tokio::test]
async fn a_first_credentials_file_is_a_success_too() {
    let lab = lab();
    let server = google(&lab, &answer(), FILE, false);
    let run = run(&lab, &server, quick()).await;
    assert_eq!(run.result, Ok("linked google".to_owned()));
    assert_reaped(&lab);
}

// Spec: An old credentials file is not success; google link times out.
#[tokio::test]
async fn an_old_credentials_file_is_not_success_and_the_wait_ends() {
    let lab = lab();
    old_file(&lab);
    let server = google(&lab, &answer(), "", false);
    let run = run(&lab, &server, quick()).await;
    assert_eq!(run.result, Err("link timed out after 5 min".to_owned()));
    assert_eq!(run.steps, [Step::Waiting(link())]);
    assert_reaped(&lab);
}

// Spec: google link cancelled.
#[tokio::test]
async fn esc_while_waiting_cancels_and_stops_the_server() {
    let lab = lab();
    let server = google(&lab, &answer(), "", false);
    let limits = Limits {
        wait: Duration::from_secs(60),
        ..quick()
    };
    let run = run_with(&lab, &server, limits, true, None).await;
    assert_eq!(run.result, Err("link cancelled".to_owned()));
    assert_reaped(&lab);
}

// Spec: start_google_auth does not answer.
#[tokio::test]
async fn a_call_that_does_not_answer_ends_in_30_s_and_stops_the_server() {
    let lab = lab();
    // A hung server: 600 s before it answers the call.
    let server = slow(&lab, &answer(), "", false, "600");
    let run = run(&lab, &server, quick()).await;
    assert_eq!(
        run.result,
        Err("link failed: start_google_auth did not answer in 30 s".to_owned())
    );
    assert!(run.steps.is_empty());
    assert_reaped(&lab);
}

// Spec: Callback ports busy.
#[tokio::test]
async fn an_error_answer_is_a_short_reason_and_no_link_is_opened() {
    let lab = lab();
    let text = "**Error:** Cannot initiate OAuth flow - callback server unavailable (ports 8000-8004 are in use)";
    let server = google(&lab, text, FILE, false);
    let run = run(&lab, &server, quick()).await;
    assert_eq!(
        run.result,
        Err("link failed: Cannot initiate OAuth flow - callback server unavailable (ports 8000-8004 are in use)".to_owned())
    );
    assert!(run.steps.is_empty());
    assert_reaped(&lab);
}

// Spec: Links that are refused.
#[tokio::test]
async fn an_answer_without_an_acceptable_link_is_refused() {
    let lab = lab();
    let evil = answer().replace("accounts.google.com", "evil.example");
    let server = google(&lab, &evil, FILE, false);
    let run = run(&lab, &server, quick()).await;
    assert_eq!(run.result, Err(Refused::Shape.notice().to_owned()));
    assert!(!format!("{run:?}").contains("evil.example"));
    assert!(run.steps.is_empty());
    assert_reaped(&lab);
}

// Spec: the server exiting.
#[tokio::test]
async fn a_server_that_exits_while_waiting_is_a_failure() {
    let lab = lab();
    let server = google(&lab, &answer(), "", true);
    let limits = Limits {
        wait: Duration::from_secs(60),
        ..quick()
    };
    let run = run(&lab, &server, limits).await;
    assert_eq!(
        run.result,
        Err("link failed: the server stopped".to_owned())
    );
    assert_reaped(&lab);
}

// Spec: a server that cannot start is a visible failure.
#[tokio::test]
async fn a_server_that_cannot_start_is_a_failure_with_a_reason() {
    let lab = lab();
    let mut server = google(&lab, &answer(), FILE, false);
    server.command = "/nonexistent/workspace-mcp".to_owned();
    let run = run(&lab, &server, quick()).await;
    let reason = run.result.unwrap_err();
    assert!(
        reason.starts_with("link failed: could not start"),
        "{reason}"
    );
}

// Spec: values exactly as a chat start.
#[tokio::test]
async fn the_server_gets_its_values_from_secrets_toml_as_in_a_chat() {
    let lab = lab();
    let path = lab.home.join("secrets.toml");
    std::fs::write(
        &path,
        format!("[google]\nGOOGLE_OAUTH_CLIENT_ID = \"{ID}\"\nUSER_GOOGLE_EMAIL = \"{EMAIL}\"\nUNLISTED = \"x\"\n"),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = google(&lab, &answer(), FILE, false);
    let run = run_with(&lab, &server, quick(), false, Some(path)).await;
    assert_eq!(run.result, Ok("linked google".to_owned()));
    let env = std::fs::read_to_string(format!("{}.env", lab.calls("google").display())).unwrap();
    assert!(
        env.contains(&format!("GOOGLE_OAUTH_CLIENT_ID={ID}")),
        "{env}"
    );
    assert!(
        env.contains(&format!("HOME={}", lab.home.display())),
        "{env}"
    );
    assert!(!env.contains("UNLISTED"), "{env}");
}

// Spec: Token files never opened.
#[tokio::test]
async fn only_the_account_files_are_statted_and_a_mode_000_file_is_never_opened() {
    let lab = lab();
    let old = old_file(&lab);
    std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o000)).unwrap();
    let seen = std::sync::Mutex::new(Vec::new());
    let (events, mut rx) = mpsc::channel(8);
    let server = google(&lab, &answer(), FILE, false);
    let stat = |p: &Path| {
        seen.lock().unwrap().push(p.to_owned());
        real_stat(p)
    };
    let ctx = Ctx {
        home: &lab.home,
        secrets: None,
        events: &events,
        limits: quick(),
    };
    let result = session(&server, &ask(), ctx, host_env, stat, std::future::pending()).await;
    assert_eq!(result, Ok("linked google".to_owned()));
    assert!(rx.try_recv().is_ok());
    let seen = seen.lock().unwrap();
    assert!(!seen.is_empty());
    assert!(seen.iter().all(|p| p == &old), "{seen:?}");
}

#[test]
fn the_defaults_are_the_specs_limits() {
    let l = Limits::default();
    assert_eq!(
        (
            l.handshake.as_secs(),
            l.call.as_secs(),
            l.wait.as_secs(),
            l.poll.as_secs()
        ),
        (20, 30, 300, 1)
    );
}

fn said(text: &str, is_error: bool) -> Outcome {
    Outcome {
        text: text.to_owned(),
        is_error,
    }
}

// Spec: an error result is `link failed: <short reason>`, sanitized, at most
// 120 characters, never a value of secrets.toml.
#[test]
fn a_reason_is_short_clean_and_holds_no_value() {
    let values = vec!["s3cr3t-value".to_owned()];
    assert_eq!(
        why(&said("ports 8000-8004 are in use", true), &values),
        "link failed: ports 8000-8004 are in use"
    );
    assert_eq!(
        why(
            &said("**Authentication Error:** bad s3cr3t-value here", false),
            &values
        ),
        "link failed: bad [redacted] here"
    );
    let long = format!("**Error:** {}", "x".repeat(300));
    let reason = why(&said(&long, false), &values);
    assert_eq!(reason.chars().count(), "link failed: ".len() + 120);
    assert_eq!(
        why(&said("bad \u{1b}[31mred\u{1b}[0m", true), &values),
        "link failed: bad red"
    );
}

#[test]
fn a_plain_answer_without_a_link_or_an_error_is_no_link() {
    assert_eq!(why(&said("hello\nthere", false), &[]), NO_LINK);
    assert_eq!(why(&said("", true), &[]), NO_LINK);
}

// S6b2 review: while it waits, another account's file is not this link.
#[tokio::test]
async fn a_change_to_another_accounts_file_is_not_success() {
    let lab = lab();
    let server = google(&lab, &answer(), "other@x.org.json", false);
    let run = run(&lab, &server, quick()).await;
    assert_eq!(run.result, Err("link timed out after 5 min".to_owned()));
    assert!(creds(&lab).join("other@x.org.json").exists());
    assert_reaped(&lab);
}
