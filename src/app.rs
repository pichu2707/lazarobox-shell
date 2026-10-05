#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppMode {
    #[default]
    Normal,
    AiChat,
    Metrics,
    Settings,
}

impl AppMode {
    pub const ALL: [AppMode; 4] = [Self::Normal, Self::AiChat, Self::Metrics, Self::Settings];

    /// Returns the next mode, wrapping from the last back to the first.
    pub fn next(self) -> Self {
        match self {
            Self::Normal => Self::AiChat,
            Self::AiChat => Self::Metrics,
            Self::Metrics => Self::Settings,
            Self::Settings => Self::Normal,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::AiChat => "AI CHAT",
            Self::Metrics => "METRICS",
            Self::Settings => "SETTINGS",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_cycles_through_all_modes_and_wraps() {
        assert_eq!(AppMode::Normal.next(), AppMode::AiChat);
        assert_eq!(AppMode::AiChat.next(), AppMode::Metrics);
        assert_eq!(AppMode::Metrics.next(), AppMode::Settings);
        assert_eq!(AppMode::Settings.next(), AppMode::Normal);
    }

    #[test]
    fn labels_are_uppercase_names() {
        let labels: Vec<_> = AppMode::ALL.iter().map(|m| m.label()).collect();
        assert_eq!(labels, ["NORMAL", "AI CHAT", "METRICS", "SETTINGS"]);
    }

    #[test]
    fn default_is_normal() {
        assert_eq!(AppMode::default(), AppMode::Normal);
    }
}
