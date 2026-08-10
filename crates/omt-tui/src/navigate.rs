//! Moving focus in a direction, geometrically.
//!
//! `docs/architecture/17-panes-and-layout.md` §6.
//!
//! **The algorithm runs over the last computed geometry, not over tree order.**
//! Tree order gives wrong answers in asymmetric layouts — the pane that is
//! visually to the left may be anywhere in the tree — which is why every
//! multiplexer that got this right does it geometrically.
//!
//! The ranking tuple is *total*, so `navigate` is a deterministic function of
//! `(geometry, from, direction)`. That is a deliberate divergence from tmux and
//! zellij, which break ties by most-recently-active. Recency makes the same
//! keypress in the same visible layout do two different things depending on
//! history the user cannot see, and there is no way to learn a rule like that.
//! Losing "it goes where I was last" is worth being able to predict anything at
//! all.

/// A pane's placed rectangle, in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Column of the left edge.
    pub left: u16,
    /// Row of the top edge.
    pub top: u16,
    /// One past the right edge.
    pub right: u16,
    /// One past the bottom edge.
    pub bottom: u16,
}

impl Rect {
    /// The centre of its horizontal extent, doubled to stay in integers.
    fn centre_x2(self) -> u32 {
        u32::from(self.left) + u32::from(self.right)
    }

    /// Likewise vertically.
    fn centre_y2(self) -> u32 {
        u32::from(self.top) + u32::from(self.bottom)
    }
}

/// One placed pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    /// Which pane.
    pub id: String,
    /// Where it is.
    pub rect: Rect,
    /// Whether it is currently drawn.
    ///
    /// A hidden pane is skipped rather than treated as absent, so a stack that
    /// is collapsed does not make its neighbours look adjacent to each other
    /// and then stop being so when it reopens.
    pub hidden: bool,
    /// Its stable pre-order index, which is the total tie-break.
    pub placement_index: u32,
}

/// Which way to move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Left.
    Left,
    /// Right.
    Right,
    /// Up.
    Up,
    /// Down.
    Down,
}

/// Whether two intervals overlap at all.
///
/// Strict on both ends: two panes that merely touch at a boundary do not
/// overlap. Treating a shared edge as overlap makes the pane diagonally above
/// a candidate for `Left`, which is the answer nobody expects.
fn overlaps(a0: u16, a1: u16, b0: u16, b1: u16) -> bool {
    a0 < b1 && a1 > b0
}

/// How many cells two intervals share.
fn overlap_cells(a0: u16, a1: u16, b0: u16, b1: u16) -> u32 {
    u32::from(a1.min(b1).saturating_sub(a0.max(b0)))
}

/// The pane focus should move to, if any.
#[must_use]
pub fn navigate(
    panes: &[Placed],
    from: &str,
    direction: Direction,
    wrap: bool,
) -> Option<String> {
    let source = panes.iter().find(|p| p.id == from)?;
    let f = source.rect;

    let candidates: Vec<&Placed> = panes
        .iter()
        .filter(|p| p.id != from && !p.hidden)
        .filter(|p| match direction {
            // `<=` rather than exact adjacency: a pane two columns away is
            // eligible when nothing is adjacent, which is what happens after a
            // hidden pane leaves a gap.
            Direction::Left => {
                p.rect.right <= f.left && overlaps(p.rect.top, p.rect.bottom, f.top, f.bottom)
            }
            Direction::Right => {
                p.rect.left >= f.right && overlaps(p.rect.top, p.rect.bottom, f.top, f.bottom)
            }
            Direction::Up => {
                p.rect.bottom <= f.top && overlaps(p.rect.left, p.rect.right, f.left, f.right)
            }
            Direction::Down => {
                p.rect.top >= f.bottom && overlaps(p.rect.left, p.rect.right, f.left, f.right)
            }
        })
        .collect();

    if candidates.is_empty() {
        if !wrap {
            return None;
        }
        // Wrap jumps to the far side: the same filter transposed, ranked by
        // greatest edge distance. Off by default, because tmux wraps and it
        // surprises people — a focus key that leaves the edge of the screen is
        // read as a bug before it is read as a feature.
        return wrap_around(panes, from, direction);
    }

    candidates
        .into_iter()
        .min_by_key(|p| rank(f, p, direction))
        .map(|p| p.id.clone())
}

/// The ranking tuple: nearest edge, then most overlap, then nearest centre,
/// then placement order.
fn rank(f: Rect, p: &Placed, direction: Direction) -> (u32, std::cmp::Reverse<u32>, u32, u32) {
    let (edge, overlap, centre) = match direction {
        Direction::Left => (
            u32::from(f.left.saturating_sub(p.rect.right)),
            overlap_cells(p.rect.top, p.rect.bottom, f.top, f.bottom),
            f.centre_y2().abs_diff(p.rect.centre_y2()),
        ),
        Direction::Right => (
            u32::from(p.rect.left.saturating_sub(f.right)),
            overlap_cells(p.rect.top, p.rect.bottom, f.top, f.bottom),
            f.centre_y2().abs_diff(p.rect.centre_y2()),
        ),
        Direction::Up => (
            u32::from(f.top.saturating_sub(p.rect.bottom)),
            overlap_cells(p.rect.left, p.rect.right, f.left, f.right),
            f.centre_x2().abs_diff(p.rect.centre_x2()),
        ),
        Direction::Down => (
            u32::from(p.rect.top.saturating_sub(f.bottom)),
            overlap_cells(p.rect.left, p.rect.right, f.left, f.right),
            f.centre_x2().abs_diff(p.rect.centre_x2()),
        ),
    };
    (edge, std::cmp::Reverse(overlap), centre, p.placement_index)
}

/// Jump to the far side.
fn wrap_around(panes: &[Placed], from: &str, direction: Direction) -> Option<String> {
    let source = panes.iter().find(|p| p.id == from)?;
    let f = source.rect;
    let opposite = match direction {
        Direction::Left => Direction::Right,
        Direction::Right => Direction::Left,
        Direction::Up => Direction::Down,
        Direction::Down => Direction::Up,
    };
    panes
        .iter()
        .filter(|p| p.id != from && !p.hidden)
        .filter(|p| match opposite {
            Direction::Left => {
                p.rect.right <= f.left && overlaps(p.rect.top, p.rect.bottom, f.top, f.bottom)
            }
            Direction::Right => {
                p.rect.left >= f.right && overlaps(p.rect.top, p.rect.bottom, f.top, f.bottom)
            }
            Direction::Up => {
                p.rect.bottom <= f.top && overlaps(p.rect.left, p.rect.right, f.left, f.right)
            }
            Direction::Down => {
                p.rect.top >= f.bottom && overlaps(p.rect.left, p.rect.right, f.left, f.right)
            }
        })
        .max_by_key(|p| {
            let (edge, ..) = {
                let r = rank(f, p, opposite);
                (r.0, r.1, r.2, r.3)
            };
            (edge, std::cmp::Reverse(p.placement_index))
        })
        .map(|p| p.id.clone())
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn pane(id: &str, left: u16, top: u16, right: u16, bottom: u16, index: u32) -> Placed {
        Placed {
            id: id.to_owned(),
            rect: Rect {
                left,
                top,
                right,
                bottom,
            },
            hidden: false,
            placement_index: index,
        }
    }

    /// Two side by side: `a | b`.
    fn side_by_side() -> Vec<Placed> {
        vec![pane("a", 0, 0, 40, 24, 0), pane("b", 40, 0, 80, 24, 1)]
    }

    /// One tall pane on the left, two stacked on the right.
    ///
    /// ```text
    /// +------+------+
    /// |      |  b   |
    /// |  a   +------+
    /// |      |  c   |
    /// +------+------+
    /// ```
    fn asymmetric() -> Vec<Placed> {
        vec![
            pane("a", 0, 0, 40, 24, 0),
            pane("b", 40, 0, 80, 12, 1),
            pane("c", 40, 12, 80, 24, 2),
        ]
    }

    #[test]
    fn focus_moves_to_the_pane_that_is_actually_there() {
        let panes = side_by_side();
        assert_eq!(
            navigate(&panes, "a", Direction::Right, false).as_deref(),
            Some("b")
        );
        assert_eq!(
            navigate(&panes, "b", Direction::Left, false).as_deref(),
            Some("a")
        );
    }

    #[test]
    fn there_is_nothing_past_the_edge_without_wrap() {
        // A focus key that leaves the edge of the screen reads as a bug before
        // it reads as a feature, which is why wrap is off by default.
        let panes = side_by_side();
        assert_eq!(navigate(&panes, "a", Direction::Left, false), None);
        assert_eq!(navigate(&panes, "b", Direction::Right, false), None);
        assert_eq!(navigate(&panes, "a", Direction::Up, false), None);
    }

    #[test]
    fn wrap_jumps_to_the_far_side_when_it_is_asked_for() {
        let panes = side_by_side();
        assert_eq!(
            navigate(&panes, "a", Direction::Left, true).as_deref(),
            Some("b")
        );
    }

    #[test]
    fn the_most_overlapping_neighbour_wins_in_an_asymmetric_layout() {
        // Tree order would give a different answer here, which is the whole
        // reason the algorithm is geometric.
        let mut panes = asymmetric();
        // Focus starts in the top half of `a`, so `b` overlaps more than `c`.
        panes[0].rect = Rect {
            left: 0,
            top: 0,
            right: 40,
            bottom: 10,
        };
        assert_eq!(
            navigate(&panes, "a", Direction::Right, false).as_deref(),
            Some("b")
        );
    }

    #[test]
    fn moving_back_from_a_stacked_neighbour_finds_the_tall_pane() {
        let panes = asymmetric();
        for id in ["b", "c"] {
            assert_eq!(
                navigate(&panes, id, Direction::Left, false).as_deref(),
                Some("a"),
                "from {id}"
            );
        }
    }

    #[test]
    fn a_pane_that_only_touches_at_a_corner_is_not_a_neighbour() {
        // Treating a shared edge as overlap makes the pane diagonally above a
        // candidate for Left, which is the answer nobody expects.
        let panes = vec![
            pane("a", 40, 12, 80, 24, 0),
            // Directly above-left, touching only at the corner.
            pane("b", 0, 0, 40, 12, 1),
        ];
        assert_eq!(navigate(&panes, "a", Direction::Left, false), None);
    }

    #[test]
    fn a_hidden_pane_is_skipped_and_the_one_behind_it_is_reachable() {
        // A collapsed stack must not make its neighbours look adjacent and
        // then stop being so when it reopens.
        let mut panes = vec![
            pane("a", 0, 0, 20, 24, 0),
            pane("b", 20, 0, 40, 24, 1),
            pane("c", 40, 0, 60, 24, 2),
        ];
        panes[1].hidden = true;
        assert_eq!(
            navigate(&panes, "a", Direction::Right, false).as_deref(),
            Some("c")
        );
    }

    #[test]
    fn the_answer_does_not_depend_on_history() {
        // The deliberate divergence from tmux and zellij. Recency makes the
        // same keypress in the same visible layout do two different things
        // depending on something the user cannot see.
        let panes = asymmetric();
        let first = navigate(&panes, "a", Direction::Right, false);
        for _ in 0..10 {
            assert_eq!(navigate(&panes, "a", Direction::Right, false), first);
        }
    }

    #[test]
    fn the_order_panes_are_listed_in_does_not_change_the_answer() {
        // `placement_index` is the tie-break, not position in the slice. If it
        // were the slice, the answer would depend on how the caller happened
        // to build the list.
        let panes = asymmetric();
        let forward = navigate(&panes, "a", Direction::Right, false);
        let mut reversed = panes;
        reversed.reverse();
        assert_eq!(navigate(&reversed, "a", Direction::Right, false), forward);
    }

    #[test]
    fn navigating_from_a_pane_that_is_not_there_is_none_rather_than_a_panic() {
        assert_eq!(navigate(&side_by_side(), "nope", Direction::Left, false), None);
    }

    #[test]
    fn a_single_pane_has_nowhere_to_go_in_any_direction() {
        let alone = vec![pane("a", 0, 0, 80, 24, 0)];
        for direction in [
            Direction::Left,
            Direction::Right,
            Direction::Up,
            Direction::Down,
        ] {
            assert_eq!(navigate(&alone, "a", direction, true), None, "{direction:?}");
        }
    }

    #[test]
    fn every_direction_round_trips_in_a_simple_grid() {
        // Four quadrants. Going one way and back must return where it started,
        // or focus movement is not something anybody can navigate by feel.
        let panes = vec![
            pane("tl", 0, 0, 40, 12, 0),
            pane("tr", 40, 0, 80, 12, 1),
            pane("bl", 0, 12, 40, 24, 2),
            pane("br", 40, 12, 80, 24, 3),
        ];
        for (from, direction, back) in [
            ("tl", Direction::Right, Direction::Left),
            ("tl", Direction::Down, Direction::Up),
            ("br", Direction::Left, Direction::Right),
            ("br", Direction::Up, Direction::Down),
        ] {
            let there = navigate(&panes, from, direction, false).expect("a neighbour");
            let again = navigate(&panes, &there, back, false).expect("back");
            assert_eq!(again, from, "{from} {direction:?}");
        }
    }
}
