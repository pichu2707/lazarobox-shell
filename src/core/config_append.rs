//! Connect Google's append to `config.toml` (openspec change `google-connect`,
//! Approach 3): the pure part. The new text is the old bytes unchanged, then
//! the catalog entries; it is kept only when both texts parse with the parser
//! lazarobox reads its config with and the server list grew by exactly the
//! announced entries. The I/O is `runtime::config_append`. A notice names at
//! most a server and a path, never a text of the file or a value.

use sha2::{Digest, Sha256};
use toml::Value;

use crate::core::{
    ai::{
        mcp_catalog::connect::Plan,
        mcp_secrets::{Refusal, SecretText, line_of},
        sanitize::clean,
    },
    config::{Config, Parsed},
};

/// The file appended to, and the backup kept of it.
pub const FILE: &str = "config.toml";
pub const BACKUP: &str = "config.toml.bak";

/// The largest file read or written.
pub const MAX_BYTES: u64 = 256 * 1024;

/// Why the append was refused; nothing was written to `config.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppendRefusal {
    /// The file is not UTF-8 or not TOML, by line.
    NotToml(usize),
    /// The file would not parse with the entries after it: an inline
    /// `ai.mcp.servers` array, which a `[[...]]` header cannot extend.
    WouldBreak,
    /// The file already declares this server, in some form.
    Declared(String),
    /// The parsed servers would not be the old ones followed by the new.
    ServersDiffer,
    /// A check or a step of the write (the secrets writer's refusals).
    File(Refusal),
}

impl From<Refusal> for AppendRefusal {
    fn from(refusal: Refusal) -> Self {
        Self::File(refusal)
    }
}

impl AppendRefusal {
    /// The one line the user sees.
    pub fn notice(&self) -> String {
        let text = match self {
            Self::NotToml(line) => format!("{FILE}: not valid TOML (line {line})"),
            Self::WouldBreak => format!(
                "{FILE}: the entries would not parse after it (an inline ai.mcp.servers list?)"
            ),
            Self::Declared(name) => format!("{FILE} already declares {name}"),
            Self::ServersDiffer => format!("{FILE}: the append would change its server list"),
            Self::File(Refusal::TooLarge) => {
                format!("{FILE}: larger than {} KiB", MAX_BYTES / 1024)
            }
            Self::File(Refusal::ReadOnly(path)) => format!(
                "{FILE} is read-only; chmod u+w {} to write it from the menu",
                path.display()
            ),
            Self::File(refusal) => refusal.notice_of(FILE),
        };
        clean(&text)
    }
}

/// `old` with `snippets` after it, when the result keeps every server of
/// `old` and adds exactly `names`, in order. The old bytes stay a prefix: a
/// newline is added when they do not end in one, and one empty line before
/// the entries. The lines added are CRLF when the file's first line break is.
/// A missing file is the empty text.
pub fn appended(old: &str, snippets: &str, names: &[&str]) -> Result<String, AppendRefusal> {
    let raw: toml::Table = old.parse().map_err(|e: toml::de::Error| {
        AppendRefusal::NotToml(line_of(old.as_bytes(), e.span().map_or(0, |s| s.start)))
    })?;
    if let Some(name) = names.iter().find(|name| declares(&raw, name)) {
        return Err(AppendRefusal::Declared((*name).to_owned()));
    }
    let before = Config::parse(old).map_err(|_| AppendRefusal::NotToml(1))?;
    let crlf = old.find('\n').is_some_and(|at| old[..at].ends_with('\r'));
    let eol = if crlf { "\r\n" } else { "\n" };
    let body = old.strip_prefix('\u{feff}').unwrap_or(old);
    let mut new = old.to_owned();
    if !body.is_empty() {
        if !new.ends_with('\n') {
            new.push_str(eol);
        }
        if !new.ends_with(&format!("{eol}{eol}")) {
            new.push_str(eol);
        }
    }
    new.push_str(&if crlf {
        snippets.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        snippets.to_owned()
    });
    if new.len() as u64 > MAX_BYTES {
        return Err(Refusal::TooLarge.into());
    }
    let after = Config::parse(&new).map_err(|_| AppendRefusal::WouldBreak)?;
    if !grew_by(&before, &after, names) {
        return Err(AppendRefusal::ServersDiffer);
    }
    Ok(new)
}

/// Whether an `ai.mcp.servers` entry of `raw` is named `name`, valid or not,
/// limited to a project or not.
fn declares(raw: &toml::Table, name: &str) -> bool {
    declared_names(raw).any(|declared| declared == name)
}

/// The `name` string of every `ai.mcp.servers` entry of `raw`, valid or not
/// (the parsed server list skips an invalid one), limited to a project or
/// not. A service named here is declared, in some form.
pub fn declared_names(raw: &toml::Table) -> impl Iterator<Item = &str> {
    raw.get("ai")
        .and_then(|ai| ai.get("mcp"))
        .and_then(|mcp| mcp.get("servers"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("name").and_then(Value::as_str))
}

/// Whether `after` lists the servers of `before` unchanged, then `names`, and
/// the append raised no new problem.
fn grew_by(before: &Parsed, after: &Parsed, names: &[&str]) -> bool {
    let (old, new) = (before.ai.mcp_servers(), after.ai.mcp_servers());
    new.len() == old.len() + names.len()
        && new[..old.len()] == *old
        && new[old.len()..]
            .iter()
            .map(|s| s.name.as_str())
            .eq(names.iter().copied())
        && before.problems == after.problems
}

/// The digest of the bytes a plan was shown with.
pub fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// One Connect Google write, built from a plan after the user's `y`: the
/// `secrets.toml` values, then the entries appended to `config.toml`. Its
/// `{:?}` never shows a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connect {
    /// (table, name, value) of each value to save.
    pub set: Vec<(&'static str, &'static str, SecretText)>,
    pub snippets: String,
    /// The servers the snippets declare, in order.
    pub names: Vec<&'static str>,
    /// The digest of `config.toml` as the plan saw it; `None`: no file.
    pub expected: Option<[u8; 32]>,
}

impl Connect {
    /// The write of `plan`, shown with `config.toml` holding `on_disk`.
    pub fn new(plan: Plan, on_disk: Option<&[u8]>) -> Self {
        Self {
            snippets: plan.snippets(),
            names: plan.append.iter().map(|e| e.id).collect(),
            set: plan
                .set
                .into_iter()
                .map(|s| (s.table, s.name, s.value))
                .collect(),
            expected: on_disk.map(digest),
        }
    }
}

/// What became of a [`Connect`]; each holds its one-line notice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Connected {
    /// The values were saved and the entries appended.
    Done(String),
    /// Nothing was written.
    Refused(String),
    /// The values were saved; `config.toml` was not changed.
    ValuesOnly(String),
}

impl Connected {
    pub fn notice(&self) -> &str {
        match self {
            Self::Done(text) | Self::Refused(text) | Self::ValuesOnly(text) => text,
        }
    }
}

#[cfg(test)]
mod tests;
