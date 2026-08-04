//! Listening for remote clients on a Unix socket.
//!
//! The socket is the local surface: the hook reaches it, the CLI reaches it,
//! and an ssh forward or a web bridge reaches it from further away. Everything
//! that arrives here goes through the same dispatch the local TUI uses.

use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use omt_proto::{FrameKind, ProtoMessage};
use omt_server::{Peer, handle};
use omt_types::{Actor, Role};

use crate::state::State;

/// Where an instance listens by default.
///
/// Under the runtime directory rather than a temp path, and named by pid so
/// two instances on one machine do not collide — which they would, and the
/// second would silently talk to the first's sessions.
#[must_use]
pub fn default_socket_path() -> PathBuf {
    let base = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join(format!("omt-{}.sock", std::process::id()))
}

/// Serve until the listener is dropped.
///
/// One thread per connection. That is the right shape for the traffic: a
/// handful of long-lived clients, each mostly idle, and a thread that blocks on
/// a read costs a stack rather than a spin.
///
/// # Errors
/// Fails if the socket cannot be created. Once listening it does not return:
/// an individual bad accept is skipped rather than ending the server.
pub fn serve(path: &Path, state: State) -> Result<()> {
    let listener = omt_transport::SocketListener::bind(path)
        .with_context(|| format!("binding {}", path.display()))?;

    loop {
        // `accept` hands back the credentials the kernel recorded at connect
        // time. A peer cannot restate them, which is what makes this an
        // authorization signal rather than a claim.
        let (stream, credentials) = match listener.accept() {
            Ok(pair) => pair,
            // One bad accept must not end the server: a client that vanished
            // mid-handshake is a normal event, not a reason to stop listening.
            Err(_) => continue,
        };

        if !credentials.is_same_user() {
            // Another user's process on the same machine gets nothing. There
            // is no configuration for this: a terminal multiplexer that could
            // be driven by a neighbouring account is a remote shell.
            continue;
        }

        let state = state.clone();
        std::thread::spawn(move || {
            let _ = serve_connection(stream, &state);
        });
    }
}

fn serve_connection(mut stream: std::os::unix::net::UnixStream, state: &State) -> io::Result<()> {
    let registry = crate::capabilities::registry(state.clone())
        .map_err(|e| io::Error::other(e.to_string()))?;
    // Local, same-user, kernel-verified. Role comes from a credential once
    // tokens are wired; until then this socket is exactly as privileged as the
    // account that owns it, which is what it already was.
    let mut peer = Peer::new(Actor::Local, Role::Operator);

    loop {
        let (_, payload) = match omt_transport::read_frame(&mut stream) {
            Ok(frame) => frame,
            // A client that hung up is the normal end of a connection.
            Err(_) => return Ok(()),
        };
        let Ok(message) = serde_json::from_slice::<ProtoMessage>(&payload) else {
            // Unparseable input ends this connection and nothing else. Trying
            // to resynchronize a stream whose framing may be wrong is how one
            // bad client corrupts another's replies.
            return Ok(());
        };
        if let Some(reply) = handle(&registry, &mut peer, message) {
            let bytes = serde_json::to_vec(&reply).map_err(io::Error::other)?;
            if omt_transport::write_frame(&mut stream, FrameKind::Text, &bytes).is_err() {
                return Ok(());
            }
        }
    }
}
