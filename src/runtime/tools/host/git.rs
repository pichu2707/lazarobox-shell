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
    sync::Arc,
    time::Duration,
};

use super::{
    Outcome, ToolHost, ToolStatus, TurnScope, ended,
    exec::{root_of, scratch_parent, staged},
};
use crate::{
    core::ai::{
        chat::{
            approval::{ApprovalAsk, add_line},
            review::CommitReview,
        },
        memory::redact::redact,
        toolchain::{SYSTEM_RX, env::scrub},
        tools::{
            Call, ToolError,
            git::{self as pure, GitError},
            label,
            mcp::refusal_text,
            paths::REFUSED,
        },
    },
    runtime::tools::{
        git::{self as runtime, Ctx, Fault, Output, Ran},
        new_nonce,
    },
};

const COULD_NOT: &str = "could not run git";
const NOT_A_REPO: &str = "the project is not a git repository";
pub(super) const NO_GIT: &str =
    "git was not found in a place the sandbox can grant (such as /usr/bin)";
const TOO_OLD: &str = "git is too old (2.32 is needed) or its environment is not usable";
pub(super) const REPO_REFUSED: &str = "the repository location is not available";
const TOO_LARGE: &str = "the output is too large to list";
const FAILED: &str = "git failed";
const TOO_MANY_PATHS: &str = "too many or too long paths to show in the question; add fewer";
const NO_MATCH: &str = "no file matches those paths";
const NOTHING_STAGED: &str = "nothing is staged";
const SECRET_STAGED: &str =
    "the index holds a file that looks like a secret; the user must unstage it";
const STAGE_TOO_LARGE: &str = "the staged diff is too large to review; stage less";
const STAGED_CHANGED: &str = "the staged changes changed after the question; ask again";
/// How long one read verb may take.
const TIMEOUT: Duration = Duration::from_secs(30);
/// How long a commit may take: its hooks may run linters.
const COMMIT_TIMEOUT: Duration = Duration::from_secs(120);
const NOT_CHECKED: &str = "the commit could not be checked against the reviewed changes";

/// An opened repository: the run context and what the preflight found.
struct Opened {
    ctx: Ctx,
    repo: runtime::Repo,
    read: super::exec::Reader,
    unreported: crate::runtime::tools::runner::Unreported,
}

/// Runs one argv in the repo's jail.
async fn exec(opened: &Opened, argv: Vec<String>, read_verb: bool) -> Result<Ran, &'static str> {
    runtime::run(&opened.ctx, &opened.repo, argv, read_verb, &*opened.read)
        .await
        .map_err(fault_text)
}

/// What every git verb needs before it can ask: a chat that can stage, a jail,
/// a root the path guard allows and no batch waiting.
async fn ready(host: &ToolHost, scope: &TurnScope) -> Result<(), &'static str> {
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

/// Judges a call before it asks. Reads the disk (the root) and the batch
/// store. An add or a commit also looks at the repository and returns the ask
/// the user answers (`None` for a read verb: the caller asks by locality).
pub(super) async fn plan(
    host: &ToolHost,
    scope: &TurnScope,
    call: &Call,
) -> Result<Option<ApprovalAsk>, &'static str> {
    // A path the model names is refused here, so a refusal opens no prompt.
    match call {
        Call::GitDiff { path: Some(p), .. } | Call::GitLog { path: Some(p), .. } => {
            pure::check(p).map_err(GitError::reason)?;
        }
        Call::GitAdd { paths } => {
            let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
            pure::check_add(&refs).map_err(GitError::reason)?;
        }
        Call::GitCommit { message } => pure::check_message(message).map_err(GitError::reason)?,
        _ => {}
    }
    ready(host, scope).await?;
    match call {
        Call::GitAdd { paths } => plan_add(host, scope, paths).await.map(Some),
        Call::GitCommit { message } => plan_commit(host, scope, message).await.map(Some),
        _ => Ok(None),
    }
}

/// The ask of an add: the paths as the prompt shows them whole, and how many
/// files a dry run says would be staged (the secret excludes apply).
async fn plan_add(
    host: &ToolHost,
    scope: &TurnScope,
    paths: &[String],
) -> Result<ApprovalAsk, &'static str> {
    let shown = add_line(paths).ok_or(TOO_MANY_PATHS)?;
    let opened = open(host, scope).await?;
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    let dry = pure::add_dry_argv(&opened.repo.dir, &refs).map_err(GitError::reason)?;
    let ran = exec(&opened, dry, true).await?;
    let files = match runtime::status_output(&ran.done) {
        Output::Text(raw) => raw.lines().filter(|l| !l.is_empty()).count(),
        Output::Truncated => return Err(TOO_LARGE),
        Output::Failed => return Err(NO_MATCH),
    };
    if files == 0 {
        return Err(NO_MATCH);
    }
    Ok(ApprovalAsk::GitAdd {
        paths: shown,
        files: Some(files),
    })
}

/// The staged diff, once the index passed every check (something is staged,
/// no secret path, nothing cut).
async fn staged_diff(opened: &Opened) -> Result<Vec<u8>, &'static str> {
    let names = exec(opened, pure::index_names_argv(&opened.repo.dir), true).await?;
    match runtime::status_output(&names.done) {
        Output::Text(raw) if raw.is_empty() => return Err(NOTHING_STAGED),
        Output::Text(raw) if pure::staged_has_secret(&raw) => return Err(SECRET_STAGED),
        Output::Text(_) => {}
        Output::Truncated => return Err(STAGE_TOO_LARGE),
        Output::Failed => return Err(FAILED),
    }
    let diff = exec(opened, pure::review_diff_argv(&opened.repo.dir), true).await?;
    let ended = &diff.done.ended;
    if ended.exit != Some(0) || ended.timed_out {
        return Err(FAILED);
    }
    if diff.done.capture.truncated() {
        return Err(STAGE_TOO_LARGE);
    }
    Ok(diff.done.capture.bytes())
}

/// The ask of a commit: the message, the signing notice and the whole staged
/// diff in the review view. Nothing is committed yet.
async fn plan_commit(
    host: &ToolHost,
    scope: &TurnScope,
    message: &str,
) -> Result<ApprovalAsk, &'static str> {
    let opened = open(host, scope).await?;
    opened.repo.identity.clone().map_err(GitError::reason)?;
    let diff = staged_diff(&opened).await?;
    let hooks = pure::hooks_notice(&opened.repo.dir, &opened.repo.configured_hooks);
    let hooks_dir = opened.repo.dir.hooks();
    let running = tokio::task::spawn_blocking(move || running_hooks(&hooks_dir))
        .await
        .map_err(|_| COULD_NOT)?;
    Ok(ApprovalAsk::Commit(Arc::new(CommitReview::new(
        message,
        hooks.as_deref(),
        &running,
        &diff,
    ))))
}

/// The commit hooks that exist and are executable in the pinned hooks folder.
fn running_hooks(dir: &str) -> Vec<&'static str> {
    (pure::COMMIT_HOOKS.into_iter())
        .filter(|name| executable(&Path::new(dir).join(name)).is_some())
        .collect()
}

/// Runs an approved (or free) verb: judged again, then the jail. `reviewed`
/// is the ask the user answered, for a commit to compare with the index.
pub(super) async fn run(
    host: &ToolHost,
    scope: &TurnScope,
    call: &Call,
    reviewed: Option<ApprovalAsk>,
) -> (Outcome, ToolStatus, String) {
    let fail = |why: &str| ended(ToolStatus::Error, why.to_owned(), why);
    // Without a nonce the output cannot be wrapped, so nothing may start.
    let Some(nonce) = new_nonce() else {
        return fail(COULD_NOT);
    };
    if let Err(why) = ready(host, scope).await {
        return fail(why);
    }
    let result = match call {
        Call::GitAdd { paths } => add(host, scope, paths, &nonce).await,
        Call::GitCommit { message } => commit(host, scope, message, reviewed, &nonce).await,
        _ => go(host, scope, call, &nonce).await,
    };
    result.unwrap_or_else(fail)
}

/// The approved add: stages the paths, secrets excluded.
async fn add(
    host: &ToolHost,
    scope: &TurnScope,
    paths: &[String],
    nonce: &str,
) -> Result<(Outcome, ToolStatus, String), &'static str> {
    let opened = open(host, scope).await?;
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    let argv = pure::add_argv(&opened.repo.dir, &refs).map_err(GitError::reason)?;
    let ran = exec(&opened, argv, false).await?;
    let ended = &ran.done.ended;
    let ok = ended.exit == Some(0) && !ended.timed_out;
    let text = if ok { "staged" } else { FAILED };
    let result = if ok {
        (ToolStatus::Ok, "git add")
    } else {
        (ToolStatus::Error, FAILED)
    };
    Ok(finish(&opened, ran, result, text.to_owned(), nonce))
}

/// The approved commit: only if the index is what the user read.
async fn commit(
    host: &ToolHost,
    scope: &TurnScope,
    message: &str,
    reviewed: Option<ApprovalAsk>,
    nonce: &str,
) -> Result<(Outcome, ToolStatus, String), &'static str> {
    let Some(ApprovalAsk::Commit(reviewed)) = reviewed else {
        return Err(COULD_NOT);
    };
    let mut opened = open(host, scope).await?;
    opened.ctx.timeout = COMMIT_TIMEOUT;
    let who = opened.repo.identity.clone().map_err(GitError::reason)?;
    if !reviewed.same_staged(&staged_diff(&opened).await?) {
        return Err(STAGED_CHANGED);
    }
    // The tree the user read, recorded before any hook can change the index.
    let before = text_of(&opened, pure::write_tree_argv(&opened.repo.dir), false).await;
    let before = before.ok_or(FAILED)?.trim().to_owned();
    let argv = pure::commit_argv(&opened.repo.dir, &who, message).map_err(GitError::reason)?;
    let ran = exec(&opened, argv, false).await?;
    let ended = &ran.done.ended;
    let ok = ended.exit == Some(0) && !ended.timed_out;
    // The output holds the hooks' words: data, wrapped like any other.
    let body = label::wrap(nonce, &ran.done.capture.text());
    if !ok {
        let text = format!("{FAILED}\n{body}");
        return Ok(finish(
            &opened,
            ran,
            (ToolStatus::Error, FAILED),
            text,
            nonce,
        ));
    }
    // Warn, never undo (user decision 2026-10-28): a hook may have changed
    // what was committed after the review.
    let Some(drift) = drift_of(&opened, &before).await else {
        return Ok(finish(
            &opened,
            ran,
            (ToolStatus::Ok, "git commit"),
            body,
            nonce,
        ));
    };
    let text = format!("{}\n{body}", redact(&drift.0).text);
    let summary = if drift.1 {
        "commit differs from the reviewed changes; a secret path was committed"
    } else {
        "commit differs from the reviewed changes"
    };
    Ok(finish(
        &opened,
        ran,
        (ToolStatus::Error, summary),
        text,
        nonce,
    ))
}

/// The text of a git run that ended well, or `None`.
async fn text_of(opened: &Opened, argv: Vec<String>, read_verb: bool) -> Option<String> {
    let ran = exec(opened, argv, read_verb).await.ok()?;
    match runtime::status_output(&ran.done) {
        Output::Text(raw) => Some(raw),
        _ => None,
    }
}

/// How the commit just made differs from the reviewed tree: the notice and
/// whether a secret path is in it, or `None` when it is the reviewed tree.
async fn drift_of(opened: &Opened, before: &str) -> Option<(String, bool)> {
    let dir = &opened.repo.dir;
    let after = text_of(opened, pure::head_tree_argv(dir), true).await;
    let after = after.as_deref().map(str::trim);
    if after == Some(before) {
        return None;
    }
    let listing = match after.and_then(|a| pure::tree_diff_argv(dir, before, a)) {
        Some(argv) => text_of(opened, argv, true).await,
        None => None,
    };
    Some(listing.map_or((NOT_CHECKED.to_owned(), false), |l| pure::drift_notice(&l)))
}

/// The result of a write verb, with the watched places it changed.
fn finish(
    opened: &Opened,
    ran: Ran,
    (status, summary): (ToolStatus, &str),
    mut text: String,
    nonce: &str,
) -> (Outcome, ToolStatus, String) {
    let mut changes = ran.changes;
    changes.extend(opened.unreported.drain());
    if let Some(note) = super::exec::changes_note(&changes, nonce) {
        text.push_str(&note);
    }
    ended(status, text, summary)
}

/// The preflight of a chat's repository: the run context and what git says.
async fn open(host: &ToolHost, scope: &TurnScope) -> Result<Opened, &'static str> {
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
    Ok(Opened {
        ctx,
        repo,
        read,
        unreported,
    })
}

async fn go(
    host: &ToolHost,
    scope: &TurnScope,
    call: &Call,
    nonce: &str,
) -> Result<(Outcome, ToolStatus, String), &'static str> {
    let opened = open(host, scope).await?;
    let repo = &opened.repo;
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
    let ran = exec(&opened, argv, true).await?;
    let mut changes = ran.changes;
    changes.extend(opened.unreported.drain());
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
    let git = path.and_then(|(_, value)| find_git(value)).ok_or(NO_GIT)?;
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

/// The first executable `git` on a PATH whose real path is in a system place
/// (the only places the jail grants): a user-installed git first on PATH is
/// skipped, not picked and then refused.
pub(super) fn find_git(path: &std::ffi::OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .filter_map(|dir| executable(&dir.join("git")))
        .find(|real| SYSTEM_RX.iter().any(|place| real.starts_with(place)))
}

fn executable(candidate: &Path) -> Option<PathBuf> {
    let real = fs::canonicalize(candidate).ok()?;
    let meta = fs::metadata(&real).ok()?;
    (meta.is_file() && meta.permissions().mode() & 0o111 != 0).then_some(real)
}

/// The fixed text of a git fault: never a path or an OS message.
pub(super) fn fault_text(fault: Fault) -> &'static str {
    match fault {
        Fault::Spec(_) => NO_GIT,
        Fault::Root | Fault::Dir => REPO_REFUSED,
        Fault::Env | Fault::Version => TOO_OLD,
        Fault::Preflight => NOT_A_REPO,
        Fault::Io(_) => COULD_NOT,
    }
}

/// The text of a status or a log: parsed and rendered with clean names, then
/// redacted like a diff. A cut or failed run is an error, never a half-parsed list.
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
    Ok(redact(&text).text)
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
