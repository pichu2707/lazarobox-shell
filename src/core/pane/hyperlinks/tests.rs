use super::super::{Pane, PaneSize};
use crate::core::links::{Link, Span};

fn pane(rows: u16, cols: u16) -> Pane {
    Pane::new(PaneSize { rows, cols }, 100)
}

fn link(url: &str, text: &str) -> String {
    format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")
}

fn feed(p: &mut Pane, text: &str) {
    p.feed(text.as_bytes());
}

fn span(row: usize, start: usize, end: usize) -> Span {
    Span { row, start, end }
}

/// The text each span of `link` covers in the rows `link_rows` reads.
fn shown(p: &mut Pane, link: &Link) -> Vec<String> {
    let (rows, _) = p.link_rows();
    (link.spans.iter())
        .map(|s| {
            rows[s.row]
                .text
                .chars()
                .skip(s.start)
                .take(s.end - s.start)
                .collect()
        })
        .collect()
}

#[test]
fn a_link_is_where_its_text_is_and_says_it_hides_its_url() {
    let mut p = pane(3, 20);
    feed(
        &mut p,
        &format!("see {} ok", link("https://a.io/x", "⧉ icon")),
    );
    assert_eq!(
        p.hyperlinks(),
        [Link {
            url: "https://a.io/x".into(),
            spans: vec![span(0, 4, 10)],
            hidden: true,
        }]
    );
}

#[test]
fn a_link_showing_its_url_hides_nothing() {
    let mut p = pane(3, 20);
    feed(&mut p, &link("https://a.io", " https://a.io"));
    let links = p.hyperlinks();
    assert_eq!(links.len(), 1);
    assert!(!links[0].hidden);
    assert_eq!(links[0].spans, [span(0, 0, 13)]);
}

#[test]
fn a_link_split_across_reads_is_found() {
    let mut p = pane(3, 20);
    feed(&mut p, "\x1b]8;;https://a.io\x1b");
    feed(&mut p, "\\a");
    feed(&mut p, "b\x1b]8;;\x1b\\");
    let links = p.hyperlinks();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].url, "https://a.io");
    assert_eq!(links[0].spans, [span(0, 0, 2)]);
}

#[test]
fn a_link_that_wraps_takes_one_span_per_row() {
    let mut p = pane(3, 10);
    feed(
        &mut p,
        &format!("1234567{}", link("https://a.io", "abcdef")),
    );
    let links = p.hyperlinks();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].spans, [span(0, 7, 10), span(1, 0, 3)]);
    assert_eq!(shown(&mut p, &links[0]), ["abc", "def"]);
}

#[test]
fn a_link_that_ends_on_the_last_column_or_after_its_line_break_is_whole() {
    let mut p = pane(3, 10);
    feed(&mut p, &format!("1234567{}", link("https://a.io", "abc")));
    assert_eq!(p.hyperlinks()[0].spans, [span(0, 7, 10)]);
    let mut p = pane(3, 10);
    feed(&mut p, &format!("{}\r\n", "\x1b]8;;https://a.io\x1b\\ab"));
    feed(&mut p, "\x1b]8;;\x1b\\");
    assert_eq!(p.hyperlinks()[0].spans, [span(0, 0, 2)]);
}

#[test]
fn a_link_cut_by_a_line_break_or_a_cursor_jump_is_no_link() {
    let mut p = pane(3, 20);
    feed(&mut p, &link("https://a.io", "ab\r\ncd"));
    assert_eq!(p.hyperlinks(), []);
    let mut p = pane(3, 20);
    feed(
        &mut p,
        &format!("    {}", link("https://a.io", "ab\x1b[1;1H")),
    );
    assert_eq!(p.hyperlinks(), []);
}

#[test]
fn a_link_drawn_over_is_gone_and_one_redrawn_the_same_stays_one() {
    let mut p = pane(3, 20);
    let drawn = link("https://a.io", "click");
    feed(&mut p, &drawn);
    feed(&mut p, &format!("\r{drawn}"));
    assert_eq!(p.hyperlinks().len(), 1);
    feed(&mut p, "\rother");
    assert_eq!(p.hyperlinks(), []);
}

#[test]
fn a_link_in_the_history_follows_its_line() {
    // Without history the line count comes from the history growing; with
    // it, from the pinned view.
    for before in ["", "a\r\nb\r\nc\r\nd\r\n"] {
        let mut p = pane(3, 20);
        feed(&mut p, before);
        let out = format!(
            "x\r\ny\r\n{}\r\nz\r\n\r\n\r\n",
            link("https://a.io", "here")
        );
        feed(&mut p, &out);
        assert_eq!(p.hyperlinks(), [], "{before:?}: the link left the view");
        p.set_scrollback(4);
        let links = p.hyperlinks();
        assert_eq!(links.len(), 1, "{before:?}");
        assert_eq!(shown(&mut p, &links[0]), ["here"], "{before:?}");
    }
}

#[test]
fn a_resize_or_a_screen_flip_forgets_links() {
    let mut p = pane(3, 20);
    feed(&mut p, &link("https://a.io", "click"));
    p.resize(PaneSize { rows: 3, cols: 30 });
    assert_eq!(p.hyperlinks(), []);

    let mut p = pane(3, 20);
    // A chunk that flips the screen forgets what it drew: TUIs draw again.
    feed(&mut p, "\x1b[?1049h");
    feed(&mut p, &link("https://a.io", "click"));
    assert_eq!(p.hyperlinks().len(), 1, "a link on the alternate screen");
    feed(&mut p, "\x1b[?1049l");
    assert_eq!(p.hyperlinks(), []);
}

#[test]
fn only_web_links_that_end_are_links_and_a_new_one_ends_the_last() {
    let mut p = pane(3, 30);
    feed(&mut p, &link("file:///etc/passwd", "secret"));
    feed(&mut p, "\r\n\x1b]8;;https://a.io\x1b\\never closed");
    assert_eq!(p.hyperlinks(), []);

    let mut p = pane(3, 30);
    feed(
        &mut p,
        "\x1b]8;;https://a.io\x1b\\ab\x1b]8;;https://b.io\x1b\\cd",
    );
    feed(&mut p, "\x1b]8;;\x1b\\");
    let links = p.hyperlinks();
    let urls: Vec<_> = links
        .iter()
        .map(|l| (l.url.as_str(), l.spans.clone()))
        .collect();
    assert_eq!(
        urls,
        [
            ("https://a.io", vec![span(0, 0, 2)]),
            ("https://b.io", vec![span(0, 2, 4)])
        ]
    );
}

#[test]
fn osc7_still_reports_the_working_directory() {
    let mut p = pane(3, 20);
    feed(&mut p, "\x1b]7;file:///home/a\x1b\\");
    assert_eq!(p.cwd(), Some(std::path::Path::new("/home/a")));
}

#[test]
fn a_link_longer_than_max_rows_is_no_link() {
    let mut p = Pane::new(PaneSize { rows: 40, cols: 2 }, 100);
    feed(
        &mut p,
        &link("https://a.io", &"x".repeat(2 * super::MAX_ROWS)),
    );
    assert_eq!(p.hyperlinks()[0].spans.len(), super::MAX_ROWS);
    let mut p = Pane::new(PaneSize { rows: 40, cols: 2 }, 100);
    feed(
        &mut p,
        &link("https://a.io", &"x".repeat(2 * super::MAX_ROWS + 1)),
    );
    assert_eq!(p.hyperlinks(), []);
}

#[test]
fn a_pane_keeps_max_links_and_forgets_the_oldest() {
    let mut recorder = super::Recorder::default();
    for line in 0..=super::MAX_LINKS {
        recorder.keep(super::Hyperlink {
            url: "https://a.io".into(),
            line,
            pieces: vec![super::Piece {
                start: 0,
                end: 1,
                text: "a".into(),
            }],
        });
    }
    assert_eq!(recorder.links.len(), super::MAX_LINKS);
    assert_eq!(recorder.links[0].line, 1);
}

#[test]
fn a_link_among_rows_of_the_same_icon_lands_on_its_own_row() {
    let mut p = pane(5, 20);
    feed(&mut p, "x\r\n");
    let out = format!(
        "⧉\r\n⧉\r\n⧉\r\n{}\r\n⧉\r\n⧉\r\n⧉\r\n",
        link("https://a.io", "⧉")
    );
    feed(&mut p, &out);
    // Lines 0..=8; the screen shows 4..=8, and the link is line 4 (row 0).
    assert_eq!(p.hyperlinks()[0].spans, [span(0, 0, 1)]);
}

#[test]
fn a_payload_vte_may_have_cut_is_no_link() {
    let mut p = pane(3, 200);
    feed(
        &mut p,
        &link(&format!("https://a.io/{}", "a;".repeat(14)), "x"),
    );
    assert_eq!(p.hyperlinks(), []);
}
