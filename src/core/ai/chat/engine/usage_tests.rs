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
