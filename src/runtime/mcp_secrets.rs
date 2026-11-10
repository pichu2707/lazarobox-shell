//! Reads `secrets.toml` (openspec `mcp-secrets`): bounded, without blocking
//! on a special file, and only when it is a regular file that only its owner
//! may read or write (mode 0600 or stricter). Called off the UI thread, on
//! each start of a chat's user servers, so an edit applies at the next start.
//!
//! Nothing here ever creates, writes, renames or chmods the file.

use std::{
    fs::OpenOptions,
    io::{ErrorKind, Read},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};

use crate::core::ai::mcp_secrets::{
    Loaded, MAX_BYTES, NOT_REGULAR, Secrets, TOO_LARGE, UNREADABLE, mode_problem,
};

/// The values of the file at `path`. A missing file is no values and no
/// problem; a file that is not used is no values and its one problem.
pub fn load(path: &Path) -> Loaded {
    match read_private(path) {
        Ok(Some(bytes)) => Secrets::parse(&bytes),
        Ok(None) => Loaded::default(),
        Err(problem) => Loaded::problem(problem),
    }
}

/// The bytes of the file, `None` when it does not exist. It is opened
/// without blocking (a FIFO would wait for a writer) and judged on the open
/// handle, so a link swapped after a check changes nothing: a regular file,
/// no group or other permission bit, at most [`MAX_BYTES`]. At most
/// `MAX_BYTES + 1` bytes are ever read.
fn read_private(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(UNREADABLE.to_owned()),
    };
    let meta = file.metadata().map_err(|_| UNREADABLE.to_owned())?;
    if !meta.is_file() {
        return Err(NOT_REGULAR.to_owned());
    }
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(mode_problem(path));
    }
    if meta.len() > MAX_BYTES {
        return Err(TOO_LARGE.to_owned());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| UNREADABLE.to_owned())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(TOO_LARGE.to_owned());
    }
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests;
