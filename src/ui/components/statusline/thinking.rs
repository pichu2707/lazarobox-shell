//! The thinking indicator of the statusline: a scanner bar, a braille
//! spinner, the stream phase, the model, the elapsed time and the count of
//! other streams. Pure state in, styled spans out; nothing is written to the
//! terminal outside ratatui.

use std::time::{Duration, Instant};

use ratatui::{style::Style, text::Span};

use crate::{
    core::ai::chat::{StreamPhase, streams::StreamInfo, wrap::text_width},
    ui::theme::LazaroboxTheme,
};

/// The spinner frames, in order.
pub const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
/// Cells of the scanner track.
pub const TRACK: usize = 8;
/// Cells of the lit segment.
pub const LIT: usize = 3;

/// What the indicator shows for one stream at one animation frame.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ThinkingView {
    pub frame: usize,
    pub phase: StreamPhase,
    pub model: String,
    pub elapsed: Duration,
    /// How many other streams run.
    pub others: usize,
}

/// The spinner glyph of `frame`.
pub fn spinner(frame: usize) -> char {
    SPINNER[frame % SPINNER.len()]
}

/// First lit cell of the scanner at `frame`. The segment bounces and always
/// stays inside the track.
pub fn scanner_start(frame: usize) -> usize {
    let reach = TRACK - LIT;
    let step = frame % (2 * reach);
    if step <= reach {
        step
    } else {
        2 * reach - step
    }
}

/// `Ns` below a minute, `MmSSs` from a minute.
pub fn elapsed_text(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m{:02}s", secs / 60, secs % 60)
    }
}

impl ThinkingView {
    /// The view of `stream` at `frame`, `now` being the injected clock.
    pub fn new(stream: &StreamInfo, others: usize, frame: usize, now: Instant) -> Self {
        Self {
            frame,
            phase: stream.phase,
            model: stream.model.clone(),
            elapsed: now.saturating_duration_since(stream.started),
            others,
        }
    }

    /// The indicator as spans (padding included) that fit `max` columns, or
    /// nothing when not even the spinner fits. Whole parts are dropped in the
    /// order bar, model, elapsed, count, phase, spinner.
    pub fn spans(&self, theme: &LazaroboxTheme, max: usize) -> Vec<Span<'static>> {
        let muted = Style::new().fg(theme.text_muted).bg(theme.bg_base);
        let purple = Style::new().fg(theme.ai_purple).bg(theme.bg_base);
        let phase = match self.phase {
            StreamPhase::Thinking => "thinking",
            StreamPhase::Answering => "answering",
        };
        let start = scanner_start(self.frame);
        let bar = vec![
            Span::styled("━".repeat(start), muted),
            Span::styled("▰".repeat(LIT), purple),
            Span::styled("━".repeat(TRACK - LIT - start), muted),
        ];
        // In display order, each with its position in the dropping order.
        let mut parts: Vec<(usize, Vec<Span<'static>>)> = vec![
            (0, bar),
            (
                5,
                vec![Span::styled(spinner(self.frame).to_string(), purple)],
            ),
            (4, vec![Span::styled(phase, muted)]),
        ];
        if !self.model.is_empty() {
            parts.push((1, vec![Span::styled(self.model.clone(), muted)]));
        }
        parts.push((2, vec![Span::styled(elapsed_text(self.elapsed), muted)]));
        if self.others > 0 {
            parts.push((3, vec![Span::styled(format!("+{}", self.others), muted)]));
        }
        let width = |parts: &[(usize, Vec<Span<'static>>)]| -> usize {
            let cells: usize = parts
                .iter()
                .flat_map(|(_, spans)| spans)
                .map(|s| text_width(&s.content))
                .sum();
            // A space around the whole and between parts.
            cells + parts.len() + 1
        };
        // Drop the part that goes first until the rest fits.
        while !parts.is_empty() && width(&parts) > max {
            let first = parts
                .iter()
                .enumerate()
                .min_by_key(|(_, (drop, _))| *drop)
                .map(|(i, _)| i);
            if let Some(i) = first {
                parts.remove(i);
            }
        }
        if parts.is_empty() {
            return Vec::new();
        }
        let mut out = vec![Span::styled(" ", muted)];
        for (n, (_, spans)) in parts.into_iter().enumerate() {
            if n > 0 {
                out.push(Span::styled(" ", muted));
            }
            out.extend(spans);
        }
        out.push(Span::styled(" ", muted));
        out
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{buffer::Buffer, layout::Rect, text::Line, widgets::Widget};

    use super::*;
    use crate::core::layout::PaneId;

    fn theme() -> LazaroboxTheme {
        LazaroboxTheme::default()
    }

    fn view(
        frame: usize,
        phase: StreamPhase,
        model: &str,
        secs: u64,
        others: usize,
    ) -> ThinkingView {
        ThinkingView {
            frame,
            phase,
            model: model.to_owned(),
            elapsed: Duration::from_secs(secs),
            others,
        }
    }

    fn text(spans: &[Span<'_>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn full(v: &ThinkingView) -> Vec<Span<'static>> {
        v.spans(&theme(), 200)
    }

    #[test]
    fn a_wide_model_name_is_measured_in_display_columns() {
        use crate::core::ai::chat::wrap::text_width;
        let v = view(0, StreamPhase::Thinking, "日本語モデル", 4, 0);
        for max in 0..60 {
            let shown = text(&v.spans(&theme(), max));
            assert!(text_width(&shown) <= max, "{shown:?} in {max}");
        }
    }

    #[test]
    fn the_spinner_walks_its_frames_in_order_and_wraps() {
        let glyphs: String = (0..=10).map(spinner).collect();
        assert_eq!(glyphs, "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏⠋");
    }

    #[test]
    fn the_lit_segment_never_leaves_the_track_and_moves() {
        for frame in 0..=1000 {
            assert!(scanner_start(frame) + LIT <= TRACK, "frame {frame}");
        }
        let positions: std::collections::BTreeSet<usize> = (0..10).map(scanner_start).collect();
        assert!(positions.len() >= 2, "{positions:?}");
        assert_eq!(scanner_start(0), scanner_start(10), "it loops");
    }

    #[test]
    fn elapsed_time_formats() {
        let text = |s| elapsed_text(Duration::from_secs(s));
        assert_eq!(
            [text(0), text(59), text(60), text(65), text(3600)],
            ["0s", "59s", "1m00s", "1m05s", "60m00s"]
        );
    }

    #[test]
    fn elapsed_comes_from_the_start_instant_not_from_the_frames() {
        let started = Instant::now();
        let stream = StreamInfo {
            pane: PaneId::for_test(2),
            started,
            phase: StreamPhase::Thinking,
            model: "m".into(),
        };
        let v = ThinkingView::new(&stream, 0, 2, started + Duration::from_secs(7));
        assert!(text(&full(&v)).contains(" 7s"), "{}", text(&full(&v)));
    }

    #[test]
    fn a_thinking_stream_shows_every_part_in_order() {
        let v = view(0, StreamPhase::Thinking, "llama3:8b", 4, 0);
        assert_eq!(text(&full(&v)), " ▰▰▰━━━━━ ⠋ thinking llama3:8b 4s ");
    }

    #[test]
    fn the_phase_flips_to_answering_and_the_rest_stays() {
        let v = view(0, StreamPhase::Answering, "llama3:8b", 4, 0);
        assert_eq!(text(&full(&v)), " ▰▰▰━━━━━ ⠋ answering llama3:8b 4s ");
    }

    #[test]
    fn the_count_of_other_streams_comes_last_and_only_when_there_are_some() {
        let v = view(3, StreamPhase::Thinking, "m", 61, 2);
        assert_eq!(text(&full(&v)), " ━━━▰▰▰━━ ⠸ thinking m 1m01s +2 ");
        assert!(!text(&full(&view(0, StreamPhase::Thinking, "m", 1, 0))).contains('+'));
    }

    #[test]
    fn an_empty_model_is_left_out() {
        let v = view(0, StreamPhase::Thinking, "", 4, 0);
        assert_eq!(text(&full(&v)), " ▰▰▰━━━━━ ⠋ thinking 4s ");
    }

    #[test]
    fn only_the_spinner_and_the_lit_cells_are_purple() {
        let t = theme();
        let v = view(0, StreamPhase::Thinking, "llama3:8b", 4, 1);
        for span in full(&v) {
            let purple = span.content.contains('⠋') || span.content.contains('▰');
            let want = if purple { t.ai_purple } else { t.text_muted };
            assert_eq!(span.style.fg, Some(want), "{:?}", span.content);
        }
    }

    #[test]
    fn drawn_cells_carry_those_colors() {
        let t = theme();
        let v = view(0, StreamPhase::Thinking, "m", 4, 0);
        let line = Line::from(full(&v));
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 1));
        line.render(Rect::new(0, 0, 40, 1), &mut buf);
        assert_eq!(buf[(1, 0)].symbol(), "▰");
        assert_eq!(buf[(1, 0)].fg, t.ai_purple);
        assert_eq!(buf[(4, 0)].symbol(), "━");
        assert_eq!(buf[(4, 0)].fg, t.text_muted);
    }

    #[test]
    fn narrow_widths_drop_whole_parts_in_order_and_never_overflow() {
        let v = view(0, StreamPhase::Thinking, "llama3:8b", 4, 1);
        // Presence of each part, in drop order.
        let present = |s: &str| {
            [
                s.contains('▰'),
                s.contains("llama3:8b"),
                s.contains("4s"),
                s.contains("+1"),
                s.contains("thinking"),
                s.contains('⠋'),
            ]
        };
        for max in 0..=80 {
            let spans = v.spans(&theme(), max);
            let shown = text(&spans);
            assert!(Line::from(spans).width() <= max, "{max}: {shown:?}");
            let flags = present(&shown);
            // A part is present only if every part dropped after it is too.
            for k in 0..flags.len() {
                if flags[k] {
                    assert!(flags[k..].iter().all(|f| *f), "{max}: {shown:?}");
                }
            }
            if !shown.is_empty() {
                assert!(flags[5], "the spinner is the last to go: {max} {shown:?}");
            }
        }
        assert!(text(&v.spans(&theme(), 0)).is_empty());
        assert!(text(&v.spans(&theme(), 200)).contains('▰'));
    }

    #[test]
    fn snapshots_of_the_indicator() {
        let started = view(0, StreamPhase::Thinking, "llama3:8b", 4, 0);
        let answering = view(5, StreamPhase::Answering, "llama3:8b", 65, 2);
        insta::assert_snapshot!(
            "thinking_indicator",
            format!("{}\n{}", text(&full(&started)), text(&full(&answering)))
        );
    }
}
