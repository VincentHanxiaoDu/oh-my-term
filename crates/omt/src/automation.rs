//! What a script needs, and what omt will not do for it.
//!
//! `docs/architecture/22-operations.md` §8.
//!
//! The key primitive is `agent.wait`: block until a session reaches a state,
//! with a timeout. Without it every script is a `sleep` loop, and a `sleep` loop
//! is either slow or wrong depending on how long you guessed.
//!
//! The condition vocabulary has one distinction that is easy to miss and
//! expensive to get wrong. **`Blocked` is a state; `Interaction` is an edge.**
//! `Blocked` matches whenever the agent is blocked on a human, including at the
//! moment of the call — which is what a script asking "is anyone needed here?"
//! wants. `Interaction` matches only when a *new* card opens after the wait
//! began, which is what a watcher wanting each card exactly once needs, because
//! `Blocked` would fire immediately on one it has already handled.
//!
//! §8.5 is the other half, and it is a documented no rather than an
//! unimplemented feature: **omt never auto-answers an interaction.** No
//! `auto_approve`, no allow-list, no danger classifier. `interaction.resolve`
//! is exposed so *your* script decides, under *your* credential, audited as
//! coming from it.

use omt_types::AgentState;
use serde::{Deserialize, Serialize};

/// What ends a wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WaitCondition {
    /// The agent is waiting for a human to say something.
    Idle,
    /// The agent needs a human before it can continue.
    Blocked,
    /// The agent is working on a turn.
    Working,
    /// The agent is gone.
    Exited,
    /// A *new* interaction opened after the wait began.
    ///
    /// An edge, not a state. A watcher that wants each card exactly once asks
    /// for this; `Blocked` would fire immediately on a card it has already
    /// handled.
    Interaction,
}

impl WaitCondition {
    /// Whether a state satisfies this condition.
    ///
    /// `Interaction` never matches a state: it is an edge, and answering it
    /// from a state would make it fire on a card the caller already saw.
    #[must_use]
    pub const fn matches(self, state: &AgentState) -> bool {
        match self {
            Self::Idle => matches!(state, AgentState::Idle),
            Self::Blocked => matches!(state, AgentState::Blocked { .. }),
            Self::Working => matches!(state, AgentState::Working { .. }),
            Self::Exited => matches!(state, AgentState::Exited { .. }),
            Self::Interaction => false,
        }
    }

    /// Whether this condition is an edge rather than a state.
    #[must_use]
    pub const fn is_edge(self) -> bool {
        matches!(self, Self::Interaction)
    }
}

/// Parse a condition from the name a script uses.
///
/// # Errors
/// Fails on a name outside the closed set, listing it — a script that typoed
/// `blockd` would otherwise wait for the full timeout and report nothing.
pub fn condition(name: &str) -> Result<WaitCondition, String> {
    match name {
        "idle" => Ok(WaitCondition::Idle),
        "blocked" => Ok(WaitCondition::Blocked),
        "working" => Ok(WaitCondition::Working),
        "exited" => Ok(WaitCondition::Exited),
        "interaction" => Ok(WaitCondition::Interaction),
        _ => Err(format!(
            "`{name}` is not a wait condition; use idle, blocked, working, exited or interaction"
        )),
    }
}

/// Whether a wait can be answered without waiting at all.
///
/// `immediate` defaults to true because the alternative is a guaranteed race:
/// a script that checks and then waits misses the transition in between, and
/// one that only waits misses a state that already holds.
#[must_use]
pub fn already_satisfied(
    conditions: &[WaitCondition],
    state: &AgentState,
    immediate: bool,
) -> Option<WaitCondition> {
    if !immediate {
        return None;
    }
    conditions.iter().copied().find(|c| c.matches(state))
}

/// What a process exits with.
///
/// Documented, because a script's error handling is only as good as the codes
/// it can branch on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Exit {
    /// It worked.
    Ok = 0,
    /// The operation ran and failed.
    Failed = 1,
    /// A bad flag or an input that does not fit the schema.
    Usage = 2,
    /// Something would have prompted and there is no terminal.
    ///
    /// Distinct from a usage error: the command was well-formed, and the fix is
    /// a flag that says "do not ask".
    NoTty = 3,
    /// Authentication or authorization.
    Denied = 4,
    /// The instance could not be reached at all.
    Unreachable = 5,
    /// A deadline passed.
    Timeout = 6,
    /// The remote ran it and *it* failed.
    ///
    /// Distinguished from `Failed` so a script can tell "box said no" from "I
    /// could not ask box" — two situations with opposite retry answers.
    RemoteFailed = 7,
}

impl Exit {
    /// The code itself.
    #[must_use]
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// The exit code for a capability error.
    #[must_use]
    pub fn for_error(code: &str) -> Self {
        match code {
            "invalid_input" => Self::Usage,
            "unauthorized" | "forbidden" => Self::Denied,
            "unavailable" => Self::Unreachable,
            "timeout" => Self::Timeout,
            _ => Self::Failed,
        }
    }
}

/// The message shown when a prompt was needed and there is no terminal.
///
/// Names the flag that would have avoided it. "This requires a TTY" tells
/// somebody they have a problem and not how to leave.
#[must_use]
pub fn no_tty(flag: &str) -> String {
    format!("this would have prompted and there is no terminal; pass {flag}")
}

/// Things omt will not do unattended, and why.
///
/// Held as data because each one is a question somebody will ask, and the
/// answer should be the same every time it is asked.
pub const REFUSED: &[(&str, &str)] = &[
    (
        "auto-answer an interaction",
        "there is no auto_approve, no allow-list and no danger classifier. interaction.resolve is exposed so your script decides, under your credential, and every resolution is audited as coming from it",
    ),
    (
        "auto-upgrade a remote",
        "a version mismatch alone is not a reason to replace somebody's binary",
    ),
    (
        "purge without --yes",
        "in a non-TTY context the dry-run manifest is the default",
    ),
    (
        "send a bug report anywhere",
        "it is written to a file and going anywhere is your decision",
    ),
    (
        "synthesize an answer inferred from the screen",
        "automation does not relax that bound; nobody is watching, which tightens the case for it",
    ),
];

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn blocked() -> AgentState {
        AgentState::Blocked {
            reason: omt_types::BlockReason::Permission,
            interaction: None,
        }
    }

    #[test]
    fn a_state_condition_matches_the_state() {
        assert!(WaitCondition::Blocked.matches(&blocked()));
        assert!(!WaitCondition::Idle.matches(&blocked()));
    }

    #[test]
    fn interaction_is_an_edge_and_never_matches_a_state() {
        // A watcher asking for it wants each new card exactly once. Answering
        // it from the current state would fire on a card it has already
        // handled.
        assert!(!WaitCondition::Interaction.matches(&blocked()));
        assert!(WaitCondition::Interaction.is_edge());
        assert!(!WaitCondition::Blocked.is_edge());
    }

    #[test]
    fn a_condition_that_already_holds_returns_immediately() {
        // The alternative is a guaranteed race: a script that checks and then
        // waits misses the transition in between.
        let matched = already_satisfied(&[WaitCondition::Blocked], &blocked(), true);
        assert_eq!(matched, Some(WaitCondition::Blocked));
    }

    #[test]
    fn immediate_can_be_turned_off_for_a_watcher() {
        // A watcher wants the *next* transition, not the current state.
        assert_eq!(
            already_satisfied(&[WaitCondition::Blocked], &blocked(), false),
            None
        );
    }

    #[test]
    fn an_edge_is_never_satisfied_immediately_even_when_asked_to_be() {
        // `immediate` is about states. An edge that fired on the current state
        // would be the bug this distinction exists to prevent.
        assert_eq!(
            already_satisfied(&[WaitCondition::Interaction], &blocked(), true),
            None
        );
    }

    #[test]
    fn the_first_matching_condition_is_the_one_reported() {
        // A script branches on which one matched, so "some condition matched"
        // is not an answer.
        let matched = already_satisfied(
            &[WaitCondition::Idle, WaitCondition::Blocked],
            &blocked(),
            true,
        );
        assert_eq!(matched, Some(WaitCondition::Blocked));
    }

    #[test]
    fn a_misspelled_condition_fails_at_once_rather_than_after_the_timeout() {
        // Otherwise a script waits thirty minutes and reports nothing.
        let error = condition("blockd").expect_err("refused");
        assert!(error.contains("blocked"), "{error}");
    }

    #[test]
    fn every_condition_name_round_trips() {
        for (name, expected) in [
            ("idle", WaitCondition::Idle),
            ("blocked", WaitCondition::Blocked),
            ("working", WaitCondition::Working),
            ("exited", WaitCondition::Exited),
            ("interaction", WaitCondition::Interaction),
        ] {
            assert_eq!(condition(name).expect(name), expected);
        }
    }

    #[test]
    fn the_exit_codes_are_the_documented_ones() {
        // A script's error handling is only as good as the codes it branches
        // on, so these are pinned rather than assumed.
        assert_eq!(Exit::Ok.code(), 0);
        assert_eq!(Exit::Failed.code(), 1);
        assert_eq!(Exit::Usage.code(), 2);
        assert_eq!(Exit::NoTty.code(), 3);
        assert_eq!(Exit::Denied.code(), 4);
        assert_eq!(Exit::Unreachable.code(), 5);
        assert_eq!(Exit::Timeout.code(), 6);
        assert_eq!(Exit::RemoteFailed.code(), 7);
    }

    #[test]
    fn a_remote_failure_is_not_the_same_code_as_a_local_one() {
        // "box said no" and "I could not ask box" have opposite retry answers.
        assert_ne!(Exit::RemoteFailed.code(), Exit::Failed.code());
        assert_ne!(Exit::RemoteFailed.code(), Exit::Unreachable.code());
    }

    #[test]
    fn a_bad_input_is_a_usage_error_and_a_refusal_is_not() {
        // A script retrying on 2 would retry forever; one retrying on 1 might
        // succeed.
        assert_eq!(Exit::for_error("invalid_input"), Exit::Usage);
        assert_eq!(Exit::for_error("precondition_failed"), Exit::Failed);
        assert_eq!(Exit::for_error("unauthorized"), Exit::Denied);
    }

    #[test]
    fn the_no_tty_message_names_the_flag_that_would_have_worked() {
        // "This requires a TTY" tells somebody they have a problem and not how
        // to leave.
        let message = no_tty("--yes");
        assert!(message.contains("--yes"), "{message}");
    }

    #[test]
    fn every_documented_refusal_carries_its_reason() {
        // Each is a question somebody will ask, and the answer should be the
        // same every time.
        assert!(REFUSED.iter().any(|(what, _)| what.contains("auto-answer")));
        for (what, why) in REFUSED {
            assert!(why.len() > 30, "{what} has no reason");
        }
    }
}
