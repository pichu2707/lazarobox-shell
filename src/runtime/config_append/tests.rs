use std::{
    cell::RefCell,
    fs,
    os::{
        fd::OwnedFd,
        unix::fs::{MetadataExt, PermissionsExt, symlink},
    },
    path::PathBuf,
};

use super::*;
use crate::{
    core::{
        ai::{
            mcp_catalog::{entry, snippet},
            mcp_secrets::SecretText,
        },
        config_append::digest,
    },
    runtime::secrets_write::{RealFs, Stat},
};

const OLD: &str = "# mine\r\nmouse = false # keep\r\n";
const SECRETS: &str = "# values\n[clarity]\nCLARITY_API_TOKEN = \"a\"\n";
const NAMES: [&str; 1] = ["gsc"];

fn snippets() -> String {
    snippet(entry("gsc").unwrap())
}

/// A temp config folder, never the user's own.
struct Lab {
    _root: tempfile::TempDir,
    dir: PathBuf,
}

fn lab() -> Lab {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("lazarobox");
    fs::create_dir(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    Lab { _root: root, dir }
}

impl Lab {
    fn config(&self) -> PathBuf {
        self.dir.join("config.toml")
    }

    fn secrets(&self) -> PathBuf {
        self.dir.join("secrets.toml")
    }

    fn put(&self, path: &Path, text: &str, mode: u32) {
        fs::write(path, text).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    fn mode(&self, path: &Path) -> u32 {
        fs::metadata(path).unwrap().mode() & 0o7777
    }

    fn read(&self, name: &str) -> String {
        fs::read_to_string(self.dir.join(name)).unwrap()
    }

    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// The append, with the plan shown on `shown` (`None`: no file).
    fn append(&self, shown: Option<&str>) -> Result<bool, AppendRefusal> {
        self.append_fs(&RealFs, shown)
    }

    fn append_fs(&self, fs: &impl SaveFs, shown: Option<&str>) -> Result<bool, AppendRefusal> {
        let expected = shown.map(|text| digest(text.as_bytes()));
        append_with(
            fs,
            Some(&self.config()),
            expected.as_ref(),
            &snippets(),
            &NAMES,
        )
    }

    /// `config.toml` holds `bytes` and nothing else is in the folder.
    fn assert_untouched(&self, bytes: &str) {
        assert_eq!(self.read("config.toml"), bytes);
        assert_eq!(self.names(), ["config.toml"]);
    }
}

// S4: the old bytes stay, the backup equals them, the mode is kept.
#[test]
fn the_append_keeps_the_old_bytes_backs_them_up_and_keeps_the_mode() {
    let lab = lab();
    lab.put(&lab.config(), OLD, 0o644);
    assert_eq!(lab.append(Some(OLD)), Ok(true));
    let new = lab.read("config.toml");
    assert_eq!(
        new,
        format!("{OLD}\r\n{}", snippets().replace('\n', "\r\n"))
    );
    assert_eq!(lab.read("config.toml.bak"), OLD);
    assert_eq!(lab.mode(&lab.config()), 0o644);
    assert_eq!(lab.names(), ["config.toml", "config.toml.bak"]);
}

#[test]
fn a_missing_file_is_created_0600_without_a_backup() {
    let lab = lab();
    assert_eq!(lab.append(None), Ok(false));
    assert_eq!(lab.read("config.toml"), snippets());
    assert_eq!(lab.mode(&lab.config()), 0o600);
    assert_eq!(lab.names(), ["config.toml"]);
}

#[test]
fn a_symlinked_file_is_refused_and_nothing_changes() {
    let lab = lab();
    let real = lab._root.path().join("elsewhere.toml");
    lab.put(&real, OLD, 0o644);
    symlink(&real, lab.config()).unwrap();
    let refusal = lab.append(Some(OLD)).unwrap_err();
    assert_eq!(refusal, AppendRefusal::File(Refusal::FileSymlink));
    assert_eq!(
        refusal.notice(),
        "config.toml is a symlink; edit it by hand"
    );
    assert_eq!(fs::read_to_string(&real).unwrap(), OLD);
    assert!(fs::symlink_metadata(lab.config()).unwrap().is_symlink());
}

#[test]
fn a_symlinked_folder_is_refused() {
    let lab = lab();
    lab.put(&lab.config(), OLD, 0o644);
    let link = lab._root.path().join("link");
    symlink(&lab.dir, &link).unwrap();
    let out = append_with(
        &RealFs,
        Some(&link.join("config.toml")),
        Some(&digest(OLD.as_bytes())),
        &snippets(),
        &NAMES,
    );
    assert_eq!(out, Err(AppendRefusal::File(Refusal::FolderSymlink)));
    lab.assert_untouched(OLD);
}

#[test]
fn a_file_or_folder_others_can_write_is_refused() {
    let lab = lab();
    lab.put(&lab.config(), OLD, 0o664);
    let refusal = lab.append(Some(OLD)).unwrap_err();
    assert_eq!(
        refusal,
        AppendRefusal::File(Refusal::FileWritable(lab.config()))
    );
    assert!(
        refusal.notice().contains("chmod go-w"),
        "{}",
        refusal.notice()
    );
    lab.assert_untouched(OLD);
    assert_eq!(lab.mode(&lab.config()), 0o664);
    lab.put(&lab.config(), OLD, 0o644);
    fs::set_permissions(&lab.dir, fs::Permissions::from_mode(0o770)).unwrap();
    assert!(matches!(
        lab.append(Some(OLD)),
        Err(AppendRefusal::File(Refusal::FolderWritable(_)))
    ));
    lab.assert_untouched(OLD);
}

#[test]
fn a_hard_linked_or_read_only_file_is_refused() {
    let lab = lab();
    lab.put(&lab.config(), OLD, 0o644);
    fs::hard_link(lab.config(), lab._root.path().join("other")).unwrap();
    assert_eq!(
        lab.append(Some(OLD)),
        Err(AppendRefusal::File(Refusal::HardLinked))
    );
    lab.assert_untouched(OLD);
    let lab = super::tests::lab();
    lab.put(&lab.config(), OLD, 0o444);
    assert!(matches!(
        lab.append(Some(OLD)),
        Err(AppendRefusal::File(Refusal::ReadOnly(_)))
    ));
    lab.assert_untouched(OLD);
}

// S4: a file that is not the one the plan was shown with is never written.
#[test]
fn a_file_changed_since_the_plan_was_shown_is_refused() {
    let lab = lab();
    lab.put(&lab.config(), OLD, 0o644);
    let changed = Err(AppendRefusal::File(Refusal::Changed));
    assert_eq!(lab.append(Some("mouse = true\n")), changed);
    // Shown missing, now present.
    assert_eq!(lab.append(None), changed);
    lab.assert_untouched(OLD);
    // Shown present, now missing.
    let lab = super::tests::lab();
    assert_eq!(lab.append(Some(OLD)), changed);
    assert!(lab.names().is_empty());
    assert_eq!(
        AppendRefusal::File(Refusal::Changed).notice(),
        "config.toml changed on disk; nothing was saved, try again"
    );
}

#[test]
fn a_parse_refusal_writes_nothing() {
    let lab = lab();
    let inline = "[ai.mcp]\nservers = []\n";
    lab.put(&lab.config(), inline, 0o644);
    assert_eq!(lab.append(Some(inline)), Err(AppendRefusal::WouldBreak));
    lab.assert_untouched(inline);
}

/// The real I/O with fixed temporary names.
#[derive(Default)]
struct NamedFs {
    names: RefCell<Vec<u64>>,
}

impl SaveFs for NamedFs {
    fn euid(&self) -> u32 {
        RealFs.euid()
    }
    fn random(&self) -> u64 {
        let mut names = self.names.borrow_mut();
        if names.is_empty() {
            RealFs.random()
        } else {
            names.remove(0)
        }
    }
    fn open_dir(&self, path: &Path) -> std::io::Result<OwnedFd> {
        RealFs.open_dir(path)
    }
    fn fstat(&self, fd: &OwnedFd) -> std::io::Result<Stat> {
        RealFs.fstat(fd)
    }
    fn open_read(&self, dir: &OwnedFd, name: &str) -> std::io::Result<OwnedFd> {
        RealFs.open_read(dir, name)
    }
    fn read(&self, fd: &OwnedFd, max: u64) -> std::io::Result<Vec<u8>> {
        RealFs.read(fd, max)
    }
    fn stat_at(&self, dir: &OwnedFd, name: &str) -> std::io::Result<Stat> {
        RealFs.stat_at(dir, name)
    }
    fn create_excl(&self, dir: &OwnedFd, name: &str) -> std::io::Result<OwnedFd> {
        RealFs.create_excl(dir, name)
    }
    fn private(&self, fd: &OwnedFd) -> std::io::Result<()> {
        RealFs.private(fd)
    }
    fn write_all(&self, fd: &OwnedFd, bytes: &[u8]) -> std::io::Result<()> {
        RealFs.write_all(fd, bytes)
    }
    fn fsync(&self, fd: &OwnedFd) -> std::io::Result<()> {
        RealFs.fsync(fd)
    }
    fn rename(&self, dir: &OwnedFd, from: &str, to: &str) -> std::io::Result<()> {
        RealFs.rename(dir, from, to)
    }
    fn unlink(&self, dir: &OwnedFd, name: &str) -> std::io::Result<()> {
        RealFs.unlink(dir, name)
    }
}

#[test]
fn three_taken_temporary_names_refuse_the_append() {
    let lab = lab();
    lab.put(&lab.config(), OLD, 0o644);
    let pid = std::process::id();
    let mut taken = Vec::new();
    for n in 1..=3u64 {
        let name = format!(".config.toml.{pid}.{n:016x}.tmp");
        fs::write(lab.dir.join(&name), "planted").unwrap();
        taken.push(name);
    }
    let fs = NamedFs {
        names: RefCell::new(vec![1, 2, 3]),
    };
    assert_eq!(
        lab.append_fs(&fs, Some(OLD)),
        Err(AppendRefusal::File(Refusal::TempExists))
    );
    assert_eq!(lab.read("config.toml"), OLD);
    for name in taken {
        assert_eq!(lab.read(&name), "planted");
    }
}

fn write(values: &[(&'static str, &'static str, &str)], shown: Option<&str>) -> Connect {
    Connect {
        set: values
            .iter()
            .map(|(table, name, value)| {
                let mut text = SecretText::new();
                text.push_str(value);
                (*table, *name, text)
            })
            .collect(),
        snippets: snippets(),
        names: NAMES.to_vec(),
        expected: shown.map(|text| digest(text.as_bytes())),
    }
}

const VALUES: [(&str, &str, &str); 3] = [
    ("gsc", "GSC_SKIP_OAUTH", "true"),
    ("gsc", "GSC_CONFIG_DIR", "/home/ana/.config/gsc"),
    ("clarity", "CLARITY_API_TOKEN", "s3cret-value"),
];

// S4: several values in one save, one backup generation, then the append.
#[test]
fn connect_saves_every_value_in_one_write_then_appends() {
    let lab = lab();
    lab.put(&lab.config(), OLD, 0o644);
    lab.put(&lab.secrets(), SECRETS, 0o600);
    let done = connect_with(&RealFs, Some(&lab.secrets()), &write(&VALUES, Some(OLD)));
    assert!(matches!(done, Connected::Done(_)), "{done:?}");
    assert_eq!(lab.read("secrets.toml.bak"), SECRETS);
    let secrets = lab.read("secrets.toml");
    assert!(secrets.starts_with("# values\n"), "{secrets:?}");
    for (_, name, value) in VALUES {
        assert!(secrets.contains(&format!("{name} = \"{value}\"")), "{name}");
    }
    assert_eq!(lab.mode(&lab.secrets()), 0o600);
    assert_eq!(lab.read("config.toml.bak"), OLD);
    assert_eq!(
        lab.names(),
        [
            "config.toml",
            "config.toml.bak",
            "secrets.toml",
            "secrets.toml.bak"
        ]
    );
    let notice = done.notice();
    assert!(
        notice.contains("gsc") && !notice.contains('\n'),
        "{notice:?}"
    );
    assert!(!notice.contains("s3cret-value"), "{notice:?}");
}

#[test]
fn a_refused_value_writes_nothing_at_all() {
    let lab = lab();
    lab.put(&lab.config(), OLD, 0o644);
    lab.put(&lab.secrets(), SECRETS, 0o644);
    let done = connect_with(&RealFs, Some(&lab.secrets()), &write(&VALUES, Some(OLD)));
    assert!(matches!(done, Connected::Refused(_)), "{done:?}");
    assert!(done.notice().contains("nothing was written"), "{done:?}");
    assert_eq!(lab.read("config.toml"), OLD);
    assert_eq!(lab.read("secrets.toml"), SECRETS);
    assert_eq!(lab.names(), ["config.toml", "secrets.toml"]);
}

// The values stay saved and the notice says the entries were not added.
#[test]
fn a_refused_append_after_the_save_says_the_values_stay_saved() {
    let lab = lab();
    let inline = "ai.mcp.servers = []\n";
    lab.put(&lab.config(), inline, 0o644);
    lab.put(&lab.secrets(), SECRETS, 0o600);
    let done = connect_with(&RealFs, Some(&lab.secrets()), &write(&VALUES, Some(inline)));
    assert!(matches!(done, Connected::ValuesOnly(_)), "{done:?}");
    let notice = done.notice();
    assert!(
        notice.contains("saved 3 values") && notice.contains("paste"),
        "{notice:?}"
    );
    assert!(!notice.contains('\n') && !notice.contains("s3cret-value"));
    assert_eq!(lab.read("config.toml"), inline);
    assert!(lab.read("secrets.toml").contains("s3cret-value"));
}

#[test]
fn no_values_to_set_writes_only_the_config() {
    let lab = lab();
    let done = connect_with(&RealFs, Some(&lab.secrets()), &write(&[], None));
    assert!(matches!(done, Connected::Done(_)), "{done:?}");
    assert_eq!(lab.names(), ["config.toml"]);
}

#[test]
fn no_config_folder_writes_nothing() {
    let done = connect_with(&RealFs, None, &write(&VALUES, None));
    assert!(matches!(done, Connected::Refused(_)), "{done:?}");
    assert!(!done.notice().contains('\n'));
}
