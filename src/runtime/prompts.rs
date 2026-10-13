//! Reads the prompt override files `<config dir>/prompts/<profile>/<task>.md`.
//! Files are untrusted: regular, valid UTF-8 and at most 16 KiB, read with a
//! bound and without ever blocking on a special file.

use std::path::Path;

use super::{
    bounded_read::{ReadError, read_regular},
    workspace::probe,
};
use crate::core::ai::{
    chat::{
        prompts::{Override, Reason, Resolved, resolve_prompt},
        workspace::{block, with_block},
    },
    config::{Profile, Task},
};

/// The name of the thread that prepares a prompt; a panic on it is caught.
pub const THREAD_NAME: &str = "prepare-prompt";

/// The system line when preparing the prompt failed (a panic or no thread).
pub const FAILED: &str =
    "prompt preparation failed: built-in prompt used, workspace context unavailable";
/// The system line when preparing the prompt took longer than the bound.
pub const TIMED_OUT: &str =
    "prompt preparation timed out: built-in prompt used, workspace context unavailable";

/// The largest override file used, in bytes.
pub const OVERRIDE_CAP: u64 = 16 * 1024;

/// Reads one override file. Never reads more than the cap plus one byte.
pub fn read_override(path: &Path) -> Override {
    match read_regular(path, OVERRIDE_CAP) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) if text.trim().is_empty() => Override::Missing,
            Ok(text) => Override::Text(text),
            Err(_) => Override::Skipped(Reason::NotUtf8),
        },
        Err(ReadError::Missing) => Override::Missing,
        Err(ReadError::NotRegular) => Override::Skipped(Reason::NotRegular),
        Err(ReadError::TooLarge) => Override::Skipped(Reason::TooLarge),
        Err(ReadError::Unreadable) => Override::Skipped(Reason::Unreadable),
    }
}

/// Resolves the prompt of `task` from the files under `config_dir`. `None`
/// (no config directory) means no files.
pub fn load_prompt(config_dir: Option<&Path>, profile: Profile, task: Task) -> Resolved {
    let read = |level: Task| match config_dir {
        Some(dir) => read_override(
            &dir.join("prompts")
                .join(profile.id())
                .join(format!("{}.md", level.id())),
        ),
        None => Override::Missing,
    };
    let general = if task == Task::General {
        Override::Missing
    } else {
        read(Task::General)
    };
    resolve_prompt(profile, task, read(task), general)
}

/// The system prompt of a conversation: the resolved prompt of the chat task
/// and, when `context` is set, the workspace block after a blank line.
/// `home` is `$HOME`, `shell` the value of `$SHELL` and `date` the local
/// date, all read by the caller so this stays testable.
pub fn prepare_prompt(
    config_dir: Option<&Path>,
    profile: Profile,
    origin: Option<&Path>,
    home: Option<&Path>,
    context: bool,
    shell: Option<&str>,
    date: String,
) -> Resolved {
    let mut resolved = load_prompt(config_dir, profile, Task::Chat);
    if context {
        let block = block(&probe(origin, home, shell, date));
        resolved.text = with_block(&resolved.text, Some(&block));
    }
    resolved
}

/// The prompt used when `prepare_prompt` cannot answer: the built-in chat
/// prompt (no override files, no workspace block) and `notice` as the system
/// line that says why.
pub fn fallback_prompt(profile: Profile, notice: &str) -> Resolved {
    let mut resolved = resolve_prompt(profile, Task::Chat, Override::Missing, Override::Missing);
    resolved.notices.push(notice.to_owned());
    resolved
}

#[cfg(test)]
mod tests;
