//! Pure key and paste encoder: turns crossterm events into the bytes a PTY child expects.
//!
//! No IO, no state. Only xterm-style sequences are produced; the Kitty keyboard
//! protocol is deliberately not used. Prefix and quit handling belong to the app layer.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::core::pane::TermModes;

const ESC: u8 = 0x1b;
const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &str = "\x1b[201~";

/// Encode a key event; `None` when the event has no PTY representation.
///
/// Only `Press` and `Repeat` are encoded: a terminal child has no notion of key
/// release without the Kitty protocol, so `Release` yields `None`. Keys with no
/// xterm sequence (media, modifier-only, `Null`, F13+) also yield `None`.
pub fn encode_key(key: KeyEvent, modes: TermModes) -> Option<Vec<u8>> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    let mods = key.modifiers;
    let shift = mods.contains(KeyModifiers::SHIFT);
    let alt = mods.contains(KeyModifiers::ALT);
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    // xterm modifier parameter: 1 + shift(1) + alt(2) + ctrl(4).
    let param = 1 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(ctrl);
    let modified = param > 1;

    // Keys with a CSI form carry modifiers in the sequence; no ESC prefix on top.
    let csi = |code: &str, final_byte: char| -> Vec<u8> {
        if modified {
            format!("\x1b[{code};{param}{final_byte}").into_bytes()
        } else {
            format!("\x1b[{}{final_byte}", if code == "1" { "" } else { code }).into_bytes()
        }
    };
    let cursor = |final_byte: char| -> Vec<u8> {
        if !modified && modes.application_cursor {
            vec![ESC, b'O', final_byte as u8]
        } else {
            csi("1", final_byte)
        }
    };

    let base: Vec<u8> = match key.code {
        KeyCode::Up => return Some(cursor('A')),
        KeyCode::Down => return Some(cursor('B')),
        KeyCode::Right => return Some(cursor('C')),
        KeyCode::Left => return Some(cursor('D')),
        KeyCode::Home => return Some(cursor('H')),
        KeyCode::End => return Some(cursor('F')),
        KeyCode::Insert => return Some(csi("2", '~')),
        KeyCode::Delete => return Some(csi("3", '~')),
        KeyCode::PageUp => return Some(csi("5", '~')),
        KeyCode::PageDown => return Some(csi("6", '~')),
        KeyCode::F(n @ 1..=4) => {
            let final_byte = [b'P', b'Q', b'R', b'S'][usize::from(n) - 1] as char;
            return Some(if modified {
                csi("1", final_byte)
            } else {
                vec![ESC, b'O', final_byte as u8]
            });
        }
        KeyCode::F(n @ 5..=12) => {
            let code = [15, 17, 18, 19, 20, 21, 23, 24][usize::from(n) - 5];
            return Some(csi(&code.to_string(), '~'));
        }
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Tab if shift => b"\x1b[Z".to_vec(),
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Enter => vec![b'\r'],
        // Ctrl+Backspace sends BS (0x08), the common xterm convention.
        KeyCode::Backspace if ctrl => vec![0x08],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![ESC],
        KeyCode::Char(ch) => char_bytes(ch, ctrl),
        _ => return None,
    };

    // Alt is an ESC prefix on top of whatever the key would send.
    Some(if alt {
        let mut out = Vec::with_capacity(base.len() + 1);
        out.push(ESC);
        out.extend(base);
        out
    } else {
        base
    })
}

/// Bytes for a character key; Ctrl maps to C0 codes where xterm defines one.
fn char_bytes(ch: char, ctrl: bool) -> Vec<u8> {
    if ctrl {
        let code = match ch {
            'a'..='z' | 'A'..='Z' => Some(ch.to_ascii_lowercase() as u8 - b'a' + 1),
            // crossterm reports 0x00 as Ctrl+Space and 0x1C..=0x1F as Ctrl+'4'..='7',
            // so the xterm Ctrl+digit mapping is what makes Ctrl+\\ etc. work.
            // Ctrl+1, Ctrl+9 and Ctrl+0 have no code and send the plain char.
            ' ' | '@' | '2' => Some(0x00),
            '[' | '3' => Some(0x1b),
            '\\' | '4' => Some(0x1c),
            ']' | '5' => Some(0x1d),
            '^' | '6' => Some(0x1e),
            '_' | '7' | '/' => Some(0x1f),
            '?' | '8' => Some(0x7f),
            _ => None,
        };
        if let Some(code) = code {
            return vec![code];
        }
    }
    let mut buf = [0u8; 4];
    ch.encode_utf8(&mut buf).as_bytes().to_vec()
}

/// Encode pasted text, wrapping it in bracketed-paste markers when the child asked for them.
///
/// When bracketed, any embedded end marker is removed (to a fixpoint, so a marker
/// re-assembled by a removal is removed too) so pasted content cannot break out of the
/// paste and inject commands. Otherwise the text is sent as if typed: `\r\n` and lone
/// `\n` become `\r` (the Enter key), following the xterm/tmux convention.
pub fn encode_paste(text: &str, modes: TermModes) -> Vec<u8> {
    if !modes.bracketed_paste {
        return normalize_newlines(text);
    }
    let clean = strip_marker(text.as_bytes(), PASTE_END.as_bytes());
    let mut out = Vec::with_capacity(clean.len() + 12);
    out.extend_from_slice(PASTE_START);
    out.extend_from_slice(&clean);
    out.extend_from_slice(PASTE_END.as_bytes());
    out
}

fn normalize_newlines(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut bytes = text.bytes().peekable();
    while let Some(b) = bytes.next() {
        match b {
            b'\r' => {
                out.push(b'\r');
                if bytes.peek() == Some(&b'\n') {
                    bytes.next();
                }
            }
            b'\n' => out.push(b'\r'),
            _ => out.push(b),
        }
    }
    out
}

/// Remove every occurrence of `marker` in a single linear pass, including occurrences
/// that only appear once an earlier one is removed (same result as repeating
/// `replace` until none is left). Output is pushed on a stack and truncated whenever its
/// tail equals the marker. The marker is ASCII, so UTF-8 validity is preserved.
fn strip_marker(input: &[u8], marker: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    for &b in input {
        out.push(b);
        if out.ends_with(marker) {
            out.truncate(out.len() - marker.len());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyEventState;

    const NORMAL: TermModes = TermModes {
        application_cursor: false,
        bracketed_paste: false,
    };
    const APP: TermModes = TermModes {
        application_cursor: true,
        bracketed_paste: false,
    };
    const BRACKETED: TermModes = TermModes {
        application_cursor: false,
        bracketed_paste: true,
    };

    const NONE: KeyModifiers = KeyModifiers::NONE;
    const SHIFT: KeyModifiers = KeyModifiers::SHIFT;
    const ALT: KeyModifiers = KeyModifiers::ALT;
    const CTRL: KeyModifiers = KeyModifiers::CONTROL;

    fn enc(code: KeyCode, mods: KeyModifiers, modes: TermModes) -> Option<Vec<u8>> {
        encode_key(KeyEvent::new(code, mods), modes)
    }

    fn c(ch: char) -> KeyCode {
        KeyCode::Char(ch)
    }

    fn check(table: &[(KeyCode, KeyModifiers, &[u8])], modes: TermModes) {
        for (code, mods, want) in table {
            assert_eq!(
                enc(*code, *mods, modes),
                Some(want.to_vec()),
                "{code:?} + {mods:?}"
            );
        }
    }

    #[test]
    fn printable_chars_are_utf8() {
        check(
            &[
                (c('a'), NONE, b"a"),
                (c('A'), SHIFT, b"A"),
                (c('A'), NONE, b"A"),
                (c('é'), NONE, "é".as_bytes()),
                (c('ñ'), NONE, "ñ".as_bytes()),
                (c('€'), NONE, "€".as_bytes()),
                (c('🦀'), NONE, "🦀".as_bytes()),
                (c(' '), NONE, b" "),
                (c('~'), SHIFT, b"~"),
            ],
            NORMAL,
        );
    }

    #[test]
    fn ctrl_letters_map_to_control_codes() {
        for (i, ch) in ('a'..='z').enumerate() {
            let want = vec![i as u8 + 1];
            assert_eq!(enc(c(ch), CTRL, NORMAL), Some(want.clone()), "ctrl+{ch}");
            let upper = ch.to_ascii_uppercase();
            assert_eq!(
                enc(c(upper), CTRL, NORMAL),
                Some(want.clone()),
                "ctrl+{upper}"
            );
            assert_eq!(
                enc(c(upper), CTRL | SHIFT, NORMAL),
                Some(want),
                "ctrl+shift+{upper}"
            );
        }
        check(&[(c('c'), CTRL, &[0x03])], NORMAL);
    }

    #[test]
    fn ctrl_punctuation_and_space() {
        check(
            &[
                (c(' '), CTRL, &[0x00]),
                (c('@'), CTRL, &[0x00]),
                (c('['), CTRL, &[0x1b]),
                (c('\\'), CTRL, &[0x1c]),
                (c(']'), CTRL, &[0x1d]),
                (c('^'), CTRL, &[0x1e]),
                (c('_'), CTRL, &[0x1f]),
                (c('?'), CTRL, &[0x7f]),
            ],
            NORMAL,
        );
    }

    #[test]
    fn ctrl_digits_follow_xterm() {
        check(
            &[
                (c('2'), CTRL, &[0x00]),
                (c('3'), CTRL, &[0x1b]),
                (c('4'), CTRL, &[0x1c]),
                (c('5'), CTRL, &[0x1d]),
                (c('6'), CTRL, &[0x1e]),
                (c('7'), CTRL, &[0x1f]),
                (c('8'), CTRL, &[0x7f]),
                (c('/'), CTRL, &[0x1f]),
                (KeyCode::Char('4'), ALT | CTRL, &[0x1b, 0x1c]),
            ],
            NORMAL,
        );
    }

    #[test]
    fn crossterm_reports_ctrl_backslash_as_ctrl_4() {
        // Real-world case: crossterm 0.29 parses byte 0x1C (Ctrl+\) as Char('4') + CONTROL.
        // It must reach the child as 0x1C (SIGQUIT), not as the literal "4".
        assert_eq!(enc(c('4'), CTRL, NORMAL), Some(vec![0x1c]));
        // 0x00 arrives as Char(' ') + CONTROL.
        assert_eq!(enc(c(' '), CTRL, NORMAL), Some(vec![0x00]));
    }

    #[test]
    fn ctrl_with_unmapped_char_sends_the_char() {
        // Ctrl+1, Ctrl+9 and Ctrl+0 have no xterm control code: the plain char is sent.
        check(
            &[
                (c('1'), CTRL, b"1"),
                (c('9'), CTRL, b"9"),
                (c('0'), CTRL, b"0"),
                (c('é'), CTRL, "é".as_bytes()),
            ],
            NORMAL,
        );
    }

    #[test]
    fn documented_limitations_are_pinned() {
        check(
            &[
                // No distinct encoding without the Kitty protocol.
                (KeyCode::Enter, CTRL, b"\r"),
                (KeyCode::Tab, CTRL, b"\t"),
                // Alt prefix on top of the Shift+Tab sequence.
                (KeyCode::BackTab, ALT | SHIFT, b"\x1b\x1b[Z"),
                (KeyCode::Backspace, CTRL | ALT, &[0x1b, 0x08]),
            ],
            NORMAL,
        );
    }

    #[test]
    fn alt_prefixes_escape() {
        check(
            &[
                (c('x'), ALT, b"\x1bx"),
                (c('X'), ALT | SHIFT, b"\x1bX"),
                (c('x'), ALT | CTRL, b"\x1b\x18"),
                (c('é'), ALT, "\u{1b}é".as_bytes()),
                (KeyCode::Enter, ALT, b"\x1b\r"),
                (KeyCode::Backspace, ALT, b"\x1b\x7f"),
                (KeyCode::Esc, ALT, b"\x1b\x1b"),
                (KeyCode::BackTab, ALT, b"\x1b\x1b[Z"),
            ],
            NORMAL,
        );
    }

    #[test]
    fn simple_keys() {
        check(
            &[
                (KeyCode::Enter, NONE, b"\r"),
                (KeyCode::Enter, SHIFT, b"\r"),
                (KeyCode::Tab, NONE, b"\t"),
                (KeyCode::Tab, SHIFT, b"\x1b[Z"),
                (KeyCode::BackTab, SHIFT, b"\x1b[Z"),
                (KeyCode::BackTab, NONE, b"\x1b[Z"),
                (KeyCode::Backspace, NONE, &[0x7f]),
                (KeyCode::Backspace, CTRL, &[0x08]),
                (KeyCode::Esc, NONE, &[0x1b]),
            ],
            NORMAL,
        );
    }

    #[test]
    fn navigation_keys_normal() {
        check(
            &[
                (KeyCode::Home, NONE, b"\x1b[H"),
                (KeyCode::End, NONE, b"\x1b[F"),
                (KeyCode::PageUp, NONE, b"\x1b[5~"),
                (KeyCode::PageDown, NONE, b"\x1b[6~"),
                (KeyCode::Insert, NONE, b"\x1b[2~"),
                (KeyCode::Delete, NONE, b"\x1b[3~"),
            ],
            NORMAL,
        );
    }

    #[test]
    fn home_end_follow_application_cursor() {
        check(
            &[
                (KeyCode::Home, NONE, b"\x1bOH"),
                (KeyCode::End, NONE, b"\x1bOF"),
                // PageUp/Down/Insert/Delete are not affected by the mode.
                (KeyCode::PageUp, NONE, b"\x1b[5~"),
                (KeyCode::Delete, NONE, b"\x1b[3~"),
            ],
            APP,
        );
    }

    #[test]
    fn function_keys() {
        check(
            &[
                (KeyCode::F(1), NONE, b"\x1bOP"),
                (KeyCode::F(2), NONE, b"\x1bOQ"),
                (KeyCode::F(3), NONE, b"\x1bOR"),
                (KeyCode::F(4), NONE, b"\x1bOS"),
                (KeyCode::F(5), NONE, b"\x1b[15~"),
                (KeyCode::F(6), NONE, b"\x1b[17~"),
                (KeyCode::F(7), NONE, b"\x1b[18~"),
                (KeyCode::F(8), NONE, b"\x1b[19~"),
                (KeyCode::F(9), NONE, b"\x1b[20~"),
                (KeyCode::F(10), NONE, b"\x1b[21~"),
                (KeyCode::F(11), NONE, b"\x1b[23~"),
                (KeyCode::F(12), NONE, b"\x1b[24~"),
            ],
            NORMAL,
        );
        // Application cursor does not affect F-keys.
        assert_eq!(enc(KeyCode::F(1), NONE, APP), Some(b"\x1bOP".to_vec()));
        assert_eq!(enc(KeyCode::F(5), NONE, APP), Some(b"\x1b[15~".to_vec()));
    }

    #[test]
    fn modified_function_keys() {
        check(
            &[
                (KeyCode::F(1), SHIFT, b"\x1b[1;2P"),
                (KeyCode::F(4), CTRL, b"\x1b[1;5S"),
                (KeyCode::F(5), SHIFT, b"\x1b[15;2~"),
                (KeyCode::F(12), CTRL | ALT, b"\x1b[24;7~"),
            ],
            NORMAL,
        );
    }

    #[test]
    fn modified_navigation_keys() {
        check(
            &[
                (KeyCode::PageUp, SHIFT, b"\x1b[5;2~"),
                (KeyCode::PageDown, CTRL, b"\x1b[6;5~"),
                (KeyCode::Insert, ALT, b"\x1b[2;3~"),
                (KeyCode::Delete, CTRL | SHIFT, b"\x1b[3;6~"),
                (KeyCode::Home, SHIFT, b"\x1b[1;2H"),
                (KeyCode::End, CTRL, b"\x1b[1;5F"),
            ],
            NORMAL,
        );
        // Application cursor does not apply when modifiers are present.
        assert_eq!(enc(KeyCode::Home, CTRL, APP), Some(b"\x1b[1;5H".to_vec()));
    }

    #[test]
    fn arrows_normal_mode() {
        check(
            &[
                (KeyCode::Up, NONE, b"\x1b[A"),
                (KeyCode::Down, NONE, b"\x1b[B"),
                (KeyCode::Right, NONE, b"\x1b[C"),
                (KeyCode::Left, NONE, b"\x1b[D"),
            ],
            NORMAL,
        );
    }

    #[test]
    fn arrows_application_mode() {
        check(
            &[
                (KeyCode::Up, NONE, b"\x1bOA"),
                (KeyCode::Down, NONE, b"\x1bOB"),
                (KeyCode::Right, NONE, b"\x1bOC"),
                (KeyCode::Left, NONE, b"\x1bOD"),
            ],
            APP,
        );
    }

    #[test]
    fn modified_arrows_ignore_application_mode() {
        for modes in [NORMAL, APP] {
            check(
                &[
                    (KeyCode::Up, SHIFT, b"\x1b[1;2A"),
                    (KeyCode::Down, ALT, b"\x1b[1;3B"),
                    (KeyCode::Right, CTRL, b"\x1b[1;5C"),
                    (KeyCode::Left, CTRL | SHIFT, b"\x1b[1;6D"),
                    (KeyCode::Left, CTRL | ALT | SHIFT, b"\x1b[1;8D"),
                ],
                modes,
            );
        }
    }

    #[test]
    fn only_press_and_repeat_are_encoded() {
        let ev = |kind| KeyEvent {
            code: c('a'),
            modifiers: NONE,
            kind,
            state: KeyEventState::NONE,
        };
        assert_eq!(
            encode_key(ev(KeyEventKind::Press), NORMAL),
            Some(b"a".to_vec())
        );
        assert_eq!(
            encode_key(ev(KeyEventKind::Repeat), NORMAL),
            Some(b"a".to_vec())
        );
        assert_eq!(encode_key(ev(KeyEventKind::Release), NORMAL), None);
    }

    #[test]
    fn unsupported_keys_yield_none() {
        use ratatui::crossterm::event::{MediaKeyCode, ModifierKeyCode};
        for code in [
            KeyCode::Null,
            KeyCode::CapsLock,
            KeyCode::F(13),
            KeyCode::Media(MediaKeyCode::Play),
            KeyCode::Modifier(ModifierKeyCode::LeftShift),
        ] {
            assert_eq!(enc(code, NONE, NORMAL), None, "{code:?}");
        }
    }

    #[test]
    fn paste_bracketed_wraps_text() {
        assert_eq!(
            encode_paste("hi", BRACKETED),
            b"\x1b[200~hi\x1b[201~".to_vec()
        );
        assert_eq!(encode_paste("", BRACKETED), b"\x1b[200~\x1b[201~".to_vec());
        assert_eq!(
            encode_paste("a\nb é", BRACKETED),
            "\u{1b}[200~a\nb é\u{1b}[201~".as_bytes().to_vec()
        );
    }

    #[test]
    fn paste_unbracketed_normalizes_newlines_to_cr() {
        assert_eq!(encode_paste("hi", NORMAL), b"hi".to_vec());
        assert_eq!(encode_paste("a\nb", APP), b"a\rb".to_vec());
        assert_eq!(encode_paste("a\r\nb", NORMAL), b"a\rb".to_vec());
        assert_eq!(encode_paste("a\r\n\nb\r", NORMAL), b"a\r\rb\r".to_vec());
        assert_eq!(encode_paste("é\n€", NORMAL), "é\r€".as_bytes().to_vec());
    }

    #[test]
    fn bracketed_paste_keeps_newlines_and_other_escapes() {
        assert_eq!(
            encode_paste("a\r\nb\x1b[31mc\x1b[200~d", BRACKETED),
            b"\x1b[200~a\r\nb\x1b[31mc\x1b[200~d\x1b[201~".to_vec()
        );
    }

    #[test]
    fn bracketed_paste_strips_deeply_nested_reassembly() {
        let depth = 50;
        let mut text = String::new();
        for _ in 0..depth {
            text.push_str("\x1b[2");
        }
        text.push_str(PASTE_END);
        // Closing tails "01~" re-assemble one marker per level.
        for _ in 0..depth {
            text.push_str("01~");
        }
        // Each removal re-assembles the next marker outward, so all 51 must vanish.
        let out = encode_paste(&text, BRACKETED);
        assert_eq!(out, b"\x1b[200~\x1b[201~".to_vec());
    }

    #[test]
    fn bracketed_paste_strips_embedded_end_marker() {
        assert_eq!(
            encode_paste("a\x1b[201~rm -rf /", BRACKETED),
            b"\x1b[200~arm -rf /\x1b[201~".to_vec()
        );
        // Re-assembly attempts are neutralised too.
        assert_eq!(
            encode_paste("x\x1b[2\x1b[201~01~y", BRACKETED),
            b"\x1b[200~xy\x1b[201~".to_vec()
        );
    }
}
