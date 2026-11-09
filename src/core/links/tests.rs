use super::*;
use crate::core::opener::is_link_url;

fn row(text: &str) -> Row<'_> {
    Row {
        text,
        lead: 0,
        joins_next: false,
    }
}

fn joined(text: &str) -> Row<'_> {
    Row {
        joins_next: true,
        ..row(text)
    }
}

fn urls(text: &str) -> Vec<String> {
    detect(&[row(text)]).into_iter().map(|l| l.url).collect()
}

fn span(row: usize, start: usize, end: usize) -> Span {
    Span { row, start, end }
}

#[test]
fn a_link_says_its_text_and_its_columns() {
    let links = detect(&[row("see https://a.io/x now")]);
    assert_eq!(
        links,
        [Link {
            url: "https://a.io/x".into(),
            spans: vec![span(0, 4, 18)],
        }]
    );
}

#[test]
fn both_web_schemes_are_links_and_no_other_is() {
    assert_eq!(urls("http://localhost:3000/"), ["http://localhost:3000/"]);
    assert_eq!(urls("https://a.io"), ["https://a.io"]);
    for text in [
        "file:///etc/passwd",
        "javascript:alert(1)",
        "ftp://a.io",
        "ssh://git@a.io",
        "HTTPS://a.io",
    ] {
        assert_eq!(urls(text), [] as [String; 0], "{text}");
    }
}

#[test]
fn a_scheme_with_no_host_is_not_a_link() {
    for text in ["https://", "https:// a.io", "https://.", "https://$HOST/x"] {
        assert_eq!(urls(text), [] as [String; 0], "{text}");
    }
}

#[test]
fn the_tail_of_another_scheme_is_not_a_link() {
    for text in ["xhttps://a.io", "git+https://a.io/r", "a-http://a.io"] {
        assert_eq!(urls(text), [] as [String; 0], "{text}");
    }
}

#[test]
fn the_punctuation_that_ends_a_sentence_is_left_out() {
    for text in [
        "go to https://a.io/x.",
        "https://a.io/x, then",
        "is it https://a.io/x?",
        "https://a.io/x!",
        "'https://a.io/x'",
        "https://a.io/x...",
    ] {
        assert_eq!(urls(text), ["https://a.io/x"], "{text}");
    }
}

#[test]
fn markdown_around_a_link_is_left_out() {
    for text in [
        "[docs](https://a.io/x)",
        "**https://a.io/x**",
        "_https://a.io/x_",
        "`https://a.io/x`",
        "<https://a.io/x>",
        "(see https://a.io/x)",
    ] {
        assert_eq!(urls(text), ["https://a.io/x"], "{text}");
    }
}

#[test]
fn the_brackets_a_link_opened_stay() {
    assert_eq!(
        urls("https://a.io/Rust_(language)"),
        ["https://a.io/Rust_(language)"]
    );
    assert_eq!(
        urls("[x](https://a.io/Rust_(language))"),
        ["https://a.io/Rust_(language)"]
    );
    assert_eq!(urls("https://[::1]:80/a"), ["https://[::1]:80/a"]);
}

#[test]
fn a_link_ends_at_what_it_cannot_hold() {
    assert_eq!(
        urls(r#"{"url":"https://a.io/x","n":1}"#),
        ["https://a.io/x"]
    );
    assert_eq!(urls("https://a.io/x|next"), ["https://a.io/x"]);
}

#[test]
fn a_query_and_a_fragment_are_part_of_the_link() {
    assert_eq!(
        urls("https://a.io/x?y=1&z=2#t"),
        ["https://a.io/x?y=1&z=2#t"]
    );
}

#[test]
fn two_links_on_a_line_are_two_links() {
    assert_eq!(
        urls("https://a.io and http://b.io"),
        ["https://a.io", "http://b.io"]
    );
}

#[test]
fn a_link_that_wraps_is_one_link_with_a_span_per_row() {
    let links = detect(&[joined("x https://a.io/lo"), row("ng/path end")]);
    assert_eq!(
        links,
        [Link {
            url: "https://a.io/long/path".into(),
            spans: vec![span(0, 2, 17), span(1, 0, 7)],
        }]
    );
}

#[test]
fn rows_that_do_not_join_are_not_read_as_one() {
    let links = detect(&[row("https://a.io/lo"), row("ng/path")]);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].url, "https://a.io/lo");
}

#[test]
fn a_scheme_split_by_the_wrap_is_still_found() {
    let links = detect(&[joined("abc http"), row("s://a.io")]);
    assert_eq!(links[0].url, "https://a.io");
    assert_eq!(links[0].spans, [span(0, 4, 8), span(1, 0, 8)]);
}

#[test]
fn the_drawing_at_the_start_of_a_row_is_not_text() {
    let first = Row {
        text: "› https://a.io/lo",
        lead: 2,
        joins_next: true,
    };
    let next = Row {
        text: "  ng",
        lead: 2,
        joins_next: false,
    };
    let links = detect(&[first, next]);
    assert_eq!(
        links,
        [Link {
            url: "https://a.io/long".into(),
            spans: vec![span(0, 2, 17), span(1, 2, 4)],
        }]
    );
}

#[test]
fn a_wide_character_before_a_link_moves_its_columns() {
    let links = detect(&[row("日本 https://a.io")]);
    assert_eq!(links[0].spans, [span(0, 5, 17)]);
}

// What is opened is what the user reads: a character that takes no room on
// screen (a bidi override, a zero-width space) ends the link.
#[test]
fn a_character_with_no_width_ends_a_link() {
    for hidden in ['\u{202E}', '\u{200B}', '\u{2060}', '\u{FEFF}', '\u{7}'] {
        let text = format!("https://a.io/x{hidden}y");
        assert_eq!(urls(&text), ["https://a.io/x"], "{hidden:?}");
    }
}

#[test]
fn every_link_found_may_be_opened() {
    for text in [
        "https://a.io/x.",
        "http://localhost:3000",
        "https://[::1]/",
        "[a](https://a.io/(b))",
    ] {
        for url in urls(text) {
            assert!(is_link_url(&url), "{url}");
        }
    }
}

#[test]
fn empty_rows_have_no_links() {
    assert_eq!(detect(&[]), []);
    assert_eq!(detect(&[row("")]), []);
}
