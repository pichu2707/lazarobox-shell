//! The dashboard drawn offscreen: a buffer as tall as the dashboard, so the
//! view scrolls over it and the pane's height never clips a card.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};

use super::cards;
use crate::core::ai::chat::dashboard::{CARD_ROWS, Dashboard, Range, Tier};
use crate::ui::theme::LazaroboxTheme;

/// Shown instead of the cards when the range has no usage.
const EMPTY: &str = "no usage recorded yet";

/// `[7d] 30d 90d All`: the active range in brackets, so no colour is needed.
fn selector(active: Range) -> String {
    let items = Range::ALL.map(|r| match r == active {
        true => format!("[{}]", r.label()),
        false => r.label().to_owned(),
    });
    items.join(" ")
}

/// The dashboard at `width` columns, `Dashboard::height` rows tall.
pub(super) fn draw(model: &Dashboard, width: u16, theme: &LazaroboxTheme) -> Buffer {
    let area = Rect::new(0, 0, width, model.height(width));
    let mut buf = Buffer::empty(area);
    let base = Style::default().bg(theme.bg_base);
    super::super::fill(&mut buf, area, base);
    if area.height == 0 {
        return buf;
    }
    let w = usize::from(width);
    buf.set_stringn(
        0,
        0,
        selector(model.range),
        w,
        base.add_modifier(Modifier::BOLD),
    );
    if model.empty {
        buf.set_stringn(0, 2, EMPTY, w, base.fg(theme.text_muted));
        return buf;
    }
    // Cards per row, and the width each takes with one column between them.
    let per_row = match Tier::of(width) {
        Tier::Row => 4,
        Tier::Grid => 2,
        _ => 1,
    };
    let card_w = (width + 1) / per_row - 1;
    for (i, kpi) in (0u16..).zip(&model.kpis) {
        let (col, row) = (i % per_row, i / per_row);
        cards::render(
            kpi,
            col * (card_w + 1),
            2 + row * (CARD_ROWS + 1),
            card_w,
            &mut buf,
            theme,
        );
    }
    buf
}
