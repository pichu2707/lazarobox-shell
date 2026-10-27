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
//! never entered; a watched folder or file that is a link is recorded by its
//! target and, when the target is inside the root, by its content too);
//! [`compare`] says what differs. The text of a change is kept up to
//! [`TEXT_CAP`], so the caller can show a diff and put the old text back; a
//! bigger file is compared by its digest and carries no text.
//!
//! The check fails closed. Git's config grammar is not re-implemented: a
//! small reader takes the plain subset exactly, and anything outside it (a
//! continuation, an escape, an odd quote, `~`, a config over 1 MiB, an
//! include chain deeper than 10, a link that cannot be resolved, a hooks
//! folder with more than [`HOOKS_ABSURD`] entries, more places or bytes than
//! the bounds of one snapshot) is a [`Kind::Unchecked`] change every time.
//! Nothing here writes.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::runtime::tools::open::open_beneath;

/// Largest file whose text is kept.
pub const TEXT_CAP: usize = 64 * 1024;
/// Largest config file that is parsed (its text shown is still cut at
/// [`TEXT_CAP`]). A bigger one is reported as [`Kind::Unchecked`].
const CONFIG_CAP: usize = 1024 * 1024;
/// Most bytes of a file that go into its digest.
const HASH_CAP: u64 = 16 * 1024 * 1024;
/// More entries than this in one hooks folder is not a hooks folder anymore:
/// it is reported as [`Kind::Unchecked`] and its entries are not read.
pub const HOOKS_ABSURD: usize = 1024;
/// How deep the submodule folders and the include chains are followed (git's
/// own limit for includes). Deeper is reported as [`Kind::Unchecked`].
const MAX_DEPTH: usize = 10;
/// A folder is counted up to this many entries, then the count stops.
const COUNT_CAP: usize = 100_000;

/// What one snapshot may cost: past either bound the rest is not read and the
/// run is reported as [`Kind::Unchecked`].
#[derive(Clone, Copy, Debug)]
struct Limits {
    /// Watched places.
    items: usize,
    /// Bytes hashed.
    bytes: u64,
}

const LIMITS: Limits = Limits {
    items: 4096,
    bytes: 256 * 1024 * 1024,
};

/// The files git looks for in a hooks folder.
const HOOK_NAMES: &[&str] = &[
    "applypatch-msg",
    "pre-applypatch",
    "post-applypatch",
    "pre-commit",
    "pre-merge-commit",
    "prepare-commit-msg",
    "commit-msg",
    "post-commit",
    "pre-rebase",
    "post-checkout",
    "post-merge",
    "pre-push",
    "pre-receive",
    "update",
    "proc-receive",
    "post-receive",
    "post-update",
    "reference-transaction",
    "push-to-checkout",
    "pre-auto-gc",
    "post-rewrite",
    "sendemail-validate",
    "fsmonitor-watchman",
    "p4-changelist",
    "p4-prepare-changelist",
    "p4-post-changelist",
    "p4-pre-submit",
    "post-index-change",
];

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
    /// Places that could not be checked with certainty, and why.
    unchecked: BTreeMap<PathBuf, String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Added,
    Changed,
    Removed,
    /// Something could not be verified with certainty (a folder with too many
    /// entries, a config this reader does not understand, a bound that was
    /// hit); `after` holds the reason. It is reported whatever changed, and
    /// the caller must treat the run as tainted ("could not verify git
    /// hooks/config after this run"): there is no text to show and nothing to
    /// put back.
    Unchecked,
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
/// as absent; one that cannot be understood is recorded as unchecked.
pub fn snapshot(root: &Path) -> Snapshot {
    snapshot_with(root, LIMITS)
}

fn snapshot_with(root: &Path, limits: Limits) -> Snapshot {
    let mut watch = Watch {
        root: root.to_path_buf(),
        real: fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf()),
        seen: BTreeSet::new(),
        snap: Snapshot::default(),
        limits,
        bytes: 0,
        exhausted: false,
    };
    watch.take(Path::new(".gitmodules"));
    let git = Path::new(".git");
    match fs::symlink_metadata(root.join(git)) {
        Ok(meta) if meta.is_dir() => watch.git_dir(git, 0, false),
        Ok(meta) => {
            // A file that points at the real git dir, or a link: recorded.
            // A link is entered only when it leads to a folder inside the root.
            let taken = watch.take_cap(git, CONFIG_CAP);
            if meta.is_file() {
                if let Some(text) = taken.and_then(|t| t.content) {
                    watch.follow_git_file(&text);
                }
            } else if let Some(dir) = watch.link_dir(git) {
                watch.git_dir(&dir, 0, false);
            }
        }
        Err(_) => {}
    }
    watch.snap
}

/// What `take` found in a place.
struct Taken {
    /// The whole file, up to the parse cap (a link's target's content).
    content: Option<Vec<u8>>,
    /// The file is over the parse cap.
    oversize: bool,
    /// The place is a link that leads outside the root.
    link_outside: bool,
    /// The place is a link whose target is inside the root.
    link_target: Option<PathBuf>,
}

/// Where a path lies.
enum Where {
    /// Inside the root, relative to it.
    Inside(PathBuf),
    /// The root itself.
    Root,
    /// Outside the root.
    Outside,
    /// Could not be told with certainty.
    Unknown,
}

/// What a relative `core.hooksPath` is relative to.
enum Base {
    At(PathBuf),
    Outside,
    Unknown,
}

/// What a watched folder slot turned out to be.
enum Slot {
    Dir,
    /// A link whose target is a folder inside the root.
    LinkDir(PathBuf),
    /// Anything else that exists: recorded as an item.
    Other,
    Absent,
}

struct Watch {
    root: PathBuf,
    /// The root without any link in it: what a resolved path is judged by.
    real: PathBuf,
    /// Folders and files already read, so a loop reads each once.
    seen: BTreeSet<PathBuf>,
    snap: Snapshot,
    limits: Limits,
    bytes: u64,
    exhausted: bool,
}

impl Watch {
    fn unchecked(&mut self, path: &Path, why: impl Into<String>) {
        self.snap
            .unchecked
            .entry(path.to_path_buf())
            .or_insert_with(|| why.into());
    }

    /// Records the watched place `rel` (relative to the root).
    fn take(&mut self, rel: &Path) -> Option<Taken> {
        self.take_cap(rel, 0)
    }

    /// [`Watch::take`] that also returns the whole content of a file up to
    /// `parse_cap` bytes. A link is recorded by its target and, when the
    /// target is a file inside the root, the target is recorded too and its
    /// content is what comes back.
    fn take_cap(&mut self, rel: &Path, parse_cap: usize) -> Option<Taken> {
        if self.exhausted {
            return None;
        }
        if self.snap.items.len() >= self.limits.items {
            self.exhausted = true;
            self.unchecked(rel, "more watched places than the bound allows");
            return None;
        }
        let read = read_item(&self.root, rel, parse_cap)?;
        self.bytes += read.hashed;
        if self.bytes > self.limits.bytes {
            self.exhausted = true;
            self.unchecked(rel, "more bytes to hash than the bound allows");
            return None;
        }
        self.snap.items.insert(rel.to_path_buf(), read.item);
        if !read.link {
            return Some(Taken {
                content: read.content,
                oversize: read.oversize,
                link_outside: false,
                link_target: None,
            });
        }
        match fs::canonicalize(self.root.join(rel)) {
            Ok(real) => match real.strip_prefix(&self.real) {
                Ok(target) if !target.as_os_str().is_empty() => {
                    let target = target.to_path_buf();
                    let inner = self.take_cap(&target, parse_cap);
                    Some(Taken {
                        content: inner.as_ref().and_then(|t| t.content.clone()),
                        oversize: inner.as_ref().is_some_and(|t| t.oversize),
                        link_outside: false,
                        link_target: Some(target),
                    })
                }
                _ => Some(Taken {
                    content: None,
                    oversize: false,
                    link_outside: true,
                    link_target: None,
                }),
            },
            // A dangling link: nothing to read.
            Err(_) => Some(Taken {
                content: None,
                oversize: false,
                link_outside: false,
                link_target: None,
            }),
        }
    }

    /// The target of the link `rel` when it is a folder inside the root.
    fn link_dir(&self, rel: &Path) -> Option<PathBuf> {
        let real = fs::canonicalize(self.root.join(rel)).ok()?;
        let target = real.strip_prefix(&self.real).ok()?;
        let meta = fs::symlink_metadata(&real).ok()?;
        (meta.is_dir() && !target.as_os_str().is_empty()).then(|| target.to_path_buf())
    }

    /// Whether `rel` is a folder with no link anywhere along the way.
    fn plain_dir(&self, rel: &Path) -> bool {
        open_beneath(&self.root, rel, true).is_ok()
    }

    /// What the watched folder `rel` is. A thing that is there but is not a
    /// plain folder (a link, a file) is recorded, so replacing a folder by
    /// one shows as a change.
    fn slot(&mut self, rel: &Path) -> Slot {
        let parent = rel.parent().unwrap_or(Path::new(""));
        if !self.plain_dir(parent) {
            return Slot::Absent;
        }
        match fs::symlink_metadata(self.root.join(rel)) {
            Err(_) => Slot::Absent,
            Ok(meta) if meta.is_dir() => Slot::Dir,
            Ok(meta) => {
                self.take(rel);
                match self.link_dir(rel) {
                    Some(dir) if meta.file_type().is_symlink() => Slot::LinkDir(dir),
                    _ => Slot::Other,
                }
            }
        }
    }

    /// Up to `limit + 1` sorted names in the plain folder `rel`, how many
    /// entries it has (counted up to [`COUNT_CAP`]) and whether the count was
    /// cut there. Entries past the limit are counted, not collected.
    fn names(&self, rel: &Path, limit: usize) -> (Vec<OsString>, usize, bool) {
        if !self.plain_dir(rel) {
            return (Vec::new(), 0, false);
        }
        let Ok(entries) = fs::read_dir(self.root.join(rel)) else {
            return (Vec::new(), 0, false);
        };
        let (mut names, mut count, mut capped) = (Vec::new(), 0, false);
        for entry in entries.flatten() {
            if count >= COUNT_CAP {
                capped = true;
                break;
            }
            count += 1;
            if names.len() <= limit {
                names.push(entry.file_name());
            }
        }
        names.sort();
        (names, count, capped)
    }

    /// The git dir `git` (relative to the root): its config files, its hooks
    /// and, one by one, the git dirs of its submodules. `module` says it is
    /// one under `.git/modules`.
    fn git_dir(&mut self, git: &Path, depth: usize, module: bool) {
        if depth > MAX_DEPTH {
            self.unchecked(git, "git dirs nested deeper than the bound allows");
            return;
        }
        if self.exhausted || !self.seen.insert(git.to_path_buf()) || !self.plain_dir(git) {
            return;
        }
        let base = self.base_of(git, module);
        self.config(&git.join("config"), 0, &base);
        self.config(&git.join("config.worktree"), 0, &base);
        self.hooks(&git.join("hooks"));
        self.modules(&git.join("modules"), depth);
    }

    /// What a relative `core.hooksPath` in the git dir `git` is relative to:
    /// its `core.worktree` when set; else the root for the project's own git
    /// dir, and unknown for a submodule's.
    fn base_of(&self, git: &Path, module: bool) -> Base {
        let here = self.real.join(git);
        let worktree = self
            .content_of(&git.join("config"))
            .and_then(|text| config_paths(&text).ok())
            .and_then(|parsed| parsed.worktree.last().cloned());
        match worktree {
            Some(value) => match self.locate(&here, &value) {
                Where::Inside(rel) => Base::At(self.real.join(rel)),
                Where::Root => Base::At(self.real.clone()),
                Where::Outside => Base::Outside,
                Where::Unknown => Base::Unknown,
            },
            None if module => Base::Unknown,
            None => Base::At(self.real.clone()),
        }
    }

    /// The content of a config file (through a link that stays inside the
    /// root), when it can be read and is not over the parse cap. Records
    /// nothing.
    fn content_of(&self, rel: &Path) -> Option<Vec<u8>> {
        let read = read_item(&self.root, rel, CONFIG_CAP)?;
        if read.link {
            let real = fs::canonicalize(self.root.join(rel)).ok()?;
            let target = real.strip_prefix(&self.real).ok()?;
            return read_item(&self.root, target, CONFIG_CAP)?.content;
        }
        read.content
    }

    /// The submodule git dirs under `dir`: a folder with a `HEAD` is one,
    /// another is a path prefix of a module name (`modules/lib/foo`).
    fn modules(&mut self, dir: &Path, depth: usize) {
        if depth > MAX_DEPTH {
            self.unchecked(dir, "submodule folders deeper than the bound allows");
            return;
        }
        match self.slot(dir) {
            Slot::Dir => {}
            Slot::LinkDir(target) => return self.modules(&target, depth + 1),
            Slot::Other | Slot::Absent => return,
        }
        let (names, count, _) = self.names(dir, self.limits.items);
        if count > self.limits.items {
            self.unchecked(dir, format!("submodule folder has {count} entries"));
            return;
        }
        for name in names {
            self.module_entry(&dir.join(name), depth);
        }
    }

    fn module_entry(&mut self, path: &Path, depth: usize) {
        match self.slot(path) {
            Slot::Dir => {
                if fs::symlink_metadata(self.root.join(path.join("HEAD"))).is_ok() {
                    self.git_dir(path, depth + 1, true);
                } else {
                    self.modules(path, depth + 1);
                }
            }
            Slot::LinkDir(target) => {
                if depth + 1 > MAX_DEPTH {
                    self.unchecked(path, "submodule folders deeper than the bound allows");
                } else {
                    self.module_entry(&target, depth + 1);
                }
            }
            Slot::Other | Slot::Absent => {}
        }
    }

    /// Every entry of the hooks folder `dir`, however many; past
    /// [`HOOKS_ABSURD`] it is only counted and reported.
    fn hooks(&mut self, dir: &Path) {
        if self.exhausted || !self.seen.insert(dir.to_path_buf()) {
            return;
        }
        match self.slot(dir) {
            Slot::Dir => {}
            Slot::LinkDir(target) => return self.hooks(&target),
            Slot::Other | Slot::Absent => return,
        }
        let (names, count, capped) = self.names(dir, HOOKS_ABSURD);
        if count > HOOKS_ABSURD {
            let why = if capped {
                format!("hooks dir has more than {COUNT_CAP} entries")
            } else {
                format!("hooks dir has {count} entries")
            };
            self.unchecked(dir, why);
            return;
        }
        for name in names {
            self.take(&dir.join(name));
        }
    }

    /// `core.hooksPath = .`: the hooks are files of the root itself.
    fn hooks_in_root(&mut self) {
        for name in HOOK_NAMES {
            self.take(Path::new(name));
        }
    }

    /// A config file, and what it points at inside the root: the hooks folder
    /// of `core.hooksPath` and the files of `include.path`.
    fn config(&mut self, rel: &Path, depth: usize, base: &Base) {
        if depth > MAX_DEPTH {
            self.unchecked(rel, "include chain deeper than the bound allows");
            return;
        }
        if self.exhausted || !self.seen.insert(rel.to_path_buf()) {
            return;
        }
        let Some(taken) = self.take_cap(rel, CONFIG_CAP) else {
            return;
        };
        if taken.oversize {
            self.unchecked(rel, "config is over the size this reader parses");
            return;
        }
        if taken.link_outside {
            self.unchecked(rel, "config is a link that leads outside the root");
            return;
        }
        let Some(content) = taken.content else {
            return;
        };
        let parsed = match config_paths(&content) {
            Ok(parsed) => parsed,
            Err(why) => {
                self.unchecked(rel, format!("config not understood: {why}"));
                return;
            }
        };
        for value in &parsed.hooks {
            self.hooks_path(rel, value, base);
        }
        let here = self.real.join(rel);
        let mut bases = vec![here.parent().unwrap_or(&self.real).to_path_buf()];
        if let Some(target) = &taken.link_target {
            let there = self.real.join(target);
            bases.extend(there.parent().map(Path::to_path_buf));
        }
        for value in &parsed.includes {
            for dir in &bases {
                match self.locate(dir, value) {
                    Where::Inside(file) => self.config(&file, depth + 1, base),
                    Where::Root | Where::Outside => {}
                    Where::Unknown => self.unchecked(rel, "an include path cannot be resolved"),
                }
            }
        }
    }

    /// One `core.hooksPath` value found in the config `from`.
    fn hooks_path(&mut self, from: &Path, value: &str, base: &Base) {
        let located = if Path::new(value).is_absolute() {
            self.locate(&self.real.clone(), value)
        } else {
            match base {
                Base::At(dir) => self.locate(&dir.clone(), value),
                Base::Outside => Where::Outside,
                Base::Unknown => Where::Unknown,
            }
        };
        match located {
            Where::Inside(dir) => self.hooks(&dir),
            Where::Root => self.hooks_in_root(),
            Where::Outside => {}
            Where::Unknown => self.unchecked(from, "core.hooksPath cannot be resolved"),
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
        match self.locate(&self.real.clone(), value.trim()) {
            Where::Inside(dir) => self.git_dir(&dir, 0, false),
            Where::Root | Where::Outside => {}
            Where::Unknown => self.unchecked(Path::new(".git"), "gitdir cannot be resolved"),
        }
    }

    /// Where `value` (a path, relative to the absolute `base`) lies. A path
    /// that exists is judged by where it really leads; one that does not is
    /// walked without touching a link: a link met on the way makes it unknown.
    fn locate(&self, base: &Path, value: &str) -> Where {
        let joined = base.join(value);
        let resolved = match fs::canonicalize(&joined) {
            Ok(real) => real,
            Err(_) => {
                let mut cur = PathBuf::new();
                for part in joined.components() {
                    match part {
                        Component::ParentDir => {
                            cur.pop();
                        }
                        Component::CurDir => {}
                        other => {
                            cur.push(other.as_os_str());
                            let link = fs::symlink_metadata(&cur)
                                .is_ok_and(|meta| meta.file_type().is_symlink());
                            if link {
                                return Where::Unknown;
                            }
                        }
                    }
                }
                cur
            }
        };
        match resolved
            .strip_prefix(&self.real)
            .or_else(|_| resolved.strip_prefix(&self.root))
        {
            Ok(rel) if rel.as_os_str().is_empty() => Where::Root,
            Ok(rel) => Where::Inside(rel.to_path_buf()),
            Err(_) => Where::Outside,
        }
    }
}

/// What a config says about places that can hold something to run.
#[derive(Default, Debug)]
struct Parsed {
    /// `core.hooksPath`.
    hooks: Vec<String>,
    /// `include.path` and `includeIf.*.path`.
    includes: Vec<String>,
    /// `core.worktree`.
    worktree: Vec<String>,
}

/// The values git config files hold that lead somewhere, read from `text`.
/// A small reader of a strict subset: `[section]`, `[section "sub"]` and
/// `[section.sub]` headers (a `key = value` may follow on the same line),
/// `key = value` lines and `#` or `;` comments. It is exact for that subset
/// and fails closed outside it: a line continuation, a stray `\r` or NUL, a
/// header or value it does not understand, a quote or a backslash in the value
/// of a key it reads, `~` or `%(prefix)` in a path, an empty path. The caller
/// treats an `Err` as "could not verify". Section and key names are case
/// insensitive.
fn config_paths(text: &[u8]) -> Result<Parsed, String> {
    let text = std::str::from_utf8(text).map_err(|_| "not UTF-8".to_owned())?;
    let mut out = Parsed::default();
    // The section's lowercase name and whether it has a subsection.
    let mut section = (String::new(), false);
    for (number, raw) in text.lines().enumerate() {
        let at = number + 1;
        if raw.contains(['\r', '\0']) {
            return Err(format!("line {at}: stray CR or NUL"));
        }
        let line = raw.trim();
        if line.ends_with('\\') {
            return Err(format!("line {at}: line continuation"));
        }
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        let entry = match line.strip_prefix('[') {
            Some(rest) => {
                let (name, sub, after) = header(rest).ok_or(format!("line {at}: header"))?;
                section = (name, sub);
                let after = after.trim();
                if after.is_empty() || after.starts_with(['#', ';']) {
                    continue;
                }
                after
            }
            None => line,
        };
        entry_into(entry, &section, &mut out).map_err(|why| format!("line {at}: {why}"))?;
    }
    Ok(out)
}

/// A header after its `[`: the lowercase name, whether it has a subsection
/// and the rest of the line.
fn header(rest: &str) -> Option<(String, bool, &str)> {
    let name_end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '.'))
        .unwrap_or(rest.len());
    let name = &rest[..name_end];
    if name.is_empty() {
        return None;
    }
    let tail = &rest[name_end..];
    if let Some(after) = tail.strip_prefix(']') {
        return Some((name.to_lowercase(), name.contains('.'), after));
    }
    let quoted = tail.trim_start_matches([' ', '\t']);
    if quoted.len() == tail.len() {
        return None;
    }
    let inner = quoted.strip_prefix('"')?;
    let close = inner.find('"')?;
    if inner[..close].contains('\\') {
        return None;
    }
    let after = inner[close + 1..].strip_prefix(']')?;
    Some((name.to_lowercase(), true, after))
}

/// One `key = value` (or a bare `key`) of `section`, into `out`.
fn entry_into(entry: &str, section: &(String, bool), out: &mut Parsed) -> Result<(), String> {
    let key_end = entry
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .unwrap_or(entry.len());
    let key = entry[..key_end].to_lowercase();
    if !key.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return Err("not a key".to_owned());
    }
    let rest = entry[key_end..].trim_start();
    let target = match (section.0.as_str(), section.1, key.as_str()) {
        ("core", false, "hookspath") => Some(&mut out.hooks),
        ("core", false, "worktree") => Some(&mut out.worktree),
        ("include", false, "path") | ("includeif", true, "path") => Some(&mut out.includes),
        _ => None,
    };
    if !(rest.is_empty() || rest.starts_with('=')) {
        return Err("not a key".to_owned());
    }
    // Another key: only the line boundary mattered.
    let Some(list) = target else { return Ok(()) };
    let Some(raw) = rest.strip_prefix('=') else {
        return Err(format!("{key} has no value"));
    };
    let value = path_value(raw.trim_start())?;
    list.push(value);
    Ok(())
}

/// A path value: bare (up to a `#` or `;` comment) or wholly in one pair of
/// quotes; nothing else.
fn path_value(raw: &str) -> Result<String, String> {
    let value = if let Some(inner) = raw.strip_prefix('"') {
        let close = inner.find('"').ok_or("unbalanced quote")?;
        let after = inner[close + 1..].trim();
        if !(after.is_empty() || after.starts_with(['#', ';'])) {
            return Err("text after the quotes".to_owned());
        }
        if inner[..close].contains('\\') {
            return Err("backslash in a value".to_owned());
        }
        inner[..close].to_owned()
    } else {
        let bare = raw.split(['#', ';']).next().unwrap_or_default().trim_end();
        if bare.contains(['"', '\\']) {
            return Err("quote or backslash in a value".to_owned());
        }
        bare.to_owned()
    };
    if value.is_empty() {
        return Err("empty path".to_owned());
    }
    if value.starts_with('~') || value.starts_with("%(prefix)") {
        return Err("path with ~ or %(prefix)".to_owned());
    }
    Ok(value)
}

/// What [`read_item`] found.
struct Reading {
    item: Item,
    /// The whole file when it is within the parse cap (never for a link).
    content: Option<Vec<u8>>,
    /// The file is over the parse cap (when there is one).
    oversize: bool,
    link: bool,
    /// Bytes that went into the digest.
    hashed: u64,
}

/// A file or a link, not a folder (`.git` is one in a plain checkout). The
/// folders above it are opened without following a link, so none leads out.
/// `parse_cap` is how much of a file is kept whole, besides the [`TEXT_CAP`]
/// of its text.
fn read_item(root: &Path, rel: &Path, parse_cap: usize) -> Option<Reading> {
    open_beneath(root, rel.parent().unwrap_or(Path::new("")), true).ok()?;
    let path = root.join(rel);
    let meta = fs::symlink_metadata(&path).ok()?;
    let mode = meta.permissions().mode() & 0o7777;
    let kind = meta.file_type();
    if kind.is_symlink() {
        let target = fs::read_link(&path).ok()?;
        let bytes = target.as_os_str().as_bytes().to_vec();
        return Some(Reading {
            hashed: bytes.len() as u64,
            item: Item {
                mode,
                digest: Sha256::digest(&bytes).into(),
                text: Some(bytes),
            },
            content: None,
            oversize: false,
            link: true,
        });
    }
    if !kind.is_file() {
        return None;
    }
    let mut hasher = Sha256::new();
    hasher.update(meta.len().to_le_bytes());
    let keep = TEXT_CAP.max(parse_cap);
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
        if size <= keep {
            text.extend_from_slice(&buf[..n]);
        }
    }
    let shown = (size <= TEXT_CAP).then(|| text[..size].to_vec());
    Some(Reading {
        item: Item {
            mode,
            digest: hasher.finalize().into(),
            text: shown,
        },
        content: (parse_cap > 0 && size <= parse_cap).then_some(text),
        oversize: parse_cap > 0 && (size > parse_cap || meta.len() > parse_cap as u64),
        link: false,
        hashed: size as u64,
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
    // What could not be checked is told every time, in either snapshot: what
    // was not looked at may have changed.
    let mut why: BTreeMap<&PathBuf, &String> = before.unchecked.iter().collect();
    why.extend(after.unchecked.iter());
    found.extend(why.into_iter().map(|(path, why)| Change {
        path: path.clone(),
        kind: Kind::Unchecked,
        before: None,
        after: Some(why.clone().into_bytes()),
        mode: (None, None),
    }));
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found
}

#[cfg(test)]
mod tests;
