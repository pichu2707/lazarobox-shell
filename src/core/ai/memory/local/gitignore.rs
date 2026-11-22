//! The `.gitignore` edit that keeps `.lzr/` out of git. It runs only when
//! lazarobox has just created `.lzr/`, and only ever appends.

use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::Path,
};

use ignore::{Match, gitignore::GitignoreBuilder};

use super::{NOTES_DIR, STORE_DIR};

/// What the edit did, one visible line each.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum GitignoreOutcome {
    AlreadyIgnored,
    /// A `!.lzr` line un-ignores it: the user shares the notes on purpose.
    Shared,
    Created,
    Added,
    Failed(String),
}

impl GitignoreOutcome {
    pub fn line(&self) -> String {
        match self {
            Self::AlreadyIgnored => "memory: .lzr/ is already ignored by git".to_owned(),
            Self::Shared => {
                "memory: .lzr/ is shared through git (.gitignore un-ignores it)".to_owned()
            }
            Self::Created => {
                "memory: created .gitignore with .lzr/ (remove it to share memory)".to_owned()
            }
            Self::Added => {
                "memory: added .lzr/ to .gitignore (remove it to share memory)".to_owned()
            }
            Self::Failed(reason) => format!(
                "memory: could not add .lzr/ to .gitignore ({reason}); add it by hand to keep notes private"
            ),
        }
    }
}

/// Decides and applies the edit for the repository at `root`.
pub fn ensure(root: &Path) -> GitignoreOutcome {
    let file = root.join(".gitignore");
    let mut matcher = GitignoreBuilder::new(root);
    // A rules file that cannot be read adds no rule: the entry is then added.
    let _ = matcher.add(&file);
    let _ = matcher.add(root.join(".git/info/exclude"));
    if let Ok(matcher) = matcher.build() {
        let probe = Path::new(STORE_DIR).join(NOTES_DIR).join("n.json");
        match matcher.matched_path_or_any_parents(probe, false) {
            Match::Ignore(_) => return GitignoreOutcome::AlreadyIgnored,
            Match::Whitelist(_) => return GitignoreOutcome::Shared,
            Match::None => {}
        }
    }
    let current = match fs::read(&file) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == ErrorKind::NotFound => None,
        Err(e) => return GitignoreOutcome::Failed(e.to_string()),
    };
    let eol = match &current {
        Some(bytes) if bytes.windows(2).any(|w| w == b"\r\n") => "\r\n",
        _ => "\n",
    };
    let lead = if current
        .as_ref()
        .is_some_and(|b| !b.is_empty() && !b.ends_with(b"\n"))
    {
        eol
    } else {
        ""
    };
    let entry = format!("{lead}{STORE_DIR}/{eol}");
    let written = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .and_then(|mut f| f.write_all(entry.as_bytes()));
    match (written, current) {
        (Err(e), _) => GitignoreOutcome::Failed(e.to_string()),
        (Ok(()), None) => GitignoreOutcome::Created,
        (Ok(()), Some(_)) => GitignoreOutcome::Added,
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt};

    use super::*;

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".git/info")).unwrap();
        dir
    }

    fn ignore_file(dir: &tempfile::TempDir) -> String {
        fs::read_to_string(dir.path().join(".gitignore")).unwrap()
    }

    #[test]
    fn an_ignored_folder_changes_nothing() {
        for entry in [".lzr", ".lzr/", "/.lzr", ".*"] {
            let dir = repo();
            let text = format!("target/\n{entry}\n");
            fs::write(dir.path().join(".gitignore"), &text).unwrap();
            assert_eq!(
                ensure(dir.path()),
                GitignoreOutcome::AlreadyIgnored,
                "{entry}"
            );
            assert_eq!(ignore_file(&dir), text);
        }
    }

    #[test]
    fn a_whitelist_line_is_respected() {
        let dir = repo();
        fs::write(dir.path().join(".gitignore"), "*\n!.lzr\n!.lzr/**\n").unwrap();
        assert_eq!(ensure(dir.path()), GitignoreOutcome::Shared);
        assert_eq!(ignore_file(&dir), "*\n!.lzr\n!.lzr/**\n");
    }

    #[test]
    fn the_git_info_exclude_file_counts() {
        let dir = repo();
        fs::write(dir.path().join(".git/info/exclude"), ".lzr/\n").unwrap();
        assert_eq!(ensure(dir.path()), GitignoreOutcome::AlreadyIgnored);
        assert!(!dir.path().join(".gitignore").exists());
    }

    #[test]
    fn no_gitignore_gets_one_with_only_the_entry() {
        let dir = repo();
        assert_eq!(ensure(dir.path()), GitignoreOutcome::Created);
        assert_eq!(ignore_file(&dir), ".lzr/\n");
    }

    #[test]
    fn the_entry_is_appended_and_other_bytes_stay() {
        let dir = repo();
        fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
        assert_eq!(ensure(dir.path()), GitignoreOutcome::Added);
        assert_eq!(ignore_file(&dir), "target/\n.lzr/\n");
    }

    #[test]
    fn a_missing_trailing_newline_gets_one_before_the_entry() {
        let dir = repo();
        fs::write(dir.path().join(".gitignore"), "target/").unwrap();
        assert_eq!(ensure(dir.path()), GitignoreOutcome::Added);
        assert_eq!(ignore_file(&dir), "target/\n.lzr/\n");
    }

    #[test]
    fn a_crlf_file_gets_a_crlf_entry() {
        let dir = repo();
        fs::write(dir.path().join(".gitignore"), "a/\r\nb/").unwrap();
        assert_eq!(ensure(dir.path()), GitignoreOutcome::Added);
        assert_eq!(ignore_file(&dir), "a/\r\nb/\r\n.lzr/\r\n");
    }

    #[test]
    fn a_second_run_adds_no_duplicate() {
        let dir = repo();
        ensure(dir.path());
        assert_eq!(ensure(dir.path()), GitignoreOutcome::AlreadyIgnored);
        assert_eq!(ignore_file(&dir), ".lzr/\n");
    }

    #[test]
    fn a_read_only_file_reports_the_failure() {
        let dir = repo();
        let file = dir.path().join(".gitignore");
        fs::write(&file, "target/\n").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o444)).unwrap();
        let outcome = ensure(dir.path());
        assert!(
            matches!(outcome, GitignoreOutcome::Failed(_)),
            "{outcome:?}"
        );
        assert!(
            outcome
                .line()
                .starts_with("memory: could not add .lzr/ to .gitignore (")
        );
        assert_eq!(ignore_file(&dir), "target/\n");
    }

    #[test]
    fn every_outcome_has_its_line() {
        let lines: Vec<String> = [
            GitignoreOutcome::AlreadyIgnored,
            GitignoreOutcome::Shared,
            GitignoreOutcome::Created,
            GitignoreOutcome::Added,
            GitignoreOutcome::Failed("disk full".into()),
        ]
        .iter()
        .map(GitignoreOutcome::line)
        .collect();
        assert_eq!(
            lines,
            [
                "memory: .lzr/ is already ignored by git",
                "memory: .lzr/ is shared through git (.gitignore un-ignores it)",
                "memory: created .gitignore with .lzr/ (remove it to share memory)",
                "memory: added .lzr/ to .gitignore (remove it to share memory)",
                "memory: could not add .lzr/ to .gitignore (disk full); add it by hand to keep notes private",
            ]
        );
    }
}
