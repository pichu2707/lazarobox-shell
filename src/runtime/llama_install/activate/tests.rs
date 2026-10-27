use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;

use super::*;
use crate::runtime::hf::tests::sha;
use crate::runtime::llama_install::unpack::tests::{Entry, archive, dir, file};

macro_rules! need_tar {
    () => {
        let have = |name| crate::runtime::ai_detect::find_on_path(name, &path()).is_some();
        if !have("tar") || !have("gzip") {
            eprintln!("SKIPPED: tar or gzip is not installed; the install checks did not run");
            return;
        }
    };
}

fn path() -> OsString {
    std::env::var_os("PATH").unwrap_or_default()
}

/// A `.download/<tag>.tar.gz` holding a fake `llama-server` that runs
/// `script`, and the release that describes it.
fn download(root: &Path, tag: &str, script: &str) -> (PathBuf, Release) {
    let work = root.join(".download").join(tag);
    fs::create_dir_all(&work).unwrap();
    let top = format!("llama-{tag}");
    let (top_dir, server) = (format!("{top}/"), format!("{top}/llama-server"));
    let lib = format!("{top}/libllama.so");
    let made = archive(
        &work,
        &[
            dir(&top_dir),
            Entry {
                mode: 0o755,
                ..file(&server, script.as_bytes())
            },
            file(&lib, b"lib"),
        ],
    );
    let to = root.join(".download").join(format!("{tag}.tar.gz"));
    fs::rename(&made, &to).unwrap();
    fs::remove_dir_all(&work).unwrap();
    let release = Release {
        tag: tag.to_owned(),
        asset: format!("{tag}.tar.gz"),
        size: fs::metadata(&to).unwrap().len(),
        sha256: sha(&fs::read(&to).unwrap()),
    };
    (to, release)
}

const OK: &str = "#!/bin/sh\necho version: 1\n";
const BROKEN: &str = "#!/bin/sh\nexit 1\n";

async fn put(root: &Path, tag: &str, script: &str) -> Result<(), String> {
    let (archive, release) = download(root, tag, script);
    install(root, &release, &archive, &path()).await
}

fn tags(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| install::tag_number(n).is_some())
        .collect();
    names.sort();
    names
}

fn current(root: &Path) -> String {
    fs::read_link(root.join("current"))
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

fn owned() -> (tempfile::TempDir, PathBuf) {
    let data = tempfile::tempdir().unwrap();
    let root = install::owned_root(data.path());
    fs::create_dir_all(&root).unwrap();
    (data, root)
}

// Spec (Layout after install): the tag folder holds llama-server directly
// and `current` points at it, so `current/llama-server` is on the PATH.
#[tokio::test]
async fn install_lays_out_the_tag_and_current() {
    need_tar!();
    let (data, root) = owned();
    put(&root, "b9000", OK).await.unwrap();
    assert_eq!(current(&root), "b9000");
    let binary = root.join("current").join("llama-server");
    assert!(root.join("b9000/libllama.so").is_file());
    assert_eq!(
        binary.metadata().unwrap().permissions().mode() & 0o111,
        0o111
    );
    assert!(install::is_owned(&binary, data.path()));
    // The archive is spent; no staging or temporary link is left.
    assert!(!root.join(".download/b9000.tar.gz").exists());
    let left: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".staging") || n == "current.new")
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn the_owned_root_falls_back_to_home_local_share() {
    let home = Path::new("/home/someone");
    let data = crate::core::desktop::data_home(None, Some(home.as_os_str())).unwrap();
    assert_eq!(
        install::owned_root(&data),
        home.join(".local/share/lazarobox/llama.cpp")
    );
}

// Spec (Version check): a llama-server that does not run never becomes
// current; the previous install stays and the new folder is removed.
#[tokio::test]
async fn a_server_that_does_not_run_keeps_the_previous_install() {
    need_tar!();
    let (_data, root) = owned();
    put(&root, "b9000", OK).await.unwrap();
    let why = put(&root, "b9010", BROKEN).await.unwrap_err();
    assert!(why.contains("does not run"), "{why}");
    assert_eq!(current(&root), "b9000");
    assert_eq!(tags(&root), ["b9000"]);
    assert!(!root.join("current.new").exists());
    assert!(root.join(".download/b9010.tar.gz").exists());
}

#[tokio::test]
async fn a_first_install_that_does_not_run_leaves_nothing_active() {
    need_tar!();
    let (_data, root) = owned();
    put(&root, "b9000", BROKEN).await.unwrap_err();
    assert!(tags(&root).is_empty());
    assert!(fs::symlink_metadata(root.join("current")).is_err());
}

// Assumption A1: older tags go, the immediately previous one stays.
#[tokio::test]
async fn prune_keeps_the_previous_tag_only() {
    need_tar!();
    let (_data, root) = owned();
    for tag in ["b9000", "b9010", "b9020"] {
        put(&root, tag, OK).await.unwrap();
    }
    assert_eq!(tags(&root), ["b9010", "b9020"]);
    assert_eq!(current(&root), "b9020");
}

// The tag `current` points at is not installed over: it would be moved aside
// while a running llama-server may use it.
#[tokio::test]
async fn the_current_tag_is_not_installed_again() {
    need_tar!();
    let (_data, root) = owned();
    put(&root, "b9000", OK).await.unwrap();
    let why = put(&root, "b9000", "#!/bin/sh\necho other\n")
        .await
        .unwrap_err();
    assert_eq!(why, "b9000 is already installed");
    assert_eq!(
        fs::read_to_string(root.join("b9000/llama-server")).unwrap(),
        OK
    );
    assert_eq!(current(&root), "b9000");
    // Nothing was left behind, and the download stays for another try.
    assert!(!fs::read_dir(&root).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".staging")
    }));
    assert!(root.join(".download/b9000.tar.gz").exists());
}

// The previous tag (kept for rollback) can be installed again.
#[tokio::test]
async fn installing_the_previous_tag_again_works() {
    need_tar!();
    let (_data, root) = owned();
    put(&root, "b9000", OK).await.unwrap();
    put(&root, "b9010", OK).await.unwrap();
    put(&root, "b9000", OK).await.unwrap();
    assert_eq!(current(&root), "b9000");
    assert_eq!(tags(&root), ["b9000", "b9010"]);
}

#[tokio::test]
async fn a_failed_reinstall_of_a_kept_tag_puts_the_old_one_back() {
    need_tar!();
    let (_data, root) = owned();
    put(&root, "b9000", OK).await.unwrap();
    put(&root, "b9010", OK).await.unwrap();
    put(&root, "b9000", BROKEN).await.unwrap_err();
    let script = fs::read_to_string(root.join("b9000/llama-server")).unwrap();
    assert_eq!(script, OK);
    assert_eq!(current(&root), "b9010");
}

// Spec (Another install running): a held lock refuses install and remove.
#[tokio::test]
async fn a_held_lock_refuses_install_and_remove() {
    need_tar!();
    let (data, root) = owned();
    let (archive, release) = download(&root, "b9000", OK);
    let held = lock(&root).unwrap();
    let why = install(&root, &release, &archive, &path())
        .await
        .unwrap_err();
    assert_eq!(why, LOCKED);
    let binary = root.join("current/llama-server");
    assert_eq!(remove(&binary, data.path()).await, Err(LOCKED.to_owned()));
    drop(held);
    install(&root, &release, &archive, &path()).await.unwrap();
}

#[tokio::test]
async fn a_stale_staging_folder_is_swept() {
    need_tar!();
    let (_data, root) = owned();
    let stale = root.join(".staging-1");
    fs::create_dir_all(stale.join("out")).unwrap();
    fs::write(stale.join("out/junk"), "x").unwrap();
    put(&root, "b9000", OK).await.unwrap();
    assert!(!stale.exists());
}

#[tokio::test]
async fn without_tar_the_install_says_so() {
    let (_data, root) = owned();
    let (archive, release) = download(&root, "b9000", OK);
    let none = OsString::from("/nonexistent");
    let why = install(&root, &release, &archive, &none).await.unwrap_err();
    assert_eq!(why, super::super::unpack::NO_TAR);
}

// The archive was checked when it was downloaded; a same-user process can
// replace it afterwards. The install checks the bytes it unpacks.
#[tokio::test]
async fn an_archive_swapped_after_the_download_is_refused() {
    need_tar!();
    let (_data, root) = owned();
    let (archive, release) = download(&root, "b9000", OK);
    let (other, _) = download(&root, "b9001", "#!/bin/sh\necho evil\n");
    fs::rename(&other, &archive).unwrap();
    let why = install(&root, &release, &archive, &path())
        .await
        .unwrap_err();
    assert!(why.contains("changed after"), "{why}");
    assert!(tags(&root).is_empty());
    assert!(fs::symlink_metadata(root.join("current")).is_err());
}

#[tokio::test]
async fn an_archive_replaced_by_a_symlink_is_refused() {
    need_tar!();
    let (_data, root) = owned();
    let (archive, release) = download(&root, "b9000", OK);
    let real = root.join("real.tar.gz");
    fs::rename(&archive, &real).unwrap();
    std::os::unix::fs::symlink(&real, &archive).unwrap();
    let why = install(&root, &release, &archive, &path())
        .await
        .unwrap_err();
    assert!(why.contains("symlink"), "{why}");
    assert!(tags(&root).is_empty());
}

#[tokio::test]
async fn a_hostile_archive_installs_nothing() {
    need_tar!();
    let (_data, root) = owned();
    let work = root.join(".download");
    fs::create_dir_all(&work).unwrap();
    let bad = archive(&work, &[file("llama-b9000/../evil", b"x")]);
    let release = Release {
        tag: "b9000".to_owned(),
        asset: "a.tar.gz".to_owned(),
        size: 0,
        sha256: sha(&fs::read(&bad).unwrap()),
    };
    install(&root, &release, &bad, &path()).await.unwrap_err();
    assert!(tags(&root).is_empty());
    assert!(!root.join("evil").exists());
}

// Spec (Remove): the owned folder goes, with older tags and the download.
#[tokio::test]
async fn remove_deletes_the_owned_install() {
    need_tar!();
    let (data, root) = owned();
    put(&root, "b9000", OK).await.unwrap();
    put(&root, "b9010", OK).await.unwrap();
    remove(&root.join("current/llama-server"), data.path())
        .await
        .unwrap();
    assert!(!root.exists());
    assert!(data.path().join("lazarobox").exists());
}

// Assumption A3: a system binary is never touched.
#[tokio::test]
async fn remove_refuses_a_binary_it_does_not_own() {
    let (data, root) = owned();
    let system = tempfile::tempdir().unwrap();
    let binary = system.path().join("llama-server");
    fs::write(&binary, "x").unwrap();
    for target in [
        binary.clone(),
        root.join("../../../etc/llama-server"),
        install::owned_root(data.path()),
    ] {
        assert!(remove(&target, data.path()).await.is_err(), "{target:?}");
    }
    assert!(binary.exists() && root.exists());
}

#[tokio::test]
async fn removing_what_is_not_there_is_fine() {
    let data = tempfile::tempdir().unwrap();
    let binary = install::current_dir(data.path()).join("llama-server");
    assert_eq!(remove(&binary, data.path()).await, Ok(()));
}

// A root that is a dangling symlink loses just the link, and does not wait
// on a lock it cannot create.
#[tokio::test]
async fn a_dangling_symlink_root_is_removed_as_a_link() {
    let data = tempfile::tempdir().unwrap();
    let root = install::owned_root(data.path());
    fs::create_dir_all(root.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(data.path().join("nowhere"), &root).unwrap();
    let binary = root.join("current/llama-server");
    remove(&binary, data.path()).await.unwrap();
    assert!(fs::symlink_metadata(&root).is_err());
}

#[tokio::test]
async fn a_symlink_root_to_a_folder_loses_only_the_link() {
    let data = tempfile::tempdir().unwrap();
    let root = install::owned_root(data.path());
    fs::create_dir_all(root.parent().unwrap()).unwrap();
    let real = data.path().join("real");
    fs::create_dir(&real).unwrap();
    fs::write(real.join("keep"), "x").unwrap();
    std::os::unix::fs::symlink(&real, &root).unwrap();
    remove(&root.join("current/llama-server"), data.path())
        .await
        .unwrap();
    assert!(fs::symlink_metadata(&root).is_err());
    assert!(real.join("keep").exists());
}

// An install that left a closed folder behind (or an archive that made one)
// must not make the remove, the prune or the staging sweep fail forever.
fn close(dir: &Path) {
    fs::set_permissions(dir, fs::Permissions::from_mode(0o000)).unwrap();
}

#[tokio::test]
async fn remove_and_sweep_get_past_closed_folders() {
    let (data, root) = owned();
    let stale = root.join(".staging-1/out/a");
    fs::create_dir_all(stale.join("b")).unwrap();
    close(&stale.join("b"));
    close(&stale);
    let old = root.join("b8000/lib");
    fs::create_dir_all(&old).unwrap();
    close(&old);
    sweep_staging(&root);
    assert!(!root.join(".staging-1").exists());
    prune(&root, "b9000", None);
    assert!(!root.join("b8000").exists());
    let again = root.join("b7000/lib");
    fs::create_dir_all(&again).unwrap();
    close(&again);
    remove(&root.join("current/llama-server"), data.path())
        .await
        .unwrap();
    assert!(!root.exists());
}

// Closed folders from a real archive: the install opens them before its
// walk and the staging goes whole.
#[tokio::test]
async fn an_archive_with_closed_folders_installs_and_leaves_no_staging() {
    need_tar!();
    let (_data, root) = owned();
    let work = root.join(".download");
    fs::create_dir_all(&work).unwrap();
    let top = "llama-b9000";
    let made = archive(
        &work,
        &[
            dir("llama-b9000/"),
            Entry {
                mode: 0o755,
                ..file("llama-b9000/llama-server", OK.as_bytes())
            },
            Entry {
                mode: 0o000,
                ..dir("llama-b9000/lib/")
            },
            file("llama-b9000/lib/x", b"x"),
        ],
    );
    let release = Release {
        tag: "b9000".to_owned(),
        asset: "a.tar.gz".to_owned(),
        size: 0,
        sha256: sha(&fs::read(&made).unwrap()),
    };
    install(&root, &release, &made, &path()).await.unwrap();
    assert!(root.join(top.replace("llama-", "")).join("lib/x").is_file());
    assert!(!fs::read_dir(&root).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".staging")
    }));
}

// A second process on the lock, not just a second handle: flock(1) holds it
// from outside this process.
#[tokio::test]
async fn a_lock_held_by_another_process_is_respected() {
    let (_data, root) = owned();
    let Some(flock) = crate::runtime::ai_detect::find_on_path("flock", &path()) else {
        eprintln!("SKIPPED: flock(1) is not installed; the cross-process lock was not checked");
        return;
    };
    let mut child = std::process::Command::new(flock)
        .arg(root.join(".lock"))
        .args(["sleep", "30"])
        .process_group(0)
        .spawn()
        .unwrap();
    // Wait until the child really holds it (the lock is not ours yet).
    let mut held = false;
    for _ in 0..100 {
        if lock(&root).is_err() {
            held = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let outcome = lock(&root).err();
    // flock(1) runs `sleep` as a child that keeps the lock: stop the group.
    // SAFETY: signalling the process group of a child spawned just above.
    unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
    let _ = child.wait();
    assert!(held, "flock(1) never took the lock");
    assert_eq!(outcome.as_deref(), Some(LOCKED));
    assert!(lock(&root).is_ok());
}

// The build that was installed is recorded next to `current`, and a plain
// install after a Vulkan one clears it.
#[tokio::test]
async fn install_records_whether_the_build_is_vulkan() {
    need_tar!();
    let (_data, root) = owned();
    assert!(!installed_vulkan(&root));
    let (archive, mut release) = download(&root, "b9000", OK);
    release.asset = "llama-b9000-bin-ubuntu-vulkan-x64.tar.gz".to_owned();
    install(&root, &release, &archive, &path()).await.unwrap();
    assert!(installed_vulkan(&root));
    put(&root, "b9100", OK).await.unwrap();
    assert!(!installed_vulkan(&root));
}
