//! The GitHub read tools in the host: the repository comes from `origin`
//! (read by lazarobox, never from the model), `gh` runs with a fixed argv, a
//! filtered environment and a capped reply, and the answer is rendered by the
//! pure core. Nothing here prints GitHub text or `gh` output anywhere but in
//! the wrapped result; every failure is a fixed sentence.

use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    path::PathBuf,
    sync::{Mutex, OnceLock, atomic::Ordering},
    time::Duration,
};

use super::{Outcome, ToolHost, ToolStatus, TurnScope, ended, files::lock_set, git::find_git};
use crate::{
    core::ai::{
        chat::{
            approval::{ApprovalAsk, ask_for},
            engine::StreamEvent,
        },
        tools::{
            Call, Locality,
            github::{self as pure, GhCall, Slug, render},
        },
    },
    runtime::{
        desktop::which,
        tools::{
            git::{Origin, origin_url},
            new_nonce,
            runner::{Run, Split, spawn_capped},
        },
    },
};

/// How long one `gh` run may take.
const TIMEOUT: Duration = Duration::from_secs(30);
/// The most stdout read from `gh`; a longer reply is refused, never cut.
const STDOUT_CAP: usize = 4 * 1024 * 1024;
/// The system places a git is looked for when the login PATH has none.
const SYSTEM_PATH: &str = "/usr/bin:/bin:/usr/local/bin";

const NO_ORIGIN: &str = "this folder has no origin remote";
const NO_REPO: &str =
    "the folder has no repository, or git refused it (ownership or a broken config)";
const NOT_GITHUB: &str = "origin is not on GitHub (only github.com is supported)";
const ORIGIN_FAILED: &str = "could not read origin";
const NO_GH: &str =
    "gh is not installed: install the GitHub CLI (https://cli.github.com) and try again";
const START_FAILED: &str = "gh could not be started";
const LOGGED_OUT: &str = "gh is not logged in: run gh auth login in a shell pane and try again";
const TIMED_OUT: &str = "GitHub did not answer in time";
const TOO_LARGE: &str = "the reply is too large";
const TOO_LARGE_COMMENTS: &str = "the reply is too large; try without comments";
const UNREADABLE: &str = "gh returned something unreadable";
const NO_NONCE: &str = "the result cannot be wrapped as data now";
const CANCELLED: &str = "cancelled";
const FORK_UNKNOWN: &str = "fork status unknown";
/// What a later call answers once the user declined this repository.
pub(super) const DECLINED: &str = "the user declined GitHub reads of this repository in this turn";

/// What the host keeps for the GitHub tools.
pub(super) struct Gh {
    /// The login PATH, set once by the runtime: where `gh` is found and what
    /// `gh` runs with.
    path: OnceLock<OsString>,
    /// The fork note of each repository read so far: successes, and a probe
    /// that timed out or found `gh` logged out (`fork status unknown`).
    forks: Mutex<HashMap<String, String>>,
    bound: Duration,
}

impl Default for Gh {
    fn default() -> Self {
        Self::with_bound(TIMEOUT)
    }
}

impl Gh {
    pub(super) fn with_bound(bound: Duration) -> Self {
        Self {
            path: OnceLock::new(),
            forks: Mutex::default(),
            bound,
        }
    }

    pub(super) fn set_path(&self, path: OsString) {
        let _ = self.path.set(path);
    }

    fn path(&self) -> OsString {
        self.path
            .get()
            .cloned()
            .or_else(|| std::env::var_os("PATH"))
            .unwrap_or_default()
    }
}

/// What the call needs once the checks before the prompt passed.
pub(super) struct Planned {
    pub(super) slug: Slug,
    program: PathBuf,
}

/// Judges a call before it asks: the repository from `origin`, then `gh`.
/// Nothing is started and nothing is asked on a refusal.
pub(super) async fn plan(host: &ToolHost, scope: &TurnScope) -> Result<Planned, &'static str> {
    let path = host.gh.path();
    let git = find_git(&path)
        .or_else(|| find_git(OsStr::new(SYSTEM_PATH)))
        .ok_or(ORIGIN_FAILED)?;
    let found = origin_url(&git, &scope.origin, host.home.as_deref())
        .await
        .map_err(|_| ORIGIN_FAILED)?;
    let slug = match found {
        Origin::Url(url) => pure::slug_of(&url).ok_or(NOT_GITHUB)?,
        Origin::NoOrigin => return Err(NO_ORIGIN),
        Origin::NotARepo => return Err(NO_REPO),
    };
    let program = which("gh", Some(&path)).ok_or(NO_GH)?;
    Ok(Planned { slug, program })
}

/// What consent a read needs.
pub(super) enum Consent {
    Free,
    /// Ask, then remember the answer under these keys of the turn.
    Ask(ApprovalAsk, String, String),
    /// The user declined this repository earlier in the turn.
    Declined,
}

/// Cloud consent, once per repository and turn: `github:<slug>` is an
/// accepted one, `github-denied:<slug>` a declined one.
pub(super) fn consent(scope: &TurnScope, call: &GhCall, slug: &Slug) -> Consent {
    // An Ollama `*-cloud` model counts as cloud here, and says so.
    let ollama_cloud = scope.locality != Locality::Cloud;
    if scope.locality != Locality::Cloud && !scope.ollama_cloud {
        return Consent::Free;
    }
    let (yes, no) = (format!("github:{slug}"), format!("github-denied:{slug}"));
    {
        let allowed = lock_set(&scope.allowed);
        if allowed.contains(&no) {
            return Consent::Declined;
        }
        if allowed.contains(&yes) {
            return Consent::Free;
        }
    }
    match ask_for(&Call::Github(*call), Locality::Cloud) {
        Some(ApprovalAsk::GithubRead { what, .. }) => Consent::Ask(
            ApprovalAsk::GithubRead {
                repo: slug.to_string(),
                what,
                ollama_cloud,
            },
            yes,
            no,
        ),
        _ => Consent::Free,
    }
}

/// The environment of `gh`: the inherited one without what could point it at
/// another host or repository, make it print debug text or page, and with
/// prompts, update notices and colour off. A pure filter, because `gh` keeps
/// its login in a keyring that needs the session variables.
pub(super) fn gh_env(
    vars: impl IntoIterator<Item = (OsString, OsString)>,
    path: &OsStr,
) -> Vec<(OsString, OsString)> {
    const OURS: [(&str, &str); 3] = [
        ("GH_PROMPT_DISABLED", "1"),
        ("GH_NO_UPDATE_NOTIFIER", "1"),
        ("NO_COLOR", "1"),
    ];
    const DROPPED: [&str; 16] = [
        "GH_HOST",
        "GH_REPO",
        "GH_ENTERPRISE_TOKEN",
        "GITHUB_ENTERPRISE_TOKEN",
        "GH_DEBUG",
        "DEBUG",
        "GH_PAGER",
        "PAGER",
        "GH_FORCE_TTY",
        // What changes the shape of the output: colour, widths, styles and the
        // accessible modes.
        "CLICOLOR_FORCE",
        "GH_ACCESSIBLE_PROMPTER",
        "GH_COLOR_LABELS",
        "GH_ACCESSIBLE_COLORS",
        "GH_MDWIDTH",
        "GLAMOUR_STYLE",
        "GH_SPINNER_DISABLED",
    ];
    let mut env: Vec<_> = vars
        .into_iter()
        .filter(|(name, _)| {
            let name = name.to_string_lossy();
            name != "PATH"
                && !DROPPED.contains(&&*name)
                && !OURS.iter().any(|(ours, _)| *ours == name)
        })
        .collect();
    env.extend(OURS.map(|(n, v)| (n.into(), v.into())));
    env.push(("PATH".into(), path.to_owned()));
    env
}

async fn gh(host: &ToolHost, planned: &Planned, argv: Vec<String>) -> std::io::Result<Split> {
    let path = host.gh.path();
    let run = Run {
        program: planned.program.clone(),
        argv: argv.into_iter().map(OsString::from).collect(),
        cwd: PathBuf::from("/"),
        env: gh_env(std::env::vars_os(), &path),
        timeout: host.gh.bound,
    };
    spawn_capped(run, STDOUT_CAP).await
}

/// The fork note of the repository: `O/R is a fork of P/Q; the parent is not
/// read`, nothing for a plain repository, `fork status unknown` when the probe
/// failed. Success, a timeout and a logged-out `gh` are cached for the run. The parent is validated as a slug
/// and never read.
async fn fork_note(host: &ToolHost, planned: &Planned) -> String {
    let key = planned.slug.to_string();
    if let Some(note) = lock_set(&host.gh.forks).get(&key) {
        return note.clone();
    }
    let Ok(split) = gh(host, planned, pure::fork_argv(&planned.slug)).await else {
        return FORK_UNKNOWN.to_owned();
    };
    // A timeout or a logged-out `gh` would happen again: remember it, so the
    // later calls of the run do not pay the probe (up to the bound) again.
    let stderr = String::from_utf8_lossy(&split.stderr);
    if split.ended.timed_out || split.ended.exit == Some(4) || stderr.contains("gh auth login") {
        lock_set(&host.gh.forks).insert(key, FORK_UNKNOWN.to_owned());
        return FORK_UNKNOWN.to_owned();
    }
    let json = (split.ended.exit == Some(0) && !split.over && !split.ended.timed_out)
        .then(|| serde_json::from_slice::<serde_json::Value>(&split.stdout).ok())
        .flatten()
        .filter(|v| v["isFork"].is_boolean());
    let Some(json) = json else {
        return FORK_UNKNOWN.to_owned();
    };
    let note = if json["isFork"] == true {
        let (owner, name) = (&json["parent"]["owner"]["login"], &json["parent"]["name"]);
        match (owner.as_str(), name.as_str()) {
            (Some(o), Some(n)) => match pure::slug_of(&format!("https://github.com/{o}/{n}")) {
                Some(parent) => format!("{key} is a fork of {parent}; the parent is not read"),
                None => format!("{key} is a fork; the parent is not read"),
            },
            _ => format!("{key} is a fork; the parent is not read"),
        }
    } else {
        String::new()
    };
    lock_set(&host.gh.forks).insert(key, note.clone());
    note
}

/// The fixed sentence for a `gh` that ended badly. Never `gh`'s own text.
fn failure(split: &Split, call: &GhCall, slug: &Slug) -> Option<String> {
    let ended = &split.ended;
    let stderr = String::from_utf8_lossy(&split.stderr);
    let number = match *call {
        GhCall::IssueView { number, .. } | GhCall::PrView { number } => Some(number),
        GhCall::IssueList { .. } => None,
    };
    Some(if ended.timed_out {
        TIMED_OUT.to_owned()
    } else if split.over {
        match call {
            GhCall::IssueView { comments: true, .. } => TOO_LARGE_COMMENTS,
            _ => TOO_LARGE,
        }
        .to_owned()
    } else if ended.exit == Some(4) || stderr.contains("gh auth login") {
        LOGGED_OUT.to_owned()
    } else if let (Some(1), Some(n), true) =
        (ended.exit, number, stderr.contains("with the number of"))
    {
        format!("#{n} was not found in {slug}")
    } else if ended.exit != Some(0) {
        match ended.exit {
            Some(code) => format!("gh failed (exit {code})"),
            None => "gh failed".to_owned(),
        }
    } else {
        return None;
    })
}

/// Runs the read. `GithubStarted` goes out first on the turn's ordered
/// channel, so it precedes the result and the next round's call; from there
/// every ending, a failure included, has started the conversation's block.
pub(super) async fn run(
    host: &ToolHost,
    scope: &TurnScope,
    call: &GhCall,
    planned: Planned,
) -> (Outcome, ToolStatus, String) {
    // With no nonce the result cannot be wrapped, so `gh` must not run.
    let Some(nonce) = new_nonce() else {
        return ended(ToolStatus::Error, NO_NONCE.to_owned(), NO_NONCE);
    };
    if scope.events.send(StreamEvent::GithubStarted).await.is_err() {
        return ended(ToolStatus::Error, CANCELLED.to_owned(), CANCELLED);
    }
    // From the next call on, the conversation's file reads ask on `*-cloud`.
    scope.github_read.store(true, Ordering::Relaxed);
    let note = fork_note(host, &planned).await;
    let split = match gh(host, &planned, pure::argv(call, &planned.slug)).await {
        Ok(split) => split,
        Err(_) => return ended(ToolStatus::Error, START_FAILED.to_owned(), START_FAILED),
    };
    if let Some(why) = failure(&split, call, &planned.slug) {
        return ended(ToolStatus::Error, why.clone(), &why);
    }
    let json = String::from_utf8_lossy(&split.stdout);
    match render::render(&nonce, call, &planned.slug, &note, &json) {
        Ok(out) => ended(ToolStatus::Ok, out.text, &out.summary),
        Err(_) => ended(ToolStatus::Error, UNREADABLE.to_owned(), UNREADABLE),
    }
}
