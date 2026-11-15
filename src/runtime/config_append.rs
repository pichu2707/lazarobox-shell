//! Connect Google's write (openspec change `google-connect`, Approach 3): the
//! values saved in `secrets.toml`, then the catalog entries appended to
//! `config.toml`. The append takes the secrets writer's safety model, not
//! `save_config`'s: folder and file judged on their handles, the size bound,
//! the bytes compared with the ones the plan was shown with, `config.toml.bak`
//! first, the file's mode kept, `O_EXCL` temp file and atomic rename. Reached
//! only from `Effect::ConnectGoogle` through the secrets writer's turn.

use std::path::Path;

use super::secrets_write::{SaveFs, Target, replace_with, save_all_with};
use crate::core::{
    ai::mcp_secrets::{BACKUP as SECRETS_BACKUP, Refusal, line_of},
    config_append::{AppendRefusal, BACKUP, Connect, Connected, FILE, MAX_BYTES, appended, digest},
};

const CONFIG: Target = Target {
    file: FILE,
    backup: Some(BACKUP),
    max: MAX_BYTES,
    keep_mode: true,
};

/// Appends `snippets`, declaring `names`, to the `config.toml` at `config`,
/// when it still holds the bytes of digest `expected` (`None`: it did not
/// exist). `true` when a backup was made.
pub fn append_with(
    fs: &impl SaveFs,
    config: Option<&Path>,
    expected: Option<&[u8; 32]>,
    snippets: &str,
    names: &[&str],
) -> Result<bool, AppendRefusal> {
    replace_with(fs, config, CONFIG, |current| {
        if current.map(digest).as_ref() != expected {
            return Err(Refusal::Changed.into());
        }
        let text = match current {
            Some(bytes) => std::str::from_utf8(bytes)
                .map_err(|e| AppendRefusal::NotToml(line_of(bytes, e.valid_up_to())))?,
            None => "",
        };
        appended(text, snippets, names)
    })
}

/// Saves the values of `write` in the `secrets.toml` at `secrets`, then
/// appends its entries to the `config.toml` next to it.
pub fn connect_with(fs: &impl SaveFs, secrets: Option<&Path>, write: &Connect) -> Connected {
    let values: Vec<(&str, &str, &str)> = write
        .set
        .iter()
        .map(|(table, name, value)| (*table, *name, value.expose()))
        .collect();
    let saved = if values.is_empty() {
        None
    } else {
        match save_all_with(fs, secrets, &values) {
            Ok(saved) => Some(saved),
            Err(refusal) => {
                return Connected::Refused(format!(
                    "connect: nothing was written: {}",
                    refusal.notice()
                ));
            }
        }
    };
    let entries = write.names.join(", ");
    let config = secrets.map(|path| path.with_file_name(FILE));
    let appended = if write.names.is_empty() {
        Ok(false)
    } else {
        append_with(
            fs,
            config.as_deref(),
            write.expected.as_ref(),
            &write.snippets,
            &write.names,
        )
    };
    let values = saved.as_ref().map(|saved| match values.len() {
        1 => format!("saved {} in secrets.toml", saved.name),
        n => format!("saved {n} values in secrets.toml"),
    });
    match (appended, values) {
        (Ok(backed_up), values) => {
            let mut done: Vec<String> = values.into_iter().collect();
            if !entries.is_empty() {
                done.push(format!("added {entries} to {FILE}"));
            }
            let mut notice = format!("connect: {}", done.join(" and "));
            if done.is_empty() {
                notice.push_str("nothing to write");
            }
            let backups: Vec<&str> = [
                saved
                    .as_ref()
                    .is_some_and(|s| s.backed_up)
                    .then_some(SECRETS_BACKUP),
                backed_up.then_some(BACKUP),
            ]
            .into_iter()
            .flatten()
            .collect();
            if !backups.is_empty() {
                notice.push_str(&format!(
                    "; the previous files stay in {}",
                    backups.join(" and ")
                ));
            }
            Connected::Done(notice)
        }
        (Err(refusal), None) => Connected::Refused(format!(
            "connect: nothing was written: {}; paste the entries by hand",
            refusal.notice()
        )),
        (Err(refusal), Some(values)) => Connected::ValuesOnly(format!(
            "connect: {values}, but {entries} {} not added: {}; paste the entries by hand",
            if write.names.len() == 1 {
                "was"
            } else {
                "were"
            },
            refusal.notice()
        )),
    }
}

#[cfg(test)]
mod tests;
