//! The transcript of a chat: entries turned into wrapped, classified lines,
//! and the scroll position over them. Pure: the widget only paints.

use std::{cell::RefCell, sync::Arc};

use super::{
    ChatState, Entry, ReasoningState,
    engine::{Role, ToolStatus},
    wrap::{expand_tabs, text_width, wrap_ranges, wrap_words},
    yank,
};

/// What a transcript line is, which decides its style.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineKind {
    User,
    Assistant,
    /// Inside a fenced block, fences included.
    Code,
    System,
    /// `waiting for the model…` / `still waiting for the model…`.
    Waiting,
    /// `(cancelled)` and the like, after a partial answer.
    Mark,
    /// Reasoning text behind the gutter.
    Reasoning,
    /// `▸ reasoning (N.Ns)` / `▾ reasoning (N.Ns)`.
    ReasoningHead,
    /// A tool call row, and the result row of one that went well.
    Tool,
    /// The result row of a tool call that failed or was denied.
    ToolAlert,
    /// The line under the last shell block that names the run binding. It is
    /// drawn, never stored, yanked or sent.
    Hint,
    Blank,
}

/// One row of the transcript, as wide as the pane at most.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Line {
    pub kind: LineKind,
    pub text: String,
    /// Display columns of the drawing prefix that `text` starts with
    /// (`› `, `┊ `, `• `, `» ` or the indent of a continuation row).
    pub lead: usize,
    /// How the row ends: a logical line ends here or it goes on in the next
    /// row.
    pub wrap: Wrap,
}

/// How a transcript row ends.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Wrap {
    /// The logical line ends here.
    #[default]
    End,
    /// Soft-wrapped: the next row goes on with nothing in between.
    Cut,
    /// Soft-wrapped at a space the wrap dropped.
    Space,
}

/// The hint under the last shell block of the last answer (tools on).
pub const RUN_HINT: &str = "Ctrl+Space e  run in pane";

/// The gutter in front of every reasoning row.
pub const GUTTER: &str = "┊ ";
/// In front of the first row of a user message.
pub const USER_PREFIX: &str = "› ";
/// In front of the first row of a tool call.
pub const TOOL_PREFIX: &str = "» ";
/// In front of a system line.
pub const SYSTEM_PREFIX: &str = "• ";

/// The rows of the transcript of `chat` at `width` columns.
pub fn build(chat: &ChatState, width: usize) -> Vec<Line> {
    layout(chat, width).0
}

/// The rows of the transcript and, per entry, the index of its first row (an
/// entry with no rows starts where the next one does).
pub fn layout(chat: &ChatState, width: usize) -> (Vec<Line>, Vec<usize>) {
    let mut out: Vec<Line> = Vec::new();
    let mut starts: Vec<usize> = Vec::new();
    let mut after_reasoning = false;
    // The hint goes under the block the run action would offer: the last
    // message, when it holds a runnable shell block.
    let hinted = (chat.run_hint() && yank::last_shell_block(chat).is_some())
        .then(|| {
            chat.entries()
                .iter()
                .rposition(|e| matches!(e, Entry::Message(_)))
        })
        .flatten();
    for (index, entry) in chat.entries().iter().enumerate() {
        let mut rows = Vec::new();
        entry_rows(entry, width, hinted == Some(index), &mut rows);
        if rows.is_empty() {
            starts.push(out.len());
            continue;
        }
        // An answer sits right under the reasoning that came before it.
        let answers = matches!(entry, Entry::Message(m) if m.role == Role::Assistant);
        let glued = after_reasoning && answers;
        if !out.is_empty() && !glued {
            out.push(blank());
        }
        starts.push(out.len());
        out.extend(rows);
        after_reasoning = matches!(entry, Entry::Reasoning(_));
    }
    if let Some(text) = chat.waiting_text() {
        if !out.is_empty() {
            out.push(blank());
        }
        push_wrapped(&mut out, LineKind::Waiting, "", "", text, width);
    }
    (out, starts)
}

fn blank() -> Line {
    Line {
        kind: LineKind::Blank,
        text: String::new(),
        lead: 0,
        wrap: Wrap::End,
    }
}

/// The rows of one entry.
fn entry_rows(entry: &Entry, width: usize, hint: bool, out: &mut Vec<Line>) {
    match entry {
        Entry::System(text) => {
            push_wrapped(out, LineKind::System, SYSTEM_PREFIX, "  ", text, width);
        }
        Entry::Message(msg) if msg.role == Role::User => {
            push_wrapped(out, LineKind::User, USER_PREFIX, "  ", &msg.text, width);
        }
        Entry::Message(msg) => {
            assistant_rows(&msg.text, width, hint, out);
            if msg.status.is_partial() {
                let mark = format!("({})", msg.status.as_str());
                push_wrapped(out, LineKind::Mark, "", "", &mark, width);
            }
        }
        Entry::Memory(line) => {
            push_wrapped(out, LineKind::ReasoningHead, "", "", &line.head(), width);
            if line.expanded() {
                push_wrapped(out, LineKind::Reasoning, GUTTER, GUTTER, line.text(), width);
            }
        }
        Entry::Tool(row) => {
            let call = format!("{} {}", row.name(), row.summary());
            push_wrapped(
                out,
                LineKind::Tool,
                TOOL_PREFIX,
                "  ",
                call.trim_end(),
                width,
            );
            if let Some((status, summary)) = row.result() {
                let kind = match status {
                    ToolStatus::Ok => LineKind::Tool,
                    ToolStatus::Error | ToolStatus::Denied => LineKind::ToolAlert,
                };
                let text = if summary.is_empty() {
                    status.as_str().to_owned()
                } else {
                    format!("{}: {summary}", status.as_str())
                };
                push_wrapped(out, kind, "  ", "    ", &text, width);
            }
        }
        Entry::Reasoning(block) => {
            let head = |glyph: char| format!("{glyph} reasoning ({})", block.seconds_label());
            match block.state() {
                ReasoningState::Streaming => {}
                ReasoningState::Collapsed => {
                    push_wrapped(out, LineKind::ReasoningHead, "", "", &head('▸'), width);
                    return;
                }
                ReasoningState::Expanded => {
                    push_wrapped(out, LineKind::ReasoningHead, "", "", &head('▾'), width);
                }
            }
            if !block.text().is_empty() {
                push_wrapped(
                    out,
                    LineKind::Reasoning,
                    GUTTER,
                    GUTTER,
                    block.text(),
                    width,
                );
            }
        }
    }
}

/// An answer: plain text with fenced blocks drawn as code. An unclosed fence
/// makes the rest of the message code. The fence lines are code too, shown as
/// they are.
pub(super) fn assistant_rows(text: &str, width: usize, hint: bool, out: &mut Vec<Line>) {
    let mut in_code = false;
    let mut after_block = None;
    for line in text.split('\n') {
        let fence = is_fence(line, in_code);
        let kind = if fence || in_code {
            LineKind::Code
        } else {
            LineKind::Assistant
        };
        if fence {
            in_code = !in_code;
        }
        push_wrapped(out, kind, "", "", line, width);
        if fence && !in_code {
            after_block = Some(out.len());
        }
    }
    // The hint goes under the last block that closed.
    if hint && let Some(at) = after_block {
        let mut rows = Vec::new();
        push_wrapped(&mut rows, LineKind::Hint, "", "", RUN_HINT, width);
        out.splice(at..at, rows);
    }
}

/// Whether `line` is a fence. One that closes a block is any line starting
/// with three backticks. One that opens a block must have no backtick after
/// its opening run (CommonMark), so inline code like ```a``` is prose.
pub(super) fn is_fence(line: &str, in_code: bool) -> bool {
    let Some(rest) = line.trim_start().strip_prefix("```") else {
        return false;
    };
    in_code || !rest.trim_start_matches('`').contains('`')
}

/// Wraps every line of `text` to `width` columns less the prefix. `first`
/// leads the first row, `rest` every other one.
fn push_wrapped(
    out: &mut Vec<Line>,
    kind: LineKind,
    first: &str,
    rest: &str,
    text: &str,
    width: usize,
) {
    let room = width.saturating_sub(text_width(first)).max(1);
    let mut lead = first;
    for logical in text.split('\n') {
        // A tab cell is not drawn, so tabs are spaces before wrapping.
        let logical = expand_tabs(logical);
        let logical = logical.as_str();
        // Code keeps every character where it is; prose breaks at spaces.
        let rows: Vec<(String, Wrap)> = if kind == LineKind::Code {
            let chars: Vec<char> = logical.chars().collect();
            wrap_ranges(&chars, room)
                .into_iter()
                .map(|range| (chars[range].iter().collect(), Wrap::Cut))
                .collect()
        } else {
            prose_rows(logical, room)
        };
        let last = rows.len().saturating_sub(1);
        for (n, (row, wrap)) in rows.into_iter().enumerate() {
            out.push(Line {
                kind,
                text: format!("{lead}{row}"),
                lead: text_width(lead),
                wrap: if n == last { Wrap::End } else { wrap },
            });
            lead = rest;
        }
    }
}

/// `logical` wrapped at spaces, each row with how it goes on: `Space` where
/// the wrap dropped spaces at the break, `Cut` where it broke a long word.
fn prose_rows(logical: &str, room: usize) -> Vec<(String, Wrap)> {
    let mut at = 0;
    wrap_words(logical, room)
        .into_iter()
        .map(|row| {
            // Rows are pieces of `logical` in order: only spaces lie between.
            let start = logical[at..].find(row.as_str()).map_or(at, |i| at + i);
            at = start + row.len();
            let wrap = if logical[at..].starts_with(' ') {
                Wrap::Space
            } else {
                Wrap::Cut
            };
            (row, wrap)
        })
        .collect()
}

/// Where the view is: glued to the bottom, or pinned to a line.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Scroll {
    #[default]
    Follow,
    /// The first visible line.
    Pinned(usize),
}

impl Scroll {
    /// The first visible line for `total` lines in `height` rows.
    pub fn top(self, total: usize, height: usize) -> usize {
        let last = total.saturating_sub(height);
        match self {
            Scroll::Follow => last,
            Scroll::Pinned(top) => top.min(last),
        }
    }

    /// One page up, with one row of overlap. Content that fits stays glued.
    pub fn page_up(self, total: usize, height: usize) -> Scroll {
        if total <= height {
            return Scroll::Follow;
        }
        Scroll::Pinned(self.top(total, height).saturating_sub(page(height)))
    }

    /// One page down; reaching the end glues the view to the bottom again.
    pub fn page_down(self, total: usize, height: usize) -> Scroll {
        match self {
            Scroll::Follow => Scroll::Follow,
            Scroll::Pinned(_) => {
                let top = self.top(total, height) + page(height);
                if top >= total.saturating_sub(height) {
                    Scroll::Follow
                } else {
                    Scroll::Pinned(top)
                }
            }
        }
    }

    /// `n` lines up. Content that fits stays glued; the top clamps at 0.
    pub fn lines_up(self, n: usize, total: usize, height: usize) -> Scroll {
        if total <= height {
            return Scroll::Follow;
        }
        Scroll::Pinned(self.top(total, height).saturating_sub(n))
    }

    /// `n` lines down; reaching the end glues the view to the bottom again.
    pub fn lines_down(self, n: usize, total: usize, height: usize) -> Scroll {
        match self {
            Scroll::Follow => Scroll::Follow,
            Scroll::Pinned(_) => {
                let top = self.top(total, height) + n;
                if top >= total.saturating_sub(height) {
                    Scroll::Follow
                } else {
                    Scroll::Pinned(top)
                }
            }
        }
    }
}

fn page(height: usize) -> usize {
    height.saturating_sub(1).max(1)
}

/// The wrapped lines of one (width, generation), kept until either changes.
#[derive(Default)]
pub struct LineCache(RefCell<Option<CachedLines>>);

/// Width, generation and the lines built for them.
type CachedLines = (usize, u64, Arc<Vec<Line>>);

impl LineCache {
    pub fn get(&self, chat: &ChatState, width: usize) -> Arc<Vec<Line>> {
        let generation = chat.generation();
        let mut slot = self.0.borrow_mut();
        if let Some((w, g, lines)) = &*slot
            && *w == width
            && *g == generation
        {
            return Arc::clone(lines);
        }
        let lines = Arc::new(build(chat, width));
        *slot = Some((width, generation, Arc::clone(&lines)));
        lines
    }
}

// Derived data: `eq` is always true so the cache never affects state equality, and `clone` drops it.
impl Clone for LineCache {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for LineCache {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for LineCache {}

impl std::fmt::Debug for LineCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LineCache")
    }
}

mod read;
pub use read::read_rows;

#[cfg(test)]
mod read_tests;
#[cfg(test)]
mod tests;
