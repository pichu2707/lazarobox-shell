//! What the Connectors area reads from disk when it opens (D20): the
//! `[[ai.mcp.servers]]` of `config.toml` and which catalog values have a
//! non-empty entry in `secrets.toml`. It holds names, never a value, so
//! neither the area nor a `{:?}` can draw one.

use std::{
    collections::{HashMap, HashSet},
    io,
};

use super::entries;
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
