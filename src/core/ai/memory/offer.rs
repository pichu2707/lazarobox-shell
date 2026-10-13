//! The pure side of the `engram serve` offer: whether to offer, what the
//! prompt says, which key confirms and how the run remembers it. No IO.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{
    health::Health,
    port::{MemCmd, Reason},
    reply::ProjectError,
};
use crate::core::ai::{config::MemoryConfig, sanitize::clean};

/// The oldest engram major version that has the HTTP API lazarobox uses.
const MIN_MAJOR: u32 = 3;

/// The longest log line shown after a failed start, in characters.
pub const LOG_LINE_CAP: usize = 120;

/// What the runtime found when it looked for `engram` on the PATH.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Binary {
    /// Nobody looked (a worker without a launcher): never an offer.
    NotChecked,
    /// No executable `engram` on the PATH.
    Missing,
    /// The first `engram` on the PATH: its absolute path and the stdout of
    /// its version command, `None` when that did not run cleanly.
    Found {
        path: String,
        version: Option<String>,
    },
}

/// The offer to start a server: which binary and which port.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Offer {
    pub path: String,
    pub port: u16,
}

impl Offer {
    /// The question. It names the absolute path that would run (ruling 4),
    /// never the bare word `engram`, so the user sees which binary it is.
    pub fn prompt(&self) -> String {
        format!(
            "engram is not running. Start \"{} serve {}\"? y start \u{b7} n not now",
            self.path, self.port
        )
    }
}

/// What to do about a server that is not running.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Decision {
    Offer(Offer),
    /// No offer, but the user is told why.
    Notice(Reason),
    Nothing,
}

/// Everything the decision depends on.
pub struct Facts<'a> {
    pub config: &'a MemoryConfig,
    /// The last health state, `None` while unknown.
    pub health: Option<&'a Health>,
    /// Why the last probe failed, when it did.
    pub reason: Option<&'a Reason>,
    /// Why the project of the chat is unusable, when it is.
    pub project: Option<&'a ProjectError>,
    /// An offer was already made in this run.
    pub used: bool,
    pub binary: &'a Binary,
}

/// Whether to offer `engram serve`, tell why not, or stay silent.
pub fn decide(facts: &Facts) -> Decision {
    let port = facts.config.port;
    if !facts.config.active() || facts.project.is_some() {
        return Decision::Nothing;
    }
    match facts.health {
        Some(Health::Old { .. }) => Decision::Notice(Reason::OldServer { port }),
        Some(Health::Foreign) => Decision::Notice(Reason::Foreign { port }),
        // Only a refused connection means nothing runs there; a timeout is a
        // server that is busy, and starting a second one would race it.
        Some(Health::Down) if facts.reason == Some(&Reason::NotRunning) && !facts.used => {
            if *facts.binary == Binary::NotChecked {
                return Decision::Nothing;
            }
            match usable(facts.binary) {
                Ok(path) => Decision::Offer(Offer {
                    path: path.to_string(),
                    port,
                }),
                Err(reason) => Decision::Notice(reason),
            }
        }
        _ => Decision::Nothing,
    }
}

/// The path of a binary that may be started: found, and a 3.x or later. Else
/// why not: none (or one whose version cannot be read) or a 1.x.
pub fn usable(binary: &Binary) -> Result<&str, Reason> {
    match binary {
        Binary::Found { path, version } => match version.as_deref().and_then(major) {
            Some(m) if m >= MIN_MAJOR => Ok(path),
            Some(_) => Err(Reason::OldBinary),
            None => Err(Reason::NoBinary),
        },
        Binary::NotChecked | Binary::Missing => Err(Reason::NoBinary),
    }
}

/// The major version in a version line such as `engram 3.4.0` or `v3.4`: the
/// integer before the first dot of the first token that starts like a number.
pub fn major(version: &str) -> Option<u32> {
    version.split_whitespace().find_map(|token| {
        let token = token.strip_prefix('v').unwrap_or(token);
        let (digits, rest) = token.split_at(token.find(|c: char| !c.is_ascii_digit())?);
        (!digits.is_empty() && rest.starts_with('.'))
            .then(|| digits.parse().ok())
            .flatten()
    })
}

/// Whether a key confirms the offer: an unmodified, lowercase `y` press.
pub fn confirms(key: KeyEvent) -> bool {
    key.kind != KeyEventKind::Release
        && key.modifiers == KeyModifiers::NONE
        && key.code == KeyCode::Char('y')
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum Stage {
    Idle,
    Pending(Offer),
    Used,
}

/// The offer as the app holds it: at most one per run, however it ends.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ServerOffer {
    stage: Stage,
}

impl Default for ServerOffer {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerOffer {
    pub fn new() -> Self {
        Self { stage: Stage::Idle }
    }

    /// Holds `offer` until the user answers. False when this run already had
    /// its offer (pending, confirmed or declined).
    pub fn accept(&mut self, offer: Offer) -> bool {
        if self.stage != Stage::Idle {
            return false;
        }
        self.stage = Stage::Pending(offer);
        true
    }

    /// The offer waiting for an answer.
    pub fn pending(&self) -> Option<&Offer> {
        match &self.stage {
            Stage::Pending(offer) => Some(offer),
            _ => None,
        }
    }

    /// The user's answer. An unmodified `y` returns the command that starts
    /// the server; any other key declines. Either way the offer is used up.
    /// With nothing pending, a key does nothing.
    pub fn answer(&mut self, key: KeyEvent) -> Option<MemCmd> {
        self.pending()?;
        self.stage = Stage::Used;
        confirms(key).then_some(MemCmd::StartServer)
    }
}

/// The last non-empty line of a server's log, sanitized and cut to
/// `LOG_LINE_CAP` characters, for the line shown when a start fails.
pub fn last_log_line(log: &[u8]) -> Option<String> {
    String::from_utf8_lossy(log)
        .lines()
        .rev()
        .map(|line| clean(line).trim().to_string())
        .find(|line| !line.is_empty())
        .map(|line| {
            line.chars()
                .take(LOG_LINE_CAP)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    use super::*;
    use crate::core::ai::{
        config::MemoryConfig,
        memory::{
            health::Health,
            port::{MemCmd, Reason},
            reply::ProjectError,
        },
    };

    const PORT: u16 = 7437;
    const BIN: &str = "/home/linuxbrew/.linuxbrew/bin/engram";

    fn active() -> MemoryConfig {
        MemoryConfig {
            enabled: true,
            acknowledged: true,
            port: PORT,
        }
    }

    fn found(version: &str) -> Binary {
        Binary::Found {
            path: BIN.into(),
            version: Some(version.into()),
        }
    }

    /// Facts for a refused connection with a 3.x binary: the offer case.
    fn down<'a>(config: &'a MemoryConfig, binary: &'a Binary) -> Facts<'a> {
        Facts {
            config,
            health: Some(&Health::Down),
            reason: Some(&Reason::NotRunning),
            project: None,
            used: false,
            binary,
        }
    }

    #[test]
    fn a_down_server_with_a_3x_binary_gets_the_absolute_path_prompt() {
        let (config, binary) = (active(), found("engram 3.4.0"));
        let decision = decide(&down(&config, &binary));
        let Decision::Offer(offer) = decision else {
            panic!("{decision:?}");
        };
        assert_eq!(offer.path, BIN);
        assert_eq!(offer.port, PORT);
        assert_eq!(
            offer.prompt(),
            "engram is not running. Start \"/home/linuxbrew/.linuxbrew/bin/engram serve 7437\"? \
             y start \u{b7} n not now"
        );
    }

    #[test]
    fn the_prompt_names_the_path_and_the_configured_port_never_the_bare_word() {
        let config = MemoryConfig {
            port: 8123,
            ..active()
        };
        let binary = Binary::Found {
            path: "/opt/e/engram".into(),
            version: Some("3.0.0".into()),
        };
        let Decision::Offer(offer) = decide(&down(&config, &binary)) else {
            panic!("no offer");
        };
        let prompt = offer.prompt();
        assert!(prompt.contains("\"/opt/e/engram serve 8123\""), "{prompt}");
        assert!(!prompt.contains("\"engram "), "{prompt}");
    }

    #[test]
    fn a_1x_binary_gives_the_upgrade_notice_and_no_offer() {
        let config = active();
        for v in ["engram 1.20.0", "1.20", "engram v2.9.9", "engram 0.1.0"] {
            let binary = found(v);
            assert_eq!(
                decide(&down(&config, &binary)),
                Decision::Notice(Reason::OldBinary),
                "{v}"
            );
        }
    }

    #[test]
    fn a_missing_or_unreadable_binary_gives_not_found() {
        let config = active();
        let unreadable = Binary::Found {
            path: BIN.into(),
            version: None,
        };
        let garbage = found("hello world");
        for binary in [Binary::Missing, unreadable, garbage] {
            assert_eq!(
                decide(&down(&config, &binary)),
                Decision::Notice(Reason::NoBinary),
                "{binary:?}"
            );
        }
    }

    #[test]
    fn only_a_found_3x_binary_is_usable() {
        assert_eq!(usable(&found("engram 3.4.0")), Ok(BIN));
        assert_eq!(usable(&found("engram 1.2.0")), Err(Reason::OldBinary));
        assert_eq!(usable(&found("???")), Err(Reason::NoBinary));
        assert_eq!(usable(&Binary::Missing), Err(Reason::NoBinary));
        assert_eq!(usable(&Binary::NotChecked), Err(Reason::NoBinary));
    }

    #[test]
    fn the_major_version_is_read_from_the_first_number() {
        assert_eq!(major("engram 3.4.0"), Some(3));
        assert_eq!(major("engram v3.4.0"), Some(3));
        assert_eq!(major("3.0.0"), Some(3));
        assert_eq!(major("engram 12.1"), Some(12));
        assert_eq!(major("engram 1.20.3"), Some(1));
        assert_eq!(major("engram dev"), None);
        assert_eq!(major(""), None);
    }

    #[test]
    fn old_foreign_unknown_and_ready_servers_get_no_offer() {
        let (config, binary) = (active(), found("engram 3.4.0"));
        let on = |health: Option<&Health>| {
            decide(&Facts {
                health,
                ..down(&config, &binary)
            })
        };
        let old = Health::Old {
            version: "0.1.0".into(),
        };
        let ready = Health::Ready {
            instance_id: "ab".into(),
            version: "3.4.0".into(),
        };
        assert_eq!(
            on(Some(&old)),
            Decision::Notice(Reason::OldServer { port: PORT })
        );
        assert_eq!(
            on(Some(&Health::Foreign)),
            Decision::Notice(Reason::Foreign { port: PORT })
        );
        assert_eq!(on(Some(&ready)), Decision::Nothing);
        assert_eq!(on(None), Decision::Nothing, "unknown health");
    }

    #[test]
    fn a_down_server_that_merely_timed_out_gets_no_offer() {
        let (config, binary) = (active(), found("engram 3.4.0"));
        let decision = decide(&Facts {
            reason: Some(&Reason::NoAnswer),
            ..down(&config, &binary)
        });
        assert_eq!(decision, Decision::Nothing);
        let decision = decide(&Facts {
            reason: None,
            ..down(&config, &binary)
        });
        assert_eq!(decision, Decision::Nothing);
    }

    #[test]
    fn an_inactive_memory_a_bad_project_or_a_used_offer_gets_none() {
        let binary = found("engram 3.4.0");
        for config in [
            MemoryConfig {
                enabled: false,
                ..active()
            },
            MemoryConfig {
                acknowledged: false,
                ..active()
            },
        ] {
            assert_eq!(
                decide(&down(&config, &binary)),
                Decision::Nothing,
                "{config:?}"
            );
        }
        let config = active();
        let used = Facts {
            used: true,
            ..down(&config, &binary)
        };
        assert_eq!(decide(&used), Decision::Nothing);
        for error in [ProjectError::NotGit, ProjectError::Invalid] {
            let facts = Facts {
                project: Some(&error),
                ..down(&config, &binary)
            };
            assert_eq!(decide(&facts), Decision::Nothing, "{error:?}");
        }
        let unchecked = Binary::NotChecked;
        assert_eq!(decide(&down(&config, &unchecked)), Decision::Nothing);
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn only_an_unmodified_lowercase_y_confirms() {
        assert!(confirms(key(KeyCode::Char('y'), KeyModifiers::NONE)));
        let no = [
            key(KeyCode::Char('Y'), KeyModifiers::NONE),
            key(KeyCode::Char('Y'), KeyModifiers::SHIFT),
            key(KeyCode::Char('y'), KeyModifiers::SHIFT),
            key(KeyCode::Char('y'), KeyModifiers::CONTROL),
            key(KeyCode::Char('y'), KeyModifiers::ALT),
            key(KeyCode::Char('n'), KeyModifiers::NONE),
            key(KeyCode::Esc, KeyModifiers::NONE),
            key(KeyCode::Enter, KeyModifiers::NONE),
            key(KeyCode::Char('x'), KeyModifiers::NONE),
        ];
        for k in no {
            assert!(!confirms(k), "{k:?}");
        }
        let mut release = key(KeyCode::Char('y'), KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        assert!(!confirms(release));
    }

    fn offer() -> Offer {
        Offer {
            path: BIN.into(),
            port: PORT,
        }
    }

    #[test]
    fn y_starts_the_server_and_any_other_key_declines_for_the_run() {
        let mut state = ServerOffer::new();
        assert!(state.pending().is_none());
        assert!(state.accept(offer()));
        assert_eq!(state.pending(), Some(&offer()));
        let y = key(KeyCode::Char('y'), KeyModifiers::NONE);
        assert_eq!(state.answer(y), Some(MemCmd::StartServer));
        assert!(state.pending().is_none());
        assert!(!state.accept(offer()), "one offer per run");

        for other in [KeyCode::Char('n'), KeyCode::Esc, KeyCode::Char('x')] {
            let mut state = ServerOffer::new();
            state.accept(offer());
            assert_eq!(
                state.answer(key(other, KeyModifiers::NONE)),
                None,
                "{other:?}"
            );
            assert!(state.pending().is_none());
            assert!(!state.accept(offer()), "{other:?}: no second offer");
        }
        let mut state = ServerOffer::new();
        state.accept(offer());
        let upper = key(KeyCode::Char('Y'), KeyModifiers::SHIFT);
        assert_eq!(state.answer(upper), None);
        assert!(!state.accept(offer()));
    }

    #[test]
    fn a_key_without_a_pending_offer_does_nothing() {
        let mut state = ServerOffer::new();
        let y = key(KeyCode::Char('y'), KeyModifiers::NONE);
        assert_eq!(state.answer(y), None);
        assert!(state.accept(offer()), "the stray key did not use the offer");
    }

    #[test]
    fn the_last_log_line_is_sanitized_and_capped_at_120_characters() {
        assert_eq!(last_log_line(b""), None);
        assert_eq!(last_log_line(b"\n  \n"), None);
        assert_eq!(
            last_log_line(b"starting\nbind: address already in use\n\n").as_deref(),
            Some("bind: address already in use")
        );
        let long = "x".repeat(300);
        assert_eq!(last_log_line(long.as_bytes()).unwrap().chars().count(), 120);
        let esc = last_log_line(b"ok\n\x1b[31mred\x1b[0m boom\x07\n").unwrap();
        assert!(!esc.contains('\x1b') && !esc.contains('\x07'), "{esc:?}");
        assert!(esc.contains("boom"));
        assert_eq!(
            last_log_line("caf\u{e9} \u{65e5}\u{672c}\n".as_bytes()).as_deref(),
            Some("caf\u{e9} \u{65e5}\u{672c}")
        );
    }
}
