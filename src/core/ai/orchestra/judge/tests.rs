//! Acceptance judges, pure core (acceptance-judges S1a).

use super::*;

fn change(path: &str, size: u64) -> Change {
    Change {
        path: path.to_owned(),
        size: Some(size),
        hash: "0123456789abcdef".to_owned(),
        applied: false,
    }
}

// Spec (acceptance-judges, Judge level from the scale of the change).
#[test]
fn a_read_only_goal_gets_no_judge_whatever_else_holds() {
    let big = [change("a.rs", 1), change("b.rs", 1)];
    let got = level(true, Some(Size::Large), &big, Tier::High);
    assert_eq!(got, JudgeLevel::None);
}

#[test]
fn a_small_single_file_change_below_medium_gets_one_judge() {
    let one = [change("a.rs", 120)];
    assert_eq!(
        level(false, Some(Size::Small), &one, Tier::Passive),
        JudgeLevel::One
    );
}

#[test]
fn two_files_get_two_judges() {
    let two = [change("a.rs", 1), change("b.rs", 1)];
    assert_eq!(
        level(false, Some(Size::Small), &two, Tier::Passive),
        JudgeLevel::Two
    );
}

#[test]
fn a_large_goal_or_a_medium_tier_gets_two_judges() {
    let one = [change("a.rs", 1)];
    assert_eq!(
        level(false, Some(Size::Large), &one, Tier::Passive),
        JudgeLevel::Two
    );
    assert_eq!(
        level(false, Some(Size::Small), &one, Tier::Medium),
        JudgeLevel::Two
    );
    assert_eq!(
        level(false, Some(Size::Small), &one, Tier::High),
        JudgeLevel::Two
    );
}

// D1: the staged bytes raise one to two at 16 KiB, never lower a level.
#[test]
fn the_byte_threshold_sits_at_sixteen_kib() {
    let below = [change("a.rs", BIG_BYTES - 1)];
    let at = [change("a.rs", BIG_BYTES)];
    assert_eq!(BIG_BYTES, 16384);
    assert_eq!(
        level(false, Some(Size::Small), &below, Tier::Passive),
        JudgeLevel::One
    );
    assert_eq!(
        level(false, Some(Size::Small), &at, Tier::Passive),
        JudgeLevel::Two
    );
}

#[test]
fn the_bytes_of_every_file_add_up_and_a_folder_counts_nothing() {
    let dir = Change {
        size: None,
        ..change("d", 0)
    };
    let sum = [change("a.rs", 9000), dir];
    assert_eq!(
        level(false, Some(Size::Small), &sum[..1], Tier::Passive),
        JudgeLevel::One
    );
    let sum = [change("a.rs", 9000), change("b.rs", 9000)];
    assert_eq!(
        level(false, Some(Size::Small), &sum, Tier::Passive),
        JudgeLevel::Two
    );
}

#[test]
fn an_unknown_size_reads_as_small() {
    let one = [change("a.rs", 1)];
    assert_eq!(level(false, None, &one, Tier::Passive), JudgeLevel::One);
}

#[test]
fn the_level_line_shows_every_input() {
    let one = [change("a.rs", 120)];
    let line = level_line(JudgeLevel::One, Some(Size::Small), &one, Tier::Passive);
    assert_eq!(
        line.as_deref(),
        Some("judge: one judge \u{b7} 1 file(s), 120 B \u{b7} tier passive \u{b7} size small")
    );
    let two = [change("a.rs", 5), change("b.rs", 6)];
    let line = level_line(JudgeLevel::Two, None, &two, Tier::Medium);
    assert_eq!(
        line.as_deref(),
        Some("judge: two judges \u{b7} 2 file(s), 11 B \u{b7} tier medium \u{b7} size unknown")
    );
    assert_eq!(
        level_line(JudgeLevel::None, None, &two, Tier::Medium),
        None
    );
}
