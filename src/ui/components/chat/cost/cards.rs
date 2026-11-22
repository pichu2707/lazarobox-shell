//! One KPI card: label and delta, value, sub-line and a sparkline of the
//! metric by day, on the panel background. The delta is an arrow plus text
//! and the sparkline is block glyphs, so the card reads with no colour.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Sparkline, Widget},
};

use super::super::fill;
use crate::core::ai::chat::dashboard::{CARD_ROWS, Kpi, bucket};
use crate::ui::theme::LazaroboxTheme;

pub(super) fn render(
    kpi: &Kpi,
    x: u16,
    y: u16,
    width: u16,
    buf: &mut Buffer,
    theme: &LazaroboxTheme,
) {
    let area = Rect::new(x, y, width, CARD_ROWS).intersection(buf.area);
    if area.width != width || area.height != CARD_ROWS {
        return;
    }
    let panel = Style::default().bg(theme.bg_panel);
    let muted = panel.fg(theme.text_muted);
    fill(buf, area, panel);
    let w = usize::from(width);
    // One column of padding on each side of the text.
    let inner = w.saturating_sub(2);
    buf.set_stringn(x + 1, y, kpi.label, inner, muted);
    let delta = kpi.delta.text();
    let at = w
        .saturating_sub(delta.chars().count() + 1)
        .max(kpi.label.len() + 2);
    buf.set_stringn(
        x + u16::try_from(at).unwrap_or(0),
        y,
        &delta,
        w.saturating_sub(at + 1),
        muted,
    );
    buf.set_stringn(
        x + 1,
        y + 1,
        &kpi.value,
        inner,
        panel.add_modifier(Modifier::BOLD),
    );
    buf.set_stringn(x + 1, y + 2, &kpi.sub, inner, muted);
    let line = Rect::new(x + 1, y + 3, width.saturating_sub(2), 1);
    Sparkline::default()
        .data(bucket(&kpi.spark, usize::from(line.width)))
        .style(panel.fg(theme.primary_cyan))
        .render(line, buf);
}
