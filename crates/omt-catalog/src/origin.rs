//! Where a capability came from, and what identifies the catalog as a whole.
//!
//! [03 §3.6](../../../docs/architecture/03-capability-catalog.md). The catalog
//! has two tiers and **the difference is declared rather than hidden**: a core
//! capability is in `DECLS`, was checked by the macro at compile time, and
//! appears in the generated `CapabilityName` union. A plugin's registers at run
//! time — after codegen has run and after the web bundle was built — so it can
//! be in none of those. Pretending otherwise would make the exhaustiveness
//! check a lie about the half of the catalog it cannot see.
//!
//! [`CatalogHash`] covers both tiers, which is the point of it existing:
//! enabling a plugin invalidates every client's cached catalog exactly as a
//! version bump does.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Which tier a capability belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Origin {
    /// Declared in the binary and checked by the macro.
    Core,
    /// Contributed by a plugin at run time, and validated by the host against
    /// the same rules the macro applies — a plugin capability that fails them
    /// is refused rather than registered in a degraded form.
    Plugin {
        /// Which plugin.
        plugin: String,
        /// Its version, so two installs of the same plugin are tellable apart.
        version: String,
    },
}

impl Origin {
    /// Whether this capability was checked at compile time.
    ///
    /// The question the parity gate asks: a core capability must appear in
    /// every generated artifact, and a plugin's cannot appear in any of them.
    #[must_use]
    pub const fn is_core(&self) -> bool {
        matches!(self, Self::Core)
    }
}

/// What identifies a catalog.
///
/// A client caches the catalog and sends the hash it has; a matching hash
/// means "nothing to send". It must therefore cover **everything a client
/// would render differently** — which is why it is computed from the entries
/// rather than from the binary's version: two builds with the same version and
/// a different plugin enabled are different catalogs, and a client that
/// believed otherwise would render a stale list of what it can do.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(transparent)]
pub struct CatalogHash(String);

impl CatalogHash {
    /// Hash a catalog from the lines that describe it.
    ///
    /// Order-independent by construction — the caller passes entries in name
    /// order and this does not sort — because the registry is already
    /// name-sorted and a hash that depended on iteration order would change
    /// for a catalog that had not.
    #[must_use]
    pub fn of<'a>(lines: impl IntoIterator<Item = &'a str>) -> Self {
        // FNV-1a over the joined description. Not a cryptographic hash and not
        // pretending to be: this detects change, and the thing it is compared
        // against arrived over an authenticated connection.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for line in lines {
            for byte in line.as_bytes() {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
            // A separator, so `["ab", "c"]` and `["a", "bc"]` differ.
            hash ^= 0xff;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Self(format!("{hash:016x}"))
    }

    /// Read it back.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CatalogHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "in a test, expect() is the assertion")]
mod tests {
    use super::*;

    #[test]
    fn a_plugin_capability_is_not_core() {
        // The parity gate branches on this: a core capability must be in every
        // generated artifact, and a plugin's cannot be in any of them.
        assert!(Origin::Core.is_core());
        assert!(
            !Origin::Plugin {
                plugin: "ntfy".to_owned(),
                version: "0.1.0".to_owned(),
            }
            .is_core()
        );
    }

    #[test]
    fn an_origin_says_which_plugin_and_which_version() {
        // Two installs of the same plugin are different catalogs, and a hash
        // that could not tell them apart would serve one's schema for the
        // other's capability.
        let json = serde_json::to_string(&Origin::Plugin {
            plugin: "ntfy".to_owned(),
            version: "0.2.0".to_owned(),
        })
        .expect("serialize");
        assert!(json.contains(r#""type":"plugin""#), "{json}");
        assert!(json.contains(r#""version":"0.2.0""#), "{json}");
    }

    #[test]
    fn the_same_catalog_hashes_the_same() {
        assert_eq!(
            CatalogHash::of(["a.one", "b.two"]),
            CatalogHash::of(["a.one", "b.two"])
        );
    }

    #[test]
    fn adding_a_plugins_capability_changes_the_hash() {
        // The whole reason it covers both tiers: enabling a plugin must
        // invalidate every client's cached catalog, or a phone renders a list
        // of things it cannot do and omits the things it can.
        let core = CatalogHash::of(["instance.info", "session.list"]);
        let with_plugin = CatalogHash::of(["instance.info", "ntfy.send", "session.list"]);
        assert_ne!(core, with_plugin);
    }

    #[test]
    fn a_changed_entry_changes_the_hash_even_at_the_same_length() {
        // A role or an effect changing is a change a client renders — it
        // decides whether to ask for confirmation from it.
        assert_ne!(
            CatalogHash::of(["session.close role=operator"]),
            CatalogHash::of(["session.close role=admin"])
        );
    }

    #[test]
    fn the_boundary_between_entries_is_part_of_the_hash() {
        // Without a separator, moving a character across the boundary is
        // invisible — and a catalog is a list, not a concatenation.
        assert_ne!(CatalogHash::of(["ab", "c"]), CatalogHash::of(["a", "bc"]));
    }

    #[test]
    fn an_empty_catalog_still_has_a_hash() {
        // An instance with nothing registered is a bug elsewhere, not a reason
        // for this to have no answer.
        assert!(!CatalogHash::of(std::iter::empty()).as_str().is_empty());
    }
}
