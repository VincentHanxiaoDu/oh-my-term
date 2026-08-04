//! What a capability call can fail with.

use serde::{Deserialize, Serialize};

/// The closed set of failure codes.
///
/// Closed so every surface can render a failure consistently without a
/// catch-all branch that says "something went wrong".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The named capability, or the thing it addresses, does not exist.
    NotFound,
    /// Someone else got there first, or the target moved on.
    Conflict,
    /// The caller's role is insufficient.
    Unauthorized,
    /// The call was well-formed but the world was not ready for it.
    PreconditionFailed,
    /// This instance cannot do it — a version gap, a missing backend.
    Unsupported,
    /// The input did not match the schema.
    InvalidInput,
    /// A bug. The only code that is not the caller's business to handle.
    Internal,
}

/// Why a `conflict` happened.
///
/// This discrimination is the whole reason `detail` exists. "Someone else
/// answered", "the agent withdrew the question" and "it timed out" want three
/// different sentences on screen, and collapsing them into one code would make
/// a client either say nothing useful or guess.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConflictState {
    /// Another actor resolved it first.
    AlreadyResolved {
        /// Who, so the loser's surface can say so rather than silently
        /// correcting itself.
        by: String,
    },
    /// The agent withdrew it before anyone answered.
    Cancelled,
    /// Nobody answered in time.
    Abandoned,
    /// The target's version moved under the caller.
    VersionMismatch {
        /// What the caller expected.
        expected: u64,
        /// What it actually was.
        actual: u64,
    },
}

/// A capability failure.
#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, thiserror::Error,
)]
#[error("{code:?}: {message}")]
pub struct CapabilityError {
    /// The stable code.
    pub code: ErrorCode,
    /// A sentence a human can read.
    pub message: String,
    /// Structured discrimination, where the code alone is not enough.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<ErrorDetail>,
}

/// Machine-readable detail attached to a failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum ErrorDetail {
    /// Why a `conflict` happened.
    Conflict(ConflictState),
    /// Anything else a handler wants to attach.
    Other(serde_json::Value),
}

impl CapabilityError {
    /// A `not_found`.
    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::NotFound,
            message: message.into(),
            detail: None,
        }
    }

    /// An `unauthorized`.
    #[must_use]
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::Unauthorized,
            message: message.into(),
            detail: None,
        }
    }

    /// An `unsupported`.
    #[must_use]
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::Unsupported,
            message: message.into(),
            detail: None,
        }
    }

    /// A `precondition_failed`.
    #[must_use]
    pub fn precondition_failed(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::PreconditionFailed,
            message: message.into(),
            detail: None,
        }
    }

    /// An `invalid_input`.
    #[must_use]
    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::InvalidInput,
            message: message.into(),
            detail: None,
        }
    }

    /// An `internal`.
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::Internal,
            message: message.into(),
            detail: None,
        }
    }

    /// A `conflict`, carrying why.
    #[must_use]
    pub fn conflict(message: impl Into<String>, state: ConflictState) -> Self {
        Self {
            code: ErrorCode::Conflict,
            message: message.into(),
            detail: Some(ErrorDetail::Conflict(state)),
        }
    }

    /// The conflict state, if this is a discriminated conflict.
    #[must_use]
    pub fn conflict_state(&self) -> Option<&ConflictState> {
        match &self.detail {
            Some(ErrorDetail::Conflict(s)) => Some(s),
            _ => None,
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    #[test]
    fn a_loser_can_tell_why_it_lost() {
        // The three conflict causes must be distinguishable, because they want
        // three different sentences on screen.
        let answered = CapabilityError::conflict(
            "answered elsewhere",
            ConflictState::AlreadyResolved {
                by: "iPhone".into(),
            },
        );
        let withdrawn = CapabilityError::conflict("withdrawn", ConflictState::Cancelled);
        let expired = CapabilityError::conflict("expired", ConflictState::Abandoned);

        assert!(matches!(
            answered.conflict_state(),
            Some(ConflictState::AlreadyResolved { .. })
        ));
        assert!(matches!(
            withdrawn.conflict_state(),
            Some(ConflictState::Cancelled)
        ));
        assert!(matches!(
            expired.conflict_state(),
            Some(ConflictState::Abandoned)
        ));
        assert_ne!(answered.detail, withdrawn.detail);
    }

    #[test]
    fn already_resolved_names_the_winner() {
        let e = CapabilityError::conflict(
            "lost the race",
            ConflictState::AlreadyResolved {
                by: "iPhone".into(),
            },
        );
        let Some(ConflictState::AlreadyResolved { by }) = e.conflict_state() else {
            panic!("expected AlreadyResolved");
        };
        assert_eq!(by, "iPhone");
    }

    #[test]
    fn errors_round_trip_with_their_detail() {
        let e = CapabilityError::conflict(
            "x",
            ConflictState::VersionMismatch {
                expected: 1,
                actual: 2,
            },
        );
        let json = serde_json::to_string(&e).expect("serialize");
        let back: CapabilityError = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(e, back);
    }

    #[test]
    fn a_plain_error_omits_detail() {
        let json = serde_json::to_string(&CapabilityError::not_found("no such session"))
            .expect("serialize");
        assert!(!json.contains("detail"), "{json}");
    }
}
