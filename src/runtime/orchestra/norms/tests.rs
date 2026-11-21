use super::*;

fn write(dir: &Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).unwrap();
}

// Spec (Short or missing norms): 3 KB and no CLAUDE.md gives the norm, no
// cut line and no error.
#[test]
fn a_short_norm_and_a_missing_one_say_nothing() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "AGENTS.md", &"a".repeat(3 * 1024));
    let read = read(dir.path());
    assert_eq!(read.norms.len(), 1);
    assert_eq!(read.norms[0].name, "AGENTS.md");
    assert_eq!(read.norms[0].text.len(), 3 * 1024);
    assert!(read.said.is_empty(), "{:?}", read.said);
}

// Spec (Long norms are cut with a notice): the first 8 KB and one line.
#[test]
fn a_long_norm_is_cut_at_8_kb_with_one_line() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "AGENTS.md", &"a".repeat(12 * 1024));
    let read = read(dir.path());
    assert_eq!(read.norms[0].text.len(), 8 * 1024);
    assert_eq!(
        read.said,
        ["judge: norms cut at 8 KB \u{b7} AGENTS.md is 12 KB"]
    );
}

// D3: AGENTS.md then CLAUDE.md.
#[test]
fn both_files_are_read_in_order() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "CLAUDE.md", "claude rules\n");
    write(dir.path(), "AGENTS.md", "agent rules\n");
    let names: Vec<_> = read(dir.path()).norms.into_iter().map(|n| n.name).collect();
    assert_eq!(names, ["AGENTS.md", "CLAUDE.md"]);
}

// M3: a symlink whose real target is a regular file inside the origin is
// read (`CLAUDE.md -> AGENTS.md` is common).
#[test]
fn a_symlink_to_a_regular_file_inside_the_origin_is_read() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "AGENTS.md", "agent rules\n");
    std::os::unix::fs::symlink("AGENTS.md", dir.path().join("CLAUDE.md")).unwrap();
    let read = read(dir.path());
    assert_eq!(read.norms.len(), 1, "the same file is sent once");
    assert_eq!(read.norms[0].name, "AGENTS.md");
    assert!(read.said.is_empty(), "{:?}", read.said);
}

// Review m3: a link to nothing is not a missing file: it says so.
#[test]
fn a_dangling_symlink_says_it_could_not_be_read() {
    let dir = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink("nowhere.md", dir.path().join("AGENTS.md")).unwrap();
    let read = read(dir.path());
    assert!(read.norms.is_empty());
    assert_eq!(read.said, ["judge: AGENTS.md could not be read"]);
}

// D3: a link out of the origin is never followed, and says so.
#[test]
fn a_symlink_out_of_the_origin_is_not_read_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    write(other.path(), "secret.md", "not a norm\n");
    std::os::unix::fs::symlink(other.path().join("secret.md"), dir.path().join("AGENTS.md"))
        .unwrap();
    write(dir.path(), "CLAUDE.md", "claude rules\n");
    let read = read(dir.path());
    assert_eq!(read.norms.len(), 1);
    assert_eq!(read.norms[0].name, "CLAUDE.md");
    assert_eq!(
        read.said,
        ["judge: AGENTS.md is not a regular file and was not read"]
    );
}

// M1 of the review: a secret in a norm is redacted and counted.
#[test]
fn a_secret_in_a_norm_is_redacted_and_said() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "AGENTS.md", "token = ghp_abcdefgh12345678\n");
    let read = read(dir.path());
    assert!(!read.norms[0].text.contains("ghp_abcdefgh12345678"));
    assert_eq!(read.said, ["judge: AGENTS.md had 1 secret(s) redacted"]);
}

// m1: a FIFO in place of the file must not block the read.
#[test]
fn a_fifo_returns_promptly_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let path = std::ffi::CString::new(dir.path().join("AGENTS.md").to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let (tx, rx) = std::sync::mpsc::channel();
    let origin = dir.path().to_path_buf();
    std::thread::spawn(move || tx.send(read(&origin)).unwrap());
    let read = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("returned");
    assert!(read.norms.is_empty());
    assert_eq!(
        read.said,
        ["judge: AGENTS.md is not a regular file and was not read"]
    );
}

// An unreadable file says so (skipped as root, who reads everything).
#[test]
fn an_unreadable_file_says_so() {
    use std::os::unix::fs::PermissionsExt;
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "AGENTS.md", "rules\n");
    let path = dir.path().join("AGENTS.md");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    assert_eq!(
        read(dir.path()).said,
        ["judge: AGENTS.md could not be read"]
    );
}

// S1a verify W3: a file with nothing but an import has no norms of its own,
// said and never silent.
#[test]
fn an_import_only_file_says_it_has_no_norms_of_its_own() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "AGENTS.md", "rules\n");
    write(dir.path(), "CLAUDE.md", "@AGENTS.md\n");
    let read = read(dir.path());
    assert_eq!(read.norms.len(), 1);
    assert_eq!(read.said, ["judge: CLAUDE.md has no norms of its own"]);
}

// A directory in place of the file is not a norm and says so.
#[test]
fn a_directory_is_not_a_norm() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("AGENTS.md")).unwrap();
    let read = read(dir.path());
    assert!(read.norms.is_empty());
    assert_eq!(
        read.said,
        ["judge: AGENTS.md is not a regular file and was not read"]
    );
}

// Review: through `read`, a secret that starts before byte 8192 is masked
// whole (the read goes a little past the cut).
#[test]
fn a_secret_across_byte_8192_is_masked_through_read() {
    let dir = tempfile::tempdir().unwrap();
    let mut text = "a".repeat(8192 - 5);
    text.push_str(" ghp_abcdefgh12345678\n");
    text.push_str(&"b".repeat(800));
    write(dir.path(), "AGENTS.md", &text);
    let read = read(dir.path());
    assert!(!read.norms[0].text.contains("ghp_"));
    assert_eq!(read.said.len(), 2, "{:?}", read.said);
    assert!(
        read.said[1].contains("1 secret(s) redacted"),
        "{:?}",
        read.said
    );
}

// Review: a link is followed only to a `.md` file.
#[test]
fn a_symlink_to_a_file_that_is_not_markdown_is_not_read() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "notes.txt", "not a norm\n");
    std::os::unix::fs::symlink("notes.txt", dir.path().join("AGENTS.md")).unwrap();
    let read = read(dir.path());
    assert!(read.norms.is_empty());
    assert_eq!(
        read.said,
        ["judge: AGENTS.md links to a file that is not .md and was not read"]
    );
}
