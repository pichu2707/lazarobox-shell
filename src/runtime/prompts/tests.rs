use std::{
    ffi::CString,
    fs,
    os::unix::{ffi::OsStrExt, fs::PermissionsExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use super::*;
use crate::core::ai::chat::prompts::{Reason, builtin};

fn dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn put(root: &Path, rel: &str, bytes: &[u8]) -> PathBuf {
    let path = root.join("prompts").join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    path
}

fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

// Spec: Missing file.
#[test]
fn a_missing_file_is_missing() {
    let d = dir();
    assert_eq!(read_override(&d.path().join("nope.md")), Override::Missing);
}

// Spec: Override content is exact.
#[test]
fn a_valid_file_is_its_text() {
    let d = dir();
    let path = put(d.path(), "analyst/chat.md", b"Be brief.\n");
    assert_eq!(read_override(&path), Override::Text("Be brief.\n".into()));
}

// Spec: Empty file.
#[test]
fn a_whitespace_only_file_is_missing() {
    let d = dir();
    let path = put(d.path(), "analyst/chat.md", b" \n\t\n");
    assert_eq!(read_override(&path), Override::Missing);
    let empty = put(d.path(), "analyst/code.md", b"");
    assert_eq!(read_override(&empty), Override::Missing);
}

// Spec: Not UTF-8.
#[test]
fn bad_utf8_is_skipped() {
    let d = dir();
    let path = put(d.path(), "analyst/chat.md", b"\xff\xfe\x00");
    assert_eq!(read_override(&path), Override::Skipped(Reason::NotUtf8));
}

// Spec: Exactly at the cap, and over the cap.
#[test]
fn the_cap_is_16384_bytes() {
    let d = dir();
    let at = put(d.path(), "analyst/chat.md", &vec![b'a'; 16384]);
    assert!(matches!(read_override(&at), Override::Text(t) if t.len() == 16384));
    let over = put(d.path(), "analyst/code.md", &vec![b'a'; 16385]);
    assert_eq!(read_override(&over), Override::Skipped(Reason::TooLarge));
}

// Spec: Unreadable.
#[test]
fn a_file_without_read_permission_cannot_be_read() {
    if is_root() {
        eprintln!("skipped: root reads everything");
        return;
    }
    let d = dir();
    let path = put(d.path(), "analyst/chat.md", b"x");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    assert_eq!(read_override(&path), Override::Skipped(Reason::Unreadable));
}

// Spec: Not a regular file (directory).
#[test]
fn a_directory_is_not_a_regular_file() {
    let d = dir();
    let path = d.path().join("prompts/analyst/chat.md");
    fs::create_dir_all(&path).unwrap();
    assert_eq!(read_override(&path), Override::Skipped(Reason::NotRegular));
}

// Spec: Not a regular file (FIFO), without blocking.
#[test]
fn a_fifo_returns_without_blocking() {
    let d = dir();
    let path = d.path().join("chat.md");
    let c = CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: `c` is a valid NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    let started = Instant::now();
    assert_eq!(read_override(&path), Override::Skipped(Reason::NotRegular));
    assert!(started.elapsed() < Duration::from_secs(2));
}

// Spec: Huge special file.
#[test]
fn a_symlink_to_dev_zero_is_ignored() {
    let d = dir();
    let path = d.path().join("chat.md");
    std::os::unix::fs::symlink("/dev/zero", &path).unwrap();
    assert_eq!(read_override(&path), Override::Skipped(Reason::NotRegular));
}

// Spec: Task override wins, other profile ignored.
#[test]
fn load_reads_the_profile_files_in_order() {
    let d = dir();
    put(d.path(), "analyst/chat.md", b"task\n");
    put(d.path(), "analyst/general.md", b"general\n");
    put(d.path(), "programmer/chat.md", b"other profile\n");
    let got = load_prompt(Some(d.path()), Profile::Analyst, Task::Chat);
    assert_eq!(got.text, "task");
    assert!(got.notices.is_empty());
    fs::remove_file(d.path().join("prompts/analyst/chat.md")).unwrap();
    let got = load_prompt(Some(d.path()), Profile::Analyst, Task::Chat);
    assert_eq!(got.text, "general");
}

// Spec: Other profile ignored; built-in when no files.
#[test]
fn load_falls_back_to_the_builtin_without_notice() {
    let d = dir();
    put(d.path(), "programmer/chat.md", b"other profile\n");
    let got = load_prompt(Some(d.path()), Profile::Analyst, Task::Chat);
    assert_eq!(got.text, builtin(Profile::Analyst, Task::Chat).trim_end());
    assert!(got.notices.is_empty());
    let none = load_prompt(None, Profile::Analyst, Task::Chat);
    assert_eq!(none, got);
}

// Spec: Bad task file, good general file.
#[test]
fn load_notes_a_bad_task_file_and_uses_general() {
    let d = dir();
    put(d.path(), "analyst/chat.md", b"\xff");
    put(d.path(), "analyst/general.md", b"general");
    let got = load_prompt(Some(d.path()), Profile::Analyst, Task::Chat);
    assert_eq!(got.text, "general");
    assert_eq!(
        got.notices,
        ["prompt override ignored: analyst/chat.md not valid UTF-8"]
    );
}
