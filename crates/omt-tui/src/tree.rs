//! Panes as a tree, so a split can be split.
//!
//! `tile` arranges N panes in one direction and divides the space evenly. That
//! is every arrangement a person cannot ask for: an editor beside two stacked
//! shells, a wide pane above three narrow ones, a split dragged to two thirds.
//! Those are not more of the same shape, they are a *tree* — each split holds
//! two children and either child may itself be a split.
//!
//! Every operation here is a pure function of the tree and the area. Nothing
//! touches a terminal, so the arrangement can be tested exactly, which matters
//! more here than almost anywhere else: an off-by-one in a rect is a column of
//! somebody's shell painted into their neighbour's, and it is invisible in a
//! byte stream.

use crate::layout::{MIN_PANE_COLS, MIN_PANE_ROWS, Rect, Split};

/// Which pane, within one tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PaneId(pub u32);

/// A node: either a pane, or a split holding two of them.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// A leaf.
    Pane(PaneId),
    /// A division of the space between two children.
    Split {
        /// Which way the space is cut.
        split: Split,
        /// How much of it the first child gets, 0..1.
        ratio: f32,
        /// Left, or top.
        first: Box<Node>,
        /// Right, or bottom.
        second: Box<Node>,
    },
}

/// Where a pane is on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    /// Which pane.
    pub pane: PaneId,
    /// Where it goes.
    pub rect: Rect,
}

/// One cell of separator between two panes.
const SEPARATOR: u16 = 1;

impl Node {
    /// A tree of one pane.
    #[must_use]
    pub const fn leaf(pane: PaneId) -> Self {
        Self::Pane(pane)
    }

    /// Every pane in the tree, in reading order.
    #[must_use]
    pub fn panes(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        self.walk(&mut |pane| out.push(pane));
        out
    }

    fn walk(&self, visit: &mut impl FnMut(PaneId)) {
        match self {
            Self::Pane(pane) => visit(*pane),
            Self::Split { first, second, .. } => {
                first.walk(visit);
                second.walk(visit);
            }
        }
    }

    /// How many panes there are.
    #[must_use]
    pub fn count(&self) -> usize {
        match self {
            Self::Pane(_) => 1,
            Self::Split { first, second, .. } => first.count() + second.count(),
        }
    }

    /// Where every pane goes, given the space.
    ///
    /// A split whose half would be below the minimum is not drawn as a split:
    /// the first child takes the whole area. Refusing to divide is better than
    /// dividing into two panes nobody can read, and better than an arrangement
    /// that silently loses a pane.
    #[must_use]
    pub fn place(&self, area: Rect) -> Vec<Placed> {
        let mut out = Vec::new();
        self.place_into(area, &mut out);
        out
    }

    fn place_into(&self, area: Rect, out: &mut Vec<Placed>) {
        match self {
            Self::Pane(pane) => out.push(Placed {
                pane: *pane,
                rect: area,
            }),
            Self::Split {
                split,
                ratio,
                first,
                second,
            } => {
                let Some((a, b)) = divide(area, *split, *ratio) else {
                    // No room to divide. The first child takes it all rather
                    // than both children taking something unusable.
                    first.place_into(area, out);
                    return;
                };
                first.place_into(a, out);
                second.place_into(b, out);
            }
        }
    }

    /// Split the pane holding `target`, putting `new` beside it.
    ///
    /// Returns whether anything changed, so a caller can tell "there was no
    /// room" from "done" without comparing trees.
    pub fn split(&mut self, target: PaneId, new: PaneId, split: Split) -> bool {
        match self {
            Self::Pane(pane) if *pane == target => {
                *self = Self::Split {
                    split,
                    ratio: 0.5,
                    first: Box::new(Self::Pane(target)),
                    second: Box::new(Self::Pane(new)),
                };
                true
            }
            Self::Pane(_) => false,
            Self::Split { first, second, .. } => {
                first.split(target, new, split) || second.split(target, new, split)
            }
        }
    }

    /// Remove a pane, collapsing the split that held it.
    ///
    /// Returns `None` when the tree was a single pane and that pane was
    /// removed: an empty tree is not representable, and a caller closing the
    /// last pane is closing the window.
    #[must_use]
    pub fn close(self, target: PaneId) -> Option<Self> {
        match self {
            Self::Pane(pane) if pane == target => None,
            leaf @ Self::Pane(_) => Some(leaf),
            Self::Split {
                split,
                ratio,
                first,
                second,
            } => match (first.close(target), second.close(target)) {
                // The sibling takes the space, which is what makes closing a
                // pane feel like closing a pane rather than rearranging the
                // whole screen.
                (None, Some(kept)) | (Some(kept), None) => Some(kept),
                (Some(a), Some(b)) => Some(Self::Split {
                    split,
                    ratio,
                    first: Box::new(a),
                    second: Box::new(b),
                }),
                (None, None) => None,
            },
        }
    }

    /// Move a split's divider, by a fraction.
    ///
    /// The split moved is the nearest one above `target` that divides in
    /// `split`'s direction: "make this pane wider" means the vertical divider
    /// next to it, whichever level of the tree that happens to live at.
    pub fn resize(&mut self, target: PaneId, split: Split, by: f32) -> bool {
        match self {
            Self::Pane(_) => false,
            Self::Split {
                split: this,
                ratio,
                first,
                second,
            } => {
                if first.resize(target, split, by) || second.resize(target, split, by) {
                    return true;
                }
                if *this != split {
                    return false;
                }
                let holds_first = first.panes().contains(&target);
                let holds_second = second.panes().contains(&target);
                if !holds_first && !holds_second {
                    return false;
                }
                // Widening the second child means moving the divider the other
                // way, which is the thing that is wrong in every hand-written
                // version of this.
                let delta = if holds_first { by } else { -by };
                // Clamped rather than free: a ratio of zero is a pane that
                // exists and cannot be seen, which is worse than one that
                // refuses to shrink further.
                //
                // The bound cannot be a fixed fraction, because the minimum is
                // in *cells*: nine tenths of two hundred columns leaves twenty
                // for the other pane, which is below the minimum, and a split
                // that cannot be drawn is a pane that vanishes. The caller
                // does not know the area here, so this keeps the ratio in a
                // range that is safe at any size and `divide` refuses what is
                // left — the two together are what make a pane impossible to
                // squeeze out.
                *ratio = (*ratio + delta).clamp(0.2, 0.8);
                true
            }
        }
    }
}

/// The pane in a direction from another, by where they are on screen.
///
/// Geometric rather than structural: "the pane to the right" means the one a
/// finger would land on, not the tree's other child. Those differ as soon as
/// the tree is more than one level deep, and the structural answer is the one
/// that feels broken.
#[must_use]
pub fn neighbour(placed: &[Placed], from: PaneId, towards: Towards) -> Option<PaneId> {
    let here = placed.iter().find(|p| p.pane == from)?.rect;
    let centre_row = here.row + here.rows / 2;
    let centre_col = here.col + here.cols / 2;

    placed
        .iter()
        .filter(|p| p.pane != from)
        .filter(|p| match towards {
            Towards::Left => p.rect.col + p.rect.cols <= here.col,
            Towards::Right => p.rect.col >= here.col + here.cols,
            Towards::Up => p.rect.row + p.rect.rows <= here.row,
            Towards::Down => p.rect.row >= here.row + here.rows,
        })
        // Among the panes on that side, the one whose span covers this pane's
        // centre line, and failing that the nearest to it.
        .min_by_key(|p| {
            let (near, off) = match towards {
                Towards::Left => (
                    here.col.saturating_sub(p.rect.col + p.rect.cols),
                    offset(centre_row, p.rect.row, p.rect.rows),
                ),
                Towards::Right => (
                    p.rect.col.saturating_sub(here.col + here.cols),
                    offset(centre_row, p.rect.row, p.rect.rows),
                ),
                Towards::Up => (
                    here.row.saturating_sub(p.rect.row + p.rect.rows),
                    offset(centre_col, p.rect.col, p.rect.cols),
                ),
                Towards::Down => (
                    p.rect.row.saturating_sub(here.row + here.rows),
                    offset(centre_col, p.rect.col, p.rect.cols),
                ),
            };
            (near, off)
        })
        .map(|p| p.pane)
}

/// Which way to move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Towards {
    /// Leftwards.
    Left,
    /// Rightwards.
    Right,
    /// Upwards.
    Up,
    /// Downwards.
    Down,
}

/// How far a line is from a span, zero when inside it.
fn offset(line: u16, start: u16, len: u16) -> u16 {
    if line < start {
        start - line
    } else if line >= start + len {
        line - (start + len) + 1
    } else {
        0
    }
}

/// Cut an area in two, or refuse.
fn divide(area: Rect, split: Split, ratio: f32) -> Option<(Rect, Rect)> {
    let ratio = ratio.clamp(0.1, 0.9);
    match split {
        Split::Vertical => {
            let usable = area.cols.checked_sub(SEPARATOR)?;
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss,
                reason = "a pane count fits a u16 and the ratio is clamped"
            )]
            let first = (f32::from(usable) * ratio).round() as u16;
            let second = usable.checked_sub(first)?;
            if first < MIN_PANE_COLS || second < MIN_PANE_COLS {
                // Not a refusal to split — the split exists — but a refusal to
                // draw it at this ratio. Pinned to the nearest arrangement
                // that fits, so dragging a divider stops at the edge instead
                // of making a pane disappear.
                if usable >= MIN_PANE_COLS * 2 {
                    let first = first.clamp(MIN_PANE_COLS, usable - MIN_PANE_COLS);
                    return Some((
                        Rect {
                            cols: first,
                            ..area
                        },
                        Rect {
                            col: area.col + first + SEPARATOR,
                            cols: usable - first,
                            ..area
                        },
                    ));
                }
                return None;
            }
            Some((
                Rect {
                    cols: first,
                    ..area
                },
                Rect {
                    col: area.col + first + SEPARATOR,
                    cols: second,
                    ..area
                },
            ))
        }
        Split::Horizontal => {
            let usable = area.rows.checked_sub(SEPARATOR)?;
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss,
                reason = "a pane count fits a u16 and the ratio is clamped"
            )]
            let first = (f32::from(usable) * ratio).round() as u16;
            let second = usable.checked_sub(first)?;
            if first < MIN_PANE_ROWS || second < MIN_PANE_ROWS {
                if usable >= MIN_PANE_ROWS * 2 {
                    let first = first.clamp(MIN_PANE_ROWS, usable - MIN_PANE_ROWS);
                    return Some((
                        Rect {
                            rows: first,
                            ..area
                        },
                        Rect {
                            row: area.row + first + SEPARATOR,
                            rows: usable - first,
                            ..area
                        },
                    ));
                }
                return None;
            }
            Some((
                Rect {
                    rows: first,
                    ..area
                },
                Rect {
                    row: area.row + first + SEPARATOR,
                    rows: second,
                    ..area
                },
            ))
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect {
        col: 0,
        row: 0,
        cols: 200,
        rows: 50,
    };

    fn p(n: u32) -> PaneId {
        PaneId(n)
    }

    fn rect_of(placed: &[Placed], pane: PaneId) -> Rect {
        placed
            .iter()
            .find(|x| x.pane == pane)
            .expect("the pane was placed")
            .rect
    }

    #[test]
    fn one_pane_takes_the_screen() {
        let tree = Node::leaf(p(1));
        assert_eq!(
            tree.place(SCREEN),
            vec![Placed {
                pane: p(1),
                rect: SCREEN
            }]
        );
    }

    #[test]
    fn a_split_leaves_exactly_one_column_between_the_halves() {
        // The separator is drawn in that column. A pane one cell too wide
        // paints over it; one cell too narrow leaves a gap that reads as a
        // rendering fault.
        let mut tree = Node::leaf(p(1));
        assert!(tree.split(p(1), p(2), Split::Vertical));
        let placed = tree.place(SCREEN);
        let left = rect_of(&placed, p(1));
        let right = rect_of(&placed, p(2));
        assert_eq!(right.col, left.col + left.cols + 1);
        assert_eq!(left.cols + right.cols + 1, SCREEN.cols);
        assert_eq!(left.rows, SCREEN.rows);
    }

    #[test]
    fn no_two_panes_ever_overlap() {
        // The failure this whole module has to make impossible: one shell's
        // output painted into another's column, which no byte-level test can
        // see.
        let mut tree = Node::leaf(p(1));
        tree.split(p(1), p(2), Split::Vertical);
        tree.split(p(2), p(3), Split::Horizontal);
        tree.split(p(1), p(4), Split::Horizontal);
        tree.split(p(4), p(5), Split::Vertical);

        let placed = tree.place(SCREEN);
        assert_eq!(placed.len(), 5);
        for (i, a) in placed.iter().enumerate() {
            for b in &placed[i + 1..] {
                let apart = a.rect.col + a.rect.cols <= b.rect.col
                    || b.rect.col + b.rect.cols <= a.rect.col
                    || a.rect.row + a.rect.rows <= b.rect.row
                    || b.rect.row + b.rect.rows <= a.rect.row;
                assert!(apart, "{:?} overlaps {:?}", a, b);
            }
        }
    }

    #[test]
    fn every_pane_stays_inside_the_screen() {
        let mut tree = Node::leaf(p(1));
        for n in 2..=6 {
            tree.split(
                p(1),
                p(n),
                if n % 2 == 0 {
                    Split::Vertical
                } else {
                    Split::Horizontal
                },
            );
        }
        for placed in tree.place(SCREEN) {
            assert!(
                placed.rect.col + placed.rect.cols <= SCREEN.cols,
                "{placed:?}"
            );
            assert!(
                placed.rect.row + placed.rect.rows <= SCREEN.rows,
                "{placed:?}"
            );
        }
    }

    #[test]
    fn a_split_with_no_room_is_not_made() {
        // Two panes nobody can read is worse than one somebody can, and worse
        // than a pane that silently vanished.
        let narrow = Rect {
            cols: 50,
            rows: 10,
            ..SCREEN
        };
        let mut tree = Node::leaf(p(1));
        tree.split(p(1), p(2), Split::Vertical);
        let placed = tree.place(narrow);
        assert_eq!(
            placed.len(),
            1,
            "it divided space it did not have: {placed:?}"
        );
        assert_eq!(placed[0].rect, narrow);
    }

    #[test]
    fn closing_a_pane_gives_its_space_to_its_sibling() {
        // Not to the whole screen, and not by rearranging everything: closing
        // a pane should feel like closing a pane.
        let mut tree = Node::leaf(p(1));
        tree.split(p(1), p(2), Split::Vertical);
        tree.split(p(2), p(3), Split::Horizontal);
        let before = rect_of(&tree.place(SCREEN), p(1));

        let tree = tree.close(p(3)).expect("two panes remain");
        let placed = tree.place(SCREEN);
        assert_eq!(placed.len(), 2);
        assert_eq!(rect_of(&placed, p(1)), before, "an unrelated pane moved");
        assert_eq!(rect_of(&placed, p(2)).rows, SCREEN.rows);
    }

    #[test]
    fn closing_the_last_pane_leaves_no_tree() {
        // An empty tree is not representable, and a caller closing the last
        // pane is closing the window.
        assert_eq!(Node::leaf(p(1)).close(p(1)), None);
    }

    #[test]
    fn closing_something_that_is_not_there_changes_nothing() {
        let mut tree = Node::leaf(p(1));
        tree.split(p(1), p(2), Split::Vertical);
        let same = tree.clone().close(p(99)).expect("nothing was closed");
        assert_eq!(same, tree);
    }

    #[test]
    fn resizing_moves_the_divider_the_way_the_pane_grows() {
        // The thing that is wrong in every hand-written version: widening the
        // *second* child means moving the divider the other way.
        let mut tree = Node::leaf(p(1));
        tree.split(p(1), p(2), Split::Vertical);
        let before = rect_of(&tree.place(SCREEN), p(2));

        assert!(tree.resize(p(2), Split::Vertical, 0.2));
        let after = rect_of(&tree.place(SCREEN), p(2));
        assert!(
            after.cols > before.cols,
            "asked for a wider pane and got {} from {}",
            after.cols,
            before.cols
        );
    }

    #[test]
    fn a_divider_never_moves_far_enough_to_hide_a_pane() {
        // A ratio of zero is a pane that exists and cannot be seen, which is
        // worse than one that refuses to shrink further.
        let mut tree = Node::leaf(p(1));
        tree.split(p(1), p(2), Split::Vertical);
        for _ in 0..20 {
            tree.resize(p(1), Split::Vertical, 0.5);
        }
        let placed = tree.place(SCREEN);
        assert_eq!(placed.len(), 2, "a pane was squeezed out of existence");
        assert!(rect_of(&placed, p(2)).cols >= MIN_PANE_COLS);
    }

    #[test]
    fn moving_right_lands_where_a_finger_would() {
        // Geometric, not structural. With a split inside a split, the tree's
        // "other child" and the pane actually to the right are different
        // panes, and the structural answer is the one that feels broken.
        let mut tree = Node::leaf(p(1));
        tree.split(p(1), p(2), Split::Vertical);
        tree.split(p(2), p(3), Split::Horizontal);
        let placed = tree.place(SCREEN);

        // 1 is the whole left column; 2 and 3 are stacked on the right.
        assert_eq!(neighbour(&placed, p(1), Towards::Right), Some(p(2)));
        assert_eq!(neighbour(&placed, p(2), Towards::Left), Some(p(1)));
        assert_eq!(neighbour(&placed, p(2), Towards::Down), Some(p(3)));
        assert_eq!(neighbour(&placed, p(3), Towards::Up), Some(p(2)));
    }

    #[test]
    fn there_is_nothing_past_the_edge() {
        let mut tree = Node::leaf(p(1));
        tree.split(p(1), p(2), Split::Vertical);
        let placed = tree.place(SCREEN);
        assert_eq!(neighbour(&placed, p(1), Towards::Left), None);
        assert_eq!(neighbour(&placed, p(2), Towards::Right), None);
        assert_eq!(neighbour(&placed, p(1), Towards::Up), None);
    }

    #[test]
    fn moving_sideways_stays_on_the_row_it_started_on() {
        // Four panes in a two-by-two. From the top left, "right" is the top
        // right — never the bottom right, which is the answer a structural
        // walk of the tree can give and the one that feels broken.
        let mut tree = Node::leaf(p(1));
        tree.split(p(1), p(2), Split::Vertical);
        tree.split(p(1), p(3), Split::Horizontal);
        tree.split(p(2), p(4), Split::Horizontal);
        let placed = tree.place(SCREEN);

        assert_eq!(neighbour(&placed, p(1), Towards::Right), Some(p(2)));
        assert_eq!(neighbour(&placed, p(3), Towards::Right), Some(p(4)));
        assert_eq!(neighbour(&placed, p(4), Towards::Left), Some(p(3)));
        assert_eq!(neighbour(&placed, p(2), Towards::Down), Some(p(4)));
    }

    #[test]
    fn the_panes_come_back_in_reading_order() {
        // Left to right, top to bottom — the order a person would number them,
        // which is the order a list of them has to be in.
        let mut tree = Node::leaf(p(1));
        tree.split(p(1), p(2), Split::Vertical);
        tree.split(p(1), p(3), Split::Horizontal);
        assert_eq!(tree.panes(), vec![p(1), p(3), p(2)]);
        assert_eq!(tree.count(), 3);
    }
}
