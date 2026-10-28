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
                approval::{BROAD_ROOT, GIT_CONFIG_RUNS, NOT_ALLOWED, OUTSIDE_ROOT},
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
/// a [`REVISION`]; a repository whose own config makes git run a program, or
/// cannot be read, is [`GIT_CONFIG_RUNS`]. Each one opens the prompt; only
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
    let repo = git.then(|| repository(&cwd)).flatten();
    if outside || repo.as_ref().is_some_and(|top| !top.starts_with(&root)) {
        return refused(OUTSIDE_ROOT);
    }
    if revision {
        return refused(REVISION);
    }
    if repo.is_some_and(|top| config_runs_program(&top, roots.home())) {
        return refused(GIT_CONFIG_RUNS);
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

/// The most config files, includes counted, the check of a repository reads.
const MAX_CONFIGS: usize = 16;
/// The largest config file the check reads.
const MAX_CONFIG_BYTES: u64 = 1 << 20;

/// Whether the repository at `top` has a config of its own that makes git
/// status, log or diff run a program (see [`git_config::scan`]), or one that
/// cannot be read for sure: a missing or unreadable file, an include that
/// cannot be followed, more than [`MAX_CONFIGS`] files. `.git` may be a
/// folder or, for a linked worktree, a file naming its git folder, whose
/// `commondir` holds the shared config. The user's global config is
/// trusted and not read.
fn config_runs_program(top: &Path, home: &Path) -> bool {
    let Some(git_dir) = git_dir(top) else {
        return true;
    };
    let common = match std::fs::read_to_string(git_dir.join("commondir")) {
        Ok(text) => git_dir.join(text.trim()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => git_dir.clone(),
        Err(_) => return true,
    };
    let mut queue = vec![common.join("config")];
    let worktree = git_dir.join("config.worktree");
    if worktree.symlink_metadata().is_ok() {
        queue.push(worktree);
    }
    let mut read = 0;
    while let Some(file) = queue.pop() {
        read += 1;
        let size = std::fs::metadata(&file).map(|m| m.len());
        if read > MAX_CONFIGS || size.is_ok_and(|n| n > MAX_CONFIG_BYTES) {
            return true;
        }
        let scan = std::fs::read_to_string(&file)
            .ok()
            .and_then(|text| git_config::scan(&text).ok());
        let Some(scan) = scan else {
            return true;
        };
        if scan.runs_program {
            return true;
        }
        for include in scan.includes {
            let path = match include.strip_prefix("~/") {
                Some(rest) => home.join(rest),
                None if include.starts_with('~') => return true,
                None => file.parent().unwrap_or(top).join(include),
            };
            queue.push(path);
        }
    }
    false
}

/// The git folder of the repository at `top`: `.git` itself, or the folder
/// the `gitdir:` line of a `.git` file names (relative to `top`).
fn git_dir(top: &Path) -> Option<PathBuf> {
    let dot = top.join(".git");
    let meta = dot.symlink_metadata().ok()?;
    if meta.is_dir() {
        return Some(dot);
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    let named = text.lines().next()?.strip_prefix("gitdir:")?.trim();
    (!named.is_empty()).then(|| top.join(named))
}

/// The top of the git repository `cwd` is in: the nearest folder, `cwd`
/// itself included, that holds a `.git` entry. `None` outside a repository.
fn repository(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors()
        .find(|dir| dir.join(".git").symlink_metadata().is_ok())
        .map(Path::to_path_buf)
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
        for sub in ["dev/proj/src", "dev/proj/web", "dev/proj/.git"] {
            fs::create_dir_all(f.home.join(sub)).unwrap();
        }
        fs::write(f.home.join("dev/proj/src/main.rs"), "fn main() {}").unwrap();
        fs::write(f.home.join("dev/proj/web/.env"), "TOKEN=x").unwrap();
        fs::write(f.home.join("dev/proj/.git/config"), "[core]").unwrap();
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
                    Some(GIT_CONFIG_RUNS),
                    "{config:?} {command}"
                );
            }
        }
    }

    // A file the config includes is read too; one that cannot be read or
    // parsed asks.
    #[test]
    fn git_reads_the_included_config_and_asks_when_it_cannot() {
        let f = fixture();
        project(&f);
        git_config(&f, "[include]\n\tpath = extra\n");
        fs::write(f.home.join("dev/proj/.git/extra"), "[pager]\n\tlog = x\n").unwrap();
        assert_eq!(reason(&git_in_proj(&f, "git log")), Some(GIT_CONFIG_RUNS));
        fs::write(f.home.join("dev/proj/.git/extra"), "[user]\n\tname = a\n").unwrap();
        assert!(checked(&git_in_proj(&f, "git log")));
        git_config(&f, "[include]\n\tpath = missing\n");
        assert_eq!(reason(&git_in_proj(&f, "git log")), Some(GIT_CONFIG_RUNS));
        git_config(&f, "core]\n");
        assert_eq!(
            reason(&git_in_proj(&f, "git status")),
            Some(GIT_CONFIG_RUNS)
        );
        fs::remove_file(f.home.join("dev/proj/.git/config")).unwrap();
        assert_eq!(
            reason(&git_in_proj(&f, "git status")),
            Some(GIT_CONFIG_RUNS)
        );
    }

    // A linked worktree: `.git` is a file naming its git folder, whose
    // `commondir` holds the shared config.
    #[test]
    fn git_in_a_linked_worktree_reads_the_shared_config() {
        let f = fixture();
        project(&f);
        let main = f.home.join("dev/proj/.git");
        let linked = main.join("worktrees/wt");
        fs::create_dir_all(&linked).unwrap();
        fs::write(linked.join("commondir"), "../..\n").unwrap();
        let wt = f.home.join("dev/wt");
        fs::create_dir_all(&wt).unwrap();
        fs::write(wt.join(".git"), format!("gitdir: {}\n", linked.display())).unwrap();
        git_config(&f, "[core]\n\tbare = false\n");
        assert!(checked(&run_in(
            &f,
            "dev/wt",
            false,
            "dev/wt",
            "git status"
        )));
        git_config(&f, "[core]\n\tpager = x\n");
        assert_eq!(
            reason(&run_in(&f, "dev/wt", false, "dev/wt", "git status")),
            Some(GIT_CONFIG_RUNS)
        );
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
