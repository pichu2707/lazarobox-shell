//! When does a command that was run in a pane count as finished? Pure: the
//! caller feeds a [`Probe`] on the existing 16 ms tick and an injected clock,
//! so every rule is testable without a pty or a timer (design D22).

use std::{
    ops::RangeInclusive,
    path::PathBuf,
    time::{Duration, Instant},
};

/// Silence after the later of the write and the last output.
pub const QUIET: Duration = Duration::from_millis(250);
/// Longest wait after the write.
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// Quiet, with echo off and a program in front, before the run counts as
/// waiting for input.
pub const WAIT_QUIET: Duration = Duration::from_secs(1);
/// Longest wait for input before the run is handed back.
pub const WAIT_CAP: Duration = Duration::from_secs(300);
/// Fixed cut-off of a model run; the waiting phase pauses it.
pub const CUT_OFF: Duration = Duration::from_secs(30);

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
    /// Whether the terminal's echo is off (a password prompt), when known.
    pub echo_off: Option<bool>,
}

/// Which end rules apply to a run.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Limits {
    /// A command the user typed: 10 s, and any key is `UserTyped`.
    Follow,
    /// A command the model ran: the waiting phase, a 30 s cut-off, and keys
    /// typed to a program do not end it.
    Run,
    /// A run that went on after the cut-off: as `Run`, with no cut-off.
    Continue,
}

/// A key the user sent to the pane during a run.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Key {
    /// The cursor's absolute line at the key; `None` while it was hidden.
    pub line: Option<usize>,
    pub enter: bool,
    /// Ctrl+C or Ctrl+D.
    pub interrupt: bool,
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
    /// 30 s passed and the command still runs (`Limits::Run`).
    StillRunning,
    /// A program waited for input for longer than [`WAIT_CAP`].
    NoInput,
    FullScreen,
    UserTyped,
    PaneGone,
}

/// The model-run state of a [`Settle`].
#[derive(Default)]
struct Ext {
    /// When the current waiting phase began.
    waiting: Option<Instant>,
    waiting_entered: bool,
    attended: bool,
    withheld: bool,
    /// Closed ranges of absolute lines that hold the user's input.
    hidden: Vec<RangeInclusive<usize>>,
    /// The range the user is typing into now.
    open: Option<RangeInclusive<usize>>,
    /// A range an Enter just closed: the next tick may still extend it.
    closing: Option<RangeInclusive<usize>>,
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
    /// A line the cursor must pass as well, when the Enter was pressed on the
    /// last row of a wrapped echo (#819).
    floor: usize,
    limits: Limits,
    /// The foreground at the last step: `None` until the first one.
    foreground: Option<bool>,
    /// The time of the last step, for events that carry no clock.
    seen: Instant,
    /// The cut-off counts from here: the later of the write, the end of the
    /// last waiting phase and the last Enter.
    base: Instant,
    /// What only a model run uses, boxed so a typed follow stays small.
    ext: Box<Ext>,
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
            floor: 0,
            limits: Limits::Follow,
            foreground: None,
            seen: written,
            base: written,
            ext: Box::default(),
        }
    }

    /// A run the model started (`Limits::Run`).
    // Used by the app from slice S1c of chat-runs-commands.
    #[allow(dead_code)]
    pub fn run(anchor: Anchor, written: Instant) -> Self {
        Self::new(anchor, written).limits(Limits::Run)
    }

    /// The end rules this run follows now.
    pub fn rules(&self) -> Limits {
        self.limits
    }

    /// The end rules this run follows.
    #[allow(dead_code)] // S1c/S2c
    pub fn limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Progress means passing `line` too (the row the cursor was on at the
    /// Enter of a command that was typed earlier).
    pub fn with_floor(mut self, line: usize) -> Self {
        self.floor = line;
        self
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
        if self.ext.waiting.take().is_some() {
            self.base = self.base.max(now);
        }
    }

    /// The user sent a key to the pane during the run. In a model run a key
    /// to a program is recorded for redaction and does not end the run; a key
    /// while the shell (or an unknown foreground) owns the pane does.
    pub fn user_key(&mut self, key: Key) {
        if self.limits == Limits::Follow || self.foreground != Some(false) {
            self.typed = true;
            return;
        }
        self.ext.attended = true;
        let Some(line) = key.line else {
            self.ext.withheld = true;
            return;
        };
        self.ext.hidden.extend(self.ext.closing.take());
        let range = self.ext.open.take().map_or(line..=line, |open| {
            (*open.start().min(&line))..=(*open.end().max(&line))
        });
        if key.enter || key.interrupt {
            self.ext.closing = Some(range);
        } else {
            self.ext.open = Some(range);
        }
        if key.enter {
            self.base = self.base.max(self.seen);
        }
    }

    /// Whether the user typed to a program during the run.
    #[allow(dead_code)] // S1c
    pub fn attended(&self) -> bool {
        self.ext.attended
    }

    /// A key was typed while the cursor was hidden: nothing may be read.
    #[allow(dead_code)] // S1c
    pub fn withheld(&self) -> bool {
        self.ext.withheld
    }

    /// The absolute lines that hold what the user typed, closed or not.
    #[allow(dead_code)] // S1c
    pub fn hidden(&self) -> Vec<RangeInclusive<usize>> {
        self.ext
            .hidden
            .iter()
            .cloned()
            .chain(self.ext.closing.clone())
            .chain(self.ext.open.clone())
            .collect()
    }

    /// Whether the run is waiting for input now.
    #[allow(dead_code)] // S1c
    pub fn waiting(&self) -> bool {
        self.ext.waiting.is_some()
    }

    /// `true` once, when the waiting phase began.
    #[allow(dead_code)] // S1c
    pub fn take_waiting(&mut self) -> bool {
        std::mem::take(&mut self.ext.waiting_entered)
    }

    /// Whether the lines read after the run line up with the anchor: false
    /// once the line generation changed (scrollback was dropped or reflowed).
    pub fn lines_comparable(&self) -> bool {
        self.generation == self.anchor.generation
    }

    /// Checks the end rules in order; `None` keeps waiting.
    ///
    /// 1. The alternate screen is `FullScreen`; a user key is `UserTyped`
    ///    (in a model run, only a key typed at the shell).
    /// 2. 10 s since the write is `TimedOut` (`Unconfirmed` when the foreground
    ///    is unknown or no cursor line was ever seen), even when the rest holds.
    /// 3. `Settled` needs ALL of: the foreground is the shell (`None` waits,
    ///    which fails closed); 250 ms of quiet; and progress, meaning the
    ///    cursor moved to a later line, the real cwd changed, or the line
    ///    generation changed (then the lines are not comparable).
    /// 4. A model run then looks for the waiting phase (see `WAIT_QUIET`),
    ///    ends it as `NoInput` after `WAIT_CAP`, and otherwise ends it as
    ///    `StillRunning` after `CUT_OFF`, a clock the waiting phase pauses.
    pub fn step(&mut self, probe: &Probe) -> Option<End> {
        self.generation = probe.generation;
        self.foreground = probe.foreground;
        self.seen = probe.now;
        if let Some(line) = probe.cursor_line {
            if let Some(open) = self.ext.open.as_mut() {
                *open = *open.start()..=*open.end().max(&line);
            }
            if let Some(c) = self.ext.closing.take() {
                self.ext.hidden.push(*c.start()..=*c.end().max(&line));
            }
        }
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
        if self.limits == Limits::Follow && since(self.written) >= TIMEOUT {
            return Some(if probe.foreground.is_none() || !self.anchored {
                End::Unconfirmed
            } else {
                End::TimedOut
            });
        }
        let progress = probe
            .cursor_line
            .is_some_and(|l| l > self.anchor.line.max(self.floor))
            || probe.cwd.as_ref().is_some_and(|c| *c != self.anchor.cwd)
            || !self.lines_comparable();
        if probe.foreground == Some(true) && since(self.last) >= QUIET && progress {
            return Some(End::Settled);
        }
        if self.limits == Limits::Follow {
            return None;
        }
        self.wait_or_cut(probe)
    }

    /// Steps 4 to 6 of a model run: the waiting phase, its cap, the cut-off.
    fn wait_or_cut(&mut self, probe: &Probe) -> Option<End> {
        let since = |from: Instant| probe.now.saturating_duration_since(from);
        let asks = probe.foreground == Some(false) && probe.echo_off == Some(true);
        // An unknown foreground or echo (`None`) never ends a wait.
        let over = probe.foreground == Some(true) || probe.echo_off == Some(false);
        if self.ext.waiting.is_some() && over {
            // Echo came back on or the shell is back.
            self.ext.waiting = None;
            self.base = self.base.max(probe.now);
        } else if self.ext.waiting.is_none() && asks && since(self.last) >= WAIT_QUIET {
            self.ext.waiting = Some(probe.now);
            self.ext.waiting_entered = true;
        }
        if let Some(since_wait) = self.ext.waiting.map(since) {
            return (since_wait >= WAIT_CAP).then_some(End::NoInput);
        }
        (self.limits == Limits::Run && since(self.base) >= CUT_OFF).then(|| {
            if probe.foreground.is_none() || !self.anchored {
                End::Unconfirmed
            } else {
                End::StillRunning
            }
        })
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
                echo_off: None,
            }
        }

        fn step(&mut self, probe: &Probe) -> Option<End> {
            self.settle.step(probe)
        }
    }

    #[test]
    fn a_floor_makes_progress_mean_passing_it() {
        let t0 = Instant::now();
        let anchor = Anchor {
            line: 5,
            col: 12,
            generation: 1,
            cwd: "/home/u".into(),
        };
        let mut settle = Settle::new(anchor, t0).with_floor(7);
        let mut probe = Probe {
            now: t0 + MS(400),
            foreground: Some(true),
            cwd: Some("/home/u".into()),
            cursor_line: Some(7),
            generation: 1,
            alt_screen: false,
            echo_off: None,
        };
        assert_eq!(settle.step(&probe), None, "still on the floor row");
        probe.cursor_line = Some(8);
        assert_eq!(settle.step(&probe), Some(End::Settled));
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
        r.settle.user_key(Key::default());
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
        r.settle.user_key(Key::default());
        assert_eq!(r.step(&r.probe(10_000)), Some(End::UserTyped));
        // The alt screen wins over a user key.
        let mut r = run();
        r.settle.user_key(Key::default());
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

    // --- Limits::Run / Limits::Continue (chat-runs-commands S1a) ---

    fn model_run() -> Run {
        let mut r = run();
        r.settle = Settle::run(r.settle.anchor().unwrap().clone(), r.t0);
        r
    }

    /// A program owns the foreground, `ms` after the write.
    fn program(r: &Run, ms: u64, echo_off: Option<bool>) -> Probe {
        let mut p = r.probe(ms);
        p.foreground = Some(false);
        p.cursor_line = Some(5);
        p.echo_off = echo_off;
        p
    }

    const KEY: Key = Key {
        line: Some(7),
        enter: false,
        interrupt: false,
    };

    #[test]
    fn follow_never_waits_and_any_key_is_user_typed() {
        let mut r = run();
        assert_eq!(r.step(&program(&r, 5_000, Some(true))), None);
        assert!(!r.settle.take_waiting(), "Follow has no waiting phase");
        r.settle.user_key(KEY);
        assert_eq!(
            r.step(&program(&r, 5_001, Some(true))),
            Some(End::UserTyped)
        );
        let mut r = run();
        assert_eq!(r.step(&program(&r, 10_000, None)), Some(End::TimedOut));
    }

    #[test]
    fn a_model_run_is_cut_off_at_thirty_seconds_not_ten() {
        let mut r = model_run();
        assert_eq!(r.step(&program(&r, 10_000, Some(false))), None);
        assert_eq!(r.step(&program(&r, 29_999, Some(false))), None);
        assert_eq!(
            r.step(&program(&r, 30_000, Some(false))),
            Some(End::StillRunning)
        );
    }

    #[test]
    fn the_cut_off_is_unconfirmed_when_the_foreground_or_the_anchor_is_unknown() {
        let mut r = model_run();
        let mut p = program(&r, 30_000, None);
        p.foreground = None;
        assert_eq!(r.step(&p), Some(End::Unconfirmed));
        let mut r = model_run();
        r.settle = Settle::run(r.settle.anchor.clone(), r.t0).unanchored();
        let mut p = program(&r, 30_000, None);
        p.cursor_line = None;
        assert_eq!(r.step(&p), Some(End::Unconfirmed));
    }

    #[test]
    fn a_program_waits_only_after_a_second_of_quiet_with_echo_off() {
        let mut r = model_run();
        assert_eq!(r.step(&program(&r, 999, Some(true))), None);
        assert!(!r.settle.take_waiting(), "not quiet for a second yet");
        assert_eq!(r.step(&program(&r, 1_000, Some(true))), None);
        assert!(r.settle.waiting());
        assert!(r.settle.take_waiting(), "true once, on entry");
        assert_eq!(r.step(&program(&r, 1_016, Some(true))), None);
        assert!(!r.settle.take_waiting());
    }

    #[test]
    fn echo_on_or_unknown_or_a_shell_foreground_never_waits() {
        for echo_off in [Some(false), None] {
            let mut r = model_run();
            assert_eq!(r.step(&program(&r, 20_000, echo_off)), None, "sleep 40");
            assert!(!r.settle.waiting());
        }
        let mut r = model_run();
        let mut p = program(&r, 2_000, Some(true));
        p.foreground = Some(true);
        p.cursor_line = Some(5);
        assert_eq!(r.step(&p), None);
        assert!(!r.settle.waiting(), "the shell owns the foreground");
        let mut r = model_run();
        let mut p = program(&r, 2_000, Some(true));
        p.alt_screen = true;
        assert_eq!(
            r.step(&p),
            Some(End::FullScreen),
            "no waiting on the alt screen"
        );
    }

    #[test]
    fn waiting_suspends_the_cut_off_and_output_resumes_it() {
        let mut r = model_run();
        r.step(&program(&r, 1_000, Some(true)));
        assert_eq!(r.step(&program(&r, 31_000, Some(true))), None);
        assert_eq!(r.step(&program(&r, 120_000, Some(true))), None);
        r.settle.output(r.t0 + MS(121_000));
        assert_eq!(r.step(&program(&r, 121_016, Some(false))), None);
        assert!(!r.settle.waiting(), "output ends the phase");
        assert_eq!(r.step(&program(&r, 150_999, Some(false))), None);
        assert_eq!(
            r.step(&program(&r, 151_000, Some(false))),
            Some(End::StillRunning)
        );
    }

    #[test]
    fn echo_coming_back_on_ends_the_phase_and_restarts_the_cut_off() {
        let mut r = model_run();
        r.step(&program(&r, 1_000, Some(true)));
        assert_eq!(r.step(&program(&r, 40_000, Some(false))), None);
        assert!(!r.settle.waiting());
        assert_eq!(r.step(&program(&r, 69_999, Some(false))), None);
        assert_eq!(
            r.step(&program(&r, 70_000, Some(false))),
            Some(End::StillRunning)
        );
    }

    #[test]
    fn a_wait_longer_than_five_minutes_ends_as_no_input() {
        let mut r = model_run();
        r.step(&program(&r, 1_000, Some(true)));
        assert_eq!(r.step(&program(&r, 300_999, Some(true))), None);
        assert_eq!(
            r.step(&program(&r, 301_000, Some(true))),
            Some(End::NoInput)
        );
    }

    #[test]
    fn an_enter_typed_to_the_program_restarts_the_cut_off() {
        let mut r = model_run();
        assert_eq!(r.step(&program(&r, 20_000, Some(false))), None);
        r.settle.user_key(Key { enter: true, ..KEY });
        assert_eq!(r.step(&program(&r, 49_999, Some(false))), None);
        assert_eq!(
            r.step(&program(&r, 50_000, Some(false))),
            Some(End::StillRunning)
        );
    }

    #[test]
    fn a_key_to_a_program_attends_the_run_and_a_key_at_the_shell_ends_it() {
        let mut r = model_run();
        r.step(&program(&r, 100, Some(true)));
        r.settle.user_key(KEY);
        assert!(r.settle.attended());
        assert_eq!(
            r.step(&program(&r, 200, Some(true))),
            None,
            "the run goes on"
        );
        // The program ends, the prompt returns and the run settles as usual.
        assert_eq!(r.step(&r.probe(900)), Some(End::Settled));
        let mut r = model_run();
        r.step(&r.probe(100));
        r.settle.user_key(KEY);
        assert_eq!(
            r.step(&r.probe(110)),
            Some(End::UserTyped),
            "the shell owned it"
        );
        let mut r = model_run();
        r.settle.user_key(KEY);
        assert_eq!(
            r.step(&program(&r, 10, None)),
            Some(End::UserTyped),
            "unknown foreground"
        );
    }

    #[test]
    fn typed_keys_extend_one_range_that_enter_closes() {
        let mut r = model_run();
        r.step(&program(&r, 100, Some(true)));
        r.settle.user_key(Key {
            line: Some(7),
            ..KEY
        });
        r.settle.user_key(Key {
            line: Some(8),
            ..KEY
        });
        let mut p = program(&r, 200, Some(true));
        p.cursor_line = Some(9);
        r.step(&p);
        assert_eq!(r.settle.hidden(), vec![7..=9], "a wrapped echo is covered");
        r.settle.user_key(Key {
            line: Some(9),
            enter: true,
            ..KEY
        });
        p.cursor_line = Some(9);
        r.step(&p);
        p.cursor_line = Some(12);
        r.step(&p);
        assert_eq!(r.settle.hidden(), vec![7..=9], "output below stays visible");
        r.settle.user_key(Key {
            line: Some(12),
            interrupt: true,
            ..KEY
        });
        r.settle.user_key(Key {
            line: Some(13),
            ..KEY
        });
        assert_eq!(r.settle.hidden(), vec![7..=9, 12..=12, 13..=13]);
        assert!(!r.settle.withheld());
    }

    #[test]
    fn a_key_with_a_hidden_cursor_withholds_the_output() {
        let mut r = model_run();
        r.step(&program(&r, 100, Some(true)));
        assert!(!r.settle.withheld());
        r.settle.user_key(Key { line: None, ..KEY });
        assert!(r.settle.withheld());
    }

    #[test]
    fn model_run_order_alt_typed_settled_wait_cap_cut_off() {
        // alt screen over a key at the shell
        let mut r = model_run();
        r.step(&r.probe(100));
        r.settle.user_key(KEY);
        let mut p = r.probe(200);
        p.alt_screen = true;
        assert_eq!(r.step(&p), Some(End::FullScreen));
        // settled over the cut-off
        let mut r = model_run();
        assert_eq!(r.step(&r.probe(30_000)), Some(End::Settled));
        // typed over settled
        let mut r = model_run();
        r.step(&r.probe(100));
        r.settle.user_key(KEY);
        assert_eq!(r.step(&r.probe(30_000)), Some(End::UserTyped));
        // the cap wins over the suspended cut-off
        let mut r = model_run();
        r.step(&program(&r, 1_000, Some(true)));
        assert_eq!(
            r.step(&program(&r, 400_000, Some(true))),
            Some(End::NoInput)
        );
    }

    #[test]
    fn a_continuation_has_no_cut_off() {
        let mut r = model_run();
        r.settle = Settle::run(r.settle.anchor.clone(), r.t0).limits(Limits::Continue);
        assert_eq!(r.step(&program(&r, 600_000, Some(false))), None);
        assert_eq!(r.step(&r.probe(600_500)), Some(End::Settled));
    }

    #[test]
    fn a_wrapped_echo_after_the_enter_stays_hidden_until_the_next_tick() {
        let mut r = model_run();
        r.step(&program(&r, 100, Some(true)));
        r.settle.user_key(Key {
            line: Some(7),
            enter: true,
            ..KEY
        });
        let mut p = program(&r, 116, Some(true));
        p.cursor_line = Some(8);
        r.step(&p);
        assert_eq!(r.settle.hidden(), vec![7..=8]);
        p.cursor_line = Some(11);
        r.step(&p);
        assert_eq!(r.settle.hidden(), vec![7..=8], "closed at the first tick");
    }

    #[test]
    fn an_unknown_probe_does_not_end_the_wait_or_show_it_twice() {
        let mut r = model_run();
        r.step(&program(&r, 1_000, Some(true)));
        assert!(r.settle.take_waiting());
        let mut p = program(&r, 100_000, Some(true));
        p.foreground = None;
        assert_eq!(r.step(&p), None);
        assert!(r.settle.waiting(), "an unknown foreground keeps the wait");
        let mut p = program(&r, 100_016, None);
        p.foreground = Some(false);
        assert_eq!(r.step(&p), None);
        assert!(r.settle.waiting(), "an unknown echo keeps the wait");
        r.step(&program(&r, 100_032, Some(true)));
        assert!(!r.settle.take_waiting(), "the line is shown once");
    }

    #[test]
    fn a_flapping_probe_does_not_restart_the_wait_cap() {
        let mut r = model_run();
        r.step(&program(&r, 1_000, Some(true)));
        r.step(&program(&r, 200_000, None));
        r.step(&program(&r, 200_016, Some(true)));
        assert_eq!(r.step(&program(&r, 300_999, Some(true))), None);
        assert_eq!(
            r.step(&program(&r, 301_000, Some(true))),
            Some(End::NoInput)
        );
    }
}
