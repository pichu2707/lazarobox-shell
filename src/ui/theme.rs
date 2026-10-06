use ratatui::style::{Color, Style};

use crate::app::{AppMode, InputMode};

#[derive(Debug, Clone)]
pub struct LazaroboxTheme {
    pub bg_base: Color,
    pub bg_panel: Color,
    pub primary_cyan: Color,
    pub ai_purple: Color,
    pub success_green: Color,
    pub warning_orange: Color,
    pub error_red: Color,
    pub text_muted: Color,
}

impl Default for LazaroboxTheme {
    fn default() -> Self {
        Self {
            bg_base: Color::from_u32(0x00181E24),
            bg_panel: Color::from_u32(0x00202831),
            primary_cyan: Color::from_u32(0x0000E5FF),
            ai_purple: Color::from_u32(0x00CBA6F7),
            success_green: Color::from_u32(0x00A6E3A1),
            warning_orange: Color::from_u32(0x00FAB387),
            error_red: Color::from_u32(0x00F38BA8),
            text_muted: Color::from_u32(0x006C7086),
        }
    }
}

impl LazaroboxTheme {
    /// Accent color of the status bar block for a view mode.
    pub fn accent(&self, mode: AppMode) -> Color {
        match mode {
            AppMode::Normal => self.primary_cyan,
            AppMode::AiChat => self.ai_purple,
            AppMode::Metrics => self.success_green,
            AppMode::Settings => self.warning_orange,
        }
    }

    /// Returns the status bar style for the active view mode.
    pub fn mode_style(&self, mode: AppMode) -> Style {
        Style::default().bg(self.accent(mode)).fg(self.bg_base)
    }

    /// Accent color for an input mode, following the vim analogy:
    /// TERMINAL is insert (green), PREFIX is pending (orange), COPY is normal
    /// (cyan) and the quit prompt is a warning (red).
    pub fn input_accent(&self, mode: InputMode) -> Color {
        match mode {
            InputMode::Terminal => self.success_green,
            InputMode::Prefix => self.warning_orange,
            InputMode::Copy(_) => self.primary_cyan,
            InputMode::ConfirmQuit => self.error_red,
        }
    }

    /// Returns the status bar style for the active input mode.
    pub fn input_mode_style(&self, mode: InputMode) -> Style {
        Style::default()
            .bg(self.input_accent(mode))
            .fg(self.bg_base)
    }

    /// Lists every theme color as `(name, hex, color)`, for previews.
    pub fn palette(&self) -> [(&'static str, &'static str, Color); 8] {
        [
            ("bg_base", "#181E24", self.bg_base),
            ("bg_panel", "#202831", self.bg_panel),
            ("primary_cyan", "#00E5FF", self.primary_cyan),
            ("ai_purple", "#CBA6F7", self.ai_purple),
            ("success_green", "#A6E3A1", self.success_green),
            ("warning_orange", "#FAB387", self.warning_orange),
            ("error_red", "#F38BA8", self.error_red),
            ("text_muted", "#6C7086", self.text_muted),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::copy::CopyState;

    #[test]
    fn default_colors_match_readme_hex_values() {
        let t = LazaroboxTheme::default();
        assert_eq!(t.bg_base, Color::from_u32(0x00181E24));
        assert_eq!(t.bg_panel, Color::from_u32(0x00202831));
        assert_eq!(t.primary_cyan, Color::from_u32(0x0000E5FF));
        assert_eq!(t.ai_purple, Color::from_u32(0x00CBA6F7));
        assert_eq!(t.success_green, Color::from_u32(0x00A6E3A1));
        assert_eq!(t.warning_orange, Color::from_u32(0x00FAB387));
        assert_eq!(t.error_red, Color::from_u32(0x00F38BA8));
        assert_eq!(t.text_muted, Color::from_u32(0x006C7086));
    }

    #[test]
    fn mode_style_maps_each_mode_to_its_accent_with_base_fg() {
        let t = LazaroboxTheme::default();
        let cases = [
            (AppMode::Normal, t.primary_cyan),
            (AppMode::AiChat, t.ai_purple),
            (AppMode::Metrics, t.success_green),
            (AppMode::Settings, t.warning_orange),
        ];
        for (mode, bg) in cases {
            let style = t.mode_style(mode);
            assert_eq!(style.bg, Some(bg), "bg for {mode:?}");
            assert_eq!(style.fg, Some(t.bg_base), "fg for {mode:?}");
        }
    }

    #[test]
    fn accent_maps_each_app_mode_to_its_palette_color() {
        let t = LazaroboxTheme::default();
        assert_eq!(t.accent(AppMode::Normal), t.primary_cyan);
        assert_eq!(t.accent(AppMode::AiChat), t.ai_purple);
        assert_eq!(t.accent(AppMode::Metrics), t.success_green);
        assert_eq!(t.accent(AppMode::Settings), t.warning_orange);
    }

    #[test]
    fn theme_is_debug_and_clone() {
        let t = LazaroboxTheme::default();
        let copy = t.clone();
        assert_eq!(copy.bg_base, t.bg_base);
        assert!(format!("{t:?}").contains("bg_base"));
    }

    #[test]
    fn input_mode_style_maps_each_mode_to_its_color_with_base_fg() {
        let t = LazaroboxTheme::default();
        let cases = [
            (InputMode::Terminal, t.success_green),
            (InputMode::Prefix, t.warning_orange),
            (InputMode::Copy(CopyState::default()), t.primary_cyan),
            (InputMode::ConfirmQuit, t.error_red),
        ];
        for (mode, bg) in cases {
            let style = t.input_mode_style(mode);
            assert_eq!(style.bg, Some(bg), "bg for {mode:?}");
            assert_eq!(style.fg, Some(t.bg_base), "fg for {mode:?}");
            assert_eq!(t.input_accent(mode), bg, "accent for {mode:?}");
        }
    }

    #[test]
    fn input_mode_colors_follow_the_theme() {
        let t = LazaroboxTheme {
            success_green: Color::Rgb(1, 2, 3),
            error_red: Color::Rgb(4, 5, 6),
            ..LazaroboxTheme::default()
        };
        assert_eq!(t.input_accent(InputMode::Terminal), Color::Rgb(1, 2, 3));
        assert_eq!(t.input_accent(InputMode::ConfirmQuit), Color::Rgb(4, 5, 6));
    }

    #[test]
    fn palette_lists_eight_entries_with_matching_hex_and_color() {
        let t = LazaroboxTheme::default();
        let palette = t.palette();
        assert_eq!(palette.len(), 8);
        for (name, hex, color) in palette {
            let value = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap();
            assert_eq!(color, Color::from_u32(value), "{name}");
        }
    }
}
