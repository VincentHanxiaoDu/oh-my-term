//! Where an agent's own hooks arrive.
//!
//! The hook binary reports what its agent is about to do, before the agent
//! draws its card. Everything downstream of that — the transcript, the
//! question card on a phone, the "waiting for a person" state — was already
//! built and had nothing feeding it: the adapters normalize, the transcripts
//! observe, the roster tracks threads, and no code path connected the socket
//! to any of them. This is that path.
//!
//! Two rules, both of them about not making things worse than no hook at all:
//!
//! * A session is used only when it is *known*. The hook carries `$OMT_SESSION`
//!   when omt spawned the agent; when it does not, the observation is dropped
//!   rather than attributed by guesswork. A wrongly-attributed observation puts
//!   one agent's question in another's conversation.
//! * The answer is always [`HookDirective::Proceed`]. omt mirrors an agent's
//!   card; it does not hold the agent's hand up while somebody's phone is in
//!   another room.

use omt_events::{AgentEvent, EventPayload, EventSourceTag};
use omt_proto::{HookAck, HookDirective, HookEvent};
use omt_types::{SessionId, Tier, Timestamp};

use crate::state::State;

/// Take one hook report into an instance, and say what the hook should do.
///
/// Never fails outward: a hook waiting on this reply has an agent waiting on
/// it, so every internal problem still answers `proceed`.
#[must_use]
pub fn ingest(state: &State, event: &HookEvent) -> HookAck {
    absorb(state, event);
    HookAck {
        nonce: event.nonce,
        directive: HookDirective::Proceed,
    }
}

/// The part that can fail, kept apart from the part that must not.
fn absorb(state: &State, event: &HookEvent) -> Option<()> {
    // Only a session the hook was *told*. See the module note: the alternative
    // to dropping an unattributed observation is inventing an attribution.
    let session: SessionId = event.correlation.session?;

    let adapters = omt_agent_adapters::builtin();
    let adapter = adapters.get(event.agent)?;
    // The whole payload where the hook sent one, so an adapter can read fields
    // omt does not model. `tool_input` is lifted to the top level because that
    // is where the adapters look for it.
    let raw = payload_of(event);
    let payloads = adapter.normalize(&event.event, &raw).ok()?;
    if payloads.is_empty() {
        return None;
    }

    let mut instance = state.lock().ok()?;
    // A hook for a session this instance does not have is not this instance's
    // hook. It belongs to another omt on the same machine, and absorbing it
    // would put its agent's words in this one's conversation.
    instance.session(session)?;
    let binding = instance.binding_for(session);
    // What the agent calls this conversation. The only thing that can be
    // handed back to it to *continue* rather than to start again, and it
    // arrives here or nowhere: nothing else omt can see knows it.
    if let Some(agent_session) = &event.agent_session {
        instance.observe_agent_session(session, agent_session.clone());
    }

    let mode = instance.session(session).map(|s| s.mode);

    let mut transcripts = state.transcripts().ok()?;
    for payload in payloads {
        transcripts.entry(session).observe(&payload);
        raise_card(&mut instance, session, binding, mode, &payload);
        instance.observe_usage(session, &payload);
        let agent_event = AgentEvent {
            session,
            binding,
            agent: event.agent,
            agent_version: event.agent_version.clone(),
            agent_session: event.agent_session.clone(),
            thread: None,
            // The instance assigns the real position when it emits. This one
            // is a placeholder and is overwritten there — positions are minted
            // in exactly one place.
            seq: omt_types::Seq::new(0),
            ts: Timestamp::now(),
            // A hook is the agent telling omt what it is doing, which is the
            // second-strongest thing omt can know and the reason the tier
            // ladder has a rung for it.
            tier: Tier::Hook,
            payload,
        };
        instance.observe_thread(session, &agent_event);
        instance.emit(
            session,
            EventSourceTag::Hook,
            EventPayload::Agent {
                event: Box::new(agent_event),
            },
        );
    }
    Some(())
}

/// Put a question on the ledger, so every surface can see somebody is being
/// waited for.
///
/// The card the agent draws locally is the real one; this is the record of it
/// — which is what a phone lists, what the panel counts, and what `omt ls`
/// reports. Whether it can be *answered* from elsewhere is a separate
/// question, and it is answered honestly below rather than optimistically:
/// offering an answer omt cannot deliver is worse than saying it cannot,
/// because somebody presses it and believes the agent heard.
fn raise_card(
    instance: &mut omt_daemon::Instance,
    session: SessionId,
    binding: omt_types::BindingId,
    mode: Option<omt_session::SessionMode>,
    payload: &omt_events::AgentPayload,
) {
    let omt_events::AgentPayload::Question { call, questions } = payload else {
        return;
    };
    let interaction = omt_events::Interaction {
        id: omt_types::InteractionId::new(),
        session,
        binding,
        kind: omt_events::InteractionKind::Choice {
            questions: questions.clone(),
        },
        // An agent on a pty draws its card wherever its cursor happens to be,
        // and selecting an option means moving a highlight omt cannot see. A
        // surface that offered the choice would be guessing, and the way
        // somebody finds out is the wrong option being chosen for them. So the
        // card is shown, read-only, with the reason and a route to the pane.
        // A protocol-backed session has a real response channel and no such
        // problem.
        deliverable: if matches!(mode, Some(omt_session::SessionMode::Pty)) {
            omt_events::Deliverable::None {
                reason: omt_events::NotDeliverableReason::NotPositionIndependent,
            }
        } else {
            omt_events::Deliverable::Native
        },
        state: omt_events::InteractionState::Open,
        opened_at: Timestamp::now(),
        expires_at: None,
    };
    // The strongest possible statement that somebody is being waited for: the
    // agent said so itself. Without this the pane's state comes from whatever
    // the screen rules made of the box it painted, and a question read as
    // "working" is the difference between noticing an agent needs you and
    // finding out twenty minutes later.
    instance.observe_agent(
        binding,
        omt_agent::SourceReading {
            tier: Tier::Hook,
            state: omt_types::AgentState::Blocked {
                reason: omt_types::BlockReason::Question,
                interaction: Some(interaction.id),
            },
            at: crate::run::monotonic_ms(),
            // Not authoritative: a hook reports events, not a lifecycle. It
            // cannot say when the question was answered, so suspending the
            // other sources would leave a pane blocked forever on a card
            // somebody dealt with in the pane itself.
            authoritative: false,
        },
    );

    let announced = interaction.clone();
    instance.ledger.open(interaction, Some(call.clone()));
    instance.emit(
        session,
        EventSourceTag::Hook,
        EventPayload::Interaction {
            interaction: Box::new(announced),
        },
    );
}

/// The JSON an adapter's `normalize` reads.
fn payload_of(event: &HookEvent) -> serde_json::Value {
    let mut raw = event
        .raw
        .clone()
        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
    if let Some(object) = raw.as_object_mut() {
        // Set from the typed fields rather than trusted from `raw`: the hook
        // parsed these, and a `raw` that disagreed with them would be the one
        // the adapter believed.
        if let Some(name) = &event.tool_name {
            object.insert("tool_name".to_owned(), name.clone().into());
        }
        if let Some(id) = &event.tool_use_id {
            object.insert("tool_use_id".to_owned(), id.clone().into());
        }
        if let Some(input) = &event.tool_input {
            object.insert("tool_input".to_owned(), input.clone());
        }
    }
    raw
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use omt_proto::HookCorrelation;
    use omt_session::{SessionKind, SessionMode};
    use omt_types::AgentKind;

    use super::*;

    fn instance_with_session() -> (State, SessionId) {
        let mut instance = omt_daemon::Instance::new();
        let workspace = instance.open_workspace("/w").expect("workspace");
        let session = instance
            .create_session(
                workspace,
                SessionKind::Agent {
                    kind: AgentKind::ClaudeCode,
                },
                SessionMode::Pty,
            )
            .expect("session");
        (State::new(instance), session)
    }

    fn ask(session: Option<SessionId>) -> HookEvent {
        HookEvent {
            nonce: 7,
            hook_proto: 1,
            agent: AgentKind::ClaudeCode,
            agent_version: None,
            agent_session: None,
            event: "PreToolUse".to_owned(),
            tool_use_id: Some("t1".to_owned()),
            tool_name: Some("AskUserQuestion".to_owned()),
            tool_input: Some(serde_json::json!({
                "questions": [{
                    "question": "Which database?",
                    "header": "Database",
                    "multiSelect": false,
                    "options": [
                        {"label": "Postgres", "description": "the safe one"},
                        {"label": "SQLite", "description": "the small one"}
                    ]
                }]
            })),
            raw: None,
            truncated: None,
            correlation: HookCorrelation {
                instance: None,
                session,
                pid: 1,
                ppid: 2,
                cwd: "/w".to_owned(),
            },
            deadline_ms: 400,
        }
    }

    #[test]
    fn a_question_reaches_the_session_that_asked_it() {
        // The whole point of the hook path: the card the agent is about to
        // draw locally is mirrored to everything else before it is drawn.
        let (state, session) = instance_with_session();
        let ack = ingest(&state, &ask(Some(session)));
        assert_eq!(ack.directive, HookDirective::Proceed);
        let transcripts = state.transcripts().expect("transcripts");
        let entries = transcripts.get(session).expect("a transcript").entries();
        assert!(
            !entries.is_empty(),
            "the hook was accepted and nothing was recorded"
        );
    }

    #[test]
    fn an_unattributed_hook_is_dropped_rather_than_guessed_at() {
        // A wrongly-attributed observation puts one agent's question in
        // another agent's conversation, which nobody can detect afterwards.
        let (state, session) = instance_with_session();
        let ack = ingest(&state, &ask(None));
        assert_eq!(ack.directive, HookDirective::Proceed);
        assert!(
            state
                .transcripts()
                .expect("transcripts")
                .get(session)
                .is_none()
        );
    }

    #[test]
    fn a_hook_for_somebody_elses_session_is_not_absorbed() {
        // Two omt instances share a machine. Believing a session id this
        // instance has never seen would be believing another one's agent.
        let (state, _) = instance_with_session();
        let stranger = SessionId::new();
        let _ = ingest(&state, &ask(Some(stranger)));
        assert!(
            state
                .transcripts()
                .expect("transcripts")
                .get(stranger)
                .is_none()
        );
    }

    #[test]
    fn an_event_the_adapter_has_no_mapping_for_still_lets_the_agent_proceed() {
        // The agent is blocked on this reply. An event omt does not recognize
        // must not cost it a deadline.
        let (state, session) = instance_with_session();
        let mut event = ask(Some(session));
        event.event = "SomethingOnlyANewerReleaseSends".to_owned();
        assert_eq!(ingest(&state, &event).directive, HookDirective::Proceed);
    }

    #[test]
    fn the_ack_echoes_the_nonce_it_was_asked_with() {
        // The hook matches the reply to its own request by nonce, and it makes
        // exactly one call in its life — an ack for a different one is worse
        // than none.
        let (state, session) = instance_with_session();
        let mut event = ask(Some(session));
        event.nonce = 4242;
        assert_eq!(ingest(&state, &event).nonce, 4242);
    }
    #[test]
    fn a_question_becomes_a_card_every_surface_can_see() {
        // The ledger is what a phone lists and what the panel counts. A
        // question recorded in a transcript and not on the ledger is one only
        // the person at the keyboard ever learns about.
        let (state, session) = instance_with_session();
        let _ = ingest(&state, &ask(Some(session)));
        let instance = state.lock().expect("lock");
        let open = instance.ledger.open_interactions();
        assert_eq!(open.len(), 1, "the question raised no card");
        assert_eq!(open[0].session, session);
    }

    #[test]
    fn a_card_on_a_pty_says_it_cannot_be_answered_from_elsewhere() {
        // An agent on a pty draws its card wherever its cursor is, and picking
        // an option means moving a highlight omt cannot see. Offering the
        // choice anyway means somebody presses it, believes the agent heard,
        // and finds out when the wrong option was taken.
        let (state, session) = instance_with_session();
        let _ = ingest(&state, &ask(Some(session)));
        let instance = state.lock().expect("lock");
        let open = instance.ledger.open_interactions();
        assert!(
            !open[0].deliverable.is_answerable(),
            "a pty card was offered as answerable"
        );
    }
    #[test]
    fn a_question_makes_the_agent_blocked_rather_than_working() {
        // The complaint this answers: an agent plainly waiting for a person
        // while every surface said it was busy. A hook is the agent saying so
        // itself, which outranks anything read off a screen.
        let (state, session) = instance_with_session();
        let _ = ingest(&state, &ask(Some(session)));
        let mut instance = state.lock().expect("lock");
        let binding = instance.binding_for(session);
        let now = crate::run::monotonic_ms();
        assert!(
            matches!(
                instance.agent_state(binding, now),
                Some(omt_types::AgentState::Blocked { .. })
            ),
            "the pane did not report that somebody is being waited for"
        );
    }
    #[test]
    fn the_agents_own_name_for_its_conversation_is_remembered() {
        // The one thing that can be handed back to an agent to *continue*
        // rather than start again, and it arrives here or nowhere: nothing
        // else omt can see knows it.
        let (state, session) = instance_with_session();
        let mut event = ask(Some(session));
        event.agent_session = Some("c-42".to_owned());
        let _ = ingest(&state, &event);
        let instance = state.lock().expect("lock");
        assert_eq!(instance.agent_session(session), Some("c-42"));
    }
}
