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
        &mut no_cancel(),
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
    let end = download(&curl, root.path(), &wrong, |_, _| {}, &mut no_cancel()).await;
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
    let end = download(&curl, root.path(), &longer, |_, _| {}, &mut no_cancel()).await;
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
        let end = download(&curl, root.path(), &release, |_, _| {}, &mut no_cancel()).await;
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
        let end = download(&curl, root.path(), &odd, |_, _| {}, &mut no_cancel()).await;
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
    let end = download(&curl, root.path(), &huge, |_, _| {}, &mut no_cancel()).await;
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
    let (tx, mut rx) = oneshot::channel();
    let curl = Curl::github(path()).local(&base);
    let wanted = release(BODY);
    let task = download(&curl, root.path(), &wanted, |_, _| {}, &mut rx);
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
    let end = download(
        &curl,
        root.path(),
        &release(BODY),
        |_, _| {},
        &mut no_cancel(),
    )
    .await;
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
    let end = download(
        &curl,
        root.path(),
        &release(BODY),
        |_, _| {},
        &mut no_cancel(),
    )
    .await;
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
    let end = download(
        &curl,
        root.path(),
        &release(BODY),
        |_, _| {},
        &mut no_cancel(),
    )
    .await;
    assert_eq!(end, End::Done(archive_of(root.path())));
    assert_eq!(std::fs::read(archive_of(root.path())).unwrap(), BODY);
    assert_eq!(header(&seen.lock().unwrap()[0], "range"), None);
}

/// A `gh` that logs its arguments to `log`, answers `auth status` with exit
/// code `auth` and runs `verify` for `attestation verify`.
fn fake_gh(dir: &Path, log: &Path, auth: i32, verify: &str) {
    fake_gh_auth(dir, log, &format!("exit {auth}"), verify);
}

/// Like [`fake_gh`], with a shell snippet for `auth status`.
fn fake_gh_auth(dir: &Path, log: &Path, auth: &str, verify: &str) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let script = format!(
        "#!/bin/sh\necho \"$@\" >> '{}'\nif [ \"$1\" = auth ]; then {auth}; fi\n{verify}\n",
        log.display()
    );
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o755)
        .open(dir.join("gh"))
        .unwrap();
    file.write_all(script.as_bytes()).unwrap();
}

fn logged(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Downloads `BODY` through a local server and attests it with the `gh`
/// found in `gh_path`.
async fn fetched(root: &Path, gh_path: &std::ffi::OsStr, release: &Release) -> Fetched {
    let (base, _) = file_server(BODY);
    let curl = Curl::github(path()).local(&base);
    download_verified(&curl, gh_path, root, release, |_, _| {}, &mut no_cancel()).await
}

// Spec (Optional attestation): `gh attestation verify` after the digest.
#[tokio::test]
async fn a_passing_attestation_continues_without_a_warning() {
    need_curl!();
    let (root, bin) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let log = bin.path().join("log");
    fake_gh(bin.path(), &log, 0, "exit 0");
    let got = fetched(root.path(), bin.path().as_os_str(), &release(BODY)).await;
    assert_eq!(
        got,
        Fetched::Done {
            archive: archive_of(root.path()),
            warning: None
        }
    );
    let calls = logged(&log);
    assert_eq!(calls[0], "auth status --hostname github.com");
    assert_eq!(
        calls[1],
        format!(
            "attestation verify {} --repo ggml-org/llama.cpp",
            archive_of(root.path()).display()
        )
    );
}

// Spec (Attestation failure aborts): archive deleted, nothing installed.
#[tokio::test]
async fn a_failing_attestation_deletes_the_archive() {
    need_curl!();
    let (root, bin) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let log = bin.path().join("log");
    fake_gh(
        bin.path(),
        &log,
        0,
        "echo 'no matching attestations found' >&2\nexit 1",
    );
    let got = fetched(root.path(), bin.path().as_os_str(), &release(BODY)).await;
    assert_eq!(
        got,
        Fetched::Failed("attestation failed: no matching attestations found".to_owned())
    );
    assert!(!archive_of(root.path()).exists());
    assert!(nothing_installed(root.path()));
}

// Spec (gh absent warns): the flow continues.
#[tokio::test]
async fn without_gh_the_flow_continues_with_a_warning() {
    need_curl!();
    let (root, empty) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let got = fetched(root.path(), empty.path().as_os_str(), &release(BODY)).await;
    assert_eq!(
        got,
        Fetched::Done {
            archive: archive_of(root.path()),
            warning: Some(NO_GH.to_owned())
        }
    );
    assert!(archive_of(root.path()).exists());
}

// D3: a gh that is logged out warns and continues, and is never asked to
// verify.
#[tokio::test]
async fn a_logged_out_gh_warns_and_continues() {
    need_curl!();
    let (root, bin) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let log = bin.path().join("log");
    fake_gh_auth(
        bin.path(),
        &log,
        "echo 'You are not logged into any GitHub hosts. To log in, run: gh auth login' >&2\nexit 1",
        "exit 1",
    );
    let got = fetched(root.path(), bin.path().as_os_str(), &release(BODY)).await;
    assert_eq!(
        got,
        Fetched::Done {
            archive: archive_of(root.path()),
            warning: Some(GH_LOGGED_OUT.to_owned())
        }
    );
    assert_eq!(logged(&log), ["auth status --hostname github.com"]);
}

// A digest mismatch stops before gh is run.
#[tokio::test]
async fn a_digest_mismatch_never_reaches_gh() {
    need_curl!();
    let (root, bin) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let log = bin.path().join("log");
    fake_gh(bin.path(), &log, 0, "exit 0");
    let mut wrong = release(BODY);
    wrong.sha256 = sha(b"9876543210");
    let got = fetched(root.path(), bin.path().as_os_str(), &wrong).await;
    assert_eq!(
        got,
        Fetched::Failed("download failed: checksum mismatch".to_owned())
    );
    assert!(logged(&log).is_empty());
}

// A cancel while gh runs stops it and keeps the verified archive.
#[tokio::test]
async fn a_cancel_stops_a_running_attestation() {
    need_curl!();
    let (root, bin) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let log = bin.path().join("log");
    fake_gh(bin.path(), &log, 0, "sleep 30");
    let (base, _) = file_server(BODY);
    let curl = Curl::github(path()).local(&base);
    let (tx, mut rx) = oneshot::channel();
    let wanted = release(BODY);
    let run = download_verified(
        &curl,
        bin.path().as_os_str(),
        root.path(),
        &wanted,
        |_, _| {},
        &mut rx,
    );
    // Cancel once gh is running `verify`, not after a fixed wait.
    let cancel = async {
        while !logged(&log)
            .iter()
            .any(|l| l.starts_with("attestation verify"))
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        tx.send(()).unwrap();
    };
    let (got, ()) =
        tokio::time::timeout(Duration::from_secs(10), async { tokio::join!(run, cancel) })
            .await
            .expect("the cancel stops gh");
    assert_eq!(got, Fetched::Cancelled);
    assert!(archive_of(root.path()).exists());
}

// A gh that never answers does not hang the install.
#[tokio::test]
async fn a_gh_that_hangs_is_stopped_after_its_time() {
    let (bin, dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let log = bin.path().join("log");
    fake_gh(bin.path(), &log, 0, "sleep 30");
    let archive = dir.path().join("a.tar.gz");
    std::fs::write(&archive, BODY).unwrap();
    let got = tokio::time::timeout(
        Duration::from_secs(10),
        attest_within(
            bin.path().as_os_str(),
            &archive,
            &mut no_cancel(),
            Duration::from_millis(300),
        ),
    )
    .await
    .expect("the time limit stops gh");
    assert_eq!(got, Attestation::Unchecked(UNCHECKED_TIMEOUT.to_owned()));
}

// A timeout is not a verification failure: the digest-verified archive stays.
#[tokio::test]
async fn an_attestation_timeout_keeps_the_archive_and_says_so() {
    need_curl!();
    let (root, bin) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let log = bin.path().join("log");
    fake_gh(bin.path(), &log, 0, "sleep 30");
    let (base, _) = file_server(BODY);
    let curl = Curl::github(path()).local(&base);
    let got = tokio::time::timeout(
        Duration::from_secs(10),
        download_verified_within(
            &curl,
            bin.path().as_os_str(),
            root.path(),
            &release(BODY),
            |_, _| {},
            &mut no_cancel(),
            Duration::from_millis(300),
        ),
    )
    .await
    .expect("the time limit stops gh");
    assert_eq!(
        got,
        Fetched::Failed("could not check the attestation: timed out".to_owned())
    );
    assert!(archive_of(root.path()).exists());
}

// A gh that exits nonzero without saying it is logged out (offline, a broken
// secondary host) is a failed check, not a logged-out gh.
#[tokio::test]
async fn an_auth_check_that_fails_otherwise_is_a_could_not_check_warning() {
    let (bin, dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let log = bin.path().join("log");
    fake_gh_auth(
        bin.path(),
        &log,
        "echo 'error connecting to api.github.com' >&2\nexit 1",
        "exit 0",
    );
    let archive = dir.path().join("a.tar.gz");
    std::fs::write(&archive, BODY).unwrap();
    let got = attest_within(
        bin.path().as_os_str(),
        &archive,
        &mut no_cancel(),
        Duration::from_secs(10),
    )
    .await;
    let Attestation::Skipped(warning) = got else {
        panic!("{got:?}");
    };
    assert!(warning.contains("could not check"), "{warning}");
    assert!(warning.contains("error connecting"), "{warning}");
    assert_ne!(warning, GH_LOGGED_OUT);
    assert_eq!(logged(&log), ["auth status --hostname github.com"]);
}

#[tokio::test]
async fn an_auth_check_that_hangs_is_a_could_not_check_warning() {
    let (bin, dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let log = bin.path().join("log");
    fake_gh_auth(bin.path(), &log, "sleep 30", "exit 0");
    let archive = dir.path().join("a.tar.gz");
    std::fs::write(&archive, BODY).unwrap();
    let got = tokio::time::timeout(
        Duration::from_secs(10),
        attest_within(
            bin.path().as_os_str(),
            &archive,
            &mut no_cancel(),
            Duration::from_millis(300),
        ),
    )
    .await
    .expect("the time limit stops gh");
    let Attestation::Skipped(warning) = got else {
        panic!("{got:?}");
    };
    assert!(warning.contains("could not check"), "{warning}");
    assert!(warning.contains("timed out"), "{warning}");
}

// gh must not be pointed elsewhere by the user's environment.
#[test]
fn gh_runs_without_host_repo_or_enterprise_token_overrides() {
    let command = gh_command(Path::new("/usr/bin/gh"), &["auth".into()]);
    let envs: Vec<_> = command.as_std().get_envs().collect();
    for name in ["GH_HOST", "GH_ENTERPRISE_TOKEN", "GH_REPO"] {
        assert!(
            envs.iter()
                .any(|(k, v)| *k == std::ffi::OsStr::new(name) && v.is_none()),
            "{name} is not cleared: {envs:?}"
        );
    }
}

// The archive reaches gh as an absolute path, so it can never read as a flag.
#[tokio::test]
async fn a_relative_archive_path_reaches_gh_as_an_absolute_one() {
    let bin = tempfile::tempdir().unwrap();
    let log = bin.path().join("log");
    fake_gh(bin.path(), &log, 0, "exit 0");
    let got = attest_within(
        bin.path().as_os_str(),
        Path::new("-rf.tar.gz"),
        &mut no_cancel(),
        Duration::from_secs(10),
    )
    .await;
    assert_eq!(got, Attestation::Verified);
    let calls = logged(&log);
    let verify: Vec<&str> = calls[1].split(' ').collect();
    assert!(Path::new(verify[2]).is_absolute(), "{calls:?}");
}

// A symlink planted at `.download` or at the archive path is refused before
// curl writes through it.
#[tokio::test]
async fn a_symlink_at_the_download_dir_or_the_archive_is_refused() {
    need_curl!();
    use std::os::unix::fs::symlink;
    let target = tempfile::tempdir().unwrap();
    // At `.download`.
    let root = tempfile::tempdir().unwrap();
    symlink(target.path(), root.path().join(".download")).unwrap();
    let (base, seen) = file_server(BODY);
    let curl = Curl::github(path()).local(&base);
    let got = download(
        &curl,
        root.path(),
        &release(BODY),
        |_, _| {},
        &mut no_cancel(),
    )
    .await;
    assert!(matches!(got, End::Failed(m) if m.contains("symlink")));
    assert!(std::fs::read_dir(target.path()).unwrap().next().is_none());
    assert!(seen.lock().unwrap().is_empty());
    // At the archive path.
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".download")).unwrap();
    let victim = target.path().join("victim");
    std::fs::write(&victim, b"keep").unwrap();
    symlink(&victim, archive_of(root.path())).unwrap();
    let got = download(
        &curl,
        root.path(),
        &release(BODY),
        |_, _| {},
        &mut no_cancel(),
    )
    .await;
    assert!(matches!(got, End::Failed(m) if m.contains("symlink")));
    assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
    assert!(seen.lock().unwrap().is_empty());
}

// The download dir is private to the user.
#[tokio::test]
async fn the_download_dir_is_created_private() {
    need_curl!();
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let (base, _) = file_server(BODY);
    let curl = Curl::github(path()).local(&base);
    let got = download(
        &curl,
        root.path(),
        &release(BODY),
        |_, _| {},
        &mut no_cancel(),
    )
    .await;
    assert!(matches!(got, End::Done(_)));
    let mode = std::fs::metadata(root.path().join(".download"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
}

/// Writes an executable `<dir>/llama-server`.
fn put_server(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).unwrap();
    let file = dir.join("llama-server");
    std::fs::write(&file, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    file
}

// Spec (chat-engines, Owned binary spawned / System binary preferred): the
// owned folder is on the PATH from the start; a forced detection finds the
// binary once it exists, loses it when it is gone, and a system one wins.
#[test]
fn detection_follows_the_owned_folder_and_prefers_a_system_binary() {
    let (data, system) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let search = install::with_owned_path(
        &std::env::join_paths([system.path()]).unwrap(),
        Some(data.path()),
    );
    let find = || crate::runtime::ai_detect::find_on_path("llama-server", &search);
    assert_eq!(find(), None);
    let owned = put_server(&install::current_dir(data.path()));
    assert_eq!(find(), Some(owned.clone()));
    assert!(install::is_owned(&owned, data.path()));
    let system_binary = put_server(system.path());
    assert_eq!(find(), Some(system_binary));
    std::fs::remove_dir_all(system.path()).unwrap();
    std::fs::remove_dir_all(install::current_dir(data.path())).unwrap();
    assert_eq!(find(), None);
}
