//! The text under a COPY selection of the transcript.

use super::{Line, LineKind, Wrap};
use crate::core::{
    ai::chat::wrap::clusters,
    copy::{LineSpan, LineText},
};

/// The clean text of the transcript rows under `span`, ready for
/// `core::copy::join`. Columns are display cells, so a wide character is read
/// when either of its cells is selected. The drawing prefix of a row is never
/// read, hint rows are skipped, and a soft-wrapped row goes on into the next
/// one (with the space the wrap dropped, for prose).
pub fn read_rows(lines: &[Line], span: LineSpan) -> Vec<LineText> {
    let last = span.last.min(lines.len().saturating_sub(1));
    (span.first..=last)
        .filter_map(|n| {
            let line = lines.get(n)?;
            if line.kind == LineKind::Hint {
                return None;
            }
            let from = if n == span.first {
                usize::from(span.first_col)
            } else {
                0
            };
            let to = if n == span.last {
                usize::from(span.last_col)
            } else {
                usize::MAX
            };
            Some(read_row(line, from, to))
        })
        .collect()
}

/// The cells `from..=to` of `line`, less its prefix.
fn read_row(line: &Line, from: usize, to: usize) -> LineText {
    let chars: Vec<char> = line.text.chars().collect();
    let mut text = String::new();
    let mut col = 0;
    for cluster in clusters(&chars) {
        let (start, end) = (col, col + cluster.width.max(1) - 1);
        col += cluster.width;
        if start >= line.lead && start <= to && end >= from {
            text.extend(&chars[cluster.chars]);
        }
    }
    if line.wrap == Wrap::Space {
        text.push(' ');
    }
    LineText {
        text,
        wrapped: line.wrap != Wrap::End,
    }
}
