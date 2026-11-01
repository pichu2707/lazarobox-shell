//! The limit rows of the usage view (ai-cost, thresholds): a number typed as
//! text, where `0` or an empty field means `off`. Dollars are kept as
//! millionths so a stored value is shown and kept exactly.

use super::*;

/// Most digits a limit takes.
const LIMIT_DIGITS: usize = 12;
/// Most decimals of a dollar limit.
const LIMIT_DECIMALS: usize = 2;
/// Millionths in a dollar.
const MICROS: u64 = 1_000_000;
/// Decimals a stored dollar limit can hold.
const MICRO_DECIMALS: usize = 6;

/// A limit as shown and as written: `off`, whole tokens, or dollars with the
/// decimals it has.
pub(super) fn limit_text(limit: Option<u64>, usd: bool) -> String {
    match limit {
        None => "off".to_owned(),
        Some(n) if !usd => n.to_string(),
        Some(n) if n % MICROS == 0 => (n / MICROS).to_string(),
        Some(n) => format!("{}.{:06}", n / MICROS, n % MICROS)
            .trim_end_matches('0')
            .to_owned(),
    }
}

/// The limit a field holds: `Ok(None)` for empty or 0 (off); `Err` is the
/// notice for a field that is no number (a lone `.`), has more than 6
/// decimals or does not fit. A stored value can carry up to 6 decimals, so
/// the field reads them all.
pub(super) fn parse_limit(field: &str, usd: bool) -> Result<Option<u64>, &'static str> {
    let (whole, frac) = field.split_once('.').unwrap_or((field, ""));
    if (whole.is_empty() && frac.is_empty() && !field.is_empty())
        || !whole
            .chars()
            .chain(frac.chars())
            .all(|c| c.is_ascii_digit())
    {
        return Err(NOTICE_LIMIT_EMPTY);
    }
    let digits = |t: &str| {
        if t.is_empty() {
            Ok(0)
        } else {
            t.parse::<u64>().map_err(|_| NOTICE_LIMIT_BIG)
        }
    };
    let n = if usd {
        if frac.len() > MICRO_DECIMALS {
            return Err(NOTICE_LIMIT_BIG);
        }
        let micros = digits(&format!("{frac:0<6}"))?;
        digits(whole)?
            .checked_mul(MICROS)
            .and_then(|w| w.checked_add(micros))
            .ok_or(NOTICE_LIMIT_BIG)?
    } else {
        digits(whole)?
    };
    Ok((n > 0).then_some(n))
}

/// Adds `c` to the field of a limit, or says why it cannot.
pub(super) fn push_limit_char(field: &mut String, c: char, usd: bool) -> Option<&'static str> {
    let digits = field.chars().filter(char::is_ascii_digit).count();
    match c {
        '0'..='9' if digits >= LIMIT_DIGITS => Some(NOTICE_LIMIT_LONG),
        '0'..='9'
            if usd
                && field
                    .split_once('.')
                    .is_some_and(|(_, d)| d.len() >= LIMIT_DECIMALS) =>
        {
            Some(NOTICE_LIMIT_DECIMALS)
        }
        '.' if usd && field.contains('.') => Some(NOTICE_LIMIT_POINT),
        '0'..='9' | '.' if c != '.' || usd => {
            field.push(c);
            None
        }
        _ => Some(NOTICE_LIMIT_KEY),
    }
}

pub(super) const HINT_LIMIT: &str = "digits · Enter · Esc";
pub(super) const NOTICE_LIMIT_KEY: &str = "Digits only";
pub(super) const NOTICE_LIMIT_OTHER: &str = "Digits, Enter or Esc only";
pub(super) const NOTICE_LIMIT_BIG: &str = "That number is too large";
pub(super) const NOTICE_LIMIT_ENTER: &str = "Enter edits this limit";
pub(super) const NOTICE_LIMIT_EMPTY: &str = "Type a number, Esc cancels";
pub(super) const NOTICE_LIMIT_LONG: &str = "At most 12 digits";
pub(super) const NOTICE_LIMIT_DECIMALS: &str = "At most 2 decimals";
pub(super) const NOTICE_LIMIT_POINT: &str = "One decimal point only";

/// The text of a limit field that starts from the shown value.
fn field_of(shown: &str) -> String {
    if shown == "off" {
        String::new()
    } else {
        shown.to_owned()
    }
}

impl MenuState {
    /// The limit row at `cursor`: whether it counts dollars.
    pub(super) fn limit_at(&self, cursor: usize) -> Option<bool> {
        match USAGE_SECTION.items.get(cursor)?.kind {
            ItemKind::Draft(AiChoice {
                value: DraftValue::Limit { usd, .. },
                ..
            }) => Some(usd),
            _ => None,
        }
    }

    /// Opens the field of the limit at `cursor` with its current value.
    pub(super) fn start_limit(&mut self, cursor: usize) {
        if let (
            Some(ai),
            Some(Item {
                kind: ItemKind::Draft(choice),
                ..
            }),
        ) = (&self.ai, USAGE_SECTION.items.get(cursor))
        {
            self.limit_edit = Some(field_of(&choice.shown(ai.draft())));
        }
    }

    /// The text the field of the limit at `cursor` starts from.
    fn limit_start(&self, cursor: usize) -> Option<String> {
        let (ai, item) = (self.ai.as_ref()?, USAGE_SECTION.items.get(cursor)?);
        match &item.kind {
            ItemKind::Draft(choice) => Some(field_of(&choice.shown(ai.draft()))),
            _ => None,
        }
    }

    /// A paste into the field: all of it is typed, or none of it and the
    /// reason why (outside an edit it says to leave the view first).
    pub(super) fn paste_limit(&mut self, text: &str) {
        let Some(cursor) = self.usage else { return };
        let usd = self.limit_at(cursor).unwrap_or(false);
        let Some(field) = self.limit_edit.as_ref() else {
            self.notice = Some(NOTICE_USAGE_KEY.to_owned());
            return;
        };
        let mut next = field.clone();
        for c in text.trim().chars() {
            if let Some(notice) = push_limit_char(&mut next, c, usd) {
                self.notice = Some(notice.to_owned());
                return;
            }
        }
        self.limit_edit = Some(next);
    }

    /// Keys while a limit is typed: digits (and one `.` for dollars), Backspace,
    /// Enter to keep it and Esc to drop the edit. Any other key says so.
    pub(super) fn on_limit_key(&mut self, key: &KeyEvent, repeat: bool) {
        let Some(cursor) = self.usage else { return };
        let usd = self.limit_at(cursor).unwrap_or(false);
        let plain = key.modifiers.difference(KeyModifiers::SHIFT).is_empty();
        let Some(field) = self.limit_edit.as_mut() else {
            return;
        };
        let refused = match key.code {
            KeyCode::Esc | KeyCode::Enter if repeat => return,
            KeyCode::Esc if plain => {
                self.limit_edit = None;
                self.notice = None;
                return;
            }
            KeyCode::Enter if plain => {
                self.notice = None;
                // An unchanged field keeps the draft as it is: a stored value
                // the field cannot retype is never rewritten.
                if self.limit_start(cursor).as_ref() == self.limit_edit.as_ref() {
                    self.limit_edit = None;
                    return;
                }
                let parsed = parse_limit(self.limit_edit.as_deref().unwrap_or(""), usd);
                match parsed {
                    Ok(limit) => self.commit_limit(cursor, limit),
                    Err(notice) => self.notice = Some(notice.to_owned()),
                }
                return;
            }
            KeyCode::Backspace if plain => {
                field.pop();
                self.notice = None;
                return;
            }
            KeyCode::Char(c) if plain => push_limit_char(field, c, usd),
            _ => Some(NOTICE_LIMIT_OTHER),
        };
        self.notice = refused.map(str::to_owned);
    }

    fn commit_limit(&mut self, cursor: usize, limit: Option<u64>) {
        self.limit_edit = None;
        if let (
            Some(ai),
            Some(Item {
                kind:
                    ItemKind::Draft(AiChoice {
                        value: DraftValue::Limit { set, .. },
                        ..
                    }),
                ..
            }),
        ) = (&mut self.ai, USAGE_SECTION.items.get(cursor))
        {
            ai.set_draft(*set, limit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dollars_parse_straight_to_millionths_with_up_to_six_decimals() {
        for (text, micros) in [
            ("2.5", 2_500_000),
            ("2", 2_000_000),
            ("0.125", 125_000),
            ("0.005", 5_000),
            ("0.000001", 1),
            (".5", 500_000),
        ] {
            assert_eq!(parse_limit(text, true), Ok(Some(micros)), "{text}");
        }
    }

    #[test]
    fn what_is_shown_parses_back_to_the_same_value() {
        for micros in [1, 5_000, 125_000, 2_500_000, 1_000_000, u64::MAX] {
            let shown = limit_text(Some(micros), true);
            assert_eq!(parse_limit(&shown, true), Ok(Some(micros)), "{shown}");
        }
        assert_eq!(limit_text(Some(125_000), true), "0.125");
        assert_eq!(limit_text(Some(u64::MAX), true), "18446744073709.551615");
        assert_eq!(
            parse_limit(&u64::MAX.to_string(), false),
            Ok(Some(u64::MAX))
        );
    }

    #[test]
    fn a_value_that_does_not_fit_or_is_no_number_is_refused_not_wrapped() {
        for text in ["18446744073710", "99999999999999999999", "1.1234567", "."] {
            assert!(parse_limit(text, true).is_err(), "{text}");
        }
        assert!(parse_limit("18446744073709551616", false).is_err());
        assert_eq!(parse_limit("", true), Ok(None));
        assert_eq!(parse_limit("0.0", true), Ok(None));
    }
}
