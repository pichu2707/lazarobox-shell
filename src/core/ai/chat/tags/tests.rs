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

// ---- the tag line (chat-history: Edit tags) --------------------------------

fn extras(tags: &Tags) -> Vec<&str> {
    tags.extras.iter().map(String::as_str).collect()
}

#[test]
fn the_first_entry_is_the_project_tag_and_the_rest_are_extras() {
    let tags = Tags::parse("proj,a,b");
    assert_eq!(tags.project.as_deref(), Some("proj"));
    assert_eq!(extras(&tags), ["a", "b"]);
}

#[test]
fn entries_are_trimmed_and_cleaned() {
    let tags = Tags::parse("  proj , a\u{200b}x ,\tb ");
    assert_eq!(tags.project.as_deref(), Some("proj"));
    assert_eq!(extras(&tags), ["ax", "b"]);
}

#[test]
fn no_tag_contains_a_comma() {
    let tags = Tags::new(Some("a,b".into()), vec!["c,d".into()]);
    assert_eq!(tags.project.as_deref(), Some("a_b"));
    assert_eq!(extras(&tags), ["c_d"]);
}

#[test]
fn an_empty_first_entry_means_no_project_tag() {
    let tags = Tags::parse(", a, b");
    assert_eq!(tags.project, None);
    assert_eq!(extras(&tags), ["a", "b"]);
    assert_eq!(Tags::parse("   "), Tags::default());
    assert_eq!(Tags::parse(""), Tags::default());
}

#[test]
fn empty_entries_are_dropped() {
    let tags = Tags::parse("proj,,a, ,b,");
    assert_eq!(extras(&tags), ["a", "b"]);
}

#[test]
fn duplicates_are_kept_once_ignoring_case() {
    let tags = Tags::parse("proj,sql,SQL,Sql,etl");
    assert_eq!(extras(&tags), ["sql", "etl"]);
}

#[test]
fn an_extra_equal_to_the_project_tag_is_dropped() {
    let tags = Tags::parse("Proj,proj,a");
    assert_eq!(tags.project.as_deref(), Some("Proj"));
    assert_eq!(extras(&tags), ["a"]);
}

#[test]
fn at_most_eight_extras_are_kept_in_order() {
    let line = (0..12)
        .map(|n| format!("t{n}"))
        .collect::<Vec<_>>()
        .join(",");
    let tags = Tags::parse(&format!("proj,{line}"));
    assert_eq!(tags.extras.len(), EXTRA_MAX);
    assert_eq!(EXTRA_MAX, 8);
    assert_eq!(tags.extras[0], "t0");
    assert_eq!(tags.extras[7], "t7");
}

#[test]
fn duplicates_do_not_use_up_the_extra_slots() {
    let mut entries = vec!["proj", "sql", "SQL", ""];
    let distinct: Vec<String> = (0..9).map(|n| format!("t{n}")).collect();
    entries.extend(distinct.iter().map(String::as_str));
    let tags = Tags::parse(&entries.join(","));
    assert_eq!(tags.extras.len(), 8);
    assert_eq!(tags.extras[0], "sql");
    assert_eq!(tags.extras[1], "t0");
}

#[test]
fn the_line_shows_the_project_tag_then_the_extras() {
    assert_eq!(Tags::parse("proj,a,b").line(), "proj, a, b");
    assert_eq!(Tags::default().line(), "");
    // Extras without a project keep their place after an empty first entry.
    assert_eq!(Tags::parse(",a").line(), ", a");
}

#[test]
fn the_line_parses_back_to_the_same_tags() {
    for line in ["proj,a,b", ",a,b", "proj", ""] {
        let tags = Tags::parse(line);
        assert_eq!(Tags::parse(&tags.line()), tags, "{line:?}");
    }
}

#[test]
fn normalizing_twice_changes_nothing() {
    let once = Tags::new(
        Some(" P ".into()),
        vec!["a".into(), "A".into(), "b,c".into()],
    );
    assert_eq!(Tags::new(once.project.clone(), once.extras.clone()), once);
}
