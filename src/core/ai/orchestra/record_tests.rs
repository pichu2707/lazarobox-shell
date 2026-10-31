use super::*;

fn worker(id: u32, status: &str) -> WorkerRecord {
    WorkerRecord {
        id,
        kind: "explore".into(),
        engine: "claude".into(),
        model: None,
        status: status.into(),
    }
}

fn record(goal: &str, workers: Vec<WorkerRecord>) -> RunRecord {
    RunRecord {
        id: 42,
        goal: goal.into(),
        phase: RunPhase::Interrupted,
        workers,
    }
}

#[test]
fn a_phase_round_trips_through_its_id_and_an_unknown_one_is_none() {
    for phase in [RunPhase::Live, RunPhase::Ended, RunPhase::Interrupted] {
        assert_eq!(RunPhase::from_id(phase.id()), Some(phase));
    }
    assert_eq!(RunPhase::from_id("paused"), None);
}

#[test]
fn only_a_queued_or_running_worker_is_unfinished() {
    let states = [
        "queued",
        "running",
        "done",
        "failed",
        "cancelled",
        "timed_out",
        "refused",
    ];
    let open: Vec<_> = states
        .iter()
        .filter(|s| worker(1, s).unfinished())
        .collect();
    assert_eq!(open, [&"queued", &"running"]);
}

#[test]
fn the_stored_goal_is_sanitized_and_cut_on_a_character() {
    assert_eq!(
        RunRecord::stored_goal("a\u{1b}[31mb"),
        clean("a\u{1b}[31mb")
    );
    assert!(!RunRecord::stored_goal("a\u{1b}[31mb").contains('\u{1b}'));
    let long = "é".repeat(GOAL_MAX);
    let stored = RunRecord::stored_goal(&long);
    assert!(stored.len() <= GOAL_MAX);
    assert!(stored.chars().all(|c| c == 'é'));
    assert!(stored.len() >= GOAL_MAX - 1);
}

#[test]
fn the_interruption_line_counts_unfinished_workers_and_says_nothing_runs() {
    let line = record(
        "fix the parser",
        vec![worker(1, "done"), worker(2, "running")],
    )
    .interrupted_line();
    assert_eq!(
        line,
        "run 42 was interrupted by a restart: 1 worker unfinished, nothing runs \u{b7} fix the parser"
    );
    let two = record("g", vec![worker(1, "queued"), worker(2, "running")]).interrupted_line();
    assert!(two.contains("2 workers unfinished"));
}

#[test]
fn the_interruption_line_clips_and_cleans_the_goal() {
    let line = record(&format!("\n\u{1b}[2Jtitle\n{}", "x".repeat(200)), vec![]).interrupted_line();
    assert!(!line.contains('\u{1b}') && !line.contains('\n'));
    assert!(line.ends_with("title"));
    assert!(record("", vec![]).interrupted_line().ends_with("(no goal)"));
    let wide = record(&"y".repeat(200), vec![]).interrupted_line();
    let shown = wide.split(" \u{b7} ").nth(1).unwrap();
    assert_eq!(shown.chars().count(), 60);
}
