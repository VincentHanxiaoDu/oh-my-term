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
    /// Advisory: who is currently looking at this card. See 12 §4.4.
    pub viewers: Vec<ActorId>,
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
to a human, and the offered action is "open the terminal view", never "retry".
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
arrow keys. **That spike is in flight**; its record is
[`../research/spike-card-answering.md`](../research/spike-card-answering.md)
(pending — it may not exist yet). If the card is arrow-key-only the responder is
`Inferred`, D3 disables it by default, and the remote surface shows the card as
observed-but-not-answerable with the terminal view one tap away. The same
Claude Code session in `native` mode has a native responder and no such gate,
which is the honest reason `native` exists.

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

| Agent | Binding | State | Interactions | Queue | Commands | Mobile surface |
|---|---|---|---|---|---|---|
| Claude Code | env marker | hooks (30 events) | **Choice + Permission**, synthetic delivery (§5.2) | native (`queue-operation`) | `system/init` + disk | **transcript** + cards |
| Codex | env marker | hooks + app-server | Permission, native | — | app-server | **transcript** + cards |
| opencode | env/argv | plugin + REST/SSE + ACP | Permission, native | uncertain | ACP `available_commands_update` | **transcript** + cards |
| Gemini CLI | argv | hooks + ACP | Permission, native | — | ACP | **transcript** + cards |
| Qwen Code | argv | inherits Gemini | Permission, native | — | ACP | **transcript** + cards |
| Cursor | env marker | hooks | Permission, native | — | disk | **transcript** + cards |
| Goose | argv | ACP | Permission, native | — | ACP | **transcript** + cards |
| Amp | argv | stream-json | Permission, degraded | — | — | **grid only** — status, no transcript |
| Aider | argv | transcript + heuristics | Text, synthetic | — | static list | **grid only** — status, no transcript |
| Crush | argv | heuristics | none | — | — | **grid only** — status, no transcript |

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
  `AgentEvent::FileChanged { path, change, tool, turn_id }`, where `change` is
  `Created | Modified | Deleted | Renamed { from }`. Every tier-3/4/5 source
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
[12 §4.1](12-collaboration.md#41-the-state-machine) defines `Cancelled` as a
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
