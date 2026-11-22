//! Number text of the usage views. Pure: no colour, no clock.

/// Units of [`compact`], smallest first.
const UNITS: [(u128, char); 4] = [
    (1_000, 'k'),
    (1_000_000, 'M'),
    (1_000_000_000, 'B'),
    (1_000_000_000_000, 'T'),
];

/// A count in short form: `999`, `1.0k`, `123.5k`, `11.6B`. One decimal,
/// rounded half up, always a `.`; a figure that would read `1000.0` moves to
/// the next unit.
pub fn compact(n: u64) -> String {
    if n < 1_000 {
        return n.to_string();
    }
    let mut at = 0;
    loop {
        let (unit, _) = UNITS[at];
        let tenths = (u128::from(n) * 10 + unit / 2) / unit;
        if tenths < 10_000 || at == UNITS.len() - 1 {
            return format!("{}.{}{}", tenths / 10, tenths % 10, UNITS[at].1);
        }
        at += 1;
    }
}

/// [`compact`] with its unit: `1.2M tok`.
pub fn tok(n: u64) -> String {
    format!("{} tok", compact(n))
}

/// A count with a comma every three digits.
pub fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A span: `38s`, `12m`, `3h 07m`, and whole hours from 100 h on.
pub fn duration(ms: u64) -> String {
    let secs = ms / 1_000;
    match secs {
        0..60 => format!("{secs}s"),
        60..3_600 => format!("{}m", secs / 60),
        3_600..360_000 => format!("{}h {:02}m", secs / 3_600, secs % 3_600 / 60),
        _ => format!("{}h", secs / 3_600),
    }
}

/// `n` of `d` as a whole percent, rounded half up. `None` when `d` is 0.
pub fn pct(n: u64, d: u64) -> Option<u64> {
    (d != 0).then(|| ((u128::from(n) * 100 + u128::from(d) / 2) / u128::from(d)) as u64)
}

#[cfg(test)]
mod tests;
