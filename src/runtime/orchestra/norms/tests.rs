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

// D3: a symlink is never followed, and a missing file is no error.
#[test]
fn a_symlink_is_skipped_without_a_line() {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    write(other.path(), "secret.md", "not a norm\n");
    std::os::unix::fs::symlink(other.path().join("secret.md"), dir.path().join("AGENTS.md"))
        .unwrap();
    write(dir.path(), "CLAUDE.md", "claude rules\n");
    let read = read(dir.path());
    assert_eq!(read.norms.len(), 1);
    assert_eq!(read.norms[0].name, "CLAUDE.md");
    assert!(read.said.is_empty(), "{:?}", read.said);
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

// A directory in place of the file is not a norm and is no error.
#[test]
fn a_directory_is_not_a_norm() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("AGENTS.md")).unwrap();
    let read = read(dir.path());
    assert!(read.norms.is_empty() && read.said.is_empty());
}
