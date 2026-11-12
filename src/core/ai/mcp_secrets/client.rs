//! The Desktop OAuth client file (openspec `mcp-secrets`, OAuth client file;
//! D27): the JSON the `ga4` login and the `gsc` OAuth way read, built from a
//! client id and a client secret the user types. Pure: the write is
//! `runtime::secrets_write`.

use super::{Refusal, SecretText};

/// The file, in the config folder.
pub const CLIENT_FILE: &str = "google-oauth-client.json";

/// The backup kept of an existing file, one generation.
pub const CLIENT_BACKUP: &str = "google-oauth-client.json.bak";

/// What every client id of a Desktop client ends in.
pub(super) const ID_SUFFIX: &str = ".apps.googleusercontent.com";

/// What the notice says of an id that is refused.
pub const BAD_ID: &str =
    "the client id must end in .apps.googleusercontent.com and use printable ASCII";

/// What the notice says of a secret that is refused.
pub const BAD_SECRET: &str =
    "the client secret must be one line of printable ASCII, without spaces";

fn printable(text: &str) -> bool {
    text.bytes().all(|b| b.is_ascii_graphic())
}

/// Whether `id` is a Desktop client id: the Google suffix, printable ASCII.
pub fn valid_id(id: &str) -> bool {
    id.len() > ID_SUFFIX.len() && id.ends_with(ID_SUFFIX) && printable(id)
}

/// Whether `secret` is one line of printable ASCII.
pub fn valid_secret(secret: &str) -> bool {
    !secret.is_empty() && printable(secret)
}

/// The file's text for `id` and `secret`, or why they are refused. Built by
/// `serde_json`, which does the escaping.
pub fn client_json(id: &str, secret: &str) -> Result<String, Refusal> {
    if !valid_id(id) {
        return Err(Refusal::BadClientId);
    }
    if !valid_secret(secret) {
        return Err(Refusal::BadClientSecret);
    }
    let json = serde_json::json!({"installed": {
        "client_id": id,
        "client_secret": secret,
        "auth_uri": "https://accounts.google.com/o/oauth2/auth",
        "token_uri": "https://oauth2.googleapis.com/token",
        "redirect_uris": ["http://localhost"],
    }});
    Ok(format!("{json:#}\n"))
}

/// The largest client file read back, in bytes (a file Google downloads is
/// under 1 KiB).
pub const CLIENT_MAX: usize = 16 * 1024;

/// The Desktop client a saved file holds. Its `{:?}` never shows the secret.
#[derive(Clone)]
pub struct Client {
    pub id: String,
    pub secret: SecretText,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("id", &self.id)
            .field("secret", &self.secret)
            .finish()
    }
}

/// The client of a file's text: the form's own file or the one Google
/// downloads (`installed.client_id`, `installed.client_secret`), checked as
/// the form checks typed values. Anything else is `BadClientFile`.
pub fn read_client(text: &str) -> Result<Client, Refusal> {
    if text.len() > CLIENT_MAX {
        return Err(Refusal::BadClientFile);
    }
    let json: serde_json::Value = serde_json::from_str(text).map_err(|_| Refusal::BadClientFile)?;
    let field = |name: &str| json.get("installed")?.get(name)?.as_str();
    let (Some(id), Some(secret)) = (field("client_id"), field("client_secret")) else {
        return Err(Refusal::BadClientFile);
    };
    if !valid_id(id) {
        return Err(Refusal::BadClientId);
    }
    if !valid_secret(secret) {
        return Err(Refusal::BadClientSecret);
    }
    let mut kept = SecretText::new();
    kept.push_str(secret);
    Ok(Client {
        id: id.to_owned(),
        secret: kept,
    })
}
