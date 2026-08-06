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

/// Call a capability and unwrap what it produced.
fn call_ok(
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

#[test]
fn a_remote_client_starts_a_shell_types_into_it_and_sees_the_output() {
    // The end-to-end claim, and the one that was false: every step below
    // existed except that a browser could not reach it. Opening a workspace it
    // could do; starting a session, taking the writer token, and seeing a byte
    // come back it could not. This test is the reason all three exist.
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
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };

    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_string_lossy().into_owned();
    let workspace = call_ok(
        &mut client,
        1,
        "workspace.open",
        serde_json::json!({ "root": root }),
        true,
    );

    let session = call_ok(
        &mut client,
        2,
        "session.create",
        serde_json::json!({
            "workspace": workspace["id"],
            "program": "/bin/sh",
            "cols": 40,
            "rows": 10,
        }),
        true,
    );
    let id = session["session"].as_str().expect("session id").to_owned();

    // Without this the write is refused outright: the token is what stops input
    // already in flight from landing in somebody else's command line, and until
    // there was a capability for it no remote client could ever type.
    let claim = call_ok(
        &mut client,
        3,
        "session.acquire",
        serde_json::json!({ "session": id, "force": false }),
        true,
    );
    let epoch = claim["epoch"].as_u64().expect("epoch");

    call_ok(
        &mut client,
        4,
        "session.write",
        serde_json::json!({ "session": id, "text": "echo hello-from-remote\n", "epoch": epoch }),
        true,
    );

    // Somebody has to move the bytes. The TUI pumps inside its render loop, so
    // a session it owns advances on its own; one created over a socket does not
    // until the surface serving it pumps too.
    let mut screen = String::new();
    for _ in 0..100 {
        {
            let mut instance = state.lock().expect("lock");
            let ids: Vec<_> = instance.sessions().iter().map(|s| s.id).collect();
            for each in ids {
                let _ = instance.pump_session(each);
            }
        }
        let snapshot = call_ok(
            &mut client,
            5,
            "session.snapshot",
            serde_json::json!({ "session": id }),
            false,
        );
        screen = snapshot["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .map(|row| {
                row.as_array()
                    .expect("runs")
                    .iter()
                    .filter_map(|r| r["text"].as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        if screen.contains("hello-from-remote\n") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        screen.contains("hello-from-remote"),
        "the shell's output never reached the client:\n{screen}"
    );

    call_ok(
        &mut client,
        6,
        "session.close",
        serde_json::json!({ "session": id }),
        true,
    );

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn a_file_goes_up_and_comes_back_down_over_the_socket() {
    // Dragging a file onto a remote session is a feature omt claimed and could
    // not do: omt-media had the chunking, the progress and the resume, and no
    // capability reached any of it.
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
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };

    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = call_ok(
        &mut client,
        1,
        "workspace.open",
        serde_json::json!({ "root": dir.path().to_string_lossy() }),
        true,
    );
    let id = workspace["id"].clone();

    // Deliberately not text: the bug this catches is a base64 tail handled
    // wrong, and on text the corrupted byte is still legible.
    let payload: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
    let encoded = {
        use std::fmt::Write as _;
        let mut hex = String::new();
        for b in &payload {
            write!(hex, "{b:02x}").expect("write");
        }
        hex
    };

    let written = call_ok(
        &mut client,
        2,
        "fs.write",
        serde_json::json!({
            "workspace": id,
            "path": "sub/dir/blob.bin",
            "data": base64(&payload),
            "chunk": 0,
            "chunks": 1,
        }),
        true,
    );
    assert_eq!(written["complete"], true);

    // On disk, under the name asked for, with directories created along the
    // way — a transfer that lands only at the workspace root is not a transfer.
    let landed = std::fs::read(dir.path().join("sub/dir/blob.bin")).expect("the file landed");
    assert_eq!(landed, payload, "the bytes changed in flight");

    let read = call_ok(
        &mut client,
        3,
        "fs.read",
        serde_json::json!({ "workspace": id, "path": "sub/dir/blob.bin", "chunk": 0 }),
        false,
    );
    assert_eq!(read["total_bytes"], 1000);
    assert_eq!(read["chunks"], 1);

    let back = decode64(read["data"].as_str().expect("data"));
    let back_hex = {
        use std::fmt::Write as _;
        let mut hex = String::new();
        for b in &back {
            write!(hex, "{b:02x}").expect("write");
        }
        hex
    };
    assert_eq!(back_hex, encoded, "what came back is not what went up");

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn a_path_that_climbs_out_of_the_workspace_is_refused() {
    // The whole reason paths are workspace-relative. A client that can write
    // `../../.ssh/authorized_keys` has turned a file transfer into a shell.
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
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };

    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = call_ok(
        &mut client,
        1,
        "workspace.open",
        serde_json::json!({ "root": dir.path().to_string_lossy() }),
        true,
    );

    send(
        &mut client,
        &ProtoMessage::Call(Call {
            request: request(2),
            capability: "fs.write".to_owned(),
            input: serde_json::json!({
                "workspace": workspace["id"],
                "path": "../escaped.txt",
                "data": base64(b"nope"),
                "chunk": 0,
                "chunks": 1,
            }),
            intent: Some(omt_types::IntentId::new()),
        }),
    );
    let ProtoMessage::Result(result) = recv(&mut client) else {
        panic!("expected a result");
    };
    assert!(
        matches!(result.outcome, CallOutcome::Err { .. }),
        "the escape was allowed"
    );
    assert!(
        !dir.path().parent().expect("parent").join("escaped.txt").exists(),
        "a file landed outside the workspace"
    );

    drop(client);
    worker.join().expect("worker");
}

/// Base64, matching what the server expects.
fn base64(bytes: &[u8]) -> String {
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

fn decode64(text: &str) -> Vec<u8> {
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
fn raise_card(
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

#[test]
fn a_remote_client_answers_a_card_and_the_second_answer_is_refused() {
    // The product's whole promise: something needs you, and you answer it from
    // wherever you are. Exactly once, because two people looking at the same
    // notification is the normal case rather than the exotic one.
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
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };

    let id = raise_card(&state, omt_events::Deliverable::Native);

    let listed = call_ok(&mut client, 1, "interaction.list", serde_json::json!({}), false);
    let cards = listed["interactions"].as_array().expect("interactions");
    assert_eq!(cards.len(), 1);
    // The command, not the tool name: "Bash" is not enough to decide on.
    assert_eq!(cards[0]["prompt"], "rm -rf /srv/data");
    assert_eq!(cards[0]["deliverable"], "native");
    assert_eq!(
        cards[0]["options"],
        serde_json::json!(["Yes", "No"]),
        "the options must be the agent's own, in its order"
    );

    let answered = call_ok(
        &mut client,
        2,
        "interaction.respond",
        serde_json::json!({ "interaction": id.to_wire(), "option": "No" }),
        true,
    );
    // Resolving, not resolved: omt has claimed the right to answer. For a
    // synthetic responder the far side is a UI omt does not own.
    assert_eq!(answered["state"], "resolving");

    send(
        &mut client,
        &ProtoMessage::Call(Call {
            request: request(3),
            capability: "interaction.respond".to_owned(),
            input: serde_json::json!({ "interaction": id.to_wire(), "option": "Yes" }),
            intent: Some(omt_types::IntentId::new()),
        }),
    );
    let ProtoMessage::Result(result) = recv(&mut client) else {
        panic!("expected a result");
    };
    assert!(
        matches!(result.outcome, CallOutcome::Err { .. }),
        "a second answer was accepted, so the agent would receive two"
    );

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn a_card_omt_cannot_deliver_is_refused_even_while_it_is_open() {
    // Answerability is a property of the deliverable, never of the state. A
    // surface that reads `open` and offers a button lets somebody believe they
    // answered something that was never sent.
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
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };

    let id = raise_card(
        &state,
        omt_events::Deliverable::None {
            reason: omt_events::NotDeliverableReason::NoResponder,
        },
    );

    let listed = call_ok(&mut client, 1, "interaction.list", serde_json::json!({}), false);
    let card = &listed["interactions"][0];
    assert_eq!(card["deliverable"], "none");
    assert!(
        card["not_deliverable_because"].is_string(),
        "a surface must be able to say why, not just that it cannot"
    );

    send(
        &mut client,
        &ProtoMessage::Call(Call {
            request: request(2),
            capability: "interaction.respond".to_owned(),
            input: serde_json::json!({ "interaction": id.to_wire(), "option": "Yes" }),
            intent: Some(omt_types::IntentId::new()),
        }),
    );
    let ProtoMessage::Result(result) = recv(&mut client) else {
        panic!("expected a result");
    };
    assert!(
        matches!(result.outcome, CallOutcome::Err { .. }),
        "omt offered to deliver an answer it has no channel for"
    );

    drop(client);
    worker.join().expect("worker");
}
