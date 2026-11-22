//! The `.gitignore` edit that keeps `.lzr/` out of git. It runs only when
//! lazarobox has just created `.lzr/`, and only ever appends. The files are
//! read with a bound and are never followed through a link.

use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

use ignore::{Match, gitignore::GitignoreBuilder};

use super::{NOTES_DIR, STORE_DIR};
use crate::runtime::bounded_read::{ReadError, read_regular};

/// The most bytes read from a rules file.
const RULES_CAP: u64 = 1024 * 1024;

/// What the edit did.
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
    /// The visible line, if the outcome has one: an edit or a failure. An
    /// ignored or shared folder, which the user chose, says nothing.
    pub fn line(&self) -> Option<String> {
        match self {
            Self::AlreadyIgnored | Self::Shared => None,
            Self::Created | Self::Added => Some(
                "added .lzr/ to .gitignore (remove the entry to share memory through git)"
                    .to_owned(),
            ),
            Self::Failed(reason) => Some(format!(
                "could not update .gitignore: {reason}; .lzr/ may be committed"
            )),
        }
    }
}

/// The words for a read that failed.
fn reason(error: ReadError) -> String {
    match error {
        ReadError::TooLarge => "larger than 1 MiB",
        ReadError::NotRegular => "not a regular file",
        ReadError::Missing | ReadError::Unreadable => "cannot be read",
    }
    .to_owned()
}

/// Appends `entry` to the file at `path`. The open never follows a link and
/// never waits: a FIFO put there after the check fails with `ENXIO`.
fn append(path: &Path, entry: &str) -> Result<(), String> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .and_then(|mut f| f.write_all(entry.as_bytes()))
        .map_err(|e| match e.raw_os_error() {
            Some(libc::ENXIO | libc::ELOOP) => "not a regular file".to_owned(),
            _ => e.to_string(),
        })
}

/// The bytes of the regular file at `path`; `None` when it does not exist.
/// A link, a FIFO or any other kind of file is refused, and so is one over
/// [`RULES_CAP`].
fn read_rules(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
        Ok(meta) if meta.is_file() => read_regular(path, RULES_CAP).map(Some).map_err(reason),
        Ok(_) => Err("not a regular file".to_owned()),
    }
}

/// The `info/exclude` file of the repository at `root`, or `None` when the
/// `.git` file points somewhere that cannot be trusted. Like the worktree
/// rules in `workspace::memory_root`, a `.git` file is believed only when it
/// names a folder inside a real `.git/modules/` or `.git/worktrees/`, and a
/// `commondir` only when it is a `.git` folder holding that git folder.
fn exclude_path(root: &Path) -> Option<PathBuf> {
    let dot_git = root.join(".git");
    if !fs::symlink_metadata(&dot_git).ok()?.is_file() {
        return Some(dot_git.join("info/exclude"));
    }
    let text = String::from_utf8(read_regular(&dot_git, RULES_CAP).ok()?).ok()?;
    let named = text.lines().find_map(|line| line.strip_prefix("gitdir:"))?;
    let gitdir = root.join(named.trim()).canonicalize().ok()?;
    let inside_git = gitdir
        .parent()
        .and_then(Path::parent)
        .is_some_and(|dir| dir.file_name().is_some_and(|name| name == ".git"));
    if !inside_git || !gitdir.is_dir() {
        return None;
    }
    let git = match read_regular(&gitdir.join("commondir"), 4096) {
        Ok(bytes) => {
            let common = gitdir
                .join(String::from_utf8_lossy(&bytes).trim())
                .canonicalize()
                .ok()?;
            let real = common.file_name().is_some_and(|name| name == ".git");
            if !real || !gitdir.starts_with(&common) {
                return None;
            }
            common
        }
        Err(_) => gitdir,
    };
    Some(git.join("info/exclude"))
}

/// Decides and applies the edit for the repository at `root`.
pub fn ensure(root: &Path) -> GitignoreOutcome {
    let file = root.join(".gitignore");
    let current = match read_rules(&file) {
        Ok(current) => current,
        Err(reason) => return GitignoreOutcome::Failed(reason),
    };
    // A rules file that cannot be read adds no rule: the entry is then added.
    let exclude = exclude_path(root).and_then(|path| read_rules(&path).ok().flatten());
    let mut matcher = GitignoreBuilder::new(root);
    for text in [&exclude, &current].into_iter().flatten() {
        for line in String::from_utf8_lossy(text).lines() {
            let _ = matcher.add_line(None, line);
        }
    }
    // The folder itself is what is asked about, so `*.json` does not count.
    // `.lzr/*` and `.lzr/memory/notes/` leave the folder itself unmatched but
    // ignore everything in it, so the notes folder is asked about too.
    if let Ok(matcher) = matcher.build() {
        for dir in [STORE_DIR.to_owned(), format!("{STORE_DIR}/{NOTES_DIR}")] {
            match matcher.matched_path_or_any_parents(&dir, true) {
                Match::Ignore(_) => return GitignoreOutcome::AlreadyIgnored,
                Match::Whitelist(_) => return GitignoreOutcome::Shared,
                Match::None => {}
            }
        }
    }
    let eol = match &current {
        Some(bytes) if bytes.windows(2).any(|w| w == b"\r\n") => "\r\n",
        _ => "\n",
    };
    let unterminated = current
        .as_ref()
        .is_some_and(|b| !b.is_empty() && !b.ends_with(b"\n"));
    let entry = format!("{}{STORE_DIR}/{eol}", if unterminated { eol } else { "" });
    let written = append(&file, &entry);
    match (written, current) {
        (Err(reason), _) => GitignoreOutcome::Failed(reason),
        (Ok(()), None) => GitignoreOutcome::Created,
        (Ok(()), Some(_)) => GitignoreOutcome::Added,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt};

    use super::*;

    const ADDED: &str = "added .lzr/ to .gitignore (remove the entry to share memory through git)";

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".git/info")).unwrap();
        dir
    }

    fn ignore_file(dir: &tempfile::TempDir) -> String {
        fs::read_to_string(dir.path().join(".gitignore")).unwrap()
    }

    /// Writes by permission do not bind root: those tests skip there.
    fn is_root() -> bool {
        // SAFETY: geteuid has no preconditions and cannot fail.
        unsafe { libc::geteuid() == 0 }
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
    fn a_rule_for_the_folders_contents_counts_as_ignoring_it() {
        for entry in [
            ".lzr/*",
            "**/.lzr",
            "**/.lzr/",
            ".lzr/**",
            ".lzr/memory/notes/",
        ] {
            let dir = repo();
            let text = format!("target/\n{entry}\n");
            fs::write(dir.path().join(".gitignore"), &text).unwrap();
            assert_eq!(
                ensure(dir.path()),
                GitignoreOutcome::AlreadyIgnored,
                "{entry}"
            );
            assert_eq!(ignore_file(&dir), text, "{entry}");
        }
    }

    /// A checkout `sub` whose `.git` file names `gitdir`, inside `dir`.
    fn checkout(dir: &tempfile::TempDir, gitdir: &Path) -> PathBuf {
        let sub = dir.path().join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
        sub
    }

    #[test]
    fn a_git_file_points_to_the_info_exclude_of_its_git_folder() {
        let dir = tempfile::tempdir().unwrap();
        let git = dir.path().join("super/.git/modules/x");
        fs::create_dir_all(git.join("info")).unwrap();
        fs::write(git.join("info/exclude"), ".lzr/\n").unwrap();
        let sub = checkout(&dir, &git);
        assert_eq!(ensure(&sub), GitignoreOutcome::AlreadyIgnored);
        assert!(!sub.join(".gitignore").exists());
    }

    #[test]
    fn a_git_folder_with_a_commondir_uses_the_common_info_exclude() {
        let dir = tempfile::tempdir().unwrap();
        let common = dir.path().join("main/.git");
        let gitdir = common.join("worktrees/w");
        fs::create_dir_all(common.join("info")).unwrap();
        fs::create_dir_all(&gitdir).unwrap();
        fs::write(common.join("info/exclude"), ".lzr/\n").unwrap();
        fs::write(gitdir.join("commondir"), "../..\n").unwrap();
        let sub = checkout(&dir, &gitdir);
        assert_eq!(ensure(&sub), GitignoreOutcome::AlreadyIgnored);
    }

    #[test]
    fn a_git_file_that_points_outside_a_git_folder_is_not_trusted() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = dir.path().join("elsewhere");
        fs::create_dir_all(elsewhere.join("info")).unwrap();
        fs::write(elsewhere.join("info/exclude"), ".lzr/\n").unwrap();
        let sub = checkout(&dir, &elsewhere);
        assert_eq!(
            ensure(&sub),
            GitignoreOutcome::Created,
            "its exclude is not read"
        );
    }

    #[test]
    fn a_commondir_that_leaves_the_git_folder_is_not_trusted() {
        let dir = tempfile::tempdir().unwrap();
        let gitdir = dir.path().join("main/.git/worktrees/w");
        let other = dir.path().join("other");
        fs::create_dir_all(&gitdir).unwrap();
        fs::create_dir_all(other.join("info")).unwrap();
        fs::write(other.join("info/exclude"), ".lzr/\n").unwrap();
        fs::write(gitdir.join("commondir"), format!("{}\n", other.display())).unwrap();
        let sub = checkout(&dir, &gitdir);
        assert_eq!(ensure(&sub), GitignoreOutcome::Created);
    }

    #[test]
    fn a_rule_for_json_files_does_not_count_as_ignoring_the_folder() {
        let dir = repo();
        fs::write(dir.path().join(".gitignore"), "*.json\n").unwrap();
        assert_eq!(ensure(dir.path()), GitignoreOutcome::Added);
        assert_eq!(ignore_file(&dir), "*.json\n.lzr/\n");
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
        if is_root() {
            return;
        }
        let dir = repo();
        let file = dir.path().join(".gitignore");
        fs::write(&file, "target/\n").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o444)).unwrap();
        let outcome = ensure(dir.path());
        assert!(
            matches!(outcome, GitignoreOutcome::Failed(_)),
            "{outcome:?}"
        );
        let line = outcome.line().unwrap();
        assert!(line.starts_with("could not update .gitignore: "), "{line}");
        assert!(line.ends_with("; .lzr/ may be committed"), "{line}");
        assert_eq!(ignore_file(&dir), "target/\n");
    }

    #[test]
    fn a_symlinked_gitignore_is_refused_and_its_target_untouched() {
        let dir = repo();
        let target = dir.path().join("elsewhere");
        fs::write(&target, "keep\n").unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join(".gitignore")).unwrap();
        assert_eq!(
            ensure(dir.path()),
            GitignoreOutcome::Failed("not a regular file".into())
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "keep\n");
        let dangling = repo();
        std::os::unix::fs::symlink(
            dangling.path().join("nowhere"),
            dangling.path().join(".gitignore"),
        )
        .unwrap();
        assert!(matches!(
            ensure(dangling.path()),
            GitignoreOutcome::Failed(_)
        ));
        assert!(!dangling.path().join("nowhere").exists());
    }

    #[test]
    fn a_fifo_gitignore_is_refused_without_blocking() {
        let dir = repo();
        let path = std::ffi::CString::new(dir.path().join(".gitignore").to_str().unwrap()).unwrap();
        // SAFETY: `path` is a valid NUL-terminated string for the call.
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        assert_eq!(
            ensure(dir.path()),
            GitignoreOutcome::Failed("not a regular file".into())
        );
    }

    #[test]
    fn a_huge_gitignore_is_refused_instead_of_read_whole() {
        let dir = repo();
        fs::write(dir.path().join(".gitignore"), vec![b'#'; 2 * 1024 * 1024]).unwrap();
        let outcome = ensure(dir.path());
        assert_eq!(
            outcome,
            GitignoreOutcome::Failed("larger than 1 MiB".into())
        );
        assert_eq!(
            outcome.line().as_deref(),
            Some("could not update .gitignore: larger than 1 MiB; .lzr/ may be committed")
        );
        assert_eq!(
            fs::metadata(dir.path().join(".gitignore")).unwrap().len(),
            2 * 1024 * 1024
        );
    }

    #[test]
    fn a_fifo_swapped_in_after_the_check_fails_the_open_instead_of_blocking() {
        let dir = repo();
        let file = dir.path().join(".gitignore");
        let path = std::ffi::CString::new(file.to_str().unwrap()).unwrap();
        // SAFETY: `path` is a valid NUL-terminated string for the call.
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        assert_eq!(append(&file, ".lzr/\n"), Err("not a regular file".into()));
    }

    #[test]
    fn a_read_failure_names_its_reason_in_words() {
        assert_eq!(reason(ReadError::TooLarge), "larger than 1 MiB");
        assert_eq!(reason(ReadError::NotRegular), "not a regular file");
        assert_eq!(reason(ReadError::Unreadable), "cannot be read");
        assert_eq!(reason(ReadError::Missing), "cannot be read");
    }

    // Spec (project-memory-store, Gitignore handling): the lines.
    #[test]
    fn only_a_real_edit_or_a_failure_has_a_line() {
        let line = |o: GitignoreOutcome| o.line();
        assert_eq!(line(GitignoreOutcome::AlreadyIgnored), None);
        assert_eq!(line(GitignoreOutcome::Shared), None);
        assert_eq!(line(GitignoreOutcome::Created).as_deref(), Some(ADDED));
        assert_eq!(line(GitignoreOutcome::Added).as_deref(), Some(ADDED));
        assert_eq!(
            line(GitignoreOutcome::Failed("disk full".into())).as_deref(),
            Some("could not update .gitignore: disk full; .lzr/ may be committed")
        );
    }
}
