use std::ffi::OsString;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::*;

const TOP: &str = "llama-b9000";

fn path() -> OsString {
    std::env::var_os("PATH").unwrap_or_default()
}

macro_rules! need_tar {
    () => {
        let have = |name| crate::runtime::ai_detect::find_on_path(name, &path()).is_some();
        if !have("tar") || !have("gzip") {
            eprintln!("SKIPPED: tar or gzip is not installed; the extraction checks did not run");
            return;
        }
    };
}

/// One archive member, written by hand so a hostile one (a `..` name, a
/// device file) can be built without root and without the tar being asked.
pub(in crate::runtime::llama_install) struct Entry<'a> {
    pub(in crate::runtime::llama_install) name: &'a str,
    pub(in crate::runtime::llama_install) kind: u8,
    pub(in crate::runtime::llama_install) link: &'a str,
    pub(in crate::runtime::llama_install) mode: u32,
    pub(in crate::runtime::llama_install) data: &'a [u8],
}

pub(in crate::runtime::llama_install) fn file<'a>(name: &'a str, data: &'a [u8]) -> Entry<'a> {
    Entry {
        name,
        kind: b'0',
        link: "",
        mode: 0o644,
        data,
    }
}

pub(in crate::runtime::llama_install) fn dir(name: &str) -> Entry<'_> {
    Entry {
        name,
        kind: b'5',
        link: "",
        mode: 0o755,
        data: b"",
    }
}

pub(in crate::runtime::llama_install) fn link<'a>(
    name: &'a str,
    kind: u8,
    target: &'a str,
) -> Entry<'a> {
    Entry {
        name,
        kind,
        link: target,
        mode: 0o777,
        data: b"",
    }
}

fn octal(field: &mut [u8], value: u64) {
    let text = format!("{value:0width$o}", width = field.len() - 1);
    field[..text.len()].copy_from_slice(text.as_bytes());
}

fn header(entry: &Entry) -> [u8; 512] {
    let mut block = [0u8; 512];
    block[..entry.name.len()].copy_from_slice(entry.name.as_bytes());
    octal(&mut block[100..108], u64::from(entry.mode));
    // Owned by root in the archive.
    octal(&mut block[108..116], 0);
    octal(&mut block[116..124], 0);
    octal(&mut block[124..136], entry.data.len() as u64);
    octal(&mut block[136..148], 1_700_000_000);
    block[148..156].fill(b' ');
    block[156] = entry.kind;
    block[157..157 + entry.link.len()].copy_from_slice(entry.link.as_bytes());
    block[257..263].copy_from_slice(b"ustar\0");
    block[263..265].copy_from_slice(b"00");
    let sum: u32 = block.iter().map(|b| u32::from(*b)).sum();
    let text = format!("{sum:06o}\0 ");
    block[148..156].copy_from_slice(text.as_bytes());
    block
}

/// Writes `<dir>/a.tar.gz` holding `entries`.
pub(in crate::runtime::llama_install) fn archive(into: &Path, entries: &[Entry]) -> PathBuf {
    let mut tar = Vec::new();
    for entry in entries {
        tar.extend_from_slice(&header(entry));
        tar.extend_from_slice(entry.data);
        tar.resize(tar.len().div_ceil(512) * 512, 0);
    }
    tar.resize(tar.len() + 1024, 0);
    let out = into.join("a.tar.gz");
    let mut gzip = Command::new("gzip")
        .arg("-c")
        .stdin(Stdio::piped())
        .stdout(std::fs::File::create(&out).unwrap())
        .spawn()
        .unwrap();
    std::io::Write::write_all(&mut gzip.stdin.take().unwrap(), &tar).unwrap();
    assert!(gzip.wait().unwrap().success());
    out
}

/// Extracts `entries` into a fresh staging folder next to the archive.
fn run(entries: &[Entry]) -> (tempfile::TempDir, Result<(), String>) {
    let work = tempfile::tempdir().unwrap();
    let archive = archive(work.path(), entries);
    let staging = work.path().join("staging");
    std::fs::create_dir(&staging).unwrap();
    let tar = find_tar(&path()).unwrap();
    let result = extract(&tar, &archive, &staging, TOP);
    (work, result)
}

fn refused(entries: &[Entry]) -> String {
    let (work, result) = run(entries);
    let why = result.expect_err("the archive must be refused");
    // Nothing escaped the staging folder.
    let outside: Vec<_> = std::fs::read_dir(work.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(outside.len(), 2, "{outside:?}");
    why
}

fn good() -> Vec<Entry<'static>> {
    vec![
        dir("llama-b9000/"),
        Entry {
            mode: 0o755,
            ..file("llama-b9000/llama-server", b"#!/bin/sh\n")
        },
        file("llama-b9000/libllama.so.0.0.9000", b"lib"),
        link("llama-b9000/libllama.so.0", b'2', "libllama.so.0.0.9000"),
    ]
}

// Spec (llama-server-install, Safe extraction): a clean archive unpacks, a
// bare-name symlink is kept, the executable bit stays.
#[test]
fn a_valid_archive_is_extracted_with_its_bare_symlink() {
    need_tar!();
    let (work, result) = run(&good());
    result.unwrap();
    let top = work.path().join("staging").join(TOP);
    let server = std::fs::metadata(top.join("llama-server")).unwrap();
    assert_eq!(server.permissions().mode() & 0o111, 0o111);
    assert_eq!(
        std::fs::read_link(top.join("libllama.so.0")).unwrap(),
        Path::new("libllama.so.0.0.9000")
    );
}

#[test]
fn a_member_with_dot_dot_is_refused() {
    need_tar!();
    let why = refused(&[file("llama-b9000/../evil", b"x")]);
    assert!(why.contains("unsafe archive member"), "{why}");
}

#[test]
fn an_absolute_member_is_refused() {
    need_tar!();
    refused(&[file("/tmp/evil", b"x")]);
}

#[test]
fn a_member_outside_the_top_folder_is_refused() {
    need_tar!();
    let why = refused(&[file("llama-b9000/ok", b"x"), file("other/evil", b"x")]);
    assert!(why.contains("outside the top directory"), "{why}");
}

#[test]
fn symlinks_that_point_anywhere_but_a_bare_name_are_refused() {
    need_tar!();
    for target in ["/etc/passwd", "..", "../x", "sub/x"] {
        let why = refused(&[link("llama-b9000/l", b'2', target)]);
        assert!(why.contains("unsafe symlink target"), "{target}: {why}");
    }
}

#[test]
fn a_symlink_to_nothing_is_refused() {
    need_tar!();
    let why = refused(&[dir("llama-b9000/"), link("llama-b9000/l", b'2', "missing")]);
    assert!(why.contains("dangling"), "{why}");
}

// A hardlink names its target from the archive root; one outside the top
// folder would link a file of the host into the install.
#[test]
fn a_hardlink_outside_the_top_folder_is_refused() {
    need_tar!();
    for target in ["/etc/passwd", "../x", "other/x"] {
        let why = refused(&[link("llama-b9000/h", b'1', target)]);
        assert!(why.contains("hardlink"), "{target}: {why}");
    }
}

#[test]
fn a_hardlink_inside_the_top_folder_is_kept() {
    need_tar!();
    let (work, result) = run(&[
        dir("llama-b9000/"),
        file("llama-b9000/a", b"same"),
        link("llama-b9000/b", b'1', "llama-b9000/a"),
    ]);
    result.unwrap();
    let top = work.path().join("staging").join(TOP);
    assert_eq!(std::fs::read(top.join("b")).unwrap(), b"same");
}

#[test]
fn device_files_are_refused() {
    need_tar!();
    let why = refused(&[Entry {
        kind: b'3',
        ..file("llama-b9000/null", b"")
    }]);
    assert!(why.contains("not a file"), "{why}");
}

// Run as a normal user tar drops the bit itself; run as root it keeps it and
// the walk refuses the file. Either way no setuid file is left behind.
#[test]
fn a_setuid_bit_never_survives() {
    need_tar!();
    let (work, result) = run(&[Entry {
        mode: 0o4755,
        ..file("llama-b9000/su", b"x")
    }]);
    match result {
        Ok(()) => {
            let meta = std::fs::metadata(work.path().join("staging").join(TOP).join("su"));
            assert_eq!(meta.unwrap().mode() & 0o6000, 0);
        }
        Err(why) => assert!(why.contains("setuid"), "{why}"),
    }
}

// The listing could be fooled by a name that reads like a hardlink line; the
// disk cannot: a file with a link outside the tree is refused.
#[test]
fn a_file_linked_outside_the_tree_is_refused_by_the_walk() {
    let work = tempfile::tempdir().unwrap();
    let top = work.path().join("staging").join(TOP);
    std::fs::create_dir_all(&top).unwrap();
    let outside = work.path().join("host-file");
    std::fs::write(&outside, "x").unwrap();
    std::fs::hard_link(&outside, top.join("h")).unwrap();
    let why = check_result(&work.path().join("staging"), TOP).unwrap_err();
    assert!(why.contains("outside the install"), "{why}");
}

// The archive says root owns everything; the files belong to whoever runs.
#[test]
fn ownership_from_the_archive_is_not_kept() {
    need_tar!();
    let (work, result) = run(&good());
    result.unwrap();
    let meta = std::fs::metadata(work.path().join("staging").join(TOP)).unwrap();
    // SAFETY: getuid has no preconditions and cannot fail.
    assert_eq!(meta.uid(), unsafe { libc::getuid() });
}

#[test]
fn something_that_is_not_an_archive_is_refused() {
    need_tar!();
    let work = tempfile::tempdir().unwrap();
    let junk = work.path().join("junk.tar.gz");
    std::fs::write(&junk, "not an archive").unwrap();
    let tar = find_tar(&path()).unwrap();
    assert!(extract(&tar, &junk, work.path(), TOP).is_err());
}

#[test]
fn too_many_members_are_refused() {
    need_tar!();
    let names: Vec<String> = (0..=MAX_ENTRIES)
        .map(|i| format!("llama-b9000/f{i}"))
        .collect();
    let entries: Vec<Entry> = names.iter().map(|n| file(n, b"")).collect();
    let why = refused(&entries);
    assert!(why.contains("too many"), "{why}");
}

#[test]
fn a_missing_tar_is_said_plainly() {
    assert_eq!(
        find_tar(std::ffi::OsStr::new("/nonexistent")),
        Err(NO_TAR.to_owned())
    );
}

fn dir_mode(name: &str, mode: u32) -> Entry<'_> {
    Entry { mode, ..dir(name) }
}

// A folder the archive closes (000, 0500) must neither stop the walk nor
// leave a tree that cannot be deleted.
#[test]
fn a_closed_folder_is_opened_and_the_tree_can_be_deleted() {
    need_tar!();
    for mode in [0o000, 0o500] {
        let (work, result) = run(&[
            dir("llama-b9000/"),
            dir_mode("llama-b9000/lib/", mode),
            file("llama-b9000/lib/x", b"x"),
            dir_mode("llama-b9000/lib/deep/", mode),
            file("llama-b9000/lib/deep/y", b"y"),
        ]);
        result.unwrap_or_else(|e| panic!("mode {mode:o}: {e}"));
        let top = work.path().join("staging").join(TOP);
        let lib = std::fs::metadata(top.join("lib")).unwrap();
        assert_eq!(lib.mode() & 0o700, 0o700, "mode {mode:o}");
        assert_eq!(std::fs::read(top.join("lib/deep/y")).unwrap(), b"y");
        remove_tree(&work.path().join("staging")).unwrap();
        assert!(!work.path().join("staging").exists());
    }
}

#[test]
fn remove_tree_opens_what_is_closed_and_never_follows_a_link() {
    let work = tempfile::tempdir().unwrap();
    let outside = work.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o500)).unwrap();
    let tree = work.path().join("tree");
    std::fs::create_dir_all(tree.join("a/b")).unwrap();
    std::fs::write(tree.join("a/b/f"), "x").unwrap();
    std::os::unix::fs::symlink(&outside, tree.join("a/link")).unwrap();
    for closed in ["a/b", "a"] {
        std::fs::set_permissions(tree.join(closed), std::fs::Permissions::from_mode(0o000))
            .unwrap();
    }
    remove_tree(&tree).unwrap();
    assert!(!tree.exists());
    // The link's target was not touched (its mode is still 0500).
    let meta = std::fs::metadata(&outside).unwrap();
    assert_eq!(meta.mode() & 0o777, 0o500);
    // A link alone goes without its target; nothing at all is fine.
    let link = work.path().join("link");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    remove_tree(&link).unwrap();
    assert!(outside.exists() && std::fs::symlink_metadata(&link).is_err());
    remove_tree(&link).unwrap();
    std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o700)).unwrap();
}

// The verbose listing joins a hardlink's name and target with " link to ":
// a member named like that could fake the target, so it is refused. Same for
// a newline in a name (GNU tar prints it escaped; the control-character
// check of `check_member` refuses the rest).
#[test]
fn a_member_named_like_a_hardlink_line_or_with_a_newline_is_refused() {
    need_tar!();
    for name in [
        "llama-b9000/a link to llama-b9000/b",
        "llama-b9000/a\nllama-b9000/b",
    ] {
        let (_work, result) = run(&[dir("llama-b9000/"), file(name, b"x")]);
        assert!(result.is_err(), "{name:?} was accepted");
    }
    let why = refused(&[link(
        "llama-b9000/h",
        b'1',
        "llama-b9000/a link to llama-b9000/b",
    )]);
    assert!(why.contains("hardlink") || why.contains("unsafe"), "{why}");
}

// A symlink that points out of the install, followed by a file written
// under it: the refusal must come from our checks, and the victim folder
// must stay empty.
#[test]
fn a_file_written_through_an_absolute_symlink_dir_is_refused_by_our_checks() {
    need_tar!();
    let victim = tempfile::tempdir().unwrap();
    let target = victim.path().to_str().unwrap().to_owned();
    let (work, result) = run(&[
        dir("llama-b9000/"),
        link("llama-b9000/sub", b'2', &target),
        file("llama-b9000/sub/evil", b"x"),
    ]);
    let why = result.expect_err("must be refused");
    assert!(why.contains("unsafe symlink"), "{why}");
    assert_eq!(std::fs::read_dir(victim.path()).unwrap().count(), 0);
    drop(work);
}
