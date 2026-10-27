//! Note titles, slugs and topic keys.

use super::kind::MemKind;
use crate::core::ai::sanitize::is_format;
use unicode_normalization::UnicodeNormalization;

/// The longest title kept, in characters.
pub const TITLE_CAP: usize = 120;
/// The longest slug, in characters.
pub const SLUG_CAP: usize = 60;

/// The title as typed, without control and format characters, trimmed and cut
/// to `TITLE_CAP` characters. Empty means the title is refused.
pub fn clean_title(title: &str) -> String {
    let kept: String = title
        .chars()
        .filter(|&c| !c.is_control() && !is_format(c))
        .collect();
    kept.trim().chars().take(TITLE_CAP).collect()
}

/// Lowercase letters and digits joined by single dashes, at most `SLUG_CAP`
/// characters, `untitled` when nothing is left. The title is NFC-normalised
/// first, so composed and decomposed spellings give the same slug. Pure and
/// deterministic.
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in title.chars().flat_map(char::to_lowercase).nfc() {
        if c.is_alphanumeric() {
            if gap && !out.is_empty() {
                out.push('-');
            }
            gap = false;
            out.push(c);
        } else {
            gap = true;
        }
    }
    let cut: String = out.chars().take(SLUG_CAP).collect();
    let cut = cut.trim_end_matches('-');
    if cut.is_empty() {
        "untitled".to_string()
    } else {
        cut.to_string()
    }
}

/// `chat/<kind>/<slug(title)>`; the `chat/` prefix marks lazarobox notes.
pub fn topic_key(kind: MemKind, title: &str) -> String {
    format!("chat/{}/{}", kind.as_str(), slug(title))
}

/// The longest caller-supplied topic key, in characters.
pub const TOPIC_KEY_CAP: usize = 200;

/// Whether `key` may be used as a caller-supplied topic key: not blank, at
/// most `TOPIC_KEY_CAP` characters, no control or format characters.
pub fn valid_topic_key(key: &str) -> bool {
    !key.trim().is_empty()
        && key.chars().count() <= TOPIC_KEY_CAP
        && !key.chars().any(|c| c.is_control() || is_format(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_topic_key_is_not_blank_bounded_and_free_of_control_characters() {
        assert!(valid_topic_key("chat/12/summary"));
        assert!(valid_topic_key(&"k".repeat(TOPIC_KEY_CAP)));
        for bad in ["", " \t", "a\nb", "a\u{1b}b", "a\u{200b}b"] {
            assert!(!valid_topic_key(bad), "{bad:?}");
        }
        assert!(!valid_topic_key(&"k".repeat(TOPIC_KEY_CAP + 1)));
    }

    #[test]
    fn slug_table_from_the_spec() {
        let table = [
            ("Use Axum!", "use-axum"),
            ("  A/B  test--x ", "a-b-test-x"),
            ("Ünï Cødé 42", "ünï-cødé-42"),
            ("日本語 メモ", "日本語-メモ"),
            ("../../etc", "etc"),
        ];
        for (title, want) in table {
            assert_eq!(slug(title), want, "{title:?}");
        }
    }

    #[test]
    fn composed_and_decomposed_titles_give_the_same_slug_and_key() {
        let nfc = "Caf\u{e9} \u{d1}and\u{fa}";
        let nfd = "Cafe\u{301} N\u{303}andu\u{301}";
        assert_ne!(nfc, nfd);
        assert_eq!(slug(nfc), slug(nfd));
        assert_eq!(slug(nfc), "caf\u{e9}-\u{f1}and\u{fa}");
        assert_eq!(
            topic_key(MemKind::Decision, nfc),
            topic_key(MemKind::Decision, nfd)
        );
    }

    #[test]
    fn long_titles_are_cut_to_sixty_chars() {
        assert_eq!(slug(&"a".repeat(200)), "a".repeat(60));
        assert_eq!(slug(&"日".repeat(100)).chars().count(), 60);
    }

    #[test]
    fn the_cut_never_leaves_a_trailing_dash() {
        let title = format!("{} b", "a".repeat(59));
        assert_eq!(slug(&title), "a".repeat(59));
    }

    #[test]
    fn nothing_alphanumeric_gives_untitled() {
        for title in ["!!! ---", "", "   ", "///", "\u{200b}"] {
            assert_eq!(slug(title), "untitled", "{title:?}");
        }
    }

    #[test]
    fn topic_key_has_the_chat_prefix_and_the_kind() {
        assert_eq!(
            topic_key(MemKind::Bugfix, "Fix  the Parser"),
            "chat/bugfix/fix-the-parser"
        );
        assert_eq!(topic_key(MemKind::Config, "!!!"), "chat/config/untitled");
    }

    #[test]
    fn a_title_cannot_add_a_path_segment() {
        let key = topic_key(MemKind::Decision, "a/b\\c/../d");
        assert_eq!(key, "chat/decision/a-b-c-d");
        assert_eq!(key.matches('/').count(), 2);
    }

    #[test]
    fn same_slug_same_key() {
        assert_eq!(
            topic_key(MemKind::Decision, "Use Axum"),
            topic_key(MemKind::Decision, "use axum!")
        );
        assert_ne!(
            topic_key(MemKind::Decision, "Use Axum"),
            topic_key(MemKind::Pattern, "Use Axum")
        );
    }

    #[test]
    fn clean_title_drops_control_and_format_chars_and_trims() {
        assert_eq!(
            clean_title("  Use\u{1b}[31m Axum\n\u{202e} "),
            "Use[31m Axum"
        );
        assert_eq!(clean_title("\u{7}\t "), "");
    }

    #[test]
    fn clean_title_cuts_to_120_chars_on_a_char_boundary() {
        let cut = clean_title(&"é".repeat(300));
        assert_eq!(cut.chars().count(), 120);
        assert_eq!(clean_title(&"x".repeat(120)).len(), 120);
    }
}
