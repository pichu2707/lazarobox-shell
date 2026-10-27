//! Unpacking a verified `llama.cpp` archive without trusting it. The system
//! `tar` does the work (no crate for one feature) under `LC_ALL=C`: the
//! member names are listed and judged before anything is written, and the
//! result is walked afterwards, because a listing cannot say what a tool
//! then writes. Only the flags GNU tar and bsdtar share are used.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::core::ai::install;
use crate::runtime::ai_detect::find_on_path;

/// What a host without `tar` reads as.
pub const NO_TAR: &str = "tar is not installed: the archive cannot be unpacked";

/// More members than any llama.cpp build has by far.
const MAX_ENTRIES: usize = 10_000;

/// Deepest folder nesting the walk follows.
const MAX_DEPTH: usize = 32;

/// The `tar` found on `path`.
pub fn find_tar(path: &OsStr) -> Result<PathBuf, String> {
    find_on_path("tar", path).ok_or_else(|| NO_TAR.to_owned())
}

/// `tar` with `args`: C locale (its output is read), no `TAR_OPTIONS`, and
/// the `PATH` it needs to find `gzip`. Its stdout, or its last error line.
fn tar_output(tar: &Path, args: &[&OsStr]) -> Result<String, String> {
    let output = Command::new(tar)
        .args(args)
        .env("LC_ALL", "C")
        .env_remove("TAR_OPTIONS")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("tar did not run: {e}"))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let line = stderr.lines().rev().find(|l| !l.trim().is_empty());
    Err(format!(
        "tar failed: {}",
        line.unwrap_or("no message").trim()
    ))
}

/// Unpacks `archive` into the empty folder `staging`, so that it holds the
/// single folder `top` and nothing that points out of it: every member name
/// and hardlink target is judged from the listing, nothing but files,
/// folders and bare-name symlinks may be in the result, and no file may be
/// linked to one outside it. Ownership is not taken from the archive. A
/// refusal leaves `staging` for the caller to delete.
pub fn extract(tar: &Path, archive: &Path, staging: &Path, top: &str) -> Result<(), String> {
    let list = tar_output(tar, &["-tzf".as_ref(), archive.as_os_str()])?;
    let mut count = 0;
    for name in list.lines() {
        count += 1;
        if count > MAX_ENTRIES {
            return Err(format!("archive has too many members (over {MAX_ENTRIES})"));
        }
        install::check_member(name, top)?;
    }
    if count == 0 {
        return Err("the archive is empty".to_owned());
    }
    let verbose = tar_output(
        tar,
        &[
            "-tzvf".as_ref(),
            archive.as_os_str(),
            "--numeric-owner".as_ref(),
        ],
    )?;
    for line in verbose.lines() {
        check_listed_kind(line, top)?;
    }
    tar_output(
        tar,
        &[
            "-xzf".as_ref(),
            archive.as_os_str(),
            "-C".as_ref(),
            staging.as_os_str(),
            "--no-same-owner".as_ref(),
        ],
    )?;
    check_result(staging, top)
}

/// One line of the verbose listing: its first character is the member type.
/// Files, folders and symlinks pass (a symlink is judged where it landed);
/// a hardlink needs a target inside the top folder; the rest (devices,
/// fifos) is refused.
fn check_listed_kind(line: &str, top: &str) -> Result<(), String> {
    match line.chars().next() {
        Some('-' | 'd' | 'l') => Ok(()),
        Some('h') => match line.split_once(" link to ") {
            Some((_, target)) => install::check_hardlink_target(target, top),
            None => Err(format!("unreadable hardlink entry {line:?}")),
        },
        _ => Err(format!(
            "archive member is not a file or a folder: {line:?}"
        )),
    }
}

/// What the result of the extraction must look like, read from the disk.
fn check_result(staging: &Path, top: &str) -> Result<(), String> {
    let mut found = fs::read_dir(staging).map_err(|e| e.to_string())?;
    let first = found.next().transpose().map_err(|e| e.to_string())?;
    let only_top = first
        .as_ref()
        .is_some_and(|e| e.file_name() == top && found.next().is_none());
    let top_dir = staging.join(top);
    if !only_top || !fs::symlink_metadata(&top_dir).is_ok_and(|m| m.is_dir()) {
        return Err(format!("the archive does not hold just {top}/"));
    }
    // (device, inode) -> (links the inode has, links found in the tree).
    let mut links = HashMap::new();
    walk(&top_dir, 0, &mut links)?;
    if links.values().any(|(have, seen)| have != seen) {
        return Err("the archive links a file to one outside the install".to_owned());
    }
    Ok(())
}

fn walk(
    dir: &Path,
    depth: usize,
    links: &mut HashMap<(u64, u64), (u64, u64)>,
) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err("the archive nests folders too deep".to_owned());
    }
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        let kind = meta.file_type();
        if kind.is_dir() {
            walk(&path, depth + 1, links)?;
        } else if kind.is_file() {
            if meta.mode() & 0o6000 != 0 {
                return Err(format!("{} is setuid or setgid", path.display()));
            }
            let seen = links
                .entry((meta.dev(), meta.ino()))
                .or_insert((meta.nlink(), 0));
            seen.1 += 1;
        } else if kind.is_symlink() {
            check_symlink(&path, dir)?;
        } else {
            return Err(format!("{} is not a file or a folder", path.display()));
        }
    }
    Ok(())
}

/// A symlink is a bare name that exists in its own folder.
fn check_symlink(path: &Path, dir: &Path) -> Result<(), String> {
    let target = fs::read_link(path).map_err(|e| e.to_string())?;
    let target = target
        .to_str()
        .ok_or_else(|| "a symlink target is not text".to_owned())?;
    install::check_link_target(target)?;
    if fs::symlink_metadata(dir.join(target)).is_err() {
        return Err(format!("dangling symlink {} -> {target}", path.display()));
    }
    Ok(())
}

#[cfg(test)]
pub(in crate::runtime::llama_install) mod tests;
