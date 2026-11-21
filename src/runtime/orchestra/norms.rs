//! The repo norm files a judge rules against (acceptance-judges D3): read by
//! lazarobox, never by the judge, from the run's origin.

use std::fs::OpenOptions;
use std::io::Read as _;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use crate::core::ai::orchestra::judge::{self, NORM_BYTES, NORM_SLACK, Norm};

/// The norms of a run and what the transcript says about them.
#[derive(Clone, Default, Debug)]
pub(super) struct Read {
    pub norms: Vec<Norm>,
    pub said: Vec<String>,
}

/// `AGENTS.md`, then `CLAUDE.md`, from `origin`. A symlink counts only when
/// its real target is a regular file inside `origin`, and a file reached by
/// two names is sent once. A missing file is silent, anything else that is not
/// read says so. Blocking: call it on the blocking pool.
pub(super) fn read(origin: &Path) -> Read {
    let mut read = Read::default();
    let mut seen: Vec<PathBuf> = Vec::new();
    for name in judge::NORM_FILES {
        let (bytes, total, real) = match head(origin, name) {
            Ok(Some(got)) => got,
            Ok(None) => continue,
            Err(line) => {
                read.said.push(line);
                continue;
            }
        };
        if seen.contains(&real) {
            continue;
        }
        seen.push(real);
        match judge::norm(name, &bytes, total) {
            Some(norm) => {
                read.said.extend(judge::norms_cut_line(&norm));
                read.said.extend(judge::norms_redacted_line(&norm));
                read.norms.push(norm);
            }
            None => read.said.push(judge::norms_empty_line(name)),
        }
    }
    read
}

/// What [`head`] read: the bytes, the file's size and its real path.
type Head = (Vec<u8>, u64, PathBuf);

/// The start of the regular file `name` of `origin` (a little past
/// [`NORM_BYTES`], for the redaction) and its size; `None` when it is missing.
/// `Err` holds the line to say.
fn head(origin: &Path, name: &str) -> Result<Option<Head>, String> {
    let not_regular = || format!("judge: {name} is not a regular file and was not read");
    let unread = || format!("judge: {name} could not be read");
    let path = origin.join(name);
    match std::fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(unread()),
    }
    // It exists, so a failure here is a link to nothing or a loop.
    let real = path.canonicalize().map_err(|_| unread())?;
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
    file.take((NORM_BYTES + NORM_SLACK) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| unread())?;
    Ok(Some((bytes, meta.len(), real)))
}

#[cfg(test)]
mod tests;
