//! The dashboard drawn offscreen: a buffer as tall as the dashboard, so the
//! view scrolls over it and the pane's height never clips a card.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};

use super::{cards, daily, heat, tables};
use crate::core::ai::chat::dashboard::{CARD_ROWS, CHART_ROWS, Dashboard, HEAT_ROWS, Range, Tier};
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

/// The dashboard at `width` columns.
pub(super) fn draw(model: &Dashboard, width: u16, theme: &LazaroboxTheme) -> Buffer {
    let cards = model.height(width);
    let area = Rect::new(0, 0, width, cards);
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
    let card_w = (width.saturating_add(1) / per_row).saturating_sub(1);
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
    // The chart, the heatmap, the models and the agents are the last blocks: each a gap,
    // then its legend or title.
    let (models, agents) = (model.tables(width), model.agents_rows());
    let heat_gap = cards.saturating_sub(agents + models + HEAT_ROWS);
    let top = heat_gap.saturating_sub(CHART_ROWS).saturating_add(1);
    daily::render(&model.daily, model.last_days, top, width, &mut buf, theme);
    heat::render(&model.heat, heat_gap + 1, width, &mut buf, theme);
    tables::render(model, heat_gap + HEAT_ROWS, width, &mut buf, theme);
    let at = heat_gap + HEAT_ROWS + models;
    tables::render_agents(model, at, width, &mut buf, theme);
    buf
}
