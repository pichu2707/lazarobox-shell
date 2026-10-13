//! The closed list of note categories.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemKind {
    Decision,
    Architecture,
    Bugfix,
    Discovery,
    Pattern,
    Config,
    Preference,
}

impl MemKind {
    pub const ALL: [MemKind; 7] = [
        MemKind::Decision,
        MemKind::Architecture,
        MemKind::Bugfix,
        MemKind::Discovery,
        MemKind::Pattern,
        MemKind::Config,
        MemKind::Preference,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            MemKind::Decision => "decision",
            MemKind::Architecture => "architecture",
            MemKind::Bugfix => "bugfix",
            MemKind::Discovery => "discovery",
            MemKind::Pattern => "pattern",
            MemKind::Config => "config",
            MemKind::Preference => "preference",
        }
    }

    /// The kind named exactly `text`; anything else is rejected.
    pub fn parse(text: &str) -> Option<MemKind> {
        Self::ALL.into_iter().find(|k| k.as_str() == text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_exactly_the_seven_kinds_in_order() {
        let names: Vec<&str> = MemKind::ALL.iter().map(|k| k.as_str()).collect();
        assert_eq!(
            names,
            [
                "decision",
                "architecture",
                "bugfix",
                "discovery",
                "pattern",
                "config",
                "preference"
            ]
        );
    }

    #[test]
    fn as_str_round_trips_through_parse() {
        for kind in MemKind::ALL {
            assert_eq!(MemKind::parse(kind.as_str()), Some(kind));
        }
    }

    #[test]
    fn rejects_values_outside_the_list() {
        for bad in ["learning", "", "Decision", " bugfix", "bugfix\n", "manual"] {
            assert_eq!(MemKind::parse(bad), None, "{bad:?}");
        }
    }
}
