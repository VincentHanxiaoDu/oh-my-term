//! Listening for remote clients on a Unix socket.
//!
//! The socket is the local surface: the hook reaches it, the CLI reaches it,
//! and an ssh forward or a web bridge reaches it from further away. Everything
//! that arrives here goes through the same dispatch the local TUI uses.

use std::io::{self, Write};
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
    // Sessions in a daemon nobody is looking at still have to advance: a pty
    // whose output is never read fills its buffer and the program blocks. This
    // is the same pump `omt web` runs, and for the same reason.
    crate::web::spawn_pump(state.clone());
    crate::scheduler::spawn(state.clone());

    // A socket left behind by a daemon that died is a file that exists and
    // refuses every connection. Removing it here means a crash does not
    // require a manual cleanup before omt will start again.
    if !crate::attach::daemon_is_running(path) {
        let _ = std::fs::remove_file(path);
    }

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

/// Relay this process's stdin and stdout to a local instance's socket.
///
/// What `omt ssh` runs on the far side. The far side resolves its own socket,
/// which is the point: a path forwarded from the near side could point at an
/// instance that has since been replaced, and the client would attach to
/// somebody else's sessions without either end noticing.
///
/// # Errors
/// Fails if the socket cannot be reached.
pub fn bridge(socket: Option<&Path>) -> Result<()> {
    let path = socket.map_or_else(default_socket_path, Path::to_path_buf);
    let stream = omt_transport::connect(&path)
        .with_context(|| format!("connecting to {}", path.display()))?;

    // Two directions, two threads. A single-threaded relay would block one way
    // while waiting on the other, which deadlocks the first time both sides
    // have something to say at once — and both sides do, on every handshake.
    let mut to_socket = stream.try_clone().context("cloning the socket")?;
    let mut from_socket = stream;

    let up = std::thread::spawn(move || {
        let mut stdin = io::stdin().lock();
        let _ = relay(&mut stdin, &mut to_socket);
        // Half-close, so the far side sees the end of input rather than
        // waiting for bytes that will never come.
        let _ = to_socket.shutdown(std::net::Shutdown::Write);
    });

    let mut stdout = io::stdout().lock();
    let _ = relay(&mut from_socket, &mut stdout);
    let _ = stdout.flush();
    let _ = up.join();
    Ok(())
}

/// Copy, flushing after every chunk.
///
/// Deliberately not `io::copy`, which never flushes. Rust's stdout is
/// block-buffered when it is a pipe — which is exactly what it is under ssh —
/// so the first frame would sit in an eight-kilobyte buffer waiting for
/// traffic that only arrives once it has been answered. The connection appears
/// to hang on the handshake, every time.
fn relay(from: &mut impl io::Read, to: &mut impl Write) -> io::Result<()> {
    let mut buf = [0u8; 32 * 1024];
    loop {
        let n = match from.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        to.write_all(&buf[..n])?;
        to.flush()?;
    }
}
