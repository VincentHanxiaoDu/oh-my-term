//! The instance: one place that owns the tree, the ledger and the event stream.
//!
//! Sequence numbers are assigned here and nowhere else. Every durable client
//! position is a `(scope, seq)` pair, so two components that both minted
//! positions would eventually produce two different events at one position —
//! which a resuming client cannot detect and cannot recover from.

pub mod instance;

pub use instance::{Instance, REPLAY_CAPACITY};
