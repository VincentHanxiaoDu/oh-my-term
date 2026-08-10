//! Where omt is willing to listen, and what it demands first.
//!
//! `docs/architecture/13-security.md` §2. The default is loopback, and the
//! defaults are enforced in code rather than documented in prose.
//!
//! The rule that gives this module its shape: **a misconfiguration fails the
//! daemon rather than warning.** A warning about an open port is a warning
//! printed once, into a log nobody reads, on a machine that is now reachable
//! from the network. There is no `--insecure` flag and no environment variable
//! that bypasses it — the escape hatch is to write the configuration that says
//! what you mean, which is a file somebody can audit and a reviewer can see.
//!
//! The unix socket is not implicitly trusted either. A 0600 socket grants full
//! control to any same-uid process, and omt cannot prevent that; what it can do
//! is refuse a differing uid before the handshake and record every connection
//! that is allowed. Recording is the whole difference between "a process took
//! over your terminal" and "a process took over your terminal and there is no
//! way to find out which".

use std::net::IpAddr;

use serde::Serialize;

/// Somewhere omt could listen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bind {
    /// A unix socket. Always present, mode 0600, peer uid checked.
    UnixSocket {
        /// Where it is.
        path: String,
    },
    /// Loopback TCP.
    Loopback {
        /// Which port.
        port: u16,
    },
    /// A tailnet address.
    Tailnet {
        /// Which port.
        port: u16,
    },
    /// Any other interface, including everything.
    Interface {
        /// Which address.
        addr: IpAddr,
        /// Which port.
        port: u16,
    },
}

/// What omt can use to tell callers apart.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Auth {
    /// A password or bearer token is configured.
    pub backend: bool,
    /// Tailscale's LocalAPI is answering, so the tailnet supplies identity.
    pub tailnet_identity: bool,
}

/// What is in front of omt on the network.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Transport {
    /// omt terminates TLS itself.
    pub tls: bool,
    /// The operator has declared a reverse proxy terminates it instead.
    ///
    /// Explicit, because "there is probably a proxy" is how a plaintext port
    /// ends up on the internet. Someone has to write it down.
    pub trust_proxy: bool,
}

/// What role an unauthenticated caller gets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DefaultRole {
    /// Nothing without a credential.
    ///
    /// The default, because a configuration that forgot to say should hand out
    /// nothing rather than something.
    #[default]
    None,
    /// Read-only.
    Viewer,
    /// More than read-only.
    Operator,
}

/// A listen configuration, before it has been checked.
#[derive(Debug, Clone)]
pub struct Listen {
    /// Where to listen.
    pub binds: Vec<Bind>,
    /// What identity is available.
    pub auth: Auth,
    /// What is in front.
    pub transport: Transport,
    /// What an unauthenticated caller gets.
    pub default_role: DefaultRole,
}

/// Why omt will not start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Refusal {
    /// What is wrong.
    pub problem: String,
    /// What to do about it, concretely.
    ///
    /// Every refusal carries one. A daemon that exits saying only "insecure
    /// configuration" leaves somebody to guess, and the guess they reach for
    /// is usually the one that turns the check off.
    pub remedy: String,
}

/// Something worth saying that is not fatal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Warning {
    /// What is worth knowing.
    pub note: String,
}

/// The result of checking a configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Verdict {
    /// Everything that must be fixed before omt starts.
    ///
    /// All of them, not the first. Fixing one and being told about the next on
    /// the following run is a conversation nobody enjoys having four times.
    pub refusals: Vec<Refusal>,
    /// Things worth knowing that do not stop it.
    pub warnings: Vec<Warning>,
}

impl Verdict {
    /// Whether omt may start.
    #[must_use]
    pub fn may_start(&self) -> bool {
        self.refusals.is_empty()
    }
}

/// Whether an address is a loopback one however it was written.
fn is_loopback(addr: IpAddr) -> bool {
    addr.is_loopback()
}

/// Whether an address means "every interface".
fn is_unspecified(addr: IpAddr) -> bool {
    addr.is_unspecified()
}

/// Check a listen configuration.
#[must_use]
pub fn check(listen: &Listen) -> Verdict {
    let mut refusals = Vec::new();
    let mut warnings = Vec::new();

    for bind in &listen.binds {
        match bind {
            // Nothing to demand: a unix socket and loopback are already
            // scoped to the machine.
            Bind::UnixSocket { .. } | Bind::Loopback { .. } => {}
            Bind::Tailnet { port } => {
                if !listen.auth.backend && !listen.auth.tailnet_identity {
                    refusals.push(Refusal {
                        problem: format!(
                            "bind.tailnet on port {port} has no way to tell callers apart"
                        ),
                        remedy: "Configure auth.password or auth.bearer, or make sure Tailscale's LocalAPI is running so the tailnet supplies identity.".to_owned(),
                    });
                }
            }
            Bind::Interface { addr, port } => {
                // An interface bind that happens to name a loopback address is
                // loopback. Refusing it would be pedantry, and pedantry in a
                // security check teaches people to work around the check.
                if is_loopback(*addr) {
                    continue;
                }
                if !listen.auth.backend {
                    refusals.push(Refusal {
                        problem: format!("refusing to bind {addr}:{port} without authentication"),
                        remedy: "Configure auth.password or auth.bearer, or bind loopback instead."
                            .to_owned(),
                    });
                }
                if !listen.transport.tls && !listen.transport.trust_proxy {
                    refusals.push(Refusal {
                        problem: format!("{addr}:{port} would carry terminal traffic in plaintext"),
                        remedy: "Configure tls, or set tls.trust_proxy = true if a reverse proxy terminates TLS in front of omt.".to_owned(),
                    });
                }
                if is_unspecified(*addr) && listen.default_role != DefaultRole::None {
                    // Not fatal, because it is a real configuration somebody
                    // may want on a private network. Prominent, because the
                    // combination means anybody who can reach the port gets a
                    // role without presenting anything.
                    warnings.push(Warning {
                        note: format!(
                            "{addr}:{port} is bound to every interface and unauthenticated callers get the {:?} role.",
                            listen.default_role
                        ),
                    });
                }
            }
        }
    }

    Verdict {
        refusals,
        warnings,
    }
}

/// Whether a connecting process may use the socket.
///
/// omt cannot stop a same-uid process from connecting — it has the same
/// privileges omt does. What it can do is refuse a different uid, and record
/// what did connect.
#[must_use]
pub fn peer_allowed(own_uid: u32, peer_uid: u32) -> bool {
    own_uid == peer_uid
}

/// What is recorded about a socket connection.
///
/// The pid, uid and argv, because "something connected" is not an audit record.
/// The point of recording is to be able to answer *which* process, weeks later,
/// from somebody who noticed something odd.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PeerRecord {
    /// Its process id.
    pub pid: u32,
    /// Its user id.
    pub uid: u32,
    /// How it was invoked.
    pub argv: String,
    /// Whether it was let in.
    pub allowed: bool,
}

// ---------------------------------------------------------------------------
// Publishing to the public internet — 13-security §10
// ---------------------------------------------------------------------------

/// What is true about an instance about to be put on the public internet.
///
/// Funnel is the case this exists for: it puts the instance on the open
/// internet, and the difference between that and a tailnet is not visible in
/// any configuration omt reads. Somebody who enabled Funnel for a demo and
/// forgot is the person this gate is for.
#[derive(Debug, Clone, Default)]
pub struct Publication {
    /// A password or bearer backend is configured.
    pub auth_backend: bool,
    /// The only identity available is the tailnet's.
    ///
    /// Meaningless under Funnel: the requests arrive from Tailscale's own
    /// infrastructure, so every caller looks like the same tailnet peer. An
    /// instance relying on it is one where everybody on the internet is that
    /// peer.
    pub tailnet_identity_only: bool,
    /// How long the configured password is, where there is one.
    pub password_length: Option<usize>,
    /// Bearer tokens are bound to a device key.
    pub bearer_device_bound: bool,
    /// Rate limiting and lockout are on.
    pub rate_limited: bool,
    /// The origins configured, verbatim.
    pub allowed_origins: Vec<String>,
    /// What an unauthenticated caller gets.
    pub default_role: DefaultRole,
}

/// The shortest password omt will publish behind.
///
/// Twelve. Not a policy about entropy — a length below which an online guess
/// against a rate-limited endpoint stops being hopeless, which is the only
/// threat this number addresses.
pub const MIN_PUBLISHED_PASSWORD: usize = 12;

/// Check an instance against the publishing gate.
///
/// Refuses rather than warns, like §2, and for the same reason: the warning
/// would be printed once onto a machine that is now on the internet.
#[must_use]
pub fn check_publication(publication: &Publication) -> Verdict {
    let mut refusals = Vec::new();

    if !publication.auth_backend {
        refusals.push(Refusal {
            problem: "this instance would be on the public internet with no authentication"
                .to_owned(),
            remedy: "Configure auth.password_file or auth.token_file before publishing."
                .to_owned(),
        });
    }
    if publication.tailnet_identity_only {
        refusals.push(Refusal {
            problem: "tailnet identity is the only authentication, and it means nothing here"
                .to_owned(),
            remedy: "Funnel requests arrive from Tailscale's infrastructure, so every caller looks like the same peer. Configure a password or bearer backend.".to_owned(),
        });
    }
    if let Some(length) = publication.password_length
        && length < MIN_PUBLISHED_PASSWORD
        && !publication.bearer_device_bound
    {
        refusals.push(Refusal {
            problem: format!(
                "the configured password is {length} characters, below the {MIN_PUBLISHED_PASSWORD} required to publish"
            ),
            remedy: "Use a longer password, or a device-bound bearer token instead.".to_owned(),
        });
    }
    if !publication.rate_limited {
        refusals.push(Refusal {
            problem: "rate limiting is off on an instance about to be published".to_owned(),
            remedy: "Leave auth.rate_limit at its default, which is on.".to_owned(),
        });
    }
    if publication.allowed_origins.iter().any(|o| o.trim() == "*") {
        refusals.push(Refusal {
            problem: "`allowed_origins` contains `*`, which allows every page on the internet"
                .to_owned(),
            remedy: "Set it to the Funnel hostname exactly.".to_owned(),
        });
    }
    if publication.allowed_origins.is_empty() {
        refusals.push(Refusal {
            problem: "`allowed_origins` is empty, so the web client cannot connect".to_owned(),
            remedy: "Set it to the Funnel hostname exactly.".to_owned(),
        });
    }
    if publication.default_role != DefaultRole::Viewer {
        refusals.push(Refusal {
            problem: format!(
                "the default role is {:?}; publishing requires Viewer",
                publication.default_role
            ),
            remedy: "Set the default role to viewer. Raising it is per-credential and deliberate, so an unscoped one cannot type into a shell by accident.".to_owned(),
        });
    }

    Verdict {
        refusals,
        warnings: Vec::new(),
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
    fn publishable() -> Publication {
        Publication {
            auth_backend: true,
            tailnet_identity_only: false,
            password_length: Some(20),
            bearer_device_bound: false,
            rate_limited: true,
            allowed_origins: vec!["https://box.example.ts.net".to_owned()],
            default_role: DefaultRole::Viewer,
        }
    }

    #[test]
    fn a_properly_configured_instance_may_be_published() {
        assert!(check_publication(&publishable()).may_start());
    }

    #[test]
    fn publishing_without_authentication_is_refused() {
        let mut p = publishable();
        p.auth_backend = false;
        assert!(!check_publication(&p).may_start());
    }

    #[test]
    fn tailnet_identity_alone_is_refused_because_it_means_nothing_here() {
        // Funnel requests arrive from Tailscale's own infrastructure, so every
        // caller looks like the same peer — and an instance relying on that is
        // one where everybody on the internet is that peer.
        let mut p = publishable();
        p.tailnet_identity_only = true;
        let v = check_publication(&p);
        assert!(!v.may_start());
        assert!(
            v.refusals.iter().any(|r| r.remedy.contains("same peer")),
            "{:?}",
            v.refusals
        );
    }

    #[test]
    fn a_short_password_is_refused_unless_the_token_is_device_bound() {
        let mut p = publishable();
        p.password_length = Some(6);
        assert!(!check_publication(&p).may_start());

        p.bearer_device_bound = true;
        assert!(check_publication(&p).may_start());
    }

    #[test]
    fn a_wildcard_origin_is_refused_outright() {
        // It allows every page on the internet, which is the one thing the
        // origin check exists to prevent.
        let mut p = publishable();
        p.allowed_origins = vec!["*".to_owned()];
        let v = check_publication(&p);
        assert!(!v.may_start());
        assert!(v.refusals.iter().any(|r| r.problem.contains("every page")));
    }

    #[test]
    fn an_empty_origin_list_is_refused_too() {
        // Not for safety — it is already safe — but because it does not work,
        // and finding that out from a phone that will not connect is worse
        // than being told now.
        let mut p = publishable();
        p.allowed_origins.clear();
        assert!(!check_publication(&p).may_start());
    }

    #[test]
    fn rate_limiting_must_be_on() {
        let mut p = publishable();
        p.rate_limited = false;
        assert!(!check_publication(&p).may_start());
    }

    #[test]
    fn the_default_role_must_be_viewer() {
        // So an unscoped credential cannot type into a shell by accident.
        // Raising it is per-credential and deliberate.
        for role in [DefaultRole::None, DefaultRole::Operator] {
            let mut p = publishable();
            p.default_role = role;
            assert!(!check_publication(&p).may_start(), "{role:?}");
        }
    }

    #[test]
    fn every_publishing_refusal_says_what_to_do() {
        let mut p = Publication::default();
        p.default_role = DefaultRole::Operator;
        let v = check_publication(&p);
        assert!(v.refusals.len() >= 4);
        for refusal in v.refusals {
            assert!(!refusal.remedy.is_empty(), "{}", refusal.problem);
        }
    }

    #[test]
    fn all_the_problems_are_listed_at_once() {
        // Publishing is a thing somebody does once, under time pressure,
        // usually to show somebody something. Four sequential refusals is four
        // restarts.
        let v = check_publication(&Publication::default());
        assert!(v.refusals.len() >= 4, "{:?}", v.refusals);
    }


    fn listen(binds: Vec<Bind>) -> Listen {
        Listen {
            binds,
            auth: Auth::default(),
            transport: Transport::default(),
            default_role: DefaultRole::None,
        }
    }

    fn addr(text: &str) -> IpAddr {
        text.parse().expect("an address")
    }

    #[test]
    fn loopback_and_a_socket_need_nothing() {
        // The default has to work with no configuration at all, or the first
        // thing anybody learns about omt is a security error.
        let l = listen(vec![
            Bind::UnixSocket {
                path: "/tmp/omt.sock".to_owned(),
            },
            Bind::Loopback { port: 7878 },
        ]);
        assert!(check(&l).may_start());
    }

    #[test]
    fn an_open_interface_without_auth_refuses_to_start() {
        // A warning about an open port is printed once, into a log nobody
        // reads, on a machine that is now reachable from the network.
        let l = listen(vec![Bind::Interface {
            addr: addr("0.0.0.0"),
            port: 7878,
        }]);
        let v = check(&l);
        assert!(!v.may_start());
        assert!(
            v.refusals.iter().any(|r| r.problem.contains("without authentication")),
            "{:?}",
            v.refusals
        );
    }

    #[test]
    fn every_refusal_says_what_to_do_about_it() {
        // A daemon that exits saying only "insecure configuration" leaves
        // somebody to guess, and the guess they reach for is the one that
        // turns the check off.
        let l = listen(vec![Bind::Interface {
            addr: addr("192.168.1.10"),
            port: 7878,
        }]);
        for refusal in check(&l).refusals {
            assert!(!refusal.remedy.is_empty(), "{}", refusal.problem);
        }
    }

    #[test]
    fn all_the_problems_are_reported_not_just_the_first() {
        // Fixing one and being told about the next on the following run is a
        // conversation nobody enjoys having four times.
        let l = listen(vec![Bind::Interface {
            addr: addr("192.168.1.10"),
            port: 7878,
        }]);
        assert_eq!(check(&l).refusals.len(), 2, "auth and plaintext");
    }

    #[test]
    fn plaintext_on_a_public_interface_refuses_unless_a_proxy_is_declared() {
        // "There is probably a proxy" is how a plaintext port ends up on the
        // internet. Someone has to write it down.
        let mut l = listen(vec![Bind::Interface {
            addr: addr("192.168.1.10"),
            port: 7878,
        }]);
        l.auth.backend = true;
        assert!(!check(&l).may_start());

        l.transport.trust_proxy = true;
        assert!(check(&l).may_start());
    }

    #[test]
    fn terminating_tls_directly_is_equally_acceptable() {
        let mut l = listen(vec![Bind::Interface {
            addr: addr("192.168.1.10"),
            port: 7878,
        }]);
        l.auth.backend = true;
        l.transport.tls = true;
        assert!(check(&l).may_start());
    }

    #[test]
    fn a_tailnet_bind_needs_either_a_backend_or_a_working_localapi() {
        let mut l = listen(vec![Bind::Tailnet { port: 7878 }]);
        assert!(!check(&l).may_start());

        l.auth.tailnet_identity = true;
        assert!(check(&l).may_start());

        let mut with_password = listen(vec![Bind::Tailnet { port: 7878 }]);
        with_password.auth.backend = true;
        assert!(check(&with_password).may_start());
    }

    #[test]
    fn an_interface_bind_that_names_loopback_is_loopback() {
        // Refusing it would be pedantry, and pedantry in a security check
        // teaches people to work around the check.
        for text in ["127.0.0.1", "::1"] {
            let l = listen(vec![Bind::Interface {
                addr: addr(text),
                port: 7878,
            }]);
            assert!(check(&l).may_start(), "{text}");
        }
    }

    #[test]
    fn binding_everything_with_a_default_role_warns_prominently() {
        // A real configuration somebody may want on a private network, and one
        // where anybody who reaches the port gets a role without presenting
        // anything.
        let mut l = listen(vec![Bind::Interface {
            addr: addr("0.0.0.0"),
            port: 7878,
        }]);
        l.auth.backend = true;
        l.transport.tls = true;
        l.default_role = DefaultRole::Viewer;
        let v = check(&l);
        assert!(v.may_start());
        assert_eq!(v.warnings.len(), 1);
        assert!(v.warnings[0].note.contains("Viewer"), "{:?}", v.warnings);
    }

    #[test]
    fn requiring_a_credential_produces_no_such_warning() {
        let mut l = listen(vec![Bind::Interface {
            addr: addr("0.0.0.0"),
            port: 7878,
        }]);
        l.auth.backend = true;
        l.transport.tls = true;
        l.default_role = DefaultRole::None;
        assert!(check(&l).warnings.is_empty());
    }

    #[test]
    fn a_socket_from_a_different_uid_is_refused() {
        // omt cannot stop a same-uid process — it has the same privileges omt
        // does — but a different uid has no business here at all.
        assert!(peer_allowed(501, 501));
        assert!(!peer_allowed(501, 0));
        assert!(!peer_allowed(501, 502));
    }

    #[test]
    fn a_peer_record_says_which_process_not_merely_that_one_connected() {
        // The point of recording is to answer *which*, weeks later, for
        // somebody who noticed something odd.
        let record = PeerRecord {
            pid: 4242,
            uid: 501,
            argv: "/usr/bin/something --flag".to_owned(),
            allowed: true,
        };
        let json = serde_json::to_string(&record).expect("json");
        assert!(json.contains("4242"));
        assert!(json.contains("--flag"));
    }
}
