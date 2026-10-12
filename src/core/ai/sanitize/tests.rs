use super::*;

/// Sanitizes `parts` chunk by chunk, then ends the stream.
fn chunked(parts: &[&str]) -> String {
    let mut sanitizer = StreamSanitizer::new();
    let mut out: String = parts.iter().map(|p| sanitizer.push(p)).collect();
    sanitizer.finish();
    out.push_str(&sanitizer.push(""));
    out
}

/// Every two-chunk cut of `text` gives `want`, so does one char at a time.
fn same_at_every_split(text: &str, want: &str) {
    assert_eq!(clean(text), want, "whole: {text:?}");
    for at in (0..=text.len()).filter(|&i| text.is_char_boundary(i)) {
        let (a, b) = text.split_at(at);
        assert_eq!(chunked(&[a, b]), want, "cut at {at}: {text:?}");
    }
    let singles: Vec<String> = text.chars().map(String::from).collect();
    let parts: Vec<&str> = singles.iter().map(String::as_str).collect();
    assert_eq!(chunked(&parts), want, "per char: {text:?}");
}

#[test]
fn control_characters_are_dropped_and_crlf_becomes_lf() {
    assert_eq!(clean("a\u{07}b\u{00}c\u{7f}d\u{9b}e\r\nf\rg"), "abcde\nfg");
    assert_eq!(clean("tab\there\nnext"), "tab\there\nnext");
    assert_eq!(clean("\u{80}x\u{9f}y"), "xy");
}

#[test]
fn csi_and_osc_sequences_are_removed_with_their_payload() {
    assert_eq!(clean("x\u{1b}[31mred\u{1b}[0m"), "xred");
    assert_eq!(clean("y\u{1b}]0;title\u{07}z"), "yz");
    assert_eq!(clean("y\u{1b}]8;;http://e\u{1b}\\link"), "ylink");
}

#[test]
fn string_sequences_end_at_st_and_only_osc_ends_at_bel() {
    let text = "a\u{1b}P1$r0m\u{1b}\\b\u{1b}_payload\u{1b}\\c\u{1b}^pm\u{1b}\\d\u{1b}Xsos\u{1b}\\e";
    assert_eq!(clean(text), "abcde");
    assert_eq!(clean("a\u{1b}Pdcs\u{07}still\u{1b}\\b"), "ab");
    assert_eq!(clean("a\u{1b}]osc\u{9c}b"), "ab");
}

#[test]
fn two_character_escape_sequences_are_dropped() {
    assert_eq!(clean("a\u{1b}cb"), "ab");
    assert_eq!(clean("a\u{1b}(Bb"), "ab");
    assert_eq!(clean("a\u{1b}\u{1b}[1mb"), "ab");
    assert_eq!(clean("a\u{1b}\nb"), "a\nb");
}

#[test]
fn bidi_and_format_characters_are_dropped() {
    assert_eq!(
        clean("a\u{202e}b\u{2066}c\u{200b}d\u{feff}e\u{ad}f\u{e0041}g"),
        "abcdefg"
    );
}

#[test]
fn a_sequence_split_at_any_point_matches_the_whole() {
    same_at_every_split("\u{1b}[31mred", "red");
    same_at_every_split("\u{1b}]0;title\u{07}", "");
    same_at_every_split(
        "x\u{1b}]8;;http://e\u{1b}\\link\u{1b}P1$r\u{1b}\\y",
        "xlinky",
    );
    same_at_every_split("a\r\nb\u{1b}(Bc\u{1b}\u{1b}[1md", "a\nbcd");
    same_at_every_split("é€😀\u{202e}ok", "é€😀ok");
}

#[test]
fn an_unterminated_sequence_is_dropped_at_finish() {
    assert_eq!(clean("ok\u{1b}]0;never ended"), "ok");
    assert_eq!(clean("ok\u{1b}[31"), "ok");
    assert_eq!(clean("ok\u{1b}"), "ok");
    assert_eq!(chunked(&["ok\u{1b}]0;never", " ended"]), "ok");
}

#[test]
fn finish_resets_the_state_for_the_next_stream() {
    let mut sanitizer = StreamSanitizer::new();
    assert_eq!(sanitizer.push("a\u{1b}]0;x"), "a");
    sanitizer.finish();
    assert_eq!(sanitizer.push("b\u{07}c"), "bc");
}

#[test]
fn an_unterminated_string_gives_up_after_the_cap_and_text_resumes() {
    let long = "a".repeat(STRING_CAP + 904);
    let out = clean(&format!("\u{1b}]0;{long}tail"));
    assert!(!out.contains('\u{1b}'));
    assert!(out.ends_with("tail"));
    // The two payload characters `0;` count towards the cap.
    assert_eq!(out.chars().count(), 2 + 904 + 4);
    let at_cap = "a".repeat(STRING_CAP - 2);
    assert_eq!(clean(&format!("\u{1b}]0;{at_cap}\u{07}tail")), "tail");
    assert_eq!(clean(&format!("\u{1b}]0;{at_cap}tail")), "tail");
}

#[test]
fn chained_string_restarts_never_swallow_more_than_the_cap() {
    // Each `ESC ] x` unit swallows two characters (`x` and the ESC), so 2048
    // units reach the cap. The next unit's introducer is refused, its `x`
    // is plain text, and that text resets the count for the units after it.
    let text = "\u{1b}]x".repeat(2100);
    assert_eq!(clean(&text), "x");
    // Without any text in between, the budget stays spent across restarts.
    let text = format!("{}end", "\u{1b}]\u{1b}]".repeat(5000));
    assert_eq!(clean(&text), "end");
}

#[test]
fn a_restart_after_a_string_escape_keeps_counting() {
    let mut sanitizer = StreamSanitizer::new();
    let mut out = String::new();
    for _ in 0..STRING_CAP {
        out.push_str(&sanitizer.push("\u{1b}]\u{1b}"));
    }
    // The cap was hit on an ESC, which can still start the next sequence.
    out.push_str(&sanitizer.push("\nshown"));
    assert_eq!(out, "\nshown");
}

#[test]
fn a_csi_without_a_final_byte_gives_up_after_the_cap() {
    let out = clean(&format!("\u{1b}[{}", "1".repeat(300)));
    assert_eq!(out, "1".repeat(300 - CSI_CAP));
    // A sequence that ends within the cap is dropped whole.
    assert_eq!(clean(&format!("\u{1b}[{}mok", "1;".repeat(100))), "ok");
}

/// Bytes through `Utf8Stream` then `StreamSanitizer`, cut at `cuts`.
fn bytes_chunked(bytes: &[u8], cuts: &[usize]) -> String {
    let (mut utf8, mut sanitizer) = (Utf8Stream::new(), StreamSanitizer::new());
    let mut out = String::new();
    let mut from = 0;
    for &to in cuts.iter().chain([&bytes.len()]) {
        out.push_str(&sanitizer.push(&utf8.push(&bytes[from..to])));
        from = to;
    }
    out.push_str(&sanitizer.push(&utf8.finish()));
    sanitizer.finish();
    out
}

#[test]
fn multibyte_characters_survive_every_internal_cut() {
    for text in ["a\u{e9}b", "a\u{20ac}b", "a\u{1f600}b"] {
        for at in 0..=text.len() {
            let bytes = text.as_bytes();
            assert_eq!(bytes_chunked(bytes, &[at]), text, "cut at {at}: {text:?}");
        }
        let all: Vec<usize> = (1..text.len()).collect();
        assert_eq!(bytes_chunked(text.as_bytes(), &all), text, "per byte");
    }
}

#[test]
fn invalid_bytes_and_a_cut_tail_become_replacement_characters() {
    assert_eq!(bytes_chunked(b"a\xffb", &[]), "a\u{fffd}b");
    assert_eq!(bytes_chunked(b"a\xe2", &[]), "a\u{fffd}");
    assert_eq!(bytes_chunked(b"a\xf0\x9f\x98", &[2, 3]), "a\u{fffd}");
    assert_eq!(bytes_chunked(b"\xe2\x82x", &[1, 2]), "\u{fffd}x");
    assert_eq!(
        bytes_chunked(b"\x80\xc0\xaf", &[]),
        "\u{fffd}\u{fffd}\u{fffd}"
    );
}

#[test]
fn finish_clears_the_pending_bytes() {
    let mut utf8 = Utf8Stream::new();
    assert_eq!(utf8.push(b"\xe2\x82"), "");
    assert_eq!(utf8.finish(), "\u{fffd}");
    assert_eq!(utf8.finish(), "");
    assert_eq!(utf8.push(b"ok"), "ok");
}

#[test]
fn random_bytes_cut_at_random_points_never_leak_a_control() {
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move |n: usize| {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (seed >> 33) as usize % n
    };
    // Weighted towards escapes, terminators and multibyte lead bytes.
    let alphabet: &[u8] =
        b"\x1b\x1b[]\\\x07\x00\r\n\txP_^X;0m \xc2\x9b\x9c\xe2\x80\xae\xf0\x9f\xff";
    for _ in 0..300 {
        let bytes: Vec<u8> = (0..next(60))
            .map(|_| alphabet[next(alphabet.len())])
            .collect();
        let whole = bytes_chunked(&bytes, &[]);
        let mut cuts: Vec<usize> = (0..next(6)).map(|_| next(bytes.len() + 1)).collect();
        cuts.sort_unstable();
        assert_eq!(bytes_chunked(&bytes, &cuts), whole, "{bytes:?} {cuts:?}");
        let bad = |c: char| (c.is_control() && c != '\n' && c != '\t') || is_format(c);
        assert!(!whole.chars().any(bad), "{bytes:?} gave {whole:?}");
    }
}
