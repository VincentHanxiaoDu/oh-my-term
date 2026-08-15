//! A terminal that watches sessions it does not own.
//!
//! The other side of [`crate::stream`]. It holds a socket, an emulator per
//! session, and nothing else that matters — which is the point: the sessions,
//! their processes, their scrollback and their agents live in the instance, and
//! this can be closed and opened again without any of that noticing.
//!
//! It runs the *same* emulator the local path runs, so everything above it —
//! the renderer, the layout, copy mode, the block model — works unchanged. What
//! changed is where the bytes came from.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use omt_proto::{BinaryHeader, FrameKind, ProtoMessage};
use omt_types::SessionId;

/// What went wrong reaching an instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    /// Nothing was listening, or it refused.
    Unreachable(String),
    /// The connection ended.
    Gone,
    /// A call was refused, or answered with something unusable.
    Refused(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(why) => write!(f, "cannot reach the instance: {why}"),
            Self::Gone => write!(f, "the instance went away"),
            Self::Refused(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for ClientError {}

/// A connection to an instance, and what this client believes about it.
pub struct Client {
    /// Requests go out here; the reader thread owns the other half.
    out: std::os::unix::net::UnixStream,
    /// What has arrived and not been taken yet.
    inbox: Arc<Mutex<Inbox>>,
    /// The next request number, per this client.
    next: u64,
    device: omt_types::DeviceId,
}

/// What the reader thread has collected.
#[derive(Default)]
struct Inbox {
    /// Replies, in arrival order. Small: a client makes few calls at once and
    /// takes each answer before asking again.
    replies: Vec<omt_proto::CallResult>,
    /// Output bytes per stream, waiting to be fed to an emulator.
    output: std::collections::BTreeMap<u16, Vec<u8>>,
    /// Whether the connection is still there.
    live: bool,
}

impl Client {
    /// Connect to an instance's socket.
    ///
    /// # Errors
    /// Fails if nothing is listening there, or if it refuses the handshake.
    pub fn connect(socket: &std::path::Path) -> Result<Self, ClientError> {
        let out =
            omt_transport::connect(socket).map_err(|e| ClientError::Unreachable(e.to_string()))?;
        let reading = out
            .try_clone()
            .map_err(|e| ClientError::Unreachable(e.to_string()))?;
        let inbox = Arc::new(Mutex::new(Inbox {
            live: true,
            ..Inbox::default()
        }));

        // One thread doing nothing but reading frames. The alternative — the
        // frame loop reading when it remembers to — is how a client ends up a
        // screenful behind while the socket's buffer fills.
        let collecting = Arc::clone(&inbox);
        std::thread::spawn(move || collect(reading, &collecting));

        let mut client = Self {
            out,
            inbox,
            next: 1,
            device: omt_types::DeviceId::new(),
        };
        client.greet()?;
        Ok(client)
    }

    /// Say hello, which is what makes every later call allowed.
    fn greet(&mut self) -> Result<(), ClientError> {
        self.send(&ProtoMessage::Hello(omt_proto::Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "omt-terminal".to_owned(),
            token: None,
        }))
    }

    /// Ask for a session's output.
    ///
    /// # Errors
    /// Fails if the connection has gone.
    pub fn subscribe(&mut self, sessions: Vec<SessionId>) -> Result<(), ClientError> {
        self.send(&ProtoMessage::Subscribe(omt_proto::Subscribe {
            filter: omt_events::Filter {
                kinds: Vec::new(),
                sessions,
            },
            since: std::collections::BTreeMap::new(),
        }))
    }

    /// Call a capability and wait for its answer.
    ///
    /// # Errors
    /// Fails if the connection has gone, or if the instance refused.
    pub fn call(
        &mut self,
        capability: &str,
        input: serde_json::Value,
        mutating: bool,
    ) -> Result<serde_json::Value, ClientError> {
        let request = omt_catalog::RequestId {
            device: self.device,
            n: self.next,
        };
        self.next += 1;
        self.send(&ProtoMessage::Call(omt_proto::Call {
            request,
            capability: capability.to_owned(),
            input,
            // Minted per call for a mutation, so a retry after a dropped
            // acknowledgement is recognisable rather than a second execution.
            // A query needs none, and sending one anyway would make the
            // catalog's own distinction unreadable from the wire.
            intent: mutating.then(omt_types::IntentId::new),
        }))?;

        // Bounded: an instance that has stopped answering is a client that
        // must find out rather than hang. The frame loop draws at 8 ms, so
        // this is generous by two orders of magnitude on a local socket.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            {
                let mut inbox = self
                    .inbox
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if !inbox.live {
                    return Err(ClientError::Gone);
                }
                if let Some(index) = inbox.replies.iter().position(|r| r.request == request) {
                    let reply = inbox.replies.remove(index);
                    return match reply.outcome {
                        omt_proto::CallOutcome::Ok { output } => Ok(output),
                        omt_proto::CallOutcome::Err { error } => {
                            Err(ClientError::Refused(error.message))
                        }
                    };
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        Err(ClientError::Refused(format!("{capability} never answered")))
    }

    /// Take whatever output has arrived for a stream.
    ///
    /// Drained rather than peeked: the caller feeds it to an emulator, and
    /// bytes fed twice are a screen painted twice.
    pub fn take_output(&mut self, stream: u16) -> Vec<u8> {
        let mut inbox = self
            .inbox
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inbox.output.remove(&stream).unwrap_or_default()
    }

    /// Whether the instance is still there.
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.inbox
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .live
    }

    fn send(&mut self, message: &ProtoMessage) -> Result<(), ClientError> {
        let bytes = serde_json::to_vec(message).map_err(|e| ClientError::Refused(e.to_string()))?;
        omt_transport::write_frame(&mut self.out, FrameKind::Text, &bytes)
            .map_err(|_| ClientError::Gone)?;
        self.out.flush().map_err(|_| ClientError::Gone)
    }
}

/// Read frames until the connection ends.
fn collect(mut stream: std::os::unix::net::UnixStream, inbox: &Arc<Mutex<Inbox>>) {
    loop {
        let mut prefix = [0u8; 5];
        if stream.read_exact(&mut prefix).is_err() {
            break;
        }
        let len = u32::from_be_bytes([prefix[0], prefix[1], prefix[2], prefix[3]]) as usize;
        // The framing crate's own ceiling. A length that exceeds it is a
        // corrupt stream, and allocating what it asks for is the first half of
        // the denial of service it would be.
        if len > FrameKind::Binary.max_payload() {
            break;
        }
        let mut payload = vec![0u8; len];
        if stream.read_exact(&mut payload).is_err() {
            break;
        }
        let mut held = inbox
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match FrameKind::from_tag(prefix[4]) {
            Some(FrameKind::Binary) if payload.len() >= BinaryHeader::SIZE => {
                let Ok(header) = BinaryHeader::decode(&payload[..BinaryHeader::SIZE]) else {
                    continue;
                };
                held.output
                    .entry(header.stream)
                    .or_default()
                    .extend_from_slice(&payload[BinaryHeader::SIZE..]);
            }
            Some(FrameKind::Text) => {
                // Anything that is not a reply is ignored rather than
                // refused: an instance newer than this client may say things
                // it has no opinion about, and hanging up on them would make
                // every future addition a breaking change.
                if let Ok(ProtoMessage::Result(result)) =
                    serde_json::from_slice::<ProtoMessage>(&payload)
                {
                    held.replies.push(result);
                }
            }
            _ => {}
        }
    }
    inbox
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .live = false;
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    /// An instance serving on a socket in a directory of its own.
    fn serving(name: &str) -> (crate::state::State, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("omt-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tempdir");
        let socket = dir.join("omt.sock");
        let state = crate::state::State::default();
        let served = state.clone();
        let path = socket.clone();
        std::thread::spawn(move || {
            let _ = crate::serve::serve_beside(&path, served);
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while omt_transport::connect(&socket).is_err() {
            assert!(std::time::Instant::now() < deadline, "never listened");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        (state, socket)
    }

    #[test]
    fn a_call_reaches_the_instance_and_comes_back() {
        let (state, socket) = serving("client-call");
        let mut client = Client::connect(&socket).expect("connect");
        client
            .call(
                "workspace.open",
                serde_json::json!({ "root": "/tmp/from-a-client" }),
                true,
            )
            .expect("the call");
        assert_eq!(state.lock().expect("lock").workspaces().len(), 1);
    }

    #[test]
    fn a_refusal_is_an_error_rather_than_an_empty_answer() {
        // An instance that says no must not look like one that said nothing:
        // a client that could not tell them apart would retry a refusal.
        let (_state, socket) = serving("client-refusal");
        let mut client = Client::connect(&socket).expect("connect");
        let answer = client.call(
            "session.close",
            serde_json::json!({ "session": "nope" }),
            true,
        );
        assert!(matches!(answer, Err(ClientError::Refused(_))), "{answer:?}");
    }

    #[test]
    fn an_instance_that_goes_away_is_noticed() {
        // The terminal has to be able to say so, rather than drawing a screen
        // that has quietly stopped changing.
        let (_state, socket) = serving("client-gone");
        let client = Client::connect(&socket).expect("connect");
        assert!(client.is_live());
        // The reader thread notices when the socket closes under it, which is
        // what a killed instance looks like from here.
        drop(client);
    }

    #[test]
    fn output_is_taken_once() {
        // Bytes fed to an emulator twice are a screen painted twice.
        let (_state, socket) = serving("client-once");
        let mut client = Client::connect(&socket).expect("connect");
        client
            .inbox
            .lock()
            .expect("lock")
            .output
            .insert(0, b"hello".to_vec());
        assert_eq!(client.take_output(0), b"hello");
        assert!(client.take_output(0).is_empty());
    }
}
