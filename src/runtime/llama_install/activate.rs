//! Putting a verified archive in place, and taking it away. Layout under the
//! owned root: `<tag>/` (the unpacked top folder, renamed so `llama-server`
//! sits right in it), `current` (a symlink to the active tag, swapped by
//! renaming a new link over the old one), `.download/`, `.staging-<pid>/`
//! and `.lock`. One `flock` on `.lock` keeps two installs, or an install and
//! a remove, from running at once.

use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio::process::Command;

use super::unpack::{extract, find_tar};
use crate::core::ai::install::{self, Release};
use crate::runtime::ai_detect::run_capped;

/// What a second install (or a remove during one) is told.
pub const LOCKED: &str = "another lazarobox is installing or removing llama.cpp; try again later";

/// How long the new `llama-server --version` may take.
const VERSION_LIMIT: Duration = Duration::from_secs(5);

/// The most output of that check that is read.
const VERSION_CAP: usize = 64 * 1024;

const LOCK_FILE: &str = ".lock";
const STAGING_PREFIX: &str = ".staging-";
const CURRENT: &str = "current";

/// Holds the `flock` on `<root>/.lock` until dropped.
struct Lock {
    _file: File,
}

fn lock(root: &Path) -> Result<Lock, String> {
    fs::create_dir_all(root).map_err(|e| format!("cannot create {}: {e}", root.display()))?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(root.join(LOCK_FILE))
        .map_err(|e| format!("cannot open the install lock: {e}"))?;
    // A process forked by another thread holds a copy of the descriptor until
    // its exec, which can make a free lock look taken for a moment: look a few
    // times before calling it held.
    for attempt in 0..5 {
        // SAFETY: flock on an fd this process owns for the whole call.
        let got = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if got == 0 {
            return Ok(Lock { _file: file });
        }
        if attempt < 4 {
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    Err(LOCKED.to_owned())
}

/// Deletes what an interrupted install left: every `.staging-*` folder. Only
/// called under the lock, so none of them is in use.
fn sweep_staging(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(STAGING_PREFIX)
        {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// Copies `archive` to `to` while hashing exactly the bytes copied, and
/// holds them to the release's size and digest. The archive is opened
/// without following a symlink; what is extracted is the private copy, so
/// a swap of the archive after the download changes nothing but the result
/// of this check.
fn copy_verified(archive: &Path, to: &Path, release: &Release) -> Result<(), String> {
    let swapped = || "the downloaded archive changed after it was checked".to_owned();
    if fs::symlink_metadata(archive).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("the archive path is a symlink; refusing it".to_owned());
    }
    let mut source = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(archive)
        .map_err(|e| format!("cannot open the archive: {e}"))?;
    if !source.metadata().is_ok_and(|m| m.is_file()) {
        return Err("the archive is not a regular file".to_owned());
    }
    let mut copy = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(to)
        .map_err(|e| e.to_string())?;
    let (mut hasher, mut total) = (Sha256::new(), 0u64);
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match source.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.to_string()),
        };
        hasher.update(&buf[..n]);
        copy.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        total += n as u64;
    }
    let sum = format!("{:x}", hasher.finalize());
    if (release.size > 0 && total != release.size) || !sum.eq_ignore_ascii_case(&release.sha256) {
        return Err(swapped());
    }
    Ok(())
}

/// Runs `<dir>/llama-server --version`; whether it started and exited 0.
async fn runs(dir: &Path, search: &OsStr) -> bool {
    let binary = dir.join("llama-server");
    let make = || {
        let mut command = Command::new(&binary);
        command
            .arg("--version")
            .env("PATH", search)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .process_group(0);
        command
    };
    run_capped(make, VERSION_LIMIT, VERSION_CAP).await.is_some()
}

/// Points `current` at `tag` by renaming a new link over the old one, so
/// there is never a moment without a `current`.
fn swap_current(root: &Path, tag: &str) -> io::Result<()> {
    let new = root.join("current.new");
    let _ = fs::remove_file(&new);
    std::os::unix::fs::symlink(tag, &new)?;
    fs::rename(&new, root.join(CURRENT)).inspect_err(|_| {
        let _ = fs::remove_file(&new);
    })
}

/// Deletes the tag folders but `keep` and `previous`: the previous one stays
/// for a manual rollback.
fn prune(root: &Path, keep: &str, previous: Option<&str>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_tag_dir = install::tag_number(&name).is_some()
            && fs::symlink_metadata(entry.path()).is_ok_and(|m| m.is_dir());
        if is_tag_dir && name != keep && Some(name.as_str()) != previous {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// The tag `current` points at, when it is one.
fn current_tag(root: &Path) -> Option<String> {
    let target = fs::read_link(root.join(CURRENT)).ok()?;
    let name = target.to_str()?.to_owned();
    install::tag_number(&name).map(|_| name)
}

/// Installs the verified `archive` of `release` under `root`: copy it into a
/// private staging folder (re-checking its digest), unpack it there,
/// rename the top folder to `<tag>/`, check that its `llama-server` runs,
/// then swap `current` and prune. Any failure leaves the previous install
/// as it was and no staging or half-installed tag behind. `path` is the
/// `PATH` for `tar` (it needs `gzip`) and for the version check.
pub async fn install(
    root: &Path,
    release: &Release,
    archive: &Path,
    path: &OsStr,
) -> Result<(), String> {
    if !super::names_are_safe(release) {
        return Err("the release names an unsafe file".to_owned());
    }
    let tar = find_tar(path)?;
    let _lock = lock(root)?;
    sweep_staging(root);
    let staging = root.join(format!("{STAGING_PREFIX}{}", std::process::id()));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&staging)
        .map_err(|e| format!("cannot create the staging folder: {e}"))?;
    let result = place(root, release, archive, path, &tar, &staging).await;
    let _ = fs::remove_dir_all(&staging);
    if result.is_ok() {
        let _ = fs::remove_file(archive);
    }
    result
}

async fn place(
    root: &Path,
    release: &Release,
    archive: &Path,
    path: &OsStr,
    tar: &Path,
    staging: &Path,
) -> Result<(), String> {
    let (copy, out) = (staging.join("archive.tar.gz"), staging.join("out"));
    let top = format!("llama-{}", release.tag);
    let job = (archive.to_owned(), release.clone(), tar.to_owned());
    let (c, o, t) = (copy.clone(), out.clone(), top.clone());
    tokio::task::spawn_blocking(move || {
        copy_verified(&job.0, &c, &job.1)?;
        fs::create_dir(&o).map_err(|e| e.to_string())?;
        extract(&job.2, &c, &o, &t)
    })
    .await
    .map_err(|_| "unpacking was interrupted".to_owned())??;

    let (tag_dir, aside) = (root.join(&release.tag), staging.join("replaced"));
    // The same tag installed again: the old folder waits to be put back.
    let replaced = fs::rename(&tag_dir, &aside).is_ok();
    let undo = |installed: bool| {
        if installed {
            let _ = fs::remove_dir_all(&tag_dir);
        }
        if replaced {
            let _ = fs::rename(&aside, &tag_dir);
        }
    };
    if let Err(e) = fs::rename(out.join(&top), &tag_dir) {
        undo(false);
        return Err(format!("cannot place the new install: {e}"));
    }
    if !runs(&tag_dir, path).await {
        undo(true);
        return Err("the new llama-server does not run on this machine".to_owned());
    }
    let previous = current_tag(root).filter(|tag| *tag != release.tag);
    if let Err(e) = swap_current(root, &release.tag) {
        undo(true);
        return Err(format!("cannot switch to the new install: {e}"));
    }
    prune(root, &release.tag, previous.as_deref());
    Ok(())
}

/// Removes the owned install, which is everything under the owned root
/// (older tags and a kept download included). Never a system binary: only
/// when `binary` is inside the owned root of `data_home`.
pub fn remove(binary: &Path, data_home: &Path) -> Result<(), String> {
    if !install::is_owned(binary, data_home) {
        return Err("this llama-server is not managed by lazarobox; it is not removed".to_owned());
    }
    let root = install::owned_root(data_home);
    if fs::symlink_metadata(&root).is_err() {
        return Ok(());
    }
    let _lock = lock(&root)?;
    let result = if fs::symlink_metadata(&root).is_ok_and(|m| m.file_type().is_symlink()) {
        fs::remove_file(&root)
    } else {
        fs::remove_dir_all(&root)
    };
    result.map_err(|e| format!("cannot remove {}: {e}", root.display()))
}

#[cfg(test)]
mod tests;
