//! The browser surface.

use anyhow::{Context, Result};
use omt_auth::CredentialStore;
use omt_server::HttpState;
use omt_types::Role;

use crate::state::State;

/// Start the HTTP and WebSocket server, minting a token to connect with.
///
/// The token is printed once, here, and nowhere else — there is no way to
/// recover it afterwards, which is what makes a leaked state directory not a
/// leaked credential.
///
/// # Errors
/// Fails if the address cannot be bound.
pub fn run(bind: &str) -> Result<()> {
    run_with_tls(bind, None)
}

/// Start the server, with TLS when a certificate is given.
///
/// # Errors
/// Fails if the address cannot be bound or the certificate cannot be read.
pub fn run_with_tls(bind: &str, tls: Option<omt_server::TlsFiles>) -> Result<()> {
    let state = State::default();
    // Started before the server, because a session created by the first request
    // must produce output. Without this, a browser can open a shell, type into
    // it, and watch a screen that never changes — every byte sits in the pty
    // buffer with nobody reading it.
    spawn_pump(state.clone());
    // Scheduled jobs fire from here too. A job that only runs when a TUI is
    // open is not a scheduled job.
    crate::scheduler::spawn(state.clone());
    // The directory `omt web` was started in, opened before anything is served.
    // A browser has no filesystem of its own to point at, so an instance with
    // no workspace gives it a session-creation form it cannot fill in — and the
    // local `omt` already treats the working directory as the obvious answer.
    if let (Ok(cwd), Ok(mut instance)) = (std::env::current_dir(), state.lock()) {
        let _ = instance.open_workspace(&cwd.display().to_string());
    }
    let events_state = state.clone();

    let mut credentials = CredentialStore::new();
    // An invite, not a credential. Printed as a credential — which this did —
    // the QR on somebody's screen is permanent access for anybody who ever saw
    // it, with nothing to revoke short of the token itself. An invite expires,
    // is exchanged once, and buys a credential belonging to the device that
    // exchanged it, which is a thing somebody can revoke when a phone is lost.
    // Admin, because this invite is printed on the machine's own terminal and
    // whoever reads it is standing at it. Minted as an operator — which it was
    // — the first device could never invite a second or revoke a lost one, so
    // device management existed and nobody could reach it.
    //
    // Every invite made *from* a device is an operator invite, so admin stays
    // with whoever started the daemon rather than spreading by default.
    let minted = credentials.mint_invite(Role::Admin, omt_types::Timestamp::now());

    let scheme = if tls.is_some() { "https" } else { "http" };
    println!("omt web on {scheme}://{bind}");

    // The QR, not the token, is the thing to point a phone at. Printed first
    // and largest because reading 32 base62 characters off one screen and
    // typing them into another is the step people give up at — and the step
    // where a token gets pasted into a chat window to get it across, which
    // puts it in somebody else's scrollback for good.
    //
    // Built from the address a phone can reach, which is not always the one
    // bound to: `0.0.0.0` means every interface to a socket and nothing at all
    // to a browser.
    let address = crate::pairing::reachable_address(bind, crate::pairing::local_ip);
    let url = crate::pairing::invite_url(scheme, &address, &minted.token);
    match crate::pairing::qr_block(&url) {
        Ok(block) => {
            println!("\nScan to open on your phone:\n");
            print!("{block}");
            println!("\n{url}");
        }
        // A terminal that cannot show the code still has to be usable, so the
        // link is printed either way below.
        Err(_) => println!("\n{url}"),
    }
    println!(
        "(this invite expires in {} minutes and can be used once)",
        omt_auth::INVITE_TTL_SECS / 60
    );

    if is_loopback(bind) {
        // A loopback address in a QR is a code that opens the phone's *own*
        // localhost. It scans perfectly and goes nowhere, which is the most
        // confusing possible failure.
        println!(
            "note: this address only works on this machine — restart with \
             `omt web --bind 0.0.0.0:PORT` to pair a phone"
        );
    }

    // Said out loud when it is missing, because the whole point of the warning
    // is that a token crossing a network in clear text is a token somebody
    // else now has.
    if tls.is_none() && !is_loopback(bind) {
        println!(
            "warning: no TLS on a non-loopback address — anyone on this network can read the token"
        );
    }

    // The event source, so a subscribing client is *sent* what happens rather
    // than polling for it.
    let state_for_events = std::sync::Arc::new(crate::events::InstanceEvents(events_state));
    // One store, shared: the server authenticates against it and the device
    // capabilities revoke through it. Two copies would let a revoked device
    // keep working until whichever one the server consulted caught up.
    let credentials = std::sync::Arc::new(std::sync::Mutex::new(credentials));
    // One watch, shared: the socket writes presence and the audit log into it,
    // and the capabilities that report them read the same one. Two would answer
    // "who is connected" differently depending on which was asked.
    let watch: omt_server::watch::Shared = std::sync::Arc::default();
    if let Ok(mut w) = watch.lock() {
        w.started(omt_types::Timestamp::now());
    }
    // One directory, shared: the join registers a device into it and the
    // identity capabilities read and revoke through the same one.
    let identities: std::sync::Arc<std::sync::Mutex<omt_identity::Directory>> =
        std::sync::Arc::default();
    let registry = crate::capabilities::registry_full(
        state,
        credentials.clone(),
        watch.clone(),
        identities.clone(),
    )
    .context("building the registry")?;
    // The origins this instance is served from, which is what 13 §6 says the
    // allow-list defaults to. Without this the check would reject the very
    // page omt itself serves — the allow-list is empty by default so that a
    // daemon serving no browser refuses browser traffic, and a daemon that
    // *is* serving one has to say where from.
    let http = HttpState::sharing(registry, credentials)
        .with_origins(served_origins(bind))
        .watching(watch)
        .with_identities(identities)
        .with_events(state_for_events);
    omt_server::http::run_with_tls(bind, http, tls).with_context(|| format!("serving on {bind}"))
}

/// Every origin a browser could legitimately reach this instance at.
///
/// Both schemes, because whether TLS terminates here or at a proxy in front is
/// not knowable from the bind address — and a page served over https that omt
/// rejected for saying so would be a very confusing afternoon.
///
/// A wildcard bind is served at every address, and the origin a browser reports
/// is whichever one somebody typed. There is no list to enumerate, so the
/// loopback forms are listed and anything else is configuration the operator
/// states — which is the same shape as the bind policy: the escape hatch is a
/// file, not a wildcard.
fn served_origins(bind: &str) -> Vec<String> {
    // Parsed rather than string-sliced, because `[::1]:7788` and `0.0.0.0:80`
    // both have colons in places a naive split gets wrong.
    let Ok(bind) = bind.parse::<std::net::SocketAddr>() else {
        // An unparseable bind is a problem the listener will report properly.
        // Guessing an origin from it would be inventing an allow-list entry
        // out of a string omt does not understand.
        return Vec::new();
    };
    let port = bind.port();
    let mut out = Vec::new();
    for host in ["127.0.0.1", "localhost", "[::1]"] {
        out.push(format!("http://{host}:{port}"));
        out.push(format!("https://{host}:{port}"));
    }
    if !bind.ip().is_loopback() && !bind.ip().is_unspecified() {
        let ip = bind.ip();
        let host = if ip.is_ipv6() {
            format!("[{ip}]")
        } else {
            ip.to_string()
        };
        out.push(format!("http://{host}:{port}"));
        out.push(format!("https://{host}:{port}"));
    }
    out
}

/// How often the pump looks for output.
///
/// A compromise, and an honest one: shorter burns a core on an idle instance,
/// longer is visible as lag between a keystroke and its echo. This is under the
/// threshold where typing feels delayed and far above a busy loop.
const PUMP_INTERVAL: std::time::Duration = std::time::Duration::from_millis(8);

/// Read every session's pty, forever.
///
/// Public because `omt serve` needs it too: a session in a daemon nobody is
/// watching still has to advance, or its pty buffer fills and the program on
/// the far end blocks.
///
/// The local TUI does this inside its render loop, which is why a terminal
/// session works and — until this existed — a browser one did not. Serving is
/// not enough: somebody has to move the bytes.
pub fn spawn_pump(state: State) {
    std::thread::spawn(move || {
        loop {
            // The lock is taken and dropped inside each tick, never held across
            // the sleep: holding it would block every capability call for the
            // entire interval and make the API stutter in time with the pump.
            if let Ok(mut instance) = state.lock() {
                let ids: Vec<_> = instance.sessions().iter().map(|s| s.id).collect();
                for id in ids {
                    // A session whose process died fails here, which is not
                    // worth stopping the pump for — the others are still live,
                    // and the close arrives through its own path.
                    let _ = instance.pump_session(id);
                }
            }
            std::thread::sleep(PUMP_INTERVAL);
        }
    });
}

/// Whether an address only accepts connections from this machine.
///
/// Textual rather than resolved: this decides whether to print a warning, and a
/// warning that needs DNS to appear is one that does not appear when it matters.
#[must_use]
pub fn is_loopback(bind: &str) -> bool {
    let host = bind.rsplit_once(':').map_or(bind, |(h, _)| h);
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host == "127.0.0.1" || host == "localhost" || host == "::1" || host.starts_with("127.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_addresses_are_recognised_in_every_spelling() {
        for bind in [
            "127.0.0.1:7777",
            "localhost:7777",
            "[::1]:7777",
            "127.1.2.3:80",
        ] {
            assert!(is_loopback(bind), "{bind} was treated as public");
        }
    }

    #[test]
    fn a_public_address_is_not_mistaken_for_loopback() {
        // Getting this wrong the other way is the dangerous direction: the
        // warning that a token is crossing a network in clear text would not
        // be printed.
        for bind in ["0.0.0.0:7777", "192.168.1.10:7777", "box.local:7777"] {
            assert!(!is_loopback(bind), "{bind} was treated as loopback");
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod origin_tests {
    use super::served_origins;

    #[test]
    fn the_page_omt_serves_is_an_allowed_origin() {
        // The check would otherwise reject the very page omt itself serves,
        // which is how a security control gets turned off rather than fixed.
        let origins = served_origins("127.0.0.1:7788");
        assert!(origins.contains(&"http://127.0.0.1:7788".to_owned()));
        assert!(origins.contains(&"http://localhost:7788".to_owned()));
    }

    #[test]
    fn both_schemes_are_allowed_because_a_proxy_may_terminate_tls() {
        // Whether TLS terminates here or in front is not knowable from the
        // bind address, and rejecting an https page for saying so would be a
        // very confusing afternoon.
        let origins = served_origins("127.0.0.1:7788");
        assert!(origins.contains(&"https://127.0.0.1:7788".to_owned()));
    }

    #[test]
    fn a_tailnet_address_is_allowed_at_its_own_host() {
        let origins = served_origins("100.100.1.3:7788");
        assert!(origins.contains(&"http://100.100.1.3:7788".to_owned()));
    }

    #[test]
    fn an_ipv6_host_is_bracketed() {
        // `http://::1:7788` is not an origin any browser will send.
        let origins = served_origins("[::1]:7788");
        assert!(origins.contains(&"http://[::1]:7788".to_owned()));
    }

    #[test]
    fn a_wildcard_bind_does_not_become_a_wildcard_allow_list() {
        // Anything beyond loopback on an unspecified bind is configuration the
        // operator states — the same shape as the bind policy, where the
        // escape hatch is a file rather than a wildcard.
        let origins = served_origins("0.0.0.0:7788");
        assert!(origins.iter().all(|o| o.contains("127.0.0.1") || o.contains("localhost") || o.contains("[::1]")));
        assert!(!origins.iter().any(|o| o.contains("0.0.0.0")));
    }

    #[test]
    fn an_unparseable_bind_invents_nothing() {
        // Guessing an origin out of a string omt does not understand would be
        // inventing an allow-list entry.
        assert!(served_origins("not-an-address").is_empty());
    }
}
