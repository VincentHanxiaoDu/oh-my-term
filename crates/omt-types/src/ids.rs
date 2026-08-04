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
