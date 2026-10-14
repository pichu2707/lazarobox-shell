use super::*;

// Spec (Project tag): Tag cleaning.
#[test]
fn a_tag_is_trimmed() {
    assert_eq!(clean_tag("  proj \t").as_deref(), Some("proj"));
}

#[test]
fn control_and_format_characters_are_removed() {
    // A bidi override and a zero-width space inside, a control at the end.
    assert_eq!(
        clean_tag("pr\u{202e}o\u{200b}j\u{7}").as_deref(),
        Some("proj")
    );
}

#[test]
fn escape_sequences_do_not_survive() {
    assert_eq!(clean_tag("a\u{1b}[31mb").as_deref(), Some("ab"));
}

#[test]
fn a_tag_is_cut_at_64_characters() {
    let long = "x".repeat(100);
    let tag = clean_tag(&long).unwrap();
    assert_eq!(tag.chars().count(), TAG_MAX);
    assert_eq!(TAG_MAX, 64);
    // Characters, not bytes.
    let wide = "é".repeat(100);
    assert_eq!(clean_tag(&wide).unwrap().chars().count(), 64);
}

#[test]
fn the_cut_does_not_leave_trailing_space() {
    let text = format!("{} tail", "x".repeat(63));
    assert_eq!(clean_tag(&text).unwrap(), "x".repeat(63));
}

#[test]
fn a_tag_that_is_empty_after_cleaning_is_no_tag() {
    assert_eq!(clean_tag(""), None);
    assert_eq!(clean_tag("  \t "), None);
    assert_eq!(clean_tag("\u{202e}\u{200b}"), None);
}

#[test]
fn a_comma_is_never_part_of_a_tag() {
    assert_eq!(clean_tag("a,b").as_deref(), Some("a_b"));
}
