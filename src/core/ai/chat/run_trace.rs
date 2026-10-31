//! The worker transcript of the run view (agent-orchestrator S9): what each
//! worker of the pane's run streamed and which tools it called, kept bounded
//! and sanitized, apart from the chat entries (a worker transcript is never
//! stored in the history nor sent back as history).

use std::collections::VecDeque;

use super::{
    ChatState, Entry,
    engine::{CallId, ToolStatus},
    transcript::{Line, LineKind, TOOL_PREFIX, assistant_rows, push_wrapped, worker_line},
};
use crate::core::ai::orchestra::{RunId, WorkerId, event::TraceItem};
use crate::core::ai::sanitize::clean;

/// The most bytes kept of one worker; the oldest go first.
pub const TRACE_CAP: usize = 128 * 1024;
/// The most workers whose transcript is kept at once.
pub const MAX_TRACED: usize = 16;
/// How far past a cut a text looks for the start of a row, in bytes.
const ROW_NEAR: usize = 1024;
/// The most bytes of a tool call row or of its result.
pub const ROW_CAP: usize = 512;
/// The row that says the start of a transcript was dropped.
pub const CUT_LINE: &str = "(earlier output dropped)";
/// What an empty transcript shows.
pub const NO_OUTPUT: &str = "(no output yet)";

#[derive(Clone, PartialEq, Eq, Debug)]
enum Part {
    Text(String),
    Tool {
        id: CallId,
        call: String,
        result: Option<(ToolStatus, String)>,
    },
}

/// One worker's transcript.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
struct WorkerTrace {
    parts: VecDeque<Part>,
    bytes: usize,
    cut: bool,
}

/// The transcripts of the pane's run and the view that is open on one.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Traces {
    kept: Vec<((RunId, WorkerId), WorkerTrace)>,
}

impl WorkerTrace {
    fn push(&mut self, item: TraceItem) {
        match item {
            TraceItem::Begin => *self = Self::default(),
            TraceItem::Text(piece) => {
                let piece = clean(&piece);
                self.bytes += piece.len();
                match self.parts.back_mut() {
                    Some(Part::Text(text)) => text.push_str(&piece),
                    _ => self.parts.push_back(Part::Text(piece)),
                }
            }
            TraceItem::Call { id, name, summary } => {
                let call = format!("{} {}", one_line(&name), one_line(&summary));
                let call = clip(call.trim());
                self.bytes += call.len();
                self.parts.push_back(Part::Tool {
                    id,
                    call,
                    result: None,
                });
            }
            TraceItem::Result {
                id,
                status,
                summary,
            } => {
                let summary = clip(&one_line(&summary));
                let open = self.parts.iter_mut().rev().find_map(|part| match part {
                    Part::Tool { id: at, result, .. } if *at == id && result.is_none() => {
                        Some(result)
                    }
                    _ => None,
                });
                // A result for a call that has no row is ignored, as in the
                // chat transcript.
                if let Some(result) = open {
                    self.bytes += summary.len();
                    *result = Some((status, summary));
                }
            }
        }
        self.trim();
    }

    /// Past [`TRACE_CAP`], drops the oldest output down to seven eighths of
    /// it (so the next pieces do not trim again), and only the excess: a
    /// part is popped whole only when it fits in what is to go, a text is cut
    /// on a character boundary, then to the next row start if one is near.
    fn trim(&mut self) {
        if self.bytes <= TRACE_CAP {
            return;
        }
        self.cut = true;
        let mut excess = self.bytes - (TRACE_CAP - TRACE_CAP / 8);
        while excess > 0 {
            let Some(front) = self.parts.front_mut() else {
                break;
            };
            let size = part_bytes(front);
            if let Part::Text(text) = front
                && size > excess
            {
                let mut at = excess;
                while !text.is_char_boundary(at) {
                    at += 1;
                }
                let mut near = text.as_bytes()[at..].iter().take(ROW_NEAR);
                if let Some(nl) = near.position(|b| *b == b'\n') {
                    at += nl + 1;
                }
                text.drain(..at);
                self.bytes -= at;
                break;
            }
            self.bytes -= size;
            excess = excess.saturating_sub(size);
            self.parts.pop_front();
        }
    }

    fn rows(&self, width: usize) -> Vec<Line> {
        let mut out = Vec::new();
        if self.cut {
            push_wrapped(&mut out, LineKind::Mark, "", "", CUT_LINE, width);
        }
        for part in &self.parts {
            match part {
                Part::Text(text) => {
                    let text = text.trim_matches('\n');
                    if !text.is_empty() {
                        assistant_rows(text, width, false, &mut out);
                    }
                }
                Part::Tool { call, result, .. } => {
                    push_wrapped(&mut out, LineKind::Tool, TOOL_PREFIX, "  ", call, width);
                    if let Some((status, summary)) = result {
                        let kind = match status {
                            ToolStatus::Ok => LineKind::Tool,
                            ToolStatus::Error | ToolStatus::Denied => LineKind::ToolAlert,
                        };
                        let text = if summary.is_empty() {
                            status.as_str().to_owned()
                        } else {
                            format!("{}: {summary}", status.as_str())
                        };
                        push_wrapped(&mut out, kind, "  ", "    ", &text, width);
                    }
                }
            }
        }
        out
    }
}

fn part_bytes(part: &Part) -> usize {
    match part {
        Part::Text(text) => text.len(),
        Part::Tool { call, result, .. } => {
            call.len() + result.as_ref().map_or(0, |(_, text)| text.len())
        }
    }
}

/// `text` cut to [`ROW_CAP`] bytes on a character boundary.
fn clip(text: &str) -> String {
    let mut end = text.len().min(ROW_CAP);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// Worker text on one clean line.
fn one_line(text: &str) -> String {
    clean(text).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The view on a worker: where it is scrolled.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RunView {
    pub run: RunId,
    pub worker: WorkerId,
    /// The first row shown; `None` follows the end.
    top: Option<usize>,
}

/// How a key moves the view.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scroll {
    Up(usize),
    Down(usize),
    Top,
    Follow,
}

impl ChatState {
    /// A step of worker `worker` of `run`. Only the pane's run is kept (a
    /// step that comes after its run ended is dropped), and only the last
    /// [`MAX_TRACED`] workers.
    pub fn trace(&mut self, run: RunId, worker: WorkerId, item: TraceItem) {
        if self.orch_run != Some(run) {
            return;
        }
        let key = (run, worker);
        let at = match self.traces.kept.iter().position(|(k, _)| *k == key) {
            Some(at) => at,
            None => {
                // A first step that is not `Begin` means the start of this
                // worker's transcript went with an evicted one.
                let cut = !matches!(item, TraceItem::Begin);
                let fresh = WorkerTrace {
                    cut,
                    ..WorkerTrace::default()
                };
                self.traces.kept.push((key, fresh));
                self.traces.kept.len() - 1
            }
        };
        self.traces.kept[at].1.push(item);
        let open = self.run_view.map(|view| (view.run, view.worker));
        if self.traces.kept.len() > MAX_TRACED
            && let Some(old) = self.traces.kept.iter().position(|(k, _)| Some(*k) != open)
        {
            self.traces.kept.remove(old);
        }
    }

    /// Drops the transcripts of runs that are neither the pane's nor the one
    /// on view.
    pub(super) fn prune_traces(&mut self) {
        let (run, view) = (self.orch_run, self.run_view.map(|view| view.run));
        self.traces
            .kept
            .retain(|((r, _), _)| Some(*r) == run || Some(*r) == view);
    }

    /// Opens the view on `worker` of the pane's run; `false` when the pane
    /// has no run.
    pub fn open_run_view(&mut self, worker: WorkerId) -> bool {
        let Some(run) = self.orch_run else {
            return false;
        };
        self.run_view = Some(RunView {
            run,
            worker,
            top: None,
        });
        true
    }

    pub fn close_run_view(&mut self) {
        self.run_view = None;
        self.prune_traces();
    }

    #[cfg(test)]
    pub fn has_trace(&self, run: RunId, worker: WorkerId) -> bool {
        self.traces.kept.iter().any(|(k, _)| *k == (run, worker))
    }

    pub fn run_view(&self) -> Option<RunView> {
        self.run_view
    }

    /// The view's title row: the worker and its status.
    pub fn run_view_head(&self) -> Option<String> {
        let view = self.run_view?;
        let row = self.entries.iter().find_map(|entry| match entry {
            Entry::Worker(row) if (row.run, row.id) == (view.run, view.worker) => Some(row),
            _ => None,
        });
        Some(match row {
            Some(row) => format!("worker {} \u{b7} {}", view.worker.0, worker_line(row)),
            None => format!("worker {}", view.worker.0),
        })
    }

    /// The transcript rows of the open view at `width` columns.
    pub fn run_view_rows(&self, width: usize) -> Vec<Line> {
        let Some(view) = self.run_view else {
            return Vec::new();
        };
        let mut rows = (self.traces.kept.iter())
            .find(|(k, _)| *k == (view.run, view.worker))
            .map_or_else(Vec::new, |(_, trace)| trace.rows(width));
        if rows.is_empty() {
            push_wrapped(&mut rows, LineKind::System, "", "", NO_OUTPUT, width);
        }
        rows
    }

    /// The first row shown of `total` in `rows` rows.
    pub fn run_view_top(&self, total: usize, rows: usize) -> usize {
        let last = total.saturating_sub(rows);
        self.run_view
            .and_then(|view| view.top)
            .map_or(last, |top| top.min(last))
    }

    pub fn run_view_scroll(&mut self, by: Scroll, total: usize, rows: usize) {
        let now = self.run_view_top(total, rows);
        let last = total.saturating_sub(rows);
        let next = match by {
            Scroll::Up(n) => now.saturating_sub(n),
            Scroll::Down(n) => now.saturating_add(n),
            Scroll::Top => 0,
            Scroll::Follow => last,
        };
        if let Some(view) = &mut self.run_view {
            view.top = (next < last).then_some(next);
        }
    }
}
