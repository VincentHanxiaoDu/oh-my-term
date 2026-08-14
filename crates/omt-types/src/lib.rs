//! Primitive domain types shared by every omt crate.
//!
//! The rule that governs this crate: **if two crates need to name the same
//! thing, it belongs here.** Not because a shared crate is tidy, but because
//! the alternative is two definitions that drift, and a type that means
//! something slightly different depending on who imported it.
//!
//! No behaviour, no I/O, no runtime. Everything above depends on these, so a
//! dependency here is a dependency everywhere.

mod diff;
mod domain;
mod ids;
mod peer;
mod seq;
mod timestamp;

pub use diff::{
    ChangeKind, DiffBase, DiffBody, DiffLine, DiffLineKind, EofNewline, FileDiff, Hunk, IntraRange,
    LineStat, RedactReason, RelPath, VcsFileState, intra_ranges,
};
pub use domain::{Actor, AgentKind, AgentState, BlockReason, DeviceClaim, Role, SessionMode, Tier};
pub use ids::{
    BindingId, BlockId, ClientId, CredentialId, DeviceId, IdentityId, InstanceId, IntentId,
    InteractionId, JobId, PaneId, SessionId, ViewId, WorkspaceId,
};
pub use peer::{
    ActorId, AgentSessionId, Fidelity, PeerInfo, PermissionMode, ResponderRef, SourceId,
    StateDependence, Version,
};
pub use seq::{Seq, SeqScope};

pub use timestamp::Timestamp;

#[cfg(test)]
#[allow(clippy::expect_used, reason = "in a test, expect() is the assertion")]
mod wire_tests {
    use super::*;

    /// Round-trip a value and return the JSON it produced.
    fn wire<T>(value: &T) -> String
    where
        T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let json = serde_json::to_string(value).expect("serialize");
        let back: T = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(&back, value, "round trip changed the value: {json}");
        json
    }

    #[test]
    fn every_tagged_enum_spells_its_variants_in_snake_case() {
        // The wire spelling is the contract every client is written against,
        // and a rename is only caught here — a Rust rename compiles, the
        // schema regenerates, and the phone stops understanding a field.
        assert_eq!(wire(&Role::Operator), r#""operator""#);
        assert_eq!(wire(&SessionMode::Pty), r#""pty""#);
        assert_eq!(wire(&AgentKind::ClaudeCode), r#""claude_code""#);
        // Internally tagged, so a client reads one field name whatever the
        // variant carries — the alternative is a bare string for some states
        // and an object for others, which every client would branch on.
        assert_eq!(wire(&AgentState::Idle), r#"{"state":"idle"}"#);
        assert_eq!(wire(&BlockReason::PlanReview), r#""plan_review""#);
        assert_eq!(wire(&Tier::Marker), r#""marker""#);
        assert_eq!(wire(&Fidelity::Synthetic), r#""synthetic""#);
        assert_eq!(wire(&StateDependence::Independent), r#""independent""#);
        assert_eq!(wire(&ChangeKind::TypeChanged), r#""type_changed""#);
        assert_eq!(wire(&DiffLineKind::Removed), r#""removed""#);
        assert_eq!(wire(&EofNewline::OldOnly), r#""old_only""#);
    }

    #[test]
    fn a_struct_variant_carries_its_fields_under_a_snake_case_tag() {
        let json = wire(&AgentState::Blocked {
            reason: BlockReason::Question,
            interaction: None,
        });
        assert!(json.contains(r#""blocked""#), "{json}");
        assert!(json.contains(r#""reason":"question""#), "{json}");
    }

    #[test]
    fn every_opaque_id_is_transparent_on_the_wire() {
        // A newtype that serialized as `{"0": "..."}` would make every client
        // unwrap a wrapper that exists only in Rust.
        assert_eq!(wire(&SourceId::new("transcript")), r#""transcript""#);
        assert_eq!(wire(&AgentSessionId::new("ses_1")), r#""ses_1""#);
        assert_eq!(wire(&ActorId::new("conn-1")), r#""conn-1""#);
        assert_eq!(wire(&Version::new("0.1.0")), r#""0.1.0""#);
        assert_eq!(
            wire(&PermissionMode::new("acceptEdits")),
            r#""acceptEdits""#
        );
        assert_eq!(wire(&RelPath::new("src/lib.rs")), r#""src/lib.rs""#);
    }

    #[test]
    fn every_uuid_id_round_trips_losslessly() {
        // `Display` is abbreviated so a log line stays readable, which means
        // it cannot be parsed back — so the wire form must not be it.
        let id = SessionId::new();
        assert_eq!(wire(&id).len(), 38, "expected a quoted uuid: {}", wire(&id));
        // Content-derived from the canonical root, so two clients that opened
        // the same directory agree without asking anybody.
        let ws = WorkspaceId::from_canonical_path("/repo");
        assert_eq!(wire(&ws), wire(&WorkspaceId::from_canonical_path("/repo")));
        assert_ne!(wire(&ws), wire(&WorkspaceId::from_canonical_path("/other")));
        assert_eq!(wire(&InteractionId::new()).len(), 38);
    }

    #[test]
    fn the_composite_types_round_trip_whole() {
        wire(&PeerInfo::unix(501));
        wire(&ResponderRef::native("acp"));
        wire(&VcsFileState::default());
        wire(&LineStat {
            added: 1,
            removed: 2,
        });
        wire(&IntraRange { start: 0, end: 3 });
        wire(&Actor::Remote {
            identity: IdentityId::new(),
            device: DeviceId::new(),
        });
    }
}
