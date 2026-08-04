//! The trait an agent adapter implements, and the vocabulary it speaks.
//!
//! Everything an agent-specific decision could hide in lives here as a method
//! rather than in a central table keyed by [`AgentKind`]. That is the whole
//! point: a third party adding an agent implements this trait, and adding a
//! method with a default does not break them.

use std::ffi::OsString;
use std::path::Path;

use omt_events::AgentPayload;
use omt_types::{AgentKind, Tier};

/// What omt knows when it is about to spawn an agent.
#[derive(Debug, Clone, Default)]
pub struct SpawnCtx {
    /// The instance id, for correlation.
    pub instance: String,
    /// The session id.
    pub session: String,
    /// Where the hook should report.
    pub socket: String,
    /// Where the hook binary lives.
    pub hook_path: Option<String>,
    /// The working directory.
    pub cwd: Option<String>,
}

/// How an agent can be recognized from the outside.
///
/// Ordered by how much it can be trusted, which is not the same as how easy it
/// is to check. An environment marker was *put there* — by the agent's own
/// launcher or by omt — so it is evidence. An argv pattern is a guess that a
/// wrapper script or a version bump can invalidate, which is why matching one
/// yields a lower tier and never a licence to emit structured content.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fingerprint {
    /// Environment variables whose presence identifies this agent.
    pub env_markers: Vec<&'static str>,
    /// Executable base names.
    pub exe_names: Vec<&'static str>,
    /// Substrings that identify it inside an argv, for runtime-wrapped bundles
    /// where the executable is `node` or `bun` and says nothing.
    pub argv_patterns: Vec<&'static str>,
    /// The variable carrying the agent's own session id, where it has one.
    pub session_id_var: Option<&'static str>,
}

/// How confidently an agent was identified, and on what evidence.
///
/// Carries the evidence rather than only the verdict, because
/// `agent.explain` has to be able to say *why* — a user looking at a wrong
/// detection needs to see what was matched, not be told a name again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    /// Which agent.
    pub agent: AgentKind,
    /// How it was found, which bounds what may be claimed from it.
    pub tier: Tier,
    /// What actually matched, in the user's words.
    pub evidence: String,
    /// The agent's own session id, if the environment carried one.
    pub agent_session: Option<String>,
}

/// Which session modes an adapter supports.
///
/// A set rather than an enum: an adapter that speaks ACP still supports PTY,
/// and the user chooses. Modelled here rather than inferred from whether
/// `acp_spawn` returns something, so "supports it but cannot right now" stays
/// expressible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionModeSet {
    /// Run the agent's own CLI on a pty.
    pub pty: bool,
    /// Drive the agent over ACP.
    pub native: bool,
}

impl SessionModeSet {
    /// The default: the agent's own CLI, unchanged.
    pub const PTY_ONLY: Self = Self {
        pty: true,
        native: false,
    };
    /// Both.
    pub const BOTH: Self = Self {
        pty: true,
        native: true,
    };
}

/// How to start an agent over ACP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpSpawn {
    /// The program.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// Environment additions.
    pub env: Vec<(String, String)>,
}

/// How an agent wants to be handed a file that already exists on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachmentReference {
    /// Its own mention syntax, e.g. `@path/to/file`.
    Mention(String),
    /// A bare path, for an agent with no syntax of its own.
    BarePath(String),
    /// Markdown, for agents that render it.
    Markdown(String),
}

/// What a file is, which changes how an agent should be handed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentClass {
    /// An image.
    Image,
    /// Text the agent can read.
    Text,
    /// Anything else.
    Binary,
}

/// What went wrong in an adapter.
#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    /// The agent's own configuration could not be read or written.
    #[error("integration for {agent:?}: {detail}")]
    Integration {
        /// Which agent.
        agent: AgentKind,
        /// What happened.
        detail: String,
    },
    /// A payload arrived in a shape this adapter does not recognize.
    #[error("unrecognized {agent:?} event `{event}`")]
    UnknownEvent {
        /// Which agent.
        agent: AgentKind,
        /// The agent's own event name, verbatim.
        event: String,
    },
}

/// One agent's knowledge, in one place.
pub trait AgentAdapter: Send + Sync {
    /// Which agent this is.
    fn kind(&self) -> AgentKind;

    /// How to recognize it from outside.
    fn fingerprint(&self) -> Fingerprint;

    /// Environment to inject when omt spawns it.
    ///
    /// This is what removes the entire class of "match the transcript to the
    /// pane" heuristics: a hook that starts with these variables already knows
    /// which pane it belongs to.
    fn spawn_env(&self, ctx: &SpawnCtx) -> Vec<(OsString, OsString)>;

    /// Which session modes it supports.
    fn supported_modes(&self) -> SessionModeSet {
        SessionModeSet::PTY_ONLY
    }

    /// How to start it over ACP, when it supports that.
    fn acp_spawn(&self, _ctx: &SpawnCtx) -> Option<AcpSpawn> {
        None
    }

    /// Turn one of the agent's own hook payloads into omt's vocabulary.
    ///
    /// The agent's event name arrives verbatim and is mapped here, per agent,
    /// rather than by the hook — so an event omt does not recognize is
    /// loggable rather than lost.
    ///
    /// # Errors
    /// Returns the event name back when this adapter has no mapping for it.
    fn normalize(
        &self,
        event: &str,
        payload: &serde_json::Value,
    ) -> Result<Vec<AgentPayload>, AdapterError>;

    /// The highest tier this adapter can reach.
    ///
    /// Stated rather than discovered, because the honest answer for some agents
    /// is [`Tier::Heuristic`], and a UI that says so is better than one that
    /// leaves the user to work it out from an empty transcript.
    fn best_tier(&self) -> Tier;

    /// How to interrupt it.
    ///
    /// The floor's only control: for a heuristic-tier agent a phone gets no
    /// blocks, no transcript and no answerable cards, so stopping it is the
    /// entire remote surface — which is why this cannot be left to a help
    /// string.
    fn interrupt(&self) -> Interrupt {
        Interrupt::ControlC
    }

    /// Its native file-reference syntax for a workspace-relative path.
    ///
    /// `None` means the agent has none, and the surface offers "insert path"
    /// rather than "insert mention".
    fn path_mention(&self, _rel: &str) -> Option<String> {
        None
    }

    /// How it wants to be handed a file on disk.
    fn attachment_reference(
        &self,
        path: &Path,
        class: AttachmentClass,
    ) -> Option<AttachmentReference> {
        let _ = class;
        Some(AttachmentReference::BarePath(path.display().to_string()))
    }

    /// Slash commands known without asking the agent.
    ///
    /// Only for agents that cannot be asked. Anything that can report its own
    /// commands should, since a static list goes stale the first time the agent
    /// ships a release.
    fn static_commands(&self) -> Vec<&'static str> {
        Vec::new()
    }
}

/// How an agent is stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interrupt {
    /// Write `Ctrl-C` to the pty. Position-independent by construction, which
    /// is why it is safe without knowing anything about screen state.
    ControlC,
    /// Write `Esc` — Claude Code's own interrupt key.
    Escape,
    /// Ask over the agent's own protocol, leaving the pane untouched.
    Native,
}

impl Interrupt {
    /// The bytes to write, for the ones that are keystrokes.
    #[must_use]
    pub const fn bytes(self) -> Option<&'static [u8]> {
        match self {
            Self::ControlC => Some(b"\x03"),
            Self::Escape => Some(b"\x1b"),
            Self::Native => None,
        }
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

    #[test]
    fn an_interrupt_that_is_a_keystroke_has_bytes() {
        assert_eq!(Interrupt::ControlC.bytes(), Some(&b"\x03"[..]));
        assert_eq!(Interrupt::Escape.bytes(), Some(&b"\x1b"[..]));
    }

    #[test]
    fn a_native_interrupt_has_none_because_it_is_not_typed() {
        // Writing bytes for it would put a keystroke into a pane whose state
        // nothing inspected.
        assert_eq!(Interrupt::Native.bytes(), None);
    }

    #[test]
    #[allow(
        clippy::assertions_on_constants,
        reason = "the constants are the thing under test: this guards a future edit to them"
    )]
    fn pty_is_always_supported() {
        // An adapter that could only be driven over a protocol would be an
        // adapter that cannot run the user's actual CLI.
        assert!(SessionModeSet::PTY_ONLY.pty);
        assert!(SessionModeSet::BOTH.pty);
        assert!(!SessionModeSet::PTY_ONLY.native);
    }
}
