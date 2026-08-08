//! Driving an agent that speaks JSON-RPC over its own stdio.
//!
//! Codex's `app-server` is this shape: a subprocess, newline-delimited JSON,
//! notifications pushed as things happen. The decoding of those notifications
//! already lives in the agent's adapter — this is only the pipe, which is why
//! it is generic over the adapter rather than knowing anything about Codex.
//!
//! What it deliberately does not do is interpret. A line it cannot parse is
//! reported and skipped, and a method the adapter has no mapping for comes back
//! as an error naming the method. Silently dropping either is how a protocol
//! source quietly degrades into a worse one while still claiming its tier.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

use omt_events::AgentPayload;

use crate::adapter::{AdapterError, AgentAdapter};

/// A line that arrived from an app-server.
///
/// Named for the line rather than for the direction, because `Incoming` is
/// already what the ACP client calls its own message type and two things with
/// one name in one crate is a name that means nothing.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerLine {
    /// A notification, already in omt's vocabulary.
    Payloads(Vec<AgentPayload>),
    /// A line that was not JSON at all.
    Unparseable {
        /// What arrived, so it can be logged rather than guessed at.
        line: String,
    },
    /// A method this adapter has no mapping for.
    Unknown {
        /// The method's own name, verbatim.
        method: String,
    },
}

/// Decode one line of an app-server's output.
///
/// Separated from the process so the interesting half is testable without
/// spawning anything: what a line means is a pure function of the line and the
/// adapter.
pub fn decode_line(adapter: &dyn AgentAdapter, line: &str) -> ServerLine {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return ServerLine::Unparseable {
            line: line.to_owned(),
        };
    };
    // A response to a request omt made carries an id; a notification does not.
    // Only notifications describe the world changing, and treating a response
    // as one would report the same event twice.
    let Some(method) = value.get("method").and_then(|m| m.as_str()) else {
        return ServerLine::Payloads(Vec::new());
    };
    let params = value
        .get("params")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    match adapter.normalize(method, &params) {
        Ok(payloads) => ServerLine::Payloads(payloads),
        Err(AdapterError::UnknownEvent { event, .. }) => ServerLine::Unknown { method: event },
        Err(_) => ServerLine::Unknown {
            method: method.to_owned(),
        },
    }
}

/// A running app-server.
pub struct AppServer {
    child: Child,
}

impl AppServer {
    /// Start one.
    ///
    /// # Errors
    /// Fails if the program cannot be run.
    pub fn spawn(program: &str, args: &[String]) -> std::io::Result<Self> {
        let child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Inherited rather than captured: an agent's diagnostics belong
            // where the user can see them, and swallowing them is how a
            // misconfigured agent looks like a broken omt.
            .stderr(Stdio::inherit())
            .spawn()?;
        Ok(Self { child })
    }

    /// Read notifications until the process ends, handing each to `sink`.
    ///
    /// # Errors
    /// Fails if the child's stdout cannot be taken.
    pub fn drive(
        &mut self,
        adapter: &dyn AgentAdapter,
        mut sink: impl FnMut(ServerLine),
    ) -> std::io::Result<()> {
        let stdout = self.child.stdout.take().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "no stdout on the app-server",
            )
        })?;
        for line in BufReader::new(stdout).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            sink(decode_line(adapter, &line));
        }
        Ok(())
    }

    /// Stop it.
    ///
    /// # Errors
    /// Fails if the child cannot be killed or waited on.
    pub fn stop(&mut self) -> std::io::Result<()> {
        self.child.kill().ok();
        self.child.wait().map(|_| ())
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;
    use crate::agents::Codex;

    #[test]
    fn a_notification_becomes_omt_payloads() {
        let out = decode_line(
            &Codex,
            r#"{"jsonrpc":"2.0","method":"turn/started","params":{"turn":{"id":"t1"}}}"#,
        );
        let ServerLine::Payloads(payloads) = out else {
            panic!("expected payloads, got {out:?}");
        };
        assert!(matches!(payloads[0], AgentPayload::TurnStart { .. }));
    }

    #[test]
    fn a_response_to_our_own_request_is_not_reported_as_an_event() {
        // It carries an id and no method. Treating it as a notification would
        // report the same thing twice.
        let out = decode_line(&Codex, r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#);
        assert_eq!(out, ServerLine::Payloads(Vec::new()));
    }

    #[test]
    fn a_method_with_no_mapping_names_itself_rather_than_vanishing() {
        // A protocol source that silently drops what it does not understand
        // degrades into a worse one while still claiming its tier.
        let out = decode_line(&Codex, r#"{"jsonrpc":"2.0","method":"brand_new_thing"}"#);
        assert_eq!(
            out,
            ServerLine::Unknown {
                method: "brand_new_thing".to_owned()
            }
        );
    }

    #[test]
    fn a_line_that_is_not_json_is_reported_with_its_contents() {
        let out = decode_line(&Codex, "this is not json");
        assert_eq!(
            out,
            ServerLine::Unparseable {
                line: "this is not json".to_owned()
            }
        );
    }

    #[test]
    fn a_real_subprocess_is_read_line_by_line() {
        // The pipe half, against an actual process: an app-server is a
        // subprocess emitting newline-delimited JSON, and this is that.
        let mut server = AppServer::spawn(
            "/bin/sh",
            &[
                "-c".to_owned(),
                r#"printf '%s\n' '{"jsonrpc":"2.0","method":"turn/started","params":{"turn":{"id":"t1"}}}' '' '{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"status":"failed"}}}'"#
                    .to_owned(),
            ],
        )
        .expect("spawn");

        let mut seen = Vec::new();
        server
            .drive(&Codex, |incoming| seen.push(incoming))
            .expect("drive");
        server.stop().ok();

        // Two notifications, and the blank line between them ignored rather
        // than reported as unparseable.
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert!(matches!(seen[0], ServerLine::Payloads(_)));
        let ServerLine::Payloads(second) = &seen[1] else {
            panic!("expected payloads");
        };
        assert!(matches!(
            second[0],
            AgentPayload::TurnEnd {
                outcome: omt_events::TurnOutcome::Error,
                ..
            }
        ));
    }
}
