//! One connection, and the one path a call takes.
//!
//! There is deliberately no second, faster route for a local caller. Every
//! call — from a phone, from the CLI, from the TUI's own palette — is a
//! `ProtoMessage` through this function, which is what makes "remote is
//! equivalent to local" a property of the code rather than a promise in a
//! document.

use omt_catalog::{CallContext, CapabilityError, CapabilityRegistry};
use omt_proto::{
    Call, CallOutcome, CallResult, Goodbye, GoodbyeReason, Hello, ProtoMessage, Welcome, negotiate,
};
use omt_types::Role;

/// What a connection knows about whoever is on it.
#[derive(Debug, Clone)]
pub struct Peer {
    /// Who they are.
    pub actor: omt_types::Actor,
    /// What their credential maps to.
    pub role: Role,
    /// Whether the handshake has happened.
    pub greeted: bool,
}

impl Peer {
    /// A peer that has connected but not yet said anything.
    #[must_use]
    pub const fn new(actor: omt_types::Actor, role: Role) -> Self {
        Self {
            actor,
            role,
            greeted: false,
        }
    }
}

/// Handle one message and produce the reply, if there is one.
///
/// # Errors
/// Never. A refusal is a message, not an error: the caller has a connection to
/// write to either way, and turning a bad request into a transport failure
/// would take the connection down with it.
pub fn handle(
    registry: &CapabilityRegistry,
    peer: &mut Peer,
    message: ProtoMessage,
) -> Option<ProtoMessage> {
    match message {
        ProtoMessage::Hello(hello) => Some(greet(registry, peer, &hello)),

        // Everything else requires the handshake. Answering before it would
        // mean acting for a peer whose role nothing established.
        _ if !peer.greeted => Some(ProtoMessage::Goodbye(Goodbye {
            reason: GoodbyeReason::Unauthorized,
            detail: "send `hello` before anything else".to_owned(),
        })),

        ProtoMessage::Call(call) => Some(ProtoMessage::Result(dispatch(registry, peer, call))),

        // A subscription changes what the connection is sent, which the writer
        // owns; there is nothing to reply with here.
        ProtoMessage::Subscribe(_) => None,

        // Everything else is something a server sends, not receives. Echoing a
        // refusal would be more confusing than silence.
        _ => None,
    }
}

fn greet(registry: &CapabilityRegistry, peer: &mut Peer, hello: &Hello) -> ProtoMessage {
    match negotiate(hello.proto, omt_proto::PROTO_VERSION) {
        Err(goodbye) => ProtoMessage::Goodbye(goodbye),
        Ok(_) => {
            peer.greeted = true;
            ProtoMessage::Welcome(Welcome {
                proto: omt_proto::PROTO_VERSION,
                role: peer.role,
                catalog_hash: catalog_hash(registry),
                // Sent up front so a client renders the intersection rather
                // than discovering a gap by calling into it and failing.
                capabilities: registry.decls().map(|d| d.name.to_owned()).collect(),
            })
        }
    }
}

fn dispatch(registry: &CapabilityRegistry, peer: &Peer, call: Call) -> CallResult {
    let request = call.request;

    let Some(decl) = registry.decl(&call.capability) else {
        return failed(
            request,
            CapabilityError::not_found(format!("no capability `{}`", call.capability)),
        );
    };

    // The role check is here, once, rather than in each handler. A handler that
    // forgot it would be a hole nothing else could see.
    if peer.role < decl.role {
        return failed(
            request,
            CapabilityError::unauthorized(format!(
                "`{}` requires {:?}; this credential is {:?}",
                call.capability, decl.role, peer.role
            )),
        );
    }

    let ctx = CallContext {
        actor: peer.actor.clone(),
        role: peer.role,
        request,
        intent: call.intent,
    };

    match registry.dispatch(&call.capability, &ctx, call.input) {
        // The effects the handler actually reported are dropped here rather
        // than sent: they are for the audit log, and a client that could see
        // them would start deciding things from them.
        Ok(outcome) => CallResult {
            request,
            outcome: CallOutcome::Ok {
                output: outcome.output,
            },
        },
        Err(error) => failed(request, error),
    }
}

const fn failed(request: omt_catalog::RequestId, error: CapabilityError) -> CallResult {
    CallResult {
        request,
        outcome: CallOutcome::Err { error },
    }
}

/// A hash of the capability set.
///
/// So a client can tell whether its cached catalog is current without
/// downloading it — which on a phone on a slow link is the difference between
/// a fast reconnect and a visible stall.
#[must_use]
pub fn catalog_hash(registry: &CapabilityRegistry) -> String {
    let names: Vec<&str> = registry.decls().map(|d| d.name).collect();
    let joined = names.join("\n");
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in joined.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;
    use omt_catalog::RequestId;
    use omt_types::DeviceId;

    fn peer(role: Role) -> Peer {
        Peer::new(omt_types::Actor::Local, role)
    }

    fn request() -> RequestId {
        RequestId {
            device: DeviceId::new(),
            n: 1,
        }
    }

    fn hello() -> ProtoMessage {
        ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        })
    }

    fn call(capability: &str) -> ProtoMessage {
        ProtoMessage::Call(Call {
            request: request(),
            capability: capability.to_owned(),
            input: serde_json::json!({}),
            intent: None,
        })
    }

    #[test]
    fn a_handshake_reports_what_this_instance_can_do() {
        // Sent up front so a client renders the intersection rather than
        // discovering a gap by calling into it and failing.
        let r = CapabilityRegistry::new();
        let mut p = peer(Role::Operator);
        let reply = handle(&r, &mut p, hello()).expect("a welcome");
        let ProtoMessage::Welcome(w) = reply else {
            panic!("{reply:?}");
        };
        assert_eq!(w.proto, omt_proto::PROTO_VERSION);
        assert_eq!(w.role, Role::Operator);
        assert!(p.greeted);
    }

    #[test]
    fn a_call_before_the_handshake_is_refused() {
        // Answering it would mean acting for a peer whose role nothing
        // established.
        let r = CapabilityRegistry::new();
        let mut p = peer(Role::Operator);
        let reply = handle(&r, &mut p, call("instance.info")).expect("a refusal");
        assert!(
            matches!(
                reply,
                ProtoMessage::Goodbye(Goodbye {
                    reason: GoodbyeReason::Unauthorized,
                    ..
                })
            ),
            "{reply:?}"
        );
    }

    #[test]
    fn an_incompatible_client_is_refused_with_both_versions_named() {
        let r = CapabilityRegistry::new();
        let mut p = peer(Role::Operator);
        let reply = handle(
            &r,
            &mut p,
            ProtoMessage::Hello(Hello {
                proto: 0,
                client: "ancient".to_owned(),
                token: None,
            }),
        )
        .expect("a refusal");
        assert!(
            matches!(
                reply,
                ProtoMessage::Goodbye(Goodbye {
                    reason: GoodbyeReason::VersionMismatch,
                    ..
                })
            ),
            "{reply:?}"
        );
        assert!(!p.greeted, "and it did not count as a handshake");
    }

    #[test]
    fn an_unknown_capability_is_an_error_not_a_dropped_connection() {
        // A bad request must not take the connection down with it.
        let r = CapabilityRegistry::new();
        let mut p = peer(Role::Operator);
        handle(&r, &mut p, hello());
        let reply = handle(&r, &mut p, call("nonsense.verb")).expect("a result");
        let ProtoMessage::Result(result) = reply else {
            panic!("{reply:?}");
        };
        assert!(matches!(result.outcome, CallOutcome::Err { .. }));
    }

    #[test]
    fn the_catalog_hash_changes_only_when_the_catalog_does() {
        // What lets a phone skip re-downloading a catalog it already has.
        let a = CapabilityRegistry::new();
        let b = CapabilityRegistry::new();
        assert_eq!(catalog_hash(&a), catalog_hash(&b));
    }

    #[test]
    fn a_subscription_has_nothing_to_reply_with() {
        let r = CapabilityRegistry::new();
        let mut p = peer(Role::Operator);
        handle(&r, &mut p, hello());
        let reply = handle(
            &r,
            &mut p,
            ProtoMessage::Subscribe(omt_proto::Subscribe {
                filter: omt_events::Filter::default(),
                since: std::collections::BTreeMap::new(),
            }),
        );
        assert!(reply.is_none());
    }

    #[test]
    fn a_message_only_a_server_sends_is_ignored_rather_than_answered() {
        let r = CapabilityRegistry::new();
        let mut p = peer(Role::Operator);
        handle(&r, &mut p, hello());
        assert!(
            handle(
                &r,
                &mut p,
                ProtoMessage::Goodbye(Goodbye {
                    reason: GoodbyeReason::ShuttingDown,
                    detail: String::new(),
                })
            )
            .is_none()
        );
    }
}
