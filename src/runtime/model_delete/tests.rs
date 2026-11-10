//! Deleting llama.cpp model files: only in a temporary home, through the
//! folders [`model_dirs`] names for it.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};

use super::*;
use crate::core::ai::gguf::{DirsEnv, model_dirs};
use crate::core::ai::picker::GgufFile;

/// A temporary home and the folders the scan reads in it.
struct Home {
    _root: tempfile::TempDir,
    root: PathBuf,
    dirs: Vec<PathBuf>,
}

impl Home {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().canonicalize().unwrap();
        let dirs = model_dirs(&DirsEnv {
            home: Some(path.join("home")),
            ..Default::default()
        });
        Home {
            _root: root,
            root: path,
            dirs,
        }
    }

    /// `~/.local/share/llama.cpp/models`.
    fn models(&self) -> PathBuf {
        self.dirs[0].clone()
    }

    /// The Hugging Face hub cache.
    fn hub(&self) -> PathBuf {
        self.dirs.last().unwrap().clone()
    }

    /// A folder next to the home: outside every scanned folder.
    fn outside(&self) -> PathBuf {
        self.root.join("outside")
    }
}

fn write(path: &Path, len: usize) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, vec![b'g'; len]).unwrap();
}

/// A Hugging Face repository `models--org--<name>` with `refs/main`.
fn new_repo(home: &Home, name: &str) -> PathBuf {
    let repo = home.hub().join(format!("models--org--{name}"));
    write(&repo.join("refs/main"), 0);
    fs::write(
        repo.join("refs/main"),
        "0123456789abcdef0123456789abcdef01234567",
    )
    .unwrap();
    repo
}

/// A blob of `len` bytes and a link to it at `snapshots/<commit>/<file>`.
fn linked(repo: &Path, commit: &str, file: &str, oid: &str, len: usize) -> PathBuf {
    write(&repo.join("blobs").join(oid), len);
    let at = repo.join("snapshots").join(commit).join(file);
    fs::create_dir_all(at.parent().unwrap()).unwrap();
    let depth = file.split('/').count() + 1;
    let up = "../".repeat(depth);
    symlink(format!("{up}blobs/{oid}"), &at).unwrap();
    at
}

fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

// Spec: a plain file goes, and the notice says what was freed (D4).
#[test]
fn a_plain_file_is_deleted_and_its_size_freed() {
    let home = Home::new();
    let file = home.models().join("qwen.gguf");
    write(&file, 7);
    let other = home.models().join("other.gguf");
    write(&other, 3);
    let plan = plan(&file, &home.dirs, &[]).unwrap();
    assert_eq!((plan.size(), plan.parts()), (7, 1));
    assert_eq!(delete(&file, &home.dirs, &[]), Ok(7));
    assert!(!exists(&file));
    assert!(exists(&other), "another model stays");
    assert!(home.models().is_dir(), "the models folder stays");
}

// Spec: a split model goes whole, its size the sum of its parts.
#[test]
fn a_split_model_goes_with_all_its_parts_and_their_sizes_add_up() {
    let home = Home::new();
    let dir = home.models().join("sub");
    let parts: Vec<PathBuf> = (1..=3)
        .map(|k| dir.join(format!("Big-Q4-{k:05}-of-00003.gguf")))
        .collect();
    for (k, part) in parts.iter().enumerate() {
        write(part, 10 * (k + 1));
    }
    let partial = dir.join("Big-Q4-00002-of-00003.gguf.incomplete");
    write(&partial, 1);
    let alone = dir.join("Big-Q4-00001-of-00002.gguf");
    write(&alone, 1);
    let plan = plan(&parts[0], &home.dirs, &[]).unwrap();
    assert_eq!((plan.size(), plan.parts()), (60, 3));
    assert_eq!(delete(&parts[0], &home.dirs, &[]), Ok(60));
    assert!(parts.iter().all(|p| !exists(p)), "every part goes");
    assert!(exists(&partial), "an .incomplete file is never touched");
    assert!(exists(&alone), "another split model stays");
}

#[test]
fn a_split_model_with_a_part_missing_deletes_the_parts_there() {
    let home = Home::new();
    let first = home.models().join("m-00001-of-00002.gguf");
    write(&first, 4);
    assert_eq!(plan(&first, &home.dirs, &[]).unwrap().parts(), 1);
    assert_eq!(delete(&first, &home.dirs, &[]), Ok(4));
}

// Spec: the snapshot link and its blob go together, and folders left empty
// go too.
#[test]
fn a_snapshot_link_goes_with_its_blob_and_the_emptied_repository() {
    let home = Home::new();
    let repo = new_repo(&home, "tiny");
    let link = linked(&repo, "c0ffee", "tiny-q4.gguf", "b10b", 9);
    let neighbour = new_repo(&home, "other");
    let kept = linked(&neighbour, "beef", "other.gguf", "0th3r", 2);
    let plan = plan(&link, &home.dirs, &[]).unwrap();
    assert_eq!((plan.size(), plan.parts()), (9, 1));
    assert_eq!(delete(&link, &home.dirs, &[]), Ok(9));
    assert!(!exists(&repo), "the repository folder is left with nothing");
    assert!(exists(&kept) && kept.is_file(), "another repository stays");
    assert!(home.hub().is_dir(), "the hub cache stays");
}

#[test]
fn a_split_model_in_the_hub_cache_goes_with_every_blob() {
    let home = Home::new();
    let repo = new_repo(&home, "split");
    let first = linked(&repo, "c1", "Q4/s-00001-of-00002.gguf", "aa", 5);
    let second = linked(&repo, "c1", "Q4/s-00002-of-00002.gguf", "bb", 6);
    assert_eq!(plan(&first, &home.dirs, &[]).unwrap().size(), 11);
    assert_eq!(delete(&first, &home.dirs, &[]), Ok(11));
    assert!(!exists(&second));
    assert!(!exists(&repo));
}

// Spec: a blob another snapshot still links to stays.
#[test]
fn a_blob_two_snapshots_share_stays_with_the_other_link() {
    let home = Home::new();
    let repo = new_repo(&home, "shared");
    let old = linked(&repo, "old", "m.gguf", "same", 8);
    let new = repo.join("snapshots/new/m.gguf");
    fs::create_dir_all(new.parent().unwrap()).unwrap();
    symlink("../../blobs/same", &new).unwrap();
    // Listed at 8 bytes, it frees nothing: the blob stays.
    assert_eq!(plan(&old, &home.dirs, &[]).unwrap().size(), 8);
    assert_eq!(delete(&old, &home.dirs, &[]), Ok(0));
    assert!(!exists(&old));
    assert!(
        !exists(&repo.join("snapshots/old")),
        "its empty snapshot goes"
    );
    assert!(new.is_file(), "the other link still reads its blob");
    assert!(exists(&repo.join("blobs/same")));
}

// Spec: only folders already empty go.
#[test]
fn a_snapshot_with_other_files_stays_and_so_does_the_repository() {
    let home = Home::new();
    let repo = new_repo(&home, "busy");
    let link = linked(&repo, "c", "m.gguf", "m1", 3);
    let readme = linked(&repo, "c", "README.md", "r1", 1);
    assert_eq!(delete(&link, &home.dirs, &[]), Ok(3));
    assert!(!exists(&link) && !exists(&repo.join("blobs/m1")));
    assert!(readme.is_file(), "the snapshot keeps its other file");
    assert!(exists(&repo.join("refs/main")));
}

// Spec: `.incomplete` and `mmproj` files are never touched.
#[test]
fn incomplete_blobs_and_mmproj_files_stay() {
    let home = Home::new();
    let repo = new_repo(&home, "vision");
    let link = linked(&repo, "c", "v-q4.gguf", "v1", 4);
    let mmproj = linked(&repo, "c", "mmproj-f16.gguf", "p1", 2);
    let partial = repo.join("blobs/x9.incomplete");
    write(&partial, 1);
    assert_eq!(delete(&link, &home.dirs, &[]), Ok(4));
    assert!(mmproj.is_file(), "the mmproj link and its blob stay");
    assert!(exists(&partial), "an .incomplete blob stays");
    assert!(exists(&repo), "a folder that is not empty stays");
    // An mmproj file is not a model the list shows.
    assert!(plan(&mmproj, &home.dirs, &[]).is_err());
    assert!(mmproj.is_file());
}

// What happens to refs/ and .no_exist: a repository left with no snapshot
// and no blob goes with its refs and its empty `.no_exist` markers; anything
// else in it keeps it.
#[test]
fn refs_and_empty_markers_go_with_the_repository_and_other_files_keep_it() {
    let home = Home::new();
    let gone = new_repo(&home, "gone");
    let link = linked(&gone, "c", "g.gguf", "g1", 2);
    write(&gone.join(".no_exist/c/tokenizer.json"), 0);
    assert_eq!(delete(&link, &home.dirs, &[]), Ok(2));
    assert!(!exists(&gone));

    let kept = new_repo(&home, "kept");
    let link = linked(&kept, "c", "k.gguf", "k1", 2);
    write(&kept.join("notes.txt"), 5);
    write(&kept.join(".no_exist/c/big"), 3);
    assert_eq!(delete(&link, &home.dirs, &[]), Ok(2));
    assert!(exists(&kept.join("notes.txt")));
    assert!(
        exists(&kept.join(".no_exist/c/big")),
        "a marker with bytes stays"
    );
    assert!(exists(&kept.join("refs/main")));
}

// Spec: a link that points outside the scanned folders is refused.
#[test]
fn a_link_out_of_the_scanned_folders_is_refused_and_nothing_goes() {
    let home = Home::new();
    let far = home.outside().join("real.gguf");
    write(&far, 5);
    let link = home.models().join("real.gguf");
    fs::create_dir_all(home.models()).unwrap();
    symlink(&far, &link).unwrap();
    assert!(plan(&link, &home.dirs, &[]).is_err());
    assert!(delete(&link, &home.dirs, &[]).is_err());
    assert!(exists(&link) && far.is_file());
}

#[test]
fn a_hub_link_to_another_repository_or_outside_its_blobs_is_refused() {
    let home = Home::new();
    let mine = new_repo(&home, "mine");
    let theirs = new_repo(&home, "theirs");
    write(&theirs.join("blobs/t1"), 4);
    let snap = mine.join("snapshots/c");
    fs::create_dir_all(&snap).unwrap();
    let cross = snap.join("cross.gguf");
    symlink("../../../models--org--theirs/blobs/t1", &cross).unwrap();
    let far = home.outside().join("far.bin");
    write(&far, 4);
    let out = snap.join("out.gguf");
    symlink(&far, &out).unwrap();
    for link in [&cross, &out] {
        assert!(delete(link, &home.dirs, &[]).is_err(), "{link:?}");
        assert!(exists(link));
    }
    assert!(exists(&theirs.join("blobs/t1")) && far.is_file());
}

#[test]
fn a_link_in_a_plain_folder_is_refused_even_inside_the_folders() {
    let home = Home::new();
    let real = home.models().join("real.gguf");
    write(&real, 5);
    let link = home.models().join("alias.gguf");
    symlink(&real, &link).unwrap();
    assert!(delete(&link, &home.dirs, &[]).is_err());
    assert!(exists(&link) && real.is_file());
}

// Spec: a `..` path and a path outside every scanned folder are refused.
#[test]
fn a_dot_dot_path_and_a_path_outside_the_folders_are_refused() {
    let home = Home::new();
    let far = home.outside().join("x.gguf");
    write(&far, 5);
    let sneaky = home.models().join("../../../../../outside/x.gguf");
    fs::create_dir_all(home.models()).unwrap();
    assert!(sneaky.is_file(), "the lexical path does reach the file");
    let relative = PathBuf::from("outside/x.gguf");
    for path in [&sneaky, &far, &relative] {
        assert!(plan(path, &home.dirs, &[]).is_err(), "{path:?}");
        assert!(delete(path, &home.dirs, &[]).is_err(), "{path:?}");
    }
    assert!(far.is_file());
}

#[test]
fn a_folder_inside_a_scanned_folder_that_links_out_is_refused() {
    let home = Home::new();
    let far = home.outside().join("x.gguf");
    write(&far, 5);
    fs::create_dir_all(home.models()).unwrap();
    symlink(home.outside(), home.models().join("door")).unwrap();
    let path = home.models().join("door/x.gguf");
    assert!(delete(&path, &home.dirs, &[]).is_err());
    assert!(far.is_file());
}

// Spec: only regular `.gguf` files the scan lists.
#[test]
fn a_folder_a_file_of_another_kind_and_a_vanished_file_are_refused() {
    let home = Home::new();
    let folder = home.models().join("dir.gguf");
    fs::create_dir_all(folder.join("inside")).unwrap();
    let text = home.models().join("notes.txt");
    write(&text, 2);
    let second = home.models().join("s-00002-of-00003.gguf");
    write(&second, 2);
    for path in [&folder, &text, &second] {
        assert!(delete(path, &home.dirs, &[]).is_err(), "{path:?}");
        assert!(exists(path), "{path:?}");
    }
    assert!(folder.join("inside").is_dir());
    // Measured on `d`, gone by `y`.
    let file = home.models().join("gone.gguf");
    write(&file, 2);
    assert!(plan(&file, &home.dirs, &[]).is_ok());
    fs::remove_file(&file).unwrap();
    assert!(delete(&file, &home.dirs, &[]).is_err());
}

// Spec: the runtime refuses a file the config uses, whatever its spelling.
#[test]
fn a_file_the_config_uses_is_refused_through_any_spelling() {
    let home = Home::new();
    let repo = new_repo(&home, "used");
    let link = linked(&repo, "c", "u.gguf", "u1", 3);
    // The config names the blob itself, the way no row shows it.
    let in_use = [repo.join("snapshots/c/../../blobs/u1")];
    assert!(plan(&link, &home.dirs, &in_use).is_err());
    assert!(delete(&link, &home.dirs, &in_use).is_err());
    assert!(link.is_file());
}

// Spec: a failure part way says what was removed and what was not.
#[test]
fn a_permission_error_part_way_reports_what_went_and_what_stayed() {
    let home = Home::new();
    let repo = new_repo(&home, "locked");
    let link = linked(&repo, "c", "l.gguf", "l1", 3);
    let blobs = repo.join("blobs");
    fs::set_permissions(&blobs, fs::Permissions::from_mode(0o555)).unwrap();
    // A user that ignores permissions cannot see the failure.
    let probe = blobs.join("probe");
    if fs::write(&probe, b"").is_ok() {
        let _ = fs::remove_file(&probe);
        fs::set_permissions(&blobs, fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    let answer = delete(&link, &home.dirs, &[]);
    fs::set_permissions(&blobs, fs::Permissions::from_mode(0o755)).unwrap();
    let why = answer.unwrap_err();
    assert!(why.starts_with("removed l.gguf"), "{why}");
    assert!(why.contains("not removed the blob of l.gguf"), "{why}");
    assert!(!exists(&link));
    assert!(exists(&blobs.join("l1")));
    assert!(exists(&repo), "a failure removes nothing more");
}

#[test]
fn a_failure_before_anything_went_just_says_why() {
    let home = Home::new();
    let file = home.models().join("ro.gguf");
    write(&file, 3);
    fs::set_permissions(home.models(), fs::Permissions::from_mode(0o555)).unwrap();
    let answer = delete(&file, &home.dirs, &[]);
    fs::set_permissions(home.models(), fs::Permissions::from_mode(0o755)).unwrap();
    if exists(&file) {
        let why = answer.unwrap_err();
        assert!(why.starts_with("cannot remove ro.gguf"), "{why}");
    }
}

fn ask(path: &Path) -> DeleteAsk {
    DeleteAsk {
        id: 7,
        target: DeleteTarget::Gguf(GgufFile {
            path: path.to_str().unwrap().to_owned(),
            size: None,
            parts: 1,
            in_use: Vec::new(),
        }),
    }
}

fn file_of(ask: &DeleteAsk) -> &GgufFile {
    match &ask.target {
        DeleteTarget::Gguf(file) => file,
        other => panic!("{other:?}"),
    }
}

// Spec: the confirmation shows the size; the reply what was freed (D4).
#[test]
fn measure_fills_the_size_and_parts_and_carry_out_the_freed_bytes() {
    let home = Home::new();
    let first = home.models().join("p-00001-of-00002.gguf");
    write(&first, 4);
    write(&home.models().join("p-00002-of-00002.gguf"), 6);
    let (measured, answer) = measure(ask(&first), &home.dirs);
    assert_eq!(answer, Ok(()));
    assert_eq!(
        (file_of(&measured).size, file_of(&measured).parts),
        (Some(10), 2)
    );
    assert!(exists(&first), "measuring removes nothing");
    let (done, answer) = carry_out(measured, &home.dirs);
    assert_eq!(answer, Ok(()));
    assert_eq!(file_of(&done).size, Some(10));
    assert_eq!(done.id, 7);
    assert!(!exists(&first));
}

#[test]
fn carry_out_refuses_an_ollama_target_and_a_file_in_use() {
    let home = Home::new();
    let ollama = DeleteAsk {
        id: 1,
        target: DeleteTarget::Ollama {
            name: "qwen3:4b".to_owned(),
            size: None,
        },
    };
    assert!(carry_out(ollama.clone(), &home.dirs).1.is_err());
    assert!(measure(ollama, &home.dirs).1.is_err());
    let file = home.models().join("used.gguf");
    write(&file, 2);
    let mut used = ask(&file);
    if let DeleteTarget::Gguf(f) = &mut used.target {
        f.in_use = vec![file.clone()];
    }
    assert!(carry_out(used, &home.dirs).1.is_err());
    assert!(file.is_file());
}

// Safety: tidying a repository never follows a link out of it. The model
// itself may go; nothing outside does, and the repository folder stays.
#[test]
fn a_no_exist_folder_that_links_out_is_left_alone() {
    let home = Home::new();
    let repo = new_repo(&home, "nx");
    let link = linked(&repo, "c", "n.gguf", "n1", 2);
    let far = home.outside().join("markers");
    write(&far.join("c9/keep-me"), 0);
    symlink(&far, repo.join(".no_exist")).unwrap();
    let _ = delete(&link, &home.dirs, &[]);
    assert!(exists(&far.join("c9/keep-me")));
    assert!(exists(&repo));
}

#[test]
fn a_no_exist_commit_that_links_out_is_left_alone() {
    let home = Home::new();
    let repo = new_repo(&home, "nc");
    let link = linked(&repo, "c", "n.gguf", "n1", 2);
    let far = home.outside().join("commit");
    write(&far.join("keep-me"), 0);
    fs::create_dir_all(repo.join(".no_exist")).unwrap();
    symlink(&far, repo.join(".no_exist/c9")).unwrap();
    let _ = delete(&link, &home.dirs, &[]);
    assert!(exists(&far.join("keep-me")));
    assert!(exists(&repo));
}

#[test]
fn a_refs_folder_that_links_out_is_left_alone() {
    let home = Home::new();
    let repo = home.hub().join("models--org--rl");
    let link = linked(&repo, "c", "r.gguf", "r1", 2);
    let far = home.outside().join("refs");
    write(&far.join("main"), 0);
    fs::write(far.join("main"), "0123456789abcdef0123456789abcdef01234567").unwrap();
    symlink(&far, repo.join("refs")).unwrap();
    let _ = delete(&link, &home.dirs, &[]);
    assert!(exists(&far.join("main")));
    assert!(exists(&repo));
}

// Safety: a snapshot that cannot be read may still link to the blob, so the
// delete is refused rather than guessed.
#[test]
fn an_unreadable_snapshot_refuses_the_delete() {
    let home = Home::new();
    let repo = new_repo(&home, "hidden");
    let link = linked(&repo, "a", "m.gguf", "same", 4);
    let other = repo.join("snapshots/b");
    fs::create_dir_all(&other).unwrap();
    symlink("../../blobs/same", other.join("m.gguf")).unwrap();
    fs::set_permissions(&other, fs::Permissions::from_mode(0o000)).unwrap();
    // A user that ignores permissions reads it anyway.
    if fs::read_dir(&other).is_ok() {
        fs::set_permissions(&other, fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    let answer = delete(&link, &home.dirs, &[]);
    fs::set_permissions(&other, fs::Permissions::from_mode(0o755)).unwrap();
    let why = answer.unwrap_err();
    assert!(why.starts_with("cannot check the other snapshots"), "{why}");
    assert!(exists(&link) && exists(&repo.join("blobs/same")));
}

// Safety: right before a removal, a file that is no longer the one checked
// stays. `delete` plans again first, so this reaches `remove` directly.
#[test]
fn a_file_replaced_after_its_check_is_not_removed() {
    let home = Home::new();
    let file = home.models().join("swap.gguf");
    write(&file, 3);
    let checked = plan(&file, &home.dirs, &[]).unwrap().parts[0].id;
    // Renamed over it while the old file still exists: another inode.
    let fresh = home.models().join("fresh.tmp");
    write(&fresh, 3);
    fs::rename(&fresh, &file).unwrap();
    let why = remove(&file, checked, None).unwrap_err();
    assert_eq!(why, "swap.gguf changed since it was checked");
    assert!(file.is_file());
}

#[test]
fn a_file_that_became_a_link_after_its_check_is_not_removed() {
    let home = Home::new();
    let repo = new_repo(&home, "kind");
    let link = linked(&repo, "c", "k.gguf", "k1", 3);
    let plan = plan(&link, &home.dirs, &[]).unwrap();
    let (id, blob) = (plan.parts[0].id, plan.parts[0].blob.as_ref().unwrap());
    assert!(
        remove(&link, id, None).is_err(),
        "a link is not a plain file"
    );
    assert!(
        remove(&blob.path, id, None).is_err(),
        "another file's identity"
    );
    assert!(exists(&link) && blob.path.is_file());
}

// Review: a link to the blob nested deeper than the scan reads cannot be
// ruled out, so the delete is refused rather than breaking that link.
#[test]
fn a_link_nested_deeper_than_the_scan_refuses_the_delete() {
    let home = Home::new();
    let repo = new_repo(&home, "deep");
    let ours = linked(&repo, "old", "m.gguf", "same", 8);
    let nested = "a/b/c/d/e/f/g/m.gguf";
    let deep = linked(&repo, "new", nested, "same", 8);
    let answer = delete(&ours, &home.dirs, &[]);
    assert!(
        answer
            .as_ref()
            .is_err_and(|e| e.contains("cannot check the other snapshots")),
        "{answer:?}"
    );
    assert!(exists(&ours));
    assert!(deep.is_file(), "the deep link still reads its blob");
}
