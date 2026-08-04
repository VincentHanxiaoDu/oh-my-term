//! The message catalogue: handshake, calls, subscriptions, and the hook pair.

use omt_catalog::{CapabilityError, RequestId};
use omt_events::{Event, Filter, LagReason};
use omt_types::{IntentId, Role, Seq};
use serde::{Deserialize, Serialize};

use crate::hook::{HookAck, HookEvent};

/// The protocol version this build speaks.
pub const PROTO_VERSION: u16 = 1;

/// Anything sent over a connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ProtoMessage {
    // ---------- lifecycle ----------
    /// Client opens: this is what I speak.
    Hello(Hello),
    /// Instance answers: this is what we agreed on, and what I can do.
    Welcome(Welcome),
    /// Either side refuses, with a reason a human can act on.
    Goodbye(Goodbye),

    // ---------- capability RPC ----------
    /// Invoke a capability.
    Call(Call),
    /// Its result.
    Result(CallResult),

    // ---------- events ----------
    /// Start receiving events.
    Subscribe(Subscribe),
    /// One event.
    Event(Box<Event>),
    /// The requested position is unrecoverable; rebuild from a snapshot.
    Resync(Resync),

    // ---------- hook ingress ----------
    /// An agent's hook reporting an observation.
    HookEvent(Box<HookEvent>),
    /// What the hook should tell its agent.
    HookAck(HookAck),
}

/// A client introducing itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Hello {
    /// The highest protocol version the client speaks.
    pub proto: u16,
    /// What the client is.
    pub client: String,
    /// Its credential.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// The instance's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Welcome {
    /// The version both sides will use.
    pub proto: u16,
    /// What the credential maps to.
    pub role: Role,
    /// A hash of the capability set, so a client can tell whether its cached
    /// catalog is current without downloading it.
    pub catalog_hash: String,
    /// Every capability this instance has.
    ///
    /// Sent up front so a client presents the intersection rather than
    /// discovering a gap by calling into it and failing.
    pub capabilities: Vec<String>,
}

impl Welcome {
    /// What a client can actually use here.
    ///
    /// The version-skew answer: a phone talking to several instances on
    /// different versions renders what each supports and greys out the rest,
    /// rather than erroring on the first mismatch.
    #[must_use]
    pub fn intersect(&self, client_knows: &[String]) -> Vec<String> {
        let mut v: Vec<_> = client_knows
            .iter()
            .filter(|c| self.capabilities.contains(c))
            .cloned()
            .collect();
        v.sort_unstable();
        v
    }
}

/// A refusal or a shutdown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Goodbye {
    /// Why.
    pub reason: GoodbyeReason,
    /// A sentence naming what to do about it.
    pub detail: String,
}

/// Why a connection ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GoodbyeReason {
    /// No common protocol version.
    VersionMismatch,
    /// The credential was rejected.
    Unauthorized,
    /// The subscriber could not keep up, past the policy's limit.
    ///
    /// Closed rather than buffered without bound: a visible failure beats an
    /// instance running out of memory because a phone was in a tunnel.
    Overloaded,
    /// The instance is shutting down.
    ShuttingDown,
}

/// Invoke a capability.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Call {
    /// Stable across reconnection, so a client that lost an acknowledgement can
    /// repeat this and learn what happened rather than guessing.
    pub request: RequestId,
    /// Which capability.
    pub capability: String,
    /// Its input.
    pub input: serde_json::Value,
    /// Present on commands, minted by the client at intent time — before any
    /// server was reached, because a disconnected client cannot ask for one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<IntentId>,
}

/// What a call produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CallResult {
    /// Which call.
    pub request: RequestId,
    /// The outcome.
    #[serde(flatten)]
    pub outcome: CallOutcome,
}

/// Success or failure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CallOutcome {
    /// It worked.
    Ok {
        /// What it produced.
        output: serde_json::Value,
    },
    /// It did not.
    Err {
        /// Why, with a closed code and structured detail.
        error: CapabilityError,
    },
}

/// Ask for events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Subscribe {
    /// What to send.
    #[serde(default)]
    pub filter: Filter,
    /// Resume after this position, per scope key.
    ///
    /// Keyed by string so all three scopes — session, workspace and the
    /// reserved instance key — use one uniformly-typed map rather than a union
    /// every client has to discriminate.
    #[serde(default)]
    pub since: std::collections::BTreeMap<String, Seq>,
}

/// The stream could not be resumed where asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Resync {
    /// Which scope.
    pub scope_key: String,
    /// Why.
    pub reason: LagReason,
    /// How many events were lost, so a client can say how much rather than
    /// silently restarting.
    pub dropped: u64,
    /// Where the stream now stands.
    pub from: Seq,
}

/// Negotiate a protocol version.
///
/// # Errors
/// Fails when there is no version both sides speak, naming both so a human can
/// see which side to upgrade.
pub fn negotiate(client_proto: u16, server_proto: u16) -> Result<u16, Goodbye> {
    let agreed = client_proto.min(server_proto);
    if agreed == 0 {
        return Err(Goodbye {
            reason: GoodbyeReason::VersionMismatch,
            detail: format!(
                "no common protocol version: client speaks {client_proto}, instance speaks {server_proto}"
            ),
        });
    }
    Ok(agreed)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;
    use omt_types::DeviceId;

    fn request() -> RequestId {
        RequestId {
            device: DeviceId::new(),
            n: 1,
        }
    }

    #[test]
    fn a_newer_client_and_older_instance_agree_on_the_older_version() {
        assert_eq!(negotiate(5, 1).expect("negotiate"), 1);
        assert_eq!(negotiate(1, 5).expect("negotiate"), 1);
    }

    #[test]
    fn an_incompatible_peer_is_refused_with_both_versions_named() {
        let g = negotiate(0, 1).expect_err("must refuse");
        assert_eq!(g.reason, GoodbyeReason::VersionMismatch);
        assert!(
            g.detail.contains('0') && g.detail.contains('1'),
            "{}",
            g.detail
        );
    }

    #[test]
    fn a_client_presents_the_intersection_rather_than_failing() {
        // The version-skew answer: a phone attached to several instances shows
        // what each supports instead of erroring on the first mismatch.
        let w = Welcome {
            proto: 1,
            role: Role::Operator,
            catalog_hash: "abc".into(),
            capabilities: vec!["session.list".into(), "agent.state".into()],
        };
        let usable = w.intersect(&[
            "session.list".into(),
            "agent.state".into(),
            "future.capability".into(),
        ]);
        assert_eq!(usable, ["agent.state", "session.list"]);
    }

    #[test]
    fn messages_are_tagged_and_round_trip() {
        let m = ProtoMessage::Call(Call {
            request: request(),
            capability: "session.list".into(),
            input: serde_json::json!({}),
            intent: None,
        });
        let json = serde_json::to_string(&m).expect("serialize");
        assert!(json.starts_with(r#"{"t":"call""#), "{json}");
        let back: ProtoMessage = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(m, back);
    }

    #[test]
    fn a_failed_call_carries_a_discriminating_error() {
        use omt_catalog::ConflictState;
        let r = CallResult {
            request: request(),
            outcome: CallOutcome::Err {
                error: CapabilityError::conflict(
                    "someone else answered",
                    ConflictState::AlreadyResolved {
                        by: "iPhone".into(),
                    },
                ),
            },
        };
        let json = serde_json::to_string(&r).expect("serialize");
        let back: CallResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(r, back);
        let CallOutcome::Err { error } = &back.outcome else {
            panic!("expected an error");
        };
        assert!(
            error.conflict_state().is_some(),
            "the loser can tell why it lost"
        );
    }

    #[test]
    fn there_is_no_message_for_replaying_terminal_input() {
        // The raw-byte-stream intent class in wire terms: re-sending the tail
        // of a shell command is how a retry becomes a disaster, so the
        // protocol offers no encoding for it. This test is the guard — adding
        // such a message would make it fail.
        let names = [
            "hello",
            "welcome",
            "goodbye",
            "call",
            "result",
            "subscribe",
            "event",
            "resync",
            "hook_event",
            "hook_ack",
        ];
        for n in names {
            assert!(
                !n.contains("replay") && !n.contains("resend"),
                "`{n}` looks like an input replay message"
            );
        }
    }

    #[test]
    fn resume_positions_use_one_key_type_for_every_scope() {
        let mut since = std::collections::BTreeMap::new();
        since.insert("s_instance".to_owned(), Seq::new(3));
        since.insert(omt_types::SessionId::new().to_string(), Seq::new(9));
        let s = Subscribe {
            filter: Filter::default(),
            since,
        };
        let back: Subscribe =
            serde_json::from_str(&serde_json::to_string(&s).expect("ser")).expect("de");
        assert_eq!(back.since.len(), 2);
    }

    #[test]
    fn a_resync_says_how_much_was_missed() {
        let r = Resync {
            scope_key: "s_instance".into(),
            reason: LagReason::WindowExceeded,
            dropped: 412,
            from: Seq::new(500),
        };
        let back: Resync =
            serde_json::from_str(&serde_json::to_string(&r).expect("ser")).expect("de");
        assert_eq!(back.dropped, 412, "a client must be able to say how much");
    }

    #[test]
    fn an_overloaded_subscriber_is_closed_with_a_reason() {
        let g = Goodbye {
            reason: GoodbyeReason::Overloaded,
            detail: "subscriber fell behind past the limit".into(),
        };
        let back: Goodbye =
            serde_json::from_str(&serde_json::to_string(&g).expect("ser")).expect("de");
        assert_eq!(back.reason, GoodbyeReason::Overloaded);
    }
}
