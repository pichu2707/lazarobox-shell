use std::os::unix::fs::PermissionsExt;

use super::*;

#[test]
fn a_scratch_folder_is_private_canonical_and_gone_when_dropped() {
    let parent = tempfile::tempdir().unwrap();
    let parent = parent.path().canonicalize().unwrap();
    let scratch = Scratch::unique(&parent.join("tmp")).unwrap();
    let path = scratch.path().to_path_buf();
    assert!(path.starts_with(parent.join("tmp")));
    assert_eq!(path, path.canonicalize().unwrap());
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    // A run leaves files behind, some of them read-only folders' worth.
    std::fs::create_dir_all(path.join("a/b")).unwrap();
    std::fs::write(path.join("a/b/f"), "x").unwrap();
    drop(scratch);
    assert!(!path.exists());
    assert!(parent.join("tmp").exists(), "only its own folder goes");
}

#[test]
fn two_scratch_folders_never_share_a_name() {
    let parent = tempfile::tempdir().unwrap();
    let one = Scratch::unique(parent.path()).unwrap();
    let two = Scratch::unique(parent.path()).unwrap();
    assert_ne!(one.path(), two.path());
}

#[test]
fn a_folder_that_exists_is_refused_and_left_alone() {
    let parent = tempfile::tempdir().unwrap();
    let taken = parent.path().join("taken");
    std::fs::create_dir(&taken).unwrap();
    std::fs::write(taken.join("keep"), "x").unwrap();
    assert!(Scratch::exact(&taken).is_err());
    assert!(taken.join("keep").exists());
}

#[test]
fn a_link_put_in_its_place_is_removed_without_following_it() {
    let parent = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("keep"), "x").unwrap();
    let scratch = Scratch::exact(&parent.path().join("go")).unwrap();
    let path = scratch.path().to_path_buf();
    // The program swapped its folder for a link to somewhere else.
    std::fs::remove_dir(&path).unwrap();
    std::os::unix::fs::symlink(outside.path(), &path).unwrap();
    drop(scratch);
    assert!(std::fs::symlink_metadata(&path).is_err());
    assert!(outside.path().join("keep").exists());
}
