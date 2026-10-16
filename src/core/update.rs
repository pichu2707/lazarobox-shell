//! The "update available" notice. Pure: version comparison, the check
//! schedule, the crates.io reply, the cache file and the texts. The request
//! and the files live in `runtime::update`.
//!
//! lazarobox only ever TELLS the user a newer version exists. Nothing here (or
//! anywhere else) installs, downloads or runs an update.

use serde_json::{Value, json};

/// The public crates.io endpoint of this crate.
pub const API_URL: &str = "https://crates.io/api/v1/crates/lazarobox-shell";

/// The command the notice shows. Text only: it is never executed.
pub const INSTALL_COMMAND: &str = "cargo install lazarobox-shell";

/// Between two checks that worked.
pub const CHECK_EVERY_SECS: u64 = 24 * 60 * 60;

/// Between a failed check and the next try: short, so a flaky network does
/// not hide an update for a day, long enough not to hammer the API.
pub const RETRY_AFTER_FAILURE_SECS: u64 = 60 * 60;

/// A release version: `major.minor.patch`, possibly a pre-release.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Version {
    core: (u64, u64, u64),
    /// Sorts below the same core without a pre-release suffix.
    stable: bool,
}

impl Version {
    /// Parses `1.2.3`, `1.2.3-rc.1` or `1.2.3+meta`. Anything else is `None`.
    pub fn parse(text: &str) -> Option<Self> {
        let (without_meta, _) = text.split_once('+').unwrap_or((text, ""));
        let (numbers, pre) = match without_meta.split_once('-') {
            Some((numbers, pre)) if !pre.is_empty() => (numbers, true),
            Some(_) => return None,
            None => (without_meta, false),
        };
        let mut parts = numbers.split('.');
        let mut next = || {
            let part = parts.next()?;
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            part.parse::<u64>().ok()
        };
        let core = (next()?, next()?, next()?);
        if parts.next().is_some() {
            return None;
        }
        Some(Self { core, stable: !pre })
    }

    /// Whether this is a final release (no `-pre` suffix).
    pub fn is_stable(&self) -> bool {
        self.stable
    }

    /// `major.minor.patch`: rebuilt from the numbers, so it is safe to show.
    pub fn core_text(&self) -> String {
        format!("{}.{}.{}", self.core.0, self.core.1, self.core.2)
    }
}

/// The newest stable version when it is newer than `current`.
pub fn newer_than(latest: &str, current: &str) -> Option<Version> {
    let latest = Version::parse(latest).filter(Version::is_stable)?;
    let current = Version::parse(current)?;
    (latest > current).then_some(latest)
}

/// What the last check left behind.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Cache {
    /// Unix seconds of the last check, worked or not.
    pub checked_at: u64,
    /// Whether that check worked.
    pub ok: bool,
    /// The newest stable version seen by a check that worked.
    pub latest: Option<String>,
}

impl Cache {
    /// Reads the cache file text. A missing field or bad JSON is `None`.
    pub fn parse(text: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(text).ok()?;
        Some(Self {
            checked_at: value.get("checked_at")?.as_u64()?,
            ok: value.get("ok")?.as_bool()?,
            latest: value
                .get("latest")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
    }

    pub fn to_json(&self) -> String {
        json!({ "checked_at": self.checked_at, "ok": self.ok, "latest": self.latest }).to_string()
    }

    /// The cache after a check at `now`: a worked check replaces `latest`, a
    /// failed one keeps what was known and only moves the clock.
    pub fn after(&self, now: u64, result: Result<String, ()>) -> Self {
        match result {
            Ok(latest) => Self {
                checked_at: now,
                ok: true,
                latest: Some(latest),
            },
            Err(()) => Self {
                checked_at: now,
                ok: false,
                latest: self.latest.clone(),
            },
        }
    }
}

/// Whether a check must go to the network at `now`, for a binary that runs
/// `current`. A worked check holds for a day, unless what it found is not
/// newer than `current` any more (the user installed it, or went back): then
/// the answer is old news and the next check is due at once. A failed check
/// keeps the short retry either way.
pub fn is_due(cache: Option<&Cache>, now: u64, current: &str) -> bool {
    let Some(cache) = cache else { return true };
    if cache.ok && latest_is_stale(cache, current) {
        return true;
    }
    let wait = if cache.ok {
        CHECK_EVERY_SECS
    } else {
        RETRY_AFTER_FAILURE_SECS
    };
    // A stamp in the future (the clock went back) must not freeze the check.
    now.checked_sub(cache.checked_at)
        .is_none_or(|elapsed| elapsed >= wait)
}

/// Whether the cached `latest` is known and no newer than `current`.
fn latest_is_stale(cache: &Cache, current: &str) -> bool {
    let latest = cache.latest.as_deref().and_then(Version::parse);
    matches!((latest, Version::parse(current)), (Some(latest), Some(current)) if latest <= current)
}

/// `crate.max_stable_version` (else `crate.max_version`) of the crates.io
/// reply, as a stable `major.minor.patch`. Pre-releases are skipped.
pub fn parse_response(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    let krate = value.get("crate")?;
    ["max_stable_version", "max_version"]
        .into_iter()
        .filter_map(|key| krate.get(key)?.as_str())
        .filter_map(Version::parse)
        .find(Version::is_stable)
        .map(|version| version.core_text())
}

/// What to show for `cache` when running `current`: the version to announce.
pub fn announced(cache: Option<&Cache>, current: &str) -> Option<String> {
    let latest = cache?.latest.as_deref()?;
    newer_than(latest, current).map(|version| version.core_text())
}

/// The statusline mark.
pub fn status_mark(latest: &str) -> String {
    format!("update {latest}")
}

/// The menu note, in the two lines the popup has room for. Joined by a space
/// they read `update <ver> available — run: cargo install lazarobox-shell`.
pub fn menu_lines(latest: &str) -> [String; 2] {
    [
        format!("update {latest} available \u{2014}"),
        format!("run: {INSTALL_COMMAND}"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache(checked_at: u64, ok: bool, latest: Option<&str>) -> Cache {
        Cache {
            checked_at,
            ok,
            latest: latest.map(str::to_owned),
        }
    }

    const CURRENT: &str = "0.15.1";

    #[test]
    fn parses_plain_and_prerelease_versions() {
        let v = Version::parse("0.15.1").unwrap();
        assert!(v.is_stable());
        assert_eq!(v.core_text(), "0.15.1");
        assert!(!Version::parse("1.0.0-rc.1").unwrap().is_stable());
        assert!(Version::parse("1.0.0+build5").unwrap().is_stable());
    }

    #[test]
    fn rejects_what_is_not_a_version() {
        for bad in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "a.b.c",
            "1.2.x",
            "-1.2.3",
            "1.2.3-",
            " 1.2.3",
            "1..3",
            "1.2.3\u{1b}[31m",
            "99999999999999999999.0.0",
        ] {
            assert_eq!(Version::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn compares_numerically_not_as_text() {
        assert!(newer_than("0.15.10", "0.15.9").is_some());
        assert!(newer_than("0.16.0", "0.15.99").is_some());
        assert!(newer_than("1.0.0", "0.99.99").is_some());
        assert!(newer_than("0.15.1", "0.15.1").is_none());
        assert!(newer_than("0.15.0", "0.15.1").is_none());
    }

    #[test]
    fn a_prerelease_latest_is_never_announced() {
        assert!(newer_than("0.16.0-rc.1", "0.15.1").is_none());
    }

    #[test]
    fn a_stable_release_is_newer_than_its_own_prerelease() {
        assert!(newer_than("0.16.0", "0.16.0-rc.1").is_some());
        assert!(newer_than("0.16.0", "0.16.0").is_none());
    }

    #[test]
    fn an_unparseable_version_announces_nothing() {
        assert!(newer_than("soon", "0.15.1").is_none());
        assert!(newer_than("0.16.0", "dev").is_none());
    }

    #[test]
    fn the_announced_text_is_rebuilt_from_the_numbers() {
        assert_eq!(
            newer_than("0.16.0+x", "0.15.1").unwrap().core_text(),
            "0.16.0"
        );
    }

    #[test]
    fn parses_the_stable_version_from_the_reply() {
        let body = r#"{"crate":{"max_stable_version":"0.16.0","max_version":"0.17.0-rc.1"}}"#;
        assert_eq!(parse_response(body).as_deref(), Some("0.16.0"));
    }

    #[test]
    fn falls_back_to_max_version_when_it_is_stable() {
        let body = r#"{"crate":{"max_version":"0.16.0"}}"#;
        assert_eq!(parse_response(body).as_deref(), Some("0.16.0"));
        let null = r#"{"crate":{"max_stable_version":null,"max_version":"0.16.0"}}"#;
        assert_eq!(parse_response(null).as_deref(), Some("0.16.0"));
    }

    #[test]
    fn skips_a_prerelease_max_version() {
        let body = r#"{"crate":{"max_version":"0.17.0-rc.1"}}"#;
        assert_eq!(parse_response(body), None);
    }

    #[test]
    fn bad_replies_give_nothing() {
        for body in [
            "",
            "not json",
            "[]",
            "{}",
            r#"{"crate":{}}"#,
            r#"{"crate":{"max_stable_version":42}}"#,
            r#"{"crate":{"max_stable_version":"soon"}}"#,
            r#"{"errors":[{"detail":"nope"}]}"#,
        ] {
            assert_eq!(parse_response(body), None, "{body:?}");
        }
    }

    #[test]
    fn without_a_cache_a_check_is_due() {
        assert!(is_due(None, 1_000, CURRENT));
    }

    #[test]
    fn a_worked_check_holds_for_24_hours() {
        let c = cache(1_000, true, Some("0.16.0"));
        assert!(!is_due(Some(&c), 1_000, CURRENT));
        assert!(!is_due(Some(&c), 1_000 + CHECK_EVERY_SECS - 1, CURRENT));
        assert!(is_due(Some(&c), 1_000 + CHECK_EVERY_SECS, CURRENT));
    }

    #[test]
    fn a_cached_latest_that_is_not_newer_is_stale_at_once() {
        // Equal: the user just installed the announced version. Older: a
        // downgrade or a yanked release. Either way the 24 h wait is moot.
        for latest in ["0.15.1", "0.15.0", "0.14.9"] {
            let c = cache(1_000, true, Some(latest));
            assert!(is_due(Some(&c), 1_000, CURRENT), "{latest}");
        }
    }

    #[test]
    fn a_cache_without_a_usable_latest_keeps_the_daily_rule() {
        for latest in [None, Some("soon")] {
            let c = cache(1_000, true, latest);
            assert!(!is_due(Some(&c), 1_001, CURRENT), "{latest:?}");
        }
    }

    #[test]
    fn a_newer_cached_latest_keeps_the_daily_rule() {
        let c = cache(1_000, true, Some("0.15.2"));
        assert!(!is_due(Some(&c), 1_000 + CHECK_EVERY_SECS - 1, CURRENT));
        assert!(is_due(Some(&c), 1_000 + CHECK_EVERY_SECS, CURRENT));
    }

    #[test]
    fn a_failed_check_keeps_the_hourly_retry_even_with_a_stale_latest() {
        let c = cache(1_000, false, Some("0.15.1"));
        assert!(!is_due(
            Some(&c),
            1_000 + RETRY_AFTER_FAILURE_SECS - 1,
            CURRENT
        ));
        assert!(is_due(Some(&c), 1_000 + RETRY_AFTER_FAILURE_SECS, CURRENT));
    }

    #[test]
    fn a_failed_check_retries_after_an_hour() {
        let c = cache(1_000, false, None);
        assert!(!is_due(
            Some(&c),
            1_000 + RETRY_AFTER_FAILURE_SECS - 1,
            CURRENT
        ));
        assert!(is_due(Some(&c), 1_000 + RETRY_AFTER_FAILURE_SECS, CURRENT));
    }

    #[test]
    fn a_clock_that_went_back_does_not_freeze_the_check() {
        let c = cache(10 * CHECK_EVERY_SECS, true, None);
        assert!(is_due(Some(&c), 1_000, CURRENT));
    }

    #[test]
    fn a_worked_check_replaces_the_latest_version() {
        let old = cache(1, true, Some("0.15.2"));
        let new = old.after(500, Ok("0.16.0".into()));
        assert_eq!(new, cache(500, true, Some("0.16.0")));
    }

    #[test]
    fn a_failed_check_keeps_the_known_version_and_moves_the_clock() {
        let old = cache(1, true, Some("0.16.0"));
        let new = old.after(500, Err(()));
        assert_eq!(new, cache(500, false, Some("0.16.0")));
    }

    #[test]
    fn the_cache_round_trips() {
        let c = cache(1_700_000_000, true, Some("0.16.0"));
        assert_eq!(Cache::parse(&c.to_json()), Some(c));
        let none = cache(5, false, None);
        assert_eq!(Cache::parse(&none.to_json()), Some(none));
    }

    #[test]
    fn a_damaged_cache_reads_as_missing() {
        for text in [
            "",
            "x",
            "{}",
            r#"{"checked_at":"a","ok":true}"#,
            r#"{"ok":true}"#,
        ] {
            assert_eq!(Cache::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn a_cache_without_latest_is_valid() {
        let c = Cache::parse(r#"{"checked_at":7,"ok":true}"#).unwrap();
        assert_eq!(c, cache(7, true, None));
    }

    #[test]
    fn announces_a_cached_newer_version_only() {
        let c = cache(1, true, Some("0.16.0"));
        assert_eq!(announced(Some(&c), "0.15.1").as_deref(), Some("0.16.0"));
        assert_eq!(announced(Some(&c), "0.16.0"), None);
        assert_eq!(announced(Some(&c), "0.17.0"), None);
        assert_eq!(announced(None, "0.15.1"), None);
        assert_eq!(announced(Some(&cache(1, true, None)), "0.15.1"), None);
    }

    #[test]
    fn a_tampered_cache_cannot_inject_text() {
        let c = cache(1, true, Some("0.16.0\u{1b}[2J"));
        assert_eq!(announced(Some(&c), "0.15.1"), None);
    }

    #[test]
    fn the_texts_are_fixed() {
        assert_eq!(status_mark("0.16.0"), "update 0.16.0");
        assert_eq!(
            menu_lines("0.16.0").join(" "),
            "update 0.16.0 available \u{2014} run: cargo install lazarobox-shell"
        );
    }
}
