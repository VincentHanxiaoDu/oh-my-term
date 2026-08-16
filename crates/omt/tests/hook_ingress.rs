//! The hook path, end to end, over a real socket.
//!
//! Every piece of this existed and none of it was connected: the hook binary
//! sent, the adapters normalized, the transcripts recorded — and no server
//! read a `HookEvent` off a socket, so an agent asking a question inside omt
//! reached nothing. A unit test of either half would still have passed.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use std::io::Write as _;
use std::process::{Command, Stdio};

use omt_proto::{FrameKind, ProtoMessage};
use omt_session::{SessionKind, SessionMode};
use omt_types::AgentKind;

/// Where the test's socket lives, one directory per test.
fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("omt-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tempdir");
    dir
}

/// Start serving and wait for the socket to answer, rather than sleeping a
/// guess.
fn serving(socket: &std::path::Path, state: omt::state::State) {
    let path = socket.to_path_buf();
    std::thread::spawn(move || {
        let _ = omt::serve::serve_beside(&path, state);
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while omt_transport::connect(socket).is_err() {
        assert!(
            std::time::Instant::now() < deadline,
            "never started listening"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn session_in(state: &omt::state::State) -> omt_types::SessionId {
    let mut instance = state.lock().expect("lock");
    let workspace = instance.open_workspace("/w").expect("workspace");
    instance
        .create_session(
            workspace,
            SessionKind::Agent {
                kind: AgentKind::ClaudeCode,
            },
            SessionMode::Pty,
        )
        .expect("session")
}

/// What Claude Code's own hook writes to the binary's stdin.
fn ask_payload() -> String {
    serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_use_id": "t1",
        "tool_name": "AskUserQuestion",
        "tool_input": {
            "questions": [{
                "question": "Which database?",
                "header": "Database",
                "multiSelect": false,
                "options": [
                    {"label": "Postgres", "description": "the safe one"},
                    {"label": "SQLite", "description": "the small one"}
                ]
            }]
        }
    })
    .to_string()
}

#[test]
fn a_question_an_agent_asks_reaches_the_instance_through_the_hook_binary() {
    // The whole flagship path: the agent's own hook system runs `omt-hook`,
    // which reports before the card is drawn, and the question is mirrored to
    // every surface while the agent's local card renders unchanged.
    let dir = temp_dir("hook-ingress");
    let socket = dir.join("omt.sock");
    let state = omt::state::State::default();
    let session = session_in(&state);
    serving(&socket, state.clone());

    // target/debug/deps/<test> → target/debug/omt-hook. Built by the same
    // workspace build that built this test.
    let hook = std::env::current_exe()
        .expect("test exe")
        .parent()
        .and_then(std::path::Path::parent)
        .expect("target dir")
        .join("omt-hook");
    assert!(hook.exists(), "{} was not built", hook.display());
    let mut child = Command::new(&hook)
        .args(["--agent", "claude-code"])
        .env("OMT_SOCK", &socket)
        .env("OMT_SESSION", session.to_wire())
        .env("OMT_HOOK_EVENT", "PreToolUse")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("running the hook binary");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(ask_payload().as_bytes())
        .expect("writing the payload");
    let out = child.wait_with_output().expect("the hook exited");

    // The agent must be told to carry on no matter what happened here.
    assert!(out.status.success(), "the hook failed its own agent");

    let transcripts = state.transcripts().expect("transcripts");
    let recorded = transcripts.get(session).expect("a transcript");
    assert!(
        !recorded.entries().is_empty(),
        "the hook ran and the instance recorded nothing"
    );

    drop(transcripts);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_hook_reporting_for_an_unknown_session_changes_nothing() {
    // Two omt instances share a machine and each has a socket. Believing a
    // session id this one has never seen would put another instance's agent in
    // this one's conversation, and nothing afterwards could detect it.
    let dir = temp_dir("hook-stranger");
    let socket = dir.join("omt.sock");
    let state = omt::state::State::default();
    let mine = session_in(&state);
    serving(&socket, state.clone());

    let stranger = omt_types::SessionId::new();
    let mut stream = omt_transport::connect(&socket).expect("connect");
    let event = ProtoMessage::HookEvent(Box::new(omt_proto::HookEvent {
        nonce: 3,
        hook_proto: 1,
        agent: AgentKind::ClaudeCode,
        agent_version: None,
        agent_session: None,
        transcript_path: None,
        event: "PreToolUse".to_owned(),
        tool_use_id: Some("t1".to_owned()),
        tool_name: Some("AskUserQuestion".to_owned()),
        tool_input: Some(serde_json::from_str(&ask_payload()).expect("payload")),
        raw: None,
        truncated: None,
        correlation: omt_proto::HookCorrelation {
            instance: None,
            session: Some(stranger),
            pid: 1,
            ppid: 2,
            cwd: "/w".to_owned(),
        },
        deadline_ms: 400,
    }));
    let bytes = serde_json::to_vec(&event).expect("encode");
    omt_transport::write_frame(&mut stream, FrameKind::Text, &bytes).expect("send");
    let (_, payload) = omt_transport::read_frame(&mut stream).expect("an ack");
    let reply: ProtoMessage = serde_json::from_slice(&payload).expect("decode");
    let ProtoMessage::HookAck(ack) = reply else {
        panic!("expected an ack");
    };
    // Answered anyway: the agent on the other end is waiting, and refusing to
    // reply would cost it its deadline for omt's benefit.
    assert_eq!(ack.directive, omt_proto::HookDirective::Proceed);

    let transcripts = state.transcripts().expect("transcripts");
    assert!(transcripts.get(stranger).is_none());
    assert!(transcripts.get(mine).is_none());

    drop(transcripts);
    let _ = std::fs::remove_dir_all(&dir);
}
