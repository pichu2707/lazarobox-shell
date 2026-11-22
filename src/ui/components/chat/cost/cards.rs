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
    // The delta sits at the right edge. When it does not fit beside the label
    // the right padding goes first, then the number is capped; it is never cut.
    let room = |pad: usize| w.saturating_sub(kpi.label.len() + 2 + pad);
    let fits = [
        (kpi.delta.text(), 1),
        (kpi.delta.text(), 0),
        (kpi.delta.capped(), 0),
    ]
    .into_iter()
    .find(|(text, pad)| text.chars().count() <= room(*pad));
    if let Some((text, pad)) = fits {
        let at = w - pad - text.chars().count();
        buf.set_stringn(x + u16::try_from(at).unwrap_or(0), y, &text, w - at, muted);
    }
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
