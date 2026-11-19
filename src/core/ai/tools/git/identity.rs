//! `git_identity_set`: the repo-local commit identity. The argv that writes
//! `user.name` and `user.email` with `git config --local`, the argv that reads
//! the email back, and the check that a config change is only that write.
//! Pure: the runtime runs the argv in the jail and passes the config texts in.

use super::{GitDir, Identity, base};

/// The two writes, name first. `--` keeps a value that starts with `-` from
/// reading as an option.
pub fn identity_set_argvs(dir: &GitDir, who: &Identity) -> [Vec<String>; 2] {
    let write = |key: &str, value: &str| {
        let mut argv = base(dir);
        argv.extend(["config", "--local", "--", key, value].map(str::to_owned));
        argv
    };
    [
        write("user.name", &who.name),
        write("user.email", &who.email),
    ]
}

/// The read-back of the repo-local email, after the write.
pub fn identity_get_argv(dir: &GitDir) -> Vec<String> {
    let mut argv = base(dir);
    argv.extend(["config", "--local", "--get", "user.email"].map(str::to_owned));
    argv
}

/// Whether a config file changed only in its `[user]` `name` and `email`
/// lines (a `[user]` header added or kept with them). `before` is `None` when
/// the file did not exist. Every other line is compared byte for byte
/// together with the section it belongs to, so a line that moves into or out
/// of a section (a `[user]` header added or removed above it) is a change,
/// and so is anything joined to a key by a trailing `\`.
pub fn only_identity_changed(before: Option<&[u8]>, after: &[u8]) -> bool {
    rest(before.unwrap_or_default()) == rest(after)
}

/// The lines of a config, each with the header of its section, without blank
/// lines, the `[user]` headers and the plain `name` and `email` keys under them.
fn rest(text: &[u8]) -> Vec<(&[u8], &[u8])> {
    const USER: &[u8] = b"[user]";
    let mut section: &[u8] = b"";
    let mut continued = false;
    let mut kept = Vec::new();
    for line in text.split(|&b| b == b'\n') {
        let trimmed = line.trim_ascii();
        let joined = std::mem::replace(&mut continued, trimmed.ends_with(b"\\"));
        // A blank line means nothing to git, wherever it sits.
        if trimmed.is_empty() && !joined {
            continue;
        }
        if !joined && trimmed.starts_with(b"[") {
            if trimmed.eq_ignore_ascii_case(USER) {
                section = USER;
                continue;
            }
            section = line;
        } else if !joined && !continued && section == USER && is_identity_key(trimmed) {
            continue;
        }
        kept.push((section, line));
    }
    kept
}

/// A `name = …` or `email = …` line (any case, any spacing around `=`).
fn is_identity_key(line: &[u8]) -> bool {
    let key = line.split(|&b| b == b'=').next().unwrap_or_default();
    let key = key.trim_ascii();
    line.contains(&b'=')
        && (key.eq_ignore_ascii_case(b"name") || key.eq_ignore_ascii_case(b"email"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gd() -> GitDir {
        GitDir::new("/home/u/repo/.git", "/home/u/repo/.git").unwrap()
    }

    fn after_verb(argv: &[String]) -> &[String] {
        let mut at = argv.iter().position(|a| a == "--no-pager").unwrap() + 1;
        while argv[at] == "-c" {
            at += 2;
        }
        &argv[at..]
    }

    // Spec S1: a repo-local write of both keys, never global, and a value
    // that starts with `-` is a value.
    #[test]
    fn set_writes_both_keys_locally() {
        let who = Identity::new("-x Ana", "ana@example.com").unwrap();
        let [name, email] = identity_set_argvs(&gd(), &who);
        assert_eq!(
            after_verb(&name),
            ["config", "--local", "--", "user.name", "-x Ana"]
        );
        assert_eq!(
            after_verb(&email),
            ["config", "--local", "--", "user.email", "ana@example.com"]
        );
        for argv in [&name, &email] {
            assert!(!argv.iter().any(|a| a == "--global" || a == "--system"));
        }
    }

    #[test]
    fn get_reads_the_local_email() {
        assert_eq!(
            after_verb(&identity_get_argv(&gd())),
            ["config", "--local", "--get", "user.email"]
        );
    }

    const BASE: &str = "[core]\n\tbare = false\n";

    // Spec S1: the integrity watcher accepts only the identity lines.
    #[test]
    fn a_new_user_section_is_only_the_identity() {
        let after = format!("{BASE}[user]\n\tname = Ana\n\temail = ana@example.com\n");
        assert!(only_identity_changed(
            Some(BASE.as_bytes()),
            after.as_bytes()
        ));
    }

    #[test]
    fn a_replaced_name_and_email_are_only_the_identity() {
        let before = format!("{BASE}[user]\n\tname = Old\n\temail = old@x.org\n");
        let after = format!("{BASE}[user]\n\tname = Ana\n\temail = ana@example.com\n");
        assert!(only_identity_changed(
            Some(before.as_bytes()),
            after.as_bytes()
        ));
    }

    #[test]
    fn a_missing_file_counts_as_empty() {
        assert!(only_identity_changed(None, b"[user]\n\tname = Ana\n"));
    }

    #[test]
    fn any_other_change_is_not_only_the_identity() {
        let id = "[user]\n\tname = Ana\n\temail = ana@example.com\n";
        for after in [
            // Another key in the user section.
            format!("{BASE}{id}\tsigningkey = ABC\n"),
            // Another section.
            format!("{BASE}{id}[core]\n\thooksPath = /tmp/h\n"),
            // A changed line elsewhere.
            format!("[core]\n\tbare = true\n{id}"),
            // A name key in another section.
            format!("{BASE}[alias]\n\tname = !sh\n{id}"),
            // A user subsection is not the user section.
            format!("{BASE}[user \"x\"]\n\tname = Ana\n"),
            // A key continued onto the next line is not a plain identity key.
            format!("{BASE}[user]\n\tname = Ana \\\n\thooksPath = /evil\n"),
        ] {
            assert!(
                !only_identity_changed(Some(BASE.as_bytes()), after.as_bytes()),
                "{after:?}"
            );
        }
    }

    // A line that changes section is a change, even when only `[user]`
    // headers were added or removed around it (verifier advisory 3).
    #[test]
    fn a_line_moved_across_a_user_header_is_a_change() {
        let before = "[core]\n\tbare = false\n[user]\n\thooksPath = /evil\n";
        let after = "[core]\n\tbare = false\n\thooksPath = /evil\n";
        assert!(!only_identity_changed(
            Some(before.as_bytes()),
            after.as_bytes()
        ));
        assert!(!only_identity_changed(
            Some(after.as_bytes()),
            before.as_bytes()
        ));
        // A removed user section that held only the identity is fine.
        let only_id = format!("{BASE}[user]\n\tname = Ana\n");
        assert!(only_identity_changed(
            Some(only_id.as_bytes()),
            BASE.as_bytes()
        ));
    }

    // Spec S1, against the git the tests run with: the writes land in the
    // repo's config only, and the watcher check accepts exactly them.
    #[test]
    fn real_git_writes_only_the_identity() {
        let Ok(repo) = tempfile::tempdir() else {
            return;
        };
        let run = |argv: &[String]| {
            std::process::Command::new("git")
                .args(argv)
                .current_dir(repo.path())
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .ok()
        };
        let init = ["init".to_owned(), "-q".to_owned()];
        if !run(&init).is_some_and(|o| o.status.success()) {
            return;
        }
        let git = repo.path().join(".git");
        let dir = GitDir::new(git.to_str().unwrap(), git.to_str().unwrap()).unwrap();
        let config = git.join("config");
        let before = std::fs::read(&config).unwrap();
        let who = Identity::new("Ana \"Pérez\"", "ana@example.com").unwrap();
        for argv in identity_set_argvs(&dir, &who) {
            assert!(run(&argv).unwrap().status.success(), "{argv:?}");
        }
        let after = std::fs::read(&config).unwrap();
        assert!(only_identity_changed(Some(&before), &after));
        let read = run(&identity_get_argv(&dir)).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&read.stdout).trim(),
            "ana@example.com"
        );
    }
}
