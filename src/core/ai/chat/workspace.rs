//! The workspace context block: a few facts about where the user is, in the
//! system prompt. Pure: the runtime reads the filesystem and the clock and
//! hands the facts here as a `Workspace`.
//!
//! Every value is data (a branch name is chosen by whoever made the
//! repository), so each one is cleaned and cut before it is written, and an
//! absolute path never reaches the block.

use std::path::{Component, Path};

use crate::core::ai::sanitize::is_format;

/// The longest value written in the block, in characters.
pub const VALUE_CAP: usize = 64;

/// The first line of the block.
pub const HEADER: &str = "Workspace (provided by lazarobox, may be partial):";

/// Where `HEAD` points.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Head {
    Branch(String),
    /// The first characters of the commit a detached `HEAD` holds.
    Detached(String),
}

/// The cwd as it is shown, never absolute.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Cwd {
    /// Relative to the repository root (`.` at the root).
    InRepo(String),
    /// Outside a repository: the directory name only.
    Outside(String),
}

/// The facts of one conversation start. A missing fact is a missing line.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Workspace {
    pub project: Option<String>,
    pub head: Option<Head>,
    pub cwd: Option<Cwd>,
    pub os: String,
    pub shell: Option<String>,
    pub date: String,
}

/// Trims `raw`, drops control and format characters and cuts it at
/// `VALUE_CAP` characters. `None` when nothing is left.
pub fn clean_value(raw: &str) -> Option<String> {
    let kept: String = raw
        .trim()
        .chars()
        .filter(|c| !c.is_control() && !is_format(*c))
        .take(VALUE_CAP)
        .collect();
    let kept = kept.trim();
    (!kept.is_empty()).then(|| kept.to_owned())
}

/// What a `HEAD` file holds: `ref: refs/heads/<name>` is a branch and a bare
/// commit id (40 or 64 hex digits) is a detached head. Anything else is
/// unknown.
pub fn parse_head(raw: &str) -> Option<Head> {
    let line = raw.lines().next()?.trim();
    if let Some(name) = line.strip_prefix("ref: refs/heads/") {
        return (!name.is_empty()).then(|| Head::Branch(name.to_owned()));
    }
    let hex = matches!(line.len(), 40 | 64) && line.bytes().all(|b| b.is_ascii_hexdigit());
    hex.then(|| Head::Detached(line[..7].to_owned()))
}

/// `cwd` relative to the repository `root`; `.` when they are the same.
/// A cwd that is not under the root is shown by its name only.
pub fn cwd_in_repo(root: &Path, cwd: &Path) -> Cwd {
    match cwd.strip_prefix(root) {
        Ok(rest) => {
            let parts: Vec<String> = rest
                .components()
                .filter_map(|c| match c {
                    Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
                    _ => None,
                })
                .collect();
            Cwd::InRepo(if parts.is_empty() {
                ".".to_owned()
            } else {
                parts.join("/")
            })
        }
        Err(_) => cwd_outside(cwd),
    }
}

/// The directory name of `cwd` (`/` for the root).
pub fn cwd_outside(cwd: &Path) -> Cwd {
    let name = cwd
        .file_name()
        .map_or_else(|| "/".to_owned(), |n| n.to_string_lossy().into_owned());
    Cwd::Outside(name)
}

/// `YYYY-MM-DD`.
pub fn format_date(year: i32, month: u32, day: u32) -> String {
    format!("{year:04}-{month:02}-{day:02}")
}

/// The block text, without a trailing newline.
pub fn block(workspace: &Workspace) -> String {
    let mut lines = vec![HEADER.to_owned()];
    let mut line = |label: &str, value: Option<String>| {
        if let Some(value) = value {
            lines.push(format!("- {label}: {value}"));
        }
    };
    line(
        "project",
        workspace.project.as_deref().and_then(clean_value),
    );
    line(
        "git branch",
        workspace.head.as_ref().and_then(|head| match head {
            Head::Branch(name) => clean_value(name),
            Head::Detached(sha) => clean_value(sha).map(|sha| format!("detached at {sha}")),
        }),
    );
    line(
        "cwd",
        workspace.cwd.as_ref().and_then(|cwd| match cwd {
            Cwd::InRepo(path) => {
                clean_value(path).map(|p| format!("{p} (relative to the repository root)"))
            }
            Cwd::Outside(name) => clean_value(name).map(|n| format!("{n} (directory name only)")),
        }),
    );
    let os = clean_value(&workspace.os);
    let shell = workspace.shell.as_deref().and_then(clean_value);
    line(
        "os",
        match (os, shell) {
            (Some(os), Some(shell)) => Some(format!("{os} · shell: {shell}")),
            (Some(os), None) => Some(os),
            (None, Some(shell)) => Some(format!("shell: {shell}")),
            (None, None) => None,
        },
    );
    line("date", clean_value(&workspace.date));
    lines.join("\n")
}

/// The system prompt of a conversation: the resolved prompt, then the block
/// after a blank line. Without a block it is the prompt alone.
pub fn with_block(prompt: &str, block: Option<&str>) -> String {
    match block {
        Some(block) => format!("{prompt}\n\n{block}"),
        None => prompt.to_owned(),
    }
}

#[cfg(test)]
mod tests;
