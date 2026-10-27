use std::os::unix::fs::PermissionsExt;

use std::path::Path;

use super::*;

#[test]
fn a_scratch_folder_is_private_canonical_and_gone_when_dropped() {
    let parent = tempfile::tempdir().unwrap();
    let parent = parent.path().canonicalize().unwrap();
    let scratch = Scratch::unique(&parent.join("tmp"), "run").unwrap();
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
    let one = Scratch::unique(parent.path(), "run").unwrap();
    let two = Scratch::unique(parent.path(), "run").unwrap();
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

#[test]
fn a_name_taken_in_advance_is_skipped_not_a_hard_error() {
    let parent = tempfile::tempdir().unwrap();
    let parent = parent.path().canonicalize().unwrap();
    // What a program in the project could plant: the names a run would pick.
    std::fs::create_dir(parent.join("go-1")).unwrap();
    std::fs::write(parent.join("go-2"), "a file").unwrap();
    std::os::unix::fs::symlink("/nonexistent", parent.join("go-3")).unwrap();
    let mut tried = Vec::new();
    let scratch = Scratch::unique_with(&parent, |attempt| {
        tried.push(attempt);
        format!("go-{}", attempt + 1)
    })
    .unwrap();
    assert_eq!(scratch.path(), parent.join("go-4"));
    assert_eq!(tried, [0, 1, 2, 3]);
}

#[test]
fn every_name_taken_gives_up_after_a_bounded_number_of_tries() {
    let parent = tempfile::tempdir().unwrap();
    std::fs::create_dir(parent.path().join("same")).unwrap();
    let mut tries = 0;
    let err = Scratch::unique_with(parent.path(), |_| {
        tries += 1;
        "same".to_owned()
    })
    .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
    assert!((1..=64).contains(&tries), "{tries}");
}

#[test]
fn the_names_of_two_runs_differ_in_more_than_a_counter() {
    let parent = tempfile::tempdir().unwrap();
    let one = Scratch::unique(parent.path(), "run").unwrap();
    let name = one
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    // prefix, pid, counter and a time part: the first name is not guessable.
    assert!(
        name.starts_with("run-") && name.matches('-').count() >= 3,
        "{name}"
    );
}

#[test]
fn a_read_only_subfolder_left_by_the_program_does_not_stop_the_removal() {
    let parent = tempfile::tempdir().unwrap();
    let scratch = Scratch::unique(parent.path(), "run").unwrap();
    let path = scratch.path().to_path_buf();
    // `go` and `cargo` leave these: a module cache is made read-only.
    std::fs::create_dir_all(path.join("mod/deep")).unwrap();
    std::fs::write(path.join("mod/deep/f"), "x").unwrap();
    std::fs::set_permissions(
        path.join("mod/deep"),
        std::fs::Permissions::from_mode(0o500),
    )
    .unwrap();
    std::fs::set_permissions(path.join("mod"), std::fs::Permissions::from_mode(0o500)).unwrap();
    assert_eq!(scratch.finish(), Vec::<PathBuf>::new());
    assert!(!path.exists());
}

#[test]
fn the_chmod_never_follows_a_link_out_of_the_folder() {
    let parent = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::set_permissions(outside.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
    let scratch = Scratch::unique(parent.path(), "run").unwrap();
    let path = scratch.path().to_path_buf();
    std::fs::create_dir(path.join("locked")).unwrap();
    std::fs::write(path.join("locked/f"), "x").unwrap();
    std::fs::set_permissions(path.join("locked"), std::fs::Permissions::from_mode(0o500)).unwrap();
    std::os::unix::fs::symlink(outside.path(), path.join("link")).unwrap();
    assert!(scratch.finish().is_empty());
    let mode = std::fs::metadata(outside.path())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o500, "a link's target is never touched");
    std::fs::set_permissions(outside.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn what_cannot_be_removed_is_reported_not_swallowed() {
    let parent = tempfile::tempdir().unwrap();
    let fence = parent.path().join("fence");
    std::fs::create_dir(&fence).unwrap();
    let scratch = Scratch::unique(&fence, "run").unwrap();
    let path = scratch.path().to_path_buf();
    // Nothing can be unlinked from a folder that is not writable.
    std::fs::set_permissions(&fence, std::fs::Permissions::from_mode(0o500)).unwrap();
    let left = scratch.finish();
    std::fs::set_permissions(&fence, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(left, std::slice::from_ref(&path));
    assert!(path.exists());
}

/// How deep the deep trees go: as deep as a path can go. `PATH_MAX` is 4096
/// on Linux (about two thousand levels) and 1024 on macOS, where a tree of
/// 1500 levels cannot even be made (`ENAMETOOLONG`); 400 levels is 800 bytes
/// of name plus the temp folder.
const DEEP: usize = if cfg!(target_os = "macos") { 400 } else { 1500 };

/// A chain of `levels` one-letter folders under `base`.
fn deep(base: &Path, levels: usize) -> PathBuf {
    let mut path = base.to_path_buf();
    for _ in 0..levels {
        path.push("d");
    }
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn the_chmod_walk_uses_no_recursion_so_a_deep_tree_cannot_overflow_the_stack() {
    let parent = tempfile::tempdir().unwrap();
    let base = parent.path().join("t");
    std::fs::create_dir(&base).unwrap();
    let bottom = deep(&base, DEEP);
    std::fs::set_permissions(&bottom, std::fs::Permissions::from_mode(0o500)).unwrap();
    // A small stack: one frame per level would not fit in it.
    let walker = base.clone();
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(move || make_removable(&walker))
        .unwrap()
        .join()
        .unwrap();
    let mode = std::fs::metadata(&bottom).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700, "the deepest folder was reached");
}

#[test]
fn a_deep_read_only_tree_is_removed_by_a_scratch_finish() {
    let parent = tempfile::tempdir().unwrap();
    let scratch = Scratch::unique(parent.path(), "run").unwrap();
    let path = scratch.path().to_path_buf();
    let bottom = deep(&path, DEEP);
    std::fs::set_permissions(&bottom, std::fs::Permissions::from_mode(0o500)).unwrap();
    std::fs::set_permissions(path.join("d"), std::fs::Permissions::from_mode(0o500)).unwrap();
    assert_eq!(scratch.finish(), Vec::<PathBuf>::new());
    assert!(!path.exists());
}

#[test]
fn the_chmod_walk_stops_at_its_depth_bound() {
    let parent = tempfile::tempdir().unwrap();
    let bottom = deep(parent.path(), 20);
    std::fs::set_permissions(&bottom, std::fs::Permissions::from_mode(0o500)).unwrap();
    walk(parent.path(), 10, usize::MAX);
    let mode = std::fs::metadata(&bottom).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o500, "past the bound nothing is changed");
    walk(parent.path(), 100, usize::MAX);
    let mode = std::fs::metadata(&bottom).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
}

#[test]
fn the_chmod_walk_stops_at_its_entry_bound() {
    let parent = tempfile::tempdir().unwrap();
    for n in 0..30 {
        let dir = parent.path().join(format!("f{n:02}"));
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    }
    walk(parent.path(), usize::MAX, 10);
    let changed = (0..30)
        .filter(|n| {
            let dir = parent.path().join(format!("f{n:02}"));
            std::fs::metadata(dir).unwrap().permissions().mode() & 0o777 == 0o700
        })
        .count();
    assert!(changed < 30, "the walk visited every entry");
    walk(parent.path(), usize::MAX, usize::MAX);
}
