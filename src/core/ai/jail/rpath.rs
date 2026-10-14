//! Where a dynamic executable looks for its shared libraries, read from the
//! ELF file itself (`DT_RPATH` or `DT_RUNPATH`). Pure: the caller supplies the
//! first bytes of the file and a way to read more.
//!
//! Some installs (Homebrew) link their programs with a long search path of
//! their own folders instead of the system ones, so the jail must let the
//! program read exactly those folders, and no others, to start at all. Only
//! the executable's own list is read: the search paths of the libraries it
//! loads are not (Homebrew sets the same one on all of them).

use std::path::{Component, Path, PathBuf};

use super::{le_u16, le_u32, le_u64};

/// The longest search path read.
pub const RPATH_MAX: usize = 16 * 1024;

/// The largest dynamic section read.
const DYNAMIC_MAX: usize = 64 * 1024;

const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const DT_NULL: u64 = 0;
const DT_STRTAB: u64 = 5;
const DT_RPATH: u64 = 15;
const DT_RUNPATH: u64 = 29;

/// A mapped piece of the file: virtual address, file offset, size.
struct Load {
    vaddr: u64,
    offset: u64,
    size: u64,
}

/// The file offset of virtual address `vaddr`, if a `PT_LOAD` maps it.
fn file_offset(loads: &[Load], vaddr: u64) -> Option<u64> {
    loads
        .iter()
        .find(|l| vaddr >= l.vaddr && vaddr - l.vaddr < l.size)
        .and_then(|l| l.offset.checked_add(vaddr - l.vaddr))
}

/// The library folders a little-endian ELF64 executable names in its own
/// `DT_RUNPATH` (else `DT_RPATH`, as the loader does), in order. `head` holds
/// the first bytes of the file, `origin` is the folder of the executable
/// (`$ORIGIN`) and `read_at(offset, len)` reads up to `len` bytes of the file.
///
/// Only plain absolute folders are returned: an empty entry (the current
/// folder), a relative one and one with `$LIB` or `$PLATFORM` depend on the
/// process or the machine and are dropped. Empty for anything that is not
/// such an ELF, has no list, or is malformed.
pub fn search_dirs(
    head: &[u8],
    origin: &Path,
    read_at: &dyn Fn(u64, usize) -> Option<Vec<u8>>,
) -> Vec<PathBuf> {
    raw_list(head, read_at)
        .map(|list| expand(&list, origin))
        .unwrap_or_default()
}

fn raw_list(head: &[u8], read_at: &dyn Fn(u64, usize) -> Option<Vec<u8>>) -> Option<String> {
    if !head.starts_with(b"\x7fELF\x02\x01") {
        return None;
    }
    let table = usize::try_from(le_u64(head, 0x20)?).ok()?;
    let entry = usize::from(le_u16(head, 0x36)?);
    let count = usize::from(le_u16(head, 0x38)?);
    if entry < 56 || table.checked_add(entry.checked_mul(count)?)? > head.len() {
        return None;
    }
    let mut loads = Vec::new();
    let mut dynamic = None;
    for i in 0..count {
        let at = table + i * entry;
        let (offset, vaddr, size) = (
            le_u64(head, at + 8)?,
            le_u64(head, at + 16)?,
            le_u64(head, at + 32)?,
        );
        match le_u32(head, at)? {
            PT_LOAD => loads.push(Load {
                vaddr,
                offset,
                size,
            }),
            PT_DYNAMIC => dynamic = Some((offset, usize::try_from(size).ok()?)),
            _ => {}
        }
    }
    let (offset, size) = dynamic?;
    let section = read_at(offset, size.min(DYNAMIC_MAX))?;
    let (mut strtab, mut rpath, mut runpath) = (None, None, None);
    for pair in section.chunks_exact(16) {
        let (tag, value) = (le_u64(pair, 0)?, le_u64(pair, 8)?);
        match tag {
            DT_NULL => break,
            DT_STRTAB => strtab = Some(value),
            DT_RPATH => rpath = Some(value),
            DT_RUNPATH => runpath = Some(value),
            _ => {}
        }
    }
    let at = file_offset(&loads, strtab?)?.checked_add(runpath.or(rpath)?)?;
    let raw = read_at(at, RPATH_MAX)?;
    let end = raw.iter().take(RPATH_MAX).position(|b| *b == 0)?;
    String::from_utf8(raw[..end].to_vec()).ok()
}

fn expand(list: &str, origin: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for entry in list.split(':') {
        let entry = entry.replace("${ORIGIN}", "$ORIGIN");
        let path = match entry.strip_prefix("$ORIGIN") {
            Some("") => origin.to_path_buf(),
            Some(rest) if rest.starts_with('/') => {
                if rest.contains('$') {
                    continue;
                }
                origin.join(rest.trim_start_matches('/'))
            }
            Some(_) => continue,
            None if entry.starts_with('/') && !entry.contains('$') => PathBuf::from(&entry),
            None => continue,
        };
        let path = lexical(&path);
        if !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

/// `path` without its `.` and `..` segments (`$ORIGIN/../lib` is `<up>/lib`).
/// Lexical only: the caller resolves symlinks.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// The OpenSSL files a Homebrew program reads when it starts. Its libraries
/// are under `<prefix>/opt/openssl@N/lib`, and the library looks for its
/// config and the CA bundle in `<prefix>/etc/openssl@N`, outside every
/// library folder: without the config the program stops at startup, without
/// the bundle it cannot verify a certificate. Both are read-only files.
pub fn homebrew_openssl_files(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for dir in dirs {
        let Some(etc) = openssl_etc(dir) else {
            continue;
        };
        for file in ["openssl.cnf", "cert.pem"] {
            let path = etc.join(file);
            if !out.contains(&path) {
                out.push(path);
            }
        }
    }
    out
}

/// `<prefix>/etc/openssl@N` for a folder shaped `<prefix>/opt/openssl@N/lib`.
fn openssl_etc(dir: &Path) -> Option<PathBuf> {
    if dir.file_name()? != "lib" {
        return None;
    }
    let package = dir.parent()?;
    let name = package.file_name()?.to_str()?;
    let version = name.strip_prefix("openssl@")?;
    if version.is_empty() || !version.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let opt = package.parent()?;
    if opt.file_name()? != "opt" {
        return None;
    }
    Some(opt.parent()?.join("etc").join(name))
}

#[cfg(test)]
pub(crate) mod tests;
