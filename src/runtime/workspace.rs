//! Reads the facts of the workspace context block from the filesystem and
//! the clock. Nothing runs `git`: the repository is found by walking up from
//! the origin cwd and the branch comes from the `HEAD` file.

use std::{
    fs,
    path::{Path, PathBuf},
};

use super::bounded_read::read_regular;
use crate::core::ai::chat::{
    tags::clean_tag,
    workspace::{Cwd, Head, Workspace, cwd_in_repo, cwd_outside, format_date, parse_head},
};

/// How many directory levels the repository walk climbs.
pub const WALK_LIMIT: usize = 64;
/// The most bytes read from a `.git` file or a `HEAD` file.
pub const GIT_FILE_CAP: u64 = 1024;

/// The facts for a chat opened in `origin`. An unknown or vanished origin
/// gives the os, shell and date only. `home` is `$HOME`: a project or a cwd
/// that is that directory is shown as `~`, because its name is the username.
/// `shell` is the value of `$SHELL`.
pub fn probe(
    origin: Option<&Path>,
    home: Option<&Path>,
    shell: Option<&str>,
    date: String,
) -> Workspace {
    let shell = shell
        .map(Path::new)
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned());
    let mut facts = Workspace {
        project: None,
        head: None,
        cwd: None,
        os: std::env::consts::OS.to_owned(),
        shell,
        date,
    };
    let Some(cwd) = origin.filter(|dir| dir.is_dir()) else {
        return facts;
    };
    match find_repo(cwd) {
        Some(repo) => {
            facts.project = Some(if is_home(&repo.project_dir, home) {
                "~".to_owned()
            } else {
                name_of(&repo.project_dir)
            });
            facts.head = repo.head_file.as_deref().and_then(read_head);
            facts.cwd = Some(cwd_in_repo(&repo.root, cwd));
        }
        None if is_home(cwd, home) => facts.cwd = Some(Cwd::Home),
        None => facts.cwd = Some(cwd_outside(cwd)),
    }
    facts
}

/// The project tag proposed for a chat opened in `cwd`: the name of the
/// repository found by the same walk as the workspace block (the main
/// repository for a linked worktree), else the name of `cwd` itself. A cwd
/// that is not an existing directory has no tag. The name is cleaned like any
/// tag (see `clean_tag`).
pub fn project_tag(cwd: &Path) -> Option<String> {
    if !cwd.is_dir() {
        return None;
    }
    let dir = find_repo(cwd).map_or_else(|| cwd.to_owned(), |repo| repo.project_dir);
    clean_tag(&name_of(&dir))
}

/// The git root of a chat's origin `cwd`: the directory that holds `.git`,
/// found by the same walk as the workspace block. `None` outside a
/// repository. User MCP servers declared for a project apply to this root only.
pub fn origin_root(cwd: &Path) -> Option<PathBuf> {
    find_repo(cwd).map(|repo| repo.root)
}

/// A repository found above the cwd.
struct Repo {
    /// The directory that holds `.git`.
    root: PathBuf,
    /// The directory whose name is the project: the main repository's for a
    /// linked worktree.
    project_dir: PathBuf,
    head_file: Option<PathBuf>,
}

/// The nearest repository at or above `cwd`, within `WALK_LIMIT` levels.
fn find_repo(cwd: &Path) -> Option<Repo> {
    cwd.ancestors().take(WALK_LIMIT).find_map(inspect)
}

/// `dir` as a repository root: `.git` is a directory, or a file with a
/// `gitdir:` line (worktrees and submodules). A symlinked `.git` is not a
/// marker (it is not followed) and the walk goes on.
fn inspect(dir: &Path) -> Option<Repo> {
    let dot_git = dir.join(".git");
    let meta = fs::symlink_metadata(&dot_git).ok()?;
    if meta.is_dir() {
        return Some(Repo {
            root: dir.to_owned(),
            project_dir: dir.to_owned(),
            head_file: Some(dot_git.join("HEAD")),
        });
    }
    if !meta.is_file() {
        return None;
    }
    let gitdir = read_regular(&dot_git, GIT_FILE_CAP)
        .ok()
        .and_then(|bytes| gitdir_of(&String::from_utf8_lossy(&bytes)))
        .map(|path| dir.join(path));
    let project_dir = gitdir
        .as_deref()
        .and_then(main_repo_of)
        .unwrap_or_else(|| dir.to_owned());
    Some(Repo {
        root: dir.to_owned(),
        project_dir,
        head_file: gitdir.map(|gitdir| gitdir.join("HEAD")),
    })
}

/// The path of the first `gitdir:` line.
fn gitdir_of(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix("gitdir:"))
        .map(|path| path.trim().to_owned())
        .filter(|path| !path.is_empty())
}

/// For a linked worktree's gitdir (`<main>/.git/worktrees/<name>`), the main
/// repository directory.
fn main_repo_of(gitdir: &Path) -> Option<PathBuf> {
    let text = gitdir.to_string_lossy();
    let at = text.find("/.git/worktrees/")?;
    Some(PathBuf::from(&text[..at]))
}

/// Whether `dir` is the home directory, by canonical path when both resolve.
fn is_home(dir: &Path, home: Option<&Path>) -> bool {
    let Some(home) = home else { return false };
    match (dir.canonicalize(), home.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => dir == home,
    }
}

/// The final path component, `/` for the root.
fn name_of(dir: &Path) -> String {
    dir.file_name()
        .map_or_else(|| "/".to_owned(), |n| n.to_string_lossy().into_owned())
}

fn read_head(file: &Path) -> Option<Head> {
    let bytes = read_regular(file, GIT_FILE_CAP).ok()?;
    parse_head(&String::from_utf8_lossy(&bytes))
}

/// Today's local date as `YYYY-MM-DD`.
pub fn local_date() -> String {
    // SAFETY: `time` accepts a null pointer; `localtime_r` writes only into
    // the zeroed `tm` it is given and the `time_t` it reads is a local.
    let (year, month, day) = unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return String::new();
        }
        (tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday)
    };
    format_date(year, month as u32, day as u32)
}

#[cfg(test)]
mod tests;
