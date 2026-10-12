use super::machine_tests::{assigned, at_models, det, index_of};
use super::*;

fn custom(draft: &AiConfig, d: &Detection) -> Picker {
    let mut p = at_models(draft, d, "claude");
    let rows = p.rows(draft, Some(d));
    p.cursor = index_of(&rows, PickKey::Custom);
    assert_eq!(p.enter(draft, Some(d)), Outcome::Stay);
    p
}

fn type_str(p: &mut Picker, s: &str) {
    for c in s.chars() {
        p.type_char(c);
    }
}

#[test]
fn validate_trims_and_rejects_blank() {
    assert_eq!(validate_custom("  my-model  "), Some("my-model".to_owned()));
    assert_eq!(validate_custom(""), None);
    assert_eq!(validate_custom("     "), None);
}

#[test]
fn validate_caps_at_128_characters() {
    let long = "a".repeat(129);
    assert_eq!(validate_custom(&long), Some("a".repeat(128)));
}

#[test]
fn validate_rejects_control_characters() {
    assert_eq!(validate_custom("a\u{7}b"), None);
}

#[test]
fn letters_that_are_shortcuts_are_typed() {
    let d = det(&[]);
    let draft = AiConfig::default();
    let mut p = custom(&draft, &d);
    type_str(&mut p, "msjkhlr-1");
    assert_eq!(p.text(), Some("msjkhlr-1"));
}

#[test]
fn whitespace_control_and_format_characters_are_dropped() {
    let d = det(&[]);
    let draft = AiConfig::default();
    let mut p = custom(&draft, &d);
    type_str(&mut p, "a b\tc\u{7}d\u{202e}e\u{200b}f");
    assert_eq!(p.text(), Some("abcdef"));
}

#[test]
fn input_beyond_128_characters_is_dropped() {
    let d = det(&[]);
    let draft = AiConfig::default();
    let mut p = custom(&draft, &d);
    type_str(&mut p, &"x".repeat(130));
    assert_eq!(p.text().map(|t| t.chars().count()), Some(128));
}

#[test]
fn backspace_removes_the_last_character() {
    let d = det(&[]);
    let draft = AiConfig::default();
    let mut p = custom(&draft, &d);
    type_str(&mut p, "abé");
    p.backspace();
    assert_eq!(p.text(), Some("ab"));
    p.backspace();
    p.backspace();
    p.backspace();
    assert_eq!(p.text(), Some(""));
}

#[test]
fn ctrl_u_clears_and_stays_in_the_field() {
    let d = det(&[]);
    let draft = AiConfig::default();
    let mut p = custom(&draft, &d);
    type_str(&mut p, "abc");
    p.clear_text();
    assert_eq!(p.text(), Some(""));
}

#[test]
fn paste_keeps_the_first_line_filtered_and_capped() {
    let d = det(&[]);
    let draft = AiConfig::default();
    let mut p = custom(&draft, &d);
    type_str(&mut p, "ab");
    p.paste("c d\nef");
    assert_eq!(p.text(), Some("abcd"));
    p.paste("\r\nxyz");
    assert_eq!(p.text(), Some("abcd"));
    p.paste(&"z".repeat(200));
    assert_eq!(p.text().map(|t| t.chars().count()), Some(128));
}

#[test]
fn enter_commits_a_valid_name_and_refuses_a_blank_one() {
    let d = det(&[]);
    let draft = AiConfig::default();
    let mut p = custom(&draft, &d);
    assert_eq!(p.enter(&draft, Some(&d)), Outcome::Stay);
    type_str(&mut p, "x");
    p.backspace();
    assert_eq!(p.enter(&draft, Some(&d)), Outcome::Stay);
    assert!(matches!(p.level, Level::Custom { .. }));
    type_str(&mut p, "ms-1");
    assert_eq!(
        p.enter(&draft, Some(&d)),
        Outcome::Commit(assigned("claude", "ms-1"))
    );
}

#[test]
fn typing_outside_the_field_does_nothing() {
    let d = det(&[]);
    let draft = AiConfig::default();
    let mut p = at_models(&draft, &d, "claude");
    let before = p.clone();
    p.type_char('a');
    p.backspace();
    p.clear_text();
    p.paste("abc");
    assert_eq!(p, before);
    assert_eq!(p.text(), None);
}

#[test]
fn leaving_the_field_discards_the_buffer() {
    let d = det(&[]);
    let draft = AiConfig::default();
    let mut p = custom(&draft, &d);
    type_str(&mut p, "abc");
    p.back(&draft, Some(&d));
    let rows = p.rows(&draft, Some(&d));
    assert_eq!(p.cursor(), index_of(&rows, PickKey::Custom));
    p.enter(&draft, Some(&d));
    assert_eq!(p.text(), Some(""));
}

// Characterization: pins the cap by characters, not bytes.
#[test]
fn the_cap_counts_characters_not_bytes() {
    let d = det(&[]);
    let draft = AiConfig::default();
    let mut p = custom(&draft, &d);
    type_str(&mut p, &"é".repeat(200));
    assert_eq!(p.text(), Some("é".repeat(128).as_str()));
}

// Characterization: a bare carriage return ends the pasted first line too.
#[test]
fn a_paste_stops_at_a_carriage_return() {
    let d = det(&[]);
    let draft = AiConfig::default();
    let mut p = custom(&draft, &d);
    p.paste("a\rb");
    assert_eq!(p.text(), Some("a"));
}

#[test]
fn validate_rejects_format_and_bidi_characters() {
    assert_eq!(validate_custom("a\u{202e}b"), None);
    assert_eq!(validate_custom("a\u{200b}b"), None);
}
