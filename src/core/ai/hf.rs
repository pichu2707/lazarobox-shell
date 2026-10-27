//! Hugging Face Hub downloads (model-download): reading the Hub's replies,
//! the GGUF choices of a repo, its license, the token and the cache layout.
//! Pure; the runtime runs curl and writes the files.
//!
//! Everything the Hub sends that ends up in a path (repo ids, commits, file
//! paths, object ids) is checked here, so a reply can never name a file
//! outside the repo's own cache folder.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::gguf::split_part;

/// The Hub every request goes to.
pub const HUB: &str = "https://huggingface.co";

/// A reply that is not the JSON the Hub sends.
pub const UNREADABLE: &str = "Hugging Face request failed: unreadable reply";

/// How many repos a search lists.
const SEARCH_LIMIT: usize = 20;

/// A repo found by a search.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RepoHit {
    pub id: String,
    pub downloads: u64,
}

/// What the Hub says about a repo: the commit its files are read at, its
/// license as stated and the names of its files.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Repo {
    pub id: String,
    pub commit: String,
    pub license_id: Option<String>,
    pub license_link: Option<String>,
    pub files: Vec<String>,
}

/// One file of a repo: its path in the repo, its size and the id it is
/// stored under in the cache (the LFS sha256, else the git object id).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HubFile {
    pub path: String,
    pub size: u64,
    pub oid: String,
}

/// One model a repo offers: a `.gguf` file, or every part of a split one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Choice {
    pub quant: String,
    pub size: u64,
    /// The files to download, the first part first.
    pub files: Vec<HubFile>,
}

impl Choice {
    /// The path of the file llama-server is started with: the only file, or
    /// the first part.
    pub fn path(&self) -> &str {
        &self.files[0].path
    }

    /// The file name of [`Choice::path`].
    pub fn name(&self) -> &str {
        file_name(self.path())
    }

    /// The picker row: `Q4_K_M  4.0 GB  a-Q4_K_M.gguf`, with `(2 parts)`
    /// after the size of a split model.
    pub fn label(&self) -> String {
        let parts = match self.files.len() {
            1 => String::new(),
            n => format!(" ({n} parts)"),
        };
        format!(
            "{}  {}{parts}  {}",
            self.quant,
            size_label(self.size),
            self.name()
        )
    }
}

/// What the picker shows of a repo: its commit, license and GGUF choices.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RepoFiles {
    pub repo: Repo,
    pub license: License,
    pub choices: Vec<Choice>,
}

impl RepoFiles {
    pub fn new(repo: Repo, files: &[HubFile]) -> Self {
        RepoFiles {
            license: license(&repo),
            choices: choices(files),
            repo,
        }
    }
}

/// A model to download: one choice of a repo at a commit.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Job {
    pub id: String,
    pub commit: String,
    pub choice: Choice,
    pub license: License,
}

/// A model's license and where to read it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct License {
    pub id: String,
    pub link: String,
}

/// A request of the picker's Hugging Face screens. The reply names the
/// request it answers, so the picker keeps only the one it still waits for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Ask {
    /// The GGUF repos a search finds.
    Search(String),
    /// The files of a valid repo id.
    Files(String),
}

/// What the runtime found for an [`Ask`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Answer {
    Repos(Vec<RepoHit>),
    Files(Box<Listing>),
}

/// What the Files and Confirm screens show of a repo: its choices, plus
/// where a download goes and the room there, read by the runtime.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Listing {
    pub files: RepoFiles,
    /// The hub cache folder; `None` when no folder could be found.
    pub hub: Option<PathBuf>,
    /// The bytes free on the hub cache's filesystem, when known.
    pub free: Option<u64>,
    /// The bytes each choice still needs, in choice order: a partial file
    /// counts as far as it goes.
    pub missing: Vec<u64>,
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

/// What a download that cannot find a hub cache folder reads as.
pub const NO_HUB: &str = "download failed: no Hugging Face cache folder (set HF_HOME)";

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

/// The environment the token is looked up in.
pub struct TokenEnv {
    pub hf_token: Option<String>,
    pub hf_token_path: Option<PathBuf>,
    pub hf_home: Option<PathBuf>,
    pub xdg_cache_home: Option<PathBuf>,
    pub home: Option<PathBuf>,
}

/// The repos of a search reply, the ones with a valid id, at most
/// [`SEARCH_LIMIT`].
pub fn parse_search(body: &str) -> Result<Vec<RepoHit>, String> {
    let Ok(Value::Array(items)) = serde_json::from_str(body) else {
        return Err(UNREADABLE.to_owned());
    };
    Ok(items
        .iter()
        .filter_map(|item| {
            let id = item.get("id")?.as_str().filter(|id| is_repo_id(id))?;
            Some(RepoHit {
                id: id.to_owned(),
                downloads: item.get("downloads").and_then(Value::as_u64).unwrap_or(0),
            })
        })
        .take(SEARCH_LIMIT)
        .collect())
}

/// A repo reply. Its id and commit must be valid, since both become paths.
pub fn parse_repo(body: &str) -> Result<Repo, String> {
    let value: Value = serde_json::from_str(body).map_err(|_| UNREADABLE.to_owned())?;
    let text = |v: Option<&Value>| v.and_then(Value::as_str).map(str::to_owned);
    let id = text(value.get("id")).filter(|id| is_repo_id(id));
    let commit = text(value.get("sha")).filter(|c| is_hex(c, 40));
    let (Some(id), Some(commit)) = (id, commit) else {
        return Err(UNREADABLE.to_owned());
    };
    let card = value.get("cardData");
    let license_id = card
        .and_then(|c| c.get("license"))
        .and_then(|l| match l {
            Value::Array(list) => list.first(),
            other => Some(other),
        })
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            value
                .get("tags")?
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .find_map(|t| t.strip_prefix("license:"))
                .map(str::to_owned)
        })
        .filter(|l| !l.trim().is_empty());
    let license_link = text(card.and_then(|c| c.get("license_link")));
    let files = value
        .get("siblings")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|s| s.get("rfilename")?.as_str())
                .filter(|p| is_safe_path(p))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Ok(Repo {
        id,
        commit,
        license_id,
        license_link,
        files,
    })
}

/// The files of a tree reply: entries of type `file` with a safe path and a
/// valid object id. Directories and anything else are left out.
pub fn parse_tree(body: &str) -> Result<Vec<HubFile>, String> {
    let Ok(Value::Array(items)) = serde_json::from_str(body) else {
        return Err(UNREADABLE.to_owned());
    };
    Ok(items
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("file"))
        .filter_map(|item| {
            let path = item.get("path")?.as_str().filter(|p| is_safe_path(p))?;
            let (oid, size) = match item.get("lfs") {
                Some(lfs) => (
                    lfs.get("oid")?.as_str().filter(|o| is_hex(o, 64))?,
                    lfs.get("size")?.as_u64()?,
                ),
                None => (
                    item.get("oid")?.as_str().filter(|o| is_hex(o, 40))?,
                    item.get("size")?.as_u64()?,
                ),
            };
            Some(HubFile {
                path: path.to_owned(),
                size,
                oid: oid.to_owned(),
            })
        })
        .collect())
}

/// The models of a repo's files: each `.gguf` that is not a vision projector,
/// a split model as one choice once all its parts are there, sorted by size
/// and then by name.
pub fn choices(files: &[HubFile]) -> Vec<Choice> {
    let mut out: Vec<Choice> = Vec::new();
    let ggufs = files.iter().filter(|f| {
        let lower = f.path.to_ascii_lowercase();
        lower.ends_with(".gguf") && !file_name(&lower).contains("mmproj")
    });
    for f in ggufs {
        let stem = &f.path[..f.path.len() - ".gguf".len()];
        match split(stem) {
            None => out.push(Choice {
                quant: quant(&f.path),
                size: f.size,
                files: vec![f.clone()],
            }),
            Some((base, 1, total)) => {
                let parts: Option<Vec<HubFile>> = (1..=total)
                    .map(|n| {
                        let path = format!("{base}-{n:05}-of-{total:05}.gguf");
                        files.iter().find(|g| g.path == path).cloned()
                    })
                    .collect();
                if let Some(parts) = parts {
                    out.push(Choice {
                        quant: quant(&f.path),
                        size: parts.iter().map(|p| p.size).sum(),
                        files: parts,
                    });
                }
            }
            Some(_) => {}
        }
    }
    out.sort_by(|a, b| a.size.cmp(&b.size).then_with(|| a.path().cmp(b.path())));
    out
}

/// `<base>-NNNNN-of-NNNNN` split into its base, part and total.
fn split(stem: &str) -> Option<(&str, u32, u32)> {
    let part = split_part(stem)?;
    let (rest, total) = stem.rsplit_once("-of-")?;
    let (base, _) = rest.rsplit_once('-')?;
    Some((base, part, total.parse().ok()?))
}

/// The quantization of a model file name: its last `Q<digit>…`,
/// `IQ<digit>…`, `F16`, `BF16` or `F32` token (tokens split by `-` and `.`),
/// upper-cased, or `?`.
pub fn quant(name: &str) -> String {
    let name = file_name(name);
    let stem = name.strip_suffix(".gguf").unwrap_or(name);
    let stem = match split(stem) {
        Some((base, _, _)) => base,
        None => stem,
    };
    let is_quant = |token: &str| {
        let upper = token.to_ascii_uppercase();
        let digit_after = |prefix: &str| {
            upper
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
        };
        digit_after("Q") || digit_after("IQ") || ["F16", "BF16", "F32"].contains(&upper.as_str())
    };
    stem.rsplit(['-', '.'])
        .find(|t| is_quant(t))
        .map_or_else(|| "?".to_owned(), str::to_ascii_uppercase)
}

/// A size in decimal units: whole numbers below a gigabyte, one decimal from
/// there (`415 MB`, `1.3 GB`).
pub fn size_label(bytes: u64) -> String {
    const UNITS: [(u64, &str); 3] = [(1_000_000_000_000, "TB"), (1_000_000_000, "GB"), (0, "")];
    for (scale, unit) in UNITS {
        if scale > 0 && bytes >= scale {
            return format!("{:.1} {unit}", bytes as f64 / scale as f64);
        }
    }
    match bytes {
        b if b >= 1_000_000 => format!("{} MB", b / 1_000_000),
        b if b >= 1_000 => format!("{} kB", b / 1_000),
        b => format!("{b} B"),
    }
}

/// The license to show before a download and the link to read it: the
/// stated link when it is https, else a `LICENSE*` file at the top of the
/// repo at its commit, else the model page.
pub fn license(repo: &Repo) -> License {
    let page = format!("{HUB}/{}", repo.id);
    let link = repo
        .license_link
        .clone()
        .filter(|l| l.starts_with("https://") && !l.chars().any(char::is_whitespace))
        .or_else(|| {
            repo.files
                .iter()
                .find(|f| !f.contains('/') && f.to_ascii_lowercase().starts_with("license"))
                .map(|f| format!("{HUB}/{}/blob/{}/{f}", repo.id, repo.commit))
        })
        .unwrap_or(page);
    License {
        id: repo
            .license_id
            .clone()
            .unwrap_or_else(|| "not stated".to_owned()),
        link,
    }
}

/// Whether `text` is a repo id: `<owner>/<name>`, each of ASCII letters,
/// digits, `.`, `_` and `-`, not starting or ending with `.` or `-`, with
/// no `..` and no `--` (the separator of the cache folder), as on the Hub.
pub fn is_repo_id(text: &str) -> bool {
    let part = |p: &str| {
        !p.is_empty()
            && p.len() <= 96
            && !p.starts_with(['.', '-'])
            && !p.ends_with(['.', '-'])
            && !p.contains("..")
            && !p.contains("--")
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    };
    matches!(text.split_once('/'), Some((owner, name)) if part(owner) && part(name))
}

/// Whether `path` is a relative path made only of normal components with no
/// control characters or backslashes, so it stays inside the folder it is
/// joined to.
fn is_safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != ".." && c.len() <= 255)
}

fn is_hex(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The search request for `text`: GGUF repos, the most downloaded first.
pub fn search_path(text: &str) -> String {
    format!(
        "/api/models?search={}&filter=gguf&sort=downloads&limit={SEARCH_LIMIT}",
        encode(text)
    )
}

/// The repo request of a valid repo id.
pub fn repo_path(id: &str) -> String {
    format!("/api/models/{id}")
}

/// The files of a repo at `commit`, folders included.
pub fn tree_path(id: &str, commit: &str) -> String {
    format!("/api/models/{id}/tree/{commit}?recursive=true")
}

/// Where a file of a repo is downloaded from.
pub fn resolve_url(id: &str, commit: &str, path: &str) -> String {
    format!("{HUB}{}", resolve_path(id, commit, path))
}

/// [`resolve_url`] without the host.
pub fn resolve_path(id: &str, commit: &str, path: &str) -> String {
    let path: Vec<String> = path.split('/').map(encode).collect();
    format!("/{id}/resolve/{commit}/{}", path.join("/"))
}

/// Percent-encodes everything but the unreserved characters.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// What an HTTP status that is not 200 reads as for repo `id`.
pub fn status_message(id: &str, status: u16) -> String {
    match status {
        401 | 403 => format!("{id} needs a Hugging Face token: set HF_TOKEN"),
        404 => format!("{id} not found on Hugging Face"),
        other => format!("Hugging Face request failed: HTTP {other}"),
    }
}

/// The token, if any: `HF_TOKEN`, else the file `$HF_TOKEN_PATH`, else
/// `$HF_HOME/token`, else `huggingface/token` in the cache home. Trimmed; a
/// blank one counts as none. Only absolute paths are read.
pub fn token(env: &TokenEnv, read: impl Fn(&Path) -> Option<String>) -> Option<String> {
    match env.hf_token.clone().and_then(clean) {
        Some(t) => Some(t),
        None => saved_token(env, read),
    }
}

/// A token trimmed; a blank one is none. The untrimmed text is zeroed.
fn clean(mut token: String) -> Option<String> {
    let trimmed = token.trim().to_owned();
    wipe(&mut token);
    Some(trimmed).filter(|t| !t.is_empty())
}

/// The token file of [`token`]'s lookup, the one `hf auth login` writes:
/// `$HF_TOKEN_PATH`, else `$HF_HOME/token`, else `huggingface/token` in the
/// cache home. Only absolute paths count.
pub fn token_file(env: &TokenEnv) -> Option<PathBuf> {
    let absolute = |p: &Option<PathBuf>| p.clone().filter(|p| p.is_absolute());
    absolute(&env.hf_token_path)
        .or_else(|| absolute(&env.hf_home).map(|h| h.join("token")))
        .or_else(|| {
            absolute(&env.xdg_cache_home)
                .or_else(|| absolute(&env.home).map(|h| h.join(".cache")))
                .map(|c| c.join("huggingface/token"))
        })
}

/// The token saved in [`token_file`], whatever `HF_TOKEN` says.
pub fn saved_token(env: &TokenEnv, read: impl Fn(&Path) -> Option<String>) -> Option<String> {
    read(&token_file(env)?).and_then(clean)
}

/// What lazarobox knows of the tokens when the menu opens: never a value.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct TokenFacts {
    /// `HF_TOKEN` is set and not blank: it is the token in use.
    pub env: bool,
    /// The token file holds a token.
    pub saved: bool,
}

/// The request that tells who a token belongs to.
pub const WHOAMI_PATH: &str = "/api/whoami-v2";

/// What a token the Hub refuses reads as.
pub const REJECTED: &str = "Hugging Face did not accept the token";

/// The account name of a whoami reply.
pub fn parse_whoami(body: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(body).map_err(|_| UNREADABLE.to_owned())?;
    value
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| UNREADABLE.to_owned())
}

/// What a whoami status that is not 200 reads as.
pub fn whoami_message(status: u16) -> String {
    match status {
        401 => REJECTED.to_owned(),
        other => format!("Hugging Face request failed: HTTP {other}"),
    }
}

/// A pasted token. It is never shown: its `Debug` hides it, and its bytes
/// are zeroed when it is dropped.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Secret(String);

impl Secret {
    pub fn new(token: String) -> Self {
        Secret(token)
    }

    /// The token, to hand to curl's stdin.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn chars(&self) -> usize {
        self.0.chars().count()
    }

    pub fn push(&mut self, c: char) {
        self.0.push(c);
    }

    pub fn pop(&mut self) {
        self.0.pop();
    }

    /// Empties it, zeroing what it held.
    pub fn clear(&mut self) {
        wipe(&mut self.0);
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Secret(<{} chars>)", self.chars())
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

/// Empties `text`, zeroing its bytes first.
pub fn wipe(text: &mut String) {
    let mut bytes = std::mem::take(text).into_bytes();
    bytes.fill(0);
    std::hint::black_box(&bytes);
}

/// An account request of the menu's `Hugging Face` row.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AccountAsk {
    /// Check a pasted token and, once the Hub accepts it, save it.
    Connect(Secret),
    /// Stop the connect in flight: unless it is already saved, its token is
    /// not.
    CancelConnect,
    /// Who the saved token belongs to.
    Check,
    /// Delete the token file.
    Disconnect,
}

/// What the runtime found for an [`AccountAsk`]: the account name, or why
/// it failed. It never carries the token.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AccountReply {
    Connected(Result<String, String>),
    Checked(Result<String, String>),
    Disconnected(Result<(), String>),
}

/// The cache folder of one repo, in the Hugging Face hub layout.
pub struct RepoCache {
    dir: PathBuf,
}

impl RepoCache {
    /// The folder of the valid repo id `id` under the hub cache `hub`.
    pub fn new(hub: &Path, id: &str) -> Self {
        RepoCache {
            dir: hub.join(format!("models--{}", id.replace('/', "--"))),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn blob(&self, oid: &str) -> PathBuf {
        self.dir.join("blobs").join(oid)
    }

    /// Where a blob is written until its download ends: not a `.gguf` name,
    /// so the model scan never lists it.
    pub fn incomplete(&self, oid: &str) -> PathBuf {
        self.dir.join("blobs").join(format!("{oid}.incomplete"))
    }

    pub fn snapshot(&self, commit: &str, path: &str) -> PathBuf {
        self.dir.join("snapshots").join(commit).join(path)
    }

    pub fn refs_main(&self) -> PathBuf {
        self.dir.join("refs/main")
    }
}

/// The relative link from `snapshots/<commit>/<path>` to its blob.
pub fn link_target(path: &str, oid: &str) -> PathBuf {
    let depth = path.split('/').count() + 1;
    let mut target = PathBuf::new();
    for _ in 0..depth {
        target.push("..");
    }
    target.join("blobs").join(oid)
}

#[cfg(test)]
mod tests;
