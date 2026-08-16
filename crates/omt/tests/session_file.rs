//! Reading the agent's own session file, end to end.
//!
//! The two things that are only true with a real file and a real state: a
//! conversation that happened before omt looked reaches the surfaces, and a
//! tool call omt already heard about from a hook is not shown a second time
//! when the file says it too.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use omt_session::{SessionKind, SessionMode};
use omt_types::{AgentKind, SessionId, Tier};

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("omt-session-file-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory");
    dir.join(name)
}

/// A state with one agent session, whose agent is already identified and whose
/// session file omt has been told about — which is the only way it is ever
/// told.
fn ready(path: &std::path::Path) -> (omt::state::State, SessionId) {
    let state = omt::state::State::default();
    let session = {
        let mut instance = state.lock().expect("lock");
        let workspace = instance.open_workspace("/w").expect("a workspace");
        let session = instance
            .create_session(
                workspace,
                SessionKind::Agent {
                    kind: AgentKind::ClaudeCode,
                },
                SessionMode::Pty,
            )
            .expect("a session");
        instance.observe_detection(
            session,
            omt_agent_adapters::Detection {
                agent: AgentKind::ClaudeCode,
                tier: Tier::Marker,
                evidence: "for the test".to_owned(),
                agent_session: None,
            },
        );
        instance.observe_transcript_path(session, path.to_path_buf());
        session
    };
    (state, session)
}

fn read(state: &omt::state::State, session: SessionId) -> usize {
    let mut instance = state.lock().expect("lock");
    omt::absorb::from_session_file(state, &mut instance, session)
}

fn said(state: &omt::state::State, session: SessionId) -> Vec<String> {
    let transcripts = state.transcripts().expect("transcripts");
    transcripts
        .get(session)
        .map(|t| {
            t.entries()
                .iter()
                .map(|e| format!("{e:?}"))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

#[test]
fn a_conversation_that_happened_before_omt_looked_still_reaches_it() {
    // Every other source observes the present. Attach to an agent that has
    // been working for an hour and the conversation view is empty — not
    // because nothing happened, but because omt was not there. The file was.
    let path = temp("before.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":"run the tests"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"running them"},{"type":"tool_use","id":"toolu_9","name":"Bash","input":{"command":"cargo test"}}]}}"#,
            "\n",
        ),
    )
    .expect("a file");
    let (state, session) = ready(&path);

    assert_eq!(read(&state, session), 3, "the whole conversation");
    let entries = said(&state, session);
    assert!(
        entries.iter().any(|e| e.contains("run the tests")),
        "what the person asked: {entries:?}"
    );
    assert!(
        entries.iter().any(|e| e.contains("cargo test")),
        "what the agent ran: {entries:?}"
    );
}

#[test]
fn what_the_hook_already_reported_is_not_reported_again() {
    // The hook and the file describe the same tool call. Both showing means
    // every call appears twice in the conversation, and every count of what an
    // agent did doubles.
    let path = temp("dedupe.jsonl");
    std::fs::write(&path, "").expect("a file");
    let (state, session) = ready(&path);

    // As a hook would: through the same absorption, at its own tier.
    {
        let mut instance = state.lock().expect("lock");
        let binding = instance.binding_for(session);
        let mut transcripts = state.transcripts().expect("transcripts");
        let mut seen = state.seen().expect("seen");
        let from = omt::absorb::From {
            session,
            binding,
            agent: AgentKind::ClaudeCode,
            agent_version: None,
            agent_session: None,
            tier: Tier::Hook,
            tag: omt_events::EventSourceTag::Hook,
            mode: None,
        };
        assert!(omt::absorb::payload(
            &mut instance,
            &mut transcripts,
            &mut seen,
            &from,
            omt_events::AgentPayload::ToolCall {
                call: "toolu_9".into(),
                name: "Bash".into(),
                input: serde_json::json!({"command": "cargo test"}),
            },
        ));
    }

    std::fs::write(
        &path,
        concat!(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_9","name":"Bash","input":{"command":"cargo test"}}]}}"#,
            "\n",
        ),
    )
    .expect("a write");

    assert_eq!(
        read(&state, session),
        0,
        "the file's copy of a call the hook already reported"
    );
    let calls = said(&state, session)
        .iter()
        .filter(|e| e.contains("cargo test"))
        .count();
    assert_eq!(calls, 1, "the call appears once");
}

#[test]
fn a_session_nobody_reported_a_path_for_reads_nothing() {
    // Never derived. The directory an agent writes into holds every
    // conversation ever held in that working directory, so a path worked out
    // from the cwd reads somebody else's session.
    let state = omt::state::State::default();
    let session = {
        let mut instance = state.lock().expect("lock");
        let workspace = instance.open_workspace("/w").expect("a workspace");
        instance
            .create_session(
                workspace,
                SessionKind::Agent {
                    kind: AgentKind::ClaudeCode,
                },
                SessionMode::Pty,
            )
            .expect("a session")
    };
    assert_eq!(read(&state, session), 0);
}

#[test]
fn what_the_agent_writes_next_is_picked_up_without_re_reading_the_rest() {
    let path = temp("growing.jsonl");
    std::fs::write(
        &path,
        format!(
            "{}\n",
            r#"{"type":"user","message":{"role":"user","content":"first"}}"#
        ),
    )
    .expect("a file");
    let (state, session) = ready(&path);
    assert_eq!(read(&state, session), 1);
    assert_eq!(read(&state, session), 0, "nothing new to say");

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("append");
    std::io::Write::write_all(
        &mut file,
        format!(
            "{}\n",
            r#"{"type":"user","message":{"role":"user","content":"second"}}"#
        )
        .as_bytes(),
    )
    .expect("a write");

    assert_eq!(read(&state, session), 1);
    let entries = said(&state, session);
    assert_eq!(
        entries.iter().filter(|e| e.contains("first")).count(),
        1,
        "the first message was not read twice: {entries:?}"
    );
}
