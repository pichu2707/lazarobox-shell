use serde_json::json;

use super::*;

fn usage(input: Option<u64>, output: Option<u64>) -> TurnUsage {
    TurnUsage {
        input,
        output,
        ..TurnUsage::default()
    }
}

#[test]
fn merge_sums_every_field_present_on_both_sides() {
    let mut sum = TurnUsage {
        input: Some(100),
        output: Some(20),
        cache_read: Some(5),
        cache_write: Some(1),
        reasoning: Some(3),
        cost_micros: Some(1_000),
        duration_ms: Some(10),
        ..TurnUsage::default()
    };
    let next = TurnUsage {
        input: Some(150),
        output: Some(30),
        cache_read: Some(7),
        cache_write: Some(2),
        reasoning: Some(4),
        cost_micros: Some(2_500),
        duration_ms: Some(15),
        ..TurnUsage::default()
    };
    sum.merge(&next);
    assert_eq!(sum.input, Some(250));
    assert_eq!(sum.output, Some(50));
    assert_eq!(sum.cache_read, Some(12));
    assert_eq!(sum.cache_write, Some(3));
    assert_eq!(sum.reasoning, Some(7));
    assert_eq!(sum.cost_micros, Some(3_500));
    assert_eq!(sum.duration_ms, Some(25));
    assert!(!sum.partial);
}

#[test]
fn merge_keeps_a_field_only_one_side_reported_and_is_not_partial() {
    let mut sum = usage(Some(100), Some(20));
    let next = TurnUsage {
        cache_read: Some(9),
        ..usage(Some(1), Some(1))
    };
    sum.merge(&next);
    assert_eq!(sum.cache_read, Some(9));
    assert_eq!(sum.input, Some(101));
    assert!(!sum.partial, "a field gap is no data, not a partial turn");
}

#[test]
fn merge_of_two_missing_fields_stays_none_not_zero() {
    let mut sum = usage(None, Some(4));
    sum.merge(&usage(None, Some(6)));
    assert_eq!(sum.input, None);
    assert_eq!(sum.output, Some(10));
    assert!(!sum.partial);
}

#[test]
fn merge_keeps_zero_as_a_real_value() {
    let mut sum = TurnUsage {
        cache_read: Some(0),
        ..usage(Some(1), Some(1))
    };
    sum.merge(&TurnUsage {
        cache_read: Some(0),
        ..usage(Some(1), Some(1))
    });
    assert_eq!(sum.cache_read, Some(0));
}

#[test]
fn merge_saturates_instead_of_wrapping() {
    let mut sum = usage(Some(u64::MAX), None);
    sum.merge(&usage(Some(5), None));
    assert_eq!(sum.input, Some(u64::MAX));
}

#[test]
fn merge_keeps_the_source_and_ors_partial() {
    let mut sum = TurnUsage {
        model: Some("a".into()),
        cost_source: Some(CostSource::EngineList),
        ..usage(Some(1), Some(1))
    };
    let next = TurnUsage {
        model: Some("b".into()),
        partial: true,
        ..usage(Some(1), Some(1))
    };
    sum.merge(&next);
    assert_eq!(sum.cost_source, Some(CostSource::EngineList));
    assert!(sum.partial);
}

#[test]
fn merge_flags_a_second_model_instead_of_picking_one() {
    let named = |m: &str| TurnUsage {
        model: Some(m.into()),
        ..usage(Some(1), Some(1))
    };
    let mut same = named("a");
    same.merge(&named("a"));
    assert!(!same.multi_model);
    let mut mixed = named("a");
    mixed.merge(&named("b"));
    assert!(mixed.multi_model);
    // Once mixed, a further merge keeps the flag.
    mixed.merge(&named("a"));
    assert!(mixed.multi_model);
    assert_eq!(mixed.model, None, "a mixed turn names no single model");
}

#[test]
fn micros_rounds_to_the_nearest_millionth() {
    assert_eq!(micros(&json!(0.004936)), Some(4_936));
    assert_eq!(micros(&json!(0.0000004)), Some(0));
    assert_eq!(micros(&json!(0.0000006)), Some(1));
    assert_eq!(micros(&json!(12.3456)), Some(12_345_600));
}

#[test]
fn micros_refuses_negative_and_non_numbers() {
    assert_eq!(micros(&json!(-0.5)), None);
    assert_eq!(micros(&json!("0.5")), None);
    assert_eq!(micros(&json!(null)), None);
    assert_eq!(micros(&json!(1e300)), None);
}

#[test]
fn count_takes_whole_non_negative_numbers_only() {
    assert_eq!(count(&json!(7)), Some(7));
    assert_eq!(count(&json!(0)), Some(0));
    assert_eq!(count(&json!(-5)), None);
    assert_eq!(count(&json!(1.5)), None);
    assert_eq!(count(&json!("many")), None);
    assert_eq!(count(&json!(null)), None);
    assert_eq!(count(&json!(18446744073709551616.0_f64)), None);
}

#[test]
fn model_id_is_cleaned_and_capped() {
    assert_eq!(
        model_id("claude-opus-5-5").as_deref(),
        Some("claude-opus-5-5")
    );
    assert_eq!(model_id("  ").as_deref(), None);
    assert_eq!(
        model_id(&"x".repeat(200)).map(|m| m.chars().count()),
        Some(80)
    );
    assert_eq!(model_id("a\u{1b}[31mb").as_deref(), Some("ab"));
}

#[test]
fn fold_of_one_request_with_usage_is_that_usage() {
    let mut fold = UsageFold::default();
    fold.add(Some(usage(Some(10), Some(2))));
    assert_eq!(fold.finish(false), Some(usage(Some(10), Some(2))));
}

#[test]
fn fold_of_one_request_without_usage_is_no_record() {
    let mut fold = UsageFold::default();
    fold.add(None);
    assert_eq!(fold.finish(false), None);
}

#[test]
fn fold_of_two_requests_is_summed_and_not_partial() {
    let mut fold = UsageFold::default();
    fold.add(Some(usage(Some(100), Some(20))));
    fold.add(Some(usage(Some(150), Some(30))));
    let total = fold.finish(false).expect("a record");
    assert_eq!(
        (total.input, total.output, total.partial),
        (Some(250), Some(50), false)
    );
}

#[test]
fn fold_marks_partial_when_a_request_reported_nothing() {
    let mut fold = UsageFold::default();
    fold.add(Some(usage(Some(100), Some(20))));
    fold.add(None);
    let total = fold.finish(false).expect("a record");
    assert_eq!(
        (total.input, total.output, total.partial),
        (Some(100), Some(20), true)
    );
}

#[test]
fn fold_marks_partial_when_the_turn_was_cut() {
    let mut fold = UsageFold::default();
    fold.add(Some(usage(Some(100), Some(20))));
    assert!(fold.finish(true).expect("a record").partial);
}

#[test]
fn fold_with_no_request_is_no_record_even_when_cut() {
    assert_eq!(UsageFold::default().finish(true), None);
}

#[test]
fn reported_drops_a_record_with_nothing_in_it() {
    assert_eq!(TurnUsage::default().reported(), None);
    let model_only = TurnUsage {
        model: Some("m".into()),
        ..TurnUsage::default()
    };
    assert_eq!(model_only.reported(), None);
}

#[test]
fn reported_keeps_a_record_with_any_count_even_zero() {
    let zero = TurnUsage {
        cache_read: Some(0),
        ..TurnUsage::default()
    };
    assert_eq!(zero.clone().reported(), Some(zero));
    let cost = TurnUsage {
        cost_micros: Some(1),
        ..TurnUsage::default()
    };
    assert_eq!(cost.clone().reported(), Some(cost));
}

#[test]
fn idle_is_true_only_when_every_count_and_cost_is_zero_or_missing() {
    assert!(TurnUsage::default().idle());
    let zeros = TurnUsage {
        input: Some(0),
        output: Some(0),
        cost_micros: Some(0),
        ..TurnUsage::default()
    };
    assert!(zeros.idle());
    assert!(!usage(Some(0), Some(1)).idle());
    assert!(
        !TurnUsage {
            cost_micros: Some(1),
            ..zeros
        }
        .idle()
    );
}

#[test]
fn net_input_takes_the_cache_out_and_saturates() {
    assert_eq!(net_input(Some(1000), Some(600)), Some(400));
    assert_eq!(net_input(Some(5), Some(9)), Some(0));
    assert_eq!(net_input(Some(12), None), Some(12));
    assert_eq!(net_input(None, Some(3)), None);
}

#[test]
fn a_model_path_keeps_only_the_file_stem() {
    assert_eq!(
        model_id("/home/u/models/qwen-7b-q4.gguf").as_deref(),
        Some("qwen-7b-q4")
    );
    assert_eq!(model_id("C:\\m\\x.gguf").as_deref(), Some("x"));
    // An org/name id is not a path.
    assert_eq!(model_id("org/name").as_deref(), Some("org/name"));
}
