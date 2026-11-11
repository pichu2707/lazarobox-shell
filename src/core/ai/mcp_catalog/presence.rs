//! What the Connectors area reads from disk when it opens (D20): the
//! `[[ai.mcp.servers]]` of `config.toml` and which catalog values have a
//! non-empty entry in `secrets.toml`. It holds names, never a value, so
//! neither the area nor a `{:?}` can draw one.

use std::{collections::HashSet, io};

use super::entries;
use crate::core::{
    ai::{config::McpServer, mcp_secrets::Secrets},
    config::Config,
};

/// The disk copy of the declared servers and the names that have a value.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Presence {
    /// `None` when `config.toml` could not be read or parsed.
    pub servers: Option<Vec<McpServer>>,
    set: HashSet<(String, String)>,
}

impl std::fmt::Debug for Presence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Presence")
            .field("servers", &self.servers)
            .field("set", &self.set.len())
            .finish()
    }
}

impl Presence {
    /// Builds it from the read of `config.toml` (a missing file declares
    /// nothing) and the parsed `secrets.toml`. Only the secret names of the
    /// catalog are looked up.
    pub fn read(config: io::Result<String>, secrets: &Secrets) -> Self {
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
                secrets
                    .value(entry.id, v.name)
                    .is_some_and(|s| !s.is_empty())
            })
            .map(|(entry, v)| (entry.id.to_owned(), v.name.to_owned()))
            .collect();
        Self { servers, set }
    }

    /// Whether `name` of `server` has a non-empty value.
    pub fn has(&self, server: &str, name: &str) -> bool {
        self.set.contains(&(server.to_owned(), name.to_owned()))
    }
}
