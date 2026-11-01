//! The first-run prefix probe in the app (terminal-agnostic S2b): it watches
//! the keys and the clock, never blocks a key, and asks the runtime to keep
//! the result.

use super::*;
use crate::core::probe::{
    CHECK_NOTICE, OFFER_NOTICE, Outcome as ProbeOutcome, Seen, read_failed, save_failed,
};

impl App {
    /// Installs the probe of this terminal row. `stored` is what the state
    /// file said (`Err` is the reason it could not be read, shown as a
    /// notice, and no probe runs); `rerun` is `--check-prefix`. A startup
    /// notice already showing defers the probe to the next start, except for
    /// a rerun, which the user asked for and which joins that notice.
    pub fn with_probe(
        mut self,
        key: String,
        stored: Result<Option<ProbeOutcome>, String>,
        rerun: bool,
    ) -> Self {
        let stored = match stored {
            Ok(stored) => stored,
            Err(reason) => {
                self.add_notice(&read_failed(&reason));
                return self;
            }
        };
        let probe = Probe::new(key, stored, rerun, (self.clock)());
        let probe = if self.notice.is_some() && !rerun {
            probe.deferred()
        } else {
            probe
        };
        if probe.is_waiting() {
            self.add_notice(CHECK_NOTICE);
        }
        self.probe = Some(probe);
        self
    }

    /// Whether the probe waits for a key: the runtime sends `ProbeTick`.
    pub fn probe_waiting(&self) -> bool {
        self.probe.as_ref().is_some_and(Probe::is_waiting)
    }

    /// Adds `text` after the notice that is showing, so neither is lost.
    fn add_notice(&mut self, text: &str) {
        self.notice = Some(match self.notice.take() {
            Some(shown) => format!("{shown}; {text}"),
            None => text.to_owned(),
        });
    }

    /// Looks at a key press before the app acts on it; the key is never
    /// held back or changed.
    pub(super) fn probe_key(&mut self, key: &KeyEvent, repeat: bool) -> Vec<Effect> {
        if repeat {
            return Vec::new();
        }
        let outcome = self
            .probe
            .as_mut()
            .and_then(|probe| probe.on_key(Seen::of(key)));
        self.probe_done(outcome)
    }

    pub(super) fn on_probe_tick(&mut self) -> Vec<Effect> {
        let now = (self.clock)();
        let outcome = self.probe.as_mut().and_then(|probe| probe.on_tick(now));
        self.probe_done(outcome)
    }

    pub(super) fn on_probe_save_failed(&mut self, reason: &str) -> Vec<Effect> {
        self.add_notice(&save_failed(reason));
        self.dirty = true;
        Vec::new()
    }

    /// A missed prefix offers the alias; every result is kept.
    fn probe_done(&mut self, outcome: Option<ProbeOutcome>) -> Vec<Effect> {
        let (Some(outcome), Some(probe)) = (outcome, self.probe.as_ref()) else {
            return Vec::new();
        };
        let key = probe.key().to_owned();
        if outcome == ProbeOutcome::Missed {
            self.notice = Some(OFFER_NOTICE.to_owned());
        }
        self.dirty = true;
        vec![Effect::SaveProbe { key, outcome }]
    }
}
