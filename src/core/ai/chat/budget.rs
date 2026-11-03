//! The context budget of a turn, counted in estimated tokens. Pure: the
//! engine's real window is probed by the runtime and handed in.
//!
//! With a known window the history gets what is left of three quarters of it
//! (the rest is the reply and slack) once the system prompt, the memory block
//! and the tool schemas are paid for. Without one, the character caps of
//! `engine` stand, and a local server pays the same head out of its cap, so
//! a turn never counts the history alone, and never leaves it under
//! [`UNKNOWN_WINDOW_FLOOR`] characters.

use super::engine::{ChatMessage, history_chars};
use crate::core::ai::{CATALOG, Kind};

/// Characters per token of the estimate: coarse, but the same for every model.
pub const CHARS_PER_TOKEN: usize = 4;
/// The share of the window a prompt may fill, in percent; the rest is left
/// for the reply.
pub const PROMPT_SHARE: usize = 75;
/// The least history, in characters, a local server keeps when its window is
/// unknown: a big system prompt or tool block cannot squeeze the chat to
/// nothing, since the real window is not known to be that small. A turn that
/// does not fit is then the model's to refuse, never a silent empty history.
pub const UNKNOWN_WINDOW_FLOOR: usize = 4_000;
/// Estimated tokens of `text`.
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(CHARS_PER_TOKEN)
}

/// How much history a turn may carry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Budget {
    /// The window is unknown: a character cap, memory already taken off.
    Chars(usize),
    /// The window is known: estimated tokens of room for the history.
    Tokens { room: usize, window: usize },
}

/// The budget of a turn on `engine`. `window` is its context window in tokens
/// when known (zero counts as unknown); `tools_reserve` is what the tool
/// block and schemas of the turn weigh in tokens (0 when it offers none).
pub fn budget(
    engine: &str,
    window: Option<usize>,
    system: &str,
    memory: &str,
    tools_reserve: usize,
) -> Budget {
    match window.filter(|&w| w > 0) {
        None => {
            let cap = history_chars(engine).saturating_sub(memory.chars().count());
            // Only a local server loads its head into a small window; a CLI
            // keeps its large cap as it always had.
            let server = CATALOG
                .iter()
                .any(|e| e.id == engine && matches!(e.kind, Kind::Server { .. }));
            if server {
                let head = system.chars().count() + tools_reserve * CHARS_PER_TOKEN;
                Budget::Chars(cap.saturating_sub(head).max(UNKNOWN_WINDOW_FLOOR))
            } else {
                Budget::Chars(cap)
            }
        }
        Some(window) => {
            let fixed = estimate_tokens(system) + estimate_tokens(memory) + tools_reserve;
            Budget::Tokens {
                room: (window * PROMPT_SHARE / 100).saturating_sub(fixed),
                window,
            }
        }
    }
}

/// How full the context of a turn is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fill {
    pub used: usize,
    pub capacity: usize,
}

impl Fill {
    /// The fill in percent, at most 100.
    pub fn percent(self) -> u8 {
        if self.capacity == 0 {
            return 100;
        }
        u8::try_from((self.used * 100 / self.capacity).min(100)).unwrap_or(100)
    }
}

/// How full a turn's context is: the system prompt, the memory block, the
/// tool reserve and the sent `messages` against the
/// window. `None` when the window is unknown: against the character cap the
/// share would say nothing about the real context.
pub fn fill(
    window: Option<usize>,
    system: &str,
    memory: &str,
    tools_reserve: usize,
    messages: &[ChatMessage],
) -> Option<Fill> {
    let capacity = window.filter(|&w| w > 0)?;
    let history: usize = messages.iter().map(|m| estimate_tokens(&m.content)).sum();
    Some(Fill {
        used: history + estimate_tokens(system) + estimate_tokens(memory) + tools_reserve,
        capacity,
    })
}

#[cfg(test)]
mod tests;
