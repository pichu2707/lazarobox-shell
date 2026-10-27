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
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::oneshot;

use super::proc::GroupKill;
use tokio::sync::mpsc;

use crate::app::AppEvent;
use crate::core::ai::hf::{
    self, AccountAsk, AccountReply, Answer, Ask, HubFile, Job, Listing, RepoCache, RepoFiles,
    RepoHit, Secret, TokenEnv, TokenFacts, link_target, status_message,
};
pub use crate::core::ai::hf::{End, space_check};
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

/// Who `curl`'s token belongs to: the account name, or why it failed. A
/// refused token reads as [`hf::REJECTED`].
pub async fn whoami(curl: &Curl) -> Result<String, String> {
    match curl.get(hf::WHOAMI_PATH).await? {
        (200, body) => hf::parse_whoami(&body),
        (code, _) => Err(hf::whoami_message(code)),
    }
}

/// What a connect that finds no token file reads as.
pub const NO_TOKEN_FILE: &str = "Hugging Face token not saved: no token file (set HF_HOME)";

/// Checks the pasted `token` with `curl` (which carries it) and, once the
/// Hub accepts it, saves it in the token file of `env`.
async fn connect(curl: &Curl, env: &TokenEnv, token: Secret) -> Result<String, String> {
    let name = whoami(curl).await?;
    let path = hf::token_file(env).ok_or_else(|| NO_TOKEN_FILE.to_owned())?;
    let saved = tokio::task::spawn_blocking(move || save_token(&path, token.expose())).await;
    match saved {
        Ok(Ok(())) => Ok(name),
        Ok(Err(e)) => Err(format!("Hugging Face token not saved: {e}")),
        Err(_) => Err("Hugging Face token not saved: interrupted".to_owned()),
    }
}

/// Deletes the token file of `env`; a missing one is already gone.
fn disconnect(env: &TokenEnv) -> Result<(), String> {
    let failed = |e: io::Error| format!("Hugging Face token not deleted: {e}");
    let Some(path) = hf::token_file(env) else {
        return Ok(());
    };
    // A symlinked file is deleted where it points: the token is there.
    let target = resolve_link(&path).map_err(failed)?;
    match std::fs::remove_file(&target) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(failed(e)),
        _ => Ok(()),
    }
}

/// Most symlinks followed when the target does not exist yet.
const MAX_LINK_HOPS: usize = 8;

/// The file `path` really names: an existing one through `canonicalize`, a
/// dangling symlink followed by hand (as the config save does), so a write
/// creates its target instead of replacing the link.
fn resolve_link(path: &Path) -> io::Result<PathBuf> {
    if let Ok(real) = std::fs::canonicalize(path) {
        return Ok(real);
    }
    let mut current = path.to_owned();
    for _ in 0..=MAX_LINK_HOPS {
        if !std::fs::symlink_metadata(&current).is_ok_and(|meta| meta.is_symlink()) {
            return Ok(current);
        }
        let link = std::fs::read_link(&current)?;
        current = current.parent().map_or(link.clone(), |dir| dir.join(link));
    }
    Err(io::Error::other("too many symlink levels"))
}

/// Writes `token` to the token file `path` with mode 0600, through a
/// temporary file in the same folder and a rename. A missing folder is made
/// with mode 0700; a symlinked file is written through.
fn save_token(path: &Path, token: &str) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let target = resolve_link(path)?;
    let (Some(dir), Some(name)) = (target.parent(), target.file_name()) else {
        return Err(io::Error::other("no token file"));
    };
    if std::fs::symlink_metadata(dir).is_err() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    let temp = dir.join(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ));
    let create = || {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
    };
    // A stale temporary file from a crashed run is replaced once.
    let mut file = match create() {
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            std::fs::remove_file(&temp)?;
            create()?
        }
        other => other?,
    };
    let written = file
        .write_all(token.as_bytes())
        .and_then(|()| file.sync_all())
        .and_then(|()| std::fs::rename(&temp, &target));
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written
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
            // Longer than the file: it cannot be resumed, so it starts over.
            if file_len(&partial) > file.size {
                let _ = std::fs::remove_file(&partial);
            }
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
            let (from, to, checked) = (partial.clone(), blob.clone(), file.clone());
            let kept = tokio::task::spawn_blocking(move || keep(&from, &to, &checked)).await;
            match kept {
                Ok(Ok(())) => {}
                Ok(Err(end)) => return end,
                Err(_) => return End::Failed("download failed: checksum interrupted".to_owned()),
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

/// Moves a finished `partial` file to its blob once its size and, for an
/// LFS file, its sha256 are right. A wrong file is removed, since resuming
/// it would only grow the mistake. Reads the whole file: run it off the
/// event loop.
fn keep(partial: &Path, blob: &Path, file: &HubFile) -> Result<(), End> {
    let failed = |why: &str| {
        let _ = std::fs::remove_file(partial);
        Err(End::Failed(format!("download failed: {why}")))
    };
    if !has_size(partial, file.size) {
        return failed("size mismatch");
    }
    // A 64-digit id is the LFS sha256; a git object id (40 digits) hashes a
    // header too, and GGUF files are always LFS.
    if file.oid.len() == 64 {
        match sha256(partial) {
            Ok(sum) if sum == file.oid => {}
            Ok(_) => return failed("checksum mismatch"),
            Err(e) => return Err(End::Failed(format!("download failed: {e}"))),
        }
    }
    std::fs::rename(partial, blob).map_err(|e| End::Failed(format!("download failed: {e}")))
}

/// The sha256 of a file, in lowercase hex.
fn sha256(path: &Path) -> io::Result<String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    io::copy(&mut std::fs::File::open(path)?, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
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

/// Most bytes of a token file read.
const TOKEN_CAP: u64 = 4 * 1024;

/// Where this process's environment says the Hugging Face token is (see
/// [`hf::token`]). Read at each request; the token is never stored or shown.
pub fn token_env() -> TokenEnv {
    let cwd = std::env::current_dir().unwrap_or_default();
    token_env_from(|name| std::env::var_os(name), &cwd)
}

/// [`token_env`] from the variables `var` gives. A relative
/// `HF_TOKEN_PATH` is read from `cwd`, as `huggingface_hub` reads it.
fn token_env_from(var: impl Fn(&str) -> Option<OsString>, cwd: &Path) -> TokenEnv {
    let path = |name: &str| var(name).map(PathBuf::from);
    TokenEnv {
        hf_token: var("HF_TOKEN").and_then(|t| t.into_string().ok()),
        hf_token_path: path("HF_TOKEN_PATH")
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| cwd.join(p)),
        hf_home: path("HF_HOME"),
        xdg_cache_home: path("XDG_CACHE_HOME"),
        home: path("HOME"),
    }
}

/// Whether `HF_TOKEN` is set and the token file holds a token: all the menu
/// learns of the tokens when it opens.
pub fn token_facts() -> TokenFacts {
    facts(&token_env())
}

fn facts(env: &TokenEnv) -> TokenFacts {
    let mut saved = hf::saved_token(env, read_token_file);
    let found = saved.is_some();
    if let Some(token) = saved.as_mut() {
        hf::wipe(token);
    }
    TokenFacts {
        env: env
            .hf_token
            .as_deref()
            .is_some_and(|t| !t.trim().is_empty()),
        saved: found,
    }
}

/// A token file, up to [`TOKEN_CAP`] bytes.
fn read_token_file(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(TOKEN_CAP)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

/// What the Files screen shows of `files`: where a download goes, the room
/// there and what each choice still needs.
fn listing(hub: Option<&Path>, files: RepoFiles) -> Listing {
    let missing = files
        .choices
        .iter()
        .map(|choice| match hub {
            Some(hub) => missing_bytes(
                hub,
                &Job {
                    id: files.repo.id.clone(),
                    commit: files.repo.commit.clone(),
                    choice: choice.clone(),
                    license: files.license.clone(),
                },
            ),
            None => choice.size,
        })
        .collect();
    let free = hub.and_then(|hub| free_space(RepoCache::new(hub, &files.repo.id).dir()));
    Listing {
        hub: hub.map(Path::to_owned),
        free,
        missing,
        files,
    }
}

/// Builds the curl of one request, carrying this token.
type MakeCurl = Arc<dyn Fn(Option<String>) -> Curl + Send + Sync>;

/// Reads where the token is, at each request.
type ReadEnv = Arc<dyn Fn() -> TokenEnv + Send + Sync>;

/// Runs the requests and the one download of the picker's Hugging Face
/// screens, and the account requests of the menu's `Hugging Face` row, in
/// the background, and sends what they find to the event loop.
pub struct Hub {
    make: MakeCurl,
    env: ReadEnv,
    /// The hub cache folder; `None` when none could be found.
    hub: Option<PathBuf>,
    events: mpsc::Sender<AppEvent>,
    /// The cancel of the running download, open while it runs.
    cancel: Option<oneshot::Sender<()>>,
}

impl Hub {
    /// A worker running curl from `path` (the `PATH` it is looked up in).
    pub fn new(
        path: OsString,
        hub: Option<PathBuf>,
        env: impl Fn() -> TokenEnv + Send + Sync + 'static,
        events: mpsc::Sender<AppEvent>,
    ) -> Self {
        Hub {
            make: Arc::new(move |token| Curl::hub(path.clone(), token)),
            env: Arc::new(env),
            hub,
            events,
            cancel: None,
        }
    }

    /// Tests: every request goes through the curl `make` builds.
    #[cfg(test)]
    fn with_curl(mut self, make: impl Fn(Option<String>) -> Curl + Send + Sync + 'static) -> Self {
        self.make = Arc::new(make);
        self
    }

    /// What builds the curl of one request, off the event loop: the token
    /// file is read there.
    fn curl(&self) -> impl FnOnce() -> Curl + Send + 'static {
        let (make, env) = (Arc::clone(&self.make), Arc::clone(&self.env));
        move || make(hf::token(&env(), read_token_file))
    }

    /// Runs one account request of the `Hugging Face` row; one
    /// `AppEvent::HfAccount` answers it. A pasted token goes only to curl's
    /// stdin and to the token file, and is dropped once the check is done.
    pub fn account(&self, ask: AccountAsk) {
        let (make, env, events) = (
            Arc::clone(&self.make),
            Arc::clone(&self.env),
            self.events.clone(),
        );
        tokio::spawn(async move {
            let env = env();
            let reply = match ask {
                AccountAsk::Connect(token) => {
                    let curl = make(Some(token.expose().to_owned()));
                    AccountReply::Connected(connect(&curl, &env, token).await)
                }
                AccountAsk::Check => {
                    let curl = make(hf::saved_token(&env, read_token_file));
                    AccountReply::Checked(whoami(&curl).await)
                }
                AccountAsk::Disconnect => AccountReply::Disconnected(disconnect(&env)),
            };
            let _ = events.send(AppEvent::HfAccount(reply)).await;
        });
    }

    /// Sends `ask` to the Hub; one `AppEvent::HubReply` answers it.
    pub fn ask(&self, ask: Ask) {
        let (curl, hub, events) = (self.curl(), self.hub.clone(), self.events.clone());
        tokio::spawn(async move {
            let curl = curl();
            let answer = match &ask {
                Ask::Search(text) => search(&curl, text).await.map(Answer::Repos),
                Ask::Files(id) => repo_files(&curl, id)
                    .await
                    .map(|files| Answer::Files(Box::new(listing(hub.as_deref(), files)))),
            };
            let _ = events.send(AppEvent::HubReply(ask, answer)).await;
        });
    }

    /// Starts downloading `job`, unless a download runs already. Its
    /// progress comes as `AppEvent::DownloadProgress` (dropped when the loop
    /// is behind) and its end as one `AppEvent::DownloadEnd`.
    pub fn start(&mut self, job: Job) {
        if self.cancel.as_ref().is_some_and(|tx| !tx.is_closed()) {
            return;
        }
        let (cancel, cancelled) = oneshot::channel();
        self.cancel = Some(cancel);
        let (curl, hub, events) = (self.curl(), self.hub.clone(), self.events.clone());
        tokio::spawn(async move {
            let curl = curl();
            let end = match hub {
                Some(hub) => {
                    let progress = |done, total| {
                        let _ = events.try_send(AppEvent::DownloadProgress { done, total });
                    };
                    download(&curl, &hub, &job, progress, cancelled).await
                }
                None => End::Failed(hf::NO_HUB.to_owned()),
            };
            let _ = events.send(AppEvent::DownloadEnd(end)).await;
        });
    }

    /// Stops the running download; its partial file stays for a resume.
    pub fn cancel(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod hub_tests {
    use std::path::PathBuf;

    use tokio::sync::mpsc;

    use super::*;
    use crate::app::AppEvent;
    use crate::core::ai::hf::{Ask, Choice, End, HubFile, License, NO_HUB};

    fn job() -> Job {
        Job {
            id: "o/r".to_owned(),
            commit: "c1".to_owned(),
            choice: Choice {
                quant: "Q4_K_M".to_owned(),
                size: 4,
                files: vec![HubFile {
                    path: "a-Q4_K_M.gguf".to_owned(),
                    size: 4,
                    oid: "a".repeat(64),
                }],
            },
            license: License {
                id: "mit".to_owned(),
                link: "https://huggingface.co/o/r".to_owned(),
            },
        }
    }

    fn no_env() -> TokenEnv {
        TokenEnv {
            hf_token: None,
            hf_token_path: None,
            hf_home: None,
            xdg_cache_home: None,
            home: None,
        }
    }

    /// A worker whose curl can never be found.
    fn worker(hub: Option<PathBuf>) -> (Hub, mpsc::Receiver<AppEvent>) {
        let (tx, rx) = mpsc::channel(8);
        (Hub::new(OsString::new(), hub, no_env, tx), rx)
    }

    #[tokio::test]
    async fn a_request_without_curl_replies_with_its_message() {
        let (hub, mut rx) = worker(None);
        hub.ask(Ask::Search("qwen".to_owned()));
        assert_eq!(
            rx.recv().await,
            Some(AppEvent::HubReply(
                Ask::Search("qwen".to_owned()),
                Err(NO_CURL.to_owned())
            ))
        );
    }

    #[tokio::test]
    async fn a_download_without_a_hub_folder_fails_clearly() {
        let (mut hub, mut rx) = worker(None);
        hub.start(job());
        assert_eq!(
            rx.recv().await,
            Some(AppEvent::DownloadEnd(End::Failed(NO_HUB.to_owned())))
        );
    }

    #[tokio::test]
    async fn a_second_download_is_refused_while_one_runs() {
        let dir = tempfile::tempdir().unwrap();
        let (mut hub, mut rx) = worker(Some(dir.path().to_owned()));
        // A download that has not ended keeps its cancel channel open.
        let (running, held) = oneshot::channel::<()>();
        hub.cancel = Some(running);
        hub.start(job());
        let second = tokio::time::timeout(Duration::from_millis(300), rx.recv()).await;
        assert!(second.is_err(), "the second download ran: {second:?}");
        drop(held);
    }

    #[tokio::test]
    async fn cancel_without_a_download_does_nothing() {
        let (mut hub, mut rx) = worker(None);
        hub.cancel();
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn the_token_file_is_read_up_to_its_cap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        std::fs::write(&path, "t".repeat(10_000)).unwrap();
        assert_eq!(read_token_file(&path).map(|t| t.len()), Some(4096));
        assert_eq!(read_token_file(&dir.path().join("none")), None);
    }

    #[test]
    fn a_listing_counts_the_bytes_still_missing() {
        let dir = tempfile::tempdir().unwrap();
        let job = job();
        let cache = RepoCache::new(dir.path(), &job.id);
        std::fs::create_dir_all(cache.dir().join("blobs")).unwrap();
        std::fs::write(cache.incomplete(&job.choice.files[0].oid), b"ab").unwrap();
        let repo = crate::core::ai::hf::Repo {
            id: job.id.clone(),
            commit: job.commit.clone(),
            license_id: None,
            license_link: None,
            files: Vec::new(),
        };
        let files = RepoFiles::new(repo, &job.choice.files);
        let found = listing(Some(dir.path()), files.clone());
        assert_eq!(found.missing, vec![2]);
        assert_eq!(found.hub.as_deref(), Some(dir.path()));
        assert!(found.free.is_some());
        let nowhere = listing(None, files);
        assert_eq!((nowhere.missing, nowhere.free), (vec![4], None));
    }
}
