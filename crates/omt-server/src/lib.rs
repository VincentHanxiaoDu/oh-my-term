//! The HTTP and WebSocket surface: generated routes over one dispatch path.
//!
//! A stub for now, carrying the route table the parity gate checks. Like the
//! TUI stub, it exists early so the gate has something real to fail against.

use omt_catalog::CapabilityRegistry;

/// A generated route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// The path.
    pub path: String,
    /// The capability it dispatches to.
    pub capability: &'static str,
}

/// Every route, derived from the registry.
///
/// Derived rather than written: a hand-maintained route table is a second place
/// to forget something.
#[must_use]
pub fn routes(registry: &CapabilityRegistry) -> Vec<Route> {
    registry
        .decls()
        .map(|d| Route {
            path: d.route(),
            capability: d.name,
        })
        .collect()
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
    fn routes_are_derived_from_declarations() {
        let r = CapabilityRegistry::new();
        assert!(routes(&r).is_empty(), "no declarations, no routes");
    }
}
