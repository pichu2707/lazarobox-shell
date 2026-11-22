//! The local project memory in the runtime: one thread that does all the file
//! IO of `.lzr/`, the `.gitignore` edit, the private ledger and the machine
//! id. The app never waits on it: commands go in, replies come back as
//! events.

use std::{
    fs::{self, DirBuilder, OpenOptions},
    io::{self, ErrorKind, Read, Write},
    os::unix::{
        fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
        io::AsRawFd,
    },
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use super::{
    bounded_read::{ReadError, read_regular},
    workspace,
};
use crate::core::{
    ai::memory::local::{
        NOTES_DIR, STORE_DIR,
        area::area_of,
        gitignore::{self, GitignoreOutcome},
        ledger::{self, Ledger, LedgerError},
        note::{LocalNote, NewNote, Unreadable},
        port::{Failure, LocalCmd, LocalReply, Root},
    },
    layout::PaneId,
};

/// The most bytes read from one note file; a bigger file is unreadable.
const NOTE_CAP: u64 = 64 * 1024;
/// The most bytes read from the ledger.
const LEDGER_CAP: u64 = 1024 * 1024;
/// Makes every temp file and note id of this run different.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// The runtime's handle on the memory thread.
pub struct LocalMemory {
    /// The thread's queue, or why it could not start.
    queue: Result<mpsc::Sender<LocalCmd>, String>,
    replies: tokio::sync::mpsc::Sender<LocalReply>,
}

impl LocalMemory {
    /// Queues a command; returns at once. With no thread to run it, the chat
    /// gets a failure instead of silence.
    pub fn send(&self, cmd: LocalCmd) {
        let cmd = match &self.queue {
            Ok(tx) => match tx.send(cmd) {
                Ok(()) => return,
                Err(mpsc::SendError(cmd)) => cmd,
            },
            Err(_) => cmd,
        };
        let reason = self
            .queue
            .as_ref()
            .err()
            .map_or("the memory thread stopped", String::as_str);
        let reply = LocalReply::Failed {
            pane: cmd.pane(),
            failure: Failure::Unavailable(reason.to_owned()),
        };
        // A full queue must not swallow the failure: wait for room off the
        // caller's thread. A closed queue means the app is gone.
        if let Err(tokio::sync::mpsc::error::TrySendError::Full(reply)) =
            self.replies.try_send(reply)
        {
            let replies = self.replies.clone();
            std::thread::spawn(move || {
                let _ = replies.blocking_send(reply);
            });
        }
    }
}

/// Starts the thread. `data` is the app's data folder (`<data>/lazarobox`),
/// where the ledger and the machine id live; without it nothing can be saved.
/// The thread ends when the handle is dropped. If it cannot start, every
/// command is answered with a visible failure.
pub fn spawn(
    data: Option<PathBuf>,
    home: Option<PathBuf>,
    replies: tokio::sync::mpsc::Sender<LocalReply>,
) -> LocalMemory {
    spawn_with(data, home, replies, |run| {
        std::thread::Builder::new()
            .name("local-memory".to_owned())
            .spawn(run)
            .map(drop)
    })
}

/// [`spawn`] with the way the thread is started injected, so a start that
/// fails can be tested. `start` runs the loop it is given, on a new thread.
fn spawn_with(
    data: Option<PathBuf>,
    home: Option<PathBuf>,
    replies: tokio::sync::mpsc::Sender<LocalReply>,
    start: impl FnOnce(Box<dyn FnOnce() + Send>) -> io::Result<()>,
) -> LocalMemory {
    let (tx, rx) = mpsc::channel();
    let answers = replies.clone();
    let run = Box::new(move || {
        serve(rx, replies, |cmd| {
            handle(data.as_deref(), home.as_deref(), cmd)
        });
    });
    LocalMemory {
        queue: start(run).map(|()| tx).map_err(|e| e.to_string()),
        replies: answers,
    }
}

/// The thread's loop: one reply per command.
fn serve(
    rx: mpsc::Receiver<LocalCmd>,
    replies: tokio::sync::mpsc::Sender<LocalReply>,
    mut run: impl FnMut(LocalCmd) -> LocalReply,
) {
    for cmd in rx {
        let pane = cmd.pane();
        let reply = catch_unwind(AssertUnwindSafe(|| run(cmd))).unwrap_or(LocalReply::Failed {
            pane,
            failure: Failure::Unavailable("an internal error stopped this command".to_owned()),
        });
        if replies.blocking_send(reply).is_err() {
            break;
        }
    }
}

/// Runs one command to its reply. Blocking file IO: the thread calls it.
pub fn handle(data: Option<&Path>, home: Option<&Path>, cmd: LocalCmd) -> LocalReply {
    match cmd {
        LocalCmd::Load { pane, epoch, cwd } => load(data, home, pane, epoch, &cwd),
        LocalCmd::Save { pane, root, note } => match save(data, &root, note) {
            Ok((id, cut, gitignore, notice)) => LocalReply::Saved {
                pane,
                id,
                cut,
                gitignore,
                notice,
            },
            Err(failure) => LocalReply::Failed { pane, failure },
        },
    }
}

fn load(
    data: Option<&Path>,
    home: Option<&Path>,
    pane: PaneId,
    epoch: u32,
    cwd: &str,
) -> LocalReply {
    let Some((store, work)) = workspace::memory_root(Path::new(cwd), home) else {
        return LocalReply::Loaded {
            pane,
            epoch,
            root: None,
            notes: Vec::new(),
            foreign: 0,
            unreadable: 0,
            newer: 0,
            ledger: None,
        };
    };
    let area = area_of(&work.to_string_lossy(), cwd).unwrap_or_else(|| ".".to_owned());
    let (ledger, ledger_error) = match data.map(|data| read_ledger(&ledger_path(data, &store))) {
        Some(Err(error)) => (Ledger::default(), Some(error)),
        found => (found.and_then(Result::ok).unwrap_or_default(), None),
    };
    let (mut notes, mut foreign, mut unreadable, mut newer) = (Vec::new(), 0, 0, 0);
    match note_files(&store) {
        Ok(files) => {
            for file in files {
                let plain = fs::symlink_metadata(&file).is_ok_and(|m| m.is_file());
                match read_regular(&file, NOTE_CAP)
                    .ok()
                    .filter(|_| plain)
                    .map(|b| LocalNote::parse(&b))
                {
                    Some(Ok(note)) if ledger.is_local(&note) => notes.push(note),
                    Some(Ok(_)) => foreign += 1,
                    Some(Err(Unreadable::Newer)) => newer += 1,
                    _ => unreadable += 1,
                }
            }
        }
        // The folder itself cannot be listed: say so, as one unreadable thing.
        Err(_) => unreadable = 1,
    }
    LocalReply::Loaded {
        pane,
        epoch,
        root: Some(Root { store, area }),
        notes,
        foreign,
        unreadable,
        newer,
        ledger: ledger_error,
    }
}

/// The note files of a store, sorted by name. A store that does not exist yet
/// has none.
fn note_files(store: &Path) -> io::Result<Vec<PathBuf>> {
    plain_store(store)?;
    let dir = match fs::read_dir(store.join(STORE_DIR).join(NOTES_DIR)) {
        Ok(dir) => dir,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut files: Vec<PathBuf> = dir
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    Ok(files)
}

/// What a save made: the note id, whether the text was cut, the `.gitignore`
/// outcome and a notice.
type Saved = (String, bool, Option<GitignoreOutcome>, Option<String>);

/// Refuses a store whose `.lzr`, `memory` or `notes` folder is a link: what
/// is read or written through it would not be in the repository.
fn plain_store(store: &Path) -> io::Result<()> {
    let mut dir = store.to_owned();
    for part in [STORE_DIR, "memory", "notes"] {
        dir.push(part);
        if fs::symlink_metadata(&dir).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(io::Error::other(format!("{part} is a link; refused")));
        }
    }
    Ok(())
}

/// Writes one note: folder, machine id, ledger claim, then the note itself.
/// The ledger goes first, so a note on disk is never left unclaimed. Returns
/// the id, whether the text was cut and, when this save created `.lzr/`, the
/// `.gitignore` outcome.
fn save(data: Option<&Path>, root: &Root, new: NewNote) -> Result<Saved, Failure> {
    let data = data.ok_or_else(|| Failure::Io("private data folder unavailable".to_owned()))?;
    private_dir(data).map_err(|e| failure(&e, data))?;
    private_dir(&data.join("memory")).map_err(|e| failure(&e, data))?;
    let _lock = lock(&data.join("memory.lock")).map_err(|e| failure(&e, data))?;
    let ledger_file = ledger_path(data, &root.store);
    let existing = read_ledger(&ledger_file).map_err(|_| {
        Failure::Io("the private ledger is damaged or newer; it was left as it is".to_owned())
    })?;
    plain_store(&root.store).map_err(|e| Failure::Io(e.to_string()))?;
    let lzr = root.store.join(STORE_DIR);
    let created = !lzr.exists();
    let notes = lzr.join(NOTES_DIR);
    fs::create_dir_all(&notes)
        .map_err(|e| failure(&e, if created { &root.store } else { &lzr }))?;
    let (machine, notice) = machine_id(data).map_err(|e| failure(&e, data))?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let cut = new.cut;
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let id = ledger::note_id(&machine, nanos, counter, &new.text);
    let note = LocalNote::from_new(new, id.clone(), machine, (nanos / 1_000_000_000) as i64);
    let mut mine = Ledger::default();
    mine.record(&note);
    let merged = mine.merged(existing);
    write_atomic(&ledger_file, merged.to_json().as_bytes(), 0o600)
        .map_err(|e| failure(&e, data))?;
    write_atomic(
        &notes.join(format!("{id}.json")),
        note.to_json().as_bytes(),
        0o644,
    )
    .map_err(|e| failure(&e, &lzr))?;
    Ok((
        id,
        cut,
        created.then(|| gitignore::ensure(&root.store)),
        notice,
    ))
}

/// A write error as a typed failure: a permission or read-only error names
/// the folder `at`.
fn failure(error: &io::Error, at: &Path) -> Failure {
    match error.kind() {
        ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem => {
            Failure::ReadOnly(at.to_string_lossy().into_owned())
        }
        _ => Failure::Io(error.to_string()),
    }
}

/// The ledger file of a store, keyed by its real path so a link to the
/// repository finds the same ledger.
fn ledger_path(data: &Path, store: &Path) -> PathBuf {
    let real = fs::canonicalize(store).unwrap_or_else(|_| store.to_owned());
    data.join("memory").join(ledger::ledger_name(&real))
}

/// The ledger at `path`. A missing one is empty; a damaged or newer one is an
/// error, so it is said and never written over.
fn read_ledger(path: &Path) -> Result<Ledger, LedgerError> {
    match read_regular(path, LEDGER_CAP) {
        Err(ReadError::Missing) => Ok(Ledger::default()),
        Err(_) => Err(LedgerError::Damaged),
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|_| LedgerError::Damaged)
            .and_then(|text| Ledger::parse(&text)),
    }
}

/// This machine's id, made on the first save. An id that exists but cannot
/// be used is made again, and the notice says so. Another process may make
/// it first, so a few tries are made; then it is an error, never a loop.
fn machine_id(data: &Path) -> io::Result<(String, Option<String>)> {
    let file = data.join("machine-id");
    for _ in 0..3 {
        let damaged = match read_regular(&file, 64) {
            Ok(bytes) => {
                let id = String::from_utf8_lossy(&bytes).trim().to_owned();
                if id.starts_with("m-") && id.len() == 18 {
                    return Ok((id, None));
                }
                true
            }
            Err(ReadError::Missing) => false,
            Err(_) => true,
        };
        let mut seed = vec![0u8; 16];
        let random = fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut seed));
        if random.is_err() {
            let now = SystemTime::now().duration_since(UNIX_EPOCH);
            seed = format!("{now:?}-{}", std::process::id()).into_bytes();
        }
        let id = ledger::machine_id(&seed);
        let line = format!("{id}\n");
        if damaged {
            write_atomic(&file, line.as_bytes(), 0o600)?;
            let notice = "memory: the machine id could not be read and was made again";
            return Ok((id, Some(notice.to_owned())));
        }
        let created = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&file)
            .and_then(|mut f| f.write_all(line.as_bytes()));
        match created {
            Ok(()) => return Ok((id, None)),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("the machine id cannot be created"))
}

/// Creates `dir` for the owner only; a folder that already exists with wider
/// permissions is tightened, because it holds the private ledger.
fn private_dir(dir: &Path) -> io::Result<()> {
    DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    if fs::metadata(dir)?.permissions().mode() & 0o077 != 0 {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// An exclusive lock on `path`, held until the returned file is dropped, so
/// two lazarobox instances never interleave a ledger read-merge-write.
fn lock(path: &Path) -> io::Result<fs::File> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?;
    // SAFETY: the descriptor is open for as long as `file` lives. An
    // interrupted wait is an error, shown like any other failed save.
    match unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } {
        0 => Ok(file),
        _ => Err(io::Error::last_os_error()),
    }
}

/// Writes `bytes` to `path` without ever leaving it half written: a temp file
/// in the same folder, synced, then renamed over it.
fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_file_name(format!(".{name}.{}-{unique}.tmp", std::process::id()));
    let written = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&tmp)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        })
        .and_then(|()| fs::rename(&tmp, path));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

#[cfg(all(test, unix))]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt};

    use super::*;
    use crate::core::{
        ai::memory::local::{
            gitignore::GitignoreOutcome,
            ledger::LedgerError,
            note::{NewNote, Origin},
            port::{Failure, Root},
        },
        layout::PaneId,
    };

    const PANE: PaneId = PaneId::FIRST;

    /// Permissions do not bind root: the read-only tests skip there.
    fn is_root() -> bool {
        // SAFETY: geteuid has no preconditions and cannot fail.
        unsafe { libc::geteuid() == 0 }
    }

    /// A repository, its data folder and the helpers to drive the thread's
    /// `handle` the way the app does.
    struct Rig {
        _tmp: tempfile::TempDir,
        repo: PathBuf,
        data: PathBuf,
    }

    impl Rig {
        fn new() -> Rig {
            let tmp = tempfile::tempdir().unwrap();
            let repo = tmp.path().join("repo");
            fs::create_dir_all(repo.join(".git")).unwrap();
            fs::create_dir_all(repo.join("frontend")).unwrap();
            let data = tmp.path().join("data");
            fs::create_dir_all(&data).unwrap();
            Rig {
                _tmp: tmp,
                repo,
                data,
            }
        }

        /// A linked worktree of the repository, laid out like `git worktree
        /// add`: a `.git` file, and `commondir` and the backlink in its git
        /// folder.
        fn linked_worktree(&self) -> PathBuf {
            let gitdir = self.repo.join(".git/worktrees/wt");
            fs::create_dir_all(&gitdir).unwrap();
            let wt = self._tmp.path().join("wt");
            fs::create_dir_all(wt.join("a")).unwrap();
            fs::write(gitdir.join("commondir"), "../..\n").unwrap();
            let back = wt.join(".git");
            fs::write(gitdir.join("gitdir"), format!("{}\n", back.display())).unwrap();
            fs::write(&back, format!("gitdir: {}\n", gitdir.display())).unwrap();
            wt
        }

        fn ledger_file(&self) -> PathBuf {
            fs::read_dir(self.data.join("memory"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path()
        }

        fn notes_dir(&self) -> PathBuf {
            self.repo.join(".lzr/memory/notes")
        }

        fn load(&self, cwd: &Path) -> LocalReply {
            handle(
                Some(&self.data),
                None,
                LocalCmd::Load {
                    pane: PANE,
                    epoch: 3,
                    cwd: cwd.to_string_lossy().into_owned(),
                },
            )
        }

        fn root(&self) -> Root {
            Root {
                store: self.repo.clone(),
                area: ".".into(),
            }
        }

        fn save(&self, text: &str) -> LocalReply {
            let note = NewNote::new("decision", ".", text, Origin::User).unwrap();
            handle(
                Some(&self.data),
                None,
                LocalCmd::Save {
                    pane: PANE,
                    root: self.root(),
                    note,
                },
            )
        }

        fn counts(&self) -> (usize, usize, usize, usize) {
            match self.load(&self.repo) {
                LocalReply::Loaded {
                    notes,
                    foreign,
                    unreadable,
                    newer,
                    ..
                } => (notes.len(), foreign, unreadable, newer),
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn the_store_folder_exists_only_after_the_first_save() {
        let rig = Rig::new();
        assert_eq!(rig.counts(), (0, 0, 0, 0));
        assert!(!rig.repo.join(".lzr").exists());
        assert!(matches!(rig.save("use Axum"), LocalReply::Saved { .. }));
        assert!(rig.notes_dir().is_dir());
        assert_eq!(rig.counts(), (1, 0, 0, 0));
    }

    #[test]
    fn a_load_replies_with_the_root_the_area_and_the_epoch() {
        let rig = Rig::new();
        let LocalReply::Loaded {
            pane, epoch, root, ..
        } = rig.load(&rig.repo.join("frontend"))
        else {
            panic!("not loaded");
        };
        assert_eq!((pane, epoch), (PANE, 3));
        assert_eq!(
            root,
            Some(Root {
                store: rig.repo.clone(),
                area: "frontend".into()
            })
        );
    }

    #[test]
    fn a_folder_outside_git_loads_nothing_and_writes_nothing() {
        let rig = Rig::new();
        let plain = rig._tmp.path().join("plain");
        fs::create_dir_all(&plain).unwrap();
        let LocalReply::Loaded { root, notes, .. } = rig.load(&plain) else {
            panic!("not loaded");
        };
        assert!(root.is_none() && notes.is_empty());
        assert_eq!(fs::read_dir(&plain).unwrap().count(), 0);
        assert_eq!(fs::read_dir(&rig.data).unwrap().count(), 0);
    }

    #[test]
    fn invalid_files_are_skipped_counted_and_left_byte_identical() {
        let rig = Rig::new();
        for i in 0..8 {
            rig.save(&format!("note {i}"));
        }
        let bad = [
            ("a.json", "{ not json".to_owned()),
            (
                "b.json",
                "<<<<<<< HEAD\n{}\n=======\n>>>>>>> x\n".to_owned(),
            ),
        ];
        for (name, text) in &bad {
            fs::write(rig.notes_dir().join(name), text).unwrap();
        }
        assert_eq!(rig.counts(), (8, 0, 2, 0));
        for (name, text) in &bad {
            assert_eq!(
                &fs::read_to_string(rig.notes_dir().join(name)).unwrap(),
                text
            );
        }
    }

    #[test]
    fn a_huge_binary_file_is_skipped_without_being_read_whole() {
        let rig = Rig::new();
        rig.save("one");
        fs::write(rig.notes_dir().join("big.json"), vec![0u8; 5 * 1024 * 1024]).unwrap();
        assert_eq!(rig.counts(), (1, 0, 1, 0));
    }

    #[test]
    fn notes_made_by_a_newer_version_are_counted_apart_and_never_rewritten() {
        let rig = Rig::new();
        rig.save("one");
        let text = r#"{"v":2,"id":"n-new","kind":"config","text":"t"}"#;
        fs::write(rig.notes_dir().join("n-new.json"), text).unwrap();
        assert_eq!(rig.counts(), (1, 0, 0, 1));
        assert_eq!(
            fs::read_to_string(rig.notes_dir().join("n-new.json")).unwrap(),
            text
        );
    }

    #[test]
    fn a_leftover_partial_write_is_not_a_note_and_the_old_notes_stay() {
        let rig = Rig::new();
        rig.save("kept");
        fs::write(rig.notes_dir().join(".n-1.json.tmp"), "{\"v\":1,\"id\":").unwrap();
        assert_eq!(rig.counts(), (1, 0, 0, 0));
        rig.save("second");
        assert_eq!(rig.counts(), (2, 0, 0, 0));
    }

    #[test]
    fn a_note_added_by_hand_or_edited_outside_is_foreign() {
        let rig = Rig::new();
        rig.save("mine");
        rig.save("also mine");
        let hand = r#"{"v":1,"id":"n-handmade","kind":"config","text":"hi","machine":"m-x"}"#;
        fs::write(rig.notes_dir().join("n-handmade.json"), hand).unwrap();
        assert_eq!(rig.counts(), (2, 1, 0, 0));
        let first = fs::read_dir(rig.notes_dir())
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name().is_some_and(|n| {
                    n.to_string_lossy().starts_with("n-")
                        && !n.to_string_lossy().contains("handmade")
                })
            })
            .unwrap();
        let text = fs::read_to_string(&first).unwrap();
        fs::write(&first, text.replace("mine", "theirs")).unwrap();
        assert_eq!(rig.counts(), (1, 2, 0, 0));
    }

    #[test]
    fn two_writers_do_not_drop_each_others_ledger_entries() {
        let rig = Rig::new();
        rig.save("a");
        // A second instance wrote its own entry meanwhile.
        let ledger = fs::read_dir(rig.data.join("memory"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let mut text = fs::read_to_string(&ledger).unwrap();
        text = text.replace("\"own\": {", "\"own\": {\n    \"n-other\": \"h\",");
        fs::write(&ledger, text).unwrap();
        rig.save("b");
        assert!(fs::read_to_string(&ledger).unwrap().contains("n-other"));
        assert_eq!(rig.counts(), (2, 0, 0, 0));
    }

    #[test]
    fn the_machine_id_is_made_once_and_written_in_the_note() {
        let rig = Rig::new();
        rig.save("a");
        rig.save("b");
        let id = fs::read_to_string(rig.data.join("machine-id")).unwrap();
        assert!(id.trim().starts_with("m-") && id.trim().len() == 18);
        let LocalReply::Loaded { notes, .. } = rig.load(&rig.repo) else {
            panic!();
        };
        assert!(notes.iter().all(|n| n.machine == id.trim()));
    }

    #[test]
    fn the_gitignore_is_edited_only_when_the_folder_is_created() {
        let rig = Rig::new();
        let gitignore = |reply: LocalReply| match reply {
            LocalReply::Saved { gitignore, .. } => gitignore,
            other => panic!("{other:?}"),
        };
        assert_eq!(gitignore(rig.save("a")), Some(GitignoreOutcome::Created));
        assert_eq!(
            fs::read_to_string(rig.repo.join(".gitignore")).unwrap(),
            ".lzr/\n"
        );
        // The user shares the notes by removing the entry: it stays removed.
        fs::write(rig.repo.join(".gitignore"), "target/\n").unwrap();
        assert_eq!(gitignore(rig.save("b")), None);
        assert_eq!(
            fs::read_to_string(rig.repo.join(".gitignore")).unwrap(),
            "target/\n"
        );
    }

    #[test]
    fn a_read_only_gitignore_still_saves_the_note_and_says_so() {
        if is_root() {
            return;
        }
        let rig = Rig::new();
        let file = rig.repo.join(".gitignore");
        fs::write(&file, "target/\n").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o444)).unwrap();
        let LocalReply::Saved { gitignore, .. } = rig.save("a") else {
            panic!("not saved");
        };
        assert!(matches!(gitignore, Some(GitignoreOutcome::Failed(_))));
        assert_eq!(rig.counts(), (1, 0, 0, 0));
    }

    #[test]
    fn a_read_only_repo_writes_nothing_and_says_so() {
        if is_root() {
            return;
        }
        let rig = Rig::new();
        fs::set_permissions(&rig.repo, fs::Permissions::from_mode(0o555)).unwrap();
        let reply = rig.save("a");
        fs::set_permissions(&rig.repo, fs::Permissions::from_mode(0o755)).unwrap();
        let LocalReply::Failed { failure, .. } = reply else {
            panic!("{reply:?}");
        };
        assert_eq!(
            failure,
            Failure::ReadOnly(rig.repo.to_string_lossy().into_owned())
        );
        assert!(!rig.repo.join(".lzr").exists() && !rig.repo.join(".gitignore").exists());
    }

    #[test]
    fn a_read_only_store_still_reads_its_notes_and_keeps_them_intact() {
        if is_root() {
            return;
        }
        let rig = Rig::new();
        rig.save("kept");
        fs::set_permissions(rig.notes_dir(), fs::Permissions::from_mode(0o555)).unwrap();
        let reply = rig.save("refused");
        let counts = rig.counts();
        fs::set_permissions(rig.notes_dir(), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            matches!(
                reply,
                LocalReply::Failed {
                    failure: Failure::ReadOnly(_),
                    ..
                }
            ),
            "{reply:?}"
        );
        assert_eq!(counts, (1, 0, 0, 0));
    }

    #[test]
    fn without_a_data_folder_nothing_is_saved_and_the_chat_is_told() {
        let rig = Rig::new();
        let note = NewNote::new("decision", ".", "x", Origin::User).unwrap();
        let reply = handle(
            None,
            None,
            LocalCmd::Save {
                pane: PANE,
                root: rig.root(),
                note,
            },
        );
        assert!(matches!(
            reply,
            LocalReply::Failed {
                failure: Failure::Io(_),
                ..
            }
        ));
        assert!(!rig.repo.join(".lzr").exists());
    }

    #[test]
    fn a_linked_worktree_reads_and_writes_the_main_checkouts_store() {
        let rig = Rig::new();
        let wt = rig.linked_worktree();
        rig.save("shared");
        let LocalReply::Loaded { root, notes, .. } = rig.load(&wt.join("a")) else {
            panic!();
        };
        assert_eq!(
            root,
            Some(Root {
                store: rig.repo.clone(),
                area: "a".into()
            })
        );
        assert_eq!(notes.len(), 1);
        assert!(!wt.join(".lzr").exists());
    }

    // M2: the ledger key is the canonical store root, so a worktree, a
    // symlinked path and the checkout itself share one ledger.
    #[test]
    fn a_worktree_and_a_symlinked_path_use_the_same_ledger() {
        let rig = Rig::new();
        let wt = rig.linked_worktree();
        rig.save("one");
        let link = rig._tmp.path().join("link");
        std::os::unix::fs::symlink(&rig.repo, &link).unwrap();
        for cwd in [wt.join("a"), link] {
            let LocalReply::Loaded { notes, foreign, .. } = rig.load(&cwd) else {
                panic!();
            };
            assert_eq!((notes.len(), foreign), (1, 0), "{cwd:?}");
        }
        assert_eq!(fs::read_dir(rig.data.join("memory")).unwrap().count(), 1);
    }

    // M3: a repository at HOME is never a project.
    #[test]
    fn a_repository_at_home_stores_nothing() {
        let rig = Rig::new();
        let reply = handle(
            Some(&rig.data),
            Some(&rig.repo),
            LocalCmd::Load {
                pane: PANE,
                epoch: 0,
                cwd: rig.repo.to_string_lossy().into_owned(),
            },
        );
        assert!(
            matches!(reply, LocalReply::Loaded { root: None, .. }),
            "{reply:?}"
        );
        assert!(!rig.repo.join(".lzr").exists());
    }

    // A damaged ledger is said out loud, trusts nothing and is never written
    // over: a later save would wipe every claim the machine had.
    #[test]
    fn a_damaged_ledger_is_reported_kept_and_blocks_saves() {
        for (text, want) in [
            ("{ broken", LedgerError::Damaged),
            (r#"{"v":2,"own":{}}"#, LedgerError::Newer),
        ] {
            let rig = Rig::new();
            rig.save("one");
            let file = rig.ledger_file();
            fs::write(&file, text).unwrap();
            let LocalReply::Loaded {
                notes,
                foreign,
                ledger,
                ..
            } = rig.load(&rig.repo)
            else {
                panic!();
            };
            assert_eq!((notes.len(), foreign, ledger), (0, 1, Some(want)), "{text}");
            let reply = rig.save("two");
            let LocalReply::Failed {
                failure: Failure::Io(reason),
                ..
            } = reply
            else {
                panic!("{reply:?}");
            };
            assert!(reason.contains("ledger"), "{reason}");
            assert_eq!(fs::read_to_string(&file).unwrap(), text, "never rewritten");
            assert_eq!(
                fs::read_dir(rig.notes_dir()).unwrap().count(),
                1,
                "no note written"
            );
        }
    }

    // Lossy decoding would turn bad bytes inside a string into a valid ledger.
    #[test]
    fn a_ledger_that_is_not_utf8_is_damaged() {
        let rig = Rig::new();
        rig.save("one");
        let file = rig.ledger_file();
        let bytes = b"{\"v\":1,\"own\":{\"n-1\":\"\xff\"}}".to_vec();
        fs::write(&file, &bytes).unwrap();
        let LocalReply::Loaded { ledger, .. } = rig.load(&rig.repo) else {
            panic!();
        };
        assert_eq!(ledger, Some(LedgerError::Damaged));
        assert!(matches!(rig.save("two"), LocalReply::Failed { .. }));
        assert_eq!(fs::read(&file).unwrap(), bytes, "never rewritten");
    }

    #[tokio::test]
    async fn a_thread_that_cannot_start_answers_every_command_with_a_failure() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        let memory = spawn_with(None, None, tx, |_| {
            Err(io::Error::new(ErrorKind::WouldBlock, "no threads left"))
        });
        memory.send(LocalCmd::Load {
            pane: PANE,
            epoch: 0,
            cwd: "/x".into(),
        });
        let Some(LocalReply::Failed { pane, failure }) = rx.recv().await else {
            panic!("no failure reply");
        };
        assert_eq!(pane, PANE);
        assert!(
            matches!(failure, Failure::Unavailable(reason) if reason.contains("no threads left"))
        );
    }

    #[tokio::test]
    async fn the_thread_answers_on_the_reply_channel() {
        let rig = Rig::new();
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        let memory = spawn(Some(rig.data.clone()), None, tx);
        memory.send(LocalCmd::Load {
            pane: PANE,
            epoch: 0,
            cwd: rig.repo.to_string_lossy().into_owned(),
        });
        let reply = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await;
        assert!(
            matches!(reply, Ok(Some(LocalReply::Loaded { .. }))),
            "{reply:?}"
        );
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn the_private_files_and_folders_are_owner_only() {
        let rig = Rig::new();
        fs::set_permissions(&rig.data, fs::Permissions::from_mode(0o755)).unwrap();
        fs::create_dir_all(rig.data.join("memory")).unwrap();
        fs::set_permissions(rig.data.join("memory"), fs::Permissions::from_mode(0o777)).unwrap();
        assert!(matches!(rig.save("one"), LocalReply::Saved { .. }));
        assert_eq!(mode(&rig.data), 0o700, "a wider data folder is tightened");
        assert_eq!(mode(&rig.data.join("memory")), 0o700);
        assert_eq!(mode(&rig.data.join("machine-id")), 0o600);
        assert_eq!(mode(&rig.ledger_file()), 0o600);
    }

    #[test]
    fn new_private_folders_are_owner_only_under_the_current_umask() {
        let rig = Rig::new();
        fs::remove_dir_all(&rig.data).unwrap();
        let saved = rig.save("one");
        assert!(matches!(saved, LocalReply::Saved { .. }), "{saved:?}");
        assert_eq!(mode(&rig.data.join("memory")), 0o700);
        assert_eq!(mode(&rig.data.join("machine-id")), 0o600);
    }

    #[test]
    fn a_machine_id_that_cannot_be_made_fails_instead_of_looping() {
        let rig = Rig::new();
        std::os::unix::fs::symlink(rig._tmp.path().join("nowhere"), rig.data.join("machine-id"))
            .unwrap();
        let LocalReply::Failed { failure, .. } = rig.save("one") else {
            panic!("saved without a machine id");
        };
        assert!(matches!(failure, Failure::Io(why) if why.contains("machine id")));
    }

    #[test]
    fn concurrent_saves_keep_every_claim_and_one_machine_id() {
        let rig = std::sync::Arc::new(Rig::new());
        let handles: Vec<_> = (0..8)
            .map(|n| {
                let rig = rig.clone();
                std::thread::spawn(move || rig.save(&format!("note number {n}")))
            })
            .collect();
        for handle in handles {
            assert!(matches!(handle.join().unwrap(), LocalReply::Saved { .. }));
        }
        assert_eq!(rig.counts(), (8, 0, 0, 0), "no claim was lost");
        let machine = fs::read_to_string(rig.data.join("machine-id")).unwrap();
        for entry in fs::read_dir(rig.notes_dir()).unwrap() {
            let text = fs::read_to_string(entry.unwrap().path()).unwrap();
            assert!(
                text.contains(machine.trim()),
                "one machine id in every note"
            );
        }
    }

    #[test]
    fn an_unreadable_machine_id_is_made_again_and_said() {
        let rig = Rig::new();
        fs::write(rig.data.join("machine-id"), "garbage").unwrap();
        let LocalReply::Saved { notice, .. } = rig.save("one") else {
            panic!("not saved");
        };
        let notice = notice.expect("the new machine id is said");
        assert!(notice.contains("machine id"), "{notice}");
        let again = fs::read_to_string(rig.data.join("machine-id")).unwrap();
        assert!(again.starts_with("m-"), "{again}");
        let LocalReply::Saved { notice, .. } = rig.save("two") else {
            panic!("not saved");
        };
        assert_eq!(notice, None, "said once");
    }

    #[test]
    fn a_linked_store_folder_is_refused_before_anything_is_written() {
        let rig = Rig::new();
        let outside = rig._tmp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, rig.repo.join(".lzr")).unwrap();
        let LocalReply::Failed { failure, .. } = rig.save("one") else {
            panic!("saved through a link");
        };
        assert!(matches!(failure, Failure::Io(why) if why.contains("link")));
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        let LocalReply::Loaded {
            notes, unreadable, ..
        } = rig.load(&rig.repo)
        else {
            panic!("not loaded");
        };
        assert_eq!(
            (notes.len(), unreadable),
            (0, 1),
            "a linked folder is not read"
        );
    }

    #[test]
    fn a_linked_note_file_is_not_read() {
        let rig = Rig::new();
        assert!(matches!(rig.save("one"), LocalReply::Saved { .. }));
        let file = fs::read_dir(rig.notes_dir())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let real = rig._tmp.path().join("real.json");
        fs::rename(&file, &real).unwrap();
        std::os::unix::fs::symlink(&real, &file).unwrap();
        assert_eq!(rig.counts(), (0, 0, 1, 0));
    }

    #[test]
    fn a_command_that_panics_gets_a_failure_and_the_thread_goes_on() {
        let (tx, rx) = mpsc::channel();
        let (replies, mut out) = tokio::sync::mpsc::channel(4);
        let load = || LocalCmd::Load {
            pane: PANE,
            epoch: 0,
            cwd: "/x".into(),
        };
        tx.send(load()).unwrap();
        tx.send(load()).unwrap();
        drop(tx);
        let mut first = true;
        serve(rx, replies, |cmd| {
            if std::mem::take(&mut first) {
                panic!("boom");
            }
            LocalReply::Failed {
                pane: cmd.pane(),
                failure: Failure::Io("second".into()),
            }
        });
        let Some(LocalReply::Failed { failure, .. }) = out.blocking_recv() else {
            panic!("no reply for the command that panicked");
        };
        assert!(matches!(failure, Failure::Unavailable(_)), "{failure:?}");
        assert!(matches!(
            out.blocking_recv(),
            Some(LocalReply::Failed {
                failure: Failure::Io(_),
                ..
            })
        ));
    }

    #[test]
    fn a_failure_is_not_dropped_when_the_reply_channel_is_full() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let filler = LocalReply::Failed {
            pane: PANE,
            failure: Failure::Io("filler".into()),
        };
        tx.try_send(filler).unwrap();
        let memory = spawn_with(None, None, tx, |_| {
            Err(io::Error::new(ErrorKind::WouldBlock, "no threads left"))
        });
        memory.send(LocalCmd::Load {
            pane: PANE,
            epoch: 0,
            cwd: "/x".into(),
        });
        let _ = rx.blocking_recv();
        let late = rx.blocking_recv();
        assert!(matches!(
            late,
            Some(LocalReply::Failed {
                failure: Failure::Unavailable(_),
                ..
            })
        ));
    }
}
