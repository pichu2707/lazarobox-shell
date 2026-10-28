//! Runtime of the git tools (design D19): the git binary, started through the
//! spawn core inside the toolchain jail, with the argv of `core/ai/tools/git`.
//!
//! The git spec is the run spec without the caches. It adds the read-only
//! global ignore and attributes files (the global config is never granted:
//! the identity travels as `-c`) and the two git dirs of the repo as writable,
//! found by a jailed read-only `rev-parse` preflight. The same preflight reads
//! the repo's effective identity and `core.hooksPath`, the one thing that runs
//! outside the jail (`git config` only reads values), and checks that git is
//! 2.32 or newer, as `GIT_CONFIG_GLOBAL` needs.

use std::ffi::OsString;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::core::ai::jail::{JailArgs, JailSpec, NetBind, Os, Rlimits, broad_place};
use crate::core::ai::toolchain::env::scrub;
use crate::core::ai::toolchain::{Layout, Refusal, Vars, toolchain_spec};
use crate::core::ai::tools::allow::Program;
use crate::core::ai::tools::git::{self as pure, GitDir, GitError, Identity};
use crate::runtime::tools::integrity::{self, Change};
use crate::runtime::tools::open::open_beneath;
use crate::runtime::tools::runner::{AfterOnDrop, Done, Run, Unreported, spawn_core, watched};
use crate::runtime::tools::scratch::Scratch;

type Read<'a> = &'a (dyn Fn(&str) -> Option<OsString> + Sync);

/// What a git run needs from the caller. Every path is canonical.
#[derive(Clone, Debug)]
pub struct Ctx {
    /// The `lazarobox` binary that serves `__jail-exec`.
    pub helper: PathBuf,
    /// The git binary.
    pub git: PathBuf,
    pub home: PathBuf,
    /// The project root (the worktree).
    pub root: PathBuf,
    /// Where each run's private `TMPDIR` is made.
    pub scratch_parent: PathBuf,
    pub timeout: Duration,
    /// Where the changes of a git run that is dropped before it ends go (see
    /// [`Unreported`]). Keep a clone and drain it after each call.
    pub unreported: Unreported,
}

/// Why a git run could not start. Short and content-free.
#[derive(Debug)]
pub enum Fault {
    /// The jail rules were refused (this git path outside a known system
    /// place, a broad root). git must live in a system place such as
    /// `/usr/bin`: a user-installed git is refused by design.
    Spec(PathBuf),
    /// A git dir is broad, next to an execute place, not a clean path, or a
    /// repository outside the root that is not a genuine linked worktree of
    /// it (a planted `.git` file, an unreadable or oversized pointer file).
    Dir,
    /// The environment holds a git variable it must not.
    Env,
    /// Git is older than 2.32 or did not say its version.
    Version,
    /// `rev-parse` failed or printed something else than two paths.
    Preflight,
    Io(io::Error),
}

impl From<io::Error> for Fault {
    fn from(e: io::Error) -> Self {
        Fault::Io(e)
    }
}

/// The folders a run reaches beyond the root.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dirs {
    /// Read only: where a linked worktree points, for the preflight.
    pub ro: Vec<PathBuf>,
    /// Read and write: the git dir and the common dir.
    pub rw: Vec<PathBuf>,
}

/// An opened repo: what the preflight found.
#[derive(Debug)]
pub struct Repo {
    pub dir: GitDir,
    pub dirs: Dirs,
    /// The repo's effective identity, or the fixed error a commit gets.
    pub identity: Result<Identity, GitError>,
    /// `core.hooksPath` as configured (empty when unset), for `hooks_notice`.
    pub configured_hooks: String,
}

/// A finished git run.
#[derive(Debug)]
pub struct Ran {
    pub done: Done,
    /// What of the private folder could not be removed.
    pub leftovers: Vec<PathBuf>,
    /// The watched places (hooks, config) the run changed, in the root and in
    /// a linked worktree's git dirs (absolute paths there). X-c3 semantics,
    /// as in the runner's `Ran`; empty for the read-only preflight calls.
    pub changes: Vec<Change>,
}

/// The rules for one git run: the toolchain rules without caches, no
/// listening sockets, the global ignore and attributes read-only, `dirs` as
/// asked. A git dir under the root needs no rule (the root holds it).
pub fn jail_spec(
    layout: &Layout,
    resolve: &dyn Fn(&Path) -> Option<PathBuf>,
    dirs: &Dirs,
    xdg: &Path,
) -> Result<JailSpec, Refusal> {
    let mut spec = toolchain_spec(layout, resolve)?;
    spec.rw = vec![layout.tmp.clone()];
    spec.net_bind = NetBind::Closed;
    spec.rlimits = Rlimits::Toolchain;
    for name in ["ignore", "attributes"] {
        if let Some(file) = resolve(&xdg.join("git").join(name)) {
            spec.ro_files.push(file);
        }
    }
    let real_home = resolve(&layout.home).unwrap_or_else(|| layout.home.clone());
    let homes = [layout.home.as_path(), real_home.as_path()];
    for (list, dir) in dirs
        .ro
        .iter()
        .map(|d| (false, d))
        .chain(dirs.rw.iter().map(|d| (true, d)))
    {
        if broad_place(dir, &homes) {
            return Err(Refusal::Root(dir.clone()));
        }
        if dir.starts_with(&layout.root) {
            continue;
        }
        if list {
            spec.rw.push(dir.clone());
        } else {
            spec.ro.push(dir.clone());
        }
    }
    // R29 again, now with the git dirs in.
    spec.check_exec(Os::Linux, &homes)
        .map_err(|_| Refusal::Root(layout.root.clone()))?;
    Ok(spec)
}

/// The names a git run may get besides the `GIT_*` ones.
const PASSED: [&str; 7] = ["PATH", "HOME", "USER", "LANG", "LC_ALL", "TMPDIR", "TERM"];

/// The environment of a git run: cleared, then the short list above (and
/// `XDG_CONFIG_HOME` when set) and the fixed git values. Checked before use.
pub fn git_env(
    read: Read,
    root: &Path,
    tmp: &Path,
    read_verb: bool,
) -> Result<Vec<(OsString, OsString)>, Fault> {
    let mut env: Vec<_> = scrub(read, root, tmp)
        .into_iter()
        .filter(|(name, _)| PASSED.iter().any(|p| name == p))
        .collect();
    if let Some(xdg) = read("XDG_CONFIG_HOME") {
        env.push((OsString::from("XDG_CONFIG_HOME"), xdg));
    }
    env.extend(
        pure::env(read_verb)
            .into_iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v))),
    );
    check_env(&env)?;
    Ok(env)
}

/// Asserts the environment is the cleared one: every fixed git value is there
/// and no other `GIT_*` name is (`GIT_EXTERNAL_DIFF`, `GIT_PAGER`,
/// `GIT_ASKPASS`, `GIT_SSH`, `GIT_CONFIG_COUNT`/`KEY_*`/`VALUE_*`,
/// `GIT_CONFIG_PARAMETERS`, `GIT_DIR`, `GIT_WORK_TREE` and the rest).
pub fn check_env(env: &[(OsString, OsString)]) -> Result<(), Fault> {
    let fixed = pure::env(false);
    let foreign = env.iter().any(|(name, _)| {
        let name = name.to_string_lossy();
        name.starts_with("GIT_")
            && name != "GIT_OPTIONAL_LOCKS"
            && !fixed.iter().any(|(k, _)| *k == name)
    });
    let complete = fixed
        .iter()
        .all(|(k, v)| env.iter().any(|(name, value)| name == k && value == v));
    if foreign || !complete {
        return Err(Fault::Env);
    }
    Ok(())
}

/// Whether `git --version` output names 2.32 or newer.
pub fn version_ok(text: &str) -> bool {
    let Some(number) = text.strip_prefix("git version ") else {
        return false;
    };
    let mut parts = number
        .split(|c: char| !c.is_ascii_digit())
        .map(|n| n.parse::<u32>());
    match (parts.next(), parts.next()) {
        (Some(Ok(major)), Some(Ok(minor))) => (major, minor) >= (2, 32),
        _ => false,
    }
}

/// The most a pointer file (`.git`, `commondir`, `gitdir`) may hold.
const POINTER_CAP: u64 = 4096;

/// A small text file read without blocking or following a link: a regular
/// file of at most [`POINTER_CAP`] bytes. `None` when it is absent.
fn read_small(path: &Path) -> Result<Option<String>, Fault> {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(Fault::Dir);
    };
    // No link is followed in any component; the type and size are those of the
    // opened file, so nothing can swap in between.
    let file = match open_beneath(parent, Path::new(name), false) {
        Ok(fd) => std::fs::File::from(fd),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Fault::Dir),
    };
    let meta = file.metadata().map_err(|_| Fault::Dir)?;
    if !meta.is_file() || meta.len() > POINTER_CAP {
        return Err(Fault::Dir);
    }
    let mut text = String::new();
    file.take(POINTER_CAP)
        .read_to_string(&mut text)
        .map_err(|_| Fault::Dir)?;
    Ok(Some(text))
}

/// Where a linked worktree's `.git` file points: its git dir and the common
/// dir that holds. Empty for a plain repo and for dirs under the root. A git
/// dir outside the root is accepted only when it is a genuine linked worktree
/// of this root: it sits directly under `<common>/worktrees/`, its `commondir`
/// names that common dir and its `gitdir` back-link names `<root>/.git`.
/// Anything else (a planted pointer at another repo) is [`Fault::Dir`].
/// Blocking (small, bounded reads): call it off the async runtime.
pub fn pointer_dirs(root: &Path) -> Result<Vec<PathBuf>, Fault> {
    let dot = root.join(".git");
    match std::fs::symlink_metadata(&dot) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(Fault::Dir),
        Ok(meta) if meta.is_dir() => return Ok(Vec::new()),
        Ok(_) => {}
    }
    let Some(text) = read_small(&dot)? else {
        return Ok(Vec::new());
    };
    let Some(target) = text.lines().next().and_then(|l| l.strip_prefix("gitdir:")) else {
        return Ok(Vec::new());
    };
    let canon = |p: PathBuf| std::fs::canonicalize(p).map_err(|_| Fault::Dir);
    let real_root = canon(root.to_path_buf())?;
    let git_dir = canon(root.join(target.trim()))?;
    let common = match read_small(&git_dir.join("commondir"))? {
        Some(rel) => Some(canon(git_dir.join(rel.trim()))?),
        None => None,
    };
    let shared = common.clone().unwrap_or_else(|| git_dir.clone());
    if git_dir.starts_with(&real_root) && shared.starts_with(&real_root) {
        return Ok(Vec::new());
    }
    let common = common.ok_or(Fault::Dir)?;
    if git_dir.parent() != Some(&common.join("worktrees")) {
        return Err(Fault::Dir);
    }
    let back = read_small(&git_dir.join("gitdir"))?.ok_or(Fault::Dir)?;
    if canon(git_dir.join(back.trim()))? != real_root.join(".git") {
        return Err(Fault::Dir);
    }
    Ok(vec![git_dir, common])
}

/// One jailed git call: a private `TMPDIR`, the cleared environment, the
/// spawn core. `dirs` are the folders this call may reach.
async fn jailed(
    ctx: &Ctx,
    dirs: &Dirs,
    argv: Vec<String>,
    read_verb: bool,
    read: Read<'_>,
    watch: bool,
) -> Result<Ran, Fault> {
    let scratch = Scratch::unique(&ctx.scratch_parent, "git")?;
    let layout = Layout {
        // The placeholder only picks the program places (the toolchain's rx
        // set), not what runs: git must be in one of those system places, and a
        // git anywhere else is refused by design (`Fault::Spec` names it).
        tool: Program::Cargo,
        home: ctx.home.clone(),
        root: ctx.root.clone(),
        program: ctx.git.clone(),
        tmp: scratch.path().to_path_buf(),
        vars: Vars::default(),
    };
    let xdg = read("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| ctx.home.join(".config"));
    let resolve = |p: &Path| std::fs::canonicalize(p).ok();
    let spec =
        jail_spec(&layout, &resolve, dirs, &xdg).map_err(|_| Fault::Spec(ctx.git.clone()))?;
    let env = git_env(read, &ctx.root, scratch.path(), read_verb)?;
    let args = JailArgs {
        min_abi: None,
        arg0: Some(OsString::from("git")),
        spec,
        program: ctx.git.clone(),
        argv: argv.into_iter().map(OsString::from).collect(),
    };
    // Hooks and filter drivers run in the jail and can write the git dirs:
    // look at the watched places before and after, like the runner does.
    let extra: Vec<PathBuf> = if watch {
        dirs.rw
            .iter()
            .filter(|d| !d.starts_with(&ctx.root))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    let mut guard = if watch {
        Some(AfterOnDrop::new(
            ctx.root.clone(),
            extra.clone(),
            watched(&ctx.root, &extra).await,
            ctx.unreported.clone(),
        ))
    } else {
        None
    };
    let done = spawn_core(Run {
        program: ctx.helper.clone(),
        argv: args.render(),
        cwd: ctx.root.clone(),
        env,
        timeout: ctx.timeout,
    })
    .await?;
    let changes = match guard.as_mut() {
        Some(guard) => {
            let after = watched(&ctx.root, &extra).await;
            integrity::compare(&guard.disarm(), &after)
        }
        None => Vec::new(),
    };
    Ok(Ran {
        done,
        leftovers: scratch.finish(),
        changes,
    })
}

/// Runs one git call (an argv from `core/ai/tools/git`) in the repo's jail.
pub async fn run(
    ctx: &Ctx,
    repo: &Repo,
    argv: Vec<String>,
    read_verb: bool,
    read: Read<'_>,
) -> Result<Ran, Fault> {
    jailed(ctx, &repo.dirs, argv, read_verb, read, true).await
}

/// The text of a finished call, when it ended well.
fn ok_text(done: &Done) -> Option<String> {
    let ended = &done.ended;
    (ended.exit == Some(0) && !ended.timed_out).then(|| done.capture.text())
}

/// The preflight: git version, the two git dirs, the identity and the
/// configured hooks path. Nothing is written.
pub async fn open(ctx: &Ctx, read: Read<'_>) -> Result<Repo, Fault> {
    let root = ctx.root.clone();
    let ro = tokio::task::spawn_blocking(move || pointer_dirs(&root))
        .await
        .map_err(|e| Fault::Io(io::Error::other(e)))??;
    let reach = Dirs { ro, rw: Vec::new() };
    let version = jailed(ctx, &reach, vec!["--version".into()], true, read, false).await?;
    if !ok_text(&version.done).is_some_and(|t| version_ok(&t)) {
        return Err(Fault::Version);
    }
    let argv = [
        "rev-parse",
        "--path-format=absolute",
        "--absolute-git-dir",
        "--git-common-dir",
    ];
    let found = jailed(
        ctx,
        &reach,
        argv.map(String::from).to_vec(),
        true,
        read,
        false,
    )
    .await?;
    let text = ok_text(&found.done).ok_or(Fault::Preflight)?;
    let [git_dir, common] = text.lines().collect::<Vec<_>>()[..] else {
        return Err(Fault::Preflight);
    };
    let canon = |p: &str| std::fs::canonicalize(p).map_err(|_| Fault::Dir);
    let (git_dir, common) = (canon(git_dir)?, canon(common)?);
    // Git must report only dirs the pointer check vouched for (or the root's).
    let vouched = |d: &Path| d.starts_with(&ctx.root) || reach.ro.iter().any(|r| r == d);
    if !vouched(&git_dir) || !vouched(&common) {
        return Err(Fault::Dir);
    }
    let name = |p: &Path| p.to_str().map(str::to_owned).ok_or(Fault::Dir);
    let dir = GitDir::new(&name(&git_dir)?, &name(&common)?).map_err(|_| Fault::Dir)?;
    let mut rw = vec![git_dir];
    if common != rw[0] {
        rw.push(common);
    }
    let (identity, configured_hooks) = config(ctx, read).await?;
    Ok(Repo {
        dir,
        dirs: Dirs { ro: Vec::new(), rw },
        identity,
        configured_hooks,
    })
}

/// Reads `user.name`, `user.email` and `core.hooksPath` as the repo sees
/// them. It runs outside the jail so the global config counts (a repo-level
/// value wins); `git config` only reads, and the values are validated by
/// [`Identity::new`] before they go anywhere.
async fn config(ctx: &Ctx, read: Read<'_>) -> Result<(Result<Identity, GitError>, String), Fault> {
    // PATH goes through the same filter as a jailed run's (no project entry).
    let mut env: Vec<(OsString, OsString)> = scrub(read, &ctx.root, &ctx.scratch_parent)
        .into_iter()
        .filter(|(name, _)| name == "PATH")
        .collect();
    env.extend(
        ["HOME", "XDG_CONFIG_HOME"]
            .iter()
            .filter_map(|n| read(n).map(|v| (OsString::from(n), v))),
    );
    env.push(("GIT_CONFIG_NOSYSTEM".into(), "1".into()));
    env.push(("GIT_TERMINAL_PROMPT".into(), "0".into()));
    let done = spawn_core(Run {
        program: ctx.git.clone(),
        argv: [
            "config",
            "-z",
            "--get-regexp",
            "^(user\\.name|user\\.email|core\\.hookspath)$",
        ]
        .map(OsString::from)
        .to_vec(),
        cwd: ctx.root.clone(),
        env,
        timeout: ctx.timeout,
    })
    .await?;
    // Exit 1 is "no such key": an unset identity, not a failure. Any other
    // end, a timeout or a cut capture would be read as "unset": refuse it.
    let ended = &done.ended;
    if !matches!(ended.exit, Some(0 | 1)) || ended.timed_out || done.capture.truncated() {
        return Err(Fault::Preflight);
    }
    let bytes = done.capture.bytes();
    let text = String::from_utf8_lossy(&bytes);
    let (mut name, mut email, mut hooks) = (String::new(), String::new(), String::new());
    for record in text.split('\0') {
        let (key, value) = record.split_once('\n').unwrap_or((record, ""));
        match key {
            "user.name" => name = value.to_owned(),
            "user.email" => email = value.to_owned(),
            "core.hookspath" => hooks = value.to_owned(),
            _ => {}
        }
    }
    Ok((Identity::new(&name, &email), hooks))
}

/// What a status call gave.
#[derive(Debug, PartialEq, Eq)]
pub enum Output {
    Text(String),
    /// The output was cut: a cut record must not be parsed, say so instead.
    Truncated,
    Failed,
}

/// The raw text of a finished `git status -z`, or why there is none.
pub fn status_output(done: &Done) -> Output {
    let ended = &done.ended;
    if ended.exit != Some(0) || ended.timed_out {
        Output::Failed
    } else if done.capture.truncated() {
        Output::Truncated
    } else {
        Output::Text(String::from_utf8_lossy(&done.capture.bytes()).into_owned())
    }
}

#[cfg(test)]
mod tests;
