//! Pane identity and screen geometry. Pure: no ratatui, no IO.

/// Identifies a pane for its whole life. Ids are never reused, so an event
/// for a closed pane can never reach a newer one.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct PaneId(u32);

impl PaneId {
    /// The first id handed out.
    pub const FIRST: PaneId = PaneId(1);
}

/// A cell rectangle in terminal coordinates.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn assert_id_traits<T: Copy + Eq + std::hash::Hash + Ord>() {}

    #[test]
    fn pane_id_is_copy_eq_hash_and_ord() {
        assert_id_traits::<PaneId>();
        let ids = HashSet::from([PaneId::FIRST, PaneId::FIRST]);
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn rect_default_is_empty_at_the_origin() {
        assert_eq!(
            Rect::default(),
            Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0
            }
        );
    }
}
