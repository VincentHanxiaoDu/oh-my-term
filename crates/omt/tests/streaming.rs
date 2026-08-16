//! A session's output, pushed to a subscriber over a real socket.
//!
//! The thing that makes a session able to outlive its terminal: the terminal
//! stops owning the pty and starts being told about it. Everything here is over
//! the wire — a socket, frames, a client emulator — because the failure this
//! guards against is precisely the one a unit test of either half cannot see.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "in a test, expect() is the assertion"
)]

use std::io::Read as _;
use std::time::{Duration, Instant};

use omt_proto::{BinaryHeader, FrameKind, ProtoMessage};

/// Somewhere for this test's socket.
fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("omt-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tempdir");
    dir
}

/// An instance with a session running a script, served on a socket.
fn serving(
    dir: &std::path::Path,
    script: &str,
) -> (omt::state::State, omt_types::SessionId, std::path::PathBuf) {
    let mut instance = omt_daemon::Instance::new();
    let workspace = instance.open_workspace("/w").expect("workspace");
    let session = instance
        .create_session(
            workspace,
            omt_session::SessionKind::Shell,
            omt_session::SessionMode::Pty,
        )
        .expect("session");
    let runtime = omt_daemon::SessionRuntime::spawn(
        session,
        &omt_pty::PtyConfig {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.to_owned()],
            size: omt_pty::PtySize::new(40, 6),
            ..omt_pty::PtyConfig::default()
        },
        omt_term::ScrollbackLimits::default(),
    )
    .expect("spawn");
    instance.attach(runtime).expect("attach");

    let state = omt::state::State::new(instance);
    let socket = dir.join("omt.sock");

    // The pump is the daemon's, not the test's: what is under test is that a
    // subscriber sees what the instance saw, and a test that pumped by hand
    // would be testing its own loop.
    let pumping = state.clone();
    let id = session;
    std::thread::spawn(move || {
        loop {
            if let Ok(mut instance) = pumping.lock() {
                let _ = instance.pump_session(id);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });

    let served = state.clone();
    let path = socket.clone();
    std::thread::spawn(move || {
        let _ = omt::serve::serve_beside(&path, served);
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while omt_transport::connect(&socket).is_err() {
        assert!(Instant::now() < deadline, "never started listening");
        std::thread::sleep(Duration::from_millis(20));
    }
    (state, session, socket)
}

/// Read frames until one of them makes `done` true, or give up.
fn read_until(
    stream: &mut std::os::unix::net::UnixStream,
    seconds: u64,
    mut done: impl FnMut(&str) -> bool,
) -> String {
    stream
        .set_read_timeout(Some(Duration::from_millis(250)))
        .expect("timeout");
    let mut seen = String::new();
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        let mut prefix = [0u8; 5];
        if stream.read_exact(&mut prefix).is_err() {
            continue;
        }
        let len = u32::from_be_bytes([prefix[0], prefix[1], prefix[2], prefix[3]]) as usize;
        let mut payload = vec![0u8; len];
        if stream.read_exact(&mut payload).is_err() {
            continue;
        }
        if prefix[4] != FrameKind::Binary as u8 {
            continue;
        }
        let header = BinaryHeader::decode(&payload[..BinaryHeader::SIZE]).expect("a header");
        assert_eq!(header.ver, BinaryHeader::VERSION);
        seen.push_str(&String::from_utf8_lossy(&payload[BinaryHeader::SIZE..]));
        if done(&seen) {
            return seen;
        }
    }
    seen
}

fn subscribe(
    socket: &std::path::Path,
    session: omt_types::SessionId,
) -> std::os::unix::net::UnixStream {
    let mut stream = omt_transport::connect(socket).expect("connect");
    let message = ProtoMessage::Subscribe(omt_proto::Subscribe {
        filter: omt_events::Filter {
            kinds: Vec::new(),
            sessions: vec![session],
        },
        since: std::collections::BTreeMap::new(),
    });
    let bytes = serde_json::to_vec(&message).expect("encode");
    omt_transport::write_frame(&mut stream, FrameKind::Text, &bytes).expect("subscribe");
    stream
}

#[test]
fn a_subscriber_is_shown_the_screen_it_arrived_to() {
    // What re-attaching is: not a history, not a replay — the screen as it is
    // now, rendered back into the bytes that would produce it.
    let dir = temp_dir("stream-seed");
    let (_state, session, socket) = serving(&dir, "printf 'already here'; sleep 30");
    // Long enough for the shell to have written before anybody subscribes.
    std::thread::sleep(Duration::from_millis(600));

    let mut stream = subscribe(&socket, session);
    let seen = read_until(&mut stream, 10, |s| s.contains("already here"));
    assert!(
        seen.contains("already here"),
        "the seed did not carry the screen: {seen:?}"
    );
    assert!(
        seen.contains("\u{1b}[2J"),
        "it was raw output rather than a seed: {seen:?}"
    );

    drop(stream);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn output_written_after_subscribing_arrives_without_asking() {
    // The half a polling client cannot have: nobody asked for this.
    let dir = temp_dir("stream-live");
    let (_state, session, socket) = serving(
        &dir,
        "printf 'first'; sleep 1; printf 'and then more'; sleep 30",
    );
    let mut stream = subscribe(&socket, session);
    let seen = read_until(&mut stream, 15, |s| s.contains("and then more"));
    assert!(
        seen.contains("and then more"),
        "live output never arrived: {seen:?}"
    );

    drop(stream);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_subscriber_can_still_make_calls_and_get_whole_replies() {
    // The hazard of a connection that speaks unprompted: a reply written into
    // the middle of a pushed frame corrupts both, and the client reads a
    // length prefix that is not one.
    let dir = temp_dir("stream-duplex");
    let (_state, session, socket) = serving(&dir, "while :; do printf 'x'; sleep 0.02; done");
    let mut stream = subscribe(&socket, session);

    // Greet, then ask something, while output is streaming the whole time.
    for message in [
        ProtoMessage::Hello(omt_proto::Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "streaming-test".to_owned(),
            token: None,
        }),
        ProtoMessage::Call(omt_proto::Call {
            request: omt_catalog::RequestId {
                device: omt_types::DeviceId::new(),
                n: 1,
            },
            capability: "session.list".to_owned(),
            input: serde_json::json!({}),
            intent: Some(omt_types::IntentId::new()),
        }),
    ] {
        let bytes = serde_json::to_vec(&message).expect("encode");
        omt_transport::write_frame(&mut stream, FrameKind::Text, &bytes).expect("send");
    }

    stream
        .set_read_timeout(Some(Duration::from_millis(250)))
        .expect("timeout");
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut result = None;
    while Instant::now() < deadline && result.is_none() {
        let mut prefix = [0u8; 5];
        if stream.read_exact(&mut prefix).is_err() {
            continue;
        }
        let len = u32::from_be_bytes([prefix[0], prefix[1], prefix[2], prefix[3]]) as usize;
        let mut payload = vec![0u8; len];
        if stream.read_exact(&mut payload).is_err() {
            continue;
        }
        if prefix[4] == FrameKind::Text as u8 {
            // Every text frame must parse. One that does not is the failure
            // this test exists for: a reply cut in half by a pushed frame.
            let parsed: ProtoMessage =
                serde_json::from_slice(&payload).expect("a whole control frame");
            if let ProtoMessage::Result(r) = parsed {
                result = Some(r);
            }
        }
    }
    let result = result.expect("no reply came back while output was streaming");
    assert!(matches!(result.outcome, omt_proto::CallOutcome::Ok { .. }));

    drop(stream);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_subscriber_going_away_takes_nothing_with_it() {
    // One client's socket closing must not stop the instance, the pump, or
    // anybody else's stream.
    let dir = temp_dir("stream-hangup");
    let (state, session, socket) = serving(&dir, "sleep 30");
    let first = subscribe(&socket, session);
    drop(first);

    let mut second = subscribe(&socket, session);
    let seen = read_until(&mut second, 10, |s| s.contains("\u{1b}[2J"));
    assert!(
        seen.contains("\u{1b}[2J"),
        "a second subscriber got nothing after the first left: {seen:?}"
    );
    assert!(
        state.lock().expect("lock").session(session).is_some(),
        "the session went with the client"
    );

    drop(second);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_client_emulator_ends_up_showing_what_the_session_shows() {
    // The whole point, end to end: the instance owns the pty, the client owns
    // an emulator, and what the client draws is what the session's screen says.
    // Nothing above this line in the terminal — the renderer, the layout, copy
    // mode — can tell the difference, which is the property that lets the
    // sessions outlive the terminal.
    let dir = temp_dir("stream-mirror");
    let (state, session, socket) = serving(
        &dir,
        "printf 'before attach\\r\\n'; sleep 1; printf 'after attach'; sleep 30",
    );
    std::thread::sleep(Duration::from_millis(500));

    let mut client = omt::client::Client::connect(&socket).expect("connect");
    client.subscribe(vec![session]).expect("subscribe");

    let mut mirror = omt_term::Terminal::new(omt_term::TermConfig {
        size: omt_term::GridSize::new(40, 6),
        ..omt_term::TermConfig::default()
    });

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut mine = String::new();
    while Instant::now() < deadline {
        let bytes = client.take_output(0);
        if !bytes.is_empty() {
            mirror.advance(&bytes);
        }
        mine = (0..6)
            .map(|r| mirror.row_text(r))
            .collect::<Vec<_>>()
            .join("\n");
        if mine.contains("before attach") && mine.contains("after attach") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let theirs = {
        let instance = state.lock().expect("lock");
        let runtime = instance.runtime(session).expect("runtime");
        (0..6)
            .map(|r| runtime.terminal().row_text(r))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert!(
        mine.contains("before attach"),
        "what happened before attaching never arrived:\n{mine}"
    );
    assert!(
        mine.contains("after attach"),
        "what happened after attaching never arrived:\n{mine}"
    );
    assert_eq!(
        mine.trim_end(),
        theirs.trim_end(),
        "the client's screen is not the session's"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_session_created_through_an_instance_outlives_the_client_that_made_it() {
    // The promise a multiplexer exists to make, at the level below the
    // terminal: the client that created the session goes away entirely, and
    // the session — and the process in it — does not notice.
    let dir = temp_dir("stream-outlive");
    let mut instance = omt_daemon::Instance::new();
    // A directory that exists: a session is spawned in its workspace's root,
    // and a chdir that fails leaves a session that was created and is already
    // dead.
    let workspace = instance
        .open_workspace(&dir.display().to_string())
        .expect("workspace");
    let state = omt::state::State::new(instance);
    let socket = dir.join("omt.sock");

    let pumping = state.clone();
    std::thread::spawn(move || {
        loop {
            if let Ok(mut instance) = pumping.lock() {
                let _ = instance.pump_all();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    let served = state.clone();
    let path = socket.clone();
    std::thread::spawn(move || {
        let _ = omt::serve::serve_beside(&path, served);
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while omt_transport::connect(&socket).is_err() {
        assert!(Instant::now() < deadline, "never started listening");
        std::thread::sleep(Duration::from_millis(20));
    }

    // A client makes a session, writes into it, and leaves.
    let session = {
        let mut client = omt::client::Client::connect(&socket).expect("connect");
        let created = client
            .call(
                "session.create",
                serde_json::json!({
                    "workspace": workspace.to_wire(),
                    "program": "/bin/sh",
                    "cols": 40,
                    "rows": 6,
                }),
                true,
            )
            .expect("create");
        let session = created["session"].as_str().expect("an id").to_owned();
        let claim = client
            .call(
                "session.acquire",
                serde_json::json!({ "session": session, "force": false }),
                true,
            )
            .expect("acquire");
        client
            .call(
                "session.write",
                serde_json::json!({
                    "session": session,
                    "text": "printf 'still here'\n",
                    "epoch": claim["epoch"].as_u64().unwrap_or(0),
                }),
                true,
            )
            .expect("write");
        session
    };
    // The client is gone. Everything below this line is somebody coming back.

    let id = omt_types::SessionId::from_wire(&session).expect("an id");
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut screen = String::new();
    while Instant::now() < deadline {
        {
            let instance = state.lock().expect("lock");
            if let Some(runtime) = instance.runtime(id) {
                screen = (0..6)
                    .map(|r| runtime.terminal().row_text(r))
                    .collect::<Vec<_>>()
                    .join("\n");
            }
        }
        if screen.contains("still here") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        screen.contains("still here"),
        "the session did not survive the client that made it:\n{screen}"
    );

    // And a new client is seeded with what it missed entirely.
    let mut returning = omt::client::Client::connect(&socket).expect("reconnect");
    returning.subscribe(vec![id]).expect("subscribe");
    let mut mirror = omt_term::Terminal::new(omt_term::TermConfig {
        size: omt_term::GridSize::new(40, 6),
        ..omt_term::TermConfig::default()
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut mine = String::new();
    while Instant::now() < deadline {
        let bytes = returning.take_output(0);
        if !bytes.is_empty() {
            mirror.advance(&bytes);
        }
        mine = (0..6)
            .map(|r| mirror.row_text(r))
            .collect::<Vec<_>>()
            .join("\n");
        if mine.contains("still here") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        mine.contains("still here"),
        "coming back showed nothing of what happened while away:\n{mine}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_subscriber_can_ask_for_history_while_output_streams() {
    // What copy mode does in an attached terminal: it asks the instance for
    // lines it never saw, on the same connection its screen is arriving on.
    let dir = temp_dir("stream-history");
    let (_state, session, socket) = serving(&dir, "seq -f older-%g 1 40; sleep 30");
    let mut client = omt::client::Client::connect(&socket).expect("connect");
    client.subscribe(vec![session]).expect("subscribe");
    std::thread::sleep(Duration::from_millis(800));

    let answer = client
        .call(
            "session.scrollback",
            serde_json::json!({ "session": session.to_wire(), "from": 0, "lines": 5 }),
            false,
        )
        .expect("the instance answered nothing");
    let lines = answer["lines"].as_array().expect("lines").len();
    assert!(lines > 0, "no history came back: {answer}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_command_that_ran_can_be_suggested_afterwards() {
    // `recall.suggest` has existed since the catalog did and has always
    // answered with nothing, because nothing recorded anything. This is the
    // whole path: a shell with shell integration runs a command, the block
    // closes, the instance writes it down, and the capability every surface
    // asks can answer.
    let dir = temp_dir("recall-round-trip");
    let mut instance = omt_daemon::Instance::new();
    let workspace = instance
        .open_workspace(&dir.display().to_string())
        .expect("workspace");
    let session = instance
        .create_session(
            workspace,
            omt_session::SessionKind::Shell,
            omt_session::SessionMode::Pty,
        )
        .expect("session");
    // The marks a shell with integration emits, without needing one installed.
    let script = "printf '\\033]133;A\\007$ \\033]133;B\\007cargo test\\033]133;C\\007'; \
                  printf 'ok\\r\\n'; printf '\\033]133;D;0\\007'; sleep 30";
    let runtime = omt_daemon::SessionRuntime::spawn(
        session,
        &omt_pty::PtyConfig {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.to_owned()],
            cwd: Some(dir.clone()),
            size: omt_pty::PtySize::new(60, 8),
            ..omt_pty::PtyConfig::default()
        },
        omt_term::ScrollbackLimits::default(),
    )
    .expect("spawn");
    instance.attach(runtime).expect("attach");

    let state = omt::state::State::new(instance);
    let mut remembered = omt::remember::Remembered::default();

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut suggested: Vec<String> = Vec::new();
    while Instant::now() < deadline {
        {
            let mut instance = state.lock().expect("lock");
            let _ = instance.pump_session(session);
            if let (Ok(mut history), Some(runtime)) = (state.recall(), instance.runtime(session)) {
                remembered.absorb(&mut history, session, workspace, runtime.terminal());
            }
        }
        suggested = state
            .recall()
            .expect("recall")
            .suggest("", workspace, 10)
            .into_iter()
            .map(|s| s.command)
            .collect();
        if !suggested.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        suggested.iter().any(|c| c == "cargo test"),
        "what ran was never written down: {suggested:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
