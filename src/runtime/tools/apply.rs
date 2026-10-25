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
    /// A step failed at `path`; whatever was done is undone.
    Failed {
        path: PathBuf,
    },
    /// No safe write on this platform; nothing was written.
    Unsupported,
}

/// Applies the entries in order (folders by depth, then files), after the
/// preflight of every target.
pub fn apply(ops: &dyn WriteOps, entries: &[Staged]) -> ApplyResult {
    if let Some(refused) = preflight(ops, entries) {
        return refused;
    }
    for staged in ordered(entries) {
        if run(ops, staged).is_err() {
            return ApplyResult::Failed {
                path: staged.target.real.clone(),
            };
        }
    }
    ApplyResult::Applied(Vec::new())
}

/// `Some(result)` when the batch must not start. Every target is read before
/// any write. A target below a folder of the same batch is absent by
/// construction and is not read.
fn preflight(ops: &dyn WriteOps, entries: &[Staged]) -> Option<ApplyResult> {
    let dirs: Vec<&Path> = entries
        .iter()
        .filter(|s| s.entry == Entry::Dir)
        .map(|s| s.target.real.as_path())
        .collect();
    let mut drifted = Vec::new();
    for staged in entries {
        let real = staged.target.real.as_path();
        if dirs.iter().any(|d| real.starts_with(d) && real != *d) {
            continue;
        }
        match ops.probe(real) {
            Ok(found) if still_as_staged(found.as_deref(), &staged.entry) => {}
            Ok(_) | Err(WriteError::Refused | WriteError::Drifted) => drifted.push(real.into()),
            Err(WriteError::Unsupported) => return Some(ApplyResult::Unsupported),
            Err(_) => return Some(ApplyResult::Failed { path: real.into() }),
        }
    }
    (!drifted.is_empty()).then_some(ApplyResult::Drifted(drifted))
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
    }

    impl Fake {
        fn with_file(self, path: &str, text: &str) -> Fake {
            self.files
                .borrow_mut()
                .insert(PathBuf::from(path), text.into());
            self
        }

        /// Counts a changing call; `Err` when this one is meant to fail.
        fn tick(&self, what: String) -> Result<(), WriteError> {
            let n = self.calls.get();
            self.calls.set(n + 1);
            if self.fail_calls.borrow().contains(&n) {
                return Err(WriteError::Io);
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
            if files.get(real).map(Vec::as_slice) != Some(expected) {
                return Err(WriteError::Drifted);
            }
            files.insert(real.to_path_buf(), bytes.to_vec());
            Ok(())
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
        assert_eq!(
            apply(&fake, &batch),
            ApplyResult::Failed {
                path: PathBuf::from("/h/p/a")
            }
        );
        assert_eq!(fake.log(), Vec::<String>::new());
        let fake = Fake::default();
        fake.probe_error
            .borrow_mut()
            .insert(PathBuf::from("/h/p/a"), WriteError::Unsupported);
        let batch = [file("/h/p/a", "x", None)];
        assert_eq!(apply(&fake, &batch), ApplyResult::Unsupported);
    }
    // We2.1: the real disk

    #[cfg(target_os = "linux")]
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
    }
}
