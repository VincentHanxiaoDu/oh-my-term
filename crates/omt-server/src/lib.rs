//! The HTTP and WebSocket surface: generated routes over one dispatch path.
//!
//! Every call takes one path through [`dispatch::handle`], whether it came
//! from a phone, the CLI or the TUI's own palette. There is deliberately no
//! faster local route: a second path is how "remote is equivalent to local"
//! stops being true without anybody noticing.

pub mod dispatch;
pub mod http;

pub use dispatch::{Peer, catalog_hash, handle};
pub use http::{DEFAULT_BIND, HttpState, TlsFiles, bearer, router, run_with_tls};

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
