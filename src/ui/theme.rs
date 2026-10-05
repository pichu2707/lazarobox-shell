use ratatui::style::{Color, Style};

use crate::app::AppMode;

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
    /// Returns the status bar style for the active mode.
    pub fn mode_style(&self, mode: &AppMode) -> Style {
        let accent = match mode {
            AppMode::Normal => self.primary_cyan,
            AppMode::AiChat => self.ai_purple,
            AppMode::Metrics => self.success_green,
            AppMode::Settings => self.warning_orange,
        };
        Style::default().bg(accent).fg(self.bg_base)
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
            let style = t.mode_style(&mode);
            assert_eq!(style.bg, Some(bg), "bg for {mode:?}");
            assert_eq!(style.fg, Some(t.bg_base), "fg for {mode:?}");
        }
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
