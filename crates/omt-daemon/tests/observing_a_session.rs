//! Does the daemon see what is running in a session?
//!
//! At this layer rather than through the CLI, because a failure three layers
//! up says only that something did not work. This was moved here *from* the
//! TUI so that a session opened from a phone is looked at by the same code
//! that looks at one opened in a terminal, and that claim is worth a test that
//! names the layer it holds at.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "in a test, expect() is the assertion"
)]

use std::time::{Duration, Instant};

use omt_daemon::Instance;

/// Everything in this process's environment that would fake a detection.
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
fn a_session_that_sets_an_agents_title_is_seen_as_that_agent() {
    let mut instance = Instance::new();
    let workspace = instance.open_workspace("/tmp").expect("workspace");
    let session = instance
        .create_session(
            workspace,
            omt_session::SessionKind::Shell,
            omt_session::SessionMode::Pty,
        )
        .expect("session");
    // A program that announces itself the way the agent does, then stays up.
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
        omt_term::ScrollbackLimits::default(),
    )
    .expect("spawn");
    instance.attach(runtime).expect("attach");

    let adapters = omt_agent_adapters::builtin();
    let refs: Vec<&dyn omt_agent_adapters::AgentAdapter> = adapters.all();
    let binding = instance.binding_for(session);

    // Pump until the title has been parsed, then observe.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut found = None;
    // Keep pumping until the *title* has been seen, not merely until
    // something was detected: a detection from any other source is what this
    // test exists to distrust.
    while Instant::now() < deadline {
        let _ = instance.pump_session(session);
        found = instance.observe_session(session, binding, &refs, 1_000);
        if instance
            .runtime(session)
            .and_then(|rt| rt.terminal().title().map(str::to_owned))
            .is_some()
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let found = found.expect("the daemon did not see the agent its session announced");
    eprintln!("detection: {found:?}");
    if let Some(rt) = instance.runtime(session) {
        eprintln!("title: {:?}", rt.terminal().title());
    }
    assert_eq!(found.agent, omt_types::AgentKind::ClaudeCode);

    // And the state the title reported, which is the point of reading it.
    let state = instance.agent_state(binding, 1_000).expect("a state");
    assert!(
        matches!(state, omt_types::AgentState::Working { .. }),
        "the title said it was working and the daemon says {state:?}"
    );

    // The same answer a capability would give, from the same binding.
    assert_eq!(
        instance.observed_agent(session).map(|d| d.agent),
        Some(omt_types::AgentKind::ClaudeCode)
    );
}
