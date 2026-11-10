use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::PathBuf,
    time::Duration,
};

use super::*;
use crate::{core::config::ConfigEdit, runtime::config_write::save_config};

/// A temp config folder, never the user's own.
struct Lab {
    dir: PathBuf,
    _root: tempfile::TempDir,
}

fn lab() -> Lab {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("lazarobox");
    fs::create_dir_all(&dir).unwrap();
    Lab { dir, _root: root }
}

impl Lab {
    fn path(&self) -> PathBuf {
        self.dir.join("secrets.toml")
    }

    /// Writes the file with `mode`.
    fn write(&self, text: &str, mode: u32) -> PathBuf {
        let path = self.path();
        fs::write(&path, text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        path
    }
}

const CLARITY: &str = "[clarity]\nCLARITY_API_TOKEN = \"tok\"\n";

// Spec mcp-secrets, Location and ownership: missing file.
#[test]
fn a_missing_file_is_no_values_and_no_problem() {
    let lab = lab();
    assert_eq!(load(&lab.path()), Loaded::default());
}

// Spec mcp-secrets, Mode 0600: 0600 is used.
#[test]
fn a_0600_file_is_used_without_a_problem() {
    let lab = lab();
    let path = lab.write(CLARITY, 0o600);
    let loaded = load(&path);
    assert_eq!(loaded.problems, Vec::<String>::new());
    assert_eq!(
        loaded.secrets.value("clarity", "CLARITY_API_TOKEN"),
        Some("tok")
    );
    // Stricter is fine too.
    fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
    assert_eq!(load(&path).problems, Vec::<String>::new());
}

// Spec mcp-secrets, Mode 0600: readable by others is not used.
#[test]
fn a_file_others_may_read_or_write_is_not_used() {
    let lab = lab();
    for mode in [0o644, 0o640, 0o602, 0o660] {
        let path = lab.write(CLARITY, mode);
        let loaded = load(&path);
        assert_eq!(loaded.secrets, Secrets::default(), "{mode:o}");
        assert_eq!(
            loaded.problems,
            [format!(
                "secrets.toml: mode must be 0600 (chmod 600 {})",
                path.display()
            )],
            "{mode:o}"
        );
    }
}

// Spec mcp-secrets, Mode 0600: directory in its place.
#[test]
fn a_directory_in_its_place_is_not_a_regular_file() {
    let lab = lab();
    fs::create_dir(lab.path()).unwrap();
    assert_eq!(
        load(&lab.path()),
        Loaded::problem("secrets.toml: not a regular file".to_owned())
    );
}

// Spec mcp-secrets, Read on each start: must not block on a special file.
#[test]
fn a_fifo_in_its_place_is_refused_without_blocking() {
    let lab = lab();
    let path = lab.path();
    let c = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    // SAFETY: a valid NUL-terminated path in a folder of this test.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || tx.send(load(&path)).unwrap());
    let loaded = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("the read does not wait for a writer");
    assert_eq!(loaded.problems, ["secrets.toml: not a regular file"]);
}

// Spec mcp-secrets, Read on each start: oversized file.
#[test]
fn a_file_over_64_kib_is_not_used() {
    let lab = lab();
    let mut text = CLARITY.to_owned();
    text.push_str(&format!("# {}\n", "x".repeat(65 * 1024)));
    let path = lab.write(&text, 0o600);
    assert_eq!(
        load(&path),
        Loaded::problem("secrets.toml: larger than 64 KiB".to_owned())
    );
    // Exactly 64 KiB is still read.
    let mut text = CLARITY.to_owned();
    let pad = 64 * 1024 - text.len() - 3;
    text.push_str(&format!("# {}\n", "x".repeat(pad)));
    assert_eq!(text.len(), 64 * 1024);
    let loaded = load(&lab.write(&text, 0o600));
    assert_eq!(loaded.problems, Vec::<String>::new());
    assert_eq!(
        loaded.secrets.value("clarity", "CLARITY_API_TOKEN"),
        Some("tok")
    );
}

// Spec mcp-secrets, Read on each start: an edit applies on the next read.
#[test]
fn each_load_reads_the_file_again() {
    let lab = lab();
    let path = lab.write("[clarity]\nCLARITY_API_TOKEN = \"a\"\n", 0o600);
    assert_eq!(
        load(&path).secrets.value("clarity", "CLARITY_API_TOKEN"),
        Some("a")
    );
    lab.write("[clarity]\nCLARITY_API_TOKEN = \"b\"\n", 0o600);
    assert_eq!(
        load(&path).secrets.value("clarity", "CLARITY_API_TOKEN"),
        Some("b")
    );
}

// Spec mcp-secrets, Problems without values: syntax error hides the line.
#[test]
fn a_syntax_error_is_one_problem_without_the_value() {
    let lab = lab();
    let path = lab.write("# values\n[clarity]\nCLARITY_API_TOKEN = \"sk-123\n", 0o600);
    let loaded = load(&path);
    assert_eq!(loaded.problems, ["secrets.toml: not valid TOML (line 3)"]);
    assert!(!format!("{loaded:?}").contains("sk-123"));
}

// Spec mcp-secrets, Location and ownership: never written.
#[test]
fn reading_the_file_and_saving_the_config_never_touch_it() {
    let lab = lab();
    let config = lab.dir.join("config.toml");
    fs::write(&config, "[statusline]\nposition = \"bottom\"\n").unwrap();
    let edit = ConfigEdit {
        table: "statusline",
        key: "position",
        value: std::borrow::Cow::Borrowed("top"),
    };
    // Without the file: nothing creates it.
    load(&lab.path());
    assert_eq!(
        save_config(Some(&config), std::slice::from_ref(&edit)),
        Ok(())
    );
    assert!(!lab.path().exists());
    // With it: same bytes, same mode, same inode, and none of its values
    // in `config.toml`.
    let path = lab.write("[clarity]\nCLARITY_API_TOKEN = \"sk-123\"\n", 0o600);
    let before = (fs::read(&path).unwrap(), fs::metadata(&path).unwrap());
    load(&path);
    assert_eq!(save_config(Some(&config), &[edit]), Ok(()));
    let after = fs::metadata(&path).unwrap();
    assert_eq!(fs::read(&path).unwrap(), before.0);
    assert_eq!(after.mode(), before.1.mode());
    assert_eq!(after.ino(), before.1.ino());
    assert_eq!(after.mtime(), before.1.mtime());
    assert!(!fs::read_to_string(&config).unwrap().contains("sk-123"));
}
