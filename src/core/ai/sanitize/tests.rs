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
