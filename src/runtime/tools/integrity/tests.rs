use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use super::*;

/// A project root with a `.git` the three watched places live in.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
    std::fs::write(root.join(".git/config"), "[core]\n\tbare = false\n").unwrap();
    std::fs::write(root.join(".git/hooks/pre-commit"), "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::write(root.join(".gitmodules"), "").unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    dir
}

fn changes(root: &Path, act: impl FnOnce()) -> Vec<Change> {
    let before = snapshot(root);
    act();
    compare(&before, &snapshot(root))
}

fn summary(found: &[Change]) -> Vec<(String, Kind)> {
    found
        .iter()
        .map(|c| (c.path.to_string_lossy().into_owned(), c.kind))
        .collect()
}

#[test]
fn an_untouched_project_and_changes_elsewhere_report_nothing() {
    let dir = project();
    let root = dir.path();
    let found = changes(root, || {
        std::fs::write(root.join("src/main.rs"), "fn main() { 1; }\n").unwrap();
        std::fs::write(root.join("new.txt"), "x").unwrap();
        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/x\n").unwrap();
    });
    assert_eq!(found, []);
}

#[test]
fn a_changed_config_gitmodules_and_hook_are_reported_with_both_texts() {
    let dir = project();
    let root = dir.path();
    let found = changes(root, || {
        std::fs::write(root.join(".git/config"), "[core]\n\thooksPath = /tmp/x\n").unwrap();
        std::fs::write(root.join(".gitmodules"), "[submodule \"a\"]\n").unwrap();
        std::fs::write(root.join(".git/hooks/pre-commit"), "#!/bin/sh\nevil\n").unwrap();
    });
    assert_eq!(
        summary(&found),
        [
            (".git/config".to_owned(), Kind::Changed),
            (".git/hooks/pre-commit".to_owned(), Kind::Changed),
            (".gitmodules".to_owned(), Kind::Changed),
        ]
    );
    let hook = &found[1];
    assert_eq!(hook.before.as_deref(), Some(&b"#!/bin/sh\nexit 0\n"[..]));
    assert_eq!(hook.after.as_deref(), Some(&b"#!/bin/sh\nevil\n"[..]));
}

#[test]
fn an_added_and_a_removed_place_are_told_apart() {
    let dir = project();
    let root = dir.path();
    let found = changes(root, || {
        std::fs::write(root.join(".git/hooks/post-checkout"), "x").unwrap();
        std::fs::remove_file(root.join(".gitmodules")).unwrap();
    });
    assert_eq!(
        summary(&found),
        [
            (".git/hooks/post-checkout".to_owned(), Kind::Added),
            (".gitmodules".to_owned(), Kind::Removed),
        ]
    );
    assert_eq!(
        (found[0].before.as_deref(), found[0].after.as_deref()),
        (None, Some(&b"x"[..]))
    );
    assert_eq!(found[1].after, None);
    assert_eq!(found[1].before.as_deref(), Some(&b""[..]));
}

#[test]
fn a_mode_change_alone_is_a_change() {
    let dir = project();
    let root = dir.path();
    let hook = root.join(".git/hooks/pre-commit");
    let found = changes(root, || {
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    });
    assert_eq!(
        summary(&found),
        [(".git/hooks/pre-commit".to_owned(), Kind::Changed)]
    );
    assert_eq!(found[0].before, found[0].after);
    assert_ne!(found[0].mode.0, found[0].mode.1);
}

#[test]
fn a_hook_swapped_for_a_link_is_a_change_and_the_link_is_never_followed() {
    let dir = project();
    let root = dir.path();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret"), "do not read").unwrap();
    let hook = root.join(".git/hooks/pre-commit");
    let found = changes(root, || {
        std::fs::remove_file(&hook).unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret"), &hook).unwrap();
    });
    assert_eq!(
        summary(&found),
        [(".git/hooks/pre-commit".to_owned(), Kind::Changed)]
    );
    // The target is told, not its content.
    let after = found[0].after.as_deref().unwrap();
    assert_eq!(
        after,
        outside.path().join("secret").as_os_str().as_encoded_bytes()
    );
}

#[test]
fn a_git_file_that_points_elsewhere_is_watched_too() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join(".git"), "gitdir: ../real/.git\n").unwrap();
    let found = changes(root, || {
        std::fs::write(root.join(".git"), "gitdir: /elsewhere\n").unwrap();
    });
    assert_eq!(summary(&found), [(".git".to_owned(), Kind::Changed)]);
}

#[test]
fn a_big_file_is_compared_by_digest_and_carries_no_text() {
    let dir = project();
    let root = dir.path();
    let big = root.join(".git/hooks/big");
    std::fs::write(&big, vec![b'a'; TEXT_CAP + 10]).unwrap();
    let found = changes(root, || {
        let mut bytes = vec![b'a'; TEXT_CAP + 10];
        *bytes.last_mut().unwrap() = b'b';
        std::fs::write(&big, bytes).unwrap();
    });
    assert_eq!(
        summary(&found),
        [(".git/hooks/big".to_owned(), Kind::Changed)]
    );
    assert_eq!((&found[0].before, &found[0].after), (&None, &None));
    // The same big file twice is no change.
    assert_eq!(changes(root, || {}), []);
}

#[test]
fn a_project_without_git_has_nothing_to_watch() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(changes(dir.path(), || {}), []);
    let found = changes(dir.path(), || {
        std::fs::write(dir.path().join(".gitmodules"), "x").unwrap();
    });
    assert_eq!(summary(&found), [(".gitmodules".to_owned(), Kind::Added)]);
}

// --- review fixes: no silent cap, no link followed, more places watched ---

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn paths(found: &[Change]) -> Vec<String> {
    summary(found).into_iter().map(|(p, _)| p).collect()
}

#[test]
fn a_hook_that_sorts_after_many_others_is_still_watched() {
    let dir = project();
    let root = dir.path();
    for n in 0..70 {
        write(root, &format!(".git/hooks/h{n:02}"), "junk");
    }
    let found = changes(root, || {
        write(root, ".git/hooks/pre-commit", "#!/bin/sh\nevil\n")
    });
    assert_eq!(
        summary(&found),
        [(".git/hooks/pre-commit".to_owned(), Kind::Changed)]
    );
}

#[test]
fn an_absurd_number_of_hooks_is_reported_never_hidden() {
    let dir = project();
    let root = dir.path();
    for n in 0..(HOOKS_ABSURD + 10) {
        write(root, &format!(".git/hooks/h{n:05}"), "junk");
    }
    // Even with nothing changed: the caller cannot trust what it cannot see.
    // (The fixture's own `pre-commit` makes it one more.)
    let found = changes(root, || {});
    assert_eq!(summary(&found), [(".git/hooks".to_owned(), Kind::Crowded)]);
    let text = found[0].after.as_deref().unwrap();
    assert_eq!(
        text,
        format!("hooks dir has {} entries", HOOKS_ABSURD + 11).as_bytes()
    );
}

#[test]
fn a_git_dir_that_is_a_link_is_recorded_and_never_entered() {
    let dir = project();
    let root = dir.path();
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "config", "SECRET-OUTSIDE\n");
    write(outside.path(), "hooks/x", "SECRET-HOOK\n");
    let found = changes(root, || {
        std::fs::remove_dir_all(root.join(".git")).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join(".git")).unwrap();
    });
    assert!(paths(&found).contains(&".git".to_owned()), "{found:?}");
    for change in &found {
        for text in [&change.before, &change.after].into_iter().flatten() {
            let text = String::from_utf8_lossy(text);
            assert!(!text.contains("SECRET"), "{change:?}");
        }
    }
    // Later edits of what it points at are not this root's business.
    let again = changes(root, || write(outside.path(), "hooks/x", "other\n"));
    assert_eq!(again, []);
}

#[test]
fn a_git_dir_that_is_a_link_swapped_for_a_dir_is_a_change() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.join(".git")).unwrap();
    let found = changes(root, || {
        std::fs::remove_file(root.join(".git")).unwrap();
        write(root, ".git/hooks/pre-commit", "x");
    });
    assert_eq!(
        summary(&found),
        [
            (".git".to_owned(), Kind::Removed),
            (".git/hooks/pre-commit".to_owned(), Kind::Added)
        ]
    );
}

#[test]
fn a_hooks_path_inside_the_root_is_watched_and_one_outside_is_not() {
    let dir = project();
    let root = dir.path();
    write(root, ".githooks/pre-commit", "#!/bin/sh\n");
    write(root, ".git/config", "[core]\n\thooksPath = .githooks\n");
    let found = changes(root, || write(root, ".githooks/pre-commit", "evil\n"));
    assert_eq!(
        summary(&found),
        [(".githooks/pre-commit".to_owned(), Kind::Changed)]
    );
    // The absolute spelling, as husky and others write it.
    write(
        root,
        ".git/config",
        &format!("[core]\n\thooksPath = \"{}/.githooks\"\n", root.display()),
    );
    let found = changes(root, || write(root, ".githooks/post-commit", "x"));
    assert_eq!(
        summary(&found),
        [(".githooks/post-commit".to_owned(), Kind::Added)]
    );
    // Outside the root: not ours to watch (and never read).
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "pre-commit", "x");
    write(
        root,
        ".git/config",
        &format!("[core]\n\thooksPath = {}\n", outside.path().display()),
    );
    assert_eq!(
        changes(root, || write(outside.path(), "pre-commit", "y")),
        []
    );
}

#[test]
fn an_included_config_inside_the_root_is_watched() {
    let dir = project();
    let root = dir.path();
    write(root, "extra.cfg", "[user]\n\tname = a\n");
    write(root, ".git/config", "[include]\n\tpath = ../extra.cfg\n");
    let found = changes(root, || {
        write(root, "extra.cfg", "[core]\n\thooksPath = /x\n")
    });
    assert_eq!(summary(&found), [("extra.cfg".to_owned(), Kind::Changed)]);
    // One that includes another: the chain is followed.
    write(root, "second.cfg", "[user]\n");
    write(root, "extra.cfg", "[include]\n\tpath = second.cfg\n");
    let found = changes(root, || write(root, "second.cfg", "[core]\n"));
    assert_eq!(summary(&found), [("second.cfg".to_owned(), Kind::Changed)]);
}

#[test]
fn a_submodule_git_dir_and_a_worktree_config_are_watched() {
    let dir = project();
    let root = dir.path();
    write(root, ".git/modules/sub/HEAD", "ref: refs/heads/x\n");
    write(root, ".git/modules/sub/config", "[core]\n");
    write(root, ".git/modules/sub/hooks/pre-commit", "ok\n");
    write(root, ".git/modules/lib/foo/HEAD", "ref: refs/heads/x\n");
    write(root, ".git/config.worktree", "[core]\n");
    let found = changes(root, || {
        write(root, ".git/modules/sub/hooks/pre-commit", "evil\n");
        write(root, ".git/modules/sub/config", "[core]\n\tfsmonitor = x\n");
        write(root, ".git/modules/lib/foo/hooks/post-merge", "evil\n");
        write(root, ".git/config.worktree", "[core]\n\thooksPath = /x\n");
        write(root, ".git/modules/sub/HEAD", "ref: refs/heads/y\n");
    });
    assert_eq!(
        paths(&found),
        [
            ".git/config.worktree",
            ".git/modules/lib/foo/hooks/post-merge",
            ".git/modules/sub/config",
            ".git/modules/sub/hooks/pre-commit",
        ]
    );
}

#[test]
fn a_git_file_pointing_inside_the_root_has_its_git_dir_watched() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    write(&root, ".git", "gitdir: real/gitdir\n");
    write(&root, "real/gitdir/config", "[core]\n");
    write(&root, "real/gitdir/hooks/pre-commit", "ok\n");
    let found = changes(&root, || {
        write(&root, "real/gitdir/hooks/pre-commit", "evil\n");
    });
    assert_eq!(
        summary(&found),
        [("real/gitdir/hooks/pre-commit".to_owned(), Kind::Changed)]
    );
    // The absolute spelling.
    write(
        &root,
        ".git",
        &format!("gitdir: {}/real/gitdir\n", root.display()),
    );
    let found = changes(&root, || {
        write(&root, "real/gitdir/config", "[core]\n\tx = 1\n")
    });
    assert_eq!(
        summary(&found),
        [("real/gitdir/config".to_owned(), Kind::Changed)]
    );
    // One that points out of the root is left alone.
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "hooks/pre-commit", "ok\n");
    write(
        &root,
        ".git",
        &format!("gitdir: {}\n", outside.path().display()),
    );
    assert_eq!(
        changes(&root, || write(
            outside.path(),
            "hooks/pre-commit",
            "evil\n"
        )),
        []
    );
}
