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
    let mut panes = Vec::new();
    let mut hidden = Vec::new();
    let mut index = 0u32;
    place(tree, area, &mut panes, &mut hidden, &mut index);

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
    let known = full.panes.iter().any(|p| p.pane == zoomed) || full.hidden.iter().any(|h| h == zoomed);
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

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;
    fn ids(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn an_even_preset_gives_every_pane_the_same_share() {
        // Naive recursive halving gives 1/2, 1/4, 1/8, 1/16, 1/16, which looks
        // nothing like "even" and is what this is built to avoid.
        let tree = preset(Preset::EvenVertical, &ids(&["a", "b", "c", "d"])).expect("a tree");
        let g = compute(&tree, Rect { left: 0, top: 0, width: 80, height: 24 });
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
        let area = Rect { left: 0, top: 0, width: 120, height: 24 };
        let two = compute(&preset(Preset::MainVertical, &ids(&["a", "b"])).expect("t"), area);
        let four = compute(
            &preset(Preset::MainVertical, &ids(&["a", "b", "c", "d"])).expect("t"),
            area,
        );
        let main_of = |g: &Geometry| {
            g.panes.iter().find(|p| p.pane == "a").expect("main").outer.width
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
            let g = compute(&tree, Rect { left: 0, top: 0, width: 120, height: 40 });
            assert_eq!(g.panes.len() + g.hidden.len(), 3, "{kind:?}");
        }
    }

    #[test]
    fn a_zoomed_pane_fills_the_area() {
        let area = Rect { left: 0, top: 0, width: 80, height: 24 };
        let tree = preset(Preset::EvenVertical, &ids(&["a", "b", "c"])).expect("t");
        let g = compute_zoomed(&tree, area, "b");
        assert_eq!(g.panes.len(), 1);
        assert_eq!(g.panes[0].outer, area);
    }

    #[test]
    fn zoom_does_not_edit_the_tree() {
        // A zoom that survived into a saved layout would have somebody restore
        // it and find their other panes gone, with no way to tell zoom was why.
        let area = Rect { left: 0, top: 0, width: 80, height: 24 };
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
        let area = Rect { left: 0, top: 0, width: 80, height: 24 };
        let tree = preset(Preset::EvenVertical, &ids(&["a", "b", "c"])).expect("t");
        let g = compute_zoomed(&tree, area, "b");
        assert_eq!(g.hidden.len(), 2);
        assert!(g.degraded.is_some());
    }

    #[test]
    fn zooming_a_pane_that_is_not_there_changes_nothing() {
        let area = Rect { left: 0, top: 0, width: 80, height: 24 };
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
        let g = compute(
            &split(Split::Vertical, 0.5, leaf("a"), leaf("b")),
            screen(),
        );
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
            let g = compute(&split(Split::Vertical, ratio, leaf("a"), leaf("b")), screen());
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
        let roomy = compute(&tree, Rect { width: 120, ..screen() });
        let cramped = compute(&tree, Rect { width: 45, ..screen() });
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
            let g = compute(&split(Split::Vertical, ratio, leaf("a"), leaf("b")), screen());
            assert_eq!(
                g.panes.len() + g.hidden.len(),
                2,
                "ratio {ratio} lost a pane"
            );
        }
    }
}
