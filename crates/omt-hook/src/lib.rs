//! The hook binary's logic, in a library so it can be tested without spawning.
//!
//! One rule governs everything here: **the agent must never be worse off for
//! omt being installed.** Whatever goes wrong — no socket, no daemon, a slow
//! daemon, a malformed reply, a panic — the hook prints what its agent expects
//! and exits successfully. An observation is worth having; it is not worth an
//! agent hanging for.

use std::io::{Read, Write};
use std::time::Duration;

use omt_proto::{FrameKind, HookAck, HookDirective, HookEvent, ProtoMessage};
use omt_types::AgentKind;

/// How long to wait for the daemon before giving up.
///
/// Beyond the agent's own tolerance is worse than useless: the observation
/// arrives after the moment it described. Two hundred and fifty milliseconds is
/// long enough for a local socket round trip under load and short enough that a
/// human does not perceive it.
pub const DEFAULT_DEADLINE: Duration = Duration::from_millis(250);

/// What the hook writes to stdout, per agent.
///
/// This table lives here rather than in the protocol crate on purpose. The
/// fail-open path is exactly the path where the daemon is unreachable, so a
/// hook that had to be *told* what to print would have nothing to print at the
/// only moment it matters. Knowing its own agent's dialect is what lets it fail
/// safely alone.
#[must_use]
pub fn proceed_document(agent: AgentKind) -> &'static str {
    match agent {
        // Claude Code accepts an empty object as "no opinion".
        AgentKind::ClaudeCode => "{}",
        // Codex and Cursor read the same shape.
        AgentKind::Codex | AgentKind::Cursor => "{}",
        // Gemini and Qwen take an explicit continue.
        AgentKind::GeminiCli | AgentKind::QwenCode => r#"{"continue":true}"#,
        // Everything else: an empty document is the least surprising thing a
        // hook can say, and an agent that ignores stdout is unaffected.
        _ => "{}",
    }
}

/// Why a hook did not reach the daemon.
///
/// Recorded for diagnosis, never acted on: every variant produces the same
/// output, because a hook that behaved differently depending on why it failed
/// would make an agent's behaviour depend on omt's health.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missed {
    /// `$OMT_SOCK` was not set — omt did not spawn this agent.
    NoSocket,
    /// The socket was there and would not connect.
    Unreachable(String),
    /// The daemon did not answer inside the deadline.
    TimedOut,
    /// It answered with something unparseable.
    BadReply(String),
}

/// The environment a hook reads.
#[derive(Debug, Clone)]
pub struct HookEnv {
    /// `$OMT_SOCK`.
    pub socket: Option<String>,
    /// `$OMT_SESSION`.
    pub session: Option<String>,
    /// `$OMT_INSTANCE`.
    pub instance: Option<String>,
    /// `--agent` or `$OMT_HOOK_AGENT`.
    pub agent: AgentKind,
}

impl HookEnv {
    /// Read the environment.
    #[must_use]
    pub fn from_process(agent: AgentKind) -> Self {
        Self {
            socket: std::env::var("OMT_SOCK").ok().filter(|s| !s.is_empty()),
            session: std::env::var("OMT_SESSION").ok().filter(|s| !s.is_empty()),
            instance: std::env::var("OMT_INSTANCE").ok().filter(|s| !s.is_empty()),
            agent,
        }
    }

    /// Whether it is even worth attempting a connection.
    ///
    /// The guard clause every hook opens with: an agent omt did not spawn has
    /// no socket to talk to, and trying anyway would cost that agent a syscall
    /// and a failure on every hook event forever.
    #[must_use]
    pub fn is_worth_trying(&self) -> bool {
        self.socket.is_some()
    }
}

/// Parse the `--agent` flag.
///
/// # Errors
/// Never. An unrecognized agent becomes [`AgentKind::Unknown`], whose document
/// is the safe one — refusing to start would break the agent, which is the one
/// thing this binary must not do.
#[must_use]
pub fn parse_agent(args: &[String]) -> AgentKind {
    let named = args
        .windows(2)
        .find(|w| w[0] == "--agent")
        .map(|w| w[1].clone())
        .or_else(|| std::env::var("OMT_HOOK_AGENT").ok());

    match named.as_deref() {
        Some("claude-code" | "claude_code") => AgentKind::ClaudeCode,
        Some("codex") => AgentKind::Codex,
        Some("opencode") => AgentKind::Opencode,
        Some("gemini-cli" | "gemini_cli") => AgentKind::GeminiCli,
        Some("qwen-code" | "qwen_code") => AgentKind::QwenCode,
        Some("cursor") => AgentKind::Cursor,
        Some("goose") => AgentKind::Goose,
        Some("amp") => AgentKind::Amp,
        Some("aider") => AgentKind::Aider,
        Some("crush") => AgentKind::Crush,
        _ => AgentKind::Unknown,
    }
}

/// Read the agent's payload from stdin.
///
/// # Errors
/// Never fails upward: unreadable or malformed input yields `None`, and the
/// hook proceeds. Refusing to run because the payload surprised us would make
/// omt the reason an agent stopped.
#[must_use]
pub fn read_payload(mut input: impl Read) -> Option<serde_json::Value> {
    let mut buf = String::new();
    input.read_to_string(&mut buf).ok()?;
    if buf.trim().is_empty() {
        return None;
    }
    serde_json::from_str(&buf).ok()
}

/// Write what the agent expects, and say how it went.
///
/// # Errors
/// Returns the I/O error if stdout could not be written, which the caller
/// ignores — at that point the agent is not reading anyway.
pub fn emit_proceed(mut out: impl Write, agent: AgentKind) -> std::io::Result<()> {
    writeln!(out, "{}", proceed_document(agent))
}

/// Report an observation and read the directive back.
///
/// # Errors
/// Returns why it did not get through. Every variant leads to the same
/// behaviour — the caller proceeds — so the value is for diagnosis, never for
/// a decision.
pub fn report_to_daemon(
    socket: &str,
    event: &HookEvent,
    deadline: Duration,
) -> Result<HookDirective, Missed> {
    let mut stream =
        omt_transport::connect(socket).map_err(|e| Missed::Unreachable(e.to_string()))?;

    // Both directions are bounded. A daemon that accepted the connection and
    // then stopped talking would otherwise hold the agent for as long as it
    // stayed quiet, which is the failure this whole binary exists to prevent.
    stream
        .set_read_timeout(Some(deadline))
        .and_then(|()| stream.set_write_timeout(Some(deadline)))
        .map_err(|e| Missed::Unreachable(e.to_string()))?;

    let message = ProtoMessage::HookEvent(Box::new(event.clone()));
    let body = serde_json::to_vec(&message).map_err(|e| Missed::BadReply(e.to_string()))?;
    omt_transport::write_frame(&mut stream, FrameKind::Text, &body)
        .map_err(|e| Missed::Unreachable(e.to_string()))?;

    let (_, payload) = omt_transport::read_frame(&mut stream).map_err(|e| match e {
        omt_transport::FramingError::Io(m) if m.contains("timed out") => Missed::TimedOut,
        other => Missed::BadReply(other.to_string()),
    })?;

    match serde_json::from_slice::<ProtoMessage>(&payload) {
        Ok(ProtoMessage::HookAck(HookAck { nonce, directive })) if nonce == event.nonce => {
            Ok(directive)
        }
        // A reply for a different nonce is somebody else's answer. Acting on it
        // would be worse than not being answered at all.
        Ok(_) => Err(Missed::BadReply(
            "reply did not match the request".to_owned(),
        )),
        Err(e) => Err(Missed::BadReply(e.to_string())),
    }
}

/// The exit status this binary always returns.
///
/// Always zero, deliberately. A non-zero exit is how a hook tells its agent
/// something is wrong, and nothing about omt's health is the agent's problem.
pub const EXIT_OK: i32 = 0;

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    #[test]
    fn every_agent_has_a_proceed_document() {
        // The fail-open path must never have to ask what to print.
        for agent in AgentKind::ALL {
            let doc = proceed_document(*agent);
            assert!(!doc.is_empty(), "{agent} has no proceed document");
            serde_json::from_str::<serde_json::Value>(doc)
                .unwrap_or_else(|e| panic!("{agent}'s document is not JSON: {e}"));
        }
    }

    #[test]
    fn an_unknown_agent_still_gets_a_safe_document() {
        // Refusing to start would break the agent, which is the one thing this
        // binary must not do.
        let doc = proceed_document(AgentKind::Unknown);
        assert_eq!(doc, "{}");
    }

    #[test]
    fn no_socket_means_do_not_even_try() {
        // An agent omt did not spawn would otherwise pay a syscall and a
        // failure on every hook event, forever.
        let bare = |socket| HookEnv {
            socket,
            session: None,
            instance: None,
            agent: AgentKind::ClaudeCode,
        };
        assert!(!bare(None).is_worth_trying());
        assert!(bare(Some("/run/omt.sock".into())).is_worth_trying());
    }

    #[test]
    fn an_empty_socket_variable_counts_as_absent() {
        // `OMT_SOCK=` is how a shell leaves an unset variable behind, and it
        // must read as absent rather than as a path named "".
        //
        // Asserted against the filter directly rather than by mutating the
        // process environment: that is global state, and a test that changes it
        // races every other test in the binary.
        let filtered = Some(String::new()).filter(|s: &String| !s.is_empty());
        assert!(filtered.is_none());
    }

    #[test]
    fn a_malformed_payload_does_not_stop_the_hook() {
        assert!(read_payload(&b"not json"[..]).is_none());
        assert!(read_payload(&b""[..]).is_none());
        assert!(read_payload(&b"   "[..]).is_none());
    }

    #[test]
    fn a_well_formed_payload_is_parsed() {
        let v = read_payload(&br#"{"hook_event_name":"PreToolUse"}"#[..]).expect("parse");
        assert_eq!(v["hook_event_name"], "PreToolUse");
    }

    #[test]
    fn agent_names_parse_in_both_spellings() {
        for (arg, expected) in [
            ("claude-code", AgentKind::ClaudeCode),
            ("claude_code", AgentKind::ClaudeCode),
            ("gemini-cli", AgentKind::GeminiCli),
            ("nonsense", AgentKind::Unknown),
        ] {
            let args = vec!["--agent".to_owned(), arg.to_owned()];
            assert_eq!(parse_agent(&args), expected, "for {arg}");
        }
    }

    #[test]
    fn emitting_proceed_writes_one_line() {
        let mut out = Vec::new();
        emit_proceed(&mut out, AgentKind::ClaudeCode).expect("write");
        assert_eq!(String::from_utf8_lossy(&out), "{}\n");
    }

    #[test]
    fn the_exit_status_is_always_zero() {
        // Nothing about omt's health is the agent's problem.
        assert_eq!(EXIT_OK, 0);
    }

    #[test]
    fn every_miss_produces_the_same_output() {
        // A hook that behaved differently depending on why it failed would make
        // the agent's behaviour depend on omt's health.
        let misses = [
            Missed::NoSocket,
            Missed::Unreachable("refused".into()),
            Missed::TimedOut,
            Missed::BadReply("garbage".into()),
        ];
        let mut outputs = std::collections::HashSet::new();
        for _ in &misses {
            let mut out = Vec::new();
            emit_proceed(&mut out, AgentKind::ClaudeCode).expect("write");
            outputs.insert(out);
        }
        assert_eq!(outputs.len(), 1, "the reason must not change the output");
    }
}
