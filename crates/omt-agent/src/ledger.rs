//! The interaction ledger: exactly-once resolution, confirmed by observation.
//!
//! Two properties this file exists to hold, and both are the kind that fail
//! quietly.
//!
//! **Exactly once.** A phone and a terminal can answer the same card in the
//! same instant. The winner is decided by a compare-and-swap; the loser is told
//! who beat it, so every surface re-renders "answered by someone else" instead
//! of two answers going out.
//!
//! **Confirmed, never asserted.** An answer delivered as keystrokes goes into a
//! UI omt does not own, so a successful write proves nothing at all. Bytes
//! going out is `Submitted`. Only an observation that the agent *recorded* the
//! answer promotes it to `Resolved` — and if the local user answered by hand
//! with a different option a moment earlier, that observation says so, and the
//! interaction is `Undelivered`, loudly.

use std::collections::BTreeMap;

use omt_events::{
    Deliverable, Interaction, InteractionResponse, InteractionState, UndeliveredReason,
};
use omt_types::{Actor, InteractionId, Timestamp};

/// How long an answer may go unconfirmed before it is reported as lost.
///
/// Bounded because the alternative is a card that lingers: a denied permission
/// may emit no completion at all, and an answer that lands whenever the agent
/// next reads input lands in whatever it is doing by then.
pub const DEFAULT_CONFIRMATION_MS: u64 = 10_000;

/// What went wrong resolving an interaction.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LedgerError {
    /// No such interaction.
    #[error("no interaction {0}")]
    Unknown(InteractionId),
    /// Somebody else answered first.
    ///
    /// Carries the winning actor rather than a rendered string, so the losing
    /// surface can show "answered on your phone" with the device's real name
    /// instead of parsing a message back apart.
    #[error("already answered by {by:?}")]
    Conflict {
        /// Who won, or `None` when the interaction left `Open` without anyone
        /// claiming it.
        by: Option<Box<Actor>>,
    },
    /// omt cannot deliver an answer to this one at all.
    #[error("this interaction is not answerable: {reason}")]
    NotDeliverable {
        /// Why not.
        reason: String,
    },
}

/// An observation that an agent recorded some answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    /// The tool call this concerns, where the agent gives one.
    pub call: Option<String>,
    /// Whether this is a completion rather than progress.
    ///
    /// Progress on the right call is not confirmation: a tool that has started
    /// is not a question that has been answered.
    pub terminal: bool,
    /// What the agent recorded, as a comparable selection rather than prose.
    pub recorded: Option<InteractionResponse>,
}

/// Why an observation did or did not confirm a submitted answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirmation {
    /// It confirms: same call, terminal, and the same answer.
    Confirms,
    /// It concerns a different call.
    DifferentCall,
    /// It is progress, not a completion.
    NotTerminal,
    /// It records a *different* answer — the local user answered by hand.
    ///
    /// The failure the whole three-part rule exists to catch, and the one that
    /// would otherwise report success for an approval nobody gave.
    AnsweredDifferently,
    /// The recorded answer could not be read.
    ///
    /// Treated as *not* confirming: an unparsable record is not evidence.
    Unreadable,
}

/// Whether an observation confirms a specific submitted answer.
///
/// All three conditions, not any of them. The third is the one that matters and
/// it is not implied by the first two: a completion on the right call proves a
/// question was answered, not that it was answered with *this*.
#[must_use]
pub fn confirms(
    obs: &Observation,
    opened_from_call: Option<&str>,
    submitted: &InteractionResponse,
) -> Confirmation {
    if let (Some(expected), Some(got)) = (opened_from_call, obs.call.as_deref())
        && expected != got
    {
        return Confirmation::DifferentCall;
    }
    if !obs.terminal {
        return Confirmation::NotTerminal;
    }
    let Some(recorded) = &obs.recorded else {
        return Confirmation::Unreadable;
    };
    if same_selection(recorded, submitted) {
        Confirmation::Confirms
    } else {
        Confirmation::AnsweredDifferently
    }
}

/// Compare on the recorded *selection*, never on rendered prose.
///
/// Two surfaces render the same choice differently, and an agent's own record
/// is a sentence. Comparing text would make a formatting difference look like a
/// different answer — and, worse, make a different answer look the same after
/// normalization.
fn same_selection(a: &InteractionResponse, b: &InteractionResponse) -> bool {
    match (a, b) {
        (
            InteractionResponse::Choice { answers: x },
            InteractionResponse::Choice { answers: y },
        ) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y)
                    .all(|(p, q)| p.labels == q.labels && p.other == q.other)
        }
        // Compared on the agent's own option id rather than on a kind: two
        // options can share a kind, and picking the wrong one of those is
        // exactly the mistake that must not read as agreement.
        (
            InteractionResponse::Permission { option: x, .. },
            InteractionResponse::Permission { option: y, .. },
        ) => x == y,
        (InteractionResponse::Text { text: x }, InteractionResponse::Text { text: y }) => x == y,
        (
            InteractionResponse::Plan { decision: x, .. },
            InteractionResponse::Plan { decision: y, .. },
        ) => x == y,
        (InteractionResponse::Escaped, InteractionResponse::Escaped) => true,
        // Different kinds entirely: whatever the agent recorded, it was not
        // this.
        _ => false,
    }
}

/// One entry, plus what the ledger needs that the wire type does not carry.
#[derive(Debug, Clone)]
struct Entry {
    interaction: Interaction,
    /// The tool call it was opened from, for correlation.
    opened_from_call: Option<String>,
    /// When the bytes went out.
    submitted_at: Option<u64>,
    /// How long to wait for confirmation.
    confirmation_ms: u64,
}

/// Every interaction an instance knows about.
#[derive(Debug, Default)]
pub struct Ledger {
    entries: BTreeMap<InteractionId, Entry>,
}

impl Ledger {
    /// An empty ledger.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a newly raised interaction.
    pub fn open(&mut self, interaction: Interaction, opened_from_call: Option<String>) {
        self.entries.insert(
            interaction.id,
            Entry {
                interaction,
                opened_from_call,
                submitted_at: None,
                confirmation_ms: DEFAULT_CONFIRMATION_MS,
            },
        );
    }

    /// Look one up.
    #[must_use]
    pub fn get(&self, id: InteractionId) -> Option<&Interaction> {
        self.entries.get(&id).map(|e| &e.interaction)
    }

    /// Everything still waiting for someone.
    #[must_use]
    pub fn open_interactions(&self) -> Vec<&Interaction> {
        self.entries
            .values()
            .map(|e| &e.interaction)
            .filter(|i| matches!(i.state, InteractionState::Open))
            .collect()
    }

    /// Claim an interaction for an actor.
    ///
    /// The compare-and-swap. Exactly one caller can move an interaction out of
    /// `Open`; every other gets the winner's identity back, so the losing
    /// surface can say who answered rather than silently doing nothing.
    ///
    /// # Errors
    /// Fails if the interaction is unknown, is not answerable at all, or has
    /// already been claimed.
    pub fn resolve(
        &mut self,
        id: InteractionId,
        by: Actor,
        at: Timestamp,
        response: InteractionResponse,
    ) -> Result<&Interaction, LedgerError> {
        let entry = self.entries.get_mut(&id).ok_or(LedgerError::Unknown(id))?;

        // Answerability is a property of the *deliverable*, not of the state.
        // Checking `Open` alone would let a surface offer to answer something
        // omt has no channel for, and the user would find out by the wrong
        // option being selected.
        if let Deliverable::None { reason } = &entry.interaction.deliverable {
            return Err(LedgerError::NotDeliverable {
                reason: format!("{reason:?}"),
            });
        }

        match &entry.interaction.state {
            InteractionState::Open => {
                entry.interaction.state = InteractionState::Resolving { by, at, response };
                Ok(&entry.interaction)
            }
            other => Err(LedgerError::Conflict {
                by: claimant(other),
            }),
        }
    }

    /// Record that the bytes went out.
    ///
    /// Deliberately not "delivered". For a synthetic responder the sink is a UI
    /// omt does not own, so this is the last thing omt actually knows.
    ///
    /// # Errors
    /// Fails if the interaction is unknown or was not being resolved.
    pub fn submitted(&mut self, id: InteractionId, now_ms: u64) -> Result<(), LedgerError> {
        let entry = self.entries.get_mut(&id).ok_or(LedgerError::Unknown(id))?;
        let InteractionState::Resolving { by, at, response } = entry.interaction.state.clone()
        else {
            return Err(LedgerError::Conflict {
                by: claimant(&entry.interaction.state),
            });
        };
        entry.submitted_at = Some(now_ms);
        entry.interaction.state = InteractionState::Submitted { by, at, response };

        // A native responder's own reply *is* the confirming observation —
        // the channel that carried the answer acknowledged it. There is
        // nothing further to wait for.
        if matches!(entry.interaction.deliverable, Deliverable::Native) {
            let InteractionState::Submitted { by, at, response } = entry.interaction.state.clone()
            else {
                return Ok(());
            };
            entry.interaction.state = InteractionState::Resolved { by, at, response };
        }
        Ok(())
    }

    /// Offer an observation, and see whether it settles anything.
    ///
    /// Returns the interactions whose state changed, so the caller can
    /// broadcast exactly those rather than re-sending everything.
    pub fn observe(&mut self, obs: &Observation) -> Vec<InteractionId> {
        let mut changed = Vec::new();
        for (id, entry) in &mut self.entries {
            let InteractionState::Submitted { by, at, response } = entry.interaction.state.clone()
            else {
                continue;
            };
            match confirms(obs, entry.opened_from_call.as_deref(), &response) {
                Confirmation::Confirms => {
                    entry.interaction.state = InteractionState::Resolved { by, at, response };
                    changed.push(*id);
                }
                Confirmation::AnsweredDifferently => {
                    let observed = obs.recorded.clone().unwrap_or(InteractionResponse::Escaped);
                    // Loud, never silent, and never retried: this is the
                    // designed signal that the accelerator broke on a new agent
                    // version, and a retry would put a keystroke into whatever
                    // the agent is doing now.
                    entry.interaction.state = InteractionState::Undelivered {
                        by,
                        at,
                        response,
                        // Carries what the agent actually recorded, so a
                        // surface can show both answers side by side rather
                        // than only reporting that something went wrong.
                        reason: UndeliveredReason::AnsweredDifferently {
                            observed: Box::new(observed),
                        },
                    };
                    changed.push(*id);
                }
                // Not about this interaction, or not evidence.
                Confirmation::DifferentCall
                | Confirmation::NotTerminal
                | Confirmation::Unreadable => {}
            }
        }
        changed
    }

    /// Expire anything whose confirmation window has passed.
    ///
    /// Returns what expired. An interaction left open indefinitely invites an
    /// answer that lands somewhere else entirely.
    pub fn expire(&mut self, now_ms: u64) -> Vec<InteractionId> {
        let mut expired = Vec::new();
        for (id, entry) in &mut self.entries {
            let InteractionState::Submitted { by, at, response } = entry.interaction.state.clone()
            else {
                continue;
            };
            let Some(sent) = entry.submitted_at else {
                continue;
            };
            if now_ms.saturating_sub(sent) >= entry.confirmation_ms {
                entry.interaction.state = InteractionState::Undelivered {
                    by,
                    at,
                    response,
                    reason: UndeliveredReason::NotConfirmed,
                };
                expired.push(*id);
            }
        }
        expired
    }
}

fn claimant(state: &InteractionState) -> Option<Box<Actor>> {
    match state {
        InteractionState::Resolving { by, .. }
        | InteractionState::Submitted { by, .. }
        | InteractionState::Resolved { by, .. }
        | InteractionState::Undelivered { by, .. }
        | InteractionState::Cancelled { by, .. } => Some(Box::new(by.clone())),
        // Nobody claimed these, so naming an actor would invent one.
        InteractionState::Open | InteractionState::Abandoned { .. } => None,
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
    use omt_events::{ChoiceAnswer, InteractionKind, NotDeliverableReason};
    use omt_types::{BindingId, DeviceId, IdentityId, SessionId};

    fn phone() -> (Actor, DeviceId) {
        let device = DeviceId::new();
        (
            Actor::Remote {
                identity: IdentityId::new(),
                device,
            },
            device,
        )
    }

    fn actor(name: &str) -> Actor {
        if name == "local" {
            Actor::Local
        } else {
            phone().0
        }
    }

    fn interaction(deliverable: Deliverable) -> Interaction {
        Interaction {
            id: InteractionId::new(),
            session: SessionId::new(),
            binding: BindingId::new(),
            kind: InteractionKind::Permission {
                tool: "Bash".to_owned(),
                input: serde_json::json!({"command": "rm -rf /tmp/x"}),
                command: Some("rm -rf /tmp/x".to_owned()),
                options: Vec::new(),
            },
            deliverable,
            state: InteractionState::Open,
            opened_at: Timestamp::now(),
            expires_at: None,
        }
    }

    fn allow() -> InteractionResponse {
        InteractionResponse::Permission {
            option: "allow".to_owned(),
            updated_input: None,
        }
    }

    fn deny() -> InteractionResponse {
        InteractionResponse::Permission {
            option: "deny".to_owned(),
            updated_input: None,
        }
    }

    fn synthetic_ledger() -> (Ledger, InteractionId) {
        let mut l = Ledger::new();
        let i = interaction(Deliverable::Synthetic {
            requires_token: true,
        });
        let id = i.id;
        l.open(i, Some("toolu_1".to_owned()));
        (l, id)
    }

    #[test]
    fn exactly_one_actor_can_claim_an_interaction() {
        // The phone and the terminal both answering at once.
        let (mut l, id) = synthetic_ledger();
        l.resolve(id, actor("phone"), Timestamp::now(), allow())
            .expect("the first claim wins");
        let err = l
            .resolve(id, actor("laptop"), Timestamp::now(), deny())
            .expect_err("the second must lose");
        assert!(matches!(err, LedgerError::Conflict { .. }), "{err:?}");
    }

    #[test]
    fn the_loser_is_told_who_won() {
        // So the losing surface can re-render "answered by someone else"
        // rather than appearing to do nothing.
        let (mut l, id) = synthetic_ledger();
        let (winner, won_by) = phone();
        l.resolve(id, winner, Timestamp::now(), allow())
            .expect("first");
        let LedgerError::Conflict { by } = l
            .resolve(id, actor("laptop"), Timestamp::now(), deny())
            .expect_err("second")
        else {
            panic!("expected a conflict");
        };
        let Some(winner) = by else {
            panic!("the winner must be identified");
        };
        assert!(
            matches!(*winner, Actor::Remote { device, .. } if device == won_by),
            "the losing surface can name the device that answered"
        );
    }

    #[test]
    fn an_undeliverable_interaction_cannot_be_answered_even_while_open() {
        // Answerability comes from the deliverable, never from the state. A
        // surface that keyed on Open would offer a button omt cannot honour.
        let mut l = Ledger::new();
        let i = interaction(Deliverable::None {
            reason: NotDeliverableReason::NoResponder,
        });
        let id = i.id;
        l.open(i, None);
        let err = l
            .resolve(id, actor("phone"), Timestamp::now(), allow())
            .expect_err("must refuse");
        assert!(matches!(err, LedgerError::NotDeliverable { .. }), "{err:?}");
    }

    #[test]
    fn submitting_is_not_resolving_for_a_synthetic_delivery() {
        // The sink is a UI omt does not own, so a successful write proves
        // nothing. This is the single most important distinction here.
        let (mut l, id) = synthetic_ledger();
        l.resolve(id, actor("phone"), Timestamp::now(), allow())
            .expect("claim");
        l.submitted(id, 1_000).expect("bytes out");
        assert!(
            matches!(
                l.get(id).expect("present").state,
                InteractionState::Submitted { .. }
            ),
            "still only submitted"
        );
    }

    #[test]
    fn a_native_delivery_is_resolved_immediately() {
        // The channel that carried the answer acknowledged it; there is
        // nothing further to observe.
        let mut l = Ledger::new();
        let i = interaction(Deliverable::Native);
        let id = i.id;
        l.open(i, None);
        l.resolve(id, actor("phone"), Timestamp::now(), allow())
            .expect("claim");
        l.submitted(id, 1_000).expect("sent");
        assert!(matches!(
            l.get(id).expect("present").state,
            InteractionState::Resolved { .. }
        ));
    }

    #[test]
    fn an_observation_of_the_same_answer_resolves_it() {
        let (mut l, id) = synthetic_ledger();
        l.resolve(id, actor("phone"), Timestamp::now(), allow())
            .expect("claim");
        l.submitted(id, 1_000).expect("sent");
        let changed = l.observe(&Observation {
            call: Some("toolu_1".to_owned()),
            terminal: true,
            recorded: Some(allow()),
        });
        assert_eq!(changed, [id]);
        assert!(matches!(
            l.get(id).expect("present").state,
            InteractionState::Resolved { .. }
        ));
    }

    #[test]
    fn an_observation_of_a_different_answer_is_undelivered_not_resolved() {
        // The failure the three-part rule exists for: the local user answered
        // by hand, with a different option, a moment before omt's bytes landed.
        // Rules one and two alone would see a completion on the right call and
        // report success for an approval nobody gave.
        let (mut l, id) = synthetic_ledger();
        l.resolve(id, actor("phone"), Timestamp::now(), allow())
            .expect("claim");
        l.submitted(id, 1_000).expect("sent");
        l.observe(&Observation {
            call: Some("toolu_1".to_owned()),
            terminal: true,
            recorded: Some(deny()),
        });
        assert!(
            matches!(
                l.get(id).expect("present").state,
                InteractionState::Undelivered {
                    reason: UndeliveredReason::AnsweredDifferently { .. },
                    ..
                }
            ),
            "{:?}",
            l.get(id).expect("present").state
        );
    }

    #[test]
    fn progress_on_the_right_call_does_not_confirm() {
        // A tool that has started is not a question that has been answered.
        let obs = Observation {
            call: Some("toolu_1".to_owned()),
            terminal: false,
            recorded: Some(allow()),
        };
        assert_eq!(
            confirms(&obs, Some("toolu_1"), &allow()),
            Confirmation::NotTerminal
        );
    }

    #[test]
    fn a_completion_on_another_call_does_not_confirm() {
        let obs = Observation {
            call: Some("toolu_2".to_owned()),
            terminal: true,
            recorded: Some(allow()),
        };
        assert_eq!(
            confirms(&obs, Some("toolu_1"), &allow()),
            Confirmation::DifferentCall
        );
    }

    #[test]
    fn an_unreadable_record_is_not_evidence() {
        // A parse failure must not be read as agreement.
        let obs = Observation {
            call: Some("toolu_1".to_owned()),
            terminal: true,
            recorded: None,
        };
        assert_eq!(
            confirms(&obs, Some("toolu_1"), &allow()),
            Confirmation::Unreadable
        );
    }

    #[test]
    fn an_unreadable_record_leaves_the_interaction_to_expire() {
        // Rather than resolving it on the strength of a record nobody could
        // read.
        let (mut l, id) = synthetic_ledger();
        l.resolve(id, actor("phone"), Timestamp::now(), allow())
            .expect("claim");
        l.submitted(id, 1_000).expect("sent");
        assert!(
            l.observe(&Observation {
                call: Some("toolu_1".to_owned()),
                terminal: true,
                recorded: None,
            })
            .is_empty()
        );
        assert!(matches!(
            l.get(id).expect("present").state,
            InteractionState::Submitted { .. }
        ));
    }

    #[test]
    fn choices_are_compared_on_labels_not_on_prose() {
        // Two surfaces render the same choice differently, and the agent's own
        // record is a sentence. Comparing text would make formatting look like
        // disagreement.
        let a = InteractionResponse::Choice {
            answers: vec![ChoiceAnswer {
                labels: vec!["Use PostgreSQL".to_owned()],
                other: None,
                comment: Some("because of the extensions".to_owned()),
            }],
        };
        let b = InteractionResponse::Choice {
            answers: vec![ChoiceAnswer {
                labels: vec!["Use PostgreSQL".to_owned()],
                other: None,
                // A comment is context, not a different answer.
                comment: None,
            }],
        };
        assert!(same_selection(&a, &b));
    }

    #[test]
    fn a_different_label_is_a_different_answer() {
        let a = InteractionResponse::Choice {
            answers: vec![ChoiceAnswer {
                labels: vec!["Use PostgreSQL".to_owned()],
                other: None,
                comment: None,
            }],
        };
        let b = InteractionResponse::Choice {
            answers: vec![ChoiceAnswer {
                labels: vec!["Use SQLite".to_owned()],
                other: None,
                comment: None,
            }],
        };
        assert!(!same_selection(&a, &b));
    }

    #[test]
    fn a_response_of_another_kind_never_confirms() {
        assert!(!same_selection(
            &allow(),
            &InteractionResponse::Text {
                text: "allow".to_owned()
            }
        ));
    }

    #[test]
    fn an_unconfirmed_answer_expires_rather_than_lingering() {
        // A denied permission may emit no completion at all, and a card left
        // open invites an answer that lands in whatever the agent is doing by
        // then.
        let (mut l, id) = synthetic_ledger();
        l.resolve(id, actor("phone"), Timestamp::now(), allow())
            .expect("claim");
        l.submitted(id, 1_000).expect("sent");
        assert!(l.expire(1_500).is_empty(), "not yet");
        let expired = l.expire(1_000 + DEFAULT_CONFIRMATION_MS);
        assert_eq!(expired, [id]);
        assert!(matches!(
            l.get(id).expect("present").state,
            InteractionState::Undelivered {
                reason: UndeliveredReason::NotConfirmed,
                ..
            }
        ));
    }

    #[test]
    fn an_undelivered_interaction_preserves_what_was_submitted() {
        // So a surface can show the user what they said and let them retype it
        // rather than losing it.
        let (mut l, id) = synthetic_ledger();
        l.resolve(id, actor("phone"), Timestamp::now(), allow())
            .expect("claim");
        l.submitted(id, 0).expect("sent");
        l.expire(DEFAULT_CONFIRMATION_MS);
        let InteractionState::Undelivered { response, .. } = &l.get(id).expect("present").state
        else {
            panic!("expected undelivered");
        };
        assert_eq!(*response, allow());
    }

    #[test]
    fn a_resolved_interaction_is_not_expired_afterwards() {
        // Terminal means terminal; re-expiring would rewrite a settled answer.
        let (mut l, id) = synthetic_ledger();
        l.resolve(id, actor("phone"), Timestamp::now(), allow())
            .expect("claim");
        l.submitted(id, 0).expect("sent");
        l.observe(&Observation {
            call: Some("toolu_1".to_owned()),
            terminal: true,
            recorded: Some(allow()),
        });
        assert!(l.expire(999_999).is_empty());
        assert!(matches!(
            l.get(id).expect("present").state,
            InteractionState::Resolved { .. }
        ));
    }

    #[test]
    fn an_agent_with_no_call_id_correlates_on_the_interaction_alone() {
        // Not every agent gives one. The correlation is weaker and is marked
        // low-confidence upstream, but it must still work.
        let mut l = Ledger::new();
        let i = interaction(Deliverable::Synthetic {
            requires_token: true,
        });
        let id = i.id;
        l.open(i, None);
        l.resolve(id, actor("phone"), Timestamp::now(), allow())
            .expect("claim");
        l.submitted(id, 0).expect("sent");
        let changed = l.observe(&Observation {
            call: None,
            terminal: true,
            recorded: Some(allow()),
        });
        assert_eq!(changed, [id]);
    }

    #[test]
    fn only_open_interactions_are_listed_as_open() {
        let (mut l, id) = synthetic_ledger();
        assert_eq!(l.open_interactions().len(), 1);
        l.resolve(id, actor("phone"), Timestamp::now(), allow())
            .expect("claim");
        assert!(
            l.open_interactions().is_empty(),
            "a claimed card is no longer awaiting anyone"
        );
    }

    #[test]
    fn resolving_something_unknown_says_so() {
        let mut l = Ledger::new();
        let err = l
            .resolve(
                InteractionId::new(),
                actor("phone"),
                Timestamp::now(),
                allow(),
            )
            .expect_err("must refuse");
        assert!(matches!(err, LedgerError::Unknown(_)));
    }
}
