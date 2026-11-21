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

/// `AGENTS.md`, then `CLAUDE.md`, from `origin`. Only a regular file counts
/// and a symlink is never followed; a missing file is no error. Blocking:
/// call it on the blocking pool.
pub(super) fn read(origin: &Path) -> Read {
    let mut read = Read::default();
    for name in judge::NORM_FILES {
        match head(&origin.join(name)) {
            Ok(None) => {}
            Ok(Some((bytes, total))) => match judge::norm(name, &bytes, total) {
                Some(norm) => {
                    read.said.extend(judge::norms_cut_line(&norm));
                    read.norms.push(norm);
                }
                None => read.said.push(judge::norms_empty_line(name)),
            },
            Err(_) => read.said.push(format!("judge: {name} could not be read")),
        }
    }
    read
}

/// The first [`NORM_BYTES`] of a regular file and its size; `None` for a
/// missing file, a symlink or anything that is not a regular file.
fn head(path: &Path) -> std::io::Result<Option<(Vec<u8>, u64)>> {
    let flags = libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY;
    let file = match OpenOptions::new().read(true).custom_flags(flags).open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) if e.raw_os_error() == Some(libc::ELOOP) => return Ok(None),
        Err(e) => return Err(e),
    };
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    file.take(NORM_BYTES as u64).read_to_end(&mut bytes)?;
    Ok(Some((bytes, meta.len())))
}

#[cfg(test)]
mod tests;
