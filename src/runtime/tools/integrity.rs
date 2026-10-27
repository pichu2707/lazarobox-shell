//! A before-and-after look at the places in a project root a toolchain run can
//! change and something outside the jail will later trust (user decision, 5B):
//! `.git/hooks/*`, `.git/config`, `.gitmodules`, and `.git` itself when it is a
//! file that points at the real git dir. The root is `rwx`, so a program can
//! write a hook that `git commit` runs later, unjailed.
//!
//! [`snapshot`] reads those places (never through a link); [`compare`] says
//! what differs. The text of a change is kept up to [`TEXT_CAP`], so the caller
//! can show a diff and put the old text back; a bigger file is compared by its
//! digest and carries no text. Nothing here writes.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Largest file whose text is kept.
pub const TEXT_CAP: usize = 64 * 1024;
/// Most bytes of a file that go into its digest.
const HASH_CAP: u64 = 16 * 1024 * 1024;
/// Most files taken from the hooks folder.
const HOOKS_MAX: usize = 64;

/// What one watched place holds.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Item {
    mode: u32,
    digest: [u8; 32],
    /// The text, or the target of a link; `None` over [`TEXT_CAP`].
    text: Option<Vec<u8>>,
}

/// The watched places of one root, by path relative to it.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Snapshot(BTreeMap<PathBuf, Item>);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Added,
    Changed,
    Removed,
}

/// One watched place that differs.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Change {
    /// Relative to the root.
    pub path: PathBuf,
    pub kind: Kind,
    /// The text before and after; `None` when absent, a link's target is its
    /// text, and `None` too for a file over [`TEXT_CAP`].
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
    /// The permission bits before and after.
    pub mode: (Option<u32>, Option<u32>),
}

/// Reads the watched places under `root`. A place that cannot be read counts
/// as absent.
pub fn snapshot(root: &Path) -> Snapshot {
    let mut items = BTreeMap::new();
    let mut take = |relative: PathBuf| {
        if let Some(item) = read_item(&root.join(&relative)) {
            items.insert(relative, item);
        }
    };
    take(PathBuf::from(".git"));
    take(PathBuf::from(".git/config"));
    take(PathBuf::from(".gitmodules"));
    if let Ok(entries) = fs::read_dir(root.join(".git/hooks")) {
        let mut names: Vec<_> = entries.flatten().map(|e| e.file_name()).collect();
        names.sort();
        for name in names.into_iter().take(HOOKS_MAX) {
            take(Path::new(".git/hooks").join(name));
        }
    }
    Snapshot(items)
}

/// A file or a link, not a folder (`.git` is one in a plain checkout).
fn read_item(path: &Path) -> Option<Item> {
    let meta = fs::symlink_metadata(path).ok()?;
    let mode = meta.permissions().mode() & 0o7777;
    let kind = meta.file_type();
    if kind.is_symlink() {
        let target = fs::read_link(path).ok()?;
        let bytes = target.as_os_str().as_bytes().to_vec();
        return Some(Item {
            mode,
            digest: Sha256::digest(&bytes).into(),
            text: Some(bytes),
        });
    }
    if !kind.is_file() {
        return None;
    }
    let mut hasher = Sha256::new();
    hasher.update(meta.len().to_le_bytes());
    let mut text = Vec::new();
    // A file bigger than this is told by its size and its first part.
    let mut file = fs::File::open(path).ok()?.take(HASH_CAP);
    let mut buf = [0u8; 8192];
    let mut size = 0usize;
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n;
        if size <= TEXT_CAP {
            text.extend_from_slice(&buf[..n]);
        }
    }
    Some(Item {
        mode,
        digest: hasher.finalize().into(),
        text: (size <= TEXT_CAP).then_some(text),
    })
}

/// What differs between two snapshots of the same root, by path.
pub fn compare(before: &Snapshot, after: &Snapshot) -> Vec<Change> {
    let mut paths: Vec<&PathBuf> = before.0.keys().chain(after.0.keys()).collect();
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .filter_map(|path| {
            let (old, new) = (before.0.get(path), after.0.get(path));
            let kind = match (old, new) {
                (None, Some(_)) => Kind::Added,
                (Some(_), None) => Kind::Removed,
                (Some(a), Some(b)) if a != b => Kind::Changed,
                _ => return None,
            };
            Some(Change {
                path: path.clone(),
                kind,
                before: old.and_then(|i| i.text.clone()),
                after: new.and_then(|i| i.text.clone()),
                mode: (old.map(|i| i.mode), new.map(|i| i.mode)),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
