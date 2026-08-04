//! The `omt-hook` ingress.
//!
//! An agent's own hook system executes a tiny binary, which reports here. This
//! is the flagship observation path: the hook fires *before* the agent draws
//! its card and carries the tool input verbatim, which is everything needed to
//! mirror an interaction to a phone while the agent's own card renders locally,
//! unchanged.
//!
//! These are protocol messages rather than capability calls, deliberately. A
//! hook is not an actor requesting a mutation — it reports an observation,
//! exactly as a transcript tailer does, and neither of those is a capability
//! call. The authorization that matters happened at the socket, where the peer
//! credentials were checked; a role check here would have no meaningful answer.
//! Dispatch's per-call machinery is also the wrong cost on a path with a
//! single-digit-millisecond budget.

use omt_types::{AgentKind, InstanceId, SessionId};
use serde::{Deserialize, Serialize};

/// The largest payload carried verbatim.
///
/// Beyond this the payload is marked truncated rather than silently shortened,
/// because a hook payload that quietly lost its tail would produce an
/// interaction missing options nobody could account for.
pub const MAX_VERBATIM_PAYLOAD: usize = 1 << 20; // 1 MiB

/// A hook reporting what its agent just did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HookEvent {
    /// A per-process nonce, not a `RequestId`.
    ///
    /// A hook has no device and lives for milliseconds; it makes exactly one
    /// call in its life, so uniqueness within the connection is enough.
    pub nonce: u64,
    /// The version of this message pair.
    ///
    /// Negotiated separately from the main protocol because the hook binary is
    /// installed into the agent's own configuration and may be older or newer
    /// than the daemon it is talking to.
    pub hook_proto: u16,
    /// Which agent.
    pub agent: AgentKind,
    /// Its version, where it could be determined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_version: Option<String>,
    /// The agent's own session id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session: Option<String>,
    /// The agent's own event name, **verbatim and un-normalized** —
    /// `"PreToolUse"`, `"beforeShellExecution"`, `"AfterTool"`.
    ///
    /// The hook does not map it; the daemon's per-agent normalizer does. Keeping
    /// the raw name means an unrecognized event is loggable rather than lost.
    pub event: String,
    /// The tool call this concerns, where the agent gave one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    /// The tool's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// The tool's input, verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<serde_json::Value>,
    /// The whole payload, as a catch-all, so a field omt does not model yet is
    /// still recoverable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<serde_json::Value>,
    /// What was cut, if anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<Truncation>,
    /// How the hook knows which session it belongs to.
    pub correlation: HookCorrelation,
    /// How long the hook will wait before giving up and letting its agent
    /// proceed.
    pub deadline_ms: u32,
}

/// What was too large to carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Truncation {
    /// Which field was cut.
    pub field: String,
    /// How many bytes it actually was.
    pub original_bytes: u64,
}

/// How a hook identifies its session.
///
/// The environment omt injected when it spawned the agent, plus enough process
/// context to correlate when omt did not spawn it. **The hook never guesses a
/// session id** — an unattributed observation is recoverable, a
/// wrongly-attributed one silently corrupts another session's state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HookCorrelation {
    /// From `$OMT_INSTANCE`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<InstanceId>,
    /// From `$OMT_SESSION`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionId>,
    /// The hook process's own pid.
    pub pid: u32,
    /// Its parent, which is usually the agent.
    pub ppid: u32,
    /// Its working directory, the last-resort correlation signal.
    pub cwd: String,
}

impl HookCorrelation {
    /// Whether the session is known outright rather than inferred.
    #[must_use]
    pub const fn is_direct(&self) -> bool {
        self.session.is_some()
    }
}

/// The daemon's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HookAck {
    /// Echoes the request's nonce.
    pub nonce: u64,
    /// What the hook should tell its agent.
    pub directive: HookDirective,
}

/// What a hook tells its agent to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "directive", rename_all = "snake_case")]
pub enum HookDirective {
    /// Carry on, unchanged.
    ///
    /// The safe directive, and the *only* one a hook needs to be able to
    /// construct without having heard anything — which is the point. The path
    /// where the daemon is unreachable is exactly the path where the hook
    /// cannot be told what to do, so failing safe must require no reply.
    Proceed,
    /// Reserved: park the call pending a decision.
    ///
    /// Not used. omt mirrors the agent's own card rather than intercepting it,
    /// so nothing is parked; the variant exists so the wire does not need a
    /// breaking change if that is ever revisited.
    Defer,
    /// Reserved: refuse the call.
    ///
    /// Not used. omt adds no permission policy of its own — it mirrors the
    /// agent's own gate exactly, and inventing a refusal the agent would not
    /// have shown would create two mental models and false confidence.
    Deny,
}

impl HookDirective {
    /// The directive to use when nothing was heard.
    #[must_use]
    pub const fn fail_open() -> Self {
        Self::Proceed
    }

    /// Whether this directive alters what the agent would have done.
    #[must_use]
    pub const fn alters_agent_behaviour(&self) -> bool {
        !matches!(self, Self::Proceed)
    }
}

impl HookEvent {
    /// Build a report, truncating an oversized payload rather than dropping it
    /// silently.
    #[must_use]
    pub fn with_payload(mut self, field: &str, value: serde_json::Value) -> Self {
        let encoded = serde_json::to_vec(&value).unwrap_or_default();
        if encoded.len() > MAX_VERBATIM_PAYLOAD {
            self.truncated = Some(Truncation {
                field: field.to_owned(),
                original_bytes: encoded.len() as u64,
            });
            if field == "tool_input" {
                self.tool_input = None;
            } else {
                self.raw = None;
            }
        } else if field == "tool_input" {
            self.tool_input = Some(value);
        } else {
            self.raw = Some(value);
        }
        self
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

    fn correlation() -> HookCorrelation {
        HookCorrelation {
            instance: Some(InstanceId::new()),
            session: Some(SessionId::new()),
            pid: 100,
            ppid: 99,
            cwd: "/w".into(),
        }
    }

    fn event() -> HookEvent {
        HookEvent {
            nonce: 1,
            hook_proto: 1,
            agent: AgentKind::ClaudeCode,
            agent_version: None,
            agent_session: None,
            event: "PreToolUse".into(),
            tool_use_id: Some("toolu_1".into()),
            tool_name: Some("AskUserQuestion".into()),
            tool_input: Some(serde_json::json!({"questions": []})),
            raw: None,
            truncated: None,
            correlation: correlation(),
            deadline_ms: 250,
        }
    }

    #[test]
    fn proceed_needs_no_reply_to_construct() {
        // The fail-open contract in wire terms: the path where the daemon is
        // unreachable is exactly the path where nothing can be received.
        assert_eq!(HookDirective::fail_open(), HookDirective::Proceed);
        assert!(!HookDirective::Proceed.alters_agent_behaviour());
    }

    #[test]
    fn the_reserved_directives_are_marked_as_altering_behaviour() {
        assert!(HookDirective::Defer.alters_agent_behaviour());
        assert!(HookDirective::Deny.alters_agent_behaviour());
    }

    #[test]
    fn the_agents_event_name_is_carried_verbatim() {
        // Normalizing here would make an unrecognized event unloggable.
        let e = event();
        let json = serde_json::to_string(&e).expect("serialize");
        assert!(json.contains(r#""event":"PreToolUse""#), "{json}");
    }

    #[test]
    fn the_tool_input_is_carried_verbatim() {
        // A human approves what will actually run, so the bytes must be the
        // agent's own.
        let input = serde_json::json!({"command": "rm -rf /tmp/x", "nested": [1, 2]});
        let e = HookEvent {
            tool_input: Some(input.clone()),
            ..event()
        };
        let back: HookEvent =
            serde_json::from_str(&serde_json::to_string(&e).expect("ser")).expect("de");
        assert_eq!(back.tool_input, Some(input));
    }

    #[test]
    fn an_oversized_payload_is_marked_not_silently_shortened() {
        let big = serde_json::json!({ "blob": "x".repeat(MAX_VERBATIM_PAYLOAD + 10) });
        let e = event().with_payload("tool_input", big);
        assert!(e.tool_input.is_none(), "the oversized value is not carried");
        let t = e.truncated.expect("truncation must be recorded");
        assert_eq!(t.field, "tool_input");
        assert!(t.original_bytes > MAX_VERBATIM_PAYLOAD as u64);
    }

    #[test]
    fn a_payload_within_the_bound_is_carried_whole() {
        let small = serde_json::json!({"ok": true});
        let e = event().with_payload("tool_input", small.clone());
        assert_eq!(e.tool_input, Some(small));
        assert!(e.truncated.is_none());
    }

    #[test]
    fn a_direct_correlation_is_distinguishable_from_an_inferred_one() {
        assert!(correlation().is_direct());
        let inferred = HookCorrelation {
            session: None,
            ..correlation()
        };
        assert!(!inferred.is_direct(), "so it can be marked low-confidence");
    }

    #[test]
    fn the_pair_round_trips() {
        let e = event();
        let back: HookEvent =
            serde_json::from_str(&serde_json::to_string(&e).expect("ser")).expect("de");
        assert_eq!(e, back);

        let a = HookAck {
            nonce: e.nonce,
            directive: HookDirective::Proceed,
        };
        let back: HookAck =
            serde_json::from_str(&serde_json::to_string(&a).expect("ser")).expect("de");
        assert_eq!(a, back);
    }

    #[test]
    fn the_ack_echoes_the_nonce() {
        // A hook makes one call in its life; the echo is how it knows the
        // answer is its own.
        let e = event();
        let a = HookAck {
            nonce: e.nonce,
            directive: HookDirective::Proceed,
        };
        assert_eq!(a.nonce, e.nonce);
    }
}
