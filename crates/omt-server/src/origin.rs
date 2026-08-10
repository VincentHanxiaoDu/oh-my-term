//! Deciding whether a browser request came from somewhere omt serves.
//!
//! `docs/architecture/13-security.md` §6. The daemon serves the web client, and
//! it must assume the same browser is simultaneously running hostile pages.
//!
//! The primary defence is the `Origin` header. A page at `evil.example` can
//! open a WebSocket to `ws://127.0.0.1:7878` — nothing stops it — but the
//! browser sends `Origin: https://evil.example` on the handshake and does not
//! let the page forge it. Checking that header is the difference between a
//! local daemon and a local daemon anybody's browser tab can drive.
//!
//! **A missing `Origin` on a state-changing request is rejected, not waved
//! through.** That is the decision worth stating: absent is not the same as
//! same-origin, and treating it as such reopens the hole for every client that
//! omits the header — which is exactly what a hostile page would do if omitting
//! it worked.
//!
//! There is no cookie anywhere in this design. Authentication is a bearer token
//! in the protocol, so there is no ambient authority for a cross-site request
//! to borrow, and classic CSRF does not apply to the WebSocket API at all.

use serde::Serialize;

/// What omt decided about a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// It came from somewhere omt serves.
    Allow,
    /// It did not, and why.
    Reject {
        /// What to log and return.
        reason: String,
    },
}

impl Decision {
    /// Whether the request may proceed.
    #[must_use]
    pub const fn allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }
}

/// Normalise an origin for comparison.
///
/// Scheme and authority only, lowercased, with a trailing slash removed. An
/// origin is not a URL and comparing them as strings is how `https://a.example`
/// and `https://a.example/` end up being two different places.
#[must_use]
pub fn normalise(origin: &str) -> String {
    let lower = origin.trim().trim_end_matches('/').to_lowercase();
    // Anything past the authority is not part of an origin. A header carrying
    // a path is either a broken client or an attempt to smuggle a prefix past
    // a `starts_with` check somewhere.
    match lower.split_once("://") {
        None => lower,
        Some((scheme, rest)) => {
            let authority = rest.split('/').next().unwrap_or(rest);
            format!("{scheme}://{authority}")
        }
    }
}

/// Whether a request may proceed.
///
/// `state_changing` covers every WebSocket upgrade and every REST request that
/// is not a plain read of the static bundle. The bundle itself is
/// unauthenticated and inert, so serving it to anybody costs nothing.
#[must_use]
pub fn check(origin: Option<&str>, allowed: &[String], state_changing: bool) -> Decision {
    if !state_changing {
        return Decision::Allow;
    }

    let Some(origin) = origin else {
        // Absent is not same-origin. Treating it as such would reopen the hole
        // for every client that omits the header, which is precisely what a
        // hostile page would do if omitting it worked.
        return Decision::Reject {
            reason: "no Origin header on a state-changing request".to_owned(),
        };
    };

    let wanted = normalise(origin);
    if wanted.is_empty() {
        return Decision::Reject {
            reason: "an empty Origin is not an origin".to_owned(),
        };
    }
    if allowed.iter().any(|a| normalise(a) == wanted) {
        Decision::Allow
    } else {
        Decision::Reject {
            reason: format!("`{wanted}` is not an origin this instance is served from"),
        }
    }
}

/// The content security policy the served bundle carries.
///
/// `'self'` everywhere and nothing else. The bundle has no CDN, no analytics
/// and no font service, so every relaxation would be for something that does
/// not exist — and a policy with an unused exception in it is a policy somebody
/// later uses.
#[must_use]
pub const fn content_security_policy() -> &'static str {
    "default-src 'self'; connect-src 'self' ws: wss:; img-src 'self' data: blob:; \
     style-src 'self' 'unsafe-inline'; script-src 'self'; object-src 'none'; \
     base-uri 'none'; frame-ancestors 'none'"
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn allowed() -> Vec<String> {
        vec![
            "http://127.0.0.1:7788".to_owned(),
            "https://box.tailnet.ts.net".to_owned(),
        ]
    }

    #[test]
    fn a_page_omt_serves_is_allowed() {
        assert!(check(Some("http://127.0.0.1:7788"), &allowed(), true).allowed());
    }

    #[test]
    fn a_hostile_page_is_rejected() {
        // The whole point: `evil.example` can open the socket, but the browser
        // tells omt where the page came from and does not let it lie.
        let d = check(Some("https://evil.example"), &allowed(), true);
        assert!(!d.allowed());
        let Decision::Reject { reason } = d else {
            panic!("expected a rejection");
        };
        assert!(reason.contains("evil.example"), "{reason}");
    }

    #[test]
    fn a_missing_origin_is_rejected_rather_than_assumed_same_origin() {
        // Absent is not same-origin. If omitting the header worked, that is
        // exactly what a hostile page would do.
        assert!(!check(None, &allowed(), true).allowed());
    }

    #[test]
    fn the_static_bundle_is_served_to_anybody() {
        // It is unauthenticated and inert, so serving it costs nothing — and
        // refusing it would break the page that has not authenticated yet.
        assert!(check(None, &allowed(), false).allowed());
        assert!(check(Some("https://evil.example"), &allowed(), false).allowed());
    }

    #[test]
    fn a_trailing_slash_does_not_make_it_a_different_place() {
        assert!(check(Some("http://127.0.0.1:7788/"), &allowed(), true).allowed());
    }

    #[test]
    fn case_does_not_make_it_a_different_place() {
        assert!(check(Some("HTTP://127.0.0.1:7788"), &allowed(), true).allowed());
    }

    #[test]
    fn a_port_is_part_of_the_origin() {
        // Otherwise anything else on the machine's loopback is same-origin,
        // which on a developer's laptop is a great many things.
        assert!(!check(Some("http://127.0.0.1:9999"), &allowed(), true).allowed());
    }

    #[test]
    fn a_scheme_is_part_of_the_origin() {
        assert!(!check(Some("https://127.0.0.1:7788"), &allowed(), true).allowed());
    }

    #[test]
    fn an_origin_carrying_a_path_is_reduced_to_its_authority() {
        // A header with a path is a broken client or an attempt to smuggle a
        // prefix past a `starts_with` check somewhere.
        assert_eq!(
            normalise("https://evil.example/http://127.0.0.1:7788"),
            "https://evil.example"
        );
        assert!(
            !check(
                Some("https://evil.example/http://127.0.0.1:7788"),
                &allowed(),
                true
            )
            .allowed()
        );
    }

    #[test]
    fn a_prefix_of_an_allowed_origin_is_not_allowed() {
        // `starts_with` would let `http://127.0.0.1:77880` through, and a
        // suffix check would let `http://evil.example#http://127.0.0.1:7788`.
        assert!(!check(Some("http://127.0.0.1:77880"), &allowed(), true).allowed());
        assert!(!check(Some("http://x127.0.0.1:7788"), &allowed(), true).allowed());
    }

    #[test]
    fn an_empty_origin_is_not_an_origin() {
        assert!(!check(Some(""), &allowed(), true).allowed());
        assert!(!check(Some("   "), &allowed(), true).allowed());
    }

    #[test]
    fn nothing_is_allowed_when_nothing_is_configured() {
        // An empty allow-list is the safe direction. A configuration mistake
        // that opened everything would be silent; one that closes everything
        // is reported by the first person who tries to connect.
        assert!(!check(Some("http://127.0.0.1:7788"), &[], true).allowed());
    }

    #[test]
    fn the_policy_names_no_host_omt_does_not_serve() {
        // The bundle has no CDN, no analytics and no font service, so every
        // relaxation would be for something that does not exist — and a policy
        // with an unused exception in it is one somebody later uses.
        let policy = content_security_policy();
        assert!(policy.contains("default-src 'self'"));
        assert!(policy.contains("object-src 'none'"));
        assert!(policy.contains("frame-ancestors 'none'"));
        assert!(!policy.contains("http://"), "{policy}");
        assert!(!policy.contains(".com"), "{policy}");
    }
}
