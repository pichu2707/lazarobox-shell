use std::fs;
use std::os::unix::fs::symlink;

use super::*;

fn touch(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"GGUF").unwrap();
}

#[test]
fn models_are_found_in_the_folders_and_below() {
    let root = tempfile::tempdir().unwrap();
    let models = root.path().join("models");
    touch(&models.join("qwen.gguf"));
    touch(&models.join("sub/llama-00001-of-00002.gguf"));
    touch(&models.join("sub/llama-00002-of-00002.gguf"));
    touch(&models.join("mmproj-f16.gguf"));
    touch(&models.join("readme.txt"));
    let cache = root.path().join("cache");
    touch(&cache.join("org_repo_tiny.gguf"));
    assert_eq!(
        find(&[models.clone(), cache.clone()]),
        vec![
            models.join("sub/llama-00001-of-00002.gguf"),
            cache.join("org_repo_tiny.gguf"),
            models.join("qwen.gguf"),
        ]
    );
}

// The Hugging Face cache: snapshot files are links into `blobs/`.
#[test]
fn a_hugging_face_snapshot_is_listed_by_its_snapshot_path() {
    let root = tempfile::tempdir().unwrap();
    let hub = root.path().join("hub");
    let repo = hub.join("models--org--repo");
    touch(&repo.join("blobs/0123abcd"));
    let snap = repo.join("snapshots/f00d");
    fs::create_dir_all(&snap).unwrap();
    symlink("../../blobs/0123abcd", snap.join("model-q4.gguf")).unwrap();
    assert_eq!(find(&[hub]), vec![snap.join("model-q4.gguf")]);
}

#[test]
fn links_to_folders_and_broken_links_are_not_followed() {
    let root = tempfile::tempdir().unwrap();
    let models = root.path().join("models");
    let outside = root.path().join("outside");
    touch(&outside.join("far.gguf"));
    fs::create_dir_all(&models).unwrap();
    symlink(&outside, models.join("link")).unwrap();
    symlink(root.path().join("gone.gguf"), models.join("broken.gguf")).unwrap();
    // A loop would hang a walk that follows links.
    symlink(&models, models.join("self")).unwrap();
    assert_eq!(find(&[models]), Vec::<PathBuf>::new());
}

#[test]
fn missing_folders_and_deep_files_are_skipped() {
    let root = tempfile::tempdir().unwrap();
    let models = root.path().join("models");
    let mut deep = models.clone();
    for level in 0..=MAX_DEPTH {
        deep = deep.join(format!("d{level}"));
    }
    touch(&deep.join("too-deep.gguf"));
    touch(&models.join("a/b/c/d/e/ok.gguf"));
    assert_eq!(
        find(&[root.path().join("nothing"), models.clone()]),
        vec![models.join("a/b/c/d/e/ok.gguf")]
    );
}

#[test]
fn the_same_folder_twice_lists_each_model_once() {
    let root = tempfile::tempdir().unwrap();
    touch(&root.path().join("m.gguf"));
    let dir = root.path().to_path_buf();
    assert_eq!(find(&[dir.clone(), dir.clone()]), vec![dir.join("m.gguf")]);
}
