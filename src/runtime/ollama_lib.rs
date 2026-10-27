//! The requests of the Ollama download screens (ollama-download): the
//! library page on ollama.com, then a manifest and the license layers of a
//! model on the registry. HTTPS GETs through the system curl, as the Hub's.

use crate::core::ai::ollama_lib::{self, Card, Manifest};

use super::hf::{Curl, Fetch, REPLY_CAP};

/// The library page: HTML, 4 MiB, no redirects.
const PAGE: Fetch = Fetch {
    accept: "text/html",
    cap: REPLY_CAP,
    cut: false,
    follow: false,
};

/// A manifest: the registry's v2 JSON, 4 MiB, no redirects.
const MANIFEST: Fetch = Fetch {
    accept: "application/vnd.docker.distribution.manifest.v2+json",
    cap: REPLY_CAP,
    cut: false,
    follow: false,
};

/// A license blob: the registry sends it on to its storage, and only the
/// start is needed for its title.
const LICENSE: Fetch = Fetch {
    accept: "text/plain",
    cap: ollama_lib::LICENSE_CAP,
    cut: true,
    follow: true,
};

/// The production curls: the library site and the registry.
pub fn curls(path: std::ffi::OsString) -> (Curl, Curl) {
    (
        Curl::ollama(path.clone(), ollama_lib::LIBRARY),
        Curl::ollama(path, ollama_lib::REGISTRY),
    )
}

/// The cards of the library page; none when the page holds none.
pub async fn library(site: &Curl) -> Result<Vec<Card>, String> {
    match site.fetch(ollama_lib::library_path(), PAGE).await? {
        (200, body) => Ok(ollama_lib::parse_library(&body)),
        (code, _) => Err(site.failed(format!("HTTP {code}"))),
    }
}

/// The manifest of `model:tag`. A name that is not safe is never sent.
pub async fn manifest(registry: &Curl, model: &str, tag: &str) -> Result<Manifest, String> {
    if !ollama_lib::is_name(model) || !ollama_lib::is_name(tag) {
        return Err(registry.failed("not a model name"));
    }
    match registry
        .fetch(&ollama_lib::manifest_path(model, tag), MANIFEST)
        .await?
    {
        (200, body) => ollama_lib::parse_manifest(&body),
        (code, _) => Err(registry.failed(format!("HTTP {code}"))),
    }
}

/// The title of the license blob `digest` of `model`.
pub async fn license(registry: &Curl, model: &str, digest: &str) -> Result<String, String> {
    if !ollama_lib::is_name(model) {
        return Err(registry.failed("not a model name"));
    }
    match registry
        .fetch(&ollama_lib::blob_path(model, digest), LICENSE)
        .await?
    {
        (200, body) => Ok(ollama_lib::license_title(&body)),
        (code, _) => Err(registry.failed(format!("HTTP {code}"))),
    }
}

#[cfg(test)]
mod tests;
