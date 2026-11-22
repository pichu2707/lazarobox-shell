use super::*;

#[test]
fn compact_pinned_values() {
    let cases: [(u64, &str); 9] = [
        (0, "0"),
        (999, "999"),
        (1_000, "1.0k"),
        (1_049, "1.0k"),
        (1_050, "1.1k"),
        (999_949, "999.9k"),
        (999_950, "1.0M"),
        (123_456, "123.5k"),
        (11_550_000_000, "11.6B"),
    ];
    for (n, want) in cases {
        assert_eq!(compact(n), want, "{n}");
    }
    assert_eq!(compact(u64::MAX), "18446744.1T");
}

#[test]
fn tok_appends_the_unit() {
    assert_eq!(tok(950), "950 tok");
    assert_eq!(tok(1_234_567), "1.2M tok");
}

#[test]
fn grouped_puts_a_comma_every_three_digits() {
    assert_eq!(grouped(0), "0");
    assert_eq!(grouped(999), "999");
    assert_eq!(grouped(1_110), "1,110");
    assert_eq!(grouped(1_000_000), "1,000,000");
    assert_eq!(grouped(u64::MAX), "18,446,744,073,709,551,615");
}

#[test]
fn duration_boundaries() {
    assert_eq!(duration(0), "0s");
    assert_eq!(duration(38_000), "38s");
    assert_eq!(duration(59_999), "59s");
    assert_eq!(duration(60_000), "1m");
    assert_eq!(duration(12 * 60_000), "12m");
    assert_eq!(duration(3_600_000), "1h 00m");
    assert_eq!(duration((3 * 60 + 7) * 60_000), "3h 07m");
    assert_eq!(duration(100 * 3_600_000), "100h");
    assert_eq!(duration(128 * 3_600_000 + 59 * 60_000), "128h");
}

#[test]
fn pct_rounds_half_up_and_guards_zero() {
    assert_eq!(pct(1, 0), None);
    assert_eq!(pct(0, 5), Some(0));
    assert_eq!(pct(1, 2), Some(50));
    assert_eq!(pct(1, 8), Some(13));
    assert_eq!(pct(1, 3), Some(33));
    assert_eq!(pct(2, 3), Some(67));
    assert_eq!(pct(u64::MAX, u64::MAX), Some(100));
}
