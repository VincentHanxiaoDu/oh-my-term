//! Why one result beat another, Is this agent stuck?, Comparing parallel runs, Timeline and digest, What is indexed.
//!
//! Split out of one 3478-line module, over P1's 2000-line limit.

use super::*;

// ---------------------------------------------------------------------------
// Why one result beat another — 20 §4
// ---------------------------------------------------------------------------

/// One candidate, as the caller describes it.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct CandidateIn {
    /// Its id.
    pub id: String,
    /// `command`, `output`, `agent_message` or `file_change`.
    pub kind: String,
    /// FTS5's bm25, negative and more-negative-is-better.
    pub bm25: f64,
    /// How many seconds ago.
    pub age_secs: u64,
    /// Whether it is in the workspace the search came from.
    #[serde(default)]
    pub in_scope: bool,
    /// The exit code, where there was one.
    #[serde(default)]
    pub exit: Option<i32>,
    /// Whether its session is still running.
    #[serde(default)]
    pub session_active: bool,
}

/// Input to `search.explain`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExplainSearchIn {
    /// The candidates to rank.
    pub candidates: Vec<CandidateIn>,
}

/// One factor's contribution.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct FactorOut {
    /// What it is called.
    pub name: String,
    /// What it multiplied by.
    pub value: f64,
    /// Why, in words.
    pub because: String,
}

/// One scored result.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ScoredOut {
    /// Which candidate.
    pub id: String,
    /// The product of every factor.
    pub score: f64,
    /// Each factor, in the order applied.
    pub factors: Vec<FactorOut>,
}

/// What `search.explain` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ExplainSearchOut {
    /// Every candidate, best first.
    pub results: Vec<ScoredOut>,
}

omt_catalog::capability! {
    /// Rank some candidates and show the arithmetic.
    pub struct SearchExplain;
    input  = ExplainSearchIn,
    output = ExplainSearchOut,
    decl = omt_catalog::Decl {
        name: "search.explain",
        group: "search",
        verb: "explain",
        title: "Why this result ranked here",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A transparent weighted product with no learning and no hidden state. Search over somebody's own history is where a wrong answer looks exactly like a right one, and a ranker that cannot explain itself gives them no way to notice.",
    },
}

/// Serves search explanation.
pub struct SearchHandler;

impl CapabilityHandler<SearchExplain> for SearchHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: ExplainSearchIn,
    ) -> Result<ExplainSearchOut, CapabilityError> {
        let candidates: Vec<omt_recall::ranking::Candidate> = input
            .candidates
            .into_iter()
            .map(|c| omt_recall::ranking::Candidate {
                id: c.id,
                kind: match c.kind.as_str() {
                    "output" => omt_recall::ranking::Kind::Output,
                    "agent_message" => omt_recall::ranking::Kind::AgentMessage,
                    "file_change" => omt_recall::ranking::Kind::FileChange,
                    _ => omt_recall::ranking::Kind::Command,
                },
                bm25: c.bm25,
                age_secs: c.age_secs,
                in_scope: c.in_scope,
                exit: c.exit,
                session_active: c.session_active,
            })
            .collect();
        Ok(ExplainSearchOut {
            results: omt_recall::ranking::rank(&candidates)
                .into_iter()
                .map(|s| ScoredOut {
                    id: s.id,
                    score: s.score,
                    factors: s
                        .factors
                        .into_iter()
                        .map(|f| FactorOut {
                            name: f.name,
                            value: f.value,
                            because: f.because,
                        })
                        .collect(),
                })
                .collect(),
        })
    }
}

// ---------------------------------------------------------------------------
// Is this agent stuck? — 20 §10
// ---------------------------------------------------------------------------

/// Input to `agent.attention`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct AttentionIn {
    /// How long it has been working, in seconds.
    #[serde(default)]
    pub working_secs: u64,
    /// The usual working time for this binding.
    #[serde(default)]
    pub baseline_working_secs: u64,
    /// How long since anything happened.
    #[serde(default)]
    pub silent_secs: u64,
    /// The usual gap between events.
    #[serde(default)]
    pub baseline_gap_secs: u64,
    /// Recent tool calls, canonicalised.
    #[serde(default)]
    pub recent_calls: Vec<String>,
    /// How many results in a row have failed.
    #[serde(default)]
    pub consecutive_failures: u32,
    /// The most recent failure.
    #[serde(default)]
    pub last_failure: Option<String>,
    /// Tokens spent since a file last changed.
    #[serde(default)]
    pub tokens_since_file_change: u64,
    /// How long since a file last changed.
    #[serde(default)]
    pub since_file_change_secs: u64,
}

/// What `agent.attention` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct AttentionOut {
    /// Whether anything fired.
    pub needs_attention: bool,
    /// Every reason, never just the first.
    pub reasons: Vec<String>,
    /// `weak` or `strong`.
    pub confidence: Option<String>,
    /// A line somebody can act on.
    pub detail: Option<String>,
}

omt_catalog::capability! {
    /// Whether an agent has stopped making progress.
    pub struct AgentAttention;
    input  = AttentionIn,
    output = AttentionOut,
    decl = omt_catalog::Decl {
        name: "agent.attention",
        group: "agent",
        verb: "attention",
        title: "Is this agent stuck",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Baselines are per binding and learned from its own history: thirty seconds is an eternity for a linter and a blink for a test suite, so a fixed threshold is wrong for everybody.",
    },
}

/// Serves attention detection.
pub struct AttentionHandler;

impl CapabilityHandler<AgentAttention> for AttentionHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: AttentionIn,
    ) -> Result<AttentionOut, CapabilityError> {
        let observation = omt_recall::stuck::Observation {
            working_secs: input.working_secs,
            baseline_working_secs: input.baseline_working_secs,
            silent_secs: input.silent_secs,
            baseline_gap_secs: input.baseline_gap_secs,
            recent_calls: input.recent_calls.into_iter().collect(),
            consecutive_failures: input.consecutive_failures,
            last_failure: input.last_failure,
            tokens_since_file_change: input.tokens_since_file_change,
            since_file_change_secs: input.since_file_change_secs,
        };
        let signal =
            omt_recall::stuck::examine(&observation, &omt_recall::stuck::Thresholds::default());
        Ok(match signal {
            None => AttentionOut {
                needs_attention: false,
                reasons: Vec::new(),
                confidence: None,
                detail: None,
            },
            Some(s) => AttentionOut {
                needs_attention: true,
                reasons: s
                    .reasons
                    .iter()
                    .map(|r| {
                        serde_json::to_value(r)
                            .ok()
                            .and_then(|v| {
                                v.get("reason").and_then(|x| x.as_str()).map(str::to_owned)
                            })
                            .unwrap_or_else(|| "unknown".to_owned())
                    })
                    .collect(),
                confidence: Some(format!("{:?}", s.confidence).to_lowercase()),
                detail: Some(s.detail),
            },
        })
    }
}

// ---------------------------------------------------------------------------
// Comparing parallel runs — 20 §9
// ---------------------------------------------------------------------------

/// One run, as the caller describes it.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct RunIn {
    /// Which session.
    pub session: String,
    /// Its branch.
    #[serde(default)]
    pub branch: Option<String>,
    /// The resolved base it is measured against.
    #[serde(default)]
    pub base_ref: Option<String>,
    /// Which agent.
    #[serde(default)]
    pub agent: Option<String>,
    /// How long, in seconds.
    #[serde(default)]
    pub elapsed_secs: u64,
    /// Files git reports changed.
    #[serde(default)]
    pub files_from_git: Vec<String>,
    /// Files the agent said it changed.
    #[serde(default)]
    pub files_from_agent: Vec<String>,
    /// Lines added.
    #[serde(default)]
    pub insertions: u32,
    /// Lines removed.
    #[serde(default)]
    pub deletions: u32,
    /// Turns.
    #[serde(default)]
    pub turns: u32,
    /// Times it stopped to ask.
    #[serde(default)]
    pub interactions: u32,
    /// How many went unanswered.
    #[serde(default)]
    pub interactions_timed_out: u32,
    /// Commands that exited non-zero.
    #[serde(default)]
    pub failed_blocks: u32,
    /// Tokens, agent-reported.
    #[serde(default)]
    pub tokens: Option<u64>,
    /// The agent's own summary.
    #[serde(default)]
    pub summary: Option<String>,
}

/// Input to `agent.compare`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct CompareIn {
    /// The runs to put side by side.
    pub runs: Vec<RunIn>,
}

/// One row of the comparison.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RowOut {
    /// Which session.
    pub session: String,
    /// Its branch.
    pub branch: Option<String>,
    /// What it was measured against.
    pub base_ref: Option<String>,
    /// Which agent.
    pub agent: Option<String>,
    /// How long, in seconds.
    pub elapsed_secs: u64,
    /// Every file either record mentions.
    pub files_touched: Vec<String>,
    /// Files the two records disagree about.
    pub files_disagreement: Vec<String>,
    /// Lines added.
    pub insertions: u32,
    /// Lines removed.
    pub deletions: u32,
    /// Turns.
    pub turns: u32,
    /// Times it stopped to ask.
    pub interactions: u32,
    /// How many went unanswered.
    pub interactions_timed_out: u32,
    /// Commands that exited non-zero.
    pub failed_blocks: u32,
    /// Tokens, agent-reported. Absent means unreported, not zero.
    pub tokens: Option<u64>,
    /// The agent's own summary, verbatim.
    pub summary: Option<String>,
}

/// What `agent.compare` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct CompareOut {
    /// One row per run, in the order given.
    pub rows: Vec<RowOut>,
    /// What is not comparable, and why.
    pub caveats: Vec<String>,
}

omt_catalog::capability! {
    /// Put parallel agent runs side by side.
    pub struct AgentCompare;
    input  = CompareIn,
    output = CompareOut,
    decl = omt_catalog::Decl {
        name: "agent.compare",
        group: "agent",
        verb: "compare",
        title: "Compare parallel runs",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Presents comparable facts; never ranks, scores or recommends. Judging requires understanding the code, and a plausible wrong recommendation about which branch to keep costs somebody real work.",
    },
}

impl CapabilityHandler<AgentCompare> for AttentionHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: CompareIn,
    ) -> Result<CompareOut, CapabilityError> {
        let runs: Vec<omt_recall::compare::Run> = input
            .runs
            .into_iter()
            .map(|r| omt_recall::compare::Run {
                session: r.session,
                branch: r.branch,
                base_ref: r.base_ref,
                agent: r.agent,
                elapsed_secs: r.elapsed_secs,
                files_from_git: r.files_from_git,
                files_from_agent: r.files_from_agent,
                insertions: r.insertions,
                deletions: r.deletions,
                turns: r.turns,
                interactions: r.interactions,
                interactions_timed_out: r.interactions_timed_out,
                failed_blocks: r.failed_blocks,
                tokens: r.tokens,
                summary: r.summary,
            })
            .collect();
        let comparison = omt_recall::compare::compare(&runs);
        Ok(CompareOut {
            rows: comparison
                .rows
                .into_iter()
                .map(|r| RowOut {
                    session: r.session,
                    branch: r.branch,
                    base_ref: r.base_ref,
                    agent: r.agent,
                    elapsed_secs: r.elapsed_secs,
                    files_touched: r.files_touched,
                    files_disagreement: r.files_disagreement,
                    insertions: r.insertions,
                    deletions: r.deletions,
                    turns: r.turns,
                    interactions: r.interactions,
                    interactions_timed_out: r.interactions_timed_out,
                    failed_blocks: r.failed_blocks,
                    tokens: r.tokens,
                    summary: r.summary,
                })
                .collect(),
            caveats: comparison.caveats,
        })
    }
}

// ---------------------------------------------------------------------------
// Timeline and digest — 20 §8
// ---------------------------------------------------------------------------

/// One event, as the caller describes it.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct TimelineEventIn {
    /// Its per-session sequence number.
    pub seq: u64,
    /// When, in seconds since the epoch.
    pub at: u64,
    /// `user_message`, `assistant_message`, `tool_call`, `file_changed`,
    /// `interaction` or `failed_block`.
    pub kind: String,
    /// What it was, in the agent's own words.
    pub label: String,
}

/// Input to `session.timeline`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct TimelineIn {
    /// The events to assemble.
    pub events: Vec<TimelineEventIn>,
}

/// One assembled line.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct TimelineEntryOut {
    /// Where it starts.
    pub seq: u64,
    /// When.
    pub at: u64,
    /// What kind.
    pub kind: String,
    /// What it was.
    pub label: String,
    /// How many identical events collapsed into it.
    pub count: u32,
    /// From the first to the last, in seconds.
    pub spanned_secs: u64,
}

/// What `session.timeline` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct TimelineOut {
    /// The lines, in sequence order.
    pub entries: Vec<TimelineEntryOut>,
    /// How many events went in.
    pub events: u32,
    /// How many tool calls.
    pub tool_calls: u32,
    /// How many files changed.
    pub files_changed: u32,
    /// How many questions were asked.
    pub interactions: u32,
    /// How many commands failed.
    pub failed_blocks: u32,
    /// A short account for somebody who was away.
    pub digest: String,
}

omt_catalog::capability! {
    /// What happened in a session, assembled and collapsed.
    pub struct SessionTimeline;
    input  = TimelineIn,
    output = TimelineOut,
    decl = omt_catalog::Decl {
        name: "session.timeline",
        group: "session",
        verb: "timeline",
        title: "What happened here",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Derived from the event log, so it can be rebuilt at any time. Only consecutive identical calls collapse, and the canonicaliser strips whitespace and nothing else — one that ignored arguments would collapse `rm -rf build` and `rm -rf /`.",
    },
}

impl CapabilityHandler<SessionTimeline> for AttentionHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: TimelineIn,
    ) -> Result<TimelineOut, CapabilityError> {
        let events: Vec<omt_recall::timeline::Event> = input
            .events
            .into_iter()
            .map(|e| omt_recall::timeline::Event {
                seq: e.seq,
                at: e.at,
                kind: match e.kind.as_str() {
                    "user_message" => omt_recall::timeline::Kind::UserMessage,
                    "assistant_message" => omt_recall::timeline::Kind::AssistantMessage,
                    "file_changed" => omt_recall::timeline::Kind::FileChanged,
                    "interaction" => omt_recall::timeline::Kind::Interaction,
                    "failed_block" => omt_recall::timeline::Kind::FailedBlock,
                    _ => omt_recall::timeline::Kind::ToolCall,
                },
                label: e.label,
            })
            .collect();
        let timeline = omt_recall::timeline::assemble(&events);
        let digest = omt_recall::timeline::digest(&timeline);
        Ok(TimelineOut {
            entries: timeline
                .entries
                .iter()
                .map(|e| TimelineEntryOut {
                    seq: e.seq,
                    at: e.at,
                    kind: format!("{:?}", e.kind).to_lowercase(),
                    label: e.label.clone(),
                    count: e.count,
                    spanned_secs: e.spanned_secs,
                })
                .collect(),
            events: timeline.stats.events,
            tool_calls: timeline.stats.tool_calls,
            files_changed: timeline.stats.files_changed,
            interactions: timeline.stats.interactions,
            failed_blocks: timeline.stats.failed_blocks,
            digest,
        })
    }
}

// ---------------------------------------------------------------------------
// What is indexed — 20 §2
// ---------------------------------------------------------------------------

/// Input to `recall.policy`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct PolicyIn {
    /// Whether the agent's reasoning is indexed.
    #[serde(default)]
    pub index_reasoning: bool,
}

/// What omt does with one source.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct SourcePolicy {
    /// Which source.
    pub source: String,
    /// `full`, `capped`, `windowed`, `structured_only` or `excluded`.
    pub treatment: String,
    /// The cap or window, in bytes, where there is one.
    pub bytes: Option<usize>,
}

/// What `recall.policy` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PolicyOut {
    /// Every source and what happens to it.
    pub sources: Vec<SourcePolicy>,
    /// Which fields of a tool call are indexed.
    pub indexed_tool_fields: Vec<String>,
}

omt_catalog::capability! {
    /// What goes into the search index, and what does not.
    pub struct RecallPolicy;
    input  = PolicyIn,
    output = PolicyOut,
    decl = omt_catalog::Decl {
        name: "recall.policy",
        group: "recall",
        verb: "policy",
        title: "What is indexed",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A search index nobody can enumerate is one nobody can reason about. Reasoning is excluded by default: it is the most privacy-sensitive text an agent produces.",
    },
}

impl CapabilityHandler<RecallPolicy> for AttentionHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: PolicyIn,
    ) -> Result<PolicyOut, CapabilityError> {
        use omt_recall::policy::{Source, Treatment};
        let settings = omt_recall::policy::Settings {
            index_reasoning: input.index_reasoning,
            ..omt_recall::policy::Settings::default()
        };
        let sources = [
            Source::BlockCommand,
            Source::BlockOutput,
            Source::UserMessage,
            Source::AssistantText,
            Source::Reasoning,
            Source::ToolCallInput,
            Source::ToolResultOutput,
            Source::FileChanged,
            Source::Interaction,
        ];
        Ok(PolicyOut {
            sources: sources
                .into_iter()
                .map(|source| {
                    let t = omt_recall::policy::treatment(source, &settings);
                    SourcePolicy {
                        source: format!("{source:?}").to_lowercase(),
                        treatment: match t {
                            Treatment::Full => "full",
                            Treatment::Capped { .. } => "capped",
                            Treatment::Windowed { .. } => "windowed",
                            Treatment::StructuredOnly => "structured_only",
                            Treatment::Excluded => "excluded",
                        }
                        .to_owned(),
                        bytes: match t {
                            Treatment::Capped { bytes } => Some(bytes),
                            Treatment::Windowed { head, tail } => Some(head + tail),
                            _ => None,
                        },
                    }
                })
                .collect(),
            indexed_tool_fields: omt_recall::policy::INDEXED_TOOL_FIELDS
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
        })
    }
}
