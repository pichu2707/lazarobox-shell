//! The repo norm files a judge rules against (acceptance-judges D3): read by
//! lazarobox, never by the judge, from the run's origin.

use std::fs::OpenOptions;
use std::io::Read as _;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use crate::core::ai::orchestra::judge::{self, NORM_BYTES, Norm};

/// The norms of a run and what the transcript says about them.
#[derive(Clone, Default, Debug)]
pub(super) struct Read {
    pub norms: Vec<Norm>,
    pub said: Vec<String>,
}

/// `AGENTS.md`, then `CLAUDE.md`, from `origin`. A symlink counts only when
/// its real target is a regular file inside `origin`; a missing file is
/// silent, anything else that is not read says so. Blocking: call it on the
/// blocking pool.
pub(super) fn read(origin: &Path) -> Read {
    let mut read = Read::default();
    for name in judge::NORM_FILES {
        match head(origin, name) {
            Ok(None) => {}
            Ok(Some((bytes, total))) => match judge::norm(name, &bytes, total) {
                Some(norm) => {
                    read.said.extend(judge::norms_cut_line(&norm));
                    read.said.extend(judge::norms_redacted_line(&norm));
                    read.norms.push(norm);
                }
                None => read.said.push(judge::norms_empty_line(name)),
            },
            Err(line) => read.said.push(line),
        }
    }
    read
}

/// The first [`NORM_BYTES`] of the regular file `name` of `origin` and its
/// size; `None` when it is missing. `Err` holds the line to say.
fn head(origin: &Path, name: &str) -> Result<Option<(Vec<u8>, u64)>, String> {
    let not_regular = || format!("judge: {name} is not a regular file and was not read");
    let unread = || format!("judge: {name} could not be read");
    let real = match origin.join(name).canonicalize() {
        Ok(real) => real,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(unread()),
    };
    let inside = origin
        .canonicalize()
        .is_ok_and(|root| real.starts_with(root));
    if !inside {
        return Err(not_regular());
    }
    let flags = libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(flags)
        .open(&real)
        .map_err(|_| unread())?;
    let meta = file.metadata().map_err(|_| unread())?;
    if !meta.is_file() {
        return Err(not_regular());
    }
    let mut bytes = Vec::new();
    file.take(NORM_BYTES as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| unread())?;
    Ok(Some((bytes, meta.len())))
}

#[cfg(test)]
mod tests;
