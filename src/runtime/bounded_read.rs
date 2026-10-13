//! Reads an untrusted file with a bound, without ever blocking on a special
//! file.

use std::{
    fs::OpenOptions,
    io::{ErrorKind, Read},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

/// Why a file was not read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadError {
    Missing,
    NotRegular,
    /// More than the cap: nothing is returned, never a truncated prefix.
    TooLarge,
    Unreadable,
}

/// The bytes of the regular file at `path`, at most `cap` of them.
///
/// The file is opened without blocking (a FIFO would otherwise wait for a
/// writer) and its type is checked on the open handle, so a symlink to a
/// device is refused before a single byte is read. At most `cap + 1` bytes
/// are ever read.
pub fn read_regular(path: &Path, cap: u64) -> Result<Vec<u8>, ReadError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| match error.kind() {
            ErrorKind::NotFound => ReadError::Missing,
            _ => ReadError::Unreadable,
        })?;
    match file.metadata() {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return Err(ReadError::NotRegular),
        Err(_) => return Err(ReadError::Unreadable),
    }
    let mut bytes = Vec::new();
    file.take(cap + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ReadError::Unreadable)?;
    if bytes.len() as u64 > cap {
        return Err(ReadError::TooLarge);
    }
    Ok(bytes)
}
