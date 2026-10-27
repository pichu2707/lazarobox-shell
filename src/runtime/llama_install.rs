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

/// What a `gh` that timed out checking the attestation reads as: the archive
/// stays, the install does not go on.
pub const UNCHECKED_TIMEOUT: &str = "could not check the attestation: timed out";

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
    if let Err(why) = refuse_links(root, &archive) {
        return failed(&why);
    }
    if size > 0 && file_len(&archive) > size {
        let _ = std::fs::remove_file(&archive);
    }
    let need = install::needed_bytes(size, file_len(&archive));
    if let Err(message) = space_check(need, free_space(root)) {
        return End::Failed(message);
    }
    if let Err(why) = create_dir(root) {
        return failed(&why);
    }
    if size == 0 || !has_size(&archive, size) {
        let path = format!("{DOWNLOAD_DIR}/{}/{}", release.tag, release.asset);
        let total = |on_disk| if size > 0 { size } else { on_disk };
        if let Some(end) = fetch(curl, &path, &archive, Some(size), cancel, |on_disk| {
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

/// Refuses a symlink at `<root>/.download` or at `archive`, before anything
/// is read, removed or written through it. (The archive is hashed here;
/// whoever unpacks it must re-check it first.)
fn refuse_links(root: &Path, archive: &Path) -> Result<(), String> {
    let is_link = |path: &Path| {
        std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
    };
    if is_link(&root.join(PARTIAL_DIR)) || is_link(archive) {
        return Err("refusing a symlink in the download directory".to_owned());
    }
    Ok(())
}

/// Creates `<root>/.download`, private to the user.
fn create_dir(root: &Path) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    let dir = root.join(PARTIAL_DIR);
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.to_string()),
    }
    if std::fs::symlink_metadata(&dir).is_ok_and(|meta| meta.is_dir()) {
        Ok(())
    } else {
        Err("the download directory is not a directory".to_owned())
    }
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
/// and then to its attestation. A failed digest or a failed verification
/// deletes the archive; a cancel, or a check that could not finish (a
/// timeout), keeps it: the archive is intact, only unchecked.
pub async fn download_verified(
    curl: &Curl,
    gh_path: &OsStr,
    root: &Path,
    release: &Release,
    progress: impl FnMut(u64, u64),
    cancel: &mut oneshot::Receiver<()>,
) -> Fetched {
    download_verified_within(curl, gh_path, root, release, progress, cancel, GH_TIMEOUT).await
}

async fn download_verified_within(
    curl: &Curl,
    gh_path: &OsStr,
    root: &Path,
    release: &Release,
    progress: impl FnMut(u64, u64),
    cancel: &mut oneshot::Receiver<()>,
    timeout: Duration,
) -> Fetched {
    let archive = match download(curl, root, release, progress, cancel).await {
        End::Done(archive) => archive,
        End::Cancelled => return Fetched::Cancelled,
        End::Failed(message) => return Fetched::Failed(message),
    };
    match attest_within(gh_path, &archive, cancel, timeout).await {
        Attestation::Verified => Fetched::Done {
            archive,
            warning: None,
        },
        Attestation::Skipped(warning) => Fetched::Done {
            archive,
            warning: Some(warning),
        },
        Attestation::Cancelled => Fetched::Cancelled,
        Attestation::Unchecked(message) => Fetched::Failed(message),
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
    /// `gh` ran the verification but did not finish it: the archive is not
    /// proven bad, so it is kept.
    Unchecked(String),
    /// `gh` ran the verification and it failed.
    Failed(String),
    Cancelled,
}

/// Runs `gh attestation verify <archive> --repo ggml-org/llama.cpp` with the
/// `gh` found on `path`, when there is one and `gh auth status` says it is
/// logged in to github.com. No shell; nothing it prints but its last error
/// line is kept.
async fn attest_within(
    path: &OsStr,
    archive: &Path,
    cancel: &mut oneshot::Receiver<()>,
    timeout: Duration,
) -> Attestation {
    let Some(gh) = which("gh", Some(path)) else {
        return Attestation::Skipped(NO_GH.to_owned());
    };
    let auth = ["auth", "status", "--hostname", "github.com"].map(OsString::from);
    match run_gh(&gh, &auth, cancel, timeout).await {
        Gh::Done => {}
        Gh::Cancelled => return Attestation::Cancelled,
        Gh::Failed(why) if logged_out(&why) => {
            return Attestation::Skipped(GH_LOGGED_OUT.to_owned());
        }
        Gh::Failed(why) => return Attestation::Skipped(unchecked_login(&why)),
        Gh::TimedOut => return Attestation::Skipped(unchecked_login("timed out")),
    }
    // Absolute, so the path can never read as a flag.
    let archive = std::path::absolute(archive).unwrap_or_else(|_| archive.to_path_buf());
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
        Gh::TimedOut => Attestation::Unchecked(UNCHECKED_TIMEOUT.to_owned()),
    }
}

/// Whether the last error line of `gh auth status` says there is no login.
fn logged_out(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    line.contains("not logged in") || line.contains("gh auth login")
}

/// The warning for a `gh auth status` that failed for another reason.
fn unchecked_login(why: &str) -> String {
    format!("gh could not check its login ({why}): the archive's attestation was not verified")
}

/// The `gh` process for `args`: the user's `GH_HOST`, `GH_REPO` and
/// `GH_ENTERPRISE_TOKEN` must not point it at another host or repo.
fn gh_command(gh: &Path, args: &[OsString]) -> Command {
    let mut command = Command::new(gh);
    command
        .args(args)
        .env_remove("GH_HOST")
        .env_remove("GH_ENTERPRISE_TOKEN")
        .env_remove("GH_REPO")
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);
    command
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
        let started = gh_command(gh, args).spawn();
        match started {
            // A binary that is being written fails with ETXTBSY for a moment.
            Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) && attempt < 10 => {
                attempt += 1;
                tokio::time::sleep(Duration::from_millis(50)).await;
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
