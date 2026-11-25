//! The `[theme]` section of `config.toml`: the colors a user sets for
//! lazarobox's own interface. A role left out keeps its default; the UI
//! (`ui::theme`) applies these over the built-in palette.

use toml::{Table, Value};

/// The table in `config.toml` that holds the colors.
pub const THEME_TABLE: &str = "theme";

/// A color as red, green and blue.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// A role of the interface that has a color, named after what it is for,
/// not after its default color.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Background,
    Panel,
    Primary,
    Info,
    Ai,
    Success,
    Warning,
    Error,
    Muted,
    Neutral,
}

impl Role {
    /// Every role, in the order of [`THEME_KEYS`].
    pub const ALL: [Role; 10] = [
        Role::Background,
        Role::Panel,
        Role::Primary,
        Role::Info,
        Role::Ai,
        Role::Success,
        Role::Warning,
        Role::Error,
        Role::Muted,
        Role::Neutral,
    ];

    /// The role's key in `[theme]`.
    pub fn key(self) -> &'static str {
        THEME_KEYS[self as usize]
    }
}

/// The keys of `[theme]`, one per [`Role`] in the same order.
pub const THEME_KEYS: &[&str] = &[
    "background",
    "panel",
    "primary",
    "info",
    "ai",
    "success",
    "warning",
    "error",
    "muted",
    "neutral",
];

/// The colors set in `[theme]`. A role without one keeps its default.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ThemeColors([Option<Rgb>; Role::ALL.len()]);

impl ThemeColors {
    /// The color set for `role`, if any.
    pub fn get(&self, role: Role) -> Option<Rgb> {
        self.0[role as usize]
    }

    /// Sets the color of `role`.
    pub fn set(&mut self, role: Role, color: Rgb) {
        self.0[role as usize] = Some(color);
    }
}

/// Parses `#RRGGBB` (either case), or `None` when it is not one.
pub fn parse_hex(text: &str) -> Option<Rgb> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
    Some(Rgb(byte(0)?, byte(2)?, byte(4)?))
}

/// `[theme]` from the parsed file. A malformed value is noted as a problem
/// and leaves its role at the default; the other roles still apply.
/// Unknown keys are noted by the caller (`config::OWNED_SECTIONS`).
pub(crate) fn read_theme(table: &Table, problems: &mut Vec<String>) -> ThemeColors {
    let mut colors = ThemeColors::default();
    let section = match table.get(THEME_TABLE) {
        None => return colors,
        Some(Value::Table(section)) => section,
        Some(_) => {
            problems.push(format!("{THEME_TABLE}: expected a table"));
            return colors;
        }
    };
    for role in Role::ALL {
        let Some(value) = section.get(role.key()) else {
            continue;
        };
        match value.as_str().and_then(parse_hex) {
            Some(color) => colors.set(role, color),
            None => problems.push(format!(
                "{THEME_TABLE}.{}: expected a color like \"#00E5FF\"",
                role.key()
            )),
        }
    }
    colors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_role_has_its_own_key() {
        assert_eq!(THEME_KEYS.len(), Role::ALL.len());
        for (i, role) in Role::ALL.into_iter().enumerate() {
            assert_eq!(role as usize, i);
            assert_eq!(role.key(), THEME_KEYS[i]);
        }
    }

    #[test]
    fn hex_colors_parse_in_either_case() {
        assert_eq!(parse_hex("#00E5FF"), Some(Rgb(0x00, 0xE5, 0xFF)));
        assert_eq!(parse_hex("#00e5ff"), Some(Rgb(0x00, 0xE5, 0xFF)));
    }

    #[test]
    fn anything_but_hash_and_six_hex_digits_is_refused() {
        for bad in [
            "",
            "#",
            "00E5FF",
            "#0E5",
            "#00E5FF0",
            "#00E5FG",
            "#+0E5FF",
            "#00E5F\u{e9}",
        ] {
            assert_eq!(parse_hex(bad), None, "{bad:?}");
        }
    }
}
