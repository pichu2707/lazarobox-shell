//! Mouse wheel decisions, pure: what one wheel tick means for the pane under
//! the pointer, and the bytes a child in mouse mode expects. Only the vertical
//! wheel is handled; the app layer owns the hit-test and applies the result.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{keys::encode_key, pane::TermModes};

/// Rows or lines one wheel tick moves a view the app owns.
pub const WHEEL_STEP: usize = 3;
/// Enables button reporting plus SGR encoding. No motion modes.
pub const CAPTURE_ON: &[u8] = b"\x1b[?1000h\x1b[?1006h";
/// Disables what [`CAPTURE_ON`] enabled.
pub const CAPTURE_OFF: &[u8] = b"\x1b[?1006l\x1b[?1000l";

/// Largest 1-based coordinate the Default (X10) encoding can carry.
const DEFAULT_MAX: u32 = 223;
/// Largest 1-based coordinate the UTF-8 (1005) encoding can carry.
const UTF8_MAX: u32 = 2015;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WheelDir {
    Up,
    Down,
}

/// Which mouse events the child asked for (mirrors the emulator's modes).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MouseMode {
    #[default]
    None,
    Press,
    PressRelease,
    ButtonMotion,
    AnyMotion,
}

impl MouseMode {
    /// Whether the child gets wheel events: the 1000-series modes. X10 (`?9`,
    /// `Press`) reports only the three buttons, so like xterm it gets none.
    fn reports_wheel(self) -> bool {
        !matches!(self, Self::None | Self::Press)
    }
}

/// How the child wants mouse events written (mirrors the emulator).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MouseEncoding {
    #[default]
    Default,
    Utf8,
    Sgr,
}

/// What a shell pane's child enabled, plus which screen it is on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MouseFacts {
    pub mode: MouseMode,
    pub encoding: MouseEncoding,
    pub alt_screen: bool,
}

/// The facts the routing needs about the pane under the pointer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaneFacts {
    Shell {
        mouse: MouseFacts,
        modes: TermModes,
        /// The pane is the one in COPY.
        copy: bool,
    },
    Chat,
}

/// What to do with one wheel tick.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum WheelAction {
    /// Bytes for the pane's PTY.
    Write(Vec<u8>),
    /// Move the main-screen scrollback view.
    Scrollback(WheelDir),
    /// Move the COPY viewport.
    CopyScroll(WheelDir),
    /// Scroll the chat transcript.
    ChatLines(WheelDir),
    Ignore,
}

/// Decides one wheel tick over a pane. `col` and `row` are the pointer cell
/// inside the pane, 0-based.
pub fn route_wheel(
    facts: PaneFacts,
    dir: WheelDir,
    col: u16,
    row: u16,
    mods: KeyModifiers,
) -> WheelAction {
    match facts {
        PaneFacts::Chat => WheelAction::ChatLines(dir),
        PaneFacts::Shell { copy: true, .. } => WheelAction::CopyScroll(dir),
        PaneFacts::Shell { mouse, .. } if mouse.mode.reports_wheel() => {
            encode_wheel(dir, col, row, mods, mouse.mode, mouse.encoding)
                .map_or(WheelAction::Ignore, WheelAction::Write)
        }
        PaneFacts::Shell { mouse, modes, .. } if mouse.alt_screen => {
            let code = match dir {
                WheelDir::Up => KeyCode::Up,
                WheelDir::Down => KeyCode::Down,
            };
            let key = KeyEvent::new(code, KeyModifiers::NONE);
            let arrow = encode_key(key, modes).unwrap_or_default();
            WheelAction::Write(arrow.repeat(WHEEL_STEP))
        }
        PaneFacts::Shell { .. } => WheelAction::Scrollback(dir),
    }
}

/// The sequence for one wheel press: button 64 (up) or 65 (down) plus the
/// modifier bits, in the child's encoding. `col` and
/// `row` are 0-based inside the pane and written 1-based. `None` when the
/// child has no wheel-reporting mode (none, or X10 `Press`) or the coordinates do not fit the encoding.
pub fn encode_wheel(
    dir: WheelDir,
    col: u16,
    row: u16,
    mods: KeyModifiers,
    mode: MouseMode,
    encoding: MouseEncoding,
) -> Option<Vec<u8>> {
    if !mode.reports_wheel() {
        return None;
    }
    let mut button: u32 = match dir {
        WheelDir::Up => 64,
        WheelDir::Down => 65,
    };
    button += 4 * u32::from(mods.contains(KeyModifiers::SHIFT))
        + 8 * u32::from(mods.contains(KeyModifiers::ALT))
        + 16 * u32::from(mods.contains(KeyModifiers::CONTROL));
    let (col, row) = (u32::from(col) + 1, u32::from(row) + 1);
    match encoding {
        MouseEncoding::Sgr => Some(format!("\x1b[<{button};{col};{row}M").into_bytes()),
        MouseEncoding::Default => {
            if col > DEFAULT_MAX || row > DEFAULT_MAX {
                return None;
            }
            // Every value is at most 255, so each is one byte.
            let bytes = [button + 32, col + 32, row + 32].map(|v| v as u8);
            Some([&b"\x1b[M"[..], &bytes].concat())
        }
        MouseEncoding::Utf8 => {
            if col > UTF8_MAX || row > UTF8_MAX {
                return None;
            }
            let mut out = b"\x1b[M".to_vec();
            for value in [button + 32, col + 32, row + 32] {
                // At most 2047, always a valid scalar value.
                let ch = char::from_u32(value)?;
                out.extend(ch.to_string().as_bytes());
            }
            Some(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: KeyModifiers = KeyModifiers::NONE;
    const MODES: TermModes = TermModes {
        application_cursor: false,
        bracketed_paste: false,
    };
    const APP_CURSOR: TermModes = TermModes {
        application_cursor: true,
        bracketed_paste: false,
    };

    fn facts(mode: MouseMode, encoding: MouseEncoding, alt_screen: bool) -> MouseFacts {
        MouseFacts {
            mode,
            encoding,
            alt_screen,
        }
    }

    fn shell(mouse: MouseFacts, modes: TermModes, copy: bool) -> PaneFacts {
        PaneFacts::Shell { mouse, modes, copy }
    }

    fn enc(
        dir: WheelDir,
        col: u16,
        row: u16,
        mods: KeyModifiers,
        mode: MouseMode,
        encoding: MouseEncoding,
    ) -> Option<Vec<u8>> {
        encode_wheel(dir, col, row, mods, mode, encoding)
    }

    // mouse-input: the capture sequences are exactly the minimal ones.
    #[test]
    fn capture_sequences_are_the_minimal_modes() {
        assert_eq!(CAPTURE_ON, b"\x1b[?1000h\x1b[?1006h");
        assert_eq!(CAPTURE_OFF, b"\x1b[?1006l\x1b[?1000l");
        assert_eq!(WHEEL_STEP, 3);
    }

    // ---- encode_wheel: golden bytes ----

    // mouse-input: SGR wheel up / down at pane-local (5, 4) 1-based.
    #[test]
    fn sgr_wheel_up_and_down_use_buttons_64_and_65() {
        let (m, e) = (MouseMode::PressRelease, MouseEncoding::Sgr);
        assert_eq!(
            enc(WheelDir::Up, 4, 3, NONE, m, e).unwrap(),
            b"\x1b[<64;5;4M"
        );
        assert_eq!(
            enc(WheelDir::Down, 4, 3, NONE, m, e).unwrap(),
            b"\x1b[<65;5;4M"
        );
        assert_eq!(
            enc(WheelDir::Up, 0, 0, NONE, m, e).unwrap(),
            b"\x1b[<64;1;1M"
        );
    }

    // mouse-input: SGR has no coordinate limit.
    #[test]
    fn sgr_carries_large_coordinates() {
        let got = enc(
            WheelDir::Down,
            2999,
            4999,
            NONE,
            MouseMode::AnyMotion,
            MouseEncoding::Sgr,
        );
        assert_eq!(got.unwrap(), b"\x1b[<65;3000;5000M");
    }

    // mouse-input: Default encoding wheel up / down.
    #[test]
    fn default_encoding_writes_three_offset_bytes() {
        let e = MouseEncoding::Default;
        assert_eq!(
            enc(WheelDir::Up, 0, 0, NONE, MouseMode::PressRelease, e).unwrap(),
            b"\x1b[M\x60\x21\x21"
        );
        assert_eq!(
            enc(WheelDir::Down, 2, 1, NONE, MouseMode::ButtonMotion, e).unwrap(),
            b"\x1b[Ma#\x22"
        );
    }

    // mouse-input: Default drops above 223 (1-based) on either axis.
    #[test]
    fn default_encoding_drops_beyond_223() {
        let (m, e) = (MouseMode::PressRelease, MouseEncoding::Default);
        let last = enc(WheelDir::Up, 222, 222, NONE, m, e).unwrap();
        assert_eq!(last, [0x1b, b'[', b'M', 0x60, 255, 255]);
        assert_eq!(enc(WheelDir::Up, 223, 0, NONE, m, e), None);
        assert_eq!(enc(WheelDir::Up, 0, 223, NONE, m, e), None);
    }

    // mouse-input: UTF-8 encoding writes 32+value as a character.
    #[test]
    fn utf8_encoding_writes_characters() {
        let (m, e) = (MouseMode::PressRelease, MouseEncoding::Utf8);
        let got = enc(WheelDir::Up, 299, 0, NONE, m, e).unwrap();
        let mut want = b"\x1b[M\x60".to_vec();
        want.extend("\u{14C}".as_bytes());
        want.push(0x21);
        assert_eq!(got, want);
        // Column 223 (1-based) is value 255: already two bytes.
        let got = enc(WheelDir::Up, 222, 0, NONE, m, e).unwrap();
        assert_eq!(got, b"\x1b[M\x60\xc3\xbf\x21");
    }

    // mouse-input: UTF-8 drops above 2015.
    #[test]
    fn utf8_encoding_drops_beyond_2015() {
        let (m, e) = (MouseMode::AnyMotion, MouseEncoding::Utf8);
        assert!(enc(WheelDir::Up, 2014, 2014, NONE, m, e).is_some());
        assert_eq!(enc(WheelDir::Up, 2015, 0, NONE, m, e), None);
        assert_eq!(enc(WheelDir::Up, 0, 2015, NONE, m, e), None);
    }

    // mouse-input: Shift +4, Alt +8, Ctrl +16 on the button.
    #[test]
    fn modifier_bits_are_added_to_the_button() {
        let (m, e) = (MouseMode::PressRelease, MouseEncoding::Sgr);
        let cases = [
            (KeyModifiers::SHIFT, "68"),
            (KeyModifiers::ALT, "72"),
            (KeyModifiers::CONTROL, "80"),
            (KeyModifiers::SHIFT | KeyModifiers::CONTROL, "84"),
            (
                KeyModifiers::SHIFT | KeyModifiers::ALT | KeyModifiers::CONTROL,
                "92",
            ),
        ];
        for (mods, button) in cases {
            let got = enc(WheelDir::Up, 0, 0, mods, m, e).unwrap();
            assert_eq!(got, format!("\x1b[<{button};1;1M").into_bytes(), "{mods:?}");
        }
        let got = enc(WheelDir::Down, 0, 0, KeyModifiers::CONTROL, m, e).unwrap();
        assert_eq!(got, b"\x1b[<81;1;1M");
    }

    // mouse-input: modifier bits also apply to the legacy encodings.
    #[test]
    fn modifier_bits_apply_to_the_legacy_encodings() {
        let got = enc(
            WheelDir::Up,
            0,
            0,
            KeyModifiers::CONTROL,
            MouseMode::PressRelease,
            MouseEncoding::Default,
        );
        // 32 + 64 + 16 = 112 = 'p'.
        assert_eq!(got.unwrap(), b"\x1b[Mp\x21\x21");
    }

    // mouse-input: Press (X10, ?9) mode has no wheel: nothing is encoded.
    #[test]
    fn press_mode_encodes_nothing() {
        for encoding in [
            MouseEncoding::Default,
            MouseEncoding::Utf8,
            MouseEncoding::Sgr,
        ] {
            assert_eq!(
                enc(WheelDir::Up, 0, 0, NONE, MouseMode::Press, encoding),
                None
            );
        }
    }

    // mouse-input: modifier bits apply in every mode that gets the wheel.
    #[test]
    fn modifier_bits_apply_in_every_wheel_mode() {
        let all = KeyModifiers::SHIFT | KeyModifiers::ALT | KeyModifiers::CONTROL;
        for mode in [
            MouseMode::PressRelease,
            MouseMode::ButtonMotion,
            MouseMode::AnyMotion,
        ] {
            let got = enc(WheelDir::Up, 0, 0, all, mode, MouseEncoding::Sgr);
            assert_eq!(got.unwrap(), b"\x1b[<92;1;1M", "{mode:?}");
        }
    }

    // mouse-input: every mouse mode receives one sequence; None encodes nothing.
    #[test]
    fn every_wheel_mode_receives_one_sequence() {
        for mode in [
            MouseMode::PressRelease,
            MouseMode::ButtonMotion,
            MouseMode::AnyMotion,
        ] {
            let got = enc(WheelDir::Up, 0, 0, NONE, mode, MouseEncoding::Sgr).unwrap();
            assert_eq!(got, b"\x1b[<64;1;1M", "{mode:?}");
        }
        for encoding in [
            MouseEncoding::Default,
            MouseEncoding::Utf8,
            MouseEncoding::Sgr,
        ] {
            assert_eq!(
                enc(WheelDir::Up, 0, 0, NONE, MouseMode::None, encoding),
                None
            );
        }
    }

    // ---- route_wheel ----

    // mouse-input: COPY wins over everything on a shell pane.
    #[test]
    fn copy_scrolls_the_copy_viewport_first() {
        let busy = facts(MouseMode::AnyMotion, MouseEncoding::Sgr, true);
        for mouse in [MouseFacts::default(), busy] {
            let f = shell(mouse, MODES, true);
            assert_eq!(
                route_wheel(f, WheelDir::Up, 0, 0, NONE),
                WheelAction::CopyScroll(WheelDir::Up)
            );
            assert_eq!(
                route_wheel(f, WheelDir::Down, 0, 0, NONE),
                WheelAction::CopyScroll(WheelDir::Down)
            );
        }
    }

    // mouse-input: a mouse mode wins over scrollback and over the alt screen.
    #[test]
    fn a_mouse_mode_forwards_the_wheel_on_either_screen() {
        for alt in [false, true] {
            let f = shell(
                facts(MouseMode::PressRelease, MouseEncoding::Sgr, alt),
                MODES,
                false,
            );
            assert_eq!(
                route_wheel(f, WheelDir::Up, 4, 3, NONE),
                WheelAction::Write(b"\x1b[<64;5;4M".to_vec()),
                "alt {alt}"
            );
        }
    }

    // mouse-input: modifiers travel to the child; one sequence per event.
    #[test]
    fn forwarded_wheel_carries_modifiers_once() {
        let f = shell(
            facts(MouseMode::ButtonMotion, MouseEncoding::Sgr, false),
            MODES,
            false,
        );
        assert_eq!(
            route_wheel(f, WheelDir::Up, 0, 0, KeyModifiers::CONTROL),
            WheelAction::Write(b"\x1b[<80;1;1M".to_vec())
        );
    }

    // mouse-input: a coordinate the encoding cannot carry drops the event.
    #[test]
    fn an_unencodable_coordinate_is_ignored() {
        let f = shell(
            facts(MouseMode::PressRelease, MouseEncoding::Default, false),
            MODES,
            false,
        );
        assert_eq!(
            route_wheel(f, WheelDir::Up, 223, 0, NONE),
            WheelAction::Ignore
        );
    }

    // mouse-input: alt screen without a mouse mode sends three arrow keys.
    #[test]
    fn the_alt_screen_gets_three_arrows() {
        let alt = facts(MouseMode::None, MouseEncoding::Default, true);
        assert_eq!(
            route_wheel(shell(alt, MODES, false), WheelDir::Down, 0, 0, NONE),
            WheelAction::Write(b"\x1b[B\x1b[B\x1b[B".to_vec())
        );
        assert_eq!(
            route_wheel(shell(alt, MODES, false), WheelDir::Up, 0, 0, NONE),
            WheelAction::Write(b"\x1b[A\x1b[A\x1b[A".to_vec())
        );
    }

    // mouse-input: the arrows honor application cursor mode (DECCKM).
    #[test]
    fn the_arrows_honor_application_cursor_mode() {
        let alt = facts(MouseMode::None, MouseEncoding::Default, true);
        assert_eq!(
            route_wheel(shell(alt, APP_CURSOR, false), WheelDir::Up, 0, 0, NONE),
            WheelAction::Write(b"\x1bOA\x1bOA\x1bOA".to_vec())
        );
        assert_eq!(
            route_wheel(shell(alt, APP_CURSOR, false), WheelDir::Down, 0, 0, NONE),
            WheelAction::Write(b"\x1bOB\x1bOB\x1bOB".to_vec())
        );
    }

    // mouse-input: modifiers do not change the arrows.
    #[test]
    fn modifiers_do_not_change_the_arrows() {
        let alt = facts(MouseMode::None, MouseEncoding::Default, true);
        assert_eq!(
            route_wheel(
                shell(alt, MODES, false),
                WheelDir::Up,
                0,
                0,
                KeyModifiers::CONTROL
            ),
            WheelAction::Write(b"\x1b[A\x1b[A\x1b[A".to_vec())
        );
    }

    // mouse-input: Press mode falls through, like xterm: arrows or scrollback.
    #[test]
    fn press_mode_falls_through_to_the_screen_rules() {
        let alt = facts(MouseMode::Press, MouseEncoding::Sgr, true);
        assert_eq!(
            route_wheel(shell(alt, MODES, false), WheelDir::Down, 0, 0, NONE),
            WheelAction::Write(b"\x1b[B\x1b[B\x1b[B".to_vec())
        );
        let main = facts(MouseMode::Press, MouseEncoding::Sgr, false);
        assert_eq!(
            route_wheel(shell(main, MODES, false), WheelDir::Up, 0, 0, NONE),
            WheelAction::Scrollback(WheelDir::Up)
        );
    }

    // mouse-input: the main screen without a mouse mode moves the view.
    #[test]
    fn the_main_screen_scrolls_back() {
        let f = shell(MouseFacts::default(), MODES, false);
        assert_eq!(
            route_wheel(f, WheelDir::Up, 9, 9, NONE),
            WheelAction::Scrollback(WheelDir::Up)
        );
        assert_eq!(
            route_wheel(f, WheelDir::Down, 0, 0, KeyModifiers::SHIFT),
            WheelAction::Scrollback(WheelDir::Down)
        );
    }

    // mouse-input: a chat scrolls its transcript, whatever its COPY state.
    #[test]
    fn a_chat_scrolls_lines() {
        assert_eq!(
            route_wheel(PaneFacts::Chat, WheelDir::Up, 3, 3, NONE),
            WheelAction::ChatLines(WheelDir::Up)
        );
        assert_eq!(
            route_wheel(PaneFacts::Chat, WheelDir::Down, 0, 0, KeyModifiers::ALT),
            WheelAction::ChatLines(WheelDir::Down)
        );
    }

    // Keys used by the arrows are the plain ones.
    #[test]
    fn the_arrow_keys_are_unmodified() {
        let up = KeyEvent::new(KeyCode::Up, NONE);
        assert_eq!(encode_key(up, MODES).unwrap(), b"\x1b[A");
    }
}
