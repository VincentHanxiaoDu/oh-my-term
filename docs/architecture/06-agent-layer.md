# Agent Layer

The design of `omt-agent` (pipeline and state) and `omt-agent-adapters`
(per-agent knowledge). This is the part of omt that does not exist anywhere
else, so it gets the most detail.

Source material: [`docs/research/agent-clis.md`](../research/agent-clis.md) for
the per-CLI facts, and [`docs/research/another tool.md`](../research/another tool.md) §3 for
prior art.

---

## 1. What this layer must deliver

1. **Which agent** is running in a session, and when that binding starts and
   ends.
2. **What state** it is in — enough to answer "does this need me?" across
   twenty sessions on four machines.
3. **Structured interactions** — question cards, permission requests, plan
   reviews — surfaced as data that any surface can render and *answer*.
4. **Ancillary semantics** the agents already expose and nobody surfaces: the
   message queue, slash commands, token usage, subagent trees, compaction.

Requirements 1 and 2 are what another tool does. Requirements 3 and 4 are why omt
exists, and they drive every design choice below.

---

## 2. The two-axis model

Detection has two independent axes that must not be conflated:

- **Binding**: *which* agent occupies the session, with a lifetime.
- **State**: what that agent is doing right now.

another tool conflates them into one polling loop; omt separates them because binding
changes are rare (seconds to hours) and state changes are frequent (hundreds of
milliseconds), and because a binding carries an identity (the agent's own
session id) that everything else keys off.

```rust
pub struct AgentBinding {
    pub id: BindingId,
    pub session: SessionId,
    pub kind: AgentKind,
    pub version: Option<String>,
    /// The agent's own session identifier, once known.
    pub agent_session: Option<AgentSessionId>,
    pub cwd: PathBuf,
    pub started_at: Timestamp,
    pub ended_at: Option<Timestamp>,
    /// Which sources are currently live for this binding.
    pub sources: Vec<SourceStatus>,
}
```

A binding ends when the foreground process group changes away from the agent, or
when a lifecycle source reports session end. Retained evidence (OSC titles,
tailed files, hook correlation) is dropped on binding end so a new agent can
never inherit the previous one's state — the same discipline another tool applies in
`clear_retained()`, and for the same reason.

---

## 3. Source model

Everything that can tell omt something about an agent implements one trait.

```rust
#[async_trait]
pub trait EventSource: Send + Sync {
    fn id(&self) -> SourceId;
    /// Confidence tier; see §4. Fixed per source, not per event.
    fn tier(&self) -> Tier;
    /// Which agents this source can serve.
    fn supports(&self, kind: AgentKind) -> bool;

    /// Begin observing a binding. The source pushes normalized events and
    /// reports its own health; it must never block and must never write to the
    /// PTY.
    async fn attach(&self, binding: &AgentBinding, sink: EventSink) -> Result<SourceHandle, SourceError>;
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Heuristic = 0,   // PTY screen/bell guesses
    Process   = 1,   // process + environ inspection
    Marker    = 2,   // omt-injected env correlation, OSC backchannel
    Transcript= 3,   // agent's own session file
    Hook      = 4,   // agent's own hook system
    Protocol  = 5,   // agent's own structured protocol (ACP, app-server, REST/SSE)
}
```

Implemented sources:

| Source | Tier | Notes |
|---|---|---|
| `ProcessProbe` | Process | foreground pgid → argv + environ; env markers beat exe names because four agents ship as Node bundles |
| `OscBackchannel` | Marker | intercepts OSC emitted by `omt-hook` via `terminalSequence`, and omt's own OSC namespace |
| `TranscriptTail` | Transcript | per-agent readers; watches the *directory* so new sessions are caught |
| `HookBridge` | Hook | receives normalized payloads from the `omt-hook` binary over the local socket |
| `AcpClient` | Protocol | generic ACP JSON-RPC client — covers opencode, Gemini, Goose, Qwen at once |
| `AppServerClient` | Protocol | Codex `app-server` |
| `OpencodeHttp` | Protocol | opencode `serve` REST + SSE |
| `PtyHeuristics` | Heuristic | activity only; structurally incapable of emitting structured content (§6) |

Sources are registered, not hard-coded (principle P2). A plugin can add one.

---

## 4. Merging: confidence tiers, not voting

One state machine per binding consumes all sources.

**Rules.**

1. Every incoming event carries its source tier. State transitions are applied
   from the highest tier that has spoken within its *freshness window*.
2. Freshness windows are per tier: Protocol/Hook 30 s, Transcript 15 s,
   Marker 10 s, Process 10 s, Heuristic 3 s. A tier that goes silent past its
   window stops suppressing lower tiers.
3. **A lower tier may never contradict a live higher tier.** It may only fill a
   gap. This is the rule that makes heuristics safe to keep enabled.
4. When a Protocol or Hook source is *authoritative* for an agent — meaning it
   reports the full lifecycle, not just session identity — the heuristic source
   is suspended entirely for that binding. Two sources of truth for the same
   fact is a bug, not redundancy. (another tool reaches the same conclusion with
   `full_lifecycle_hook_authority`.)
5. Transitions to `Idle` are debounced (three confirmations within 700 ms) to
   avoid flicker between an agent's tool calls; transitions to `Blocked` are
   *not* debounced, because latency there is the whole product.

**Observable state.**

```rust
pub enum AgentState {
    Starting,
    Idle,
    Working { since: Timestamp, detail: Option<String> },
    /// Needs a human. Always carries the reason, and an interaction id when
    /// the block is structured rather than merely observed.
    Blocked { since: Timestamp, reason: BlockReason, interaction: Option<InteractionId> },
    Exited { code: Option<i32> },
    Unknown,
}

pub enum BlockReason { Question, Permission, PlanReview, Elicitation, Input, Unspecified }
```

`Blocked { interaction: Some(_) }` is answerable from anywhere.
`Blocked { interaction: None }` means omt can see that something needs a human
but cannot render it — the phone shows "needs you", and the user opens the
terminal view. That degradation is explicit and visible, never silent.

### Explainability is a feature

`agent.explain` returns the full decision: every source, its tier, its last
event, its freshness, which one won, and why. another tool's `agent explain` is the
single best idea in that codebase and omt copies the *idea* (not the code).
Without it, a mis-detection is unfalsifiable and every bug report is useless.

---

## 5. Interactions — the flagship path

An `Interaction` is a request from an agent for a human decision, promoted to a
first-class, addressable, resolvable object.

```rust
pub struct Interaction {
    pub id: InteractionId,
    pub session: SessionId,
    pub binding: BindingId,
    pub kind: InteractionKind,
    pub opened_at: Timestamp,
    /// Deadline after which omt must answer or release, if the mechanism has one.
    pub expires_at: Option<Timestamp>,
    pub state: InteractionState,
    /// How an answer gets back to the agent.
    pub responder: ResponderRef,
}

pub enum InteractionState {
    Open,
    Resolving { by: Actor, at: Timestamp },
    Resolved  { by: Actor, at: Timestamp, response: InteractionResponse },
    Cancelled { reason: CancelReason },   // agent withdrew, timed out, or session died
}

pub enum InteractionKind {
    /// Claude Code AskUserQuestion; ACP/MCP elicitation with a choice schema.
    Choice { questions: Vec<ChoiceQuestion> },
    Permission {
        tool: String,
        input: serde_json::Value,
        command: Option<String>,
        diff: Option<UnifiedDiff>,
        options: Vec<PermissionOption>,
    },
    PlanReview { plan: String },
    Text { prompt: String, placeholder: Option<String>, multiline: bool },
}

pub struct ChoiceQuestion {
    pub question: String,
    /// Short tab label, ~12 chars — Claude Code's `header`.
    pub header: String,
    pub multi_select: bool,
    pub options: Vec<ChoiceOption>,   // { label, description }
    /// Every surface offers free text in addition to the options, because the
    /// native clients do.
    pub allow_free_text: bool,
}
```

`Choice` maps 1:1 onto the verified `AskUserQuestion` schema, so there is no
lossy translation on the flagship case.

### 5.1 Lifecycle

```
agent emits ──► source normalizes ──► ledger opens Interaction ──► broadcast
                                                │
                     ┌──────────────────────────┼──────────────────────────┐
                     ▼                          ▼                          ▼
                 TUI card                  web card (phone)          API client
                     └──────────── interaction.resolve ─────────────┘
                                            │
                            ledger CAS: Open → Resolving → Resolved
                                            │
                                    responder injects
                                            │
                                  agent proceeds; broadcast
```

**Exactly-once resolution** is enforced by a compare-and-swap on
`InteractionState` inside the ledger. The loser of a race gets
`conflict` with the winning actor's identity, and every surface immediately
re-renders the card as answered-by-someone-else. This is the concrete answer to
"the phone and the TUI both answer at once" (see
[12 — Collaboration](12-collaboration.md)).

### 5.2 Responders — how the answer gets back

```rust
#[async_trait]
pub trait Responder: Send + Sync {
    fn fidelity(&self) -> Fidelity;   // Native | Synthetic
    async fn respond(&self, i: &Interaction, r: &InteractionResponse) -> Result<(), RespondError>;
}
```

| Mechanism | Agents | Fidelity |
|---|---|---|
| Hook decision on a deferred `PreToolUse` | Claude Code (and Codex/Cursor/Gemini for permissions) | Native |
| ACP `session/request_permission` response | opencode, Gemini, Goose, Qwen | Native |
| opencode plugin `permission.ask` reply | opencode | Native |
| app-server approval response | Codex | Native |
| **Synthesized keystrokes into the PTY** | anything else | Synthetic |

The synthetic responder exists under a precise rule, stated in
[D3](decisions.md#d3--synthetic-input-is-bounded-by-state-dependence-not-by-tool-danger):
**the bound is state dependence, not tool danger.**

```rust
pub enum StateDependence {
    /// The answer is the same regardless of the agent's internal UI state:
    /// typing `1`/`2`/`3`, `y`/`n`, free text, or a line-oriented CLI's stdin.
    Independent,
    /// Producing the answer requires knowing where a highlight bar currently
    /// sits — i.e. state omt inferred from the screen.
    Inferred,
}
```

- `Independent` is allowed and enabled by default. Writing `y\n` to Aider is not
  a hack; stdin is its documented input channel. A TUI that accepts `1`/`2`/`3`
  is equally safe, because no inference step exists.
- `Inferred` — counting arrow keys against a cursor position derived from the
  screen — is **disabled by default**. A version bump, locale change or resize
  invalidates the inference, omt selects the *wrong* option, and on a phone the
  user cannot see that it went wrong. Enabling it is per-agent, explicit, and
  labelled experimental everywhere it surfaces.
- Adapters must discover whether a prompt offers a position-independent form and
  prefer it. That discovery is a testable property of the adapter.
- It is **never** used when a native responder is available.
- Every synthetic response is tagged `fidelity: synthetic` in the event stream
  and visibly attributed as omt-typed on every surface.
- It requires the writer token, and it is serialized with human input.

Note what this rule deliberately does *not* say: it does not exempt "dangerous"
tools. Per [D1](decisions.md#d1--omt-adds-no-policy-layer-over-an-agents-permission-semantics),
omt does not classify an agent's operations by danger — that is the agent's own
permission model's job, and duplicating it would create two mental models and
false confidence. another tool's `agent.prompt` (type text, send Enter 300 ms later to
survive paste debouncing) is the prior art here, and its fragility comes from
exactly the inference step this rule forbids.

### 5.3 The deferral mechanism (and its risk)

The highest-value path is Claude Code's `PreToolUse` hook returning

```json
{"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "defer"}}
```

for `tool_name == "AskUserQuestion"`. The hook payload contains `tool_input`
verbatim — the exact `questions` array — and deferring parks the call so a
remote client can answer it.

**This is an unverified assumption and it gates the flagship feature.** Two
things are unknown: whether `defer` parks the call long enough for a human
round-trip, and what its timeout is. Therefore:

1. A spike change (`spike-defer-semantics`) runs first and measures it. Nothing
   else depends on the answer until it lands.
2. The design has a fallback that does not require deferral at all, and it is
   built regardless: `omt-hook` reports the interaction *without* deferring, the
   card renders remotely as **answerable**, and the answer is delivered by the
   synthetic responder driving the agent's own card. Worse fidelity, same
   feature, no unverified dependency.
3. `omt-hook` blocks only up to a configured budget (default 250 ms beyond the
   agent's own tolerance) and always fails open. An agent must never hang
   because omt is slow or dead. This is non-negotiable: the hook's default
   behaviour on any error is to return `{}` and get out of the way.

---

## 6. The heuristic floor

`PtyHeuristics` exists for Aider, Amp, Crush, Goose in TUI mode, and for any
user who declines to install hooks. Its output type is deliberately narrow:

```rust
pub enum Activity { Busy, Idle, NeedsAttention, Unknown }
```

It can produce nothing else. There is no code path from screen text to an
`Interaction`. Signals used, in order of robustness: alternate-screen entry,
spinner-cadence line rewrites, prompt glyph on the last non-empty row,
bracketed-paste transitions, and BEL/OSC 9/OSC 777 → `NeedsAttention`.

Screen text is read from the live bottom buffer, never the scrolled viewport, so
a user reading scrollback cannot change detection — another tool's `detection_text()`
gets this right and it is worth stating explicitly.

Why the hard cap: locale changes, a theme change, or a minor version bump
rewrites an agent's TUI. Under this design that degrades attentiveness. Under a
screen-scraping design it would silently produce *wrong cards*, and a wrong card
that a user taps "Allow" on is a security incident.

---

## 7. Adapters

```rust
pub trait AgentAdapter: Send + Sync {
    fn kind(&self) -> AgentKind;

    /// Fingerprints for tier-1 detection: env markers (preferred), exe names,
    /// argv patterns for runtime-wrapped bundles.
    fn fingerprint(&self) -> Fingerprint;

    /// Env to inject when omt spawns this agent (correlation ids, hook config).
    fn spawn_env(&self, ctx: &SpawnCtx) -> Vec<(OsString, OsString)>;

    /// Sources this adapter can construct for a binding, best-first.
    fn sources(&self) -> Vec<Box<dyn EventSource>>;

    /// Native responders, best-first.
    fn responders(&self) -> Vec<Box<dyn Responder>>;

    /// Hook/plugin installation into the agent's own config.
    fn integration(&self) -> Option<&dyn Integration>;

    /// Resolved slash commands for remote completion.
    fn commands(&self, b: &AgentBinding) -> BoxFuture<Result<Vec<SlashCommand>, AdapterError>>;
}
```

### 7.1 Integration installer

`Integration` writes omt's hook into the agent's own configuration —
`~/.claude/settings.json`, `~/.codex/hooks.json`, `~/.cursor/hooks.json`,
Gemini's settings, opencode's plugin directory. Rules learned from another tool:

- **Merge, never overwrite.** Users already have hooks; this machine has
  another tool's installed in three of those files right now.
- Preserve formatting and comments — edit the JSONC concrete syntax tree, do not
  round-trip through a value.
- Stamp every installed artifact with `omt-integration-version` so stale
  installs are detectable and upgradable.
- Every install is reversible (`omt integration uninstall`) and previewable
  (`--dry-run` printing the exact diff).
- Guard clauses at the top of the hook: if `OMT_SOCK` is unset or the socket is
  gone, exit 0 immediately.

### 7.2 Correlation

omt injects `OMT_INSTANCE`, `OMT_SESSION`, `OMT_SOCK` into every spawned PTY.
Any hook, plugin or wrapper therefore already knows which pane it belongs to,
which removes the entire class of "match the transcript to the pane" heuristics.
Agents omt did not spawn are correlated by native session id from environ
(`CLAUDE_CODE_SESSION_ID`, `CODEX_THREAD_ID`, `CURSOR_CONVERSATION_ID`) or, as a
last resort, by `(cwd, start-time)` proximity against transcript files —
explicitly marked low-confidence in `agent.explain`.

### 7.3 Coverage matrix (initial)

| Agent | Binding | State | Interactions | Queue | Commands |
|---|---|---|---|---|---|
| Claude Code | env marker | hooks (30 events) | **Choice + Permission, native** | native (`queue-operation`) | `system/init` + disk |
| Codex | env marker | hooks + app-server | Permission, native | — | app-server |
| opencode | env/argv | plugin + REST/SSE + ACP | Permission, native | uncertain | ACP `available_commands_update` |
| Gemini CLI | argv | hooks + ACP | Permission, native | — | ACP |
| Qwen Code | argv | inherits Gemini | Permission, native | — | ACP |
| Cursor | env marker | hooks | Permission, native | — | disk |
| Goose | argv | ACP | Permission, native | — | ACP |
| Amp | argv | stream-json | Permission, degraded | — | — |
| Aider | argv | transcript + heuristics | Text, synthetic | — | static list |
| Crush | argv | heuristics | none | — | — |

`Interaction::Choice` is Claude-Code-only today. That is the differentiator, not
a gap; every other agent degrades to `Permission` and `Text`, which are
universal via ACP.

---

## 8. Ancillary semantics

- **Message queue.** Claude Code writes `queue-operation` (`enqueue`/`remove`)
  lines to its transcript. omt mirrors the pending queue to every surface and
  exposes `agent.queue.enqueue`/`remove`, so a phone can queue work for an agent
  that is mid-turn — which is exactly what one wants to do from a phone. No
  other CLI exposes this cleanly; the model degrades to a local omt-side queue
  that is flushed when the agent goes idle, clearly labelled as omt-managed.
- **Slash commands.** Surfaced through `agent.commands.list` from the agent's
  own resolution (never from omt guessing), giving the web client a real
  completion popup with descriptions and argument hints.
- **Usage, rate limits, compaction, subagents.** Normalized into `AgentEvent`
  payloads and rendered as session metadata. Subagent threads are a tree via
  `thread.parent`, unifying Claude Code's `isSidechain`, Codex's subagent thread
  source, and opencode's `session.parent_id`.
- **Session summaries.** Claude Code emits an `away_summary` — a natural
  language "where is this session at". It is displayed verbatim on the session
  card; omt never generates one itself (P4: omt does not talk to models).

---

## 9. Failure modes and their handling

| Failure | Behaviour |
|---|---|
| omt daemon dies while an interaction is deferred | `omt-hook` times out and fails open; the agent shows its own card; on restart omt reconciles and marks the interaction `Cancelled { daemon_restart }` |
| Hook installed but socket unreachable | hook exits 0 in <5 ms; state falls back to transcript/heuristics; `agent.explain` reports the degradation |
| Two sources disagree | higher tier wins; the disagreement is recorded and visible in `agent.explain` |
| Agent version changes its transcript schema | transcript reader is versioned and validates; on parse failure it disables itself for that binding and reports, rather than emitting garbage |
| User runs an agent inside tmux inside omt | binding is `Unknown`; documented as unsupported for observation; the terminal still works |
| Agent spawns subagents | separate threads under one binding; state is the aggregate (any subagent blocked ⇒ session needs you) |

---

## 10. Open questions

1. `permissionDecision: "defer"` — does it park long enough, and with what
   timeout? Gates the highest-fidelity path. Spike first. *(fallback designed)*
2. Do Codex/Cursor/Gemini hook payloads match the Claude-Code shape closely
   enough for one normalizer? Verify with a logging hook.
3. Does hook-emitted OSC actually reach omt's PTY for non-Claude agents, or is
   hook stdout swallowed?
4. opencode `session_input` table — is it the type-ahead queue? Would extend the
   queue feature beyond Claude Code.
5. Codex `app-server`: are approvals first-class methods there? If so it is a
   better responder than hooks for Codex.
6. Goose/Crush session DB schemas, for transcript-tier support.
7. Whether Amp's `--stream-json` is byte-compatible with Claude Code's, or only
   structurally similar.
