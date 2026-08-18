//! Do the two surfaces say the same thing about the same agent?
//!
//! They did not. Agent observation lived in the TUI's loop, so a session
//! opened through the daemon — every session a phone ever opens — was never
//! looked at: the web client could report that a session existed while being
//! structurally unable to say what was running in it. The wording differed
//! too, separately: one surface said "blocked" and the other "waiting on you",
//! for the same fact.
//!
//! This drives the daemon the way a phone does — capability calls, no
//! terminal — and asserts it can answer the question the TUI answers.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "in a test, expect() is the assertion"
)]

use std::time::{Duration, Instant};

/// Everything in this process's environment that would fake a detection.
///
/// The suite runs inside an agent session. Without this, the probe matches the
/// harness's own environment and reports an agent for a plain shell — which is
/// how this feature stayed green while not working.
fn stripped() -> Vec<String> {
    const PREFIXES: [&str; 6] = [
        "CLAUDE",
        "ANTHROPIC",
        "AI_AGENT",
        "CODEX",
        "OPENCODE",
        "GEMINI",
    ];
    std::env::vars()
        .map(|(name, _)| name)
        .filter(|name| {
            let upper = name.to_uppercase();
            PREFIXES.iter().any(|p| upper.starts_with(p))
        })
        .collect()
}

#[test]
fn the_daemon_can_answer_what_is_running_without_a_terminal() {
    let mut instance = omt_daemon::Instance::new();
    let workspace = instance.open_workspace("/tmp").expect("workspace");
    let session = instance
        .create_session(
            workspace,
            omt_session::SessionKind::Shell,
            omt_session::SessionMode::Pty,
        )
        .expect("session");

    // A program that announces itself the way an agent does, and stays up.
    let runtime = omt_daemon::SessionRuntime::spawn(
        session,
        &omt_pty::PtyConfig {
            program: "/bin/sh".into(),
            args: vec![
                "-c".to_owned(),
                "printf '\\033]0;\\342\\240\\201 Claude Code\\007'; sleep 30".to_owned(),
            ],
            cwd: Some("/tmp".into()),
            size: omt_pty::PtySize::new(80, 24),
            env: vec![("TERM".to_owned(), "xterm-256color".to_owned())],
            env_remove: stripped(),
        },
        omt_daemon::SessionLimits::default(),
    )
    .expect("spawn");
    instance.attach(runtime).expect("attach");

    let adapters = omt_agent_adapters::builtin();
    let refs: Vec<&dyn omt_agent_adapters::AgentAdapter> = adapters.all();
    let binding = instance.binding_for(session);

    // Until it has both the identity and a state, not merely the identity:
    // the first observation of a session can find the agent before its title
    // has been parsed, and reports what it has. That is correct and
    // self-correcting, and a test that stopped at the first answer would be
    // asserting against the moment before the one it means.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let _ = instance.pump_session(session);
        let _ = instance.observe_session(session, binding, &refs, 1_000);
        if instance.observed_agent(session).is_some()
            && matches!(
                instance.agent_state(binding, 1_000),
                Some(omt_types::AgentState::Working { .. })
            )
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    // What a phone asks for, answered by the same instance a phone talks to.
    let found = instance
        .observed_agent(session)
        .expect("the daemon cannot say what is running in its own session");
    assert_eq!(found.agent, omt_types::AgentKind::ClaudeCode);

    let state = instance.agent_state(binding, 1_000).expect("a state");
    assert!(
        matches!(state, omt_types::AgentState::Working { .. }),
        "the agent said it was working and the daemon says {state:?}"
    );

    // And the words both surfaces put on a screen come from one function, so
    // a person moving between them is not asked to work out that two words
    // mean the same thing.
    assert_eq!(omt_tui::state_word(&state, true), "working");
    assert_eq!(
        omt_tui::state_word(&omt_types::AgentState::Idle, false),
        "done",
        "an agent that finished while nobody looked is not merely idle"
    );
}
