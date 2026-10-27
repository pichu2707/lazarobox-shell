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
