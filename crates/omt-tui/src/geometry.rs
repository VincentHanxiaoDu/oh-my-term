//! Turning a split tree into rectangles.
//!
//! `docs/architecture/17-panes-and-layout.md` §2. The only place geometry
//! exists — every other part of omt reads placements rather than computing
//! them, because two pieces of code that both work out where a pane is will
//! eventually disagree, and the disagreement shows up as a pane drawing over
//! its neighbour.
//!
//! The interesting part is what happens when the panes do not fit. A phone in
//! portrait is a hundred and twenty cells tall and thirty wide, and a layout
//! built on a desktop will not fit in it. Three ways to handle that:
//!
//! - Shrink everything below usability. Every pane becomes four columns wide
//!   and none of them is readable, which is the worst outcome because it looks
//!   like it worked.
//! - Fail. Nothing renders, on a screen the user is holding.
//! - **Drop panes that cannot be placed at their minimum, and say which.**
//!
//! The third is what this does. `hidden` names them and `degraded` explains,
//! so a client can tell somebody "two panes do not fit here" instead of showing
//! them a grid of unreadable slivers.

/// A rectangle in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Column of the left edge.
    pub left: u16,
    /// Row of the top edge.
    pub top: u16,
    /// How wide.
    pub width: u16,
    /// How tall.
    pub height: u16,
}

impl Rect {
    /// One past the right edge.
    #[must_use]
    pub const fn right(self) -> u16 {
        self.left + self.width
    }

    /// One past the bottom edge.
    #[must_use]
    pub const fn bottom(self) -> u16 {
        self.top + self.height
    }
}

/// Which way a node splits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Split {
    /// Children side by side.
    Vertical,
    /// Children stacked.
    Horizontal,
}

/// A split tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// A pane.
    Leaf {
        /// Which pane.
        id: String,
    },
    /// A split, with the fraction the first child takes.
    Split {
        /// Which way.
        direction: Split,
        /// How much of the space the first child gets, 0..1.
        ratio: f32,
        /// The first child.
        first: Box<Node>,
        /// The second.
        second: Box<Node>,
    },
}

/// One placed pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    /// Which pane.
    pub pane: String,
    /// Where it is, including any title bar, excluding dividers.
    pub outer: Rect,
    /// Its stable pre-order index.
    pub index: u32,
}

/// Why some panes were not placed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Degradation {
    /// How many were dropped.
    pub dropped: usize,
    /// What to tell somebody.
    pub reason: String,
}

/// The result of laying out a tree.
#[derive(Debug, Clone, PartialEq)]
pub struct Geometry {
    /// Every pane that received a rect, in stable pre-order.
    pub panes: Vec<Placement>,
    /// Panes that exist in the tree but could not be placed at their minimum.
    pub hidden: Vec<String>,
    /// Set when anything was hidden, so a client can explain itself.
    pub degraded: Option<Degradation>,
}

/// The smallest a pane may be and still be worth drawing.
///
/// Twenty by three: narrower than twenty and a shell prompt wraps on itself;
/// shorter than three and there is no room for a line of output under a
/// command. Below that a pane is not a small pane, it is a decoration.
pub const MIN_WIDTH: u16 = 20;
/// Likewise.
pub const MIN_HEIGHT: u16 = 3;

/// Lay a tree out inside a rectangle.
///
/// Panes are visited in pre-order, so `index` is stable across recomputation —
/// which is what lets focus navigation break ties by something that does not
/// change when the window is resized.
#[must_use]
pub fn compute(tree: &Node, area: Rect) -> Geometry {
    let mut panes: Vec<Placement> = Vec::new();
    let mut hidden: Vec<String> = Vec::new();
    let mut index = 0u32;
    place(tree, area, &mut panes, &mut hidden, &mut index);

    // If the split left nothing placeable, show the first pane full-screen.
    //
    // Found by a test at thirty columns: four even splits give each pane seven
    // columns, all of them below the minimum, and the result was a screen with
    // nothing on it. That is the "fail" outcome this module's own comment
    // rejects — on a phone somebody is holding, an empty screen is worse than
    // any arrangement. One readable pane and an honest count of what is hidden
    // is the floor.
    if panes.is_empty()
        && !hidden.is_empty()
        && area.width >= MIN_WIDTH
        && area.height >= MIN_HEIGHT
    {
        let first = hidden.remove(0);
        panes.push(Placement {
            pane: first,
            outer: area,
            index: 0,
        });
    }

    let degraded = if hidden.is_empty() {
        None
    } else {
        Some(Degradation {
            dropped: hidden.len(),
            // Named rather than counted only: "2 panes hidden" leaves somebody
            // hunting for which, and the answer is not on screen by definition.
            reason: format!(
                "{} pane{} will not fit at {}×{}: {}",
                hidden.len(),
                if hidden.len() == 1 { "" } else { "s" },
                area.width,
                area.height,
                hidden.join(", ")
            ),
        })
    };
    Geometry {
        panes,
        hidden,
        degraded,
    }
}

fn place(
    node: &Node,
    area: Rect,
    panes: &mut Vec<Placement>,
    hidden: &mut Vec<String>,
    index: &mut u32,
) {
    match node {
        Node::Leaf { id } => {
            let here = *index;
            *index += 1;
            if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
                hidden.push(id.clone());
            } else {
                panes.push(Placement {
                    pane: id.clone(),
                    outer: area,
                    index: here,
                });
            }
        }
        Node::Split {
            direction,
            ratio,
            first,
            second,
        } => {
            let ratio = ratio.clamp(0.05, 0.95);
            match direction {
                Split::Vertical => {
                    // Rounded rather than truncated, and the second child takes
                    // the remainder. Truncating both would lose a column to
                    // rounding and leave a one-cell gap nothing draws into.
                    #[allow(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        clippy::cast_precision_loss,
                        reason = "cell counts are small and non-negative"
                    )]
                    let left_width = (f32::from(area.width) * ratio).round() as u16;
                    let left_width = left_width.min(area.width);
                    place(
                        first,
                        Rect {
                            width: left_width,
                            ..area
                        },
                        panes,
                        hidden,
                        index,
                    );
                    place(
                        second,
                        Rect {
                            left: area.left + left_width,
                            width: area.width - left_width,
                            ..area
                        },
                        panes,
                        hidden,
                        index,
                    );
                }
                Split::Horizontal => {
                    #[allow(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        clippy::cast_precision_loss,
                        reason = "cell counts are small and non-negative"
                    )]
                    let top_height = (f32::from(area.height) * ratio).round() as u16;
                    let top_height = top_height.min(area.height);
                    place(
                        first,
                        Rect {
                            height: top_height,
                            ..area
                        },
                        panes,
                        hidden,
                        index,
                    );
                    place(
                        second,
                        Rect {
                            top: area.top + top_height,
                            height: area.height - top_height,
                            ..area
                        },
                        panes,
                        hidden,
                        index,
                    );
                }
            }
        }
    }
}

/// A named arrangement.
///
/// `docs/architecture/17-panes-and-layout.md` §4. Presets exist because the
/// arrangement somebody wants is almost never worth building split by split,
/// and because a layout that can be named can be restored — which is what makes
/// §10's persistence possible at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// Every pane the same width, side by side.
    EvenVertical,
    /// Every pane the same height, stacked.
    EvenHorizontal,
    /// One large pane on the left, the rest stacked on the right.
    MainVertical,
    /// One large pane on top, the rest side by side below.
    MainHorizontal,
}

/// Build a tree of `panes` in a preset arrangement.
///
/// Returns `None` for no panes: an empty layout is not a layout, and returning
/// an empty tree would push the question onto every caller.
#[must_use]
pub fn preset(kind: Preset, panes: &[String]) -> Option<Node> {
    let (first, rest) = panes.split_first()?;
    if rest.is_empty() {
        return Some(Node::Leaf { id: first.clone() });
    }
    Some(match kind {
        Preset::EvenVertical => even(Split::Vertical, panes),
        Preset::EvenHorizontal => even(Split::Horizontal, panes),
        // The main pane keeps half regardless of how many others there are.
        // Proportional would shrink it as panes are added, which is backwards:
        // the main pane is the one being worked in, and adding a log tail
        // should not make the thing you are reading narrower.
        Preset::MainVertical => Node::Split {
            direction: Split::Vertical,
            ratio: 0.5,
            first: Box::new(Node::Leaf { id: first.clone() }),
            second: Box::new(even(Split::Horizontal, rest)),
        },
        Preset::MainHorizontal => Node::Split {
            direction: Split::Horizontal,
            ratio: 0.5,
            first: Box::new(Node::Leaf { id: first.clone() }),
            second: Box::new(even(Split::Vertical, rest)),
        },
    })
}

/// Split `panes` evenly along one axis.
///
/// Built as a right-leaning chain with a decreasing ratio rather than by
/// halving, so five panes are five equal fifths and not 1/2, 1/4, 1/8, 1/16,
/// 1/16 — which is what naive recursive halving gives and looks nothing like
/// "even".
fn even(direction: Split, panes: &[String]) -> Node {
    match panes.split_first() {
        None | Some((_, [])) => Node::Leaf {
            id: panes.first().cloned().unwrap_or_default(),
        },
        Some((first, rest)) => {
            #[allow(clippy::cast_precision_loss, reason = "pane counts are small")]
            let ratio = 1.0 / panes.len() as f32;
            Node::Split {
                direction,
                ratio,
                first: Box::new(Node::Leaf { id: first.clone() }),
                second: Box::new(even(direction, rest)),
            }
        }
    }
}

/// Lay out with one pane zoomed to fill the area.
///
/// §5. Zoom is a *view* state, not a tree edit: the tree is untouched and
/// unzooming is not an undo. Editing the tree would mean a zoom that survives
/// into a saved layout, and somebody restoring it would find their other panes
/// gone with no way to tell that zoom was why.
///
/// Every other pane is reported as hidden, which is true — they are not drawn —
/// and means a client showing "n hidden" needs no special case for zoom.
#[must_use]
pub fn compute_zoomed(tree: &Node, area: Rect, zoomed: &str) -> Geometry {
    let full = compute(tree, area);
    let known =
        full.panes.iter().any(|p| p.pane == zoomed) || full.hidden.iter().any(|h| h == zoomed);
    if !known {
        return full;
    }
    let index = full
        .panes
        .iter()
        .find(|p| p.pane == zoomed)
        .map_or(0, |p| p.index);
    let hidden: Vec<String> = full
        .panes
        .iter()
        .map(|p| p.pane.clone())
        .chain(full.hidden.iter().cloned())
        .filter(|id| id != zoomed)
        .collect();
    Geometry {
        panes: vec![Placement {
            pane: zoomed.to_owned(),
            outer: area,
            index,
        }],
        degraded: if hidden.is_empty() {
            None
        } else {
            Some(Degradation {
                dropped: hidden.len(),
                reason: format!("`{zoomed}` is zoomed; {} pane(s) hidden", hidden.len()),
            })
        },
        hidden,
    }
}

// ---------------------------------------------------------------------------
// Floats, stacks, and moving panes — §5
// ---------------------------------------------------------------------------

/// A pane that sits above the layout rather than in it.
///
/// Floats exist for the things that are not part of the arrangement: a scratch
/// shell, a quick `git log`. They are clamped into the area rather than
/// scrolled to, because a float that opens partly off-screen on a narrow client
/// is a float somebody cannot close.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Float {
    /// Which pane.
    pub pane: String,
    /// Where it would like to be.
    pub rect: Rect,
}

/// Several panes sharing one slot, one of them visible.
///
/// A stack is not a layout of its own — it occupies exactly one leaf's rect, so
/// collapsing or expanding one never reflows anything around it. That is the
/// property worth having: a stack that resized its neighbours would move the
/// pane somebody is reading every time they cycled a background one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stack {
    /// The leaf whose rect the stack occupies.
    pub at: String,
    /// Every member, in order.
    pub members: Vec<String>,
    /// Which member is showing.
    pub visible: usize,
}

/// Place floats and stacks on top of a computed geometry.
///
/// Floats are clamped to the area and dropped if they cannot meet the minimum,
/// through the same rule as any other pane — a float is not exempt from being
/// too small to read.
#[must_use]
pub fn overlay(base: Geometry, area: Rect, floats: &[Float], stacks: &[Stack]) -> Geometry {
    let mut panes = base.panes;
    let mut hidden = base.hidden;

    for stack in stacks {
        let Some(slot) = panes.iter().position(|p| p.pane == stack.at) else {
            continue;
        };
        let Some(showing) = stack.members.get(stack.visible) else {
            continue;
        };
        let rect = panes[slot].outer;
        let index = panes[slot].index;
        panes[slot] = Placement {
            pane: showing.clone(),
            outer: rect,
            index,
        };
        // The rest are hidden rather than absent: somebody has to be able to
        // see that four things are in this stack and one is showing.
        hidden.extend(stack.members.iter().filter(|m| *m != showing).cloned());
    }

    for float in floats {
        let clamped = clamp(float.rect, area);
        if clamped.width < MIN_WIDTH || clamped.height < MIN_HEIGHT {
            hidden.push(float.pane.clone());
            continue;
        }
        // Floats are appended, so they are drawn last and therefore on top.
        // Order in this list is the z-order, which is the simplest thing that
        // is also true.
        let index = u32::try_from(panes.len()).unwrap_or(u32::MAX);
        panes.push(Placement {
            pane: float.pane.clone(),
            outer: clamped,
            index,
        });
    }

    let degraded = if hidden.is_empty() {
        None
    } else {
        Some(Degradation {
            dropped: hidden.len(),
            reason: format!("{} pane(s) not drawn: {}", hidden.len(), hidden.join(", ")),
        })
    };
    Geometry {
        panes,
        hidden,
        degraded,
    }
}

/// Pull a rectangle inside another, preserving its size where it can.
fn clamp(rect: Rect, area: Rect) -> Rect {
    let width = rect.width.min(area.width);
    let height = rect.height.min(area.height);
    Rect {
        left: rect
            .left
            .min(area.right().saturating_sub(width))
            .max(area.left),
        top: rect
            .top
            .min(area.bottom().saturating_sub(height))
            .max(area.top),
        width,
        height,
    }
}

/// Swap two panes' positions in a tree.
///
/// A swap rather than a cut-and-paste. Moving a pane by removing and
/// re-inserting it changes the shape of the tree around the hole, so the panes
/// that did not move end up somewhere new — which is not what anybody means by
/// "move this pane there".
#[must_use]
pub fn swap(tree: &Node, one: &str, other: &str) -> Node {
    // Both have to exist. Without this check, swapping with a name that is not
    // in the tree *renames* the pane that is — it silently becomes the missing
    // one, and the session it was showing is still running with nothing
    // pointing at it.
    if one == other || !contains(tree, one) || !contains(tree, other) {
        return tree.clone();
    }
    swap_unchecked(tree, one, other)
}

/// Whether a pane is anywhere in a tree.
#[must_use]
pub fn contains(tree: &Node, id: &str) -> bool {
    match tree {
        Node::Leaf { id: leaf } => leaf == id,
        Node::Split { first, second, .. } => contains(first, id) || contains(second, id),
    }
}

fn swap_unchecked(tree: &Node, one: &str, other: &str) -> Node {
    match tree {
        Node::Leaf { id } => Node::Leaf {
            id: if id == one {
                other.to_owned()
            } else if id == other {
                one.to_owned()
            } else {
                id.clone()
            },
        },
        Node::Split {
            direction,
            ratio,
            first,
            second,
        } => Node::Split {
            direction: *direction,
            ratio: *ratio,
            first: Box::new(swap_unchecked(first, one, other)),
            second: Box::new(swap_unchecked(second, one, other)),
        },
    }
}

// ---------------------------------------------------------------------------
// Saving and restoring a layout — §10
// ---------------------------------------------------------------------------

/// A layout, in a form that survives a restart.
///
/// The tree and nothing else. Rects are not stored, because they are a function
/// of the tree and the screen — saving them would restore a desktop layout onto
/// a phone as a desktop layout, which is the bug §3 exists to prevent.
///
/// Zoom is not stored either. It is a view state, and a saved zoom would have
/// somebody restore a layout and find one pane where four should be.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Saved {
    /// What it is called.
    pub name: String,
    /// The tree, flattened.
    pub tree: SavedNode,
}

/// A tree, in a form serde can carry.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SavedNode {
    /// A pane.
    Leaf {
        /// Which pane.
        id: String,
    },
    /// A split.
    Split {
        /// `vertical` or `horizontal`.
        direction: String,
        /// The first child's share.
        ratio: f32,
        /// The first child.
        first: Box<SavedNode>,
        /// The second.
        second: Box<SavedNode>,
    },
}

impl From<&Node> for SavedNode {
    fn from(node: &Node) -> Self {
        match node {
            Node::Leaf { id } => Self::Leaf { id: id.clone() },
            Node::Split {
                direction,
                ratio,
                first,
                second,
            } => Self::Split {
                direction: match direction {
                    Split::Vertical => "vertical".to_owned(),
                    Split::Horizontal => "horizontal".to_owned(),
                },
                ratio: *ratio,
                first: Box::new(Self::from(first.as_ref())),
                second: Box::new(Self::from(second.as_ref())),
            },
        }
    }
}

impl From<&SavedNode> for Node {
    fn from(node: &SavedNode) -> Self {
        match node {
            SavedNode::Leaf { id } => Self::Leaf { id: id.clone() },
            SavedNode::Split {
                direction,
                ratio,
                first,
                second,
            } => Self::Split {
                // An unrecognised direction becomes vertical rather than
                // failing the whole restore. A layout file from a future
                // version should give back the panes it can, not nothing.
                direction: if direction == "horizontal" {
                    Split::Horizontal
                } else {
                    Split::Vertical
                },
                ratio: *ratio,
                first: Box::new(Self::from(first.as_ref())),
                second: Box::new(Self::from(second.as_ref())),
            },
        }
    }
}

/// Every pane named in a saved layout, in pre-order.
#[must_use]
pub fn panes_of(node: &SavedNode) -> Vec<String> {
    match node {
        SavedNode::Leaf { id } => vec![id.clone()],
        SavedNode::Split { first, second, .. } => {
            let mut out = panes_of(first);
            out.extend(panes_of(second));
            out
        }
    }
}

/// Restore a saved layout, keeping only the panes that still exist.
///
/// A layout is a description of sessions that were running, and by the time it
/// is restored some of them are not. Dropping the missing ones and rebuilding
/// the rest is the only outcome that gives somebody back what they still have —
/// failing gives them nothing, and keeping the names gives them panes showing
/// dead sessions they then have to close one by one.
#[must_use]
pub fn restore(saved: &Saved, alive: &[String]) -> Option<(Node, Vec<String>)> {
    let named = panes_of(&saved.tree);
    let missing: Vec<String> = named
        .iter()
        .filter(|id| !alive.contains(id))
        .cloned()
        .collect();
    let surviving: Vec<String> = named.into_iter().filter(|id| alive.contains(id)).collect();
    if surviving.is_empty() {
        return None;
    }
    // Rebuilt rather than pruned in place: pruning leaves splits with one
    // child, and a split with one child renders as a pane that has mysteriously
    // stopped filling its space.
    let tree = if surviving.len() == panes_of(&saved.tree).len() {
        Node::from(&saved.tree)
    } else {
        preset(Preset::EvenVertical, &surviving)?
    };
    Some((tree, missing))
}

// ---------------------------------------------------------------------------
// Rendering: dividers and edges — §8
// ---------------------------------------------------------------------------

/// The line between two panes, addressable so it can be dragged.
///
/// Derived from the placements rather than tracked alongside them. A divider
/// stored separately is one that can disagree with the panes it sits between,
/// and the disagreement is a drag handle a few cells away from the line it
/// appears to be on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Divider {
    /// Which pane is before it.
    pub before: String,
    /// Which pane is after it.
    pub after: String,
    /// Whether it runs top-to-bottom.
    pub vertical: bool,
    /// Where the line is: the column for a vertical divider, the row for a
    /// horizontal one.
    pub at: u16,
    /// Where it starts along its own axis.
    pub from: u16,
    /// Where it ends.
    pub to: u16,
}

/// Which sides of a pane touch the outer edge of the screen.
///
/// Border drawing needs this: a pane against the screen edge draws no border
/// there, and one that did would waste a column that is already scarce on a
/// phone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edges {
    /// Touches the left edge.
    pub left: bool,
    /// Touches the top.
    pub top: bool,
    /// Touches the right.
    pub right: bool,
    /// Touches the bottom.
    pub bottom: bool,
}

/// Which sides of a placement touch the container.
#[must_use]
pub fn edges(placement: &Placement, area: Rect) -> Edges {
    Edges {
        left: placement.outer.left == area.left,
        top: placement.outer.top == area.top,
        right: placement.outer.right() == area.right(),
        bottom: placement.outer.bottom() == area.bottom(),
    }
}

/// Every divider implied by a set of placements.
#[must_use]
pub fn dividers(geometry: &Geometry) -> Vec<Divider> {
    let mut out = Vec::new();
    for (i, one) in geometry.panes.iter().enumerate() {
        for other in &geometry.panes[i + 1..] {
            let a = one.outer;
            let b = other.outer;
            // Vertical: they share a column boundary and overlap in rows.
            if a.right() == b.left && a.top < b.bottom() && b.top < a.bottom() {
                out.push(Divider {
                    before: one.pane.clone(),
                    after: other.pane.clone(),
                    vertical: true,
                    at: a.right(),
                    from: a.top.max(b.top),
                    to: a.bottom().min(b.bottom()),
                });
            } else if b.right() == a.left && b.top < a.bottom() && a.top < b.bottom() {
                out.push(Divider {
                    before: other.pane.clone(),
                    after: one.pane.clone(),
                    vertical: true,
                    at: b.right(),
                    from: a.top.max(b.top),
                    to: a.bottom().min(b.bottom()),
                });
            } else if a.bottom() == b.top && a.left < b.right() && b.left < a.right() {
                out.push(Divider {
                    before: one.pane.clone(),
                    after: other.pane.clone(),
                    vertical: false,
                    at: a.bottom(),
                    from: a.left.max(b.left),
                    to: a.right().min(b.right()),
                });
            } else if b.bottom() == a.top && b.left < a.right() && a.left < b.right() {
                out.push(Divider {
                    before: other.pane.clone(),
                    after: one.pane.clone(),
                    vertical: false,
                    at: b.bottom(),
                    from: a.left.max(b.left),
                    to: a.right().min(b.right()),
                });
            }
        }
    }
    out.sort_by(|x, y| {
        x.vertical
            .cmp(&y.vertical)
            .then(x.at.cmp(&y.at))
            .then(x.from.cmp(&y.from))
    });
    out
}

/// Which divider a point is on, if any.
///
/// The tolerance is asymmetric on purpose: a divider is one cell wide and a
/// finger is not, so the hit area extends by `slop` on each side. Without it a
/// divider on a touch screen is a line nobody can grab.
#[must_use]
pub fn divider_at(dividers: &[Divider], column: u16, row: u16, slop: u16) -> Option<&Divider> {
    dividers.iter().find(|d| {
        if d.vertical {
            column.abs_diff(d.at) <= slop && row >= d.from && row < d.to
        } else {
            row.abs_diff(d.at) <= slop && column >= d.from && column < d.to
        }
    })
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;
    #[test]
    fn a_split_produces_one_divider_between_its_children() {
        let g = compute(&split(Split::Vertical, 0.5, leaf("a"), leaf("b")), screen());
        let d = dividers(&g);
        assert_eq!(d.len(), 1);
        assert!(d[0].vertical);
        assert_eq!(d[0].at, g.panes[0].outer.right());
    }

    #[test]
    fn a_divider_spans_only_where_the_two_panes_actually_meet() {
        // One tall pane beside two stacked ones: the divider between the tall
        // pane and the top one covers the top half, not the whole height.
        let tree = split(
            Split::Vertical,
            0.5,
            leaf("a"),
            split(Split::Horizontal, 0.5, leaf("b"), leaf("c")),
        );
        let g = compute(&tree, screen());
        let d = dividers(&g);
        let a_b = d
            .iter()
            .find(|x| x.before == "a" && x.after == "b")
            .expect("a divider between a and b");
        assert_eq!(a_b.from, 0);
        assert_eq!(a_b.to, 12);
    }

    #[test]
    fn panes_that_do_not_touch_have_no_divider_between_them() {
        let tree = split(
            Split::Vertical,
            0.5,
            leaf("a"),
            split(Split::Horizontal, 0.5, leaf("b"), leaf("c")),
        );
        let d = dividers(&compute(&tree, screen()));
        assert!(
            !d.iter()
                .any(|x| (x.before == "b" && x.after == "c") && x.vertical)
        );
    }

    #[test]
    fn a_divider_can_be_grabbed_with_a_finger() {
        // One cell wide and a finger is not, so without slop a divider on a
        // touch screen is a line nobody can grab.
        let g = compute(&split(Split::Vertical, 0.5, leaf("a"), leaf("b")), screen());
        let d = dividers(&g);
        let at = d[0].at;
        assert!(divider_at(&d, at, 5, 0).is_some(), "exact");
        assert!(divider_at(&d, at + 2, 5, 2).is_some(), "within slop");
        assert!(divider_at(&d, at + 9, 5, 2).is_none(), "far away");
    }

    #[test]
    fn a_point_off_the_end_of_a_divider_is_not_on_it() {
        let g = compute(
            &split(
                Split::Vertical,
                0.5,
                leaf("a"),
                split(Split::Horizontal, 0.5, leaf("b"), leaf("c")),
            ),
            screen(),
        );
        let d = dividers(&g);
        let a_b = d
            .iter()
            .find(|x| x.before == "a" && x.after == "b")
            .expect("found");
        assert!(divider_at(std::slice::from_ref(a_b), a_b.at, 20, 0).is_none());
    }

    #[test]
    fn a_pane_against_the_screen_edge_knows_it() {
        // A border drawn there wastes a column that is already scarce on a
        // phone.
        let g = compute(&split(Split::Vertical, 0.5, leaf("a"), leaf("b")), screen());
        let a = edges(&g.panes[0], screen());
        assert!(a.left && a.top && a.bottom);
        assert!(!a.right, "it meets `b`, not the screen");
        let b = edges(&g.panes[1], screen());
        assert!(b.right);
        assert!(!b.left);
    }

    #[test]
    fn a_single_pane_touches_every_edge_and_has_no_dividers() {
        let g = compute(&leaf("a"), screen());
        let e = edges(&g.panes[0], screen());
        assert!(e.left && e.top && e.right && e.bottom);
        assert!(dividers(&g).is_empty());
    }

    #[test]
    fn a_phone_sized_screen_gets_a_layout_it_can_show() {
        // §7. Mobile is a target, not a fallback: the same tree on a narrow
        // screen must produce something usable rather than four slivers.
        let phone = Rect {
            left: 0,
            top: 0,
            width: 30,
            height: 60,
        };
        let tree = preset(Preset::EvenVertical, &ids(&["a", "b", "c", "d"])).expect("t");
        let g = compute(&tree, phone);
        assert!(!g.panes.is_empty(), "something is shown");
        assert!(g.panes.iter().all(|p| p.outer.width >= MIN_WIDTH));
        assert!(g.degraded.is_some(), "and it says what it dropped");
    }

    #[test]
    fn the_same_tree_gives_different_answers_on_different_screens() {
        // The point of computing geometry per client rather than sharing one.
        let tree = preset(Preset::EvenVertical, &ids(&["a", "b", "c"])).expect("t");
        let desktop = compute(
            &tree,
            Rect {
                left: 0,
                top: 0,
                width: 200,
                height: 50,
            },
        );
        let phone = compute(
            &tree,
            Rect {
                left: 0,
                top: 0,
                width: 30,
                height: 60,
            },
        );
        assert_eq!(desktop.panes.len(), 3);
        assert!(phone.panes.len() < desktop.panes.len());
    }

    #[test]
    fn a_layout_round_trips_through_its_saved_form() {
        let tree = preset(Preset::MainVertical, &ids(&["a", "b", "c"])).expect("t");
        let saved = Saved {
            name: "work".to_owned(),
            tree: SavedNode::from(&tree),
        };
        let json = serde_json::to_string(&saved).expect("to json");
        let back: Saved = serde_json::from_str(&json).expect("from json");
        assert_eq!(Node::from(&back.tree), tree);
    }

    #[test]
    fn a_saved_layout_holds_no_rectangles() {
        // Rects are a function of the tree and the screen. Saving them would
        // restore a desktop layout onto a phone as a desktop layout, which is
        // the bug the degradation rule exists to prevent.
        let tree = preset(Preset::EvenVertical, &ids(&["a", "b"])).expect("t");
        let json = serde_json::to_string(&Saved {
            name: "x".to_owned(),
            tree: SavedNode::from(&tree),
        })
        .expect("json");
        for word in ["left", "top", "width", "height"] {
            assert!(!json.contains(word), "{word} leaked into {json}");
        }
    }

    #[test]
    fn restoring_keeps_the_panes_that_still_exist_and_names_the_rest() {
        // A layout describes sessions that were running, and by restore time
        // some are not.
        let tree = preset(Preset::EvenVertical, &ids(&["a", "b", "c"])).expect("t");
        let saved = Saved {
            name: "w".to_owned(),
            tree: SavedNode::from(&tree),
        };
        let (restored, missing) = restore(&saved, &ids(&["a", "c"])).expect("something survived");
        assert_eq!(missing, ["b"]);
        let g = compute(
            &restored,
            Rect {
                left: 0,
                top: 0,
                width: 80,
                height: 24,
            },
        );
        let names: Vec<String> = g.panes.iter().map(|p| p.pane.clone()).collect();
        assert_eq!(names, ["a", "c"]);
    }

    #[test]
    fn a_restore_with_nothing_left_is_none_rather_than_an_empty_layout() {
        let tree = preset(Preset::EvenVertical, &ids(&["a", "b"])).expect("t");
        let saved = Saved {
            name: "w".to_owned(),
            tree: SavedNode::from(&tree),
        };
        assert!(restore(&saved, &[]).is_none());
    }

    #[test]
    fn a_fully_alive_layout_restores_its_exact_shape() {
        // The common case must not be rebuilt into something merely
        // equivalent: somebody saved a main-vertical and expects it back.
        let tree = preset(Preset::MainVertical, &ids(&["a", "b", "c"])).expect("t");
        let saved = Saved {
            name: "w".to_owned(),
            tree: SavedNode::from(&tree),
        };
        let (restored, missing) = restore(&saved, &ids(&["a", "b", "c"])).expect("all alive");
        assert!(missing.is_empty());
        assert_eq!(restored, tree);
    }

    #[test]
    fn a_pruned_layout_has_no_split_with_one_child() {
        // Pruning in place leaves them, and a split with one child renders as
        // a pane that has mysteriously stopped filling its space.
        let tree = preset(Preset::MainVertical, &ids(&["a", "b", "c", "d"])).expect("t");
        let saved = Saved {
            name: "w".to_owned(),
            tree: SavedNode::from(&tree),
        };
        let (restored, _) = restore(&saved, &ids(&["a", "d"])).expect("some alive");
        let g = compute(
            &restored,
            Rect {
                left: 0,
                top: 0,
                width: 80,
                height: 24,
            },
        );
        assert_eq!(
            g.panes.iter().map(|p| p.outer.width).sum::<u16>(),
            80,
            "the restored panes fill the screen"
        );
    }

    #[test]
    fn a_layout_from_a_future_version_gives_back_the_panes_it_can() {
        // Failing the whole restore over one unrecognised field would hand
        // somebody nothing at all.
        let json = r#"{"name":"w","tree":{"kind":"split","direction":"diagonal","ratio":0.5,"first":{"kind":"leaf","id":"a"},"second":{"kind":"leaf","id":"b"}}}"#;
        let saved: Saved = serde_json::from_str(json).expect("parses");
        let (tree, _) = restore(&saved, &ids(&["a", "b"])).expect("restores");
        let g = compute(
            &tree,
            Rect {
                left: 0,
                top: 0,
                width: 80,
                height: 24,
            },
        );
        assert_eq!(g.panes.len(), 2);
    }

    #[test]
    fn a_float_sits_on_top_of_the_layout() {
        let area = Rect {
            left: 0,
            top: 0,
            width: 80,
            height: 24,
        };
        let tree = preset(Preset::EvenVertical, &ids(&["a", "b"])).expect("t");
        let g = overlay(
            compute(&tree, area),
            area,
            &[Float {
                pane: "scratch".to_owned(),
                rect: Rect {
                    left: 10,
                    top: 4,
                    width: 40,
                    height: 10,
                },
            }],
            &[],
        );
        assert_eq!(g.panes.last().expect("last").pane, "scratch", "drawn last");
        assert_eq!(g.panes.len(), 3);
    }

    #[test]
    fn a_float_is_clamped_into_the_screen_rather_than_hanging_off_it() {
        // One that opens partly off-screen on a narrow client is one somebody
        // cannot close.
        let area = Rect {
            left: 0,
            top: 0,
            width: 40,
            height: 20,
        };
        let g = overlay(
            compute(&leaf("a"), area),
            area,
            &[Float {
                pane: "f".to_owned(),
                rect: Rect {
                    left: 200,
                    top: 200,
                    width: 30,
                    height: 8,
                },
            }],
            &[],
        );
        let f = g.panes.iter().find(|p| p.pane == "f").expect("placed");
        assert!(f.outer.right() <= area.right(), "{:?}", f.outer);
        assert!(f.outer.bottom() <= area.bottom(), "{:?}", f.outer);
    }

    #[test]
    fn a_float_too_small_to_read_is_dropped_like_any_other_pane() {
        let area = Rect {
            left: 0,
            top: 0,
            width: 80,
            height: 24,
        };
        let g = overlay(
            compute(&leaf("a"), area),
            area,
            &[Float {
                pane: "f".to_owned(),
                rect: Rect {
                    left: 0,
                    top: 0,
                    width: 5,
                    height: 1,
                },
            }],
            &[],
        );
        assert!(g.hidden.contains(&"f".to_owned()));
    }

    #[test]
    fn a_stack_occupies_exactly_one_slot_and_reflows_nothing() {
        // A stack that resized its neighbours would move the pane somebody is
        // reading every time they cycled a background one.
        let area = Rect {
            left: 0,
            top: 0,
            width: 80,
            height: 24,
        };
        let tree = preset(Preset::EvenVertical, &ids(&["a", "b"])).expect("t");
        let plain = compute(&tree, area);
        let b_rect = plain.panes.iter().find(|p| p.pane == "b").expect("b").outer;
        let stacked = overlay(
            plain,
            area,
            &[],
            &[Stack {
                at: "b".to_owned(),
                members: ids(&["b", "b2", "b3"]),
                visible: 0,
            }],
        );
        let a = stacked.panes.iter().find(|p| p.pane == "a").expect("a");
        assert_eq!(a.outer.width, 40, "the neighbour did not move");
        let showing = stacked.panes.iter().find(|p| p.pane == "b").expect("b");
        assert_eq!(showing.outer, b_rect);
    }

    #[test]
    fn cycling_a_stack_swaps_which_member_is_drawn_in_the_same_rect() {
        let area = Rect {
            left: 0,
            top: 0,
            width: 80,
            height: 24,
        };
        let tree = preset(Preset::EvenVertical, &ids(&["a", "b"])).expect("t");
        let stack = |visible| Stack {
            at: "b".to_owned(),
            members: ids(&["b", "b2"]),
            visible,
        };
        let first = overlay(compute(&tree, area), area, &[], &[stack(0)]);
        let second = overlay(compute(&tree, area), area, &[], &[stack(1)]);
        let rect_of =
            |g: &Geometry, id: &str| g.panes.iter().find(|p| p.pane == id).map(|p| p.outer);
        assert_eq!(rect_of(&first, "b"), rect_of(&second, "b2"));
        assert!(rect_of(&second, "b").is_none());
    }

    #[test]
    fn the_members_a_stack_conceals_are_reported_as_hidden() {
        // Somebody has to be able to see that four things are in this stack
        // and one is showing.
        let area = Rect {
            left: 0,
            top: 0,
            width: 80,
            height: 24,
        };
        let g = overlay(
            compute(&leaf("b"), area),
            area,
            &[],
            &[Stack {
                at: "b".to_owned(),
                members: ids(&["b", "b2", "b3"]),
                visible: 0,
            }],
        );
        assert_eq!(g.hidden.len(), 2);
    }

    #[test]
    fn moving_a_pane_swaps_it_rather_than_reshaping_the_tree() {
        // Removing and re-inserting changes the shape around the hole, so
        // panes that did not move end up somewhere new — which is not what
        // anybody means by "move this pane there".
        let area = Rect {
            left: 0,
            top: 0,
            width: 120,
            height: 40,
        };
        let tree = preset(Preset::MainVertical, &ids(&["a", "b", "c"])).expect("t");
        let before = compute(&tree, area);
        let after = compute(&swap(&tree, "a", "c"), area);
        let rect_of =
            |g: &Geometry, id: &str| g.panes.iter().find(|p| p.pane == id).map(|p| p.outer);
        assert_eq!(rect_of(&before, "a"), rect_of(&after, "c"));
        assert_eq!(rect_of(&before, "c"), rect_of(&after, "a"));
        assert_eq!(
            rect_of(&before, "b"),
            rect_of(&after, "b"),
            "b did not move"
        );
    }

    #[test]
    fn swapping_a_pane_with_itself_changes_nothing() {
        let tree = preset(Preset::EvenVertical, &ids(&["a", "b"])).expect("t");
        assert_eq!(swap(&tree, "a", "a"), tree);
    }

    #[test]
    fn swapping_something_absent_leaves_the_tree_alone() {
        // The first version compared swap(a, nope) with swap(nope, a) and
        // passed — both renamed `a` to `nope`. Two wrong answers agreeing is
        // not a test, and the real cost would have been a pane silently
        // becoming a name nothing refers to while its session kept running.
        let tree = preset(Preset::EvenVertical, &ids(&["a", "b"])).expect("t");
        assert_eq!(swap(&tree, "a", "nope"), tree);
        assert_eq!(swap(&tree, "nope", "a"), tree);
        assert!(contains(&swap(&tree, "a", "nope"), "a"));
    }

    fn ids(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn an_even_preset_gives_every_pane_the_same_share() {
        // Naive recursive halving gives 1/2, 1/4, 1/8, 1/16, 1/16, which looks
        // nothing like "even" and is what this is built to avoid.
        let tree = preset(Preset::EvenVertical, &ids(&["a", "b", "c", "d"])).expect("a tree");
        let g = compute(
            &tree,
            Rect {
                left: 0,
                top: 0,
                width: 80,
                height: 24,
            },
        );
        assert_eq!(g.panes.len(), 4);
        let widths: Vec<u16> = g.panes.iter().map(|p| p.outer.width).collect();
        let smallest = widths.iter().min().copied().expect("some");
        let largest = widths.iter().max().copied().expect("some");
        assert!(largest - smallest <= 1, "{widths:?}");
    }

    #[test]
    fn the_main_pane_keeps_its_share_as_others_are_added() {
        // Proportional would shrink it as panes arrive, which is backwards:
        // the main pane is the one being worked in, and adding a log tail
        // should not narrow the thing you are reading.
        let area = Rect {
            left: 0,
            top: 0,
            width: 120,
            height: 24,
        };
        let two = compute(
            &preset(Preset::MainVertical, &ids(&["a", "b"])).expect("t"),
            area,
        );
        let four = compute(
            &preset(Preset::MainVertical, &ids(&["a", "b", "c", "d"])).expect("t"),
            area,
        );
        let main_of = |g: &Geometry| {
            g.panes
                .iter()
                .find(|p| p.pane == "a")
                .expect("main")
                .outer
                .width
        };
        assert_eq!(main_of(&two), main_of(&four));
    }

    #[test]
    fn a_single_pane_preset_is_just_that_pane() {
        let tree = preset(Preset::MainVertical, &ids(&["only"])).expect("a tree");
        assert_eq!(tree, leaf("only"));
    }

    #[test]
    fn no_panes_is_not_a_layout() {
        // Returning an empty tree would push the question onto every caller.
        assert!(preset(Preset::EvenVertical, &[]).is_none());
    }

    #[test]
    fn every_preset_places_every_pane_it_was_given() {
        let names = ids(&["a", "b", "c"]);
        for kind in [
            Preset::EvenVertical,
            Preset::EvenHorizontal,
            Preset::MainVertical,
            Preset::MainHorizontal,
        ] {
            let tree = preset(kind, &names).expect("a tree");
            let g = compute(
                &tree,
                Rect {
                    left: 0,
                    top: 0,
                    width: 120,
                    height: 40,
                },
            );
            assert_eq!(g.panes.len() + g.hidden.len(), 3, "{kind:?}");
        }
    }

    #[test]
    fn a_zoomed_pane_fills_the_area() {
        let area = Rect {
            left: 0,
            top: 0,
            width: 80,
            height: 24,
        };
        let tree = preset(Preset::EvenVertical, &ids(&["a", "b", "c"])).expect("t");
        let g = compute_zoomed(&tree, area, "b");
        assert_eq!(g.panes.len(), 1);
        assert_eq!(g.panes[0].outer, area);
    }

    #[test]
    fn zoom_does_not_edit_the_tree() {
        // A zoom that survived into a saved layout would have somebody restore
        // it and find their other panes gone, with no way to tell zoom was why.
        let area = Rect {
            left: 0,
            top: 0,
            width: 80,
            height: 24,
        };
        let tree = preset(Preset::EvenVertical, &ids(&["a", "b", "c"])).expect("t");
        let before = tree.clone();
        let _ = compute_zoomed(&tree, area, "b");
        assert_eq!(tree, before);
        assert_eq!(compute(&tree, area).panes.len(), 3, "unzoom restores");
    }

    #[test]
    fn the_panes_a_zoom_hides_are_reported_as_hidden() {
        // True — they are not drawn — and it means a client showing "n hidden"
        // needs no special case for zoom.
        let area = Rect {
            left: 0,
            top: 0,
            width: 80,
            height: 24,
        };
        let tree = preset(Preset::EvenVertical, &ids(&["a", "b", "c"])).expect("t");
        let g = compute_zoomed(&tree, area, "b");
        assert_eq!(g.hidden.len(), 2);
        assert!(g.degraded.is_some());
    }

    #[test]
    fn zooming_a_pane_that_is_not_there_changes_nothing() {
        let area = Rect {
            left: 0,
            top: 0,
            width: 80,
            height: 24,
        };
        let tree = preset(Preset::EvenVertical, &ids(&["a", "b"])).expect("t");
        assert_eq!(compute_zoomed(&tree, area, "nope"), compute(&tree, area));
    }

    fn leaf(id: &str) -> Node {
        Node::Leaf { id: id.to_owned() }
    }

    fn split(direction: Split, ratio: f32, first: Node, second: Node) -> Node {
        Node::Split {
            direction,
            ratio,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    fn screen() -> Rect {
        Rect {
            left: 0,
            top: 0,
            width: 80,
            height: 24,
        }
    }

    #[test]
    fn a_single_pane_takes_the_whole_area() {
        let g = compute(&leaf("a"), screen());
        assert_eq!(g.panes.len(), 1);
        assert_eq!(g.panes[0].outer, screen());
        assert!(g.degraded.is_none());
    }

    #[test]
    fn a_split_divides_the_space_and_leaves_no_gap() {
        // A column lost to rounding is a one-cell strip nothing draws into,
        // and it shows as a flickering line between two panes.
        let g = compute(&split(Split::Vertical, 0.5, leaf("a"), leaf("b")), screen());
        assert_eq!(g.panes[0].outer.right(), g.panes[1].outer.left);
        assert_eq!(
            g.panes[0].outer.width + g.panes[1].outer.width,
            screen().width
        );
    }

    #[test]
    fn an_uneven_ratio_still_leaves_no_gap() {
        // Only the ratios where both children clear the minimum. At 0.1 of
        // eighty columns the first child is eight wide and is dropped, which
        // is the design working rather than a gap.
        for ratio in [0.33f32, 0.5, 0.67] {
            let g = compute(
                &split(Split::Vertical, ratio, leaf("a"), leaf("b")),
                screen(),
            );
            assert_eq!(g.panes.len(), 2, "ratio {ratio}");
            assert_eq!(
                g.panes[0].outer.width + g.panes[1].outer.width,
                screen().width,
                "ratio {ratio}"
            );
        }
    }

    #[test]
    fn a_ratio_that_starves_one_child_drops_it_rather_than_drawing_it() {
        // Eight columns is not a small pane, it is a decoration.
        let g = compute(&split(Split::Vertical, 0.1, leaf("a"), leaf("b")), screen());
        assert_eq!(g.hidden, ["a"]);
        assert_eq!(g.panes.len(), 1);
        assert!(g.degraded.is_some());
    }

    #[test]
    fn a_horizontal_split_divides_the_other_way() {
        let g = compute(
            &split(Split::Horizontal, 0.5, leaf("a"), leaf("b")),
            screen(),
        );
        assert_eq!(g.panes[0].outer.bottom(), g.panes[1].outer.top);
        assert_eq!(
            g.panes[0].outer.height + g.panes[1].outer.height,
            screen().height
        );
    }

    #[test]
    fn a_pane_too_small_to_use_is_dropped_rather_than_drawn_as_a_sliver() {
        // The worst outcome is shrinking everything below usability, because
        // it looks like it worked.
        let narrow = Rect {
            width: 30,
            ..screen()
        };
        let g = compute(
            &split(
                Split::Vertical,
                0.5,
                leaf("a"),
                split(Split::Vertical, 0.5, leaf("b"), leaf("c")),
            ),
            narrow,
        );
        assert!(!g.hidden.is_empty());
        assert!(g.panes.iter().all(|p| p.outer.width >= MIN_WIDTH));
    }

    #[test]
    fn what_was_dropped_is_named_not_just_counted() {
        // "2 panes hidden" leaves somebody hunting for which, and the answer
        // is not on screen by definition.
        let tiny = Rect {
            width: 25,
            height: 24,
            ..screen()
        };
        let g = compute(&split(Split::Vertical, 0.5, leaf("a"), leaf("b")), tiny);
        let degraded = g.degraded.expect("something was dropped");
        assert_eq!(degraded.dropped, g.hidden.len());
        for id in &g.hidden {
            assert!(degraded.reason.contains(id), "{}", degraded.reason);
        }
    }

    #[test]
    fn a_narrow_screen_shows_one_readable_pane_rather_than_nothing() {
        // The floor. An empty screen on a phone somebody is holding is worse
        // than any arrangement, and it is what four even splits at thirty
        // columns produced before this.
        let phone = Rect {
            left: 0,
            top: 0,
            width: 30,
            height: 60,
        };
        let tree = preset(Preset::EvenVertical, &ids(&["a", "b", "c", "d"])).expect("t");
        let g = compute(&tree, phone);
        assert_eq!(g.panes.len(), 1);
        assert_eq!(g.panes[0].outer, phone);
        assert_eq!(g.hidden.len(), 3);
    }

    #[test]
    fn a_screen_too_small_for_anything_hides_everything_rather_than_panicking() {
        let nothing = Rect {
            left: 0,
            top: 0,
            width: 4,
            height: 1,
        };
        let g = compute(&split(Split::Vertical, 0.5, leaf("a"), leaf("b")), nothing);
        assert!(g.panes.is_empty());
        assert_eq!(g.hidden.len(), 2);
        assert!(g.degraded.is_some());
    }

    #[test]
    fn the_index_is_stable_across_a_resize() {
        // Focus navigation breaks ties by this, so it must not change when
        // somebody drags a window edge.
        let tree = split(
            Split::Vertical,
            0.5,
            leaf("a"),
            split(Split::Horizontal, 0.5, leaf("b"), leaf("c")),
        );
        let wide = compute(&tree, screen());
        let taller = compute(
            &tree,
            Rect {
                height: 48,
                ..screen()
            },
        );
        let index_of = |g: &Geometry, id: &str| {
            g.panes
                .iter()
                .find(|p| p.pane == id)
                .map(|p| p.index)
                .expect("placed")
        };
        for id in ["a", "b", "c"] {
            assert_eq!(index_of(&wide, id), index_of(&taller, id), "{id}");
        }
    }

    #[test]
    fn an_index_is_assigned_even_to_a_pane_that_was_hidden() {
        // Otherwise a pane appearing after a resize would renumber everything
        // after it, and focus would jump for reasons nobody can see.
        let tree = split(
            Split::Vertical,
            0.5,
            leaf("a"),
            split(Split::Vertical, 0.5, leaf("b"), leaf("c")),
        );
        let roomy = compute(
            &tree,
            Rect {
                width: 120,
                ..screen()
            },
        );
        let cramped = compute(
            &tree,
            Rect {
                width: 45,
                ..screen()
            },
        );
        let a_roomy = roomy.panes.iter().find(|p| p.pane == "a").expect("a").index;
        let a_cramped = cramped
            .panes
            .iter()
            .find(|p| p.pane == "a")
            .expect("a")
            .index;
        assert_eq!(a_roomy, a_cramped);
    }

    #[test]
    fn no_two_panes_overlap() {
        // The bug that two independent geometry calculations produce, and the
        // reason this is the only place geometry exists.
        let tree = split(
            Split::Vertical,
            0.4,
            leaf("a"),
            split(Split::Horizontal, 0.6, leaf("b"), leaf("c")),
        );
        let g = compute(&tree, screen());
        for (i, one) in g.panes.iter().enumerate() {
            for other in &g.panes[i + 1..] {
                let apart = one.outer.right() <= other.outer.left
                    || other.outer.right() <= one.outer.left
                    || one.outer.bottom() <= other.outer.top
                    || other.outer.bottom() <= one.outer.top;
                assert!(apart, "{:?} overlaps {:?}", one.outer, other.outer);
            }
        }
    }

    #[test]
    fn an_absurd_ratio_is_clamped_rather_than_producing_an_empty_pane() {
        for ratio in [-5.0f32, 0.0, 1.0, 12.0] {
            let g = compute(
                &split(Split::Vertical, ratio, leaf("a"), leaf("b")),
                screen(),
            );
            assert_eq!(
                g.panes.len() + g.hidden.len(),
                2,
                "ratio {ratio} lost a pane"
            );
        }
    }
}
