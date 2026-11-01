//! Tool marks: whether a model can drive lazarobox's tools. Pure.
//!
//! The tool support of an Ollama model comes from `POST /api/show`, never
//! from the `capabilities` of `/api/tags`: on Ollama 0.30.10 the listing can
//! omit `tools` that show reports. A model of 3B or more that declares tools
//! is `Capable`; a smaller one is `Unreliable`; anything else, including an
//! unknown size or support, gets no mark.

/// Whether a model declares tool support.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tools {
    Yes,
    No,
    #[default]
    Unknown,
}

/// The mark a model row carries.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolMark {
    Capable,
    Unreliable,
}

/// How the tool badge is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ToolGlyph {
    Emoji,
    #[default]
    Text,
}

/// The glyph for a terminal: text on the Linux console and on any
/// non-UTF-8 locale, where the emoji would not draw.
// `utf8` comes from lazarobox's own environment, not from the host terminal.
pub fn tool_glyph(term: Option<&str>, utf8: bool) -> ToolGlyph {
    let plain = term.is_some_and(|t| t == "linux" || t == "dumb" || t.starts_with("linux-"));
    if utf8 && !plain {
        ToolGlyph::Emoji
    } else {
        ToolGlyph::Text
    }
}

/// The badge text of a mark; the single place that maps both.
pub fn badge_text(mark: ToolMark, glyph: ToolGlyph) -> &'static str {
    match (mark, glyph) {
        (ToolMark::Capable, ToolGlyph::Emoji) => "🔧",
        (ToolMark::Capable, ToolGlyph::Text) => "tools",
        (ToolMark::Unreliable, _) => "tools: unreliable",
    }
}

/// Parameters of a size such as `8.0B`, `137M` or `8x7B`.
pub fn params_of(size: &str) -> Option<u64> {
    super::ollama_lib::size_params(&size.to_ascii_lowercase())
}

/// The mark for a model's tool support and size.
pub fn tool_mark(tools: Tools, params: Option<u64>) -> Option<ToolMark> {
    match (tools, params?) {
        (Tools::Yes, n) if n >= RELIABLE_PARAMS => Some(ToolMark::Capable),
        (Tools::Yes, _) => Some(ToolMark::Unreliable),
        _ => None,
    }
}

/// Smallest size (3B) at which tool calls are trusted.
const RELIABLE_PARAMS: u64 = 3_000_000_000;

#[cfg(test)]
mod tests;
