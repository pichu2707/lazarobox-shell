//! How the real-codex test reads the outcome of a turn. Pure string rules, kept
//! apart from the macOS-only test so they run (and are tested) on every system.

/// What the turn's error text says.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The server refused the made-up key: the request crossed the jail.
    Auth,
    /// The jail seems to break codex; holds the markers found.
    Broken(Vec<&'static str>),
    /// An answer of a service outside this repo (geo block, rate limit, a
    /// Cloudflare challenge): says nothing about the jail. Holds the marker.
    External(&'static str),
    /// Anything else.
    Other,
}

/// The server's answer to a bad key. Only server-side wording: codex's own
/// text about logging in says nothing about the network.
pub const AUTH: [&str; 2] = ["401 unauthorized", "incorrect api key"];

/// What a broken jail looks like. Exact failures only: a word such as `tls` or
/// `dns` also shows up in healthy output.
pub const BROKEN: [&str; 9] = [
    "unknownissuer",
    "invalid peer certificate",
    "operation not permitted",
    "sandbox_apply",
    "could not be applied",
    "connection refused",
    "network is unreachable",
    "dns error",
    "failed to lookup address",
];

/// Answers of services outside the jail that make the run inconclusive.
pub const EXTERNAL: [&str; 7] = [
    "status 403",
    "403 forbidden",
    "status 429",
    "429 too many",
    "unsupported_country",
    "cloudflare",
    "just a moment",
];

/// Classifies an error text. A broken marker wins, so a 401 that comes with a
/// certificate failure is still a failure.
pub fn classify(text: &str) -> Outcome {
    let lower = text.to_lowercase();
    let broken: Vec<_> = BROKEN.into_iter().filter(|p| lower.contains(p)).collect();
    if !broken.is_empty() {
        return Outcome::Broken(broken);
    }
    if AUTH.iter().any(|p| lower.contains(p)) {
        return Outcome::Auth;
    }
    match EXTERNAL.into_iter().find(|p| lower.contains(p)) {
        Some(marker) => Outcome::External(marker),
        None => Outcome::Other,
    }
}
