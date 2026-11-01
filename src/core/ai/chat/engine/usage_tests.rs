use super::*;

fn used(input: u64, output: u64, cost_micros: Option<u64>) -> Usage {
    Usage {
        input,
        output,
        cost_micros,
    }
}

#[test]
fn two_reports_add_their_tokens_and_their_costs() {
    assert_eq!(
        used(1, 2, Some(30)).plus(used(10, 20, Some(300))),
        used(11, 22, Some(330))
    );
}

#[test]
fn a_cost_is_known_only_when_both_reports_have_one() {
    assert_eq!(used(1, 1, Some(5)).plus(used(1, 1, None)).cost_micros, None);
    assert_eq!(used(1, 1, None).plus(used(1, 1, Some(5))).cost_micros, None);
    assert_eq!(used(1, 1, None).plus(used(1, 1, None)).cost_micros, None);
}

#[test]
fn sums_saturate_instead_of_wrapping() {
    let sum = used(u64::MAX, 0, Some(u64::MAX)).plus(used(5, 0, Some(5)));
    assert_eq!(sum, used(u64::MAX, 0, Some(u64::MAX)));
}

#[test]
fn dollars_become_millionths_and_nonsense_is_not_a_cost() {
    assert_eq!(Usage::micros(0.004936), Some(4936));
    assert_eq!(Usage::micros(0.0), Some(0));
    for bad in [-0.5, f64::NAN, f64::INFINITY] {
        assert_eq!(Usage::micros(bad), None, "{bad}");
    }
}

#[test]
fn big_counts_move_to_millions_instead_of_reading_1000_0k() {
    let label = |n| used(n, 0, None).label();
    assert_eq!(label(999), "999 in 0 out");
    assert_eq!(label(999_949), "999.9k in 0 out");
    assert_eq!(label(999_950), "1.0M in 0 out");
    assert_eq!(label(12_345_678), "12.3M in 0 out");
}

#[test]
fn a_cost_too_small_to_show_is_not_printed_as_zero() {
    let label = |m| used(1, 1, Some(m)).label();
    assert_eq!(label(0), "1 in 1 out $0.0000");
    assert_eq!(label(1), "1 in 1 out <$0.0001");
    assert_eq!(label(49), "1 in 1 out <$0.0001");
    assert_eq!(label(50), "1 in 1 out $0.0001");
    assert_eq!(label(4936), "1 in 1 out $0.0049");
}

fn turn(
    input: Option<u64>,
    output: Option<u64>,
    cache: (Option<u64>, Option<u64>),
    cost: Option<u64>,
) -> TurnUsage {
    TurnUsage {
        input,
        output,
        cache_read: cache.0,
        cache_write: cache.1,
        cost_micros: cost,
        ..TurnUsage::default()
    }
}

#[test]
fn a_turn_becomes_a_row_figure_only_with_both_counts() {
    const NONE: (Option<u64>, Option<u64>) = (None, None);
    let cases = [
        // (turn, row figure)
        (turn(Some(7), None, NONE, None), None),
        (turn(None, Some(3), NONE, None), None),
        (turn(None, Some(3), (Some(9), None), None), None),
        (turn(None, None, NONE, Some(5)), None),
        (turn(Some(2), Some(4), NONE, None), Some(used(2, 4, None))),
        // The cache buckets join the prompt count.
        (
            turn(Some(2), Some(4), (Some(30), Some(600)), None),
            Some(used(632, 4, None)),
        ),
        (
            turn(Some(2), Some(4), (None, Some(600)), None),
            Some(used(602, 4, None)),
        ),
        // The cost passes through as it was reported.
        (
            turn(Some(2), Some(4), NONE, Some(4936)),
            Some(used(2, 4, Some(4936))),
        ),
        // Sums saturate instead of wrapping.
        (
            turn(Some(u64::MAX), Some(1), (Some(5), Some(5)), None),
            Some(used(u64::MAX, 1, None)),
        ),
    ];
    for (given, want) in cases {
        assert_eq!(Usage::from_turn(&given), want, "{given:?}");
    }
}
