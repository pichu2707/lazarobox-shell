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
    assert_eq!(level_line(JudgeLevel::None, None, &two, Tier::Medium), None);
}

// Spec (acceptance-judges, The agreed standard has three sources).
fn spec(id: &str, text: &str) -> Spec {
    Spec {
        id: id.to_owned(),
        text: text.to_owned(),
    }
}

fn notes(count: usize) -> Vec<String> {
    (1..=count).map(|n| format!("note {n}")).collect()
}

fn norm_of(name: &str) -> Norm {
    Norm {
        name: name.to_owned(),
        text: "be tidy".to_owned(),
        cut_from: None,
    }
}

#[test]
fn the_criteria_come_in_order_with_stable_ids() {
    let got = standard(
        "add a flag",
        &notes(2),
        &[spec("S1", "the flag is off by default")],
        vec![norm_of("AGENTS.md")],
    );
    let texts: Vec<(&str, &str)> = got
        .criteria
        .iter()
        .map(|c| (c.id.as_str(), c.text.as_str()))
        .collect();
    assert_eq!(
        texts,
        [
            ("C1", "add a flag"),
            ("C2", "note 1"),
            ("C3", "note 2"),
            ("C4", "S1: the flag is off by default"),
            ("C5", "the change follows the repo norms below"),
        ]
    );
    assert!(!got.thin);
}

#[test]
fn a_small_goal_has_only_the_goal_and_its_reasons() {
    let got = standard("fix the typo", &notes(1), &[], Vec::new());
    assert_eq!(got.criteria.len(), 2);
    assert!(!got.thin);
}

#[test]
fn a_goal_with_nothing_beyond_itself_is_thin_even_with_norms() {
    let got = standard("fix the typo", &[], &[], vec![norm_of("AGENTS.md")]);
    assert!(got.thin);
    assert_eq!(got.criteria.len(), 2);
    assert!(standard("fix the typo", &[], &[], Vec::new()).thin);
}

#[test]
fn at_most_twelve_criteria_and_the_norms_one_is_kept() {
    let got = standard("g", &notes(30), &[], vec![norm_of("AGENTS.md")]);
    assert_eq!(got.criteria.len(), MAX_CRITERIA);
    assert_eq!(got.criteria[11].id, "C12");
    assert_eq!(
        got.criteria[11].text,
        "the change follows the repo norms below"
    );
    let got = standard("g", &notes(30), &[], Vec::new());
    assert_eq!(got.criteria.len(), MAX_CRITERIA);
    assert_eq!(got.criteria[11].text, "note 11");
}

#[test]
fn every_criterion_is_one_clean_line() {
    let got = standard(
        "first line\nsecond\u{1b}[0m",
        &["a\u{200b}b\nc".to_owned()],
        &[],
        Vec::new(),
    );
    assert_eq!(got.criteria[0].text, "first line");
    assert_eq!(got.criteria[1].text, "ab");
    let long = "x".repeat(500);
    let got = standard(&long, &[], &[], Vec::new());
    assert!(got.criteria[0].text.len() <= 200);
    assert!(got.criteria[0].text.ends_with('\u{2026}'));
}

#[test]
fn the_handoff_notes_are_the_reasons_of_the_closed_steps_without_blanks() {
    use super::super::{
        RunId,
        ledger::{Judgement, Verdict},
        run::{Roster, Run},
        steps::{Grant, Mark},
    };
    use crate::core::ai::{chat::engine::Availability, config::Task};
    let ready = Availability::Ready {
        engine: "claude".to_owned(),
        model: None,
    };
    let roster = Roster::new(vec![
        (Task::Orchestrate, ready.clone()),
        (Task::Code, ready),
    ]);
    let mut run = Run::new(RunId(1), roster, std::time::Duration::from_secs(60));
    let note = |reason: &str, done: &str| Judgement {
        verdict: Verdict::OnCourse,
        reason: reason.to_owned(),
        done: done.to_owned(),
        left: String::new(),
        decisions: String::new(),
    };
    assert!(handoff_notes(run.steps()).is_empty());
    for (step, mark) in [
        (Step::Authorize, Mark::Grant(Grant::Implement)),
        (Step::Explore, Mark::None),
        (Step::Resolve, Mark::None),
        (Step::Classify, Mark::Size(Size::Small)),
    ] {
        run.handoff_step(step, mark, note("fits", "x")).unwrap();
    }
    assert_eq!(
        handoff_notes(run.steps()),
        [
            "resolve reason: fits",
            "resolve done: x",
            "classify reason: fits",
            "classify done: x"
        ]
    );
}

// Spec (Repo norms are capped).
#[test]
fn a_short_file_reaches_the_judge_whole() {
    let got = norm("AGENTS.md", b"# Rules\nbe tidy\n", 16).unwrap();
    assert_eq!(got.name, "AGENTS.md");
    assert_eq!(got.text, "# Rules\nbe tidy\n");
    assert_eq!(got.cut_from, None);
}

#[test]
fn a_long_file_is_cut_at_eight_kib_and_says_its_size() {
    let long = "a".repeat(12 * 1024);
    let got = norm("AGENTS.md", long.as_bytes(), long.len() as u64).unwrap();
    assert_eq!(got.text.len(), NORM_BYTES);
    assert_eq!(got.cut_from, Some(12 * 1024));
    // The caller read only the head: the total still says it was cut.
    let got = norm("AGENTS.md", &long.as_bytes()[..NORM_BYTES], 12 * 1024).unwrap();
    assert_eq!(got.text.len(), NORM_BYTES);
    assert_eq!(got.cut_from, Some(12 * 1024));
}

#[test]
fn the_cut_never_splits_a_multibyte_character() {
    let long = "\u{20ac}".repeat(4000);
    assert_eq!(long.len(), 12000);
    for bytes in [long.as_bytes(), &long.as_bytes()[..NORM_BYTES]] {
        let got = norm("CLAUDE.md", bytes, 12000).unwrap();
        assert!(!got.text.contains('\u{fffd}'));
        assert!(got.text.len() <= NORM_BYTES && got.text.len() > NORM_BYTES - 3);
        assert!(got.text.chars().all(|c| c == '\u{20ac}'));
    }
}

#[test]
fn invalid_utf8_is_read_lossily() {
    let got = norm("AGENTS.md", &[0xff, b'o', b'k'], 3).unwrap();
    assert_eq!(got.text, "\u{fffd}ok");
}

#[test]
fn an_import_line_of_a_file_already_read_is_dropped() {
    assert_eq!(norm("CLAUDE.md", b"@AGENTS.md\n", 11), None);
    let got = norm("CLAUDE.md", b"# Rules\n@AGENTS.md\nbe tidy\n", 27).unwrap();
    assert_eq!(got.text, "# Rules\nbe tidy\n");
}

#[test]
fn an_empty_file_is_not_a_norm() {
    assert_eq!(norm("AGENTS.md", b"", 0), None);
    assert_eq!(norm("AGENTS.md", b"  \n\n", 4), None);
}

// Spec (Verdict format with evidence and location).
fn fault(criterion: usize, at: Option<&str>, text: &str) -> Fault {
    Fault {
        criterion,
        at: at.map(str::to_owned),
        text: text.to_owned(),
    }
}

fn met(evidence: &str) -> Option<(State, String)> {
    Some((State::Met, evidence.to_owned()))
}

#[test]
fn a_well_formed_fail_names_the_criterion_and_where() {
    let text = "CRITERION C1 met | the flag exists\n\
                CRITERION C2 not met | no check on the input\n\
                FINDING C2 | at: src/a.rs:40-52 | the check is missing\n\
                VERDICT fail";
    let got = parse_ruling(text, 2);
    assert_eq!(got.verdict, Ok(Verdict::Fail));
    assert_eq!(
        got.criteria,
        [
            met("the flag exists"),
            Some((State::NotMet, "no check on the input".to_owned()))
        ]
    );
    assert_eq!(
        got.faults,
        [fault(2, Some("src/a.rs:40-52"), "the check is missing")]
    );
    assert_eq!(got.malformed, 0);
}

#[test]
fn a_finding_without_a_location_is_kept_as_unknown() {
    let text = "CRITERION C1 not met | x\nFINDING C1 | the check is missing\nVERDICT fail";
    let got = parse_ruling(text, 1);
    assert_eq!(got.faults, [fault(1, None, "the check is missing")]);
    let text = "CRITERION C1 not met | x\nFINDING C1 | at: | still missing\nVERDICT fail";
    assert_eq!(
        parse_ruling(text, 1).faults,
        [fault(1, None, "still missing")]
    );
}

#[test]
fn a_not_met_criterion_with_no_finding_gets_one_from_its_evidence() {
    let text = "CRITERION C1 not met | line 9 drops the error\nVERDICT fail";
    let got = parse_ruling(text, 1);
    assert_eq!(got.faults, [fault(1, None, "line 9 drops the error")]);
}

#[test]
fn prose_json_and_blank_text_are_no_verdict() {
    let prose = "The change looks good to me, I would merge it.";
    assert_eq!(parse_ruling(prose, 1).verdict, Err(NoVerdict::Unparseable));
    let json = r#"{"verdict": "pass", "criteria": [{"id": "C1", "state": "met"}]}"#;
    assert_eq!(parse_ruling(json, 1).verdict, Err(NoVerdict::Unparseable));
    assert_eq!(parse_ruling("", 1).verdict, Err(NoVerdict::Empty));
    assert_eq!(parse_ruling(" \n\t\n", 0).verdict, Err(NoVerdict::Empty));
}

#[test]
fn an_unseen_line_is_no_verdict_even_beside_a_pass() {
    let text = "UNSEEN: the staged text was not shown\nCRITERION C1 met | x\nVERDICT pass";
    assert_eq!(parse_ruling(text, 1).verdict, Err(NoVerdict::Unseen));
}

#[test]
fn contradictions_are_no_verdict_never_a_pass() {
    let pass_with_failure = "CRITERION C1 not met | x\nVERDICT pass";
    assert_eq!(
        parse_ruling(pass_with_failure, 1).verdict,
        Err(NoVerdict::Inconsistent)
    );
    let fail_with_none = "CRITERION C1 met | x\nVERDICT fail";
    assert_eq!(
        parse_ruling(fail_with_none, 1).verdict,
        Err(NoVerdict::Inconsistent)
    );
    let bare_pass = "VERDICT pass";
    assert_eq!(
        parse_ruling(bare_pass, 2).verdict,
        Err(NoVerdict::Inconsistent)
    );
    let two_verdicts = "CRITERION C1 met | x\nVERDICT pass\nVERDICT fail";
    assert_eq!(
        parse_ruling(two_verdicts, 1).verdict,
        Err(NoVerdict::Inconsistent)
    );
    let repeated = "CRITERION C1 met | x\nVERDICT pass\nVERDICT pass";
    assert_eq!(parse_ruling(repeated, 1).verdict, Ok(Verdict::Pass));
}

#[test]
fn a_pass_carries_its_unverified_criteria_and_reasons() {
    let text = "CRITERION C1 met | a\nCRITERION C2 met | b\n\
                CRITERION C3 unknown | tests not seen\nVERDICT pass";
    let got = parse_ruling(text, 3);
    assert_eq!(got.verdict, Ok(Verdict::Pass));
    assert_eq!(got.unverified(), [(3, "tests not seen")]);
    assert!(got.faults.is_empty());
}

#[test]
fn marks_and_case_do_not_hide_a_line() {
    let text = "- **criterion c1 MET** | fine\n> `Finding C1 | AT: a.rs:3 | odd`\n\
                * **Verdict**: Pass";
    let got = parse_ruling(text, 1);
    assert_eq!(got.verdict, Ok(Verdict::Pass));
    assert_eq!(got.criteria, [met("fine")]);
    assert_eq!(got.faults, [fault(1, Some("a.rs:3"), "odd")]);
}

#[test]
fn the_first_line_of_a_criterion_counts() {
    let text = "CRITERION C1 met | first\nCRITERION C1 not met | second\nVERDICT pass";
    let got = parse_ruling(text, 1);
    assert_eq!(got.criteria, [met("first")]);
    assert_eq!(got.verdict, Ok(Verdict::Pass));
}

#[test]
fn bad_shapes_and_unknown_ids_are_counted_never_dropped_silently() {
    let text = "CRITERION C9 met | x\nCRITERION C1 fine | x\nVERDICT maybe\n\
                FINDING C7 | at: a.rs | x\nCRITERION C1 met | ok";
    let got = parse_ruling(text, 1);
    assert_eq!(got.malformed, 4);
    assert_eq!(got.verdict, Err(NoVerdict::Unparseable));
    assert_eq!(got.criteria, [met("ok")]);
}

#[test]
fn a_hostile_location_is_one_clean_capped_line() {
    let text = "CRITERION C1 not met | x\n\
                FINDING C1 | at: src/a\u{1b}[31m.rs\u{7}\u{202e}:4 | bad\nVERDICT fail";
    let at = parse_ruling(text, 1).faults[0].at.clone().unwrap();
    assert!(!at.chars().any(|c| c.is_control() || c == '\u{202e}'));
    let long = format!(
        "CRITERION C1 not met | {0}\nFINDING C1 | at: {0} | {0}\nVERDICT fail",
        "y".repeat(400)
    );
    let got = parse_ruling(&long, 1);
    let (_, evidence) = got.criteria[0].clone().unwrap();
    assert!(evidence.chars().count() <= 120);
    assert!(got.faults[0].at.as_ref().unwrap().chars().count() <= 120);
    assert!(got.faults[0].text.chars().count() <= 120);
}
