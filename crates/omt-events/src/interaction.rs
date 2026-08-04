//! Interactions: an agent asking a human something, promoted to an object.

use omt_types::{Actor, BindingId, InteractionId, SessionId, Timestamp};
use serde::{Deserialize, Serialize};

/// A request from an agent for a human decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Interaction {
    /// Stable identity, so it can be answered from anywhere.
    pub id: InteractionId,
    /// The session it belongs to.
    pub session: SessionId,
    /// The agent occupancy that raised it. Guards against an answer arriving
    /// after the agent it was meant for has been replaced.
    pub binding: BindingId,
    /// What is being asked.
    pub kind: InteractionKind,
    /// Whether omt can deliver an answer at all, and over which channel.
    ///
    /// Computed once when the interaction is created, from the responder and
    /// the card type. **Remote answerability renders from this, never from
    /// `state == Open`** — otherwise a surface offers to answer something omt
    /// cannot safely answer, and the user finds out by the wrong option being
    /// selected.
    pub deliverable: Deliverable,
    /// Where it is in its lifecycle.
    pub state: InteractionState,
    /// When it was raised.
    pub opened_at: Timestamp,
    /// When it expires, where the mechanism has a deadline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<Timestamp>,
}

/// What an agent is asking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InteractionKind {
    /// A structured question with enumerated options.
    Choice {
        /// One or more questions, in the order the agent asked them.
        questions: Vec<ChoiceQuestion>,
    },
    /// A tool or command wants approval.
    Permission {
        /// The tool's name.
        tool: String,
        /// Its arguments, **verbatim** — the same bytes the agent will act on,
        /// so a human approves what will actually run.
        input: serde_json::Value,
        /// A rendered command, where there is one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command: Option<String>,
        /// The agent's own options, verbatim and in its order. omt neither adds,
        /// removes nor reorders: the indices are how an answer is delivered.
        options: Vec<PermissionOption>,
    },
    /// A plan offered for review.
    PlanReview {
        /// The plan text.
        plan: String,
    },
    /// A free-text answer is wanted.
    Text {
        /// What is being asked.
        prompt: String,
        /// Placeholder for the input.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        /// Whether the answer is expected to span lines.
        multiline: bool,
    },
}

/// One question within a [`InteractionKind::Choice`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ChoiceQuestion {
    /// The question.
    pub question: String,
    /// A short tab label, around a dozen characters.
    pub header: String,
    /// Whether more than one option may be chosen.
    pub multi_select: bool,
    /// The options, in the agent's order.
    pub options: Vec<ChoiceOption>,
    /// Whether free text is accepted alongside the options.
    pub allow_free_text: bool,
}

/// One option of a question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ChoiceOption {
    /// What the user sees and what the answer is recorded as.
    pub label: String,
    /// Why they might pick it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// One option of a permission prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PermissionOption {
    /// The agent's own id for it, which is how an answer names it.
    ///
    /// Answering by `kind` alone is ambiguous: an agent may legitimately offer
    /// two options that are both "allow" and differ only in scope.
    pub id: String,
    /// What the user sees.
    pub label: String,
    /// Roughly what it does, for a surface that wants to style it.
    pub kind: PermissionOptionKind,
}

/// The shape of a permission option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PermissionOptionKind {
    /// Allow this once.
    AllowOnce,
    /// Allow, and stop asking.
    AllowAlways,
    /// Refuse this once.
    DenyOnce,
    /// Refuse, and stop asking.
    DenyAlways,
}

/// Whether an answer can be delivered, and how.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Deliverable {
    /// The responder has a real response channel — ACP, a plugin, an
    /// app-server. No keystrokes involved, and no writer token needed.
    Native,
    /// Keystrokes into the agent's own TUI, as a gated transaction.
    Synthetic {
        /// Always true for a synthetic delivery; carried explicitly so a client
        /// can render the gate without knowing the rule.
        requires_token: bool,
    },
    /// omt cannot answer this one. The surface shows it read-only, with the
    /// reason, and a route to the terminal.
    None {
        /// Why not — so the UI can say *why*, not just "unavailable".
        reason: NotDeliverableReason,
    },
}

impl Deliverable {
    /// Whether a surface may offer an answering affordance.
    #[must_use]
    pub const fn is_answerable(&self) -> bool {
        !matches!(self, Self::None { .. })
    }
}

/// Why omt cannot answer an interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotDeliverableReason {
    /// Answering would require inferring where a highlight sits.
    NotPositionIndependent,
    /// Submitting needs navigation omt cannot do safely.
    SubmitRequiresNavigation,
    /// The option list is longer than the accelerators available.
    TooManyOptions,
    /// The agent is on its alternate screen, where the accelerator is absent.
    AlternateScreen,
    /// The focused row takes text, so a digit would be typed rather than
    /// selected — a silent failure, which is why it is refused.
    FreeTextFocused,
    /// This agent exposes no way to answer at all.
    NoResponder,
}

/// Where an interaction is in its lifecycle.
///
/// Seven states, and the three in the middle are the point. Writing an answer
/// is not the same as the answer taking effect: when the delivery channel is a
/// UI omt does not own, a successful write proves nothing, so `Submitted` and
/// `Resolved` are different facts and only one of them is success.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum InteractionState {
    /// Waiting for someone.
    Open,
    /// A decision has been accepted but not yet written.
    ///
    /// Carries the response, so an interruption here can report *what* was
    /// lost rather than only that something was.
    Resolving {
        /// Who decided.
        by: Actor,
        /// When.
        at: Timestamp,
        /// What they decided.
        response: InteractionResponse,
    },
    /// The bytes went out. **Not** proof the agent received them.
    Submitted {
        /// Who decided.
        by: Actor,
        /// When it was written.
        at: Timestamp,
        /// What was written.
        response: InteractionResponse,
    },
    /// omt observed the agent record the answer. The only success state.
    Resolved {
        /// Who decided.
        by: Actor,
        /// When it was confirmed.
        at: Timestamp,
        /// What was decided.
        response: InteractionResponse,
    },
    /// Written, or committed and then lost, with no confirming observation.
    ///
    /// The response is preserved so a user can see what did not land, and it
    /// is never retried automatically.
    Undelivered {
        /// Who decided.
        by: Actor,
        /// When it was given up on.
        at: Timestamp,
        /// What was attempted.
        response: InteractionResponse,
        /// Why it is considered undelivered.
        reason: UndeliveredReason,
    },
    /// Withdrawn by the agent or by a timeout.
    Cancelled {
        /// Who or what withdrew it.
        by: Actor,
        /// When.
        at: Timestamp,
        /// Why.
        reason: CancelReason,
    },
    /// Nobody decided anything and the world moved on.
    Abandoned {
        /// When.
        at: Timestamp,
        /// What happened.
        detail: String,
    },
}

impl InteractionState {
    /// Whether this state can still be answered.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        matches!(self, Self::Open)
    }

    /// Whether the lifecycle is over.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Resolved { .. }
                | Self::Undelivered { .. }
                | Self::Cancelled { .. }
                | Self::Abandoned { .. }
        )
    }
}

/// Why an answer is considered undelivered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum UndeliveredReason {
    /// No confirming observation arrived inside the window.
    NotConfirmed,
    /// A completion arrived, but the agent recorded a *different* answer —
    /// almost always because a human answered at the keyboard first.
    ///
    /// Never retried, and surfaced with both answers, because this is also the
    /// signal that the delivery mechanism itself broke on a new agent version.
    AnsweredDifferently {
        /// What the agent actually recorded.
        observed: Box<InteractionResponse>,
    },
    /// The daemon restarted with a decision recorded but unwritten, or written
    /// and unconfirmed. Never retried — omt cannot know which.
    DaemonRestart,
    /// The gated transaction failed its preconditions.
    PreconditionFailed,
}

/// Why an interaction was withdrawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CancelReason {
    /// The agent withdrew the question itself.
    AgentWithdrew,
    /// The agent auto-advanced past it, with no actor involved.
    AgentAutoAdvanced,
    /// It timed out.
    Timeout,
    /// The session ended.
    SessionEnded,
}

/// What was answered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InteractionResponse {
    /// One answer per question.
    Choice {
        /// Per-question answers, positionally matching the questions.
        answers: Vec<ChoiceAnswer>,
    },
    /// A permission decision.
    Permission {
        /// Which option, by the agent's own id — not by kind, which can be
        /// ambiguous when two options share one.
        option: String,
        /// Modified arguments, where the responder supports editing before
        /// approving.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        updated_input: Option<serde_json::Value>,
    },
    /// A plan decision.
    Plan {
        /// What was decided.
        decision: PlanDecision,
        /// Optional accompanying note.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// Free text.
    Text {
        /// What was typed.
        text: String,
    },
    /// The escape hatch — what "cancel" actually is, since a human cannot
    /// withdraw an agent's question, only answer it with a refusal.
    Escaped,
}

/// One question's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ChoiceAnswer {
    /// The chosen labels, matching the agent's own spelling so the recorded
    /// answer can be compared against what was sent.
    pub labels: Vec<String>,
    /// Free text, where the question allowed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other: Option<String>,
    /// An accompanying comment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// What was decided about a plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanDecision {
    /// Go ahead.
    Approve,
    /// Do not.
    Reject,
    /// Change it first.
    RequestChanges,
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn now() -> Timestamp {
        Timestamp::UNIX_EPOCH
    }

    #[test]
    fn answerability_comes_from_deliverable_not_from_openness() {
        // The rule that stops a surface offering to answer something omt
        // cannot safely answer.
        let undeliverable = Deliverable::None {
            reason: NotDeliverableReason::TooManyOptions,
        };
        assert!(!undeliverable.is_answerable());
        assert!(Deliverable::Native.is_answerable());
        assert!(
            Deliverable::Synthetic {
                requires_token: true
            }
            .is_answerable()
        );
    }

    #[test]
    fn an_undeliverable_interaction_carries_its_reason() {
        // So a UI can say *why*, rather than "unavailable".
        let d = Deliverable::None {
            reason: NotDeliverableReason::FreeTextFocused,
        };
        let json = serde_json::to_string(&d).expect("serialize");
        assert!(json.contains("free_text_focused"), "{json}");
    }

    #[test]
    fn submitted_is_not_resolved() {
        // Writing an answer is not the answer taking effect.
        let submitted = InteractionState::Submitted {
            by: Actor::Local,
            at: now(),
            response: InteractionResponse::Escaped,
        };
        assert!(!submitted.is_terminal(), "Submitted is still in flight");
        assert!(!submitted.is_open());
    }

    #[test]
    fn resolving_carries_the_response_so_a_crash_can_report_it() {
        let s = InteractionState::Resolving {
            by: Actor::Local,
            at: now(),
            response: InteractionResponse::Text { text: "yes".into() },
        };
        let InteractionState::Resolving { response, .. } = &s else {
            panic!("expected Resolving");
        };
        assert_eq!(response, &InteractionResponse::Text { text: "yes".into() });
    }

    #[test]
    fn terminal_states_are_exactly_the_four() {
        let by = Actor::Local;
        let r = InteractionResponse::Escaped;
        assert!(
            InteractionState::Resolved {
                by: by.clone(),
                at: now(),
                response: r.clone()
            }
            .is_terminal()
        );
        assert!(
            InteractionState::Undelivered {
                by: by.clone(),
                at: now(),
                response: r.clone(),
                reason: UndeliveredReason::NotConfirmed
            }
            .is_terminal()
        );
        assert!(
            InteractionState::Cancelled {
                by,
                at: now(),
                reason: CancelReason::Timeout
            }
            .is_terminal()
        );
        assert!(
            InteractionState::Abandoned {
                at: now(),
                detail: "x".into()
            }
            .is_terminal()
        );
        assert!(!InteractionState::Open.is_terminal());
    }

    #[test]
    fn answered_differently_preserves_what_the_agent_recorded() {
        // The mismatch case: a human answered at the keyboard first, with
        // something else. Both answers must survive so a user can see it, and
        // so a broken accelerator is loud rather than silent.
        let reason = UndeliveredReason::AnsweredDifferently {
            observed: Box::new(InteractionResponse::Text { text: "no".into() }),
        };
        let json = serde_json::to_string(&reason).expect("serialize");
        let back: UndeliveredReason = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(reason, back);
    }

    #[test]
    fn a_permission_answer_names_the_option_by_id() {
        // Answering by kind alone is ambiguous when two options are both
        // "allow" and differ only in scope.
        let r = InteractionResponse::Permission {
            option: "allow_for_session".into(),
            updated_input: None,
        };
        let json = serde_json::to_string(&r).expect("serialize");
        assert!(json.contains("allow_for_session"), "{json}");
    }

    #[test]
    fn escaped_is_how_cancel_is_expressed() {
        // A human cannot withdraw an agent's question, only answer it with a
        // refusal — so there is no separate cancel, and this is it.
        let json = serde_json::to_string(&InteractionResponse::Escaped).expect("serialize");
        assert_eq!(json, r#"{"type":"escaped"}"#);
    }

    #[test]
    fn the_whole_interaction_round_trips() {
        let i = Interaction {
            id: InteractionId::new(),
            session: SessionId::new(),
            binding: BindingId::new(),
            kind: InteractionKind::Choice {
                questions: vec![ChoiceQuestion {
                    question: "Which?".into(),
                    header: "Pick".into(),
                    multi_select: false,
                    options: vec![ChoiceOption {
                        label: "A".into(),
                        description: None,
                    }],
                    allow_free_text: true,
                }],
            },
            deliverable: Deliverable::Synthetic {
                requires_token: true,
            },
            state: InteractionState::Open,
            opened_at: now(),
            expires_at: None,
        };
        let json = serde_json::to_string(&i).expect("serialize");
        let back: Interaction = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(i, back);
    }
}
