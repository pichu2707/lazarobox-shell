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
                approval::{BROAD_ROOT, NOT_ALLOWED, OUTSIDE_ROOT},
                engine::CallId,
            },
            tools::{
                exec::{Verb, parse},
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
/// Why a search does not run on its own: its folder holds more entries than
/// the check looks at.
pub const TOO_LARGE: &str = "the folder is too large to check";
/// The most entries the check of a `grep -r` or an `rg` looks at.
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
/// ([`BROAD_ROOT`]); then, path by path, a refused path, a walk (`find` or a
/// search) that reaches a secret root, or a search that would read a
/// sensitive file below the folder it walks, is [`NOT_ALLOWED`], and a
/// searched folder with more than [`MAX_SEARCHED`] entries is [`TOO_LARGE`];
/// last, a path the command acts on outside the root is [`OUTSIDE_ROOT`].
/// Each one opens the prompt; only [`NOT_ALLOWED`] counts toward the pause,
/// [`TOO_LARGE`] does not.
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
    let (mut cd_target, mut outside) = (None, false);
    for target in safe.targets() {
        let PathVerdict::Allowed(real) = resolve_exec(&roots, &cwd, target.input, target.want)
        else {
            return refused(NOT_ALLOWED);
        };
        if target.confined && !real.starts_with(&root) {
            outside = true;
        } else if target.searched
            && let Err(reason) = searchable(&roots, &real, safe.verb())
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
    RunEvent::Checked {
        call,
        cwd,
        cd_target,
    }
}

/// Whether `verb` may walk `real`, a resolved path, on its own. No secret
/// root may lie at or below it: `find` would list its names, a search would
/// read its files. `find` lists names only, so an ordinary name such as
/// `.env` does not stop it. A search also reads every file it walks, so none
/// may be sensitive by name, position or as a git setting: `grep -r` reads
/// hidden entries too; `rg` skips them by default (its flags that read them
/// are not allowed), so hidden entries below `real` are not looked at, but
/// the plain names inside a hidden folder it is given are. Links are not
/// followed, as no verb follows them below its arguments. A folder with more
/// than [`MAX_SEARCHED`] entries is not looked through.
fn searchable(roots: &SecretRoots, real: &Path, verb: Verb) -> Result<(), &'static str> {
    if roots.holds_root(real) {
        return Err(NOT_ALLOWED);
    }
    let hidden = match verb {
        Verb::Grep => true,
        Verb::Rg => false,
        Verb::Cd | Verb::Pwd | Verb::Ls | Verb::Tree | Verb::Find => return Ok(()),
    };
    if !real.is_dir() {
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
            if !hidden && name.as_encoded_bytes().starts_with(b".") {
                continue;
            }
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

    // L32: git always asks, even in a plain repository inside the root: the
    // check refuses it with the grammar's reason, as at the baseline.
    #[test]
    fn git_never_passes_the_check_even_in_a_plain_repository() {
        let f = fixture();
        project(&f);
        for command in ["git status", "git log --oneline", "git diff"] {
            for cloud in [false, true] {
                assert_eq!(
                    reason(&run_in(&f, "dev/proj", cloud, "dev/proj", command)),
                    Some("not a safe command"),
                    "{command}"
                );
            }
        }
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
        // T2b: rg walks the same folder before it runs, under the same cap.
        assert_eq!(
            reason(&run_in(&f, "dev/big", false, "dev/big", "rg foo")),
            Some(TOO_LARGE)
        );
        assert!(checked(&run_in(
            &f, "dev/big", false, "dev/big", "rg foo 1"
        )));
    }

    // T2b: rg skips hidden entries but reads every other file below what it
    // searches. Given a hidden folder, it reads the plain names inside it:
    // `.git/config` may hold a credential, so the search asks (NOT_ALLOWED,
    // the refusal that counts toward the pause).
    #[test]
    fn an_rg_search_that_would_read_a_secret_named_file_asks() {
        let f = fixture();
        project(&f);
        for (cwd, command) in [
            ("dev/proj", "rg url .git"),
            ("dev/proj", "rg -n url src .git"),
            ("dev/proj/.git", "rg url"),
        ] {
            assert_eq!(
                reason(&run_in(&f, "dev/proj", false, cwd, command)),
                Some(NOT_ALLOWED),
                "{command} in {cwd}"
            );
        }
        // Without the git setting the same folder holds nothing secret.
        assert!(checked(&run_in(
            &f,
            "dev/proj",
            false,
            "dev/proj",
            "rg url .git/hooks"
        )));
    }

    // T2b: find lists the names below what it walks: a secret root there,
    // `~/.ssh` here, asks (NOT_ALLOWED), as for grep and rg.
    #[test]
    fn find_over_a_secret_root_asks() {
        let f = fixture();
        project(&f);
        for (cwd, command) in [
            ("", "find . -name x"),
            ("", "find"),
            ("dev/proj", "find ~ -maxdepth 2"),
            ("dev", "find .. -type f"),
        ] {
            let event = check(
                &env(Some(&f.home)),
                CallId(4),
                f.home.join(cwd),
                command,
                &scope(&f.home, false),
            );
            assert_eq!(reason(&event), Some(NOT_ALLOWED), "{command} in {cwd:?}");
        }
    }

    // T2b keeps: a plain rg search, a find inside a project, even over a
    // `.env` it only names, and grep -r as before.
    #[test]
    fn plain_searches_inside_a_project_still_run() {
        let f = fixture();
        project(&f);
        fs::write(f.home.join("dev/proj/README.md"), "url").unwrap();
        for command in [
            "rg url",
            "rg -n url src web",
            "find . -name main.rs",
            "find web -name .env",
            "find . -type f -maxdepth 3",
            "grep -rn fn src",
        ] {
            let event = run_in(&f, "dev/proj", false, "dev/proj", command);
            assert!(checked(&event), "{command}: {event:?}");
        }
        for command in ["grep -r TOKEN web", "grep -r url ."] {
            assert_eq!(
                reason(&run_in(&f, "dev/proj", false, "dev/proj", command)),
                Some(NOT_ALLOWED),
                "{command}"
            );
        }
    }

    // L14: under a root that is HOME or `/`, nothing a cloud engine sends
    // runs on its own.
    #[test]
    fn a_cloud_chat_under_a_broad_root_asks() {
        let f = fixture();
        project(&f);
        let env = env(Some(&f.home));
        for root in [f.home.clone(), PathBuf::from("/")] {
            for command in ["ls", "cd src", "pwd", "rg foo"] {
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
        for command in ["ls", "cd src", "rg foo", "find . -name x"] {
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
