//! What the wait line of a turn says it waits for, and for how long.
//!
//! The runtime clock of a server turn (`runtime::loopback::Wait`) sends one
//! `Slow` event once the first-byte clock has run [`SLOW_AFTER`] without a
//! byte, held time left out. The seconds drawn come from that event, never
//! from the moment of the send: the send may be followed by a model start or
//! a retry the app does not see, but `Slow` is the clock's own word.

use std::{borrow::Cow, time::Duration};

use std::time::Instant;

use super::{
    ChatState, Entry, Phase, STILL_WAITING, WAITING, adapters::is_cli_engine, approval::Origin,
};

/// No byte of a reply after this long and the server clock says `Slow`.
pub const SLOW_AFTER: Duration = Duration::from_secs(3);
/// How long a request waits for its first byte before the turn gives up. The
/// runtime reads this very value for every engine.
pub const FIRST_BYTE_LIMIT: Duration = Duration::from_secs(120);

/// Once the reply has started, a server turn gives up after this long without
/// a read. The runtime reads this very value for the server engines.
pub const IDLE_LIMIT: Duration = Duration::from_secs(60);

/// A user answer holds the turn.
pub const WAITING_USER: &str = "waiting for your answer…";

/// The suffix of the wait line, as long as `room` columns allow: the seconds
/// and the limit, or the seconds alone, or nothing.
pub fn suffix(elapsed: Duration, limit: Duration, room: usize) -> Option<String> {
    let secs = elapsed.as_secs();
    let full = format!(" {secs}s (gives up at {}s)", limit.as_secs());
    let short = format!(" {secs}s");
    [full, short]
        .into_iter()
        .find(|text| text.chars().count() <= room)
}

impl ChatState {
    /// The wait line: what the turn waits for while no byte of the answer has
    /// arrived. An answer of the user comes first, then a tool that runs,
    /// then the model.
    pub fn waiting_text(&self) -> Option<Cow<'static, str>> {
        let Phase::Waiting { slow } = self.phase else {
            return None;
        };
        // Reasoning counts as the first byte: its block replaces the text.
        if self.live_reasoning().is_some() {
            return None;
        }
        if self.approvals.pending().is_some() {
            return Some(WAITING_USER.into());
        }
        if let Some(name) = self.running_tool() {
            return Some(format!("running {name}…").into());
        }
        Some(if slow { STILL_WAITING } else { WAITING }.into())
    }

    /// The name of a tool call that has no result yet.
    fn running_tool(&self) -> Option<&str> {
        self.entries.iter().rev().find_map(|entry| match entry {
            Entry::Tool(row) if row.result().is_none() => Some(row.name()),
            _ => None,
        })
    }

    /// Seconds on the wait line: only a server turn whose clock said `Slow`,
    /// and only while the line waits for the model. A CLI is left out: its
    /// idle clock restarts at every read, which the app cannot see.
    pub fn wait_elapsed(&self, now: Instant) -> Option<Duration> {
        let waits_for_model = matches!(self.phase, Phase::Waiting { slow: true })
            && self.waiting_text().as_deref() == Some(STILL_WAITING);
        if !waits_for_model || is_cli_engine(self.wait_engine()) || self.held_since.is_some() {
            return None;
        }
        self.wait_since
            .map(|since| now.saturating_duration_since(since))
    }

    /// The engine the request in flight went to. A summary run does not set
    /// `request_engine` (the usage, the auto-run scope and the llama.cpp
    /// restart check read it as the engine of a chat turn), so its engine is
    /// the one of its side run.
    fn wait_engine(&self) -> &str {
        match (&self.summarizing, &self.side_run) {
            (Some(_), Some(run)) => run.engine(),
            _ => &self.request_engine,
        }
    }

    /// The limit the wait line states: the first-byte one, or, once the
    /// runtime said a read restarted its idle clock, the seconds from the
    /// anchor at which that clock gives up.
    pub fn wait_limit(&self) -> Duration {
        match (self.idle_from, self.wait_since) {
            (Some(read), Some(since)) => read.saturating_duration_since(since) + IDLE_LIMIT,
            _ => FIRST_BYTE_LIMIT,
        }
    }

    /// The runtime read part of the reply at `now`, and the model has still
    /// said nothing: its idle clock restarted. Only after `Slow`, and only
    /// while the line waits for the model; the seconds' anchor is untouched.
    pub(super) fn clock_read(&mut self, now: Instant) {
        if matches!(self.phase, Phase::Waiting { slow: true }) && self.wait_since.is_some() {
            self.idle_from = Some(now);
        }
    }

    /// The clock said `Slow` at `now`: it started [`SLOW_AFTER`] before.
    pub(super) fn clock_said_slow(&mut self, now: Instant) {
        self.wait_since = Some(now.checked_sub(SLOW_AFTER).unwrap_or(now));
        self.held_since = None;
        self.idle_from = None;
    }

    /// The clock of the request is over or starts again: forget its start.
    pub(super) fn clock_reset(&mut self) {
        self.wait_since = None;
        self.held_since = None;
        self.idle_from = None;
    }

    /// Follows the pauses of the clock: a tool call that waits for the user
    /// stops it (`runtime::tools::pending`), and that time is not counted.
    /// An approval the user started (`Origin::Block`) does not stop it.
    /// Called on every tick.
    pub fn observe_hold(&mut self, now: Instant) {
        let Some(since) = self.wait_since.as_mut() else {
            return;
        };
        let held = self
            .approvals
            .pending()
            .is_some_and(|a| a.origin() == Origin::Tool);
        match (held, self.held_since) {
            (true, None) => self.held_since = Some(now),
            (false, Some(from)) => {
                *since += now.saturating_duration_since(from);
                self.held_since = None;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
