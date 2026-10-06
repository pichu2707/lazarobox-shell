//! Copy mode: vim-style, read-only navigation of the scrollback.
//!
//! The offset counts rows above the live screen (0 = bottom), matching
//! `Pane::set_scrollback`. This module is pure; the app applies the offset.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopyMotion {
    LineUp,
    LineDown,
    HalfUp,
    HalfDown,
    Top,
    Bottom,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopyCommand {
    Move(CopyMotion),
    Exit,
    Ignore,
}

/// Viewport position in copy mode plus the half-typed `gg`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CopyState {
    /// Rows above the live screen (0 = bottom).
    pub offset: usize,
    pending_g: bool,
}

impl CopyState {
    /// Interprets a key. A pending `g` is dropped by any key other than `g`,
    /// which is then handled on its own.
    pub fn on_key(&mut self, key: &KeyEvent) -> CopyCommand {
        let was_pending = std::mem::take(&mut self.pending_g);
        let plain = key.modifiers.difference(KeyModifiers::SHIFT).is_empty();
        let ctrl = key.modifiers == KeyModifiers::CONTROL;
        match key.code {
            KeyCode::Char('g') if plain => {
                if was_pending {
                    CopyCommand::Move(CopyMotion::Top)
                } else {
                    self.pending_g = true;
                    CopyCommand::Ignore
                }
            }
            KeyCode::Char('j') if plain => CopyCommand::Move(CopyMotion::LineDown),
            KeyCode::Char('k') if plain => CopyCommand::Move(CopyMotion::LineUp),
            KeyCode::Char('G') if plain => CopyCommand::Move(CopyMotion::Bottom),
            KeyCode::Char('d') if ctrl => CopyCommand::Move(CopyMotion::HalfDown),
            KeyCode::Char('u') if ctrl => CopyCommand::Move(CopyMotion::HalfUp),
            KeyCode::Char('q' | 'i') if plain => self.exit(),
            KeyCode::Esc if plain => self.exit(),
            _ => CopyCommand::Ignore,
        }
    }

    fn exit(&mut self) -> CopyCommand {
        self.offset = 0;
        CopyCommand::Exit
    }

    /// Moves the viewport, clamped to `0..=max` (`max` = rows of history).
    /// `rows` is the pane height; a half page is `rows / 2`, at least 1.
    pub fn apply(&mut self, motion: CopyMotion, max: usize, rows: u16) {
        let half = usize::from(rows / 2).max(1);
        let target = match motion {
            CopyMotion::LineUp => self.offset.saturating_add(1),
            CopyMotion::LineDown => self.offset.saturating_sub(1),
            CopyMotion::HalfUp => self.offset.saturating_add(half),
            CopyMotion::HalfDown => self.offset.saturating_sub(half),
            CopyMotion::Top => max,
            CopyMotion::Bottom => 0,
        };
        self.offset = target.min(max);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    const ROWS: u16 = 24;

    fn press(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn at(offset: usize) -> CopyState {
        CopyState {
            offset,
            ..CopyState::default()
        }
    }

    #[test]
    fn keys_map_to_motions() {
        let mut s = CopyState::default();
        let cases = [
            (press('j'), CopyMotion::LineDown),
            (press('k'), CopyMotion::LineUp),
            (ctrl('d'), CopyMotion::HalfDown),
            (ctrl('u'), CopyMotion::HalfUp),
            (
                KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT),
                CopyMotion::Bottom,
            ),
            (press('G'), CopyMotion::Bottom),
        ];
        for (key, motion) in cases {
            assert_eq!(s.on_key(&key), CopyCommand::Move(motion), "{key:?}");
        }
    }

    #[test]
    fn exit_keys_are_q_esc_and_i() {
        for key in [
            press('q'),
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            press('i'),
        ] {
            let mut s = at(7);
            assert_eq!(s.on_key(&key), CopyCommand::Exit, "{key:?}");
            assert_eq!(s.offset, 0, "exit resets the offset for {key:?}");
        }
    }

    #[test]
    fn unrelated_keys_are_ignored() {
        let mut s = CopyState::default();
        assert_eq!(s.on_key(&press('x')), CopyCommand::Ignore);
        assert_eq!(
            s.on_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            CopyCommand::Ignore
        );
        assert_eq!(s.on_key(&ctrl('x')), CopyCommand::Ignore);
    }

    #[test]
    fn double_g_goes_to_the_top() {
        let mut s = CopyState::default();
        assert_eq!(s.on_key(&press('g')), CopyCommand::Ignore);
        assert_eq!(s.on_key(&press('g')), CopyCommand::Move(CopyMotion::Top));
        // The pending state is consumed: a third g starts over.
        assert_eq!(s.on_key(&press('g')), CopyCommand::Ignore);
    }

    #[test]
    fn lone_g_is_discarded_by_the_next_key() {
        let mut s = CopyState::default();
        s.on_key(&press('g'));
        assert_eq!(
            s.on_key(&press('j')),
            CopyCommand::Move(CopyMotion::LineDown)
        );
        // The g was dropped, so a single g afterwards must not reach the top.
        assert_eq!(s.on_key(&press('g')), CopyCommand::Ignore);
    }

    #[test]
    fn line_motions_move_one_row_and_return() {
        let mut s = at(0);
        s.apply(CopyMotion::LineUp, 100, ROWS);
        assert_eq!(s.offset, 1);
        s.apply(CopyMotion::LineDown, 100, ROWS);
        assert_eq!(s.offset, 0);
    }

    #[test]
    fn half_page_moves_half_the_pane_height() {
        let mut s = at(0);
        s.apply(CopyMotion::HalfUp, 100, ROWS);
        assert_eq!(s.offset, 12);
        s.apply(CopyMotion::HalfDown, 100, ROWS);
        assert_eq!(s.offset, 0);
        s.apply(CopyMotion::HalfUp, 100, 10);
        assert_eq!(s.offset, 5);
    }

    #[test]
    fn half_page_moves_at_least_one_row_on_a_tiny_pane() {
        let mut s = at(0);
        s.apply(CopyMotion::HalfUp, 100, 1);
        assert_eq!(s.offset, 1);
    }

    #[test]
    fn top_and_bottom_jump_to_the_bounds() {
        let mut s = at(40);
        s.apply(CopyMotion::Top, 100, ROWS);
        assert_eq!(s.offset, 100);
        s.apply(CopyMotion::Bottom, 100, ROWS);
        assert_eq!(s.offset, 0);
    }

    #[test]
    fn motions_clamp_at_the_top() {
        for motion in [CopyMotion::LineUp, CopyMotion::HalfUp, CopyMotion::Top] {
            let mut s = at(100);
            s.apply(motion, 100, ROWS);
            assert_eq!(s.offset, 100, "{motion:?}");
        }
        let mut s = at(95);
        s.apply(CopyMotion::HalfUp, 100, ROWS);
        assert_eq!(s.offset, 100);
    }

    #[test]
    fn motions_clamp_at_the_bottom() {
        for motion in [
            CopyMotion::LineDown,
            CopyMotion::HalfDown,
            CopyMotion::Bottom,
        ] {
            let mut s = at(0);
            s.apply(motion, 100, ROWS);
            assert_eq!(s.offset, 0, "{motion:?}");
        }
        let mut s = at(5);
        s.apply(CopyMotion::HalfDown, 100, ROWS);
        assert_eq!(s.offset, 0);
    }

    #[test]
    fn no_history_keeps_every_motion_at_zero() {
        // Alternate screen: scrollback is empty, so max == 0.
        for motion in [
            CopyMotion::LineUp,
            CopyMotion::LineDown,
            CopyMotion::HalfUp,
            CopyMotion::HalfDown,
            CopyMotion::Top,
            CopyMotion::Bottom,
        ] {
            let mut s = at(0);
            s.apply(motion, 0, ROWS);
            assert_eq!(s.offset, 0, "{motion:?}");
        }
    }

    #[test]
    fn stale_offset_beyond_history_is_clamped_on_move() {
        let mut s = at(50);
        s.apply(CopyMotion::LineDown, 10, ROWS);
        assert_eq!(s.offset, 10);
    }
}
