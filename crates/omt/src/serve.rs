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

/// Refuse to start in a directory this uid does not own.
///
/// A predictable fallback path under `/tmp` is one a hostile local user can
/// pre-create, and then every blob omt writes lands in their directory. The
/// check is on the socket's parent because that is the directory omt is about
/// to write into (22 §2.3).
fn guard_runtime_dir(socket: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt as _;

    let Some(parent) = socket.parent() else {
        return Ok(());
    };
    // A directory that does not exist yet is one omt is about to create with
    // its own umask, so there is nothing to distrust.
    let Ok(meta) = std::fs::symlink_metadata(parent) else {
        return Ok(());
    };
    #[allow(
        unsafe_code,
        reason = "getuid has no safe wrapper in std and cannot fail"
    )]
    let uid = unsafe { libc::getuid() };
    // The system temp directory is shared by design and omt only ever writes
    // its own uid-suffixed subdirectory inside it, which is the thing worth
    // checking. Checking the shared parent itself would refuse to start on
    // every machine.
    if parent == std::env::temp_dir() {
        return Ok(());
    }
    let verdict = crate::multiuser::check_dir(
        meta.file_type().is_symlink(),
        meta.uid(),
        meta.mode() & 0o777,
        uid,
    )
    .map_err(|refusal| anyhow::anyhow!("refusing to use {}: {refusal}", parent.display()))?;

    if verdict == crate::multiuser::DirVerdict::Correct {
        // Said out loud rather than fixed quietly: a mode a careless `chmod`
        // widened is an event somebody should see.
        eprintln!(
            "omt: narrowed {} to {:04o} — it was readable by other users on this machine",
            parent.display(),
            crate::multiuser::DIR_MODE
        );
        std::fs::set_permissions(
            parent,
            std::os::unix::fs::PermissionsExt::from_mode(crate::multiuser::DIR_MODE),
        )?;
    }
    Ok(())
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
    // Before anything is opened. A daemon started from a shell with `umask 000`
    // would otherwise create 0666 files, and inheriting a umask is not a
    // security decision omt gets to delegate (22 §2.3).
    #[allow(
        unsafe_code,
        reason = "umask has no safe wrapper in std and cannot fail"
    )]
    unsafe {
        libc::umask(crate::multiuser::UMASK as libc::mode_t);
    }
    guard_runtime_dir(path)?;

    // Sessions in a daemon nobody is looking at still have to advance: a pty
    // whose output is never read fills its buffer and the program blocks. This
    // is the same pump `omt web` runs, and for the same reason.
    crate::web::spawn_pump(state.clone());
    crate::scheduler::spawn(state.clone());

    // What was open last time, before anything new is served. A restore that
    // ran after the first client connected would show it an empty instance and
    // then change it underneath — worse than being slightly slower to start.
    // Keyed by this daemon's socket. Shared, two instances on one machine
    // restore each other's sessions and then overwrite each other's snapshot.
    crate::persist::restore_on_start_for(&state, Some(path));
    crate::persist::spawn_autosave_for(state.clone(), Some(path));

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

/// Serve beside a terminal that is drawing its own sessions.
///
/// The same connections [`serve`] takes — capability calls, subscriptions and
/// hook reports — with none of the ownership: no pump, no restore, no
/// autosave. Those belong to whoever *runs* the instance, and this process
/// already does; a second pump reading the same ptys, or a second autosave
/// writing the same snapshot, would race the terminal that started it.
///
/// It listens at all so that agents in these panes have somewhere to report,
/// and so that `omt ls` in another window sees the sessions on this screen
/// rather than nothing at all.
///
/// # Errors
/// Fails if the socket cannot be created.
pub fn serve_beside(path: &Path, state: State) -> Result<()> {
    guard_runtime_dir(path)?;
    let _ = std::fs::remove_file(path);
    let listener = omt_transport::SocketListener::bind(path)
        .with_context(|| format!("binding {}", path.display()))?;
    loop {
        let Ok((stream, credentials)) = listener.accept() else {
            continue;
        };
        // Same rule as the daemon's: another account's process on this machine
        // gets nothing, and there is no setting that changes it.
        if !credentials.is_same_user() {
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

    // The write half, shared with a pusher once one is started. Locked around
    // a whole frame: a reply written into the middle of a pushed frame would
    // corrupt both, and the client would read a length prefix that is not one.
    let writer: crate::stream::Writer =
        std::sync::Arc::new(std::sync::Mutex::new(stream.try_clone()?));
    // How the pusher learns this connection is over. A thread that outlived
    // its connection would hold the instance lock every few milliseconds
    // forever, for a client that is gone.
    let live = std::sync::Arc::new(std::sync::Mutex::new(true));
    let _hangup = Hangup(std::sync::Arc::clone(&live));

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
        // Answered before dispatch, and without a handshake. A hook is a
        // short-lived process reporting an observation, not an actor asking
        // for a mutation — dispatch would refuse it as ungreeted, and making
        // it greet would spend two round trips of an agent's deadline on a
        // negotiation with nothing to negotiate.
        if let ProtoMessage::HookEvent(hook) = message {
            let ack = ProtoMessage::HookAck(crate::hooks::ingest(state, &hook));
            let bytes = serde_json::to_vec(&ack).map_err(io::Error::other)?;
            let mut out = writer
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if omt_transport::write_frame(&mut *out, FrameKind::Text, &bytes).is_err() {
                return Ok(());
            }
            continue;
        }
        // A subscription turns this connection from request-and-answer into
        // something that also speaks unprompted. Dispatch answers `Subscribe`
        // with nothing, which is why it never reached anywhere until now.
        if let ProtoMessage::Subscribe(ref wanted) = message {
            let sessions = wanted.filter.sessions.clone();
            let state = state.clone();
            let writer = std::sync::Arc::clone(&writer);
            let live = std::sync::Arc::clone(&live);
            std::thread::spawn(move || {
                crate::stream::push_loop(&state, &writer, sessions, &live);
            });
            continue;
        }

        if let Some(reply) = handle(&registry, &mut peer, message) {
            let bytes = serde_json::to_vec(&reply).map_err(io::Error::other)?;
            let mut out = writer
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if omt_transport::write_frame(&mut *out, FrameKind::Text, &bytes).is_err() {
                return Ok(());
            }
        }
    }
}

/// Tells a pusher its connection is over.
///
/// A guard rather than a flag somebody remembers to clear: this function
/// returns from a dozen places, and the one that forgot would leave a thread
/// taking the instance lock every few milliseconds for a client that is gone.
struct Hangup(std::sync::Arc<std::sync::Mutex<bool>>);

impl Drop for Hangup {
    fn drop(&mut self) {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
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
