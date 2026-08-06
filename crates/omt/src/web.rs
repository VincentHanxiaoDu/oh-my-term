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
    let registry = crate::capabilities::registry(state).context("building the registry")?;

    let mut credentials = CredentialStore::new();
    let minted = credentials.mint(Role::Operator, "first run", None, None);

    println!("omt web on http://{bind}");
    println!("token: {}", minted.token);
    println!("(shown once — it cannot be recovered)");

    omt_server::http::run(bind, HttpState::new(registry, credentials))
        .with_context(|| format!("serving on {bind}"))
}

/// How often the pump looks for output.
///
/// A compromise, and an honest one: shorter burns a core on an idle instance,
/// longer is visible as lag between a keystroke and its echo. This is under the
/// threshold where typing feels delayed and far above a busy loop.
const PUMP_INTERVAL: std::time::Duration = std::time::Duration::from_millis(8);

/// Read every session's pty, forever.
///
/// The local TUI does this inside its render loop, which is why a terminal
/// session works and — until this existed — a browser one did not. Serving is
/// not enough: somebody has to move the bytes.
fn spawn_pump(state: State) {
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
