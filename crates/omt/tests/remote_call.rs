//! A capability call over a real socket, framed on the wire.
//!
//! The claim this checks is the one the whole architecture rests on: a phone
//! and the local TUI reach the same state by the same path. So this does not
//! call a handler — it opens a socket, writes frames, and reads the reply,
//! exactly as a remote client would.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use omt_proto::{Call, CallOutcome, Hello, ProtoMessage};
use omt_server::{Peer, handle};
use omt_types::{Actor, DeviceId, Role};

fn client_server_pair() -> (
    std::os::unix::net::UnixStream,
    std::os::unix::net::UnixStream,
) {
    std::os::unix::net::UnixStream::pair().expect("socketpair")
}

fn send(stream: &mut std::os::unix::net::UnixStream, message: &ProtoMessage) {
    let bytes = serde_json::to_vec(message).expect("serialize");
    omt_transport::write_frame(stream, omt_proto::FrameKind::Text, &bytes).expect("write");
}

fn recv(stream: &mut std::os::unix::net::UnixStream) -> ProtoMessage {
    let (_, payload) = omt_transport::read_frame(stream).expect("read");
    serde_json::from_slice(&payload).expect("parse")
}

/// Serve until the client hangs up.
fn serve(mut stream: std::os::unix::net::UnixStream, state: omt::state::State) {
    let registry = omt::capabilities::registry(state).expect("registry");
    let mut peer = Peer::new(Actor::Local, Role::Operator);
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

fn request(n: u64) -> omt_catalog::RequestId {
    omt_catalog::RequestId {
        device: DeviceId::new(),
        n,
    }
}

#[test]
fn a_remote_client_opens_a_workspace_the_local_side_can_see() {
    // The whole architecture's claim, exercised rather than asserted: a call
    // arriving over a socket reaches the same instance the TUI drives.
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(welcome) = recv(&mut client) else {
        panic!("expected a welcome");
    };
    assert!(
        welcome.capabilities.contains(&"workspace.open".to_owned()),
        "the catalog was not advertised: {:?}",
        welcome.capabilities
    );

    send(
        &mut client,
        &ProtoMessage::Call(Call {
            request: request(1),
            capability: "workspace.open".to_owned(),
            input: serde_json::json!({ "root": "/tmp/remote-call-test" }),
            // Minted by the client at intent time. The catalog refuses a
            // command without one, so a retry after a dropped acknowledgement
            // is recognisable rather than a second execution.
            intent: Some(omt_types::IntentId::new()),
        }),
    );
    let ProtoMessage::Result(result) = recv(&mut client) else {
        panic!("expected a result");
    };
    let CallOutcome::Ok { output } = result.outcome else {
        panic!("{:?}", result.outcome);
    };
    assert_eq!(output["already_open"], false);

    // The local side sees it, because there is only one instance.
    let instance = state.lock().expect("lock");
    assert_eq!(instance.workspaces().len(), 1);
    assert_eq!(instance.workspaces()[0].root, "/tmp/remote-call-test");
    drop(instance);

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn opening_the_same_workspace_twice_says_it_was_already_open() {
    // Idempotent by construction — the id is derived from the path — and the
    // caller can tell, rather than being told it created something it did not.
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let worker = std::thread::spawn({
        let state = state.clone();
        move || serve(server, state)
    });

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    recv(&mut client);

    let mut outcomes = Vec::new();
    for n in 1..=2 {
        send(
            &mut client,
            &ProtoMessage::Call(Call {
                request: request(n),
                capability: "workspace.open".to_owned(),
                input: serde_json::json!({ "root": "/tmp/twice" }),
                intent: Some(omt_types::IntentId::new()),
            }),
        );
        let ProtoMessage::Result(result) = recv(&mut client) else {
            panic!("expected a result");
        };
        let CallOutcome::Ok { output } = result.outcome else {
            panic!("{:?}", result.outcome);
        };
        outcomes.push(output);
    }

    assert_eq!(outcomes[0]["already_open"], false);
    assert_eq!(outcomes[1]["already_open"], true);
    assert_eq!(
        outcomes[0]["id"], outcomes[1]["id"],
        "one workspace, not two"
    );
    assert_eq!(state.lock().expect("lock").workspaces().len(), 1);

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn a_result_carries_the_request_id_the_client_minted() {
    // What lets a client that lost an acknowledgement ask again and find out
    // what happened, rather than guessing.
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let worker = std::thread::spawn(move || serve(server, state));

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    recv(&mut client);

    let mine = request(42);
    send(
        &mut client,
        &ProtoMessage::Call(Call {
            request: mine,
            capability: "session.list".to_owned(),
            input: serde_json::json!({}),
            intent: None,
        }),
    );
    let ProtoMessage::Result(result) = recv(&mut client) else {
        panic!("expected a result");
    };
    assert_eq!(result.request, mine);

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn a_command_without_an_intent_id_is_refused() {
    // So a retry after a dropped acknowledgement is recognisable rather than a
    // second execution. Enforced by the catalog, once, for every command.
    let registry = omt::capabilities::registry(omt::state::State::default()).expect("registry");
    let mut peer = Peer::new(Actor::Local, Role::Operator);
    peer.greeted = true;

    let reply = handle(
        &registry,
        &mut peer,
        ProtoMessage::Call(Call {
            request: request(1),
            capability: "workspace.open".to_owned(),
            input: serde_json::json!({ "root": "/tmp/x" }),
            intent: None,
        }),
    )
    .expect("a result");
    let ProtoMessage::Result(result) = reply else {
        panic!("{reply:?}");
    };
    assert!(matches!(result.outcome, CallOutcome::Err { .. }));
}

#[test]
fn a_viewer_cannot_invoke_an_operator_capability() {
    // The role check lives in dispatch, once, rather than in each handler — a
    // handler that forgot it would be a hole nothing else could see.
    let registry = omt::capabilities::registry(omt::state::State::default()).expect("registry");
    let mut viewer = Peer::new(Actor::Local, Role::Viewer);
    viewer.greeted = true;

    let reply = handle(
        &registry,
        &mut viewer,
        ProtoMessage::Call(Call {
            request: request(1),
            capability: "workspace.open".to_owned(),
            input: serde_json::json!({ "root": "/tmp/x" }),
            intent: Some(omt_types::IntentId::new()),
        }),
    )
    .expect("a result");
    let ProtoMessage::Result(result) = reply else {
        panic!("{reply:?}");
    };
    assert!(
        matches!(result.outcome, CallOutcome::Err { .. }),
        "a viewer opened a workspace"
    );
}

#[test]
fn the_binary_serves_on_a_socket_a_client_can_reach() {
    // The dispatch path existed before this; nothing bound a socket, so
    // nothing outside the process could reach it. This is the difference
    // between a design and a running service.
    let dir = std::env::temp_dir().join(format!("omt-serve-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tempdir");
    let socket = dir.join("omt.sock");

    let state = omt::state::State::default();
    let served = state.clone();
    let path = socket.clone();
    std::thread::spawn(move || {
        let _ = omt::serve::serve(&path, served);
    });

    // Wait for it to bind rather than sleeping a guess.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut stream = loop {
        if let Ok(s) = omt_transport::connect(&socket) {
            break s;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "never started listening"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    };

    send(
        &mut stream,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "integration".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(welcome) = recv(&mut stream) else {
        panic!("expected a welcome");
    };
    assert!(welcome.capabilities.contains(&"session.list".to_owned()));

    send(
        &mut stream,
        &ProtoMessage::Call(Call {
            request: request(1),
            capability: "workspace.open".to_owned(),
            input: serde_json::json!({ "root": "/tmp/over-the-socket" }),
            intent: Some(omt_types::IntentId::new()),
        }),
    );
    let ProtoMessage::Result(result) = recv(&mut stream) else {
        panic!("expected a result");
    };
    assert!(matches!(result.outcome, CallOutcome::Ok { .. }));

    // And it landed in the instance this process holds.
    assert_eq!(state.lock().expect("lock").workspaces().len(), 1);

    drop(stream);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_bridge_relays_stdin_and_stdout_to_a_live_instance() {
    // What `omt ssh` runs on the far side. Without this the remote path is a
    // design: ssh would carry bytes to a process that could not answer.
    let dir = std::env::temp_dir().join(format!("omt-bridge-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tempdir");
    let socket = dir.join("omt.sock");

    let state = omt::state::State::default();
    let served = state.clone();
    let path = socket.clone();
    std::thread::spawn(move || {
        let _ = omt::serve::serve(&path, served);
    });

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while omt_transport::connect(&socket).is_err() {
        assert!(
            std::time::Instant::now() < deadline,
            "never started listening"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    // Exactly what ssh would run on the far side.
    let exe = std::env::current_exe()
        .expect("test exe")
        .parent()
        .and_then(std::path::Path::parent)
        .expect("target dir")
        .join("omt");
    let mut child = std::process::Command::new(exe)
        .args(["bridge", "--socket", socket.to_str().expect("utf8")])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn bridge");

    let mut to = child.stdin.take().expect("stdin");
    let mut from = child.stdout.take().expect("stdout");

    let hello = ProtoMessage::Hello(Hello {
        proto: omt_proto::PROTO_VERSION,
        client: "bridged".to_owned(),
        token: None,
    });
    let bytes = serde_json::to_vec(&hello).expect("encode");
    omt_transport::write_frame(&mut to, omt_proto::FrameKind::Text, &bytes).expect("write");
    use std::io::Write as _;
    to.flush().expect("flush");

    let (_, payload) = omt_transport::read_frame(&mut from).expect("read");
    let reply: ProtoMessage = serde_json::from_slice(&payload).expect("decode");
    assert!(
        matches!(reply, ProtoMessage::Welcome(_)),
        "the bridge did not relay a welcome back: {reply:?}"
    );

    drop(to);
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);
}
