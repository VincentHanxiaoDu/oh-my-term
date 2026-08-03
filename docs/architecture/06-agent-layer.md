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

Requirements 1 and 2 are what another tool does. Requirements 3 and 4 drive every design
choice below — though
[D9](decisions.md#d9--positioning-what-omt-may-and-may-not-claim) is the honest
calibration here: requirement 3 is **table stakes** across this category, not a
differentiator. What is not table stakes is answering it *from a phone while the
user's real interactive TUI is live on screen*.

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
    /// `pty` (default) or `native` (ACP). Defined in
    /// [05 §1](05-session-model.md#13-session-modes-d8), which owns `SessionMode`. D8.
    pub mode: SessionMode,
    /// Which sources are currently live for this binding.
    pub sources: Vec<SourceStatus>,
}
```

A binding ends when the foreground process group changes away from the agent, or
when a lifecycle source reports session end. Retained evidence (OSC titles,
tailed files, hook correlation) is dropped on binding end so a new agent can
never inherit the previous one's state — the same discipline another tool applies in
`clear_retained()`, and for the same reason.

### 2.1 Session modes

An agent occupies a session in one of two modes — `SessionMode::Pty` or
`SessionMode::Native` — defined in
[05 §1](05-session-model.md#13-session-modes-d8) and decided by
[D8](decisions.md#d8--two-session-modes-pty-default-and-native-acp).

- **`pty` (default).** The agent draws its own TUI in a real PTY and omt observes
  it from outside. The tiered source model of §3–§4 applies in full.
- **`native`.** omt spawns the agent in ACP mode. There is **no TUI and no PTY**;
  the ACP connection is the sole event source and the sole responder, and the
  merge rules of §4 are inert because there is only one source to merge.

This corrects a framing that appears elsewhere in this document: **ACP is a
replacement front end, not an observability sidecar.** You cannot run a CLI's own
TUI and speak ACP to the same process. `AcpClient` therefore appears twice in
this design, and the two appearances are not the same thing:

1. as a tier-`Protocol` `EventSource` for a **`pty`** session whose agent happens
   to expose ACP alongside its TUI (a second endpoint on a process omt is already
   observing), and
2. as **the whole of a `native`** session — transport, event stream, responder
   and renderer input, with nothing else present.

---

## 3. Source model

Everything that can tell omt something about an agent implements one trait. The
`Tier` ladder below — and the whole tiered source model — applies to **`pty`
sessions only** ([D8](decisions.md#d8--two-session-modes-pty-default-and-native-acp),
§2.1); a `native` session has exactly one source.

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
| `PtyHeuristics` | Heuristic | activity only; structurally incapable of emitting structured content (§6). It implements `HeuristicSource`, not `EventSource` — the tier-0 ban is a type, not a rule ([§8.4](#84-which-tier-may-produce-which-payload)) |

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

`AgentState` lives in `omt-types` and is the **only** vocabulary for agent
activity on any surface. Wire names are the `snake_case` variant names —
`starting`, `idle`, `working`, `blocked`, `exited`, `unknown`. There is no
separate `busy` or `needs_attention` state: `working` is busy, and "needs you"
is `blocked`. (`Activity` in §6 is an internal *input* type of the heuristic
source, never an observable state.)

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
first-class, addressable, resolvable object. "Flagship" here means *most
engineering depth*, not *most defensible claim*:
[D9](decisions.md#d9--positioning-what-omt-may-and-may-not-claim) holds the
narrower claim, which is answering one of these cards from a phone **while the
user's real interactive TUI is on screen**.

> **Ownership.** This document defines `Interaction`, `InteractionKind`,
> `InteractionResponse` and their fields — it is the single source of truth for
> the *shape*. [12 §4](12-collaboration.md#4-interaction-ownership) defines the
> *concurrency semantics* (who may resolve, exactly-once, conflict handling,
> timeouts) and wins on those. `web/src/generated/events.ts`
> ([08 §2.1](08-web-client.md#21-what-codegen-emits)) is generated from these
> types; the JSON on the wire uses the serde names below, `type` as the enum
> tag, and `snake_case` throughout.

```rust
pub struct Interaction {
    pub id: InteractionId,
    pub session: SessionId,
    pub binding: BindingId,
    pub kind: InteractionKind,
    pub opened_at: Timestamp,
    /// Deadline after which the agent proceeds on its own, if the mechanism has
    /// one. Named `timeout_at` on the wire and in 12 §4.
    pub timeout_at: Option<Timestamp>,
    pub state: InteractionState,
    /// How an answer gets back to the agent. §5.2.
    pub responder: ResponderRef,
    /// Whether omt can deliver an answer to *this* card at all, and over which
    /// channel. Computed once by the normalizer from `responder` + `kind`;
    /// see §5.2.1. Remote answerability renders from this and never from
    /// `state == Open` (D13).
    pub deliverable: Deliverable,
    /// Advisory: who is currently looking at this card. See 12 §4.4.
    pub viewers: Vec<ActorId>,
}

/// Whether an answer can be delivered, and over which channel. §5.2.1.
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Deliverable {
    /// The responder has a real response channel (ACP, plugin, app-server).
    /// Not gated by the writer token — 12 §3.1.
    Native,
    /// Keystrokes into the agent's own TUI. Delivery is D13's gated
    /// transaction; `requires_token` is `true` for every synthetic delivery
    /// and is carried explicitly so a client can render the gate without
    /// knowing the rule.
    Synthetic { requires_token: bool },
    /// omt cannot answer this card. The surface shows it read-only with
    /// `reason` and offers the terminal view (§4, D16).
    None { reason: NotDeliverableReason },
}

#[serde(rename_all = "snake_case")]
pub enum NotDeliverableReason {
    /// Submitting requires cursor navigation — multiSelect, plan review.
    NotPositionIndependent,
    /// The option's index is not derivable from the payload — a *specific*
    /// deny on a permission prompt (D16).
    IndexNotDerivable,
    /// The responder is `Inferred` and not enabled for this agent (D3).
    InferredResponderDisabled,
    /// No responder covers this kind for this agent.
    NoResponder,
}

/// Semantics and transitions: 12 §4.1.
pub enum InteractionState {
    Open,
    /// CAS won; the answer is committed as a decision but not yet written.
    /// **Carries the response** — see the note below; without it a crash
    /// between CAS and delivery cannot report what was lost.
    Resolving { by: Actor, at: Timestamp, response: InteractionResponse },
    /// The bytes have been written to the delivery channel. Not yet proof
    /// the agent received them.
    Submitted { by: Actor, at: Timestamp, response: InteractionResponse },
    /// omt **observed the agent record the answer**. The only success state.
    Resolved  { by: Actor, at: Timestamp, response: InteractionResponse },
    /// Written, but no confirming observation arrived inside the window.
    /// The response text is preserved so the user can see what was lost.
    Undelivered { by: Actor, at: Timestamp, response: InteractionResponse,
                  reason: UndeliveredReason },
    Cancelled { by: Actor, reason: CancelReason, at: Timestamp },
    Abandoned { at: Timestamp, detail: String },
}

pub enum UndeliveredReason {
    /// No confirming observation inside the bounded window.
    NotConfirmed,
    /// A completion arrived for this call, but the agent recorded a *different*
    /// answer — almost always because the local user answered by hand first.
    /// See §5.1.1; this is never retried and is surfaced with both answers.
    AnsweredDifferently { observed: InteractionResponse },
    /// The daemon restarted with a decision recorded but unwritten, or
    /// written and unconfirmed. Never retried — see below.
    DaemonRestart,
    /// The gated PTY transaction (D13) failed its preconditions.
    PreconditionFailed,
}

#[serde(tag = "type", rename_all = "snake_case")]
pub enum InteractionKind {
    /// Claude Code AskUserQuestion; ACP/MCP elicitation with a choice schema.
    Choice { questions: Vec<ChoiceQuestion> },
    Permission {
        tool: String,
        /// The verbatim tool input, unmodified.
        input: serde_json::Value,
        /// The shell command, for exec-shaped tools.
        command: Option<String>,
        /// Structured diff for edit-shaped tools. `FileDiff` is defined in
        /// [15 §3.2](15-workspace-explorer.md#32-vcs-model) so one renderer
        /// serves both the explorer and this card (15 §8.5).
        diff: Option<FileDiff>,
        /// The agent's own option list, verbatim and in the agent's order.
        /// omt neither adds, removes, nor reorders entries (D1).
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

/// One of the agent's own suggestions, passed through unchanged.
pub struct PermissionOption { pub id: String, pub label: String, pub kind: PermissionOptionKind }
pub enum PermissionOptionKind { Allow, AllowAlways, Deny, DenyAlways, Edit }
```

`Choice` maps 1:1 onto the verified `AskUserQuestion` schema, so there is no
lossy translation on the flagship case.

### 5.0 `InteractionResponse`

One response type, tagged by `type`, index-aligned to the request. Rendering it
into the exact prose or JSON the agent expects is the **daemon's** job, not any
surface's ([08 §5.2.2](08-web-client.md#522-other--free-text)), so the formatting
has one implementation and one fixture.

```rust
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InteractionResponse {
    /// One `ChoiceAnswer` per question, index-aligned with `questions`.
    Choices { answers: Vec<ChoiceAnswer> },
    Permission { decision: PermissionOptionKind,
                 updated_input: Option<serde_json::Value>,
                 reason: Option<String> },
    Text { value: String },
    PlanReview { decision: PlanDecision, reason: Option<String> },
}

pub struct ChoiceAnswer {
    /// Selected option labels. Length 1 unless `multi_select`.
    pub labels: Vec<String>,
    /// Free text entered via the synthetic "Other…" row. Mutually exclusive
    /// with a non-empty `labels`.
    pub other: Option<String>,
    /// Extra context attached to a chosen option — Claude Code's `n` key.
    pub comment: Option<String>,
}
```

### 5.1 Lifecycle

```
agent emits ──► source normalizes ──► ledger opens Interaction ──► broadcast
                                                │
                     ┌──────────────────────────┼──────────────────────────┐
                     ▼                          ▼                          ▼
                 TUI card                  web card (phone)          API client
                     └──────────── interaction.resolve ─────────────┘
                                            │
                          ledger CAS: Open → Resolving{response}
                                            │
                                    responder injects
                                            │
                                      → Submitted
                                            │
                     ┌──────────────────────┴──────────────────────┐
        omt observes the agent record it            window elapses with no
        (PostToolUse / transcript entry /              confirming observation
         tool result) within the window                        │
                     ▼                                          ▼
                 → Resolved                              → Undelivered
```

#### 5.1.1 What counts as a confirming observation

The window alone is not a predicate. An observation confirms a *specific*
submitted answer only when all three hold:

1. **Same call.** The observation carries the `tool_use_id` the interaction was
   opened from. For agents with no such id, the correlation is
   `(agent_session, interaction kind, opened_at ± window)` and is marked
   low-confidence in `agent.explain`.
2. **Terminal for that call.** It is a completion, not progress — a
   `PostToolUse`, a `tool_result`, or the transcript's own record of the answer.
3. **The recorded answer equals the submitted one.**

Rule 3 is the one that matters and it is not redundant with rule 1. The failure
it catches is the common one: the local user answers the card by hand, with a
*different* option, a moment before omt's bytes land. Rules 1 and 2 alone would
see a completion on the right `tool_use_id` and report `Resolved` — omt would
tell the remote user their answer was applied when a different answer was.

Equality is compared on the **recorded selection**, not on rendered prose:
`ChoiceAnswer.labels` against the labels omt submitted, and a permission
decision against the option id. Where an agent records only prose — Claude Code's
`tool_result` is a sentence of `"question"="label"` pairs — the labels are
extracted and compared, and a parse failure is treated as *not* confirming.

A mismatch is never silent and never a retry. It resolves as
`Undelivered { reason: AnsweredDifferently { observed } }`, every surface shows
what the agent actually recorded next to what was submitted, and the event is
**loud and reportable** — per
[`spike-card-answering.md`](../research/spike-card-answering.md), a mismatch is
the designed signal that the accelerator mechanism itself broke on a new agent
version, which is exactly the failure
[D16](decisions.md#d16--remote-answering-is-per-card-type-and-the-preconditions-are-empirical)
relies on being visible rather than silent.

Per-agent coverage for observing a *resolution* — as distinct from raising one —
is a column of §7.3's matrix. Where it is absent the interaction must **expire**
rather than linger: a denied permission may emit no completion at all, and a
card left open indefinitely invites an answer that lands in whatever the agent is
doing by then.

**Exactly-once resolution** is enforced by a compare-and-swap on
`InteractionState` inside the ledger. The loser of a race gets
`conflict` with the winning actor's identity, and every surface immediately
re-renders the card as answered-by-someone-else. This is the concrete answer to
"the phone and the TUI both answer at once" (see
[12 — Collaboration](12-collaboration.md)).

**Delivery is confirmed by observation, never asserted.** Per
[D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)
consequence 1, an answer delivered by synthetic input belongs to the
*externally-confirmed intent* class: the sink is a UI omt does not own, so a
successful write proves nothing. `Submitted` records that the bytes went out;
only an observation that the agent recorded the answer — a `PostToolUse` hook, a
transcript entry, a tool result — promotes it to `Resolved`. If the bounded
confirmation window (default 10 s, per-adapter) elapses first, the interaction
goes to `Undelivered { reason: NotConfirmed, response }` and every surface says
*"your answer may not have reached the agent — check the terminal"*, showing the
preserved response text. A `Native` responder's own transport-level reply is
itself the confirming observation, so native paths move `Submitted → Resolved`
immediately.

**An injection is at-most-once and is never retried.** Not on reconnect, not on
daemon restart, not by a reconnecting client repeating its `intent_id`, not by
any automated actor at all. The only actor permitted to re-answer is a **human
who can see the screen**. The reason is D15's: a duplicate keystroke into a UI is
not a duplicate row that dedup can absorb, it is a keystroke landing *somewhere
else entirely* — whatever the agent is doing now. `Undelivered` is therefore a
terminal state that the system never tries to repair on its own; it is a report
to a human, and the offered action is "open the terminal view" or "go to the
card", never "retry" — §5.2.2 specifies which surface offers which.
(`interaction.resolve`'s idempotency key still replays the *stored result* of an
already-applied call — that is a read of the ledger, not a second write.)

### 5.2 Responders — how the answer gets back

```rust
#[async_trait]
pub trait Responder: Send + Sync {
    fn fidelity(&self) -> Fidelity;   // Native | Synthetic
    /// Only meaningful for `Synthetic`; `Native` responders return
    /// `Independent`. Required by D3: it is the axis that decides whether a
    /// responder is enabled by default.
    fn state_dependence(&self) -> StateDependence;
    /// Whether this channel can deliver a modified tool input alongside an
    /// approval. A property of the channel, not of the agent's option list.
    /// See §5.4. D9.
    fn supports_edit(&self) -> bool { false }
    async fn respond(&self, i: &Interaction, r: &InteractionResponse) -> Result<(), RespondError>;
}
```

[D11](decisions.md#d11--omt-mirrors-the-agents-own-card-it-does-not-intercept-or-replace-it)
removed deferral, and with it the hook response slot as a delivery channel. The
agent's own CLI draws its own card locally; omt mirrors it and delivers a remote
answer **through that card**. This changes the table materially — Claude Code no
longer has a native responder for `Choice`:

| Mechanism | Agents / kinds | Fidelity |
|---|---|---|
| ACP `session/request_permission` response | opencode, Gemini, Goose, Qwen; all `native` sessions ([D8](decisions.md#d8--two-session-modes-pty-default-and-native-acp)) | Native |
| ACP `elicitation/create` response | ACP agents offering it — `Choice` in `native` mode | Native |
| opencode plugin `permission.ask` reply | opencode | Native |
| app-server approval response | Codex | Native |
| **Synthesized keystrokes into the agent's own card** | Claude Code `Choice` and `Permission` in `pty` mode; anything else | Synthetic |

**Claude Code's `Choice` now falls to the synthetic responder** in `pty` mode,
gated by [D3](decisions.md#d3--synthetic-input-is-bounded-by-state-dependence-not-by-tool-danger)
(is the answer position-independent?) and by
[D13](decisions.md#d13--synthetic-delivery-is-a-gated-transaction-never-a-bare-write)
(token, quiescence, re-verification, atomic write). Whether that responder is
`Independent` — and therefore on by default — depends on whether the
`AskUserQuestion` card accepts typing a number or letter, or requires counting
arrow keys.

**That question is settled: it accepts a digit.**
[`../research/spike-card-answering.md`](../research/spike-card-answering.md)
verified live against Claude Code v2.1.x that an ASCII digit resolves the option
at that *absolute* index in the option list and submits in the same keystroke —
proven by pressing `↓ ↓ 1` and getting option 1, not option 3. The responder is
therefore `Independent` and **on by default** for a single-select `Choice`.

The answerability that follows is per card type, not blanket, and is carried on
the interaction as `deliverable` (§5.1):
[D16](decisions.md#d16--remote-answering-is-per-card-type-and-the-preconditions-are-empirical)
records single-select `Choice` and a permission *allow* as `Synthetic`, and
multi-select, plan review and a specific *deny* as `None` — for those the remote
surface shows the card observed-but-not-answerable, with the reason and the
terminal view one tap away. The same Claude Code session in `native` mode has a
native responder and no such gate, which is the honest reason `native` exists.

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

#### 5.2.1 `deliverable` — a field of `Interaction`, computed by the normalizer

> **Choice recorded here.** [D13](decisions.md#d13--synthetic-delivery-is-a-gated-transaction-never-a-bare-write)
> writes the shape as `Open { deliverable: … }`. **This document places
> `deliverable` on the `Interaction` struct instead, not inside the `Open`
> variant**, and D13's spelling is to be read as shorthand for it. The reason is
> that deliverability is a property of the *interaction and its responder*, not
> of its openness: it stays meaningful in `Submitted` and `Undelivered`, where a
> client must be able to say whether the card was answerable from here at all or
> whether it always needed a human at the keyboard — which is what §5.2.2's
> failure affordance renders. Putting it inside `Open` would
> delete exactly the information the failure surface needs. It also keeps
> `InteractionState`'s variants about *transitions* and nothing else, and it
> matches how [12 §3.1](12-collaboration.md#31-what-it-governs) and
> [05 §5.4](05-session-model.md#54-what-is-not-gated) already write it:
> `Interaction.deliverable`.

**It does not duplicate `ResponderRef` — it is derived from it.** The responder
answers *"what channel exists for this agent and kind?"*; `deliverable` answers
*"can that channel actually carry an answer to **this** card, and does the caller
need the writer token?"* [D16](decisions.md#d16--remote-answering-is-per-card-type-and-the-preconditions-are-empirical)
is why the second question is not the first: one Claude Code `pty` responder
backs four card types with three different answers. So `deliverable` is
**computed, never reported by a source**, from `(responder.fidelity,
responder.state_dependence, kind, agent, mode)`:

- `Native` fidelity → `Native`.
- `Synthetic` + `Independent` + the kind passes the per-card-type table below →
  `Synthetic { requires_token: true }`.
- `Synthetic` + `Inferred` and not explicitly enabled → `None { InferredResponderDisabled }`.
- no responder → `None { NoResponder }`.

**When it is computed:** once, in the normalizer, at the point where a source's
payload becomes an `Interaction` (§5.1) — before the ledger opens it and
therefore before any surface can see it. It is not recomputed per subscriber,
which is what makes every surface agree. It is recomputed only if the binding's
session mode changes, which re-opens the interaction anyway.

**Per-card-type mapping — Claude Code**, from D16's verified table. `native`
mode is ACP throughout, so every row is `Native`.

| `InteractionKind` | `pty` mode | `native` mode |
|---|---|---|
| `Choice`, all questions `multi_select: false` | `Synthetic { requires_token: true }` | `Native` |
| `Choice`, any `multi_select: true` | `None { NotPositionIndependent }` | `Native` |
| `Permission` — the `Allow` / `AllowAlways` options | `Synthetic { requires_token: true }` | `Native` |
| `Permission` — a *specific* `Deny` / `DenyAlways` option | `None { IndexNotDerivable }` | `Native` |
| `PlanReview` | `None { NotPositionIndependent }` | `Native` |
| `Text` | `Synthetic { requires_token: true }` | `Native` |

A `Permission` card is therefore **partly** deliverable in `pty` mode: the card
is `Synthetic` and the surface offers allow, while `Esc` — D16's universal safe
negative — is offered as the only negative. A specific deny is rendered
read-only with its reason. This is the one kind where `deliverable` is not the
whole story, and the per-option detail lives in `PermissionOption`, not in a
second enum.

The runtime preconditions ([12 §4.6](12-collaboration.md#46-preconditions-on-a-synthetic-delivery),
including D16's *"is a number rendered on the row?"* check) are evaluated at
**delivery** time, not here. A card can be `Synthetic` and still fail its
transaction; that is `Undelivered { PreconditionFailed }`, and `deliverable` is
what tells the client what to offer next — which is the subject of the next
section.

#### 5.2.2 What `deliverable` drives when delivery fails

The reason `deliverable` survives into `Submitted` and `Undelivered` (§5.2.1) is
that a failed delivery has to tell a human what they can still do. Until this
section it did not: every surface rendered *"your answer may not have reached the
agent — check the terminal"* and offered nothing to act on. The field now drives
a concrete affordance, and the affordance differs by **where the client is**, not
by role — [D2](decisions.md#d2--remote-is-exactly-equivalent-to-local) is intact,
because the difference is physical reach, not authority.

There is **no Retry control on any surface, ever.** An injection is D15's
externally-confirmed class and is never repeated by an automated actor; the
ledger rejects a second resolver on an `Undelivered` interaction
([12 §4.1](12-collaboration.md#41-the-invariant)); and the only actor permitted
to re-answer is a human who can see the screen. What every surface offers
instead is a way to *become* that human, or to reach one.

| Surface | What is offered on an `Undelivered` card |
|---|---|
| A client attached to the session's own instance with the pane on screen — the local TUI, or a desktop web client showing that session | **Go to the card** (`interaction.focus_latest`, §4.4: focus the pane, unzoom, scroll it into view) and **Copy my answer** (the preserved `response`, onto *this* client's clipboard — `media.clipboard.write`, `Locality::Local`). The agent's own card is most likely still waiting; the human answers it there, by hand, with the text one paste away. |
| Any other client — a phone, a client attached to a different instance | **Open the terminal view** for that session ([D14](decisions.md#d14--agent-sessions-get-a-transcript-surface-blocks-are-for-shell-work)'s third surface, always available), with the preserved response shown read-only above it. This is the honest answer: a remote user cannot answer the card, but they can *look* at exactly what the agent is showing and decide whether to walk to the machine. |

`deliverable` decides the sentence attached to both, and the distinction is the
one a user actually needs:

- **`Native` or `Synthetic { .. }`** — the card *was* answerable from here, and
  the delivery is what failed. The copy reads *"your answer was not confirmed —
  the agent's card is probably still waiting. Answer it at the terminal."* Paired
  with `reason`: `PreconditionFailed` adds *"nothing was typed"* (the strongest
  case for the card still being open and untouched), `NotConfirmed` adds *"omt
  typed an answer but never saw the agent record it"*, and
  `AnsweredDifferently { observed }` shows what the agent recorded instead, which
  usually means someone at the keyboard already dealt with it.
- **`None { reason }`** — the card was never answerable from omt at all
  (multi-select, a plan review, a specific deny, no responder). The copy says so
  and names the reason: *"this card can only be answered at the terminal."* There
  is nothing to re-attempt, and telling the user that is more useful than an
  identical failure banner.

A client that cannot render either affordance — a CLI printing a result — prints
the same two facts as text: the preserved response, and the session to open.
That is the third surface's answer and it needs no capability of its own.

### 5.3 The deferral mechanism — demoted to an optional optimization

> **Superseded by [D11](decisions.md#d11--omt-mirrors-the-agents-own-card-it-does-not-intercept-or-replace-it).**
> omt does **not** park the tool call. Parking means the CLI never draws its own
> card, which destroys the local user's native experience — the thing omt exists
> to preserve. The `PreToolUse` hook is still the *observation* channel: it fires
> before the card is drawn and carries `tool_input` verbatim, which is everything
> needed to mirror the interaction remotely. What it is no longer is the
> *delivery* channel. The paragraphs below are retained because the mechanism
> remains a legitimate future optimization for a user who explicitly opts into
> it, and because point 3's fail-open rule is unconditional.

The mechanism itself is Claude Code's `PreToolUse` hook returning

```json
{"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "defer"}}
```

for `tool_name == "AskUserQuestion"`. The hook payload contains `tool_input`
verbatim — the exact `questions` array — and deferring parks the call so a
remote client can answer it.

Whether `defer` parks the call long enough for a human round-trip is still
unverified — but under D11 nothing depends on the answer. The
`spike-defer-semantics` change is **demoted**; the highest-priority experiment is
now the card-answering spike named in §5.2. Therefore:

1. `omt-hook` reports the interaction *without* deferring. This is the shipped
   path, not a fallback: the card renders remotely, and the answer is delivered
   by the synthetic responder driving the agent's own card, under D3 and D13.
2. Deferral, if it ever ships, is per-agent opt-in for a user who prefers a
   parked call to a live local card, and it is labelled as taking the local
   native card away.
3. `omt-hook` blocks only up to a configured budget (default 250 ms beyond the
   agent's own tolerance) and always fails open. An agent must never hang
   because omt is slow or dead. This is non-negotiable: the hook's default
   behaviour on any error is to return `{}` and get out of the way.

### 5.4 Editing an argument before approving

Required by [D9](decisions.md#d9--positioning-what-omt-may-and-may-not-claim)
consequence 3. This is the producer of `PermissionOptionKind::Edit` and of
`InteractionResponse::Permission { updated_input }`, both declared above.

**What it means.** The user approves the agent's proposed tool call with a
modified `input` — answering the agent's own prompt with a different argument,
which is exactly what the agent's own UI already allows. It stays inside
[D1](decisions.md#d1--omt-adds-no-policy-layer-over-an-agents-permission-semantics)
because omt adds no policy: it changes an *argument*, not the decision procedure,
and the agent still decides what it will accept.

**Shape.** `InteractionResponse::Permission { decision: Edit, updated_input:
Some(_) }` is the only shape in which `updated_input` is meaningful. A
`updated_input` carried on any other decision is a protocol error and is rejected
with `precondition_failed` rather than silently dropped.

**Who may offer it.** This is the load-bearing decision, because D1 forbids omt
adding entries to the agent's own option list. The rule:

- omt **never** adds an `Edit` entry to the `PermissionOption`s the agent
  supplied. That list is passed through verbatim, in the agent's order (§5).
- Editing is instead offered whenever the **responder** declares it deliverable.
  The `Responder` trait (§5.2) gains:

  ```rust
      /// Whether this channel can deliver a modified tool input alongside an
      /// approval. Not a property of the agent's suggestion list. D9.
      fn supports_edit(&self) -> bool { false }
  ```

  Delivering an edited input is a property of the *channel* — Claude Code's
  `PreToolUse` `updatedInput`, ACP's permission response — not of what the agent
  happened to suggest. Offering an edit affordance that the channel can honour is
  not omt inventing an option; it is omt exposing a capability the transport
  already has.

  The alternative — *offer `Edit` only when the agent listed it in `options`* —
  is **rejected and recorded as such**: it would make the feature unavailable on
  channels that demonstrably support it, purely because an agent's suggestion
  list is a UI hint rather than a capability declaration.

**Where it is available, and where it is not.** `supports_edit()` is a property
of the channel, and the channels do not agree — so this feature is present on one
session mode and absent on the other. That has to be stated here rather than
discovered from a table three sections up:

| Responder | `supports_edit()` | Consequence |
|---|---|---|
| ACP `session/request_permission` — every `native` session ([D8](decisions.md#d8--two-session-modes-pty-default-and-native-acp)), and the ACP agents in `pty` mode that still answer natively | **true** | The full editor, on every surface: [08 §5.3](08-web-client.md#53-permission--approval-cards--kindtype--permission)'s sheet on the web, and the `CARD_FOCUSED` map's `e` in the TUI ([16 §8.3](16-input-and-keymap.md#83-contextual-maps)) |
| opencode plugin reply, Codex app-server approval | **true** where the reply carries a modified input; each adapter declares it | as above |
| **Claude Code in `pty` mode** — the synthetic responder | **false** | No edit affordance anywhere. The channel is a single ASCII digit into the agent's own card (§5.2.1, [D16](decisions.md#d16--remote-answering-is-per-card-type-and-the-preconditions-are-empirical)); a keystroke cannot carry an `updated_input`, and there is no second channel to carry one alongside it |

Two things follow, and both are deliberate:

1. **The affordance is absent, not disabled.** A greyed-out *Edit* button would
   advertise a feature the transport cannot perform and invite the user to hunt
   for the setting that enables it. [08 §5.3](08-web-client.md#53-permission--approval-cards--kindtype--permission)
   already renders it this way; this document had simply never said why.
2. **No TUI argument editor is specified for `pty` mode**, and none should be.
   In `pty` mode the local user is looking at the agent's own card, which has its
   own amend affordance (Claude Code prints `Tab to amend` on permission cards) —
   omt drawing a second editor over it would be exactly the replacement card
   [D11](decisions.md#d11--omt-mirrors-the-agents-own-card-it-does-not-intercept-or-replace-it)
   forbids, and it would have to deliver its result through the digit channel it
   does not fit through. The `e` binding is therefore installed only where the
   responder reports `supports_edit()`.

This is a real gap against the competitor [D9](decisions.md#d9--positioning-what-omt-may-and-may-not-claim)
measured omt against, in the flagship agent's default mode, and it is recorded as
one rather than hidden: `native` mode is where argument editing lives, and that
is one more honest reason `native` exists.

**Validation.** omt does not validate `updated_input` against a schema it
invented. Where the channel supplies the tool's own input schema, the editor is
schema-aware (field types, enums, required keys); otherwise it is a raw JSON
editor and the agent rejects what it does not accept. omt never "fixes up" an
edited input.

**Attribution.** The edited call is tagged in the event stream and visibly
attributed on every surface as user-edited, carrying both the original and the
submitted input — the same discipline
[D3](decisions.md#d3--synthetic-input-is-bounded-by-state-dependence-not-by-tool-danger)
applies to synthetic responses, for the same reason: a reader of the transcript
must never mistake an omt-mediated change for something the agent proposed.

Rendering — the editor, the diff, the mobile sheet — is
[08 §5.3](08-web-client.md#53-permission--approval-cards--kindtype--permission)'s.

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

    /// Which session modes this adapter supports. D8. `Pty` only is the
    /// default; an adapter that can be driven over ACP declares `Native` too.
    /// `SessionModeSet` is defined in [05 §1.1](05-session-model.md#11-types)
    /// alongside `SessionMode`; it is *not* the keymap's `ModeSet`
    /// ([16 §6.5](16-input-and-keymap.md#65-the-keymap-abstraction)).
    fn supported_modes(&self) -> SessionModeSet { SessionModeSet::PTY_ONLY }

    /// Argv and env for spawning this agent in ACP mode. `None` when the
    /// adapter does not support `SessionMode::Native`.
    fn acp_spawn(&self, ctx: &SpawnCtx) -> Option<AcpSpawn> { None }

    /// Sources this adapter can construct for a binding, best-first.
    fn sources(&self) -> Vec<Box<dyn EventSource>>;

    /// Native responders, best-first.
    fn responders(&self) -> Vec<Box<dyn Responder>>;

    /// Hook/plugin installation into the agent's own config.
    fn integration(&self) -> Option<&dyn Integration>;

    /// Resolved slash commands for remote completion.
    fn commands(&self, b: &AgentBinding) -> BoxFuture<Result<Vec<SlashCommand>, AdapterError>>;

    /// This agent's native file-reference syntax for a workspace-relative path
    /// (Claude Code and opencode: `@crates/omt-term/src/lib.rs`). `None` means
    /// the agent has none, and the surface offers "insert path" instead of
    /// "insert mention". Used by
    /// [15 §8.2](15-workspace-explorer.md#82-inserting-a-path-or-file-into-an-agent-prompt).
    fn path_mention(&self, rel: &RelPath) -> Option<String> { None }

    /// How this agent wants to be handed an attachment that already exists on
    /// disk. See [09 §7.1](09-ssh-and-media.md#71-handing-the-image-to-the-agent)
    /// for the per-agent table and [09 §4.3.7](09-ssh-and-media.md#437-what-the-agent-finally-receives)
    /// for the `AttachmentReference` enum.
    fn attachment_reference(&self, path: &Path, meta: &BlobMeta,
                            class: &AttachmentClass) -> Option<AttachmentReference>;
}
```

Both `path_mention` and `attachment_reference` are agent-native knowledge, so they
live on the adapter rather than in a central table keyed by `AgentKind`
([P4](01-principles.md#p4--native-semantics-observe-never-re-implement)).
`path_mention` is defaulted, so adding it does not break third-party adapters
([P2](01-principles.md#p2--pluggable-extension-without-modification)). In
`native` mode `sources()` is not consulted at all — the ACP connection built from
`acp_spawn` is the only source (§2.1).

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

Build order is fixed by
[D5](decisions.md#d5--initial-agent-coverage): **Claude Code (full depth) →
the generic ACP adapter → Codex → the heuristic floor.** The ACP adapter is
built *early*, not last, precisely so that `AgentAdapter`, `EventSource` and
`Responder` are validated against a second shape before they are frozen — one
implementation covers opencode, Gemini CLI, Goose and Qwen Code at once, which
is the best coverage-per-unit-work available. An adapter trait shaped only by
Claude Code is the failure this ordering exists to prevent.

| Agent | Binding | State | Interactions | **Resolution observed by** | **Interrupt via** | Queue | Commands | Mobile surface |
|---|---|---|---|---|---|---|---|---|
| Claude Code | env marker | hooks (30 events) | **Choice + Permission**, synthetic delivery (§5.2) | `PostToolUse` on the same `tool_use_id`; `tool_result` carries the answer, so equality is checkable. **A denied permission may emit nothing — those expire.** | `Esc` (its own interrupt key) | native (`queue-operation`) | `system/init` + disk | **transcript** + cards |
| Codex | env marker | hooks + app-server | Permission, native | app-server response — the channel's own reply confirms | app-server | — | app-server | **transcript** + cards |
| opencode | env/argv | plugin + REST/SSE + ACP | Permission, native | ACP/plugin response | ACP `session/cancel` | uncertain | ACP `available_commands_update` | **transcript** + cards |
| Gemini CLI | argv | hooks + ACP | Permission, native | ACP response | ACP `session/cancel` | — | ACP | **transcript** + cards |
| Qwen Code | argv | inherits Gemini | Permission, native | ACP response | ACP `session/cancel` | — | ACP | **transcript** + cards |
| Cursor | env marker | hooks | Permission, native | hook completion | `Ctrl+C` | — | disk | **transcript** + cards |
| Goose | argv | ACP | Permission, native | ACP response | ACP `session/cancel` | — | ACP | **transcript** + cards |
| Amp | argv | stream-json | Permission, degraded | **none — interactions expire** | `Ctrl+C` | — | — | **grid only** — status, no transcript |
| Aider | argv | transcript + heuristics | Text, synthetic | transcript entry; equality checkable | `Ctrl+C` | — | static list | **grid only** — status, no transcript |
| Crush | argv | heuristics | none | n/a | `Ctrl+C` | — | — | **grid only** — status, no transcript |

**The interrupt column exists because interrupt is the floor's only control.**
For a heuristic-tier agent ([D5](decisions.md#d5--initial-agent-coverage) track 4)
a phone gets no blocks, no transcript and no answerable cards — stopping the
agent is the *entire* remote control surface, so "how do we interrupt this one"
cannot be left to a CLI help string. Where an agent exposes a native cancel,
`agent.interrupt` uses it and the pane is untouched; where it does not, the
fallback is `Ctrl+C` written to the PTY, which is what every CLI is interrupted
with and requires no inference about screen state
([D3](decisions.md#d3--synthetic-input-is-bounded-by-state-dependence-not-by-tool-danger)
is satisfied: a control character is position-independent by construction).

**The "mobile surface" column, and the floor stated honestly.** Required by
[D14](decisions.md#d14--agent-sessions-get-a-transcript-surface-blocks-are-for-shell-work).
No agent session gets the **block view** — [04 §6.4](04-terminal-core.md#64-the-fallback-heuristic--no-shell-integration)
suppresses segmentation for a session with an agent binding and no OSC 133,
because neither close condition can ever fire while the agent holds the
foreground. What replaces it is the **transcript view** ([08 §4](08-web-client.md#4-view-modes)),
built from the merged event stream, and it is available only where the binding
reaches **tier ≥ Transcript source** (§3–§4).

Below that line the answer is *nothing*: a heuristic-tier agent
([D5](decisions.md#d5--initial-agent-coverage) track 4 — Aider, Amp, Crush in TUI
mode) gets **neither blocks nor a transcript**. On a phone that session is
`busy | idle | needs you` plus a letterboxed 200-column grid, and the UI says so
rather than leaving the user to discover it. Aider's `Text` interactions still
render as cards; what is missing is the scrollable history.

The table above describes `pty` mode. Of these agents, **opencode, Gemini CLI and
Cursor ship an ACP mode**, as do the first-party **Claude** and **Codex** ACP
adapters, so those five can additionally run as `native` sessions
([D8](decisions.md#d8--two-session-modes-pty-default-and-native-acp)).
D8's warning applies verbatim: the Claude ACP adapter wraps the **Agent SDK, not
the Claude Code CLI**, so a `native` Claude session is *not running Claude Code*
— none of its keybindings, its `/voice`, its permission UX or its on-disk
sessions. That must always be a deliberate, labelled, informed choice.

`Interaction::Choice` is Claude-Code-only today. That is a coverage fact, not a
differentiator: remote question cards are commoditized
([D9](decisions.md#d9--positioning-what-omt-may-and-may-not-claim)), and a
shipping competitor already has them with i18n and argument editing. The
differentiated claim is narrower — **answering such a card from a phone while the
user's real interactive TUI is on screen, with both sides in sync**, which is a
consequence of [D11](decisions.md#d11--omt-mirrors-the-agents-own-card-it-does-not-intercept-or-replace-it)'s
mirror-don't-intercept shape and which nobody else does. Every other agent degrades to
`Permission` and `Text`, which are universal via ACP.

---

## 8. Ancillary semantics

- **Message queue.** Claude Code writes `queue-operation` (`enqueue`/`remove`)
  lines to its transcript. omt mirrors the pending queue to every surface and
  exposes `agent.queue.enqueue`/`remove`, so a phone can queue work for an agent
  that is mid-turn — which is exactly what one wants to do from a phone. No
  other CLI exposes this cleanly; the model degrades to a local omt-side queue
  that is flushed when the agent goes idle, clearly labelled as omt-managed.

  Four requirements from
  [D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism):

  1. **`agent.queue.enqueue` carries a `BindingId` and requires
     `AgentState::Working`.** The session id alone is not a sufficient target:
     a replayed or delayed enqueue against a session whose agent has since
     exited types the text **into the shell prompt, where it is executed as a
     command**. The call fails `precondition_failed` if the named binding is not
     the session's current binding, or if the binding is not `Working`. D3 does
     not protect here — it governs answers, and "submitting typed text to a
     prompt box" is on its allowed list; the failure is target identity, not
     state inference.
  2. **The omt-managed fallback queue needs a durable intent log.** It is
     memory-only in the current design, so every non-Claude agent's queued text
     dies unrecorded on `kill -9`. It is persisted through `omt-store` with its
     own row in [21 §6.2](21-data-lifecycle.md#62-what-kill--9-loses), and each
     entry carries `created_at` and `valid_until`; an entry past `valid_until`
     is presented for re-confirmation rather than silently flushed.
  3. **`queue-operation` is an enqueue receipt, and must be read as delivery
     confirmation, not as mirror data.** After an injected enqueue, omt waits a
     bounded window for a matching `queue-operation enqueue` line in the
     transcript. On a match the entry renders `Queued`; on timeout it renders
     `Failed { not_confirmed }` with the text preserved, and — as with
     interactions — it is **never retried automatically**.
  4. **`queue: unknown` is a distinct rendering.** When the transcript reader is
     stale, disabled or has never seen a `queue-operation` line for a binding
     that should have one, surfaces show `unknown`, never an empty queue. An
     empty queue is a claim; a broken reader must not be able to make it.

- **Slash commands.** Surfaced through `agent.commands.list` from the agent's
  own resolution (never from omt guessing), giving the web client a real
  completion popup with descriptions and argument hints.
- **File changes.** Tool calls that touch files are normalized into
  `AgentPayload::FileChanged { path, change, tool, turn }` (§8.2), where `change`
  is `Created | Modified | Deleted | Renamed { from }`. Every tier-3/4/5 source
  already carries the information (hook `PostToolUse` for `Edit`/`Write`/
  `MultiEdit`, ACP `tool_call_update` with a file location, transcript tool
  results), so this is normalization, not new observation. It is *attribution*,
  not truth about the filesystem — git status is the truth. Consumed by
  [15 §8.3](15-workspace-explorer.md#83-files-the-agent-changed-this-session),
  which keys the set by `BindingId` and clears it on binding end (§2).
- **Usage, rate limits, compaction, subagents.** Normalized into `AgentEvent`
  payloads and rendered as session metadata. Subagent threads are a tree via
  `thread.parent`, unifying Claude Code's `isSidechain`, Codex's subagent thread
  source, and opencode's `session.parent_id`.
- **Session summaries.** Claude Code emits an `away_summary` — a natural
  language "where is this session at". It is displayed verbatim on the session
  card; omt never generates one itself (P4: omt does not talk to models).

**The queue operations, and which of them each agent has.**

There are two kinds of queue behind one group of capabilities, and every
operation's semantics follow from which one it is acting on. This distinction is
load-bearing and was previously left implicit:

| Queue kind | Who owns the order | Agents |
|---|---|---|
| **Mirrored** | the agent. omt observes `queue-operation` lines and reproduces them; every mutation is an injection into a UI omt does not own | Claude Code |
| **omt-managed** | omt. The entries are rows in omt's own durable intent log, flushed into the agent when it goes idle, and labelled as omt-managed on every surface | every other agent — the fallback described above |

**`agent.queue.remove` is constrained exactly as `enqueue` is.** The four
requirements above constrain `enqueue` to the hilt and said nothing about
`remove`, which is backwards: removing the wrong entry is a silent loss of work,
and on a mirrored queue a removal is the same class of act as an enqueue — an
at-most-once write into the agent's own UI.

- It carries a `BindingId` and requires `AgentState::Working`, for requirement
  1's reason unchanged: a stale `remove` against a session whose agent has exited
  has no queue to act on and must fail rather than type.
- It names the entry by the id `agent.queue.list` returned **and** by a hash of
  the entry's text. If the entry at that id no longer carries that text — the
  agent consumed it, or the local user reordered around it — the call fails
  `precondition_failed` and nothing is written. An id alone is a position, and a
  position in a queue that is draining is not a stable target.
- **What confirms it: Claude Code writes a `queue-operation remove` line, and
  that line is the receipt.** Requirement 3's rule applies unchanged and in the
  same direction — the line is delivery confirmation, not mirror data. omt waits
  a bounded window (the same per-adapter default as an enqueue) for a
  `queue-operation remove` naming that entry.
- **When it is not confirmed inside the window**, the entry renders
  `Removal not confirmed — the item may still be queued; check the terminal`,
  with the entry's text preserved and still shown in the mirrored queue. It is
  **never retried automatically**, for D15's reason: a repeated removal against a
  queue that has since drained removes a *different* message. Exactly as with an
  interaction, the offered action is to open the terminal view, never to try
  again.
- On an omt-managed queue a removal is a row deletion in omt's own log, is
  confirmed by the write itself, and needs none of the above.

`remove`'s intent class therefore moves from `Cas` to
[D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)'s
**externally-confirmed** class, because `intent` is static and must be the
strictest the capability can require — the same argument already recorded for
`interaction.resolve`.

**`agent.queue.edit` — new, and offered only on an omt-managed queue.**
[08 §6.1](08-web-client.md#61-live-message-queue) renders queued text inline, which
invites editing it, and the composition *remove + re-enqueue* is not an
acceptable substitute: it is two separate externally-confirmed intents, each with
its own confirmation window, and the first one is the unconfirmable one. A
removal that lands followed by an enqueue that does not leaves the user with a
queue that has silently lost a message and nothing safe to retry. So:

- On an **omt-managed** queue, `agent.queue.edit { binding, entry, expected_hash,
  text }` rewrites the row in place under a CAS on the entry version. omt owns
  the sink, so this is an ordinary `Cas` command with no confirmation window and
  no loss mode.
- On a **mirrored** queue it returns `unsupported`, with the reason: Claude Code
  emits `enqueue` and `remove` and no third verb, so there is no edit primitive
  to drive and no receipt that would confirm one. The surface renders those
  entries read-only rather than offering an edit that would decompose into the
  lossy pair.

**Queue reorder — declared, and `agent.queue.reorder` is what
[08 §6.1](08-web-client.md#61-live-message-queue)'s "only if the instance reports a
queue-reorder capability" refers to.** The condition was written against a
capability that did not exist anywhere in the catalog. It exists now, with the
same split and for the same reason:

- **omt-managed queues can be reordered.** The order is a column in omt's own
  log; `agent.queue.reorder { binding, order: Vec<EntryId>, expected_version }`
  is a CAS over the whole ordering, which is the only shape that is safe when two
  clients drag at once.
- **Claude Code's mirrored queue cannot be, and no shipped agent's own queue
  can.** There is no `queue-operation move`, no documented reorder gesture omt
  could drive position-independently ([D3](decisions.md#d3--synthetic-input-is-bounded-by-state-dependence-not-by-tool-danger)
  would forbid an inferred one), and therefore no receipt. The capability returns
  `unsupported` for that binding and the client hides the drag handles — the same
  absent-not-disabled rule §5.4 applies to argument editing.

Stated plainly, because it is the sort of thing a reader will otherwise assume
the other way round: **the flagship agent is the one that cannot reorder or edit
its queue, and every other agent can, precisely because omt is holding their
queue for them.** That inversion is real and is worth naming rather than
smoothing over.
### 8.1 `AgentEvent` — the envelope

Everything above, plus the whole content of the transcript view, travels as one
type. `AgentEvent` is the **normalized per-binding agent stream**: the single
output of the merge in §4 and the single input of every read-side consumer.

> **Ownership.** This document owns `AgentEvent`, `AgentPayload` and their inner
> types. [07 §3.7](07-remote-protocol.md#37-subscriptions) wraps an `AgentEvent`
> in the protocol `Event` envelope under `kind: "agent"` and owns nothing about
> its shape. `web/src/generated/events.ts` ([08 §2.1](08-web-client.md#21-what-codegen-emits))
> is generated from these types. Serde conventions are the glossary's:
> `snake_case` fields and variants, `type` as the payload tag.
>
> Other documents write `AgentEvent::FileChanged` as shorthand; the exact path is
> `AgentPayload::FileChanged`, carried in an `AgentEvent`. The two spellings mean
> the same thing and the field is `turn`.

```rust
pub struct AgentEvent {
    // ---- identity ----
    pub session: SessionId,
    /// Which binding produced this. Everything the read side keys by binding —
    /// 15 §8.3's changed-file set, the queue, `agent.explain` — keys on this,
    /// and clears when the binding ends (§2).
    pub binding: BindingId,
    pub agent: AgentKind,
    pub agent_version: Option<String>,
    /// The agent's own session identifier, when known (§7.2).
    pub agent_session: Option<AgentSessionId>,
    /// Which thread inside the binding. Subagents are threads, not bindings.
    pub thread: ThreadRef,

    // ---- ordering and provenance ----
    /// The session's `Seq` space ([03 §4](03-capability-catalog.md#4-events-are-the-read-side-twin)).
    pub seq: Seq,
    pub ts: Timestamp,
    /// The tier that produced this event, and the specific source inside it.
    /// `tier` is what §4's merge and 08 §4.4's provenance chip read; `source`
    /// is what `agent.explain` reports.
    pub tier: Tier,
    pub source: SourceId,

    // ---- place ----
    pub cwd: PathBuf,
    pub git_branch: Option<String>,

    pub payload: AgentPayload,
}

/// Subagent trees. Unifies Claude Code's `isSidechain`/`parent_tool_use_id`,
/// Codex's subagent thread source, and opencode's `session.parent_id` (§8).
pub struct ThreadRef {
    pub id: ThreadId,
    pub parent: Option<ThreadId>,
    pub is_subagent: bool,
    /// The agent's own label for the subagent, verbatim. omt never invents one.
    pub label: Option<String>,
}
```

**On the duplication with the protocol envelope.** `session`, `seq`, `ts` and a
coarse `source` tag also appear in 07's `Event` envelope. That is deliberate and
is not drift: the agent event log is persisted standalone
([21 §1](21-data-lifecycle.md), row 7 — `agent.jsonl.zst`) and must be readable
without the protocol frame that once carried it. The rule is that the values are
**one value written twice**: the wrapping envelope copies them, never recomputes
them, and codegen emits an equality assertion. `AgentEvent::tier` is the precise
form; the envelope's `source` tag is the same value widened to the closed
producer set of [07 §3.7](07-remote-protocol.md#37-subscriptions).

### 8.2 `AgentPayload` — the variants

Twenty variants in seven groups. Every one is consumed by a named document
(§8.3); nothing here exists speculatively.

```rust
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentPayload {
    // ================= lifecycle =================
    SessionStart {
        reason: StartReason,
        model: Option<String>,
        /// The agent's own permission posture, verbatim and read-only (D1).
        permission_mode: Option<String>,
        transcript_path: Option<PathBuf>,
    },
    SessionEnd { reason: SessionEndReason, code: Option<i32> },
    /// What this agent can do, as the agent itself reports it. Never probed,
    /// never guessed (P4).
    Capabilities {
        tools: Vec<String>,
        slash_commands: Vec<SlashCommand>,
        mcp_servers: Vec<McpServerStatus>,
        models: Vec<String>,
        /// The modes the agent offers, and which one is live.
        permission_modes: Vec<String>,
        active_permission_mode: Option<String>,
    },

    // ================= turn state =================
    TurnStart { turn: TurnId, trigger: TurnTrigger },
    TurnEnd {
        turn: TurnId,
        outcome: TurnOutcome,
        /// The agent's own closing message, when the mechanism reports it
        /// (Claude Code's `Stop` hook carries `last_assistant_message`).
        last_message: Option<String>,
        turn_count: Option<u32>,
    },
    /// Extended thinking / reasoning. Subsumes the draft model's separate
    /// `Thinking` variant — see the note below.
    Reasoning { turn: Option<TurnId>, text: Option<String>, partial: bool,
                tokens: Option<u64> },
    /// The fields of [20 §11.2](20-recall-and-usage.md#112-normalization)'s
    /// `UsageEvent` that are not already in the envelope. **`cost_usd` is
    /// agent-reported only**; omt never computes a price from a token count and
    /// a rate card, and `None` is rendered as *not reported*, never as 0.
    Usage { model: Option<String>, tokens: Tokens, cost_usd: Option<f64>,
            context_window: Option<u64>, accounting: Accounting },
    /// Observed from the agent, never inferred (20 §11.3's `RateLimitState`).
    RateLimit { kind: String, status: String, resets_at: Option<Timestamp>,
                detail: Option<String> },
    Compaction { phase: CompactionPhase, trigger: Option<String>,
                 tokens_before: Option<u64>, tokens_after: Option<u64> },

    // ================= content =================
    UserMessage { turn: Option<TurnId>, text: String, origin: MessageOrigin,
                  attachments: Vec<BlobId> },
    AssistantText { turn: Option<TurnId>, text: String, partial: bool,
                    phase: Option<AssistantPhase> },

    // ================= tools =================
    ToolCall {
        turn: Option<TurnId>,
        call: ToolCallId,
        name: String,
        /// Verbatim, unmodified — the same bytes the agent will act on.
        input: serde_json::Value,
        status: ToolStatus,
        /// Set when this call was made by a tool, not by the model directly.
        parent: Option<ToolCallId>,
    },
    ToolResult { call: ToolCallId, outcome: ToolOutcome,
                 output: Option<ToolOutput>, error: Option<String>,
                 duration_ms: Option<u64> },
    /// The agent's own plan / todo list, as it revises it.
    Plan { turn: Option<TurnId>, steps: Vec<PlanStep> },
    /// Attribution, not truth about the filesystem — git status is the truth
    /// ([15 §8.3](15-workspace-explorer.md#83-files-the-agent-changed-this-session)).
    /// `path` is absolute as the agent reported it; the explorer maps it to a
    /// `RelPath` against the workspace root and drops what falls outside.
    FileChanged { path: PathBuf, change: FileChange, tool: Option<String>,
                  turn: Option<TurnId> },

    // ================= interaction =================
    /// A card was raised. The `Interaction` object itself is owned by §5 and
    /// lives in the ledger; this payload **references** it and restates only
    /// what a transcript needs to render a row in place.
    InteractionRaised { interaction: InteractionId, kind_tag: InteractionKindTag,
                        summary: String, timeout_at: Option<Timestamp> },
    /// The card reached a terminal state. `state` is §5's `InteractionState`,
    /// which already carries `by`, `at`, `response` and the reason.
    InteractionResolved { interaction: InteractionId, state: InteractionState },

    // ================= queue =================
    /// D15's `agent.queue.enqueue` semantics. Carries the `BindingId` of the
    /// binding the queue belongs to — which is the envelope's `binding`, so it
    /// is not repeated here; a consumer that has the payload has the binding.
    QueueChanged { op: QueueOp, entry: Option<QueueEntry>, pending: QueueView },

    // ================= fallback =================
    /// The agent's own notification — Claude Code's `Notification` hook, a BEL
    /// with text, an OSC 777 body. Never omt's words.
    Notification { kind: String, message: String },
    /// The heuristic floor's guess (§6). Structurally incapable of carrying
    /// content: it holds `Activity` and nothing else, and it is the **only**
    /// payload a tier-0 source can construct (§8.4).
    Activity { guess: Activity },
}

pub enum StartReason { Startup, Resume, Clear, Compact, Fork }
pub enum SessionEndReason { UserExit, Error, Timeout, Replaced, Unknown }
pub enum TurnTrigger { Human, Queue, Remote, Hook, Subagent }
pub enum TurnOutcome { Completed, Aborted, Error { message: String } }
pub enum MessageOrigin { Human, Queue, Remote, Synthetic }
pub enum AssistantPhase { Commentary, Final }
pub enum ToolStatus { Pending, Running, Completed, Error }
pub enum ToolOutcome { Ok, Error, Denied, Cancelled }
pub enum CompactionPhase { Before, After }
pub enum FileChange { Created, Modified, Deleted, Renamed { from: PathBuf } }
pub enum QueueOp { Enqueue, Remove, Consume }
pub struct QueueEntry {
    pub id: QueueEntryId,
    pub text: String,
    pub origin: MessageOrigin,
    pub created_at: Timestamp,
    /// D15 consequence 3: an entry past this is re-confirmed, never silently
    /// flushed.
    pub valid_until: Option<Timestamp>,
    pub state: QueueEntryState,
}
pub enum QueueEntryState { Pending, Queued, Consumed, Failed { reason: String } }
/// Makes §8's fourth queue requirement unrepresentable-to-violate: a broken or
/// absent reader cannot claim an empty queue, because "empty" and "I do not
/// know" are different values.
pub enum QueueView { Known(Vec<QueueEntry>), Unknown }
/// The discriminant of §5's `InteractionKind`, without the payload — so the
/// transcript can render "a permission card was raised here" without this
/// stream becoming a second copy of the ledger.
pub enum InteractionKindTag { Choice, Permission, PlanReview, Text }
```

`Tokens`, `Accounting`, `SlashCommand` and `BlobId` are owned elsewhere
([20 §11.2](20-recall-and-usage.md#112-normalization), §7, [09 §2](09-ssh-and-media.md#2-the-blob-store))
and are referenced, not restated.

**Three choices recorded here.**

1. **`Thinking` is folded into `Reasoning`.** The earlier draft
   ([`agent-clis.md` §12.2](../research/agent-clis.md)) had both: `Thinking`
   under turn state carrying a token delta, and `Reasoning` under content
   carrying text. They are the same phenomenon observed through two mechanisms,
   and two variants would mean every consumer handles both identically or
   handles them inconsistently. One variant carries both `text` and `tokens`,
   either of which may be absent. The *spinner-level* fact "the agent is
   thinking right now" is not an event at all — it is `AgentState::Working`.
2. **Interactions appear here as raise + terminal outcome only.** Intermediate
   transitions (`Open → Resolving → Submitted`) travel on the protocol's
   `interaction` kind ([07 §3.7](07-remote-protocol.md#37-subscriptions)), not
   here. A transcript needs to render a card and how it ended; a client tracking
   a live card subscribes to `interaction`. Duplicating every transition into
   both streams would give two orderings of the same fact.
3. **`InteractionRaised` does not inline the `Interaction`.** It carries the id,
   the kind tag and a one-line `summary` for the transcript row. The object is
   fetched from the ledger (`interaction.get`) or arrives on the `interaction`
   kind. §5 is the single source of truth for the shape, and a second inlined
   copy in a persisted log would age badly against the ledger's `state`.

**Deliberately omitted**, so their absence is a decision rather than an
oversight:

- The draft's `RawPty { state_guess }` — replaced by `Activity`, which carries
  §6's enum and cannot be extended with text without changing §6 first.
- An `Error` payload. 20 §8.1's `EntryKind::Error` is *derived*, from
  `TurnEnd { outcome: Error }`, `ToolResult { outcome: Error }` and
  `SessionEnd { reason: Error }`. Instance- and session-level faults are
  [22 §4.4](22-operations.md#44-per-session-fault-isolation-r7)'s `SessionFault`
  and are not agent events.
- A `Diff` field on `FileChanged`. The draft carried `diff: Option<String>`;
  structured diffs are [15 §3.2](15-workspace-explorer.md#32-vcs-model)'s
  `FileDiff`, fetched on demand, and a unified-diff string on every file write
  would multiply the agent log's size for data git already has.
- `SessionSummary`. `away_summary` is carried as `Notification`-adjacent session
  metadata on the binding, not as a stream event; 20 §8.2's `AgentSummary` reads
  it from there.

### 8.3 Who consumes each payload

Every row names a document that needs the variant. A variant with no consumer
does not ship.

| Payload | Consumed by |
|---|---|
| `SessionStart` / `SessionEnd` | [20 §8.1](20-recall-and-usage.md#81-the-timeline) `EntryKind::SessionStart`/`SessionEnd`; §2's binding lifecycle; [08 §4.4](08-web-client.md#44-transcript-view) transcript head |
| `Capabilities` | [08 §6.2](08-web-client.md#62-slash-commands-as-a-real-completion-popup) completion popup; `agent.commands.list` (§8); §10 Q4 / [13 §7.2](13-security.md#72-remotely-resolving-an-agent-interaction) permission posture |
| `TurnStart` / `TurnEnd` | 20 §8.1 `EntryKind::Turn`; 08 §4.4 turn grouping; §4's `Working`/`Idle` transitions; [20 §10.1](20-recall-and-usage.md#101-the-detector) `StuckReason::LongWorking` |
| `Reasoning` | 08 §4.4 (collapsed by default); 20 §11 token accounting |
| `Usage` | [20 §11.2](20-recall-and-usage.md#112-normalization) `UsageEvent`, `usage.query`, 20 §9's `ComparisonRow`; [08 §6](08-web-client.md#6-agent-session-dashboard) readout |
| `RateLimit` | 20 §11.3 `RateLimitState`; the session card |
| `Compaction` | 20 §8.1 `EntryKind::Compaction`; 08 §4.4's gap marker; [20](20-recall-and-usage.md)'s `Coverage` |
| `UserMessage` | 08 §4.4; [20 §3.2](20-recall-and-usage.md#32-schema) `DocKind::user_msg` |
| `AssistantText` | 08 §4.4; 20 `DocKind::assistant_msg` |
| `ToolCall` / `ToolResult` | 08 §4.4; 20 `DocKind::tool_call`, `EntryKind::ToolCall`, `StuckReason::RepeatedCall`/`RepeatedFailure` |
| `Plan` | 08 §4.4 inline plan rows; [20 §8.2](20-recall-and-usage.md#82-the-digest) |
| `FileChanged` | [15 §8.3](15-workspace-explorer.md#83-files-the-agent-changed-this-session) changed-file set; 20 §8.1 `EntryKind::FileChanged`; 20 §9 `files_touched`; 20 `DocKind::file_change` |
| `InteractionRaised` / `InteractionResolved` | 08 §4.4 inline cards; 20 §8.1 `EntryKind::Interaction` + `ResolutionSummary`; 20 `DocKind::interaction`; [D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism) consequence 9's attention log |
| `QueueChanged` | [08 §6.1](08-web-client.md#61-live-message-queue) live queue; §8's `agent.queue.*`; D15's confirm-by-observation |
| `Notification` | [D12](decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead)'s open-and-replay attention list; 20 §8.2 digest |
| `Activity` | §4's merge input only. **Never rendered as content** — it produces an `AgentState`, and 08 renders that as a chip |

### 8.4 Which tier may produce which payload

§4 rule 3 — *a lower tier may never contradict a live higher tier* — is a merge
rule. This table is the stricter, structural companion: which payloads a source
at a given tier is permitted to construct **at all**.

| Payload group | Heuristic 0 | Process 1 | Marker 2 | Transcript 3 | Hook 4 | Protocol 5 |
|---|:--:|:--:|:--:|:--:|:--:|:--:|
| `Activity` | **only this** | — | — | — | — | — |
| `SessionStart` / `SessionEnd` | — | ✓ | ✓ | ✓ | ✓ | ✓ |
| `Notification` | — | — | ✓ | ✓ | ✓ | ✓ |
| `Capabilities` | — | — | — | ✓ | ✓ | ✓ |
| `TurnStart` / `TurnEnd` / `Compaction` | — | — | — | ✓ | ✓ | ✓ |
| `UserMessage` / `AssistantText` / `Reasoning` | — | — | — | ✓ | ✓ | ✓ |
| `ToolCall` / `ToolResult` / `Plan` / `FileChanged` | — | — | — | ✓ | ✓ | ✓ |
| `Usage` / `RateLimit` | — | — | — | ✓ | ✓ | ✓ |
| `InteractionRaised` / `InteractionResolved` | — | — | — | ✓ ¹ | ✓ | ✓ |
| `QueueChanged` | — | — | — | ✓ | ✓ | ✓ |

¹ A transcript source observes an interaction **retroactively**, so its
`InteractionRaised` may arrive after the card has already closed. The ledger
treats a raise for an interaction it has never seen and whose window has passed
as history, not as a new open card.

**Enforced by the type system where it matters, by one table elsewhere.** The
tier-0 ban is the row that is a security property (§6: a wrong card a user taps
"Allow" on is an incident), so it is structural: a heuristic source does not get
an `EventSink` at all.

```rust
/// Tier-0 sources implement this instead of `EventSource` (§3). There is no
/// method on `ActivitySink` that accepts an `AgentPayload`, so "screen text
/// became a tool call" is not a bug that can be written.
#[async_trait]
pub trait HeuristicSource: Send + Sync {
    fn id(&self) -> SourceId;
    fn supports(&self, kind: AgentKind) -> bool;
    async fn attach(&self, binding: &AgentBinding, sink: ActivitySink)
        -> Result<SourceHandle, SourceError>;
}

impl ActivitySink {
    /// The only emit method that exists.
    pub fn emit(&self, guess: Activity);
}
```

The remaining rows are enforced by one table-driven check inside `EventSink`,
which rejects an out-of-tier payload with `SourceError::TierViolation`, disables
that source for the binding, and reports it through `agent.explain` — the same
handling as a transcript schema mismatch (§9). Six marker types for forty
cells would buy less than the fixture test that walks this table does.

### 8.5 Sufficiency for the transcript view

[D14](decisions.md#d14--agent-sessions-get-a-transcript-surface-blocks-are-for-shell-work)
makes the transcript view the primary mobile surface for an agent session, so
the set above has to be enough to render a readable conversation without the
grid. It is, and here is the mapping
([08 §4.4](08-web-client.md#44-transcript-view) renders it):

#### Streaming granularity is a property of the source, not of the model

`AssistantText` and `Reasoning` both carry `partial`, and the renderer coalesces
`partial: true` fragments into one row — so token-by-token streaming is a
first-class shape in the model. What a surface actually *receives*, however,
depends on which tier is feeding it, and an implementer should not go looking
for a bug when the answer is physics:

| Session | Source | Granularity |
|---|---|---|
| `native` (ACP) | `session/update` → `agent_message_chunk` | **token-level** — the agent streams chunks and omt forwards them as `partial` |
| `pty`, terminal view | the PTY byte stream | **token-level** — this *is* the agent typing; nothing is reconstructed |
| `pty`, transcript view | transcript tailing | **message-level** — an agent writes its JSONL a message at a time, not a token at a time. There is no finer signal to tail |
| `pty`, hooks | hook events | **event-level** — hooks fire on lifecycle boundaries, not on content |

So a `pty` session's transcript view shows an assistant message when it lands,
while its terminal view shows the same text appearing character by character.
Both are correct; they are different observations of the same thing. Where an
agent offers a genuine streaming channel omt uses it, and where it does not, omt
does not fabricate one by re-deriving tokens from the screen — that would be
tier-0 content, which [§6](#6-the-heuristic-floor) forbids.

| Row a reader expects | Built from |
|---|---|
| "I asked this" | `UserMessage`, with an omt-typed chip when `origin` is `Remote` or `Synthetic` |
| "it said this" | `AssistantText`, coalescing `partial: true` fragments into the final row |
| "it thought about it" | `Reasoning`, collapsed by default |
| "it ran this, and this happened" | `ToolCall` + the `ToolResult` with the same `call`, rendered as one expandable row |
| "it is planning" | `Plan`, re-rendered in place on each revision |
| "it changed these files" | `FileChanged`, grouped per turn, tapping through to 15's diff |
| "it asked me something" | `InteractionRaised` → the inline card (08 §5), replaced in place by `InteractionResolved`'s outcome |
| "a subagent did this" | any payload with `thread.is_subagent`, nested under the `ToolCall` whose id is `thread.parent` |
| turn boundaries, elapsed time, token cost | `TurnStart`/`TurnEnd`/`Usage` as a turn footer |
| "the context was compacted here" | `Compaction`, as a divider — which is also where the transcript legitimately loses history |
| "this session started / ended" | `SessionStart`/`SessionEnd` |
| provenance of any row | `AgentEvent::tier`, per 08 §4.4 |

What the set deliberately cannot render is a heuristic-tier session, because
`Activity` is all there is. That is D14's stated floor and §7.3's "grid only"
column, and it is shown to the user rather than discovered.

---

## 9. Failure modes and their handling

| Failure | Behaviour |
|---|---|
| omt daemon dies while an interaction is open | `omt-hook` times out and fails open; the agent shows its own card. On restart omt reconciles: an interaction with **no decision recorded** (`Open`) becomes `Abandoned { daemon_restart }`; one with a decision recorded (`Resolving` or `Submitted`) becomes `Undelivered { reason: DaemonRestart, response }`, preserving the text. **Never `Cancelled`, and never retried.** See below. |
| Hook installed but socket unreachable | hook exits 0 in <5 ms; state falls back to transcript/heuristics; `agent.explain` reports the degradation |
| Two sources disagree | higher tier wins; the disagreement is recorded and visible in `agent.explain` |
| Agent version changes its transcript schema | transcript reader is versioned and validates; on parse failure it disables itself for that binding and reports, rather than emitting garbage |
| User runs an agent inside tmux inside omt | binding is `Unknown`; documented as unsupported for observation; the terminal still works |
| Agent spawns subagents | separate threads under one binding; state is the aggregate (any subagent blocked ⇒ session needs you) |
| Injected answer written, no confirming observation | `Undelivered { reason: NotConfirmed, response }` after the bounded window; surfaced as *"your answer may not have reached the agent — check the terminal"* with the terminal view offered. Never auto-retried (§5.1) |
| **`native` session: the ACP transport closes** | not `Orphaned`-by-PTY-death — there is no PTY (§2.1). The binding ends, open interactions are `Abandoned`, and the session is shown as disconnected with the **agent's own resume** as the recovery path (ACP `session/load` against the agent's session id), never a synthetic replay by omt |

**Why a daemon restart is not `Cancelled`.** The earlier design marked these
`Cancelled { daemon_restart }`, which was wrong three times over.
[12 §4.1](12-collaboration.md#41-the-invariant) defines `Cancelled` as a
decision *by an actor* — and here no actor decided anything; the daemon fell
over. It also discards a decision the user was told had been accepted, reporting
"cancelled" for an answer that may well have reached the agent. And it could not
have done better anyway, because `Resolving` did not carry the response, so there
was nothing left to report. `Resolving` and `Submitted` now carry it (§5), so
the honest states are available: `Abandoned` where nobody decided, `Undelivered`
where somebody did.

---

## 10. Open questions

1. **Does Claude Code's `AskUserQuestion` card accept a position-independent
   selection** (typing a number or letter), or does it require counting arrow
   keys? This is the highest-priority experiment
   ([D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)):
   its answer decides whether the flagship remote-answer path is on by default.
   Tracked in `../research/spike-card-answering.md` (pending). The `defer` spike
   it replaced is demoted to an optional optimization (§5.3).
2. Do Codex/Cursor/Gemini hook payloads match the Claude-Code shape closely
   enough for one normalizer? Verify with a logging hook.
3. Does hook-emitted OSC actually reach omt's PTY for non-Claude agents, or is
   hook stdout swallowed?
4. **Surfacing each agent's own permission posture.** [13 §7.2](13-security.md#72-remotely-resolving-an-agent-interaction)
   requires omt to display the agent's active permission mode read-only on every
   surface. Each CLI names it differently (`permissionMode`,
   `--dangerously-skip-permissions`, ACP `session/set_mode`, Codex's approval
   policy). What is the normalized shape, and can it be read reliably for each?
5. opencode `session_input` table — is it the type-ahead queue? Would extend the
   queue feature beyond Claude Code.
6. Codex `app-server`: are approvals first-class methods there? If so it is a
   better responder than hooks for Codex.
7. Goose/Crush session DB schemas, for transcript-tier support.
8. Whether Amp's `--stream-json` is byte-compatible with Claude Code's, or only
   structurally similar.
9. **Block attribution when two sources disagree** — carried over from
   [04 §11.5](04-terminal-core.md#11-open-questions). Proposed rule:
   hook/protocol-sourced attribution outranks writer-token attribution, and the
   loser is retained as `Attribution::contested`. Needs agreement across 04, 06
   and [12](12-collaboration.md).
10. **Writer-token pre-emption by an agent** — carried over from
    [05 §13.3](05-session-model.md#13-open-questions). When a hook-channel
    resolution completes and the agent's next action is a PTY write while a
    human holds the token: queue behind the human (current lean, with a visible
    indicator) or pre-empt?
