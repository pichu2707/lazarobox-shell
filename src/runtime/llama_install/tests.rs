use std::thread;
use std::time::Duration;

use tokio::sync::oneshot;

use super::*;
use crate::core::ai::install::{Arch, NO_RELEASE};
use crate::core::ai::jail::Os;
use crate::runtime::hf::tests::{
    curl_available, file_server, header, no_cancel, ok, reply, serve, sha,
};

macro_rules! need_curl {
    () => {
        if !curl_available() {
            eprintln!("SKIPPED: curl is not installed; the curl-backed checks did not run");
            return;
        }
    };
}

const LINUX_X64: Platform = Platform {
    os: Os::Linux,
    arch: Arch::X64,
};

const ASSET: &str = "llama-b9000-bin-ubuntu-x64.tar.gz";

fn path() -> std::ffi::OsString {
    std::env::var_os("PATH").unwrap_or_default()
}

/// One release of the GitHub list, with `(asset name, digest)` pairs.
fn release_json(tag: &str, assets: &[(&str, Option<&str>)]) -> String {
    let assets: Vec<String> = assets
        .iter()
        .map(|(name, digest)| match digest {
            Some(digest) => {
                format!(r#"{{"name":"{name}","size":1234,"digest":"{digest}"}}"#)
            }
            None => format!(r#"{{"name":"{name}","size":1234}}"#),
        })
        .collect();
    format!(r#"{{"tag_name":"{tag}","assets":[{}]}}"#, assets.join(","))
}

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn list(releases: &[String]) -> String {
    format!("[{}]", releases.join(","))
}

fn asset_of(tag: &str) -> String {
    format!("llama-{tag}-bin-ubuntu-x64.tar.gz")
}

// Spec (llama-server-install, Newest matching tag wins): b9010 lacks the
// asset and v0.7.0 is not a build tag.
#[tokio::test]
async fn resolve_takes_the_newest_build_tag_with_the_asset() {
    need_curl!();
    let (a, b) = (asset_of("b9000"), asset_of("b8990"));
    let body = list(&[
        release_json("v0.7.0", &[]),
        release_json("b9000", &[(&a, Some(&digest('a')))]),
        release_json("b9010", &[]),
        release_json("b8990", &[(&b, Some(&digest('b')))]),
    ]);
    let (base, seen) = serve(Box::new(move |_| ok(&body)));
    let curl = Curl::github_api(path()).local(&base);
    let release = resolve(&curl, LINUX_X64, Variant::Cpu).await.unwrap();
    assert_eq!(release.tag, "b9000");
    assert_eq!(release.asset, ASSET);
    assert_eq!(release.sha256, "a".repeat(64));
    let request = seen.lock().unwrap()[0].clone();
    assert!(
        request.starts_with("GET /repos/ggml-org/llama.cpp/releases?per_page=10 "),
        "{request}"
    );
}

// Spec (Numeric comparison): b1000 beats b999.
#[tokio::test]
async fn resolve_compares_tags_by_number() {
    need_curl!();
    let (a, b) = (asset_of("b999"), asset_of("b1000"));
    let body = list(&[
        release_json("b999", &[(&a, Some(&digest('a')))]),
        release_json("b1000", &[(&b, Some(&digest('b')))]),
    ]);
    let (base, _) = serve(Box::new(move |_| ok(&body)));
    let curl = Curl::github_api(path()).local(&base);
    let release = resolve(&curl, LINUX_X64, Variant::Cpu).await.unwrap();
    assert_eq!(release.tag, "b1000");
}

// Spec (No usable release): readable error.
#[tokio::test]
async fn resolve_without_a_usable_release_says_so() {
    need_curl!();
    let body = list(&[release_json("v0.7.0", &[])]);
    let (base, _) = serve(Box::new(move |_| ok(&body)));
    let curl = Curl::github_api(path()).local(&base);
    assert_eq!(
        resolve(&curl, LINUX_X64, Variant::Cpu).await,
        Err(NO_RELEASE.to_owned())
    );
    // A platform with no upstream build reads the same.
    let windows = Platform {
        os: Os::Other,
        arch: Arch::X64,
    };
    assert_eq!(
        resolve(&curl, windows, Variant::Cpu).await,
        Err(NO_RELEASE.to_owned())
    );
}

// Spec (Errors, Rate limited): no stack, no raw body.
#[tokio::test]
async fn resolve_reads_a_rate_limit_as_one_short_message() {
    need_curl!();
    for status in ["403 Forbidden", "429 Too Many Requests"] {
        let (base, _) = serve(Box::new(move |_| {
            reply(
                status,
                br#"{"message":"API rate limit exceeded for 1.2.3.4","documentation_url":"x"}"#,
            )
        }));
        let curl = Curl::github_api(path()).local(&base);
        let err = resolve(&curl, LINUX_X64, Variant::Cpu).await.unwrap_err();
        assert_eq!(err, RATE_LIMITED, "{status}");
        assert!(err.contains("60 requests/hour"), "{err}");
        assert!(!err.contains("1.2.3.4"), "{err}");
    }
}

#[tokio::test]
async fn resolve_names_any_other_status() {
    need_curl!();
    let (base, _) = serve(Box::new(|_| reply("500 Internal Server Error", b"boom")));
    let curl = Curl::github_api(path()).local(&base);
    assert_eq!(
        resolve(&curl, LINUX_X64, Variant::Cpu).await,
        Err("GitHub request failed: HTTP 500".to_owned())
    );
}

const BODY: &[u8] = b"0123456789";

fn release(body: &[u8]) -> Release {
    Release {
        tag: "b9000".to_owned(),
        asset: ASSET.to_owned(),
        size: body.len() as u64,
        sha256: sha(body),
    }
}

fn archive_of(root: &Path) -> PathBuf {
    root.join(".download").join(ASSET)
}

/// The root holds no install: no tag directory, no staging, no `current`.
fn nothing_installed(root: &Path) -> bool {
    std::fs::read_dir(root)
        .map(|entries| {
            entries
                .flatten()
                .all(|e| e.file_name() == std::ffi::OsStr::new(".download"))
        })
        .unwrap_or(true)
}

// Spec (Digest matches): the archive stays and the flow continues.
#[tokio::test]
async fn a_matching_digest_keeps_the_archive_and_reports_progress() {
    need_curl!();
    let (base, seen) = file_server(BODY);
    let root = tempfile::tempdir().unwrap();
    let curl = Curl::github(path()).local(&base);
    let mut last = (0, 0);
    let end = download(
        &curl,
        root.path(),
        &release(BODY),
        |done, total| last = (done, total),
        no_cancel(),
    )
    .await;
    assert_eq!(end, End::Done(archive_of(root.path())));
    assert_eq!(std::fs::read(archive_of(root.path())).unwrap(), BODY);
    assert_eq!(last, (10, 10));
    // The URL is built from tag and asset, never taken from the API.
    let request = seen.lock().unwrap()[0].clone();
    assert!(
        request.starts_with(&format!(
            "GET /ggml-org/llama.cpp/releases/download/b9000/{ASSET} "
        )),
        "{request}"
    );
}

// Spec (Digest mismatch): archive deleted, nothing installed.
#[tokio::test]
async fn a_digest_mismatch_deletes_the_archive() {
    need_curl!();
    let (base, _) = file_server(BODY);
    let root = tempfile::tempdir().unwrap();
    let mut wrong = release(BODY);
    wrong.sha256 = sha(b"9876543210");
    let curl = Curl::github(path()).local(&base);
    let end = download(&curl, root.path(), &wrong, |_, _| {}, no_cancel()).await;
    assert_eq!(
        end,
        End::Failed("download failed: checksum mismatch".to_owned())
    );
    assert!(!archive_of(root.path()).exists());
    assert!(nothing_installed(root.path()));
}

#[tokio::test]
async fn a_wrong_size_deletes_the_archive_before_hashing() {
    need_curl!();
    let (base, _) = file_server(BODY);
    let root = tempfile::tempdir().unwrap();
    let mut longer = release(BODY);
    longer.size = 12;
    let curl = Curl::github(path()).local(&base);
    let end = download(&curl, root.path(), &longer, |_, _| {}, no_cancel()).await;
    assert_eq!(
        end,
        End::Failed("download failed: size mismatch".to_owned())
    );
    assert!(!archive_of(root.path()).exists());
}

// Spec (Missing digest): aborts before any request or file.
#[tokio::test]
async fn a_release_without_a_digest_aborts_before_the_network() {
    need_curl!();
    let (base, seen) = file_server(BODY);
    let root = tempfile::tempdir().unwrap();
    let curl = Curl::github(path()).local(&base);
    for bad in ["", "abc", &"g".repeat(64)] {
        let mut release = release(BODY);
        release.sha256 = bad.to_owned();
        let end = download(&curl, root.path(), &release, |_, _| {}, no_cancel()).await;
        let End::Failed(message) = end else {
            panic!("{bad:?} did not fail: {end:?}")
        };
        assert!(message.contains("digest"), "{message}");
    }
    assert!(seen.lock().unwrap().is_empty());
    assert!(!root.path().join(".download").exists());
}

// The download URL is made from the tag and asset: an odd one never gets
// into a path.
#[tokio::test]
async fn a_release_with_an_unsafe_name_aborts_before_the_network() {
    need_curl!();
    let (base, seen) = file_server(BODY);
    let root = tempfile::tempdir().unwrap();
    let curl = Curl::github(path()).local(&base);
    for (tag, asset) in [
        ("b9000/../x", ASSET),
        ("v0.7.0", ASSET),
        ("b9000", "../evil.tar.gz"),
        ("b9000", "a/b.tar.gz"),
    ] {
        let mut odd = release(BODY);
        odd.tag = tag.to_owned();
        odd.asset = asset.to_owned();
        let end = download(&curl, root.path(), &odd, |_, _| {}, no_cancel()).await;
        assert!(matches!(end, End::Failed(_)), "{tag} {asset}: {end:?}");
    }
    assert!(seen.lock().unwrap().is_empty());
}

// Spec (Disk space check): the shortfall is shown and nothing is fetched.
#[tokio::test]
async fn too_little_room_blocks_the_download_and_names_the_shortfall() {
    need_curl!();
    let (base, seen) = file_server(BODY);
    let root = tempfile::tempdir().unwrap();
    let mut huge = release(BODY);
    huge.size = u64::MAX / 8;
    let curl = Curl::github(path()).local(&base);
    let end = download(&curl, root.path(), &huge, |_, _| {}, no_cancel()).await;
    let End::Failed(message) = end else {
        panic!("{end:?}")
    };
    assert!(message.starts_with("not enough disk space"), "{message}");
    assert!(seen.lock().unwrap().is_empty());
    assert!(!root.path().join(".download").exists());
}

// Spec (Cancel and cleanup): a cancel keeps the partial file for a resume.
#[tokio::test]
async fn a_cancel_keeps_the_partial_archive() {
    need_curl!();
    let (base, _) = serve(Box::new(|_| {
        thread::sleep(Duration::from_secs(30));
        Vec::new()
    }));
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".download")).unwrap();
    std::fs::write(archive_of(root.path()), b"0123").unwrap();
    let (tx, rx) = oneshot::channel();
    let curl = Curl::github(path()).local(&base);
    let wanted = release(BODY);
    let task = download(&curl, root.path(), &wanted, |_, _| {}, rx);
    let cancel = async {
        tokio::time::sleep(Duration::from_millis(300)).await;
        tx.send(()).unwrap();
    };
    let (end, ()) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(task, cancel)
    })
    .await
    .expect("the cancel stops curl");
    assert_eq!(end, End::Cancelled);
    assert_eq!(std::fs::read(archive_of(root.path())).unwrap(), b"0123");
    assert!(nothing_installed(root.path()));
}

#[tokio::test]
async fn a_partial_archive_resumes_from_where_it_stopped() {
    need_curl!();
    let (base, seen) = file_server(BODY);
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".download")).unwrap();
    std::fs::write(archive_of(root.path()), b"0123").unwrap();
    let curl = Curl::github(path()).local(&base);
    let end = download(&curl, root.path(), &release(BODY), |_, _| {}, no_cancel()).await;
    assert_eq!(end, End::Done(archive_of(root.path())));
    assert_eq!(std::fs::read(archive_of(root.path())).unwrap(), BODY);
    let request = seen.lock().unwrap()[0].clone();
    assert_eq!(header(&request, "range"), Some("bytes=4-"));
}

#[tokio::test]
async fn a_complete_archive_is_checked_without_a_request() {
    need_curl!();
    let (base, seen) = file_server(BODY);
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".download")).unwrap();
    std::fs::write(archive_of(root.path()), BODY).unwrap();
    let curl = Curl::github(path()).local(&base);
    let end = download(&curl, root.path(), &release(BODY), |_, _| {}, no_cancel()).await;
    assert_eq!(end, End::Done(archive_of(root.path())));
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_oversized_partial_archive_starts_over() {
    need_curl!();
    let (base, seen) = file_server(BODY);
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".download")).unwrap();
    std::fs::write(archive_of(root.path()), b"0123456789xx").unwrap();
    let curl = Curl::github(path()).local(&base);
    let end = download(&curl, root.path(), &release(BODY), |_, _| {}, no_cancel()).await;
    assert_eq!(end, End::Done(archive_of(root.path())));
    assert_eq!(std::fs::read(archive_of(root.path())).unwrap(), BODY);
    assert_eq!(header(&seen.lock().unwrap()[0], "range"), None);
}
