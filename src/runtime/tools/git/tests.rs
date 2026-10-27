//! The git jail: the spec and the environment are checked on fake paths; the
//! preflight and the hardening on a temp repo (skipped without git), with a
//! stand-in for the jail helper.

use std::collections::HashSet;
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::*;
use crate::core::ai::jail::NetBind;
use crate::core::ai::toolchain::{Layout, Vars};
use crate::core::ai::tools::allow::Program;
use crate::core::ai::tools::git::{self as pure, GitError};
use crate::core::ai::tools::run::{Capture, Ended};
use crate::runtime::tools::integrity::Kind;
use crate::runtime::tools::runner::{Done, Unreported};

const HOME: &str = "/home/u";
const ROOT: &str = "/home/u/proj";
const TMP: &str = "/run/lb/tmp/1";

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

fn fs(existing: &[&str]) -> impl Fn(&Path) -> Option<PathBuf> {
    let set: HashSet<PathBuf> = existing.iter().map(|s| p(s)).collect();
    move |path| set.contains(path).then(|| path.to_path_buf())
}

fn layout() -> Layout {
    Layout {
        tool: Program::Cargo,
        home: p(HOME),
        root: p(ROOT),
        program: p("/usr/bin/git"),
        tmp: p(TMP),
        vars: Vars::default(),
    }
}

fn dirs(rw: &[&str]) -> Dirs {
    Dirs {
        ro: vec![],
        rw: rw.iter().map(|s| p(s)).collect(),
    }
}

// --- the spec ---

#[test]
fn the_git_spec_has_no_cache_and_only_the_private_tmp_and_the_git_dirs_are_writable() {
    let existing = [
        "/home/u/.cargo/registry",
        "/home/u/.npm",
        "/home/u/.config/git/ignore",
    ];
    let spec = jail_spec(
        &layout(),
        &fs(&existing),
        &dirs(&["/srv/repo/.git/worktrees/w", "/srv/repo/.git"]),
        Path::new("/home/u/.config"),
    )
    .unwrap();
    assert_eq!(
        spec.rw,
        [p(TMP), p("/srv/repo/.git/worktrees/w"), p("/srv/repo/.git")]
    );
    assert_eq!(spec.rwx, [p(ROOT)]);
    assert_eq!(spec.net_bind, NetBind::Closed);
}

#[test]
fn a_git_dir_inside_the_root_adds_no_rule_because_the_root_already_holds_it() {
    let spec = jail_spec(
        &layout(),
        &fs(&[]),
        &dirs(&["/home/u/proj/.git"]),
        Path::new("/home/u/.config"),
    )
    .unwrap();
    assert_eq!(spec.rw, [p(TMP)]);
}

#[test]
fn only_the_global_ignore_and_attributes_are_readable_and_never_the_global_config() {
    let existing = [
        "/home/u/.config/git/ignore",
        "/home/u/.config/git/attributes",
        "/home/u/.config/git/config",
        "/home/u/.gitconfig",
    ];
    let spec = jail_spec(
        &layout(),
        &fs(&existing),
        &dirs(&[]),
        Path::new("/home/u/.config"),
    )
    .unwrap();
    let granted: Vec<&PathBuf> = spec
        .ro_files
        .iter()
        .filter(|f| f.starts_with(HOME))
        .collect();
    assert_eq!(
        granted,
        [
            &p("/home/u/.config/git/ignore"),
            &p("/home/u/.config/git/attributes")
        ]
    );
}

#[test]
fn a_git_dir_that_is_broad_or_next_to_an_execute_place_is_refused() {
    for bad in [
        "/home/u",
        "/home/u/.ssh/x",
        "/usr/lib/repo.git",
        "/tmp/repo/.git",
    ] {
        let got = jail_spec(
            &layout(),
            &fs(&[]),
            &dirs(&[bad]),
            Path::new("/home/u/.config"),
        );
        assert!(got.is_err(), "{bad}");
    }
}

#[test]
fn the_preflight_spec_reads_the_pointed_dirs_and_writes_none() {
    let want = Dirs {
        ro: vec![p("/srv/repo/.git/worktrees/w"), p("/srv/repo/.git")],
        rw: vec![],
    };
    let spec = jail_spec(&layout(), &fs(&[]), &want, Path::new("/home/u/.config")).unwrap();
    assert_eq!(spec.ro.iter().filter(|r| r.starts_with("/srv")).count(), 2);
    assert_eq!(spec.rw, [p(TMP)]);
}

// --- the environment ---

fn parent(name: &str) -> Option<OsString> {
    match name {
        "PATH" => Some("/usr/bin:/bin".into()),
        "HOME" => Some("/home/u".into()),
        "XDG_CONFIG_HOME" => Some("/home/u/.xdg".into()),
        "GIT_EXTERNAL_DIFF"
        | "GIT_PAGER"
        | "GIT_ASKPASS"
        | "GIT_SSH"
        | "GIT_CONFIG_COUNT"
        | "GIT_CONFIG_KEY_0"
        | "GIT_CONFIG_VALUE_0"
        | "GIT_CONFIG_PARAMETERS"
        | "GIT_DIR"
        | "GIT_WORK_TREE"
        | "GIT_CONFIG_GLOBAL"
        | "SSH_AUTH_SOCK" => Some("planted".into()),
        _ => None,
    }
}

fn names(env: &[(OsString, OsString)]) -> Vec<String> {
    env.iter()
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .collect()
}

fn value(env: &[(OsString, OsString)], name: &str) -> Option<String> {
    env.iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.to_string_lossy().into_owned())
}

#[test]
fn nothing_git_or_secret_of_the_parent_reaches_git_and_the_fixed_values_are_set() {
    let env = git_env(&parent, Path::new(ROOT), Path::new(TMP), true).unwrap();
    let seen = names(&env);
    for bad in [
        "GIT_EXTERNAL_DIFF",
        "GIT_PAGER",
        "GIT_ASKPASS",
        "GIT_SSH",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_KEY_0",
        "GIT_CONFIG_VALUE_0",
        "GIT_CONFIG_PARAMETERS",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "SSH_AUTH_SOCK",
    ] {
        assert!(!seen.contains(&bad.to_owned()), "{bad}");
    }
    assert_eq!(value(&env, "GIT_CONFIG_NOSYSTEM").as_deref(), Some("1"));
    assert_eq!(
        value(&env, "GIT_CONFIG_GLOBAL").as_deref(),
        Some("/dev/null")
    );
    assert_eq!(value(&env, "GIT_TERMINAL_PROMPT").as_deref(), Some("0"));
    assert_eq!(value(&env, "GIT_EDITOR").as_deref(), Some("true"));
    assert_eq!(value(&env, "GIT_OPTIONAL_LOCKS").as_deref(), Some("0"));
    assert_eq!(
        value(&env, "XDG_CONFIG_HOME").as_deref(),
        Some("/home/u/.xdg")
    );
    assert!(
        value(
            &git_env(&parent, Path::new(ROOT), Path::new(TMP), false).unwrap(),
            "GIT_OPTIONAL_LOCKS"
        )
        .is_none()
    );
}

#[test]
fn an_environment_with_a_foreign_git_variable_or_a_missing_fixed_one_is_refused() {
    let pair = |k: &str, v: &str| (OsString::from(k), OsString::from(v));
    let good = git_env(&parent, Path::new(ROOT), Path::new(TMP), true).unwrap();
    assert!(check_env(&good).is_ok());
    for planted in [
        "GIT_EXTERNAL_DIFF",
        "GIT_CONFIG_KEY_3",
        "GIT_DIR",
        "GIT_SSH",
    ] {
        let mut env = good.clone();
        env.push(pair(planted, "x"));
        assert!(check_env(&env).is_err(), "{planted}");
    }
    let mut wrong = good.clone();
    wrong.retain(|(k, _)| k != "GIT_CONFIG_GLOBAL");
    assert!(check_env(&wrong).is_err());
}

// --- the version ---

#[test]
fn the_version_must_be_2_32_or_newer() {
    for (text, ok) in [
        ("git version 2.47.3\n", true),
        ("git version 2.32.0", true),
        ("git version 2.31.9", false),
        ("git version 2.39.5 (Apple Git-154)", true),
        ("git version 1.9.1", false),
        ("git version 3.0.0", true),
        ("not git", false),
        ("", false),
    ] {
        assert_eq!(version_ok(text), ok, "{text}");
    }
}

// --- truncated output ---

fn done_with(bytes: usize, exit: i32) -> Done {
    let mut capture = Capture::new();
    capture.push(&vec![b'a'; bytes]);
    Done {
        ended: Ended {
            exit: Some(exit),
            signal: None,
            timed_out: false,
            cancelled: false,
        },
        capture,
        duration: Duration::ZERO,
    }
}

#[test]
fn a_status_that_was_cut_is_reported_as_truncated_and_not_parsed() {
    assert_eq!(
        status_output(&done_with(10, 0)),
        Output::Text("a".repeat(10))
    );
    assert_eq!(status_output(&done_with(10_000_000, 0)), Output::Truncated);
    assert_eq!(status_output(&done_with(10, 1)), Output::Failed);
}

// --- the temp repo ---

/// The first git on `PATH` that the jail accepts. A git outside the system
/// places (Homebrew's `/opt/homebrew` on Apple silicon) is refused by design
/// (`Fault::Spec`), so a test that needs a git to run skips there instead of
/// failing on a refusal that is the feature.
fn git_bin() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join("git"))
        .filter_map(|c| std::fs::canonicalize(c).ok())
        .find(|real| {
            crate::core::ai::toolchain::SYSTEM_RX
                .iter()
                .any(|place| real.starts_with(place))
        })
}

const HELPER: &str = "#!/bin/sh\nwhile [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n";

struct Repo {
    _dir: tempfile::TempDir,
    base: PathBuf,
    root: PathBuf,
    ctx: Ctx,
}

/// A repo under `$HOME` (a root in `/tmp` is refused as broad), or `None`
/// when git is missing or `$HOME` is not usable.
fn repo() -> Option<Repo> {
    let git = git_bin()?;
    let home = std::fs::canonicalize(std::env::var_os("HOME")?).ok()?;
    let dir = tempfile::tempdir_in(&home).ok()?;
    let base = std::fs::canonicalize(dir.path()).ok()?;
    let helper = base.join("helper");
    std::fs::write(&helper, HELPER).unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let root = base.join("proj");
    std::fs::create_dir(&root).unwrap();
    let ctx = Ctx {
        helper,
        git,
        home,
        root: root.clone(),
        scratch_parent: base.join("tmp"),
        timeout: Duration::from_secs(30),
        unreported: Unreported::default(),
    };
    let repo = Repo {
        _dir: dir,
        base,
        root,
        ctx,
    };
    sh(&repo.root, &["init", "-q"]);
    sh(&repo.root, &["config", "user.name", "Repo Dev"]);
    sh(&repo.root, &["config", "user.email", "dev@example.com"]);
    std::fs::write(repo.root.join("a.txt"), "one\n").unwrap();
    sh(&repo.root, &["add", "."]);
    sh(&repo.root, &["commit", "-q", "-m", "first"]);
    Some(repo)
}

/// A test that cannot run says so (a silent pass would read as coverage).
fn skipped() {
    eprintln!("skipped: git or a usable $HOME is missing");
}

fn sh(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn real(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

#[tokio::test]
async fn the_preflight_finds_the_git_dirs_and_the_repo_identity() {
    let Some(repo) = repo() else { return skipped() };
    let opened = open(&repo.ctx, &real).await.unwrap();
    let want = repo.root.join(".git");
    assert_eq!(
        opened.dir,
        pure::GitDir::new(want.to_str().unwrap(), want.to_str().unwrap()).unwrap()
    );
    assert_eq!(
        opened.identity,
        Ok(pure::Identity::new("Repo Dev", "dev@example.com").unwrap())
    );
    assert_eq!(opened.configured_hooks, "");
}

#[tokio::test]
async fn an_invalid_or_missing_identity_is_a_fixed_error_and_the_hooks_path_is_read() {
    let Some(repo) = repo() else { return skipped() };
    sh(&repo.root, &["config", "user.email", "no-at-sign"]);
    sh(&repo.root, &["config", "core.hooksPath", ".githooks"]);
    let opened = open(&repo.ctx, &real).await.unwrap();
    assert_eq!(opened.identity, Err(GitError::Identity));
    assert_eq!(opened.configured_hooks, ".githooks");
}

#[tokio::test]
async fn a_linked_worktree_gets_its_own_git_dir_and_the_common_one_for_hooks() {
    let Some(repo) = repo() else { return skipped() };
    let wt = repo.base.join("wt");
    sh(&repo.root, &["worktree", "add", "-q", wt.to_str().unwrap()]);
    let mut ctx = repo.ctx.clone();
    ctx.root = wt.clone();
    let opened = open(&ctx, &real).await.unwrap();
    let common = repo.root.join(".git");
    let linked = common.join("worktrees/wt");
    let want = pure::GitDir::new(linked.to_str().unwrap(), common.to_str().unwrap()).unwrap();
    assert_eq!(opened.dir, want);
    // Both live outside the worktree: both are writable in its jail.
    assert_eq!(opened.dirs.rw, [linked, common]);
    // And a status through the jail works there.
    let ran = run(&ctx, &opened, pure::status_argv(&opened.dir), true, &real)
        .await
        .unwrap();
    assert_eq!(ran.done.ended.exit, Some(0), "{}", ran.done.capture.text());
}

#[tokio::test]
async fn a_planted_fsmonitor_and_external_diff_never_run() {
    let Some(repo) = repo() else { return skipped() };
    let marker = repo.base.join("marker");
    let script = repo.base.join("plant.sh");
    std::fs::write(&script, format!("#!/bin/sh\ntouch {}\n", marker.display())).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    sh(
        &repo.root,
        &["config", "core.fsmonitor", script.to_str().unwrap()],
    );
    sh(
        &repo.root,
        &["config", "diff.external", script.to_str().unwrap()],
    );
    sh(
        &repo.root,
        &["config", "core.pager", script.to_str().unwrap()],
    );
    std::fs::write(repo.root.join("a.txt"), "two\n").unwrap();

    // Control: the plant is live for a plain git.
    sh(&repo.root, &["status"]);
    sh(&repo.root, &["diff"]);
    assert!(marker.exists(), "the plant must work without the hardening");
    std::fs::remove_file(&marker).unwrap();

    let opened = open(&repo.ctx, &real).await.unwrap();
    for argv in [
        pure::status_argv(&opened.dir),
        pure::diff_argv(&opened.dir, false, None).unwrap(),
    ] {
        let ran = run(&repo.ctx, &opened, argv, true, &real).await.unwrap();
        assert_eq!(ran.done.ended.exit, Some(0), "{}", ran.done.capture.text());
    }
    assert!(!marker.exists(), "a planted program ran");
}

#[tokio::test]
async fn a_commit_goes_through_with_the_repo_identity() {
    let Some(repo) = repo() else { return skipped() };
    std::fs::write(repo.root.join("a.txt"), "two\n").unwrap();
    let opened = open(&repo.ctx, &real).await.unwrap();
    let add = pure::add_argv(&opened.dir, &["a.txt"]).unwrap();
    assert_eq!(
        run(&repo.ctx, &opened, add, false, &real)
            .await
            .unwrap()
            .done
            .ended
            .exit,
        Some(0)
    );
    let who = opened.identity.clone().unwrap();
    let commit = pure::commit_argv(&opened.dir, &who, "second").unwrap();
    let ran = run(&repo.ctx, &opened, commit, false, &real).await.unwrap();
    assert_eq!(ran.done.ended.exit, Some(0), "{}", ran.done.capture.text());
    let out = Command::new("git")
        .args(["log", "-1", "--format=%an <%ae>"])
        .current_dir(&repo.root)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "Repo Dev <dev@example.com>"
    );
}

#[tokio::test]
async fn the_pointer_dirs_of_a_linked_worktree_are_read_from_its_git_file() {
    let Some(repo) = repo() else { return skipped() };
    let wt = repo.base.join("wt");
    sh(&repo.root, &["worktree", "add", "-q", wt.to_str().unwrap()]);
    let found = pointer_dirs(&wt).unwrap();
    let common = repo.root.join(".git");
    assert_eq!(found, [common.join("worktrees/wt"), common]);
    assert!(pointer_dirs(&repo.root).unwrap().is_empty());
}

// --- review fixes: a planted `.git` file ---

fn plant_git_file(base: &Path, name: &str, target: &Path) -> PathBuf {
    let evil = base.join(name);
    std::fs::create_dir(&evil).unwrap();
    std::fs::write(evil.join(".git"), format!("gitdir: {}\n", target.display())).unwrap();
    evil
}

#[tokio::test]
async fn a_git_file_that_points_at_another_repo_is_refused() {
    let Some(repo) = repo() else { return skipped() };
    let evil = plant_git_file(&repo.base, "evil", &repo.root.join(".git"));
    let mut ctx = repo.ctx.clone();
    ctx.root = evil;
    assert!(matches!(pointer_dirs(&ctx.root), Err(Fault::Dir)));
    assert!(matches!(open(&ctx, &real).await, Err(Fault::Dir)));
}

#[tokio::test]
async fn a_git_file_that_points_at_the_worktree_of_another_checkout_is_refused() {
    let Some(repo) = repo() else { return skipped() };
    let wt = repo.base.join("wt");
    sh(&repo.root, &["worktree", "add", "-q", wt.to_str().unwrap()]);
    // A real linked git dir, but its back-link names `wt`, not the root here.
    let linked = repo.root.join(".git/worktrees/wt");
    let evil = plant_git_file(&repo.base, "evil", &linked);
    assert!(matches!(pointer_dirs(&evil), Err(Fault::Dir)));
    // And a forged back-link is not enough: the git dir is not a worktree of
    // the common dir it names.
    let fake = repo.base.join("fake-git");
    std::fs::create_dir(&fake).unwrap();
    std::fs::write(
        fake.join("commondir"),
        repo.root.join(".git").to_str().unwrap(),
    )
    .unwrap();
    std::fs::write(
        fake.join("gitdir"),
        format!("{}\n", evil.join(".git").display()),
    )
    .unwrap();
    std::fs::write(evil.join(".git"), format!("gitdir: {}\n", fake.display())).unwrap();
    assert!(matches!(pointer_dirs(&evil), Err(Fault::Dir)));
}

#[tokio::test]
async fn a_git_file_that_is_a_link_or_too_big_is_refused_without_reading_it() {
    let Some(repo) = repo() else { return skipped() };
    let linked = repo.base.join("linked");
    std::fs::create_dir(&linked).unwrap();
    std::os::unix::fs::symlink("/dev/zero", linked.join(".git")).unwrap();
    let big = repo.base.join("big");
    std::fs::create_dir(&big).unwrap();
    std::fs::write(big.join(".git"), vec![b'a'; 1024 * 1024]).unwrap();
    for root in [linked, big] {
        let got = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::task::spawn_blocking(move || pointer_dirs(&root)),
        )
        .await
        .expect("it must not read without bound")
        .unwrap();
        assert!(matches!(got, Err(Fault::Dir)));
    }
}

// --- review fixes: the integrity check around a git run ---

fn hook(dir: &Path, name: &str, body: &str) {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

async fn add_and_commit(ctx: &Ctx, opened: &super::Repo) -> (Ran, Ran) {
    let add = pure::add_argv(&opened.dir, &["a.txt"]).unwrap();
    let added = run(ctx, opened, add, false, &real).await.unwrap();
    let who = opened.identity.clone().unwrap();
    let commit = pure::commit_argv(&opened.dir, &who, "second").unwrap();
    let committed = run(ctx, opened, commit, false, &real).await.unwrap();
    (added, committed)
}

#[tokio::test]
async fn a_hook_that_writes_another_hook_during_a_commit_is_reported() {
    let Some(repo) = repo() else { return skipped() };
    hook(
        &repo.root.join(".git/hooks"),
        "pre-commit",
        "echo planted > .git/hooks/pre-push",
    );
    std::fs::write(repo.root.join("a.txt"), "two\n").unwrap();
    let opened = open(&repo.ctx, &real).await.unwrap();
    let (added, committed) = add_and_commit(&repo.ctx, &opened).await;
    assert!(added.changes.is_empty(), "{:?}", added.changes);
    assert_eq!(committed.done.ended.exit, Some(0));
    let found: Vec<_> = committed
        .changes
        .iter()
        .map(|c| (c.path.clone(), c.kind))
        .collect();
    assert_eq!(found, [(p(".git/hooks/pre-push"), Kind::Added)]);
}

#[tokio::test]
async fn a_hook_that_writes_in_the_common_dir_of_a_linked_worktree_is_reported() {
    let Some(repo) = repo() else { return skipped() };
    let wt = repo.base.join("wt");
    sh(&repo.root, &["worktree", "add", "-q", wt.to_str().unwrap()]);
    let common = repo.root.join(".git");
    let planted = common.join("hooks/pre-push");
    hook(
        &common.join("hooks"),
        "pre-commit",
        &format!("echo planted > {}", planted.display()),
    );
    let mut ctx = repo.ctx.clone();
    ctx.root = wt.clone();
    std::fs::write(wt.join("a.txt"), "two\n").unwrap();
    let opened = open(&ctx, &real).await.unwrap();
    let (_, committed) = add_and_commit(&ctx, &opened).await;
    assert_eq!(committed.done.ended.exit, Some(0));
    assert!(
        committed
            .changes
            .iter()
            .any(|c| c.path == planted && c.kind == Kind::Added),
        "{:?}",
        committed.changes
    );
}

// --- review fixes: the unjailed `git config` ---

fn script_ctx(body: &str, timeout: Duration) -> (tempfile::TempDir, Ctx) {
    let dir = tempfile::tempdir().unwrap();
    let git = dir.path().join("git");
    std::fs::write(&git, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
    let ctx = Ctx {
        helper: dir.path().join("helper"),
        git,
        home: dir.path().to_path_buf(),
        root: dir.path().to_path_buf(),
        scratch_parent: dir.path().join("tmp"),
        timeout,
        unreported: Unreported::default(),
    };
    (dir, ctx)
}

#[tokio::test]
async fn a_git_config_that_fails_times_out_or_is_cut_is_a_preflight_fault() {
    let second = Duration::from_secs(10);
    for body in ["exit 2", "exit 128", "kill -9 $$"] {
        let (_dir, ctx) = script_ctx(body, second);
        assert!(
            matches!(config(&ctx, &real).await, Err(Fault::Preflight)),
            "{body}"
        );
    }
    let (_dir, ctx) = script_ctx("sleep 30", Duration::from_millis(200));
    assert!(matches!(config(&ctx, &real).await, Err(Fault::Preflight)));
    let (_dir, ctx) = script_ctx("head -c 10000000 /dev/zero", second);
    assert!(matches!(config(&ctx, &real).await, Err(Fault::Preflight)));
}

#[tokio::test]
async fn a_git_config_that_finds_nothing_is_an_unset_identity_and_a_value_is_read() {
    let (_dir, ctx) = script_ctx("exit 1", Duration::from_secs(10));
    let (identity, hooks) = config(&ctx, &real).await.unwrap();
    assert_eq!((identity, hooks), (Err(GitError::Identity), String::new()));
    let (_dir, ctx) = script_ctx(
        "printf 'user.name\\nA\\0user.email\\na@b.c\\0'",
        Duration::from_secs(10),
    );
    let (identity, _) = config(&ctx, &real).await.unwrap();
    assert_eq!(identity, Ok(pure::Identity::new("A", "a@b.c").unwrap()));
}

#[tokio::test]
async fn git_config_gets_the_path_without_project_entries() {
    let (_dir, ctx) = script_ctx(
        "printf '%s' \"$PATH\" > \"$PWD/seen\"; exit 1",
        Duration::from_secs(10),
    );
    let path = |name: &str| match name {
        "PATH" => Some(OsString::from(format!(
            "/usr/bin:{}/bin:relative",
            ctx.root.display()
        ))),
        _ => None,
    };
    let _ = config(&ctx, &path).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(ctx.root.join("seen")).unwrap(),
        "/usr/bin"
    );
}
