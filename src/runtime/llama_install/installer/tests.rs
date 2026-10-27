use std::path::Path;
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
    let (base, _) = serve(Box::new(|_| {
        thread::sleep(Duration::from_secs(30));
        Vec::new()
    }));
    let home = tempfile::tempdir().unwrap();
    let (mut installer, mut rx) = installer(&base, Some(home.path()));
    installer.ask(InstallAsk::Download(5, release()));
    tokio::time::sleep(Duration::from_millis(300)).await;
    installer.ask(InstallAsk::Cancel);
    let (_, id, answer) = reply_of(&mut rx).await;
    assert_eq!((id, answer), (5, Err(CANCELLED.to_owned())));
}

// Spec (one install at a time): a second step is refused, the first goes on.
#[tokio::test]
async fn a_second_download_is_refused_while_one_runs() {
    need_curl!();
    let (base, _) = serve(Box::new(|_| {
        thread::sleep(Duration::from_secs(30));
        Vec::new()
    }));
    let home = tempfile::tempdir().unwrap();
    let (mut installer, mut rx) = installer(&base, Some(home.path()));
    installer.ask(InstallAsk::Download(1, release()));
    installer.ask(InstallAsk::Download(2, release()));
    let (_, id, answer) = reply_of(&mut rx).await;
    assert_eq!((id, answer), (2, Err(BUSY.to_owned())));
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
