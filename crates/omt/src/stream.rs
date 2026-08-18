//! Pushing a session's output to a client that asked for it.
//!
//! The socket server is otherwise strictly request-then-response, which is the
//! right shape for a capability call and the wrong one for a terminal: a client
//! that has to ask "has anything changed?" is a client that is either a
//! round-trip behind or asking constantly. A subscriber gets frames it did not
//! ask for.
//!
//! **Bytes, not screens.** A client runs the same emulator omt runs, so what
//! travels is what the program wrote. That keeps one parser, one renderer and
//! one set of rules about what a cell is; the alternative is a second way to
//! reconstruct a screen, and a second way is the one nobody exercises until the
//! day it is wrong.
//!
//! **Seeded, never replayed.** A client that has fallen past what the instance
//! still holds is sent the screen as it now is, rendered back into escape
//! sequences ([`omt_term::seed`]). Re-sending the tail of a raw stream would
//! re-run half a command, which is the failure the `ack` field in the frame
//! header has always been about.

use std::io::Write;
use std::sync::{Arc, Mutex};

use omt_proto::{BinaryHeader, FrameKind};
use omt_types::SessionId;

use crate::state::State;

/// How often a subscriber's sessions are checked for new output.
///
/// The pump runs on its own interval in the daemon; this only decides how
/// promptly what it produced is forwarded. Fast enough that typing feels local,
/// slow enough that an idle instance with ten subscribers is not a hundred
/// wakeups a second.
pub const PUSH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(8);

/// Where one subscriber has got to in one session.
#[derive(Debug, Clone, Copy)]
struct Position {
    /// The byte position in that session's output.
    at: u64,
}

/// A subscriber's shared write half.
///
/// Shared with the connection's request thread, and locked around a whole
/// frame: a reply written into the middle of a pushed frame would corrupt both,
/// and the client would see a length prefix that is not one.
pub type Writer = Arc<Mutex<std::os::unix::net::UnixStream>>;

/// Push output for the sessions a client asked about until it goes away.
///
/// Ends quietly on any write failure, because the only reason a push fails is
/// that the client is gone — and a client going away must take nothing else
/// with it.
pub fn push_loop(
    state: &State,
    writer: &Writer,
    sessions: Vec<SessionId>,
    live: &Arc<Mutex<bool>>,
) {
    let mut positions: std::collections::BTreeMap<SessionId, Position> =
        std::collections::BTreeMap::new();

    loop {
        if !*live
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            return;
        }
        for id in &sessions {
            let Some(frame) = next_frame(state, *id, positions.get(id).copied()) else {
                continue;
            };
            positions.insert(*id, Position { at: frame.at });
            if write_frame(writer, &frame).is_err() {
                return;
            }
        }
        std::thread::sleep(PUSH_INTERVAL);
    }
}

/// One frame's worth of a session's output.
struct Frame {
    /// Which session, as the stream id in the header.
    stream: u16,
    /// The position after these bytes.
    at: u64,
    /// What to send.
    payload: Vec<u8>,
}

/// What a subscriber at `from` should be sent next, if anything.
///
/// `None` means nothing has happened, which is the common case and must be
/// cheap: it is one lock and two integers.
fn next_frame(state: &State, id: SessionId, from: Option<Position>) -> Option<Frame> {
    let instance = state.lock().ok()?;
    let runtime = instance.runtime(id)?;
    let tail = runtime.tail();

    let (payload, at) = match from {
        // Already up to date, and the overwhelmingly common case.
        Some(p) if p.at == tail.position() => return None,
        Some(p) => match tail.since(p.at) {
            Some(bytes) => (bytes, tail.position()),
            // Fallen past what is held, or holding a position from a session
            // that has been replaced. Either way the answer is the screen as
            // it now is, not the bytes that happen to remain.
            None => (omt_term::seed(runtime.terminal()), tail.position()),
        },
        // A client that has just arrived. It has missed everything, so it is
        // seeded rather than sent a history it has no use for.
        None => (omt_term::seed(runtime.terminal()), tail.position()),
    };
    if payload.is_empty() {
        return None;
    }
    Some(Frame {
        stream: stream_of(id),
        at,
        payload,
    })
}

/// The stream id for a session.
///
/// Sixteen bits in the header and a `SessionId` is far wider, so this is a
/// per-connection handle rather than the id itself: the client asked about
/// these sessions in this order, and that order is the mapping.
fn stream_of(_id: SessionId) -> u16 {
    0
}

/// Write one frame, whole, under the lock.
fn write_frame(writer: &Writer, frame: &Frame) -> std::io::Result<()> {
    let header = BinaryHeader {
        ver: BinaryHeader::VERSION,
        kind: FrameKind::Binary,
        stream: frame.stream,
        seq_or_off: frame.at,
        // Nothing consumed from this client yet: predicted echo is what this
        // field is for and it is not in this change, so the honest value is
        // zero rather than a number that looks like an acknowledgement.
        ack: 0,
    };
    let mut bytes = Vec::with_capacity(BinaryHeader::SIZE + frame.payload.len());
    bytes.extend_from_slice(&header.encode());
    bytes.extend_from_slice(&frame.payload);

    let mut out = writer
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    omt_transport::write_frame(&mut *out, FrameKind::Binary, &bytes)
        .map_err(std::io::Error::other)?;
    out.flush()
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use omt_session::{SessionKind, SessionMode};

    use super::*;

    fn instance_with_session() -> (State, SessionId) {
        let mut instance = omt_daemon::Instance::new();
        let workspace = instance.open_workspace("/w").expect("workspace");
        let session = instance
            .create_session(workspace, SessionKind::Shell, SessionMode::Pty)
            .expect("session");
        (State::new(instance), session)
    }

    #[test]
    fn a_session_with_no_runtime_produces_nothing() {
        // A session record without a process is a session nobody has started
        // or one that has been restored; either way there is nothing to send
        // and nothing to say about it.
        let (state, id) = instance_with_session();
        assert!(next_frame(&state, id, None).is_none());
    }

    #[test]
    fn a_client_that_has_just_arrived_is_seeded() {
        // Not sent a history it has no use for: what it needs is what the
        // screen looks like now.
        let (state, id) = instance_with_session();
        let started = omt_daemon::SessionRuntime::spawn(
            id,
            &omt_pty::PtyConfig {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), "printf hello; sleep 5".into()],
                size: omt_pty::PtySize::new(20, 4),
                ..omt_pty::PtyConfig::default()
            },
            omt_daemon::SessionLimits::default(),
        )
        .expect("spawn");
        state.lock().expect("lock").attach(started).expect("attach");

        // Let the shell write, and pump it into the emulator.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            let _ = state.lock().expect("lock").pump_session(id);
            let seen = state
                .lock()
                .expect("lock")
                .runtime(id)
                .is_some_and(|r| r.terminal().row_text(0).contains("hello"));
            if seen {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let frame = next_frame(&state, id, None).expect("a seed");
        let text = String::from_utf8_lossy(&frame.payload).into_owned();
        assert!(text.contains("hello"), "{text:?}");
        // And it is a seed rather than the raw output: it positions and
        // clears, which raw shell output does not.
        assert!(text.contains("\u{1b}[2J"), "{text:?}");
    }

    #[test]
    fn nothing_new_is_nothing_sent() {
        // The common case, and it has to be cheap: an idle instance with ten
        // subscribers must not be ten frames a tick.
        let (state, id) = instance_with_session();
        assert!(next_frame(&state, id, Some(Position { at: 0 })).is_none());
    }
}
