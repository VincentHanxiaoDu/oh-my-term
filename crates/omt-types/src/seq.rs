//! Sequence positions and the scopes they are counted in.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::ids::{SessionId, WorkspaceId};

/// A position in a scope's event stream.
///
/// `u64`, everywhere, including on the wire. A `u32` would wrap, and a client
/// resuming after a wrap rejoins at a point that looks valid and is not —
/// believing itself caught up while silently missing everything. Four bytes per
/// frame buys away a class of bug no test would reliably catch, and at 10⁹
/// increments per second a `u64` lasts about 580 years, so there is no
/// wrap behaviour to define. That absence is the point.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
    schemars::JsonSchema,
)]
#[serde(transparent)]
pub struct Seq(u64);

impl Seq {
    /// The position before any event has been issued.
    pub const ZERO: Self = Self(0);

    /// Wrap a raw position.
    #[must_use]
    pub const fn new(n: u64) -> Self {
        Self(n)
    }

    /// The raw position.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The next position after this one.
    ///
    /// Saturating rather than wrapping: if a system ever reached `u64::MAX`,
    /// stalling at the top is survivable, while wrapping to zero would make
    /// every resume request ambiguous.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

impl fmt::Display for Seq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Which stream a sequence position belongs to.
///
/// Three spaces, not one. A phone watching one session must not have its
/// resume position disturbed by traffic in another, and there is deliberately
/// no global total order — providing one would mean a single allocator on
/// every event in the system, which is the wrong thing to put on the PTY hot
/// path for a guarantee nothing needs.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
    schemars::JsonSchema,
)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum SeqScope {
    /// Counted within one session.
    Session {
        /// The session.
        session: SessionId,
    },
    /// Counted within one workspace.
    Workspace {
        /// The workspace.
        workspace: WorkspaceId,
    },
    /// Counted for the instance as a whole.
    ///
    /// Carries no id: a connection is already bound to exactly one instance, so
    /// sending one would be redundant at best and a second, contradictable
    /// source of truth at worst.
    Instance,
}

impl SeqScope {
    /// The reserved key an instance-scoped stream is resumed by.
    ///
    /// A single uniformly-typed key across all three scopes, rather than a
    /// union the wire and every client would have to discriminate.
    pub const INSTANCE_KEY: &'static str = "s_instance";

    /// The key a client supplies to resume this scope.
    #[must_use]
    pub fn resume_key(&self) -> String {
        match self {
            Self::Session { session } => session.to_string(),
            Self::Workspace { workspace } => workspace.to_string(),
            Self::Instance => Self::INSTANCE_KEY.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seq_advances() {
        assert_eq!(Seq::ZERO.next().get(), 1);
        assert_eq!(Seq::new(41).next(), Seq::new(42));
    }

    #[test]
    fn seq_saturates_rather_than_wrapping() {
        // Wrapping is the failure this type exists to prevent: a resume after a
        // wrap rejoins at a plausible-looking wrong point.
        let top = Seq::new(u64::MAX);
        assert_eq!(top.next(), top);
    }

    #[test]
    fn seq_is_a_bare_number_on_the_wire() {
        let json = serde_json::to_string(&Seq::new(7)).expect("serialize");
        assert_eq!(json, "7");
    }

    #[test]
    fn instance_scope_resumes_by_the_reserved_key() {
        assert_eq!(SeqScope::Instance.resume_key(), "s_instance");
    }

    #[test]
    fn scopes_resume_by_distinct_keys() {
        let a = SeqScope::Session { session: SessionId::new() };
        let b = SeqScope::Workspace {
            workspace: WorkspaceId::from_canonical_path("/x"),
        };
        assert_ne!(a.resume_key(), b.resume_key());
        assert_ne!(a.resume_key(), SeqScope::Instance.resume_key());
    }
}
