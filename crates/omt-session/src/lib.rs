//! The session tree, and who is allowed to type into it.
//!
//! Two things this crate makes structural rather than conventional. A pane
//! holds a session *id*, so closing a pane cannot kill a session even by
//! mistake. And every write carries the writer-token epoch it believed it held,
//! so input in flight when the token changes hands is rejected rather than
//! landing in somebody else's command line.

pub mod fanout;
pub mod tree;
pub mod writer;

pub use fanout::{Arm, ArmState, Fanout, FanoutError};
pub use tree::{
    Instance, InstanceLimits, LayoutView, Pane, Session, SessionKind, SessionMode, SessionState,
    TreeError, Workspace,
};
pub use writer::{
    Epoch, IDLE_TIMEOUT_MS, TAKEOVER_GRACE_MS, Takeover, WriterError, WriterPolicy, WriterState,
    WriterToken,
};
