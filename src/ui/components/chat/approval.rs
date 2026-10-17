//! The run-in-pane confirm drawn over the composer of a chat pane: where the
//! command goes, the exact command, and the keys. Everything is already
//! sanitized text; a tab is drawn as an arrow so nothing is drawn raw.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};

use super::{ChatView, fill};
use crate::core::ai::chat::{approval::ApprovalPrompt, wrap::wrap_ranges};

/// The most rows of the command drawn, the clip marker included.
const MAX_COMMAND_ROWS: usize = 6;
/// How a tab is drawn.
const TAB: char = '→';

/// What a row of the prompt holds.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) enum Part {
    Head(String),
    Command(String),
    Marker(String),
    Keys(String),
}

/// The command as rows `width` columns wide, one or more per logical line.
fn command_rows(command: &str, width: usize) -> Vec<String> {
    command
        .split('\n')
        .flat_map(|line| {
            let chars: Vec<char> = line
                .chars()
                .filter(|c| *c != '\r')
                .map(|c| if c == '\t' { TAB } else { c })
                .collect();
            wrap_ranges(&chars, width)
                .into_iter()
                .map(|range| chars[range].iter().collect::<String>())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The rows of the prompt for `width` x `height`, at most `height` of them.
/// What does not fit is never dropped silently: a clipped command says how
/// many lines it has.
pub(super) fn parts(prompt: &ApprovalPrompt, width: usize, height: usize) -> Vec<Part> {
    let rows = command_rows(&prompt.command, width.max(1));
    let lines = prompt.command.split('\n').count();
    let head = Part::Head(prompt.target.clone());
    let marker = |lines: usize| Part::Marker(format!("… clipped: {lines} lines in all"));
    match height {
        0 => Vec::new(),
        1 => vec![head],
        2 if rows.len() == 1 => vec![head, Part::Command(rows[0].clone())],
        2 => vec![head, marker(lines)],
        _ => {
            let room = (height - 2).min(MAX_COMMAND_ROWS);
            let mut out = vec![head];
            if rows.len() <= room {
                out.extend(rows.into_iter().map(Part::Command));
            } else {
                out.extend(rows.into_iter().take(room - 1).map(Part::Command));
                out.push(marker(lines));
            }
            out.push(Part::Keys(prompt.keys.to_owned()));
            out
        }
    }
}

/// The rows the whole prompt wants at `width`.
pub(super) fn wanted(prompt: &ApprovalPrompt, width: usize) -> usize {
    parts(prompt, width, MAX_COMMAND_ROWS + 2).len()
}

impl ChatView<'_> {
    /// Draws the prompt in `area`, over the composer.
    pub(super) fn render_approval(&self, prompt: &ApprovalPrompt, area: Rect, buf: &mut Buffer) {
        fill(buf, area, Style::default().bg(self.theme.bg_panel));
        let base = Style::default().bg(self.theme.bg_panel);
        for (row, part) in parts(prompt, usize::from(area.width), usize::from(area.height))
            .iter()
            .enumerate()
        {
            let y = area.y + u16::try_from(row).unwrap_or(0);
            let (text, style) = match part {
                Part::Head(t) => (
                    t,
                    base.fg(self.theme.warning_orange)
                        .add_modifier(Modifier::BOLD),
                ),
                Part::Command(t) => (t, base.fg(self.theme.success_green)),
                Part::Marker(t) | Part::Keys(t) => (t, base.fg(self.theme.text_muted)),
            };
            buf.set_stringn(area.x, y, text, usize::from(area.width), style);
        }
    }
}
