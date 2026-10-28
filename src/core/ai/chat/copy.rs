//! COPY on a chat pane: the pure key table and position.
//!
//! The transcript rows are the lines `0..total`, `rows` of them on screen
//! (see `geometry`). The shell's `CopyState` moves a cursor and a selection
//! over them; its offset (lines below the view) maps onto the chat's
//! `transcript::Scroll`, which the app keeps as the source of truth.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::transcript::Scroll;
use crate::core::{
    copy::{CopyCommand, CopyMotion, CopyState, Geometry, Point},
    prefix::PREFIX_KEY,
};

/// What a key does in chat COPY.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChatCopyCommand {
    /// A cursor motion, applied with `CopyState::apply_motion`.
    Move(CopyMotion),
    /// `y` with a selection: copy it.
    Yank,
    /// Ctrl+Y: copy the last answer.
    Answer,
    /// Ctrl+B: copy the last code block of the last answer.
    CodeBlock,
    /// `q`, or Esc with no selection: back to TERMINAL, following.
    Exit,
    /// Ctrl+Space: the prefix, as in any pane.
    Prefix,
    /// Swallowed.
    Ignore,
}

/// The transcript as COPY sees it: `total` lines, the last `rows` of them
/// (or all, when they fit) below the history.
pub fn geometry(total: usize, rows: usize, cols: u16) -> Geometry {
    let shown = rows.min(total);
    Geometry {
        first_line: 0,
        history: total - shown,
        rows: u16::try_from(shown).unwrap_or(u16::MAX),
        cols,
    }
}

/// Lines below the view of a transcript of `geo` shown at `scroll`.
fn offset(geo: Geometry, scroll: Scroll) -> usize {
    match scroll {
        Scroll::Follow => 0,
        Scroll::Pinned(top) => geo.history - top.min(geo.history),
    }
}

/// Cursor, selection and view of chat COPY, plus the width the rows were
/// wrapped at.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ChatCopy {
    /// Cursor, selection and offset (transcript lines below the view).
    pub state: CopyState,
    cols: u16,
}

impl ChatCopy {
    /// COPY entered on a transcript of `geo` shown at `scroll`: the view does
    /// not move and the cursor is on the last visible row.
    pub fn enter(geo: Geometry, scroll: Scroll) -> Self {
        Self {
            state: CopyState::enter(geo, offset(geo, scroll), None, 0),
            cols: geo.cols,
        }
    }

    /// Brings the state in line with the transcript before a key or a draw.
    /// The chat's `scroll` is the source of truth for the view. Another width
    /// rewraps every row, so COPY starts over as when entered; fewer rows (a
    /// reply rewrapped as it streams) clamp the cursor and the anchor.
    pub fn sync(&mut self, geo: Geometry, scroll: Scroll) {
        if geo.cols != self.cols {
            *self = Self::enter(geo, scroll);
            return;
        }
        self.state.offset = offset(geo, scroll);
        let last_line = (geo.history + usize::from(geo.rows)).saturating_sub(1);
        let last_col = geo.cols.saturating_sub(1);
        let clamp = |p: &mut Point| {
            p.line = p.line.min(last_line);
            p.col = p.col.min(last_col);
        };
        clamp(&mut self.state.cursor);
        if let Some(selection) = &mut self.state.selection {
            clamp(&mut selection.anchor);
        }
    }

    /// Where the view goes for the current offset: following at 0, pinned
    /// to the top line otherwise.
    pub fn scroll(&self, geo: Geometry) -> Scroll {
        match self.state.offset.min(geo.history) {
            0 => Scroll::Follow,
            offset => Scroll::Pinned(geo.history - offset),
        }
    }

    /// Resolves one key. The prefix, a held `g`/`G`, `i` and the chat's own
    /// copy keys (Ctrl+Y, Ctrl+B) are handled here; every other key is the
    /// shell's COPY key table.
    pub fn on_key(&mut self, key: &KeyEvent, repeat: bool) -> ChatCopyCommand {
        let plain = key.modifiers.difference(KeyModifiers::SHIFT).is_empty();
        let ctrl = key.modifiers == KeyModifiers::CONTROL;
        let own = |command| {
            if repeat {
                ChatCopyCommand::Ignore
            } else {
                command
            }
        };
        let command = match key.code {
            _ if PREFIX_KEY.matches(key) => ChatCopyCommand::Prefix,
            // A held g or G must not jump: only a fresh press moves to an end.
            KeyCode::Char('g' | 'G') if repeat => ChatCopyCommand::Ignore,
            KeyCode::Char('y') if ctrl => own(ChatCopyCommand::Answer),
            KeyCode::Char('b') if ctrl => own(ChatCopyCommand::CodeBlock),
            // A shell's `i` leaves COPY to type; here it would only drop the
            // view, so it does nothing.
            KeyCode::Char('i') if plain => ChatCopyCommand::Ignore,
            _ => {
                return match self.state.on_key(key, repeat) {
                    CopyCommand::Move(motion) => ChatCopyCommand::Move(motion),
                    CopyCommand::Yank => ChatCopyCommand::Yank,
                    CopyCommand::Exit => ChatCopyCommand::Exit,
                    CopyCommand::Ignore => ChatCopyCommand::Ignore,
                };
            }
        };
        // A key handled here discards a pending `g`, like any other key.
        self.state.cancel_g();
        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::copy::SelectionKind;

    fn press(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    fn ch(c: char) -> KeyEvent {
        let mods = if c.is_ascii_uppercase() {
            KeyModifiers::SHIFT
        } else {
            KeyModifiers::NONE
        };
        press(KeyCode::Char(c), mods)
    }

    fn ctrl(c: char) -> KeyEvent {
        press(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn esc() -> KeyEvent {
        press(KeyCode::Esc, KeyModifiers::NONE)
    }

    fn run(keys: &[KeyEvent]) -> Vec<ChatCopyCommand> {
        let mut copy = ChatCopy::default();
        keys.iter().map(|k| copy.on_key(k, false)).collect()
    }

    /// 100 lines, 20 on screen, 40 columns.
    fn geo() -> Geometry {
        geometry(100, 20, 40)
    }

    #[test]
    fn the_transcript_is_lines_zero_to_total_with_the_view_at_the_bottom() {
        assert_eq!(
            geo(),
            Geometry {
                first_line: 0,
                history: 80,
                rows: 20,
                cols: 40
            }
        );
        // A transcript that fits has no history and only its own rows.
        assert_eq!(
            geometry(5, 20, 40),
            Geometry {
                first_line: 0,
                history: 0,
                rows: 5,
                cols: 40
            }
        );
    }

    #[test]
    fn the_scroll_and_motion_keys_map_to_the_shell_motions() {
        use ChatCopyCommand::*;
        use CopyMotion::*;
        let keys = [
            ch('j'),
            ch('k'),
            ch('h'),
            ch('l'),
            ch('0'),
            ch('$'),
            ctrl('d'),
            ctrl('u'),
            ch('G'),
            press(KeyCode::Char(' '), KeyModifiers::CONTROL),
        ];
        assert_eq!(
            run(&keys),
            [
                Move(LineDown),
                Move(LineUp),
                Move(Left),
                Move(Right),
                Move(LineStart),
                Move(LineEnd),
                Move(HalfDown),
                Move(HalfUp),
                Move(Bottom),
                Prefix
            ]
        );
    }

    #[test]
    fn v_and_capital_v_select_and_y_copies_only_a_selection() {
        let mut copy = ChatCopy::default();
        assert_eq!(copy.on_key(&ch('y'), false), ChatCopyCommand::Ignore);
        assert_eq!(copy.on_key(&ch('v'), false), ChatCopyCommand::Ignore);
        let kind = |c: &ChatCopy| c.state.selection.map(|s| s.kind);
        assert_eq!(kind(&copy), Some(SelectionKind::Char));
        copy.on_key(&ch('V'), false);
        assert_eq!(kind(&copy), Some(SelectionKind::Line));
        assert_eq!(copy.on_key(&ch('y'), false), ChatCopyCommand::Yank);
    }

    #[test]
    fn esc_clears_the_selection_first_and_a_second_esc_leaves() {
        use ChatCopyCommand::*;
        let mut copy = ChatCopy::default();
        copy.on_key(&ch('v'), false);
        assert_eq!(copy.on_key(&esc(), false), Ignore);
        assert_eq!(copy.state.selection, None);
        assert_eq!(copy.on_key(&esc(), false), Exit);
        assert_eq!(run(&[ch('v'), ch('q')]), [Ignore, Exit], "q leaves at once");
    }

    #[test]
    fn ctrl_y_and_ctrl_b_copy_the_answer_and_its_code_block_once_per_press() {
        use ChatCopyCommand::*;
        assert_eq!(run(&[ctrl('y'), ctrl('b')]), [Answer, CodeBlock]);
        let mut copy = ChatCopy::default();
        assert_eq!(copy.on_key(&ctrl('y'), true), Ignore);
        assert_eq!(copy.on_key(&ctrl('b'), true), Ignore);
    }

    #[test]
    fn gg_goes_to_the_top() {
        use ChatCopyCommand::*;
        assert_eq!(run(&[ch('g'), ch('g')]), [Ignore, Move(CopyMotion::Top)]);
        // The pair is consumed: a third `g` starts a new one.
        assert_eq!(
            run(&[ch('g'), ch('g'), ch('g')]),
            [Ignore, Move(CopyMotion::Top), Ignore]
        );
    }

    #[test]
    fn a_lone_g_is_discarded_by_any_other_key() {
        use ChatCopyCommand::*;
        assert_eq!(
            run(&[ch('g'), ch('j'), ch('g')]),
            [Ignore, Move(CopyMotion::LineDown), Ignore]
        );
        assert_eq!(run(&[ch('g'), ch('x'), ch('g')]), [Ignore, Ignore, Ignore]);
        assert_eq!(run(&[ch('g'), ch('i'), ch('g')]), [Ignore, Ignore, Ignore]);
        assert_eq!(
            run(&[ch('g'), ctrl('y'), ch('g')]),
            [Ignore, Answer, Ignore]
        );
    }

    #[test]
    fn editing_keys_are_swallowed_and_i_does_not_leave() {
        let swallowed = [
            ch('i'),
            ch('a'),
            ch('x'),
            press(KeyCode::Enter, KeyModifiers::NONE),
            press(KeyCode::Backspace, KeyModifiers::NONE),
            press(KeyCode::Tab, KeyModifiers::NONE),
            ctrl('c'),
            press(KeyCode::Char('j'), KeyModifiers::ALT),
            press(KeyCode::Char('q'), KeyModifiers::CONTROL),
        ];
        for key in swallowed {
            let mut copy = ChatCopy::default();
            assert_eq!(copy.on_key(&key, false), ChatCopyCommand::Ignore, "{key:?}");
        }
    }

    #[test]
    fn a_held_g_and_capital_g_do_nothing() {
        use ChatCopyCommand::*;
        let mut copy = ChatCopy::default();
        assert_eq!(copy.on_key(&ch('g'), false), Ignore);
        assert_eq!(
            copy.on_key(&ch('g'), true),
            Ignore,
            "a held g never ends the pair"
        );
        assert_eq!(copy.on_key(&ch('g'), true), Ignore);
        assert_eq!(copy.on_key(&ch('G'), true), Ignore);
        // A pressed g after a repeated one still starts a fresh pair.
        assert_eq!(copy.on_key(&ch('g'), false), Ignore);
        assert_eq!(copy.on_key(&ch('g'), false), Move(CopyMotion::Top));
    }

    #[test]
    fn a_held_scroll_key_acts_and_a_held_exit_or_selection_key_does_not() {
        use ChatCopyCommand::*;
        let mut copy = ChatCopy::default();
        assert_eq!(copy.on_key(&ch('j'), true), Move(CopyMotion::LineDown));
        assert_eq!(copy.on_key(&ch('k'), true), Move(CopyMotion::LineUp));
        assert_eq!(copy.on_key(&ctrl('d'), true), Move(CopyMotion::HalfDown));
        assert_eq!(copy.on_key(&ctrl('u'), true), Move(CopyMotion::HalfUp));
        assert_eq!(copy.on_key(&esc(), true), Ignore);
        assert_eq!(copy.on_key(&ch('q'), true), Ignore);
        assert_eq!(copy.on_key(&ch('v'), true), Ignore);
        assert_eq!(copy.state.selection, None);
    }

    #[test]
    fn entering_keeps_the_view_and_puts_the_cursor_on_the_last_visible_row() {
        let copy = ChatCopy::enter(geo(), Scroll::Follow);
        assert_eq!(copy.state.offset, 0);
        assert_eq!(copy.state.cursor, Point { line: 99, col: 0 });
        let copy = ChatCopy::enter(geo(), Scroll::Pinned(30));
        assert_eq!(copy.state.offset, 50);
        assert_eq!(copy.state.cursor, Point { line: 49, col: 0 });
        assert_eq!(copy.scroll(geo()), Scroll::Pinned(30));
        // A short transcript: the cursor is on its last row, not below it.
        let copy = ChatCopy::enter(geometry(3, 20, 40), Scroll::Follow);
        assert_eq!(copy.state.cursor.line, 2);
    }

    #[test]
    fn the_offset_maps_onto_the_scroll_and_zero_follows() {
        let mut copy = ChatCopy::enter(geo(), Scroll::Follow);
        assert_eq!(copy.scroll(geo()), Scroll::Follow);
        copy.state.apply_motion(CopyMotion::HalfUp, geo(), 0);
        assert_eq!(copy.state.offset, 10);
        assert_eq!(copy.scroll(geo()), Scroll::Pinned(70));
        copy.state.apply_motion(CopyMotion::Top, geo(), 0);
        assert_eq!(copy.scroll(geo()), Scroll::Pinned(0));
        copy.state.apply_motion(CopyMotion::Bottom, geo(), 0);
        assert_eq!(copy.scroll(geo()), Scroll::Follow);
    }

    #[test]
    fn a_sync_takes_the_offset_from_the_scroll() {
        let mut copy = ChatCopy::enter(geo(), Scroll::Pinned(30));
        // Five more lines came in under a pinned view.
        let grown = geometry(105, 20, 40);
        copy.sync(grown, Scroll::Pinned(30));
        assert_eq!(copy.state.offset, 55);
        assert_eq!(copy.state.cursor.line, 49, "the cursor stays on its line");
    }

    #[test]
    fn a_sync_with_fewer_rows_clamps_the_cursor_and_the_anchor() {
        let mut copy = ChatCopy::enter(geo(), Scroll::Follow);
        copy.on_key(&ch('v'), false);
        copy.state.cursor.col = 30;
        copy.sync(geometry(60, 20, 40), Scroll::Follow);
        assert_eq!(copy.state.cursor, Point { line: 59, col: 30 });
        let anchor = copy.state.selection.expect("kept").anchor;
        assert_eq!(anchor.line, 59);
    }

    #[test]
    fn a_sync_at_another_width_drops_the_selection_and_starts_over() {
        let mut copy = ChatCopy::enter(geo(), Scroll::Follow);
        copy.on_key(&ch('v'), false);
        copy.state.apply_motion(CopyMotion::HalfUp, geo(), 0);
        let narrow = geometry(130, 20, 30);
        copy.sync(narrow, Scroll::Follow);
        assert_eq!(copy.state.selection, None);
        assert_eq!(copy.state.offset, 0);
        assert_eq!(copy.state.cursor, Point { line: 129, col: 0 });
    }
}
