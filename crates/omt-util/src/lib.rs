//! Cross-cutting helpers with no domain knowledge.
//!
//! Everything here would otherwise be reimplemented slightly differently in
//! three crates. Nothing here knows what a session or an agent is — the moment
//! something does, it belongs in the crate that owns that concept.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// A monotonic counter.
///
/// Lives here rather than in `omt-types` because it is *behaviour*, and
/// `omt-types` carries none. It sits on the PTY hot path, so it is a single
/// relaxed atomic increment and nothing else: no lock, no allocation, no
/// clock read.
#[derive(Debug, Default)]
pub struct SeqGenerator(AtomicU64);

impl SeqGenerator {
    /// A generator whose first issued value is 1.
    #[must_use]
    pub const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    /// A generator that resumes after `last`.
    ///
    /// Used on restart: positions must never be reused, or a client presenting
    /// an old position would be silently accepted at the wrong place.
    #[must_use]
    pub const fn resuming_after(last: u64) -> Self {
        Self(AtomicU64::new(last))
    }

    /// Issue the next position.
    ///
    /// `Relaxed` is sufficient and deliberate: the guarantee wanted is
    /// uniqueness and monotonicity of the values themselves, not ordering
    /// against other memory. Anything stronger would be a fence on the
    /// hottest path in the system for a property nothing reads.
    pub fn next(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// The highest position issued so far.
    pub fn current(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// A registry of named implementations behind a trait.
///
/// Every extension point in omt — agent adapters, transports, auth backends,
/// storage, speech-to-text, notification sinks — is one of these. Sharing the
/// type means they share their failure modes too: a duplicate registration is
/// refused here once rather than handled five ways.
#[derive(Debug)]
pub struct Registry<T> {
    entries: BTreeMap<String, T>,
}

impl<T> Default for Registry<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Registry<T> {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self { entries: BTreeMap::new() }
    }

    /// Register `value` under `name`.
    ///
    /// # Errors
    /// Returns [`RegistryError::Duplicate`] if `name` is taken. Refusing is the
    /// point: silently replacing would mean whichever crate linked last wins,
    /// which is a bug that reproduces differently on every machine.
    pub fn register(&mut self, name: impl Into<String>, value: T) -> Result<(), RegistryError> {
        let name = name.into();
        if self.entries.contains_key(&name) {
            return Err(RegistryError::Duplicate { name });
        }
        self.entries.insert(name, value);
        Ok(())
    }

    /// Look up by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&T> {
        self.entries.get(name)
    }

    /// Every entry, in name order.
    ///
    /// Sorted, so anything derived from a registry — generated code, a
    /// dump, a diff — is byte-identical run to run rather than dependent on
    /// hash or link order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &T)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// How many entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// What can go wrong registering an implementation.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RegistryError {
    /// A name was registered twice.
    #[error("`{name}` is already registered; a name collision would silently shadow one of them")]
    Duplicate {
        /// The contested name.
        name: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequences_start_at_one_and_are_unique() {
        let g = SeqGenerator::new();
        assert_eq!(g.next(), 1);
        assert_eq!(g.next(), 2);
        assert_eq!(g.current(), 2);
    }

    #[test]
    fn a_resumed_generator_never_reissues() {
        // The restart case: reusing a position would make a stale resume
        // request look valid.
        let g = SeqGenerator::resuming_after(41);
        assert_eq!(g.next(), 42);
    }

    #[test]
    fn concurrent_issuance_yields_no_duplicates() {
        use std::collections::HashSet;
        use std::sync::Arc;

        let g = Arc::new(SeqGenerator::new());
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let g = Arc::clone(&g);
                std::thread::spawn(move || (0..1000).map(|_| g.next()).collect::<Vec<_>>())
            })
            .collect();

        let mut seen = HashSet::new();
        for h in handles {
            for n in h.join().expect("thread panicked") {
                assert!(seen.insert(n), "position {n} issued twice");
            }
        }
        assert_eq!(seen.len(), 8000);
    }

    #[test]
    fn registry_refuses_duplicates() {
        let mut r = Registry::new();
        r.register("a", 1).expect("first registration");
        let err = r.register("a", 2).expect_err("duplicate must be refused");
        assert_eq!(err, RegistryError::Duplicate { name: "a".into() });
        assert_eq!(r.get("a"), Some(&1), "the original must survive");
    }

    #[test]
    fn registry_iterates_in_name_order() {
        // Determinism of derived artifacts depends on this.
        let mut r = Registry::new();
        for n in ["zebra", "apple", "mango"] {
            r.register(n, ()).expect("register");
        }
        let names: Vec<_> = r.iter().map(|(k, _)| k).collect();
        assert_eq!(names, ["apple", "mango", "zebra"]);
    }
}
