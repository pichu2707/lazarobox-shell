//! The guided `llama-server` install (llama-server-install), the part that
//! talks to the network: which release to take, and the archive downloaded
//! and held to its published digest. Like the Hugging Face downloads it uses
//! the system `curl`, through the same [`Curl`] (https only, resume, the
//! process group killed on a cancel). Nothing here reads a GitHub token.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;
use tokio::sync::oneshot;

use super::desktop::which;
use super::hf::{
    Curl, End, error_line, fetch, file_len, finish, free_space, has_size, sha256, space_check,
};
use super::proc::GroupKill;
use crate::core::ai::install::{self, Platform, Release, Variant};

/// What a GitHub 403 or 429 reads as (the API allows 60 requests an hour
/// without a token). Never the raw reply.
pub const RATE_LIMITED: &str =
    "GitHub rate limit reached (60 requests/hour without a token); try again later";

/// The newest releases, which is where the build tags are. `/releases/latest`
/// is not used: it points at a tag with no binaries.
const RELEASES_PATH: &str = "/repos/ggml-org/llama.cpp/releases?per_page=10";

/// Where the archive of a release is downloaded from, under the GitHub host.
const DOWNLOAD_DIR: &str = "/ggml-org/llama.cpp/releases/download";

/// Where archives wait, inside the owned root, so a cancel can resume.
const PARTIAL_DIR: &str = ".download";

/// The repo whose releases carry the attestation of an archive.
const REPO: &str = "ggml-org/llama.cpp";

/// What a missing `gh` reads as: the install goes on.
pub const NO_GH: &str = "gh is not installed: the archive's attestation was not verified";

/// What a `gh` without a login reads as: the install goes on.
pub const GH_LOGGED_OUT: &str = "gh is not logged in: the archive's attestation was not verified";

/// How long one `gh` run may take before it is stopped.
const GH_TIMEOUT: Duration = Duration::from_secs(60);

/// The release to install for `platform` and `variant`: the newest build tag
/// that has the asset, with its size and digest.
pub async fn resolve(curl: &Curl, platform: Platform, variant: Variant) -> Result<Release, String> {
    match curl.get(RELEASES_PATH).await? {
        (200, body) => {
            install::pick_release(&body, |tag| install::asset_name(tag, platform, variant))
        }
        (403 | 429, _) => Err(RATE_LIMITED.to_owned()),
        (code, _) => Err(format!("GitHub request failed: HTTP {code}")),
    }
}

/// Where the archive of `release` is kept while it downloads and after it is
/// verified: `<root>/.download/<asset>`.
pub fn archive_path(root: &Path, release: &Release) -> PathBuf {
    root.join(PARTIAL_DIR).join(&release.asset)
}

/// Whether `release` names something safe to build a URL and a path from: a
/// `b<digits>` tag and a plain file name.
fn names_are_safe(release: &Release) -> bool {
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_');
    install::tag_number(&release.tag).is_some()
        && !release.asset.starts_with('.')
        && !release.asset.is_empty()
        && release.asset.chars().all(plain)
}

/// Whether `digest` is 64 hex digits.
fn is_digest(digest: &str) -> bool {
    digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Downloads the archive of `release` into `<root>/.download/`, calling
/// `progress(done, total)` about every tick, and holds it to the release's
/// size and SHA-256 (computed here, not by a tool). A wrong archive is
/// deleted; a cancel keeps the partial file, so the next download resumes
/// it. `End::Done` carries the verified archive.
pub async fn download(
    curl: &Curl,
    root: &Path,
    release: &Release,
    mut progress: impl FnMut(u64, u64),
    cancel: &mut oneshot::Receiver<()>,
) -> End {
    let failed = |why: &str| End::Failed(format!("download failed: {why}"));
    if !names_are_safe(release) {
        return failed("the release names an unsafe file");
    }
    if !is_digest(&release.sha256) {
        return failed("the release has no sha256 digest to check the archive against");
    }
    let archive = archive_path(root, release);
    let size = release.size;
    // Longer than the archive: it cannot be resumed, so it starts over.
    if size > 0 && file_len(&archive) > size {
        let _ = std::fs::remove_file(&archive);
    }
    let need = install::needed_bytes(size, file_len(&archive));
    if let Err(message) = space_check(need, free_space(root)) {
        return End::Failed(message);
    }
    if let Err(e) = std::fs::create_dir_all(root.join(PARTIAL_DIR)) {
        return failed(&e.to_string());
    }
    if size == 0 || !has_size(&archive, size) {
        let path = format!("{DOWNLOAD_DIR}/{}/{}", release.tag, release.asset);
        let total = |on_disk| if size > 0 { size } else { on_disk };
        if let Some(end) = fetch(curl, &path, &archive, cancel, |on_disk| {
            progress(on_disk, total(on_disk));
        })
        .await
        {
            return end;
        }
    }
    let (file, expected) = (archive.clone(), release.clone());
    match tokio::task::spawn_blocking(move || verify(&file, &expected)).await {
        Ok(Ok(())) => {}
        Ok(Err(end)) => return end,
        Err(_) => return failed("checksum interrupted"),
    }
    progress(file_len(&archive), file_len(&archive));
    End::Done(archive)
}

/// Checks the size and the SHA-256 of `archive` against `release`. A wrong
/// archive is removed, since resuming it would only grow the mistake. Reads
/// the whole file: run it off the event loop.
fn verify(archive: &Path, release: &Release) -> Result<(), End> {
    let failed = |why: &str| {
        let _ = std::fs::remove_file(archive);
        Err(End::Failed(format!("download failed: {why}")))
    };
    if release.size > 0 && !has_size(archive, release.size) {
        return failed("size mismatch");
    }
    match sha256(archive) {
        Ok(sum) if sum.eq_ignore_ascii_case(&release.sha256) => Ok(()),
        Ok(_) => failed("checksum mismatch"),
        Err(e) => Err(End::Failed(format!("download failed: {e}"))),
    }
}

/// What the download step ends with.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Fetched {
    /// The archive passed the digest and, when `gh` could check it, its
    /// attestation. `warning` says why the attestation was skipped.
    Done {
        archive: PathBuf,
        warning: Option<String>,
    },
    Cancelled,
    Failed(String),
}

/// The download step as the wizard runs it: the archive, held to its digest
/// and then to its attestation. Either failure deletes the archive; a cancel
/// keeps it for a resume.
pub async fn download_verified(
    curl: &Curl,
    gh_path: &OsStr,
    root: &Path,
    release: &Release,
    progress: impl FnMut(u64, u64),
    cancel: &mut oneshot::Receiver<()>,
) -> Fetched {
    let archive = match download(curl, root, release, progress, cancel).await {
        End::Done(archive) => archive,
        End::Cancelled => return Fetched::Cancelled,
        End::Failed(message) => return Fetched::Failed(message),
    };
    match attest(gh_path, &archive, cancel).await {
        Attestation::Verified => Fetched::Done {
            archive,
            warning: None,
        },
        Attestation::Skipped(warning) => Fetched::Done {
            archive,
            warning: Some(warning),
        },
        Attestation::Cancelled => Fetched::Cancelled,
        Attestation::Failed(message) => {
            let _ = std::fs::remove_file(&archive);
            Fetched::Failed(message)
        }
    }
}

/// What the attestation check found.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Attestation {
    Verified,
    /// `gh` is missing or logged out: the reason, shown as a warning.
    Skipped(String),
    Failed(String),
    Cancelled,
}

/// Runs `gh attestation verify <archive> --repo ggml-org/llama.cpp` with the
/// `gh` found on `path`, when there is one and `gh auth status` says it is
/// logged in. No shell; nothing it prints but its last error line is kept.
async fn attest(path: &OsStr, archive: &Path, cancel: &mut oneshot::Receiver<()>) -> Attestation {
    attest_within(path, archive, cancel, GH_TIMEOUT).await
}

async fn attest_within(
    path: &OsStr,
    archive: &Path,
    cancel: &mut oneshot::Receiver<()>,
    timeout: Duration,
) -> Attestation {
    let Some(gh) = which("gh", Some(path)) else {
        return Attestation::Skipped(NO_GH.to_owned());
    };
    match run_gh(&gh, &["auth".into(), "status".into()], cancel, timeout).await {
        Gh::Done => {}
        Gh::Cancelled => return Attestation::Cancelled,
        Gh::Failed(_) | Gh::TimedOut => return Attestation::Skipped(GH_LOGGED_OUT.to_owned()),
    }
    let args = [
        "attestation".into(),
        "verify".into(),
        archive.into(),
        "--repo".into(),
        REPO.into(),
    ];
    match run_gh(&gh, &args, cancel, timeout).await {
        Gh::Done => Attestation::Verified,
        Gh::Cancelled => Attestation::Cancelled,
        Gh::Failed(why) => Attestation::Failed(format!("attestation failed: {why}")),
        Gh::TimedOut => Attestation::Failed("attestation failed: gh timed out".to_owned()),
    }
}

/// How one `gh` run ended.
enum Gh {
    Done,
    /// It failed: its last error line, or why it did not start.
    Failed(String),
    TimedOut,
    Cancelled,
}

/// Runs `gh` with `args`, in its own process group, stopped by `cancel` or
/// after `timeout`.
async fn run_gh(
    gh: &Path,
    args: &[OsString],
    cancel: &mut oneshot::Receiver<()>,
    timeout: Duration,
) -> Gh {
    let mut attempt = 0;
    let mut child = loop {
        let started = Command::new(gh)
            .args(args)
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_NO_UPDATE_NOTIFIER", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .process_group(0)
            .spawn();
        match started {
            // A binary that is being written fails with ETXTBSY for a moment.
            Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) && attempt < 4 => {
                attempt += 1;
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(e) => return Gh::Failed(e.to_string()),
            Ok(child) => break child,
        }
    };
    let mut guard = child
        .id()
        .and_then(|pid| libc::pid_t::try_from(pid).ok())
        .and_then(GroupKill::new);
    tokio::select! {
        biased;
        _ = &mut *cancel => {
            let _ = finish(&mut child, &mut guard, true).await;
            Gh::Cancelled
        }
        () = tokio::time::sleep(timeout) => {
            let _ = finish(&mut child, &mut guard, true).await;
            Gh::TimedOut
        }
        (line, status) = async {
            let line = error_line(&mut child).await;
            (line, finish(&mut child, &mut guard, false).await)
        } => match status {
            Ok(status) if status.success() => Gh::Done,
            _ => Gh::Failed(line),
        },
    }
}

#[cfg(test)]
mod tests;
