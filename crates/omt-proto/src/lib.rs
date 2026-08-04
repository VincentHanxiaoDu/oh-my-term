//! The wire protocol between an omt instance and a client.
//!
//! Framing, the handshake, capability calls, event subscription and resume,
//! terminal byte frames, and the hook ingress. This crate defines the messages
//! and owns no policy: it says what can be expressed, not what is allowed —
//! authorization lives in dispatch, so no transport can accidentally bypass it.

mod frame;
mod hook;
mod message;

pub use frame::{BinaryHeader, FrameError, FrameKind, MAX_TEXT_FRAME};
pub use hook::{
    HookAck, HookCorrelation, HookDirective, HookEvent, MAX_VERBATIM_PAYLOAD, Truncation,
};
pub use message::{
    Call, CallOutcome, CallResult, Goodbye, GoodbyeReason, Hello, PROTO_VERSION, ProtoMessage,
    Resync, Subscribe, Welcome, negotiate,
};
