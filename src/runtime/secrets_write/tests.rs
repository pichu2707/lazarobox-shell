use std::{
    cell::{Cell, RefCell},
    fs,
    os::{
        fd::OwnedFd,
        unix::fs::{MetadataExt, PermissionsExt, symlink},
    },
    path::PathBuf,
};

use super::*;

const OLD: &str =
    "# mine\n[ga4]\nGOOGLE_CLOUD_PROJECT = \"p1\" # mine\n\n[clarity]\nCLARITY_API_TOKEN = \"a\"\n";

/// A temp config folder, never the user's own, with the seam hooks.
struct Lab {
    root: tempfile::TempDir,
    dir: PathBuf,
}

fn lab() -> Lab {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("lazarobox");
    fs::create_dir(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    Lab { root, dir }
}

impl Lab {
    fn path(&self) -> PathBuf {
        self.dir.join("secrets.toml")
    }

    fn bak(&self) -> PathBuf {
        self.dir.join("secrets.toml.bak")
    }

    fn write(&self, text: &str, mode: u32) {
        let path = self.path();
        fs::write(&path, text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
    }

    fn mode(&self, path: &Path) -> u32 {
        fs::metadata(path).unwrap().mode() & 0o7777
    }

    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn save(&self, value: &str) -> Result<Saved, Refusal> {
        save_with(
            &TestFs::default(),
            Some(&self.path()),
            "clarity",
            "CLARITY_API_TOKEN",
            value,
        )
    }

    fn save_with_fs(&self, fs: &TestFs) -> Result<Saved, Refusal> {
        save_with(fs, Some(&self.path()), "clarity", "CLARITY_API_TOKEN", "b")
    }

    fn notice(&self, result: Result<Saved, Refusal>) -> String {
        result.unwrap_err().notice()
    }

    /// Nothing but `secrets.toml` with these bytes and this mode is left.
    fn assert_untouched(&self, bytes: &str, mode: u32) {
        assert_eq!(fs::read_to_string(self.path()).unwrap(), bytes);
        assert_eq!(self.mode(&self.path()), mode);
        assert_eq!(self.names(), ["secrets.toml"]);
    }
}

/// The real I/O with faults: the n-th call (from 0) of an operation fails, the
/// uid is another one, the temporary names are fixed, and a hook runs before
/// the first `stat_at`.
#[derive(Default)]
struct TestFs {
    fail: Option<(&'static str, usize)>,
    uid: Option<u32>,
    file_owner: Option<u32>,
    names: RefCell<Vec<u64>>,
    calls: RefCell<Vec<&'static str>>,
    /// Runs before the n-th (from 0) `stat_at` of the file.
    on_recheck: Option<(usize, Box<dyn Fn()>)>,
    rechecks: Cell<usize>,
    /// A strict umask: new files come out 0400.
    strict_umask: bool,
}

impl TestFs {
    fn failing(op: &'static str, nth: usize) -> Self {
        Self {
            fail: Some((op, nth)),
            ..Self::default()
        }
    }

    fn step(&self, op: &'static str) -> io::Result<()> {
        let nth = self.calls.borrow().iter().filter(|c| **c == op).count();
        self.calls.borrow_mut().push(op);
        match self.fail {
            Some((failing, at)) if failing == op && at == nth => Err(io::Error::other("injected")),
            _ => Ok(()),
        }
    }
}

impl SaveFs for TestFs {
    fn euid(&self) -> u32 {
        self.uid.unwrap_or_else(|| RealFs.euid())
    }
    fn random(&self) -> u64 {
        let mut names = self.names.borrow_mut();
        if names.is_empty() {
            RealFs.random()
        } else {
            names.remove(0)
        }
    }
    fn open_dir(&self, path: &Path) -> io::Result<OwnedFd> {
        RealFs.open_dir(path)
    }
    fn fstat(&self, fd: &OwnedFd) -> io::Result<Stat> {
        let mut stat = RealFs.fstat(fd)?;
        if let Some(owner) = self.file_owner.filter(|_| stat.regular) {
            stat.uid = owner;
        }
        Ok(stat)
    }
    fn open_read(&self, dir: &OwnedFd, name: &str) -> io::Result<OwnedFd> {
        self.step("open_read")?;
        RealFs.open_read(dir, name)
    }
    fn read(&self, fd: &OwnedFd, max: u64) -> io::Result<Vec<u8>> {
        RealFs.read(fd, max)
    }
    fn stat_at(&self, dir: &OwnedFd, name: &str) -> io::Result<Stat> {
        if name == "secrets.toml" {
            let nth = self.rechecks.replace(self.rechecks.get() + 1);
            let due = self.on_recheck.as_ref().filter(|(at, _)| *at == nth);
            if let Some((_, hook)) = due {
                hook();
            }
        }
        RealFs.stat_at(dir, name)
    }
    fn create_excl(&self, dir: &OwnedFd, name: &str) -> io::Result<OwnedFd> {
        let fd = RealFs.create_excl(dir, name)?;
        if self.strict_umask {
            use std::os::fd::AsRawFd;
            assert_eq!(unsafe { libc::fchmod(fd.as_raw_fd(), 0o400) }, 0);
        }
        Ok(fd)
    }
    fn private(&self, fd: &OwnedFd) -> io::Result<()> {
        RealFs.private(fd)
    }
    fn write_all(&self, fd: &OwnedFd, bytes: &[u8]) -> io::Result<()> {
        self.step("write")?;
        RealFs.write_all(fd, bytes)
    }
    fn fsync(&self, fd: &OwnedFd) -> io::Result<()> {
        self.step("fsync")?;
        RealFs.fsync(fd)
    }
    fn rename(&self, dir: &OwnedFd, from: &str, to: &str) -> io::Result<()> {
        self.step("rename")?;
        RealFs.rename(dir, from, to)
    }
    fn unlink(&self, dir: &OwnedFd, name: &str) -> io::Result<()> {
        RealFs.unlink(dir, name)
    }
}

// Spec Menu save: Only the value changes (and the backup).
#[test]
fn only_the_value_changes_and_the_old_bytes_go_to_the_backup() {
    let lab = lab();
    lab.write(OLD, 0o600);
    let saved = lab.save("b").unwrap();
    assert_eq!(
        saved.notice(),
        "saved CLARITY_API_TOKEN; the previous values stay in secrets.toml.bak (0600) until you delete it"
    );
    assert_eq!(
        fs::read_to_string(lab.path()).unwrap(),
        OLD.replace("\"a\"", "\"b\"")
    );
    assert_eq!(lab.mode(&lab.path()), 0o600);
    assert_eq!(fs::read_to_string(lab.bak()).unwrap(), OLD);
    assert_eq!(lab.mode(&lab.bak()), 0o600);
    assert_eq!(lab.names(), ["secrets.toml", "secrets.toml.bak"]);
}

// Spec Menu save: Missing file is created 0600.
#[test]
fn a_missing_file_is_created_0600_without_a_backup() {
    let lab = lab();
    let saved = lab.save("t").unwrap();
    assert_eq!(saved.notice(), "saved CLARITY_API_TOKEN");
    assert_eq!(
        fs::read_to_string(lab.path()).unwrap(),
        "[clarity]\nCLARITY_API_TOKEN = \"t\"\n"
    );
    assert_eq!(lab.mode(&lab.path()), 0o600);
    assert_eq!(lab.names(), ["secrets.toml"]);
}

#[test]
fn a_missing_folder_is_created_0700() {
    let lab = lab();
    let path = lab
        .root
        .path()
        .join("new")
        .join("lazarobox")
        .join("secrets.toml");
    save_with(
        &TestFs::default(),
        Some(&path),
        "clarity",
        "CLARITY_API_TOKEN",
        "t",
    )
    .unwrap();
    assert_eq!(lab.mode(path.parent().unwrap()), 0o700);
    assert_eq!(lab.mode(&path), 0o600);
}

#[test]
fn no_config_folder_writes_nothing() {
    let notice = save_with(
        &TestFs::default(),
        None,
        "clarity",
        "CLARITY_API_TOKEN",
        "t",
    )
    .unwrap_err()
    .notice();
    assert_eq!(notice, "secrets.toml: no config folder");
    assert_eq!(
        delete_with(&TestFs::default(), None).unwrap_err().notice(),
        "secrets.toml: no config folder"
    );
}

// Spec Menu save: A name the entry does not list (nothing is created).
#[test]
fn a_name_the_entry_does_not_list_creates_nothing() {
    let lab = lab();
    let path = lab.root.path().join("never").join("secrets.toml");
    let notice = save_with(&TestFs::default(), Some(&path), "clarity", "PATH", "v")
        .unwrap_err()
        .notice();
    assert_eq!(
        notice,
        "secrets.toml: clarity.PATH is not a value of this connector"
    );
    assert!(!path.parent().unwrap().exists());
}

// Spec Menu save: Symlinked config folder refused.
#[test]
fn a_symlinked_config_folder_is_refused() {
    let lab = lab();
    let real = lab.root.path().join("elsewhere");
    fs::create_dir(&real).unwrap();
    fs::set_permissions(&real, fs::Permissions::from_mode(0o700)).unwrap();
    let link = lab.root.path().join("link");
    symlink(&real, &link).unwrap();
    let notice = save_with(
        &TestFs::default(),
        Some(&link.join("secrets.toml")),
        "clarity",
        "CLARITY_API_TOKEN",
        "t",
    )
    .unwrap_err()
    .notice();
    assert_eq!(
        notice,
        "the config folder is a symlink; edit secrets.toml by hand"
    );
    assert_eq!(fs::read_dir(&real).unwrap().count(), 0);
}

// Spec Menu save: Group-writable config folder (umask 002).
#[test]
fn a_config_folder_others_can_write_is_refused() {
    let lab = lab();
    lab.write(OLD, 0o600);
    for mode in [0o775, 0o777, 0o770, 0o722] {
        fs::set_permissions(&lab.dir, fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(
            lab.notice(lab.save("b")),
            format!(
                "the config folder is writable by group or others; run chmod 700 {}",
                lab.dir.display()
            )
        );
        assert_eq!(fs::read_to_string(lab.path()).unwrap(), OLD);
        assert_eq!(lab.names(), ["secrets.toml"]);
    }
}

#[test]
fn a_config_folder_of_another_owner_is_refused() {
    let lab = lab();
    let fs = TestFs {
        uid: Some(RealFs.euid() + 1),
        ..TestFs::default()
    };
    assert_eq!(
        lab.notice(lab.save_with_fs(&fs)),
        "the config folder is not owned by you; edit secrets.toml by hand"
    );
}

// Spec Menu save: Symlink refused.
#[test]
fn a_symlinked_file_is_refused_and_nothing_changes() {
    let lab = lab();
    let target = lab.root.path().join("target.toml");
    fs::write(&target, OLD).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    symlink(&target, lab.path()).unwrap();
    assert_eq!(
        lab.notice(lab.save("b")),
        "secrets.toml is a symlink; edit it by hand"
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), OLD);
    assert!(fs::symlink_metadata(lab.path()).unwrap().is_symlink());
    assert_eq!(lab.names(), ["secrets.toml"]);
}

// Spec Menu save: Loose mode refused, Read-only file refused.
#[test]
fn a_mode_other_than_0600_is_refused_with_the_file_untouched() {
    let lab = lab();
    for mode in [0o644, 0o640, 0o660, 0o700, 0o604, 0o755] {
        lab.write(OLD, mode);
        assert_eq!(
            lab.notice(lab.save("b")),
            format!(
                "secrets.toml: mode must be 0600 (chmod 600 {})",
                lab.path().display()
            ),
            "{mode:o}"
        );
        lab.assert_untouched(OLD, mode);
    }
    lab.write(OLD, 0o400);
    assert_eq!(
        lab.notice(lab.save("b")),
        format!(
            "secrets.toml is read-only (mode 0400); chmod 600 {} to save from the menu",
            lab.path().display()
        )
    );
    lab.assert_untouched(OLD, 0o400);
}

// Spec Menu save: Foreign owner refused (through the seam).
#[test]
fn a_file_of_another_owner_is_refused() {
    let lab = lab();
    lab.write(OLD, 0o600);
    let fs = TestFs {
        file_owner: Some(RealFs.euid() + 1),
        ..TestFs::default()
    };
    assert_eq!(
        lab.notice(lab.save_with_fs(&fs)),
        "secrets.toml is not owned by you; edit it by hand"
    );
    lab.assert_untouched(OLD, 0o600);
}

// Spec Menu save: Directory in its place refused.
#[test]
fn a_directory_in_its_place_is_refused_and_creates_nothing() {
    let lab = lab();
    fs::create_dir(lab.path()).unwrap();
    assert_eq!(
        lab.notice(lab.save("b")),
        "secrets.toml: not a regular file"
    );
    assert_eq!(lab.names(), ["secrets.toml"]);
    assert!(lab.path().is_dir());
}

// Spec Menu save: FIFO in its place.
#[test]
fn a_fifo_in_its_place_returns_at_once() {
    let lab = lab();
    let c = CString::new(lab.path().to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    assert_eq!(
        lab.notice(lab.save("b")),
        "secrets.toml: not a regular file"
    );
    assert_eq!(lab.names(), ["secrets.toml"]);
}

#[test]
fn a_file_over_64_kib_or_that_cannot_be_read_is_refused() {
    let lab = lab();
    lab.write(&"#".repeat(65 * 1024), 0o600);
    assert_eq!(
        lab.notice(lab.save("b")),
        "secrets.toml: larger than 64 KiB"
    );
    lab.write(OLD, 0o600);
    let fs = TestFs::failing("open_read", 0);
    assert_eq!(
        lab.notice(lab.save_with_fs(&fs)),
        "secrets.toml: cannot be read"
    );
    lab.assert_untouched(OLD, 0o600);
}

// Spec Menu save: Unparseable file never overwritten.
#[test]
fn an_unparseable_file_is_never_overwritten_and_the_backup_stays() {
    let lab = lab();
    let bad = "[clarity]\n\nCLARITY_API_TOKEN = \"sk-123\n";
    lab.write(bad, 0o600);
    fs::write(lab.bak(), "X").unwrap();
    let notice = lab.notice(lab.save("b"));
    assert_eq!(
        notice,
        "secrets.toml: not valid TOML (line 3); fix it by hand, nothing was saved"
    );
    assert!(!notice.contains("sk-123"));
    assert_eq!(fs::read_to_string(lab.path()).unwrap(), bad);
    assert_eq!(fs::read_to_string(lab.bak()).unwrap(), "X");
    // Bytes that are not UTF-8 are refused the same way.
    fs::write(lab.path(), b"[clarity]\n\xff\n").unwrap();
    assert_eq!(
        lab.notice(lab.save("b")),
        "secrets.toml: not valid TOML (line 2); fix it by hand, nothing was saved"
    );
}

// Spec Menu save: A refused save keeps the backup.
#[test]
fn a_refused_save_keeps_the_backup() {
    let lab = lab();
    lab.write(OLD, 0o644);
    fs::write(lab.bak(), "X").unwrap();
    assert!(lab.save("b").is_err());
    assert_eq!(fs::read_to_string(lab.bak()).unwrap(), "X");
}

// Spec Menu save: Pre-planted symlink at the temporary name.
#[test]
fn a_symlink_at_the_temporary_name_is_never_followed() {
    let lab = lab();
    lab.write(OLD, 0o600);
    let outside = lab.root.path().join("outside");
    fs::write(&outside, "keep").unwrap();
    let planted = lab.dir.join(format!(
        ".secrets.toml.{}.{:016x}.tmp",
        std::process::id(),
        1
    ));
    symlink(&outside, &planted).unwrap();
    let fs = TestFs {
        names: RefCell::new(vec![1, 2, 3]),
        ..TestFs::default()
    };
    lab.save_with_fs(&fs).unwrap();
    assert_eq!(fs::read_to_string(&outside).unwrap(), "keep");
    assert!(fs::symlink_metadata(&planted).unwrap().is_symlink());
    assert_eq!(lab.mode(&lab.path()), 0o600);
    assert!(fs::read_to_string(lab.path()).unwrap().contains("\"b\""));
}

#[test]
fn three_taken_temporary_names_refuse_the_save() {
    let lab = lab();
    lab.write(OLD, 0o600);
    for n in 1..=3u64 {
        let name = format!(".secrets.toml.{}.{n:016x}.tmp", std::process::id());
        fs::write(lab.dir.join(name), "mine").unwrap();
    }
    let fs = TestFs {
        names: RefCell::new(vec![1, 2, 3]),
        ..TestFs::default()
    };
    assert_eq!(
        lab.notice(lab.save_with_fs(&fs)),
        "secrets.toml: not saved (temporary file exists)"
    );
    assert_eq!(fs::read_to_string(lab.path()).unwrap(), OLD);
    // The names that were taken are still there, untouched.
    assert_eq!(lab.names().len(), 4);
}

// Spec Menu save: Changed on disk.
#[test]
fn a_file_changed_before_the_rename_is_not_overwritten() {
    let lab = lab();
    lab.write(OLD, 0o600);
    fs::write(lab.bak(), "X").unwrap();
    let path = lab.path();
    let fs = TestFs {
        on_recheck: Some((
            0,
            Box::new(move || {
                fs::write(&path, "[clarity]\nCLARITY_API_TOKEN = \"hand\"\n").unwrap();
            }),
        )),
        ..TestFs::default()
    };
    assert_eq!(
        lab.notice(lab.save_with_fs(&fs)),
        "secrets.toml changed on disk; nothing was saved, try again"
    );
    assert_eq!(
        fs::read_to_string(lab.path()).unwrap(),
        "[clarity]\nCLARITY_API_TOKEN = \"hand\"\n"
    );
    assert_eq!(fs::read_to_string(lab.bak()).unwrap(), "X");
    assert_eq!(lab.names(), ["secrets.toml", "secrets.toml.bak"]);
}

// Spec Menu save: Failed write leaves the original.
#[test]
fn a_failed_step_leaves_the_original_and_no_temporary_file() {
    for (op, nth) in [
        ("write", 0),
        ("fsync", 0),
        ("rename", 0),
        ("write", 1),
        ("rename", 1),
    ] {
        let lab = lab();
        lab.write(OLD, 0o600);
        let notice = lab.notice(lab.save_with_fs(&TestFs::failing(op, nth)));
        assert!(
            notice.starts_with("secrets.toml: not saved ("),
            "{op} {nth}: {notice}"
        );
        assert_eq!(fs::read_to_string(lab.path()).unwrap(), OLD, "{op} {nth}");
        assert_eq!(lab.mode(&lab.path()), 0o600);
        let names = lab.names();
        assert!(
            !names.iter().any(|n| n.ends_with(".tmp")),
            "{op} {nth}: {names:?}"
        );
    }
    // A failure before the backup leaves no backup at all; a missing file
    // fails the same way.
    let lab = lab();
    assert!(lab.save_with_fs(&TestFs::failing("rename", 0)).is_err());
    assert_eq!(lab.names(), Vec::<String>::new());
}

#[test]
fn a_backup_that_is_a_link_refuses_the_save_and_is_left() {
    let lab = lab();
    lab.write(OLD, 0o600);
    let target = lab.root.path().join("target");
    fs::write(&target, "keep").unwrap();
    symlink(&target, lab.bak()).unwrap();
    assert_eq!(
        lab.notice(lab.save("b")),
        "secrets.toml.bak is not a regular file of yours; left as it is"
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "keep");
    assert_eq!(fs::read_to_string(lab.path()).unwrap(), OLD);
    assert!(fs::symlink_metadata(lab.bak()).unwrap().is_symlink());
}

// Spec Menu save: Delete the backup.
#[test]
fn delete_backup_removes_a_regular_file_and_refuses_a_link() {
    let lab = lab();
    assert_eq!(
        delete_with(&TestFs::default(), Some(&lab.path()))
            .unwrap_err()
            .notice(),
        "secrets.toml.bak: there is no backup"
    );
    fs::write(lab.bak(), "X").unwrap();
    delete_with(&TestFs::default(), Some(&lab.path())).unwrap();
    assert!(!lab.bak().exists());
    let target = lab.root.path().join("target");
    fs::write(&target, "keep").unwrap();
    symlink(&target, lab.bak()).unwrap();
    assert_eq!(
        delete_with(&TestFs::default(), Some(&lab.path()))
            .unwrap_err()
            .notice(),
        "secrets.toml.bak is not a regular file of yours; left as it is"
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "keep");
    assert!(fs::symlink_metadata(lab.bak()).unwrap().is_symlink());
    // Another owner's file is left too.
    fs::remove_file(lab.bak()).unwrap();
    fs::write(lab.bak(), "X").unwrap();
    let other = TestFs {
        uid: Some(RealFs.euid() + 1),
        ..TestFs::default()
    };
    assert!(delete_with(&other, Some(&lab.path())).is_err());
    assert!(lab.bak().exists());
}

// Review: the second re-check, after the backup and right before the rename.
#[test]
fn a_file_changed_after_the_backup_is_not_overwritten() {
    let lab = lab();
    lab.write(OLD, 0o600);
    let path = lab.path();
    let fs = TestFs {
        on_recheck: Some((
            1,
            Box::new(move || {
                fs::write(&path, "[clarity]\nCLARITY_API_TOKEN = \"hand\"\n").unwrap();
            }),
        )),
        ..TestFs::default()
    };
    assert_eq!(
        lab.notice(lab.save_with_fs(&fs)),
        "secrets.toml changed on disk; nothing was saved, try again"
    );
    assert_eq!(
        fs::read_to_string(lab.path()).unwrap(),
        "[clarity]\nCLARITY_API_TOKEN = \"hand\"\n"
    );
    // The backup is what the save read; no temporary file is left.
    assert_eq!(fs::read_to_string(lab.bak()).unwrap(), OLD);
    assert_eq!(lab.names(), ["secrets.toml", "secrets.toml.bak"]);
}

// Review: a strict umask (0277) never yields a 0400 file or backup.
#[test]
fn a_strict_umask_still_gives_0600() {
    let lab = lab();
    lab.write(OLD, 0o600);
    let fs = TestFs {
        strict_umask: true,
        ..TestFs::default()
    };
    lab.save_with_fs(&fs).unwrap();
    assert_eq!(lab.mode(&lab.path()), 0o600);
    assert_eq!(lab.mode(&lab.bak()), 0o600);
    // A new file as well.
    fs::remove_file(lab.path()).unwrap();
    fs::remove_file(lab.bak()).unwrap();
    lab.save_with_fs(&fs).unwrap();
    assert_eq!(lab.mode(&lab.path()), 0o600);
}

// Review: a file with other hard links is refused.
#[test]
fn a_file_with_other_hard_links_is_refused() {
    let lab = lab();
    lab.write(OLD, 0o600);
    fs::hard_link(lab.path(), lab.root.path().join("other")).unwrap();
    assert_eq!(
        lab.notice(lab.save("b")),
        "secrets.toml has other hard links; edit it by hand"
    );
    assert_eq!(fs::read_to_string(lab.path()).unwrap(), OLD);
    assert_eq!(lab.names(), ["secrets.toml"]);
}

// Review: a result over 64 KiB is never written (the pure edit refuses it
// first, and the writer checks again before the temp file).
#[test]
fn a_result_over_64_kib_writes_nothing() {
    let lab = lab();
    lab.write(&format!("# {}\n", "c".repeat(60 * 1024)), 0o600);
    assert_eq!(
        lab.notice(lab.save(&"x".repeat(8 * 1024))),
        "secrets.toml: larger than 64 KiB"
    );
    assert_eq!(lab.names(), ["secrets.toml"]);
}
