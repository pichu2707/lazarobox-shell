//! What the Connectors area reads from disk when it opens (D20): the
//! `[[ai.mcp.servers]]` of `config.toml` and which catalog values have a
//! non-empty entry in `secrets.toml`. It holds names, never a value, so
//! neither the area nor a `{:?}` can draw one.

use std::{
    collections::{HashMap, HashSet},
    io,
    path::Path,
};

use super::{entries, gsc, link};
use crate::core::{
    ai::{config::McpServer, mcp_secrets::Loaded},
    config::Config,
};

/// The disk copy of the declared servers and the names that have a value.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Presence {
    /// `None` when `config.toml` could not be read or parsed.
    pub servers: Option<Vec<McpServer>>,
    set: HashSet<(String, String)>,
    /// The values the entries opt out of confidentiality (`Var.shown`): ids,
    /// paths, an e-mail. A confidential value is never kept here.
    shown: HashMap<(String, String), String>,
    /// `secrets.toml.bak` exists.
    backup: bool,
    /// `google-oauth-client.json` exists.
    client: bool,
    /// The home folder, for the presets that live under it.
    home: Option<String>,
    /// `gsc` in its OAuth way: whether its token file exists (`stat` only).
    linked: Option<bool>,
    /// `google` with an account: whether its credentials file exists.
    google: Option<bool>,
    /// What made `secrets.toml` unused or partly unused: one line each, naming
    /// a table, a key or a path at most, never a value.
    pub problems: Vec<String>,
}

impl std::fmt::Debug for Presence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Presence")
            .field("servers", &self.servers)
            .field("set", &self.set.len())
            .field("shown", &self.shown.len())
            .field("problems", &self.problems.len())
            .finish()
    }
}

impl Presence {
    /// Builds it from the read of `config.toml` (a missing file declares
    /// nothing) and the parsed `secrets.toml`. Only the secret names of the
    /// catalog are looked up.
    pub fn read(config: io::Result<String>, loaded: &Loaded) -> Self {
        let servers = match config {
            Ok(text) => Config::parse(&text)
                .ok()
                .map(|parsed| parsed.ai.mcp_servers().to_vec()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Some(Vec::new()),
            Err(_) => None,
        };
        let set = entries()
            .iter()
            .flat_map(|entry| {
                entry
                    .env
                    .iter()
                    .filter(|v| v.secret)
                    .map(move |v| (entry, v))
            })
            .filter(|(entry, v)| {
                loaded
                    .secrets
                    .value(entry.id, v.name)
                    .is_some_and(|s| !s.is_empty())
            })
            .map(|(entry, v)| (entry.id.to_owned(), v.name.to_owned()))
            .collect();
        let shown = entries()
            .iter()
            .flat_map(|entry| entry.env.iter().map(move |v| (entry, v)))
            .filter(|(_, v)| v.secret && !v.confidential())
            .filter_map(|(entry, v)| {
                let value = loaded.secrets.value(entry.id, v.name)?;
                (!value.is_empty())
                    .then(|| ((entry.id.to_owned(), v.name.to_owned()), value.to_owned()))
            })
            .collect();
        Self {
            servers,
            set,
            shown,
            backup: false,
            client: false,
            home: None,
            linked: None,
            google: None,
            problems: loaded.problems.clone(),
        }
    }

    pub fn with_backup(mut self, yes: bool) -> Self {
        self.backup = yes;
        self
    }

    pub fn with_home(mut self, home: Option<String>) -> Self {
        self.home = home;
        self
    }

    /// Looks for the token of a declared `gsc` whose values select the OAuth
    /// way: `exists` is a `stat`, and the file is never opened.
    pub fn with_token(mut self, exists: impl Fn(&Path) -> bool) -> Self {
        let value = |name: &str| self.shown("gsc", name).map(str::to_owned);
        let server = self.servers.iter().flatten().find(|s| s.name == "gsc");
        self.linked = server.and_then(|s| gsc::linked(s, &value, exists));
        self
    }

    /// Looks for the credentials file of the account of a declared `google`:
    /// `exists` is a `stat`, and the file is never opened.
    pub fn with_credentials(mut self, exists: impl Fn(&Path) -> bool) -> Self {
        let declared = self.servers.iter().flatten().any(|s| s.name == "google");
        let email = self.shown("google", "USER_GOOGLE_EMAIL");
        let home = self
            .home
            .as_deref()
            .map(Path::new)
            .filter(|h| h.is_absolute());
        if let (true, Some(email), Some(home)) = (declared, email, home) {
            let dir = home.join(".google_workspace_mcp/credentials");
            let names = link::credentials_names(email);
            self.google = Some(names.iter().any(|name| exists(&dir.join(name))));
        }
        self
    }

    /// `Some(linked)` when `google` is declared with an account.
    pub fn google_linked(&self) -> Option<bool> {
        self.google
    }

    /// `Some(linked)` when `gsc` is in its OAuth way.
    pub fn linked(&self) -> Option<bool> {
        self.linked
    }

    pub fn backup(&self) -> bool {
        self.backup
    }

    pub fn with_client_file(mut self, yes: bool) -> Self {
        self.client = yes;
        self
    }

    /// `google-oauth-client.json` exists.
    pub fn client_file(&self) -> bool {
        self.client
    }

    pub fn home(&self) -> &str {
        self.home.as_deref().unwrap_or("")
    }

    /// The value of a non-confidential name, as saved.
    pub fn shown(&self, server: &str, name: &str) -> Option<&str> {
        let key = (server.to_owned(), name.to_owned());
        self.shown.get(&key).map(String::as_str)
    }

    /// Whether `name` of `server` has a non-empty value.
    pub fn has(&self, server: &str, name: &str) -> bool {
        self.set.contains(&(server.to_owned(), name.to_owned()))
    }
}
