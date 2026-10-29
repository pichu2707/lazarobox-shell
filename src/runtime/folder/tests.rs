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
    prober.start(pane, f.root.join("home/project"), Purpose::Cd(1));
    let Some(Answer::Probed(got_pane, purpose, result)) = rx.recv().await else {
        panic!("expected a probe answer");
    };
    assert_eq!((got_pane, purpose), (pane, Purpose::Cd(1)));
    assert_eq!(result.unwrap().verdict, Verdict::Concrete);
}

#[tokio::test]
async fn a_prober_answers_the_candidates_and_the_completions_through_its_channel() {
    let f = git_fixture();
    let home = f.root.join("home");
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let prober = Prober::new(f.env.clone(), tx);
    let pane = crate::core::layout::PaneId::FIRST;
    prober.start_many(
        pane,
        7,
        vec![(Source::PaneRoot, home.join("project/src"))],
        true,
    );
    let Some(Answer::Many(got, 7, found)) = rx.recv().await else {
        panic!("expected the candidates");
    };
    assert_eq!(got, pane);
    assert_eq!(found[0].1.as_ref().unwrap().path, home.join("project"));
    prober.start_completion(pane, 7, "~/pro".into(), home, "pro".into());
    let Some(Answer::Completed(got, 7, text, found)) = rx.recv().await else {
        panic!("expected the completions");
    };
    assert_eq!(
        (got, text.as_str(), found.names),
        (pane, "~/pro", vec!["project".to_owned()])
    );
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

use crate::core::ai::chat::folder_picker::{Completion, Source, merge};
use crate::core::ai::tools::paths::SecretRoots;

fn roots_of(f: &Fixture) -> SecretRoots {
    crate::runtime::tools::fs::load_roots(&f.env).unwrap()
}

fn probe_many(
    f: &Fixture,
    paths: &[(Source, PathBuf)],
    git: bool,
) -> Vec<(Source, Result<Probed, Refusal>)> {
    let roots = roots_of(f);
    paths
        .iter()
        .filter_map(|(source, path)| candidate(&roots, *source, path, git).map(|r| (*source, r)))
        .collect()
}

fn listed(f: &Fixture, dir: &Path, prefix: &str) -> Vec<String> {
    complete(&roots_of(f), dir, prefix).names
}

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
        &f,
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
        &f,
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
        &f,
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
    assert_eq!(listed(&f, &home, "al"), ["alpha", "alps"]);
    assert_eq!(
        listed(&f, &home, ""),
        ["alpha", "alps", "beta", "link", "project"]
    );
    assert_eq!(listed(&f, &home, ".al"), [".alpine"]);
    assert_eq!(listed(&f, &home, "zzz"), Vec::<String>::new());
    assert!(listed(&f, &home.join("nope"), "").is_empty());
    assert!(listed(&f, &home.join("note.txt"), "").is_empty());
}

#[test]
fn completion_is_capped() {
    let f = fixture();
    let home = f.root.join("home");
    for n in 0..COMPLETIONS_MAX + 10 {
        fs::create_dir_all(home.join(format!("d{n:03}"))).unwrap();
    }
    let got = listed(&f, &home, "d");
    assert_eq!(got.len(), COMPLETIONS_MAX);
    assert_eq!(got[0], "d000");
}

#[tokio::test]
async fn one_stalled_candidate_does_not_take_the_others_with_it() {
    use std::time::{Duration, Instant};
    let items = vec![
        (Source::LastUsed, PathBuf::from("/h/ok")),
        (Source::Recent, PathBuf::from("/h/hangs")),
        (Source::PaneRoot, PathBuf::from("/h/skipped")),
        (Source::LastShell, PathBuf::from("/h/also-ok")),
    ];
    let work = |source, path: &Path| {
        let name = path.file_name().unwrap().to_str().unwrap();
        match name {
            "hangs" => std::thread::sleep(Duration::from_millis(500)),
            "skipped" => return None,
            _ => {}
        }
        Some(Ok(Probed {
            path: path.to_path_buf(),
            verdict: if source == Source::LastUsed {
                Verdict::Concrete
            } else {
                Verdict::Home
            },
        }))
    };
    let started = Instant::now();
    let got = each(items, Duration::from_millis(50), work).await;
    assert!(started.elapsed() < Duration::from_millis(400));
    let shape: Vec<_> = got.iter().map(|(s, r)| (*s, r.is_ok())).collect();
    assert_eq!(
        shape,
        [
            (Source::LastUsed, true),
            (Source::Recent, false),
            (Source::LastShell, true)
        ]
    );
    assert_eq!(got[1].1, Err(Refusal::NotFound));
}

#[tokio::test]
async fn a_second_request_while_the_first_is_stuck_answers_empty_at_once() {
    let f = git_fixture();
    let home = f.root.join("home");
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let prober = Prober::new(f.env.clone(), tx);
    let pane = crate::core::layout::PaneId::FIRST;
    // The earlier requests are still on the disk.
    let _many = prober.many.start().expect("free");
    let _completion = prober.completions.start().expect("free");
    prober.start_many(pane, 1, vec![(Source::Recent, home.join("project"))], false);
    prober.start_completion(pane, 1, "~/".into(), home, String::new());
    let mut seen = 0;
    while seen < 2 {
        match rx.recv().await.expect("an answer") {
            Answer::Many(_, 1, found) => assert!(found.is_empty()),
            Answer::Completed(_, 1, _, found) => assert_eq!(found, Completion::default()),
            other => panic!("unexpected {other:?}"),
        }
        seen += 1;
    }
}

#[tokio::test]
async fn the_flight_frees_when_the_work_ends() {
    let f = fixture();
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let prober = Prober::new(f.env.clone(), tx);
    let pane = crate::core::layout::PaneId::FIRST;
    prober.start_completion(pane, 1, String::new(), f.root.join("home"), String::new());
    let Some(Answer::Completed(..)) = rx.recv().await else {
        panic!("expected the completions");
    };
    // The answer is sent after the blocking work, so the slot is free again.
    for _ in 0..50 {
        if let Some(held) = prober.completions.start() {
            drop(held);
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("the flight was never freed");
}

#[test]
fn completion_never_lists_inside_secret_or_outside_folders() {
    let f = git_fixture();
    let home = f.root.join("home");
    fs::create_dir_all(home.join(".ssh/keys")).unwrap();
    fs::create_dir_all(f.root.join("srv/x/inner")).unwrap();
    assert!(listed(&f, &home.join(".ssh"), "").is_empty());
    assert!(listed(&f, &f.root.join("srv/x"), "").is_empty());
    assert!(listed(&f, Path::new("/usr"), "").is_empty());
    assert!(listed(&f, Path::new("/"), "").is_empty());
}

#[test]
fn completion_skips_links_into_secret_folders_and_names_with_control_characters() {
    let f = git_fixture();
    let home = f.root.join("home");
    symlink(home.join(".ssh"), home.join("innocent")).unwrap();
    fs::create_dir_all(home.join("bad\nname")).unwrap();
    fs::create_dir_all(home.join("good")).unwrap();
    let got = listed(&f, &home, "");
    assert!(got.contains(&"good".to_owned()), "{got:?}");
    assert!(!got.contains(&"innocent".to_owned()), "{got:?}");
    assert!(!got.iter().any(|n| n.contains('\n')), "{got:?}");
}

#[test]
fn completion_examines_a_bounded_number_of_entries() {
    let f = fixture();
    let home = f.root.join("home");
    // None of these may be listed: every one is a link into a secret folder.
    for n in 0..COMPLETIONS_MAX * 4 + 20 {
        symlink(home.join(".ssh"), home.join(format!("l{n:03}"))).unwrap();
    }
    let got = complete(&roots_of(&f), &home, "l");
    assert!(got.names.is_empty());
    assert!(
        got.truncated,
        "the cap was hit, so the list is not complete"
    );
}

#[test]
fn hidden_secret_folders_are_never_completed_and_never_listed_inside() {
    let f = fixture();
    let home = f.root.join("home");
    for sub in [".gnupg/private", ".config/gh", ".config/ok"] {
        fs::create_dir_all(home.join(sub)).unwrap();
    }
    let roots = roots_of(&f);
    let hidden = complete(&roots, &home, ".").names;
    assert!(!hidden.contains(&".ssh".to_owned()), "{hidden:?}");
    assert!(!hidden.contains(&".gnupg".to_owned()), "{hidden:?}");
    assert!(complete(&roots, &home.join(".gnupg"), "").names.is_empty());
    assert!(
        complete(&roots, &home.join(".config/gh"), "")
            .names
            .is_empty()
    );
}

#[test]
fn names_with_invisible_format_characters_are_dropped() {
    let f = fixture();
    let home = f.root.join("home");
    for name in [
        "evil\u{202e}gpj",
        "zero\u{200b}width",
        "bom\u{feff}",
        "fine",
    ] {
        fs::create_dir_all(home.join(name)).unwrap();
    }
    let got = listed(&f, &home, "");
    assert!(got.contains(&"fine".to_owned()), "{got:?}");
    assert!(
        got.iter()
            .all(|n| !n.contains(['\u{202e}', '\u{200b}', '\u{feff}'])),
        "{got:?}"
    );
}
