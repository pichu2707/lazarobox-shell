use std::{fs, path::Path};

use super::*;
use crate::core::ai::chat::workspace::{Cwd, Head, block};

const DATE: &str = "2026-10-13";

fn probe_in(origin: &Path) -> Workspace {
    probe(Some(origin), None, Some("/usr/bin/zsh"), DATE.to_owned())
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

// Spec: A symlinked `.git` is not a repository marker.
#[test]
fn a_symlinked_dot_git_is_not_followed_and_the_walk_goes_on() {
    let tmp = tempfile::tempdir().unwrap();
    let real = repo(tmp.path(), "real", "ref: refs/heads/secret\n");
    let linked = tmp.path().join("linked");
    fs::create_dir(&linked).unwrap();
    std::os::unix::fs::symlink(real.join(".git"), linked.join(".git")).unwrap();
    let got = probe_in(&linked);
    assert_eq!(got.project, None);
    assert_eq!(got.head, None);
    assert_eq!(got.cwd, Some(Cwd::Outside("linked".into())));
}

#[test]
fn a_symlinked_dot_git_lets_an_outer_repository_win() {
    let tmp = tempfile::tempdir().unwrap();
    let outer = repo(tmp.path(), "outer", "ref: refs/heads/main\n");
    let real = repo(tmp.path(), "real", "ref: refs/heads/secret\n");
    let inner = outer.join("inner");
    fs::create_dir(&inner).unwrap();
    std::os::unix::fs::symlink(real.join(".git"), inner.join(".git")).unwrap();
    let got = probe_in(&inner);
    assert_eq!(got.project.as_deref(), Some("outer"));
    assert_eq!(got.head, Some(Head::Branch("main".into())));
    assert_eq!(got.cwd, Some(Cwd::InRepo("inner".into())));
}

fn probe_with_home(origin: &Path, home: &Path) -> Workspace {
    probe(Some(origin), Some(home), Some("zsh"), DATE.to_owned())
}

// Spec: The home directory is shown as `~`.
#[test]
fn a_repository_at_home_is_named_tilde() {
    let tmp = tempfile::tempdir().unwrap();
    let home = repo(tmp.path(), "javi", "ref: refs/heads/main\n");
    let sub = home.join("dotfiles/nvim");
    fs::create_dir_all(&sub).unwrap();
    let got = probe_with_home(&sub, &home);
    assert_eq!(got.project.as_deref(), Some("~"));
    assert_eq!(got.cwd, Some(Cwd::InRepo("dotfiles/nvim".into())));
    let text = block(&got);
    assert!(!text.contains("javi"), "{text}");
}

#[test]
fn a_cwd_outside_a_repository_that_is_home_is_a_tilde() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("javi");
    fs::create_dir(&home).unwrap();
    let got = probe_with_home(&home, &home);
    assert_eq!(got.cwd, Some(Cwd::Home));
    assert!(!block(&got).contains("javi"));
}

#[test]
fn home_is_matched_through_a_symlink() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("javi");
    fs::create_dir(&home).unwrap();
    let alias = tmp.path().join("alias");
    std::os::unix::fs::symlink(&home, &alias).unwrap();
    assert_eq!(probe_with_home(&home, &alias).cwd, Some(Cwd::Home));
}

#[test]
fn a_subdirectory_of_home_outside_a_repository_keeps_its_name() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("javi");
    let notes = home.join("notes");
    fs::create_dir_all(&notes).unwrap();
    let got = probe_with_home(&notes, &home);
    assert_eq!(got.cwd, Some(Cwd::Outside("notes".into())));
}

#[test]
fn a_worktree_of_a_repository_at_home_is_named_tilde() {
    let tmp = tempfile::tempdir().unwrap();
    let main = repo(tmp.path(), "javi", "ref: refs/heads/main\n");
    let gitdir = main.join(".git/worktrees/wt");
    fs::create_dir_all(&gitdir).unwrap();
    fs::write(gitdir.join("HEAD"), "ref: refs/heads/x\n").unwrap();
    let wt = tmp.path().join("wt");
    fs::create_dir(&wt).unwrap();
    fs::write(wt.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
    assert_eq!(probe_with_home(&wt, &main).project.as_deref(), Some("~"));
}

// Spec: Unknown origin cwd, and one that vanished.
#[test]
fn an_unknown_or_vanished_origin_gives_os_shell_and_date_only() {
    let none = probe(None, None, Some("zsh"), DATE.to_owned());
    assert_eq!((none.project, none.head, none.cwd), (None, None, None));
    assert_eq!(none.shell.as_deref(), Some("zsh"));
    let gone = probe(
        Some(Path::new("/no/such/dir/at/all")),
        None,
        None,
        DATE.to_owned(),
    );
    assert_eq!((gone.project, gone.head, gone.cwd), (None, None, None));
    assert_eq!(gone.shell, None);
}

#[test]
fn the_shell_is_the_basename_of_shell() {
    let w = |s| probe(None, None, s, DATE.to_owned()).shell;
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

// Chat history: the project tag of a conversation (chat-history, Project tag).
fn tag_in(cwd: &Path) -> Option<String> {
    project_tag(cwd)
}

#[test]
fn the_tag_of_a_git_repository_is_the_repo_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path(), "proj", "ref: refs/heads/main\n");
    let deep = dir.join("src/deep");
    fs::create_dir_all(&deep).unwrap();
    assert_eq!(tag_in(&deep).as_deref(), Some("proj"));
    assert_eq!(tag_in(&dir).as_deref(), Some("proj"));
}

#[test]
fn a_worktree_file_without_gitdir_gives_its_own_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let wt = tmp.path().join("wt");
    fs::create_dir_all(wt.join("a")).unwrap();
    fs::write(wt.join(".git"), "nothing useful\n").unwrap();
    assert_eq!(tag_in(&wt.join("a")).as_deref(), Some("wt"));
}

// R13: the worktrees of one project share a tag.
#[test]
fn a_linked_worktree_gives_the_main_repository() {
    let tmp = tempfile::tempdir().unwrap();
    let main = repo(tmp.path(), "proj", "ref: refs/heads/main\n");
    let wt = tmp.path().join("wt");
    fs::create_dir_all(wt.join("a")).unwrap();
    let gitdir = main.join(".git/worktrees/wt");
    fs::create_dir_all(&gitdir).unwrap();
    fs::write(wt.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
    assert_eq!(tag_in(&wt.join("a")).as_deref(), Some("proj"));
}

#[test]
fn outside_a_repository_the_tag_is_the_cwd_name() {
    let tmp = tempfile::tempdir().unwrap();
    let notes = tmp.path().join("notes");
    fs::create_dir_all(&notes).unwrap();
    assert_eq!(tag_in(&notes).as_deref(), Some("notes"));
}

#[test]
fn an_unknown_or_vanished_cwd_has_no_tag() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(tag_in(&tmp.path().join("gone")), None);
    // A file is not a directory either.
    let file = tmp.path().join("file");
    fs::write(&file, "x").unwrap();
    assert_eq!(tag_in(&file), None);
}

#[test]
fn the_tag_walk_reaches_64_levels_and_no_further() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path(), "p", "ref: refs/heads/main\n");
    let mut deep = dir.clone();
    for _ in 0..63 {
        deep.push("d");
    }
    fs::create_dir_all(&deep).unwrap();
    assert_eq!(tag_in(&deep).as_deref(), Some("p"));
    deep.push("d");
    fs::create_dir_all(&deep).unwrap();
    // Out of reach: the repository is not found, so the cwd names the tag.
    assert_eq!(tag_in(&deep).as_deref(), Some("d"));
}

#[test]
fn a_symlinked_dot_git_is_not_a_marker_for_the_tag() {
    let tmp = tempfile::tempdir().unwrap();
    let outer = repo(tmp.path(), "outer", "ref: refs/heads/main\n");
    let inner = outer.join("inner");
    fs::create_dir_all(&inner).unwrap();
    std::os::unix::fs::symlink(outer.join(".git"), inner.join(".git")).unwrap();
    assert_eq!(tag_in(&inner).as_deref(), Some("outer"));
}

#[test]
fn a_long_name_with_a_bidi_override_is_cleaned() {
    let tmp = tempfile::tempdir().unwrap();
    let name = format!("a\u{202e}{}", "b".repeat(99));
    let dir = tmp.path().join(&name);
    fs::create_dir_all(&dir).unwrap();
    let tag = tag_in(&dir).unwrap();
    assert_eq!(tag.chars().count(), 64);
    assert!(tag.starts_with("ab"), "{tag}");
    assert!(!tag.contains('\u{202e}'));
}

#[test]
fn the_root_is_named_slash() {
    assert_eq!(tag_in(Path::new("/")).as_deref(), Some("/"));
}

// Spec: No git process. The walk only reads the filesystem; nothing in this
// module can start a process.
#[test]
fn the_module_never_starts_a_process() {
    let source = include_str!("../workspace.rs");
    let code = source.split("#[cfg(test)]").next().unwrap();
    assert!(!code.contains("Command"), "no process is spawned");
    assert!(!code.contains("process::"), "no process API is used");
}

// Spec (mcp-servers): project servers apply to the chat's origin git root.
#[test]
fn the_origin_root_is_the_repository_root_above_the_cwd() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path(), "app", "ref: refs/heads/main\n");
    let nested = dir.join("src/core");
    fs::create_dir_all(&nested).unwrap();
    assert_eq!(origin_root(&nested), Some(dir.clone()));
    assert_eq!(origin_root(&dir), Some(dir));
    let plain = tmp.path().join("plain");
    fs::create_dir_all(&plain).unwrap();
    assert_eq!(origin_root(&plain), None);
}

// Spec (project-memory-store): memory is keyed by the repository root.
#[test]
fn memory_from_the_root_or_a_subfolder_resolves_to_one_store() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = real(&repo(tmp.path(), "app", "ref: refs/heads/main\n"));
    let nested = dir.join("backend/api");
    fs::create_dir_all(&nested).unwrap();
    assert_eq!(memory_root(&dir, None), Some((dir.clone(), dir.clone())));
    assert_eq!(memory_root(&nested, None), Some((dir.clone(), dir)));
    let plain = tmp.path().join("plain");
    fs::create_dir_all(&plain).unwrap();
    assert_eq!(memory_root(&plain, None), None);
}

fn real(path: &Path) -> std::path::PathBuf {
    path.canonicalize().unwrap()
}

/// A linked worktree `name` of `main`, laid out like `git worktree add`:
/// `commondir` and the `gitdir` backlink in the worktree's git folder.
/// `gitdir_line` is what the checkout's `.git` file says.
fn linked(
    root: &Path,
    main_git: &Path,
    name: &str,
    gitdir_line: Option<&str>,
) -> std::path::PathBuf {
    let gitdir = main_git.join("worktrees").join(name);
    fs::create_dir_all(&gitdir).unwrap();
    let wt = root.join(name);
    fs::create_dir_all(wt.join("a")).unwrap();
    fs::write(gitdir.join("commondir"), "../..\n").unwrap();
    fs::write(
        gitdir.join("gitdir"),
        format!("{}\n", wt.join(".git").display()),
    )
    .unwrap();
    let line = gitdir_line.map_or_else(|| gitdir.display().to_string(), str::to_owned);
    fs::write(wt.join(".git"), format!("gitdir: {line}\n")).unwrap();
    wt
}

// Owner decision 2: a linked worktree shares the main checkout's `.lzr/`, and
// the area is relative to the worktree it is in.
#[test]
fn a_linked_worktree_stores_in_the_main_checkout_with_its_own_work_root() {
    let tmp = tempfile::tempdir().unwrap();
    let main = real(&repo(tmp.path(), "mainrepo", "ref: refs/heads/main\n"));
    let wt = linked(&real(tmp.path()), &main.join(".git"), "wt", None);
    assert_eq!(memory_root(&wt.join("a"), None), Some((main, wt)));
}

#[test]
fn a_relative_gitdir_and_a_symlinked_checkout_give_the_same_store() {
    let tmp = tempfile::tempdir().unwrap();
    let top = real(tmp.path());
    let main = real(&repo(&top, "mainrepo", "ref: refs/heads/main\n"));
    let wt = linked(
        &top,
        &main.join(".git"),
        "wt",
        Some("../mainrepo/.git/worktrees/wt"),
    );
    let from_worktree = memory_root(&wt.join("a"), None).unwrap().0;
    let link = top.join("link");
    std::os::unix::fs::symlink(&main, &link).unwrap();
    let from_link = memory_root(&link, None).unwrap().0;
    assert_eq!(from_worktree, main);
    assert_eq!(from_link, main, "the store root is canonical");
}

// A forged `.git` file must not move the store (and the `.gitignore` write and
// the trusted notes) into another repository or into HOME.
#[test]
fn a_forged_gitdir_keeps_the_store_in_its_own_checkout() {
    let tmp = tempfile::tempdir().unwrap();
    let top = real(tmp.path());
    let victim = real(&repo(&top, "victim", "ref: refs/heads/main\n"));
    let other = linked(&top, &victim.join(".git"), "real-wt", None);
    // The forger copies the victim's worktree gitdir into its own `.git` file:
    // the backlink there names `real-wt`, not this checkout.
    let forged = top.join("forged");
    fs::create_dir_all(&forged).unwrap();
    let line = victim.join(".git/worktrees/real-wt");
    fs::write(forged.join(".git"), format!("gitdir: {}\n", line.display())).unwrap();
    assert_eq!(memory_root(&forged, None), Some((forged.clone(), forged)));
    // No commondir at all, or one that leaves the repository's `.git`.
    let loose = top.join("loose");
    fs::create_dir_all(&loose).unwrap();
    fs::write(
        loose.join(".git"),
        format!("gitdir: {}\n", top.join("victim/.git").display()),
    )
    .unwrap();
    assert_eq!(memory_root(&loose, None), Some((loose.clone(), loose)));
    assert!(other.exists());
}

#[test]
fn a_bare_layout_is_not_shared() {
    let tmp = tempfile::tempdir().unwrap();
    let top = real(tmp.path());
    let bare = top.join("p/.bare");
    fs::create_dir_all(&bare).unwrap();
    let wt = linked(&top, &bare, "x", None);
    assert_eq!(memory_root(&wt, None), Some((wt.clone(), wt)));
}

#[test]
fn a_submodule_inside_a_linked_worktree_keeps_its_own_store() {
    let tmp = tempfile::tempdir().unwrap();
    let top = real(tmp.path());
    let main = real(&repo(&top, "mainrepo", "ref: refs/heads/main\n"));
    let wt = linked(&top, &main.join(".git"), "wt", None);
    let gitdir = main.join(".git/worktrees/wt/modules/sub");
    fs::create_dir_all(&gitdir).unwrap();
    let sub = wt.join("sub");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
    assert_eq!(memory_root(&sub, None), Some((sub.clone(), sub)));
}

#[test]
fn a_submodule_keeps_its_own_store() {
    let tmp = tempfile::tempdir().unwrap();
    let main = real(&repo(tmp.path(), "mainrepo", "ref: refs/heads/main\n"));
    let gitdir = main.join(".git/modules/sub");
    fs::create_dir_all(&gitdir).unwrap();
    let sub = main.join("sub");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
    assert_eq!(memory_root(&sub, None), Some((sub.clone(), sub)));
}

// HOME is never a project: a dotfiles repository at `~/.git` gets no `~/.lzr`.
#[test]
fn home_is_never_a_memory_root() {
    let tmp = tempfile::tempdir().unwrap();
    let home = real(&repo(tmp.path(), "home", "ref: refs/heads/main\n"));
    fs::create_dir_all(home.join("notes")).unwrap();
    assert_eq!(memory_root(&home, Some(&home)), None);
    assert_eq!(memory_root(&home.join("notes"), Some(&home)), None);
    // A repository below HOME is still a project.
    let app = real(&repo(&home, "app", "ref: refs/heads/main\n"));
    assert_eq!(memory_root(&app, Some(&home)), Some((app.clone(), app)));
}

// The attacker controls the whole layout: a `.git` file, a fake git folder,
// a `commondir` that names the victim's `.git` and a backlink that names this
// checkout. Only a git folder that sits inside the victim's own
// `.git/worktrees/` passes, and nobody can create one there from outside.
#[test]
fn a_layout_the_attacker_builds_entirely_does_not_reach_the_victim() {
    let tmp = tempfile::tempdir().unwrap();
    let top = real(tmp.path());
    let victim = real(&repo(&top, "victim", "ref: refs/heads/main\n"));
    let evil = top.join("evil");
    fs::create_dir_all(evil.join("fake")).unwrap();
    fs::write(evil.join(".git"), "gitdir: fake\n").unwrap();
    let common = victim.join(".git");
    fs::write(
        evil.join("fake/commondir"),
        format!("{}\n", common.display()),
    )
    .unwrap();
    fs::write(
        evil.join("fake/gitdir"),
        format!("{}\n", evil.join(".git").display()),
    )
    .unwrap();
    assert_eq!(memory_root(&evil, None), Some((evil.clone(), evil)));
}

// A `commondir` that names a `.git` FILE is not a main repository.
#[test]
fn a_commondir_that_is_a_file_named_dot_git_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let top = real(tmp.path());
    let file_home = top.join("holder");
    fs::create_dir_all(&file_home).unwrap();
    fs::write(file_home.join(".git"), "gitdir: x\n").unwrap();
    let wt = linked(&top, &top.join("unused"), "wt", None);
    let gitdir = top.join("unused/worktrees/wt");
    fs::write(
        gitdir.join("commondir"),
        format!("{}\n", file_home.join(".git").display()),
    )
    .unwrap();
    assert_eq!(memory_root(&wt, None), Some((wt.clone(), wt)));
}

// A linked worktree checked out at HOME is still HOME, whatever main repo it
// belongs to.
#[test]
fn a_linked_worktree_at_home_is_never_a_memory_root() {
    let tmp = tempfile::tempdir().unwrap();
    let top = real(tmp.path());
    let main = real(&repo(&top, "mainrepo", "ref: refs/heads/main\n"));
    let wt = linked(&top, &main.join(".git"), "wt", None);
    assert!(memory_root(&wt, None).is_some());
    assert_eq!(memory_root(&wt.join("a"), Some(&wt)), None);
}

// When HOME cannot be resolved the guard compares the names as they are.
#[test]
fn the_home_comparison_falls_back_to_the_names_when_a_path_cannot_be_resolved() {
    assert!(same_dir(
        Path::new("/no/such/home"),
        Path::new("/no/such/home")
    ));
    assert!(!same_dir(
        Path::new("/no/such/home"),
        Path::new("/no/such/other")
    ));
    let tmp = tempfile::tempdir().unwrap();
    let dir = real(tmp.path());
    assert!(same_dir(&dir.join("."), &dir));
}
