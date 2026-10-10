//! User configuration (`config.toml`). Pure: parsing and defaults only; the
//! file lookup and read live at the runtime edge.

use std::{
    ffi::OsString,
    fmt,
    io::{self, ErrorKind},
    path::PathBuf,
};

use toml::{Table, Value};

/// Which edge of the screen a bar sits on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BarPosition {
    Top,
    #[default]
    Bottom,
}

/// Where the statusline and the tab bar go.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BarPositions {
    pub statusline: BarPosition,
    pub tabbar: BarPosition,
}

impl Default for BarPositions {
    fn default() -> Self {
        Self {
            statusline: BarPosition::Bottom,
            tabbar: BarPosition::Top,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Config {
    pub bars: BarPositions,
    /// Reserved for the future config menu: accepted, no effect yet.
    pub mouse: bool,
}

/// Why a config file was rejected; the text is one short line.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConfigError(String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A readable file: the config, plus one line per key that was invalid and
/// fell back to its default.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Parsed {
    pub config: Config,
    pub problems: Vec<String>,
}

impl Config {
    /// Parses `config.toml`. Missing keys take their defaults and unknown
    /// keys are ignored, so older builds can read newer files.
    pub fn parse(text: &str) -> Result<Parsed, ConfigError> {
        let table: Table = text.parse().map_err(|error: toml::de::Error| {
            let message = error.message().lines().next().unwrap_or_default().trim();
            ConfigError(
                if message.is_empty() {
                    "invalid TOML"
                } else {
                    message
                }
                .to_owned(),
            )
        })?;
        let defaults = Config::default();
        let mut problems = Vec::new();
        let mut position = |section: &str, default: BarPosition| {
            read_position(&table, section, &mut problems).unwrap_or(default)
        };
        let statusline = position("statusline", defaults.bars.statusline);
        let tabbar = position("tabbar", defaults.bars.tabbar);
        let mouse = match table.get("mouse") {
            None => defaults.mouse,
            Some(Value::Boolean(on)) => *on,
            Some(_) => {
                problems.push("mouse: expected true or false".to_owned());
                defaults.mouse
            }
        };
        Ok(Parsed {
            config: Self {
                bars: BarPositions { statusline, tabbar },
                mouse,
            },
            problems,
        })
    }

    /// The config for the result of reading the file, plus the startup
    /// notice when something needs the user's attention.
    pub fn load(read: io::Result<String>) -> (Self, Option<String>) {
        let text = match read {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return (Self::default(), None),
            Err(error) => return (Self::default(), Some(format!("config: {error}"))),
        };
        match Self::parse(&text) {
            Ok(Parsed { config, problems }) if !problems.is_empty() => {
                let more = match problems.len() {
                    1 => String::new(),
                    n => format!(" (+{} more)", n - 1),
                };
                (config, Some(format!("config: {}{more}", problems[0])))
            }
            Ok(Parsed { config, .. }) if config.mouse => (
                config,
                Some("config: mouse is not supported yet".to_owned()),
            ),
            Ok(Parsed { config, .. }) => (config, None),
            Err(error) => (Self::default(), Some(format!("config: {error}"))),
        }
    }
}

/// `section.position`, or `None` (with a problem noted) when it is missing
/// or invalid, so the caller falls back to the default for that key only.
fn read_position(table: &Table, section: &str, problems: &mut Vec<String>) -> Option<BarPosition> {
    let section_table = match table.get(section) {
        None => return None,
        Some(Value::Table(section_table)) => section_table,
        Some(_) => {
            problems.push(format!("{section}: expected a table"));
            return None;
        }
    };
    match section_table.get("position")?.as_str() {
        Some("top") => Some(BarPosition::Top),
        Some("bottom") => Some(BarPosition::Bottom),
        _ => {
            problems.push(format!(
                "{section}.position: expected \"top\" or \"bottom\""
            ));
            None
        }
    }
}

/// `$XDG_CONFIG_HOME/lazarobox/config.toml`, else `$HOME/.config/...`. Per
/// the XDG spec, an empty or relative `XDG_CONFIG_HOME` or `HOME` is ignored.
pub fn config_path(xdg: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let base = xdg
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            let home = PathBuf::from(home?);
            home.is_absolute().then(|| home.join(".config"))
        })?;
    Some(base.join("lazarobox").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use BarPosition::{Bottom, Top};

    fn bars(statusline: BarPosition, tabbar: BarPosition) -> BarPositions {
        BarPositions { statusline, tabbar }
    }

    fn parse(text: &str) -> Config {
        Config::parse(text).unwrap().config
    }

    fn problems(text: &str) -> Vec<String> {
        Config::parse(text).unwrap().problems
    }

    // Spec: Defaults.
    #[test]
    fn defaults_are_statusline_bottom_tabbar_top_no_mouse() {
        let expected = Config {
            bars: bars(Bottom, Top),
            mouse: false,
        };
        assert_eq!(Config::default(), expected);
        assert_eq!(parse(""), expected);
        assert_eq!(parse("[statusline]\n[tabbar]\n"), expected);
    }

    // Spec: Each position value.
    #[test]
    fn each_position_is_read() {
        assert_eq!(
            parse("[statusline]\nposition = \"top\"").bars.statusline,
            Top
        );
        assert_eq!(
            parse("[statusline]\nposition = \"bottom\"").bars.statusline,
            Bottom
        );
        assert_eq!(parse("[tabbar]\nposition = \"top\"").bars.tabbar, Top);
        assert_eq!(parse("[tabbar]\nposition = \"bottom\"").bars.tabbar, Bottom);
    }

    #[test]
    fn a_partial_file_keeps_the_other_defaults() {
        let c = parse("[tabbar]\nposition = \"bottom\"");
        assert_eq!(c.bars, bars(Bottom, Bottom));
    }

    #[test]
    fn mouse_is_read_at_the_top_level() {
        assert!(parse("mouse = true").mouse);
        assert!(!parse("mouse = false").mouse);
    }

    // Spec: Invalid values fall back per key.
    #[test]
    fn only_unparseable_toml_is_an_error() {
        for text in ["[statusline]\nposition = ", "not toml at all", "[tabbar"] {
            let e = Config::parse(text).unwrap_err().to_string();
            assert!(!e.is_empty() && !e.contains('\n'), "{text:?}: {e:?}");
        }
    }

    #[test]
    fn an_invalid_value_falls_back_for_that_key_only() {
        let text =
            "mouse = true\n[statusline]\nposition = \"middle\"\n[tabbar]\nposition = \"bottom\"";
        let parsed = Config::parse(text).unwrap();
        // The bad key is the default; every other key survives.
        assert_eq!(parsed.config.bars, bars(Bottom, Bottom));
        assert!(parsed.config.mouse);
        assert_eq!(
            parsed.problems,
            [r#"statusline.position: expected "top" or "bottom""#]
        );
    }

    #[test]
    fn each_invalid_key_and_type_is_reported_with_its_key() {
        for (text, expected) in [
            (
                "[statusline]\nposition = \"middle\"",
                r#"statusline.position: expected "top" or "bottom""#,
            ),
            (
                "[tabbar]\nposition = \"TOP\"",
                r#"tabbar.position: expected "top" or "bottom""#,
            ),
            (
                "[tabbar]\nposition = 3",
                r#"tabbar.position: expected "top" or "bottom""#,
            ),
            ("mouse = \"yes\"", "mouse: expected true or false"),
            ("statusline = 3", "statusline: expected a table"),
        ] {
            assert_eq!(problems(text), [expected], "{text:?}");
            assert_eq!(parse(text), Config::default(), "{text:?}");
        }
    }

    #[test]
    fn a_bad_mouse_keeps_the_valid_positions() {
        let c = parse("mouse = 1\n[statusline]\nposition = \"top\"");
        assert_eq!(c.bars.statusline, Top);
        assert!(!c.mouse);
    }

    #[test]
    fn valid_files_have_no_problems() {
        assert!(problems("[statusline]\nposition = \"top\"\nx = 1").is_empty());
    }

    // Spec: Unknown keys ignored.
    #[test]
    fn unknown_keys_and_tables_are_ignored() {
        let c = parse("theme = \"x\"\n[statusline]\nposition = \"top\"\ncolor = 1\n[other]\na = 1");
        assert_eq!(c.bars.statusline, Top);
    }

    // Spec: Missing file, unreadable file, invalid file, mouse notice.
    #[test]
    fn a_missing_file_is_silent_defaults() {
        let err = io::Error::from(ErrorKind::NotFound);
        assert_eq!(Config::load(Err(err)), (Config::default(), None));
    }

    #[test]
    fn an_unreadable_file_gives_defaults_and_a_notice() {
        let err = io::Error::from(ErrorKind::PermissionDenied);
        let (config, notice) = Config::load(Err(err));
        assert_eq!(config, Config::default());
        let notice = notice.unwrap();
        assert!(notice.starts_with("config: "), "{notice}");
        assert!(!notice.contains('\n'));
    }

    #[test]
    fn unparseable_toml_gives_all_defaults_and_a_notice() {
        let (config, notice) = Config::load(Ok("mouse = false\n[tabbar\n".into()));
        assert_eq!(config, Config::default());
        let notice = notice.unwrap();
        assert!(notice.starts_with("config: "), "{notice}");
        assert!(!notice.contains('\n'), "{notice}");
    }

    #[test]
    fn an_invalid_key_keeps_the_rest_and_the_notice_names_it() {
        let text = "[statusline]\nposition = \"top\"\n[tabbar]\nposition = \"left\"";
        let (config, notice) = Config::load(Ok(text.into()));
        assert_eq!(config.bars, bars(Top, Top));
        assert_eq!(
            notice.as_deref(),
            Some(r#"config: tabbar.position: expected "top" or "bottom""#)
        );
    }

    #[test]
    fn several_invalid_keys_report_the_first_and_count_the_rest() {
        let text = "mouse = 3\n[statusline]\nposition = \"x\"\n[tabbar]\nposition = \"y\"";
        let (config, notice) = Config::load(Ok(text.into()));
        assert_eq!(config, Config::default());
        assert_eq!(
            notice.as_deref(),
            Some(r#"config: statusline.position: expected "top" or "bottom" (+2 more)"#)
        );
    }

    #[test]
    fn a_problem_notice_wins_over_the_mouse_notice() {
        let (config, notice) = Config::load(Ok("mouse = true\n[tabbar]\nposition = 1".into()));
        assert!(config.mouse);
        assert!(notice.unwrap().contains("tabbar.position"));
    }

    #[test]
    fn a_valid_file_loads_without_a_notice() {
        let (config, notice) = Config::load(Ok("[statusline]\nposition = \"top\"".into()));
        assert_eq!(config.bars.statusline, Top);
        assert_eq!(notice, None);
    }

    #[test]
    fn mouse_true_is_accepted_with_a_notice() {
        let text = "mouse = true\n[tabbar]\nposition = \"bottom\"";
        let (config, notice) = Config::load(Ok(text.into()));
        assert!(config.mouse);
        assert_eq!(config.bars.tabbar, Bottom);
        assert_eq!(
            notice.as_deref(),
            Some("config: mouse is not supported yet")
        );
    }

    // Spec: File location.
    #[test]
    fn the_path_prefers_xdg_then_home() {
        let p =
            |x: Option<&str>, h: Option<&str>| config_path(x.map(Into::into), h.map(Into::into));
        let xdg = Some("/x/lazarobox/config.toml".into());
        let home = Some("/h/.config/lazarobox/config.toml".into());
        assert_eq!(p(Some("/x"), Some("/h")), xdg);
        assert_eq!(p(None, Some("/h")), home);
        assert_eq!(p(Some(""), Some("/h")), home);
        assert_eq!(p(Some("rel"), Some("/h")), home);
        assert_eq!(p(None, None), None);
        // Spec: Relative or empty HOME is ignored.
        assert_eq!(p(None, Some("")), None);
        assert_eq!(p(None, Some("rel")), None);
        assert_eq!(p(Some("/x"), Some("")), xdg);
    }
}
