//! The review view drawn over the whole chat pane: a counter on top, the rows
//! of the batch below it and the keys on the last row.
//!
//! The rows are the only place the text of a diff is drawn. They are taken
//! from `ReviewView::visible`, the rows the coverage counts, and each one
//! comes from `text_for_review_renderer_only`: already escaped, so no control,
//! bidi or invisible character reaches the terminal.

use std::time::{Duration, Instant};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};

use super::chat::fill;
use crate::{
    core::ai::chat::{
        review::{LineKind, WARN_TRUNCATED},
        review_view::ReviewView,
    },
    ui::theme::LazaroboxTheme,
};

const KEYS_READY: &str = "y approve · n reject · ↑↓ PgUp PgDn scroll";
const KEYS_LOCKED: &str = "y locked · n reject · ↑↓ PgUp PgDn scroll";
const KEYS_DISCARD: &str = "n discard · ↑↓ PgUp PgDn scroll";

/// `0.4s`, rounded up to a tenth.
fn seconds(left: Duration) -> String {
    let tenths = left.as_millis().div_ceil(100);
    format!("{}.{}s", tenths / 10, tenths % 10)
}

/// The counter row, and whether it is a warning.
fn header(view: &ReviewView, now: Instant) -> (String, bool) {
    if !view.reviewable() {
        return (WARN_TRUNCATED.to_owned(), true);
    }
    let (total, remaining) = (view.total(), view.remaining());
    let left = view.arm_left(now);
    let gate = if remaining > 0 {
        format!("{remaining} remaining · y locked")
    } else if left > Duration::ZERO {
        format!("y in {}", seconds(left))
    } else {
        "y ready".to_owned()
    };
    let counts = view.review().counts();
    (
        format!("{counts} · seen {}/{total} · {gate}", total - remaining),
        false,
    )
}

fn style_of(kind: LineKind, theme: &LazaroboxTheme) -> Style {
    let base = Style::default().bg(theme.bg_base);
    match kind {
        LineKind::Summary => base.fg(theme.primary_cyan).add_modifier(Modifier::BOLD),
        LineKind::File => base.add_modifier(Modifier::BOLD),
        LineKind::Warning => base.fg(theme.warning_orange).add_modifier(Modifier::BOLD),
        LineKind::Hunk => base.fg(theme.info_blue),
        LineKind::Added => base.fg(theme.success_green),
        LineKind::Removed => base.fg(theme.error_red),
        LineKind::Context => base,
        LineKind::Note => base.fg(theme.text_muted),
    }
}

/// Draws the view in `area`. A pane of one row shows the keys only, of two
/// rows the counter and the keys: the way out is the last to go.
pub fn render(
    view: &ReviewView,
    now: Instant,
    theme: &LazaroboxTheme,
    area: Rect,
    buf: &mut Buffer,
) {
    let base = Style::default().bg(theme.bg_base);
    fill(buf, area, base);
    let width = usize::from(area.width);
    let put = |buf: &mut Buffer, row: u16, text: &str, style: Style| {
        buf.set_stringn(area.x, area.y + row, text, width, style);
    };
    if area.height >= 2 {
        let (text, warn) = header(view, now);
        let colour = if warn {
            theme.warning_orange
        } else {
            theme.primary_cyan
        };
        put(buf, 0, &text, base.fg(colour).add_modifier(Modifier::BOLD));
    }
    for (n, row) in view.visible(usize::from(area.height)).iter().enumerate() {
        if row.fits(width) {
            let y = u16::try_from(n + 1).unwrap_or(u16::MAX);
            put(
                buf,
                y,
                row.text_for_review_renderer_only(),
                style_of(row.kind, theme),
            );
        }
    }
    let keys = if !view.reviewable() {
        KEYS_DISCARD
    } else if view.can_approve(now) {
        KEYS_READY
    } else {
        KEYS_LOCKED
    };
    put(buf, area.height - 1, keys, base.fg(theme.text_muted));
}
