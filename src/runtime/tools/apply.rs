//! Applying a reviewed batch to the disk, or not at all.
//!
//! The user approved a snapshot, so nothing is written until every target is
//! read again and found as staged (a create still absent, an overwrite still
//! holding its `base`). Any difference refuses the whole batch. Everything
//! that touches the disk goes through [`WriteOps`], so a test can inject a
//! fault at any step; the real one ([`Disk`]) only calls `write.rs`, the one
//! file allowed to change the tree.
//!
//! What comes back toward the model carries paths and counts, never text: the
//! diff rows of a reviewed file may hold a secret the model never saw.

use super::write::{self, WriteError};
use crate::core::ai::tools::{
    diff::{diff, escape},
    paths::SecretRoots,
    stage::{Entry, Staged},
};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

/// A file bigger than this cannot be what was staged (a base is at most 1 MiB),
/// so the preflight reads no more of it.
const PROBE_CAP: u64 = 2 << 20;

/// Every way the apply touches the disk.
pub trait WriteOps {
    /// The bytes of the file `real`, `None` when the name is free. `Err` when
    /// it is a link, a hard link, a folder, a denied path...
    fn probe(&self, real: &Path) -> Result<Option<Vec<u8>>, WriteError>;
    fn make_dir(&self, real: &Path) -> Result<(), WriteError>;
    fn create_file(&self, real: &Path, bytes: &[u8]) -> Result<(), WriteError>;
    /// Replaces `real` only while it still holds `expected`.
    fn replace_file(&self, real: &Path, expected: &[u8], bytes: &[u8]) -> Result<(), WriteError>;
    /// Removes `real` only while it still holds `expected` (undo of a create).
    fn delete_file(&self, real: &Path, expected: &[u8]) -> Result<(), WriteError>;
    /// Removes the empty folder `real` (undo of a folder).
    fn delete_dir(&self, real: &Path) -> Result<(), WriteError>;
}

/// The real disk, through the checked primitives of `write.rs`.
pub struct Disk<'a> {
    roots: &'a SecretRoots,
}

impl<'a> Disk<'a> {
    pub fn new(roots: &'a SecretRoots) -> Disk<'a> {
        Disk { roots }
    }
}

impl WriteOps for Disk<'_> {
    fn probe(&self, real: &Path) -> Result<Option<Vec<u8>>, WriteError> {
        let Some(file) = write::open_target(self.roots, real)? else {
            return Ok(None);
        };
        let mut bytes = Vec::new();
        file.take(PROBE_CAP)
            .read_to_end(&mut bytes)
            .map_err(|_| WriteError::Io)?;
        Ok(Some(bytes))
    }

    fn make_dir(&self, real: &Path) -> Result<(), WriteError> {
        write::make_dir(self.roots, real)
    }

    fn create_file(&self, real: &Path, bytes: &[u8]) -> Result<(), WriteError> {
        write::create_file(self.roots, real, bytes)
    }

    fn replace_file(&self, real: &Path, expected: &[u8], bytes: &[u8]) -> Result<(), WriteError> {
        write::replace_file(self.roots, real, expected, bytes)
    }

    fn delete_file(&self, real: &Path, expected: &[u8]) -> Result<(), WriteError> {
        write::delete_file(self.roots, real, expected)
    }

    fn delete_dir(&self, real: &Path) -> Result<(), WriteError> {
        write::delete_dir(self.roots, real)
    }
}

/// What one applied path became.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeKind {
    Created,
    Edited,
    DirCreated,
}

/// One path of an applied batch.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Change {
    pub path: PathBuf,
    pub kind: ChangeKind,
    pub bytes: u64,
    pub added: u32,
    pub removed: u32,
}

/// How an apply ended. No variant holds file text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ApplyResult {
    Applied(Vec<Change>),
    /// Some target is no longer as staged; nothing was written.
    Drifted(Vec<PathBuf>),
    /// The batch contradicts itself at `path` (the same path twice, a file
    /// that is also a folder of another entry); nothing was written.
    Invalid {
        path: PathBuf,
    },
    /// A step failed at `path`; whatever was done is undone. `undone` tells
    /// whether any earlier step had to be, so `false` means nothing was written.
    Failed {
        path: PathBuf,
        undone: bool,
    },
    /// A step failed and an undo failed too: `changed` is exactly what is left
    /// changed on disk, in the order the undo met it. It holds the paths this
    /// batch changed and could not put back, and the paths someone else
    /// replaced, which are left as they are.
    Partial {
        changed: Vec<PathBuf>,
    },
    /// No safe write on this platform; nothing was written.
    Unsupported,
}

impl ApplyResult {
    /// Whether the whole batch is on disk.
    pub fn applied(&self) -> bool {
        matches!(self, ApplyResult::Applied(_))
    }

    /// The text for the model: paths and counts, never file text or diff rows.
    pub fn summary(&self) -> String {
        let list = |paths: &[PathBuf]| -> String {
            paths
                .iter()
                .map(|p| format!("\n{}", escape(&p.to_string_lossy())))
                .collect()
        };
        match self {
            ApplyResult::Applied(changes) => {
                let mut out = format!("applied {} paths:", changes.len());
                for c in changes {
                    let kind = match c.kind {
                        ChangeKind::Created => "created",
                        ChangeKind::Edited => "edited",
                        ChangeKind::DirCreated => "folder",
                    };
                    let path = escape(&c.path.to_string_lossy()).into_owned();
                    out += &format!(
                        "\n{kind} {path} (+{} -{}, {} bytes)",
                        c.added, c.removed, c.bytes
                    );
                }
                out
            }
            ApplyResult::Drifted(paths) => format!(
                "refused: {} paths changed since the review, nothing was written:{}",
                paths.len(),
                list(paths)
            ),
            ApplyResult::Invalid { path } => format!(
                "refused: the batch is inconsistent at {}, nothing was written",
                escape(&path.to_string_lossy())
            ),
            ApplyResult::Failed { path, undone } => format!(
                "failed at {}: {}",
                escape(&path.to_string_lossy()),
                if *undone {
                    "every earlier change was undone"
                } else {
                    "nothing was written"
                }
            ),
            ApplyResult::Partial { changed } => format!(
                "partial: {} path{} still changed on disk (by this batch, or by someone else and left as it is), check it by hand:{}",
                changed.len(),
                if changed.len() == 1 { " is" } else { "s are" },
                list(changed)
            ),
            ApplyResult::Unsupported => {
                "writes are not supported on this platform, nothing was written".into()
            }
        }
    }

    /// What the model hears of an apply the user made that closed the batch.
    /// Fixed templates and counts only: [`ApplyResult::summary`] names paths,
    /// and the model must not learn the path of anything it did not write.
    pub fn told(&self) -> String {
        let s = |n: usize| if n == 1 { "" } else { "s" };
        match self {
            ApplyResult::Applied(changes) => format!(
                "The user applied the batch you left staged: applied {} paths.",
                changes.len()
            ),
            ApplyResult::Partial { changed } => format!(
                "The user's apply of your staged batch failed part way; {} path{} still changed on disk.",
                changed.len(),
                s(changed.len())
            ),
            ApplyResult::Drifted(paths) => format!(
                "The user's apply of your staged batch was refused; {} path{} changed since the review and nothing was written.",
                paths.len(),
                s(paths.len())
            ),
            ApplyResult::Failed { undone, .. } => format!(
                "The user's apply of your staged batch failed; {}.",
                if *undone {
                    "every earlier change was undone"
                } else {
                    "nothing was written"
                }
            ),
            ApplyResult::Invalid { .. } | ApplyResult::Unsupported => {
                "The user's apply of your staged batch was refused; nothing was written.".to_owned()
            }
        }
    }
}

/// Applies the entries in order (folders by depth, then files), after the
/// preflight of every target. The entries done so far are the journal: when a
/// step fails, they are undone in reverse. A step that finds other bytes than
/// the reviewed ones (a race after the preflight) reports drift.
pub fn apply(ops: &dyn WriteOps, entries: &[Staged]) -> ApplyResult {
    if let Some(refused) = preflight(ops, entries) {
        return refused;
    }
    let mut done: Vec<&Staged> = Vec::new();
    for staged in ordered(entries) {
        if let Err(error) = run(ops, staged) {
            let changed = rollback(ops, &done);
            let path = staged.target.real.clone();
            return match error {
                _ if !changed.is_empty() => ApplyResult::Partial { changed },
                _ if raced(&error, &staged.entry) => ApplyResult::Drifted(vec![path]),
                _ => ApplyResult::Failed {
                    path,
                    undone: !done.is_empty(),
                },
            };
        }
        done.push(staged);
    }
    ApplyResult::Applied(done.into_iter().map(change).collect())
}

/// Whether a forward step failed because the disk is no longer as reviewed,
/// the cases the preflight calls drift: other bytes, a taken name, a vanished
/// file to edit, a target that turned into a link.
fn raced(error: &WriteError, entry: &Entry) -> bool {
    match error {
        WriteError::Drifted | WriteError::Exists | WriteError::Refused => true,
        WriteError::Missing => matches!(entry, Entry::File { base: Some(_), .. }),
        WriteError::Unsupported | WriteError::NotEmpty | WriteError::Io => false,
    }
}

/// Undoes the journal in reverse and goes on past a failure, so as much as
/// possible is put back. Returns the paths whose undo failed: exactly what is
/// left changed.
fn rollback(ops: &dyn WriteOps, done: &[&Staged]) -> Vec<PathBuf> {
    done.iter()
        .rev()
        .filter(|staged| undo(ops, staged).is_err())
        .map(|staged| staged.target.real.clone())
        .collect()
}

/// The record of one finished step.
fn change(staged: &Staged) -> Change {
    let path = staged.target.real.clone();
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    match &staged.entry {
        Entry::Dir => Change {
            path,
            kind: ChangeKind::DirCreated,
            bytes: 0,
            added: 0,
            removed: 0,
        },
        Entry::File { text, base } => {
            let rows = diff(base.as_deref(), text);
            Change {
                path,
                kind: if base.is_some() {
                    ChangeKind::Edited
                } else {
                    ChangeKind::Created
                },
                bytes: text.len() as u64,
                added: count(rows.added),
                removed: count(rows.removed),
            }
        }
    }
}

/// `Some(result)` when the batch must not start. Every target is read before
/// any write. A target below a folder of the same batch is absent by
/// construction and is not read.
fn preflight(ops: &dyn WriteOps, entries: &[Staged]) -> Option<ApplyResult> {
    if let Some(path) = contradiction(entries) {
        return Some(ApplyResult::Invalid { path });
    }
    let dirs: Vec<&Path> = entries
        .iter()
        .filter(|s| s.entry == Entry::Dir)
        .map(|s| s.target.real.as_path())
        .collect();
    let mut drifted = Vec::new();
    for staged in entries {
        let real = staged.target.real.as_path();
        let new_folder = dirs.iter().any(|d| real.starts_with(d) && real != *d);
        // An edit below a folder this batch makes cannot have a reviewed base:
        // it is probed, and found drifted.
        if new_folder && matches!(staged.entry, Entry::File { base: None, .. } | Entry::Dir) {
            continue;
        }
        match ops.probe(real) {
            Ok(found) if still_as_staged(found.as_deref(), &staged.entry) => {}
            Ok(_) | Err(WriteError::Refused | WriteError::Drifted) => drifted.push(real.into()),
            Err(WriteError::Unsupported) => return Some(ApplyResult::Unsupported),
            Err(_) => {
                return Some(ApplyResult::Failed {
                    path: real.into(),
                    undone: false,
                });
            }
        }
    }
    (!drifted.is_empty()).then_some(ApplyResult::Drifted(drifted))
}

/// The first path the batch names twice, or names as a file and as the folder
/// of another entry. No order of writes could make such a batch true.
fn contradiction(entries: &[Staged]) -> Option<PathBuf> {
    let reals = || entries.iter().map(|s| s.target.real.as_path());
    for (i, staged) in entries.iter().enumerate() {
        let real = staged.target.real.as_path();
        if reals().skip(i + 1).any(|other| other == real) {
            return Some(real.into());
        }
        let is_file = matches!(staged.entry, Entry::File { .. });
        if is_file && reals().any(|other| other != real && other.starts_with(real)) {
            return Some(real.into());
        }
    }
    None
}

/// Whether what the disk holds is what the batch was staged against.
fn still_as_staged(found: Option<&[u8]>, entry: &Entry) -> bool {
    match (entry, found) {
        (Entry::Dir | Entry::File { base: None, .. }, None) => true,
        (
            Entry::File {
                base: Some(base), ..
            },
            Some(bytes),
        ) => bytes == base.as_bytes(),
        _ => false,
    }
}

/// Folders first, shallowest first, then the files in staging order.
fn ordered(entries: &[Staged]) -> Vec<&Staged> {
    let mut all: Vec<&Staged> = entries.iter().collect();
    all.sort_by_key(|s| match s.entry {
        Entry::Dir => (0, s.target.real.components().count()),
        Entry::File { .. } => (1, 0),
    });
    all
}

/// Puts back what one finished step changed. A created file is removed only
/// while it holds what we wrote; an overwritten one is restored from its base
/// only while it holds what we wrote. A created file or folder that is already
/// gone is the pre-batch state, so it is no failure; an edited file that is
/// gone has lost its old text, so it is.
fn undo(ops: &dyn WriteOps, staged: &Staged) -> Result<(), WriteError> {
    let real = staged.target.real.as_path();
    let gone_is_fine = |done: Result<(), WriteError>| match done {
        Err(WriteError::Missing) => Ok(()),
        other => other,
    };
    match &staged.entry {
        Entry::Dir => gone_is_fine(ops.delete_dir(real)),
        Entry::File { text, base: None } => gone_is_fine(ops.delete_file(real, text.as_bytes())),
        Entry::File {
            text,
            base: Some(base),
        } => ops.replace_file(real, text.as_bytes(), base.as_bytes()),
    }
}

/// One forward step.
fn run(ops: &dyn WriteOps, staged: &Staged) -> Result<(), WriteError> {
    let real = staged.target.real.as_path();
    match &staged.entry {
        Entry::Dir => ops.make_dir(real),
        Entry::File { text, base: None } => ops.create_file(real, text.as_bytes()),
        Entry::File {
            text,
            base: Some(base),
        } => ops.replace_file(real, base.as_bytes(), text.as_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ai::tools::{
        paths::WriteTarget,
        stage::{Entry, Staged},
    };
    use std::{
        cell::{Cell, RefCell},
        collections::{BTreeMap, BTreeSet},
        path::{Path, PathBuf},
    };

    /// An in-memory disk that counts every changing call and fails the ones
    /// named in `fail_calls` (0-based, undo calls included).
    #[derive(Default)]
    struct Fake {
        files: RefCell<BTreeMap<PathBuf, Vec<u8>>>,
        dirs: RefCell<BTreeSet<PathBuf>>,
        log: RefCell<Vec<String>>,
        calls: Cell<usize>,
        fail_calls: RefCell<BTreeSet<usize>>,
        probe_error: RefCell<BTreeMap<PathBuf, WriteError>>,
        /// What a failing call reports.
        error: Cell<Option<WriteError>>,
    }

    impl Fake {
        fn with_file(self, path: &str, text: &str) -> Fake {
            self.files
                .borrow_mut()
                .insert(PathBuf::from(path), text.into());
            self
        }

        fn failing(self, calls: &[usize]) -> Fake {
            self.fail_calls.borrow_mut().extend(calls);
            self
        }

        /// Counts a changing call; `Err` when this one is meant to fail.
        fn tick(&self, what: String) -> Result<(), WriteError> {
            let n = self.calls.get();
            self.calls.set(n + 1);
            if self.fail_calls.borrow().contains(&n) {
                return Err(self.error.get().unwrap_or(WriteError::Io));
            }
            self.log.borrow_mut().push(what);
            Ok(())
        }

        fn text(&self, path: &str) -> Option<String> {
            let files = self.files.borrow();
            files
                .get(Path::new(path))
                .map(|b| String::from_utf8(b.clone()).unwrap())
        }

        fn log(&self) -> Vec<String> {
            self.log.borrow().clone()
        }
    }

    impl WriteOps for Fake {
        fn probe(&self, real: &Path) -> Result<Option<Vec<u8>>, WriteError> {
            if let Some(error) = self.probe_error.borrow().get(real) {
                return Err(*error);
            }
            if self.dirs.borrow().contains(real) {
                return Err(WriteError::Refused);
            }
            Ok(self.files.borrow().get(real).cloned())
        }

        fn make_dir(&self, real: &Path) -> Result<(), WriteError> {
            self.tick(format!("mkdir {}", real.display()))?;
            self.dirs.borrow_mut().insert(real.to_path_buf());
            Ok(())
        }

        fn create_file(&self, real: &Path, bytes: &[u8]) -> Result<(), WriteError> {
            self.tick(format!("create {}", real.display()))?;
            self.files
                .borrow_mut()
                .insert(real.to_path_buf(), bytes.to_vec());
            Ok(())
        }

        fn replace_file(
            &self,
            real: &Path,
            expected: &[u8],
            bytes: &[u8],
        ) -> Result<(), WriteError> {
            self.tick(format!("replace {}", real.display()))?;
            let mut files = self.files.borrow_mut();
            match files.get(real).map(Vec::as_slice) {
                None => return Err(WriteError::Missing),
                Some(found) if found != expected => return Err(WriteError::Drifted),
                Some(_) => {}
            }
            files.insert(real.to_path_buf(), bytes.to_vec());
            Ok(())
        }

        fn delete_file(&self, real: &Path, expected: &[u8]) -> Result<(), WriteError> {
            self.tick(format!("delete {}", real.display()))?;
            let mut files = self.files.borrow_mut();
            match files.get(real).map(Vec::as_slice) {
                None => return Err(WriteError::Missing),
                Some(found) if found != expected => return Err(WriteError::Drifted),
                Some(_) => {}
            }
            files.remove(real);
            Ok(())
        }

        fn delete_dir(&self, real: &Path) -> Result<(), WriteError> {
            self.tick(format!("rmdir {}", real.display()))?;
            if !self.dirs.borrow().contains(real) {
                return Err(WriteError::Missing);
            }
            if self.files.borrow().keys().any(|f| f.starts_with(real)) {
                return Err(WriteError::Io); // not empty
            }
            self.dirs.borrow_mut().remove(real);
            Ok(())
        }
    }

    /// A `Fake` that lets a test act on the disk just before an undo step
    /// (a delete, or a replace) touches a path.
    struct Hook<'a>(&'a Fake, &'a dyn Fn(&Fake, &Path));

    impl WriteOps for Hook<'_> {
        fn probe(&self, real: &Path) -> Result<Option<Vec<u8>>, WriteError> {
            self.0.probe(real)
        }
        fn make_dir(&self, real: &Path) -> Result<(), WriteError> {
            self.0.make_dir(real)
        }
        fn create_file(&self, real: &Path, bytes: &[u8]) -> Result<(), WriteError> {
            self.0.create_file(real, bytes)
        }
        fn replace_file(&self, r: &Path, e: &[u8], b: &[u8]) -> Result<(), WriteError> {
            (self.1)(self.0, r);
            self.0.replace_file(r, e, b)
        }
        fn delete_file(&self, real: &Path, expected: &[u8]) -> Result<(), WriteError> {
            (self.1)(self.0, real);
            self.0.delete_file(real, expected)
        }
        fn delete_dir(&self, real: &Path) -> Result<(), WriteError> {
            (self.1)(self.0, real);
            self.0.delete_dir(real)
        }
    }

    fn failed(path: &str, undone: bool) -> ApplyResult {
        ApplyResult::Failed {
            path: PathBuf::from(path),
            undone,
        }
    }

    fn target(path: &str) -> WriteTarget {
        WriteTarget {
            real: PathBuf::from(path),
            build_file: false,
        }
    }

    fn file(path: &str, text: &str, base: Option<&str>) -> Staged {
        Staged {
            target: target(path),
            entry: Entry::File {
                text: text.into(),
                base: base.map(Into::into),
            },
        }
    }

    fn dir(path: &str) -> Staged {
        Staged {
            target: target(path),
            entry: Entry::Dir,
        }
    }

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    // We2.1: preflight

    #[test]
    fn an_overwrite_whose_disk_text_changed_refuses_the_whole_batch() {
        let fake = Fake::default()
            .with_file("/h/p/a", "someone else")
            .with_file("/h/p/b", "old b");
        let batch = [
            file("/h/p/b", "new b", Some("old b")),
            file("/h/p/a", "new a", Some("old a")),
            file("/h/p/c", "new c", None),
        ];
        assert_eq!(
            apply(&fake, &batch),
            ApplyResult::Drifted(paths(&["/h/p/a"]))
        );
        assert_eq!(fake.log(), Vec::<String>::new());
        assert_eq!(fake.text("/h/p/b").as_deref(), Some("old b"));
        assert_eq!(fake.text("/h/p/c"), None);
    }

    #[test]
    fn a_create_whose_name_is_now_taken_drifts_and_every_drifted_path_is_listed() {
        let fake = Fake::default()
            .with_file("/h/p/a", "taken")
            .with_file("/h/p/b", "also");
        let batch = [
            file("/h/p/a", "x", None),
            file("/h/p/b", "y", Some("different")),
        ];
        assert_eq!(
            apply(&fake, &batch),
            ApplyResult::Drifted(paths(&["/h/p/a", "/h/p/b"]))
        );
        assert_eq!(fake.log(), Vec::<String>::new());
    }

    #[test]
    fn an_overwrite_whose_file_vanished_drifts() {
        let fake = Fake::default();
        let batch = [file("/h/p/a", "x", Some("old"))];
        assert_eq!(
            apply(&fake, &batch),
            ApplyResult::Drifted(paths(&["/h/p/a"]))
        );
    }

    #[test]
    fn a_folder_that_exists_now_drifts_and_a_refused_target_drifts_too() {
        let fake = Fake::default();
        fake.dirs.borrow_mut().insert(PathBuf::from("/h/p/d"));
        fake.probe_error
            .borrow_mut()
            .insert(PathBuf::from("/h/p/link"), WriteError::Refused);
        let batch = [
            dir("/h/p/d"),
            file("/h/p/link", "x", Some("y")),
            file("/h/p/ok", "x", None),
        ];
        assert_eq!(
            apply(&fake, &batch),
            ApplyResult::Drifted(paths(&["/h/p/d", "/h/p/link"]))
        );
        assert_eq!(fake.log(), Vec::<String>::new());
    }

    #[test]
    fn a_clean_batch_passes_the_preflight_and_is_written() {
        let fake = Fake::default().with_file("/h/p/b", "old b");
        let batch = [
            file("/h/p/b", "new b", Some("old b")),
            file("/h/p/a", "new a", None),
        ];
        assert!(matches!(apply(&fake, &batch), ApplyResult::Applied(_)));
        assert_eq!(fake.text("/h/p/b").as_deref(), Some("new b"));
        assert_eq!(fake.text("/h/p/a").as_deref(), Some("new a"));
    }

    #[test]
    fn a_target_below_a_folder_of_the_same_batch_needs_no_probe() {
        // `/h/p/new` does not exist yet, so a probe of its children would fail.
        let fake = Fake::default();
        fake.probe_error
            .borrow_mut()
            .insert(PathBuf::from("/h/p/new/a"), WriteError::Refused);
        let batch = [file("/h/p/new/a", "x", None), dir("/h/p/new")];
        assert!(matches!(apply(&fake, &batch), ApplyResult::Applied(_)));
        assert_eq!(fake.text("/h/p/new/a").as_deref(), Some("x"));
    }

    #[test]
    fn a_probe_that_cannot_judge_stops_before_any_write() {
        let fake = Fake::default();
        fake.probe_error
            .borrow_mut()
            .insert(PathBuf::from("/h/p/a"), WriteError::Io);
        let batch = [file("/h/p/b", "x", None), file("/h/p/a", "x", None)];
        assert_eq!(apply(&fake, &batch), failed("/h/p/a", false));
        assert_eq!(fake.log(), Vec::<String>::new());
        let fake = Fake::default();
        fake.probe_error
            .borrow_mut()
            .insert(PathBuf::from("/h/p/a"), WriteError::Unsupported);
        let batch = [file("/h/p/a", "x", None)];
        assert_eq!(apply(&fake, &batch), ApplyResult::Unsupported);
    }
    // We2.2: journal and rollback

    #[test]
    fn a_fault_at_the_third_step_puts_the_first_two_back() {
        let fake = Fake::default().with_file("/h/p/b", "old b").failing(&[2]);
        let batch = [
            file("/h/p/a", "new a", None),
            file("/h/p/b", "new b", Some("old b")),
            file("/h/p/c", "new c", None),
        ];
        assert_eq!(apply(&fake, &batch), failed("/h/p/c", true));
        assert_eq!(fake.text("/h/p/a"), None);
        assert_eq!(fake.text("/h/p/b").as_deref(), Some("old b"));
        assert_eq!(fake.text("/h/p/c"), None);
        // Undone in reverse: the edit first, then the create.
        assert_eq!(
            fake.log(),
            [
                "create /h/p/a",
                "replace /h/p/b",
                "replace /h/p/b",
                "delete /h/p/a"
            ]
        );
    }

    #[test]
    fn folders_are_made_shallowest_first_and_removed_deepest_first() {
        let fake = Fake::default().failing(&[3]);
        let batch = [
            file("/h/p/x/y/a", "a", None),
            dir("/h/p/x/y"),
            dir("/h/p/x"),
            file("/h/p/b", "b", None),
        ];
        assert!(matches!(apply(&fake, &batch), ApplyResult::Failed { .. }));
        assert_eq!(
            fake.log(),
            [
                "mkdir /h/p/x",
                "mkdir /h/p/x/y",
                "create /h/p/x/y/a",
                "delete /h/p/x/y/a",
                "rmdir /h/p/x/y",
                "rmdir /h/p/x",
            ]
        );
        assert!(fake.dirs.borrow().is_empty());
        assert!(fake.files.borrow().is_empty());
    }

    #[test]
    fn a_fault_at_the_first_step_has_nothing_to_undo() {
        let fake = Fake::default().failing(&[0]);
        let batch = [file("/h/p/a", "a", None), file("/h/p/b", "b", None)];
        assert_eq!(apply(&fake, &batch), failed("/h/p/a", false));
        assert_eq!(fake.log(), Vec::<String>::new());
    }

    #[test]
    fn a_file_that_drifts_between_the_preflight_and_its_write_reports_drift_after_the_undo() {
        let fake = Fake::default().with_file("/h/p/b", "old b").failing(&[1]);
        fake.error.set(Some(WriteError::Drifted));
        let batch = [
            file("/h/p/a", "a", None),
            file("/h/p/b", "new b", Some("old b")),
        ];
        assert_eq!(
            apply(&fake, &batch),
            ApplyResult::Drifted(paths(&["/h/p/b"]))
        );
        assert_eq!(fake.text("/h/p/a"), None);
    }

    // We2 review fixes

    #[test]
    fn an_undo_that_finds_a_created_file_or_folder_already_gone_is_a_clean_undo() {
        // The disk already holds the pre-batch state: nothing is left changed.
        let fake = Fake::default().failing(&[1]);
        let gone = |fake: &Fake, real: &Path| {
            fake.files.borrow_mut().remove(real);
        };
        let batch = [file("/h/p/a", "a", None), file("/h/p/b", "b", None)];
        assert_eq!(apply(&Hook(&fake, &gone), &batch), failed("/h/p/b", true));

        let fake = Fake::default().failing(&[1]);
        let gone = |fake: &Fake, real: &Path| {
            fake.dirs.borrow_mut().remove(real);
        };
        let batch = [dir("/h/p/x"), dir("/h/p/x/y")];
        assert_eq!(apply(&Hook(&fake, &gone), &batch), failed("/h/p/x/y", true));
    }

    #[test]
    fn an_undo_that_finds_an_edited_file_gone_is_partial_because_the_old_text_is_lost() {
        let fake = Fake::default().with_file("/h/p/b", "old b").failing(&[1]);
        let gone = |fake: &Fake, real: &Path| {
            if fake.text("/h/p/b").as_deref() == Some("new b") {
                fake.files.borrow_mut().remove(real);
            }
        };
        let batch = [
            file("/h/p/b", "new b", Some("old b")),
            file("/h/p/c", "c", None),
        ];
        assert_eq!(
            apply(&Hook(&fake, &gone), &batch),
            ApplyResult::Partial {
                changed: paths(&["/h/p/b"])
            }
        );
    }

    #[test]
    fn a_forward_step_that_races_reports_drift_like_the_preflight_would() {
        let drift = |batch: &[Staged], error, fake: Fake| {
            fake.error.set(Some(error));
            let got = apply(&fake, batch);
            (got, fake)
        };
        // A create finds the name taken.
        let batch = [file("/h/p/a", "a", None), file("/h/p/b", "b", None)];
        let (got, fake) = drift(&batch, WriteError::Exists, Fake::default().failing(&[1]));
        assert_eq!(got, ApplyResult::Drifted(paths(&["/h/p/b"])));
        assert_eq!(fake.text("/h/p/a"), None);
        // A make_dir finds the folder there.
        let batch = [dir("/h/p/x"), dir("/h/p/x/y")];
        let (got, fake) = drift(&batch, WriteError::Exists, Fake::default().failing(&[1]));
        assert_eq!(got, ApplyResult::Drifted(paths(&["/h/p/x/y"])));
        assert!(fake.dirs.borrow().is_empty());
        // An edit finds no file.
        let batch = [
            file("/h/p/a", "a", None),
            file("/h/p/b", "new", Some("old")),
        ];
        let start = Fake::default().with_file("/h/p/b", "old").failing(&[1]);
        let (got, fake) = drift(&batch, WriteError::Missing, start);
        assert_eq!(got, ApplyResult::Drifted(paths(&["/h/p/b"])));
        assert_eq!(fake.text("/h/p/a"), None);
        // A target swapped for a link is refused.
        let start = Fake::default().with_file("/h/p/b", "old").failing(&[1]);
        let (got, _) = drift(&batch, WriteError::Refused, start);
        assert_eq!(got, ApplyResult::Drifted(paths(&["/h/p/b"])));
    }

    #[test]
    fn a_forward_step_that_fails_for_any_other_reason_stays_failed() {
        // `Missing` on a create means the parent is gone: not a reviewed state.
        for error in [WriteError::Io, WriteError::Missing] {
            let fake = Fake::default().failing(&[1]);
            fake.error.set(Some(error));
            let batch = [file("/h/p/a", "a", None), file("/h/p/b", "b", None)];
            assert_eq!(apply(&fake, &batch), failed("/h/p/b", true), "{error:?}");
        }
    }

    #[test]
    fn a_batch_that_names_a_path_twice_is_refused_before_any_write() {
        let batch = [
            file("/h/p/a", "one", None),
            file("/h/p/b", "b", None),
            file("/h/p/a", "two", None),
        ];
        let fake = Fake::default();
        let got = apply(&fake, &batch);
        assert_eq!(
            got,
            ApplyResult::Invalid {
                path: "/h/p/a".into()
            }
        );
        assert_eq!(fake.log(), Vec::<String>::new());
        let got = apply(&fake, &[dir("/h/p/d"), dir("/h/p/d")]);
        assert_eq!(
            got,
            ApplyResult::Invalid {
                path: "/h/p/d".into()
            }
        );
    }

    #[test]
    fn a_file_that_is_also_the_folder_of_another_entry_is_refused() {
        let fake = Fake::default();
        for batch in [
            [file("/h/p/a", "x", None), file("/h/p/a/b", "y", None)],
            [file("/h/p/a/b", "y", None), file("/h/p/a", "x", None)],
            [file("/h/p/a", "x", None), dir("/h/p/a/d")],
        ] {
            let got = apply(&fake, &batch);
            assert_eq!(
                got,
                ApplyResult::Invalid {
                    path: "/h/p/a".into()
                }
            );
        }
        assert_eq!(fake.log(), Vec::<String>::new());
        // A name that only shares a prefix is another path.
        let batch = [file("/h/p/a", "x", None), file("/h/p/ab", "y", None)];
        assert!(matches!(apply(&fake, &batch), ApplyResult::Applied(_)));
    }

    #[test]
    fn an_edit_below_a_folder_made_by_the_same_batch_drifts() {
        // The folder is new, so the file cannot hold a reviewed base.
        let fake = Fake::default();
        let batch = [
            dir("/h/p/new"),
            file("/h/p/new/a", "x", Some("old")),
            file("/h/p/new/b", "y", None),
        ];
        assert_eq!(
            apply(&fake, &batch),
            ApplyResult::Drifted(paths(&["/h/p/new/a"]))
        );
        assert_eq!(fake.log(), Vec::<String>::new());
    }

    // We2.3: Partial and Change records

    #[test]
    fn a_failing_undo_reports_partial_with_exactly_what_is_still_changed() {
        // Fail the third write (call 2), then the restore of the first (call 4).
        let fake = Fake::default().failing(&[2, 4]);
        let batch = [
            file("/h/p/a", "a", None),
            file("/h/p/b", "b", None),
            file("/h/p/c", "c", None),
        ];
        assert_eq!(
            apply(&fake, &batch),
            ApplyResult::Partial {
                changed: paths(&["/h/p/a"])
            }
        );
        // The undo went on past the failure: b is gone, a is left.
        assert_eq!(fake.text("/h/p/a").as_deref(), Some("a"));
        assert_eq!(fake.text("/h/p/b"), None);
    }

    #[test]
    fn a_folder_whose_file_could_not_be_removed_is_left_changed_too() {
        let fake = Fake::default().failing(&[2, 3]);
        let batch = [
            dir("/h/p/d"),
            file("/h/p/d/a", "a", None),
            file("/h/p/d/b", "b", None),
        ];
        // b fails (2), the undo of a fails (3), so the folder cannot go either.
        assert_eq!(
            apply(&fake, &batch),
            ApplyResult::Partial {
                changed: paths(&["/h/p/d/a", "/h/p/d"])
            }
        );
    }

    #[test]
    fn an_undo_that_finds_a_replaced_file_leaves_it_and_says_so() {
        let fake = Fake::default().failing(&[1]);
        fake.error.set(Some(WriteError::Io));
        let batch = [file("/h/p/a", "a", None), file("/h/p/b", "b", None)];
        // Someone swaps a's content after we wrote it, before the undo.
        let swap = |fake: &Fake, real: &Path| {
            fake.files
                .borrow_mut()
                .insert(real.to_path_buf(), b"theirs".to_vec());
        };
        assert_eq!(
            apply(&Hook(&fake, &swap), &batch),
            ApplyResult::Partial {
                changed: paths(&["/h/p/a"])
            }
        );
        assert_eq!(fake.text("/h/p/a").as_deref(), Some("theirs"));
    }

    #[test]
    fn success_returns_one_change_per_path_with_kind_bytes_and_line_counts() {
        let fake = Fake::default().with_file("/h/p/e", "a\nb\n");
        let batch = [
            file("/h/p/n", "x\ny\n", None),
            file("/h/p/e", "a\nc\nd\n", Some("a\nb\n")),
            dir("/h/p/d"),
        ];
        let change = |path: &str, kind, bytes, added, removed| Change {
            path: PathBuf::from(path),
            kind,
            bytes,
            added,
            removed,
        };
        assert_eq!(
            apply(&fake, &batch),
            ApplyResult::Applied(vec![
                change("/h/p/d", ChangeKind::DirCreated, 0, 0, 0),
                change("/h/p/n", ChangeKind::Created, 4, 2, 0),
                change("/h/p/e", ChangeKind::Edited, 6, 2, 1),
            ])
        );
    }

    #[test]
    fn what_goes_toward_the_model_names_paths_and_counts_and_never_text() {
        let secret = "API_KEY=hunter2";
        let fake = Fake::default().with_file("/h/p/e", secret);
        let batch = [
            file("/h/p/n", secret, None),
            file("/h/p/e", "other", Some(secret)),
        ];
        let done = apply(&fake, &batch);
        assert_eq!(
            done.summary(),
            "applied 2 paths:\ncreated /h/p/n (+1 -0, 15 bytes)\nedited /h/p/e (+1 -1, 5 bytes)"
        );
        let drifted = ApplyResult::Drifted(paths(&["/h/p/a", "/h/p/b"]));
        assert_eq!(
            drifted.summary(),
            "refused: 2 paths changed since the review, nothing was written:\n/h/p/a\n/h/p/b"
        );
        let failed = super::tests::failed("/h/p/c", true);
        assert_eq!(
            failed.summary(),
            "failed at /h/p/c: every earlier change was undone"
        );
        assert_eq!(
            super::tests::failed("/h/p/c", false).summary(),
            "failed at /h/p/c: nothing was written"
        );
        assert_eq!(
            ApplyResult::Invalid {
                path: PathBuf::from("/h/p/c")
            }
            .summary(),
            "refused: the batch is inconsistent at /h/p/c, nothing was written"
        );
        let partial = ApplyResult::Partial {
            changed: paths(&["/h/p/a"]),
        };
        assert_eq!(
            partial.summary(),
            "partial: 1 path is still changed on disk (by this batch, or by someone else and left as it is), check it by hand:\n/h/p/a"
        );
        assert_eq!(
            ApplyResult::Unsupported.summary(),
            "writes are not supported on this platform, nothing was written"
        );
        for result in [&done, &drifted, &failed, &partial] {
            assert!(!result.summary().contains("hunter2"));
            assert!(!format!("{result:?}").contains("hunter2"));
        }
    }

    // We2.1: the real disk

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    mod on_disk {
        use super::*;

        struct Fx {
            _dir: tempfile::TempDir,
            home: PathBuf,
            roots: SecretRoots,
        }

        fn fx() -> Fx {
            let dir = tempfile::tempdir().unwrap();
            let home = dir.path().canonicalize().unwrap();
            std::fs::create_dir_all(home.join("proj")).unwrap();
            let roots = SecretRoots::load(&home, &[]).unwrap();
            Fx {
                _dir: dir,
                home,
                roots,
            }
        }

        fn at(f: &Fx, rel: &str) -> String {
            f.home.join(rel).to_string_lossy().into_owned()
        }

        #[test]
        fn a_clean_batch_lands_on_the_disk() {
            let f = fx();
            std::fs::write(f.home.join("proj/old.txt"), "old").unwrap();
            let batch = [
                file(&at(&f, "proj/new/a.txt"), "a", None),
                dir(&at(&f, "proj/new")),
                file(&at(&f, "proj/old.txt"), "newer", Some("old")),
            ];
            let got = apply(&Disk::new(&f.roots), &batch);
            assert!(matches!(got, ApplyResult::Applied(_)), "{got:?}");
            let read = |rel: &str| std::fs::read_to_string(f.home.join(rel)).unwrap();
            assert_eq!(read("proj/new/a.txt"), "a");
            assert_eq!(read("proj/old.txt"), "newer");
        }

        #[test]
        fn a_target_that_became_a_symlink_is_refused_and_nothing_is_written_through() {
            let f = fx();
            std::fs::write(f.home.join("proj/real.txt"), "old").unwrap();
            std::os::unix::fs::symlink(f.home.join("proj/real.txt"), f.home.join("proj/t.txt"))
                .unwrap();
            let batch = [
                file(&at(&f, "proj/first.txt"), "x", None),
                file(&at(&f, "proj/t.txt"), "new", Some("old")),
            ];
            let got = apply(&Disk::new(&f.roots), &batch);
            assert_eq!(got, ApplyResult::Drifted(vec![f.home.join("proj/t.txt")]));
            let read = |rel: &str| std::fs::read_to_string(f.home.join(rel)).unwrap();
            assert_eq!(read("proj/real.txt"), "old");
            assert!(!f.home.join("proj/first.txt").exists());
        }

        #[test]
        fn a_hard_linked_target_and_a_changed_file_drift() {
            let f = fx();
            std::fs::write(f.home.join("proj/a.txt"), "old").unwrap();
            std::fs::hard_link(f.home.join("proj/a.txt"), f.home.join("proj/b.txt")).unwrap();
            std::fs::write(f.home.join("proj/c.txt"), "edited since").unwrap();
            let batch = [
                file(&at(&f, "proj/a.txt"), "new", Some("old")),
                file(&at(&f, "proj/c.txt"), "new", Some("old")),
            ];
            let got = apply(&Disk::new(&f.roots), &batch);
            assert_eq!(
                got,
                ApplyResult::Drifted(vec![f.home.join("proj/a.txt"), f.home.join("proj/c.txt")])
            );
            assert_eq!(
                std::fs::read_to_string(f.home.join("proj/b.txt")).unwrap(),
                "old"
            );
        }

        /// A step that cannot work whatever the user is: its parent folder
        /// (`made/sub`) is not there, and nothing in the batch makes it.
        fn broken_step(f: &Fx) -> Staged {
            file(&at(f, "proj/made/sub/c.txt"), "c", None)
        }

        #[test]
        fn a_write_that_cannot_be_made_rolls_the_earlier_ones_back_on_the_real_disk() {
            let f = fx();
            std::fs::write(f.home.join("proj/old.txt"), "old").unwrap();
            let batch = [
                dir(&at(&f, "proj/made")),
                file(&at(&f, "proj/old.txt"), "newer", Some("old")),
                file(&at(&f, "proj/made/a.txt"), "a", None),
                broken_step(&f),
            ];
            let got = apply(&Disk::new(&f.roots), &batch);
            assert_eq!(
                got,
                ApplyResult::Drifted(vec![f.home.join("proj/made/sub/c.txt")])
            );
            assert!(!f.home.join("proj/made").exists());
            assert_eq!(
                std::fs::read_to_string(f.home.join("proj/old.txt")).unwrap(),
                "old"
            );
        }

        #[test]
        fn an_edited_file_restored_by_the_undo_keeps_its_mode() {
            use std::os::unix::fs::PermissionsExt;
            let f = fx();
            let old = f.home.join("proj/old.txt");
            std::fs::write(&old, "old").unwrap();
            std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o640)).unwrap();
            let batch = [
                dir(&at(&f, "proj/made")),
                file(&at(&f, "proj/old.txt"), "newer", Some("old")),
                broken_step(&f),
            ];
            let got = apply(&Disk::new(&f.roots), &batch);
            assert!(matches!(got, ApplyResult::Drifted(_)), "{got:?}");
            assert_eq!(std::fs::read_to_string(&old).unwrap(), "old");
            let mode = std::fs::metadata(&old).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o640);
        }

        /// The real disk, with a hook run just before a folder is removed.
        struct Plant<'a>(Disk<'a>, &'a dyn Fn(&Path));

        impl WriteOps for Plant<'_> {
            fn probe(&self, real: &Path) -> Result<Option<Vec<u8>>, WriteError> {
                self.0.probe(real)
            }
            fn make_dir(&self, real: &Path) -> Result<(), WriteError> {
                self.0.make_dir(real)
            }
            fn create_file(&self, real: &Path, bytes: &[u8]) -> Result<(), WriteError> {
                self.0.create_file(real, bytes)
            }
            fn replace_file(&self, r: &Path, e: &[u8], b: &[u8]) -> Result<(), WriteError> {
                self.0.replace_file(r, e, b)
            }
            fn delete_file(&self, real: &Path, expected: &[u8]) -> Result<(), WriteError> {
                self.0.delete_file(real, expected)
            }
            fn delete_dir(&self, real: &Path) -> Result<(), WriteError> {
                (self.1)(real);
                self.0.delete_dir(real)
            }
        }

        #[test]
        fn a_folder_that_got_a_user_file_before_the_undo_is_partial_and_the_file_is_kept() {
            let f = fx();
            let batch = [dir(&at(&f, "proj/made")), broken_step(&f)];
            let user = f.home.join("proj/made/user.txt");
            let plant = |_: &Path| std::fs::write(&user, "theirs").unwrap();
            let got = apply(&Plant(Disk::new(&f.roots), &plant), &batch);
            assert_eq!(
                got,
                ApplyResult::Partial {
                    changed: vec![f.home.join("proj/made")]
                }
            );
            assert_eq!(std::fs::read_to_string(&user).unwrap(), "theirs");
        }
    }

    // The model-facing line of a refused or failed apply holds no path.
    #[test]
    fn the_told_line_of_a_refused_or_failed_apply_names_no_path() {
        let path = PathBuf::from("/home/u/secret-project/key.txt");
        let results = [
            ApplyResult::Invalid { path: path.clone() },
            ApplyResult::Failed {
                path: path.clone(),
                undone: true,
            },
            ApplyResult::Failed {
                path: path.clone(),
                undone: false,
            },
            ApplyResult::Drifted(vec![path.clone()]),
            ApplyResult::Partial {
                changed: vec![path.clone()],
            },
            ApplyResult::Unsupported,
        ];
        for result in results {
            let line = result.told();
            assert!(
                !line.contains("secret") && !line.contains("key.txt"),
                "{line}"
            );
            assert!(!line.contains('/'), "{line}");
        }
        assert_eq!(
            ApplyResult::Invalid { path }.told(),
            "The user's apply of your staged batch was refused; nothing was written."
        );
    }
}
