use super::*;
use crate::core::ai::chat::folder::{Probed, Purpose, Verdict};
use crate::runtime::tools::ToolEnv;
use std::{
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    env: ToolEnv,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for sub in ["home/project/src", "home/.ssh", "srv/x"] {
        fs::create_dir_all(root.join(sub)).unwrap();
    }
    fs::write(root.join("home/note.txt"), "x").unwrap();
    symlink(root.join("home/project"), root.join("home/link")).unwrap();
    let env = ToolEnv {
        home: Some(root.join("home").into_os_string()),
        xdg_config_home: None,
        xdg_data_home: None,
        origin: PathBuf::from("/"),
    };
    Fixture {
        _dir: dir,
        root,
        env,
    }
}

#[test]
fn a_symlink_is_resolved_to_its_target() {
    let f = fixture();
    let got = probe(&f.env, &f.root.join("home/link")).unwrap();
    assert_eq!(got.path, f.root.join("home/project"));
    assert_eq!(got.verdict, Verdict::Concrete);
}

#[test]
fn a_bad_path_is_refused_with_its_reason() {
    let f = fixture();
    let home = f.root.join("home");
    let refused = |path: &Path| probe(&f.env, path).unwrap_err();
    assert_eq!(refused(&home.join("nope")), Refusal::NotFound);
    assert_eq!(refused(&home.join("note.txt")), Refusal::NotAFolder);
    assert_eq!(refused(&home.join(".ssh")), Refusal::NotAllowed);
    assert_eq!(refused(Path::new("/")), Refusal::Root);
}

#[test]
fn broad_folders_keep_their_verdict() {
    let f = fixture();
    let verdict = |path: &Path| probe(&f.env, path).unwrap().verdict;
    assert_eq!(verdict(&f.root.join("home")), Verdict::Home);
    assert_eq!(verdict(&f.root), Verdict::AboveHome);
    assert_eq!(verdict(&f.root.join("srv/x")), Verdict::Outside);
}

#[test]
fn no_home_refuses_everything_like_the_tools() {
    let f = fixture();
    let env = ToolEnv {
        home: None,
        ..f.env.clone()
    };
    assert_eq!(
        probe(&env, &f.root.join("home/project")).unwrap_err(),
        Refusal::NotAllowed
    );
}

#[test]
fn root_is_root_even_without_home_and_a_relative_path_is_not_found() {
    let f = fixture();
    let env = ToolEnv {
        home: None,
        ..f.env.clone()
    };
    assert_eq!(probe(&env, Path::new("/")).unwrap_err(), Refusal::Root);
    // Never resolved against the process cwd.
    assert_eq!(
        probe(&f.env, Path::new("home")).unwrap_err(),
        Refusal::NotFound
    );
    assert_eq!(
        probe(&f.env, Path::new(".")).unwrap_err(),
        Refusal::NotFound
    );
}

#[tokio::test]
async fn a_probe_that_hangs_answers_not_found_after_the_bound() {
    let slow = || {
        std::thread::sleep(std::time::Duration::from_millis(400));
        Ok(Probed {
            path: PathBuf::from("/late"),
            verdict: Verdict::Concrete,
        })
    };
    let started = std::time::Instant::now();
    let got = bounded(std::time::Duration::from_millis(30), slow).await;
    assert_eq!(got, Err(Refusal::NotFound));
    assert!(started.elapsed() < std::time::Duration::from_millis(300));
    assert_eq!(TIMEOUT, std::time::Duration::from_secs(3));
}

#[tokio::test]
async fn a_prober_answers_off_the_loop_through_its_channel() {
    let f = fixture();
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let prober = Prober::new(f.env.clone(), tx);
    let pane = crate::core::layout::PaneId::FIRST;
    prober.start(pane, f.root.join("home/project"), Purpose::Cd);
    let (got_pane, purpose, result) = rx.recv().await.expect("an answer");
    assert_eq!((got_pane, purpose), (pane, Purpose::Cd));
    assert_eq!(result.unwrap().verdict, Verdict::Concrete);
}

// The verdict must match the building blocks the tool host judges a root by
// (`root_of`: a directory that is not HOME and passes `judge_real`;
// `concrete_root`: a folder that does not hold HOME). Those two are private
// to the host, so their expressions are repeated here on purpose.
#[test]
fn the_verdict_matches_the_tool_root_policy() {
    let f = fixture();
    let roots = crate::runtime::tools::fs::load_roots(&f.env).unwrap();
    for sub in [
        "home/project",
        "home/project/src",
        "home/.ssh",
        "home",
        "srv/x",
        "",
    ] {
        let dir = f.root.join(sub);
        let real = dir.canonicalize().unwrap();
        let tool_root = real.is_dir() && real != roots.home() && roots.judge_real(&real);
        let concrete = !roots.home().starts_with(&real);
        let got = probe(&f.env, &dir);
        let is_concrete = matches!(&got, Ok(p) if p.verdict == Verdict::Concrete);
        assert_eq!(is_concrete, tool_root, "{sub}");
        let broad_home =
            matches!(&got, Ok(p) if matches!(p.verdict, Verdict::Home | Verdict::AboveHome));
        assert_eq!(broad_home, !concrete, "{sub}");
    }
}

// ---- the picker's data sources (chat-folder, PR B1) -------------------------

use crate::core::ai::chat::folder_picker::{Source, merge};

/// The fixture plus a git repository, a worktree (its `.git` is a file), a
/// plain folder, and a dotfiles repo in HOME itself.
fn git_fixture() -> Fixture {
    let f = fixture();
    let home = f.root.join("home");
    for sub in [
        "project/.git",
        "project/src/deep",
        "wt/sub",
        "plain/x",
        ".git",
    ] {
        fs::create_dir_all(home.join(sub)).unwrap();
    }
    fs::write(home.join("wt/.git"), "gitdir: /elsewhere").unwrap();
    f
}

#[test]
fn the_walk_finds_a_repository_or_a_worktree_and_stops_below_home() {
    let f = git_fixture();
    let home = f.root.join("home");
    let walk = |sub: &str| walk_git(&home, &home.join(sub));
    assert_eq!(walk("project/src/deep"), Some(home.join("project")));
    assert_eq!(walk("project"), Some(home.join("project")));
    assert_eq!(walk("wt/sub"), Some(home.join("wt")));
    // HOME has a `.git` and is still never a root.
    assert_eq!(walk("plain/x"), None);
    assert_eq!(walk_git(&home, &home), None);
    assert_eq!(walk_git(&home, &f.root.join("srv/x")), None);
    assert_eq!(walk_git(&home, Path::new("/")), None);
    assert_eq!(walk("gone"), None);
}

#[test]
fn many_paths_are_judged_one_by_one() {
    let f = git_fixture();
    let home = f.root.join("home");
    let got = probe_many(
        &f.env,
        &[
            (Source::Recent, home.join("project")),
            (Source::Recent, home.clone()),
            (Source::Recent, Path::new("/").to_path_buf()),
            (Source::Recent, home.join("gone")),
            (Source::LastShell, home.join(".ssh")),
        ],
        false,
    );
    let verdicts: Vec<_> = got
        .iter()
        .map(|(_, r)| r.as_ref().map(|p| p.verdict).map_err(|e| *e))
        .collect();
    assert_eq!(
        verdicts,
        [
            Ok(Verdict::Concrete),
            Ok(Verdict::Home),
            Err(Refusal::Root),
            Err(Refusal::NotFound),
            Err(Refusal::NotAllowed),
        ]
    );
    assert_eq!(got[4].0, Source::LastShell);
}

#[test]
fn the_git_walk_moves_pane_folders_to_their_root_and_drops_the_rest() {
    let f = git_fixture();
    let home = f.root.join("home");
    let got = probe_many(
        &f.env,
        &[
            (Source::PaneRoot, home.join("project/src/deep")),
            (Source::PaneRoot, home.join("plain/x")),
            (Source::Recent, home.join("plain")),
        ],
        true,
    );
    let paths: Vec<_> = got
        .iter()
        .map(|(s, r)| (*s, r.as_ref().unwrap().path.clone()))
        .collect();
    assert_eq!(
        paths,
        [
            (Source::PaneRoot, home.join("project")),
            (Source::Recent, home.join("plain")),
        ]
    );
}

#[test]
fn what_the_picker_lists_is_only_what_the_validator_calls_concrete() {
    let f = git_fixture();
    let home = f.root.join("home");
    let found = probe_many(
        &f.env,
        &[
            (Source::Recent, home.clone()),
            (Source::Recent, f.root.clone()),
            (Source::Recent, Path::new("/").to_path_buf()),
            (Source::Recent, Path::new("/usr").to_path_buf()),
            (Source::Recent, f.root.join("srv/x")),
            (Source::Recent, home.join("gone")),
            (Source::Recent, home.join(".ssh")),
            (Source::Recent, home.join("project/src")),
            (Source::LastShell, home.join("link")),
        ],
        false,
    );
    let rows = merge(found);
    let listed: Vec<_> = rows.iter().map(|r| r.path.clone()).collect();
    // `link` is a symlink to `project`, listed under its real path.
    assert_eq!(listed, [home.join("project/src"), home.join("project")]);
}

#[test]
fn completion_lists_directories_only_and_hides_dot_folders_unless_asked() {
    let f = fixture();
    let home = f.root.join("home");
    for sub in ["alpha", "alps", "beta", ".alpine"] {
        fs::create_dir_all(home.join(sub)).unwrap();
    }
    fs::write(home.join("alpha.txt"), "x").unwrap();
    assert_eq!(complete(&home, "al"), ["alpha", "alps"]);
    assert_eq!(
        complete(&home, ""),
        ["alpha", "alps", "beta", "link", "project"]
    );
    assert_eq!(complete(&home, ".al"), [".alpine"]);
    assert_eq!(complete(&home, "zzz"), Vec::<String>::new());
    assert!(complete(&home.join("nope"), "").is_empty());
    assert!(complete(&home.join("note.txt"), "").is_empty());
}

#[test]
fn completion_is_capped() {
    let f = fixture();
    let home = f.root.join("home");
    for n in 0..COMPLETIONS_MAX + 10 {
        fs::create_dir_all(home.join(format!("d{n:03}"))).unwrap();
    }
    let got = complete(&home, "d");
    assert_eq!(got.len(), COMPLETIONS_MAX);
    assert_eq!(got[0], "d000");
}
