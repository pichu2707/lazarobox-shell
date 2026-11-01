//! The first-run prefix probe: does Ctrl+Space reach lazarobox in this
//! terminal? The probe only observes keys, it never blocks one. Pure: the
//! caller injects the clock and the environment, and keeps the results in
//! `terminals.json`: a JSON object of `terminal key -> "pass" | "alias" |
//! "missed"` (see [`parse`] and [`render`]).

use std::{
    collections::BTreeMap,
    fmt,
    time::{Duration, Instant},
};

use ratatui::crossterm::event::KeyEvent;

use crate::core::prefix::{ALIAS_KEY, ALIAS_KEY_LEGACY, PREFIX_KEY};

/// How long the probe waits for a key before it counts the prefix as missed.
pub const PROBE_WINDOW: Duration = Duration::from_secs(10);

/// Shown while the probe waits (first run and `--check-prefix`).
pub const CHECK_NOTICE: &str = "Checking the prefix key: press Ctrl+Space";

/// Advice when Ctrl+Space did not arrive. Only advice: Ctrl+] always works.
pub const OFFER_NOTICE: &str = "Ctrl+Space did not reach lazarobox; use Ctrl+] as the prefix key";

/// Shown when the state file cannot be read; no probe runs then.
pub fn read_failed(reason: &str) -> String {
    format!("Could not read the prefix check result ({reason})")
}

/// Shown when the result could not be written; the app keeps working.
pub fn save_failed(reason: &str) -> String {
    format!("Could not remember the prefix check result ({reason})")
}

/// What the probe learned about one terminal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// Ctrl+Space arrived.
    Pass,
    /// Ctrl+] arrived first: the user already found the alias.
    Alias,
    /// Another key or nothing arrived within the window.
    Missed,
}

impl Outcome {
    fn name(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Alias => "alias",
            Self::Missed => "missed",
        }
    }
}

/// Which key reached the app, as far as the probe cares.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Seen {
    /// Ctrl+Space.
    Prefix,
    /// Ctrl+] (either spelling).
    Alias,
    /// Anything else.
    Other,
}

impl Seen {
    /// Classifies a key press.
    pub fn of(key: &KeyEvent) -> Self {
        if PREFIX_KEY.matches(key) {
            Self::Prefix
        } else if ALIAS_KEY.matches(key) || ALIAS_KEY_LEGACY.matches(key) {
            Self::Alias
        } else {
            Self::Other
        }
    }
}

/// The row name of a terminal: `TERM_PROGRAM`, else `Windows Terminal` when
/// `WT_SESSION` is set, else `TERM`, else `unknown`. It only names the stored
/// row; it never changes how the prefix behaves.
pub fn terminal_key(term_program: Option<&str>, wt_session: bool, term: Option<&str>) -> String {
    let set = |v: Option<&str>| {
        v.map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
    };
    set(term_program)
        .or_else(|| wt_session.then(|| "Windows Terminal".to_owned()))
        .or_else(|| set(term))
        .unwrap_or_else(|| "unknown".to_owned())
}

/// Why the stored results could not be used.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ParseError {
    /// The file has no content.
    Empty,
    /// The content is not JSON.
    Json,
    /// The JSON is not an object of strings.
    Shape,
    /// A value is not `pass`, `alias` or `missed`.
    UnknownValue(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("the file is empty"),
            Self::Json => f.write_str("the file is not valid JSON"),
            Self::Shape => f.write_str("the file is not a map of terminal to result"),
            Self::UnknownValue(value) => write!(f, "unknown result \"{value}\""),
        }
    }
}

/// Reads the file: a JSON object of `terminal key -> "pass" | "alias" |
/// "missed"`.
pub fn parse(text: &str) -> Result<BTreeMap<String, Outcome>, ParseError> {
    if text.trim().is_empty() {
        return Err(ParseError::Empty);
    }
    let value: serde_json::Value = serde_json::from_str(text).map_err(|_| ParseError::Json)?;
    let serde_json::Value::Object(map) = value else {
        return Err(ParseError::Shape);
    };
    map.into_iter()
        .map(|(key, value)| {
            let serde_json::Value::String(name) = value else {
                return Err(ParseError::Shape);
            };
            let outcome = match name.as_str() {
                "pass" => Outcome::Pass,
                "alias" => Outcome::Alias,
                "missed" => Outcome::Missed,
                _ => return Err(ParseError::UnknownValue(name)),
            };
            Ok((key, outcome))
        })
        .collect()
}

/// Writes the file `parse` reads.
pub fn render(results: &BTreeMap<String, Outcome>) -> String {
    let map: serde_json::Map<String, serde_json::Value> = results
        .iter()
        .map(|(key, outcome)| (key.clone(), outcome.name().into()))
        .collect();
    let mut text = serde_json::to_string_pretty(&map).unwrap_or_default();
    text.push('\n');
    text
}

/// The probe of one terminal.
#[derive(Clone, Debug)]
pub struct Probe {
    key: String,
    /// When the wait began; `None` when no probe runs.
    waiting: Option<Instant>,
    /// The stored result is `missed`: Ctrl+Space arriving later heals it.
    heal: bool,
}

impl Probe {
    /// `stored` is the remembered result of this terminal; `rerun` asks for
    /// the probe again (`--check-prefix`).
    pub fn new(key: String, stored: Option<Outcome>, rerun: bool, now: Instant) -> Self {
        Self {
            key,
            waiting: (stored.is_none() || rerun).then_some(now),
            heal: stored == Some(Outcome::Missed),
        }
    }

    /// Postpones the probe to the next start (a startup notice is showing).
    pub fn deferred(mut self) -> Self {
        self.waiting = None;
        self
    }

    /// The terminal row this probe writes.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Whether the probe waits for a key (the runtime arms its tick then).
    pub fn is_waiting(&self) -> bool {
        self.waiting.is_some()
    }

    /// A key press. `Some` is a result to remember: the probe's own, or the
    /// heal of a stored `missed` when Ctrl+Space arrives later.
    pub fn on_key(&mut self, seen: Seen) -> Option<Outcome> {
        if self.waiting.take().is_some() {
            self.heal = false;
            return Some(match seen {
                Seen::Prefix => Outcome::Pass,
                Seen::Alias => Outcome::Alias,
                Seen::Other => Outcome::Missed,
            });
        }
        if self.heal && seen == Seen::Prefix {
            self.heal = false;
            return Some(Outcome::Pass);
        }
        None
    }

    /// The clock moved. `Some(Missed)` once the window is over.
    pub fn on_tick(&mut self, now: Instant) -> Option<Outcome> {
        let since = self.waiting?;
        if now.saturating_duration_since(since) < PROBE_WINDOW {
            return None;
        }
        self.waiting = None;
        self.heal = false;
        Some(Outcome::Missed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

    fn ev(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn keys_are_told_apart() {
        let ctrl = KeyModifiers::CONTROL;
        assert_eq!(Seen::of(&ev(KeyCode::Char(' '), ctrl)), Seen::Prefix);
        assert_eq!(Seen::of(&ev(KeyCode::Char(']'), ctrl)), Seen::Alias);
        assert_eq!(Seen::of(&ev(KeyCode::Char('5'), ctrl)), Seen::Alias);
        assert_eq!(
            Seen::of(&ev(KeyCode::Char(' '), KeyModifiers::NONE)),
            Seen::Other
        );
        assert_eq!(Seen::of(&ev(KeyCode::Char('a'), ctrl)), Seen::Other);
    }

    #[test]
    fn the_terminal_key_follows_the_environment_order() {
        assert_eq!(terminal_key(Some("kitty"), true, Some("xterm")), "kitty");
        assert_eq!(terminal_key(None, true, Some("xterm")), "Windows Terminal");
        assert_eq!(
            terminal_key(None, false, Some("xterm-256color")),
            "xterm-256color"
        );
        assert_eq!(terminal_key(None, false, None), "unknown");
        // Empty values count as unset.
        assert_eq!(terminal_key(Some(""), false, Some("")), "unknown");
        assert_eq!(terminal_key(Some("  "), false, Some("screen")), "screen");
    }

    #[test]
    fn results_round_trip_through_json() {
        let results = BTreeMap::from([
            ("kitty".to_owned(), Outcome::Pass),
            ("Windows Terminal".to_owned(), Outcome::Missed),
            ("xterm".to_owned(), Outcome::Alias),
        ]);
        assert_eq!(parse(&render(&results)), Ok(results));
        assert_eq!(parse("{}"), Ok(BTreeMap::new()));
        assert_eq!(
            parse(r#"{"kitty":"pass"}"#),
            Ok(BTreeMap::from([("kitty".to_owned(), Outcome::Pass)]))
        );
    }

    #[test]
    fn a_bad_file_is_a_typed_error_not_a_panic() {
        assert_eq!(parse(""), Err(ParseError::Empty));
        assert_eq!(parse("  \n"), Err(ParseError::Empty));
        assert_eq!(parse("{nope"), Err(ParseError::Json));
        assert_eq!(parse("[]"), Err(ParseError::Shape));
        assert_eq!(parse(r#"{"kitty":1}"#), Err(ParseError::Shape));
        assert_eq!(
            parse(r#"{"kitty":"maybe"}"#),
            Err(ParseError::UnknownValue("maybe".into()))
        );
        for error in [
            ParseError::Empty,
            ParseError::Json,
            ParseError::Shape,
            ParseError::UnknownValue("x".into()),
        ] {
            assert!(!error.to_string().is_empty());
        }
    }

    #[test]
    fn a_first_run_waits_and_each_key_ends_it_once() {
        let cases = [
            (Seen::Prefix, Outcome::Pass),
            (Seen::Alias, Outcome::Alias),
            (Seen::Other, Outcome::Missed),
        ];
        for (seen, outcome) in cases {
            let mut p = Probe::new("kitty".into(), None, false, t0());
            assert!(p.is_waiting());
            assert_eq!(p.on_key(seen), Some(outcome));
            assert!(!p.is_waiting());
            // One result per probe.
            assert_eq!(p.on_key(Seen::Prefix), None);
        }
    }

    #[test]
    fn no_key_for_the_window_is_a_miss_and_earlier_ticks_keep_waiting() {
        let start = t0();
        let mut p = Probe::new("kitty".into(), None, false, start);
        assert_eq!(
            p.on_tick(start + PROBE_WINDOW - Duration::from_millis(1)),
            None
        );
        assert!(p.is_waiting());
        assert_eq!(p.on_tick(start + PROBE_WINDOW), Some(Outcome::Missed));
        assert!(!p.is_waiting());
        assert_eq!(p.on_tick(start + PROBE_WINDOW * 2), None);
    }

    #[test]
    fn a_stored_result_means_no_probe_unless_asked() {
        for stored in [Outcome::Pass, Outcome::Alias, Outcome::Missed] {
            let p = Probe::new("kitty".into(), Some(stored), false, t0());
            assert!(!p.is_waiting());
            let p = Probe::new("kitty".into(), Some(stored), true, t0());
            assert!(p.is_waiting());
        }
    }

    #[test]
    fn a_deferred_probe_never_runs_and_stores_nothing() {
        let start = t0();
        let mut p = Probe::new("kitty".into(), None, false, start).deferred();
        assert!(!p.is_waiting());
        assert_eq!(p.on_key(Seen::Prefix), None);
        assert_eq!(p.on_tick(start + PROBE_WINDOW * 2), None);
    }

    #[test]
    fn a_stored_miss_heals_once_when_ctrl_space_arrives() {
        let mut p = Probe::new("kitty".into(), Some(Outcome::Missed), false, t0());
        assert_eq!(p.on_key(Seen::Other), None);
        assert_eq!(p.on_key(Seen::Alias), None);
        assert_eq!(p.on_key(Seen::Prefix), Some(Outcome::Pass));
        assert_eq!(p.on_key(Seen::Prefix), None);
    }

    #[test]
    fn a_stored_pass_or_alias_never_saves_again() {
        for stored in [Outcome::Pass, Outcome::Alias] {
            let mut p = Probe::new("kitty".into(), Some(stored), false, t0());
            assert_eq!(p.on_key(Seen::Prefix), None);
        }
    }

    #[test]
    fn a_rerun_over_a_miss_is_one_result_not_two() {
        let mut p = Probe::new("kitty".into(), Some(Outcome::Missed), true, t0());
        assert_eq!(p.on_key(Seen::Prefix), Some(Outcome::Pass));
        assert_eq!(p.on_key(Seen::Prefix), None);
    }

    #[test]
    fn a_deferred_probe_still_heals_a_stored_miss() {
        let mut p = Probe::new("kitty".into(), Some(Outcome::Missed), false, t0()).deferred();
        assert_eq!(p.on_key(Seen::Prefix), Some(Outcome::Pass));
    }

    #[test]
    fn the_probe_names_its_terminal_row() {
        assert_eq!(Probe::new("kitty".into(), None, false, t0()).key(), "kitty");
    }

    #[test]
    fn the_consts_have_the_agreed_text() {
        assert_eq!(PROBE_WINDOW, Duration::from_secs(10));
        assert_eq!(CHECK_NOTICE, "Checking the prefix key: press Ctrl+Space");
        assert!(OFFER_NOTICE.contains("Ctrl+]"));
    }
}
