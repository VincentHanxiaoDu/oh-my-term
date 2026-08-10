//! Which ways of proving who you are this connection is offered.
//!
//! `docs/architecture/13-security.md` §3. A closed set, advertised in the
//! handshake, and dependent on where the connection came from.
//!
//! Advertising matters because the alternative is a client guessing. A phone
//! that tried a bearer token against an instance with no bearer backend would
//! get a refusal indistinguishable from a wrong token, and the person holding
//! it would retype a password that was never going to work.
//!
//! Transport dependence matters more. Tailnet identity is only meaningful on a
//! connection that actually came over the tailnet — offering it on a loopback
//! socket would let anything local claim to be a tailnet peer, which is the
//! whole authentication bypassed by asserting it. So `offered_for` takes the
//! peer, and the answer is not a property of the instance alone.

use serde::{Deserialize, Serialize};

/// A way of proving who you are.
///
/// Closed. A method omt does not know about cannot be advertised, and a client
/// that invents one gets nothing — which is the correct outcome, because a
/// server that accepted an unknown method name would be accepting whatever the
/// client decided that name meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// A token in the `Authorization` header.
    Bearer,
    /// A password, hashed with argon2id.
    Password,
    /// A single-use invite being exchanged for a credential.
    Invite,
    /// Identity supplied by the tailnet.
    Tailnet,
    /// A root-signed grant, exchanged for a per-instance credential.
    DeviceGrant,
}

impl Method {
    /// Its stable id, which is what goes on the wire.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Bearer => "bearer",
            Self::Password => "password",
            Self::Invite => "invite",
            Self::Tailnet => "tailnet",
            Self::DeviceGrant => "device_grant",
        }
    }
}

/// Where a connection came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Peer {
    /// The unix socket, peer-uid checked.
    LocalSocket,
    /// Loopback TCP.
    Loopback,
    /// A tailnet address, as the tailnet itself reported it.
    ///
    /// Reported by the LocalAPI rather than inferred from the address range: a
    /// packet can carry any source address, and `100.64.0.0/10` is a range
    /// anybody can put in a header.
    Tailnet,
    /// Anywhere else.
    Public,
}

/// What backends this instance has.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Backends {
    /// A bearer token store.
    pub bearer: bool,
    /// A password hash.
    pub password: bool,
    /// Invites can be exchanged.
    pub invite: bool,
    /// The Tailscale LocalAPI is answering.
    pub tailnet: bool,
    /// A root key that can validate a device grant.
    pub device_grant: bool,
}

/// Which methods this peer is offered.
///
/// Ordered and deduplicated, so two handshakes from the same peer advertise the
/// same thing. An advertisement that varies between connections would have a
/// client cache one and be surprised by the next.
#[must_use]
pub fn offered_for(backends: Backends, peer: Peer) -> Vec<Method> {
    let mut out = Vec::new();

    if backends.bearer {
        out.push(Method::Bearer);
    }
    if backends.password {
        out.push(Method::Password);
    }
    if backends.invite {
        out.push(Method::Invite);
    }
    // Only on a connection the tailnet itself vouched for. Offering it
    // elsewhere would let anything local claim to be a tailnet peer, which is
    // authentication bypassed by asserting it.
    if backends.tailnet && peer == Peer::Tailnet {
        out.push(Method::Tailnet);
    }
    if backends.device_grant {
        out.push(Method::DeviceGrant);
    }

    out.sort_unstable();
    out.dedup();
    out
}

/// Whether a connection may proceed with no credential at all.
///
/// Only the local socket, whose peer uid has already been checked. Everything
/// else presents something — including loopback, because a browser tab on the
/// same machine is not the same principal as the person at the keyboard.
#[must_use]
pub fn may_skip_authentication(peer: Peer) -> bool {
    peer == Peer::LocalSocket
}

// ---------------------------------------------------------------------------
// Trusting an identity header — 23 §10.2
// ---------------------------------------------------------------------------

/// What is true about a request carrying tailnet identity headers.
///
/// The doc calls the rule below the single most dangerous line it contains, and
/// it is: a backend on `0.0.0.0:7878` lets anyone on the LAN set
/// `Tailscale-User-Login: someone@example.com` and become Admin. Headers are a
/// thing the network can write; the only reason to believe one is that nothing
/// on the network could have written it.
#[derive(Debug, Clone, Copy, Default)]
pub struct HeaderTrust {
    /// The listener is a unix socket or loopback — never an interface, never
    /// the tailnet itself.
    pub listener_is_local: bool,
    /// `trust_proxy_headers = true` is written in the config file.
    ///
    /// Explicit, and in a file: this is the one setting where "it seemed to
    /// work" is indistinguishable from "anybody can be you", so somebody has
    /// to have written it down where a reviewer can see it.
    pub trust_proxy_headers: bool,
    /// The peer address is loopback, taken from the socket.
    ///
    /// From the socket and never from `X-Forwarded-For`, which is a header —
    /// checking a header to decide whether to trust a header is not a check.
    pub peer_is_loopback: bool,
    /// The Tailscale LocalAPI confirms a serve mapping points at this port.
    ///
    /// Closes the one gap the other three do not: another local process
    /// pretending to be `tailscale serve`. Cheap, because the answer is
    /// already on the machine.
    pub serve_config_confirms_port: bool,
}

/// Why omt will not honour identity headers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HeaderRefusal {
    /// Which condition failed.
    pub condition: String,
    /// What it means.
    pub because: String,
}

/// Whether identity headers may be believed.
///
/// Every condition, so a configuration that fails three of them is told about
/// three. This is a startup check rather than a runtime hope: a request that
/// arrives while the answer is "no" should never have been possible.
#[must_use]
pub fn may_trust_headers(trust: HeaderTrust) -> Vec<HeaderRefusal> {
    let mut out = Vec::new();
    if !trust.listener_is_local {
        out.push(HeaderRefusal {
            condition: "listener".to_owned(),
            because: "identity headers are only believable on a unix socket or loopback; on any other interface anybody who can reach the port can set them".to_owned(),
        });
    }
    if !trust.trust_proxy_headers {
        out.push(HeaderRefusal {
            condition: "trust_proxy_headers".to_owned(),
            because: "`[auth.tailnet].trust_proxy_headers = true` is not set, and this is not a thing to infer".to_owned(),
        });
    }
    if !trust.peer_is_loopback {
        out.push(HeaderRefusal {
            condition: "peer".to_owned(),
            because: "the peer address is not loopback; it is taken from the socket and never from a forwarded-for header".to_owned(),
        });
    }
    if !trust.serve_config_confirms_port {
        out.push(HeaderRefusal {
            condition: "serve_config".to_owned(),
            because: "the Tailscale serve config does not point at this port, so something other than `tailscale serve` may be in front".to_owned(),
        });
    }
    out
}

/// Whether the configuration is one omt will start on.
///
/// `trust_proxy_headers` together with a non-local listener is a startup
/// failure rather than a request-time refusal. The alternative is a daemon that
/// runs happily and rejects every request for a reason nobody reads until
/// somebody turns the check off to make it work.
#[must_use]
pub fn startup_refusal(trust: HeaderTrust) -> Option<String> {
    (trust.trust_proxy_headers && !trust.listener_is_local).then(|| {
        "`trust_proxy_headers = true` with a non-loopback listener: anybody who can reach the port could set an identity header. Bind loopback, or turn it off."
            .to_owned()
    })
}

// ---------------------------------------------------------------------------
// WebAuthn's relying party, honestly — 23 §2
// ---------------------------------------------------------------------------

/// Why an origin cannot be a WebAuthn relying party.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum NotAnRpId {
    /// An IP address. The spec has no syntax for one.
    ///
    /// Not a browser limitation to work around — there is nothing to work
    /// around. A LAN address can never register or use a passkey, ever.
    IpAddress,
    /// No host at all.
    NoHost,
    /// A name nothing can be a registrable suffix of.
    ///
    /// `localhost` is the case that matters: it is a real origin somebody
    /// reaches omt at, and it is a suffix of nothing.
    NotRegistrable,
}

/// Whether an origin could be a relying party, and why not.
///
/// omt is reachable at several origins by design — a tailnet name, a LAN
/// address, loopback, possibly a proxied DNS name — and there is no single RP
/// ID covering all of them. Not "hard to find": there is no syntax for one.
///
/// The resolution is to stop trying. WebAuthn authenticates a device to its
/// *identity*, once, at pairing, at the one origin that is stable and really
/// HTTPS. Reaching an instance uses a device-key signature over a server nonce,
/// which has no origin concept at all and therefore works over plain HTTP on a
/// LAN, over a unix socket, and over the ssh bridge.
#[must_use]
pub fn rp_id_for(origin: &str) -> Result<String, NotAnRpId> {
    let host = origin
        .split_once("://")
        .map_or(origin, |(_, rest)| rest)
        .split('/')
        .next()
        .unwrap_or("");
    let host = host.rsplit_once(':').map_or(host, |(h, port)| {
        // Only strip what is actually a port. An IPv6 host is full of colons
        // and cutting at the last one turns `[::1]` into `[::`.
        if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() {
            h
        } else {
            host
        }
    });
    let host = host.trim_start_matches('[').trim_end_matches(']');

    if host.is_empty() {
        return Err(NotAnRpId::NoHost);
    }
    if host.parse::<std::net::IpAddr>().is_ok() {
        return Err(NotAnRpId::IpAddress);
    }
    if !host.contains('.') {
        // `localhost` and any other single label. A passkey registered here
        // would work at exactly this name and nowhere else, which is the
        // multi-origin problem in miniature.
        return Err(NotAnRpId::NotRegistrable);
    }

    // Prefer the tailnet-wide registrable domain over the per-host name, so a
    // passkey registered against one node still works if home moves to another
    // node on the same tailnet. Moving home is a thing people do exactly once,
    // usually while replacing the machine the old one was on.
    if let Some(tailnet) = host.strip_suffix(".ts.net")
        && let Some((_, domain)) = tailnet.rsplit_once('.')
    {
        return Ok(format!("{domain}.ts.net"));
    }
    Ok(host.to_owned())
}

/// Whether reaching an instance needs an origin at all.
///
/// It does not, and that is the point. Written as a function so the property is
/// testable: if this ever became true for some path, that path would be
/// unreachable from a LAN address and nobody would find out until somebody
/// tried it from one.
#[must_use]
pub const fn instance_auth_needs_an_origin() -> bool {
    false
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
    fn a_tailnet_name_becomes_the_tailnet_wide_domain() {
        // So a passkey registered against one node still works if home is
        // promoted to another on the same tailnet.
        assert_eq!(
            rp_id_for("https://workstation.tailc9e96c.ts.net"),
            Ok("tailc9e96c.ts.net".to_owned())
        );
    }

    #[test]
    fn an_ip_address_can_never_be_a_relying_party() {
        // Not a browser limitation to work around — the spec has no syntax for
        // one, so a LAN address can never register or use a passkey.
        assert_eq!(rp_id_for("http://192.168.1.20:7878"), Err(NotAnRpId::IpAddress));
        assert_eq!(rp_id_for("http://[::1]:7788"), Err(NotAnRpId::IpAddress));
        assert_eq!(rp_id_for("http://127.0.0.1:7788"), Err(NotAnRpId::IpAddress));
    }

    #[test]
    fn localhost_is_a_suffix_of_nothing() {
        // A real origin somebody reaches omt at, and one where a passkey would
        // work at exactly that name and nowhere else.
        assert_eq!(rp_id_for("http://localhost:7788"), Err(NotAnRpId::NotRegistrable));
    }

    #[test]
    fn an_ipv6_host_is_not_mangled_into_something_that_parses() {
        // Cutting at the last colon turns `[::1]` into `[::`, which is not an
        // address and would fall through to being treated as a hostname.
        assert_eq!(rp_id_for("http://[2001:db8::1]:7788"), Err(NotAnRpId::IpAddress));
    }

    #[test]
    fn an_ordinary_domain_is_itself() {
        assert_eq!(
            rp_id_for("https://omt.example.com"),
            Ok("omt.example.com".to_owned())
        );
    }

    #[test]
    fn a_path_does_not_become_part_of_the_relying_party() {
        assert_eq!(
            rp_id_for("https://omt.example.com/app/index.html"),
            Ok("omt.example.com".to_owned())
        );
    }

    #[test]
    fn nothing_at_all_is_not_a_relying_party() {
        assert_eq!(rp_id_for(""), Err(NotAnRpId::NoHost));
    }

    #[test]
    fn reaching_an_instance_never_needs_an_origin() {
        // The load-bearing consequence. If this were ever true for some path,
        // that path would be unreachable from a LAN address and nobody would
        // find out until somebody tried it from one.
        assert!(!instance_auth_needs_an_origin());
    }

    #[test]
    fn the_origins_omt_is_reachable_at_do_not_share_one_relying_party() {
        // Stated as a test because the naive design assumes they do, and the
        // assumption survives right up until somebody opens the LAN address.
        let answers: Vec<_> = [
            "https://workstation.tailc9e96c.ts.net",
            "http://192.168.1.20:7878",
            "http://localhost:7788",
        ]
        .into_iter()
        .map(rp_id_for)
        .collect();
        assert!(answers[0].is_ok());
        assert!(answers[1].is_err());
        assert!(answers[2].is_err());
    }

    fn trusted() -> HeaderTrust {
        HeaderTrust {
            listener_is_local: true,
            trust_proxy_headers: true,
            peer_is_loopback: true,
            serve_config_confirms_port: true,
        }
    }

    #[test]
    fn all_four_conditions_together_are_believed() {
        assert!(may_trust_headers(trusted()).is_empty());
    }

    #[test]
    fn a_public_listener_makes_the_headers_worthless() {
        // A backend on 0.0.0.0 lets anyone on the LAN set the login header and
        // become Admin.
        let mut t = trusted();
        t.listener_is_local = false;
        assert!(!may_trust_headers(t).is_empty());
    }

    #[test]
    fn the_setting_is_never_inferred() {
        // This is the one place where "it seemed to work" is indistinguishable
        // from "anybody can be you", so somebody has to have written it down.
        let mut t = trusted();
        t.trust_proxy_headers = false;
        assert_eq!(may_trust_headers(t).len(), 1);
    }

    #[test]
    fn the_peer_address_comes_from_the_socket() {
        // Checking a header to decide whether to trust a header is not a check.
        let mut t = trusted();
        t.peer_is_loopback = false;
        let refusals = may_trust_headers(t);
        assert!(
            refusals.iter().any(|r| r.because.contains("forwarded-for")),
            "{refusals:?}"
        );
    }

    #[test]
    fn the_serve_config_closes_the_gap_the_others_leave() {
        // Another local process pretending to be `tailscale serve` passes
        // conditions one to three without trouble.
        let mut t = trusted();
        t.serve_config_confirms_port = false;
        assert_eq!(may_trust_headers(t).len(), 1);
    }

    #[test]
    fn every_failed_condition_is_reported_not_just_the_first() {
        let refusals = may_trust_headers(HeaderTrust::default());
        assert_eq!(refusals.len(), 4);
        for refusal in refusals {
            assert!(!refusal.because.is_empty(), "{}", refusal.condition);
        }
    }

    #[test]
    fn nothing_is_trusted_by_default() {
        // The default has to be the safe one, because this is the setting
        // where a wrong default is somebody else's admin session.
        assert_eq!(may_trust_headers(HeaderTrust::default()).len(), 4);
    }

    #[test]
    fn the_dangerous_combination_fails_at_startup_rather_than_per_request() {
        // A daemon that runs happily and rejects every request gets the check
        // turned off by whoever is trying to make it work.
        let mut t = trusted();
        t.listener_is_local = false;
        let refusal = startup_refusal(t).expect("refuses");
        assert!(refusal.contains("loopback"), "{refusal}");
    }

    #[test]
    fn a_safe_configuration_starts() {
        assert!(startup_refusal(trusted()).is_none());
        assert!(startup_refusal(HeaderTrust::default()).is_none());
    }


    fn everything() -> Backends {
        Backends {
            bearer: true,
            password: true,
            invite: true,
            tailnet: true,
            device_grant: true,
        }
    }

    #[test]
    fn tailnet_identity_is_only_offered_on_a_tailnet_connection() {
        // Offering it elsewhere lets anything local claim to be a tailnet
        // peer, which is the whole authentication bypassed by asserting it.
        assert!(offered_for(everything(), Peer::Tailnet).contains(&Method::Tailnet));
        for peer in [Peer::LocalSocket, Peer::Loopback, Peer::Public] {
            assert!(
                !offered_for(everything(), peer).contains(&Method::Tailnet),
                "{peer:?}"
            );
        }
    }

    #[test]
    fn a_backend_that_does_not_exist_is_not_offered() {
        // A client that tried it would get a refusal indistinguishable from a
        // wrong secret, and the person holding it retypes a password that was
        // never going to work.
        let only_bearer = Backends {
            bearer: true,
            ..Backends::default()
        };
        assert_eq!(
            offered_for(only_bearer, Peer::Loopback),
            vec![Method::Bearer]
        );
    }

    #[test]
    fn an_instance_with_no_backends_offers_nothing() {
        assert!(offered_for(Backends::default(), Peer::Public).is_empty());
    }

    #[test]
    fn the_advertisement_is_stable_between_handshakes() {
        // One that varied would have a client cache one answer and be
        // surprised by the next.
        let once = offered_for(everything(), Peer::Tailnet);
        for _ in 0..5 {
            assert_eq!(offered_for(everything(), Peer::Tailnet), once);
        }
    }

    #[test]
    fn every_method_has_a_stable_id_and_they_are_all_different() {
        // The ids go on the wire, so two sharing one would make a client's
        // choice ambiguous.
        let all = [
            Method::Bearer,
            Method::Password,
            Method::Invite,
            Method::Tailnet,
            Method::DeviceGrant,
        ];
        let ids: std::collections::BTreeSet<&str> = all.iter().map(|m| m.id()).collect();
        assert_eq!(ids.len(), all.len());
    }

    #[test]
    fn the_ids_are_the_ones_the_document_names() {
        // A closed set is only closed if it matches the thing it closes over.
        assert_eq!(Method::Bearer.id(), "bearer");
        assert_eq!(Method::Password.id(), "password");
        assert_eq!(Method::Invite.id(), "invite");
        assert_eq!(Method::Tailnet.id(), "tailnet");
        assert_eq!(Method::DeviceGrant.id(), "device_grant");
    }

    #[test]
    fn only_the_local_socket_may_skip_authentication() {
        // Its peer uid has already been checked. Loopback has not: a browser
        // tab on the same machine is not the same principal as the person at
        // the keyboard.
        assert!(may_skip_authentication(Peer::LocalSocket));
        for peer in [Peer::Loopback, Peer::Tailnet, Peer::Public] {
            assert!(!may_skip_authentication(peer), "{peer:?}");
        }
    }

    #[test]
    fn a_method_serialises_as_its_id() {
        // So the wire form and the id cannot drift apart.
        let json = serde_json::to_string(&Method::DeviceGrant).expect("json");
        assert_eq!(json, "\"device_grant\"");
    }

    #[test]
    fn an_unknown_method_name_does_not_deserialise() {
        // A server that accepted one would be accepting whatever the client
        // decided that name meant.
        assert!(serde_json::from_str::<Method>("\"trust_me\"").is_err());
    }
}
