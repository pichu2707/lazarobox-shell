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
        std::fs::write(
            root.join(".git/config"),
            "[core]\n\thooksPath = /nowhere/x\n",
        )
        .unwrap();
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
    assert_eq!(
        summary(&found),
        [(".git/hooks".to_owned(), Kind::Unchecked)]
    );
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

// --- review fixes, round 2: fail closed ---

fn unchecked(found: &[Change]) -> Vec<String> {
    found
        .iter()
        .filter(|c| c.kind == Kind::Unchecked)
        .map(|c| c.path.to_string_lossy().into_owned())
        .collect()
}

/// A project whose config is `config` and which has `.githooks/pre-commit`;
/// the hook is then rewritten and what the check says is returned.
fn with_config(config: &str) -> Vec<Change> {
    let dir = project();
    let root = dir.path();
    write(root, ".githooks/pre-commit", "#!/bin/sh\n");
    write(root, "extra.cfg", "[user]\n");
    write(root, ".git/config", config);
    changes(root, || {
        write(root, ".githooks/pre-commit", "evil\n");
        write(root, "extra.cfg", "[core]\n\thooksPath = /x\n");
    })
}

fn watched_hook(found: &[Change]) -> bool {
    paths(found).contains(&".githooks/pre-commit".to_owned())
}

#[test]
fn a_config_over_the_text_cap_is_still_parsed() {
    let dir = project();
    let root = dir.path();
    write(root, ".githooks/pre-commit", "#!/bin/sh\n");
    let mut text = "# padding\n".repeat(TEXT_CAP / 10 + 100);
    text.push_str("[core]\n\thooksPath = .githooks\n");
    write(root, ".git/config", &text);
    let found = changes(root, || write(root, ".githooks/pre-commit", "evil\n"));
    assert!(watched_hook(&found), "{found:?}");
    assert_eq!(unchecked(&found), Vec::<String>::new());
}

#[test]
fn a_config_over_the_parse_cap_is_unchecked() {
    let dir = project();
    let root = dir.path();
    write(
        root,
        ".git/config",
        &"# padding\n".repeat(CONFIG_CAP / 10 + 10),
    );
    let found = changes(root, || {});
    assert_eq!(unchecked(&found), [".git/config"], "{found:?}");
}

#[test]
fn the_plain_spellings_of_hooks_path_are_all_watched_and_none_is_unchecked() {
    for config in [
        "[core]\n\thooksPath = .githooks\n",
        "[core] hooksPath = .githooks\n",
        "[Core]\n\tHooksPath = .githooks\n",
        "[CORE]\n\tHOOKSPATH=.githooks\n",
        "[core]\n\thooksPath = \".githooks\"\n",
        "[core]\n\thooksPath = .githooks # the hooks\n",
        "[core]\n\thooksPath = .githooks ; the hooks\n",
        "[core]\n\thooksPath = \".githooks\" # the hooks\n",
        "[core]\n\t# hooksPath = other\n\thooksPath = ./.githooks\n",
        "[alias]\n\tx = !echo \"a b\"\n[core]\n\thooksPath = .githooks\n",
    ] {
        let found = with_config(config);
        assert!(watched_hook(&found), "{config:?}: {found:?}");
        assert_eq!(unchecked(&found), Vec::<String>::new(), "{config:?}");
    }
}

#[test]
fn what_the_reader_does_not_understand_is_unchecked_not_skipped() {
    for config in [
        // A backslash continuation, in the key and in an unrelated line.
        "[core]\n\thooksPath = .git\\\nhooks\n",
        "[alias]\n\tx = a \\\n b\n[core]\n\thooksPath = .githooks\n",
        // Quotes inside the value, unbalanced, with text after, escapes.
        "[core]\n\thooksPath = .git\"hooks\"\n",
        "[core]\n\thooksPath = \".githooks\n",
        "[core]\n\thooksPath = \".git\"hooks\n",
        "[core]\n\thooksPath = \".git\\\\hooks\"\n",
        "[core]\n\thooksPath = .git\\thooks\n",
        // Home-relative and prefix paths, an empty path.
        "[core]\n\thooksPath = ~/hooks\n",
        "[core]\n\thooksPath = %(prefix)/hooks\n",
        "[core]\n\thooksPath =\n",
        "[core]\n\thooksPath\n",
        // Headers that are not a plain header.
        "[core \n\thooksPath = .githooks\n",
        "[ core ]\n\thooksPath = .githooks\n",
        "[core \"a\\\"b\"]\n\thooksPath = .githooks\n",
        "[core\"x\"]\n",
        // A stray CR, a NUL, a line that is not a key.
        "[core]\n\thooksPath = .githooks\r\0\n",
        "[core]\n\t= x\n",
        "\u{feff}[core]\n\thooksPath = .githooks\n",
    ] {
        let found = with_config(config);
        assert_eq!(unchecked(&found), [".git/config"], "{config:?}: {found:?}");
    }
    let not_utf8 = project();
    std::fs::write(not_utf8.path().join(".git/config"), b"[core]\n\xff\xfe\n").unwrap();
    let found = changes(not_utf8.path(), || {});
    assert_eq!(unchecked(&found), [".git/config"]);
}

#[test]
fn includes_in_every_plain_form_are_followed() {
    for config in [
        "[include]\n\tpath = ../extra.cfg\n",
        "[include] path = ../extra.cfg\n",
        "[Include]\n\tPath = \"../extra.cfg\"\n",
        "[includeIf \"gitdir:~/work/\"]\n\tpath = ../extra.cfg\n",
        "[includeif \"onbranch:main\"]\n\tpath = ../extra.cfg\n",
    ] {
        let dir = project();
        let root = dir.path();
        write(root, "extra.cfg", "[user]\n");
        write(root, ".git/config", config);
        let found = changes(root, || {
            write(root, "extra.cfg", "[core]\n\thooksPath = /x\n")
        });
        assert_eq!(paths(&found), ["extra.cfg"], "{config:?}: {found:?}");
        assert_eq!(unchecked(&found), Vec::<String>::new(), "{config:?}");
    }
}

#[test]
fn an_include_in_the_git_dir_is_resolved_against_the_config_folder() {
    let dir = project();
    let root = dir.path();
    write(root, ".git/extra.cfg", "[user]\n");
    write(root, ".git/config", "[include] path = extra.cfg\n");
    let found = changes(root, || write(root, ".git/extra.cfg", "[core]\n"));
    assert_eq!(
        summary(&found),
        [(".git/extra.cfg".to_owned(), Kind::Changed)]
    );
    write(
        root,
        ".git/config",
        "[includeIf \"gitdir:~/x/\"]\n\tpath = extra.cfg\n",
    );
    let found = changes(root, || write(root, ".git/extra.cfg", "[user]\n"));
    assert_eq!(
        summary(&found),
        [(".git/extra.cfg".to_owned(), Kind::Changed)]
    );
}

#[test]
fn a_section_that_only_looks_like_core_or_include_is_not_read() {
    let found = with_config(
        "[core.sub]\n\thooksPath = .githooks\n[include \"x\"]\n\tpath = extra.cfg\n\
         [other]\n\thooksPath = .githooks\n",
    );
    assert_eq!(found, []);
}

#[test]
fn an_include_chain_deeper_than_gits_limit_is_unchecked() {
    let chain = |files: usize| {
        let dir = project();
        let root = dir.path();
        write(root, ".git/config", "[include]\n\tpath = ../inc1.cfg\n");
        for n in 1..=files {
            let next = if n < files {
                format!("[include]\n\tpath = inc{}.cfg\n", n + 1)
            } else {
                "[user]\n".to_owned()
            };
            write(root, &format!("inc{n}.cfg"), &next);
        }
        changes(root, || {})
    };
    assert_eq!(unchecked(&chain(10)), Vec::<String>::new());
    assert_eq!(unchecked(&chain(11)).len(), 1);
}

#[test]
fn a_hooks_dir_replaced_by_a_link_is_a_change_and_its_target_is_watched() {
    let dir = project();
    let root = dir.path();
    write(root, "evil/pre-commit", "ok\n");
    let found = changes(root, || {
        std::fs::remove_dir_all(root.join(".git/hooks")).unwrap();
        std::os::unix::fs::symlink("../evil", root.join(".git/hooks")).unwrap();
    });
    assert!(
        summary(&found).contains(&(".git/hooks".to_owned(), Kind::Added)),
        "{found:?}"
    );
    assert!(
        summary(&found).contains(&(".git/hooks/pre-commit".to_owned(), Kind::Removed)),
        "{found:?}"
    );
    // The hooks git will now run are the target's: they are watched too.
    let found = changes(root, || write(root, "evil/pre-commit", "evil\n"));
    assert_eq!(
        summary(&found),
        [("evil/pre-commit".to_owned(), Kind::Changed)]
    );
}

#[test]
fn a_hooks_dir_or_modules_dir_swapped_for_a_file_or_an_outside_link_is_a_change() {
    let dir = project();
    let root = dir.path();
    let outside = tempfile::tempdir().unwrap();
    let found = changes(root, || {
        std::fs::remove_dir_all(root.join(".git/hooks")).unwrap();
        std::fs::write(root.join(".git/hooks"), "not a dir").unwrap();
    });
    assert!(
        summary(&found).contains(&(".git/hooks".to_owned(), Kind::Added)),
        "{found:?}"
    );
    let found = changes(root, || {
        std::os::unix::fs::symlink(outside.path(), root.join(".git/modules")).unwrap();
    });
    assert_eq!(summary(&found), [(".git/modules".to_owned(), Kind::Added)]);
}

#[test]
fn a_hooks_path_that_does_not_exist_yet_is_watched_when_it_appears() {
    let dir = project();
    let root = dir.path();
    write(root, ".git/config", "[core]\n\thooksPath = .githooks/sub\n");
    let found = changes(root, || write(root, ".githooks/sub/pre-commit", "evil\n"));
    assert_eq!(
        summary(&found),
        [(".githooks/sub/pre-commit".to_owned(), Kind::Added)]
    );
}

#[test]
fn a_hooks_path_of_dot_watches_the_hook_files_of_the_root() {
    let dir = project();
    let root = dir.path();
    write(root, "pre-commit", "#!/bin/sh\n");
    write(root, ".git/config", "[core]\n\thooksPath = .\n");
    let found = changes(root, || {
        write(root, "pre-commit", "evil\n");
        write(root, "post-checkout", "evil\n");
        write(root, "README.md", "not a hook\n");
    });
    assert_eq!(
        summary(&found),
        [
            ("post-checkout".to_owned(), Kind::Added),
            ("pre-commit".to_owned(), Kind::Changed),
        ]
    );
    // Spelled out, and through the absolute path of the root.
    write(
        root,
        ".git/config",
        &format!("[core]\n\thooksPath = {}\n", root.display()),
    );
    let found = changes(root, || write(root, "pre-commit", "again\n"));
    assert_eq!(summary(&found), [("pre-commit".to_owned(), Kind::Changed)]);
}

#[test]
fn a_hook_that_is_a_link_is_watched_through_its_target_inside_the_root() {
    let dir = project();
    let root = dir.path();
    write(root, "scripts/hook", "#!/bin/sh\n");
    std::fs::remove_file(root.join(".git/hooks/pre-commit")).unwrap();
    std::os::unix::fs::symlink("../../scripts/hook", root.join(".git/hooks/pre-commit")).unwrap();
    let found = changes(root, || write(root, "scripts/hook", "evil\n"));
    assert_eq!(
        summary(&found),
        [("scripts/hook".to_owned(), Kind::Changed)]
    );
    assert_eq!(found[0].after.as_deref(), Some(&b"evil\n"[..]));
}

#[test]
fn a_config_that_is_a_link_is_parsed_from_its_target_content() {
    let dir = project();
    let root = dir.path();
    write(root, ".githooks/pre-commit", "#!/bin/sh\n");
    write(root, "shared.cfg", "[core]\n\thooksPath = .githooks\n");
    std::fs::remove_file(root.join(".git/config")).unwrap();
    std::os::unix::fs::symlink("../shared.cfg", root.join(".git/config")).unwrap();
    let found = changes(root, || write(root, ".githooks/pre-commit", "evil\n"));
    assert_eq!(
        summary(&found),
        [(".githooks/pre-commit".to_owned(), Kind::Changed)]
    );
    // The target's own edits show too, with its text.
    let found = changes(root, || {
        write(
            root,
            "shared.cfg",
            "[core]\n\thooksPath = .githooks\n\tbare = true\n",
        )
    });
    assert_eq!(summary(&found), [("shared.cfg".to_owned(), Kind::Changed)]);
    assert_eq!(unchecked(&found), Vec::<String>::new());
}

#[test]
fn a_config_that_links_outside_the_root_is_unchecked() {
    let dir = project();
    let root = dir.path();
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "cfg", "[core]\n\thooksPath = .githooks\n");
    std::fs::remove_file(root.join(".git/config")).unwrap();
    std::os::unix::fs::symlink(outside.path().join("cfg"), root.join(".git/config")).unwrap();
    let found = changes(root, || {});
    assert_eq!(unchecked(&found), [".git/config"], "{found:?}");
}

fn module(root: &Path, config: &str) {
    write(root, ".git/modules/lib/HEAD", "ref: refs/heads/x\n");
    write(root, ".git/modules/lib/config", config);
    write(root, "lib/hooks/pre-commit", "ok\n");
    write(root, "hooks/pre-commit", "superproject hook\n");
}

#[test]
fn a_submodule_hooks_path_is_resolved_against_its_own_worktree() {
    let dir = project();
    let root = dir.path();
    module(
        root,
        "[core]\n\tworktree = ../../../lib\n\thooksPath = hooks\n",
    );
    let found = changes(root, || {
        write(root, "lib/hooks/pre-commit", "evil\n");
        write(root, "hooks/pre-commit", "evil too\n");
    });
    // The module's folder, not the superproject's `hooks`.
    assert_eq!(
        summary(&found),
        [("lib/hooks/pre-commit".to_owned(), Kind::Changed)]
    );
    assert_eq!(unchecked(&found), Vec::<String>::new());
}

#[test]
fn a_relative_submodule_hooks_path_with_no_worktree_is_unchecked() {
    let dir = project();
    let root = dir.path();
    module(root, "[core]\n\thooksPath = hooks\n");
    let found = changes(root, || {});
    assert_eq!(unchecked(&found), [".git/modules/lib/config"], "{found:?}");
    // An absolute one needs no worktree; a worktree that cannot be resolved
    // (a link on the way, nothing there) is not guessed either.
    module(
        root,
        &format!("[core]\n\thooksPath = {}/lib/hooks\n", root.display()),
    );
    let found = changes(root, || write(root, "lib/hooks/pre-commit", "evil\n"));
    assert_eq!(
        summary(&found),
        [("lib/hooks/pre-commit".to_owned(), Kind::Changed)]
    );
}

#[test]
fn a_submodule_worktree_outside_the_root_leaves_nothing_to_watch() {
    let dir = project();
    let root = dir.path();
    module(
        root,
        "[core]\n\tworktree = ../../../../elsewhere\n\thooksPath = hooks\n",
    );
    let found = changes(root, || write(root, "lib/hooks/pre-commit", "x\n"));
    assert_eq!(found, []);
}

#[test]
fn past_the_item_bound_the_run_is_unchecked() {
    let dir = project();
    let root = dir.path();
    for n in 0..20 {
        write(root, &format!(".git/hooks/h{n:02}"), "junk");
    }
    let limits = Limits {
        items: 8,
        bytes: 1 << 20,
    };
    let found = compare(&snapshot_with(root, limits), &snapshot_with(root, limits));
    assert_eq!(unchecked(&found).len(), 1, "{found:?}");
    // Within the bound nothing is said.
    let roomy = Limits {
        items: 100,
        bytes: 1 << 20,
    };
    assert_eq!(
        compare(&snapshot_with(root, roomy), &snapshot_with(root, roomy)),
        []
    );
}

#[test]
fn past_the_byte_bound_the_run_is_unchecked() {
    let dir = project();
    let root = dir.path();
    write(root, ".git/hooks/big", &"x".repeat(5000));
    write(root, ".git/hooks/big2", &"x".repeat(5000));
    let limits = Limits {
        items: 100,
        bytes: 6000,
    };
    let found = compare(&snapshot_with(root, limits), &snapshot_with(root, limits));
    assert_eq!(unchecked(&found).len(), 1, "{found:?}");
}

#[test]
fn a_crowded_folder_is_counted_without_collecting_every_name() {
    let dir = project();
    let root = dir.path();
    for n in 0..(HOOKS_ABSURD + 3) {
        write(root, &format!(".git/modules/m{n:05}/x"), "j");
    }
    // Too many module entries for the bound: unchecked, not a long walk.
    let limits = Limits {
        items: 50,
        bytes: 1 << 20,
    };
    let found = compare(&snapshot_with(root, limits), &snapshot_with(root, limits));
    assert_eq!(unchecked(&found), [".git/modules"], "{found:?}");
}

#[test]
fn an_unchecked_place_in_the_before_snapshot_is_still_told() {
    let dir = project();
    let root = dir.path();
    write(root, ".git/config", "[core]\n\thooksPath = ~/x\n");
    let before = snapshot(root);
    write(root, ".git/config", "[core]\n");
    let found = compare(&before, &snapshot(root));
    assert_eq!(unchecked(&found), [".git/config"]);
}

#[test]
fn the_reader_takes_the_plain_subset_exactly() {
    let parsed = config_paths(
        b"# c\n[Core]\n\tHooksPath = a\n[core] hookspath = \"b\" ; x\n\
          [include]\npath = c\n[includeIf \"gitdir:/x\"]\n\tpath = d\n[core]\nworktree = ../w\n",
    )
    .unwrap();
    assert_eq!(parsed.hooks, ["a", "b"]);
    assert_eq!(parsed.includes, ["c", "d"]);
    assert_eq!(parsed.worktree, ["../w"]);
    assert!(config_paths(b"[core]\n\tbare\n").is_ok());
    assert!(config_paths(b"[core]\n\thooksPath\n").is_err());
}
