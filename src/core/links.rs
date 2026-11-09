//! The web links in the text a pane shows. Pure: it takes the visible rows
//! and says where each link is, so a click on a cell or a list of links can
//! be answered from the same place.
//!
//! Only what the user can read counts. A link is its text, with no hidden
//! target, and a character that takes no room on screen ends it.

use unicode_width::UnicodeWidthChar;

const SCHEMES: [&str; 2] = ["https://", "http://"];

/// Cannot be part of a link without being escaped.
const STOPS: [char; 9] = ['"', '<', '>', '`', '\\', '^', '{', '|', '}'];

/// Taken as the sentence or the Markdown around a link when they end it.
const TRAILING: [char; 10] = ['.', ',', ';', ':', '!', '?', '\'', '*', '_', '~'];

/// One row of text as it is drawn.
#[derive(Clone, Copy, Debug)]
pub struct Row<'a> {
    pub text: &'a str,
    /// Display columns at the start of `text` that are drawing, not text.
    pub lead: usize,
    /// The next row goes on from this one with nothing in between.
    pub joins_next: bool,
}

/// The columns `start..end` of `row` that a link takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span {
    pub row: usize,
    pub start: usize,
    pub end: usize,
}

/// A link and where it is drawn: one span per row it takes.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Link {
    pub url: String,
    pub spans: Vec<Span>,
}

#[derive(Clone, Copy)]
struct Cell {
    ch: char,
    row: usize,
    col: usize,
    width: usize,
}

/// The links of `rows`, in reading order.
pub fn detect(rows: &[Row]) -> Vec<Link> {
    let mut links = Vec::new();
    let mut line = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        push_cells(&mut line, index, row);
        if !row.joins_next {
            scan(&line, &mut links);
            line.clear();
        }
    }
    scan(&line, &mut links);
    links
}

fn push_cells(line: &mut Vec<Cell>, row: usize, text: &Row) {
    let mut col = 0;
    for ch in text.text.chars() {
        let width = ch.width().unwrap_or(0);
        if col >= text.lead {
            line.push(Cell {
                ch,
                row,
                col,
                width,
            });
        }
        col += width;
    }
}

fn scan(line: &[Cell], links: &mut Vec<Link>) {
    let mut at = 0;
    while at < line.len() {
        match link_end(line, at) {
            Some(end) => {
                links.push(link(&line[at..end]));
                at = end;
            }
            None => at += 1,
        }
    }
}

/// Where the link that starts at `at` ends, if one starts there.
fn link_end(line: &[Cell], at: usize) -> Option<usize> {
    if at > 0 && extends_scheme(line[at - 1].ch) {
        return None;
    }
    let rest = &line[at..];
    let scheme = SCHEMES
        .iter()
        .find(|scheme| starts_with(rest, scheme))?
        .chars()
        .count();
    let stop = rest.iter().position(ends_link).unwrap_or(rest.len());
    let len = trimmed(&rest[..stop]);
    let host = rest[..len].get(scheme)?.ch;
    (host.is_alphanumeric() || host == '[').then_some(at + len)
}

/// `https://` inside `git+https://` or `xhttps://` is another scheme's tail.
fn extends_scheme(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '+' | '-' | '.')
}

fn starts_with(cells: &[Cell], text: &str) -> bool {
    let mut cells = cells.iter();
    text.chars()
        .all(|ch| cells.next().is_some_and(|cell| cell.ch == ch))
}

fn ends_link(cell: &Cell) -> bool {
    cell.width == 0 || cell.ch.is_whitespace() || STOPS.contains(&cell.ch)
}

/// How many of `cells` are left once what closes the text around the link
/// is taken off. A closing bracket stays when the link opened it.
fn trimmed(cells: &[Cell]) -> usize {
    let mut len = cells.len();
    while len > 0 {
        let drop = match cells[len - 1].ch {
            ')' => unbalanced(&cells[..len], '(', ')'),
            ']' => unbalanced(&cells[..len], '[', ']'),
            ch => TRAILING.contains(&ch),
        };
        if !drop {
            break;
        }
        len -= 1;
    }
    len
}

fn unbalanced(cells: &[Cell], open: char, close: char) -> bool {
    let count = |wanted| cells.iter().filter(|cell| cell.ch == wanted).count();
    count(close) > count(open)
}

fn link(cells: &[Cell]) -> Link {
    let mut spans: Vec<Span> = Vec::new();
    for cell in cells {
        let end = cell.col + cell.width;
        match spans.last_mut().filter(|span| span.row == cell.row) {
            Some(span) => span.end = end,
            None => spans.push(Span {
                row: cell.row,
                start: cell.col,
                end,
            }),
        }
    }
    Link {
        url: cells.iter().map(|cell| cell.ch).collect(),
        spans,
    }
}

#[cfg(test)]
mod tests;
