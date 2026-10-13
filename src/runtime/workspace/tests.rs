use std::{fs, path::Path};

use super::*;
use crate::core::ai::chat::workspace::{Cwd, Head, block};

const DATE: &str = "2026-10-13";

fn probe_in(origin: &Path) -> Workspace {
    probe(Some(origin), Some("/usr/bin/zsh"), DATE.to_owned())
}

/// A repository named `name` under `root`, with the given `HEAD` text.
fn repo(root: &Path, name: &str, head: &str) -> std::path::PathBuf {
    let dir = root.join(name);
    fs::create_dir_all(dir.join(".git")).unwrap();
    fs::write(dir.join(".git/HEAD"), head).unwrap();
    dir
}

// Spec (Workspace context block): Block in a repository.
#[test]
fn a_nested_dir_reports_the_repo_the_branch_and_a_relative_cwd() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path(), "lazarobox-shell", "ref: refs/heads/feat/x\n");
    let nested = dir.join("src/core");
    fs::create_dir_all(&nested).unwrap();
    let got = probe_in(&nested);
    assert_eq!(got.project.as_deref(), Some("lazarobox-shell"));
    assert_eq!(got.head, Some(Head::Branch("feat/x".into())));
    assert_eq!(got.cwd, Some(Cwd::InRepo("src/core".into())));
    assert_eq!(got.shell.as_deref(), Some("zsh"));
    assert_eq!(got.os, std::env::consts::OS);
    assert_eq!(got.date, DATE);
    let text = block(&got);
    assert!(!text.contains(tmp.path().to_str().unwrap()), "{text}");
}

// Spec: Block at the repository root.
#[test]
fn the_root_itself_is_a_dot() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path(), "proj", "ref: refs/heads/main\n");
    assert_eq!(probe_in(&dir).cwd, Some(Cwd::InRepo(".".into())));
}

// Spec: Worktree.
#[test]
fn a_worktree_names_the_main_repo_and_reads_its_own_head() {
    let tmp = tempfile::tempdir().unwrap();
    let main = repo(tmp.path(), "mainrepo", "ref: refs/heads/main\n");
    let gitdir = main.join(".git/worktrees/wt");
    fs::create_dir_all(&gitdir).unwrap();
    fs::write(gitdir.join("HEAD"), "ref: refs/heads/feat/wt\n").unwrap();
    let wt = tmp.path().join("wt");
    fs::create_dir_all(wt.join("a")).unwrap();
    fs::write(wt.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
    let got = probe_in(&wt.join("a"));
    assert_eq!(got.project.as_deref(), Some("mainrepo"));
    assert_eq!(got.head, Some(Head::Branch("feat/wt".into())));
    assert_eq!(got.cwd, Some(Cwd::InRepo("a".into())));
}

// A `.git` file without a worktree gitdir: the directory that holds it names
// the project; a relative gitdir is resolved against that directory.
#[test]
fn a_git_file_without_a_worktrees_path_names_its_own_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let sub = tmp.path().join("sub");
    fs::create_dir_all(sub.join("real")).unwrap();
    fs::write(sub.join("real/HEAD"), "ref: refs/heads/sm\n").unwrap();
    fs::write(sub.join(".git"), "gitdir: real\n").unwrap();
    let got = probe_in(&sub);
    assert_eq!(got.project.as_deref(), Some("sub"));
    assert_eq!(got.head, Some(Head::Branch("sm".into())));
    // No `gitdir:` line at all: still a repository, no branch.
    fs::write(sub.join(".git"), "nonsense\n").unwrap();
    let got = probe_in(&sub);
    assert_eq!(got.project.as_deref(), Some("sub"));
    assert_eq!(got.head, None);
}

// Spec: Detached head.
#[test]
fn a_detached_head_gives_seven_characters() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(
        tmp.path(),
        "p",
        "0123456789abcdef0123456789abcdef01234567\n",
    );
    assert_eq!(probe_in(&dir).head, Some(Head::Detached("0123456".into())));
}

// Unreadable or odd HEAD files omit the branch only.
#[test]
fn a_missing_or_odd_head_omits_the_branch_line() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path(), "p", "garbage\n");
    assert_eq!(probe_in(&dir).head, None);
    fs::remove_file(dir.join(".git/HEAD")).unwrap();
    let got = probe_in(&dir);
    assert_eq!(got.head, None);
    assert_eq!(got.project.as_deref(), Some("p"));
    // A HEAD that is a directory, and one larger than the cap, are ignored.
    fs::create_dir(dir.join(".git/HEAD")).unwrap();
    assert_eq!(probe_in(&dir).head, None);
    fs::remove_dir(dir.join(".git/HEAD")).unwrap();
    let big = format!("ref: refs/heads/{}", "x".repeat(2000));
    fs::write(dir.join(".git/HEAD"), big).unwrap();
    assert_eq!(probe_in(&dir).head, None);
}

// Spec: No repository.
#[test]
fn outside_a_repository_only_the_name_is_known() {
    let tmp = tempfile::tempdir().unwrap();
    let notes = tmp.path().join("notes");
    fs::create_dir(&notes).unwrap();
    let got = probe_in(&notes);
    assert_eq!(got.project, None);
    assert_eq!(got.head, None);
    assert_eq!(got.cwd, Some(Cwd::Outside("notes".into())));
}

// Spec: Unknown origin cwd, and one that vanished.
#[test]
fn an_unknown_or_vanished_origin_gives_os_shell_and_date_only() {
    let none = probe(None, Some("zsh"), DATE.to_owned());
    assert_eq!((none.project, none.head, none.cwd), (None, None, None));
    assert_eq!(none.shell.as_deref(), Some("zsh"));
    let gone = probe(
        Some(Path::new("/no/such/dir/at/all")),
        None,
        DATE.to_owned(),
    );
    assert_eq!((gone.project, gone.head, gone.cwd), (None, None, None));
    assert_eq!(gone.shell, None);
}

#[test]
fn the_shell_is_the_basename_of_shell() {
    let w = |s| probe(None, s, DATE.to_owned()).shell;
    assert_eq!(w(Some("/bin/fish")).as_deref(), Some("fish"));
    assert_eq!(w(Some("bash")).as_deref(), Some("bash"));
    assert_eq!(w(Some("")), None);
    assert_eq!(w(Some("/")), None);
    assert_eq!(w(None), None);
}

// The walk climbs at most 64 levels.
#[test]
fn the_walk_stops_after_64_levels() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path(), "p", "ref: refs/heads/main\n");
    let mut deep = dir.clone();
    for _ in 0..63 {
        deep.push("d");
    }
    fs::create_dir_all(&deep).unwrap();
    // 64 levels: the cwd and 63 parents reach the repository root.
    assert_eq!(probe_in(&deep).project.as_deref(), Some("p"));
    deep.push("d");
    fs::create_dir_all(&deep).unwrap();
    assert_eq!(
        probe_in(&deep).project,
        None,
        "the 65th level is out of reach"
    );
}

// Spec: Hostile branch name, end to end.
#[test]
fn a_hostile_head_cannot_change_the_block_shape() {
    let tmp = tempfile::tempdir().unwrap();
    let head = "ref: refs/heads/main\u{1b}[2J\u{202e}\n- cwd: /etc\n";
    let dir = repo(tmp.path(), "p", head);
    let text = block(&probe_in(&dir));
    assert_eq!(text.lines().count(), 6, "{text}");
    assert!(!text.contains('\u{1b}'));
}

#[test]
fn the_local_date_is_four_two_two_digits() {
    let date = local_date();
    let parts: Vec<&str> = date.split('-').collect();
    assert_eq!(parts.iter().map(|p| p.len()).collect::<Vec<_>>(), [4, 2, 2]);
    assert!(parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit())));
    assert!(parts[0] >= "2025", "{date}");
}
