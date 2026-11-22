//! The Models table and the provider bar. Every row starts with its provider
//! glyph and every bar segment is that glyph repeated, so both read with no
//! colour; the legend names each provider with its share.

use ratatui::{buffer::Buffer, style::Style};

use crate::core::ai::chat::dashboard::{Dashboard, NO_TOKENS, segments, share_legend, table};
use crate::ui::theme::LazaroboxTheme;

/// Draws the blocks from `y`: a gap, the Models title, the table and its
/// notes, a gap, the Providers title, the bar and its legend.
pub(super) fn render(
    model: &Dashboard,
    y: u16,
    width: u16,
    buf: &mut Buffer,
    theme: &LazaroboxTheme,
) {
    if model.models.is_empty() {
        return;
    }
    let base = Style::default().bg(theme.bg_base);
    let (muted, w) = (base.fg(theme.text_muted), usize::from(width));
    let t = table(&model.models, w);
    buf.set_stringn(0, y + 1, "Models · by total tokens", w, muted);
    buf.set_stringn(0, y + 2, &t.head, w, muted);
    let mut at = y + 3;
    for (provider, line) in &t.rows {
        buf.set_stringn(0, at, line, w, base);
        // The glyph is the first cell; its colour only helps.
        buf.set_stringn(
            0,
            at,
            provider.glyph().to_string(),
            1,
            base.fg(theme.provider_color(*provider)),
        );
        at += 1;
    }
    for note in &t.notes {
        buf.set_stringn(0, at, note, w, muted);
        at += 1;
    }
    buf.set_stringn(0, at + 1, "Providers · share of tokens", w, muted);
    at += 2;
    if model.providers.is_empty() {
        buf.set_stringn(0, at, NO_TOKENS, w, muted);
        return;
    }
    let mut x = 0;
    for (provider, cells) in segments(&model.providers, w) {
        let glyph = provider.glyph().to_string();
        let style = base.fg(theme.provider_color(provider));
        for _ in 0..cells {
            buf.set_stringn(x, at, &glyph, 1, style);
            x += 1;
        }
    }
    for (n, line) in (1u16..).zip(share_legend(&model.providers, w)) {
        buf.set_stringn(0, at + n, line, w, base);
    }
}
