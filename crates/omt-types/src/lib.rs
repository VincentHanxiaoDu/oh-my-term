//! Primitive domain types shared by every omt crate.
//!
//! The rule that governs this crate: **if two crates need to name the same
//! thing, it belongs here.** Not because a shared crate is tidy, but because
//! the alternative is two definitions that drift, and a type that means
//! something slightly different depending on who imported it.
//!
//! No behaviour, no I/O, no runtime. Everything above depends on these, so a
//! dependency here is a dependency everywhere.

mod domain;
mod ids;
mod seq;
mod timestamp;

pub use domain::{Actor, AgentKind, AgentState, BlockReason, Role, SessionMode, Tier};
pub use ids::{
    BindingId, BlockId, ClientId, CredentialId, DeviceId, IdentityId, InstanceId, IntentId,
    InteractionId, JobId, PaneId, SessionId, ViewId, WorkspaceId,
};
pub use seq::{Seq, SeqScope};

pub use timestamp::Timestamp;
