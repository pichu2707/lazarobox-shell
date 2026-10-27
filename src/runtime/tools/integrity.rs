//! A before-and-after look at the places in a project root a toolchain run can
//! change and something outside the jail will later trust (user decision, 5B):
//! the git dir's `config`, `config.worktree` and `hooks/*`, the same for each
//! submodule under `.git/modules`, `.gitmodules`, `.git` itself when it is a
//! file or a link, and what the config points at inside the root:
//! `core.hooksPath`, `include.path` and the `gitdir:` of a `.git` file. The
//! root is `rwx`, so a program can write a hook that `git commit` runs later,
//! unjailed.
//!
//! [`snapshot`] reads those places and no link is followed in any folder of
//! the way (a `.git` that is a link to `~/.ssh` is recorded as a link and
//! never entered); [`compare`] says what differs. The text of a change is kept
//! up to [`TEXT_CAP`], so the caller can show a diff and put the old text
//! back; a bigger file is compared by its digest and carries no text. A hooks
//! folder is never cut short: every entry is read, and one with more than
//! [`HOOKS_ABSURD`] is reported as a [`Kind::Crowded`] change every time.
//! Nothing here writes.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::runtime::tools::open::open_beneath;

/// Largest file whose text is kept.
pub const TEXT_CAP: usize = 64 * 1024;
/// Most bytes of a file that go into its digest.
const HASH_CAP: u64 = 16 * 1024 * 1024;
/// More entries than this in one hooks folder is not a hooks folder anymore:
/// it is reported as [`Kind::Crowded`] and its entries are not read.
pub const HOOKS_ABSURD: usize = 1024;
/// How deep the submodule folders and the include chains are followed.
const DEPTH: usize = 6;

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
pub struct Snapshot {
    items: BTreeMap<PathBuf, Item>,
    /// Folders with more than [`HOOKS_ABSURD`] entries, and how many.
    crowded: BTreeMap<PathBuf, usize>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Added,
    Changed,
    Removed,
    /// A folder with too many entries to watch (`after` says how many). It is
    /// reported whatever changed, and the caller must treat the run as
    /// tainted: there is no text to show and nothing to put back.
    Crowded,
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
    let mut watch = Watch {
        root: root.to_path_buf(),
        real: fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf()),
        seen: BTreeSet::new(),
        snap: Snapshot::default(),
    };
    watch.take(Path::new(".gitmodules"));
    match fs::symlink_metadata(root.join(".git")) {
        Ok(meta) if meta.is_dir() => watch.git_dir(Path::new(".git"), 0),
        Ok(meta) => {
            // A file that points at the real git dir, or a link: recorded,
            // and a link is never entered.
            let item = watch.take(Path::new(".git"));
            if let (true, Some(Some(text))) = (meta.is_file(), item.map(|i| i.text)) {
                watch.follow_git_file(&text);
            }
        }
        Err(_) => {}
    }
    watch.snap
}

struct Watch {
    root: PathBuf,
    /// The root without any link in it: what a resolved path is judged by.
    real: PathBuf,
    /// Folders and files already read, so a loop reads each once.
    seen: BTreeSet<PathBuf>,
    snap: Snapshot,
}

impl Watch {
    /// Records the watched place `rel` (relative to the root) and returns it.
    fn take(&mut self, rel: &Path) -> Option<Item> {
        let item = read_item(&self.root, rel)?;
        self.snap.items.insert(rel.to_path_buf(), item.clone());
        Some(item)
    }

    /// Whether `rel` is a folder with no link anywhere along the way.
    fn plain_dir(&self, rel: &Path) -> bool {
        open_beneath(&self.root, rel, true).is_ok()
    }

    /// The sorted names in the plain folder `rel`.
    fn names(&self, rel: &Path) -> Vec<std::ffi::OsString> {
        if !self.plain_dir(rel) {
            return Vec::new();
        }
        let Ok(entries) = fs::read_dir(self.root.join(rel)) else {
            return Vec::new();
        };
        let mut names: Vec<_> = entries.flatten().map(|e| e.file_name()).collect();
        names.sort();
        names
    }

    /// The git dir `git` (relative to the root): its config files, its hooks
    /// and, one by one, the git dirs of its submodules.
    fn git_dir(&mut self, git: &Path, depth: usize) {
        if depth > DEPTH || !self.seen.insert(git.to_path_buf()) || !self.plain_dir(git) {
            return;
        }
        self.config(&git.join("config"), 0);
        self.config(&git.join("config.worktree"), 0);
        self.hooks(&git.join("hooks"));
        self.modules(&git.join("modules"), depth);
    }

    /// The submodule git dirs under `dir`: a folder with a `HEAD` is one,
    /// another is a path prefix of a module name (`modules/lib/foo`).
    fn modules(&mut self, dir: &Path, depth: usize) {
        if depth > DEPTH {
            return;
        }
        for name in self.names(dir) {
            let path = dir.join(name);
            if fs::symlink_metadata(self.root.join(path.join("HEAD"))).is_ok() {
                self.git_dir(&path, depth + 1);
            } else {
                self.modules(&path, depth + 1);
            }
        }
    }

    /// Every entry of the hooks folder `dir`, however many; past
    /// [`HOOKS_ABSURD`] it is only counted.
    fn hooks(&mut self, dir: &Path) {
        if !self.seen.insert(dir.to_path_buf()) {
            return;
        }
        let names = self.names(dir);
        if names.len() > HOOKS_ABSURD {
            self.snap.crowded.insert(dir.to_path_buf(), names.len());
            return;
        }
        for name in names {
            self.take(&dir.join(name));
        }
    }

    /// A config file, and what it points at inside the root: the hooks folder
    /// of `core.hooksPath` and the files of `include.path`.
    fn config(&mut self, rel: &Path, depth: usize) {
        if depth > DEPTH || !self.seen.insert(rel.to_path_buf()) {
            return;
        }
        let Some(Item {
            text: Some(text), ..
        }) = self.take(rel)
        else {
            return;
        };
        let (hooks, includes) = config_paths(&text);
        for value in hooks {
            // Relative to the working tree, which is the root.
            if let Some(dir) = self.inside(&self.root.clone(), &value) {
                self.hooks(&dir);
            }
        }
        let here = self.real.join(rel);
        for value in includes {
            let base = here.parent().unwrap_or(&self.real).to_path_buf();
            if let Some(file) = self.inside(&base, &value) {
                self.config(&file, depth + 1);
            }
        }
    }

    /// The `gitdir:` of a `.git` file, when it leads inside the root.
    fn follow_git_file(&mut self, text: &[u8]) {
        let text = String::from_utf8_lossy(text);
        let Some(value) = text
            .lines()
            .find_map(|line| line.trim().strip_prefix("gitdir:"))
        else {
            return;
        };
        if let Some(dir) = self.inside(&self.root.clone(), value.trim()) {
            self.git_dir(&dir, 0);
        }
    }

    /// `value` (a path, relative to `base`) as a path relative to the root,
    /// when it really lies inside it and is not the root itself.
    fn inside(&self, base: &Path, value: &str) -> Option<PathBuf> {
        let value = Path::new(value);
        let real = fs::canonicalize(base.join(value)).ok()?;
        let rel = real.strip_prefix(&self.real).ok()?;
        (!rel.as_os_str().is_empty()).then(|| rel.to_path_buf())
    }
}

/// The values of `core.hooksPath` and of `include.path` / `includeIf.*.path`
/// in the text of a git config. A small reader: sections, `key = value`,
/// quotes and `#` or `;` comments.
fn config_paths(text: &[u8]) -> (Vec<String>, Vec<String>) {
    let (mut hooks, mut includes) = (Vec::new(), Vec::new());
    let mut section = String::new();
    for line in String::from_utf8_lossy(text).lines() {
        let line = line.trim();
        if let Some(head) = line.strip_prefix('[') {
            let name: String = head
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != ']' && *c != '"')
                .collect();
            section = name.to_lowercase();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_lowercase();
        let value = value.trim();
        let value = match value.strip_prefix('"') {
            Some(quoted) => quoted.split('"').next().unwrap_or_default(),
            None => value.split(['#', ';']).next().unwrap_or_default().trim(),
        };
        if value.is_empty() {
            continue;
        }
        match (section.as_str(), key.as_str()) {
            ("core", "hookspath") => hooks.push(value.to_owned()),
            ("include" | "includeif", "path") => includes.push(value.to_owned()),
            _ => {}
        }
    }
    (hooks, includes)
}

/// A file or a link, not a folder (`.git` is one in a plain checkout). The
/// folders above it are opened without following a link, so none leads out.
fn read_item(root: &Path, rel: &Path) -> Option<Item> {
    open_beneath(root, rel.parent().unwrap_or(Path::new("")), true).ok()?;
    let path = root.join(rel);
    let meta = fs::symlink_metadata(&path).ok()?;
    let mode = meta.permissions().mode() & 0o7777;
    let kind = meta.file_type();
    if kind.is_symlink() {
        let target = fs::read_link(&path).ok()?;
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
    let mut file = fs::File::from(open_beneath(root, rel, false).ok()?).take(HASH_CAP);
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
    let mut paths: Vec<&PathBuf> = before.items.keys().chain(after.items.keys()).collect();
    paths.sort();
    paths.dedup();
    let mut found: Vec<Change> = paths
        .into_iter()
        .filter_map(|path| {
            let (old, new) = (before.items.get(path), after.items.get(path));
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
        .collect();
    // A crowded folder is told every time: its entries were not looked at.
    found.extend(after.crowded.iter().map(|(path, count)| Change {
        path: path.clone(),
        kind: Kind::Crowded,
        before: None,
        after: Some(format!("hooks dir has {count} entries").into_bytes()),
        mode: (None, None),
    }));
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found
}

#[cfg(test)]
mod tests;
