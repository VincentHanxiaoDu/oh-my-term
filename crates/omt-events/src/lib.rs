//! The event model: one envelope, one closed vocabulary, one stream.
//!
//! State is broadcast once. The native TUI subscribes on the same terms as a
//! phone, with the same schema and the same identifiers, because an
//! "internal" event the API cannot see is how two surfaces start disagreeing
//! about what happened.

mod agent;
mod envelope;
mod interaction;

pub use agent::{
    ActivityGuess, AgentEvent, AgentPayload, CompactionPhase, FileChange, MessageOrigin, PlanStep,
    PlanStepStatus, QueueOp, SlashCommand, StartReason, ThreadRef, TurnOutcome, TurnTrigger,
};
pub use envelope::{
    Event, EventKind, EventPayload, EventSourceTag, Filter, InstanceEvent, LagReason,
    PresenceEvent, ReplayWindow, ResumeOutcome, SessionTreeEvent, TerminalEvent,
};
pub use interaction::{
    CancelReason, ChoiceAnswer, ChoiceOption, ChoiceQuestion, Deliverable, Interaction,
    InteractionKind, InteractionResponse, InteractionState, NotDeliverableReason, PermissionOption,
    PermissionOptionKind, PlanDecision, UndeliveredReason,
};
