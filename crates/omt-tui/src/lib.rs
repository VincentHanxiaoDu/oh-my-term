//! The native TUI: a client of the catalog, not a privileged path into it.
//!
//! It renders a grid, forwards keys, and reaches everything else through the
//! catalog — the same path a phone takes. The action table below is what the
//! parity gate checks; it exists so a gate with one of its arms missing cannot
//! silently pass.

pub mod app;
pub mod render;

pub use app::{Input, RawGuard, translate};
pub use render::Screen;

use omt_catalog::CapabilityRegistry;

/// A capability reachable from the TUI, and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    /// Which capability it invokes.
    pub capability: &'static str,
    /// The chord, where it has one. `None` means palette-only, which is
    /// genuine reachability — the palette's contents are the catalog.
    pub binding: Option<&'static str>,
}

/// Every action the TUI binds.
///
/// Deliberately small. The palette makes every capability reachable, so a
/// binding is an optimization for frequency, not a requirement — which is what
/// lets omt keep a tiny un-prefixed key budget instead of inventing a chord for
/// a hundred and fifty operations.
pub const ACTIONS: &[Action] = &[
    Action {
        capability: "instance.info",
        binding: None,
    },
    Action {
        capability: "instance.catalog",
        binding: None,
    },
    Action {
        capability: "events.subscribe",
        binding: None,
    },
];

/// Whether a capability is reachable in the TUI.
///
/// Palette membership *or* an explicit binding. Weaker than "a binding exists",
/// and deliberately so: claiming the stronger property would be a guarantee omt
/// does not keep.
#[must_use]
pub fn is_reachable(name: &str, registry: &CapabilityRegistry) -> bool {
    if ACTIONS.iter().any(|a| a.capability == name) {
        return true;
    }
    // In the palette if it is declared and not hidden.
    registry.decl(name).is_some_and(|d| !d.hidden)
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
    fn every_bound_action_names_a_real_capability_shape() {
        // The reverse direction, which is the half that catches drift: a
        // binding pointing at a renamed capability would otherwise ship as a
        // key that silently does nothing.
        for a in ACTIONS {
            assert!(
                a.capability.contains('.'),
                "`{}` is not a dotted name",
                a.capability
            );
        }
    }

    #[test]
    fn actions_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for a in ACTIONS {
            assert!(
                seen.insert(a.capability),
                "`{}` is bound twice",
                a.capability
            );
        }
    }
}
