//! The "update available" notice. Pure: version comparison, the check
//! schedule, the crates.io reply, the cache file and the texts. The request
//! and the files live in `runtime::update`.
//!
//! lazarobox TELLS the user a newer version exists. It installs one only when
//! the user asks (Enter on the Updates row) and only for a binary that
//! `cargo install` put in `$CARGO_HOME/bin`: [`plan`] decides, the runtime
//! executes. Any other install gets the command to copy, never a run.

use std::{
    ffi::OsString,
    path::{Component, Path, PathBuf},
};

use serde_json::{Value, json};

/// The public crates.io endpoint of this crate.
pub const API_URL: &str = "https://crates.io/api/v1/crates/lazarobox-shell";

/// The command the notice shows and the update pane runs. `--locked` builds
/// with the dependency versions the release was tested with.
pub const INSTALL_COMMAND: &str = "cargo install lazarobox-shell --locked";

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
    /// The lazarobox version that wrote this file. A cache from another
    /// version (or from one that did not record it) says nothing about what
    /// the running version has already seen.
    pub checked_by: Option<String>,
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
            checked_by: value
                .get("checked_by")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
    }

    pub fn to_json(&self) -> String {
        json!({
            "checked_at": self.checked_at,
            "ok": self.ok,
            "latest": self.latest,
            "checked_by": self.checked_by,
        })
        .to_string()
    }

    /// The cache after a check at `now` by lazarobox `by`: a worked check
    /// replaces `latest`, a failed one keeps what was known and only moves the
    /// clock.
    pub fn after(&self, now: u64, result: Result<String, ()>, by: &str) -> Self {
        let checked_by = Some(by.to_owned());
        match result {
            Ok(latest) => Self {
                checked_at: now,
                ok: true,
                latest: Some(latest),
                checked_by,
            },
            Err(()) => Self {
                checked_at: now,
                ok: false,
                latest: self.latest.clone(),
                checked_by,
            },
        }
    }
}

/// Whether a check must go to the network at `now`, for a binary that runs
/// `current`. A worked check holds for a day, unless what it found is not
/// newer than `current` any more (the user installed it, or went back): then
/// the answer is old news and the next check is due at once, but only when
/// another lazarobox wrote it (an upgrade: this version has not looked yet).
/// The same version that found nothing newer waits its day, so an up-to-date
/// user is not asked on every start. A failed check keeps the short retry.
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

/// Whether the cache was written by another lazarobox than `current` and its
/// `latest` is known and no newer than `current`.
fn latest_is_stale(cache: &Cache, current: &str) -> bool {
    if cache.checked_by.as_deref() == Some(current) {
        return false;
    }
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

/// What Enter does on the Updates row.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum UpdateAction {
    /// No newer version is known: nothing to do.
    UpToDate,
    /// Not a cargo install (or no cargo to run): show the command to copy.
    ShowCommand,
    /// Run `program install lazarobox-shell --locked` in a new pane.
    CargoInstall {
        /// The trusted `cargo`, as an absolute path.
        program: PathBuf,
        /// The install root: the parent of the cargo bin dir.
        root: PathBuf,
        /// The version being installed, for the final line.
        version: String,
    },
}

/// `<cargo home>/bin`: `$CARGO_HOME` when it is absolute, else `~/.cargo`.
pub fn cargo_bin_dir(cargo_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    let absolute = |p: Option<&Path>| p.filter(|p| p.is_absolute()).map(Path::to_path_buf);
    let base = absolute(cargo_home).or_else(|| absolute(home).map(|h| h.join(".cargo")))?;
    Some(base.join("bin"))
}

/// The arguments of `cargo` for [`INSTALL_COMMAND`], plus `--root`: cargo
/// then installs over the running binary even when `CARGO_INSTALL_ROOT` or
/// `install.root` point somewhere else.
pub fn install_args(root: &str) -> Vec<String> {
    ["install", "lazarobox-shell", "--locked", "--root", root]
        .map(str::to_owned)
        .to_vec()
}

/// Whether `exe` is a file under the cargo bin dir `bin`.
fn under(exe: &Path, bin: &Path) -> bool {
    // `starts_with` is textual: a `..` could climb out of the dir.
    exe.is_absolute()
        && exe.starts_with(bin)
        && !exe.components().any(|c| c == Component::ParentDir)
}

/// Why Enter only shows the command, for the footer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fallback {
    /// The running binary is not under the cargo bin dir (or is unknown).
    NotCargoInstall,
    /// It is, but there is no `cargo` to run.
    NoCargo,
    /// It is, and a `cargo` was found but is not trusted.
    UntrustedCargo,
    /// It is, and a `cargo` is trusted, but the install folder's name is not
    /// text, so it cannot be given to cargo.
    UnusablePath,
}

impl Fallback {
    /// The footer on the Updates row: the action first, then the reason. One
    /// line of 48 characters at most, so it fits the popup (51 columns).
    pub fn footer(self) -> &'static str {
        match self {
            Fallback::NotCargoInstall => "Enter or y copies it \u{b7} installed another way",
            Fallback::NoCargo => "Enter or y copies it \u{b7} Rust is not installed",
            Fallback::UntrustedCargo => "Enter or y copies it \u{b7} Rust folder is too open",
            Fallback::UnusablePath => "Enter or y copies it \u{b7} folder name is unusual",
        }
    }
}

/// Decides what Enter does. Pure: the caller gives the running binary
/// (`exe`), the cargo bin dir (both canonical, see [`cargo_bin_dir`]) and the
/// `cargo` it found and trusts. The install is only run for a binary under
/// that dir: it is the one `cargo install` replaces. A distro package, a
/// release tarball or a build tree must not be overwritten behind the package
/// manager's back. The install root is the parent of the bin dir.
pub fn plan(
    latest: Option<&str>,
    exe: Option<&Path>,
    bin_dir: Option<&Path>,
    cargo: Option<&Path>,
) -> UpdateAction {
    let Some(latest) = latest else {
        return UpdateAction::UpToDate;
    };
    let root = match (exe, bin_dir) {
        (Some(exe), Some(bin)) if under(exe, bin) => bin.parent(),
        _ => None,
    };
    // The root goes to cargo as text; one that is not UTF-8 is not run.
    match (cargo, root.filter(|r| r.to_str().is_some())) {
        (Some(program), Some(root)) => UpdateAction::CargoInstall {
            program: program.to_path_buf(),
            root: root.to_path_buf(),
            version: latest.to_owned(),
        },
        _ => UpdateAction::ShowCommand,
    }
}

/// What the machine says about how lazarobox got here, read once at start-up
/// by the runtime. The paths are canonical when they exist.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct InstallEnv {
    /// The running binary.
    pub exe: Option<PathBuf>,
    /// The cargo bin dir (`$CARGO_HOME/bin`, else `~/.cargo/bin`).
    pub bin_dir: Option<PathBuf>,
    /// The `cargo` to run, trusted, as found (the rustup proxy dispatches on
    /// the name it is started under, so it is not resolved).
    pub cargo: Option<PathBuf>,
    /// A `cargo` was found but is not trusted (so `cargo` is `None`).
    pub cargo_refused: bool,
    /// The login `PATH`, given to cargo so it finds `rustc` and the linker.
    pub path: OsString,
}

impl InstallEnv {
    /// Why [`plan`] gives [`UpdateAction::ShowCommand`] for this machine.
    pub fn fallback(&self) -> Fallback {
        let cargo_install = match (&self.exe, &self.bin_dir) {
            (Some(exe), Some(bin)) => under(exe, bin),
            _ => false,
        };
        match (cargo_install, &self.cargo) {
            (false, _) => Fallback::NotCargoInstall,
            // A trusted cargo under the cargo dir only fails to plan when the
            // install root is not text (see `plan`).
            (true, Some(_)) => Fallback::UnusablePath,
            (true, None) if self.cargo_refused => Fallback::UntrustedCargo,
            (true, None) => Fallback::NoCargo,
        }
    }

    /// [`plan`] for this machine.
    pub fn plan(&self, latest: Option<&str>) -> UpdateAction {
        plan(
            latest,
            self.exe.as_deref(),
            self.bin_dir.as_deref(),
            self.cargo.as_deref(),
        )
    }
}

/// The first line of the update pane, in plain words: what started, that it
/// takes a while, and that lazarobox must stay open.
pub fn started_text(version: &str) -> String {
    format!(
        "Updating lazarobox to {version}. It compiles on your machine and can take a few minutes. Keep lazarobox open until it says it is done."
    )
}

/// The final line of a successful install.
pub fn installed_text(version: &str) -> String {
    format!("Updated to {version}. Restart lazarobox to use it.")
}

/// The Updates row value once an install worked, until the restart.
pub fn installed_value(version: &str) -> String {
    format!("{version} installed: restart")
}

/// A failure line: what happened, then the command to run by hand.
fn failure(what: &str) -> String {
    format!("The update {what}. To update, run in a terminal: {INSTALL_COMMAND}")
}

/// The final line when cargo could not even be started.
pub fn not_started_text() -> String {
    failure("could not start")
}

/// The final line when the child ended without an exit status.
pub fn no_status_text() -> String {
    failure("stopped unexpectedly")
}

/// The first line of the pane: what is about to run, written by lazarobox.
/// Control characters in a path are shown as `?`.
pub fn running_text(program: &Path, args: &[String]) -> String {
    let clean = |text: String| -> String {
        text.chars()
            .map(|c| if c.is_control() { '?' } else { c })
            .collect()
    };
    clean(format!("running {} {}", program.display(), args.join(" ")))
}

/// The final line of a failed install.
pub fn failed_text(code: u32) -> String {
    failure(&format!("did not finish (error {code})"))
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

/// The menu note, in the two lines the popup has room for.
pub fn menu_lines(latest: &str) -> [String; 2] {
    [
        format!("lazarobox {latest} is available."),
        "Go to Updates and press Enter.".to_owned(),
    ]
}

/// The menu note once Enter could not start the install: it cannot update
/// itself here, and the command to copy. Without Rust the command alone will
/// not work, so the first line says to install it first.
pub fn fallback_lines(why: Fallback) -> [String; 2] {
    let first = match why {
        Fallback::NoCargo => "Install Rust first (rustup.rs), then run:",
        _ => "lazarobox cannot update itself here. Run:",
    };
    [first.to_owned(), INSTALL_COMMAND.to_owned()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache(checked_at: u64, ok: bool, latest: Option<&str>) -> Cache {
        Cache {
            checked_at,
            ok,
            latest: latest.map(str::to_owned),
            checked_by: Some(CURRENT.to_owned()),
        }
    }

    impl Cache {
        fn with_clock(self, checked_at: u64) -> Self {
            Self { checked_at, ..self }
        }
    }

    /// A cache written by another lazarobox (or an old one with no field).
    fn cache_by(by: Option<&str>, latest: Option<&str>) -> Cache {
        Cache {
            checked_by: by.map(str::to_owned),
            ..cache(1_000, true, latest)
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
    fn an_up_to_date_cache_of_this_version_waits_a_day() {
        // This version looked and found nothing newer: once a day, not on
        // every start.
        for latest in ["0.15.1", "0.15.0"] {
            let c = cache_by(Some(CURRENT), Some(latest));
            assert!(!is_due(Some(&c), 1_001, CURRENT), "{latest}");
            assert!(!is_due(Some(&c), 1_000 + CHECK_EVERY_SECS - 1, CURRENT));
            assert!(is_due(Some(&c), 1_000 + CHECK_EVERY_SECS, CURRENT));
        }
    }

    #[test]
    fn after_an_upgrade_a_cache_that_is_not_newer_is_due_at_once() {
        // Another version wrote it, and the version it found is the one now
        // running (or older): the answer is old news.
        for latest in ["0.15.1", "0.15.0", "0.14.9"] {
            let c = cache_by(Some("0.15.0"), Some(latest));
            assert!(is_due(Some(&c), 1_000, CURRENT), "{latest}");
        }
    }

    #[test]
    fn an_old_cache_without_the_field_is_due_at_once() {
        let c = cache_by(None, Some("0.15.1"));
        assert!(is_due(Some(&c), 1_000, CURRENT));
    }

    #[test]
    fn another_versions_cache_with_a_newer_latest_keeps_the_daily_rule() {
        for by in [None, Some("0.15.0")] {
            let c = cache_by(by, Some("0.15.2"));
            assert!(!is_due(Some(&c), 1_000 + CHECK_EVERY_SECS - 1, CURRENT));
            assert!(is_due(Some(&c), 1_000 + CHECK_EVERY_SECS, CURRENT));
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
        let c = Cache {
            ok: false,
            ..cache_by(Some("0.15.0"), Some("0.15.1"))
        };
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
        let new = old.after(500, Ok("0.16.0".into()), CURRENT);
        assert_eq!(new, cache(500, true, Some("0.16.0")));
    }

    #[test]
    fn a_failed_check_keeps_the_known_version_and_moves_the_clock() {
        let old = cache(1, true, Some("0.16.0"));
        let new = old.after(500, Err(()), CURRENT);
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
        assert_eq!(c, cache_by(None, None).with_clock(7));
    }

    #[test]
    fn the_writer_version_round_trips_and_an_old_file_has_none() {
        let c = Cache::parse(r#"{"checked_at":7,"ok":true,"checked_by":"0.15.3"}"#).unwrap();
        assert_eq!(c.checked_by.as_deref(), Some("0.15.3"));
        assert!(c.to_json().contains(r#""checked_by":"0.15.3""#));
        let old = Cache::parse(r#"{"checked_at":7,"ok":true,"latest":"0.16.0"}"#).unwrap();
        assert_eq!(old.checked_by, None);
    }

    #[test]
    fn every_check_stamps_the_version_that_made_it() {
        let old = cache_by(Some("0.15.0"), Some("0.15.0"));
        let ok = old.after(9, Ok("0.15.2".into()), "0.15.3");
        assert_eq!(ok.checked_by.as_deref(), Some("0.15.3"));
        let failed = old.after(9, Err(()), "0.15.3");
        assert_eq!(failed.checked_by.as_deref(), Some("0.15.3"));
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

    fn home() -> Option<&'static Path> {
        Some(Path::new("/home/ana"))
    }

    fn bin() -> Option<&'static Path> {
        Some(Path::new("/home/ana/.cargo/bin"))
    }

    fn cargo_path() -> Option<&'static Path> {
        Some(Path::new("/home/ana/.cargo/bin/cargo"))
    }

    #[test]
    fn the_cargo_bin_dir_defaults_to_the_home_cargo() {
        assert_eq!(
            cargo_bin_dir(None, home()),
            Some(PathBuf::from("/home/ana/.cargo/bin"))
        );
        assert_eq!(
            cargo_bin_dir(Some(Path::new("/opt/cargo")), home()),
            Some(PathBuf::from("/opt/cargo/bin"))
        );
    }

    #[test]
    fn a_relative_cargo_home_or_home_is_ignored() {
        assert_eq!(
            cargo_bin_dir(Some(Path::new("cargo")), home()),
            Some(PathBuf::from("/home/ana/.cargo/bin")),
            "a relative CARGO_HOME falls back to ~/.cargo"
        );
        assert_eq!(cargo_bin_dir(None, Some(Path::new("ana"))), None);
        assert_eq!(cargo_bin_dir(None, None), None);
    }

    #[test]
    fn without_an_update_there_is_nothing_to_do() {
        let exe = Path::new("/home/ana/.cargo/bin/lazarobox");
        assert_eq!(
            plan(None, Some(exe), bin(), cargo_path()),
            UpdateAction::UpToDate
        );
    }

    #[test]
    fn a_binary_in_the_cargo_bin_dir_installs_with_cargo_over_its_root() {
        let exe = Path::new("/home/ana/.cargo/bin/lazarobox");
        assert_eq!(
            plan(Some("0.16.0"), Some(exe), bin(), cargo_path()),
            UpdateAction::CargoInstall {
                program: PathBuf::from("/home/ana/.cargo/bin/cargo"),
                root: PathBuf::from("/home/ana/.cargo"),
                version: "0.16.0".into()
            }
        );
    }

    #[test]
    fn the_root_follows_the_bin_dir() {
        let exe = Path::new("/opt/cargo/bin/lazarobox");
        let action = plan(
            Some("0.16.0"),
            Some(exe),
            Some(Path::new("/opt/cargo/bin")),
            Some(Path::new("/usr/bin/cargo")),
        );
        assert!(matches!(
            action,
            UpdateAction::CargoInstall { ref root, .. } if root == Path::new("/opt/cargo")
        ));
        let old_default = Path::new("/home/ana/.cargo/bin/lazarobox");
        assert_eq!(
            plan(
                Some("0.16.0"),
                Some(old_default),
                Some(Path::new("/opt/cargo/bin")),
                Some(Path::new("/usr/bin/cargo"))
            ),
            UpdateAction::ShowCommand
        );
    }

    #[test]
    fn any_other_install_only_shows_the_command() {
        for exe in [
            "/usr/bin/lazarobox",
            "/usr/local/bin/lazarobox",
            "/home/ana/src/lazarobox/target/release/lazarobox",
            "/home/ana/.cargo/binx/lazarobox",
            "/home/ana/.cargo/lazarobox",
            "bin/lazarobox",
        ] {
            assert_eq!(
                plan(Some("0.16.0"), Some(Path::new(exe)), bin(), cargo_path()),
                UpdateAction::ShowCommand,
                "{exe}"
            );
        }
    }

    #[test]
    fn an_unknown_binary_or_bin_dir_only_shows_the_command() {
        assert_eq!(
            plan(Some("0.16.0"), None, bin(), cargo_path()),
            UpdateAction::ShowCommand
        );
        let exe = Path::new("/home/ana/.cargo/bin/lazarobox");
        assert_eq!(
            plan(Some("0.16.0"), Some(exe), None, cargo_path()),
            UpdateAction::ShowCommand
        );
    }

    #[test]
    fn without_a_trusted_cargo_the_command_is_shown() {
        let exe = Path::new("/home/ana/.cargo/bin/lazarobox");
        assert_eq!(
            plan(Some("0.16.0"), Some(exe), bin(), None),
            UpdateAction::ShowCommand
        );
    }

    #[test]
    fn the_fallback_footers_are_plain_and_fit_the_popup() {
        for (why, text) in [
            (
                Fallback::NotCargoInstall,
                "Enter or y copies it \u{b7} installed another way",
            ),
            (
                Fallback::NoCargo,
                "Enter or y copies it \u{b7} Rust is not installed",
            ),
            (
                Fallback::UntrustedCargo,
                "Enter or y copies it \u{b7} Rust folder is too open",
            ),
            (
                Fallback::UnusablePath,
                "Enter or y copies it \u{b7} folder name is unusual",
            ),
        ] {
            assert_eq!(why.footer(), text);
            assert!(text.chars().count() <= 48, "{text}");
        }
    }

    #[test]
    fn the_note_tells_a_user_without_rust_to_install_it_first() {
        let none = fallback_lines(Fallback::NoCargo);
        assert_eq!(none[0], "Install Rust first (rustup.rs), then run:");
        assert_eq!(none[1], INSTALL_COMMAND);
        for why in [
            Fallback::NotCargoInstall,
            Fallback::UntrustedCargo,
            Fallback::UnusablePath,
        ] {
            let lines = fallback_lines(why);
            assert_eq!(
                lines[0], "lazarobox cannot update itself here. Run:",
                "{why:?}"
            );
            assert_eq!(lines[1], INSTALL_COMMAND, "{why:?}");
        }
        // The popup is 51 columns with its borders and a leading space.
        for why in [
            Fallback::NotCargoInstall,
            Fallback::NoCargo,
            Fallback::UntrustedCargo,
            Fallback::UnusablePath,
        ] {
            for line in fallback_lines(why) {
                assert!(line.chars().count() <= 48, "{line}");
            }
        }
    }

    #[test]
    fn the_fallback_names_the_first_thing_that_stopped_the_install() {
        let env = |exe: &str, cargo: Option<&str>, refused: bool| InstallEnv {
            exe: Some(exe.into()),
            bin_dir: Some("/home/ana/.cargo/bin".into()),
            cargo: cargo.map(Into::into),
            cargo_refused: refused,
            path: OsString::new(),
        };
        let mine = "/home/ana/.cargo/bin/lazarobox";
        // Not under the cargo bin dir: cargo does not matter.
        assert_eq!(
            env("/usr/bin/lazarobox", None, true).fallback(),
            Fallback::NotCargoInstall
        );
        assert_eq!(env(mine, None, true).fallback(), Fallback::UntrustedCargo);
        assert_eq!(env(mine, None, false).fallback(), Fallback::NoCargo);
        assert_eq!(
            InstallEnv::default().fallback(),
            Fallback::NotCargoInstall,
            "an unknown binary"
        );
    }

    #[test]
    fn a_trusted_cargo_that_cannot_run_for_a_folder_name_is_not_called_not_cargo() {
        use std::os::unix::ffi::OsStringExt;
        // The root goes to cargo as text; a name that is not UTF-8 is not run,
        // although the binary is under the cargo dir and a cargo is trusted.
        let bin = PathBuf::from(OsString::from_vec(b"/home/an\xff/.cargo/bin".to_vec()));
        let env = InstallEnv {
            exe: Some(bin.join("lazarobox")),
            bin_dir: Some(bin),
            cargo: Some("/home/ana/.cargo/bin/cargo".into()),
            cargo_refused: false,
            path: OsString::new(),
        };
        assert_eq!(env.plan(Some("0.16.0")), UpdateAction::ShowCommand);
        assert_eq!(env.fallback(), Fallback::UnusablePath);
    }

    #[test]
    fn a_dotdot_path_does_not_pass_for_the_bin_dir() {
        let exe = Path::new("/home/ana/.cargo/bin/../../evil/lazarobox");
        assert_eq!(
            plan(Some("0.16.0"), Some(exe), bin(), cargo_path()),
            UpdateAction::ShowCommand
        );
    }

    #[test]
    fn a_root_that_is_not_text_is_not_run() {
        use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
        let bin_dir = Path::new(OsStr::from_bytes(b"/home/an\xff/.cargo/bin"));
        let exe = bin_dir.join("lazarobox");
        assert_eq!(
            plan(Some("0.16.0"), Some(&exe), Some(bin_dir), cargo_path()),
            UpdateAction::ShowCommand
        );
    }

    #[test]
    fn the_install_always_names_its_root() {
        assert_eq!(
            install_args("/home/ana/.cargo"),
            [
                "install",
                "lazarobox-shell",
                "--locked",
                "--root",
                "/home/ana/.cargo"
            ]
        );
        // The shown command is the same one without the root.
        assert!(install_args("/r")[..3].join(" ").starts_with("install"));
        assert_eq!(
            INSTALL_COMMAND,
            format!("cargo {}", install_args("/r")[..3].join(" "))
        );
    }

    #[test]
    fn the_first_line_names_what_runs_and_hides_control_characters() {
        let args = install_args("/home/ana/.cargo");
        assert_eq!(
            running_text(Path::new("/home/ana/.cargo/bin/cargo"), &args),
            "running /home/ana/.cargo/bin/cargo install lazarobox-shell --locked --root /home/ana/.cargo"
        );
        let hostile = running_text(Path::new("/x/\u{1b}[2Jcargo"), &args);
        assert!(!hostile.contains('\u{1b}'), "{hostile:?}");
    }

    #[test]
    fn the_install_texts_are_plain_and_say_what_to_do_next() {
        assert_eq!(
            started_text("0.16.0"),
            "Updating lazarobox to 0.16.0. It compiles on your machine and can take a few minutes. Keep lazarobox open until it says it is done."
        );
        assert_eq!(
            installed_text("0.16.0"),
            "Updated to 0.16.0. Restart lazarobox to use it."
        );
        assert_eq!(installed_value("0.16.0"), "0.16.0 installed: restart");
        // Every failure says what happened, then the command to run by hand.
        let by_hand = "run in a terminal: cargo install lazarobox-shell --locked";
        for text in [failed_text(101), not_started_text(), no_status_text()] {
            assert!(text.starts_with("The update "), "{text}");
            assert!(text.ends_with(by_hand), "{text}");
        }
        assert!(failed_text(101).contains("(error 101)"));
    }

    #[test]
    fn the_texts_are_fixed() {
        assert_eq!(status_mark("0.16.0"), "update 0.16.0");
        assert_eq!(
            menu_lines("0.16.0").join(" "),
            "lazarobox 0.16.0 is available. Go to Updates and press Enter."
        );
        assert_eq!(
            fallback_lines(Fallback::NotCargoInstall).join(" "),
            "lazarobox cannot update itself here. Run: cargo install lazarobox-shell --locked"
        );
    }
}
