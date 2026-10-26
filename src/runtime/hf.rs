//! Hugging Face downloads (model-download): the system `curl` asks the Hub
//! and downloads a model into its cache. As for the update check, no TLS
//! stack is added: curl brings HTTPS, the proxy and the CA settings.
//!
//! The token never reaches an argument list: curl reads it as a config line
//! on stdin, and does not forward it to the host a download redirects to.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::oneshot;

use super::proc::GroupKill;
use crate::core::ai::hf::{
    self, HubFile, Job, RepoCache, RepoFiles, RepoHit, link_target, size_label, status_message,
};
use crate::core::ai::sanitize;

/// What a missing curl reads as.
pub const NO_CURL: &str = "Hugging Face needs curl on the PATH";

/// An API request gets this long, connect included.
const API_TIMEOUT_SECS: u64 = 15;

/// Most bytes of an API reply kept; a longer one fails.
const REPLY_CAP: usize = 4 * 1024 * 1024;

/// Most bytes of curl's error output read.
const ERROR_CAP: u64 = 4 * 1024;

/// How often the progress of a download is read.
const TICK: Duration = Duration::from_millis(250);

/// How curl is run for the Hub.
pub struct Curl {
    program: OsString,
    /// The `PATH` curl is looked up in.
    path: OsString,
    /// `https://huggingface.co`; a local server in tests.
    base: String,
    https_only: bool,
    /// Test requests must not go through the developer's proxy.
    noproxy: bool,
    token: Option<String>,
    user_agent: String,
}

impl Curl {
    /// The production curl: the Hub over HTTPS only.
    pub fn hub(path: OsString, token: Option<String>) -> Self {
        Curl {
            program: "curl".into(),
            path,
            base: hf::HUB.to_owned(),
            https_only: true,
            noproxy: false,
            token,
            user_agent: super::update::user_agent(env!("CARGO_PKG_VERSION")),
        }
    }

    /// The arguments every request starts with.
    fn common(&self) -> Vec<String> {
        // `-q` first: not the user's ~/.curlrc, which could add redirects or
        // output files.
        let mut args: Vec<String> = ["-q", "--silent", "--show-error", "--user-agent"]
            .map(str::to_owned)
            .into();
        args.push(self.user_agent.clone());
        if self.https_only {
            args.extend(["--proto", "=https", "--proto-redir", "=https"].map(str::to_owned));
        }
        if self.noproxy {
            args.extend(["--noproxy", "*"].map(str::to_owned));
        }
        if self.stdin_config().is_some() {
            args.extend(["--config", "-"].map(str::to_owned));
        }
        args
    }

    /// An API GET of `path`: no redirects, the status after the body.
    pub fn api_args(&self, path: &str) -> Vec<String> {
        let mut args = self.common();
        let secs = API_TIMEOUT_SECS.to_string();
        args.extend(
            [
                "--max-time",
                &secs,
                "--connect-timeout",
                &secs,
                "--max-filesize",
                &REPLY_CAP.to_string(),
                "--header",
                "Accept: application/json",
                "--write-out",
                "\n%{http_code}",
                "--",
            ]
            .map(str::to_owned),
        );
        args.push(format!("{}{path}", self.base));
        args
    }

    /// A download of `path` into `out`, resuming what `out` already holds.
    pub fn download_args(&self, path: &str, out: &Path) -> Vec<String> {
        let mut args = self.common();
        args.extend(
            [
                "--location",
                "--fail",
                "--connect-timeout",
                &API_TIMEOUT_SECS.to_string(),
                "--continue-at",
                "-",
                "--output",
                &out.to_string_lossy(),
                "--",
            ]
            .map(str::to_owned),
        );
        args.push(format!("{}{path}", self.base));
        args
    }

    /// The config line curl reads on stdin: the token as a header. `None`
    /// without a token, or with one that cannot be a header value.
    pub fn stdin_config(&self) -> Option<String> {
        let token = self.token.as_deref()?;
        let fits = !token.is_empty()
            && token
                .bytes()
                .all(|b| b.is_ascii_graphic() && b != b'"' && b != b'\\');
        fits.then(|| format!("header = \"Authorization: Bearer {token}\"\n"))
    }

    /// Starts curl with `args` in its own process group, writing the token
    /// config on its stdin.
    async fn spawn(&self, args: &[String]) -> Result<(Child, Option<GroupKill>), String> {
        // An empty or relative entry makes the lookup search the current
        // directory, where a planted `curl` could be: refuse such a `PATH`.
        let safe =
            !self.path.is_empty() && std::env::split_paths(&self.path).all(|dir| dir.is_absolute());
        if !safe {
            return Err(NO_CURL.to_owned());
        }
        let config = self.stdin_config();
        let mut child = Command::new(&self.program)
            .args(args)
            .env("PATH", &self.path)
            .stdin(if config.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .process_group(0)
            .spawn()
            .map_err(|e| match e.kind() {
                io::ErrorKind::NotFound => NO_CURL.to_owned(),
                _ => format!("Hugging Face request failed: {e}"),
            })?;
        let guard = child
            .id()
            .and_then(|pid| libc::pid_t::try_from(pid).ok())
            .and_then(GroupKill::new);
        if let (Some(config), Some(mut stdin)) = (config, child.stdin.take()) {
            // Dropped at the end of the block: curl sees the end of its config.
            let _ = stdin.write_all(config.as_bytes()).await;
        }
        Ok((child, guard))
    }

    /// An API GET: the status and the body, or why it failed.
    async fn get(&self, path: &str) -> Result<(u16, String), String> {
        let (mut child, mut guard) = self.spawn(&self.api_args(path)).await?;
        let mut out = Vec::new();
        let limit = REPLY_CAP as u64 + 8;
        let read = match child.stdout.take() {
            Some(stdout) => stdout.take(limit).read_to_end(&mut out).await.is_ok(),
            None => false,
        };
        let too_long = out.len() as u64 >= limit;
        let error = error_line(&mut child).await;
        let status = finish(&mut child, &mut guard, !read || too_long).await;
        if too_long {
            return Err("Hugging Face request failed: reply too long".to_owned());
        }
        let text = String::from_utf8(out).map_err(|_| hf::UNREADABLE.to_owned())?;
        let (body, code) = text.rsplit_once('\n').unwrap_or(("", &text));
        match code.parse::<u16>() {
            Ok(code) if code != 0 => Ok((code, body.to_owned())),
            _ if status.is_ok_and(|s| s.success()) => Err(hf::UNREADABLE.to_owned()),
            _ => Err(format!("Hugging Face request failed: {error}")),
        }
    }
}

/// Reaps curl and disarms its guard; with `kill`, the group goes first.
async fn finish(
    child: &mut Child,
    guard: &mut Option<GroupKill>,
    kill: bool,
) -> io::Result<ExitStatus> {
    match guard {
        Some(guard) if kill => guard.kill_and_reap(child).await,
        Some(guard) => guard.reap(child).await,
        None => {
            if kill {
                let _ = child.start_kill();
            }
            child.wait().await
        }
    }
}

/// The last line curl wrote on stderr, cleaned for the screen.
async fn error_line(child: &mut Child) -> String {
    let mut err = Vec::new();
    if let Some(stderr) = child.stderr.take() {
        let _ = stderr.take(ERROR_CAP).read_to_end(&mut err).await;
    }
    let text = String::from_utf8_lossy(&err);
    let line = text
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");
    let line = line.trim().trim_start_matches("curl: ");
    let line = sanitize::clean(line);
    if line.is_empty() {
        "no reply".to_owned()
    } else {
        line
    }
}

/// The GGUF repos a search finds.
pub async fn search(curl: &Curl, text: &str) -> Result<Vec<RepoHit>, String> {
    match curl.get(&hf::search_path(text)).await? {
        (200, body) => hf::parse_search(&body),
        (code, _) => Err(format!("Hugging Face request failed: HTTP {code}")),
    }
}

/// A repo's commit, license and GGUF choices: its info, then its files at
/// that commit.
pub async fn repo_files(curl: &Curl, id: &str) -> Result<RepoFiles, String> {
    let repo = match curl.get(&hf::repo_path(id)).await? {
        (200, body) => hf::parse_repo(&body)?,
        (code, _) => return Err(status_message(id, code)),
    };
    let files = match curl.get(&hf::tree_path(id, &repo.commit)).await? {
        (200, body) => hf::parse_tree(&body)?,
        (code, _) => return Err(status_message(id, code)),
    };
    Ok(RepoFiles::new(repo, &files))
}

/// How a download ended.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum End {
    /// The path llama-server is started with: the snapshot link of the
    /// first file.
    Done(PathBuf),
    Cancelled,
    Failed(String),
}

/// Downloads every file of `job` into the hub cache `hub`, calling
/// `progress(done, total)` about every [`TICK`], until it ends or `cancel`
/// fires (or its sender is dropped). A cancel keeps the partial file, so the
/// next download of the same file resumes it.
pub async fn download(
    curl: &Curl,
    hub: &Path,
    job: &Job,
    mut progress: impl FnMut(u64, u64),
    mut cancel: oneshot::Receiver<()>,
) -> End {
    let cache = RepoCache::new(hub, &job.id);
    let total = job.choice.size;
    if let Err(message) = space_check(missing_bytes(hub, job), free_space(cache.dir())) {
        return End::Failed(message);
    }
    if let Err(e) = std::fs::create_dir_all(cache.dir().join("blobs")) {
        return End::Failed(format!("download failed: {e}"));
    }
    let mut done = 0;
    for file in &job.choice.files {
        if cancel.try_recv().is_ok() {
            return End::Cancelled;
        }
        let blob = cache.blob(&file.oid);
        if !has_size(&blob, file.size) {
            let partial = cache.incomplete(&file.oid);
            if !has_size(&partial, file.size) {
                let path = hf::resolve_path(&job.id, &job.commit, &file.path);
                let fetched = fetch(curl, &path, &partial, &mut cancel, |on_disk| {
                    progress(done + on_disk, total);
                })
                .await;
                if let Some(end) = fetched {
                    return end;
                }
            }
            if let Err(end) = keep(&partial, &blob, file) {
                return end;
            }
        }
        if let Err(e) = link(&cache, &job.commit, file) {
            return End::Failed(format!("download failed: {e}"));
        }
        done += file.size;
        progress(done, total);
    }
    if let Err(e) = write_ref(&cache, &job.commit) {
        return End::Failed(format!("download failed: {e}"));
    }
    End::Done(cache.snapshot(&job.commit, job.choice.path()))
}

/// Runs one curl download into `partial`. `None` when curl ended well;
/// otherwise how the download ends.
async fn fetch(
    curl: &Curl,
    path: &str,
    partial: &Path,
    cancel: &mut oneshot::Receiver<()>,
    mut on_disk: impl FnMut(u64),
) -> Option<End> {
    let (mut child, mut guard) = match curl.spawn(&curl.download_args(path, partial)).await {
        Ok(started) => started,
        Err(message) => return Some(End::Failed(message)),
    };
    let status = loop {
        tokio::select! {
            biased;
            _ = &mut *cancel => {
                let _ = finish(&mut child, &mut guard, true).await;
                return Some(End::Cancelled);
            }
            status = child.wait() => break status,
            () = tokio::time::sleep(TICK) => on_disk(file_len(partial)),
        }
    };
    // Reaped by the wait above: the group must not be signalled any more.
    if let Some(guard) = guard.as_mut() {
        guard.disarm();
    }
    match status {
        Ok(status) if status.success() => None,
        _ => Some(End::Failed(format!(
            "download failed: {} (run it again to resume)",
            error_line(&mut child).await
        ))),
    }
}

/// Moves a finished `partial` file to its blob once its size is right; a
/// wrong size removes it, since resuming it would only grow the mistake.
fn keep(partial: &Path, blob: &Path, file: &HubFile) -> Result<(), End> {
    if !has_size(partial, file.size) {
        let _ = std::fs::remove_file(partial);
        return Err(End::Failed("download failed: size mismatch".to_owned()));
    }
    std::fs::rename(partial, blob).map_err(|e| End::Failed(format!("download failed: {e}")))
}

/// Points `snapshots/<commit>/<path>` at the file's blob, replacing an older
/// link.
fn link(cache: &RepoCache, commit: &str, file: &HubFile) -> io::Result<()> {
    let at = cache.snapshot(commit, &file.path);
    if let Some(parent) = at.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::fs::symlink_metadata(&at).is_ok() {
        std::fs::remove_file(&at)?;
    }
    std::os::unix::fs::symlink(link_target(&file.path, &file.oid), &at)
}

fn write_ref(cache: &RepoCache, commit: &str) -> io::Result<()> {
    let at = cache.refs_main();
    if let Some(parent) = at.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(at, commit)
}

fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).map_or(0, |m| m.len())
}

fn has_size(path: &Path, size: u64) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() == size)
}

/// The bytes of `job` not yet on disk: blobs already there count as done,
/// partial files as far as they go.
pub fn missing_bytes(hub: &Path, job: &Job) -> u64 {
    let cache = RepoCache::new(hub, &job.id);
    job.choice
        .files
        .iter()
        .map(|f| {
            if has_size(&cache.blob(&f.oid), f.size) {
                0
            } else {
                f.size.saturating_sub(file_len(&cache.incomplete(&f.oid)))
            }
        })
        .sum()
}

/// Whether `need` bytes fit in `free`. An unknown free space never blocks.
pub fn space_check(need: u64, free: Option<u64>) -> Result<(), String> {
    match free {
        Some(free) if free < need => Err(format!(
            "not enough disk space: {} needed, {} free",
            size_label(need),
            size_label(free)
        )),
        _ => Ok(()),
    }
}

/// The bytes free for this user on the filesystem of `path`, or of its
/// nearest existing parent.
pub fn free_space(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let existing = path.ancestors().find(|p| p.exists())?;
    let c_path = std::ffi::CString::new(existing.as_os_str().as_bytes()).ok()?;
    // SAFETY: an all-zero statvfs is a valid value to be filled.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c_path` is a NUL-terminated path and `stat` is writable.
    if unsafe { libc::statvfs(c_path.as_ptr(), &raw mut stat) } != 0 {
        return None;
    }
    #[allow(clippy::useless_conversion)] // The field types differ by platform.
    Some(u64::from(stat.f_bavail).saturating_mul(u64::from(stat.f_frsize)))
}

#[cfg(test)]
mod tests;
