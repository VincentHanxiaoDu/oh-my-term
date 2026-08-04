//! The normalized agent event stream.
//!
//! Everything omt observes about an agent, in one vocabulary that does not
//! depend on which agent produced it or which tier saw it. A surface rendering
//! a conversation consumes this and needs no per-agent code — which is the
//! whole reason it exists.

use omt_types::{AgentKind, BindingId, InteractionId, Seq, SessionId, Tier, Timestamp};
use serde::{Deserialize, Serialize};

/// One observation about an agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AgentEvent {
    /// Which session.
    pub session: SessionId,
    /// Which agent occupancy — so a late event from a replaced agent is
    /// recognisable rather than attributed to its successor.
    pub binding: BindingId,
    /// Which agent.
    pub agent: AgentKind,
    /// Its version, where a source could say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_version: Option<String>,
    /// The agent's own session id, where it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session: Option<String>,
    /// Which thread — a subagent's work nests under the tool call that spawned
    /// it rather than appearing inline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<ThreadRef>,
    /// Position in the session's stream.
    pub seq: Seq,
    /// When it was observed.
    pub ts: Timestamp,
    /// Which tier produced it. Doubles as the confidence, since the merge rule
    /// is that a lower tier may fill a gap but never contradict a live higher
    /// one.
    pub tier: Tier,
    /// What happened.
    pub payload: AgentPayload,
}

impl AgentEvent {
    /// Whether this event is structurally permitted from its tier.
    ///
    /// The tier ladder's guard, checked rather than assumed: a heuristic source
    /// may report that something is happening, never *what*. A locale change or
    /// a version bump should cost attentiveness, never correctness — a card a
    /// user might tap "Allow" on must come from a source that was told, not one
    /// that read pixels.
    #[must_use]
    pub fn tier_permits_payload(&self) -> bool {
        if self.payload.is_structured() {
            self.tier.may_emit_structured_content()
        } else {
            true
        }
    }
}

/// Which conversation thread an event belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ThreadRef {
    /// The thread's own id.
    pub id: String,
    /// The tool call that spawned it, for a subagent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Whether this is a subagent rather than the main thread.
    pub is_subagent: bool,
    /// A label, where the agent gives one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// What an agent event says.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentPayload {
    // ---------- lifecycle ----------
    /// A session began.
    SessionStart {
        /// Why it began.
        reason: StartReason,
        /// The model in use, where known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    /// A session ended.
    SessionEnd {
        /// Why.
        reason: String,
    },
    /// What the agent can do — its tools, commands and models.
    Capabilities {
        /// Tool names.
        tools: Vec<String>,
        /// Slash commands, resolved by the agent itself rather than guessed.
        slash_commands: Vec<SlashCommand>,
        /// Models it offers.
        models: Vec<String>,
    },

    // ---------- turn state ----------
    /// A turn began.
    TurnStart {
        /// The turn's id, where the agent gives one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn: Option<String>,
        /// What started it.
        trigger: TurnTrigger,
    },
    /// A turn ended.
    TurnEnd {
        /// The turn.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn: Option<String>,
        /// How it ended.
        outcome: TurnOutcome,
    },
    /// Token usage, **as the agent reported it**.
    ///
    /// omt never computes a price: prices change, plans differ, and a wrong
    /// number is worse than none.
    Usage {
        /// Input tokens.
        input: u64,
        /// Output tokens.
        output: u64,
        /// Tokens read from cache.
        cache_read: u64,
        /// Tokens written to cache.
        cache_write: u64,
        /// Cost, only if the agent itself stated one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost_usd: Option<f64>,
    },
    /// A rate limit was reported.
    RateLimit {
        /// What the agent said.
        status: String,
        /// When it resets, where stated.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resets_at: Option<Timestamp>,
    },
    /// The context was compacted — and this is where a transcript legitimately
    /// loses history.
    Compaction {
        /// Before or after.
        phase: CompactionPhase,
        /// What triggered it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trigger: Option<String>,
    },

    // ---------- content ----------
    /// The user said something.
    UserMessage {
        /// The text.
        text: String,
        /// Where it came from, so a synthetic turn is not mistaken for a human
        /// one on any surface.
        origin: MessageOrigin,
    },
    /// The agent said something.
    AssistantText {
        /// The text, or a fragment of it.
        text: String,
        /// Whether more of this message is coming.
        ///
        /// Whether a surface ever sees `true` depends on the tier: a protocol
        /// streams chunks, a transcript is written a message at a time. Both
        /// are correct observations of the same thing.
        partial: bool,
    },
    /// The agent's reasoning.
    Reasoning {
        /// The text, or a fragment.
        text: String,
        /// Whether more is coming.
        partial: bool,
    },

    // ---------- tools ----------
    /// A tool was called.
    ToolCall {
        /// The call's id, which correlates it with its result.
        call: String,
        /// The tool.
        name: String,
        /// Its arguments, verbatim.
        input: serde_json::Value,
    },
    /// A tool finished.
    ToolResult {
        /// Which call.
        call: String,
        /// What it produced.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<serde_json::Value>,
        /// What went wrong, if anything.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        /// How long it took.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
    },
    /// The agent is planning.
    Plan {
        /// The steps, re-rendered in place on each revision.
        steps: Vec<PlanStep>,
    },
    /// A file changed.
    FileChanged {
        /// Which file.
        path: String,
        /// What happened to it.
        change: FileChange,
        /// The tool that did it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool: Option<String>,
    },

    // ---------- interactions ----------
    /// An interaction was raised.
    InteractionRaised {
        /// Which one. The object itself travels in its own event kind; this
        /// references it rather than duplicating it.
        interaction: InteractionId,
    },
    /// An interaction reached a terminal state.
    InteractionResolved {
        /// Which one.
        interaction: InteractionId,
    },

    // ---------- queue ----------
    /// The agent's message queue changed.
    QueueChanged {
        /// What happened.
        op: QueueOp,
        /// The text involved.
        text: String,
        /// The queue as it now stands.
        pending: Vec<String>,
    },

    // ---------- fallback ----------
    /// The agent said something omt does not model.
    Notification {
        /// Its own category.
        kind: String,
        /// What it said.
        message: String,
    },
    /// A heuristic guess, and the *only* payload a tier-0 source may emit.
    Activity {
        /// The guess.
        state: ActivityGuess,
    },
}

impl AgentPayload {
    /// Whether this payload asserts structure a heuristic source could not know.
    #[must_use]
    pub const fn is_structured(&self) -> bool {
        !matches!(self, Self::Activity { .. })
    }
}

/// A coarse activity guess. Deliberately the only thing screen heuristics can
/// say — there is no code path from screen text to a tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActivityGuess {
    /// Something is happening.
    Busy,
    /// Nothing is happening.
    Idle,
    /// Something appears to want a human.
    NeedsAttention,
    /// Cannot tell.
    Unknown,
}

/// Why a session started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StartReason {
    /// Fresh start.
    Startup,
    /// Resumed a previous session.
    Resume,
    /// After a clear.
    Clear,
    /// After a compaction.
    Compact,
    /// Forked from another session.
    Fork,
}

/// What started a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TurnTrigger {
    /// A person typed it.
    Human,
    /// It came off the queue.
    Queued,
    /// A remote client sent it.
    Remote,
    /// A hook or automation.
    Hook,
}

/// How a turn ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TurnOutcome {
    /// It finished.
    Completed,
    /// It was interrupted.
    Aborted,
    /// It failed.
    Error,
}

/// Which side of a compaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompactionPhase {
    /// About to compact.
    Before,
    /// Just compacted.
    After,
}

/// Where a user message came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MessageOrigin {
    /// Typed by a person at the terminal.
    Human,
    /// Delivered from the queue.
    Queue,
    /// Sent by a remote client.
    Remote,
    /// Typed by omt on someone's behalf, and always labelled as such.
    Synthetic,
}

/// What happened to a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileChange {
    /// It appeared.
    Created,
    /// It changed.
    Modified,
    /// It went away.
    Deleted,
}

/// A queue operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueueOp {
    /// Added.
    Enqueue,
    /// Taken out without being run.
    Remove,
    /// Taken out and run.
    Consume,
}

/// One step of a plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlanStep {
    /// What it is.
    pub title: String,
    /// Where it is.
    pub status: PlanStepStatus,
}

/// A plan step's status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanStepStatus {
    /// Not started.
    Pending,
    /// In progress.
    InProgress,
    /// Done.
    Completed,
}

/// A slash command the agent itself resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SlashCommand {
    /// Its name, without the slash.
    pub name: String,
    /// What it does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// What arguments it takes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argument_hint: Option<String>,
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn event(tier: Tier, payload: AgentPayload) -> AgentEvent {
        AgentEvent {
            session: SessionId::new(),
            binding: BindingId::new(),
            agent: AgentKind::ClaudeCode,
            agent_version: None,
            agent_session: None,
            thread: None,
            seq: Seq::new(1),
            ts: Timestamp::UNIX_EPOCH,
            tier,
            payload,
        }
    }

    #[test]
    fn a_heuristic_source_may_not_emit_structure() {
        // The security property: there is no path from screen text to a tool
        // call, because a wrong card is worse than a missing one.
        let e = event(
            Tier::Heuristic,
            AgentPayload::ToolCall {
                call: "1".into(),
                name: "Bash".into(),
                input: serde_json::json!({}),
            },
        );
        assert!(!e.tier_permits_payload());
    }

    #[test]
    fn a_heuristic_source_may_emit_an_activity_guess() {
        let e = event(
            Tier::Heuristic,
            AgentPayload::Activity {
                state: ActivityGuess::Busy,
            },
        );
        assert!(e.tier_permits_payload());
    }

    #[test]
    fn structured_tiers_may_emit_structure() {
        for tier in [Tier::Transcript, Tier::Hook, Tier::Protocol] {
            let e = event(
                tier,
                AgentPayload::ToolCall {
                    call: "1".into(),
                    name: "Bash".into(),
                    input: serde_json::json!({}),
                },
            );
            assert!(e.tier_permits_payload(), "{tier:?} was told, so it may say");
        }
    }

    #[test]
    fn process_and_marker_tiers_may_not_emit_structure() {
        // They identify *which* agent, not *what* it is doing.
        for tier in [Tier::Process, Tier::Marker] {
            let e = event(tier, AgentPayload::Plan { steps: vec![] });
            assert!(!e.tier_permits_payload(), "{tier:?} must not");
        }
    }

    #[test]
    fn activity_is_the_only_unstructured_payload() {
        assert!(
            !AgentPayload::Activity {
                state: ActivityGuess::Idle
            }
            .is_structured()
        );
        assert!(
            AgentPayload::Notification {
                kind: "x".into(),
                message: "y".into()
            }
            .is_structured()
        );
    }

    #[test]
    fn partial_marks_a_streamed_fragment() {
        let e = AgentPayload::AssistantText {
            text: "ance".into(),
            partial: true,
        };
        let json = serde_json::to_string(&e).expect("serialize");
        assert!(json.contains(r#""partial":true"#), "{json}");
    }

    #[test]
    fn payloads_are_tagged_and_round_trip() {
        let p = AgentPayload::Usage {
            input: 10,
            output: 20,
            cache_read: 5,
            cache_write: 0,
            cost_usd: None,
        };
        let json = serde_json::to_string(&p).expect("serialize");
        assert!(json.starts_with(r#"{"type":"usage""#), "{json}");
        let back: AgentPayload = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(p, back);
    }

    #[test]
    fn cost_is_absent_unless_the_agent_stated_one() {
        // omt never computes a price; a wrong number is worse than none.
        let p = AgentPayload::Usage {
            input: 1,
            output: 1,
            cache_read: 0,
            cache_write: 0,
            cost_usd: None,
        };
        let json = serde_json::to_string(&p).expect("serialize");
        assert!(!json.contains("cost_usd"), "{json}");
    }

    #[test]
    fn a_synthetic_message_is_distinguishable_from_a_human_one() {
        // So no surface can present omt's typing as the user's.
        let p = AgentPayload::UserMessage {
            text: "1".into(),
            origin: MessageOrigin::Synthetic,
        };
        let json = serde_json::to_string(&p).expect("serialize");
        assert!(json.contains("synthetic"), "{json}");
    }

    #[test]
    fn a_whole_event_round_trips() {
        let e = event(
            Tier::Hook,
            AgentPayload::TurnStart {
                turn: None,
                trigger: TurnTrigger::Human,
            },
        );
        let json = serde_json::to_string(&e).expect("serialize");
        let back: AgentEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(e, back);
    }
}
