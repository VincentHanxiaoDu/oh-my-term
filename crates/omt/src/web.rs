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
    let registry = crate::capabilities::registry(state).context("building the registry")?;

    let mut credentials = CredentialStore::new();
    let minted = credentials.mint(Role::Operator, "first run", None, None);

    println!("omt web on http://{bind}");
    println!("token: {}", minted.token);
    println!("(shown once — it cannot be recovered)");

    omt_server::http::run(bind, HttpState::new(registry, credentials))
        .with_context(|| format!("serving on {bind}"))
}
