//! The git read verbs in the host: `git_status`, `git_diff` and `git_log`.
//! A call is judged here BEFORE the user is asked (no jail, a batch staged,
//! a root the path guard refuses) and again after the `y`. The jail, the
//! argv and the parse live in `runtime/tools/git` and `core/ai/tools/git`;
//! this file only wires them to a call and shapes what the model gets back.

use std::{
    ffi::OsString,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use super::{
    Outcome, ToolHost, ToolStatus, TurnScope, ended,
    exec::{root_of, scratch_parent, staged},
};
use crate::{
    core::ai::{
        sanitize::redact_tokens,
        toolchain::env::scrub,
        tools::{
            Call, ToolError,
            git::{self as pure, GitError},
            label,
            mcp::refusal_text,
            paths::REFUSED,
        },
    },
    runtime::tools::{
        git::{self as runtime, Ctx, Fault, Output},
        new_nonce,
    },
};

const COULD_NOT: &str = "could not run git";
const NOT_A_REPO: &str = "the project is not a git repository";
const NO_GIT: &str = "git was not found in a place the sandbox can grant (such as /usr/bin)";
const TOO_OLD: &str = "git is too old (2.32 is needed) or its environment is not usable";
const REPO_REFUSED: &str = "the repository location is not available";
const TOO_LARGE: &str = "the output is too large to list";
const FAILED: &str = "git failed";
/// How long one read verb may take.
const TIMEOUT: Duration = Duration::from_secs(30);

/// Judges a call before it asks. Reads the disk (the root) and the batch store.
pub(super) async fn plan(host: &ToolHost, scope: &TurnScope) -> Result<(), &'static str> {
    let Some(key) = scope.stage_key else {
        return Err(refusal_text(ToolError::UnknownTool));
    };
    if !host.exec.jailed() {
        return Err(super::exec::NO_JAIL);
    }
    let (env, origin, stages) = (
        host.env(&scope.origin),
        scope.origin.clone(),
        host.stages.clone(),
    );
    tokio::task::spawn_blocking(move || {
        root_of(&env, &origin)?;
        staged(&stages, key)
    })
    .await
    .map_err(|_| COULD_NOT)?
}

/// Runs an approved (or free) read verb: judged again, then the jail.
pub(super) async fn run(
    host: &ToolHost,
    scope: &TurnScope,
    call: &Call,
) -> (Outcome, ToolStatus, String) {
    let fail = |why: &str| ended(ToolStatus::Error, why.to_owned(), why);
    // Without a nonce the output cannot be wrapped, so nothing may start.
    let Some(nonce) = new_nonce() else {
        return fail(COULD_NOT);
    };
    if let Err(why) = plan(host, scope).await {
        return fail(why);
    }
    go(host, scope, call, &nonce).await.unwrap_or_else(fail)
}

async fn go(
    host: &ToolHost,
    scope: &TurnScope,
    call: &Call,
    nonce: &str,
) -> Result<(Outcome, ToolStatus, String), &'static str> {
    let key = scope.stage_key.ok_or(COULD_NOT)?;
    let helper = host.exec.helper.clone().ok_or(COULD_NOT)?;
    let parent = scratch_parent(host).ok_or(COULD_NOT)?;
    let read = host.exec.read.clone();
    let unreported = host.exec.sink(key);
    let (env, origin, home) = (
        host.env(&scope.origin),
        scope.origin.clone(),
        host.home.clone(),
    );
    let ctx = {
        let (read, unreported) = (read.clone(), unreported.clone());
        tokio::task::spawn_blocking(move || {
            context(&*read, &env, &origin, home, parent, helper, unreported)
        })
        .await
        .map_err(|_| COULD_NOT)??
    };
    let repo = runtime::open(&ctx, &*read).await.map_err(fault_text)?;
    let (argv, label) = match call {
        Call::GitStatus => (Ok(pure::status_argv(&repo.dir)), "git status"),
        Call::GitDiff { staged, path } => (
            pure::diff_argv(&repo.dir, *staged, path.as_deref()),
            "git diff",
        ),
        Call::GitLog { max_count, path } => (
            pure::log_argv(&repo.dir, *max_count, path.as_deref()),
            "git log",
        ),
        _ => return Err(refusal_text(ToolError::UnknownTool)),
    };
    let argv = argv.map_err(|why: GitError| why.reason())?;
    let ran = runtime::run(&ctx, &repo, argv, true, &*read)
        .await
        .map_err(fault_text)?;
    let mut changes = ran.changes;
    changes.extend(unreported.drain());
    let body = match call {
        Call::GitDiff { .. } => diff_text(&ran.done),
        _ => listing(call, &ran.done),
    };
    let (status, mut text) = match body {
        Ok(body) => (ToolStatus::Ok, label::wrap(nonce, &body)),
        Err(why) => (ToolStatus::Error, why.to_owned()),
    };
    if let Some(note) = super::exec::changes_note(&changes, nonce) {
        text.push_str(&note);
    }
    let summary = if status == ToolStatus::Ok {
        label
    } else {
        FAILED
    };
    Ok(ended(status, text, summary))
}

/// What a git run needs: the helper, a git in a system place, the canonical
/// home and root. Reads the disk: blocking.
fn context(
    read: &(dyn Fn(&str) -> Option<OsString> + Sync),
    env: &crate::runtime::tools::ToolEnv,
    origin: &Path,
    home: Option<OsString>,
    parent: PathBuf,
    helper: PathBuf,
    unreported: crate::runtime::tools::runner::Unreported,
) -> Result<Ctx, &'static str> {
    let (_, root) = root_of(env, origin)?;
    let home = home.and_then(|h| fs::canonicalize(h).ok()).ok_or(REFUSED)?;
    let scrubbed = scrub(read, &root, &parent);
    let path = scrubbed.iter().find(|(name, _)| name == "PATH");
    let git = path
        .into_iter()
        .flat_map(|(_, value)| std::env::split_paths(value))
        .find_map(|dir| executable(&dir.join("git")))
        .ok_or(NO_GIT)?;
    Ok(Ctx {
        helper,
        git,
        home,
        root,
        scratch_parent: parent,
        timeout: TIMEOUT,
        unreported,
    })
}

fn executable(candidate: &Path) -> Option<PathBuf> {
    let real = fs::canonicalize(candidate).ok()?;
    let meta = fs::metadata(&real).ok()?;
    (meta.is_file() && meta.permissions().mode() & 0o111 != 0).then_some(real)
}

/// The fixed text of a git fault: never a path or an OS message.
fn fault_text(fault: Fault) -> &'static str {
    match fault {
        Fault::Spec(_) => NO_GIT,
        Fault::Dir => REPO_REFUSED,
        Fault::Env | Fault::Version => TOO_OLD,
        Fault::Preflight => NOT_A_REPO,
        Fault::Io(_) => COULD_NOT,
    }
}

/// The text of a status or a log: parsed and rendered with clean names, then
/// redacted. A cut or failed run is an error, never a half-parsed list.
fn listing(
    call: &Call,
    done: &crate::runtime::tools::runner::Done,
) -> Result<String, &'static str> {
    let raw = match runtime::status_output(done) {
        Output::Text(raw) => raw,
        Output::Truncated => return Err(TOO_LARGE),
        Output::Failed => return Err(FAILED),
    };
    let text = match call {
        Call::GitStatus => pure::render_status(&pure::parse_status(&raw)),
        _ => pure::render_log(&pure::parse_log(&raw)),
    };
    Ok(redact_tokens(&text))
}

/// The text of a diff: the capture is already cleaned, cut and redacted.
fn diff_text(done: &crate::runtime::tools::runner::Done) -> Result<String, &'static str> {
    let ended = &done.ended;
    if ended.exit != Some(0) || ended.timed_out {
        return Err(FAILED);
    }
    let text = done.capture.text();
    Ok(if text.trim().is_empty() {
        "(no changes)".to_owned()
    } else {
        text
    })
}
