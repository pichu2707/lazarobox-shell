use super::*;
use crate::core::ai::chat::engine::{CallId, ToolStatus};

fn call(trail: &mut Trail, id: u64, name: &str, summary: &str) {
    trail.call(CallId(id), name, summary);
}

#[test]
fn a_call_and_its_result_are_one_line_with_the_outcome() {
    let mut trail = Trail::default();
    call(&mut trail, 1, "read_file", "src/main.rs");
    trail.result(CallId(1), ToolStatus::Ok, "");
    assert_eq!(trail.lines(), vec!["read_file src/main.rs -> ok"]);
}

#[test]
fn a_refused_call_keeps_its_reason_and_an_error_its_text() {
    let mut trail = Trail::default();
    call(&mut trail, 1, "spawn", "code");
    trail.result(CallId(1), ToolStatus::Denied, "step authorize is open");
    call(&mut trail, 2, "exec", "cargo test");
    trail.result(CallId(2), ToolStatus::Error, "exit 101");
    assert_eq!(
        trail.lines(),
        vec![
            "spawn code -> refused: step authorize is open",
            "exec cargo test -> error: exit 101"
        ]
    );
}

#[test]
fn a_call_with_no_result_says_so_and_a_stray_result_is_ignored() {
    let mut trail = Trail::default();
    call(&mut trail, 1, "wait", "worker 1");
    trail.result(CallId(9), ToolStatus::Ok, "");
    assert_eq!(trail.lines(), vec!["wait worker 1 -> no result"]);
}

#[test]
fn a_handoff_keeps_the_step_and_the_verdict() {
    let mut trail = Trail::default();
    trail.handoff("step explore", "on_course", "mapped the files");
    assert_eq!(
        trail.lines(),
        vec!["handoff step explore on_course: mapped the files"]
    );
}

#[test]
fn text_is_one_clean_line_without_tokens_and_cut() {
    let mut trail = Trail::default();
    call(
        &mut trail,
        1,
        "http\nget",
        "Authorization: Bearer ghp_abcdefghijklmnopqrstuvwxyz0123456789",
    );
    let line = &trail.lines()[0];
    assert!(!line.contains('\n') && !line.contains("ghp_abcdefghijklmnopqrstuvwxyz"));
    call(&mut trail, 2, "x", &"y".repeat(5000));
    assert!(trail.lines()[1].len() <= LINE_BYTES);
}

#[test]
fn a_long_call_keeps_its_outcome() {
    let long = "x".repeat(300);
    for (status, detail, want) in [
        (ToolStatus::Ok, "", "-> ok"),
        (ToolStatus::Denied, "gate closed", "-> refused: gate closed"),
        (ToolStatus::Error, "exit 101", "-> error: exit 101"),
    ] {
        let mut trail = Trail::default();
        call(&mut trail, 1, "run_command", &long);
        trail.result(CallId(1), status, detail);
        let line = &trail.lines()[0];
        assert!(line.ends_with(want), "{line}");
        assert!(line.len() <= LINE_BYTES, "{}", line.len());
    }
}

#[test]
fn a_stored_line_with_detail_or_a_cut_look_alike_round_trips() {
    let mut trail = Trail::default();
    call(&mut trail, 1, "cut 7", "x");
    trail.result(CallId(1), ToolStatus::Error, "bad: thing");
    let back = Trail::from_text(&trail.to_text());
    assert_eq!(back.lines(), vec!["cut 7 x -> error: bad: thing"]);
    assert_eq!(back.cut(), 0);
}

#[test]
fn secrets_in_a_call_or_its_result_are_scrubbed() {
    let mut trail = Trail::default();
    call(
        &mut trail,
        1,
        "run_command",
        "mysql -pSecret12 --password hunter2",
    );
    trail.result(CallId(1), ToolStatus::Error, "PASSWORD=abc123");
    let line = &trail.lines()[0];
    assert!(!line.contains("Secret12") && !line.contains("hunter2") && !line.contains("abc123"));
}

#[test]
fn past_the_cap_the_oldest_go_and_the_count_is_kept() {
    let mut trail = Trail::default();
    for n in 0..(MAX_STEPS as u64 + 5) {
        call(&mut trail, n, "tool", &n.to_string());
        trail.result(CallId(n), ToolStatus::Ok, "");
    }
    assert_eq!(trail.lines().len(), MAX_STEPS);
    assert_eq!(trail.cut(), 5);
    assert_eq!(trail.lines()[0], "tool 5 -> ok");
}

#[test]
fn the_text_round_trips_with_its_cut_count() {
    let mut trail = Trail::default();
    for n in 0..(MAX_STEPS as u64 + 2) {
        call(&mut trail, n, "tool", &n.to_string());
    }
    let back = Trail::from_text(&trail.to_text());
    assert_eq!(back.lines(), trail.lines());
    assert_eq!(back.cut(), 2);
}

#[test]
fn shown_lists_the_last_steps_and_counts_the_rest() {
    let mut trail = Trail::default();
    for n in 0..(SHOWN as u64 + 4) {
        call(&mut trail, n, "tool", &n.to_string());
    }
    let shown = trail.shown(SHOWN);
    assert_eq!(shown.len(), SHOWN + 1);
    assert_eq!(shown[0], "step trail: 4 earlier steps not shown");
    assert_eq!(shown[1], "  tool 4 -> no result");
    assert!(Trail::default().shown(SHOWN).is_empty());
}

#[test]
fn a_handoff_reason_is_scrubbed() {
    let mut trail = Trail::default();
    trail.handoff(
        "worker 1",
        "blocked",
        "needs PASSWORD=abc123 and --token xyz",
    );
    let line = &trail.lines()[0];
    assert!(!line.contains("abc123") && !line.contains("xyz"), "{line}");
    assert!(line.starts_with("handoff worker 1 blocked:"));
}

#[test]
fn absorbing_early_steps_puts_them_first_and_keeps_the_cap() {
    let mut early = Trail::default();
    call(&mut early, 1, "early", "a");
    let mut trail = Trail::default();
    call(&mut trail, 2, "late", "b");
    trail.absorb(early);
    assert_eq!(
        trail.lines(),
        vec!["early a -> no result", "late b -> no result"]
    );
    let mut full = Trail::default();
    for n in 0..MAX_STEPS as u64 {
        call(&mut full, n, "t", &n.to_string());
    }
    let mut more = Trail::default();
    call(&mut more, 99, "e", "z");
    full.absorb(more);
    assert_eq!(full.lines().len(), MAX_STEPS);
    assert_eq!(full.cut(), 1);
}

#[test]
fn a_huge_input_is_cut_before_it_is_scrubbed() {
    let huge = "a=1&".repeat(1_000_000);
    let mut trail = Trail::default();
    call(&mut trail, 1, "run_command", &huge);
    trail.result(CallId(1), ToolStatus::Error, &huge);
    trail.handoff("worker 1", "blocked", &huge);
    assert!(trail.lines().iter().all(|l| l.len() <= LINE_BYTES));
}

fn finished(trail: &mut Trail, id: u64, name: &str) {
    call(trail, id, name, "x");
    trail.result(CallId(id), ToolStatus::Ok, "");
}

#[test]
fn carry_puts_old_lines_then_the_separator_then_the_new_ones() {
    let mut earlier = Trail::default();
    finished(&mut earlier, 1, "old");
    let mut trail = Trail::default();
    finished(&mut trail, 2, "new");
    trail.carry(earlier, 7);
    assert_eq!(
        trail.lines(),
        vec!["old x -> ok", "resumed from run 7", "new x -> ok"]
    );
}

#[test]
fn carry_adds_the_cut_counts_and_drops_the_oldest_first() {
    let mut trail = Trail::from_text("cut 3\nnew x -> ok");
    trail.carry(Trail::from_text("cut 5\nold x -> ok"), 7);
    assert_eq!(trail.cut(), 8);
    assert_eq!(
        trail.shown(SHOWN)[0],
        "step trail: 8 earlier steps not shown"
    );

    let mut earlier = Trail::default();
    for n in 0..MAX_STEPS as u64 {
        finished(&mut earlier, n, &format!("old{n}"));
    }
    let mut trail = Trail::default();
    finished(&mut trail, 99, "new");
    trail.carry(earlier, 7);
    let lines = trail.lines();
    assert_eq!(lines.len(), MAX_STEPS);
    assert_eq!(lines[0], "old2 x -> ok");
    assert_eq!(lines[MAX_STEPS - 1], "new x -> ok");
    assert_eq!(trail.cut(), 2);
}

#[test]
fn a_chain_of_resumes_keeps_every_earlier_run_and_separator() {
    let mut a = Trail::default();
    finished(&mut a, 1, "a");
    let mut b = Trail::default();
    finished(&mut b, 2, "b");
    b.carry(Trail::from_text(&a.to_text()), 1);
    let mut c = Trail::default();
    finished(&mut c, 3, "c");
    c.carry(Trail::from_text(&b.to_text()), 2);
    assert_eq!(
        c.lines(),
        vec![
            "a x -> ok",
            "resumed from run 1",
            "b x -> ok",
            "resumed from run 2",
            "c x -> ok"
        ]
    );
}

#[test]
fn an_empty_predecessor_adds_no_separator() {
    let mut trail = Trail::default();
    finished(&mut trail, 1, "new");
    trail.carry(Trail::default(), 7);
    assert_eq!(trail.lines(), vec!["new x -> ok"]);
}
