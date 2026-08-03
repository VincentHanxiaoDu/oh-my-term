# Remote Continuity — Working Together Across Devices, Not a Second Screen

The architecture already has every primitive this document needs: presence
([12 §2](../architecture/12-collaboration.md#2-presence-is-first-class-state)),
the writer token
([12 §3](../architecture/12-collaboration.md#3-the-writer-token)), the
interaction ledger
([06 §5](../architecture/06-agent-layer.md#5-interactions--the-flagship-path)),
event resume
([07 §5](../architecture/07-remote-protocol.md#5-resume-and-reliability)),
per-client layout views
([17 §3.3](../architecture/17-panes-and-layout.md#33-decision-per-client-layout-views-one-shared-default)),
and the reconnect/replay path
([07 §5.2](../architecture/07-remote-protocol.md#52-replay-window)).

What does not yet exist is the design that makes them **add up**. A user who
picks up their phone should not experience "a remote view of my laptop". They
should experience *the same work, continued*. That is the requirement this
document specifies, and it is a product requirement, not a transport one: every
mechanism below could be implemented correctly and still produce a second-class
experience if the state taxonomy, the ranking, and the defaults are wrong.

Related:
[00 — Overview](../architecture/00-overview.md) ·
[03 — Capability catalog](../architecture/03-capability-catalog.md) ·
[05 — Session model](../architecture/05-session-model.md) ·
[06 — Agent layer](../architecture/06-agent-layer.md) ·
[07 — Remote protocol](../architecture/07-remote-protocol.md) ·
[08 — Web client](../architecture/08-web-client.md) ·
[12 — Collaboration](../architecture/12-collaboration.md) ·
[17 — Panes and layout](../architecture/17-panes-and-layout.md) ·
[Decision log](../architecture/decisions.md) ·
Research: [connectivity](../research/connectivity.md) ·
[vscode-remote](../research/vscode-remote.md)

> **Authority note.** This is a *design* document. Where it appears to contradict
> an architecture document, the architecture document wins and this one is the
> bug — except for the state taxonomy in §1, which is new and which the
> architecture docs should be updated to reference. New capabilities and events
> this design requires are collected in §10 and are flagged as **ADDITIONS**;
> none of them are assumed to exist yet.

---

## 1. The continuity model

### 1.1 What "the same work" means, concretely

The user's mental object is not a session and not a device. It is **a piece of
work in progress**: "the migration on the workstation", "the flaky test on the
laptop". A device is a window onto it, and windows should not carry state that
belongs to the work.

Continuity therefore has exactly one rule, from which everything else is
derived:

> **State follows the work if losing it would make the user repeat a decision or
> retype something. State stays on the device if moving it would make the device
> render badly.**

Everything that is neither — recents, notification preferences, the last thing
you were looking at — belongs to the *person*, which is a third scope the
architecture does not yet have and which §1.3 introduces.

### 1.2 The taxonomy

Three scopes. Every piece of user-visible state belongs to exactly one, and the
placement is a design decision with a stated cost.

| Scope | Lives in | Survives | Key |
|---|---|---|---|
| **Global** (per instance) | `omt-store` on the instance | daemon restart | `(InstanceId, SessionId)` or `(InstanceId, WorkspaceId)` |
| **Per-actor** (per person) | `omt-store` on the instance, keyed by a stable *identity*, not a connection | daemon restart, device loss | `(InstanceId, IdentityId)` |
| **Per-device** | the client (IndexedDB), never the server | app reinstall: no | `DeviceId` |

#### Global — the work itself

| State | Why global |
|---|---|
| Session existence, argv, cwd, env, workspace binding | It *is* the work. [05 §1](../architecture/05-session-model.md#1-the-object-model). |
| Scrollback, blocks, command history | Produced by the work, not by a viewer. A phone must see the same bytes the laptop saw. |
| Agent state, tool calls, usage, queue | The agent is one process; there is one truth about what it is doing. |
| **Interaction ledger** | Exactly-once resolution is meaningless if it is per-device. [12 §4.1](../architecture/12-collaboration.md#41-the-invariant). |
| Writer token, epoch, authoritative PTY size | One PTY, one size, one writer. [12 §3](../architecture/12-collaboration.md#3-the-writer-token). |
| The `Primary` layout view | The workspace's canonical arrangement; shared by definition. [17 §3.3](../architecture/17-panes-and-layout.md#33-decision-per-client-layout-views-one-shared-default). |
| Instance config, themes, keymaps as *configuration* | [10 — Configuration](../architecture/10-configuration.md) owns this; a theme is instance state so the TUI and the phone look like one product ([08 §9.2](../architecture/08-web-client.md#92-theming)). |

#### Per-actor — the person's place in the work

This is the scope that makes handoff possible, and it is the one the
architecture is missing. It is **not** per-connection: an `ActorId` is stable
only for the lifetime of a connection
([12 §1](../architecture/12-collaboration.md#1-actors)), which is exactly the
wrong lifetime. It must be keyed by an identity that outlives both the
connection and the device.

`IdentityId` is **not** an addition of this document. It is defined and owned by
[23 §1.1](../architecture/23-identity-and-devices.md#11-four-types): the
blake3-256 of the identity root public key, rendered
`idn_<crockford-base32>`, self-certifying and mintable with no issuer. This
document only *uses* it as the per-actor key. What is new here is `ActorContinuity`
and the state hanging off it.

Note also where the two documents divide: [23] owns the **registry** — an
identity's devices, its instance list, its revocations — while `ActorContinuity`
below is **per instance and is not replicated by the registry**. The registry
tells you which devices belong to a person; it never carries their recents,
drafts or mutes, and promoting or demoting a home instance moves none of it
([23 §3.1](../architecture/23-identity-and-devices.md#31-what-it-is)).

```rust
/// ADDITION. Per-instance, per-identity continuity state. Persisted.
pub struct ActorContinuity {
    pub identity: IdentityId,
    /// Where this person was last working, most recent first, capped at 32.
    pub recents: Vec<Recent>,
    /// Unsent composer text, per target. §2.4.
    pub drafts: HashMap<DraftKey, Draft>,
    /// Which sessions this person has explicitly muted, and until when.
    pub mutes: HashMap<SessionId, MuteUntil>,
    // `notify: NotifyPrefs` removed by D12 — no notification backend ships,
    // so there is nothing to route. Reserved in outline at §5.6.
    /// Per-session read position. Under D12 this is load-bearing: it defines
    /// the window of the attention log (§5.2) and clearing it is how the
    /// "while you were away" band empties.
    pub read_marks: HashMap<SessionId, Seq>,
    /// Per-session view-mode preference *intent* — see §1.4 for why this is
    /// a hint and not a device setting.
    pub preferred_surface: HashMap<SessionId, SurfaceIntent>,
}

pub struct Recent {
    pub workspace: WorkspaceId,
    pub session: Option<SessionId>,
    pub last_active: OffsetDateTime,
    /// Which device produced this entry — used for the "back to the desk" hint
    /// in §2.5, never for filtering.
    pub via: DeviceId,
}
```

Justification, item by item:

- **Recents / last active workspace.** The single most valuable thing to carry.
  Without it, opening the phone means choosing from a list; with it, the phone
  opens on the thing you were doing 40 seconds ago. It cannot be per-device
  (that is the whole point) and it cannot be global (it is *your* place, and a
  second person's place is different).
- **Drafts.** Half a prompt typed at the laptop is a *decision partially
  expressed*. Losing it makes the user retype — the exact failure the rule in
  §1.1 names. It is per-actor and not global because two people composing into
  one session must not overwrite each other.
- **Mutes, notification prefs, read marks.** Notifications are delivered to a
  person across their devices; a mute that applies to the phone but not the
  watch is a bug, and a mute stored on the phone is lost when the phone is
  replaced.
- **`preferred_surface`.** "I always want block view for this session" is an
  intent about the *session*, not the screen. §1.4 covers how it interacts with
  the per-device default.

#### Per-device — how this screen renders

| State | Why per-device |
|---|---|
| Viewport (cols/rows/dpr), font size, pinch zoom | Physical property of the screen. Syncing it is actively harmful ([17 §3.3](../architecture/17-panes-and-layout.md#33-decision-per-client-layout-views-one-shared-default)). |
| The client's `LayoutView` (`Adaptive` on a phone) | A phone cannot render a four-way split; forcing it to is anti-pattern A1 (§9). |
| Scroll offset, selection, search-in-progress | Already per-client in `PaneView` ([05 §4](../architecture/05-session-model.md#4-attachment-detach-and-multi-client-viewing)). Syncing scroll between a laptop and a phone means two people fighting a scrollbar. |
| Which of the three surfaces is the *default* | Derived from viewport width and session kind — block, transcript or terminal ([08 §4](../architecture/08-web-client.md#4-view-modes)); overridden by `preferred_surface` when the user set one explicitly. |
| Keymap overlay, virtual key bar layout state, haptics | Input-hardware properties. |
| Credentials, device keypair | Device-bound by [13 §3](../architecture/13-security.md). Moving them would break revocation-per-device. |
| Predictive-echo engagement state, measured RTT | Per-link. |

### 1.3 Where per-actor state is stored, and why on the instance

Three options were considered.

| Option | Verdict |
|---|---|
| **Client-side only, synced device-to-device** | Rejected. Requires a sync service or peer discovery between a phone and a laptop that may never be online together. omt has no cloud ([00 §8](../architecture/00-overview.md#8-what-omt-is-not)). |
| **On the instance, per identity** | **Chosen.** The instance is already authoritative for the work, already persistent, already reachable by every device that can see the work at all. If the instance is unreachable, its per-actor state is irrelevant because its sessions are too. |
| **A designated "home" instance holding state for all instances** | Rejected. It reintroduces the cluster that [07 §1.1](../architecture/07-remote-protocol.md#11-the-shape) deliberately does not have, and creates a single point of failure for a federation whose whole virtue is not having one. |

Consequence, stated plainly: **recents and drafts are per instance.** The
client's unified recents list is a client-side merge, exactly like the unified
session list. A user with four instances gets four `ActorContinuity` records,
merged by `last_active`. This is the same trade the federation already makes and
it fails in the same benign way — an offline instance contributes nothing.

### 1.4 The one deliberate exception: surface intent

`preferred_surface` is per-actor but is *overridden* per-device whenever the
device cannot honour it:

```rust
pub enum SurfaceIntent { Auto, Blocks, Terminal }

/// Resolution order, evaluated on the client at attach time.
fn resolve_surface(intent: SurfaceIntent, viewport_px: u32) -> Surface {
    match intent {
        SurfaceIntent::Terminal if viewport_px < 600 => Surface::Terminal, // honoured, letterboxed
        SurfaceIntent::Blocks   => Surface::Blocks,
        SurfaceIntent::Terminal => Surface::Terminal,
        SurfaceIntent::Auto     => if viewport_px < 600 { Surface::Blocks } else { Surface::Terminal },
    }
}
```

Note that `Terminal` on a phone is *honoured*, not overridden — per
[D2](../architecture/decisions.md#d2--remote-is-exactly-equivalent-to-local) the
phone is not a reduced client, and
[07 §4.3](../architecture/07-remote-protocol.md#43-the-resize-problem) already
specifies letterboxed rendering that is correct if small. The default differs by
device; the *permission* does not.

---

## 2. Handoff

### 2.1 The journey

```
 laptop, 16:02   working in `api-gateway`, claude running, half a prompt typed
 laptop, 16:04   lid closes.  writer token idle-releases at 16:05:30 (12 §3.3)
 phone,  16:11   unlock, tap omt
                 ─────────────────────────────────────────────────────────────
                 ▸ the app opens on `api-gateway`, block view, scrolled to the
                   latest agent turn, the composer pre-filled with the draft,
                   with a one-line "continued from laptop · 9 min ago" chip
 phone,  16:12   types the rest of the prompt, sends
 desk,   16:40   laptop wakes; the TUI shows the prompt as sent-by-iPhone and
                 the agent's reply already in the scrollback. Nothing to resume.
```

Nothing in that sequence involves the user choosing an instance, choosing a
session, or being told about a connection.

### 2.2 What the phone shows on open

**Never a file tree of nothing, never an empty instance list, never a spinner
over a blank screen.** The open sequence is:

1. **Frame 1 (0 ms, offline-capable).** Paint the last known unified view from
   IndexedDB — the same rows, greyed with a "last seen 16:04" stamp. This is
   [connectivity §3.5 item 2](../research/connectivity.md#35-concrete-recommendation-for-reconnect--resume-cross-ref-07-5)'s
   client-side instant repaint, applied to the *home screen* and not only to the
   terminal. Painting stale-but-labelled beats painting nothing.
2. **Frame 2 (~1 RTT, warm resume).** Reconnect with `session_token` +
   `since_seq` ([07 §3.4](../architecture/07-remote-protocol.md#34-auth)),
   fetch `continuity.get` (§10 **ADDITION**), and re-rank.
3. **Frame 3 (auto-navigate, only if the ranking is confident).** If the top
   ranked candidate scores above the threshold in §2.3, the client navigates
   directly into it. Otherwise it stays on the ranked list.

The auto-navigate decision is the delicate one, so it is explicit: **auto-open
only when the top candidate's score exceeds the runner-up's by 1.5× *and*
exceeds an absolute floor.** A near-tie means the user genuinely has two things
going on, and guessing wrong costs them a back-tap and their trust in the
ranking. A "⌂" control always returns to the list and, once used twice in one
session, the client suppresses auto-navigate for 10 minutes (a cheap, local,
self-correcting heuristic that needs no setting).

### 2.3 The continuity ranking

Given N sessions across M instances, rank the candidates for "what do you want
right now". Computed client-side, over the merged view, because only the client
knows which notification was tapped and which instances are reachable.

```ts
// ADDITION (client-side; no protocol surface).
interface RankInput {
  session: UnifiedSession;              // 08 §3.3
  openInteractionAgeMs: number | null;  // null when none
  agentState: "blocked" | "working" | "idle";
  lastActivityMs: number;               // since now
  lastActorWasMe: boolean;              // from ActorContinuity.recents
  hasDraft: boolean;
  otherDeviceAttached: boolean;         // presence: another of MY devices
  otherDeviceWriting: boolean;          // that device holds the writer token
  muted: boolean;
  reachable: boolean;
}

function score(c: RankInput): number {
  // NOTE: `deepLinkTarget: return Infinity` was removed. It short-circuited the
  // whole ranking on "this open came from a notification tap" — and under
  // D12 nothing taps, so the branch was unreachable. With no notification to
  // defer to, the ranking below is the *only* thing deciding what the user sees
  // first, which makes every weight in it load-bearing. See §5.
  if (!c.reachable) return -1;                    // still listed, never ranked first
  let s = 0;
  if (c.openInteractionAgeMs !== null) {
    s += 1000;                                    // "needs you" dominates everything
    s += Math.min(200, c.openInteractionAgeMs / 3000);   // older = more urgent, capped
  } else if (c.agentState === "blocked") {
    s += 600;                                     // blocked but unrenderable (06 §4)
  } else if (c.agentState === "working") {
    s += 120;
  }
  s += 300 * recency(c.lastActivityMs);           // recency(x): 1 at 0s, 0.5 at 5min, →0 at 6h
  if (c.lastActorWasMe) s += 150;                 // my work outranks a colleague's
  if (c.hasDraft)       s += 250;                 // an unfinished sentence is a strong signal
  if (c.otherDeviceWriting)  s -= 400;            // §3: the laptop is actively driving
  else if (c.otherDeviceAttached) s -= 80;        // merely open: mild deprioritization
  if (c.muted) s -= 500;
  return s;
}
```

Design notes, each of which is a decision:

- **There is no "the user told us where to go" input any more.** Under
  [D12](../architecture/decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead)
  the app is opened from the home screen, not from a tap on a specific question,
  so the ranking below is the sole judgement of what to show first. It used to be
  a tiebreak behind an explicit instruction; it is now the instruction.
- **`openInteraction` outweighs everything else combined.** The product exists
  to shorten the time between "an agent blocked" and "a human answered".
- **Age *increases* urgency, capped.** An interaction open for 4 minutes is more
  likely the reason you picked up the phone than one that opened 3 seconds ago —
  but past ~10 minutes the signal saturates and recency should take over again.
- **`otherDeviceWriting` is a large negative, not a filter.** If your laptop is
  actively being typed into, that session is the *least* likely thing you want on
  the phone — you are looking at it. But it must remain reachable, because
  "check what the laptop is doing from the couch" is real.
- **A draft is worth more than 5 minutes of recency.** Empirically the strongest
  intent signal available: the user began composing and stopped.
- **Unreachable instances score below everything but are never hidden**
  ([08 §3.3](../architecture/08-web-client.md#33-the-unified-session-list):
  disappearing rows on a subway is worse than stale rows).

The score is exposed in a debug panel (`Settings → Diagnostics → Ranking`)
because a ranking nobody can inspect is a ranking nobody can trust or tune.

### 2.4 Drafts

A draft is unsent composer text. It is the highest-value, lowest-cost piece of
continuity in the whole design.

```rust
/// ADDITION.
pub struct Draft {
    pub key: DraftKey,
    pub text: String,
    /// Byte offset of the caret, so the phone restores it mid-sentence.
    pub caret: usize,
    /// Attachments already uploaded to the blob store but not yet sent (09 §4.3).
    pub blobs: Vec<BlobId>,
    pub updated_at: OffsetDateTime,
    pub updated_by: DeviceId,
    /// Monotonic per key. Last-write-wins with a CAS, exactly like config.set
    /// (12 §6 C4) — a stale device cannot clobber a newer draft silently.
    pub version: u64,
}

pub enum DraftKey {
    /// The prompt composer for a session's bound agent.
    AgentPrompt { session: SessionId },
    /// A free-text interaction card's answer field (08 §5.5 already persists
    /// this locally; this promotes it to per-actor).
    Interaction { interaction: InteractionId },
    /// Plan-review "request changes" text.
    PlanReview { interaction: InteractionId },
}
```

Rules:

- **Sync is debounced at 800 ms of typing inactivity, and forced on blur and on
  the tab going to background.** Not per
  keystroke — that would put a capability call on the input path for a feature
  worth one round trip a minute.
- **PTY input is never a draft.** Bytes typed into a terminal have already been
  delivered; there is no unsent state to carry, and reconstructing a half-typed
  shell command on another device would require typing it into the PTY, which is
  a writer-token operation with a consequence. The composer is the boundary.
- **Conflict resolution is last-write-wins with a visible loser.** If the phone
  and the laptop both have text, the phone shows the newer one and offers
  *"laptop also has a draft (2 lines) — view"* rather than merging. Merging free
  text is guaranteed to produce a sentence neither person wrote.
- **Drafts are cleared on send**, and on `interaction.resolve` for the card
  keys — including when someone *else* resolved it, since the text no longer has
  a destination. The clear is broadcast (§10 `continuity.draft_changed`) so the
  laptop's composer empties too instead of holding a ghost.
- **Drafts expire after 7 days** and are counted against nothing — they are text.

### 2.5 The reverse handoff

Walking back to the desk is the case most products forget. The laptop TUI wakes
into a world that moved.

1. **The TUI reconnects and resyncs** (it is a client of its own instance for
   presence purposes but reads state directly). It must not replay 30 minutes of
   agent output as if it were live: everything that happened while the actor was
   away is rendered *below a "while you were away" separator* with a count —
   `── 14 blocks, 1 interaction answered from iPhone, 16:11–16:38 ──`.
2. **Nothing auto-scrolls past that separator.** The user decides when to catch
   up. Auto-jumping to live output is how a user loses the thing they came back
   to look at.
3. **Answers made elsewhere are attributed, not hidden.** A card resolved from
   the phone shows as `Answered: Postgres · iPhone · 16:12`, per
   [12 §4.2 step 3](../architecture/12-collaboration.md#42-the-race-concretely).
4. **Drafts flow back.** Text finished on the phone but not sent appears in the
   TUI composer with the same "from iPhone" chip.
5. **The writer token is not pre-acquired.** The TUI takes it on the user's first
   keystroke (§4), not on attach. Attaching is not an intent to type.

### 2.6 Handoff is not "transfer"

There is deliberately **no** "send this session to my phone" button, and no
pairing gesture between devices (no Handoff-style proximity, no QR-per-session).
Both were considered and rejected: they require the user to *predict* the
handoff, which is exactly what they cannot do — the whole scenario is that they
stood up and walked away. Continuity that requires a pre-commitment is not
continuity.

The one exception that earns its keep is **"open this on…"** in the session
overflow menu. Under [D12](../architecture/decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead)
it cannot push anything, so it degrades honestly: it records a **surface intent**
(§1.4) against the target device, and that device opens to the named session the
next time it connects — plus a copyable link for the user to send themselves by
whatever channel they already use. It is for the deliberate case ("I want to
read this diff on the big screen"), and it is not on the primary path.

---

## 3. Presence made useful

Presence that only draws avatars is decoration and should not be built. Every
presence surface below is specified with **the behaviour it changes**.

### 3.1 The single-user, two-device case is the common one

[D4](../architecture/decisions.md#d4--single-user-many-devices--with-the-interfaces-left-open-for-many-users)
says the primary scenario is one person on several devices. Naive presence UI
handles this atrociously: it tells you that *you* are watching, decorates every
session with your own avatar, and produces notification noise from your own
actions.

**Rule: presence entries whose identity equals the viewer's are rendered
differently, and never as a person.**

```ts
// ADDITION (client-side classification over the presence event stream).
type PresencePeer =
  | { kind: "me"; device: DeviceId; label: string; liveness: Liveness }   // my other device
  | { kind: "other"; actor: ActorId; label: string; liveness: Liveness }; // someone else
```

| Surface | `kind: "me"` | `kind: "other"` |
|---|---|---|
| Session row | a small device glyph (`▣ laptop`), muted colour, no avatar | avatar + name |
| Terminal header | `laptop is driving · tap to take over` | `Ada is driving · request` |
| Interaction card | `you're looking at this on laptop too` (suppressed after 5 s) | `Ada is answering…` (§3.3) |
| Notifications | **suppressed** when another of *my* devices is `Active` and attached to that session (§5.6) | never suppressed |
| Dashboard header | nothing | `2 people connected` |

The notification suppression rule is the one that most changes daily life: if you
are sitting at your laptop with the session open and an agent asks a question,
your phone should not buzz. The laptop is already telling you.

### 3.2 What each surface shows and why

| Signal | Shown as | The behaviour it changes |
|---|---|---|
| Device attached to a session | device glyph in the session row and pane border | You know the laptop is watching before you take the writer token — the takeover is not a surprise. |
| Who holds the writer token | `✎ laptop` badge; input bar state | You do not type into a disabled field. This is [12 §3.4](../architecture/12-collaboration.md#34-visual-indication-is-mandatory-on-every-surface)'s mandate; §4 makes it unobtrusive. |
| Someone is *answering* an interaction right now | the card's action row shows a pulsing `answering…` with the actor label; buttons stay enabled | Stops the double-answer race *before* the CAS rejects it. |
| A device is mid-typing | `laptop is typing…` on the session row, debounced 2 s on, 4 s off | Tells you the session is actively driven by a human, not an agent — which is the difference between "join in" and "wait". |
| Liveness (`Active`/`Idle`/`Background`/`Stale`) | opacity + a tooltip | Drives the writer-token silent handoff (§4.3) and the takeover grace skip. |

### 3.3 "Answering right now" — the one new presence signal

`Interaction.viewers` exists today and is advisory
([12 §4.4](../architecture/12-collaboration.md#44-advisory-viewer-presence-on-a-card)).
Viewing is a weak signal; **composing an answer** is a strong one. This design
adds it, and keeps the explicit rejection of a *claim* intact — it changes no
authority, only what is displayed.

```rust
/// ADDITION. Advisory, ephemeral, never persisted, never a lock.
/// Emitted on the presence event kind, TTL 8 s, refreshed while the user is
/// interacting with the card.
pub struct InteractionActivity {
    pub interaction: InteractionId,
    pub actor: ActorId,
    pub device: DeviceId,
    pub activity: CardActivity,
    pub expires_at: OffsetDateTime,
}

pub enum CardActivity {
    Viewing,
    /// An option is selected / text is being typed, but not submitted.
    Composing,
    /// `interaction.resolve` is in flight from this actor.
    Submitting,
}
```

Rules that keep this from becoming a lock in disguise:

- **It never disables a control.** Anyone may still answer; the CAS still
  decides. A soft lock was rejected in 12 §4.4 and this does not reintroduce it.
- **`Submitting` from another actor renders a 400 ms delay on your own submit
  button's ripple, not a disable.** Enough to make a human hesitate, not enough
  to block them. If the other submission lands first, you get the normal
  `conflict` path with its transient note.
- **It expires.** A device that dies mid-compose leaves nothing behind after 8 s.
- **`kind: "me"` composing is not shown at all after 5 s** — you know you have
  the card open on the laptop; a permanent banner about yourself is noise.

### 3.4 What presence deliberately does not do

- No "last seen 3 hours ago" for offline devices on the session row. That is a
  social-app pattern; here it answers no question.
- No typing indicator for *agent* output. An agent is always "typing"; the
  `AgentState` chip already says what it is doing.
- No presence in any out-of-band payload. There is no notification channel at
  all in v1 ([D12](../architecture/decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead)),
  and a future `Notifier` plugin is given a pointer, never presence.

---

## 4. The writer token as a UX

[12 §3](../architecture/12-collaboration.md#3-the-writer-token) defines the
semantics and this document does not change one of them. What follows is how the
semantics are *presented*, and the governing requirement is:

> **A single user with two devices must never perform an explicit acquire step,
> and must never lose keystrokes.**

Those two are in tension only if acquisition is slow or if failure is silent.

### 4.1 Implicit acquisition on first keystroke

The composer and the terminal input are **enabled by default** whenever the token
is `Free`, or held by an `Idle`/`Background`/`Stale` actor. The user types; the
client acquires.

```ts
// Client-side input gate. One place, both surfaces.
async function onFirstInput(sess: SessionState, bytes: Uint8Array) {
  if (sess.writer?.isMe) return send(bytes);

  if (sess.writer == null || isSoftFree(sess.writer)) {
    // Optimistic: the input bar was already enabled, and 12 §7.1 permits an
    // optimistic acquire on a Free token. Keystrokes buffer locally, ordered.
    inputBuffer.push(bytes);
    const r = await rpc("session.writer.acquire", {
      session: sess.id, keep_size: shouldKeepSize(sess),
    });
    if (r.ok) return flush(inputBuffer, r.epoch);
    return onAcquireLost(r.error);          // §4.5
  }
  // Genuinely contended: the bar was already visibly disabled. See §4.4.
}

/// Held, but by an actor who is provably not typing.
function isSoftFree(w: Writer): boolean {
  return w.liveness === "idle" || w.liveness === "background" || w.liveness === "stale"
      || (Date.now() - w.lastInputAt) > 15_000;
}
```

Two things make this safe:

- **The buffer is bounded and ordered.** At most 4 KiB or 1.5 s of keystrokes
  are held. Beyond either, input is rejected loudly rather than accumulated —
  buffering forever is how a user types a paragraph into the void.
- **Every flushed write carries the epoch** returned by the acquire
  ([12 §3.2](../architecture/12-collaboration.md#32-state)), so a token that
  changed hands between the acquire and the flush rejects the bytes rather than
  landing them in someone else's editing session.

### 4.2 The timing rules, stated exactly

| Rule | Value | Rationale |
|---|---|---|
| Token auto-releases after no input | **90 s** (12 §3.3, unchanged) | Prevents a closed laptop holding a session. |
| Treated as *soft-free* by another of the **same identity's** devices | **15 s** since last input, or liveness ∉ {`Active`} | This is the silent-handoff window. It is short because the contended case for one identity is nearly nonexistent, and long enough to survive a pause for thought at the keyboard. |
| Treated as soft-free for a **different identity** | never; only the 90 s auto-release applies | Another person's pause is not an invitation. |
| Silent handoff notification to the losing device | a toast, no modal | §4.5. |
| Explicit takeover grace | **5 s** (12 §3.3, unchanged) | Only reachable in the genuinely contended case. |
| Grace skipped entirely when the holder is `Background`/`Stale` | **yes** | Resolves [12 §9 Q2](../architecture/12-collaboration.md#9-open-questions) in favour of skipping: a phone in a pocket can neither see the countdown nor cancel it, so the grace is a pure 5 s delay with no possible consent. |

**Silent handoff, concretely.** Phone types at 16:11; laptop last typed at
16:04; laptop's liveness is `Idle`. The phone's `session.writer.acquire` succeeds
without `force`, because the daemon's acquire path treats a soft-free token from
the same identity as `Free`. This is the one semantic **ADDITION** §10 requires
in the token itself, and it is small: `acquire` gains an `assume_idle: bool`
which the daemon honours only when `holder.identity == requester.identity` and
`isSoftFree(holder)` on the server's own clock.

### 4.3 What the indicator looks like

Unobtrusive means: present at a glance, never a modal, never in the thumb zone.

```
 phone, block view, I hold the token
 ┌──────────────────────────────────────┐
 │ api-gateway · claude            ✎    │   ← a single filled pen glyph, accent colour
 └──────────────────────────────────────┘

 phone, block view, my laptop holds it and is Active
 ┌──────────────────────────────────────┐
 │ api-gateway · claude       ✎ laptop  │   ← glyph + device, muted
 └──────────────────────────────────────┘
   [ composer disabled: "laptop is typing · tap to take over" ]

 phone, laptop holds it but is Idle 40 s
 ┌──────────────────────────────────────┐
 │ api-gateway · claude       ✎ laptop  │   ← same, but composer is ENABLED
 └──────────────────────────────────────┘
   [ composer placeholder: "message claude" ]   ← no mention of the token at all
```

The third state is the important one and it is the common one. The user sees a
faint indication that the laptop was recently driving, and types anyway. The
token moves. Nobody read anything about locks.

### 4.4 The genuinely contended case

Two `Active` actors, or one actor typing *right now*. Only here does an explicit
control appear, and only here does the 5 s grace happen:

- The composer is **visibly disabled before the user touches it**, with the
  holder named — [12 §3.4](../architecture/12-collaboration.md#34-visual-indication-is-mandatory-on-every-surface)'s
  non-negotiable rule.
- The disabled bar is itself the button: tapping it sends
  `writer.acquire { force: true }`, showing a 5 s countdown sheet.
- The holder sees a full-width banner with `[k]eep`. One cancel per takeover; a
  second request within 60 s cannot be cancelled (12 §3.3).
- If taking the token would resize the PTY by more than 20 %, the takeover sheet
  offers **"take input without resizing"** (`keep_size: true`) as the *default*
  button on a phone, because a phone taking authoritative size from a laptop is
  almost never what anyone wants
  ([07 §4.3](../architecture/07-remote-protocol.md#43-the-resize-problem)).

### 4.5 What the loser sees

Never nothing, and never a silent revert
([12 §7.3](../architecture/12-collaboration.md#73-correction-is-always-visible)).

| Situation | What the losing surface shows |
|---|---|
| Optimistic acquire failed; buffered keystrokes not sent | Toast: *"Not sent — `laptop` is typing."* The buffered text is **restored into the composer** where one exists, and copied to the clipboard with an "undo/copy" affordance where the target was the raw terminal. Losing typed characters is the failure this whole section exists to prevent. |
| Token taken from me by force | Banner: *"`iPhone` took input 2 s ago."* Input bar disables. No countdown (it already elapsed). |
| Token idle-released while I was reading | Nothing. It is not an event; the bar simply stays enabled and the next keystroke re-acquires. |
| Epoch-stale write rejected mid-flight | *"Input not sent — laptop took over"* ([12 §6 C1](../architecture/12-collaboration.md#c1--two-clients-type-into-the-same-session-at-once)), with the same restore-into-composer behaviour. |

---

## 5. Open and replay — from cold start to the right screen

> **Rewritten for [D12](../architecture/decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead).**
> This section previously specified a notification-tap chain: push → deep link →
> service-worker pre-warm → a 535 ms cold-start budget to an answerable card.
> **None of it has a producer.** No notification backend ships, so nothing ever
> taps, `src=push` is never set, the SW has no `push` event to pre-warm on, and
> the budget measured a segment that does not exist. What survives is everything
> after the tap — and it now has to carry the whole journey.

### 5.1 The chain, as it actually is

```
 agent blocks, and nothing buzzes                       (D12: this is the cost)
   └─ InteractionLedger opens int_88                    (06 §5.1)
        └─ ... time passes. Possibly the interaction goes terminal in the gap.
             └─ USER OPENS THE APP                      ← the only trigger there is
                  └─ app shell from SW precache
                       └─ reconnect: hello{resume} → welcome
                            └─ Resync per session       (07 §5.2)
                                 └─ REFETCH, mandatory before ranking:
                                      · interaction.list { since_read_mark,
                                                           include_terminal }
                                      · attention state per session
                                 └─ rank (§2.3)
                                      └─ PRESENT: what needs you, first
```

The user learns their agent needed them **when they next open a client**. That
is the deal D12 makes, and this section is the design that makes it a good one
rather than a resignation. Everything here is more important than it was under
push, not less: it is now the *only* path.

### 5.2 The refetch is not optional

A `Resync` rebuilds what *is*. It does not surface what *happened*. An
interaction that opened **and reached a terminal state** entirely inside the
offline gap is in no snapshot and in no replayed event — the user opens the app
to a calm, idle session and never learns their agent asked at 02:14 and gave up
at 02:19.

So before the ranking in §2.3 runs, the client fetches:

1. **`interaction.list { since_read_mark: true, include_terminal: true }`** —
   the durable attention log
   ([20 §12.5](../architecture/20-recall-and-usage.md#125-attention-and-the-durable-attention-log)),
   which is the only record of interactions that came and went while
   disconnected. Entries carry a discriminated outcome, so *"the laptop answered
   this"*, *"the agent gave up"* and *"your answer may not have reached it"* are
   three different lines, never one grey "closed".
2. **Attention state per session** — the live half.

This is a protocol requirement, stated in
[07 §5.2](../architecture/07-remote-protocol.md#52-replay-window), not a client
preference. Ranking on live state alone silently drops exactly the events the
user most needed to see.

### 5.3 What the first screen shows

Two bands, in this order, and the order is the whole design:

- **Needs you now** — sessions with an open interaction, then `Blocked`
  sessions omt cannot render a card for. These are actionable; §5.5's answer
  flow applies unchanged.
- **Happened while you were away** — the attention log's terminal entries, each
  with what was asked, what became of it, and when. **Not dismissible by
  scrolling past**: they clear when the read mark advances, i.e. when the user
  actually looks at the session. An entry whose outcome is `Undelivered` offers
  the terminal view; one that is `Abandoned` offers *"send this as a message
  instead"* (§5.5's rule, which was always the right one and is now the common
  one).

Below both, the digest
([20 §8.2](../architecture/20-recall-and-usage.md#82-the-digest)) when the user
has been away ≥ 30 minutes, then the ranked session list. The digest's
interaction counts — including expired, abandoned and undelivered — are required
rendering for exactly this reason.

### 5.4 Cold-start latency budget

Measured from **app foreground to the ranked first screen**, phone on LTE,
tailnet direct. Under D12 this is the number the product lives or dies on: it
replaces a notification tap, so it has to feel like one.

| Segment | Budget (p50) | Notes |
|---|---|---|
| OS icon tap → PWA process start | 250 ms | Not ours. iOS standalone PWA cold start. |
| App shell from SW precache | 80 ms | Cached; never network ([08 §8.6](../architecture/08-web-client.md#86-pwa--installable-with-no-push)). |
| Paint skeleton: last-known session list from local state | 30 ms | Shown immediately, marked stale, never presented as current. |
| TCP + TLS (1.3 resumption) | 60 ms | |
| `hello{resume: session_token}` → `welcome` | 45 ms | Warm path. **Must not hit argon2id** — [connectivity §5](../research/connectivity.md#5-latency-budget) puts that at 100–300 ms of CPU alone. |
| `Resync` + snapshot for the top session | 80 ms | Only the session that will be presented first; the rest resync lazily. |
| `interaction.list` + attention state | 60 ms | One round trip, issued **in parallel** with the resync above — it does not depend on it. |
| Rank + render | 35 ms | |
| **Total** | **≈ 640 ms** | Target **< 900 ms p50, < 2.5 s p95**. |

Three mitigations, replacing the SW pre-warm that no longer has a trigger:

- **Render the stale list instantly, correct it in place.** The last-known
  ranked list is in local storage and paints before the socket opens, visibly
  marked stale. Reordering when the truth arrives is acceptable; a spinner for
  600 ms is not.
- **Parallel, not sequential.** The attention refetch is independent of the
  per-session resync and must not be serialized behind it. The previous chain's
  serial dependency was affordable when a push payload had already delivered the
  card; it is not affordable now.
- **A cold credential path is never on this route.** If the resume token
  expired, the client shows the stale list with a *"reconnecting"* strip rather
  than an auth screen, and only falls back to full auth if resume fails.

### 5.5 Answer, confirmation, and what happens next

Unchanged in substance — this is the part of the old §5 that was always right.

- **`interaction.resolve` is never optimistic**
  ([12 §7.1](../architecture/12-collaboration.md#71-what-may-be-optimistic)). The
  chosen option shows a spinner in place; the card does not move until the server
  answers.
- **On success**: the card animates to its resolved state, a haptic
  `navigator.vibrate(15)` fires.
- **Success now means observed, not written.** Per
  [06 §5.1](../architecture/06-agent-layer.md#51-lifecycle) the card passes
  through `Submitted` before `Resolved`, and the UI must not claim victory at
  `Submitted`. It shows *"sent — waiting for the agent"* until the confirming
  observation arrives, and on `Undelivered` it shows *"your answer may not have
  reached the agent — check the terminal"* with the response text preserved and
  the terminal view one tap away. **No retry button**: an injection is
  at-most-once, and the only actor permitted to re-answer is a human looking at
  the screen.
- **Then what?** The default is **stay, don't return**. The card resolves in
  place and the view remains on that session, scrolled to the agent's next
  output. Rationale: the most common next event is the agent producing something
  in the next few seconds — often another question — and bouncing the user back
  to a list means they miss it. A **"⌂ Done"** button appears in the header for
  60 s afterwards for the "answer and pocket the phone" case; it is prominent but
  not automatic.
- **Answered elsewhere while you were opening it.** The `conflict` path
  ([12 §4.2](../architecture/12-collaboration.md#42-the-race-concretely)) renders
  as the resolved card with *"Answered: Postgres — by laptop, 3 s ago"* and a
  transient note. Notably, if this happens **before** the user has made a
  selection, it is not an error state at all — it is just the card's current
  truth, shown calmly. The apologetic "your answer was not applied" note appears
  only when the user actually chose something.
- **Already `Abandoned`** (the agent moved on —
  [12 §6 C10](../architecture/12-collaboration.md#c10--resolution-arrives-after-the-agent-already-moved-on)):
  *"Too late — the agent continued without an answer."* Never silently swallowed.
  The card offers **"send this as a message instead"**, which puts the intended
  answer into the composer as a prompt. The user's decision is not wasted. Under
  D12 this is no longer a rare race — it is the normal outcome of an overnight
  gap — so it must be a first-class, unapologetic rendering rather than an error
  path.

### 5.6 Noise control — deferred, and why the design is kept

There are no notifications to control, so `NotifyPrefs` has no consumer in v1
and **ships as nothing**. It is retained here in outline because
[D12](../architecture/decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead)
keeps the `Notifier` extension point open on purpose — a future native app or a
user's ntfy plugin needs exactly this policy, and rediscovering it is waste.

Two of its rules **do** apply in v1, because they govern the attention *screen*
rather than a notification:

1. **`Actionable` is the default lens.** The first band is things the user can
   act on; the "while you were away" band is separate and quieter. A turn ending
   is not an obligation.
2. **Never surface your own action.** Items attributed to the same identity are
   never shown as needing attention. Being told about a thing you just did is the
   fastest possible way to look broken.

The remainder — quiet hours, coalescing windows, cross-device push dedup,
presence suppression of a push — presupposes a push channel and is dormant:

```rust
/// RESERVED. No consumer in v1 (D12). Retained for a future Notifier plugin
/// or native app; not persisted, not exposed as config, not in the catalog.
pub struct NotifyPrefs {
    pub level: NotifyLevel,                    // Actionable (default) | Involved | Everything | Muted
    pub sessions: HashMap<SessionId, NotifyLevel>,
    pub quiet_hours: Option<QuietHours>,
    pub suppress_when_present: bool,
    pub coalesce: Duration,
}
```

Its most important rule is recorded for whoever builds that plugin: **default to
`Actionable`**. The tempting default (notify on everything, let the user turn it
down) trains people to disable notifications in week one, after which the
feature is dead.

---

## 6. Attaching a second machine in 30 seconds

The target: from "I have omt on my laptop and on a build server" to "my phone
shows both, merged" in under 30 seconds and without typing a URL.

### 6.1 The flow

```
 on the server (or over ssh):     $ omt pair
                                  ┌──────────────────────────┐
                                  │  ▄▄▄▄▄ ▄ ▄▄  ▄▄▄▄▄       │
                                  │  █   █ ██▄▀  █   █       │   verification: 4172
                                  │  █▄▄▄█ ▀ ▄▀  █▄▄▄█       │   expires in 4:47
                                  │                          │
                                  │  https://build.tail1234. │
                                  │  ts.net/#/join?i=…       │
                                  └──────────────────────────┘

 on the phone:   omt → "＋" → Scan  →  camera  →  "build — is the code 4172?" → Add
                 └─────────────────── ~12 seconds ───────────────────┘
```

Everything here is already specified and this design only fixes the *sequence*:

- The QR encodes the invite URL and nothing else, token in the fragment, 5 min
  TTL, single use, with a 4-digit verification code
  ([connectivity §4.2](../research/connectivity.md#42-qr-code-pairing),
  [13 §3.2](../architecture/13-security.md)).
- The invite is exchanged for a **device-bound credential** on first use
  ([07 §1.3](../architecture/07-remote-protocol.md#13-adding-an-instance)).
- On a tailnet, the phone can skip the QR entirely: the add-instance sheet lists
  tailnet peers advertising omt, one tap each.

### 6.2 What is stored where

| Item | Where | Notes |
|---|---|---|
| Per-instance credential (device-bound) | phone, IndexedDB, optionally encrypted with a device passphrase ([08 §3.1](../architecture/08-web-client.md#31-model)) | Never in a URL after first use; `history.replaceState` cleans it. |
| Device keypair | phone, non-extractable `CryptoKey` | Revocation is per device. |
| ~~Push subscription~~ | — | **None.** No notification backend ships ([D12](../architecture/decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead)), so federation costs no per-instance subscription. A future `Notifier` plugin would reintroduce the N-subscriptions problem, and §6.3's rule about hiding it still applies then. |
| `ActorContinuity` (recents, drafts, prefs) | each instance | §1.3. |
| Instance label, colour, order | phone (per-device) | Purely presentational. |

### 6.3 Making N connections feel like one product

This is the part that determines whether "attach a second machine" feels like
adding a feature or adding a chore.

1. **There is no instance switcher, and no per-instance home screen.** The home
   screen is the merged, attention-sorted list
   ([07 §1.6](../architecture/07-remote-protocol.md#16-the-unified-session-list)).
   Instance is a **subtitle and a filter chip**, never the primary grouping.
   Someone with one instance sees no instance chrome at all.
2. **One attention screen, not N.** There are no notifications
   ([D12](../architecture/decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead)),
   so what must be unified is the open-and-replay screen (§5.3): the client
   fetches the attention log from every reachable instance in parallel and
   presents **one** merged "needs you" band and **one** "while you were away"
   band, with instance as a subtitle. An unreachable instance contributes a
   single honest row — *"workstation: not reachable, last seen 2 h ago"* — never
   a silent omission, because a missing instance and a quiet instance look
   identical otherwise.
3. **One settings surface with per-instance rows**, not a settings screen per
   instance.
4. **Adding an instance never resets the view.** After pairing, the client
   returns to exactly the screen the user was on, with the new instance's rows
   merging in. A "connected to `build` · 3 sessions" toast is the entire
   announcement.
5. **Capability differences degrade per row, never per view**
   ([08 §3.4](../architecture/08-web-client.md#34-graceful-degradation-across-catalog-versions)):
   a control unavailable on one instance is disabled *with the reason*, not
   hidden, so the product does not appear to change shape when you scroll.
6. **Cross-instance actions are honest about their boundary.** Selecting sessions
   on two instances and hitting an action gates on the intersection
   ([07 §1.5](../architecture/07-remote-protocol.md#15-federating-across-versions)),
   with the excluded instance named.

### 6.4 Failure at pairing time

Pairing is the highest-abandonment moment in any product like this, so every
failure names its fix, in the style of
[09 §5.5](../architecture/09-ssh-and-media.md#55-tier-4--out-of-band-and-why-it-is-fine):

| Failure | Message |
|---|---|
| QR encodes a loopback URL | `omt pair` **refuses to draw the QR** and prints the `tailscale serve` command ([connectivity §4.2](../research/connectivity.md#42-qr-code-pairing)). |
| Invite expired | The phone says *"This code expired. Run `omt pair` again."* — the desktop redraws automatically. |
| Verification code mismatch | *"That code is from a different machine."* Abort, do not store. |
| Instance reachable but proto-incompatible | *"`build` runs omt 0.2 (protocol 0); this app needs protocol 1. Update `build`."* Terminal state, no retry ([07 §1.4](../architecture/07-remote-protocol.md#14-per-instance-connection-state)). |
| TLS failure on a self-signed cert | Named explicitly with the fingerprint, and the tailnet path recommended. Never a generic browser interstitial if we can help it. |

---

## 7. Making remote feel local — latency and honesty

### 7.1 The two rules

1. **Optimism is permitted only where the client can predict the outcome and
   where being wrong is recoverable without losing user intent.**
2. **Whatever is not confirmed must look unconfirmed.** A UI that lies briefly
   and then quietly corrects itself is worse than a slow one
   ([12 §7.3](../architecture/12-collaboration.md#73-correction-is-always-visible)).

### 7.2 The exact table

Extends [12 §7.1](../architecture/12-collaboration.md#71-what-may-be-optimistic)
with the continuity surfaces; where the two overlap, 12 wins.

| Action | Optimistic | Pending appearance | Revert |
|---|---|---|---|
| Terminal keystroke echo | **yes**, RTT-gated (§7.3) | underline when RTT > 50 ms | repaint from server state |
| Scroll, collapse, tab, pane focus, view switch | **yes** | none | n/a (local only) |
| Composer text / draft edit | **yes** (local truth) | none | draft sync conflict → §2.4 |
| `agent.queue.enqueue` | **yes** | greyed row + spinner | row removed, toast |
| `session.writer.acquire` on `Free`/soft-free | **yes** | input bar enabled, tiny pulse on the `✎` glyph | keystrokes restored to composer (§4.5) |
| Draft sync (`continuity.draft.set`) | **yes**, fire-and-forget | none | last-write-wins; loser is shown, never merged |
| Mute / notification pref | **yes** | none | toast on failure |
| `session.send_text` (composer submit) | **partial**: the message appears in the transcript as "sending" | dimmed with a spinner | stays, marked failed, with **Retry** — never disappears |
| `interaction.resolve` | **NO** | spinner on the chosen option | n/a |
| Writer takeover (`force`) | **NO** | 5 s countdown | n/a |
| `config.set`, anything `DESTRUCTIVE` | **NO** | confirm sheet then spinner | n/a |
| `session.close`, `agent.interrupt` | **NO** | button shows a spinner | n/a |

`agent.interrupt` deserves a note: it is tempting to make optimistic because
users tap it impatiently. It is not, because "I stopped it" is a claim about the
agent's state that only the agent can confirm, and a false one is dangerous.

### 7.3 Predictive echo — the rules from mosh

[connectivity §3.1](../research/connectivity.md#31-what-mosh-actually-does-and-what-to-steal)
specifies the mechanism; the design commitments are:

- **RTT-gated.** No prediction below ~20 ms smoothed RTT — nothing to win, and
  every prediction is a chance to be wrong.
- **Epoch/confidence state machine.** Predictions are displayed only after the
  server has confirmed one. A program that does not echo (`sudo`, `vim`, an
  agent's TUI) stops producing predictions within one round trip.
- **Underline when slow.** Unconfirmed characters are underlined only above
  ~50 ms RTT, where the marking is information rather than noise.
- **Bounded to printable characters, `Backspace`, `Left`/`Right`, outside the
  alt-screen.**
- **Protocol prerequisite.** The terminal frame header must carry the highest
  client input sequence the PTY has consumed — the `ack` field
  [connectivity §3.1](../research/connectivity.md#31-what-mosh-actually-does-and-what-to-steal)
  asks for and [07 §3.6](../architecture/07-remote-protocol.md#36-binary-payloads)'s
  8-byte header does not yet have. **This must be added before the wire freezes**,
  even though predictive echo itself is a fast-follow. Adding a field later is a
  protocol break; reserving it now is free.

### 7.4 The spinner rule

> **Nothing shows a spinner before 200 ms. Nothing goes unmarked after 400 ms.**

| Elapsed | Presentation |
|---|---|
| 0–200 ms | Nothing. The control stays in its pressed state. A spinner that flashes for 90 ms reads as a glitch and makes a fast app feel unreliable. |
| 200–400 ms | Inline, in-place progress: the control's own affordance animates (a ring on the tapped option, a pulse on the send button). No layout shift, no overlay. |
| 400 ms – 2 s | The same, plus the surface enters the **unconfirmed** style for any optimistic overlay ([12 §7.2](../architecture/12-collaboration.md#72-applying-corrections)). |
| > 2 s | Explicit: *"still waiting on `workstation`…"* with a **Cancel** that sends the protocol's `Cancel` ([07 §3.2](../architecture/07-remote-protocol.md#32-the-envelope)). Never an indefinite modal. |

Layout must never shift when a spinner appears; the space is reserved. Reflowing
a card because a request is slow is how a user taps the wrong button.

### 7.5 Reverting without losing work

The universal rule: **a reverted action returns the user's input to a place they
can act on it.** Concretely — failed composer submit stays in the transcript as
retryable; rejected keystrokes go back to the composer or the clipboard (§4.5);
a failed draft sync keeps the local text and shows the other version; a failed
`interaction.resolve` leaves the selection made, so a retry is one tap.

The only thing ever discarded silently is a *prediction* (§7.3), because it was
never the user's input — it was our guess about the display.

---

## 8. The degradation ladder

Five rungs. The user must always know which one they are on, and the indicator
must never be silent about a downgrade.

### 8.1 The rungs and the indicator

| Rung | Definition | Indicator |
|---|---|---|
| **1 · Full** | Connected, RTT < 150 ms, no lag events | Nothing. A green dot on a healthy connection is noise. |
| **2 · High latency** | RTT > 150 ms sustained 5 s, or DERP-relayed | Amber pip in the header + `↻ 280 ms` on tap. If relayed, the tooltip says so and links to the fix — [07 §7 item 4](../architecture/07-remote-protocol.md#7-latency-budget). |
| **3 · Intermittent** | ≥2 reconnects in 60 s, or a `Lagged`/`Resync` | Amber pip + `catching up` badge on affected sessions ([07 §6.3](../architecture/07-remote-protocol.md#63-how-the-client-learns)). An in-window silent reconnect is **rung 1** — [connectivity §3.5 item 3](../research/connectivity.md#35-concrete-recommendation-for-reconnect--resume-cross-ref-07-5): a phone reconnects many times an hour and each must not be an event. |
| **4 · Offline** | No transport, client-side | Red banner: *"Offline — showing state from 16:04."* Every timestamp switches to absolute. |
| **5 · Instance unreachable** | Some instances up, one down | That instance's rows grey with `last seen`; the rest of the app is rung 1. Per-instance, never global. |

### 8.2 Per-capability behaviour

| Capability | 2 · High latency | 3 · Intermittent | 4 · Offline | 5 · Instance down |
|---|---|---|---|---|
| Read scrollback / blocks | Normal; prefetch widened | Served from cache with a staleness stamp | Cache only, stamped | Cache only |
| Live terminal bytes | Coalesced (07 §6.2) | Snapshot-collapsed, `catching up` badge | Frozen, dimmed, `paused` | Frozen |
| Typing into the PTY | Predictive echo engages (§7.3) | Buffered ≤1.5 s then rejected loudly | **Rejected immediately** — typing into a dead socket must fail loudly ([08 §8.5](../architecture/08-web-client.md#85-offline-and-reconnect)) | Rejected |
| Composer draft | Normal | Normal (local) | **Fully available**, saved locally, synced on reconnect | Available; syncs when that instance returns |
| Send prompt | Normal | Queued with `caused_by`, replayed | Queued (idempotent by request id), shown as *"1 pending"* | Queued for that instance |
| `interaction.resolve` | Normal, no optimism | Retried; idempotent by `(interaction, actor, response)` so a retry is safe (12 §4.1) | **Queued and clearly labelled *"will send when reconnected — the agent may have moved on"*** | Same, per instance |
| Interaction *arrival* | Normal | Lossless queue never drops them (07 §6.1) | Push still arrives — it does not use the WebSocket | Nothing arrives; the row says so |
| Media upload (09 §4.3) | Normal, chunked | **Resumed from the last acked offset** | Deferred, held in the draft's `blobs` | Deferred |
| Voice / STT | Normal | Falls back to batch (no interim text) | Recorded locally, uploaded on reconnect, ≤2 min | Deferred |
| Layout / pane ops | Normal | Normal (local view), promotes on reconnect | Local view only; [`layout.promote`](../architecture/17-panes-and-layout.md#92-layout) disabled with a reason | Disabled with a reason |
| Config changes | Normal | CAS retried | **Disabled** — CAS semantics cannot be honoured offline | Disabled |

### 8.3 The honesty rules

- **Every downgrade is announced once**, at the moment it happens, and the
  indicator persists while it lasts. Silent degradation is the failure mode this
  ladder exists to prevent.
- **Every upgrade is silent.** Recovering is not news; the indicator simply
  disappears. Announcing recovery doubles the notification count for no
  decision-relevant information.
- **Timestamps become absolute below rung 3.** "2 minutes ago" is a lie when the
  data is 40 minutes old and the clock is the only thing still updating.
- **No rung ever hides a control.** Controls disable with a reason, per
  [08 §3.4](../architecture/08-web-client.md#34-graceful-degradation-across-catalog-versions).

---

## 9. Anti-patterns this design rejects

**A1 — Forcing one layout on every device.** Rejected in
[17 §3.3](../architecture/17-panes-and-layout.md#33-decision-per-client-layout-views-one-shared-default)
and reaffirmed here: a phone gets its own `Adaptive` view. tmux's `latest`
(reflow everyone to the newest client) is the most-complained-about multiplexer
behaviour in existence, and `largest` (crop the phone into a four-way split) is
merely a different way to be unusable. Layout is per-device because layout is a
property of the screen.

**A2 — The phone as a read-only viewer.** Directly forbidden by
[D2](../architecture/decisions.md#d2--remote-is-exactly-equivalent-to-local). Any
design that makes remote *safer* by making it weaker produces the second-class
mobile experience the product exists to eliminate. Notably this includes the
subtler version: shipping "view-only for now, editing later". The parity test
([03 §5](../architecture/03-capability-catalog.md#5-the-parity-contract)) exists
so that this cannot happen accidentally.

**A3 — An explicit "take control" step for a single user.** If you own both
devices, asking you to acquire a lock on yourself is pure ceremony. §4 makes
acquisition implicit on first keystroke with a soft-free window, and keeps the
explicit path for the case that actually has two humans in it.

**A4 — Notifying on every turn end.** Rejected as a default (§5.6). An agent
finishing a turn is not an obligation; treating it as one produces a phone that
buzzes forty times an hour, which produces a user who disables notifications,
which kills the flagship scenario. `Involved` exists for people who want it.

**A5 — Syncing scroll position, selection, or viewport across devices.** Two
screens fighting over one scrollbar. Scroll is per-device
([05 §4](../architecture/05-session-model.md#4-attachment-detach-and-multi-client-viewing)).
The *session* is shared; the *reading position* is not.

**A6 — A "continuity" cloud service.** No hosted state, no relay, no account
([00 §8](../architecture/00-overview.md#8-what-omt-is-not)). Per-actor state
lives on the instances that own the work, which is why §1.3 accepts the
per-instance split rather than inventing a home server.

**A7 — Requiring a pre-commitment to hand off.** No "send to phone" gesture on
the primary path (§2.6). The user cannot predict when they will stand up.

**A8 — A spinner for everything, or a modal progress dialog.** §7.4. A modal
that blocks a phone screen for a 180 ms request is a bug with a UI.

**A9 — Hiding unsupported controls.** Making the phone look like a different
product than the laptop is exactly the confusion
[P3](../architecture/01-principles.md#p3--parity-one-capability-three-surfaces)
exists to prevent. Disable with a reason.

**A10 — Auto-scrolling the returning desktop to live output.** §2.5. The user
came back to look at something; racing them to the bottom of the buffer is the
digital equivalent of tidying up while someone is still reading.

**A11 — Treating an in-window reconnect as an event.** Rung 1, not rung 3 (§8.1).
A status pip at most.

**A12 — A soft claim / lock on interaction cards.** Rejected in
[12 §4.4](../architecture/12-collaboration.md#44-advisory-viewer-presence-on-a-card)
and not reintroduced by §3.3: `CardActivity` changes what is displayed and never
what is permitted.

---

## 10. Required additions

Everything below **does not exist yet**. Declared in doc-03 style so they can be
lifted into the catalog when this design is implemented.

### 10.1 Capabilities

```rust
capability! {
    /// Fetch this actor's continuity state on this instance.
    name = "continuity.get", group = "continuity", verb = "get",
    kind = Query, role = Role::Operator,
    input  = ContinuityGet { include_drafts: bool },
    output = ContinuityState { recents: Vec<Recent>, drafts: Vec<Draft>,
                               read_marks: Vec<(SessionId, Seq)> },
    effects = [],
}

capability! {
    /// Record that this actor is (or stopped) working somewhere. Called on
    /// session focus, debounced 5 s client-side.
    name = "continuity.touch", group = "continuity", verb = "touch",
    kind = Command, role = Role::Operator,
    input  = ContinuityTouch { workspace: WorkspaceId, session: Option<SessionId> },
    output = ContinuityTouchAck { seq: Seq },
    effects = [],
}

capability! {
    /// Upsert or clear a draft. `version` is CAS; a mismatch returns
    /// `precondition_failed` with the current draft (§2.4).
    name = "continuity.draft.set", group = "continuity", verb = "draft-set",
    kind = Command, role = Role::Operator,
    input  = DraftSet { key: DraftKey, text: Option<String>, caret: usize,
                        blobs: Vec<BlobId>, version: u64 },
    output = DraftSetAck { version: u64, seq: Seq },
    effects = [],
}

capability! {
    /// Advance this actor's read mark for a session. Under D12 this is what
    /// clears an entry from the "while you were away" band (§5.3), so it is a
    /// primary capability rather than bookkeeping.
    name = "continuity.read_mark.set", group = "continuity", verb = "read-mark-set",
    kind = Command, role = Role::Operator,
    input  = ReadMarkSet { session: SessionId, seq: Seq },
    output = ReadMarkSetAck { seq: Seq },
    effects = [],
}

// REMOVED by D12: `continuity.notify.set` and `continuity.notification.ack`.
// Neither has a producer without a notification backend — there are no
// preferences to set and no delivery to acknowledge. `NotifyPrefs` is retained
// as a reserved type only (§5.6); it is not persisted and not in the catalog.
// A future `Notifier` plugin reintroduces both, and §5.6 records their design.

capability! {
    /// Advisory card activity (§3.3). Ephemeral, TTL 8 s, never persisted.
    name = "interaction.activity", group = "interaction", verb = "activity",
    kind = Command, role = Role::Operator,
    input  = SetActivity { interaction: InteractionId, activity: CardActivity },
    output = SetActivityAck {},
    effects = [],
}
```

Plus one modification to an existing capability:

- **`session.writer.acquire` gains `assume_idle: bool`** (§4.2). Honoured only
  when `holder.identity == requester.identity` **and** the holder is soft-free by
  the server's clock. It is not `force`: it opens no `PendingTakeover`, fires no
  countdown, and is audited as `acquire`, not as a forced takeover.

### 10.2 Events

All on the existing `presence` and a new `continuity` event kind
([07 §3.7](../architecture/07-remote-protocol.md#37-subscriptions)'s `kinds`
list gains `continuity`).

| Event | Kind | Payload | Purpose |
|---|---|---|---|
| `continuity.draft_changed` | `continuity` | `{ key, version, updated_by, cleared: bool }` | The other device's composer updates or empties. Text is **not** in the event — the receiver fetches it, so a draft is not broadcast to every subscriber of the session. |
| `continuity.recents_changed` | `continuity` | `{ recents: Vec<Recent> }` | Keeps a second device's home ranking fresh. Coalesced to at most one per 10 s. |
| `continuity.read_mark_changed` | `continuity` | `{ session, seq, by }` | Reading a session on the phone clears its "while you were away" entry on the laptop. Replaces `continuity.notify_changed`, which D12 removes along with `NotifyPrefs`. |
| `interaction.activity` | `presence` | `InteractionActivity` | §3.3. |
| `presence.identity_changed` | `presence` | `{ actor, identity }` | Lets a client classify `me` vs `other` (§3.1) without guessing from labels. |

### 10.3 Model additions

- `IdentityId` (owned by
  [23 §1.1](../architecture/23-identity-and-devices.md#11-four-types), not
  defined here) carried on `Actor` and on `Credential`, so per-actor state has a
  key that outlives a connection. This partially answers
  [12 §9 Q6](../architecture/12-collaboration.md#9-open-questions)
  (cross-instance presence) *within* an instance; the cross-instance federated
  identity question remains open.
- `ActorContinuity`, `Draft`, `Recent`, `ReadMark`, `InteractionActivity`
  (§1.2, §2.4, §3.3), persisted in `omt-store` except `InteractionActivity`,
  which is memory-only. Each has a row in
  [21 §1](../architecture/21-data-lifecycle.md#1-the-inventory) and a loss window
  in [21 §6.2](../architecture/21-data-lifecycle.md#62-what-kill--9-loses) —
  CI-enforced, so this list cannot drift out of the durability table.
  `NotifyPrefs` is **not** persisted (§5.6, D12).
- An **`ack: u32`** field in the terminal binary frame header
  ([07 §3.6](../architecture/07-remote-protocol.md#36-binary-payloads)) carrying
  the highest consumed client input sequence (§7.3). Required *before the wire
  freezes*. **Now reserved in 07 §3.6**, with a second rationale recorded there
  that is stronger than this one: it is the only safe resumption mechanism for
  [D15](../architecture/decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)'s
  raw-byte-stream class, which must never be replayed. Predictive echo can ship
  later; correct reconnect cannot.

### 10.4 Parity notes

Every capability above is `Operator` and appears on all three surfaces, per
[03 §5](../architecture/03-capability-catalog.md#5-the-parity-contract). The TUI
bindings are not decorative: the TUI writes drafts (that is where handoff
*starts*), calls `continuity.touch` on focus change, advances read marks, and
renders `interaction.activity`. No capability here is `Parity::Exempt`.

---

## 11. OPEN QUESTIONS

1. **Is per-instance `ActorContinuity` good enough?** A user with four instances
   has four recents lists merged client-side. Drafts are unambiguous (a draft
   belongs to a session, which belongs to an instance), but *recents* ordering
   across instances relies on clock agreement between machines, which
   [12 §5.2](../architecture/12-collaboration.md#52-what-is-not-guaranteed)
   explicitly does not guarantee. Probably fine at minute granularity; unmeasured.
2. **Which identity is a given credential's?** `IdentityId`'s *derivation* is
   settled and not open here: it is the blake3-256 of the identity root public
   key, owned by
   [23 §1.1](../architecture/23-identity-and-devices.md#11-four-types). What is
   open is the **mapping** — today every credential on an instance belongs to the
   same person, and the moment a second person exists, "which identity issued
   this credential" needs an answer that
   [23](../architecture/23-identity-and-devices.md) owns (see
   [23 §13 Q6](../architecture/23-identity-and-devices.md#13-open-questions),
   multi-user identities on one instance). Getting this wrong makes drafts leak
   between people, so it must be settled before multi-user.
3. **Auto-navigate thresholds** (1.5× runner-up, absolute floor) in §2.2 are
   guesses. They need real usage; the suppression heuristic (two back-taps →
   10 min off) is a guess on top of a guess.
4. **Ranking weights** (§2.3) are unvalidated. In particular the draft weight
   (250, ≈ 5 minutes of recency) and the `otherDeviceWriting` penalty (−400) are
   the two most likely to be wrong.
5. **Is the 640 ms open-to-ranked-screen budget (§5.4) achievable on iOS?**
   The SW pre-warm that used to absorb the p95 is gone with push
   ([D12](../architecture/decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead)),
   and this budget now carries the entire discovery journey. The stale-list-first
   mitigation is unvalidated: it may read as "the app showed me the wrong thing
   and then jumped". Needs a device test, and it is the highest-value measurement
   in this document.
6. **Does the soft-free window (15 s) fight the 90 s idle release?** Two timers
   governing the same token, with different owners (client heuristic vs. server
   policy) is a smell. **Recommended resolution, stated explicitly rather than
   left as a maybe: make the server's idle release identity-aware — 15 s when the
   requester's identity equals the holder's, 90 s otherwise — and delete the
   client-side soft-free rule of §4.1–§4.2 entirely.** That leaves exactly one timer,
   owned by the server, with the client's role reduced to rendering it. The
   design in §4.1–§4.2 is left as written until
   [12 §9](../architecture/12-collaboration.md#9-open-questions) — which owns the
   writer token and its 90 s release ([12 §3.3](../architecture/12-collaboration.md#3-the-writer-token))
   — adopts the change; this document does not get to change 12's timer
   unilaterally.
7. **Draft sync on a shared session with two people.** Last-write-wins with a
   visible loser (§2.4) is defensible for one person on two devices and mediocre
   for two people. If real multi-human use appears, per-actor draft keys
   (`(DraftKey, IdentityId)`) is the obvious fix and is a schema change.
8. **Quiet hours timezone.** Carried from the device, but which device, when they
   disagree? Currently: last writer wins. Probably wrong for someone travelling.
9. **`continuity.touch` frequency.** Debounced at 5 s, but a user tabbing between
   sessions generates a lot of writes to a persisted structure. May need to be
   memory-first with a periodic flush.
10. **What happens to drafts when a session is closed or dies?** Currently they
    persist for their 7-day TTL and point at nothing. Offering "restart this
    session with your draft" would be better, and interacts with
    [05 §8](../architecture/05-session-model.md#8-persistence-and-restore)'s
    `Orphaned` restore path.
11. **Cold-start budget (§5.4) has never been measured on a real iOS PWA.** If
    the true p50 is 2 s rather than 535 ms, the one-tap-answer story needs the
    notification action buttons to carry more weight — which is itself
    [08 §11 Q1](../architecture/08-web-client.md#11-open-questions)'s unverified
    assumption.
