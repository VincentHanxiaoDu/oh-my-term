//! Domain vocabularies: what an agent is, what state it is in, who is acting.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Which coding agent occupies a session.
///
/// A closed set, because every open-ended alternative ends the same way: an
/// adapter that silently does nothing for a value nobody enumerated. Adding an
/// agent is a deliberate edit here plus an adapter, which is the right amount
/// of friction.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AgentKind {
    /// Anthropic's Claude Code.
    ClaudeCode,
    /// OpenAI's Codex CLI.
    Codex,
    /// sst's opencode.
    Opencode,
    /// Google's Gemini CLI.
    GeminiCli,
    /// QwenLM's Qwen Code.
    QwenCode,
    /// Cursor's `cursor-agent`.
    Cursor,
    /// Block's Goose.
    Goose,
    /// Sourcegraph's Amp.
    Amp,
    /// Aider.
    Aider,
    /// Charmbracelet's Crush.
    Crush,
    /// Something omt could not identify.
    Unknown,
}

impl AgentKind {
    /// Every variant, for exhaustive tests and for enumerating adapters.
    pub const ALL: &'static [Self] = &[
        Self::ClaudeCode,
        Self::Codex,
        Self::Opencode,
        Self::GeminiCli,
        Self::QwenCode,
        Self::Cursor,
        Self::Goose,
        Self::Amp,
        Self::Aider,
        Self::Crush,
        Self::Unknown,
    ];

    /// The wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude_code",
            Self::Codex => "codex",
            Self::Opencode => "opencode",
            Self::GeminiCli => "gemini_cli",
            Self::QwenCode => "qwen_code",
            Self::Cursor => "cursor",
            Self::Goose => "goose",
            Self::Amp => "amp",
            Self::Aider => "aider",
            Self::Crush => "crush",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for AgentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How a session runs its agent.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    /// The user's real CLI in a real PTY, observed from outside.
    ///
    /// The default, and the product's premise.
    Pty,
    /// The agent spawned in ACP mode, with omt rendering everything.
    ///
    /// No TUI exists in this mode — it is a replacement front end, not an
    /// observability sidecar — so choosing it is choosing not to run the
    /// agent's own interface. That must always be deliberate.
    Native,
}

/// What an agent is doing, as far as omt can tell.
///
/// The only vocabulary for agent activity on any surface. There is no separate
/// `busy` or `needs_attention`: `Working` is busy and `Blocked` is needs-you,
/// and having one name per concept is what stops three surfaces inventing
/// three synonyms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AgentState {
    /// Spawned, not yet identified or ready.
    Starting,
    /// Waiting for a human to say something.
    Idle,
    /// Working on a turn.
    Working {
        /// Optional detail, when a source can say what it is doing.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    /// Needs a human before it can continue.
    Blocked {
        /// Why it is blocked.
        reason: BlockReason,
        /// The interaction to answer, when the block is structured enough to
        /// be answerable. `None` means omt can see that something needs a
        /// human but cannot render it — an honest, visible degradation rather
        /// than a silent one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        interaction: Option<crate::ids::InteractionId>,
    },
    /// The process is gone.
    Exited {
        /// Its exit code, where one was observed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<i32>,
    },
    /// No source can currently say.
    Unknown,
}

impl AgentState {
    /// Whether a human is being waited on.
    #[must_use]
    pub const fn needs_human(&self) -> bool {
        matches!(self, Self::Blocked { .. })
    }
}

/// Why an agent is blocked.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum BlockReason {
    /// A structured question with enumerated options.
    Question,
    /// A tool or command wants approval.
    Permission,
    /// A plan is offered for review.
    PlanReview,
    /// A free-text answer is wanted.
    Elicitation,
    /// Plain input is wanted.
    Input,
    /// Something needs a human, and the source cannot say what.
    Unspecified,
}

/// How much authority a credential carries.
///
/// Roles answer *who you shared this instance with*. They are not a way to
/// degrade the owner's own devices: an authenticated `Operator` is equivalent
/// to sitting at the TUI, because access control decides who may connect, not
/// what they may do once connected.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// May read. Cannot change anything.
    Viewer,
    /// May do anything the owner can do at the terminal.
    Operator,
    /// May also administer the instance itself.
    Admin,
}

impl Role {
    /// Whether this role satisfies a requirement for `needed`.
    #[must_use]
    pub fn satisfies(self, needed: Self) -> bool {
        self >= needed
    }
}

/// Who performed an action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Actor {
    /// The TUI in this process.
    Local,
    /// A connected client.
    Remote {
        /// Which identity.
        identity: crate::ids::IdentityId,
        /// Which of that identity's devices.
        device: crate::ids::DeviceId,
    },
    /// A plugin, acting for someone.
    Plugin {
        /// The plugin's id.
        plugin: String,
        /// The actor it is acting for, so attribution survives delegation.
        on_behalf_of: Box<Actor>,
    },
    /// The instance itself — a timeout firing, a policy applying.
    System,
    /// The agent, which can resolve its own interaction by giving up on it.
    Agent,
}

impl fmt::Display for Actor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local => f.write_str("local"),
            Self::Remote { device, .. } => write!(f, "{device}"),
            Self::Plugin { plugin, on_behalf_of } => write!(f, "plugin:{plugin} for {on_behalf_of}"),
            Self::System => f.write_str("system"),
            Self::Agent => f.write_str("agent"),
        }
    }
}

/// Which observation tier a fact came from.
///
/// Ordered by confidence. Only the ordering drives behaviour — a lower tier may
/// never contradict a live higher one, it may only fill a gap — so the named
/// variants exist to be self-documenting and to let a plugin slot a new source
/// in at the right authority, not because each has its own rule.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Screen and bell guesses. Activity only, never structured content.
    Heuristic,
    /// Process and environment inspection.
    Process,
    /// omt-injected correlation, and its own OSC backchannel.
    Marker,
    /// The agent's own session file.
    Transcript,
    /// The agent's own hook system.
    Hook,
    /// The agent's own structured protocol.
    Protocol,
}

impl Tier {
    /// Whether a source at this tier may emit structured content.
    ///
    /// The tier ladder's whole point. A locale change or a version bump should
    /// cost attentiveness, never correctness — so a card a user might tap
    /// "Allow" on can only come from a source that was *told*, never from one
    /// that inferred it from pixels.
    #[must_use]
    pub const fn may_emit_structured_content(self) -> bool {
        matches!(self, Self::Transcript | Self::Hook | Self::Protocol)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_are_ordered_by_authority() {
        assert!(Role::Admin.satisfies(Role::Operator));
        assert!(Role::Operator.satisfies(Role::Viewer));
        assert!(!Role::Viewer.satisfies(Role::Operator));
        assert!(Role::Viewer.satisfies(Role::Viewer));
    }

    #[test]
    fn only_structured_tiers_may_emit_structure() {
        // This is the security property, so it gets an exhaustive test rather
        // than a spot check: a new tier added without thought fails here.
        for t in [Tier::Heuristic, Tier::Process, Tier::Marker] {
            assert!(!t.may_emit_structured_content(), "{t:?} must not");
        }
        for t in [Tier::Transcript, Tier::Hook, Tier::Protocol] {
            assert!(t.may_emit_structured_content(), "{t:?} must");
        }
    }

    #[test]
    fn tiers_are_ordered_by_confidence() {
        assert!(Tier::Protocol > Tier::Hook);
        assert!(Tier::Hook > Tier::Transcript);
        assert!(Tier::Transcript > Tier::Marker);
        assert!(Tier::Marker > Tier::Process);
        assert!(Tier::Process > Tier::Heuristic);
    }

    #[test]
    fn blocked_is_the_only_state_needing_a_human() {
        assert!(AgentState::Blocked { reason: BlockReason::Question, interaction: None }
            .needs_human());
        for s in [
            AgentState::Starting,
            AgentState::Idle,
            AgentState::Working { detail: None },
            AgentState::Exited { code: Some(0) },
            AgentState::Unknown,
        ] {
            assert!(!s.needs_human(), "{s:?}");
        }
    }

    #[test]
    fn agent_state_is_tagged_on_the_wire() {
        let json = serde_json::to_string(&AgentState::Working { detail: None }).expect("serialize");
        assert_eq!(json, r#"{"state":"working"}"#);
    }

    #[test]
    fn plugin_actors_retain_attribution() {
        let a = Actor::Plugin {
            plugin: "telegram".into(),
            on_behalf_of: Box::new(Actor::Local),
        };
        assert_eq!(a.to_string(), "plugin:telegram for local");
    }

    #[test]
    fn every_agent_kind_is_in_all() {
        // ALL is what adapters and tests enumerate; a variant missing from it
        // would silently never be covered.
        assert_eq!(AgentKind::ALL.len(), 11);
        for k in AgentKind::ALL {
            assert!(!k.as_str().is_empty());
        }
    }
}
