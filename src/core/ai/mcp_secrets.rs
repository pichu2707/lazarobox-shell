//! The values of the user's MCP servers, from `secrets.toml` (openspec
//! `mcp-secrets`, decision C6).
//!
//! The file sits next to `config.toml` and the user edits it: lazarobox never
//! creates, writes or chmods it, except by the menu save of one value (see
//! [`edit`] and `runtime::secrets_write`). Each top-level table is named after a server
//! and holds that server's variables. A value reaches only the server whose
//! name is its table, and only for a name its `[[ai.mcp.servers]]` entry lists
//! in `env` ([`Secrets::lookup_for`]); the spawn code still drops `HOME`, the
//! session token and loader variables whatever the source.
//!
//! Pure: the runtime reads the file (bounded, mode 0600, regular file) and
//! hands the bytes here. A problem names at most a table and a key, never a
//! value, and a syntax error gives its line only, because the parser's own
//! message can quote the text of the line.

use std::{
    collections::HashMap,
    ffi::OsString,
    fmt,
    path::{Path, PathBuf},
};

use toml::Value;

use super::{
    config::{McpServer, valid_env_name},
    sanitize::{clean, redact_tokens},
};
use crate::core::config::config_path;

/// The file name, in the folder of `config.toml`.
pub const FILE: &str = "secrets.toml";

/// The backup the menu save keeps of the file, one generation.
pub const BACKUP: &str = "secrets.toml.bak";

/// The largest file that is used; a larger one is not read at all.
pub const MAX_BYTES: u64 = 64 * 1024;

/// The problem of a path that is not a regular file.
pub const NOT_REGULAR: &str = "secrets.toml: not a regular file";

/// The problem of a file over [`MAX_BYTES`].
pub const TOO_LARGE: &str = "secrets.toml: larger than 64 KiB";

/// The problem of a file that exists but cannot be opened or read.
pub const UNREADABLE: &str = "secrets.toml: cannot be read";

/// The longest key or table name a problem shows, in characters.
const SHOWN_MAX: usize = 64;

/// Where the file is: next to `config.toml`, so
/// `$XDG_CONFIG_HOME/lazarobox/secrets.toml`, else
/// `$HOME/.config/lazarobox/secrets.toml` (same rules as [`config_path`]).
pub fn secrets_path(xdg: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    Some(config_path(xdg, home)?.with_file_name(FILE))
}

/// The problem of a file that others may read. The path is sanitized.
pub fn mode_problem(path: &Path) -> String {
    clean(&format!(
        "secrets.toml: mode must be 0600 (chmod 600 {})",
        path.display()
    ))
}

/// The values of the file, per server table. Its `Debug` shows the table
/// names and how many values each holds, never a variable name or a value.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secrets {
    tables: HashMap<String, HashMap<String, String>>,
}

impl fmt::Debug for Secrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut names: Vec<(&str, usize)> = self
            .tables
            .iter()
            .map(|(name, values)| (name.as_str(), values.len()))
            .collect();
        names.sort_unstable();
        f.debug_struct("Secrets").field("tables", &names).finish()
    }
}

/// What reading the file gave: the values kept, and one line per problem.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Loaded {
    pub secrets: Secrets,
    pub problems: Vec<String>,
}

impl Loaded {
    /// No values, and `problem` as the only problem.
    pub fn problem(problem: String) -> Self {
        Self {
            secrets: Secrets::default(),
            problems: vec![problem],
        }
    }
}

impl Secrets {
    /// Parses the bytes of the file. Bad keys and tables are skipped alone and
    /// reported; text that is not UTF-8 or not TOML is reported by line and
    /// gives no values at all.
    pub fn parse(bytes: &[u8]) -> Loaded {
        let text = match std::str::from_utf8(bytes) {
            Ok(text) => text,
            Err(error) => return Loaded::problem(syntax_problem(bytes, error.valid_up_to())),
        };
        let table = match text.parse::<toml::Table>() {
            Ok(table) => table,
            Err(error) => {
                let at = error.span().map_or(0, |span| span.start);
                return Loaded::problem(syntax_problem(bytes, at));
            }
        };
        let mut loaded = Loaded::default();
        for (server, values) in table {
            let Value::Table(values) = values else {
                loaded.problems.push(clean(&format!(
                    "secrets.toml: {}: expected a table",
                    redact_tokens(&shown(&server))
                )));
                continue;
            };
            let mut kept = HashMap::new();
            for (name, value) in values {
                // A rejected name may be a token pasted in the wrong place,
                // so it is redacted like a secret in a tool result.
                let (key, problem) = if !valid_env_name(&name) {
                    (redact_tokens(&shown(&name)), "expected a variable name")
                } else if let Value::String(value) = value {
                    kept.insert(name, value);
                    continue;
                } else {
                    (shown(&name), "expected a string")
                };
                loaded.problems.push(clean(&format!(
                    "secrets.toml: {}.{key}: {problem}",
                    redact_tokens(&shown(&server))
                )));
            }
            loaded.secrets.tables.insert(server, kept);
        }
        loaded
    }

    /// The value of `name` in the table of `server`, if any.
    pub fn value(&self, server: &str, name: &str) -> Option<&str> {
        self.tables.get(server)?.get(name).map(String::as_str)
    }

    /// The value lookup of one server: a name its entry lists in `env` takes
    /// its value from the server's own table, else from `host` (lazarobox's
    /// environment); any other name comes from `host` only, so the base
    /// variables of the spawn are never taken from the file.
    pub fn lookup_for<'a>(
        &'a self,
        server: &'a McpServer,
        host: &'a impl Fn(&str) -> Option<OsString>,
    ) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            let named = server.env.iter().any(|listed| listed == name);
            named
                .then(|| self.value(&server.name, name))
                .flatten()
                .map(OsString::from)
                .or_else(|| host(name))
        }
    }
}

/// Every value the child of `server` gets (each name of its `env` through
/// `lookup`, the file first and the host as fallback: what the child really
/// receives), and the forms a log shows it in: percent-encoded (both cases of
/// the hex digits) and JSON-escaped. What [`redact_values`] removes. An empty
/// value is skipped.
pub fn redact_list(server: &McpServer, lookup: &impl Fn(&str) -> Option<OsString>) -> Vec<String> {
    let mut list: Vec<String> = Vec::new();
    for name in &server.env {
        let Some(value) = lookup(name) else { continue };
        let value = value.to_string_lossy().into_owned();
        if value.is_empty() {
            continue;
        }
        let json = serde_json::to_string(&value).unwrap_or_default();
        let json = json.trim_matches('"').to_owned();
        let upper = percent_encode(&value);
        let lower = percent_encode_lower(&value);
        for form in [value, upper, lower, json] {
            if !list.contains(&form) {
                list.push(form);
            }
        }
    }
    list
}

fn percent_with(text: &str, digits: &[u8; 16]) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(digits[usize::from(byte >> 4)]));
            out.push(char::from(digits[usize::from(byte & 15)]));
        }
    }
    out
}

fn percent_encode(text: &str) -> String {
    percent_with(text, b"0123456789ABCDEF")
}

fn percent_encode_lower(text: &str) -> String {
    percent_with(text, b"0123456789abcdef")
}

/// What replaces a value in text that a server said.
pub const REDACTED: &str = "[redacted]";

/// `text` with every occurrence of each non-empty `values` entry replaced by
/// [`REDACTED`], the longest value first so a value that holds another is not
/// left half shown. Call it on the WHOLE text, before any cut: a cut may split
/// a value and leave its start in view. Text a server says (stderr, an RPC
/// error, a tool result) can echo a value in a shape no pattern knows. A value
/// of one character, or of whitespace only, replaces every such character in
/// the text: the text is mangled, never leaked.
pub fn redact_values(text: &str, values: &[String]) -> String {
    let mut ordered: Vec<&str> = values
        .iter()
        .map(String::as_str)
        .filter(|v| !v.is_empty())
        .collect();
    ordered.sort_by_key(|v| std::cmp::Reverse(v.len()));
    // One pass, so a value that sits inside `[redacted]` is never matched.
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(first) = rest.chars().next() {
        match ordered.iter().find(|v| rest.starts_with(**v)) {
            Some(value) => {
                out.push_str(REDACTED);
                rest = &rest[value.len()..];
            }
            None => {
                out.push(first);
                rest = &rest[first.len_utf8()..];
            }
        }
    }
    out
}

/// The problem of text that is not UTF-8 or not TOML, by the line of byte
/// `at`: never the parser's message, which can quote a value.
fn syntax_problem(bytes: &[u8], at: usize) -> String {
    format!("secrets.toml: not valid TOML (line {})", line_of(bytes, at))
}

/// A table or key name as a problem shows it: bare when TOML would write it
/// bare, else quoted with its control characters escaped; cut when long.
fn shown(name: &str) -> String {
    let bare = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    let text = if bare {
        name.to_owned()
    } else {
        format!("\"{}\"", name.escape_debug())
    };
    if text.chars().count() > SHOWN_MAX {
        let cut: String = text.chars().take(SHOWN_MAX).collect();
        format!("{cut}…")
    } else {
        text
    }
}

mod client;
mod save;
mod text;
pub use client::{
    BAD_ID, BAD_SECRET, CLIENT_BACKUP, CLIENT_FILE, CLIENT_MAX, Client, client_json, read_client,
    valid_id, valid_secret,
};
pub use save::{Refusal, edit, edit_all, line_of};
pub use text::SecretText;

#[cfg(test)]
mod client_tests;

#[cfg(test)]
mod text_tests;

#[cfg(test)]
mod tests;
