//! The link session of `google` (S6b2).

use std::{
    ffi::OsString,
    future::Future,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use serde_json::json;
use tokio::{sync::mpsc, time::Instant};

use super::{
    Outcome,
    client::{ClientError, McpClient, Timeouts},
    external::connect,
};
use crate::{
    app::AppEvent,
    core::ai::{
        config::McpServer,
        mcp_catalog::link::{Ask, NO_LINK, Refused, Step, credentials_names, sign_in_url},
        mcp_secrets::{redact_list, redact_values},
        sanitize::clean,
    },
    runtime::mcp_secrets::load_for_start,
};

/// How long each part of a session may take.
#[derive(Clone, Copy)]
pub struct Limits {
    pub handshake: Duration,
    pub call: Duration,
    pub wait: Duration,
    pub poll: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            handshake: Duration::from_secs(20),
            call: Duration::from_secs(30),
            wait: Duration::from_secs(300),
            poll: Duration::from_secs(1),
        }
    }
}

/// Where the session runs and what it reports to.
pub struct Ctx<'a> {
    pub home: &'a Path,
    pub secrets: Option<PathBuf>,
    pub events: &'a mpsc::Sender<AppEvent>,
    pub limits: Limits,
}

/// The tool that starts the server's own OAuth flow; the entry hides it from
/// the model, so only this session calls it, by its raw name.
const AUTH: &str = "start_google_auth";
const CANCELLED: &str = "link cancelled";
const STOPPED: &str = "link failed: the server stopped";
/// The most characters of a server's reason that are shown.
const REASON_MAX: usize = 120;

/// The notice of a failure: `reason` redacted, cleaned and cut.
fn failed(reason: &str, values: &[String]) -> String {
    let text = redact_values(&clean(&redact_values(reason, values)), values);
    format!(
        "link failed: {}",
        text.chars().take(REASON_MAX).collect::<String>()
    )
}

/// Why an answer holds no link: an error it says (workspace-mcp returns its
/// errors as plain text starting `**Error:**`), else that it has no link.
fn why(outcome: &Outcome, values: &[String]) -> String {
    let line = outcome
        .text
        .lines()
        .map(|l| l.replace('*', ""))
        .find(|l| !l.trim().is_empty());
    let line = line.as_deref().map(str::trim).unwrap_or_default();
    let error = ["Authentication Error:", "Error:"]
        .iter()
        .find_map(|label| line.strip_prefix(label));
    match (error, outcome.is_error) {
        (Some(reason), _) => failed(reason.trim(), values),
        (None, true) if !line.is_empty() => failed(line, values),
        _ => NO_LINK.to_owned(),
    }
}

/// Starts the declared `google` as a chat start does, asks it to start its
/// sign-in, says the link (`Step::Waiting`) and waits for the credentials file
/// of the account to change (`stat` only: it is never opened). The server is
/// stopped, whatever the end. The result text is never shown whole.
pub async fn session(
    server: &McpServer,
    ask: &Ask,
    ctx: Ctx<'_>,
    host: impl Fn(&str) -> Option<OsString>,
    stat: impl Fn(&Path) -> Option<SystemTime>,
    cancel: impl Future<Output = ()>,
) -> Result<String, String> {
    let loaded = load_for_start(ctx.secrets.clone()).await;
    let own = loaded.secrets.lookup_for(server, &host);
    let values = redact_list(server, &own);
    let timeouts = Timeouts {
        handshake: ctx.limits.handshake,
        call: ctx.limits.call,
    };
    let mut cancel = std::pin::pin!(cancel);
    let connected = tokio::select! {
        result = connect(server, ctx.home, &own, &values, timeouts) => result,
        () = &mut cancel => return Err(CANCELLED.to_owned()),
    };
    let (mut client, _) = connected.map_err(|reason| failed(&reason, &values))?;
    client.keep_results_whole();
    let ended = drive(&mut client, ask, &ctx, &stat, &values, cancel).await;
    client.shutdown().await;
    ended
}

async fn drive(
    client: &mut McpClient,
    ask: &Ask,
    ctx: &Ctx<'_>,
    stat: &impl Fn(&Path) -> Option<SystemTime>,
    values: &[String],
    cancel: impl Future<Output = ()>,
) -> Result<String, String> {
    let Ctx {
        home,
        events,
        limits,
        ..
    } = ctx;
    let dir = home.join(".google_workspace_mcp/credentials");
    let paths: Vec<PathBuf> = credentials_names(&ask.email)
        .iter()
        .map(|n| dir.join(n))
        .collect();
    let stamps = |paths: &[PathBuf]| paths.iter().map(|p| stat(p)).collect::<Vec<_>>();
    let baseline = stamps(&paths);
    let mut cancel = std::pin::pin!(cancel);
    let arguments = json!({"service_name": "Google Workspace", "user_google_email": ask.email});
    let outcome = client
        .call_checked(AUTH, arguments, &mut cancel)
        .await
        .map_err(|error| match error {
            ClientError::Timeout => format!("link failed: {AUTH} did not answer in 30 s"),
            ClientError::Cancelled => CANCELLED.to_owned(),
            ClientError::Closed => STOPPED.to_owned(),
            other => failed(&other.reason(), values),
        })?;
    let url = match sign_in_url(&outcome.text, &ask.client_id) {
        Ok(url) => url,
        Err(Refused::NoLink) => return Err(why(&outcome, values)),
        Err(refused) => return Err(refused.notice().to_owned()),
    };
    let _ = events
        .send(AppEvent::McpLink(Step::Waiting(url.to_owned())))
        .await;
    let end = Instant::now() + limits.wait;
    loop {
        tokio::select! {
            () = &mut cancel => return Err(CANCELLED.to_owned()),
            () = tokio::time::sleep(limits.poll) => {}
        }
        let now = stamps(&paths);
        if now
            .iter()
            .zip(&baseline)
            .any(|(now, was)| now.is_some() && now != was)
        {
            return Ok("linked google".to_owned());
        }
        if client.has_exited() {
            return Err(STOPPED.to_owned());
        }
        if Instant::now() >= end {
            return Err("link timed out after 5 min".to_owned());
        }
    }
}

#[cfg(test)]
mod tests;
