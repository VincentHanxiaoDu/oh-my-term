//! Identifier newtypes.
//!
//! Every id is a distinct type so that passing a `SessionId` where a `PaneId`
//! belongs is a compile error rather than a runtime mystery. The cost is a
//! macro; the benefit is that the confusion cannot happen.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Declares a UUID-backed identifier with a stable display prefix.
///
/// The prefix is what makes an id readable in a log or an error message —
/// `sess_1f0c…` says what it is without a schema in hand.
/// An id that could not be read back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not a `{expected}` id in its wire form")]
pub struct IdParseError {
    /// The prefix that was expected.
    pub expected: &'static str,
}

macro_rules! uuid_id {
    ($(#[$m:meta])* $name:ident, $prefix:literal) => {
        $(#[$m])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
            Serialize, Deserialize, schemars::JsonSchema,
        )]
        #[serde(transparent)]
        pub struct $name(uuid::Uuid);

        impl $name {
            /// The textual prefix this id renders with.
            pub const PREFIX: &'static str = $prefix;

            /// Mint a fresh, random id.
            #[must_use]
            pub fn new() -> Self {
                Self(uuid::Uuid::new_v4())
            }

            /// Wrap an existing UUID.
            #[must_use]
            pub const fn from_uuid(u: uuid::Uuid) -> Self {
                Self(u)
            }

            /// The underlying UUID.
            #[must_use]
            pub const fn as_uuid(&self) -> &uuid::Uuid {
                &self.0
            }

            /// The **lossless** form, for anywhere this id must survive a round
            /// trip through a string.
            ///
            /// [`fmt::Display`] is deliberately abbreviated so a log line stays
            /// readable, which means it cannot be parsed back. Everywhere an id
            /// is written out to be read again — an injected environment
            /// variable, a deep link, a stored record — has to use this
            /// instead, or the identity is quietly destroyed at the boundary.
            #[must_use]
            pub fn to_wire(&self) -> String {
                format!("{}_{}", $prefix, self.0.simple())
            }

            /// Recover an id from [`Self::to_wire`].
            ///
            /// The prefix is checked, not skipped: a `PaneId` handed to
            /// `SessionId::from_wire` is a bug that would otherwise produce a
            /// valid-looking id pointing at nothing.
            #[must_use]
            pub fn from_wire(s: &str) -> Option<Self> {
                let rest = s.strip_prefix($prefix)?.strip_prefix('_')?;
                uuid::Uuid::parse_str(rest).ok().map(Self)
            }
        }

        impl std::str::FromStr for $name {
            type Err = IdParseError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Self::from_wire(s).ok_or(IdParseError {
                    expected: $prefix,
                })
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                // Short form: enough to disambiguate in a log line, short
                // enough to read. The full UUID is one `as_uuid()` away.
                let s = self.0.simple().to_string();
                write!(f, "{}_{}", $prefix, &s[..12])
            }
        }
    };
}

uuid_id!(
    /// One omt daemon on one machine.
    InstanceId, "inst"
);
uuid_id!(
    /// One logical terminal: a PTY or a native agent connection, plus its state.
    SessionId, "sess"
);
uuid_id!(
    /// A viewport onto a session inside a layout. Presentation only.
    PaneId, "pane"
);
uuid_id!(
    /// A named arrangement of panes over a workspace's sessions.
    ViewId, "view"
);
uuid_id!(
    /// One connected client.
    ClientId, "clnt"
);
uuid_id!(
    /// A person. Stable across their devices.
    IdentityId, "idty"
);
uuid_id!(
    /// One browser or app install on one physical device.
    DeviceId, "devc"
);
uuid_id!(
    /// One issued credential.
    CredentialId, "cred"
);
uuid_id!(
    /// An agent's occupancy of a session, with a lifetime.
    BindingId, "bind"
);
uuid_id!(
    /// A request from an agent for a human decision.
    InteractionId, "intr"
);
uuid_id!(
    /// A client-generated identity for one intended mutation.
    ///
    /// Minted at intent time, before any server is reached, because a client
    /// that has lost its connection cannot ask for one.
    IntentId, "itnt"
);
uuid_id!(
    /// A long-running call the caller can poll or cancel.
    JobId, "job"
);
uuid_id!(
    /// One command block in a session's scrollback.
    BlockId, "blck"
);

/// A workspace, identified by where it is rather than by a random number.
///
/// Two clients that open the same directory must agree they are in the same
/// workspace without coordinating, so the id is derived from the canonical
/// path. It is content-addressed for the same reason a git tree hash is: the
/// identity *is* the content.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(transparent)]
pub struct WorkspaceId([u8; 16]);

impl WorkspaceId {
    /// The textual prefix this id renders with.
    pub const PREFIX: &'static str = "wksp";

    /// Derive the id of the workspace rooted at `canonical_path`.
    ///
    /// The caller is responsible for canonicalization — this function cannot
    /// tell `/tmp` from `/private/tmp`, and guessing would produce two ids for
    /// one directory.
    #[must_use]
    pub fn from_canonical_path(canonical_path: &str) -> Self {
        let digest = blake3::hash(canonical_path.as_bytes());
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&digest.as_bytes()[..16]);
        Self(bytes)
    }

    /// The raw derivation output.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// The **lossless** form, for anywhere this id must survive a string.
    ///
    /// As with every other id, [`fmt::Display`] is abbreviated for logs and
    /// cannot be parsed back. This one is hand-written rather than macro
    /// generated, which is exactly how it came to be missing the wire form
    /// while every other id had one.
    #[must_use]
    pub fn to_wire(&self) -> String {
        let mut out = String::with_capacity(Self::PREFIX.len() + 33);
        out.push_str(Self::PREFIX);
        out.push('_');
        for b in &self.0 {
            out.push_str(&format!("{b:02x}"));
        }
        out
    }

    /// Recover an id from [`Self::to_wire`].
    #[must_use]
    pub fn from_wire(s: &str) -> Option<Self> {
        let hex = s.strip_prefix(Self::PREFIX)?.strip_prefix('_')?;
        if hex.len() != 32 {
            return None;
        }
        let mut bytes = [0u8; 16];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
        }
        Some(Self(bytes))
    }
}

impl std::str::FromStr for WorkspaceId {
    type Err = IdParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_wire(s).ok_or(IdParseError {
            expected: Self::PREFIX,
        })
    }
}

impl fmt::Display for WorkspaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}_", Self::PREFIX)?;
        for b in &self.0[..6] {
            write!(f, "{b:02x}")?;
        }
        Ok(())
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

    #[test]
    fn ids_are_distinct_types() {
        // The point of the newtypes: this file would not compile if
        // `SessionId` and `PaneId` were interchangeable. Asserting their
        // prefixes differ is the runtime shadow of that compile-time property.
        assert_ne!(SessionId::PREFIX, PaneId::PREFIX);
    }

    #[test]
    fn display_is_prefixed_and_short() {
        let s = SessionId::new().to_string();
        assert!(s.starts_with("sess_"), "{s}");
        assert_eq!(s.len(), "sess_".len() + 12);
    }

    #[test]
    fn workspace_id_is_stable_for_a_path() {
        let a = WorkspaceId::from_canonical_path("/home/v/src/omt");
        let b = WorkspaceId::from_canonical_path("/home/v/src/omt");
        assert_eq!(a, b, "same path must yield the same workspace");
    }

    #[test]
    fn workspace_id_separates_different_paths() {
        let a = WorkspaceId::from_canonical_path("/home/v/src/omt");
        let b = WorkspaceId::from_canonical_path("/home/v/src/other");
        assert_ne!(a, b);
    }

    #[test]
    fn ids_round_trip_through_json_transparently() {
        // `serde(transparent)` means the wire form is the bare UUID, not an
        // object. A change here is a wire break, so the test asserts the shape.
        let id = SessionId::new();
        let json = serde_json::to_string(&id).expect("serialize");
        assert!(json.starts_with('"'), "expected a bare string, got {json}");
        let back: SessionId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(id, back);
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod wire_tests {
    use super::*;

    #[test]
    fn the_wire_form_round_trips() {
        // Everywhere an id is written out to be read again — an injected
        // environment variable, a deep link, a stored record — depends on this.
        let id = SessionId::new();
        assert_eq!(SessionId::from_wire(&id.to_wire()), Some(id));
    }

    #[test]
    fn the_display_form_deliberately_does_not() {
        // Display is abbreviated so a log line stays readable. That is fine as
        // long as nothing tries to parse it back, which is exactly why the two
        // forms are separate methods rather than one.
        let id = SessionId::new();
        assert!(id.to_string().len() < id.to_wire().len());
        assert_eq!(
            SessionId::from_wire(&id.to_string()),
            None,
            "the short form must not parse, or it would round-trip to a *different* id"
        );
    }

    #[test]
    fn a_workspace_id_round_trips_through_its_wire_form_too() {
        // Hand-written rather than macro generated, which is exactly how it
        // came to be missing this while every other id had it.
        let id = WorkspaceId::from_canonical_path("/home/me/project");
        assert_eq!(WorkspaceId::from_wire(&id.to_wire()), Some(id));
        assert_eq!(WorkspaceId::from_wire(&id.to_string()), None);
        assert_eq!(WorkspaceId::from_wire("wksp_short"), None);
    }

    #[test]
    fn an_id_of_another_kind_is_refused() {
        // A PaneId handed to SessionId::from_wire would otherwise produce a
        // valid-looking id pointing at nothing.
        let pane = PaneId::new();
        assert_eq!(SessionId::from_wire(&pane.to_wire()), None);
        assert!(PaneId::from_wire(&pane.to_wire()).is_some());
    }

    #[test]
    fn nonsense_is_refused_rather_than_defaulted() {
        for bad in ["", "sess_", "sess_notauuid", "wholly unrelated"] {
            assert_eq!(SessionId::from_wire(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn parsing_names_the_prefix_it_wanted() {
        let err: Result<SessionId, _> = "nope".parse();
        let err = err.expect_err("must refuse");
        assert!(err.to_string().contains(SessionId::PREFIX), "{err}");
    }
}
