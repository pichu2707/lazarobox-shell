//! How the real-codex test reads the outcome of a turn. Pure string rules, kept
//! apart from the macOS-only test so they run (and are tested) on every system.

/// What the turn's error text says.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The server refused the made-up key: the request crossed the jail.
    Auth,
    /// The jail seems to break codex; holds the markers found.
    Broken(Vec<&'static str>),
    /// An answer of a service outside this repo (geo block, rate limit):
    /// says nothing about the jail. Holds the marker.
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
pub const EXTERNAL: [&str; 5] = [
    "status 403",
    "403 forbidden",
    "status 429",
    "429 too many",
    "unsupported_country",
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

/// What the test does with a finished turn.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The server refused the made-up key: the jail lets codex through.
    Pass,
    /// An outside answer says nothing about the jail: skip. Holds the marker.
    External(&'static str),
    /// The test fails; holds the message.
    Fail(String),
}

/// Judges a turn. Only the error events can pass or skip it: a bogus key must
/// produce an error event, and wording elsewhere in the dump (the agent's own
/// text, say) proves nothing. With no error event the dump is read only to name
/// a broken jail in the failure.
pub fn judge(errors: &str, dump: &str) -> Verdict {
    if errors.is_empty() {
        return match classify(dump) {
            Outcome::Broken(markers) => Verdict::Fail(format!(
                "no error event, and the jail seems to break codex ({markers:?}):\n{dump}"
            )),
            _ => Verdict::Fail(format!(
                "no error event: a made-up key must produce one:\n{dump}"
            )),
        };
    }
    match classify(errors) {
        Outcome::Auth => Verdict::Pass,
        Outcome::External(marker) => Verdict::External(marker),
        Outcome::Broken(markers) => Verdict::Fail(format!(
            "the jail seems to break codex ({markers:?}):\n{dump}"
        )),
        Outcome::Other => Verdict::Fail(format!(
            "expected the server's 401 for the made-up key:\n{dump}"
        )),
    }
}
