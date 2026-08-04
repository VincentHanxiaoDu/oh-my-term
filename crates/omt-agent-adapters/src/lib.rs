//! One module per agent, each implementing [`AgentAdapter`].
//!
//! The build order is deliberate: Claude Code to full depth, then the generic
//! ACP adapter, then the floor. The ACP adapter is second rather than last
//! precisely so the trait is validated against a *second* shape before it is
//! frozen — one shaped only by Claude Code would fit exactly one agent, and
//! nobody would find out until the second one was written.

pub mod adapter;
pub mod agents;
pub mod bounded;
pub mod detect;
pub mod install;
pub mod registry;

pub use adapter::{
    AcpSpawn, AdapterError, AgentAdapter, AttachmentClass, AttachmentReference, Detection,
    Fingerprint, Interrupt, SessionModeSet, SpawnCtx,
};
pub use agents::{ClaudeCode, GenericAcp, HeuristicFloor, ScreenSignals, guess_activity};
pub use bounded::{
    BoundedError, MAX_FILE_BYTES, MAX_NESTING_DEPTH, MAX_STRUCTURAL_TOKENS, check_structure,
    parse_line, read_bounded, read_json,
};
pub use detect::{Observation, detect, may_emit_structured};
pub use install::{
    INSTALL_VERSION, InstallError, Plan, STAMP_KEY, events_for, is_stale, plan_install,
    plan_uninstall,
};
pub use registry::{AdapterSet, builtin};
