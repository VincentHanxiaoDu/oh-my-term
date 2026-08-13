//! Who is on the other end of a connection, and what an agent calls itself.
//!
//! Three names the architecture uses that had no type. Each is named by more
//! than one crate, which is the rule that puts it here
//! ([02 L0](../../../docs/architecture/02-crate-map.md)): `PeerInfo` by
//! `omt-transport`, `omt-proto` and `omt-identity`; `SourceId` by `omt-agent`,
//! `omt-recall` and the audit log; `AgentSessionId` by `omt-agent`,
//! `omt-agent-adapters` and the wire.
//!
//! They were each a bare `String` at every one of those sites. A `String` is
//! not wrong so much as unenforceable: nothing stops an agent's session id
//! being passed where a source id belongs, and both read as `String` in a
//! signature.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A string newtype whose whole job is to not be interchangeable with another.
macro_rules! opaque_id {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(
            Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash,
            Serialize, Deserialize, schemars::JsonSchema,
        )]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Wrap a string.
            #[must_use]
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Read it back.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Take the string out.
            #[must_use]
            pub fn into_string(self) -> String {
                self.0
            }

            /// Whether it carries nothing.
            #[must_use]
            pub fn is_empty(&self) -> bool {
                self.0.is_empty()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }
    };
}

opaque_id! {
    /// Which [`crate::Tier`]-bearing observer produced an event.
    ///
    /// `omt-agent` registers sources under it, `agent.explain` reports it back
    /// to a user asking why omt believes something, and the audit log records
    /// it. Not a uuid: a source names itself something a human can read in the
    /// explanation, which is the whole point of the field.
    SourceId
}

opaque_id! {
    /// The **agent's own** session identifier.
    ///
    /// `CLAUDE_CODE_SESSION_ID`, `CODEX_THREAD_ID`, an ACP session id. Never
    /// minted by omt and never parsed by it — omt reads it out of an
    /// environment or a protocol and carries it, so a shape assumption here
    /// would be a bug the first time an agent changed its format.
    AgentSessionId
}

opaque_id! {
    /// An agent's permission mode, in the agent's own vocabulary.
    ///
    /// A newtype rather than an enum, and deliberately: [10 §8](../../../docs/architecture/10-configuration.md)
    /// says the mode is *passed through* and that omt never overrides it
    /// silently, and 06 §8 carries the agent's list verbatim —
    /// `acceptEdits`, `auto`, and whatever the next release adds. A closed
    /// enum would refuse a mode the agent supports, which is the opposite of
    /// passing it through. What the type buys is that it cannot be confused
    /// with the other strings on the same struct.
    PermissionMode
}

/// Who is on the other end, as far as the transport can tell.
///
/// Every field is optional because every field is transport-dependent: a unix
/// socket has a uid and no address, a TCP connection has an address and no
/// uid, and a tailnet identity exists only where one is in use. Reported
/// rather than inferred — a peer omt cannot identify is `None`, never a guess,
/// because 13 §5 makes access decisions from this.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PeerInfo {
    /// The source address, where the transport has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addr: Option<String>,
    /// The peer's uid, which only a unix socket can report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<u32>,
    /// The tailnet identity, where the connection came over one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tailnet: Option<String>,
}

impl PeerInfo {
    /// A peer on a unix socket, identified by its uid.
    #[must_use]
    pub const fn unix(uid: u32) -> Self {
        Self {
            addr: None,
            uid: Some(uid),
            tailnet: None,
        }
    }

    /// A peer at an address.
    #[must_use]
    pub fn at(addr: impl Into<String>) -> Self {
        Self {
            addr: Some(addr.into()),
            uid: None,
            tailnet: None,
        }
    }

    /// Whether anything at all is known about this peer.
    ///
    /// The question 13 §5 asks before treating a connection as local: an
    /// entirely unidentified peer is not the same as one identified as remote.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        self.addr.is_none() && self.uid.is_none() && self.tailnet.is_none()
    }
}

/// Whether an answer reaches the agent through its own protocol or by typing.
///
/// [06 §5.2](../../../docs/architecture/06-agent-layer.md). The axis that
/// decides whether a card is answerable remotely at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Fidelity {
    /// The agent has a real response channel — ACP, a plugin, an app-server.
    Native,
    /// The answer is keystrokes into the agent's own card.
    Synthetic,
}

/// Whether producing an answer requires knowing the agent's screen state.
///
/// [D3](../../../docs/architecture/decisions.md). The bound on synthetic input
/// is state dependence, **not** tool danger: writing `y\n` to a program whose
/// documented input channel is stdin is not a hack, and counting arrow keys
/// against a highlight position inferred from pixels is, whatever the tool at
/// the other end happens to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StateDependence {
    /// The answer is the same whatever the agent's UI is doing: a digit, a
    /// `y`/`n`, free text, a line-oriented stdin.
    Independent,
    /// The answer requires knowing where a highlight bar currently sits —
    /// state omt inferred from the screen. Disabled by default, because a
    /// version bump or a resize invalidates the inference, omt picks the wrong
    /// option, and on a phone nobody can see that it went wrong.
    Inferred,
}

/// How an answer gets back to the agent, as carried on an interaction.
///
/// The *reference*, not the responder: a card travels to a phone and the
/// object that can actually write to a pty does not. What the phone needs is
/// the three properties `deliverable` is computed from
/// ([06 §5.2.1](../../../docs/architecture/06-agent-layer.md)), which is
/// exactly this struct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResponderRef {
    /// Which responder, for the audit trail and for `agent.explain`.
    pub source: SourceId,
    /// Protocol or keystrokes.
    pub fidelity: Fidelity,
    /// Only meaningful for [`Fidelity::Synthetic`]; a native responder is
    /// always [`StateDependence::Independent`].
    pub state_dependence: StateDependence,
    /// Whether this channel can carry a modified tool input alongside an
    /// approval — a property of the channel, not of the agent's option list
    /// ([D9](../../../docs/architecture/decisions.md)).
    #[serde(default)]
    pub supports_edit: bool,
}

impl ResponderRef {
    /// A responder that speaks the agent's own protocol.
    #[must_use]
    pub fn native(source: impl Into<SourceId>) -> Self {
        Self {
            source: source.into(),
            fidelity: Fidelity::Native,
            // Not a default being filled in: a native responder does not read
            // the screen, so there is nothing for its answer to depend on.
            state_dependence: StateDependence::Independent,
            supports_edit: false,
        }
    }

    /// A responder that types into the agent's own card.
    #[must_use]
    pub fn synthetic(source: impl Into<SourceId>, state_dependence: StateDependence) -> Self {
        Self {
            source: source.into(),
            fidelity: Fidelity::Synthetic,
            state_dependence,
            supports_edit: false,
        }
    }

    /// Whether omt may use this responder without an explicit opt-in.
    ///
    /// D3's rule in one place rather than at each call site: anything native,
    /// and anything synthetic whose answer does not depend on inferred state.
    #[must_use]
    pub const fn enabled_by_default(&self) -> bool {
        matches!(self.fidelity, Fidelity::Native)
            || matches!(self.state_dependence, StateDependence::Independent)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "in a test, expect() is the assertion")]
mod tests {
    use super::*;

    #[test]
    fn the_ids_are_not_interchangeable() {
        // The entire point. These were both `String` at every site that named
        // them, and a signature taking two strings accepts them either way
        // round.
        let source = SourceId::new("transcript-tail");
        let agent = AgentSessionId::new("ses_2e70");
        assert_ne!(source.as_str(), agent.as_str());
        // A compile error is the assertion that cannot be written here; what
        // can be is that neither type erased its contents on the way through.
        assert_eq!(source.to_string(), "transcript-tail");
        assert_eq!(agent.to_string(), "ses_2e70");
    }

    #[test]
    fn an_agent_session_id_is_carried_verbatim() {
        // Never minted or parsed by omt. A shape assumption here breaks the
        // first time an agent changes its format.
        for raw in ["ses_2e70d8700ffe3Sfps9jaDaYMOG", "1f0c-4e", "", "not/a/uuid"] {
            assert_eq!(AgentSessionId::new(raw).as_str(), raw);
        }
    }

    #[test]
    fn a_permission_mode_omt_has_never_heard_of_survives() {
        // 10 §8: passed through, never overridden silently. A closed enum
        // would refuse a mode the agent supports.
        let next_release = PermissionMode::new("someModeInventedNextTuesday");
        assert_eq!(
            serde_json::to_string(&next_release).expect("serialize"),
            "\"someModeInventedNextTuesday\""
        );
    }

    #[test]
    fn an_unidentified_peer_is_not_the_same_as_an_identified_one() {
        // 13 §5 makes access decisions from this, and "nothing is known" must
        // not read as "known to be local".
        assert!(PeerInfo::default().is_unknown());
        assert!(!PeerInfo::unix(501).is_unknown());
        assert!(!PeerInfo::at("10.0.0.2:9000").is_unknown());
    }

    #[test]
    fn a_peer_omits_what_its_transport_cannot_report() {
        // A unix socket has no address and a TCP connection has no uid.
        // Serializing a `null` for the other would invite a client to render
        // it as an empty field rather than an absent one.
        let json = serde_json::to_string(&PeerInfo::unix(501)).expect("serialize");
        assert_eq!(json, r#"{"uid":501}"#);
    }

    #[test]
    fn a_native_responder_is_never_state_dependent() {
        // It does not read the screen, so there is nothing for its answer to
        // depend on. Constructed rather than defaulted, so this cannot drift.
        let r = ResponderRef::native("acp");
        assert_eq!(r.state_dependence, StateDependence::Independent);
        assert!(r.enabled_by_default());
    }

    #[test]
    fn an_inferred_synthetic_responder_is_off_by_default() {
        // D3. A version bump or a resize invalidates the inference, omt
        // selects the wrong option, and on a phone nobody sees it go wrong.
        let r = ResponderRef::synthetic("pty-keystrokes", StateDependence::Inferred);
        assert!(!r.enabled_by_default());
    }

    #[test]
    fn an_independent_synthetic_responder_is_on_by_default() {
        // Claude Code's `AskUserQuestion` accepts a digit at an absolute
        // index, which is what makes this case exist at all.
        let r = ResponderRef::synthetic("pty-keystrokes", StateDependence::Independent);
        assert!(r.enabled_by_default());
    }

    #[test]
    fn every_type_here_round_trips_through_its_wire_form() {
        // A rename is caught as a test failure rather than as a client that
        // stopped understanding a field.
        let r = ResponderRef::synthetic("s", StateDependence::Independent);
        let json = serde_json::to_string(&r).expect("serialize");
        assert!(json.contains(r#""fidelity":"synthetic""#), "{json}");
        assert!(
            json.contains(r#""state_dependence":"independent""#),
            "{json}"
        );
        let back: ResponderRef = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, r);
    }
}
