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

/// Orientation of a split: the direction its two children are laid out in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    /// Side by side, divided by a vertical line.
    X,
    /// Stacked, divided by a horizontal line.
    Y,
}

/// A focus direction on screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Left,
    Down,
    Up,
    Right,
}

/// Smallest usable pane as `(rows, cols)`: a prompt plus one output line, and
/// room for a short prompt.
pub const MIN_PANE: (u16, u16) = (2, 10);

/// Binary layout tree of one tab. Leaves are panes.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Node {
    Leaf(PaneId),
    Split(Box<Split>),
}

/// Two children divided by one separator cell. `weights` are relative sizes.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Split {
    pub axis: Axis,
    pub weights: [u16; 2],
    pub children: [Node; 2],
}

/// Why a split was not applied. The tree is left untouched.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SplitError {
    /// The target pane is too small to hold two panes of at least `MIN_PANE`.
    TooSmall,
    /// The target pane is not in the tree.
    NotFound,
}

/// Outcome of removing a pane from a tree.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Removal {
    /// The pane is not in the tree; nothing changed.
    NotFound,
    /// The pane is gone and its sibling took its space.
    Removed,
    /// The pane is the only one left. The tree is unchanged: the caller closes
    /// the tab.
    WasLast,
}

/// The 1-cell line between the two children of a split. `axis` is the axis of
/// the owning split: `X` is a vertical line (`len` rows), `Y` a horizontal one
/// (`len` columns), starting at `(x, y)`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Separator {
    pub axis: Axis,
    pub x: u16,
    pub y: u16,
    pub len: u16,
}

/// Computed geometry of a tree: pane rects in depth-first order plus separators.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Tiling {
    pub panes: Vec<(PaneId, Rect)>,
    pub separators: Vec<Separator>,
}

/// Hands out pane ids. Ids are never reused, so it never hands out one twice.
#[derive(Debug)]
pub struct PaneIds {
    next: u32,
}

impl Default for PaneIds {
    fn default() -> Self {
        Self {
            next: PaneId::FIRST.0,
        }
    }
}

impl PaneIds {
    /// The next unused id. Panics on exhaustion rather than wrapping into reuse.
    pub fn alloc(&mut self) -> PaneId {
        let id = PaneId(self.next);
        self.next = self.next.checked_add(1).expect("pane id space exhausted");
        id
    }
}

/// Splits `extent` cells between two children. One cell goes to the separator
/// and the rest is divided by weight with the first child rounded down, then
/// clamped so both children keep at least one cell. Below 2 free cells there
/// is no room for a separator: the first child keeps everything.
/// Returns `(first, second, separator)` sizes.
fn divide(extent: u16, weights: [u16; 2]) -> (u16, u16, u16) {
    let avail = extent.saturating_sub(1);
    if avail < 2 {
        return (extent, 0, 0);
    }
    // Zero weights carry no preference: treat them as a tie.
    let [w0, w1] = if weights == [0, 0] { [1, 1] } else { weights };
    let total = u32::from(w0) + u32::from(w1);
    let first = (u32::from(avail) * u32::from(w0) / total).clamp(1, u32::from(avail) - 1);
    let first = first as u16; // <= avail <= u16::MAX
    (first, avail - first, 1)
}

impl Node {
    /// Splits the leaf `target` into itself and the new pane `new` (second
    /// child). `area` is the area this tree is tiled in. The weights are set to
    /// the real cell sizes, so the first pane gets half of what is left after
    /// the separator and the new pane the rest.
    pub fn split(
        &mut self,
        area: Rect,
        target: PaneId,
        new: PaneId,
        axis: Axis,
    ) -> Result<(), SplitError> {
        let rect = tile(self, area)
            .panes
            .iter()
            .find(|(id, _)| *id == target)
            .map(|(_, r)| *r)
            .ok_or(SplitError::NotFound)?;
        let (extent, min) = match axis {
            Axis::X => (rect.width, MIN_PANE.1),
            Axis::Y => (rect.height, MIN_PANE.0),
        };
        // Two minimum panes plus the separator.
        if u32::from(extent) < 2 * u32::from(min) + 1 {
            return Err(SplitError::TooSmall);
        }
        let avail = extent - 1;
        let first = avail / 2;
        let leaf = self
            .leaf_mut(target)
            .expect("the target was found in the tiling");
        *leaf = Node::Split(Box::new(Split {
            axis,
            weights: [first, avail - first],
            children: [Node::Leaf(target), Node::Leaf(new)],
        }));
        Ok(())
    }

    /// Removes the leaf `id`: its sibling replaces the parent split and so
    /// takes all the freed space.
    pub fn remove(&mut self, id: PaneId) -> Removal {
        let split = match self {
            Node::Leaf(leaf) if *leaf == id => return Removal::WasLast,
            Node::Leaf(_) => return Removal::NotFound,
            Node::Split(split) => split,
        };
        if let Some(removed) = split.children.iter().position(|c| *c == Node::Leaf(id)) {
            let sibling = std::mem::replace(&mut split.children[1 - removed], Node::Leaf(id));
            *self = sibling;
            return Removal::Removed;
        }
        if split
            .children
            .iter_mut()
            .any(|c| c.remove(id) == Removal::Removed)
        {
            Removal::Removed
        } else {
            Removal::NotFound
        }
    }

    /// The panes of the tree in depth-first order.
    pub fn leaves(&self) -> Vec<PaneId> {
        match self {
            Node::Leaf(id) => vec![*id],
            Node::Split(split) => split.children.iter().flat_map(Node::leaves).collect(),
        }
    }

    fn leaf_mut(&mut self, id: PaneId) -> Option<&mut Node> {
        match self {
            Node::Leaf(leaf) if *leaf == id => Some(self),
            Node::Leaf(_) => None,
            Node::Split(split) => split.children.iter_mut().find_map(|c| c.leaf_mut(id)),
        }
    }
}

/// Computes the rect of every pane and the separators inside `area`.
pub fn tile(node: &Node, area: Rect) -> Tiling {
    let mut tiling = Tiling::default();
    tile_into(node, area, &mut tiling);
    tiling
}

fn tile_into(node: &Node, area: Rect, out: &mut Tiling) {
    let split = match node {
        Node::Leaf(id) => {
            out.panes.push((*id, area));
            return;
        }
        Node::Split(split) => split,
    };
    let [a, b] = &split.children;
    match split.axis {
        Axis::X => {
            let (first, second, sep) = divide(area.width, split.weights);
            let x2 = area.x.saturating_add(first).saturating_add(sep);
            if sep == 1 {
                out.separators.push(Separator {
                    axis: Axis::X,
                    x: area.x.saturating_add(first),
                    y: area.y,
                    len: area.height,
                });
            }
            tile_into(
                a,
                Rect {
                    width: first,
                    ..area
                },
                out,
            );
            tile_into(
                b,
                Rect {
                    x: x2,
                    width: second,
                    ..area
                },
                out,
            );
        }
        Axis::Y => {
            let (first, second, sep) = divide(area.height, split.weights);
            let y2 = area.y.saturating_add(first).saturating_add(sep);
            if sep == 1 {
                out.separators.push(Separator {
                    axis: Axis::Y,
                    x: area.x,
                    y: area.y.saturating_add(first),
                    len: area.width,
                });
            }
            tile_into(
                a,
                Rect {
                    height: first,
                    ..area
                },
                out,
            );
            tile_into(
                b,
                Rect {
                    y: y2,
                    height: second,
                    ..area
                },
                out,
            );
        }
    }
}

/// The pane whose rect covers the cell `(x, y)`; separators belong to no pane.
pub fn pane_at(t: &Tiling, x: u16, y: u16) -> Option<PaneId> {
    t.panes
        .iter()
        .find(|(_, r)| {
            (r.x..r.x.saturating_add(r.width)).contains(&x)
                && (r.y..r.y.saturating_add(r.height)).contains(&y)
        })
        .map(|(id, _)| *id)
}

/// Reads the `[start, end)` run of a rect along one axis.
type Span = fn(&Rect) -> (u32, u32);

/// The `[start, end)` run of `r` along the x axis, widened so edges never overflow.
fn span_x(r: &Rect) -> (u32, u32) {
    (u32::from(r.x), u32::from(r.x) + u32::from(r.width))
}

fn span_y(r: &Rect) -> (u32, u32) {
    (u32::from(r.y), u32::from(r.y) + u32::from(r.height))
}

/// The pane to focus when moving `dir` from `from`: a visible pane whose edge
/// faces `from` across the 1-cell separator and overlaps it. The largest
/// overlap wins; a tie goes to the lowest start. `None` at an edge (no wrap),
/// for an unknown `from`, and never a zero-area pane.
pub fn neighbour(t: &Tiling, from: PaneId, dir: Direction) -> Option<PaneId> {
    let (_, f) = t.panes.iter().find(|(id, _)| *id == from)?;
    // `along` runs in the direction of travel, `across` is perpendicular to it.
    let (along, across): (Span, Span) = match dir {
        Direction::Left | Direction::Right => (span_x, span_y),
        Direction::Up | Direction::Down => (span_y, span_x),
    };
    let (f_along, f_across) = (along(f), across(f));
    t.panes
        .iter()
        .filter(|(id, c)| *id != from && c.width > 0 && c.height > 0)
        .filter(|(_, c)| match dir {
            Direction::Left | Direction::Up => along(c).1 + 1 == f_along.0,
            Direction::Right | Direction::Down => f_along.1 + 1 == along(c).0,
        })
        .filter_map(|(id, c)| {
            let c_across = across(c);
            let overlap = f_across
                .1
                .min(c_across.1)
                .saturating_sub(f_across.0.max(c_across.0));
            (overlap > 0).then_some((*id, overlap, c_across.0))
        })
        .max_by_key(|&(_, overlap, start)| (overlap, std::cmp::Reverse(start)))
        .map(|(id, _, _)| id)
}

impl Separator {
    /// The `(offset, len)` run of this separator's cells that touch `focused`,
    /// counted from the separator's start; `None` when it does not touch it.
    pub fn highlight(&self, focused: Rect) -> Option<(u16, u16)> {
        // (cross position of the line, start/length of the focused rect along
        // the line, and its start/length across the line).
        let (line_at, along, across) = match self.axis {
            Axis::X => (
                self.x,
                (focused.y, focused.height),
                (focused.x, focused.width),
            ),
            Axis::Y => (
                self.y,
                (focused.x, focused.width),
                (focused.y, focused.height),
            ),
        };
        let line_start = match self.axis {
            Axis::X => self.y,
            Axis::Y => self.x,
        };
        let touches =
            across.0.saturating_add(across.1) == line_at || line_at.saturating_add(1) == across.0;
        let start = along.0.max(line_start);
        let end = along
            .0
            .saturating_add(along.1)
            .min(line_start.saturating_add(self.len));
        (touches && start < end).then(|| (start - line_start, end - start))
    }
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

    /// `n` fresh ids, allocated in order.
    fn ids(n: usize) -> Vec<PaneId> {
        let mut alloc = PaneIds::default();
        (0..n).map(|_| alloc.alloc()).collect()
    }

    fn rect(x: u16, y: u16, width: u16, height: u16) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn split(axis: Axis, weights: [u16; 2], a: Node, b: Node) -> Node {
        Node::Split(Box::new(Split {
            axis,
            weights,
            children: [a, b],
        }))
    }

    fn rect_of(t: &Tiling, id: PaneId) -> Rect {
        t.panes.iter().find(|(p, _)| *p == id).unwrap().1
    }

    #[test]
    fn allocator_starts_at_first_and_never_reuses_an_id() {
        let mut alloc = PaneIds::default();
        let a = alloc.alloc();
        let b = alloc.alloc();
        let c = alloc.alloc();
        assert_eq!(a, PaneId::FIRST);
        assert_eq!(a.0, 1, "ids start at 1");
        assert!(a < b && b < c, "ids grow monotonically");
    }

    #[test]
    #[should_panic(expected = "pane id space exhausted")]
    fn allocator_panics_instead_of_wrapping() {
        let mut alloc = PaneIds { next: u32::MAX };
        alloc.alloc();
    }

    #[test]
    fn single_leaf_takes_the_whole_area() {
        let id = ids(1)[0];
        let area = rect(3, 4, 80, 24);
        let t = tile(&Node::Leaf(id), area);
        assert_eq!(t.panes, vec![(id, area)]);
        assert!(t.separators.is_empty());
    }

    #[test]
    fn side_by_side_split_halves_with_a_separator_column() {
        let [a, b] = ids(2)[..] else { unreachable!() };
        let tree = split(Axis::X, [1, 1], Node::Leaf(a), Node::Leaf(b));
        let t = tile(&tree, rect(0, 0, 81, 24));
        assert_eq!(rect_of(&t, a), rect(0, 0, 40, 24));
        assert_eq!(rect_of(&t, b), rect(41, 0, 40, 24));
        assert_eq!(
            t.separators,
            vec![Separator {
                axis: Axis::X,
                x: 40,
                y: 0,
                len: 24
            }]
        );
    }

    #[test]
    fn stacked_split_halves_with_a_separator_row() {
        let [a, b] = ids(2)[..] else { unreachable!() };
        let tree = split(Axis::Y, [1, 1], Node::Leaf(a), Node::Leaf(b));
        let t = tile(&tree, rect(2, 1, 80, 11));
        assert_eq!(rect_of(&t, a), rect(2, 1, 80, 5));
        assert_eq!(rect_of(&t, b), rect(2, 7, 80, 5));
        assert_eq!(
            t.separators,
            vec![Separator {
                axis: Axis::Y,
                x: 2,
                y: 6,
                len: 80
            }]
        );
    }

    #[test]
    fn rounding_follows_the_weights_and_gives_the_remainder_to_the_second() {
        let [a, b] = ids(2)[..] else { unreachable!() };
        // (extent, weights, expected first width): avail = extent - 1.
        let cases = [
            (80, [1, 1], 39), // avail 79 -> 79 * 1 / 2 = 39, second 40
            (80, [40, 39], 40),
            (80, [1, 2], 26),    // 79 / 3 = 26
            (80, [2, 1], 52),    // 158 / 3 = 52
            (80, [1, 1000], 1),  // clamped up to 1
            (80, [1000, 1], 78), // clamped down to avail - 1
            (80, [0, 0], 39),    // zero weights behave as a tie
            (80, [0, 5], 1),
            (80, [5, 0], 78),        // zero second weight: clamped to avail - 1
            (80, [u16::MAX, 0], 78), // same at the weight extreme
            (80, [0, u16::MAX], 1),  // mirrored
            (80, [u16::MAX, u16::MAX], 39), // no u16 overflow
        ];
        for (extent, weights, first) in cases {
            let tree = split(Axis::X, weights, Node::Leaf(a), Node::Leaf(b));
            let t = tile(&tree, rect(0, 0, extent, 3));
            assert_eq!(rect_of(&t, a).width, first, "{weights:?}");
            assert_eq!(rect_of(&t, b).width, extent - 1 - first, "{weights:?}");
        }
    }

    #[test]
    fn rects_and_separators_sum_to_the_area_for_nested_splits() {
        let [a, b, c, d] = ids(4)[..] else {
            unreachable!()
        };
        let right = split(
            Axis::Y,
            [3, 5],
            Node::Leaf(b),
            split(Axis::X, [1, 1], Node::Leaf(c), Node::Leaf(d)),
        );
        let tree = split(Axis::X, [2, 3], Node::Leaf(a), right);
        for (w, h) in [(80, 24), (79, 23), (31, 9), (21, 5), (200, 60)] {
            let area = rect(5, 7, w, h);
            let t = tile(&tree, area);
            let mut cover = vec![0u8; w as usize * h as usize];
            let mut mark = |r: Rect| {
                for y in r.y..r.y + r.height {
                    for x in r.x..r.x + r.width {
                        cover[(y - area.y) as usize * w as usize + (x - area.x) as usize] += 1;
                    }
                }
            };
            for (_, r) in &t.panes {
                mark(*r);
            }
            for s in &t.separators {
                match s.axis {
                    Axis::X => mark(rect(s.x, s.y, 1, s.len)),
                    Axis::Y => mark(rect(s.x, s.y, s.len, 1)),
                }
            }
            assert!(cover.iter().all(|&n| n == 1), "{w}x{h}: exact cover");
        }
    }

    #[test]
    fn degenerate_extents_never_panic_and_the_first_child_keeps_everything() {
        let [a, b] = ids(2)[..] else { unreachable!() };
        for axis in [Axis::X, Axis::Y] {
            for extent in 0..=2u16 {
                let tree = split(axis, [1, 1], Node::Leaf(a), Node::Leaf(b));
                let area = match axis {
                    Axis::X => rect(7, 7, extent, 4),
                    Axis::Y => rect(7, 7, 4, extent),
                };
                let t = tile(&tree, area);
                assert_eq!(rect_of(&t, a), area, "{axis:?} {extent}");
                let second = rect_of(&t, b);
                assert_eq!(second.width * second.height, 0, "{axis:?} {extent}");
                assert!(t.separators.is_empty(), "{axis:?} {extent}");
            }
        }
    }

    #[test]
    fn an_empty_area_and_the_far_corner_of_the_plane_do_not_overflow() {
        let [a, b] = ids(2)[..] else { unreachable!() };
        let tree = split(Axis::X, [1, 1], Node::Leaf(a), Node::Leaf(b));
        tile(&tree, rect(0, 0, 0, 0));
        let t = tile(&tree, rect(u16::MAX - 3, u16::MAX - 3, 10, 10));
        assert_eq!(t.panes.len(), 2);
    }

    #[test]
    fn pane_at_finds_the_pane_covering_a_cell_and_none_on_separators() {
        let [a, b] = ids(2)[..] else { unreachable!() };
        let tree = split(Axis::X, [1, 1], Node::Leaf(a), Node::Leaf(b));
        let t = tile(&tree, rect(0, 0, 81, 24));
        assert_eq!(pane_at(&t, 0, 0), Some(a));
        assert_eq!(pane_at(&t, 39, 23), Some(a));
        assert_eq!(pane_at(&t, 40, 5), None, "separator column");
        assert_eq!(pane_at(&t, 41, 0), Some(b));
        assert_eq!(pane_at(&t, 80, 23), Some(b));
        assert_eq!(pane_at(&t, 81, 0), None, "outside");
        assert_eq!(pane_at(&t, 0, 24), None, "outside");
    }

    #[test]
    fn pane_at_ignores_zero_size_panes() {
        let [a, b] = ids(2)[..] else { unreachable!() };
        let tree = split(Axis::X, [1, 1], Node::Leaf(a), Node::Leaf(b));
        let t = tile(&tree, rect(0, 0, 2, 4));
        assert_eq!(pane_at(&t, 1, 0), Some(a));
        assert_eq!(pane_at(&t, 2, 0), None);
    }

    #[test]
    fn highlight_covers_the_separator_cells_next_to_the_focused_pane() {
        let [a, b, c] = ids(3)[..] else {
            unreachable!()
        };
        // a | (b over c): the root separator spans the full height.
        let right = split(Axis::Y, [1, 1], Node::Leaf(b), Node::Leaf(c));
        let tree = split(Axis::X, [1, 1], Node::Leaf(a), right);
        let t = tile(&tree, rect(0, 0, 81, 21));
        let root = t.separators[0];
        let horizontal = t.separators[1];
        assert_eq!((root.axis, horizontal.axis), (Axis::X, Axis::Y));
        // Left pane touches the whole vertical separator and nothing else.
        assert_eq!(root.highlight(rect_of(&t, a)), Some((0, 21)));
        assert_eq!(horizontal.highlight(rect_of(&t, a)), None);
        // Top-right pane touches the upper half of the vertical separator.
        assert_eq!(root.highlight(rect_of(&t, b)), Some((0, 10)));
        assert_eq!(horizontal.highlight(rect_of(&t, b)), Some((0, 40)));
        // Bottom-right pane: lower half, offset past the top pane.
        assert_eq!(root.highlight(rect_of(&t, c)), Some((11, 10)));
        assert_eq!(horizontal.highlight(rect_of(&t, c)), Some((0, 40)));
    }

    #[test]
    fn highlight_is_none_for_a_pane_that_is_not_adjacent() {
        let [a, b, c] = ids(3)[..] else {
            unreachable!()
        };
        let tree = split(
            Axis::X,
            [1, 1],
            Node::Leaf(a),
            split(Axis::X, [1, 1], Node::Leaf(b), Node::Leaf(c)),
        );
        let t = tile(&tree, rect(0, 0, 82, 5));
        let (left_sep, right_sep) = (t.separators[0], t.separators[1]);
        assert_eq!(left_sep.highlight(rect_of(&t, a)), Some((0, 5)));
        assert_eq!(right_sep.highlight(rect_of(&t, a)), None);
        assert_eq!(left_sep.highlight(rect_of(&t, c)), None);
        assert_eq!(right_sep.highlight(rect_of(&t, c)), Some((0, 5)));
        assert_eq!(left_sep.highlight(Rect::default()), None);
    }

    #[test]
    fn highlight_is_none_when_the_rect_edge_is_on_the_line_but_past_its_end() {
        let sep = Separator {
            axis: Axis::X,
            x: 10,
            y: 0,
            len: 5,
        };
        // Right edge touches x = 10, but the rows 5..10 lie past the separator.
        assert_eq!(sep.highlight(rect(0, 5, 10, 5)), None);
        let sep = Separator {
            axis: Axis::Y,
            x: 0,
            y: 10,
            len: 5,
        };
        assert_eq!(sep.highlight(rect(5, 0, 5, 10)), None);
    }

    #[test]
    fn highlight_is_clipped_to_the_separator_length() {
        let sep = Separator {
            axis: Axis::X,
            x: 10,
            y: 4,
            len: 5,
        };
        // A tall pane next to a short split reaches beyond both ends.
        assert_eq!(sep.highlight(rect(0, 0, 10, 20)), Some((0, 5)));
        let sep = Separator {
            axis: Axis::Y,
            x: 4,
            y: 10,
            len: 5,
        };
        assert_eq!(sep.highlight(rect(0, 0, 20, 10)), Some((0, 5)));
    }

    fn leaf(id: PaneId) -> Node {
        Node::Leaf(id)
    }

    #[test]
    fn split_right_and_below_make_two_panes_with_weights_equal_to_cell_sizes() {
        let [a, b] = ids(2)[..] else { unreachable!() };
        let mut tree = leaf(a);
        assert_eq!(tree.split(rect(0, 0, 81, 24), a, b, Axis::X), Ok(()));
        assert_eq!(tree, split(Axis::X, [40, 40], leaf(a), leaf(b)));
        let t = tile(&tree, rect(0, 0, 81, 24));
        assert_eq!(rect_of(&t, a), rect(0, 0, 40, 24));
        assert_eq!(rect_of(&t, b), rect(41, 0, 40, 24));

        let mut tree = leaf(a);
        assert_eq!(tree.split(rect(0, 0, 80, 11), a, b, Axis::Y), Ok(()));
        assert_eq!(tree, split(Axis::Y, [5, 5], leaf(a), leaf(b)));
        let t = tile(&tree, rect(0, 0, 80, 11));
        assert_eq!(rect_of(&t, a), rect(0, 0, 80, 5));
        assert_eq!(rect_of(&t, b), rect(0, 6, 80, 5));
    }

    #[test]
    fn split_gives_the_odd_cell_to_the_new_pane() {
        let [a, b] = ids(2)[..] else { unreachable!() };
        let mut tree = leaf(a);
        tree.split(rect(0, 0, 80, 24), a, b, Axis::X).unwrap();
        assert_eq!(tree, split(Axis::X, [39, 40], leaf(a), leaf(b)));
        let t = tile(&tree, rect(0, 0, 80, 24));
        assert_eq!(rect_of(&t, a).width, 39);
        assert_eq!(rect_of(&t, b).width, 40);
    }

    #[test]
    fn split_applies_at_the_minimum_and_is_refused_one_cell_less() {
        let [a, b] = ids(2)[..] else { unreachable!() };
        // Side by side needs 21 columns; the other extent is irrelevant.
        for (width, height, ok) in [(21, 1, true), (20, 100, false)] {
            let mut tree = leaf(a);
            let r = tree.split(rect(0, 0, width, height), a, b, Axis::X);
            assert_eq!(r.is_ok(), ok, "X {width}x{height}");
            if ok {
                let t = tile(&tree, rect(0, 0, width, height));
                assert_eq!(rect_of(&t, a).width, 10);
                assert_eq!(rect_of(&t, b).width, 10);
            } else {
                assert_eq!(r, Err(SplitError::TooSmall));
                assert_eq!(tree, leaf(a), "refused split leaves the tree untouched");
            }
        }
        // Stacked needs 5 rows; the other extent is irrelevant.
        for (width, height, ok) in [(1, 5, true), (100, 4, false)] {
            let mut tree = leaf(a);
            let r = tree.split(rect(0, 0, width, height), a, b, Axis::Y);
            assert_eq!(r.is_ok(), ok, "Y {width}x{height}");
            if ok {
                let t = tile(&tree, rect(0, 0, width, height));
                assert_eq!(rect_of(&t, a).height, 2);
                assert_eq!(rect_of(&t, b).height, 2);
            } else {
                assert_eq!(r, Err(SplitError::TooSmall));
                assert_eq!(tree, leaf(a));
            }
        }
    }

    #[test]
    fn split_checks_the_target_rect_not_the_whole_area() {
        let [a, b, c] = ids(3)[..] else {
            unreachable!()
        };
        // 41 columns: a and b get 20 each, below the 21 a further split needs.
        let mut tree = split(Axis::X, [20, 20], leaf(a), leaf(b));
        let before = tree.clone();
        assert_eq!(
            tree.split(rect(0, 0, 41, 10), b, c, Axis::X),
            Err(SplitError::TooSmall)
        );
        assert_eq!(tree, before);
    }

    #[test]
    fn split_replaces_the_target_leaf_in_place_inside_a_nested_tree() {
        let [a, b, c] = ids(3)[..] else {
            unreachable!()
        };
        let area = rect(0, 0, 81, 24);
        let mut tree = split(Axis::X, [40, 40], leaf(a), leaf(b));
        assert_eq!(tree.split(area, b, c, Axis::Y), Ok(()));
        assert_eq!(
            tree,
            split(
                Axis::X,
                [40, 40],
                leaf(a),
                split(Axis::Y, [11, 12], leaf(b), leaf(c))
            )
        );
        // The first child splits too, keeping its position.
        let d = ids(4)[3];
        assert_eq!(tree.split(area, a, d, Axis::Y), Ok(()));
        let Node::Split(root) = &tree else {
            unreachable!()
        };
        assert_eq!(root.children[0], split(Axis::Y, [11, 12], leaf(a), leaf(d)));
    }

    #[test]
    fn split_of_an_unknown_target_is_not_found_and_changes_nothing() {
        let [a, b, c] = ids(3)[..] else {
            unreachable!()
        };
        let mut tree = split(Axis::X, [40, 40], leaf(a), leaf(b));
        let before = tree.clone();
        assert_eq!(
            tree.split(rect(0, 0, 81, 24), c, c, Axis::X),
            Err(SplitError::NotFound)
        );
        assert_eq!(tree, before);
    }

    #[test]
    fn remove_collapses_the_split_so_the_sibling_takes_the_whole_area() {
        let [a, b] = ids(2)[..] else { unreachable!() };
        let area = rect(0, 0, 81, 24);
        // Either child can go; the other one must be the one that stays.
        let mut tree = split(Axis::X, [40, 40], leaf(a), leaf(b));
        assert_eq!(tree.remove(b), Removal::Removed);
        assert_eq!(tree, leaf(a));
        assert_eq!(tile(&tree, area).panes, vec![(a, area)]);

        let mut tree = split(Axis::X, [40, 40], leaf(a), leaf(b));
        assert_eq!(tree.remove(a), Removal::Removed);
        assert_eq!(tree, leaf(b));
        assert_eq!(tile(&tree, area).panes, vec![(b, area)]);
    }

    #[test]
    fn remove_after_split_restores_the_parent_rect() {
        let [a, b] = ids(2)[..] else { unreachable!() };
        let area = rect(3, 2, 80, 24);
        let mut tree = leaf(a);
        let before = tile(&tree, area);
        tree.split(area, a, b, Axis::Y).unwrap();
        assert_eq!(tree.remove(b), Removal::Removed);
        assert_eq!(tile(&tree, area), before);
    }

    #[test]
    fn remove_in_a_nested_tree_keeps_the_rest_of_the_structure() {
        let [a, b, c] = ids(3)[..] else {
            unreachable!()
        };
        let area = rect(0, 0, 81, 21);
        let right = split(Axis::Y, [5, 15], leaf(b), leaf(c));
        let tree = split(Axis::X, [40, 40], leaf(a), right.clone());

        let mut t = tree.clone();
        assert_eq!(t.remove(b), Removal::Removed);
        assert_eq!(t, split(Axis::X, [40, 40], leaf(a), leaf(c)));
        assert_eq!(rect_of(&tile(&t, area), c), rect(41, 0, 40, 21));

        let mut t = tree.clone();
        assert_eq!(t.remove(a), Removal::Removed);
        assert_eq!(t, right, "the surviving subtree keeps its own weights");
        assert_eq!(rect_of(&tile(&t, area), b), rect(0, 0, 81, 5));

        // A leaf in the first child's subtree collapses there.
        let mut t = split(
            Axis::X,
            [40, 40],
            split(Axis::Y, [5, 15], leaf(a), leaf(b)),
            leaf(c),
        );
        assert_eq!(t.remove(b), Removal::Removed);
        assert_eq!(t, split(Axis::X, [40, 40], leaf(a), leaf(c)));
    }

    #[test]
    fn removing_the_last_pane_reports_was_last_and_an_unknown_one_not_found() {
        let [a, b, c] = ids(3)[..] else {
            unreachable!()
        };
        let mut tree = leaf(a);
        assert_eq!(tree.remove(a), Removal::WasLast);
        assert_eq!(tree, leaf(a), "the caller closes the tab");
        assert_eq!(tree.remove(b), Removal::NotFound);
        assert_eq!(tree, leaf(a));

        let mut tree = split(Axis::X, [40, 40], leaf(a), leaf(b));
        let before = tree.clone();
        assert_eq!(tree.remove(c), Removal::NotFound);
        assert_eq!(tree, before);
    }

    #[test]
    fn leaves_lists_the_panes_depth_first() {
        let [a, b, c, d] = ids(4)[..] else {
            unreachable!()
        };
        assert_eq!(leaf(a).leaves(), vec![a]);
        let tree = split(
            Axis::X,
            [1, 1],
            split(Axis::Y, [1, 1], leaf(a), leaf(b)),
            split(Axis::Y, [1, 1], leaf(c), leaf(d)),
        );
        assert_eq!(tree.leaves(), vec![a, b, c, d]);
    }

    #[test]
    fn focus_after_closing_the_focused_pane_is_the_one_covering_its_old_top_left() {
        let [a, b, c] = ids(3)[..] else {
            unreachable!()
        };
        let area = rect(0, 0, 81, 21);
        let right = split(Axis::Y, [10, 10], leaf(b), leaf(c));
        let mut tree = split(Axis::X, [40, 40], leaf(a), right);
        // B (top right) is closed: C expands over the right column.
        let old = rect_of(&tile(&tree, area), b);
        assert_eq!(tree.remove(b), Removal::Removed);
        assert_eq!(pane_at(&tile(&tree, area), old.x, old.y), Some(c));
        // Closing the right pane of two: the left one covers its old corner.
        let old = rect_of(&tile(&tree, area), c);
        assert_eq!(tree.remove(c), Removal::Removed);
        assert_eq!(pane_at(&tile(&tree, area), old.x, old.y), Some(a));
    }

    /// A hand-made tiling, so a test controls every rect exactly.
    fn tiling(panes: &[(PaneId, Rect)]) -> Tiling {
        Tiling {
            panes: panes.to_vec(),
            separators: Vec::new(),
        }
    }

    #[test]
    fn neighbour_moves_across_the_separator_and_stops_at_the_edges() {
        let [a, b] = ids(2)[..] else { unreachable!() };
        let tree = split(Axis::X, [40, 40], leaf(a), leaf(b));
        let t = tile(&tree, rect(0, 0, 81, 24));
        assert_eq!(neighbour(&t, a, Direction::Right), Some(b));
        assert_eq!(neighbour(&t, b, Direction::Left), Some(a));
        // No wrap, and nothing above or below.
        assert_eq!(neighbour(&t, a, Direction::Left), None);
        assert_eq!(neighbour(&t, b, Direction::Right), None);
        assert_eq!(neighbour(&t, a, Direction::Up), None);
        assert_eq!(neighbour(&t, a, Direction::Down), None);

        let tree = split(Axis::Y, [11, 12], leaf(a), leaf(b));
        let t = tile(&tree, rect(0, 0, 80, 24));
        assert_eq!(neighbour(&t, a, Direction::Down), Some(b));
        assert_eq!(neighbour(&t, b, Direction::Up), Some(a));
        assert_eq!(neighbour(&t, a, Direction::Up), None);
        assert_eq!(neighbour(&t, b, Direction::Down), None);
        assert_eq!(neighbour(&t, a, Direction::Left), None);
    }

    #[test]
    fn neighbour_requires_exactly_one_separator_cell_between_the_rects() {
        let [a, b] = ids(2)[..] else { unreachable!() };
        let from = rect(10, 10, 5, 5);
        let cases = [
            // (candidate rect, direction, expected)
            (rect(16, 10, 5, 5), Direction::Right, Some(b)), // 1-cell gap
            (rect(15, 10, 5, 5), Direction::Right, None),    // touching
            (rect(17, 10, 5, 5), Direction::Right, None),    // 2-cell gap
            (rect(4, 10, 5, 5), Direction::Left, Some(b)),
            (rect(5, 10, 5, 5), Direction::Left, None),
            (rect(3, 10, 5, 5), Direction::Left, None),
            (rect(10, 16, 5, 5), Direction::Down, Some(b)),
            (rect(10, 15, 5, 5), Direction::Down, None),
            (rect(10, 17, 5, 5), Direction::Down, None),
            (rect(10, 4, 5, 5), Direction::Up, Some(b)),
            (rect(10, 5, 5, 5), Direction::Up, None),
            (rect(10, 3, 5, 5), Direction::Up, None),
            // Wrong side for the direction.
            (rect(16, 10, 5, 5), Direction::Left, None),
            (rect(4, 10, 5, 5), Direction::Right, None),
            // Adjacent but only diagonal: no overlap on the other axis.
            (rect(16, 15, 5, 5), Direction::Right, None),
            (rect(16, 5, 5, 4), Direction::Right, None),
        ];
        for (candidate, dir, expected) in cases {
            let t = tiling(&[(a, from), (b, candidate)]);
            assert_eq!(neighbour(&t, a, dir), expected, "{candidate:?} {dir:?}");
        }
    }

    #[test]
    fn neighbour_picks_the_largest_overlap_not_the_tree_order() {
        let [a, b, c] = ids(3)[..] else {
            unreachable!()
        };
        // b (top, 5 rows) comes first in the tree but c overlaps `a` on 15 rows.
        let right = split(Axis::Y, [5, 15], leaf(b), leaf(c));
        let tree = split(Axis::X, [40, 40], leaf(a), right);
        let t = tile(&tree, rect(0, 0, 81, 21));
        assert_eq!(neighbour(&t, a, Direction::Right), Some(c));
        assert_eq!(neighbour(&t, b, Direction::Left), Some(a));
        assert_eq!(neighbour(&t, c, Direction::Left), Some(a));
        assert_eq!(neighbour(&t, b, Direction::Down), Some(c));
        assert_eq!(neighbour(&t, c, Direction::Up), Some(b));

        // Mirrored: the larger overlap is the first one in tree order.
        let right = split(Axis::Y, [15, 5], leaf(b), leaf(c));
        let tree = split(Axis::X, [40, 40], leaf(a), right);
        let t = tile(&tree, rect(0, 0, 81, 21));
        assert_eq!(neighbour(&t, a, Direction::Right), Some(b));
    }

    #[test]
    fn neighbour_breaks_an_overlap_tie_by_the_lowest_start() {
        let [a, b, c] = ids(3)[..] else {
            unreachable!()
        };
        // Horizontal movement: b and c each overlap `a` on 10 rows.
        let right = split(Axis::Y, [10, 10], leaf(b), leaf(c));
        let tree = split(Axis::X, [40, 40], leaf(a), right);
        let t = tile(&tree, rect(0, 0, 81, 21));
        assert_eq!(neighbour(&t, a, Direction::Right), Some(b), "lowest y");

        // Vertical movement: a and b each overlap `c` on 40 columns.
        let top = split(Axis::X, [40, 40], leaf(a), leaf(b));
        let tree = split(Axis::Y, [10, 10], top, leaf(c));
        let t = tile(&tree, rect(0, 0, 81, 21));
        assert_eq!(neighbour(&t, c, Direction::Up), Some(a), "lowest x");

        // Tree order must not matter: the same tie with the panes swapped.
        let tree = split(
            Axis::X,
            [40, 40],
            leaf(a),
            split(Axis::Y, [10, 10], leaf(c), leaf(b)),
        );
        let t = tile(&tree, rect(0, 0, 81, 21));
        assert_eq!(neighbour(&t, a, Direction::Right), Some(c));
    }

    #[test]
    fn neighbour_skips_zero_area_rects() {
        let [a, b, c] = ids(3)[..] else {
            unreachable!()
        };
        let from = rect(0, 0, 5, 5);
        // A zero-width and a zero-height pane sitting exactly where a
        // neighbour would be: focus must not land on an invisible pane.
        let t = tiling(&[(a, from), (b, rect(6, 0, 0, 5))]);
        assert_eq!(neighbour(&t, a, Direction::Right), None);
        let t = tiling(&[(a, from), (b, rect(0, 6, 5, 0))]);
        assert_eq!(neighbour(&t, a, Direction::Down), None);
        // Left and up, too.
        let from = rect(10, 10, 5, 5);
        let t = tiling(&[(a, from), (b, rect(9, 10, 0, 5))]);
        assert_eq!(neighbour(&t, a, Direction::Left), None);
        let t = tiling(&[(a, from), (b, rect(10, 9, 5, 0))]);
        assert_eq!(neighbour(&t, a, Direction::Up), None);
        // A visible pane with less overlap wins over an invisible bigger one.
        let t = tiling(&[
            (a, rect(0, 0, 5, 5)),
            (b, rect(6, 0, 0, 5)),
            (c, rect(6, 3, 4, 5)),
        ]);
        assert_eq!(neighbour(&t, a, Direction::Right), Some(c));
    }

    #[test]
    fn neighbour_ignores_the_origin_pane_and_an_unknown_one() {
        let [a, b] = ids(2)[..] else { unreachable!() };
        let t = tiling(&[(a, rect(0, 0, 5, 5))]);
        for dir in [
            Direction::Left,
            Direction::Down,
            Direction::Up,
            Direction::Right,
        ] {
            assert_eq!(neighbour(&t, a, dir), None);
            assert_eq!(neighbour(&t, b, dir), None, "unknown pane");
        }
    }

    #[test]
    fn neighbour_does_not_overflow_at_the_far_corner_of_the_plane() {
        let [a, b] = ids(2)[..] else { unreachable!() };
        let t = tiling(&[
            (a, rect(u16::MAX - 4, u16::MAX - 4, 4, 4)),
            (b, rect(0, 0, u16::MAX, u16::MAX)),
        ]);
        for dir in [
            Direction::Left,
            Direction::Down,
            Direction::Up,
            Direction::Right,
        ] {
            neighbour(&t, a, dir);
            neighbour(&t, b, dir);
        }
    }
}
