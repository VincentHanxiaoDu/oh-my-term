//! What both halves of the remote-call suite need.
//!
//! A `mod common` rather than a duplicated prelude: the two files are one
//! suite split for P1's file-length limit, and two copies of a socket pair
//! would drift the moment one of them was fixed.

#![allow(
    dead_code,
    reason = "each half uses a subset, and Cargo compiles this module into both"
)]
#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use omt_proto::{Call, CallOutcome, ProtoMessage};
use omt_server::{Peer, handle};
use omt_types::{Actor, DeviceId, Role};

pub fn client_server_pair() -> (
    std::os::unix::net::UnixStream,
    std::os::unix::net::UnixStream,
) {
    std::os::unix::net::UnixStream::pair().expect("socketpair")
}

pub fn send(stream: &mut std::os::unix::net::UnixStream, message: &ProtoMessage) {
    let bytes = serde_json::to_vec(message).expect("serialize");
    omt_transport::write_frame(stream, omt_proto::FrameKind::Text, &bytes).expect("write");
}

pub fn recv(stream: &mut std::os::unix::net::UnixStream) -> ProtoMessage {
    let (_, payload) = omt_transport::read_frame(stream).expect("read");
    serde_json::from_slice(&payload).expect("parse")
}

/// Serve until the client hangs up.
pub fn serve(stream: std::os::unix::net::UnixStream, state: omt::state::State) {
    serve_as(stream_role(), stream, state);
}

/// Serve as a particular role.
pub fn serve_as(role: Role, mut stream: std::os::unix::net::UnixStream, state: omt::state::State) {
    let registry = omt::capabilities::registry(state).expect("registry");
    let mut peer = Peer::new(Actor::Local, role);
    loop {
        let Ok((_, payload)) = omt_transport::read_frame(&mut stream) else {
            return;
        };
        let Ok(message) = serde_json::from_slice::<ProtoMessage>(&payload) else {
            return;
        };
        if let Some(reply) = handle(&registry, &mut peer, message) {
            let bytes = serde_json::to_vec(&reply).expect("serialize");
            if omt_transport::write_frame(&mut stream, omt_proto::FrameKind::Text, &bytes).is_err()
            {
                return;
            }
        }
    }
}

pub fn request(n: u64) -> omt_catalog::RequestId {
    omt_catalog::RequestId {
        device: DeviceId::new(),
        n,
    }
}

/// Call a capability and unwrap what it produced.
pub fn call_ok(
    client: &mut std::os::unix::net::UnixStream,
    n: u64,
    capability: &str,
    input: serde_json::Value,
    command: bool,
) -> serde_json::Value {
    send(
        client,
        &ProtoMessage::Call(Call {
            request: request(n),
            capability: capability.to_owned(),
            input,
            intent: command.then(omt_types::IntentId::new),
        }),
    );
    let ProtoMessage::Result(result) = recv(client) else {
        panic!("expected a result for {capability}");
    };
    match result.outcome {
        CallOutcome::Ok { output } => output,
        CallOutcome::Err { error } => panic!("{capability} failed: {error:?}"),
    }
}

/// Base64, matching what the server expects.
pub fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for group in bytes.chunks(3) {
        let b = [
            group[0],
            group.get(1).copied().unwrap_or(0),
            group.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= group.len() {
                out.push(A[((n >> (18 - i * 6)) & 0x3f) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn decode64(text: &str) -> Vec<u8> {
    let value = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            _ => 63,
        }
    };
    let body: Vec<u8> = text.bytes().take_while(|c| *c != b'=').collect();
    let mut out = Vec::new();
    for group in body.chunks(4) {
        let mut n = 0u32;
        for (i, c) in group.iter().enumerate() {
            n |= value(*c) << (18 - i * 6);
        }
        let bytes = match group.len() {
            2 => 1,
            3 => 2,
            _ => 3,
        };
        for i in 0..bytes {
            out.push(((n >> (16 - i * 8)) & 0xff) as u8);
        }
    }
    out
}

/// Put a card in the ledger, as an adapter would.
pub fn raise_card(
    state: &omt::state::State,
    deliverable: omt_events::Deliverable,
) -> omt_types::InteractionId {
    let id = omt_types::InteractionId::new();
    let interaction = omt_events::Interaction {
        id,
        session: omt_types::SessionId::new(),
        binding: omt_types::BindingId::new(),
        kind: omt_events::InteractionKind::Permission {
            tool: "Bash".to_owned(),
            input: serde_json::json!({ "command": "rm -rf /srv/data" }),
            command: Some("rm -rf /srv/data".to_owned()),
            options: vec![
                omt_events::PermissionOption {
                    id: "allow".to_owned(),
                    label: "Yes".to_owned(),
                    kind: omt_events::PermissionOptionKind::AllowOnce,
                },
                omt_events::PermissionOption {
                    id: "deny".to_owned(),
                    label: "No".to_owned(),
                    kind: omt_events::PermissionOptionKind::DenyOnce,
                },
            ],
        },
        deliverable,
        state: omt_events::InteractionState::Open,
        opened_at: omt_types::Timestamp::now(),
        expires_at: None,
    };
    state.lock().expect("lock").ledger.open(interaction, None);
    id
}

/// A repository with one commit, which is the least git will work with.
pub fn git_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .output()
            .expect("git");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "test"]);
    std::fs::write(dir.path().join("a.txt"), "one\ntwo\nthree\n").expect("write");
    git(&["add", "."]);
    git(&["commit", "-qm", "first"]);
    dir
}

/// The role most of these tests want: everything a person at the terminal does.
pub const fn stream_role() -> Role {
    Role::Operator
}

pub mod screen_harness;
