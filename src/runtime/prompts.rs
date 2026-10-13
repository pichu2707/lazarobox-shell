//! Reads the prompt override files `<config dir>/prompts/<profile>/<task>.md`.
//! Files are untrusted: regular, valid UTF-8 and at most 16 KiB, read with a
//! bound and without ever blocking on a special file.

use std::path::Path;

use super::bounded_read::{ReadError, read_regular};
use crate::core::ai::{
    chat::prompts::{Override, Reason, Resolved, resolve_prompt},
    config::{Profile, Task},
};

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

#[cfg(test)]
mod tests;
