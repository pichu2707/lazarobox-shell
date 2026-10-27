//! `run_command` in the host. A call is judged here BEFORE the user is asked
//! (no jail, root policy, folder, allow-list, staged changes, a run already
//! going), and judged again after the `y`, because the disk and the chat may
//! have changed while the prompt was open. Nothing here starts a process: the
//! spawn core is `runner.rs`.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde_json::Value;

use super::{
    TurnScope,
    files::lock,
    gate::{CHAT_BUSY, RunGate},
};
use crate::{
    core::ai::{
        jail::{Backend, JailSupport},
        sanitize::clean,
        tools::{
            Call, ToolError,
            allow::{RunError, RunRequest, parse_run},
            mcp::refusal_text,
            paths::{REFUSED, plain_relative},
            stage::{StageKey, State},
        },
    },
    runtime::tools::{ToolEnv, fs::load_roots, stage::Stages},
};

/// The refusal of a run on a system with no Landlock jail (design D18).
pub(super) const NO_JAIL: &str = "commands need the Linux sandbox (Landlock ABI 3+)";
const PROPOSE_FIRST: &str = "staged changes are not proposed: propose them first";
const IN_REVIEW: &str = "a batch is waiting for the user's review";
/// Text the prompt could not show exactly as it will run.
const UNSHOWABLE: &str = "command cannot be shown safely";
const COULD_NOT: &str = "could not run";
/// The most bytes of `package.json` read to find its scripts.
const PACKAGE_MAX: u64 = 256 * 1024;

/// What the host knows about running commands: who may run, and the system
/// facts a test replaces.
pub(super) struct Exec {
    pub(super) gate: RunGate,
    /// What the jail probe says now.
    pub(super) jail: Box<dyn Fn() -> JailSupport + Send + Sync>,
}

impl Exec {
    /// The real machine.
    pub(super) fn system() -> Self {
        Self {
            gate: RunGate::default(),
            jail: Box::new(crate::runtime::jail::probe),
        }
    }

    fn jailed(&self) -> bool {
        matches!(
            (self.jail)(),
            JailSupport::Available {
                backend: Backend::Landlock,
                ..
            }
        )
    }
}

/// A command the allow-list let through, and where it runs. Every path is
/// canonical.
pub(super) struct Plan {
    pub(super) request: RunRequest,
    pub(super) root: PathBuf,
    pub(super) cwd: PathBuf,
    pub(super) key: StageKey,
}

/// Judges a call. The checks come in the order of the spec; the first refusal
/// is the text the model gets.
pub(super) async fn plan(
    host: &super::ToolHost,
    scope: &TurnScope,
    call: &Call,
) -> Result<Plan, &'static str> {
    let (Some(key), Call::RunCommand { args }) = (scope.stage_key, call) else {
        return Err(refusal_text(ToolError::UnknownTool));
    };
    if !host.exec.jailed() {
        return Err(NO_JAIL);
    }
    let (env, origin, args) = (host.env(&scope.origin), scope.origin.clone(), args.clone());
    let stages = host.stages.clone();
    let judged = tokio::task::spawn_blocking(move || {
        let found = inspect(&env, &origin, &args)?;
        staged(&stages, key)?;
        Ok::<_, &'static str>(found)
    })
    .await
    .map_err(|_| COULD_NOT)??;
    if host.exec.gate.is_busy(key) {
        return Err(CHAT_BUSY);
    }
    let (request, root, cwd) = judged;
    Ok(Plan {
        request,
        root,
        cwd,
        key,
    })
}

/// The root policy of the chat origin, the folder, the allow-list and what the
/// prompt can show. Reads the disk.
fn inspect(
    env: &ToolEnv,
    origin: &Path,
    args: &Value,
) -> Result<(RunRequest, PathBuf, PathBuf), &'static str> {
    let roots = load_roots(env).map_err(|_| REFUSED)?;
    let root = origin
        .is_absolute()
        .then(|| fs::canonicalize(origin).ok())
        .flatten()
        .filter(|root| root.is_dir() && root != roots.home() && roots.judge_real(root))
        .ok_or(REFUSED)?;
    let rel = match args["cwd"].as_str() {
        Some(text) => plain_relative(text).ok_or(REFUSED)?,
        None => PathBuf::new(),
    };
    let cwd = fs::canonicalize(root.join(rel))
        .ok()
        .filter(|cwd| cwd.starts_with(&root) && cwd.is_dir() && roots.judge_real(cwd))
        .ok_or(REFUSED)?;
    let request = parse_run(args, &scripts(&cwd)).map_err(|why| match why {
        RunError::Shape(shape) => refusal_text(shape),
        RunError::Refused(refusal) => refusal.text(),
    })?;
    if request.args.iter().any(|arg| clean(arg) != *arg) {
        return Err(UNSHOWABLE);
    }
    Ok((request, root, cwd))
}

/// A command runs against the disk, never against staged content.
fn staged(stages: &Arc<std::sync::Mutex<Stages>>, key: StageKey) -> Result<(), &'static str> {
    match lock(stages).batch(key).map(|batch| batch.state()) {
        Some(State::Staging) => Err(PROPOSE_FIRST),
        Some(State::Proposed) => Err(IN_REVIEW),
        _ => Ok(()),
    }
}

/// The script names of the `package.json` in `dir`: none when it is missing,
/// not a plain file, too big or not JSON.
fn scripts(dir: &Path) -> Vec<String> {
    let path = dir.join("package.json");
    let Ok(meta) = fs::symlink_metadata(&path) else {
        return Vec::new();
    };
    if !meta.is_file() || meta.len() > PACKAGE_MAX {
        return Vec::new();
    }
    let text = fs::read_to_string(&path).unwrap_or_default();
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|json| {
            json["scripts"]
                .as_object()
                .map(|s| s.keys().cloned().collect())
        })
        .unwrap_or_default()
}
