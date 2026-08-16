//! Taking an agent's own words into everything that shows them.
//!
//! Two sources say the same things in the same vocabulary: the hook, as the
//! agent acts, and the agent's session file, after it has. Both end here, and
//! they differ in exactly two values — the tier and the source tag — which is
//! the tier ladder working at the granularity of one event instead of one
//! binding.
//!
//! What is not shared is the deciding. A payload arrives already normalized by
//! the agent's own adapter; this is the part that puts it on the transcript,
//! the ledger, the roster and the stream, once.

use omt_events::{AgentEvent, AgentPayload, EventPayload, EventSourceTag};
use omt_session::SessionMode;
use omt_types::{AgentKind, BindingId, SessionId, Tier, Timestamp};

use crate::state::State;

/// How many event identities are remembered per session.
///
/// Bounded because a day-long conversation would otherwise grow this without
/// limit. Past the bound the oldest are forgotten, so a very old duplicate
/// could reappear — which is a better failure than a process that grows until
/// it is killed.
const SEEN_LIMIT: usize = 4096;

/// What has already been reported, per session.
///
/// Keyed by the agent's own ids — a tool call id, a record uuid — so this is a
/// fact rather than a comparison of two pieces of text that look alike.
#[derive(Debug, Default)]
pub struct Seen(std::collections::BTreeMap<SessionId, Ring>);

/// A bounded set that remembers insertion order.
#[derive(Debug, Default)]
struct Ring {
    set: std::collections::BTreeSet<String>,
    order: std::collections::VecDeque<String>,
}

impl Ring {
    /// Record an identity. False if it was already there.
    fn insert(&mut self, key: String) -> bool {
        if !self.set.insert(key.clone()) {
            return false;
        }
        self.order.push_back(key);
        if self.order.len() > SEEN_LIMIT
            && let Some(oldest) = self.order.pop_front()
        {
            self.set.remove(&oldest);
        }
        true
    }
}

impl Seen {
    /// Record that a session has reported this identity, and say whether it is
    /// new.
    pub fn first_time(&mut self, session: SessionId, key: String) -> bool {
        self.0.entry(session).or_default().insert(key)
    }

    /// Forget a session's identities.
    pub fn forget(&mut self, session: SessionId) {
        self.0.remove(&session);
    }
}

/// The identity of a payload, where it has one the agent minted.
///
/// Only tool calls and their results have one. A message does not, and giving
/// it a key made from its own text would suppress an agent that legitimately
/// said the same thing twice — which is common, and which reads as omt having
/// lost a message.
#[must_use]
pub fn identity(payload: &AgentPayload) -> Option<String> {
    match payload {
        AgentPayload::ToolCall { call, .. } => Some(format!("call:{call}")),
        AgentPayload::ToolResult { call, .. } => Some(format!("result:{call}")),
        AgentPayload::Question { call, .. } => Some(format!("question:{call}")),
        _ => None,
    }
}

/// Everything one payload needs to be taken in.
pub struct From {
    /// Which session it belongs to. Never guessed — see [`crate::hooks`].
    pub session: SessionId,
    /// The binding the observer is already using.
    pub binding: BindingId,
    /// Which agent, as omt has already identified it. Never taken from the
    /// report itself.
    pub agent: AgentKind,
    /// Its version, where a source said.
    pub agent_version: Option<String>,
    /// The agent's own conversation id, where a source said.
    pub agent_session: Option<String>,
    /// How strong the source is.
    pub tier: Tier,
    /// Which source.
    pub tag: EventSourceTag,
    /// The session's mode, for the card.
    pub mode: Option<SessionMode>,
}

/// Put one payload everywhere it belongs.
///
/// Returns false when it was already reported for this session from any
/// source, which is the normal case for the file's copy of what a hook already
/// said. The caller holds the instance because the frame loop holds it for a
/// whole iteration; locking again here deadlocks the terminal, silently.
pub fn payload(
    instance: &mut omt_daemon::Instance,
    transcripts: &mut crate::transcript::Transcripts,
    seen: &mut Seen,
    from: &From,
    payload: AgentPayload,
) -> bool {
    if let Some(key) = identity(&payload)
        && !seen.first_time(from.session, key)
    {
        return false;
    }
    transcripts.entry(from.session).observe(&payload);
    crate::hooks::raise_card(instance, from.session, from.binding, from.mode, &payload);
    instance.observe_usage(from.session, &payload);
    let event = AgentEvent {
        session: from.session,
        binding: from.binding,
        agent: from.agent,
        agent_version: from.agent_version.clone(),
        agent_session: from.agent_session.clone(),
        thread: None,
        // The instance assigns the real position when it emits. Positions are
        // minted in exactly one place.
        seq: omt_types::Seq::new(0),
        ts: Timestamp::now(),
        tier: from.tier,
        payload,
    };
    instance.observe_thread(from.session, &event);
    instance.emit(
        from.session,
        from.tag,
        EventPayload::Agent {
            event: Box::new(event),
        },
    );
    true
}

/// Read whatever the agent has written down since the last look, and take it
/// in.
///
/// Returns how many payloads were new. Everything here is best-effort: a
/// session with no reported path, no identified agent or no adapter mapping
/// reads nothing, which is the state omt was in for every agent before this
/// existed.
///
/// Takes the instance the caller already holds. Locking it again here would
/// deadlock the frame loop — silently, with a terminal that simply stops.
pub fn from_session_file(
    state: &State,
    instance: &mut omt_daemon::Instance,
    session: SessionId,
) -> usize {
    let Some(path) = instance.transcript_path(session).map(Path::to_path_buf) else {
        return 0;
    };
    // The agent is the one already observed for this pane. Never the one a
    // record names: a file omt was pointed at could say anything, and
    // believing it is how one agent's words enter another's conversation.
    let Some(agent) = instance.observed_agent(session).map(|d| d.agent) else {
        return 0;
    };
    let adapters = omt_agent_adapters::builtin();
    let Some(adapter) = adapters.get(agent) else {
        return 0;
    };
    let records = {
        let Ok(mut readings) = state.readings() else {
            return 0;
        };
        readings.entry(session).read(&path)
    };
    if records.is_empty() {
        return 0;
    }
    let binding = instance.binding_for(session);
    let mode = instance.session(session).map(|s| s.mode);
    let agent_session = instance.agent_session(session).map(str::to_owned);
    let from = From {
        session,
        binding,
        agent,
        agent_version: None,
        agent_session,
        // The agent's own writing: above anything inferred, below the hook,
        // which said the same thing at the moment it happened rather than
        // after.
        tier: Tier::Transcript,
        tag: EventSourceTag::Transcript,
        mode,
    };
    let (Ok(mut transcripts), Ok(mut seen)) = (state.transcripts(), state.seen()) else {
        return 0;
    };
    let mut taken = 0;
    for record in records {
        for one in adapter.transcript_records(&record) {
            if payload(instance, &mut transcripts, &mut seen, &from, one) {
                taken += 1;
            }
        }
    }
    taken
}

use std::path::Path;

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_call_is_taken_once_however_many_sources_report_it() {
        // The hook and the file describe the same call. A conversation showing
        // both shows every call twice, and any count of what an agent did
        // would double.
        let mut seen = Seen::default();
        let session = SessionId::new();
        let key = identity(&AgentPayload::ToolCall {
            call: "toolu_1".into(),
            name: "Bash".into(),
            input: serde_json::Value::Null,
        })
        .expect("a tool call has an identity");
        assert!(seen.first_time(session, key.clone()));
        assert!(!seen.first_time(session, key));
    }

    #[test]
    fn two_sessions_do_not_suppress_each_other() {
        // Call ids are the agent's, and two agents mint them independently.
        let mut seen = Seen::default();
        let key = "call:toolu_1".to_owned();
        assert!(seen.first_time(SessionId::new(), key.clone()));
        assert!(seen.first_time(SessionId::new(), key));
    }

    #[test]
    fn a_message_is_not_given_an_identity_at_all() {
        // An agent legitimately says the same thing twice. Suppressing by text
        // would read as omt having lost a message.
        assert!(
            identity(&AgentPayload::AssistantText {
                text: "done".into(),
                partial: false,
            })
            .is_none()
        );
    }

    #[test]
    fn what_is_remembered_is_bounded() {
        // A day-long conversation must not grow this without limit.
        let mut seen = Seen::default();
        let session = SessionId::new();
        for i in 0..(SEEN_LIMIT + 10) {
            assert!(seen.first_time(session, format!("call:{i}")));
        }
        let ring = seen.0.get(&session).expect("a ring");
        assert_eq!(ring.order.len(), SEEN_LIMIT);
        assert_eq!(ring.set.len(), SEEN_LIMIT);
        // The newest are the ones kept.
        assert!(ring.set.contains(&format!("call:{}", SEEN_LIMIT + 9)));
    }

    #[test]
    fn a_session_that_ended_takes_its_identities_with_it() {
        let mut seen = Seen::default();
        let session = SessionId::new();
        assert!(seen.first_time(session, "call:1".into()));
        seen.forget(session);
        assert!(seen.first_time(session, "call:1".into()));
    }
}
