//! Reads the prompt override files `<config dir>/prompts/<profile>/<task>.md`.
//! Files are untrusted: regular, valid UTF-8 and at most 16 KiB, read with a
//! bound and without ever blocking on a special file.

use std::{
    fs::OpenOptions,
    io::{ErrorKind, Read},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

use crate::core::ai::{
    chat::prompts::{Override, Reason, Resolved, resolve_prompt},
    config::{Profile, Task},
};

/// The largest override file used, in bytes.
pub const OVERRIDE_CAP: u64 = 16 * 1024;

/// Reads one override file. Never reads more than the cap plus one byte.
///
/// The file is opened without blocking (a FIFO would otherwise wait for a
/// writer) and its type is checked on the open handle, so a symlink to a
/// device is refused before a single byte is read.
pub fn read_override(path: &Path) -> Override {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Override::Missing,
        Err(_) => return Override::Skipped(Reason::Unreadable),
    };
    match file.metadata() {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return Override::Skipped(Reason::NotRegular),
        Err(_) => return Override::Skipped(Reason::Unreadable),
    }
    let mut bytes = Vec::new();
    // A directory opens fine but its read fails: caught by the type check.
    if file.take(OVERRIDE_CAP + 1).read_to_end(&mut bytes).is_err() {
        return Override::Skipped(Reason::Unreadable);
    }
    if bytes.len() as u64 > OVERRIDE_CAP {
        return Override::Skipped(Reason::TooLarge);
    }
    match String::from_utf8(bytes) {
        Ok(text) if text.trim().is_empty() => Override::Missing,
        Ok(text) => Override::Text(text),
        Err(_) => Override::Skipped(Reason::NotUtf8),
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
