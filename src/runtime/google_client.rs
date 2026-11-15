//! The lazarobox Google client at run time (openspec change
//! `gsc-lazarobox-client`): one `curl` GET of the client file, the pin check
//! of `core::ai::mcp_secrets`, the cache written by the secrets writer, and
//! the fall back to the saved copy. Only Link calls it. Every
//! failure has a notice; none holds the secret or the body.

use std::path::Path;

use super::{
    hf::{CLIENT, Curl, REPLY_TOO_LONG},
    secrets_write::{RealFs, google::read_cache, save_cache_with},
};
use crate::core::ai::mcp_secrets::{
    CACHE_FILE, URL_PATH, Why, accept, failure_notice, fallback_notice, not_saved_notice,
};

/// How a refresh ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refresh {
    /// The cache holds the client just fetched.
    Fresh,
    /// The fetch failed and the saved copy is used; the notice says why.
    Stale(String),
    /// Nothing can be linked with: the notice says why.
    Failed(String),
}

/// Fetches the client and caches it next to `secrets`. With no pin, no
/// request is made. When the fetch or its check fails, a cache that passes
/// the pin and the file checks is used and left as it is.
pub async fn refresh(curl: &Curl, pin: Option<&str>, secrets: Option<&Path>) -> Refresh {
    if pin.is_none_or(str::is_empty) {
        return Refresh::Failed(failure_notice(&Why::NotConfigured));
    }
    let answer = match curl.fetch(URL_PATH, CLIENT).await {
        Ok((status, body)) => accept(status, &body, pin),
        Err(line) if line.ends_with(REPLY_TOO_LONG) => Err(Why::TooLarge),
        Err(line) => Err(Why::Curl(line)),
    };
    let secrets = secrets.map(Path::to_path_buf);
    match answer {
        Ok(client) => {
            let saved = tokio::task::spawn_blocking(move || {
                save_cache_with(
                    &RealFs,
                    secrets.as_deref(),
                    &client.id,
                    client.secret.expose(),
                )
            })
            .await;
            match saved {
                Ok(Ok(())) => Refresh::Fresh,
                Ok(Err(refusal)) => {
                    Refresh::Failed(not_saved_notice(&refusal.notice_of(CACHE_FILE)))
                }
                Err(_) => Refresh::Failed(not_saved_notice("the task failed")),
            }
        }
        Err(why) => {
            let cached = tokio::task::spawn_blocking({
                let pin = pin.map(str::to_owned);
                move || read_cache(&RealFs, secrets.as_deref(), pin.as_deref())
            })
            .await;
            match cached {
                Ok(Ok(Some(_))) => Refresh::Stale(fallback_notice(&why)),
                Ok(Ok(None)) => Refresh::Failed(failure_notice(&why)),
                Ok(Err(problem)) => Refresh::Failed(format!(
                    "{}; the saved copy is not used: {problem}",
                    failure_notice(&why)
                )),
                Err(_) => Refresh::Failed(failure_notice(&why)),
            }
        }
    }
}

#[cfg(test)]
mod tests;
