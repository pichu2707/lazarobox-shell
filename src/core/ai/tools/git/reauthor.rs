//! `git_commit_reauthor`: rewrite the author of HEAD with the repo identity.
//! The argv of the amend, of the checks before it (HEAD, staged changes, a
//! remote-tracking branch holding HEAD) and the parse of HEAD. Pure.

use super::{GitDir, Identity, base, parse::tidy};

/// The most characters of a subject or author the question shows.
pub const HEAD_SHOWN: usize = 120;

/// HEAD as the question shows it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Head {
    /// The full object name: the run compares it, so HEAD cannot move
    /// between the question and the amend.
    pub id: String,
    /// `Name <email>`, made one clean line and cut for the question.
    pub author: String,
    /// `Name <email>` as git recorded it, whole: what the "already that
    /// author" check compares.
    pub raw_author: String,
    /// The first line of the message, made one clean line.
    pub subject: String,
}

/// HEAD's id, author name, author email and subject, NUL separated.
pub fn head_argv(dir: &GitDir) -> Vec<String> {
    let mut argv = base(dir);
    argv.extend(
        [
            "log",
            "-1",
            "--no-show-signature",
            "--format=%H%x00%an%x00%ae%x00%s",
            "HEAD",
            "--",
        ]
        .map(str::to_owned),
    );
    argv
}

/// The output of [`head_argv`], or `None` when it is not one commit.
pub fn parse_head(raw: &str) -> Option<Head> {
    let mut parts = raw.trim_end_matches('\n').split('\0');
    let (id, name, email, subject) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || !is_object_name(id) {
        return None;
    }
    let cut = |text: &str| tidy(text).chars().take(HEAD_SHOWN).collect::<String>();
    Some(Head {
        id: id.to_owned(),
        author: cut(&format!("{name} <{email}>")),
        raw_author: format!("{name} <{email}>"),
        subject: cut(subject),
    })
}

/// Exit 0 when nothing is staged, 1 when something is. Nothing is left out:
/// an amend would fold any staged path into the commit, a secret one too.
pub fn staged_argv(dir: &GitDir) -> Vec<String> {
    let mut argv = base(dir);
    argv.extend(
        [
            "diff",
            "--cached",
            "--quiet",
            "--no-ext-diff",
            "--ignore-submodules=none",
        ]
        .map(str::to_owned),
    );
    argv
}

/// The remote-tracking branches that hold `id`, one per line. `None` unless
/// `id` is an object name (never an option).
pub fn remote_contains_argv(dir: &GitDir, id: &str) -> Option<Vec<String>> {
    if !is_object_name(id) {
        return None;
    }
    let mut argv = base(dir);
    argv.extend(
        [
            "for-each-ref",
            "--format=%(refname)",
            "--contains",
            id,
            "refs/remotes",
        ]
        .map(str::to_owned),
    );
    Some(argv)
}

/// `git commit --amend --reset-author --no-edit` with the repo identity as
/// author and committer, signing off and the message kept byte for byte.
/// Hooks are not skipped: they run in the jail, as for a commit.
pub fn reauthor_argv(dir: &GitDir, who: &Identity) -> Vec<String> {
    let mut argv = base(dir);
    for setting in [
        "commit.gpgsign=false".to_owned(),
        format!("user.name={}", who.name),
        format!("user.email={}", who.email),
    ] {
        argv.push("-c".to_owned());
        argv.push(setting);
    }
    argv.extend(
        [
            "commit",
            "--amend",
            "--reset-author",
            "--no-edit",
            "--cleanup=verbatim",
            "--allow-empty",
        ]
        .map(str::to_owned),
    );
    argv
}

/// `Name <email>` of an identity, as git writes the author.
pub fn author_of(who: &Identity) -> String {
    format!("{} <{}>", who.name, who.email)
}

fn is_object_name(text: &str) -> bool {
    (40..=64).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::*;

    fn gd() -> GitDir {
        GitDir::new("/home/u/repo/.git", "/home/u/repo/.git").unwrap()
    }

    fn who() -> Identity {
        Identity::new("Ana Pérez", "ana@example.com").unwrap()
    }

    fn after_verb(argv: &[String]) -> &[String] {
        let mut at = argv.iter().position(|a| a == "--no-pager").unwrap() + 1;
        while argv[at] == "-c" {
            at += 2;
        }
        &argv[at..]
    }

    const ID: &str = "0123456789abcdef0123456789abcdef01234567";

    // Spec S2: HEAD only, author reset to the repo identity, message kept,
    // signing off.
    #[test]
    fn the_amend_resets_the_author_of_head_only() {
        let argv = reauthor_argv(&gd(), &who());
        assert_eq!(
            after_verb(&argv),
            [
                "commit",
                "--amend",
                "--reset-author",
                "--no-edit",
                "--cleanup=verbatim",
                "--allow-empty"
            ]
        );
        let pair = |key: &str| argv.windows(2).any(|w| w[0] == "-c" && w[1] == key);
        assert!(pair("user.name=Ana Pérez") && pair("user.email=ana@example.com"));
        assert!(pair("commit.gpgsign=false"));
        assert!(!argv.iter().any(|a| a == "--no-verify"));
    }

    #[test]
    fn the_checks_name_head_and_the_remote_branches() {
        assert_eq!(
            after_verb(&staged_argv(&gd()))[..3],
            ["diff", "--cached", "--quiet"]
        );
        assert_eq!(
            after_verb(&remote_contains_argv(&gd(), ID).unwrap()),
            [
                "for-each-ref",
                "--format=%(refname)",
                "--contains",
                ID,
                "refs/remotes"
            ]
        );
        for bad in ["", "HEAD", "--all", "0123", &format!("{ID}x")] {
            assert_eq!(remote_contains_argv(&gd(), bad), None, "{bad:?}");
        }
    }

    #[test]
    fn head_parses_into_one_clean_line_each() {
        let raw = format!("{ID}\0Old Name\0old@x.org\0fix: \u{202e}thing\n");
        assert_eq!(
            parse_head(&raw),
            Some(Head {
                id: ID.into(),
                author: "Old Name <old@x.org>".into(),
                raw_author: "Old Name <old@x.org>".into(),
                subject: "fix: ?thing".into(),
            })
        );
        let long = format!("{ID}\0n\0e\0{}", "s".repeat(500));
        assert_eq!(
            parse_head(&long).unwrap().subject.chars().count(),
            HEAD_SHOWN
        );
        // The shown author is cut; the compared one is whole.
        let wide = format!("{ID}\0{}\0e@x.org\0s", "n".repeat(200));
        let head = parse_head(&wide).unwrap();
        assert_eq!(head.author.chars().count(), HEAD_SHOWN);
        assert_eq!(head.raw_author, format!("{} <e@x.org>", "n".repeat(200)));
        for bad in [
            "",
            "x\0n\0e\0s",
            &format!("{ID}\0n\0e"),
            &format!("{ID}\0n\0e\0s\0x"),
        ] {
            assert_eq!(parse_head(bad), None, "{bad:?}");
        }
    }

    fn git(dir: &Path, argv: &[String]) -> std::process::Output {
        Command::new("git")
            .args(argv)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            // No GIT_AUTHOR_*: the jail clears them, and they would win over
            // the `-c user.*` of the amend.
            .env_remove("GIT_AUTHOR_NAME")
            .env_remove("GIT_AUTHOR_EMAIL")
            .env_remove("GIT_COMMITTER_NAME")
            .env_remove("GIT_COMMITTER_EMAIL")
            .output()
            .unwrap()
    }

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|w| (*w).to_owned()).collect()
    }

    // Spec S2, against the git the tests run with: the checks see a staged
    // change and a remote-tracking branch, and the amend changes the author
    // and nothing else.
    #[test]
    fn real_git_reauthors_head_and_keeps_tree_and_message() {
        let Ok(repo) = tempfile::tempdir() else {
            return;
        };
        let dir = repo.path();
        if !git(dir, &words(&["init", "-q"])).status.success() {
            return;
        }
        std::fs::write(dir.join("a.txt"), "a\n").unwrap();
        git(dir, &words(&["add", "a.txt"]));
        git(
            dir,
            &words(&[
                "-c",
                "user.name=Old Name",
                "-c",
                "user.email=old@x.org",
                "commit",
                "-q",
                "-m",
                "first\n\n  body kept  \n",
            ]),
        );
        let gitdir = dir.join(".git");
        let gd = GitDir::new(gitdir.to_str().unwrap(), gitdir.to_str().unwrap()).unwrap();
        let out = |argv: &[String]| String::from_utf8_lossy(&git(dir, argv).stdout).into_owned();

        let head = parse_head(&out(&head_argv(&gd))).unwrap();
        assert_eq!(
            (head.author.as_str(), head.subject.as_str()),
            ("Old Name <old@x.org>", "first")
        );
        assert_eq!(git(dir, &staged_argv(&gd)).status.code(), Some(0));
        assert_eq!(out(&remote_contains_argv(&gd, &head.id).unwrap()), "");

        std::fs::write(dir.join("a.txt"), "b\n").unwrap();
        git(dir, &words(&["add", "a.txt"]));
        assert_eq!(git(dir, &staged_argv(&gd)).status.code(), Some(1));
        git(dir, &words(&["reset", "-q"]));

        git(
            dir,
            &words(&["update-ref", "refs/remotes/origin/main", &head.id]),
        );
        assert_eq!(
            out(&remote_contains_argv(&gd, &head.id).unwrap()),
            "refs/remotes/origin/main\n"
        );
        git(
            dir,
            &words(&["update-ref", "-d", "refs/remotes/origin/main"]),
        );

        let tree = out(&words(&["rev-parse", "HEAD^{tree}"]));
        let message = out(&words(&["log", "-1", "--format=%B"]));
        assert!(git(dir, &reauthor_argv(&gd, &who())).status.success());
        let after = parse_head(&out(&head_argv(&gd))).unwrap();
        assert_eq!(after.author, "Ana Pérez <ana@example.com>");
        assert_ne!(after.id, head.id);
        assert_eq!(
            out(&words(&["log", "-1", "--format=%cn <%ce>"])).trim(),
            "Ana Pérez <ana@example.com>"
        );
        assert_eq!(out(&words(&["rev-parse", "HEAD^{tree}"])), tree);
        assert_eq!(out(&words(&["log", "-1", "--format=%B"])), message);
        assert_eq!(out(&words(&["rev-list", "--count", "HEAD"])).trim(), "1");
    }
}
