//! Tool rows: what the transcript shows of a tool call. A row is made only
//! from the `ToolCall` and `ToolResult` events of the host that ran the call;
//! answer text is never read for them. Rows are shown, never stored and never
//! sent to the engine (like reasoning).

use super::{
    ChatState, Entry, Phase,
    engine::{CallId, ToolStatus},
};

/// What a result row shows for a call whose turn ended before its result.
pub const NO_RESULT: &str = "the turn ended first";

/// One tool call as the transcript shows it: the call and, once the host
/// reports it, its result.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ToolRow {
    id: CallId,
    name: String,
    summary: String,
    result: Option<(ToolStatus, String)>,
}

impl ToolRow {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The short, sanitized argument summary.
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// The status and the short summary of the result, once it arrived.
    pub fn result(&self) -> Option<(ToolStatus, &str)> {
        self.result
            .as_ref()
            .map(|(status, text)| (*status, &**text))
    }
}

impl ChatState {
    /// A call was announced: its row appears, above the answer being written
    /// (a row is part of how the answer came to be, like reasoning). Ignored
    /// when the id already has a row, so a call is shown once. A tool event
    /// is stream activity, so the "still waiting" text goes.
    pub(super) fn tool_call(&mut self, id: CallId, name: String, summary: String) {
        if self.tool_row(id).is_some() {
            return;
        }
        if let Phase::Waiting { slow } = &mut self.phase {
            *slow = false;
        }
        let row = Entry::Tool(ToolRow {
            id,
            name,
            summary,
            result: None,
        });
        let live_answer = (self.phase == Phase::Streaming)
            .then(|| {
                self.entries
                    .iter()
                    .rposition(|e| matches!(e, Entry::Message(_)))
            })
            .flatten();
        match live_answer {
            Some(at) => self.entries.insert(at, row),
            None => self.entries.push(row),
        }
    }

    /// The result of a call: completes its row. A result for an id with no
    /// row, or whose row already has one, is ignored: a call is paired once.
    pub(super) fn tool_result(&mut self, id: CallId, status: ToolStatus, summary: String) {
        if let Some(row) = self.tool_row(id)
            && row.result.is_none()
        {
            row.result = Some((status, summary));
        }
    }

    /// The turn is over: a call that never got its result ends as an error
    /// row, so no row stays pending.
    pub(super) fn settle_tool_rows(&mut self) {
        for entry in &mut self.entries {
            if let Entry::Tool(row) = entry
                && row.result.is_none()
            {
                row.result = Some((ToolStatus::Error, NO_RESULT.to_owned()));
            }
        }
    }

    fn tool_row(&mut self, id: CallId) -> Option<&mut ToolRow> {
        self.entries.iter_mut().find_map(|e| match e {
            Entry::Tool(row) if row.id == id => Some(row),
            _ => None,
        })
    }
}
