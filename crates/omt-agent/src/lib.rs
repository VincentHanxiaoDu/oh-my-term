//! Agent bindings: what is running, what it is doing, and what it is asking.
//!
//! Two machines, and both exist to stop a plausible wrong answer.
//!
//! [`merge`] resolves disagreeing sources by confidence rather than by vote,
//! because a majority of guesses is still a guess.
//!
//! [`ledger`] holds interactions to exactly-once resolution and refuses to call
//! an answer delivered until something observed the agent record it.

pub mod ledger;
pub mod merge;
pub mod threads;
pub mod usage;

pub use ledger::{
    Confirmation, DEFAULT_CONFIRMATION_MS, Ledger, LedgerError, Observation, confirms,
};
pub use merge::{
    ConsideredSource, Explanation, Liveness, MergeMachine, Millis, SourceReading, WEDGED_AFTER_MS,
    freshness_window, is_answerable, needs_human,
};
pub use threads::{MAIN_THREAD, RosterSummary, Thread, ThreadRoster};
pub use usage::{Headroom, RateLimit, Usage, UsageLedger};
