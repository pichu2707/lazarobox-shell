//! The context budget of a turn, counted in estimated tokens. Pure: the
//! engine's real window is probed by the runtime and handed in.
//!
//! With a known window the history gets what is left of three quarters of it
//! (the rest is the reply and slack) once the system prompt, the memory block
//! and the tool schemas are paid for. Without one, the character caps of
//! `engine` stand, as they always did.

use super::engine::{ChatMessage, history_chars};

/// Characters per token of the estimate: coarse, but the same for every model.
pub const CHARS_PER_TOKEN: usize = 4;
/// The share of the window a prompt may fill, in percent; the rest is left
/// for the reply.
pub const PROMPT_SHARE: usize = 75;
/// What the tool schemas of a turn are assumed to cost, in tokens. They are
/// attached after the budget is set, so it is a flat reserve.
pub const TOOLS_RESERVE_TOKENS: usize = 1_500;

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

/// The tokens a turn reserves for the tool schemas: the flat reserve when the
/// turn offers tools, nothing otherwise.
fn tools_reserve(tools: bool) -> usize {
    if tools { TOOLS_RESERVE_TOKENS } else { 0 }
}

/// The budget of a turn on `engine`. `window` is its context window in tokens
/// when known (zero counts as unknown); `tools` is whether the turn offers
/// tools, which is what the reserve is for.
pub fn budget(
    engine: &str,
    window: Option<usize>,
    system: &str,
    memory: &str,
    tools: bool,
) -> Budget {
    match window.filter(|&w| w > 0) {
        None => Budget::Chars(history_chars(engine).saturating_sub(memory.chars().count())),
        Some(window) => {
            let fixed = estimate_tokens(system) + estimate_tokens(memory) + tools_reserve(tools);
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
/// tool reserve (when tools are offered) and the sent `messages` against the
/// window. `None` when the window is unknown: against the character cap the
/// share would say nothing about the real context.
pub fn fill(
    window: Option<usize>,
    system: &str,
    memory: &str,
    tools: bool,
    messages: &[ChatMessage],
) -> Option<Fill> {
    let capacity = window.filter(|&w| w > 0)?;
    let history: usize = messages.iter().map(|m| estimate_tokens(&m.content)).sum();
    Some(Fill {
        used: history + estimate_tokens(system) + estimate_tokens(memory) + tools_reserve(tools),
        capacity,
    })
}

#[cfg(test)]
mod tests;
