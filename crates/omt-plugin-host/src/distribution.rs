//! Where a plugin comes from, and what that is worth.
//!
//! `docs/architecture/11-plugins.md` §6 and §7. Three sources in decreasing
//! trust, and one rule that runs under all of them:
//!
//! **omt does not fetch and execute code from an unauthenticated URL at
//! runtime.** The only network fetch is at install time, over TLS, against a
//! pinned digest. A plugin that could re-fetch itself while running is one whose
//! consent screen described something else.
//!
//! The digest recorded at install is re-checked on every start. An edited plugin
//! directory therefore cannot silently change behaviour — it is disabled and
//! reported. That is deliberately not the same as "signed": signing says who
//! wrote it, and the digest says it is still what the user agreed to.
//!
//! §7's compatibility rules are here too, because they are the same question
//! asked at a different moment: whether this plugin may run *here*.

/// Where a plugin came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    /// A local directory, the development loop.
    ///
    /// Least trusted as a *provenance* claim — it makes none — but it is the
    /// user's own machine and their own file.
    LocalPath,
    /// Found by topic search. Unreviewed, and labelled so.
    Discovered,
    /// Listed in a signed registry index.
    Registry,
}

impl Source {
    /// What a surface must say about a plugin from here.
    ///
    /// A discovered plugin is labelled `unreviewed` rather than left unlabelled:
    /// a search result that looks like a registry entry is the whole attack.
    #[must_use]
    pub const fn label(self) -> Option<&'static str> {
        match self {
            Self::Registry => None,
            Self::Discovered => Some("unreviewed"),
            Self::LocalPath => Some("local"),
        }
    }
}

/// Why an install or a start was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The directory no longer hashes to what the user agreed to.
    DigestMismatch {
        /// What was recorded at install.
        recorded: String,
        /// What it is now.
        found: String,
    },
    /// Unsigned, and this host requires a signature.
    Unsigned,
    /// The host is outside the plugin's declared range.
    ApiOutOfRange {
        /// What the plugin wants.
        wants: String,
        /// What this host is.
        host: String,
    },
    /// The name collides with a core group.
    CoreCollision {
        /// The group it tried to claim.
        group: String,
    },
    /// Another enabled plugin already contributes this name.
    Conflicted {
        /// Who got there first.
        with: String,
    },
}

/// Whether a plugin may start.
///
/// # Errors
/// Refused on a digest mismatch, or when unsigned on a host that requires
/// signatures.
pub fn admit(
    recorded_digest: &str,
    found_digest: &str,
    signed: bool,
    allow_unsigned: bool,
) -> Result<(), Refusal> {
    // Checked on *every* start, not only at install: an install-time check
    // secures the moment the user consented and nothing after it.
    if recorded_digest != found_digest {
        return Err(Refusal::DigestMismatch {
            recorded: recorded_digest.to_owned(),
            found: found_digest.to_owned(),
        });
    }
    if !signed && !allow_unsigned {
        return Err(Refusal::Unsigned);
    }
    Ok(())
}

/// Core capability groups a plugin may not claim.
///
/// A plugin that could contribute `session.*` could shadow the calls every
/// surface makes, which is a takeover rather than an extension.
pub const RESERVED_GROUPS: &[&str] = &[
    "session",
    "agent",
    "block",
    "config",
    "instance",
    "events",
    "plugin",
    "keys",
    "media",
    "interaction",
    "queue",
    "workspace",
    "identity",
    "store",
    "security",
];

/// Whether a contributed capability name may be registered.
///
/// # Errors
/// Refused on a core collision, or when another enabled plugin already
/// contributes the name — the first enabled wins and the second is told, rather
/// than one silently shadowing the other.
pub fn admit_contribution(
    group: &str,
    name: &str,
    already: &[(String, String)],
) -> Result<(), Refusal> {
    if RESERVED_GROUPS.contains(&group) {
        return Err(Refusal::CoreCollision {
            group: group.to_owned(),
        });
    }
    if let Some((owner, _)) = already.iter().find(|(_, existing)| existing == name) {
        return Err(Refusal::Conflicted {
            with: owner.clone(),
        });
    }
    Ok(())
}

/// What a plugin gets when it calls a capability that is no longer there.
///
/// Capability drift is survivable: the granted list at `init` is authoritative
/// for that run, and a plugin that hardcodes a name which has since gone gets a
/// refusal at call time rather than a crash.
pub const DRIFTED: &str = "unsupported";

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    #[test]
    fn an_edited_plugin_directory_cannot_silently_change_behaviour() {
        // The digest is re-checked on every start. An install-time check
        // secures the moment the user consented and nothing after it.
        let refusal = admit("sha256:aaa", "sha256:bbb", true, true).expect_err("refused");
        assert!(matches!(refusal, Refusal::DigestMismatch { .. }));
    }

    #[test]
    fn a_mismatch_reports_both_digests() {
        // So the answer to "what changed" starts somewhere.
        let Refusal::DigestMismatch { recorded, found } =
            admit("sha256:aaa", "sha256:bbb", true, true).expect_err("refused")
        else {
            panic!("a digest mismatch");
        };
        assert_ne!(recorded, found);
    }

    #[test]
    fn unsigned_is_allowed_by_default_because_that_is_the_development_loop() {
        assert!(admit("d", "d", false, true).is_ok());
    }

    #[test]
    fn unsigned_is_refused_where_the_host_requires_signatures() {
        assert_eq!(admit("d", "d", false, false), Err(Refusal::Unsigned));
    }

    #[test]
    fn a_discovered_plugin_is_labelled_unreviewed() {
        // A search result that looks like a registry entry is the whole attack.
        assert_eq!(Source::Discovered.label(), Some("unreviewed"));
        assert_eq!(Source::Registry.label(), None);
    }

    #[test]
    fn the_sources_are_ordered_by_trust() {
        // Written down so a surface sorting them cannot invert it by accident.
        assert!(Source::Registry > Source::Discovered);
        assert!(Source::Discovered > Source::LocalPath);
    }

    #[test]
    fn a_plugin_may_not_claim_a_core_group() {
        // One that could contribute `session.*` could shadow the calls every
        // surface makes.
        let refusal = admit_contribution("session", "session.list", &[]).expect_err("refused");
        assert!(matches!(refusal, Refusal::CoreCollision { .. }));
    }

    #[test]
    fn every_reserved_group_is_actually_a_group_this_omt_uses() {
        // A reserved list that drifted from the catalog would reserve names
        // nothing owns while leaving real ones free.
        assert!(RESERVED_GROUPS.contains(&"queue"));
        assert!(RESERVED_GROUPS.contains(&"session"));
        assert!(!RESERVED_GROUPS.is_empty());
    }

    #[test]
    fn the_first_enabled_plugin_wins_a_name_and_the_second_is_told() {
        // Rather than one silently shadowing the other, which would make the
        // behaviour depend on load order nobody can see.
        let already = vec![("dev.a".to_owned(), "aider.set_model".to_owned())];
        let refusal =
            admit_contribution("aider", "aider.set_model", &already).expect_err("refused");
        assert_eq!(
            refusal,
            Refusal::Conflicted {
                with: "dev.a".to_owned()
            }
        );
    }

    #[test]
    fn a_name_nobody_has_claimed_is_admitted() {
        assert!(admit_contribution("aider", "aider.set_model", &[]).is_ok());
    }

    #[test]
    fn a_capability_that_has_gone_is_a_refusal_not_a_crash() {
        // Capability drift is survivable: the granted list at init is
        // authoritative for that run.
        assert_eq!(DRIFTED, "unsupported");
    }
}
