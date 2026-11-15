//! The last round of an orchestrator turn: what its run needs to know when
//! the turn ends (agent-orchestrator, "Gates on step state are visible and
//! bounded"). Read from the events after the pipe, so it holds what the pane
//! was shown.

use crate::core::ai::chat::engine::StreamEvent;
use crate::core::ai::orchestra::run::TurnEnd;
use crate::core::ai::orchestra::tools::written_call;

/// The most text kept: the end of the answer, where a written call sits.
pub(super) const CAP: usize = 16 * 1024;

#[derive(Default)]
pub(super) struct Tail {
    done: bool,
    text: String,
}

impl Tail {
    pub(super) fn see(&mut self, event: &StreamEvent) {
        match event {
            StreamEvent::Delta(text) => {
                self.text.push_str(text);
                let mut cut = self.text.len().saturating_sub(CAP);
                while !self.text.is_char_boundary(cut) {
                    cut += 1;
                }
                self.text.drain(..cut);
            }
            // A call or a notice ends the round: the text before it was
            // answered by what followed (a reminded call drops out).
            StreamEvent::ToolCall { .. } | StreamEvent::Notice(_) => self.text.clear(),
            StreamEvent::Done(_) => self.done = true,
            _ => {}
        }
    }

    pub(super) fn end(&self) -> TurnEnd {
        TurnEnd {
            answered: self.done,
            written: written_call(&self.text),
        }
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.text.len()
    }
}
