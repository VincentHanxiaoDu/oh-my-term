//! Where a source puts what it observed, and what it is allowed to put there.
//!
//! [06 §8.4](../../../docs/architecture/06-agent-layer.md) gives a table of
//! which payloads a source at each tier may construct **at all** — stricter
//! than the merge rule, which only says a lower tier cannot contradict a live
//! higher one. Most of that table is a check. One row of it is a type.
//!
//! The row that is a type is tier 0. A wrong card that somebody taps "Allow"
//! on is an incident, not a glitch, so a heuristic source does not get an
//! [`EventSink`] at all — it gets an [`ActivitySink`], which has no method
//! that accepts an [`AgentPayload`]. "Screen text became a tool call" is not a
//! bug that can be written, rather than one that is caught.
//!
//! The remaining rows are one table-driven check inside `EventSink`. Six
//! marker types for forty cells would buy less than the test that walks the
//! table does.

use omt_types::Tier;

use crate::agent::{ActivityGuess, AgentPayload};

/// What went wrong in a source.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceError {
    /// It tried to emit a payload its tier may not produce.
    ///
    /// The source is disabled for that binding rather than having the one
    /// payload dropped: a source that produced an out-of-tier payload once is
    /// a source whose other payloads are now in question, and `agent.explain`
    /// has to be able to say why it went quiet.
    #[error("a {tier:?} source may not emit {payload}")]
    TierViolation {
        /// The tier that tried.
        tier: Tier,
        /// Which payload, by its own name, so a log line identifies it.
        payload: &'static str,
    },
}

/// The name of a payload's group, as 06 §8.4's table names it.
#[must_use]
pub fn payload_name(payload: &AgentPayload) -> &'static str {
    match payload {
        AgentPayload::Activity { .. } => "Activity",
        AgentPayload::SessionStart { .. } => "SessionStart",
        AgentPayload::SessionEnd { .. } => "SessionEnd",
        AgentPayload::Notification { .. } => "Notification",
        AgentPayload::Capabilities { .. } => "Capabilities",
        AgentPayload::TurnStart { .. } => "TurnStart",
        AgentPayload::TurnEnd { .. } => "TurnEnd",
        AgentPayload::Compaction { .. } => "Compaction",
        AgentPayload::UserMessage { .. } => "UserMessage",
        AgentPayload::AssistantText { .. } => "AssistantText",
        AgentPayload::Reasoning { .. } => "Reasoning",
        AgentPayload::ToolCall { .. } => "ToolCall",
        AgentPayload::ToolResult { .. } => "ToolResult",
        AgentPayload::Plan { .. } => "Plan",
        AgentPayload::FileChanged { .. } => "FileChanged",
        AgentPayload::Usage { .. } => "Usage",
        AgentPayload::RateLimit { .. } => "RateLimit",
        AgentPayload::Question { .. } => "Question",
        AgentPayload::InteractionRaised { .. } => "InteractionRaised",
        AgentPayload::InteractionResolved { .. } => "InteractionResolved",
        AgentPayload::QueueChanged { .. } => "QueueChanged",
    }
}

/// The lowest tier permitted to produce a payload, per 06 §8.4.
///
/// A function rather than a table of tier→payloads, because the table's shape
/// is a staircase: every row is "this tier and everything above it". Written
/// the other way round, forty cells would have to be kept consistent by hand.
#[must_use]
pub fn floor_for(payload: &AgentPayload) -> Tier {
    match payload {
        // The only thing tier 0 may say, and the reason the type exists.
        AgentPayload::Activity { .. } => Tier::Heuristic,
        // A process table can tell that an agent started and that it is gone.
        AgentPayload::SessionStart { .. } | AgentPayload::SessionEnd { .. } => Tier::Process,
        // A marker was *put there* by the agent's launcher or by omt, so it is
        // a statement rather than a resemblance.
        AgentPayload::Notification { .. } => Tier::Marker,
        // Everything else describes what the agent is doing or saying, which
        // only a source reading the agent's own record can know.
        _ => Tier::Transcript,
    }
}

/// Whether a source at this tier may construct this payload.
#[must_use]
pub fn permitted(tier: Tier, payload: &AgentPayload) -> bool {
    tier >= floor_for(payload)
}

/// Where a source above tier 0 puts what it observed.
///
/// Holds the tier so the table check has something to check against — a sink
/// that took the tier per call would let a source claim a different one per
/// payload, which is the thing being prevented.
pub struct EventSink {
    tier: Tier,
    emit: Box<dyn FnMut(AgentPayload) + Send>,
}

impl std::fmt::Debug for EventSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventSink")
            .field("tier", &self.tier)
            .finish()
    }
}

impl EventSink {
    /// A sink for a source at `tier`.
    ///
    /// # Panics
    /// Never. A tier-0 caller is refused at compile time by having no
    /// constructor to reach — [`ActivitySink::new`] is what it can call.
    #[must_use]
    pub fn new(tier: Tier, emit: impl FnMut(AgentPayload) + Send + 'static) -> Self {
        Self {
            tier,
            emit: Box::new(emit),
        }
    }

    /// The tier this sink speaks at.
    #[must_use]
    pub const fn tier(&self) -> Tier {
        self.tier
    }

    /// Emit an observation.
    ///
    /// # Errors
    /// [`SourceError::TierViolation`] when the payload is above this source's
    /// tier. The payload is **not** emitted, and the caller is expected to
    /// disable the source for that binding rather than carry on.
    pub fn emit(&mut self, payload: AgentPayload) -> Result<(), SourceError> {
        if !permitted(self.tier, &payload) {
            return Err(SourceError::TierViolation {
                tier: self.tier,
                payload: payload_name(&payload),
            });
        }
        (self.emit)(payload);
        Ok(())
    }
}

/// Where a tier-0 source puts what it guessed.
///
/// The whole design is what is *absent*: there is no method here that accepts
/// an [`AgentPayload`]. A heuristic source cannot emit a tool call because it
/// has nothing to emit one through, which is a stronger guarantee than a check
/// that could be forgotten at one call site.
pub struct ActivitySink {
    emit: Box<dyn FnMut(ActivityGuess) + Send>,
}

impl std::fmt::Debug for ActivitySink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ActivitySink")
    }
}

impl ActivitySink {
    /// A sink for a tier-0 source.
    pub fn new(emit: impl FnMut(ActivityGuess) + Send + 'static) -> Self {
        Self {
            emit: Box::new(emit),
        }
    }

    /// The only emit method that exists.
    pub fn emit(&mut self, guess: ActivityGuess) {
        (self.emit)(guess);
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
    use std::sync::{Arc, Mutex};

    fn collector() -> (Arc<Mutex<Vec<AgentPayload>>>, EventSink) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink_seen = Arc::clone(&seen);
        let sink = EventSink::new(Tier::Protocol, move |p| {
            sink_seen.lock().expect("lock").push(p);
        });
        (seen, sink)
    }

    fn tool_call() -> AgentPayload {
        AgentPayload::ToolCall {
            call: "c1".to_owned(),
            name: "bash".to_owned(),
            input: serde_json::Value::Null,
        }
    }

    #[test]
    fn a_heuristic_sink_has_no_way_to_emit_a_payload() {
        // The property is what is missing, so the test is that the only thing
        // reachable is a guess. If somebody adds an `AgentPayload` method to
        // `ActivitySink`, this test still passes — and 06 §8.4's whole
        // argument is gone. The compile-time half is the type; this is the
        // reminder attached to it.
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink_seen = Arc::clone(&seen);
        let mut sink = ActivitySink::new(move |g| sink_seen.lock().expect("lock").push(g));
        sink.emit(ActivityGuess::Busy);
        assert_eq!(*seen.lock().expect("lock"), vec![ActivityGuess::Busy]);
    }

    #[test]
    fn a_process_source_cannot_report_what_an_agent_said() {
        // It read a process table. Anything about the conversation would be
        // invented, and a card assembled from it is the incident §6 describes.
        let mut sink = EventSink::new(Tier::Process, |_| {});
        let err = sink.emit(tool_call()).expect_err("must be refused");
        assert_eq!(
            err,
            SourceError::TierViolation {
                tier: Tier::Process,
                payload: "ToolCall"
            }
        );
    }

    #[test]
    fn a_refused_payload_is_not_emitted() {
        // Reporting the violation and delivering the payload anyway would be
        // the worst of both: a log line nobody reads and a card somebody taps.
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink_seen = Arc::clone(&seen);
        let mut sink = EventSink::new(Tier::Heuristic, move |p| {
            sink_seen.lock().expect("lock").push(p);
        });
        assert!(sink.emit(tool_call()).is_err());
        assert!(seen.lock().expect("lock").is_empty());
    }

    #[test]
    fn a_protocol_source_may_say_anything() {
        let (seen, mut sink) = collector();
        sink.emit(tool_call()).expect("a protocol source was told");
        assert_eq!(seen.lock().expect("lock").len(), 1);
    }

    #[test]
    fn the_whole_table_is_walked() {
        // 06 §8.4's table, cell by cell. A payload added to `AgentPayload`
        // without a row here defaults to `Transcript`, which is the safe
        // direction — but `payload_name` is exhaustive, so it will not
        // compile until somebody has at least looked.
        let cases: &[(AgentPayload, Tier)] = &[
            (
                AgentPayload::Activity {
                    state: ActivityGuess::Busy,
                },
                Tier::Heuristic,
            ),
            (
                AgentPayload::SessionStart {
                    reason: crate::agent::StartReason::Startup,
                    model: None,
                },
                Tier::Process,
            ),
            (
                AgentPayload::SessionEnd {
                    reason: "done".to_owned(),
                },
                Tier::Process,
            ),
            (
                AgentPayload::Notification {
                    kind: "n".to_owned(),
                    message: "m".to_owned(),
                },
                Tier::Marker,
            ),
            (
                AgentPayload::Capabilities {
                    tools: Vec::new(),
                    slash_commands: Vec::new(),
                    models: Vec::new(),
                },
                Tier::Transcript,
            ),
            (tool_call(), Tier::Transcript),
        ];

        for (payload, floor) in cases {
            assert_eq!(
                floor_for(payload),
                *floor,
                "{} sits at the wrong tier",
                payload_name(payload)
            );
            // Every tier at or above the floor may produce it, and every tier
            // below may not. The staircase, checked rather than assumed.
            for tier in [
                Tier::Heuristic,
                Tier::Process,
                Tier::Marker,
                Tier::Transcript,
                Tier::Hook,
                Tier::Protocol,
            ] {
                assert_eq!(
                    permitted(tier, payload),
                    tier >= *floor,
                    "{:?} vs {}",
                    tier,
                    payload_name(payload)
                );
            }
        }
    }

    #[test]
    fn activity_is_the_only_thing_the_floor_may_say() {
        // The row that is a security property, stated as an assertion rather
        // than left implicit in the staircase.
        for payload in [
            tool_call(),
            AgentPayload::AssistantText {
                text: "hello".to_owned(),
                partial: false,
            },
            AgentPayload::SessionStart {
                reason: crate::agent::StartReason::Startup,
                model: None,
            },
        ] {
            assert!(
                !permitted(Tier::Heuristic, &payload),
                "the floor was allowed to emit {}",
                payload_name(&payload)
            );
        }
        assert!(permitted(
            Tier::Heuristic,
            &AgentPayload::Activity {
                state: ActivityGuess::Idle
            }
        ));
    }
}
