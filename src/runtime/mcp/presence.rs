//! What the Connectors area reads when it opens (D20): `config.toml` and
//! which values `secrets.toml` holds. Read only, off the UI thread, and the
//! answer carries names, never a value ([`Presence`]).

use std::path::PathBuf;

use tokio::sync::mpsc;

use crate::{
    app::AppEvent,
    core::ai::{
        mcp_catalog::presence::Presence,
        mcp_secrets::{BACKUP, CLIENT_FILE},
    },
    runtime::mcp_secrets::load_for_start,
};

/// Answers each `Effect::LoadPresence` with one `AppEvent::McpPresence`.
pub struct Reader {
    config: Option<PathBuf>,
    secrets: Option<PathBuf>,
    events: mpsc::Sender<AppEvent>,
}

impl Reader {
    pub fn new(
        config: Option<PathBuf>,
        secrets: Option<PathBuf>,
        events: mpsc::Sender<AppEvent>,
    ) -> Self {
        Self {
            config,
            secrets,
            events,
        }
    }

    /// Reads both files on a task of its own. Without a config folder the
    /// list is empty, as a missing file.
    pub fn load(&self) {
        let (config, secrets, events) = (
            self.config.clone(),
            self.secrets.clone(),
            self.events.clone(),
        );
        tokio::spawn(async move {
            let text = match config {
                Some(path) => tokio::task::spawn_blocking(move || std::fs::read_to_string(path))
                    .await
                    .unwrap_or_else(|_| Err(std::io::Error::other("read failed"))),
                None => Err(std::io::ErrorKind::NotFound.into()),
            };
            let exists = |name: &str| {
                secrets
                    .as_deref()
                    .map(|path| path.with_file_name(name))
                    .is_some_and(|path| std::fs::symlink_metadata(path).is_ok())
            };
            let (backup, client) = (exists(BACKUP), exists(CLIENT_FILE));
            let loaded = load_for_start(secrets).await;
            let home = std::env::var("HOME").ok();
            let presence = Presence::read(text, &loaded)
                .with_backup(backup)
                .with_client_file(client)
                .with_home(home)
                // A `stat`: a token file is never opened, and an empty one is
                // not a link.
                .with_token(|token| std::fs::metadata(token).is_ok_and(|m| m.len() > 0))
                .with_credentials(|file| std::fs::metadata(file).is_ok_and(|m| m.len() > 0));
            let _ = events.send(AppEvent::McpPresence(presence)).await;
        });
    }
}

#[cfg(test)]
mod tests;
