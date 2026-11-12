//! Connect Google's reads (openspec change `google-connect`, S5): the OAuth
//! client file, and the plan checked against the files it will be written
//! to. Each file is opened by handle in the folder judged as the writer judges
//! it, so a refusal the write would meet shows on the plan instead. Nothing
//! here writes, and a notice names a file or a path, never a value.

use std::{io::ErrorKind, path::Path};

use super::{RealFs, SECRETS, SaveFs, Target, open_folder, read_current};
use crate::core::{
    ai::{
        mcp_catalog::{
            connect::{Ask, Input, Preview, preview},
            presence::Presence,
        },
        mcp_secrets::{
            CLIENT_FILE, CLIENT_MAX, Client, FILE, Refusal, Secrets, line_of, read_client,
        },
    },
    config_append::{self, AppendRefusal},
};

/// `config.toml` as the append judges it, for the plan's read.
const CONFIG: Target = Target {
    file: config_append::FILE,
    backup: config_append::BACKUP,
    max: config_append::MAX_BYTES,
    keep_mode: true,
};

/// The plan's notice when there is no client file.
pub const NO_CLIENT: &str = "connect: no google-oauth-client.json yet; press o to save your client";

/// The Desktop client in `google-oauth-client.json` next to `secrets`, `None`
/// when there is none (no folder is created). Judged on the open handle: a
/// regular file of the user's, not a symlink, not hard-linked, with no group
/// or other permission, at most [`CLIENT_MAX`] bytes. The secret stays in the
/// [`Client`]; a refusal never holds a text of the file.
pub fn read_client_with(
    fs: &impl SaveFs,
    secrets: Option<&Path>,
) -> Result<Option<Client>, Refusal> {
    let Some(folder) = secrets.and_then(Path::parent) else {
        return Ok(None);
    };
    if std::fs::symlink_metadata(folder).is_err() {
        return Ok(None);
    }
    let dir = open_folder(fs, secrets)?;
    let fd = match fs.open_read(&dir, CLIENT_FILE) {
        Ok(fd) => fd,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) if e.raw_os_error() == Some(libc::ELOOP) => return Err(Refusal::FileSymlink),
        Err(_) => return Err(Refusal::Unreadable),
    };
    let stat = fs.fstat(&fd).map_err(|_| Refusal::Unreadable)?;
    if !stat.regular {
        return Err(Refusal::NotRegular);
    }
    if stat.uid != fs.euid() {
        return Err(Refusal::FileOwner);
    }
    if stat.nlink > 1 {
        return Err(Refusal::HardLinked);
    }
    if stat.mode & 0o077 != 0 {
        return Err(Refusal::FileMode(folder.join(CLIENT_FILE)));
    }
    let max = CLIENT_MAX as u64;
    if stat.len > max {
        return Err(Refusal::TooLarge);
    }
    let bytes = fs.read(&fd, max + 1).map_err(|_| Refusal::Unreadable)?;
    if bytes.len() as u64 > max {
        return Err(Refusal::TooLarge);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| Refusal::BadClientFile)?;
    read_client(text).map(Some)
}

/// What the screen's `client:` line says: the id, `None` when there is no
/// file, or the notice of a refused one.
pub fn client_id(secrets: Option<&Path>) -> Result<Option<String>, String> {
    read_client_with(&RealFs, secrets)
        .map(|client| client.map(|c| c.id))
        .map_err(|refusal| refusal.notice_of(CLIENT_FILE))
}

/// The plan of `ask` from the files next to `secrets` as they are now: the
/// client, `secrets.toml` (which values hold something) and `config.toml`
/// (which servers are declared, and the bytes the append will start from),
/// each read with the writer's checks. The answer is the preview, or the one
/// line that says why there is none.
pub fn plan_with(
    fs: &impl SaveFs,
    secrets: Option<&Path>,
    home: Option<&str>,
    ask: &Ask,
) -> Result<Preview, String> {
    let client = match read_client_with(fs, secrets) {
        Ok(Some(client)) => client,
        Ok(None) => return Err(NO_CLIENT.to_owned()),
        Err(refusal) => return Err(refusal.notice_of(CLIENT_FILE)),
    };
    let dir = open_folder(fs, secrets).map_err(|refusal| refusal.notice())?;
    let folder = secrets.and_then(Path::parent).unwrap_or(Path::new(""));
    let config = read_current(fs, &dir, &folder.join(config_append::FILE), CONFIG)
        .map_err(|refusal| AppendRefusal::from(refusal).notice())?
        .map(|(bytes, _)| bytes);
    let stored = read_current(fs, &dir, &folder.join(FILE), SECRETS)
        .map_err(|refusal| refusal.notice())?
        .map(|(bytes, _)| bytes);
    let stored = match &stored {
        Some(bytes) => std::str::from_utf8(bytes)
            .map_err(|e| Refusal::NotToml(line_of(bytes, e.valid_up_to())).notice())?,
        None => "",
    };
    let text = match &config {
        Some(bytes) => {
            let text = std::str::from_utf8(bytes)
                .map_err(|e| AppendRefusal::NotToml(line_of(bytes, e.valid_up_to())).notice())?;
            let body = text.strip_prefix('\u{feff}').unwrap_or(text);
            if let Err(e) = body.parse::<toml::Table>() {
                let at = e.span().map_or(0, |span| span.start);
                return Err(AppendRefusal::NotToml(line_of(body.as_bytes(), at)).notice());
            }
            Ok(body.to_owned())
        }
        None => Err(ErrorKind::NotFound.into()),
    };
    let presence = Presence::read(text, &Secrets::parse(stored.as_bytes()));
    let Some(servers) = presence.servers.as_deref() else {
        return Err("connect: config.toml could not be read; fix it, then plan again".to_owned());
    };
    let has = |table: &str, name: &str| presence.has(table, name);
    let input = Input {
        servers,
        names: &presence.names,
        has: &has,
        home,
        client: &client,
        email: &ask.email,
        chosen: &ask.chosen,
    };
    preview(&input, config.as_deref(), stored)
}

#[cfg(test)]
mod tests;
