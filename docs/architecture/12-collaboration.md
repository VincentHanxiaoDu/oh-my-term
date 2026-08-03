# Concurrency and Collaboration

Multiple humans, on multiple devices, plus multiple agents, act on one instance
at the same time. This document is the authoritative specification of what
happens when they collide.

It is the runtime half of principle
[P6](01-principles.md#p6--collaboration-is-a-runtime-feature-not-just-a-workflow).

Related: [Decision log](decisions.md) ·
[03 — Capability catalog](03-capability-catalog.md) ·
[05 — Session model](05-session-model.md) ·
[06 — Agent layer](06-agent-layer.md) ·
[07 — Remote protocol](07-remote-protocol.md) ·
[13 — Security model](13-security.md)

> **Authority note.** [05 — Session model](05-session-model.md) describes the
> writer token as part of a session's data model — its fields, its persistence,
> its place in the tree. **This document defines its semantics**: who may
> acquire it, what takeover means, what the timeouts are, and how every conflict
> resolves. Where the two disagree, this document wins and 05 is the bug.

---

## 1. Actors

Everything that can cause a state change is an `Actor`, and every state change
records one. There are no anonymous mutations.

```rust
pub struct Actor {
    pub id: ActorId,
    pub kind: ActorKind,
    /// Human-facing label, e.g. "iPhone (Vincent)", "laptop TUI", "hook".
    pub label: String,
    pub role: Role,                  // Viewer < Operator < Admin
    pub credential: Option<CredentialId>,
}

pub enum ActorKind {
    /// The in-process TUI on the machine running the daemon.
    Local { pid: u32 },
    /// A remote client over a Transport.
    Remote { device: DeviceId, transport: TransportKind },
    /// The CLI over the unix socket (short-lived, one command).
    Cli { uid: u32 },
    /// A plugin acting on its own behalf.
    Plugin { plugin: PluginId },
    /// The daemon itself: timeouts, policies, restore.
    System,
    /// An agent process, via a hook or native protocol.
    Agent { session: SessionId, agent: AgentKind },
}
```

`Actor` is carried in `CallContext` (see
[03 §3](03-capability-catalog.md#35-the-dispatch-path)), stamped onto every event, and
written to the audit log (§8). An `ActorId` is stable for the lifetime of a
connection; a device that reconnects gets a new `ActorId` but keeps its
`DeviceId`, which is what presence and audit group by.

---

## 2. Presence is first-class state

Presence is not a UI nicety layered on top; it is part of the session tree,
persisted for the duration of the connection, queryable via `session.list` and
`presence.list`, and broadcast as events like anything else.

```rust
pub struct Presence {
    pub actor: ActorId,
    pub device: DeviceId,
    pub label: String,
    pub role: Role,
    pub connected_at: OffsetDateTime,
    /// What this actor is currently looking at. A client may view several.
    pub viewing: Vec<ViewFocus>,
    /// Sessions where this actor currently holds the writer token.
    pub writing: Vec<SessionId>,
    pub client: ClientInfo,          // kind (web/tui/cli), version, platform
    pub liveness: Liveness,          // Active | Idle { since } | Background | Stale
}

pub struct ViewFocus {
    /// The shared identity, and what every surface actually highlights.
    pub session: SessionId,
    /// Which *arrangement* the actor is looking at this session in.
    pub view: ViewId,
    /// The surface being used, because it changes what the actor can see.
    pub surface: Surface,            // Terminal | Blocks | Transcript | InteractionCard | Dashboard
    /// Reported viewport; drives the size policy in 07 §4.3.
    pub viewport: Option<TermSize>,
}
```

**`ViewFocus` is keyed on `(ViewId, SessionId)`, not on `PaneId`.** With
per-view layouts ([17 §3.3](17-panes-and-layout.md#33-decision-per-client-layout-views-one-shared-default))
a pane belongs to exactly one view, so a `PaneId` from the phone's `Adaptive`
view names nothing the laptop can render — presence would be structurally
unable to say "the phone is watching this". The `SessionId` crosses views
intact; the `ViewId` is carried so a client can tell "watching the same
arrangement I am" from "watching the same session in their own arrangement",
which are different social facts. [05 §4](05-session-model.md#4-attachment-detach-and-multi-client-viewing)'s
`ClientView::viewing` carries the same pairs.

Rules:

- **Presence is broadcast, not polled.** `presence.changed` events fire on
  attach, detach, focus change, viewport change (debounced 250 ms), liveness
  change, and writer changes.
- **`liveness` is derived, not reported.** `Active` = input or focus change
  within 60 s; `Idle` after that; `Background` when the client declared a
  backgrounded tab ([07 §5.4](07-remote-protocol.md#54-mobile-specifics));
  `Stale` when keepalive has been missed but the grace window has not expired.
- **Every surface shows it.** The TUI renders attached remotes in the pane
  border ("👁 iPhone"), the web client renders an avatar row, the CLI prints it
  in `omt session list --presence`. A user must never be surprised that someone
  else is watching.
- **Viewers count.** A `Viewer`-role actor appears in presence exactly like an
  operator, with its role visible. Read access to a terminal is not innocuous —
  it can include secrets — and hiding it would be dishonest.

Presence is *not* persisted across daemon restart. On restart, presence is empty
and clients repopulate it by reconnecting. That is deliberate: presence is
connection state, and a persisted copy would assert that someone is connected
when nobody is.

Consequently, **presence is not where "when did I last see this?" lives.** A
durable last-seen position is a **read mark** per `(actor, session)`, owned by
[20](20-recall-and-usage.md) — it survives restarts because it is a fact about
what a person has read, not about who is currently attached. `digest.since_last_seen`
reads the mark, never presence.

---

## 3. The writer token

### 3.1 What it governs

The writer token gates **every path that puts input into a session, whatever it
is dressed as**. The rule is not "is this capability named `write`" but "do the
bytes or the message end up in the one input channel the human is also using".

In a `pty` session that channel is the PTY, so the token gates the capabilities
carrying `Effects::WRITES_PTY` — `session.send_text`, `session.send_keys`,
`session.write_bytes`, and `session.resize` when it requests authoritative
sizing — **and every synthetic delivery**, below.

In a `native` session ([D8](decisions.md#d8--two-session-modes-pty-default-and-native-acp))
there is no PTY at all, and `agent.prompt` is the **only** input path. The token
gates `agent.prompt` there, exactly as it gates `session.send_text` in a `pty`
session — same rule, different channel. This is the normative statement that
[05 §5.1](05-session-model.md#51-the-rule) defers to. Nothing about `native`
mode weakens arbitration: two devices prompting one agent interleave two
conversations into one, which is the same failure as two people typing into one
shell, minus the visual evidence.

**`interaction.resolve` is gated by its delivery mechanism, not exempt.** An
earlier draft exempted it wholesale on the rationale that *"it goes back through
the agent's own channel"*. [D11](decisions.md#d11--omt-mirrors-the-agents-own-card-it-does-not-intercept-or-replace-it)
deleted that rationale: omt no longer parks the call and answers through the
hook's response slot; the agent draws its own card and the answer is delivered
to *that card*. So:

| [`Interaction.deliverable`](06-agent-layer.md#521-deliverable--a-field-of-interaction-computed-by-the-normalizer) | Channel | Token |
|---|---|---|
| `Native` — ACP `session/request_permission`, the opencode plugin, the Codex app-server | the agent's own RPC | **not gated**; the flagship "answer from your phone while someone else is at the keyboard" case is preserved exactly here |
| `Synthetic { requires_token }` — keystrokes at the agent's own TUI | the PTY | **gated**, as a transaction, below |
| `None { reason }` | — | not answerable remotely at all; the surface says "needs you" and shows the reason |

A `Synthetic` resolve is not a token-holding *write*, it is a **gated
transaction** per [D13](decisions.md#d13--synthetic-delivery-is-a-gated-transaction-never-a-bare-write):
acquire the token, verify input quiescence (no human bytes for a quiet period),
re-verify against the freshest source that the agent is still in the same
interaction, write the answer as one unit, release. The quiet period is
`injection_quiescence` (§3.3.1) and the re-verification is the six checks of
[§4.6](#46-preconditions-on-a-synthetic-delivery). Any check failing fails the
resolve with `conflict` or `precondition_failed`, visibly, on the surface that
attempted it; a partial write is never permitted. **The local TUI's passthrough
bytes into a session with an open interaction pass through this same
serialization point** — designed in [§3.5](#35-the-serialization-point), and the
only place the two writers can be ordered,
because the human at the keyboard is not a capability caller and the ledger
cannot see them. Delivery is confirmed by observation, never assumed: see §4.5.

It deliberately does **not** gate:

- `agent.prompt` in a `pty` session where the adapter has a structured submit
  path (ACP, stream-json stdin). Prompts are queued and serialized by the agent
  layer, not by the token. Where the *only* available path is synthesized
  keystrokes, the token **is** required, because that path is PTY input wearing
  a hat. (In a `native` session `agent.prompt` is always gated — see above.)
- reads of any kind, scrollback, search, layout, config.

### 3.2 State

```rust
pub struct WriterToken {
    pub holder: ActorId,
    pub acquired_at: OffsetDateTime,
    pub last_input_at: OffsetDateTime,
    /// Monotonic; increments on every successful acquire or takeover.
    pub epoch: u64,
    /// If true, acquiring did not change the PTY size (07 §4.3).
    pub keep_size: bool,
    /// Set while a takeover is pending.
    pub takeover: Option<PendingTakeover>,
}

pub struct PendingTakeover {
    pub requester: ActorId,
    pub requested_at: OffsetDateTime,
    pub grace_deadline: OffsetDateTime,
}
```

`epoch` is what makes late input safe: every PTY write carries the epoch the
client believed it held, and the daemon rejects writes whose epoch is stale.
Without it, a client whose token was taken over mid-flight could land keystrokes
in someone else's editing session.

### 3.3 Lifecycle

```
        ┌──────────┐  acquire (free)          ┌──────────┐
        │   Free   │ ───────────────────────► │   Held   │
        └──────────┘                          └──────────┘
             ▲                                  │      ▲
   release / │                     takeover req │      │ holder acts
   idle 90 s │                                  ▼      │  (cancels)
             │                          ┌───────────────────┐
             └────────────────────────  │ Held + Takeover   │
                 grace expires,         │     pending       │
                 requester becomes      └───────────────────┘
                 holder (new epoch)
```

| Operation | Capability | Semantics |
|---|---|---|
| **Auto-acquire** | — (implicit, on the first write) | The token is a **lease**. When the token is `Free`, the caller is `Operator`+, and **no other client has written to this session within `writer_quiet_period` (default 15 s)**, that client's first write acquires the token implicitly, with a new `epoch`, audited as a normal `acquire` by that actor. Governed by `WriterPolicy.auto_acquire`, default `true` ([05 §5.2](05-session-model.md) holds the field; the semantics are here). Never applies to `Viewer`. |
| **Acquire** | `session.writer.acquire` | Succeeds immediately if `Free`. Fails `conflict` if held, with the holder in `detail`. Requires `Operator`. |
| **Takeover** | `session.writer.acquire { force: true }` | Legal only for `Operator`+. Opens a `PendingTakeover` with a **grace window of 5 s**. Every surface shows a countdown. The holder may `session.writer.keep` to cancel it once per takeover; a second takeover request within 60 s cannot be cancelled. |
| **Release** | `session.writer.release` | Immediate; token becomes `Free`. |
| **Idle release** | — | After **90 s** with no input, the token auto-releases (`Actor::System`). Prevents a closed laptop from holding a session hostage. |
| **Disconnect** | — | Token released immediately on transport close; no grace, because the holder is provably gone. |
| **Daemon restart** | — | All tokens released. |

**Why the auto-acquire rule keys on recency and not on attachment count.** The
earlier rule was *"when exactly one client is attached"*, and
[D13](decisions.md#d13--synthetic-delivery-is-a-gated-transaction-never-a-bare-write)
supersedes it because it **never fires in
[D4](decisions.md#d4--single-user-many-devices--with-the-interfaces-left-open-for-many-users)'s
own primary scenario**: one person with a laptop and a phone always has two
clients attached, so under the old rule every single write on either device
would demand an explicit `session.writer.acquire` — the mechanism would be
maximally visible in exactly the case it was meant to be invisible in.
Recency gives the intended property (the mechanism disappears for one person
working alone, on however many devices) without giving up arbitration (the
moment two devices are *both* writing, the quiet period does not elapse and the
second one gets `conflict` with a takeover offer).

#### 3.3.1 Lease parameters

All tunable under `[session.writer]`; the defaults are **chosen here, not
measured**, and are expected to move once there is real use.

| Parameter | Default | What it bounds |
|---|---|---|
| `writer_quiet_period` | **15 s** | Auto-acquire. Long enough to cover reading output between commands and a slow typist's think-time; short enough that picking up the phone after glancing away is not a takeover ceremony. |
| `injection_quiescence` | **750 ms** | [D13](decisions.md#d13--synthetic-delivery-is-a-gated-transaction-never-a-bare-write) step 2: no *human* byte may have arrived at the session's input channel for this long before a synthetic write is emitted. Above a fast typist's inter-keystroke interval (~80–150 ms) with a wide margin, and below the ~1 s at which a remote tap starts to feel unacknowledged. |
| `injection_quiescence_wait` | **2 s** | How long the gated transaction *waits* for quiescence before giving up. On expiry the resolve fails `precondition_failed` with `detail.state = "local_input_active"` and the phone renders "couldn't answer — someone is typing at the terminal". It never waits indefinitely: a user typing steadily must produce a visible failure, not a write that lands minutes later. |
| `idle_timeout` | 90 s | Idle release (below). |
| `takeover_grace` | 5 s | Takeover (below). |

`injection_quiescence` and `injection_quiescence_wait` are the two numbers D13
requires and that no document previously gave. They are per-session overridable,
because a session driven over a high-latency link may need a larger quiet period
than one on the local machine.

The grace window exists so takeover is *polite by default and never blocked*.
Waiting forever for a consenting handoff fails the core use case (your laptop is
asleep in a bag); taking instantly fails the pairing case. Five seconds with a
visible countdown and one cancel is the compromise, and the `force` flag is
audited every time.

### 3.4 Visual indication is mandatory on every surface

Not a suggestion — a parity requirement checked in review:

| Surface | Indication |
|---|---|
| TUI | pane border colour + `✎ iPhone` in the pane title; a full-width banner during a pending takeover with the countdown and a `[k]eep` binding |
| Web | input bar disabled with "driven by laptop TUI — tap to request", writer avatar highlighted, takeover countdown as a modal sheet |
| CLI | `omt session list` writer column; `omt session send-text` fails with an actionable error naming the holder and suggesting `--force` |

The rule the whole design rests on: **a user must never type into a session and
have the keystrokes silently go nowhere.** Every input path either succeeds,
fails loudly, or is visibly disabled before the user starts typing.

### 3.5 The serialization point

[D13](decisions.md#d13--synthetic-delivery-is-a-gated-transaction-never-a-bare-write),
§3.1 and §4.5 all rest on the existence of one place where the human at the
keyboard and an omt resolver can be ordered against each other. This section
designs it. It is a **design choice made here**, not a number quoted from
elsewhere.

**Where it lives.** It is the **session input gate** in `omt-session`, the
single function through which *every* byte reaches a `pty` session's PTY write
end — the local TUI's passthrough, `session.send_text` / `send_keys` /
`write_bytes` from any client, and the synthetic responder. There is exactly one
per session, it is the owner of the `WriterToken`, and the PTY's write half is
private to it: no other code in the daemon holds a writable handle. That
exclusivity is the whole mechanism. The local TUI is *not* exempt — under
[D11](decisions.md#d11--omt-mirrors-the-agents-own-card-it-does-not-intercept-or-replace-it)
the card in the pane is the agent's own, so the local user's keystrokes are the
second writer, and a passthrough path that bypassed the gate would reintroduce
exactly the `12\r\r` interleaving D13 exists to prevent.

```rust
/// The only writable handle to a `pty` session's input. One per session.
pub struct InputGate {
    pty_write: PtyWriteHalf,          // private; nothing else in the daemon holds one
    writer: WriterState,              // §3.2
    /// Timestamp of the last byte from a *human* source. Updated on every
    /// passthrough and every client write; read only by `inject`.
    last_human_byte_at: Instant,
    /// `Some` while a session has an interaction the ledger considers in flight.
    open_interaction: Option<InteractionId>,
}

pub enum InputSource { Human { actor: ActorId, epoch: u64 }, Synthetic { intent: IntentId } }

impl InputGate {
    /// The hot path. Epoch-checks, stamps `last_human_byte_at`, writes.
    pub fn write(&mut self, src: InputSource, bytes: &[u8]) -> Result<usize, InputError>;
    /// The gated transaction: quiescence, preconditions, ordered writes, all
    /// while nothing else can reach `pty_write`. Never partial.
    pub fn inject(&mut self, plan: &InjectionPlan) -> Result<Submitted, InjectError>;
}
```

**What it costs on the keystroke path.** In the common case — `open_interaction`
is `None` — `write` is an integer compare against the current epoch, one
`Instant` store, and the `write(2)` that was going to happen anyway. No lock is
contended, because in a `pty` session the gate is owned by that session's task
and `omt-session` is single-threaded per session by construction
([05](05-session-model.md)'s "synchronous, deterministic state machine"). The
added cost is a branch and a timestamp: **nothing measurable** against the
budget in [07 §7](07-remote-protocol.md#7-latency-budget), and well inside
[04 §9.1](04-terminal-core.md#91-targets)'s targets. Crucially, a local
keystroke is **never delayed to wait for a resolver**: the human always wins the
race for the gate, and it is the injection that backs off (`injection_quiescence`
above) and then fails visibly.

**What it does when no interaction is open.** Nothing beyond the above. It does
not buffer, does not batch, does not defer, and does not consult the ledger —
`open_interaction` is a field the ledger sets, not a query the gate makes. There
is no mode switch and no second code path: the gate is always in the loop, so it
cannot be "forgotten" for a session that acquires an interaction a moment later,
which a lazily-installed gate could be. `inject` is the only entry point that
reads `last_human_byte_at`, and it exists only for the `Synthetic` deliverable;
a `Native` resolve never touches the gate at all
(§3.1's table), and a `native` session has no gate because it has no PTY.

---

## 4. Interaction ownership

An `Interaction` (a question card, a permission request, a plan review) is the
highest-stakes concurrent object in the system: it maps onto a real decision
that a real agent is blocked on, and resolving it twice is not merely
inconsistent — it can approve something the user meant to deny.

### 4.1 The invariant

**An `Interaction` transitions from `Open` to a terminal state exactly once, by
exactly one actor, and the resolution is broadcast to every subscriber.**

`Interaction` and `InteractionState` are defined once, in
[06 §5](06-agent-layer.md#5-interactions--the-flagship-path), which owns the
type. This document owns their *concurrency semantics*. The state machine, for
reference:

```rust
pub enum InteractionState {
    Open,
    /// CAS won; the answer is committed as a decision but not yet written.
    /// **Carries the `response`**: a crash between the CAS and the write must
    /// be able to report what was lost
    /// ([D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)
    /// consequence 2).
    Resolving { by: Actor, at: OffsetDateTime, response: InteractionResponse },
    /// The bytes have been written to the delivery channel. **Not** proof the
    /// agent received them — see §4.5.
    Submitted { by: Actor, at: OffsetDateTime, response: InteractionResponse },
    /// omt **observed the agent record the answer**. The only success state.
    Resolved { by: Actor, at: OffsetDateTime, response: InteractionResponse },
    /// Written (or committed and then lost), with no confirming observation
    /// inside the bounded window. The response text is preserved.
    Undelivered { by: Actor, at: OffsetDateTime, response: InteractionResponse,
                  reason: UndeliveredReason },
    Cancelled { by: Actor, reason: CancelReason, at: OffsetDateTime },
    /// The agent went away (crashed, timed out and proceeded, or the user hit
    /// Esc in the TUI) before anyone answered. Terminal, distinct from
    /// Cancelled, because nobody decided anything.
    Abandoned { at: OffsetDateTime, detail: String },
}
```

Seven variants, matching [06 §5](06-agent-layer.md#5-interactions--the-flagship-path)
verbatim — that document owns the type, this one owns the transitions.
`Submitted` and `Undelivered` are not optional refinements: without them the
ledger `fsync`s `Resolved` and asserts a delivery it has not verified, which is
exactly the defect
[D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)
consequence 1 exists to fix. `UndeliveredReason` is
`NotConfirmed | DaemonRestart | PreconditionFailed` (06 §5).

Terminal states are `Resolved`, `Undelivered`, `Cancelled` and `Abandoned`.
`Resolving` and `Submitted` are both in-flight and both reject a second
resolver with `AlreadyResolved`.

`Interaction::viewers: Vec<ActorId>` is advisory presence on a card (§4.4).

The ledger lives in `omt-agent` and is guarded by a single mutex per session.
Resolution is a compare-and-swap on `state`:

```rust
impl InteractionLedger {
    /// The only mutation path. Returns Err(AlreadyResolved) for every caller
    /// after the first — including the local TUI.
    pub fn resolve(
        &self,
        id: InteractionId,
        by: Actor,
        response: InteractionResponse,
    ) -> Result<Resolution, LedgerError> {
        let mut e = self.entry(id)?;
        match &e.state {
            InteractionState::Open => {
                // 1. CAS `Open -> Resolving { by, at, response }`. The response
                //    is stored *before* anything is written, so a crash here is
                //    reportable rather than silent.
                // 2. Hand to the `Responder`. For `Synthetic` delivery that is
                //    §3.1's gated transaction (token, quiescence, the §4.6
                //    preconditions, re-verification, atomic write); for `Native`
                //    it is one RPC reply.
                // 3. On a successful write: `-> Submitted { by, at, response }`.
                //    A failed precondition instead leaves the interaction
                //    `Undelivered { reason: PreconditionFailed }` and returns
                //    `precondition_failed` to the caller. Nothing is retried.
                // 4. `Submitted -> Resolved` happens **elsewhere**, when the
                //    observation arrives (§4.5). `Submitted -> Undelivered
                //    { reason: NotConfirmed }` when the window elapses first.
                //    `resolve()` never writes `Resolved` itself.
                // Every transition emits an event.
            }
            InteractionState::Resolving { by, at, .. }
            | InteractionState::Submitted { by, at, .. }
            | InteractionState::Resolved { by, at, .. }
            | InteractionState::Undelivered { by, at, .. } =>
                Err(LedgerError::AlreadyResolved { by: by.clone(), at: *at }),
            InteractionState::Cancelled { .. } => Err(LedgerError::Cancelled),
            InteractionState::Abandoned { .. } => Err(LedgerError::Abandoned),
        }
    }
}
```

`Undelivered` rejects a later resolver exactly like `Resolved` does: the
decision was taken and the bytes are gone or unaccounted for, so a second
injection would land in whatever the agent is doing *now*. The only actor
permitted to re-answer is a human who can see the screen (§4.5).

`interaction.resolve` is **idempotent by
`(interaction_id, identity_or_device, intent_id)`** — a client-minted
`intent_id`, persisted client-side, plus the stable identity behind the actor
([D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)
consequence 6). A retry under the same key returns the original outcome with
`ok: true`; a different identity gets `conflict`.

The key was previously `(interaction_id, actor, response)` and **broke on
exactly the retry it was written for**: `ActorId` is minted per connection, so a
phone whose socket dropped mid-call reconnects as a *different actor* and its own
retry reads as a stranger overriding it — a spurious conflict banner in the one
case (flaky mobile network) where retry is normal. Keying on the device or
identity survives the reconnect; keying on `intent_id` rather than on `response`
means a retry is recognised as the same *intent* even where the response is not
byte-identical (D9's argument editing can rewrite it).

**Idempotency does not imply the write is replayable.** For a `Synthetic`
delivery the retry-safe answer is *"return what happened to the original
intent"*, never *"inject again"*: an injection is D15's externally-confirmed
class and is never retried by anything but a human who can see the screen (§4.5).

**`LedgerError` maps onto three distinguishable protocol errors, not one.**
[07](07-remote-protocol.md)'s error enum is closed, so the discrimination is
carried in `detail.state` rather than in new codes
([D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)
consequence 10):

| `LedgerError` | Code | `detail.state` | Rendering |
|---|---|---|---|
| `AlreadyResolved { by, at }` | `conflict` | `"resolved"` | "Answered by iPhone (Vincent) 0.2 s earlier." Someone else decided. |
| `Cancelled { reason }` | `conflict` | `"cancelled"` | "This question was withdrawn" / "timed out — the agent used its own default." A decision was taken away. |
| `Abandoned { detail }` | `conflict` | `"abandoned"` | "Too late — the agent continued without an answer." Nobody decided anything. |

Collapsing these onto a bare `conflict` — which the design did — leaves a phone
unable to tell "someone else answered" from "the agent gave up", and those call
for opposite next actions: trust the outcome, versus go look at the terminal.
`detail` additionally carries `by` and `at` for `resolved`, and `reason` for
`cancelled`.

### 4.2 The race, concretely

The dangerous version is not two phones — it is a phone and the TUI, because
they resolve through *different mechanisms*:

```
t=0    Agent raises AskUserQuestion; omt observes it (PreToolUse hook, or an ACP
       request) and the ledger opens Interaction int_88 as `Open`. The hook does
       **not** defer (D11) — the walkthrough below is the `Native`-delivery case.
t=1    Card rendered in the TUI and on the phone.
t=2    Phone taps "Postgres".      TUI user presses Enter on "SQLite".
```

Resolution:

1. Both paths funnel into `interaction.resolve` — **the TUI has no privileged
   path** ([P3](01-principles.md#p3--parity-one-capability-three-surfaces)).
   The TUI's key handler dispatches the capability exactly as the phone does.
2. The ledger mutex serializes them. Whoever the mutex admits first wins.
3. The loser receives `conflict` with `detail.resolved_by`, and *because the
   winner's `InteractionResolved` event is broadcast anyway*, both surfaces
   converge on the same rendering within one round trip: the card becomes
   "Answered: Postgres — by iPhone (Vincent)".
4. The loser's surface additionally shows a transient, dismissible note: "Your
   answer was not applied — answered by iPhone 0.2 s earlier." Silent
   correction is unacceptable here; the user believes they made a decision.
5. Only the winning response reaches the agent, over the single in-flight
   request the `Native` channel is blocking on — an ACP
   `session/request_permission`, an opencode-plugin `permission.ask`, a Codex
   app-server approval.

The thing that makes this safe is that a `Native` channel holds exactly **one**
in-flight response slot per interaction and the daemon writes to it once — so
even a bug in the ledger cannot produce two decisions. (`omt-hook` used to
supply that slot; [D11](decisions.md#d11--omt-mirrors-the-agents-own-card-it-does-not-intercept-or-replace-it)
removed the hook's response slot entirely, which is what the note below is
about.)

> **That property no longer holds for the `Synthetic` path, and this walkthrough
> is now the `Native`-delivery case only.**
> [D11](decisions.md#d11--omt-mirrors-the-agents-own-card-it-does-not-intercept-or-replace-it)
> moved delivery from the hook's response slot to the agent's own card, and a PTY
> has no single-slot property: it accepts arbitrarily many writers and offers no
> compare-and-swap. The ledger still serializes omt's resolvers against each
> other; it **cannot** serialize omt against the human at the keyboard, who is
> not a capability caller. §4.5 is that case, and §3.1's gated transaction is
> what replaces the lost property.

### 4.3 Timeouts

`Interaction::timeout_at` comes from the agent, where the mechanism supplies one
(an ACP request's own deadline; a configured `askUserQuestionTimeout`). On expiry, `Actor::System` resolves it as
`Cancelled { reason: Timeout }`, which lets the agent fall back to *its own*
default behaviour.

**There is a third resolver, and it is not omt's.** Local and remote humans are
two; `Actor::System` above is omt's *own* timer firing on
`Interaction::timeout_at` and is a third. The fourth is the agent resolving its
own card with **no actor at all**: Claude Code's `askUserQuestionTimeout` /
`CLAUDE_AFK_TIMEOUT_MS` makes an `AskUserQuestion` card **auto-advance itself**
with whatever answers exist, emitting `tengu_ask_user_question_afk_auto_advance`
([`spike-card-answering.md` §2.1](../research/spike-card-answering.md)). This is
an event omt **observes** rather than one it causes, and the distinction is
load-bearing: omt's own timer is a state transition it performs, the agent's is
a fact it discovers from the event stream after the fact.

It is modelled as `Cancelled { by: Actor::Agent { .. }, reason:
CancelReason::AgentAutoAdvanced }`. Without that third resolver the ledger sees
a card resolve that it neither performed nor attributed, and reports a **phantom
conflict** — the phone is told "someone else answered" naming a human who did
nothing. Two consequences:

- The gated transaction refuses to inject when an AFK timeout is configured and
  the card is within `afk_margin` (**default 3 s**, chosen here) of it — the
  auto-advance would race the write ([§4.6](#46-preconditions-on-a-synthetic-delivery),
  spike §4 precondition 4).
- Where the agent exposes the configured timeout, omt renders it on the card as
  a deadline exactly like `timeout_at`; where it does not, the card carries no
  progress bar and the auto-advance surfaces only when it happens.

**Otherwise there is no omt-side policy resolution.** There is no omt-side
policy auto-resolution — no "always allow reads in this workspace" rule, no
allow-list, no auto-approve — per
[D1](decisions.md#d1--omt-adds-no-policy-layer-over-an-agents-permission-semantics).
Persistent rules of that shape belong to the agent CLI's own permission
configuration, which omt surfaces read-only and never overrides. Correspondingly
there is no per-credential interaction policy: `Operator` may resolve any
interaction the agent posed, `Viewer` may resolve none
([13 §4](13-security.md#4-roles-and-their-mapping-onto-the-catalog)).

### 4.4 Advisory viewer presence on a card

`Interaction.viewers` exists so that two humans do not both answer. It is
advisory — it does not lock anything — and it drives a small "laptop TUI is
looking at this" hint. Optimizing further (a soft lock, a claim) was rejected:
the interaction is *already* exactly-once, and a claim mechanism would add a
failure mode (a claimed-and-then-disconnected card) to solve a cosmetic problem.

### 4.5 The local keyboard racing an injected answer

This is the case [D11](decisions.md#d11--omt-mirrors-the-agents-own-card-it-does-not-intercept-or-replace-it)
created and the most dangerous one in this document. It is **not** §4.2, and
§4.2's mitigation does not cover it.

```
t=0    Agent raises AskUserQuestion. The agent's *own CLI* draws its *own* card,
       locally, in the pane. omt mirrors it to the phone.
t=1    Phone taps "SQLite". The ledger CAS succeeds; delivery is synthetic —
       omt must type at the agent's card.
t=2    The local user, looking at that same card, presses "2" then Enter.
```

Without serialization the two streams interleave into `12\r\r` and the agent
reads option "12". **Neither side observes a conflict**: the ledger records
resolved-by-phone, the agent did something else, and the audit log is false.
Silent divergence with a false record is worse than any visible failure.

**Why §4.2's rule is insufficient here.** C2's "immediately replace the
interactive card, then swallow the next Enter for 500 ms" protects only cards
**omt draws**. Under D11 the card in the pane is the *agent's*, drawn by the
agent's own renderer into the PTY. omt does not own it, cannot replace it, and
cannot swallow a keypress it never sees as a keypress — the local TUI's job in
`pty` mode is to pass bytes through. A swallow window is also the wrong shape: it
is a fixed-duration guess, and the hazard here is not a stale keypress arriving
late but two live writers overlapping.

**The resolution** is §3.1's gated transaction, whose preconditions are exactly
the ones this race violates
([D13](decisions.md#d13--synthetic-delivery-is-a-gated-transaction-never-a-bare-write)):

1. The injection acquires the **writer token**, and the local TUI's passthrough
   bytes into a session with an open interaction go through the same
   serialization point — the `InputGate` of [§3.5](#35-the-serialization-point).
   That is the only place a human and a resolver can be ordered against each
   other.
2. **Input quiescence** is verified — no human bytes for `injection_quiescence`
   (750 ms, §3.3.1) — so the `t=2` user who is mid-keystroke blocks the
   injection rather than colliding with it. The gate waits at most
   `injection_quiescence_wait` (2 s) and then fails visibly.
3. The interaction is **re-verified against the freshest source** as still open
   and still the same, so a local answer that landed first is detected before
   anything is typed. This is [§4.6](#46-preconditions-on-a-synthetic-delivery)'s
   six checks, which also catch the free-text focus hazard, an over-nine option
   list, and an alt-screen renderer.
4. The answer is written as **one unit** — one key per `write(2)`, never
   bracket-wrapped (§4.6) — then the token is released.

Any precondition failing fails the resolve with `conflict` or
`precondition_failed` on the surface that attempted it — the phone says "couldn't
answer: someone is typing at the terminal", which is true, actionable, and much
better than a silent wrong selection.

**Delivery is confirmed by observation, never assumed.** Writing the bytes moves
the interaction to `Submitted { by, response, at }`, not to `Resolved`.
`Resolved` requires omt to *observe the agent record the answer* — a
`PostToolUse` hook, a transcript entry, a tool result — inside a bounded window;
otherwise it becomes `Undelivered { reason, response }`, the response text is
preserved, and every surface says *"your answer may not have reached the agent —
check the terminal"* (D15 consequence 1). And an injection is **never retried**:
not on reconnect, not on daemon restart, not by any actor except a human who can
see the screen. A crash between the CAS and the injection goes to `Undelivered`,
which is why `Resolving` must carry the response (D15 consequence 2).

### 4.6 Preconditions on a synthetic delivery

D13's step 3 says "re-verify against the freshest source". This section says
what that means concretely. Every check below was established **VERIFIED-LIVE**
against Claude Code v2.1.220 in
[`spike-card-answering.md`](../research/spike-card-answering.md) §3 and §4, and
each one guards a failure that is **silent** — the remote user sees success and
the local user sees something wrong — which is the class
[D3](decisions.md#d3--synthetic-input-is-bounded-by-state-dependence-not-by-tool-danger)
exists to prevent. All are evaluated against the **freshest rendered screen**,
not the hook payload, inside the transaction while the token is held. Any one
failing fails the resolve with `precondition_failed`, visibly, on the surface
that attempted it; nothing is written.

| # | Precondition | The failure it prevents |
|---|---|---|
| **P1** | The card is present and is the expected one — the question text from the payload matches the live screen. | A late answer landing in whatever the agent is doing now (§C10). |
| **P2** | The intended row is **printed with its number** at the computed index: `\s*(❯\s*)?N\.\s*<label>`. | The single highest-value check. Claude Code's `hideIndexes` flag suppresses the printed number *and* the digit accelerator together, so a visible `N.` is direct evidence the digit will work and a matching label is direct evidence the index is right. No number → refuse. |
| **P3** | The card is **not in text-entry mode** — the focused row is not `type: "input"`, no cursor is parked in an input row, and the hint line does not contain `ctrl+g to edit in`. | **The free-text focus hazard.** If the focused row is an input — which happens when the local user arrows onto "Other", or presses `Tab` on a permission card per its own `Tab to amend` hint — the digit is typed as **literal text into the box, silently**. The remote user sees no error and the local user sees a stray digit. Position-independence is a property of the *(card, state)* pair, not of the card type, and omt cannot learn the state from the hook payload. |
| **P4** | The option list is **≤ 9 entries**. | Digits are `1`–`9` and `0` is a no-op (`parseInt("0")-1 == -1` fails the bounds test). A tenth option has no accelerator, so omt must refuse remote resolution outright rather than pick a reachable neighbour. |
| **P5** | The pane is **not on the alternate screen**. | `tui: "fullscreen"` (or `CLAUDE_CODE_NO_FLICKER=1`) is a real, user-selectable, actively-upsold Claude Code renderer using the alt screen with virtualised scrollback. P1–P3 are screen-derived and cannot be trusted against a scrollback omt does not model. Detected at runtime from `ESC[?1049h` — see [04 §6.4](04-terminal-core.md#64-the-fallback-heuristic--no-shell-integration). |
| **P6** | If an AFK timeout is configured, the card is not within `afk_margin` (3 s) of it. | The agent's own auto-advance would race the write (§4.3). |

Two **transport** rules apply to the write itself, and are not preconditions but
properties of how the bytes are emitted:

- **Never bracket the write.** Claude Code enables `ESC[?2004h`; a digit wrapped
  in `ESC[200~ … ESC[201~` does **nothing**, while a bare digit resolves. omt's
  remote-input path bracket-wraps client text — correct for pasted prose — so
  synthetic answers must bypass that path entirely. Silent failure, not loud.
- **One key per write.** Coalesced bytes arriving in a single read are not
  decoded as separate key events (`b"13"` toggled nothing; `b"1"` then `b"3"`
  toggled both). Multi-byte answers are separate ordered `write(2)` calls, still
  inside one token-held transaction, so "a partial write is never permitted"
  still holds.

Per-card answerability is
[D16](decisions.md#d16--remote-answering-is-per-card-type-and-the-preconditions-are-empirical)'s
table and is a separate, earlier gate: these preconditions are checked only for
cards D16 already declares remotely answerable.

---

## 5. Ordering and causality

### 5.1 Guarantees

| # | Guarantee | Mechanism |
|---|---|---|
| G1 | Events for one session are **totally ordered** by `seq`, and every subscriber observes that order. | single sequence allocator per session, assigned inside the state mutation |
| G2 | A capability call that mutates state returns the `seq` of the resulting event. | `CallResult.output` carries `seq` by convention for all commands |
| G3 | A client that has seen `seq = N` has seen every effect with `seq ≤ N` for that session. | replay window + resync ([07 §5](07-remote-protocol.md#5-resume-and-reliability)) |
| G4 | Terminal bytes are ordered *with* events for the same session. | shared sequence space (07 §5.1) |
| G5 | Input from one actor to one session is delivered to the PTY in send order. | per-(actor, session) FIFO in the input path |
| G6 | Two different sessions have **no** ordering relationship. | by construction; do not add one |

G6 is a deliberate non-guarantee. A global total order would make the sequence
allocator a contention point across every PTY on the machine for a property no
UI needs.

### 5.2 What is not guaranteed

- **No causal ordering across sessions.** If a capability call on session A
  causes an event in session B (a plugin might), the client may see B's event
  before A's result. Where this matters, the events carry an explicit
  `caused_by: RequestId`.
- **No global wall-clock ordering.** `ts` is for humans and logs. Never sort by
  it; sort by `seq`.
- **No cross-instance ordering.** Federated views are merged client-side by
  arrival, and the client must not present a merged timeline as causal.

### 5.3 Request ids and causation

Every event carries `caused_by: Option<RequestId>`. This is what lets a client
match its optimistic update to the authoritative event (§6) without guessing,
and what lets the audit log tie an event back to a call.

`RequestId` must be **stable across reconnects** — `(DeviceId, monotonic u64)`
persisted client-side, with a bounded recent-results cache in dispatch that
replays the stored result on a repeat
([D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)
consequence 5, owned by [07](07-remote-protocol.md)). A `RequestId` unique only
per *connection* leaves a client whose socket died mid-call permanently unable to
learn whether the call applied, which is the same defect §4.1's old idempotency
key had, in the transport rather than in the ledger.

---

## 6. Conflict cases and their resolutions

The complete enumeration. Every one has a defined resolution and a defined
user-visible consequence.

### C1 — Two clients type into the same session at once

**Cannot happen.** Only the writer-token holder may write (§3). The second
client's input path is disabled before they type, and if a race slips through
(input in flight when the token changed hands), the epoch check rejects it with
`precondition_failed { expected_epoch, actual_epoch }` and the client shows
"input not sent — laptop took over".

### C2 — A phone answers a question card while the TUI user is arrow-keying it

**The phone wins if it resolves first**, and vice versa; §4.2 is the full walk.
The important sub-case: the TUI user is *mid-navigation* (has moved the
selection but not pressed Enter) when the phone's resolution arrives. The TUI
must:

1. immediately replace the interactive card with the resolved card,
2. **swallow the next Enter** for 500 ms, so a keypress already in flight from
   the user's fingers does not land on whatever UI is now underneath.

That second rule is unglamorous and it is the difference between "collaboration
works" and "my phone answered a question and then my terminal ran something".

**Scope: this is the omt-drawn-card case only** — a `native` session, or a
mirrored card the phone answers through a `Native` channel while the TUI shows
omt's own rendering. When the card in the pane is the *agent's own*, omt cannot
replace it and cannot swallow the keypress; that is §4.5, and it needs §3.1's
gated transaction rather than a swallow window.

### C3 — A client resizes while another is attached

Governed by [07 §4.3](07-remote-protocol.md#43-the-resize-problem). Only the
writer's viewport (or a pin) is authoritative; other clients letterbox. A
non-writer's `term_resize` updates presence and nothing else. If the size policy
is `Smallest`, any client's viewport change may resize the PTY, and every
attached client is told why: `terminal.resized { cols, rows, cause:
"smallest_viewport", actor }`.

### C4 — Two clients run the same capability simultaneously

Split by capability kind:

- **Idempotent commands** (`session.attach`, `writer.release`, `pane.focus`):
  both succeed; the second is a no-op that still returns `ok` with the current
  `seq`.
- **Exactly-once commands** (`interaction.resolve`, `blocks.rerun`,
  `session.close`): serialized by the owning state mutex; the loser gets
  `conflict` with detail naming the winner.
- **Append-with-dedup-key commands** (`agent.queue.enqueue`): two *distinct*
  intents both apply, in mutex admission order, and both actors see both entries
  with their authors. But a **repeat of the same intent is a no-op returning the
  original entry**, not a second one. This corrects an earlier "Additive — both
  apply" classification, which contradicted
  [08 §8.5](08-web-client.md)'s idempotent-by-id treatment;
  [D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)
  resolves it in favour of **idempotent**, because "both apply" makes every
  reconnect retry double-enqueue the user's text, and the user cannot tell their
  own retry from a genuine second thought. The input therefore carries a
  client-minted `intent_id`, deduped against a bounded server-side cache, and a
  `BindingId` naming the agent the text is *for* — without it a replayed enqueue
  against a session whose agent has since exited lands in the shell prompt and is
  executed as a command. It additionally requires `AgentState::Working` and
  carries a `valid_until`, after which it needs re-confirmation rather than
  silent replay (D15 consequence 3).
- **Last-write-wins commands** (`config.set`, `session.rename`): a
  compare-and-set on a version field. Clients send the version they read; a
  mismatch returns `precondition_failed` with the current value, and the web
  client shows a merge prompt rather than clobbering. Blind overwrite requires
  an explicit `force: true`.

### C5 — Two clients acquire the writer token simultaneously

Mutex; loser gets `conflict`. If both used `force: true`, the second request
*replaces* the first's `PendingTakeover` (the grace deadline is not extended) —
so a takeover storm resolves to the most recent requester, not to whoever
happened to ask first, which matches user intent.

### C6 — The writer disconnects mid-command

The token releases immediately (§3.3), but the PTY is mid-line. omt does **not**
inject a `Ctrl-C` or a newline — that would be omt typing on the user's behalf,
which P4 forbids for anything consequential. The next writer sees the partial
line as it is, and the UI notes "previous writer disconnected with unsent
input".

### C7 — An agent emits an interaction while nobody is attached

Normal. The interaction sits `Open` in the ledger and the first client to attach
receives it in its `since_seq` replay or its snapshot. **Nobody is notified while
disconnected** — [D12](decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead)
ships no push backend in v1; open-and-replay is the path. This is the whole point
of the product, and it is why the durable **attention log** matters: an
interaction that opened *and went terminal* entirely inside an offline gap is
live in no snapshot and may fall outside the replay window, so without it the
user opens the app to an idle session and never learns their agent asked and gave
up. It is queried as `interaction.list { since_read_mark, include_terminal }` and
rendered first on reconnect
([D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)
consequence 9).

### C8 — Two agents in two sessions want the same worktree

Out of scope for the runtime: omt does not lock the filesystem and will not
pretend to. It *does* surface it — `workspace.vcs.summary` marks a worktree with
more than one active agent session, and the dashboard shows a warning badge.
Detection, not prevention.

### C9 — A plugin and a human act on the same object

Plugins are actors with roles ([11 — Plugin system](11-plugins.md)). They obey
every rule in this document, including the writer token. A plugin that wants PTY
input must acquire the token and will lose to a human's `force` takeover.

### C10 — Resolution arrives after the agent already moved on

The agent timed out and proceeded, or exited. The ledger returns `Abandoned`;
the client shows "too late — the agent continued without an answer", carried as
`conflict` with `detail.state = "abandoned"` so it is distinguishable from
"someone else answered" (§4.1). The audit log records the attempted response.
This is *not* silently swallowed: the user must know their tap did nothing.

Note the ordering requirement this implies: for a `Synthetic` delivery the
staleness check must happen **before** any bytes are written, because after the
agent has moved on a late injection lands in whatever it is doing *now*. That is
§3.1's re-verification step, and where an agent offers no observable signal that
a card was resolved, the card must **expire** rather than linger
([D13](decisions.md#d13--synthetic-delivery-is-a-gated-transaction-never-a-bare-write)).

### C11 — The local human's keyboard races an injected answer

The most dangerous case in the document, and the one D11 created. Fully worked in
§4.5: the resolution is §3.1's gated transaction — token, quiescence,
re-verification, atomic write — and the failure mode it prevents is a silent
wrong selection with a false audit record, not a visible conflict.

---

## 7. Optimistic UI

The web client on a phone over a relayed tailnet has a 100–250 ms round trip.
Applying every change only after confirmation makes the UI feel broken. Applying
changes optimistically and getting them wrong makes it *be* broken. The rule set
below is what separates the two.

### 7.1 What may be optimistic

| Action | Optimistic? | Why |
|---|---|---|
| terminal keystroke echo | **yes**, bounded | see [07 §7](07-remote-protocol.md#7-latency-budget); only when not in alt-screen and bracketed-paste is off |
| pane focus, tab switch, scroll | **yes** | purely local view state |
| collapsing/expanding a block | **yes** | local view state |
| enqueueing a prompt | **yes**, shown greyed with a spinner | append-with-dedup-key: the `intent_id` makes the retry safe, so the optimistic entry is either confirmed or replaced by the server's copy, never duplicated (C4) |
| writer acquire on a `Free` token | **yes**, input bar enables immediately | high hit rate; failure is recoverable and visible |
| **`interaction.resolve`** | **NO** | exactly-once, consequential, and the failure mode is "you think you denied something you approved". Doubly so on the `Synthetic` path, where the UI must not show `Resolved` before omt has *observed* the agent record the answer — the honest intermediate rendering is `Submitted` ("sent — waiting for the agent to confirm"), which is a real state, not an optimistic one (§4.5) |
| **anything with `Effects::DESTRUCTIVE`** | **NO** | catalog-driven, mechanical; note this is about omt's *own* capabilities, never a judgement about an agent's tools ([D1](decisions.md#d1--omt-adds-no-policy-layer-over-an-agents-permission-semantics)) |
| `config.set` | **NO** | CAS semantics; showing the new value before it lands invites lost updates |
| writer takeover (`force`) | **NO** | it has a 5 s grace window; there is nothing to be optimistic about |

The rule is derived from the catalog rather than hand-maintained: the generated
TS client marks a capability `optimistic: false` when it declares
`DESTRUCTIVE`, or when it is in the explicit exactly-once list. A new
destructive capability is non-optimistic by default, which is the correct
failure direction.

### 7.2 Applying corrections

Optimistic state is held in a **pending overlay** keyed by `RequestId`, never
merged into the authoritative store:

```ts
type Pending<T> = { requestId: string; applied: T; at: number };

// Render = authoritative state with pending overlay applied on top.
// Reconcile on either:
//   - CallResult(ok)   → drop the overlay; the authoritative event (matched by
//                        caused_by === requestId) has already landed or will.
//   - CallResult(err)  → drop the overlay, restore, and surface the error.
//   - 2 s timeout      → drop the overlay, mark the surface "unconfirmed".
```

Because every event carries `caused_by` (§5.3), reconciliation never guesses.
And because a `Resync` ([07 §5.2](07-remote-protocol.md#52-replay-window))
discards session state, it also discards that session's overlays — a resynced
view is authoritative by definition.

### 7.3 Correction is always visible

Reverting an optimistic update without telling the user is the worst outcome in
this design: it produces a UI that lies briefly and then quietly changes its
mind. Every reverted optimistic action produces a toast naming the action and
the reason, and every action that could not be confirmed within 2 s is rendered
in an explicit "unconfirmed" style until it resolves.

---

## 8. Audit log

Every consequential action is recorded, locally, append-only, on the instance
that performed it.

```rust
pub struct AuditEntry {
    pub seq: u64,                       // instance-wide, monotonic
    pub ts: OffsetDateTime,
    pub actor: Actor,                   // with credential id and device
    pub peer: Option<PeerInfo>,         // source address / uid / tailnet identity
    pub action: AuditAction,
    pub target: AuditTarget,            // session, interaction, config key, plugin…
    pub effects: EffectBits,            // copied from the capability declaration
    pub outcome: AuditOutcome,          // Ok | Denied { reason } | Error { code }
    pub request_id: Option<RequestId>,
    /// Redacted per 13 §8. Never contains PTY bytes or secret-shaped strings.
    pub detail: serde_json::Value,
}
```

What is recorded:

- every capability call whose declaration carries any `effects` bit,
- every auth event (success, failure, credential issued, revoked, invite used),
- every writer acquire / takeover / forced takeover / release, with the epoch,
- every interaction open, submission and terminal state, with the actor and
  whether it was human or timeout — **and the response, recorded carefully**:
  - the **selection** (which option, approve/deny) verbatim: it is small, it is
    bounded, and it is the decision a "who approved that?" investigation is
    looking for;
  - any **edited tool input** ([D9](decisions.md#d9--positioning-what-omt-may-and-may-not-claim)
    consequence 3 added argument editing before approval) as a **`blake3` hash
    plus a redacted diff against the agent's original input** — never verbatim.
    An `updated_input` is unbounded (a whole file body, a patch) and is exactly
    the field a user pastes a token or a connection string into. The hash proves
    *which* input was approved, the diff shows *what the human changed*, and
    together they answer the forensic question without turning the audit log into
    a secret store. Redaction is [13 §8](13-security.md)'s, the same rules as
    `detail`;
  - a `Submitted` entry and its later `Resolved` / `Undelivered` outcome are
    **separate records**. The audit log must never assert delivery omt only
    attempted ([D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)
    consequence 1) — an `Undelivered` interaction whose log said "resolved" is
    the false record D13 exists to prevent,
- every configuration change (key, old value hash, new value hash),
- every plugin install/enable/call.

What is **not** recorded: PTY input bytes and PTY output. Keystroke logging
every session would make the audit log the most dangerous file on the machine.
The audit records *that* actor X wrote N bytes to session S at time T, not what
those bytes were. Interaction *selections* are recorded because they are
decisions, they are small, and they are exactly what a "who approved that?"
investigation needs; edited tool arguments are recorded as a hash and a redacted
diff for the reason given above — the same reasoning applied to a field that is
neither small nor bounded.

Storage: `omt-store`, its own append-only log, rotated by size, retained 90 days
by default, with `0600` permissions. Exposed as `audit.query` (Admin role only)
and rendered in the web client as a per-session activity timeline — because the
audit log is not only a forensic tool, it is the answer to "what happened while
I was away?", which is a daily question for this product.

---

## 9. OPEN QUESTIONS

1. **Idle writer-release at 90 s** may be too aggressive for a user reading
   output while composing a thought, and too slow for handing off. Should it be
   per-session and remembered? Needs use.
2. **Takeover grace of 5 s**: interacts badly with a holder whose client is
   `Background` (a phone in a pocket can neither see the countdown nor cancel
   it). Proposal: skip the grace entirely when the holder's liveness is
   `Background` or `Stale`. Not yet decided.
3. **Should `Viewer` presence be hideable?** Argued no above, but a shared
   demo/streaming case may want a "hidden observer" mode. If added, it must be
   an instance-level admin setting that is itself visible, never a per-client
   flag.
4. **Interaction viewers as a soft claim** was rejected (§4.4). Revisit if real
   multi-human use shows double-answering is common rather than theoretical.
5. **Multi-agent collaboration** (two agents in one session, or an agent acting
   as `Actor::Agent` on another session) is modelled but unexercised. The writer
   token semantics for `ActorKind::Agent` in particular need a real use case
   before they are fixed. Coordinate with [06 — Agent layer](06-agent-layer.md).
6. **`assume_idle` — same-identity silent handoff.**
   [docs/design/remote-continuity.md §4](../design/remote-continuity.md) proposes
   `session.writer.acquire { assume_idle: bool }`: when the requester and the
   current holder are the *same human identity* on a different device (laptop in
   a bag, phone in hand), skip the 5 s grace and hand the token over silently,
   since there is nobody to be interrupted. It is attractive and it is **not
   adopted here.** Two things must be settled first: omt has no federated
   identity notion today (open question 7 below), so "same human" is currently
   only expressible as "same credential"; and a silent handoff removes the one
   affordance — the countdown — that tells a user their input is about to stop
   working, which §3.4's rule exists to prevent. Decide it together with open
   question 2 (`Background`/`Stale` holders), which is the same problem
   approached from liveness rather than identity.
7. **Cross-instance presence.** A user attached to four instances appears as
   four unrelated actors. Should the client assert a federated identity so the
   TUI can say "Vincent is on your machine and two others"? That requires a
   shared identity notion omt currently does not have, and may not want.
8. **Audit log for terminal writes**: currently byte counts only. Some users
   (shared/team boxes) will want full input capture. If offered it must be
   opt-in, per-session, loudly indicated in presence on every surface, and
   probably encrypted at rest. Coordinate with [13](13-security.md).
9. **C4's last-write-wins version field** is not yet specified for the session
   tree as a whole (only per-object). Concurrent layout edits from two clients
   may need a coarser lock. Coordinate with
   [05 — Session model](05-session-model.md).
