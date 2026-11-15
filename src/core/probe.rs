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

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::core::prefix::{ALIAS_KEY, ALIAS_KEY_LEGACY, PREFIX_KEY};

/// How long the probe waits for a key before it counts the prefix as missed.
pub const PROBE_WINDOW: Duration = Duration::from_secs(10);

/// Shown while the probe waits (first run and `--check-prefix`).
pub const CHECK_NOTICE: &str = "Checking the prefix key: press Ctrl+Space";

/// Advice when Ctrl+Space did not arrive. Only advice: Ctrl+] always works.
pub const OFFER_NOTICE: &str = "Ctrl+Space did not reach lazarobox; use Ctrl+] as the prefix key";

/// Shown when the state file cannot be read; no probe runs then. `reason`
/// names the file path.
pub fn read_failed(reason: &str) -> String {
    format!("Could not read the prefix check result: {reason}; delete the file to check again")
}

/// Shown when the result could not be written; the app keeps working.
/// `reason` names the file path.
pub fn save_failed(reason: &str) -> String {
    format!("Could not keep the prefix check result: {reason}")
}

/// What the probe learned about one terminal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// Ctrl+Space arrived.
    Pass,
    /// Ctrl+] arrived first: the user already found the alias.
    Alias,
    /// A key other than typing arrived and it was not Ctrl+Space or the alias.
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
    /// An unmodified printable key other than the space bar: the user is
    /// typing, so the probe keeps waiting.
    Plain,
    /// Anything else.
    Other,
}

impl Seen {
    /// Classifies a key press.
    pub fn of(key: &KeyEvent) -> Self {
        let typed = matches!(key.code, KeyCode::Char(c) if c != ' ')
            && (key.modifiers - KeyModifiers::SHIFT).is_empty();
        if PREFIX_KEY.matches(key) {
            Self::Prefix
        } else if ALIAS_KEY.matches(key) || ALIAS_KEY_LEGACY.matches(key) {
            Self::Alias
        } else if typed {
            Self::Plain
        } else {
            Self::Other
        }
    }
}

/// The row name of a terminal, from the first of these that is set (`var`
/// reads the environment; empty values count as unset):
///
/// 1. `TERM_PROGRAM`;
/// 2. `Windows Terminal` when `WT_SESSION` is set;
/// 3. a terminal-specific variable: `KITTY_WINDOW_ID` is `kitty`,
///    `WEZTERM_EXECUTABLE` is `WezTerm`, `ALACRITTY_WINDOW_ID` or
///    `ALACRITTY_SOCKET` is `Alacritty`, `KONSOLE_VERSION` is `Konsole`,
///    `VTE_VERSION` is `vte` (GNOME Terminal, Tilix and the rest share VTE's
///    key handling);
/// 4. `TERM`, followed by ` (name)` when it is generic (`xterm` or `xterm-`
///    plus a colour depth, as `xterm-256color`) and `emulator`, the process
///    name of the terminal found by [`ancestor_emulator`], gives one. zutty,
///    st and xterm itself only set that, so `TERM` alone would share one row.
///    `emulator` runs only then, and never over ssh (`SSH_CONNECTION` or
///    `SSH_TTY` set), where the ancestor is `sshd`, not the terminal;
/// 5. `unknown`.
///
/// The process walk is best effort: it silently falls back to plain `TERM`
/// when `/proc` is unreadable, the chain is deeper than the limit, or it
/// reaches pid 1. The key is only a row name, so the fallback costs one extra
/// probe, not a failure.
///
/// It only names the stored row; it never changes how the prefix behaves.
/// Rows stored under the old generic key (`xterm-256color`) no longer match,
/// so those terminals probe once more.
pub fn terminal_key(
    var: impl Fn(&str) -> Option<String>,
    emulator: impl FnOnce() -> Option<String>,
) -> String {
    let set = |name: &str| {
        var(name)
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    };
    if let Some(program) = set("TERM_PROGRAM") {
        return program;
    }
    if set("WT_SESSION").is_some() {
        return "Windows Terminal".to_owned();
    }
    const SPECIFIC: [(&[&str], &str); 5] = [
        (&["KITTY_WINDOW_ID"], "kitty"),
        (&["WEZTERM_EXECUTABLE"], "WezTerm"),
        (&["ALACRITTY_WINDOW_ID", "ALACRITTY_SOCKET"], "Alacritty"),
        (&["KONSOLE_VERSION"], "Konsole"),
        (&["VTE_VERSION"], "vte"),
    ];
    if let Some((_, name)) = SPECIFIC
        .iter()
        .find(|(names, _)| names.iter().any(|n| set(n).is_some()))
    {
        return (*name).to_owned();
    }
    let Some(term) = set("TERM") else {
        return "unknown".to_owned();
    };
    if !is_generic_term(&term) || set("SSH_CONNECTION").is_some() || set("SSH_TTY").is_some() {
        return term;
    }
    match emulator()
        .map(|e| e.trim().to_owned())
        .filter(|e| !e.is_empty())
    {
        Some(name) => format!("{term} ({name})"),
        None => term,
    }
}

/// `xterm`, or `xterm-` plus only a colour depth: names that many terminals
/// share. `xterm-kitty` and the like carry a vendor and are not generic.
fn is_generic_term(term: &str) -> bool {
    const DEPTHS: [&str; 8] = [
        "color",
        "16color",
        "88color",
        "256color",
        "direct",
        "direct2",
        "direct16",
        "direct256",
    ];
    term == "xterm"
        || term
            .strip_prefix("xterm-")
            .is_some_and(|d| DEPTHS.contains(&d))
}

/// Process names that sit between the terminal and lazarobox.
const NOT_AN_EMULATOR: [&str; 38] = [
    "sh",
    "bash",
    "zsh",
    "fish",
    "dash",
    "ksh",
    "mksh",
    "tcsh",
    "csh",
    "nu",
    "elvish",
    "xonsh",
    "login",
    "sudo",
    "su",
    "doas",
    "env",
    "script",
    "lazarobox",
    "lazarobox-shell",
    "sshd",
    "sshd-session",
    "ssh",
    "mosh-server",
    "mosh-client",
    "tmux: server",
    "tmux",
    "screen",
    "cargo",
    "just",
    "make",
    "nvim",
    "vim",
    "python3",
    "python",
    "node",
    "pwsh",
    "sudo-rs",
];

/// How many ancestors [`ancestor_emulator`] looks at.
const WALK_LIMIT: usize = 8;

/// The parent pid in a `/proc/<pid>/stat` line: the second field after the
/// closing parenthesis of the command name, which may hold spaces and
/// parentheses itself (`pid (comm) state ppid ...`).
pub fn parent_pid(stat: &str) -> Option<u32> {
    let after = &stat[stat.rfind(')')? + 1..];
    after.split_whitespace().nth(1)?.parse().ok()
}

/// The terminal emulator's process name: walks up from `start` (the parent
/// of lazarobox), skipping shells and wrappers, and returns the first other
/// name. `read` gives a pid's `(comm, parent pid)`. Pid 1, `init`,
/// `systemd`, a missing process, a name with control characters and
/// `WALK_LIMIT` levels all end the walk with no name.
pub fn ancestor_emulator(
    start: u32,
    read: impl Fn(u32) -> Option<(String, u32)>,
) -> Option<String> {
    let mut pid = start;
    for _ in 0..WALK_LIMIT {
        if pid <= 1 {
            return None;
        }
        let (comm, parent) = read(pid)?;
        let comm = comm.trim();
        if comm.is_empty()
            || comm.chars().any(char::is_control)
            || matches!(comm, "init" | "systemd")
        {
            return None;
        }
        if !NOT_AN_EMULATOR.contains(&comm) {
            return Some(comm.to_owned());
        }
        pid = parent;
    }
    None
}

/// The emulator of this process, from `/proc` on Linux; no name elsewhere.
#[cfg(target_os = "linux")]
pub fn current_emulator() -> Option<String> {
    ancestor_emulator(std::os::unix::process::parent_id(), |pid| {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
        Some((comm.trim_end().to_owned(), parent_pid(&stat)?))
    })
}

/// The emulator of this process: unknown off Linux.
#[cfg(not(target_os = "linux"))]
pub fn current_emulator() -> Option<String> {
    None
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

/// Where the wait stands.
#[derive(Clone, Copy, Debug)]
enum Wait {
    /// No tick yet: the app may still be starting, so the window is closed.
    NotStarted,
    /// The first tick came at this time.
    Since(Instant),
}

/// The probe of one terminal.
#[derive(Clone, Debug)]
pub struct Probe {
    key: String,
    /// `None` when no probe runs.
    waiting: Option<Wait>,
    /// The stored result is `missed`: Ctrl+Space arriving later heals it.
    heal: bool,
}

impl Probe {
    /// `stored` is the remembered result of this terminal; `rerun` asks for
    /// the probe again (`--check-prefix`). The window starts at the first
    /// [`Probe::on_tick`].
    pub fn new(key: String, stored: Option<Outcome>, rerun: bool) -> Self {
        Self {
            key,
            waiting: (stored.is_none() || rerun).then_some(Wait::NotStarted),
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
    /// heal of a stored `missed` when Ctrl+Space arrives later. Typing
    /// (`Seen::Plain`) is not an answer and keeps the probe waiting.
    pub fn on_key(&mut self, seen: Seen) -> Option<Outcome> {
        if self.waiting.is_some() && seen != Seen::Plain {
            self.waiting = None;
            self.heal = false;
            return Some(match seen {
                Seen::Prefix => Outcome::Pass,
                Seen::Alias => Outcome::Alias,
                Seen::Other | Seen::Plain => Outcome::Missed,
            });
        }
        if self.heal && seen == Seen::Prefix {
            self.heal = false;
            return Some(Outcome::Pass);
        }
        None
    }

    /// The clock moved. The first tick opens the window; once it is over the
    /// probe ends with no result (nothing is stored and no offer is shown:
    /// it runs again on the next start). Check [`Probe::is_waiting`] to see
    /// that it ended.
    pub fn on_tick(&mut self, now: Instant) -> Option<Outcome> {
        match self.waiting {
            Some(Wait::NotStarted) => self.waiting = Some(Wait::Since(now)),
            Some(Wait::Since(since)) if now.saturating_duration_since(since) >= PROBE_WINDOW => {
                self.waiting = None;
            }
            _ => {}
        }
        None
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
        // A plain space is a key press that is not the prefix: a miss.
        assert_eq!(
            Seen::of(&ev(KeyCode::Char(' '), KeyModifiers::NONE)),
            Seen::Other
        );
        assert_eq!(Seen::of(&ev(KeyCode::Char('a'), ctrl)), Seen::Other);
        // Typing does not count.
        for c in ['a', '7', ';', 'é'] {
            assert_eq!(
                Seen::of(&ev(KeyCode::Char(c), KeyModifiers::NONE)),
                Seen::Plain,
                "{c}"
            );
        }
        assert_eq!(
            Seen::of(&ev(KeyCode::Char('A'), KeyModifiers::SHIFT)),
            Seen::Plain
        );
        // Modified or non-character keys are misses.
        for other in [
            ev(KeyCode::Char('a'), KeyModifiers::ALT),
            ev(KeyCode::Esc, KeyModifiers::NONE),
            ev(KeyCode::Enter, KeyModifiers::NONE),
            ev(KeyCode::Up, KeyModifiers::NONE),
            ev(KeyCode::Char(' '), KeyModifiers::SHIFT),
        ] {
            assert_eq!(Seen::of(&other), Seen::Other, "{other:?}");
        }
    }

    /// An injected environment: only the listed variables are set.
    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            vars.iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn the_terminal_key_follows_the_environment_order() {
        let key = |vars: &[(&str, &str)]| terminal_key(env(vars), || None);
        assert_eq!(
            key(&[
                ("TERM_PROGRAM", "kitty"),
                ("WT_SESSION", "1"),
                ("TERM", "xterm")
            ]),
            "kitty"
        );
        assert_eq!(
            key(&[("WT_SESSION", "1"), ("TERM", "xterm")]),
            "Windows Terminal"
        );
        assert_eq!(key(&[("TERM", "xterm-kitty")]), "xterm-kitty");
        assert_eq!(key(&[("TERM", "xterm-256color")]), "xterm-256color");
        assert_eq!(key(&[]), "unknown");
        // Empty values count as unset.
        assert_eq!(key(&[("TERM_PROGRAM", ""), ("TERM", "")]), "unknown");
        assert_eq!(key(&[("TERM_PROGRAM", "  "), ("TERM", "screen")]), "screen");
    }

    #[test]
    fn terminal_specific_variables_come_before_term() {
        let key = |vars: &[(&str, &str)]| terminal_key(env(vars), || Some("zutty".to_owned()));
        let t = ("TERM", "xterm-256color");
        assert_eq!(key(&[("KITTY_WINDOW_ID", "3"), t]), "kitty");
        assert_eq!(
            key(&[("WEZTERM_EXECUTABLE", "/usr/bin/wezterm"), t]),
            "WezTerm"
        );
        assert_eq!(key(&[("ALACRITTY_WINDOW_ID", "1"), t]), "Alacritty");
        assert_eq!(key(&[("ALACRITTY_SOCKET", "/run/a.sock"), t]), "Alacritty");
        assert_eq!(key(&[("KONSOLE_VERSION", "240800"), t]), "Konsole");
        assert_eq!(key(&[("VTE_VERSION", "7600"), t]), "vte");
        // TERM_PROGRAM and WT_SESSION still win over them.
        assert_eq!(
            key(&[("TERM_PROGRAM", "WezTerm"), ("KITTY_WINDOW_ID", "3"), t]),
            "WezTerm"
        );
        assert_eq!(
            key(&[("WT_SESSION", "x"), ("VTE_VERSION", "7600"), t]),
            "Windows Terminal"
        );
        // An empty value is unset.
        assert_eq!(key(&[("VTE_VERSION", " "), t]), "xterm-256color (zutty)");
    }

    #[test]
    fn a_generic_term_gets_the_emulator_name_appended() {
        let key = |term: &str, name: Option<&str>| {
            terminal_key(env(&[("TERM", term)]), || name.map(str::to_owned))
        };
        assert_eq!(
            key("xterm-256color", Some("zutty")),
            "xterm-256color (zutty)"
        );
        assert_eq!(key("xterm", Some("xterm")), "xterm (xterm)");
        assert_eq!(key("xterm-color", Some("st")), "xterm-color (st)");
        assert_eq!(key("xterm-direct", Some("st")), "xterm-direct (st)");
        assert_eq!(key("xterm-256color", None), "xterm-256color");
        assert_eq!(key("xterm-256color", Some("  ")), "xterm-256color");
        // A vendor suffix or another family already tells terminals apart.
        assert_eq!(key("xterm-kitty", Some("kitty")), "xterm-kitty");
        assert_eq!(key("foot", Some("foot")), "foot");
        assert_eq!(key("screen-256color", Some("tmux")), "screen-256color");
        // A terminal-specific variable is not suffixed either.
        assert_eq!(
            terminal_key(env(&[("VTE_VERSION", "1"), ("TERM", "xterm")]), || Some(
                "gnome-terminal-".to_owned()
            )),
            "vte"
        );
    }

    #[test]
    fn the_walk_runs_only_when_a_generic_term_decides_the_key() {
        use std::cell::Cell;
        let key = |vars: &[(&str, &str)]| {
            let calls = Cell::new(0);
            let k = terminal_key(env(vars), || {
                calls.set(calls.get() + 1);
                Some("zutty".to_owned())
            });
            (k, calls.get())
        };
        let t = ("TERM", "xterm-256color");
        assert_eq!(key(&[t]), ("xterm-256color (zutty)".to_owned(), 1));
        for vars in [
            vec![("TERM_PROGRAM", "kitty"), t],
            vec![("WT_SESSION", "1"), t],
            vec![("VTE_VERSION", "1"), t],
            vec![("TERM", "xterm-kitty")],
            vec![("TERM", "screen")],
            vec![],
            vec![t, ("SSH_CONNECTION", "1.2.3.4 1 5.6.7.8 22")],
            vec![t, ("SSH_TTY", "/dev/pts/3")],
        ] {
            assert_eq!(key(&vars).1, 0, "{vars:?}");
        }
        // Over ssh the key stays plain TERM.
        assert_eq!(key(&[t, ("SSH_TTY", "/dev/pts/3")]).0, "xterm-256color");
        // Empty ssh variables do not count.
        assert_eq!(key(&[t, ("SSH_TTY", "")]).1, 1);
    }

    #[test]
    fn wrappers_and_session_tools_keep_the_walk_going() {
        for name in [
            "sshd",
            "sshd-session",
            "ssh",
            "mosh-server",
            "mosh-client",
            "tmux: server",
            "tmux",
            "screen",
            "cargo",
            "just",
            "make",
            "nvim",
            "vim",
            "python3",
            "python",
            "node",
            "pwsh",
            "sudo-rs",
        ] {
            let tree = [(40, name, 30), (30, "zutty", 1)];
            assert_eq!(
                ancestor_emulator(40, table(&tree)),
                Some("zutty".into()),
                "{name}"
            );
        }
    }

    #[test]
    fn a_name_with_control_characters_is_no_name() {
        let tree = [(40, "zu\x1b[31mtty", 1)];
        assert_eq!(ancestor_emulator(40, table(&tree)), None);
        let tree = [(40, "bash", 30), (30, "a\nb", 20), (20, "zutty", 1)];
        assert_eq!(ancestor_emulator(40, table(&tree)), None);
    }

    #[test]
    fn the_parent_pid_is_read_after_the_closing_paren_of_comm() {
        assert_eq!(parent_pid("42 (zutty) S 7 42 42 0"), Some(7));
        // The command name may hold spaces and parentheses.
        assert_eq!(parent_pid("42 (we ird) (x) S 9 42"), Some(9));
        assert_eq!(parent_pid("42 (a) S"), None);
        assert_eq!(parent_pid("42 (a) S x 1"), None);
        assert_eq!(parent_pid("no parens"), None);
        assert_eq!(parent_pid(""), None);
    }

    /// A process table: pid -> (comm, parent pid).
    fn table<'a>(rows: &'a [(u32, &'a str, u32)]) -> impl Fn(u32) -> Option<(String, u32)> + 'a {
        move |pid| {
            rows.iter()
                .find(|(p, _, _)| *p == pid)
                .map(|(_, c, pp)| ((*c).to_owned(), *pp))
        }
    }

    #[test]
    fn the_emulator_is_the_first_ancestor_that_is_not_a_shell_or_wrapper() {
        let tree = [
            (50, "lazarobox-shell", 40),
            (40, "bash", 30),
            (30, "sudo", 25),
            (25, "zutty", 1),
            (1, "systemd", 0),
        ];
        assert_eq!(ancestor_emulator(40, table(&tree)), Some("zutty".into()));
        assert_eq!(ancestor_emulator(50, table(&tree)), Some("zutty".into()));
        // Reaching pid 1 gives no name, and so does init as the first hit.
        let bare = [(40, "zsh", 1), (1, "systemd", 0)];
        assert_eq!(ancestor_emulator(40, table(&bare)), None);
        let init = [(40, "fish", 1), (1, "init", 0)];
        assert_eq!(ancestor_emulator(40, table(&init)), None);
        // A missing process or an empty name ends the walk.
        assert_eq!(ancestor_emulator(99, table(&tree)), None);
        assert_eq!(ancestor_emulator(0, table(&tree)), None);
        let blank = [(40, "", 30), (30, "zutty", 1)];
        assert_eq!(ancestor_emulator(40, table(&blank)), None);
    }

    #[test]
    fn the_walk_is_bounded_and_survives_a_loop() {
        // Nine shells deep: the emulator is past the limit.
        let deep: Vec<(u32, &str, u32)> = (10..19)
            .map(|p| (p, "bash", p + 1))
            .chain([(19, "zutty", 1)])
            .collect();
        assert_eq!(ancestor_emulator(10, table(&deep)), None);
        let near: Vec<(u32, &str, u32)> = (10..17)
            .map(|p| (p, "bash", p + 1))
            .chain([(17, "zutty", 1)])
            .collect();
        assert_eq!(ancestor_emulator(10, table(&near)), Some("zutty".into()));
        let cycle = [(5, "bash", 6), (6, "sh", 5)];
        assert_eq!(ancestor_emulator(5, table(&cycle)), None);
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
            let mut p = Probe::new("kitty".into(), None, false);
            assert!(p.is_waiting());
            assert_eq!(p.on_key(seen), Some(outcome));
            assert!(!p.is_waiting());
            // One result per probe.
            assert_eq!(p.on_key(Seen::Prefix), None);
        }
    }

    // The user decided: a quiet window stores nothing and offers nothing.
    #[test]
    fn a_quiet_window_ends_the_probe_with_no_result() {
        let start = t0();
        let mut p = Probe::new("kitty".into(), None, false);
        // The first tick starts the window.
        assert_eq!(p.on_tick(start), None);
        assert_eq!(
            p.on_tick(start + PROBE_WINDOW - Duration::from_millis(1)),
            None
        );
        assert!(p.is_waiting());
        assert_eq!(p.on_tick(start + PROBE_WINDOW), None);
        assert!(!p.is_waiting());
        assert_eq!(p.on_key(Seen::Prefix), None);
    }

    // The window must not run while the app is still starting.
    #[test]
    fn the_window_starts_at_the_first_tick_not_at_construction() {
        let start = t0();
        let mut p = Probe::new("kitty".into(), None, false);
        let late = start + Duration::from_secs(60);
        assert_eq!(p.on_tick(late), None);
        assert!(p.is_waiting());
        assert_eq!(
            p.on_tick(late + PROBE_WINDOW - Duration::from_millis(1)),
            None
        );
        assert!(p.is_waiting());
        assert_eq!(p.on_tick(late + PROBE_WINDOW), None);
        assert!(!p.is_waiting());
    }

    #[test]
    fn typing_keeps_the_probe_waiting() {
        let mut p = Probe::new("kitty".into(), None, false);
        assert_eq!(p.on_key(Seen::Plain), None);
        assert!(p.is_waiting());
        assert_eq!(p.on_key(Seen::Other), Some(Outcome::Missed));
    }

    #[test]
    fn a_timed_out_rerun_over_a_miss_can_still_heal() {
        let start = t0();
        let mut p = Probe::new("kitty".into(), Some(Outcome::Missed), true);
        p.on_tick(start);
        assert_eq!(p.on_tick(start + PROBE_WINDOW), None);
        assert_eq!(p.on_key(Seen::Prefix), Some(Outcome::Pass));
    }

    #[test]
    fn a_stored_result_means_no_probe_unless_asked() {
        for stored in [Outcome::Pass, Outcome::Alias, Outcome::Missed] {
            let p = Probe::new("kitty".into(), Some(stored), false);
            assert!(!p.is_waiting());
            let p = Probe::new("kitty".into(), Some(stored), true);
            assert!(p.is_waiting());
        }
    }

    #[test]
    fn a_deferred_probe_never_runs_and_stores_nothing() {
        let start = t0();
        let mut p = Probe::new("kitty".into(), None, false).deferred();
        assert!(!p.is_waiting());
        assert_eq!(p.on_key(Seen::Prefix), None);
        assert_eq!(p.on_tick(start + PROBE_WINDOW * 2), None);
    }

    #[test]
    fn a_stored_miss_heals_once_when_ctrl_space_arrives() {
        let mut p = Probe::new("kitty".into(), Some(Outcome::Missed), false);
        assert_eq!(p.on_key(Seen::Other), None);
        assert_eq!(p.on_key(Seen::Alias), None);
        assert_eq!(p.on_key(Seen::Prefix), Some(Outcome::Pass));
        assert_eq!(p.on_key(Seen::Prefix), None);
    }

    #[test]
    fn a_stored_pass_or_alias_never_saves_again() {
        for stored in [Outcome::Pass, Outcome::Alias] {
            let mut p = Probe::new("kitty".into(), Some(stored), false);
            assert_eq!(p.on_key(Seen::Prefix), None);
        }
    }

    #[test]
    fn a_rerun_over_a_miss_is_one_result_not_two() {
        let mut p = Probe::new("kitty".into(), Some(Outcome::Missed), true);
        assert_eq!(p.on_key(Seen::Prefix), Some(Outcome::Pass));
        assert_eq!(p.on_key(Seen::Prefix), None);
    }

    #[test]
    fn a_deferred_probe_still_heals_a_stored_miss() {
        let mut p = Probe::new("kitty".into(), Some(Outcome::Missed), false).deferred();
        assert_eq!(p.on_key(Seen::Prefix), Some(Outcome::Pass));
    }

    #[test]
    fn the_probe_names_its_terminal_row() {
        assert_eq!(Probe::new("kitty".into(), None, false).key(), "kitty");
    }

    #[test]
    fn the_consts_have_the_agreed_text() {
        assert_eq!(PROBE_WINDOW, Duration::from_secs(10));
        assert_eq!(CHECK_NOTICE, "Checking the prefix key: press Ctrl+Space");
        assert!(OFFER_NOTICE.contains("Ctrl+]"));
    }
}
