//! The GitHub star prompt (goal #744). After some days of use the chat asks
//! once whether the user wants to star the repository; `g` stars it (or opens
//! its page). A local mark remembers the answer: a star, or an opened page, is
//! final, and a dismissal is asked again only by a newer version and after
//! [`ASK_AGAIN_SECS`]. Pure: the runtime reads and writes the mark.

use serde_json::{Value, json};

use crate::core::update::Version;

/// The repository, as `gh api user/starred/<owner>/<repo>` takes it.
pub const REPO: &str = "pichu2707/lazarobox-shell";

/// The repository page, opened when `gh` cannot star it.
pub const REPO_URL: &str = "https://github.com/pichu2707/lazarobox-shell";

/// Distinct local days with a completed chat answer before the first ask.
pub const MIN_DAYS: u64 = 5;

/// The least time between a dismissal and the next ask: 60 days.
pub const ASK_AGAIN_SECS: u64 = 60 * 24 * 60 * 60;

/// What the user answered last.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Mark {
    /// Starred, or the repository page opened: never asked again.
    Done,
    /// Dismissed at `at` (Unix seconds) while running `version`.
    Dismissed { version: String, at: u64 },
}

impl Mark {
    /// Reads the mark file text. Bad JSON, an unknown state or a dismissal
    /// without a valid version and time is `None`, as if never asked.
    pub fn parse(text: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(text).ok()?;
        match value.get("state")?.as_str()? {
            "done" => Some(Self::Done),
            "dismissed" => {
                let version = value.get("version")?.as_str()?;
                Version::parse(version)?;
                Some(Self::Dismissed {
                    version: version.to_owned(),
                    at: value.get("at")?.as_u64()?,
                })
            }
            _ => None,
        }
    }

    pub fn to_json(&self) -> String {
        match self {
            Self::Done => json!({ "state": "done" }),
            Self::Dismissed { version, at } => {
                json!({ "state": "dismissed", "version": version, "at": at })
            }
        }
        .to_string()
    }
}

/// What the start-up check found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Due {
    /// Show the prompt; `gh` says whether `g` stars with `gh` or opens the page.
    Ask { gh: bool },
    /// Nothing to show this run.
    No,
}

/// The outcome of starring with `gh`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum StarReply {
    /// GitHub took the star.
    Starred,
    /// `gh` is missing or not logged in.
    NoGh,
    /// Anything else, with `gh`'s message.
    Failed(String),
}

/// Whether the mark lets the prompt show to lazarobox `current` at `now`:
/// never asked, or dismissed by an older version at least
/// [`ASK_AGAIN_SECS`] ago.
pub fn may_ask(mark: Option<&Mark>, current: &str, now: u64) -> bool {
    match mark {
        None => true,
        Some(Mark::Done) => false,
        Some(Mark::Dismissed { version, at }) => {
            let newer = match (Version::parse(current), Version::parse(version)) {
                (Some(current), Some(then)) => current > then,
                _ => false,
            };
            newer
                && now
                    .checked_sub(*at)
                    .is_some_and(|gone| gone >= ASK_AGAIN_SECS)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 24 * 60 * 60;

    fn dismissed(version: &str, at: u64) -> Mark {
        Mark::Dismissed {
            version: version.to_owned(),
            at,
        }
    }

    #[test]
    fn a_mark_round_trips_through_its_json() {
        for mark in [Mark::Done, dismissed("1.26.0", 1_700_000_000)] {
            assert_eq!(Mark::parse(&mark.to_json()), Some(mark.clone()), "{mark:?}");
        }
        assert_eq!(Mark::parse(r#"{"state":"done"}"#), Some(Mark::Done));
    }

    #[test]
    fn a_bad_mark_reads_as_never_asked() {
        for text in [
            "",
            "not json",
            "[]",
            r#"{"state":"maybe"}"#,
            r#"{"state":"dismissed","at":5}"#,
            r#"{"state":"dismissed","version":"1.26.0"}"#,
            r#"{"state":"dismissed","version":"one","at":5}"#,
            r#"{"state":"dismissed","version":"1.26.0","at":-5}"#,
        ] {
            assert_eq!(Mark::parse(text), None, "{text}");
        }
    }

    #[test]
    fn never_asked_may_ask() {
        assert!(may_ask(None, "1.26.0", 0));
    }

    #[test]
    fn a_star_or_an_opened_page_is_final() {
        assert!(!may_ask(Some(&Mark::Done), "9.0.0", u64::MAX));
    }

    #[test]
    fn a_dismissal_asks_again_only_on_a_newer_version_after_60_days() {
        let at = 1_700_000_000;
        let mark = dismissed("1.26.0", at);
        // A newer version, 60 days on: asks.
        assert!(may_ask(Some(&mark), "1.27.0", at + 60 * DAY));
        assert!(may_ask(Some(&mark), "1.26.1", at + 90 * DAY));
        // A newer version too soon.
        assert!(!may_ask(Some(&mark), "1.27.0", at + 60 * DAY - 1));
        // The same or an older version, however late.
        assert!(!may_ask(Some(&mark), "1.26.0", at + 400 * DAY));
        assert!(!may_ask(Some(&mark), "1.25.0", at + 400 * DAY));
        // A clock set back before the dismissal.
        assert!(!may_ask(Some(&mark), "1.27.0", at - DAY));
        // A running version that does not parse never asks again.
        assert!(!may_ask(Some(&mark), "dev", at + 400 * DAY));
    }
}
