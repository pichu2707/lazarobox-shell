//! When does a command that was run in a pane count as finished? Pure: the
//! caller feeds a [`Probe`] on the existing 16 ms tick and an injected clock,
//! so every rule is testable without a pty or a timer (design D22).

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

/// Silence after the later of the write and the last output.
pub const QUIET: Duration = Duration::from_millis(250);
/// Longest wait after the write.
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// What the pane looked like at the write.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Anchor {
    pub line: usize,
    /// The cursor column at the write. Capture does not need it: the echo
    /// ends on the first row that does not wrap.
    pub col: u16,
    pub generation: u32,
    pub cwd: PathBuf,
}

/// What the pane looks like now. `None` means the runtime could not tell.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Probe {
    pub now: Instant,
    /// Whether the shell itself is the foreground process.
    pub foreground: Option<bool>,
    pub cwd: Option<PathBuf>,
    pub cursor_line: Option<usize>,
    pub generation: u32,
    pub alt_screen: bool,
}

/// How a run ended. `PaneGone` is never produced by [`Settle::step`]: the
/// app reports it when the pane closes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum End {
    Settled,
    TimedOut,
    /// 10 s passed and the foreground was never known, so the run may well
    /// have finished: the model is not told it is "still running".
    Unconfirmed,
    FullScreen,
    UserTyped,
    PaneGone,
}

/// The state of one run in flight.
pub struct Settle {
    anchor: Anchor,
    written: Instant,
    last: Instant,
    typed: bool,
    generation: u32,
    /// Whether `anchor.line` is the cursor's line at the write.
    anchored: bool,
}

impl Settle {
    /// A run written at `written`.
    pub fn new(anchor: Anchor, written: Instant) -> Self {
        let generation = anchor.generation;
        Self {
            anchor,
            written,
            last: written,
            typed: false,
            generation,
            anchored: true,
        }
    }

    /// The cursor was hidden at the write, so `anchor.line` is a placeholder:
    /// the first probe that sees a cursor line becomes the anchor, and a run
    /// that never sees one cannot confirm it finished.
    pub fn unanchored(mut self) -> Self {
        self.anchored = false;
        self
    }

    /// What the pane looked like at the write, once it is known: `None` while
    /// the cursor was hidden at the write and no probe has seen a line yet.
    pub fn anchor(&self) -> Option<&Anchor> {
        self.anchored.then_some(&self.anchor)
    }

    /// The pane printed something at `now`. Within a tick the app calls this
    /// BEFORE [`Settle::step`], so output of the same tick restarts the quiet.
    pub fn output(&mut self, now: Instant) {
        self.last = self.last.max(now);
    }

    /// The user sent a key to the pane during the run.
    pub fn user_key(&mut self) {
        self.typed = true;
    }

    /// Whether the lines read after the run line up with the anchor: false
    /// once the line generation changed (scrollback was dropped or reflowed).
    pub fn lines_comparable(&self) -> bool {
        self.generation == self.anchor.generation
    }

    /// Checks the end rules in order; `None` keeps waiting.
    ///
    /// 1. The alternate screen is `FullScreen`; a user key is `UserTyped`.
    /// 2. 10 s since the write is `TimedOut` (`Unconfirmed` when the foreground
    ///    is unknown or no cursor line was ever seen), even when the rest holds.
    /// 3. `Settled` needs ALL of: the foreground is the shell (`None` waits,
    ///    which fails closed); 250 ms of quiet; and progress, meaning the
    ///    cursor moved to a later line, the real cwd changed, or the line
    ///    generation changed (then the lines are not comparable).
    pub fn step(&mut self, probe: &Probe) -> Option<End> {
        self.generation = probe.generation;
        if let (false, Some(line)) = (self.anchored, probe.cursor_line) {
            self.anchor.line = line;
            self.anchored = true;
        }
        if probe.alt_screen {
            return Some(End::FullScreen);
        }
        if self.typed {
            return Some(End::UserTyped);
        }
        let since = |from: Instant| probe.now.saturating_duration_since(from);
        if since(self.written) >= TIMEOUT {
            return Some(if probe.foreground.is_none() || !self.anchored {
                End::Unconfirmed
            } else {
                End::TimedOut
            });
        }
        let progress = probe.cursor_line.is_some_and(|l| l > self.anchor.line)
            || probe.cwd.as_ref().is_some_and(|c| *c != self.anchor.cwd)
            || !self.lines_comparable();
        (probe.foreground == Some(true) && since(self.last) >= QUIET && progress)
            .then_some(End::Settled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    struct Run {
        t0: Instant,
        settle: Settle,
    }

    fn run() -> Run {
        let t0 = Instant::now();
        let anchor = Anchor {
            line: 5,
            col: 12,
            generation: 1,
            cwd: "/home/u".into(),
        };
        Run {
            t0,
            settle: Settle::new(anchor, t0),
        }
    }

    impl Run {
        /// A shell at the prompt, one line below the anchor, `ms` after the write.
        fn probe(&self, ms: u64) -> Probe {
            Probe {
                now: self.t0 + MS(ms),
                foreground: Some(true),
                cwd: Some("/home/u".into()),
                cursor_line: Some(6),
                generation: 1,
                alt_screen: false,
            }
        }

        fn step(&mut self, probe: &Probe) -> Option<End> {
            self.settle.step(probe)
        }
    }

    #[test]
    fn the_anchor_is_known_only_once_a_cursor_line_was_seen() {
        let r = run();
        assert_eq!(r.settle.anchor().map(|a| a.line), Some(5));
        let mut hidden = Run {
            t0: r.t0,
            settle: Settle::new(r.settle.anchor().unwrap().clone(), r.t0).unanchored(),
        };
        assert_eq!(hidden.settle.anchor(), None);
        let mut p = hidden.probe(10);
        p.cursor_line = Some(9);
        hidden.step(&p);
        assert_eq!(hidden.settle.anchor().map(|a| a.line), Some(9));
    }

    #[test]
    fn a_silent_mkdir_settles_after_the_quiet_once_the_cursor_advanced() {
        let mut r = run();
        let mut p = r.probe(100);
        p.cursor_line = Some(5);
        assert_eq!(r.step(&p), None, "the prompt has not moved yet");
        assert_eq!(r.step(&r.probe(249)), None, "not quiet for 250 ms");
        assert_eq!(r.step(&r.probe(250)), Some(End::Settled));
    }

    #[test]
    fn output_restarts_the_quiet() {
        let mut r = run();
        r.settle.output(r.t0 + MS(200));
        assert_eq!(r.step(&r.probe(400)), None);
        assert_eq!(r.step(&r.probe(450)), Some(End::Settled));
    }

    #[test]
    fn no_progress_keeps_waiting() {
        let mut r = run();
        let mut p = r.probe(5_000);
        p.cursor_line = Some(5);
        assert_eq!(r.step(&p), None);
        p.cursor_line = None;
        assert_eq!(r.step(&p), None);
    }

    #[test]
    fn cd_settles_through_the_cwd_signal() {
        let mut r = run();
        let mut p = r.probe(300);
        p.cursor_line = Some(5);
        p.cwd = Some("/home/u/dev".into());
        assert_eq!(r.step(&p), Some(End::Settled));
    }

    #[test]
    fn an_unknown_cwd_is_not_progress() {
        let mut r = run();
        let mut p = r.probe(300);
        p.cursor_line = Some(5);
        p.cwd = None;
        assert_eq!(r.step(&p), None);
    }

    #[test]
    fn an_unknown_or_busy_foreground_never_settles() {
        for foreground in [None, Some(false)] {
            let mut r = run();
            let mut p = r.probe(5_000);
            p.foreground = foreground;
            assert_eq!(r.step(&p), None);
            p.now = r.t0 + TIMEOUT;
            let end = if foreground.is_none() {
                End::Unconfirmed
            } else {
                End::TimedOut
            };
            assert_eq!(r.step(&p), Some(end), "ends only at the timeout");
        }
    }

    #[test]
    fn a_generation_change_settles_and_the_lines_are_not_comparable() {
        let mut r = run();
        assert!(r.settle.lines_comparable());
        let mut p = r.probe(300);
        p.cursor_line = Some(5);
        p.generation = 2;
        assert_eq!(r.step(&p), Some(End::Settled));
        assert!(!r.settle.lines_comparable());
    }

    #[test]
    fn the_alt_screen_aborts_as_full_screen() {
        let mut r = run();
        let mut p = r.probe(10);
        p.alt_screen = true;
        assert_eq!(r.step(&p), Some(End::FullScreen));
    }

    #[test]
    fn a_user_key_aborts_as_user_typed() {
        let mut r = run();
        r.settle.user_key();
        assert_eq!(r.step(&r.probe(10)), Some(End::UserTyped));
    }

    #[test]
    fn the_timeout_is_ten_seconds_after_the_write() {
        let mut r = run();
        let mut p = r.probe(9_999);
        p.cursor_line = Some(5);
        assert_eq!(r.step(&p), None);
        p.now = r.t0 + TIMEOUT;
        assert_eq!(r.step(&p), Some(End::TimedOut));
    }

    #[test]
    fn rule_order_abort_then_timeout_then_settle() {
        let mut r = run();
        // The timeout wins over a run that also satisfies every settle rule.
        assert_eq!(r.step(&r.probe(10_000)), Some(End::TimedOut));
        // The alt screen and a user key win over the timeout.
        let mut p = r.probe(10_000);
        p.alt_screen = true;
        assert_eq!(r.step(&p), Some(End::FullScreen));
        let mut r = run();
        r.settle.user_key();
        assert_eq!(r.step(&r.probe(10_000)), Some(End::UserTyped));
        // The alt screen wins over a user key.
        let mut r = run();
        r.settle.user_key();
        let mut p = r.probe(10);
        p.alt_screen = true;
        assert_eq!(r.step(&p), Some(End::FullScreen));
    }

    #[test]
    fn an_unconfirmed_foreground_at_the_timeout_is_not_still_running() {
        let mut r = run();
        let mut p = r.probe(10_000);
        p.foreground = None;
        assert_eq!(r.step(&p), Some(End::Unconfirmed));
        p.foreground = Some(false);
        assert_eq!(r.step(&p), Some(End::TimedOut), "a program does run");
    }

    #[test]
    fn a_probe_older_than_the_write_does_not_panic_or_end_the_run() {
        let mut r = run();
        let mut p = r.probe(0);
        p.now = r.t0.checked_sub(MS(5)).unwrap();
        assert_eq!(r.step(&p), None);
    }

    // A cursor hidden at the write gives no line to move past: the first
    // visible one is the anchor, and only a later line is progress.
    #[test]
    fn an_unanchored_run_anchors_on_the_first_visible_cursor() {
        let mut r = run();
        r.settle = Settle::new(r.settle.anchor.clone(), r.t0).unanchored();
        let mut p = r.probe(300);
        p.cursor_line = None;
        assert_eq!(r.step(&p), None, "hidden: nothing to compare");
        let mut p = r.probe(400);
        p.cursor_line = Some(9);
        assert_eq!(r.step(&p), None, "this line is the anchor, not progress");
        let mut p = r.probe(800);
        p.cursor_line = Some(10);
        assert_eq!(r.step(&p), Some(End::Settled));
    }

    #[test]
    fn a_run_that_never_showed_a_cursor_cannot_confirm_it_finished() {
        let mut r = run();
        r.settle = Settle::new(r.settle.anchor.clone(), r.t0).unanchored();
        let mut p = r.probe(10_000);
        p.cursor_line = None;
        assert_eq!(r.step(&p), Some(End::Unconfirmed));
        // Anchored late, the plain timeout rules apply again.
        let mut r2 = run();
        r2.settle = Settle::new(r2.settle.anchor.clone(), r2.t0).unanchored();
        let mut p = r2.probe(100);
        p.cursor_line = Some(5);
        r2.step(&p);
        let mut p = r2.probe(10_000);
        p.cursor_line = Some(5);
        assert_eq!(r2.step(&p), Some(End::TimedOut));
    }
}
