//! The check half of a run (design D21): the path policy for the exact bytes
//! of a safe command, against the shell's real directory. The disk work runs
//! on the blocking pool under a bound; a failure or timeout is a refusal.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use tokio::{sync::mpsc, time::timeout};

use crate::{
    app::{RunEvent, RunScope},
    core::{
        ai::{
            chat::{
                approval::{BROAD_ROOT, GIT_NOT_PLAIN, NOT_ALLOWED, OUTSIDE_ROOT},
                engine::CallId,
            },
            tools::{
                exec::{REVISION, Verb, git_config, parse},
                paths::{PathVerdict, SecretRoots, is_excluded_name, is_git_config, resolve_exec},
            },
        },
        layout::PaneId,
    },
    runtime::tools::{ToolEnv, fs::load_roots},
};

/// Longest the disk work of one check may take.
pub const TIMEOUT: Duration = Duration::from_secs(2);
pub const TIMED_OUT: &str = "the path check did not finish in time";
const NO_ROOTS: &str = "the secret locations cannot be read";
/// Why a recursive search does not run on its own: its folder holds more
/// entries than the check looks at.
pub const TOO_LARGE: &str = "the folder is too large to check";
/// The most entries the check of a recursive `grep` looks at.
pub const MAX_SEARCHED: usize = 5_000;

/// What the check reads from the process: the same environment the read tools
/// judge paths by.
pub fn process_env() -> ToolEnv {
    ToolEnv {
        home: std::env::var_os("HOME"),
        xdg_config_home: std::env::var_os("XDG_CONFIG_HOME"),
        xdg_data_home: std::env::var_os("XDG_DATA_HOME"),
        origin: PathBuf::from("/"),
    }
}

/// Starts the checks of runs and sends their answers to the event loop.
pub struct Checker {
    env: ToolEnv,
    tx: mpsc::Sender<(PaneId, RunEvent)>,
}

impl Checker {
    pub fn new(env: ToolEnv, tx: mpsc::Sender<(PaneId, RunEvent)>) -> Self {
        Self { env, tx }
    }

    /// Checks `command` for `pane` against `cwd` on the blocking pool and
    /// sends exactly one answer. Returns at once; needs a tokio runtime.
    pub fn start(
        &self,
        pane: PaneId,
        call: CallId,
        cwd: PathBuf,
        command: String,
        scope: RunScope,
    ) {
        let (env, tx) = (self.env.clone(), self.tx.clone());
        tokio::spawn(async move {
            let work = move || check(&env, call, cwd, &command, &scope);
            let event = within(TIMEOUT, call, work).await;
            let _ = tx.send((pane, event)).await;
        });
    }
}

/// Parses `command` again (the same bytes the app routed) and judges every
/// path it touches against `cwd`, the shell's real directory, and against
/// the chat's root in `scope`. The answer carries `cwd` and, for a `cd`, the
/// real directory it goes to.
///
/// In order: a cloud chat whose root is HOME or `/` runs nothing on its own
/// ([`BROAD_ROOT`]); a refused path, or a search that would read a sensitive
/// file below the folder it walks, is [`NOT_ALLOWED`]; a path the command
/// acts on outside the root, or the repository of a git command starting
/// above it, is [`OUTSIDE_ROOT`]; a git argument that is no existing path is
/// a [`REVISION`]; a git command outside a plain repository (see
/// [`plain_repository`]) or with none found is [`GIT_NOT_PLAIN`]. Each one opens the prompt; only
/// [`NOT_ALLOWED`] counts toward the pause.
pub fn check(
    env: &ToolEnv,
    call: CallId,
    cwd: PathBuf,
    command: &str,
    scope: &RunScope,
) -> RunEvent {
    let refused = |reason| RunEvent::Refused { call, reason };
    let safe = match parse(command) {
        Ok(safe) => safe,
        Err(fallback) => return refused(fallback.why),
    };
    let Ok(roots) = load_roots(env) else {
        return refused(NO_ROOTS);
    };
    let Ok(root) = scope.root.canonicalize() else {
        return refused(OUTSIDE_ROOT);
    };
    // HOME or a folder that holds it, `/` among them (L14).
    if scope.cloud && roots.home().starts_with(&root) {
        return refused(BROAD_ROOT);
    }
    let git = safe.verb() == Verb::Git;
    let (mut cd_target, mut outside, mut revision) = (None, false, false);
    for target in safe.targets() {
        // A git argument that names nothing on disk is a revision (`main`).
        if git && target.input != "." && !exists(roots.home(), &cwd, target.input) {
            revision = true;
            continue;
        }
        let PathVerdict::Allowed(real) = resolve_exec(&roots, &cwd, target.input, target.want)
        else {
            return refused(NOT_ALLOWED);
        };
        if target.confined && !real.starts_with(&root) {
            outside = true;
        } else if target.searched
            && let Err(reason) = searchable(&roots, &real, safe.verb() == Verb::Grep)
        {
            return refused(reason);
        }
        if safe.verb() == Verb::Cd {
            cd_target = Some(real);
        }
    }
    if outside {
        return refused(OUTSIDE_ROOT);
    }
    if git {
        let Some(top) = repository(&cwd) else {
            return refused(GIT_NOT_PLAIN);
        };
        if !top.starts_with(&root) {
            return refused(OUTSIDE_ROOT);
        }
        if revision {
            return refused(REVISION);
        }
        if !plain_repository(&top) {
            return refused(GIT_NOT_PLAIN);
        }
    }
    RunEvent::Checked {
        call,
        cwd,
        cd_target,
    }
}

/// Whether a search may read the files below `real`, a resolved path: no
/// secret root lies below it, and, for `grep -r`, which reads hidden files
/// too, no entry it would read is sensitive by name, position or as a git
/// setting. `rg` skips hidden entries by default and its flags that read them
/// are not allowed, so only the secret roots are looked for. Links are not
/// followed, as neither tool follows them below its arguments. A folder with
/// more than [`MAX_SEARCHED`] entries is not looked through.
fn searchable(roots: &SecretRoots, real: &Path, hidden: bool) -> Result<(), &'static str> {
    if roots.holds_root(real) {
        return Err(NOT_ALLOWED);
    }
    if !hidden || !real.is_dir() {
        return Ok(());
    }
    let (mut queue, mut seen) = (vec![real.to_path_buf()], 0usize);
    while let Some(dir) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > MAX_SEARCHED {
                return Err(TOO_LARGE);
            }
            let (path, name) = (entry.path(), entry.file_name());
            let sensitive = is_excluded_name(&name.to_string_lossy())
                || is_git_config(&path)
                || !roots.judge_real(&path);
            if sensitive {
                return Err(NOT_ALLOWED);
            }
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                queue.push(path);
            }
        }
    }
    Ok(())
}

/// Whether `input`, read against `cwd` with `~` expanded to `home`, names
/// something on disk (a link counts, even a dangling one).
fn exists(home: &Path, cwd: &Path, input: &str) -> bool {
    let full = match input.strip_prefix('~') {
        Some("") => home.to_path_buf(),
        Some(rest) => match rest.strip_prefix('/') {
            Some(rest) => home.join(rest),
            None => return false,
        },
        None => cwd.join(input),
    };
    full.symlink_metadata().is_ok()
}

/// The largest config file the check reads.
const MAX_CONFIG_BYTES: u64 = 1 << 20;

/// Whether the repository whose work tree is `top` is a plain one, the only
/// kind git runs in on its own (L29): its `.git` is a real folder, or a
/// file naming the git folder of a linked worktree, that git takes for a git
/// folder; its config (the shared one and `config.worktree`) sets only known
/// harmless keys ([`git_config::is_plain`]); no hook is installed; it has no
/// submodule and no object alternates. Anything that cannot be read is not
/// plain.
fn plain_repository(top: &Path) -> bool {
    let Some(git_dir) = git_dir(top) else {
        return false;
    };
    let common = match std::fs::read_to_string(git_dir.join("commondir")) {
        Ok(text) => git_dir.join(text.trim()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => git_dir.clone(),
        Err(_) => return false,
    };
    if !git_dir.join("HEAD").is_file() || !looks_like_git_dir(&common) {
        return false;
    }
    let worktree = git_dir.join("config.worktree");
    let configs = std::iter::once(common.join("config"))
        .chain(worktree.symlink_metadata().is_ok().then_some(worktree));
    for file in configs {
        let size = std::fs::metadata(&file).map(|m| m.len());
        let text = std::fs::read_to_string(&file).ok();
        if !size.is_ok_and(|n| n <= MAX_CONFIG_BYTES)
            || !text.is_some_and(|t| git_config::is_plain(&t))
        {
            return false;
        }
    }
    let present = |path: PathBuf| path.symlink_metadata().is_ok();
    let extras = [
        top.join(".gitmodules"),
        git_dir.join("modules"),
        common.join("modules"),
        common.join("objects/info/alternates"),
        common.join("objects/info/http-alternates"),
    ];
    !extras.into_iter().any(present) && no_hooks(&git_dir) && no_hooks(&common)
}

/// Whether the `hooks` folder of a git folder holds only `.sample` files
/// (or does not exist). One that cannot be read holds something.
fn no_hooks(git_dir: &Path) -> bool {
    match std::fs::read_dir(git_dir.join("hooks")) {
        Ok(entries) => entries
            .into_iter()
            .all(|entry| entry.is_ok_and(|e| e.file_name().to_string_lossy().ends_with(".sample"))),
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
    }
}

/// Whether git takes `dir` for a git folder: a `HEAD` file, `objects/` and
/// `refs/`.
fn looks_like_git_dir(dir: &Path) -> bool {
    dir.join("HEAD").is_file() && dir.join("objects").is_dir() && dir.join("refs").is_dir()
}

/// The git folder of the repository at `top`: `.git` itself when it is a
/// real folder, or the folder the `gitdir:` line of a `.git` file names
/// (relative to `top`). A `.git` link is none: it may lead anywhere.
fn git_dir(top: &Path) -> Option<PathBuf> {
    let dot = top.join(".git");
    let meta = dot.symlink_metadata().ok()?;
    if meta.is_dir() {
        return Some(dot);
    }
    if !meta.is_file() {
        return None;
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    let named = text.lines().next()?.strip_prefix("gitdir:")?.trim();
    (!named.is_empty()).then(|| top.join(named))
}

/// The work tree top of the repository git finds from `cwd`, the way git
/// looks: in each folder from `cwd` up, a `.git` entry first, then the folder
/// itself as a bare git folder. `None` when the first one found is a git
/// folder of its own (a bare repository inside the project, or `cwd` within
/// a `.git`), or when there is no repository at all: git would read a
/// config this check did not.
fn repository(cwd: &Path) -> Option<PathBuf> {
    for dir in cwd.ancestors() {
        if dir.join(".git").symlink_metadata().is_ok() {
            return Some(dir.to_path_buf());
        }
        if looks_like_git_dir(dir) {
            return None;
        }
    }
    None
}

/// Runs `work` on the blocking pool for at most `bound`. A check that is too
/// slow, or whose thread dies, is refused; the thread itself cannot be
/// stopped, but nothing waits for it.
pub async fn within(
    bound: Duration,
    call: CallId,
    work: impl FnOnce() -> RunEvent + Send + 'static,
) -> RunEvent {
    match timeout(bound, tokio::task::spawn_blocking(work)).await {
        Ok(Ok(event)) => event,
        _ => RunEvent::Refused {
            call,
            reason: TIMED_OUT,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct Fixture {
        _dir: tempfile::TempDir,
        home: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for sub in ["home/dev/app", "home/.ssh", "srv/data"] {
            fs::create_dir_all(root.join(sub)).unwrap();
        }
        Fixture {
            home: root.join("home"),
            _dir: dir,
        }
    }

    fn env(home: Option<&Path>) -> ToolEnv {
        ToolEnv {
            home: home.map(|h| h.as_os_str().to_owned()),
            xdg_config_home: None,
            xdg_data_home: None,
            origin: PathBuf::from("/"),
        }
    }

    /// A local engine whose root is `/`: every path is in the root, so the
    /// path policy alone decides, as before the root rule.
    fn anywhere() -> RunScope {
        RunScope {
            root: PathBuf::from("/"),
            cloud: false,
        }
    }

    fn run(f: &Fixture, cwd: &str, command: &str) -> RunEvent {
        check(
            &env(Some(&f.home)),
            CallId(4),
            f.home.join(cwd),
            command,
            &anywhere(),
        )
    }

    fn reason(event: &RunEvent) -> Option<&'static str> {
        match event {
            RunEvent::Refused { reason, .. } => Some(reason),
            RunEvent::Checked { .. } | RunEvent::NotRun { .. } | RunEvent::Probe { .. } => None,
        }
    }

    // S3.8: the answer carries the checked cwd and the resolved cd target.
    #[test]
    fn a_safe_command_is_checked_against_the_real_cwd_and_answers_with_it() {
        let f = fixture();
        assert_eq!(
            run(&f, "dev", "ls app"),
            RunEvent::Checked {
                call: CallId(4),
                cwd: f.home.join("dev"),
                cd_target: None,
            }
        );
        assert_eq!(
            run(&f, "dev", "cd app"),
            RunEvent::Checked {
                call: CallId(4),
                cwd: f.home.join("dev"),
                cd_target: Some(f.home.join("dev/app")),
            }
        );
        assert!(matches!(
            run(&f, "dev", "rg -n foo app"),
            RunEvent::Checked { .. }
        ));
    }

    #[test]
    fn a_path_outside_home_runs_unless_it_is_secret() {
        let f = fixture();
        let data = f.home.parent().unwrap().join("srv/data");
        let cd = format!("cd {}", data.display());
        assert!(matches!(run(&f, "dev", &cd), RunEvent::Checked { .. }));
        assert!(reason(&run(&f, "dev", "cd ../.ssh")).is_some());
        assert!(reason(&run(&f, "dev", "cd ~/.ssh")).is_some());
        assert!(reason(&run(&f, ".ssh", "ls")).is_some(), "the cwd itself");
    }

    #[test]
    fn ls_and_cd_go_outside_home_but_tree_and_a_search_do_not() {
        let f = fixture();
        let srv = f.home.parent().unwrap().join("srv");
        let abs = |verb: &str, rest: &str| format!("{verb} {}{rest}", srv.display());
        for command in [abs("ls", ""), abs("ls -la", "/data"), abs("cd", "/data")] {
            assert!(
                matches!(run(&f, "dev", &command), RunEvent::Checked { .. }),
                "{command}"
            );
        }
        for command in [
            abs("tree", ""),
            abs("tree -d", "/data"),
            abs("rg foo", "/data"),
        ] {
            assert_eq!(
                reason(&run(&f, "dev", &command)),
                Some(NOT_ALLOWED),
                "{command}"
            );
        }
        // Inside HOME the confined verbs still run; the cwd counts for tree.
        assert!(matches!(
            run(&f, "dev", "tree app"),
            RunEvent::Checked { .. }
        ));
        let outside_cwd = check(
            &env(Some(&f.home)),
            CallId(4),
            srv.clone(),
            "tree",
            &anywhere(),
        );
        assert_eq!(reason(&outside_cwd), Some(NOT_ALLOWED));
        let ls_there = check(&env(Some(&f.home)), CallId(4), srv, "ls", &anywhere());
        assert!(matches!(ls_there, RunEvent::Checked { .. }));
    }

    #[test]
    fn a_link_outside_home_into_a_secret_folder_is_refused_for_ls_and_cd() {
        let f = fixture();
        let link = f.home.parent().unwrap().join("srv/to-ssh");
        std::os::unix::fs::symlink(f.home.join(".ssh"), &link).unwrap();
        for verb in ["ls", "ls -la", "cd", "tree"] {
            let command = format!("{verb} {}", link.display());
            assert_eq!(
                reason(&run(&f, "dev", &command)),
                Some(NOT_ALLOWED),
                "{command}"
            );
        }
        assert_eq!(reason(&run(&f, "dev", "ls ~/.ssh")), Some(NOT_ALLOWED));
    }

    #[test]
    fn tree_with_a_depth_stays_inside_home_and_dot_dot_cannot_leave_it() {
        let f = fixture();
        let srv = f.home.parent().unwrap().join("srv/data");
        assert!(matches!(
            run(&f, "dev", "tree -L 2 app"),
            RunEvent::Checked { .. }
        ));
        let outside = format!("tree -L 2 {}", srv.display());
        assert_eq!(reason(&run(&f, "dev", &outside)), Some(NOT_ALLOWED));
        // `../..` from dev/ is the folder that holds HOME: outside it.
        assert_eq!(reason(&run(&f, "dev", "tree ../..")), Some(NOT_ALLOWED));
        assert!(matches!(
            run(&f, "dev", "tree .."),
            RunEvent::Checked { .. }
        ));
    }

    #[test]
    fn a_link_inside_home_to_the_outside_is_listed_by_ls_and_cd_not_by_tree() {
        let f = fixture();
        let target = f.home.parent().unwrap().join("srv/data");
        std::os::unix::fs::symlink(&target, f.home.join("dev/out")).unwrap();
        for command in ["ls out", "cd out"] {
            assert!(
                matches!(run(&f, "dev", command), RunEvent::Checked { .. }),
                "{command}"
            );
        }
        assert_eq!(reason(&run(&f, "dev", "tree out")), Some(NOT_ALLOWED));
    }

    // `/proc/self` is lazarobox's own process, not the pane's shell. That is
    // conservative: the shell's own cwd is checked separately.
    #[test]
    fn a_proc_root_path_into_a_secret_folder_is_refused() {
        if !Path::new("/proc/self/root").exists() {
            return;
        }
        let f = fixture();
        let secret = format!("/proc/self/root{}/.ssh", f.home.display());
        for verb in ["ls", "cd", "ls -la"] {
            let command = format!("{verb} {secret}");
            assert_eq!(
                reason(&run(&f, "dev", &command)),
                Some(NOT_ALLOWED),
                "{command}"
            );
        }
        // The same prefix on an ordinary folder still resolves and runs.
        let plain = format!("ls /proc/self/root{}/dev", f.home.display());
        assert!(matches!(run(&f, "dev", &plain), RunEvent::Checked { .. }));
    }

    #[test]
    fn a_search_outside_home_and_a_missing_directory_are_refused() {
        let f = fixture();
        assert_eq!(reason(&run(&f, "dev", "rg x ../../srv")), Some(NOT_ALLOWED));
        assert!(reason(&run(&f, "dev", "cd nowhere")).is_some());
    }

    #[test]
    fn the_command_is_parsed_again_and_the_grammar_reason_comes_back() {
        let f = fixture();
        assert_eq!(reason(&run(&f, "dev", "ls *")), Some("shell metacharacter"));
        assert_eq!(
            reason(&run(&f, "dev", "cargo build")),
            Some("not a safe command")
        );
    }

    #[test]
    fn roots_that_cannot_be_loaded_refuse_everything() {
        let f = fixture();
        let none = check(&env(None), CallId(1), f.home.clone(), "pwd", &anywhere());
        assert!(reason(&none).is_some());
        let relative = ToolEnv {
            home: Some("relative".into()),
            ..env(None)
        };
        assert!(
            reason(&check(
                &relative,
                CallId(1),
                f.home.clone(),
                "pwd",
                &anywhere()
            ))
            .is_some()
        );
    }

    fn scope(root: &Path, cloud: bool) -> RunScope {
        RunScope {
            root: root.to_path_buf(),
            cloud,
        }
    }

    /// `command` in `home/<cwd>` for a chat whose root is `home/<root>`.
    fn run_in(f: &Fixture, root: &str, cloud: bool, cwd: &str, command: &str) -> RunEvent {
        let root = f.home.join(root);
        let env = env(Some(&f.home));
        check(
            &env,
            CallId(4),
            f.home.join(cwd),
            command,
            &scope(&root, cloud),
        )
    }

    /// A project below HOME: `dev/proj` with source files, a git folder and
    /// an environment file in `dev/proj/web`.
    fn project(f: &Fixture) {
        for sub in ["dev/proj/src", "dev/proj/web"] {
            fs::create_dir_all(f.home.join(sub)).unwrap();
        }
        git_folder(&f.home.join("dev/proj/.git"), PLAIN_CONFIG);
        fs::write(f.home.join("dev/proj/src/main.rs"), "fn main() {}").unwrap();
        fs::write(f.home.join("dev/proj/web/.env"), "TOKEN=x").unwrap();
    }

    /// What `git init` writes.
    const PLAIN_CONFIG: &str = "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n";

    /// A folder git takes for a git folder: `HEAD`, `objects/`, `refs/`,
    /// a config and the sample hooks.
    fn git_folder(dir: &Path, config: &str) {
        for sub in ["objects/info", "refs/heads", "hooks"] {
            fs::create_dir_all(dir.join(sub)).unwrap();
        }
        fs::write(dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        fs::write(dir.join("config"), config).unwrap();
        fs::write(dir.join("hooks/pre-commit.sample"), "#!/bin/sh\n").unwrap();
    }

    fn checked(event: &RunEvent) -> bool {
        matches!(event, RunEvent::Checked { .. })
    }

    // S1: search and navigation inside the chat's root run on their own, on
    // a local and on a cloud engine.
    #[test]
    fn search_inside_the_root_runs_without_a_prompt() {
        let f = fixture();
        project(&f);
        for cloud in [false, true] {
            for (cwd, command) in [
                ("dev/proj", "rg foo"),
                ("dev/proj", "rg -n foo src"),
                ("dev/proj", "grep -rn foo src"),
                ("dev/proj", "grep foo src/main.rs"),
                ("dev/proj", "find . -name main.rs"),
                ("dev/proj", "git status"),
                ("dev/proj/src", "git --no-pager log --oneline -n 5"),
                ("dev/proj", "git diff --stat src"),
                ("dev/proj", "tree src"),
            ] {
                let event = run_in(&f, "dev/proj", cloud, cwd, command);
                assert!(checked(&event), "{command} (cloud {cloud}): {event:?}");
            }
        }
    }

    // L22: `cd` and `ls` inside the root still run at once.
    #[test]
    fn cd_and_ls_inside_the_root_still_run() {
        let f = fixture();
        project(&f);
        for cloud in [false, true] {
            assert!(checked(&run_in(&f, "dev/proj", cloud, "dev/proj", "ls")));
            assert!(checked(&run_in(
                &f,
                "dev/proj",
                cloud,
                "dev/proj",
                "ls -la src"
            )));
            assert!(checked(&run_in(
                &f,
                "dev/proj",
                cloud,
                "dev/proj/src",
                "pwd"
            )));
            assert_eq!(
                run_in(&f, "dev/proj", cloud, "dev/proj", "cd src"),
                RunEvent::Checked {
                    call: CallId(4),
                    cwd: f.home.join("dev/proj"),
                    cd_target: Some(f.home.join("dev/proj/src")),
                }
            );
        }
    }

    // L22: a target elsewhere asks: the path argument, or the pane's own
    // directory when the command names none; a bare `cd` goes HOME.
    #[test]
    fn cd_ls_and_search_outside_the_root_ask() {
        let f = fixture();
        project(&f);
        let srv = f.home.parent().unwrap().join("srv/data");
        let outside = [
            ("dev/proj", "cd .."),
            ("dev/proj", "cd"),
            ("dev/proj", "ls ../.."),
            ("dev/proj", &format!("ls {}", srv.display())),
            ("dev/proj", &format!("cd {}", srv.display())),
            ("dev", "ls"),
            ("dev", "tree"),
            ("dev", "find"),
            ("dev/proj", "rg foo .."),
            ("dev/proj", "grep foo ../app"),
            ("dev", "git status"),
        ];
        for (cwd, command) in outside {
            assert_eq!(
                reason(&run_in(&f, "dev/proj", false, cwd, command)),
                Some(OUTSIDE_ROOT),
                "{command} in {cwd}"
            );
        }
        // With a path inside the root, the pane's directory does not matter.
        assert!(checked(&run_in(
            &f,
            "dev/proj",
            false,
            "dev",
            "ls proj/src"
        )));
        assert!(checked(&run_in(
            &f,
            "dev/proj",
            false,
            "dev",
            "rg foo proj/src"
        )));
    }

    // Ruling 1: `pwd` reads no file and goes nowhere: it runs at once
    // wherever the pane is, as before; a secret folder is still refused.
    #[test]
    fn pwd_runs_wherever_the_pane_is() {
        let f = fixture();
        project(&f);
        let srv = f.home.parent().unwrap().join("srv/data");
        for cloud in [false, true] {
            assert!(checked(&run_in(&f, "dev/proj", cloud, "dev", "pwd")));
            let env = env(Some(&f.home));
            let root = f.home.join("dev/proj");
            let there = check(&env, CallId(4), srv.clone(), "pwd", &scope(&root, cloud));
            assert!(checked(&there), "{there:?}");
        }
        assert_eq!(
            reason(&run_in(&f, "dev/proj", false, ".ssh", "pwd")),
            Some(NOT_ALLOWED)
        );
    }

    /// Writes the config of the fixture's repository `dev/proj`.
    fn git_config(f: &Fixture, text: &str) {
        fs::write(f.home.join("dev/proj/.git/config"), text).unwrap();
    }

    fn git_in_proj(f: &Fixture, command: &str) -> RunEvent {
        run_in(f, "dev/proj", false, "dev/proj", command)
    }

    // Ruling 2: a repository whose own config makes status, log or diff run
    // a program asks; a clean one runs.
    #[test]
    fn git_asks_when_the_repository_config_runs_a_program() {
        let f = fixture();
        project(&f);
        git_config(
            &f,
            "[core]\n\trepositoryformatversion = 0\n\tbare = false\n",
        );
        for command in ["git status", "git log --oneline", "git diff"] {
            assert!(checked(&git_in_proj(&f, command)), "{command}");
        }
        for config in [
            "[core]\n\tfsmonitor = .git/hooks/watch\n",
            "[diff \"bin\"]\n\ttextconv = ./conv\n",
            "[filter \"crypt\"]\n\tclean = ./enc\n",
        ] {
            git_config(&f, config);
            for command in ["git status", "git log -p", "git diff"] {
                assert_eq!(
                    reason(&git_in_proj(&f, command)),
                    Some(GIT_NOT_PLAIN),
                    "{config:?} {command}"
                );
            }
        }
    }

    // B1: a continued line makes git read the next line as part of the
    // value, so a header there is no header and the key after it is
    // `core.fsmonitor`.
    #[test]
    fn git_asks_when_a_continued_line_hides_a_key() {
        let f = fixture();
        project(&f);
        git_config(
            &f,
            "[core]\n\tcomment = x \\\n[foo]\n\tfsmonitor = /x/hook\n",
        );
        assert_eq!(reason(&git_in_proj(&f, "git status")), Some(GIT_NOT_PLAIN));
    }

    // B2: a bare repository inside the project is a git folder of its own:
    // git uses its config, whether or not a `.git` lies above.
    #[test]
    fn git_in_an_embedded_bare_repository_asks() {
        let f = fixture();
        project(&f);
        let evil = f.home.join("dev/proj/evil.git");
        git_folder(&evil, "[core]\n\tpager = /x/run\n");
        assert_eq!(
            reason(&run_in(
                &f,
                "dev/proj",
                false,
                "dev/proj/evil.git",
                "git log"
            )),
            Some(GIT_NOT_PLAIN)
        );
        let alone = f.home.join("dev/bare/evil.git");
        git_folder(&alone, "[core]\n\tpager = /x/run\n");
        assert_eq!(
            reason(&run_in(
                &f,
                "dev/bare",
                false,
                "dev/bare/evil.git",
                "git log"
            )),
            Some(GIT_NOT_PLAIN)
        );
        // No repository at all: nothing tells what git would read.
        fs::create_dir_all(f.home.join("dev/none")).unwrap();
        assert_eq!(
            reason(&run_in(&f, "dev/none", false, "dev/none", "git status")),
            Some(GIT_NOT_PLAIN)
        );
    }

    // B3: the config of a submodule runs during the parent's `git status`.
    #[test]
    fn git_status_asks_when_a_submodule_config_runs_a_program() {
        let f = fixture();
        project(&f);
        let sub = f.home.join("dev/proj/.git/modules/sub");
        git_folder(&sub, "[core]\n\tfsmonitor = /x/hook\n");
        assert_eq!(reason(&git_in_proj(&f, "git status")), Some(GIT_NOT_PLAIN));
        fs::remove_dir_all(f.home.join("dev/proj/.git/modules")).unwrap();
        assert!(checked(&git_in_proj(&f, "git status")));
        fs::write(f.home.join("dev/proj/.gitmodules"), "[submodule \"s\"]\n").unwrap();
        assert_eq!(reason(&git_in_proj(&f, "git status")), Some(GIT_NOT_PLAIN));
    }

    // B4: a hook that git status fires, a hooks folder elsewhere, and a work
    // tree outside the root.
    #[test]
    fn git_asks_for_hooks_and_a_worktree_elsewhere() {
        let f = fixture();
        project(&f);
        let hook = f.home.join("dev/proj/.git/hooks/post-index-change");
        fs::write(&hook, "#!/bin/sh\n").unwrap();
        assert_eq!(reason(&git_in_proj(&f, "git status")), Some(GIT_NOT_PLAIN));
        fs::remove_file(&hook).unwrap();
        assert!(checked(&git_in_proj(&f, "git status")));
        for key in ["hooksPath = /x/hooks", "worktree = /srv"] {
            git_config(&f, &format!("{PLAIN_CONFIG}\t{key}\n"));
            assert_eq!(
                reason(&git_in_proj(&f, "git diff")),
                Some(GIT_NOT_PLAIN),
                "{key}"
            );
        }
    }

    // Objects read from another repository, outside the root.
    #[test]
    fn git_asks_when_the_objects_have_alternates() {
        let f = fixture();
        project(&f);
        for name in ["alternates", "http-alternates"] {
            let file = f.home.join("dev/proj/.git/objects/info").join(name);
            fs::write(&file, "/srv/other/.git/objects\n").unwrap();
            assert_eq!(
                reason(&git_in_proj(&f, "git log")),
                Some(GIT_NOT_PLAIN),
                "{name}"
            );
            fs::remove_file(&file).unwrap();
        }
    }

    // L29: a fresh repository, and a clone with a remote and branch
    // tracking, still run on their own. With the real git when it is here.
    #[test]
    fn a_plain_repository_still_runs_git_unasked() {
        let f = fixture();
        project(&f);
        git_config(
            &f,
            &format!(
                "{PLAIN_CONFIG}[remote \"origin\"]\n\turl = git@example.com:a/b.git\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n[branch \"main\"]\n\tremote = origin\n\tmerge = refs/heads/main\n[user]\n\tname = A\n\temail = a@b.c\n"
            ),
        );
        for command in ["git status", "git log --oneline", "git diff --stat"] {
            assert!(checked(&git_in_proj(&f, command)), "{command}");
        }
        let git = |args: &[&str], dir: &Path| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("HOME", &f.home)
                .output()
                .is_ok_and(|out| out.status.success())
        };
        let dev = f.home.join("dev");
        if !git(&["init", "-q", "-b", "main", "fresh"], &dev) {
            return;
        }
        let commit = ["-c", "user.name=a", "-c", "user.email=b@c", "commit", "-q"];
        assert!(git(
            &[&commit[..], &["--allow-empty", "-m", "x"]].concat(),
            &dev.join("fresh")
        ));
        assert!(git(&["clone", "-q", "fresh", "cloned"], &dev));
        for repo in ["dev/fresh", "dev/cloned"] {
            for command in ["git status", "git log --oneline"] {
                let event = run_in(&f, repo, false, repo, command);
                assert!(checked(&event), "{repo} {command}: {event:?}");
            }
        }
    }

    // `log.showSignature` makes `git log` run the gpg program the config
    // names; under the allow-list (L29) any `log` or `gpg` key asks.
    #[test]
    fn git_log_asks_when_the_repository_config_checks_signatures() {
        let f = fixture();
        project(&f);
        for config in [
            "[log]\n\tshowSignature = true\n[gpg]\n\tprogram = ./sign\n",
            "[log]\n\tshowSignature = false\n",
        ] {
            git_config(&f, config);
            assert_eq!(reason(&git_in_proj(&f, "git log")), Some(GIT_NOT_PLAIN));
        }
    }

    // Any include asks (L29), and so does a config that cannot be read or
    // parsed.
    #[test]
    fn git_asks_for_an_include_or_a_config_it_cannot_read() {
        let f = fixture();
        project(&f);
        fs::write(f.home.join("dev/proj/.git/extra"), "[user]\n\tname = a\n").unwrap();
        for config in [
            "[include]\n\tpath = extra\n",
            "[includeIf \"gitdir:/\"]\n\tpath = extra\n",
            "core]\n",
        ] {
            git_config(&f, config);
            assert_eq!(
                reason(&git_in_proj(&f, "git log")),
                Some(GIT_NOT_PLAIN),
                "{config:?}"
            );
        }
        fs::remove_file(f.home.join("dev/proj/.git/config")).unwrap();
        assert_eq!(reason(&git_in_proj(&f, "git status")), Some(GIT_NOT_PLAIN));
    }

    // A linked worktree: `.git` is a file naming its git folder, whose
    // `commondir` holds the shared config; `config.worktree` is read too.
    #[test]
    fn git_in_a_linked_worktree_reads_the_shared_config() {
        let f = fixture();
        project(&f);
        let main = f.home.join("dev/proj/.git");
        let linked = main.join("worktrees/wt");
        fs::create_dir_all(&linked).unwrap();
        fs::write(linked.join("commondir"), "../..\n").unwrap();
        fs::write(linked.join("HEAD"), "ref: refs/heads/wt\n").unwrap();
        let wt = f.home.join("dev/wt");
        fs::create_dir_all(&wt).unwrap();
        fs::write(wt.join(".git"), format!("gitdir: {}\n", linked.display())).unwrap();
        let status = |f: &Fixture| run_in(f, "dev/wt", false, "dev/wt", "git status");
        assert!(checked(&status(&f)));
        git_config(&f, "[core]\n\tpager = x\n");
        assert_eq!(reason(&status(&f)), Some(GIT_NOT_PLAIN));
        git_config(&f, PLAIN_CONFIG);
        fs::write(linked.join("config.worktree"), "[core]\n\tfsmonitor = x\n").unwrap();
        assert_eq!(reason(&status(&f)), Some(GIT_NOT_PLAIN));
        fs::remove_file(linked.join("config.worktree")).unwrap();
        // A `.git` link may lead anywhere: it asks.
        fs::remove_file(wt.join(".git")).unwrap();
        std::os::unix::fs::symlink(&main, wt.join(".git")).unwrap();
        assert_eq!(reason(&status(&f)), Some(GIT_NOT_PLAIN));
    }

    // Ruling 3: a git argument that is no existing path is a revision: it
    // asks with its own reason, which never counts toward the pause.
    #[test]
    fn a_git_revision_asks_and_a_path_runs() {
        let f = fixture();
        project(&f);
        assert_eq!(reason(&git_in_proj(&f, "git diff main")), Some(REVISION));
        assert_eq!(reason(&git_in_proj(&f, "git log main src")), Some(REVISION));
        assert!(checked(&git_in_proj(&f, "git diff src")));
        assert!(checked(&git_in_proj(&f, "git log --oneline src/main.rs")));
        assert_eq!(
            reason(&git_in_proj(&f, "git diff web/.env")),
            Some(NOT_ALLOWED)
        );
    }

    // Git reads the whole repository it is in: one that starts above the
    // root reaches outside it.
    #[test]
    fn git_in_a_repository_that_starts_above_the_root_asks() {
        let f = fixture();
        project(&f);
        assert_eq!(
            reason(&run_in(
                &f,
                "dev/proj/src",
                false,
                "dev/proj/src",
                "git status"
            )),
            Some(OUTSIDE_ROOT)
        );
        assert!(checked(&run_in(
            &f,
            "dev/proj",
            false,
            "dev/proj/src",
            "git status"
        )));
    }

    // S2: a sensitive path stays refused, and a search that would read one
    // below the folder it walks does not run on its own either.
    #[test]
    fn a_search_of_a_sensitive_path_is_refused() {
        let f = fixture();
        project(&f);
        for command in [
            "grep TOKEN web/.env",
            "rg TOKEN web/.env",
            "grep -r TOKEN web",
            "grep -rn url .git",
            "grep -r url .",
            "ls ~/.ssh",
        ] {
            assert_eq!(
                reason(&run_in(&f, "dev/proj", false, "dev/proj", command)),
                Some(NOT_ALLOWED),
                "{command}"
            );
        }
        // rg skips hidden files by default; `--hidden` is not allowed.
        assert!(checked(&run_in(
            &f,
            "dev/proj",
            false,
            "dev/proj",
            "rg TOKEN web"
        )));
        // A secret folder whose real path is below the searched one.
        std::os::unix::fs::symlink(f.home.join("dev/proj/src"), f.home.join(".aws")).unwrap();
        assert_eq!(
            reason(&run_in(&f, "dev/proj", false, "dev/proj", "rg foo")),
            Some(NOT_ALLOWED)
        );
    }

    #[test]
    fn a_recursive_grep_of_a_folder_too_large_to_check_asks() {
        let f = fixture();
        let big = f.home.join("dev/big");
        fs::create_dir_all(&big).unwrap();
        for n in 0..=MAX_SEARCHED {
            fs::write(big.join(n.to_string()), "").unwrap();
        }
        assert_eq!(
            reason(&run_in(&f, "dev/big", false, "dev/big", "grep -r foo .")),
            Some(TOO_LARGE)
        );
        assert!(checked(&run_in(
            &f,
            "dev/big",
            false,
            "dev/big",
            "grep foo 1"
        )));
    }

    // L14: under a root that is HOME or `/`, nothing a cloud engine sends
    // runs on its own.
    #[test]
    fn a_cloud_chat_under_a_broad_root_asks() {
        let f = fixture();
        project(&f);
        let env = env(Some(&f.home));
        for root in [f.home.clone(), PathBuf::from("/")] {
            for command in ["ls", "cd src", "pwd", "rg foo", "git status"] {
                let event = check(
                    &env,
                    CallId(4),
                    f.home.join("dev/proj"),
                    command,
                    &scope(&root, true),
                );
                assert_eq!(reason(&event), Some(BROAD_ROOT), "{command} under {root:?}");
            }
        }
    }

    // L16: a local engine under a broad root keeps the folder rule: the
    // root holds everything below it.
    #[test]
    fn a_local_chat_under_a_broad_root_runs() {
        let f = fixture();
        project(&f);
        let env = env(Some(&f.home));
        for command in ["ls", "cd src", "rg foo", "git status", "find . -name x"] {
            let event = check(
                &env,
                CallId(4),
                f.home.join("dev/proj"),
                command,
                &scope(&f.home, false),
            );
            assert!(checked(&event), "{command}: {event:?}");
        }
        let srv = f.home.parent().unwrap().join("srv/data");
        let event = check(&env, CallId(4), srv, "ls", &scope(&f.home, false));
        assert_eq!(reason(&event), Some(OUTSIDE_ROOT), "outside HOME");
    }

    // S3, L20: mkdir and other commands are no longer checked to run.
    #[test]
    fn mkdir_and_other_commands_are_not_run() {
        let f = fixture();
        for command in ["mkdir x", "mkdir -p app/new", "cargo build"] {
            assert_eq!(
                reason(&run(&f, "dev", command)),
                Some("not a safe command"),
                "{command}"
            );
        }
    }

    // The disk work is bounded: a slow answer becomes a refusal.
    #[tokio::test]
    async fn a_check_that_takes_too_long_is_refused() {
        let slow = within(Duration::from_millis(20), CallId(8), || {
            std::thread::sleep(Duration::from_millis(400));
            RunEvent::Checked {
                call: CallId(8),
                cwd: PathBuf::from("/"),
                cd_target: None,
            }
        })
        .await;
        assert_eq!(
            slow,
            RunEvent::Refused {
                call: CallId(8),
                reason: TIMED_OUT
            }
        );
        let quick = within(Duration::from_secs(5), CallId(9), || RunEvent::Refused {
            call: CallId(9),
            reason: "x",
        })
        .await;
        assert_eq!(reason(&quick), Some("x"));
        assert_eq!(TIMEOUT, Duration::from_secs(2));
    }
}
