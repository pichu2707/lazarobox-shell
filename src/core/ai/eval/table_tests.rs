//! The pass rule, the acceptance rule and the score table.

use std::time::Duration;

use serde_json::json;

use super::{
    Category,
    scenarios::{READ_ONLY, WRITE_AND_TEST},
    score,
    table::{Header, Row, Run, Verdict, accept, date, hardware, median, passes, render},
};
use crate::core::ai::chat::adapters::ToolExchange;

fn ex(name: &str, arguments: serde_json::Value) -> ToolExchange {
    ToolExchange {
        name: name.to_owned(),
        id: None,
        index: None,
        arguments,
        result: "ok".to_owned(),
    }
}

fn good_list() -> [ToolExchange; 5] {
    [
        ex("write_file", json!({"path": "fib.py"})),
        ex("write_file", json!({"path": "test_fib.py"})),
        ex("propose_batch", json!({})),
        ex("run_command", json!({"program": "pytest"})),
        ex("git_commit", json!({"message": "add fibonacci"})),
    ]
}

fn good_run(secs: u64) -> Run {
    Run::new(
        score(&WRITE_AND_TEST, &good_list(), "Done."),
        Duration::from_secs(secs),
    )
}

fn bad_run(secs: u64) -> Run {
    let list = [ex("write_file", json!({"path": "fib.py"}))];
    Run::new(
        score(&WRITE_AND_TEST, &list, "Done."),
        Duration::from_secs(secs),
    )
}

fn row(model: &str, passed: usize, runs: usize) -> Row {
    let list = (0..runs)
        .map(|i| {
            if i < passed {
                good_run(10)
            } else {
                bad_run(10)
            }
        })
        .collect();
    Row::new(model, &WRITE_AND_TEST, list)
}

fn header() -> Header {
    Header {
        date: "2026-11-15".to_owned(),
        ollama: "0.30.10".to_owned(),
        hardware: "gpu-name, 16 GiB".to_owned(),
        sha: "abc1234".to_owned(),
        runs: 5,
    }
}

#[test]
fn four_of_five_passes_and_three_fails() {
    assert!(passes(4, 5));
    assert!(passes(5, 5));
    assert!(!passes(3, 5));
    assert!(!passes(0, 0));
}

#[test]
fn a_run_over_ten_minutes_fails_whatever_it_produced() {
    assert!(good_run(600).passed());
    assert!(!good_run(601).passed());
    let late = Run::new(good_run(1).result, Duration::from_millis(600_001));
    assert!(!late.passed());
}

#[test]
fn an_unexpected_approval_fails_the_run() {
    let mut run = good_run(10);
    run.unexpected = true;
    assert!(!run.passed());
}

#[test]
fn acceptance_needs_a_rise_and_no_fall_below_passing() {
    let before = [row("qwen2.5:7b", 2, 5), row("qwen3:14b", 5, 5)];
    let raised = [row("qwen2.5:7b", 3, 5), row("qwen3:14b", 5, 5)];
    assert_eq!(accept(&before, &raised), Verdict::Accepted);
    let fell = [row("qwen2.5:7b", 5, 5), row("qwen3:14b", 2, 5)];
    assert_eq!(
        accept(&before, &fell),
        Verdict::Refused(vec!["qwen3:14b write-and-test".to_owned()])
    );
    assert_eq!(accept(&before, &before), Verdict::Refused(Vec::new()));
}

#[test]
fn five_to_four_is_noise_when_another_combination_rose() {
    let before = [row("qwen2.5:7b", 2, 5), row("qwen3:14b", 5, 5)];
    let after = [row("qwen2.5:7b", 3, 5), row("qwen3:14b", 4, 5)];
    assert_eq!(accept(&before, &after), Verdict::Accepted);
}

#[test]
fn a_borderline_combination_must_be_repeated() {
    let before = [row("qwen3:14b", 3, 5)];
    let after = [row("qwen3:14b", 4, 5)];
    assert_eq!(
        accept(&before, &after),
        Verdict::Repeat(vec!["qwen3:14b write-and-test".to_owned()])
    );
    let fell_to_four = [row("qwen3:14b", 5, 5)];
    assert_eq!(
        accept(&fell_to_four, &after),
        Verdict::Repeat(vec!["qwen3:14b write-and-test".to_owned()])
    );
}

#[test]
fn informative_models_never_block() {
    let before = [row("qwen3:14b-nothink", 5, 5), row("qwen3:14b", 2, 5)];
    let after = [row("qwen3:14b-nothink", 0, 5), row("qwen3:14b", 3, 5)];
    assert_eq!(accept(&before, &after), Verdict::Accepted);
}

#[test]
fn the_median_of_five_is_the_middle_one() {
    assert_eq!(median(&[50, 10, 30, 20, 40]), 30);
    assert_eq!(median(&[10, 20]), 15);
    assert_eq!(median(&[]), 0);
}

#[test]
fn the_date_is_utc_year_month_day() {
    assert_eq!(date(0), "1970-01-01");
    assert_eq!(date(1_709_251_199), "2024-02-29");
    assert_eq!(date(1_794_700_800), "2026-11-15");
}

#[test]
fn hardware_falls_back_from_env_to_gpu_to_cpu() {
    assert_eq!(hardware(Some("my box"), Some(("gpu", "1 GiB"))), "my box");
    assert_eq!(hardware(None, Some(("gpu", "1 GiB"))), "gpu, 1 GiB");
    assert_eq!(hardware(None, None), "cpu");
    assert_eq!(hardware(Some(" "), None), "cpu");
}

#[test]
fn the_table_has_a_header_and_one_row_per_model_and_task() {
    let table = render(&header(), &[row("qwen3:14b", 4, 5)]);
    for line in [
        "lazarobox local-model eval, format 1",
        "date: 2026-11-15",
        "ollama: 0.30.10",
        "hardware: gpu-name, 16 GiB",
        "runs: 5",
        "lazarobox: abc1234",
    ] {
        assert!(table.lines().any(|l| l == line), "{line}\n{table}");
    }
    let data = table.lines().find(|l| l.starts_with("qwen3:14b ")).unwrap();
    assert!(data.contains("write-and-test"), "{data}");
    assert!(data.contains("4/5"), "{data}");
    assert!(data.contains("missing:pytest x1"), "{data}");
}

#[test]
fn the_row_lists_the_times_and_the_median() {
    let times = [10, 20, 30, 40, 50].map(good_run).into_iter().collect();
    let table = render(&header(), &[Row::new("m", &WRITE_AND_TEST, times)]);
    assert!(table.contains("10 20 30 40 50"), "{table}");
    assert!(table.contains("median 30"), "{table}");
}

#[test]
fn failure_kinds_are_counted() {
    let mut slow = good_run(601);
    slow.unexpected = true;
    let table = render(
        &header(),
        &[Row::new("m", &WRITE_AND_TEST, vec![slow, bad_run(5)])],
    );
    assert!(table.contains("time x1"), "{table}");
    assert!(table.contains("unexpected x1"), "{table}");
    assert!(table.contains("missing:pytest x1"), "{table}");
    assert!(table.contains("missing:commit x1"), "{table}");
}

#[test]
fn the_summary_counts_combinations_per_model_and_category() {
    let read = Row::new("m", &READ_ONLY, vec![bad_run(1)]);
    let table = render(&header(), &[row("m", 5, 5), read]);
    assert!(table.contains("m write-and-test 1/1"), "{table}");
    assert!(table.contains("m read-only 0/1"), "{table}");
    assert_eq!(Category::ReadOnly.key(), "read-only");
}

#[test]
fn the_table_is_stable_ascii_text() {
    let rows = [row("b", 5, 5), row("a", 3, 5)];
    let first = render(&header(), &rows);
    assert_eq!(first, render(&header(), &rows));
    assert!(first.is_ascii());
    assert!(!first.contains('\r') && !first.contains('\x1b'));
    for word in first.split_whitespace().filter(|w| w.contains('/')) {
        assert!(
            word.chars().all(|c| c.is_ascii_digit() || c == '/'),
            "path-like: {word}"
        );
    }
    let a = first.find("\na ").unwrap();
    let b = first.find("\nb ").unwrap();
    assert!(a < b, "rows are sorted by model:\n{first}");
}

#[test]
fn every_failed_run_prints_its_reason() {
    let answer = "Please provide a commit message";
    let asked = Run::new(
        score(&WRITE_AND_TEST, &good_list(), answer),
        Duration::from_secs(9),
    );
    let mut over = good_run(601);
    over.unexpected = true;
    let runs = vec![good_run(5), asked, bad_run(7), over];
    let table = render(&header(), &[Row::new("m", &WRITE_AND_TEST, runs)]);
    let lines: Vec<&str> = table.lines().filter(|l| l.contains(" run ")).collect();
    assert_eq!(lines.len(), 3, "{table}");
    assert!(lines[0].ends_with("run 2: asked back"), "{table}");
    assert!(lines[1].contains("run 3: missing:write-test"), "{table}");
    assert!(lines[1].contains("missing:pytest"), "{table}");
    assert!(
        lines[2].ends_with("run 4: unexpected approval, over time"),
        "{table}"
    );
    assert!(!table.contains("run 1:"), "{table}");
}

#[test]
fn a_reason_names_the_pair_and_the_tool() {
    let list = [
        ex("write_file", json!({"path": "fib.py"})),
        ex("write_file", json!({"path": "test_fib.py"})),
        ex("git_commit", json!({"message": "x"})),
        ex("propose_batch", json!({})),
        ex("run_command", json!({"program": "pytest"})),
    ];
    let run = Run::new(score(&WRITE_AND_TEST, &list, "Done."), Duration::ZERO);
    let table = render(&header(), &[Row::new("m", &WRITE_AND_TEST, vec![run])]);
    assert!(table.contains("out of order:batch>commit"), "{table}");
    assert!(table.contains("ignored literal"), "{table}");
    let read = [ex("write_file", json!({"path": "x"}))];
    let run = Run::new(score(&READ_ONLY, &read, "Done."), Duration::ZERO);
    let table = render(&header(), &[Row::new("m", &READ_ONLY, vec![run])]);
    assert!(table.contains("forbidden:write_file"), "{table}");
}
