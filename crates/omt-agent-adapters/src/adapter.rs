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
    /// How this agent names itself in the terminal title, and what it puts
    /// there to say what it is doing.
    ///
    /// An agent that sets a title is *telling* its host what it is, through
    /// the one channel every terminal has had since before any of this
    /// existed. It needs no permission, it survives ssh, and it does not
    /// depend on how the binary was launched or signed — which is the whole
    /// list of things that make reading the process table unreliable.
    pub title: Option<TitleMarks>,
}

/// What an agent writes in the terminal title.
///
/// Claude Code writes `✳ Claude Code` at rest and swaps the leading glyph for
/// a braille spinner frame while it works. That single string carries both the
/// identity and the state, and omt already parses it and threw it away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleMarks {
    /// A substring that identifies the agent. Matched case-insensitively.
    pub names: Vec<&'static str>,
    /// Leading characters that mean the agent is working.
    ///
    /// Ranges rather than a list, because a spinner is an animation: Claude
    /// Code cycles the braille block, and enumerating ten frames is a list
    /// that goes stale on the next frame they add.
    pub working_prefix: Vec<std::ops::RangeInclusive<char>>,
    /// Leading characters that mean it is waiting for something to do.
    pub idle_prefix: Vec<char>,
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

    /// What this agent's screen says it is doing.
    ///
    /// Empty by default, which means the floor decides: an adapter with no
    /// rules is one nobody has watched closely enough yet, and that is an
    /// honest state to be in. A wrong rule is worse than no rule, because a
    /// wrong rule is confident.
    ///
    /// Each rule names the region it looks at. The same words appear in
    /// scrollback, in a diff, and in a question answered ten minutes ago, so a
    /// rule without a region is a rule that fires on all three.
    fn screen_rules(&self) -> Vec<crate::rules::Rule> {
        Vec::new()
    }

    /// The rules actually used: a loaded set where there is one, and the
    /// shipped set otherwise.
    ///
    /// An agent CLI moves a line on its own schedule, which is not omt's, and
    /// a release is a bad unit of response to that. A file replaces the
    /// shipped rules for one agent; anything wrong with it — missing,
    /// malformed, too new, describing a different agent — leaves them in
    /// place, because the shipped rules are better than nothing in every one
    /// of those cases.
    fn rules_in_use(&self) -> Vec<crate::rules::Rule> {
        self.rules_in_use_from(crate::manifest::directory().as_deref())
    }

    /// The same, from a named directory.
    ///
    /// The directory is a parameter because the environment is process state:
    /// two tests that each pointed `OMT_AGENT_RULES` somewhere raced through
    /// libtest's threads, and one of them failed roughly one run in three
    /// while the comment beside them claimed they were single-threaded. A
    /// caller that knows where to look says so; only the process entry point
    /// consults the environment.
    fn rules_in_use_from(&self, dir: Option<&std::path::Path>) -> Vec<crate::rules::Rule> {
        dir.and_then(|dir| {
            crate::manifest::load_from(dir, &format!("{:?}", self.kind()).to_lowercase())
        })
        .unwrap_or_else(|| self.screen_rules())
    }

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

    /// Turn one record of the agent's own session file into omt's vocabulary.
    ///
    /// Separate from [`AgentAdapter::normalize`] because a session file is not
    /// a stream of hook events: it is what the agent wrote down, in whatever
    /// shape that agent writes, and most of its records are bookkeeping omt
    /// has no opinion on. Returning nothing is the ordinary answer, not an
    /// error — which is why this returns a list rather than a `Result`.
    ///
    /// Empty by default: an agent whose file nobody has read is an honest
    /// state, and a mapping guessed from one example file is how another
    /// agent's records get read as this one's.
    fn transcript_records(&self, _record: &serde_json::Value) -> Vec<AgentPayload> {
        Vec::new()
    }

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

    /// How to start this agent again on the conversation it was having.
    ///
    /// A session that outlives its terminal is the premise of a multiplexer,
    /// and an agent restarted without this is a fresh one sitting in the same
    /// directory with none of the context — which looks like the session
    /// survived and did not.
    ///
    /// `None` means omt does not know how to resume this agent, which is the
    /// honest answer for most of them and is said out loud rather than
    /// approximated: a wrong flag starts a *new* conversation while claiming
    /// to continue one, and the person finds out from the agent's answer.
    ///
    /// The id is the agent's own, as its hooks report it — never one omt
    /// minted.
    fn resume_argv(&self, _agent_session: &str) -> Option<Vec<String>> {
        None
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
