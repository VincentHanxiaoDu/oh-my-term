//! The adapter against the Codex that is installed, not a fixture.
//!
//! Everything else about this adapter is checked against payloads written by
//! hand, which proves the table is self-consistent and nothing more. This runs
//! the real `codex app-server`, reads what it actually emits, and asserts the
//! adapter understands it — which is the only thing that catches a method name
//! guessed wrong.
//!
//! Ignored by default because it needs Codex installed. Run it with
//! `cargo test -p omt-agent-adapters -- --include-ignored`.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use std::io::Write;
use std::process::{Command, Stdio};

use omt_agent_adapters::{ServerLine, agents::Codex, decode_line};

fn codex_is_installed() -> bool {
    Command::new("codex")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Ask the real app-server to initialize and collect what it says.
fn real_notifications() -> Vec<String> {
    let mut child = Command::new("codex")
        .arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("codex app-server");

    {
        let mut stdin = child.stdin.take().expect("stdin");
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"clientInfo":{{"name":"omt","title":"omt","version":"0.1.0"}}}}}}"#
        )
        .expect("write");
        stdin.flush().ok();
        // Dropped, so the server sees EOF and exits rather than this test
        // waiting on a process that is waiting on it.
    }

    let output = child.wait_with_output().expect("wait");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .filter(|l| !l.trim().is_empty())
        .collect()
}

#[test]
#[ignore = "needs codex installed"]
fn the_real_app_server_speaks_what_this_client_reads() {
    if !codex_is_installed() {
        return;
    }
    let lines = real_notifications();
    assert!(!lines.is_empty(), "the app-server said nothing at all");

    // Every line is JSON the client can parse. A line it cannot is the whole
    // failure mode this test exists for.
    for line in &lines {
        assert!(
            !matches!(decode_line(&Codex, line), ServerLine::Unparseable { .. }),
            "the real server emitted a line this client cannot parse:\n{line}"
        );
    }
}

#[test]
#[ignore = "needs codex installed"]
fn the_real_app_server_uses_slash_separated_method_names() {
    // The thing a hand-written fixture cannot tell you. An earlier version of
    // this adapter guessed `task_started` and would have matched nothing at
    // all: Codex spells its notifications `turn/started`.
    if !codex_is_installed() {
        return;
    }
    let methods: Vec<String> = real_notifications()
        .iter()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v.get("method").and_then(|m| m.as_str()).map(str::to_owned))
        .collect();

    assert!(
        !methods.is_empty(),
        "the app-server sent no notifications to check"
    );
    assert!(
        methods.iter().all(|m| m.contains('/')),
        "a method was not slash-separated after all: {methods:?}"
    );
}

#[test]
#[ignore = "needs codex installed"]
fn a_reply_to_our_own_request_is_not_reported_as_an_event() {
    // The initialize response comes back with an id and no method. Reporting it
    // as a notification would put a phantom event in the session's stream.
    if !codex_is_installed() {
        return;
    }
    let replies: Vec<String> = real_notifications()
        .into_iter()
        .filter(|l| {
            serde_json::from_str::<serde_json::Value>(l)
                .ok()
                .is_some_and(|v| v.get("id").is_some() && v.get("method").is_none())
        })
        .collect();
    assert!(!replies.is_empty(), "no reply came back to compare against");
    for reply in replies {
        assert_eq!(
            decode_line(&Codex, &reply),
            ServerLine::Payloads(Vec::new()),
            "a reply was decoded as an event: {reply}"
        );
    }
}
