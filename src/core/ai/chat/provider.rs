//! Which provider an engine and model belong to, and how it is marked. The
//! glyph is meaning, so it lives here; its colour belongs to the UI.

/// Engines that run on the user's machine, whatever the model.
const LOCAL_ENGINES: [&str; 4] = ["ollama", "llamacpp", "lmstudio", "vllm"];

/// Who makes the model, or `Local` when it runs here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Provider {
    Anthropic,
    Google,
    OpenAi,
    Local,
    Other,
}

impl Provider {
    pub const ALL: [Provider; 5] = [
        Self::Anthropic,
        Self::Google,
        Self::OpenAi,
        Self::Local,
        Self::Other,
    ];

    /// One column wide in every width table, never an emoji.
    pub fn glyph(self) -> char {
        match self {
            Self::Anthropic => '\u{273B}',
            Self::Google => '\u{2726}',
            Self::OpenAi => '\u{2742}',
            Self::Local => '\u{2B21}',
            Self::Other => '\u{2B26}',
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Anthropic => "Anthropic",
            Self::Google => "Google",
            Self::OpenAi => "OpenAI",
            Self::Local => "Local",
            Self::Other => "Other",
        }
    }
}

/// The provider of a row: by engine first, then by model prefix.
pub fn classify(engine: &str, model: Option<&str>) -> Provider {
    match engine {
        "claude" => return Provider::Anthropic,
        "gemini" => return Provider::Google,
        "codex" => return Provider::OpenAi,
        e if LOCAL_ENGINES.contains(&e) => return Provider::Local,
        _ => {}
    }
    let model = model.unwrap_or_default();
    // `o1`, `o3` and `o4` name a model only whole or before a separator
    // (`o3-mini`, `o1:latest`), never as the start of a longer word.
    let reasoning = ["o1", "o3", "o4"].iter().any(|p| {
        model
            .strip_prefix(p)
            .is_some_and(|rest| rest.chars().next().is_none_or(|c| !c.is_alphanumeric()))
    });
    let openai = ["gpt-", "codex"];
    if model.starts_with("claude") {
        Provider::Anthropic
    } else if model.starts_with("gemini") {
        Provider::Google
    } else if reasoning || openai.iter().any(|p| model.starts_with(p)) {
        Provider::OpenAi
    } else {
        Provider::Other
    }
}

#[cfg(test)]
mod tests;
