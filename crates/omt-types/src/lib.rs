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
    ChangeKind, DiffBase, DiffBody, DiffLine, DiffLineKind, EofNewline, FileDiff, Hunk,
    IntraRange, LineStat, RedactReason, RelPath, VcsFileState, intra_ranges,
};
pub use domain::{Actor, AgentKind, AgentState, BlockReason, DeviceClaim, Role, SessionMode, Tier};
pub use peer::{
    AgentSessionId, Fidelity, PeerInfo, PermissionMode, ResponderRef, SourceId, StateDependence,
};
pub use ids::{
    BindingId, BlockId, ClientId, CredentialId, DeviceId, IdentityId, InstanceId, IntentId,
    InteractionId, JobId, PaneId, SessionId, ViewId, WorkspaceId,
};
pub use seq::{Seq, SeqScope};

pub use timestamp::Timestamp;
