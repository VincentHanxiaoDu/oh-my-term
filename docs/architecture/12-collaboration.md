# Concurrency and Collaboration

Multiple humans, on multiple devices, plus multiple agents, act on one instance
at the same time. This document is the authoritative specification of what
happens when they collide.

It is the runtime half of principle
[P6](01-principles.md#p6--collaboration-is-a-runtime-feature-not-just-a-workflow).

Related: [03 — Capability catalog](03-capability-catalog.md) ·
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
[03 §3](03-capability-catalog.md#3-dispatch)), stamped onto every event, and
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
    pub session: SessionId,
    pub pane: Option<PaneId>,
    /// The surface being used, because it changes what the actor can see.
    pub surface: Surface,            // Terminal | Blocks | InteractionCard | Dashboard
    /// Reported viewport; drives the size policy in 07 §4.3.
    pub viewport: Option<TermSize>,
}
```

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
and clients repopulate it by reconnecting.

---

## 3. The writer token

### 3.1 What it governs

The writer token gates exactly one thing: **byte-level input to a session's
PTY**. Concretely, the capabilities carrying `Effects::WRITES_PTY` —
`session.send_text`, `session.send_keys`, `session.write_bytes`, and
`session.resize` when it requests authoritative sizing.

It deliberately does **not** gate:

- `interaction.resolve` — answering a question card is not PTY input; it goes
  back through the agent's own channel (P4), and gating it behind a token would
  break the flagship "answer from your phone while someone else is at the
  keyboard" case. Its own single-resolution rule is §4.
- `agent.prompt` where the adapter has a structured submit path (ACP, stream-json
  stdin). Prompts are queued and serialized by the agent layer, not by the token.
  Where the *only* available path is synthesized keystrokes, the token **is**
  required, because that path is PTY input wearing a hat.
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
| **Acquire** | `session.writer.acquire` | Succeeds immediately if `Free`. Fails `conflict` if held, with the holder in `detail`. Requires `Operator`. |
| **Takeover** | `session.writer.acquire { force: true }` | Legal only for `Operator`+. Opens a `PendingTakeover` with a **grace window of 5 s**. Every surface shows a countdown. The holder may `session.writer.keep` to cancel it once per takeover; a second takeover request within 60 s cannot be cancelled. |
| **Release** | `session.writer.release` | Immediate; token becomes `Free`. |
| **Idle release** | — | After **90 s** with no input, the token auto-releases (`Actor::System`). Prevents a closed laptop from holding a session hostage. |
| **Disconnect** | — | Token released immediately on transport close; no grace, because the holder is provably gone. |
| **Daemon restart** | — | All tokens released. |

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

---

## 4. Interaction ownership

An `Interaction` (a question card, a permission request, a plan review) is the
highest-stakes concurrent object in the system: it maps onto a real decision
that a real agent is blocked on, and resolving it twice is not merely
inconsistent — it can approve something the user meant to deny.

### 4.1 The invariant

**An `Interaction` transitions from `Open` to a terminal state exactly once, by
exactly one actor, and the resolution is broadcast to every subscriber.**

```rust
pub struct Interaction {
    pub id: InteractionId,
    pub session: SessionId,
    pub kind: InteractionKind,       // Choice | Permission | Text | PlanReview
    pub opened_at: OffsetDateTime,
    pub timeout_at: Option<OffsetDateTime>,
    pub state: InteractionState,
    /// Advisory: who is currently looking at this card, for the UI.
    pub viewers: Vec<ActorId>,
}

pub enum InteractionState {
    Open,
    Resolved { by: Actor, response: InteractionResponse, at: OffsetDateTime },
    Cancelled { by: Actor, reason: CancelReason, at: OffsetDateTime },
    /// The agent went away (crashed, or the user hit Esc in the TUI) before
    /// anyone answered. Terminal, and distinct from Cancelled.
    Abandoned { at: OffsetDateTime, detail: String },
}
```

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
            InteractionState::Open => { /* CAS: set Resolved, emit event, return Ok */ }
            InteractionState::Resolved { by, at, .. } =>
                Err(LedgerError::AlreadyResolved { by: by.clone(), at: *at }),
            InteractionState::Cancelled { .. } => Err(LedgerError::Cancelled),
            InteractionState::Abandoned { .. } => Err(LedgerError::Abandoned),
        }
    }
}
```

`interaction.resolve` is **idempotent by `(interaction_id, actor, response)`**:
if the same actor resolves the same interaction with the same response twice
(a retry after a flaky network), the second call returns the original
`Resolution` with `ok: true`. A *different* response, or a different actor, gets
`conflict`. This distinction matters because retry-on-timeout is normal on
mobile and must not produce a spurious conflict banner.

### 4.2 The race, concretely

The dangerous version is not two phones — it is a phone and the TUI, because
they resolve through *different mechanisms*:

```
t=0    Claude Code PreToolUse{AskUserQuestion} → hook → defer → Interaction int_88 Open
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
5. Only the winning response reaches the agent, via the single hook response
   that `omt-hook` is blocking on.

The one thing that makes this safe is that `omt-hook` holds exactly one
in-flight response slot per interaction and the daemon writes to it once. Even
a bug in the ledger cannot produce two hook decisions.

### 4.3 Timeouts and policy resolution

`Interaction::timeout_at` comes from the agent (Claude Code's defer window, ACP
timeouts). On expiry, `Actor::System` resolves it as `Cancelled { reason:
Timeout }`, which is what the agent's own default behaviour would be. Policy
auto-resolution (a configured "always allow reads in this workspace" rule)
resolves as `Actor::System` with `reason: Policy { rule_id }` and is audited
identically to a human decision — see [13 §7](13-security.md), which also
specifies which interaction kinds a remote credential is allowed to resolve at
all.

### 4.4 Advisory viewer presence on a card

`Interaction.viewers` exists so that two humans do not both answer. It is
advisory — it does not lock anything — and it drives a small "laptop TUI is
looking at this" hint. Optimizing further (a soft lock, a claim) was rejected:
the interaction is *already* exactly-once, and a claim mechanism would add a
failure mode (a claimed-and-then-disconnected card) to solve a cosmetic problem.

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
- **Additive commands** (`agent.queue.enqueue`): both apply; ordering is the
  mutex admission order and both actors see both entries with their authors.
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

Normal. The interaction sits `Open` in the ledger, the daemon fires a push
notification ([07 §8](07-remote-protocol.md#8-notifications-to-a-closed-tab)),
and the first client to attach receives it in its `since_seq` replay or its
snapshot. This is the whole point of the product.

### C8 — Two agents in two sessions want the same worktree

Out of scope for the runtime: omt does not lock the filesystem and will not
pretend to. It *does* surface it — `workspace.git.status` marks a worktree with
more than one active agent session, and the dashboard shows a warning badge.
Detection, not prevention.

### C9 — A plugin and a human act on the same object

Plugins are actors with roles ([11 — Plugin system](11-plugins.md)). They obey
every rule in this document, including the writer token. A plugin that wants PTY
input must acquire the token and will lose to a human's `force` takeover.

### C10 — Resolution arrives after the agent already moved on

The hook response slot is gone (the agent timed out and proceeded). The ledger
returns `Abandoned`; the client shows "too late — the agent continued without an
answer", and the audit log records the attempted response. This is *not*
silently swallowed: the user must know their tap did nothing.

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
| enqueueing a prompt | **yes**, shown greyed with a spinner | additive, conflict-free (C4) |
| writer acquire on a `Free` token | **yes**, input bar enables immediately | high hit rate; failure is recoverable and visible |
| **`interaction.resolve`** | **NO** | exactly-once, consequential, and the failure mode is "you think you denied something you approved" |
| **anything with `Effects::DESTRUCTIVE`** | **NO** | catalog-driven, mechanical |
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
- every interaction open and resolution, **including the response**, the actor,
  and whether it was human, policy, or timeout,
- every configuration change (key, old value hash, new value hash),
- every plugin install/enable/call.

What is **not** recorded: PTY input bytes and PTY output. Keystroke logging
every session would make the audit log the most dangerous file on the machine.
The audit records *that* actor X wrote N bytes to session S at time T, not what
those bytes were. Interaction responses are recorded because they are decisions,
they are small, and they are exactly what a "who approved that?" investigation
needs.

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
6. **Cross-instance presence.** A user attached to four instances appears as
   four unrelated actors. Should the client assert a federated identity so the
   TUI can say "Vincent is on your machine and two others"? That requires a
   shared identity notion omt currently does not have, and may not want.
7. **Audit log for terminal writes**: currently byte counts only. Some users
   (shared/team boxes) will want full input capture. If offered it must be
   opt-in, per-session, loudly indicated in presence on every surface, and
   probably encrypted at rest. Coordinate with [13](13-security.md).
8. **C4's last-write-wins version field** is not yet specified for the session
   tree as a whole (only per-object). Concurrent layout edits from two clients
   may need a coarser lock. Coordinate with
   [05 — Session model](05-session-model.md).
