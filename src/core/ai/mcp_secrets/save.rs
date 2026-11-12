//! The pure part of the menu save (openspec `mcp-secrets`, Menu save; D22):
//! the one value of `secrets.toml` that changes, and every reason a save is
//! refused with its notice. The I/O is `runtime::secrets_write`. A notice
//! names at most a server, a name and a path, never a value or a text of the
//! file.

use std::{io::ErrorKind, path::PathBuf};

use toml_edit::{DocumentMut, Item, Value};

use crate::core::ai::{mcp_catalog, sanitize::clean};

/// Why a save (or a backup deletion) was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    NotAValue {
        server: String,
        name: String,
    },
    BadValue,
    ValueTooLong,
    NoFolder,
    FolderSymlink,
    FolderOwner,
    FolderWritable(PathBuf),
    FileSymlink,
    NotRegular,
    FileOwner,
    HardLinked,
    FileMode(PathBuf),
    /// A file that may keep its mode (`config.toml`) is writable by others.
    FileWritable(PathBuf),
    ReadOnly(PathBuf),
    TooLarge,
    Unreadable,
    NotToml(usize),
    NotPlainTable(String),
    NotString {
        server: String,
        name: String,
    },
    Changed,
    TempExists,
    BackupUnsafe,
    NoBackup,
    BadClientId,
    BadClientSecret,
    /// `google-oauth-client.json` is not a Desktop client file.
    BadClientFile,
    Exists,
    Io(ErrorKind),
}

impl Refusal {
    /// The one line the user sees.
    pub fn notice(&self) -> String {
        let text = match self {
            Self::NotAValue { server, name } => {
                format!("secrets.toml: {server}.{name} is not a value of this connector")
            }
            Self::BadValue => {
                "secrets.toml: the value must be one line without control characters".to_owned()
            }
            Self::ValueTooLong => {
                format!(
                    "secrets.toml: the value is longer than {} KiB",
                    MAX_VALUE / 1024
                )
            }
            Self::NoFolder => "secrets.toml: no config folder".to_owned(),
            Self::FolderSymlink => {
                "the config folder is a symlink; edit secrets.toml by hand".to_owned()
            }
            Self::FolderOwner => {
                "the config folder is not owned by you; edit secrets.toml by hand".to_owned()
            }
            Self::FolderWritable(path) => format!(
                "the config folder is writable by group or others; run chmod 700 {}",
                path.display()
            ),
            Self::FileSymlink => "secrets.toml is a symlink; edit it by hand".to_owned(),
            Self::NotRegular => super::NOT_REGULAR.to_owned(),
            Self::FileOwner => "secrets.toml is not owned by you; edit it by hand".to_owned(),
            Self::HardLinked => "secrets.toml has other hard links; edit it by hand".to_owned(),
            Self::FileMode(path) => super::mode_problem(path),
            Self::FileWritable(path) => format!(
                "secrets.toml is writable by group or others; run chmod go-w {}",
                path.display()
            ),
            Self::ReadOnly(path) => format!(
                "secrets.toml is read-only (mode 0400); chmod 600 {} to save from the menu",
                path.display()
            ),
            Self::TooLarge => super::TOO_LARGE.to_owned(),
            Self::Unreadable => super::UNREADABLE.to_owned(),
            Self::NotToml(line) => format!(
                "secrets.toml: not valid TOML (line {line}); fix it by hand, nothing was saved"
            ),
            Self::NotPlainTable(server) => {
                format!("secrets.toml: [{server}] is not a plain table; edit it by hand")
            }
            Self::NotString { server, name } => {
                format!("secrets.toml: {server}.{name}: expected a string; edit it by hand")
            }
            Self::Changed => {
                "secrets.toml changed on disk; nothing was saved, try again".to_owned()
            }
            Self::TempExists => "secrets.toml: not saved (temporary file exists)".to_owned(),
            Self::BackupUnsafe => {
                "secrets.toml.bak is not a regular file of yours; left as it is".to_owned()
            }
            Self::NoBackup => "secrets.toml.bak: there is no backup".to_owned(),
            Self::BadClientId => super::BAD_ID.to_owned(),
            Self::BadClientSecret => super::BAD_SECRET.to_owned(),
            Self::BadClientFile => format!(
                "{} is not a Desktop OAuth client; save it again (o)",
                super::CLIENT_FILE
            ),
            Self::Exists => "secrets.toml exists; open the form again to replace it".to_owned(),
            Self::Io(kind) => format!("secrets.toml: not saved ({kind})"),
        };
        clean(&text)
    }
}

impl Refusal {
    /// The notice for a save of `file` instead of `secrets.toml`.
    pub fn notice_of(&self, file: &str) -> String {
        self.notice().replace(super::FILE, file)
    }
}

/// The longest value a save accepts, in bytes.
pub const MAX_VALUE: usize = 8 * 1024;

/// The line (from 1) of byte `at`, for a syntax problem: never the parser's
/// own message, which can quote a value.
pub fn line_of(bytes: &[u8], at: usize) -> usize {
    let before = &bytes[..at.min(bytes.len())];
    before.iter().filter(|&&b| b == b'\n').count() + 1
}

/// `text` with `name` of the table `server` set to `value`. Only that value
/// changes: an existing key keeps its place, its key text and the comment
/// after it; a missing key goes at the end of its table; a missing table goes
/// at the end of the file after one empty line. A file whose lines all end in
/// CRLF gets CRLF on the lines added.
pub fn edit(text: &str, server: &str, name: &str, value: &str) -> Result<String, Refusal> {
    if value.len() > MAX_VALUE {
        return Err(Refusal::ValueTooLong);
    }
    // A leading BOM is kept as it is, outside what is parsed and edited.
    let (bom, body) = match text.strip_prefix('\u{feff}') {
        Some(rest) => ("\u{feff}", rest),
        None => ("", text),
    };
    let out = edit_body(body, server, name, value)?;
    if (bom.len() + out.len()) as u64 > super::MAX_BYTES {
        return Err(Refusal::TooLarge);
    }
    Ok(format!("{bom}{out}"))
}

/// `text` with each (server, name, value) of `values` set in turn, by the
/// rules of [`edit`]: the first refusal refuses them all.
pub fn edit_all(text: &str, values: &[(&str, &str, &str)]) -> Result<String, Refusal> {
    values
        .iter()
        .try_fold(text.to_owned(), |text, (server, name, value)| {
            edit(&text, server, name, value)
        })
}

fn edit_body(text: &str, server: &str, name: &str, value: &str) -> Result<String, Refusal> {
    let listed = mcp_catalog::entry(server).is_some_and(|e| e.env.iter().any(|v| v.name == name));
    if !listed {
        return Err(Refusal::NotAValue {
            server: server.to_owned(),
            name: name.to_owned(),
        });
    }
    if value.chars().any(char::is_control) {
        return Err(Refusal::BadValue);
    }
    let mut doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| {
        Refusal::NotToml(line_of(text.as_bytes(), e.span().map_or(0, |s| s.start)))
    })?;
    let crlf = text.contains("\r\n") && !text.replace("\r\n", "").contains('\n');
    let eol = if crlf { "\r\n" } else { "\n" };
    let quoted = format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""));
    let mut new: Value = quoted.parse().map_err(|_| Refusal::BadValue)?;
    let Some(item) = doc.get_mut(server) else {
        let mut out = text.to_owned();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push_str(eol);
        }
        if !out.is_empty() && !out.ends_with(&format!("{eol}{eol}")) {
            out.push_str(eol);
        }
        return Ok(format!("{out}[{server}]{eol}{name} = {quoted}{eol}"));
    };
    let plain = || Refusal::NotPlainTable(server.to_owned());
    let table = item.as_table_mut().ok_or_else(plain)?;
    if table.is_dotted() {
        return Err(plain());
    }
    match table.get_mut(name) {
        Some(old) => {
            let Some(old) = old.as_value_mut().filter(|v| v.is_str()) else {
                return Err(Refusal::NotString {
                    server: server.to_owned(),
                    name: name.to_owned(),
                });
            };
            *new.decor_mut() = old.decor().clone();
            *old = new;
        }
        None => {
            table.set_implicit(false);
            table.insert(name, Item::Value(new));
        }
    }
    let out = doc.to_string();
    // Every line ending of a CRLF file is CRLF, so a bare LF is one added.
    Ok(if crlf {
        out.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        out
    })
}

#[cfg(test)]
mod tests;
