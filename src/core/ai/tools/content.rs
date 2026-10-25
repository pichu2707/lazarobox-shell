//! What a model may write into a file. Pure: no IO.
//!
//! Content is refused, never cleaned: the review shows the text that would be
//! written, so a rewrite behind the user's back is not an option. A write is
//! refused for any control, invisible or line-ending character that could hide
//! text from a reviewer.

use std::fmt;

use crate::core::ai::sanitize::is_format;

/// Why a text was refused. Each kind is a short fixed message: it goes back to
/// a model, and the offending character is never echoed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ContentError {
    /// A control character other than tab and newline: C0, DEL or C1.
    Control,
    /// A carriage return: CRLF or bare CR line endings.
    CarriageReturn,
    /// A bidi, zero-width or other invisible format character.
    Invisible,
    /// An existing file that is not UTF-8 text, or holds a NUL.
    NotText,
}

impl fmt::Display for ContentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ContentError::Control => "content holds a control character",
            ContentError::CarriageReturn => "content holds a carriage return; use LF line endings",
            ContentError::Invisible => "content holds an invisible or bidi character",
            ContentError::NotText => "the file is not UTF-8 text without NUL",
        })
    }
}

/// Specials block characters that `is_format` does not list: the interlinear
/// annotation marks U+FFF9..U+FFFB.
fn is_hidden(c: char) -> bool {
    is_format(c) || matches!(c, '\u{180e}' | '\u{fff9}'..='\u{fffb}')
}

/// Whether `text` may be written as it is. It is never changed, and no final
/// newline is added or required.
pub fn check_new(text: &str) -> Result<(), ContentError> {
    for c in text.chars() {
        match c {
            '\r' => return Err(ContentError::CarriageReturn),
            '\t' | '\n' => {}
            c if c.is_control() => return Err(ContentError::Control),
            c if is_hidden(c) => return Err(ContentError::Invisible),
            _ => {}
        }
    }
    Ok(())
}

/// The text of an existing file that an edit or an overwrite may start from:
/// UTF-8, with no NUL and no carriage return. Other controls are fine, the
/// review shows them as `<U+XXXX>`.
pub fn check_existing(bytes: &[u8]) -> Result<&str, ContentError> {
    let text = std::str::from_utf8(bytes).map_err(|_| ContentError::NotText)?;
    if text.contains('\0') {
        return Err(ContentError::NotText);
    }
    if text.contains('\r') {
        return Err(ContentError::CarriageReturn);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_with_tabs_and_newlines_is_accepted_as_it_is() {
        for ok in [
            "",
            "fn main() {\n\tprintln!(\"hi\");\n}\n",
            "no newline at the end",
            "accents \u{e9} \u{4e2d}\u{6587} \u{1f980}",
            "\n\n\t\t",
        ] {
            assert_eq!(check_new(ok), Ok(()), "{ok:?}");
        }
    }

    #[test]
    fn a_missing_final_newline_is_not_a_reason_to_refuse_and_nothing_adds_one() {
        let text = "last line";
        assert_eq!(check_new(text), Ok(()));
        assert_eq!(check_existing(text.as_bytes()), Ok(text));
    }

    #[test]
    fn c0_controls_other_than_tab_and_newline_are_refused() {
        for code in (0u32..0x20).filter(|c| *c != 9 && *c != 10 && *c != 13) {
            let text = format!("a{}b", char::from_u32(code).unwrap());
            assert_eq!(check_new(&text), Err(ContentError::Control), "U+{code:04X}");
        }
    }

    #[test]
    fn del_and_the_c1_block_are_refused() {
        for code in std::iter::once(0x7fu32).chain(0x80..=0x9f) {
            let text = format!("a{}b", char::from_u32(code).unwrap());
            assert_eq!(check_new(&text), Err(ContentError::Control), "U+{code:04X}");
        }
        // The edges of the block are ordinary characters.
        assert_eq!(check_new("\u{7e}\u{a0}\u{a1}"), Ok(()));
    }

    #[test]
    fn a_carriage_return_is_its_own_refusal() {
        assert_eq!(check_new("a\r\nb"), Err(ContentError::CarriageReturn));
        assert_eq!(check_new("a\rb"), Err(ContentError::CarriageReturn));
    }

    #[test]
    fn the_fixed_invisible_list_is_refused_including_the_joiner() {
        let listed = [
            0x00adu32, 0x061c, 0x180e, 0x200b, 0x200c, 0x200d, 0x200e, 0x200f, 0x202a, 0x202b,
            0x202c, 0x202d, 0x202e, 0x2060, 0x2061, 0x2062, 0x2063, 0x2064, 0x2066, 0x2067, 0x2068,
            0x2069, 0xfeff, 0xfff9, 0xfffa, 0xfffb, 0xe0000, 0xe0041, 0xe007f,
        ];
        for code in listed {
            let text = format!("a{}b", char::from_u32(code).unwrap());
            assert_eq!(
                check_new(&text),
                Err(ContentError::Invisible),
                "U+{code:04X}"
            );
        }
    }

    #[test]
    fn characters_next_to_the_invisible_ranges_stay_allowed() {
        for code in [
            0x00ac_u32, 0x00ae, 0x061b, 0x061d, 0x2010, 0x205f, 0x2065, 0x2070, 0xfff8, 0xfffc,
            0xe0080,
        ] {
            let text = format!("a{}b", char::from_u32(code).unwrap());
            assert_eq!(check_new(&text), Ok(()), "U+{code:04X}");
        }
    }

    #[test]
    fn a_refusal_names_its_kind_without_echoing_the_character() {
        for (err, word) in [
            (ContentError::Control, "control"),
            (ContentError::CarriageReturn, "line ending"),
            (ContentError::Invisible, "invisible"),
            (ContentError::NotText, "UTF-8"),
        ] {
            let text = err.to_string();
            assert!(text.contains(word), "{text}");
            assert!(text.chars().all(|c| !c.is_control()), "{text:?}");
        }
    }

    #[test]
    fn an_existing_file_must_be_utf8_without_cr_or_nul() {
        assert_eq!(check_existing(b"fn a() {}\n"), Ok("fn a() {}\n"));
        assert_eq!(check_existing(b"caf\xc3\xa9"), Ok("caf\u{e9}"));
        // Latin-1 is not UTF-8.
        assert_eq!(check_existing(b"caf\xe9"), Err(ContentError::NotText));
        assert_eq!(check_existing(b"a\0b"), Err(ContentError::NotText));
        assert_eq!(check_existing(b"a\r\nb"), Err(ContentError::CarriageReturn));
    }

    #[test]
    fn an_existing_file_may_hold_other_controls_the_review_will_show() {
        // The review prints these as `<U+XXXX>`; only CR and NUL block an edit.
        assert_eq!(check_existing(b"a\x1b[0mb"), Ok("a\u{1b}[0mb"));
        assert_eq!(check_existing("a\u{202e}b".as_bytes()), Ok("a\u{202e}b"));
    }
}
