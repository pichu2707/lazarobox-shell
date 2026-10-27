use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use super::*;
use crate::core::ai::install::{Platform, Variant, asset_name};
use crate::runtime::hf::tests::{curl_available, file_server, ok, serve, sha};
use crate::runtime::llama_install::{NO_GH, RATE_LIMITED};

macro_rules! need_curl {
    () => {
        if !curl_available() {
            eprintln!("SKIPPED: curl is not installed; the curl-backed checks did not run");
            return;
        }
    };
}

const BODY: &[u8] = b"0123456789";

fn real_path() -> OsString {
    std::env::var_os("PATH").unwrap_or_default()
}

fn release() -> Release {
    Release {
        tag: "b9000".to_owned(),
        asset: "llama-b9000-bin-ubuntu-x64.tar.gz".to_owned(),
        size: BODY.len() as u64,
        sha256: sha(BODY),
    }
}

/// An installer whose curl talks to `base`, with no `gh` anywhere on its
/// `PATH`, and the events it sends.
fn installer(base: &str, home: Option<&Path>) -> (Installer, mpsc::Receiver<AppEvent>) {
    let (tx, rx) = mpsc::channel(32);
    let base = base.to_owned();
    let nowhere = OsString::from("/nonexistent");
    let installer =
        Installer::new(nowhere, home.map(Path::to_path_buf), tx).with_curl(move |api| {
            if api {
                Curl::github_api(real_path()).local(&base)
            } else {
                Curl::github(real_path()).local(&base)
            }
        });
    (installer, rx)
}

async fn next(rx: &mut mpsc::Receiver<AppEvent>) -> AppEvent {
    tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("an event arrives")
        .expect("the channel is open")
}

/// The next reply, with every event before it.
async fn reply_of(
    rx: &mut mpsc::Receiver<AppEvent>,
) -> (Vec<AppEvent>, u64, Result<Reply, String>) {
    let mut before = Vec::new();
    loop {
        match next(rx).await {
            AppEvent::InstallReply(id, answer) => return (before, id, answer),
            other => before.push(other),
        }
    }
}

/// Waits until the server got a request: the download is under way.
async fn request_seen(seen: &Arc<Mutex<Vec<String>>>) {
    for _ in 0..1000 {
        if !seen.lock().unwrap().is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the server never got a request");
}

// Spec (Newest matching tag wins): the lookup answers with the release.
#[tokio::test]
async fn resolve_replies_with_the_release_under_the_same_id() {
    need_curl!();
    let asset = asset_name("b9000", Platform::CURRENT, Variant::Cpu).expect("a build here");
    let body = format!(
        r#"[{{"tag_name":"b9000","assets":[{{"name":"{asset}","size":1234,"digest":"sha256:{}"}}]}}]"#,
        "a".repeat(64)
    );
    let (base, _) = serve(Box::new(move |_| ok(&body)));
    let (mut installer, mut rx) = installer(&base, None);
    installer.ask(InstallAsk::Resolve(4, Variant::Cpu));
    let (_, id, answer) = reply_of(&mut rx).await;
    assert_eq!(id, 4);
    let Ok(Reply::Resolved(found)) = answer else {
        panic!("not resolved: {answer:?}");
    };
    assert_eq!((found.tag.as_str(), found.asset), ("b9000", asset));
}

#[tokio::test]
async fn a_failed_lookup_replies_with_its_message() {
    need_curl!();
    let (base, _) = serve(Box::new(|_| {
        b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
    }));
    let (mut installer, mut rx) = installer(&base, None);
    installer.ask(InstallAsk::Resolve(1, Variant::Cpu));
    let (_, id, answer) = reply_of(&mut rx).await;
    assert_eq!((id, answer), (1, Err(RATE_LIMITED.to_owned())));
}

// Spec (Optional attestation): the warning is sent before the reply.
#[tokio::test]
async fn a_download_reports_progress_and_the_attestation_warning_then_replies() {
    need_curl!();
    let (base, _) = file_server(BODY);
    let home = tempfile::tempdir().unwrap();
    let (mut installer, mut rx) = installer(&base, Some(home.path()));
    installer.ask(InstallAsk::Download(7, release()));
    let (before, id, answer) = reply_of(&mut rx).await;
    assert_eq!((id, answer), (7, Ok(Reply::Downloaded)));
    assert!(
        before.contains(&AppEvent::InstallWarning(NO_GH.to_owned())),
        "{before:?}"
    );
    assert!(
        before.contains(&AppEvent::InstallProgress {
            id: 7,
            done: 10,
            total: 10
        }),
        "{before:?}"
    );
    let root = install::owned_root(home.path());
    assert_eq!(
        std::fs::read(archive_path(&root, &release())).unwrap(),
        BODY
    );
}

// Design D9: a cancel still ends with the reply of the download's own id.
#[tokio::test]
async fn a_cancel_replies_for_the_download_it_stopped() {
    need_curl!();
    let (base, seen) = serve(Box::new(|_| {
        thread::sleep(Duration::from_secs(30));
        Vec::new()
    }));
    let home = tempfile::tempdir().unwrap();
    let (mut installer, mut rx) = installer(&base, Some(home.path()));
    installer.ask(InstallAsk::Download(5, release()));
    request_seen(&seen).await;
    installer.ask(InstallAsk::Cancel);
    let (_, id, answer) = reply_of(&mut rx).await;
    assert_eq!((id, answer), (5, Err(CANCELLED.to_owned())));
}

// Spec (one install at a time): a second step is refused, the first goes on.
#[tokio::test]
async fn a_second_download_is_refused_while_one_runs() {
    need_curl!();
    let (base, seen) = serve(Box::new(|_| {
        thread::sleep(Duration::from_secs(30));
        Vec::new()
    }));
    let home = tempfile::tempdir().unwrap();
    let (mut installer, mut rx) = installer(&base, Some(home.path()));
    installer.ask(InstallAsk::Download(1, release()));
    request_seen(&seen).await;
    installer.ask(InstallAsk::Download(2, release()));
    let (_, id, answer) = reply_of(&mut rx).await;
    assert_eq!((id, answer), (2, Err(BUSY.to_owned())));
    // The refusal left the cancel of the download that runs alone.
    assert!(installer.cancel.lock().unwrap().is_some());
    installer.ask(InstallAsk::Cancel);
    let (_, id, answer) = reply_of(&mut rx).await;
    assert_eq!((id, answer), (1, Err(CANCELLED.to_owned())));
}

#[tokio::test]
async fn the_slot_is_free_again_once_a_step_ended() {
    need_curl!();
    let (base, _) = file_server(BODY);
    let home = tempfile::tempdir().unwrap();
    let (mut installer, mut rx) = installer(&base, Some(home.path()));
    installer.ask(InstallAsk::Download(1, release()));
    assert_eq!(reply_of(&mut rx).await.1, 1);
    installer.ask(InstallAsk::Install(2, release()));
    let (_, id, answer) = reply_of(&mut rx).await;
    assert_eq!(id, 2);
    assert_ne!(answer, Err(BUSY.to_owned()));
}

#[tokio::test]
async fn without_a_data_home_nothing_is_downloaded_or_installed() {
    let (mut installer, mut rx) = installer("http://127.0.0.1:1", None);
    installer.ask(InstallAsk::Download(1, release()));
    assert_eq!(reply_of(&mut rx).await.1, 1,);
    installer.ask(InstallAsk::Install(2, release()));
    let (_, id, answer) = reply_of(&mut rx).await;
    assert_eq!((id, answer), (2, Err(NO_HOME.to_owned())));
}

#[tokio::test]
async fn an_install_that_fails_replies_with_the_reason() {
    let home = tempfile::tempdir().unwrap();
    // No tar on the installer's PATH: the install refuses before touching disk.
    let (mut installer, mut rx) = installer("http://127.0.0.1:1", Some(home.path()));
    installer.ask(InstallAsk::Install(3, release()));
    let (_, id, answer) = reply_of(&mut rx).await;
    assert_eq!(id, 3);
    assert!(answer.is_err(), "{answer:?}");
}

// Design D9: a Vulkan build is offered only with an ICD file on disk.
#[test]
fn vulkan_needs_an_icd_file_in_one_of_the_folders() {
    let (empty, with, other) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    std::fs::write(with.path().join("nvidia_icd.json"), "{}").unwrap();
    let missing = other.path().join("none");
    assert!(!has_icd_in(&[empty.path(), missing.as_path()]));
    assert!(has_icd_in(&[empty.path(), with.path()]));
}

#[test]
fn a_folder_with_only_a_subfolder_or_a_non_json_file_has_no_icd() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("x_icd.json")).unwrap();
    std::fs::write(dir.path().join("readme.txt"), "").unwrap();
    assert!(!has_icd_in(&[dir.path()]));
}

#[test]
fn the_busy_message_names_the_step_still_running() {
    assert_eq!(BUSY, "another install step is still running");
}

// Review fix: a step that panics still replies, and frees the slot.
#[tokio::test]
async fn a_step_that_panics_replies_with_its_own_id_and_frees_the_slot() {
    let (mut installer, mut rx) = installer("http://127.0.0.1:1", None);
    let slot = Slot::take(&installer.busy, &installer.cancel).expect("a free slot");
    installer.spawn(11, Some(slot), async {
        if true {
            panic!("an injected failure");
        }
        Ok(Reply::Installed)
    });
    let (_, id, answer) = reply_of(&mut rx).await;
    assert_eq!((id, answer), (11, Err(CRASHED.to_owned())));
    installer.ask(InstallAsk::Install(12, release()));
    let (_, id, answer) = reply_of(&mut rx).await;
    assert_eq!(id, 12);
    assert_ne!(answer, Err(BUSY.to_owned()));
}

// Review fix: the cancel of a download that ended is not kept.
#[tokio::test]
async fn the_cancel_is_cleared_when_the_download_ends() {
    need_curl!();
    let (base, _) = file_server(BODY);
    let home = tempfile::tempdir().unwrap();
    let (mut installer, mut rx) = installer(&base, Some(home.path()));
    installer.ask(InstallAsk::Download(1, release()));
    assert_eq!(reply_of(&mut rx).await.1, 1);
    assert!(installer.cancel.lock().unwrap().is_none());
}

/// An owned install of `tag` under a fresh data home: `current` -> `<tag>`.
fn owned_home(tag: &str) -> (tempfile::TempDir, PathBuf) {
    let home = tempfile::tempdir().unwrap();
    let root = install::owned_root(home.path());
    std::fs::create_dir_all(root.join(tag)).unwrap();
    std::fs::write(root.join(tag).join("llama-server"), b"#!/bin/sh\n").unwrap();
    std::os::unix::fs::symlink(tag, root.join("current")).unwrap();
    let binary = root.join("current").join("llama-server");
    (home, binary)
}

// Spec (Remove llama-server): the owned folder goes, the reply keeps the id.
#[tokio::test]
async fn remove_deletes_the_owned_install_and_replies_under_the_same_id() {
    let (home, binary) = owned_home("b9000");
    let (mut installer, mut rx) = installer("http://127.0.0.1:1", Some(home.path()));
    installer.ask(InstallAsk::Remove(9, binary));
    let (_, id, answer) = reply_of(&mut rx).await;
    assert_eq!((id, answer), (9, Ok(Reply::Removed)));
    assert!(!install::owned_root(home.path()).exists());
}

/// Makes `installer` count the stops of the managed server; with `root` it
/// also notes whether the owned folder still existed when the stop ran.
fn stop_counter(
    installer: &mut Installer,
    root: Option<(PathBuf, Arc<AtomicBool>)>,
) -> Arc<AtomicUsize> {
    let stops = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&stops);
    installer.set_stop(Arc::new(move || {
        counted.fetch_add(1, Ordering::SeqCst);
        if let Some((root, existed)) = &root {
            existed.store(root.exists(), Ordering::SeqCst);
        }
        Box::pin(async {})
    }));
    stops
}

// Remove stops the managed server before the files go.
#[tokio::test]
async fn remove_stops_the_managed_server_before_deleting() {
    let (home, binary) = owned_home("b9000");
    let (mut installer, mut rx) = installer("http://127.0.0.1:1", Some(home.path()));
    let existed = Arc::new(AtomicBool::new(false));
    let root = install::owned_root(home.path());
    let stops = stop_counter(&mut installer, Some((root.clone(), Arc::clone(&existed))));
    installer.ask(InstallAsk::Remove(4, binary));
    let (_, _, answer) = reply_of(&mut rx).await;
    assert_eq!(answer, Ok(Reply::Removed));
    assert_eq!(stops.load(Ordering::SeqCst), 1);
    assert!(
        existed.load(Ordering::SeqCst),
        "stopped while the files still existed"
    );
    assert!(!root.exists());
}

// Spec (Remove deletes owned only): a system binary is never touched.
#[tokio::test]
async fn remove_refuses_a_binary_outside_the_owned_folder() {
    let (home, _) = owned_home("b9000");
    let system = tempfile::tempdir().unwrap();
    let binary = system.path().join("llama-server");
    std::fs::write(&binary, b"x").unwrap();
    let (mut installer, mut rx) = installer("http://127.0.0.1:1", Some(home.path()));
    let stops = stop_counter(&mut installer, None);
    installer.ask(InstallAsk::Remove(2, binary.clone()));
    let (_, id, answer) = reply_of(&mut rx).await;
    assert_eq!(id, 2);
    assert!(answer.is_err(), "{answer:?}");
    assert!(binary.exists());
    assert_eq!(
        stops.load(Ordering::SeqCst),
        0,
        "a system binary stops nothing"
    );
    assert!(install::owned_root(home.path()).exists());
}

#[tokio::test]
async fn remove_without_a_data_home_says_so() {
    let (mut installer, mut rx) = installer("http://127.0.0.1:1", None);
    installer.ask(InstallAsk::Remove(3, "/x/llama-server".into()));
    let (_, _, answer) = reply_of(&mut rx).await;
    assert_eq!(answer, Err(NO_HOME.to_owned()));
}

fn releases_of(tag: &str) -> String {
    let asset = asset_name(tag, Platform::CURRENT, Variant::Cpu).expect("a build here");
    format!(
        r#"[{{"tag_name":"{tag}","assets":[{{"name":"{asset}","size":1234,"digest":"sha256:{}"}}]}}]"#,
        "a".repeat(64)
    )
}

/// Whether an event arrives within a short while.
async fn quiet(rx: &mut mpsc::Receiver<AppEvent>) -> bool {
    tokio::time::timeout(Duration::from_millis(500), rx.recv())
        .await
        .is_err()
}

// Spec (Newer tag notice): a notice names the new tag; nothing is downloaded.
#[tokio::test]
async fn a_newer_build_becomes_a_notice() {
    need_curl!();
    let body = releases_of("b9100");
    let (base, seen) = serve(Box::new(move |_| ok(&body)));
    let (home, _) = owned_home("b9000");
    let (mut installer, mut rx) = installer(&base, Some(home.path()));
    installer.ask(InstallAsk::CheckNewer);
    let AppEvent::InstallNewer(notice) = next(&mut rx).await else {
        panic!("expected a notice");
    };
    assert!(
        notice.contains("b9100") && notice.contains("b9000"),
        "{notice}"
    );
    assert_eq!(seen.lock().unwrap().len(), 1, "only the releases lookup");
}

// Spec (Offline check silent) and (Pinned tag): nothing to say, nothing shown.
#[tokio::test]
async fn the_newer_check_is_silent_when_there_is_nothing_newer_or_it_fails() {
    need_curl!();
    let same = releases_of("b9000");
    let (base, _) = serve(Box::new(move |_| ok(&same)));
    let (home, _) = owned_home("b9000");
    let (mut installer, mut rx) = installer(&base, Some(home.path()));
    installer.ask(InstallAsk::CheckNewer);
    assert!(quiet(&mut rx).await, "same tag");

    let (base, _) = serve(Box::new(|_| {
        b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
    }));
    let (mut installer, mut rx) = self::installer(&base, Some(home.path()));
    installer.ask(InstallAsk::CheckNewer);
    assert!(quiet(&mut rx).await, "rate limited");

    let (mut installer, mut rx) = self::installer("http://127.0.0.1:1", Some(home.path()));
    installer.ask(InstallAsk::CheckNewer);
    assert!(quiet(&mut rx).await, "offline");
}

#[tokio::test]
async fn the_newer_check_asks_nothing_without_an_owned_install() {
    need_curl!();
    let body = releases_of("b9100");
    let (base, seen) = serve(Box::new(move |_| ok(&body)));
    let home = tempfile::tempdir().unwrap();
    let (mut installer, mut rx) = installer(&base, Some(home.path()));
    installer.ask(InstallAsk::CheckNewer);
    assert!(quiet(&mut rx).await);
    assert!(
        seen.lock().unwrap().is_empty(),
        "no request without an install"
    );
}

/// The releases list holding only the Vulkan build of `tag`.
fn vulkan_releases_of(tag: &str) -> String {
    let asset = asset_name(tag, Platform::CURRENT, Variant::Vulkan).expect("a vulkan build here");
    format!(
        r#"[{{"tag_name":"{tag}","assets":[{{"name":"{asset}","size":1234,"digest":"sha256:{}"}}]}}]"#,
        "a".repeat(64)
    )
}

// A Vulkan install is compared with the newest Vulkan build, and the notice says so.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_vulkan_install_is_checked_against_the_vulkan_build() {
    need_curl!();
    let body = vulkan_releases_of("b9100");
    let (base, _) = serve(Box::new(move |_| ok(&body)));
    let (home, _) = owned_home("b9000");
    std::fs::write(install::owned_root(home.path()).join("variant"), "vulkan").unwrap();
    let (mut installer, mut rx) = installer(&base, Some(home.path()));
    installer.ask(InstallAsk::CheckNewer);
    let AppEvent::InstallNewer(notice) = next(&mut rx).await else {
        panic!("expected a notice");
    };
    assert!(
        notice.contains("b9100") && notice.contains("Vulkan"),
        "{notice}"
    );
}

// No notice while an install or a remove runs: the tag it names is about to change.
#[tokio::test]
async fn the_newer_check_is_dropped_while_a_step_runs() {
    need_curl!();
    let body = releases_of("b9100");
    let (base, _) = serve(Box::new(move |_| ok(&body)));
    let (home, _) = owned_home("b9000");
    let (mut installer, mut rx) = installer(&base, Some(home.path()));
    let _slot = installer.slot(1).expect("the slot is free");
    installer.ask(InstallAsk::CheckNewer);
    assert!(quiet(&mut rx).await);
}

// The installed tag is read again right before the notice goes out.
#[tokio::test]
async fn the_newer_notice_is_dropped_when_the_install_changed_meanwhile() {
    need_curl!();
    let body = releases_of("b9100");
    let (home, _) = owned_home("b9000");
    let root = install::owned_root(home.path());
    std::fs::create_dir_all(root.join("b9100")).unwrap();
    let flip = root.clone();
    let (base, _) = serve(Box::new(move |_| {
        // The install lands while the lookup is in flight.
        let _ = std::fs::remove_file(flip.join("current"));
        let _ = std::os::unix::fs::symlink("b9100", flip.join("current"));
        ok(&body)
    }));
    let (mut installer, mut rx) = installer(&base, Some(home.path()));
    installer.ask(InstallAsk::CheckNewer);
    assert!(quiet(&mut rx).await);
}
