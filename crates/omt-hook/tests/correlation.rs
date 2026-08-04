//! The correlation path, end to end through a string.
//!
//! omt injects the session id into the agent's environment; the hook reads that
//! string back and puts it in the report. If the two ends disagree about the
//! spelling, every observation is unattributed and the flagship path silently
//! degrades to guessing — with no error anywhere.

#![allow(clippy::expect_used, reason = "in a test, expect() is the assertion")]

use omt_agent_adapters::{AgentAdapter, ClaudeCode, SpawnCtx};
use omt_hook::{HookEnv, build_event};
use omt_types::{AgentKind, InstanceId, SessionId};

#[test]
fn an_injected_session_id_survives_the_round_trip_into_a_report() {
    let session = SessionId::new();
    let instance = InstanceId::new();

    // Exactly what the adapter injects when omt spawns the agent.
    let injected = ClaudeCode.spawn_env(&SpawnCtx {
        instance: instance.to_wire(),
        session: session.to_wire(),
        socket: "/tmp/omt.sock".to_owned(),
        ..SpawnCtx::default()
    });
    let value = |key: &str| {
        injected
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.to_string_lossy().into_owned())
            .expect("injected")
    };

    // Exactly what the hook reads back out of it.
    let env = HookEnv {
        socket: Some(value("OMT_SOCK")),
        session: Some(value("OMT_SESSION")),
        instance: Some(value("OMT_INSTANCE")),
        agent: AgentKind::ClaudeCode,
    };
    let event = build_event(&env, None);

    assert_eq!(
        event.correlation.session,
        Some(session),
        "the session did not survive being written to an env var and read back"
    );
    assert_eq!(event.correlation.instance, Some(instance));
    assert!(
        event.correlation.is_direct(),
        "and it is a direct correlation, not an inferred one"
    );
}

#[test]
fn an_abbreviated_id_does_not_correlate_to_something_wrong() {
    // The bug this guards: Display is lossy, so injecting `to_string()` would
    // produce an id the hook cannot read — and the failure would be silent.
    let session = SessionId::new();
    let env = HookEnv {
        socket: Some("/tmp/omt.sock".to_owned()),
        session: Some(session.to_string()),
        instance: None,
        agent: AgentKind::ClaudeCode,
    };
    let event = build_event(&env, None);
    assert_eq!(event.correlation.session, None);
    assert!(
        !event.correlation.is_direct(),
        "an unattributed observation is recoverable; a wrongly attributed one is not"
    );
}
