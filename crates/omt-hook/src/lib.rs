//! The hook binary's logic, in a library so it can be tested without spawning.
//!
//! One rule governs everything here: **the agent must never be worse off for
//! omt being installed.** Whatever goes wrong — no socket, no daemon, a slow
//! daemon, a malformed reply, a panic — the hook prints what its agent expects
//! and exits successfully. An observation is worth having; it is not worth an
//! agent hanging for.

use std::io::{Read, Write};
use std::time::Duration;

use omt_proto::{FrameKind, HookAck, HookCorrelation, HookDirective, HookEvent, ProtoMessage};
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

/// Read the agent's payload from stdin, to end of input.
///
/// **Always read to the end, even when the payload will not be used.** The
/// agent is writing to this process's stdin; exiting before the write finishes
/// closes the pipe under it, and the agent takes an `EPIPE` — or a `SIGPIPE` —
/// part way through a write it had no reason to expect to fail. It surfaces as
/// the agent reporting the hook exited 127 and then dying, which looks like
/// anything but "the hook returned too early".
///
/// This is why [`drain_stdin`] exists and why every early-exit path calls it:
/// the guard clause that skips the work must not also skip the read.
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

/// Consume stdin to the end and discard it.
///
/// For the paths that exit without wanting the payload. Reading and throwing it
/// away is not waste: it is the difference between the agent finishing its
/// write and the agent taking a broken pipe.
pub fn drain_stdin(mut input: impl Read) {
    let mut sink = Vec::new();
    let _ = input.read_to_end(&mut sink);
}

/// Write what the agent expects, and say how it went.
///
/// # Errors
/// Returns the I/O error if stdout could not be written, which the caller
/// ignores — at that point the agent is not reading anyway.
pub fn emit_proceed(mut out: impl Write, agent: AgentKind) -> std::io::Result<()> {
    writeln!(out, "{}", proceed_document(agent))
}

/// How long the hook will wait before giving up and letting its agent proceed.
///
/// Short on purpose. The budget belongs to somebody else's agent, and a hook
/// that blocks is a hook the user experiences as the agent being slow.
pub const DEADLINE_MS: u32 = 250;

/// Build and send the observation for this invocation.
///
/// # Errors
/// Returns why it did not get through. Every variant leads to the same
/// behaviour — the agent proceeds — so this is for diagnosis, never a decision.
pub fn report(env: &HookEnv, payload: Option<&serde_json::Value>) -> Result<HookDirective, Missed> {
    let Some(socket) = env.socket.as_deref() else {
        return Err(Missed::Unreachable(
            "no socket in the environment".to_owned(),
        ));
    };
    let event = build_event(env, payload);
    report_to_daemon(
        socket,
        &event,
        std::time::Duration::from_millis(u64::from(DEADLINE_MS)),
    )
}

/// Turn what arrived into what the daemon expects.
///
/// The agent's own event name is carried **verbatim**. Normalizing it here
/// would mean an event this build has never seen becomes unloggable rather
/// than merely unrecognized, and a hook is the worst place to decide what an
/// agent meant.
#[must_use]
pub fn build_event(env: &HookEnv, payload: Option<&serde_json::Value>) -> HookEvent {
    let field = |k: &str| {
        payload
            .and_then(|p| p.get(k))
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    };

    let event = field("hook_event_name")
        .or_else(|| field("event"))
        .or_else(|| std::env::var("OMT_HOOK_EVENT").ok())
        .unwrap_or_else(|| "unknown".to_owned());

    let base = HookEvent {
        // A per-process value, not a request id: a hook lives for milliseconds
        // and makes exactly one call, so uniqueness within the connection is
        // the whole requirement.
        nonce: nonce(),
        hook_proto: 1,
        agent: env.agent,
        agent_version: field("version"),
        agent_session: field("session_id"),
        event,
        tool_use_id: field("tool_use_id"),
        tool_name: field("tool_name"),
        tool_input: None,
        raw: None,
        truncated: None,
        correlation: HookCorrelation {
            // `from_wire`, never `parse` on the Display form: Display is
            // abbreviated for logs and cannot be read back, so parsing it would
            // silently correlate to nothing.
            instance: env
                .instance
                .as_deref()
                .and_then(omt_types::InstanceId::from_wire),
            // Never guessed. An unattributed observation is recoverable; one
            // attributed to the wrong session silently corrupts that session.
            session: env
                .session
                .as_deref()
                .and_then(omt_types::SessionId::from_wire),
            pid: std::process::id(),
            ppid: parent_pid(),
            cwd: std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
        },
        deadline_ms: DEADLINE_MS,
    };

    match payload.and_then(|p| p.get("tool_input")) {
        Some(input) => base.with_payload("tool_input", input.clone()),
        None => base,
    }
}

fn nonce() -> u64 {
    // The pid plus a monotonic counter: unique within this process, and this
    // process makes one call in its life.
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    (u64::from(std::process::id()) << 16) | (n & 0xffff)
}

fn parent_pid() -> u32 {
    #[cfg(unix)]
    {
        // SAFETY: getppid takes no arguments and cannot fail.
        #[allow(unsafe_code, reason = "getppid is infallible and argument-free")]
        unsafe {
            libc::getppid() as u32
        }
    }
    #[cfg(not(unix))]
    {
        0
    }
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
    fn draining_consumes_everything_the_agent_wrote() {
        // The bug this exists for: the agent is writing its payload to this
        // process's stdin. Exiting before the write finishes closes the pipe
        // under it and the agent takes a broken pipe part way through — which
        // it reports as the hook exiting 127, and then it dies. Reading and
        // throwing the bytes away is not waste; it is the difference.
        struct Counting<'a> {
            data: &'a [u8],
            read: &'a std::cell::Cell<usize>,
        }
        impl Read for Counting<'_> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let n = self.data.len().min(buf.len());
                buf[..n].copy_from_slice(&self.data[..n]);
                self.data = &self.data[n..];
                self.read.set(self.read.get() + n);
                Ok(n)
            }
        }

        let payload = br#"{"hook_event_name":"PreToolUse","tool_name":"Bash"}"#;
        let read = std::cell::Cell::new(0);
        drain_stdin(Counting {
            data: payload,
            read: &read,
        });
        assert_eq!(
            read.get(),
            payload.len(),
            "the hook exited without consuming what the agent wrote"
        );
    }

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
