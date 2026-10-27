//! `run_command` in the host. A call is judged here BEFORE the user is asked
//! (no jail, root policy, folder, allow-list, staged changes, a run already
//! going), and judged again after the `y`, because the disk and the chat may
//! have changed while the prompt was open. Nothing here starts a process: the
//! spawn core is `runner.rs`.

use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

use serde_json::Value;

use super::{
    Outcome, ToolHost, ToolStatus, TurnScope, ended,
    files::lock,
    gate::{CHAT_BUSY, RunGate},
};
use crate::{
    core::ai::{
        chat::approval::{ApprovalAsk, script_line},
        jail::JailSupport,
        sanitize::clean,
        toolchain::{Layout, Refusal, Vars, env::scrub, toolchain_spec},
        tools::{
            Call, ToolError,
            allow::{Program, RunError, RunRequest, parse_run},
            label,
            mcp::refusal_text,
            paths::{REFUSED, plain_relative},
            run::RunOutcome,
            stage::{StageKey, State},
        },
    },
    runtime::tools::{
        ToolEnv,
        fs::load_roots,
        integrity::{Change, Kind},
        new_nonce,
        runner::{Job, Unreported, run_jailed},
        scratch::Scratch,
        stage::Stages,
    },
};

/// The refusal of a run on a system with no Landlock jail (design D18).
pub(super) const NO_JAIL: &str = "commands need the Linux sandbox (Landlock ABI 3+)";
const PROPOSE_FIRST: &str = "staged changes are not proposed: propose them first";
const IN_REVIEW: &str = "a batch is waiting for the user's review";
/// Text the prompt could not show exactly as it will run.
const UNSHOWABLE: &str = "command cannot be shown safely";
const COULD_NOT: &str = "could not run";
const NOT_FOUND: &str = "the program was not found on the PATH";
const NO_PLACE: &str = "the program is not in a place the sandbox can grant";
const NO_SANDBOX: &str = "the sandbox could not be built for this run";
const NOT_ENFORCED: &str = "the Linux sandbox could not be applied";
const NO_PACKAGE: &str = "run it from the folder that has package.json";
const PACKAGE_CHANGED: &str = "package.json changed after the question; ask again";
/// The most bytes of `package.json` read to find its scripts.
const PACKAGE_MAX: u64 = 256 * 1024;

/// The parent's environment, by name.
pub(super) type Reader = Arc<dyn Fn(&str) -> Option<OsString> + Send + Sync>;

/// What the host knows about running commands: who may run, and the system
/// facts a test replaces.
pub(super) struct Exec {
    pub(super) gate: RunGate,
    /// What the jail probe says now.
    pub(super) jail: Box<dyn Fn() -> JailSupport + Send + Sync>,
    /// The `lazarobox` binary that serves `__jail-exec`.
    pub(super) helper: Option<PathBuf>,
    /// The environment the child's own is rebuilt from.
    pub(super) read: Reader,
    /// Changes of runs that were dropped before they ended, by chat.
    pub(super) sinks: Mutex<HashMap<StageKey, Unreported>>,
}

impl Exec {
    /// The real machine.
    pub(super) fn system() -> Self {
        Self {
            gate: RunGate::default(),
            jail: Box::new(crate::runtime::jail::probe),
            helper: std::env::current_exe().and_then(fs::canonicalize).ok(),
            read: Arc::new(|name| std::env::var_os(name)),
            sinks: Mutex::default(),
        }
    }

    /// The sink of `key`'s runs that were dropped before they ended (a shared
    /// handle): the next ask peeks at it for the user's notice, and every result drains
    /// it for the model.
    pub(super) fn sink(&self, key: StageKey) -> Unreported {
        let mut sinks = self.sinks.lock().unwrap_or_else(PoisonError::into_inner);
        sinks.entry(key).or_default().clone()
    }

    fn jailed(&self) -> bool {
        (self.jail)().runs_commands()
    }
}

/// A command the allow-list let through, and where it runs. Every path is
/// canonical.
pub(super) struct Plan {
    /// What a package manager will read from `package.json` when it runs.
    pub(super) pin: Option<Pin>,
    pub(super) request: RunRequest,
    pub(super) root: PathBuf,
    pub(super) cwd: PathBuf,
    pub(super) key: StageKey,
}

/// The scripts a `npm`, `yarn` or `pnpm` command runs, as they were when the
/// user was asked. The manager reads the body when it runs, not the host, so
/// the host keeps the whole `scripts` object (not a hash of it: nothing to
/// collide) and refuses to run when it is not the same after the `y`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) struct Pin {
    /// The sanitized lines the prompt shows: the script and its `pre`/`post`.
    pub(super) shown: Option<String>,
    scripts: String,
}

/// Judges a call. The checks come in the order of the spec; the first refusal
/// is the text the model gets.
pub(super) async fn plan(
    host: &ToolHost,
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
    let (request, root, cwd, pin) = judged;
    Ok(Plan {
        pin,
        request,
        root,
        cwd,
        key,
    })
}

/// Fills in what only the host knows: the script bodies, the timeout the run
/// will have and what an earlier cancelled run changed.
pub(super) fn fill(host: &ToolHost, plan: &Plan, ask: &mut ApprovalAsk) {
    if let ApprovalAsk::Exec {
        script,
        timeout_secs,
        earlier,
        ..
    } = ask
    {
        script.clone_from(&plan.pin.as_ref().and_then(|pin| pin.shown.clone()));
        *timeout_secs = plan.request.timeout.as_secs();
        *earlier = earlier_note(&host.exec.sink(plan.key).peek());
    }
}

/// One line about the changes a cancelled run left in the sink: the names are
/// the program's, so they are cleaned and capped. `None` when there are none.
fn earlier_note(changes: &[Change]) -> Option<String> {
    const NAMES: usize = 3;
    let mut parts: Vec<String> = changes
        .iter()
        .take(NAMES)
        .map(|change| {
            let what = match change.kind {
                Kind::Added => "added",
                Kind::Changed => "changed",
                Kind::Removed => "removed",
                _ => return "could not verify git hooks and config".to_owned(),
            };
            clean(&format!("{what} {}", change.path.display()))
        })
        .collect();
    if parts.is_empty() {
        return None;
    }
    if changes.len() > NAMES {
        parts.push(format!("and {} more", changes.len() - NAMES));
    }
    Some(format!(
        "cancelled, it left git hooks or config touched (not reverted): {}",
        parts.join(", ")
    ))
}

/// The root policy of the chat origin, the folder, the allow-list and what the
/// prompt can show. Reads the disk.
fn inspect(
    env: &ToolEnv,
    origin: &Path,
    args: &Value,
) -> Result<(RunRequest, PathBuf, PathBuf, Option<Pin>), &'static str> {
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
    let package = Package::read(&cwd);
    // A manager walks up to a parent `package.json` that nothing here would
    // show or pin, so a script runs only from the folder that has its own.
    if package.missing && runs_scripts(args) {
        return Err(NO_PACKAGE);
    }
    let request = parse_run(args, &package.names()).map_err(|why| match why {
        RunError::Shape(shape) => refusal_text(shape),
        RunError::Refused(refusal) => refusal.text(),
    })?;
    if request
        .args
        .iter()
        .any(|arg| clean(arg) != *arg || arg.chars().any(char::is_control))
    {
        return Err(UNSHOWABLE);
    }
    let pin = pin(&request, &package);
    Ok((request, root, cwd, pin))
}

/// A command runs against the disk, never against staged content.
fn staged(stages: &Arc<std::sync::Mutex<Stages>>, key: StageKey) -> Result<(), &'static str> {
    match lock(stages).batch(key).map(|batch| batch.state()) {
        Some(State::Staging) => Err(PROPOSE_FIRST),
        Some(State::Proposed) => Err(IN_REVIEW),
        _ => Ok(()),
    }
}

/// The `scripts` of the `package.json` in a folder: none when it is missing,
/// not a plain file, too big or not JSON.
struct Package {
    scripts: serde_json::Map<String, Value>,
    /// There is no plain `package.json` in the folder.
    missing: bool,
}

/// Whether `args` is a `npm`, `yarn` or `pnpm` call of `test` or `run`.
fn runs_scripts(args: &Value) -> bool {
    let manager = args["program"]
        .as_str()
        .and_then(Program::parse)
        .is_some_and(|p| matches!(p, Program::Npm | Program::Pnpm | Program::Yarn));
    manager && matches!(args["args"][0].as_str(), Some("test" | "run"))
}

impl Package {
    fn read(dir: &Path) -> Self {
        let path = dir.join("package.json");
        let none = || Self {
            scripts: Default::default(),
            missing: true,
        };
        let Ok(meta) = fs::symlink_metadata(&path) else {
            return none();
        };
        if !meta.is_file() || meta.len() > PACKAGE_MAX {
            return none();
        }
        let text = fs::read_to_string(&path).unwrap_or_default();
        let scripts = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|json| json["scripts"].as_object().cloned());
        Self {
            scripts: scripts.unwrap_or_default(),
            missing: false,
        }
    }

    fn names(&self) -> Vec<String> {
        self.scripts.keys().cloned().collect()
    }

    fn body(&self, name: &str) -> Option<&str> {
        self.scripts.get(name).and_then(Value::as_str)
    }
}

/// What the user must see, and what must still hold after the `y`, for a
/// package manager. `None` for any other program.
fn pin(request: &RunRequest, package: &Package) -> Option<Pin> {
    if !matches!(
        request.program,
        Program::Npm | Program::Pnpm | Program::Yarn
    ) {
        return None;
    }
    // `install` and `ci` run the lifecycle scripts of the project (and of its
    // dependencies, which the prompt says in general terms).
    let keys: Vec<String> = match request.args.first().map(String::as_str) {
        Some("test") => vec!["pretest".into(), "test".into(), "posttest".into()],
        Some("run") => request
            .args
            .get(1)
            .into_iter()
            .flat_map(|name| [format!("pre{name}"), name.clone(), format!("post{name}")])
            .collect(),
        Some("install" | "ci") => ["preinstall", "install", "postinstall", "prepare"]
            .map(str::to_owned)
            .to_vec(),
        _ => Vec::new(),
    };
    let lines: Vec<String> = keys
        .into_iter()
        .filter_map(|key| package.body(&key).map(|body| script_line(&key, body)))
        .collect();
    Some(Pin {
        shown: (!lines.is_empty()).then(|| lines.join("\n")),
        scripts: Value::Object(package.scripts.clone()).to_string(),
    })
}

/// Runs an approved command: judged again, then one run per chat and two in
/// the app, then the jail. A non-zero exit is an ok result.
pub(super) async fn run(
    host: &ToolHost,
    scope: &TurnScope,
    call: &Call,
    asked: Option<Pin>,
) -> (Outcome, ToolStatus, String) {
    let fail = |why: &str| ended(ToolStatus::Error, why.to_owned(), why);
    // Without a nonce the output cannot be wrapped, so nothing may start.
    let Some(nonce) = new_nonce() else {
        return fail(COULD_NOT);
    };
    let plan = match plan(host, scope, call).await {
        Ok(plan) => plan,
        Err(why) => return fail(why),
    };
    // The disk may have changed while the prompt was open.
    if plan.pin != asked {
        return fail(PACKAGE_CHANGED);
    }
    let permit = match host.exec.gate.acquire(plan.key).await {
        Ok(permit) => permit,
        Err(refused) => return fail(refused.text()),
    };
    let done = go(host, plan, &nonce).await;
    drop(permit);
    done.unwrap_or_else(fail)
}

async fn go(
    host: &ToolHost,
    plan: Plan,
    nonce: &str,
) -> Result<(Outcome, ToolStatus, String), &'static str> {
    let Some(helper) = host.exec.helper.clone() else {
        return Err(NO_SANDBOX);
    };
    let unreported = host.exec.sink(plan.key);
    let read = host.exec.read.clone();
    let parent = scratch_parent(host);
    let argv_text: Vec<String> = std::iter::once(plan.request.program.name().to_owned())
        .chain(plan.request.args.iter().cloned())
        .collect();
    let job = {
        let (read, unreported) = (read.clone(), unreported.clone());
        let home = host.home.clone();
        tokio::task::spawn_blocking(move || job(&*read, home, parent, plan, helper, unreported))
            .await
            .map_err(|_| COULD_NOT)??
    };
    // What the run changed is told on every path, an error included: the
    // watched places are read whatever the program did, and a run that is
    // dropped hands its changes over through the sink.
    let ran = match run_jailed(job, &*read).await {
        Ok(ran) => ran,
        Err(_) => return Ok(failed(COULD_NOT, unreported.drain(), nonce)),
    };
    let mut changes = ran.changes;
    changes.extend(unreported.drain());
    // Only the helper's own status channel says the jail was not applied: the
    // program's output (and its exit 125) can be forged by the program.
    if ran.not_enforced.is_some() {
        return Ok(failed(NOT_ENFORCED, changes, nonce));
    }
    let capture = &ran.done.capture;
    let outcome = RunOutcome::new(argv_text, ran.done.ended, capture, ran.done.duration);
    let mut text = outcome.render(nonce, capture);
    let summary = text.lines().next().unwrap_or_default().to_owned();
    if let Some(note) = changes_note(&changes, nonce) {
        text.push_str(&note);
    }
    if !ran.leftovers.is_empty() {
        text.push_str("\nsome private temporary files could not be removed");
    }
    Ok(ended(ToolStatus::Ok, text, &summary))
}

/// An error result that still says what the run changed.
fn failed(why: &str, changes: Vec<Change>, nonce: &str) -> (Outcome, ToolStatus, String) {
    let mut text = why.to_owned();
    if let Some(note) = changes_note(&changes, nonce) {
        text.push_str(&note);
    }
    ended(ToolStatus::Error, text, why)
}

/// Where the private temp folders go: `$XDG_RUNTIME_DIR`, else the data folder.
fn scratch_parent(host: &ToolHost) -> Option<PathBuf> {
    let absolute = |path: OsString| Some(PathBuf::from(path)).filter(|p| p.is_absolute());
    if let Some(runtime) = (host.exec.read)("XDG_RUNTIME_DIR").and_then(absolute) {
        return Some(runtime.join("lazarobox-run"));
    }
    let data = host.xdg_data_home.clone().and_then(absolute).or_else(|| {
        let home = host.home.clone().and_then(absolute)?;
        Some(home.join(".local/share"))
    })?;
    Some(data.join("lazarobox/run"))
}

/// Builds the job: the scratch folder, the program found on the scrubbed
/// `PATH`, and the sandbox rules. Reads and makes folders: blocking.
fn job(
    read: &(dyn Fn(&str) -> Option<OsString> + Sync),
    home: Option<OsString>,
    parent: Option<PathBuf>,
    plan: Plan,
    helper: PathBuf,
    unreported: Unreported,
) -> Result<Job, &'static str> {
    let canonical = |path: OsString| fs::canonicalize(path).ok();
    let home = home.and_then(canonical).ok_or(REFUSED)?;
    let scratch = Scratch::unique(&parent.ok_or(NO_SANDBOX)?, "run").map_err(|_| COULD_NOT)?;
    let tool = plan.request.program;
    let env = scrub(read, &plan.root, scratch.path());
    let path = env.iter().find(|(name, _)| name == "PATH").map(|(_, v)| v);
    let program = find(tool, &plan.root, path.map(OsString::as_os_str)).ok_or(NOT_FOUND)?;
    let var = |name: &str| read(name).and_then(canonical);
    let layout = Layout {
        tool,
        home,
        root: plan.root.clone(),
        program: program.clone(),
        tmp: scratch.path().to_path_buf(),
        vars: Vars {
            cargo_home: var("CARGO_HOME"),
            rustup_home: var("RUSTUP_HOME"),
            goroot: var("GOROOT"),
        },
    };
    let spec = toolchain_spec(&layout, &|p| crate::runtime::jail::resolve(p))
        .map_err(|why| sandbox_text(&why))?;
    Ok(Job {
        helper,
        spec,
        tool,
        program,
        argv: plan.request.args.iter().map(OsString::from).collect(),
        root: plan.root,
        cwd: plan.cwd,
        scratch,
        timeout: plan.request.timeout,
        unreported,
    })
}

fn sandbox_text(why: &Refusal) -> &'static str {
    match why {
        Refusal::Root(_) => REFUSED,
        Refusal::Program(_) | Refusal::InRoot(_) => NO_PLACE,
        _ => NO_SANDBOX,
    }
}

/// The executable file `tool` names: the project's pytest venv first, then the
/// first hit on `path`, with every link followed. The venv is the project's
/// own, so it counts only while it stays in the project: a link to `/bin/sh`
/// there is a shell, and the model chooses the arguments.
pub(super) fn find(tool: Program, root: &Path, path: Option<&OsStr>) -> Option<PathBuf> {
    let executable = |candidate: PathBuf| {
        let real = fs::canonicalize(candidate).ok()?;
        let meta = fs::metadata(&real).ok()?;
        (meta.is_file() && meta.permissions().mode() & 0o111 != 0).then_some(real)
    };
    let venv = (tool == Program::Pytest)
        .then(|| executable(root.join(".venv/bin/pytest")))
        .flatten()
        .filter(|real| real.starts_with(root));
    venv.or_else(|| {
        path.into_iter()
            .flat_map(std::env::split_paths)
            .find_map(|dir| executable(dir.join(tool.name())))
    })
}

/// What the run did to the watched git places, as text for the model: the
/// names are the program's, so they sit inside the data wrapper. This is the
/// one place that reads an integrity kind.
pub(super) fn changes_note(changes: &[Change], nonce: &str) -> Option<String> {
    if changes.is_empty() {
        return None;
    }
    let checked = |kind: Kind| matches!(kind, Kind::Added | Kind::Changed | Kind::Removed);
    let list: Vec<String> = changes
        .iter()
        .filter(|change| checked(change.kind))
        .map(|change| {
            let what = match change.kind {
                Kind::Added => "added",
                Kind::Changed => "changed",
                _ => "removed",
            };
            format!("{what} {}", change.path.display())
        })
        .collect();
    let mut note = String::new();
    if changes.iter().any(|change| !checked(change.kind)) {
        note.push_str(
            "\ncould not verify git hooks and config after this run: treat the project as \
             tainted and tell the user.",
        );
    }
    if !list.is_empty() {
        note.push_str("\ngit hooks or config changed during the run (not reverted):\n");
        note.push_str(&label::wrap(nonce, &list.join("\n")));
    }
    Some(note)
}
