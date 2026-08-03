# Session Model — `omt-session`

`omt-session` owns the object graph the whole product is organized around:
**Instance → Workspace → Session → Pane**, plus layout, focus, attachment,
write arbitration, persistence and history. It is L3 in the
[crate map](02-crate-map.md); it owns `omt-term` and `omt-pty` instances and
emits `omt-events`. It contains no transport, no auth, and no UI.

Related: [00 — Overview](00-overview.md) · [01 — Principles](01-principles.md) ·
[03 — Capability catalog](03-capability-catalog.md) ·
[04 — Terminal core](04-terminal-core.md) · [06 — Agent layer](06-agent-layer.md) ·
[07 — Remote protocol](07-remote-protocol.md) ·
[12 — Collaboration](12-collaboration.md) · [13 — Security](13-security.md).

The crate's contract in one paragraph: it is a **synchronous, deterministic state
machine over the session tree**, driven by (a) capability calls arriving from
`omt-daemon`, (b) PTY read completions, and (c) timers. It emits events and
returns effects; it never spawns a task, per
[rule 5 of the crate map](02-crate-map.md#dependency-rules-mechanically-checked).
That is what makes the whole tree testable without a runtime and replayable from
a log.

---

## 1. The object model

```
Instance                 one omt daemon on one machine
 └─ Workspace            a project root (canonical path; usually a git repo/worktree)
     └─ Session          one logical terminal: a PTY + omt-term state + agent binding
         └─ (shown in)
Pane                     a viewport onto a session, a leaf of a Layout tree
Layout                   a BSP tree of panes, owned by a Workspace
Client                   an attached surface (TUI, phone, CLI) with presence
```

Two relationships deserve emphasis because they are where most multiplexers get
it wrong:

- **Pane → Session is many-to-one.** A session may be visible in several panes,
  in several layouts, on several clients, simultaneously. A pane is presentation
  only; closing a pane never kills a session.
- **Session → Workspace is many-to-one and stable.** Multiple sessions in one
  directory is the *normal* case (agent + dev server + shell), not an edge case.
  A session's workspace is fixed at creation and does not follow `cd` — the
  session's *current* directory is tracked separately (from OSC 7, see
  [04 §5.4](04-terminal-core.md#54-osc)) and is what "open a new session here"
  uses.

### 1.1 Types

```rust
pub struct Instance {
    pub id: InstanceId,
    workspaces: IndexMap<WorkspaceId, Workspace>,
    sessions: IndexMap<SessionId, Session>,     // flat; the authoritative store
    clients: IndexMap<ClientId, Client>,
    seq: SeqGenerator,                           // per-session generators live on Session
    history: HistoryStore,
    limits: InstanceLimits,
}

pub struct Workspace {
    pub id: WorkspaceId,
    pub root: CanonicalPath,          // identity — see §7
    pub name: String,                 // display; defaults to the directory basename
    pub git: Option<GitIdentity>,     // repo root, worktree, branch
    pub layout: Layout,               // BSP tree of PaneIds
    pub sessions: Vec<SessionId>,     // membership, ordered by creation
    pub focus: Option<PaneId>,
    pub created_at: Timestamp,
}

pub struct Session {
    pub id: SessionId,
    pub workspace: WorkspaceId,
    pub title: SessionTitle,          // explicit override, else OSC 2, else command
    pub kind: SessionKind,            // Shell | Command { argv } | Agent { kind }
    pub state: SessionState,
    pub term: Terminal,               // omt-term, §4 of doc 04
    pub pty: PtyHandle,               // omt-pty
    pub cwd: Option<PathBuf>,         // live, from OSC 7 / proc inspection
    pub agent: Option<AgentBinding>,  // see 06; a property, not a child object
    pub writer: WriterState,          // §5
    pub viewers: SmallVec<[ClientId; 4]>,
    pub seq: Seq,                     // monotonic per session; every event carries it
    pub created_at: Timestamp,
    pub exited_at: Option<Timestamp>,
    pub env: SessionEnv,              // what omt injected, for reproducible restore
}

pub enum SessionState {
    Starting,
    Live,
    /// The child exited; scrollback and blocks remain readable.
    Exited { status: ExitStatus, at: Timestamp },
    /// The daemon restarted and this session was restored from the store, but
    /// its process is gone. Content is readable; input is refused.
    Orphaned { restored_from: SnapshotId },
    Closing,
}

pub struct Pane {
    pub id: PaneId,
    pub session: SessionId,
    /// Per-pane viewport: scroll offset, selection, search cursor, zoom.
    pub view: PaneView,
    pub size: GridSize,               // derived from the layout and the client's size
}
```

### 1.2 Identity and lifetimes

| Type | Identity | Lifetime | Stable across |
|---|---|---|---|
| `InstanceId` | UUIDv7, generated once, persisted | forever | daemon restart, hostname change |
| `WorkspaceId` | derived: `blake3(canonical_root)[..16]` | as long as the path is open | daemon restart, rename |
| `SessionId` | UUIDv7 | until closed *and* evicted from history | detach, reattach, daemon restart |
| `PaneId` | UUIDv7 | until the pane is closed | client disconnect |
| `ClientId` | UUIDv7, minted at attach | one connection | nothing — a reconnect is a new client |

`WorkspaceId` being **derived** rather than random is deliberate: two omt
instances that open the same path (e.g. after a restore) agree on the id
without coordination, and a workspace can be referenced by path in the CLI
without a lookup round-trip. `SessionId` being random is equally deliberate: a
session's identity must not change when it is renamed or moved between
workspaces.

**Lifetime rules:**

1. Closing a **pane** removes it from the layout. If it was the last pane
   showing a session, the session keeps running (detached). This is the tmux
   behaviour users expect and the opposite of a tabbed terminal.
2. Closing a **session** kills the PTY (SIGHUP, then SIGKILL after
   `close_grace`, default 3 s), removes all panes showing it, and moves its
   metadata to history.
3. Closing a **workspace** detaches all its sessions; sessions are *not* killed
   unless `close_sessions: true` is passed. The layout is persisted so reopening
   the path restores it.
4. An **exited** session is retained for `exited_retention` (default 30 min, or
   until explicitly closed) so the user can read the last output and re-run.

---

## 2. Layout: the BSP tree

```rust
pub enum Layout {
    Leaf(PaneId),
    Split {
        id: SplitId,
        dir: Direction,               // Horizontal (side by side) | Vertical (stacked)
        /// Children with fractional weights summing to 1.0. N-ary, not binary —
        /// see rationale below.
        children: Vec<(f32, Layout)>,
    },
    /// Exactly one pane fills the workspace; the real tree is preserved.
    Zoom { pane: PaneId, saved: Box<Layout> },
    Empty,
}
```

**Decision: an n-ary tree, not a strictly binary one.** A binary tree makes
"three equal columns" representable only as nested splits, which then behave
wrongly when the middle one is closed (the remaining two get 1/2 and 1/2 of an
oddly-shaped region rather than 1/2 and 1/2 of the whole). The n-ary form makes
even-split, balance, and close-and-redistribute all trivial and total. Splits
with the same direction as their parent are automatically flattened into it,
which keeps the tree canonical: **no `Split` ever has a child `Split` of the same
direction**, and that invariant is asserted after every operation.

### 2.1 Operations

```rust
impl Layout {
    pub fn split(&mut self, at: PaneId, dir: Direction, new: PaneId, ratio: f32)
        -> Result<(), LayoutError>;
    pub fn close(&mut self, pane: PaneId) -> Result<Option<PaneId>, LayoutError>; // returns new focus
    pub fn resize(&mut self, pane: PaneId, edge: Edge, delta: Fraction) -> Result<(), LayoutError>;
    pub fn swap(&mut self, a: PaneId, b: PaneId) -> Result<(), LayoutError>;
    pub fn move_pane(&mut self, pane: PaneId, to: PaneId, dir: Direction) -> Result<(), LayoutError>;
    pub fn zoom(&mut self, pane: PaneId) -> Result<(), LayoutError>;
    pub fn unzoom(&mut self) -> Result<(), LayoutError>;
    pub fn navigate(&self, from: PaneId, dir: Direction2D) -> Option<PaneId>;
    pub fn apply_preset(&mut self, preset: LayoutPreset);   // Even | MainVertical | MainHorizontal | Tiled
    /// Geometry for a given viewport, honouring minimums.
    pub fn compute(&self, area: Rect, min: GridSize) -> Vec<(PaneId, Rect)>;
}
```

Rules that make the tree behave:

- **`close` redistributes** the closed child's weight proportionally among its
  siblings, then collapses a `Split` with one remaining child into that child.
  Focus moves to the *spatially nearest* sibling (right, else left, else parent's
  next), which matches user expectation better than "next in tree order".
- **`compute` enforces minimums** (default 20×3 cells). If the area cannot hold
  every pane at minimum, panes are dropped from the geometry (not from the tree)
  and reported as `hidden` — the layout survives a phone attaching at 40 columns.
  This is how the same workspace is renderable on a laptop and a phone.
- **`navigate`** uses geometric adjacency computed from the last `compute`, not
  tree order, so directional movement feels right in asymmetric layouts.
- **Zoom is non-destructive** and per-workspace, not per-client. A second client
  viewing the same workspace sees the zoom. (A client that wants a private view
  opens its own workspace view — see §6.)

### 2.2 Sizing with multiple clients

A session's PTY has exactly one size. When N clients view the same workspace at
different terminal sizes, we must choose one.

**The negotiation rule is specified once, in
[07 §4.3](07-remote-protocol.md#43-the-resize-problem), which owns it:** each
session has one authoritative `(cols, rows)` with a `ViewportPolicy` whose
`SizeOwner` is `Writer` (default), `Pinned { by }`, or `Smallest` (opt-in). This
crate stores the policy on the session, applies the resulting size to the PTY,
and reports every client's viewport for presence.

What follows from that here:

- Every client reports its viewport; a non-authoritative client's report updates
  presence and nothing else. Clients whose viewport differs from the
  authoritative size render scaled-to-fit-width and letterboxed, never cropped.
- `SizeOwner::Smallest` is the "nobody is cropped" pairing mode, and is the only
  mode in which any attached client's viewport change can resize the PTY.
- Resizes are debounced and coalesced (250 ms); a drag-resize produces one
  `resize` call to `omt-term`, not sixty. See
  [04 §3](04-terminal-core.md#3-reflow-on-resize) for what a resize costs.
- A client in block view receives no PTY bytes and is never a sizing
  participant, so a phone reading the block list cannot shrink a laptop's
  terminal.

---

## 3. Focus

Focus is a three-level concept and each level is separately addressable, because
"which pane is focused" means different things to the TUI, to a phone, and to
the routing of input.

```rust
pub struct FocusState {
    /// Instance-wide: which workspace the *local* TUI is showing.
    pub active_workspace: Option<WorkspaceId>,
    /// Per workspace: which pane is focused.
    pub focused_pane: HashMap<WorkspaceId, PaneId>,
    /// Per client: what that client is looking at (presence, §6).
    pub client_view: HashMap<ClientId, ClientView>,
}
```

- **Focus is not write permission.** Focusing a pane does not acquire the writer
  token (§5). A client can focus a pane to read it while another client drives
  it. Conflating the two is the mistake that makes collaborative terminals
  unusable.
- Focus changes emit `FocusChanged` events, and `omt-term` is told so it can
  answer DECSET 1004 focus reporting correctly — an agent CLI that dims when
  unfocused should dim when *no client* is focused on it, which is a property of
  the whole instance, not of one client.

---

## 4. Attachment, detach, and multi-client viewing

```rust
pub struct Client {
    pub id: ClientId,
    pub kind: ClientKind,             // LocalTui | Web { user_agent } | Cli | Plugin
    /// The `Actor` this client acts as — id, kind, label, role, credential.
    /// Defined once in [12 §1](12-collaboration.md#1-actors).
    pub actor: Actor,
    pub role: Role,                   // mirror of `actor.role`; Viewer < Operator < Admin
    pub caps: ClientCapabilities,     // can render images? mouse? kitty keyboard?
    pub view: ClientView,
    pub attached_at: Timestamp,
    pub last_seen: Timestamp,
}

pub struct ClientView {
    pub workspace: Option<WorkspaceId>,
    pub panes: SmallVec<[PaneId; 4]>,     // what is on screen right now
    /// Reported viewport. Authoritative only per 07 §4.3's `ViewportPolicy`.
    pub size: GridSize,
    pub mode: ViewMode,                   // Grid | Blocks
}
```

**Attach** is `session.attach { session, since_seq, mode }`. The instance replies
with a snapshot appropriate to `mode`:

- `ViewMode::Grid` → a grid snapshot plus a stream cursor (see
  [04 §4.4](04-terminal-core.md#44-the-web-mapping-xtermjs) for the model and
  [07 §4.2](07-remote-protocol.md#42-the-decision-c-hybrid-byte-stream-primary)
  for the wire encoding).
- `ViewMode::Blocks` → the last N blocks with their metadata and styled text.

If `since_seq` is supplied and still within the retained event window, the
instance replays from it instead — that is the reconnect path, and it is what
makes a phone coming back from a tunnel resume mid-stream rather than flashing.
If the seq is too old, the reply is `resync_required` plus a fresh snapshot.

**Detach** is implicit (transport closed) or explicit
(`session.detach`/`instance.detach`). Detaching:

- removes the client from `viewers` and from the size negotiation;
- releases any writer token it held immediately, on explicit detach *and* on
  transport close — the holder is provably gone
  ([12 §3.3](12-collaboration.md#33-lifecycle));
- leaves the session running. Always. A session is never killed by a client
  going away.

**Multi-client viewing** of one session is the default and needs no mode. Every
viewer receives the same event stream with the same `seq`, so all clients are
eventually consistent and provably ordered
([P6 causality](01-principles.md#p6--collaboration-is-a-runtime-feature-not-just-a-workflow)).
Per-client state — scroll offset, selection, search — lives in `PaneView` on the
client's own pane, so two people can scroll independently while watching the
same live output.

---

## 5. The writer token

This is the concrete answer to [P6](01-principles.md#p6--collaboration-is-a-runtime-feature-not-just-a-workflow)'s
input-arbitration requirement.

> **Ownership.** This section describes the writer token as part of a session's
> **data model** — where it lives in the tree, what it persists, how it
> interacts with attach/detach. **[12 §3](12-collaboration.md#3-the-writer-token)
> defines its semantics** — who may acquire it, what takeover means, the
> timeouts, the epoch check, and how every conflict resolves. Where the two
> disagree, 12 wins and this section is the bug. The numbers below are quoted
> from 12, not set here.

### 5.1 The rule

> **At most one actor may write to a session's PTY at any moment. Holding the
> writer token is a visible, explicit, revocable state, and every client always
> knows who holds it.**

Silent last-write-wins is not acceptable: two people (or a person and an agent)
typing into the same shell interleave into a corrupted command line, and the
failure is invisible until something destructive runs.

### 5.2 Model

The type is `WriterToken`, defined in
[12 §3.2](12-collaboration.md#32-state). `Session::writer` holds one:

```rust
pub struct WriterState {
    /// `None` == Free. See 12 §3.3 for the state machine.
    pub token: Option<WriterToken>,     // { holder, acquired_at, last_input_at,
                                        //   epoch, keep_size, takeover }
    pub policy: WriterPolicy,
}

pub struct WriterPolicy {
    pub idle_timeout: Duration,         // 90 s — 12 §3.3
    pub takeover_grace: Duration,       // 5 s  — 12 §3.3
    pub auto_acquire: bool,             // default true for a single attached client
}
```

`epoch` is the load-bearing field for this crate: every PTY write carries the
epoch the client believed it held, and `omt-session` rejects writes with a stale
epoch. Without it, input in flight when a token changed hands lands in someone
else's editing session.

### 5.3 Operations

Semantics are [12 §3.3](12-collaboration.md#33-lifecycle)'s; the handlers are
here.

```
session.writer.acquire   { session, reason?, force: bool }
                         -> Acquired { epoch, holder } | Conflict { holder, takeover? }
session.writer.release   { session }                  -> Released
session.writer.keep      { session }                  -> Kept   // holder cancels a takeover
session.writer.status    { session }                  -> WriterStatus
```

Takeover is `acquire { force: true }`, not a separate capability: it opens a
`PendingTakeover` with a 5 s grace window that the holder may cancel once with
`writer.keep`. There is no queue of pending claims and no `TakeoverPolicy` enum —
a takeover storm resolves to the most recent requester (12 §6 C5).

Consequences for the data model:

- **`auto_acquire`.** When exactly one client is attached with `Operator`+ and
  the token is `Free`, the first write implicitly acquires it. Single-user
  operation therefore never sees the mechanism at all — the token only becomes
  visible when there is contention, which is the correct ergonomic trade.
- **Writes are checked, not queued.** `session.send_text` / `send_keys` /
  `write_bytes` from a non-holder returns `precondition_failed` with the current
  holder in the structured error detail, so the client can offer "request
  takeover". A stale epoch returns
  `precondition_failed { expected_epoch, actual_epoch }`. Buffering a
  non-holder's keystrokes and replaying them later would be worse than refusing.
- **Idle release** after 90 s of no writes (`Actor::System`), and **immediate
  release on transport close** — the holder is provably gone. All tokens release
  on daemon restart.
- **Agents hold the token too.** When the agent layer injects text as a last
  resort ([D3](decisions.md#d3--synthetic-input-is-bounded-by-state-dependence-not-by-tool-danger)),
  it acquires the token as `ActorKind::Agent`, so a human sees "claude is typing"
  rather than mysterious characters, and can take over.
- **Observers always see the state.** `WriterStatus` is part of every session
  snapshot and every change is an event, so the TUI can render "driven by:
  phone (Ada) · 42 s" in the pane border and a phone can render the same. The
  per-surface indication requirements are
  [12 §3.4](12-collaboration.md#34-visual-indication-is-mandatory-on-every-surface).
- **Read is never gated.** Scrollback, blocks, search, files, diffs and events
  are available to every viewer regardless of the token.

### 5.4 What is not gated

Deliberately outside the token, because they are not PTY writes and serializing
them would break the product ([12 §3.1](12-collaboration.md#31-what-it-governs)
is authoritative):

- `interaction.resolve` — answering an agent's question goes back through the
  agent's own channel, not the PTY, and is separately guaranteed to resolve
  exactly once by the interaction ledger ([06](06-agent-layer.md),
  [12 §4](12-collaboration.md#4-interaction-ownership)).
- `agent.prompt` where the adapter has a structured submit path. Where the only
  path is synthesized keystrokes, the token **is** required.
- Scroll, search, selection, block folding — per-client view state.

`session.resize` **is** gated when it carries `request_authoritative: true`,
because that is a PTY-affecting write; a plain viewport report is not.

---

## 6. Presence

Presence is state, not a side channel, so that a laptop can see a phone is
watching.

**The `Presence` type and its rules are defined once, in
[12 §2](12-collaboration.md#2-presence-is-first-class-state)** — including
`ViewFocus`, the derived `Liveness`, the debounce, and the requirement that every
surface show it. `omt-session` is where it *lives*: it is derived from
`Instance::clients` and each client's `ClientView`, projected per session, and
included in `session.list` / `workspace.list` output.

Emitted as `presence.changed` on attach, detach, view change, viewport change,
liveness change and writer change. Presence is **not** persisted across daemon
restart; clients repopulate it by reconnecting.

---

## 7. Workspace identity

```rust
pub struct CanonicalPath(PathBuf);   // fully resolved: symlinks, `..`, case per-FS

pub struct GitIdentity {
    pub repo_root: CanonicalPath,       // the *common* dir's work tree
    pub worktree_root: CanonicalPath,   // this checkout
    pub is_worktree: bool,              // true when worktree_root != repo_root
    pub head: Option<String>,           // branch name, or short SHA when detached
    pub common_dir: CanonicalPath,      // `git rev-parse --git-common-dir`
}
```

**Identity is `CanonicalPath`, and a git worktree is its own workspace.**

Rationale: `git worktree add ../feature-x` is exactly how people run two agents
on two branches of the same repo at once — it is the flagship omt workflow. If
worktrees collapsed into one workspace by repo root, those two agents would
share a layout, a history scope and a name, which is wrong in every respect.
So: same repo, different worktrees ⇒ different workspaces, related by
`common_dir`.

Consequences and details:

- Workspaces sharing a `common_dir` are **grouped** in the UI ("myrepo · main",
  "myrepo · feature-x") and `workspace.list` returns the grouping, so the phone
  can show them together without them being the same object.
- Canonicalization resolves symlinks. `/tmp` → `/private/tmp` on macOS,
  `~/work` → `/Volumes/…` — without this, the same directory reached two ways
  produces two workspaces and two histories. Case sensitivity follows the
  filesystem, probed once per mount point and cached.
- Git state is refreshed on: workspace open, block completion (the shell snippet
  already reports branch — see [04 §7.2](04-terminal-core.md#72-what-they-emit--standard-osc-133-not-a-private-namespace)),
  and on demand. We never poll, and every `git` invocation uses
  `GIT_OPTIONAL_LOCKS=0`.
- A workspace whose root has been deleted enters `Missing`. Its sessions keep
  running (their processes may still hold the inode); it is greyed in the UI and
  refuses new sessions.
- Non-git directories are perfectly valid workspaces. `GitIdentity` is optional.

---

## 8. Persistence and restore

Coordinated with `omt-store` (L3, `Store` trait). The design goal:

> **A daemon restart — planned or crash — loses at most the tail of output.
> Session metadata, layout, blocks and history survive. Processes do not.**

We do not attempt process survival across daemon restart in v1. Reparenting PTYs
to a separate supervisor is possible (that is how `tmux`'s server works) but it
is a different architecture; see the open questions.

### 8.1 What is snapshotted vs. replayed

| Data | Mechanism | Rationale |
|---|---|---|
| Instance/workspace/session/pane tree, layout, focus | **Snapshot** on every structural change, debounced 500 ms | Small, changes rarely, must be exactly right |
| Session env, argv, cwd at spawn | Snapshot at creation, immutable | Needed to offer a faithful re-spawn |
| Block metadata (command, exit, cwd, git, timing, attribution) | **Append-only log**, one record per block close | The valuable, permanent artifact; also feeds history (§9) |
| Scrollback | **Chunk snapshot**, delta by chunk generation | Per [04 §2.4](04-terminal-core.md#24-scrollback-blocks-of-logical-lines) each chunk has a generation; only changed chunks are written. Written every `scrollback_flush` (default 10 s) and on clean shutdown |
| Live grid (viewport) | Snapshot with the scrollback flush | Small and bounded; makes restore show the last screen |
| Raw PTY byte ring | **Not persisted** | Bounded, ephemeral, only used for live client resume |
| Interaction ledger | Append-only log (owned by `omt-agent`) | Must be exactly-once across restart |
| Credentials | `omt-auth` + `omt-store`, separate file, 0600 | [P8](01-principles.md#p8--security-by-default-no-ambient-trust) |

The append-only log plus periodic snapshots is the standard WAL shape and is
what bounds crash loss: on restart we load the newest snapshot and replay log
records after it.

### 8.2 Crash semantics

```rust
pub enum RestoreOutcome {
    /// Clean shutdown marker found; everything restored.
    Clean { sessions: usize },
    /// No marker; snapshot + log replayed. `lost_tail` estimates the output
    /// written after the last scrollback flush.
    Recovered { sessions: usize, lost_tail: Duration },
    /// Log or snapshot corrupt past a point; restored to the last consistent
    /// record and the remainder quarantined (never deleted).
    Partial { sessions: usize, quarantined: PathBuf, reason: String },
}
```

Rules:

- Every log record is length-prefixed and checksummed. A torn tail record (the
  classic crash artifact) is truncated, not treated as corruption.
- Snapshots are written to a temp file and `rename`d — never written in place.
- A restore never *silently* drops data. `Partial` surfaces as a warning event
  and a banner in every surface.
- Restored sessions come back as `SessionState::Orphaned`. Their content is
  readable, searchable and copyable; writes return `precondition_failed`. The UI
  offers **"restart"** — re-spawn the same argv, in the same cwd, with the same
  injected env, into the same pane, keeping the old scrollback above a
  separator. This is materially better than either silently killing the session
  or pretending it is alive.
- **Retention**: scrollback snapshots are capped per session
  (`store.max_scrollback_bytes`, default 8 MiB) and per instance
  (default 1 GiB), evicting oldest-first. Block metadata is retained far longer
  (default 1 year) because it is small and it is the history.

### 8.3 Versioning

Every persisted structure carries a `format_version` from v1, and
`tests/fixtures/store/v1/` holds checked-in files that CI loads on every build
([P5](01-principles.md#p5--production-grade-from-the-first-commit)). Loading a
newer version than we understand is an error with an actionable message, never a
best-effort partial parse.

---

## 9. Command history

Distinct from scrollback: history is the *list of commands*, structured, durable,
and searchable long after the output is gone. It is populated from block closure
(§8.1), so it exists only where blocks exist — with shell integration it is
complete; without it, heuristic blocks contribute nothing (they have no reliable
command text), and that is an honest degradation rather than a guess.

```rust
pub struct HistoryEntry {
    pub id: HistoryId,
    pub command: String,
    pub workspace: Option<WorkspaceId>,
    pub cwd: Option<PathBuf>,
    pub git_branch: Option<String>,
    pub exit: Option<ExitStatus>,
    pub started_at: Timestamp,
    pub duration: Option<Duration>,
    pub session: SessionId,
    pub host: Option<RemoteHost>,
    pub attribution: Attribution,      // human or agent — see 04 §6.2
}

pub struct HistoryQuery {
    pub scope: HistoryScope,           // Global | Workspace(id) | Session(id) | Cwd(path)
    pub filter: Option<String>,        // fuzzy or literal, per `mode`
    pub mode: MatchMode,               // Prefix | Substring | Fuzzy | Regex
    pub only_successful: bool,
    pub dedup: Dedup,                  // None | KeepNewest
    pub attribution: Option<AttributionFilter>,
    pub limit: u32,
    pub before: Option<HistoryId>,     // cursor pagination
}
```

Decisions:

- **Two scopes that matter: per-workspace and global.** Per-workspace is the
  default for up-arrow-style recall, because the command you want in this repo is
  almost never the command you ran in a different one. Global is the default for
  the palette/search. Per-cwd is available and useful ("what do I usually run in
  `crates/omt-term`?") but is not a default.
- **Dedup keeps the newest occurrence**, walking backwards with a seen-set, and
  keeps **separate seen-sets per attribution class** so an agent's repeated
  `cargo test` does not evict the human's. Dedup is applied at query time, not
  at write time — the raw record of every invocation is preserved, because
  "how many times did this fail before it worked" is real information.
- **Storage** is SQLite via the `Store` backend, with indices on
  `(workspace, started_at)`, `(cwd, started_at)` and an FTS index on `command`.
  Fuzzy matching is done in Rust over an FTS-narrowed candidate set, not in SQL.
- **Sync across sessions is automatic and immediate**: history is instance-level
  state, so a command run in one session is queryable from another the moment
  its block closes, with a `HistoryAppended` event so open palettes update live.
  This is strictly better than shells' per-process `HISTFILE` semantics and is
  worth advertising.
- **Sync across instances is out of scope for v1.** A federating web client
  ([00 §7](00-overview.md)) queries each instance and merges client-side. No
  cloud, per [00 §8](00-overview.md).
- **Redaction.** Commands matching configured secret patterns (default: `export
  *_TOKEN=`, `*_KEY=`, `--password`, and anything the shell marked as
  `HISTCONTROL=ignorespace`) are stored with the value replaced by `‹redacted›`.
  The block's *output* is not redacted — that is a different problem — but the
  durable, searchable, syncable artifact is.

---

## 10. Capability surface

Every operation in this document is reached through the
[capability catalog](03-capability-catalog.md). `omt-session` provides the
handlers; `omt-daemon` registers them. Roles: `V`iewer < `O`perator < `A`dmin.

### 10.1 `workspace.*`

| Capability | Role | Input | Output |
|---|---|---|---|
| `workspace.list` | V | `{ include_sessions: bool }` | `{ workspaces: [WorkspaceInfo], groups: [WorktreeGroup] }` |
| `workspace.get` | V | `{ workspace }` | `WorkspaceInfo` |
| `workspace.open` | O | `{ path, name? }` | `{ workspace, created: bool }` |
| `workspace.close` | O | `{ workspace, close_sessions: bool }` | `{ closed_sessions: [SessionId] }` |
| `workspace.rename` | O | `{ workspace, name }` | `WorkspaceInfo` |
| `workspace.layout.get` | V | `{ workspace }` | `{ layout: LayoutTree, geometry: [(PaneId, Rect)] }` |
| `workspace.layout.set` | O | `{ workspace, layout }` | `{ layout }` |
| `workspace.layout.preset` | O | `{ workspace, preset }` | `{ layout }` |
| `workspace.focus` | O | `{ workspace, pane }` | `{ focused: PaneId }` |
| `workspace.git.status` | V | `{ workspace }` | `VcsSummary` — **deprecated alias** for `workspace.vcs.summary` ([15 §6](15-workspace-explorer.md#6-capabilities)); kept for two minor versions per [03 §7](03-capability-catalog.md#7-versioning) |
| `workspace.worktree.list` | V | `{ workspace }` | `{ worktrees: [WorktreeInfo] }` |
| `workspace.worktree.add` | O | `{ workspace, path, branch, create_branch }` | `{ workspace: WorkspaceId }` — effects: `WRITES_FS`, `SPAWNS_PROCESS` |
| `workspace.history` | V | `HistoryQuery` (scope forced to this workspace) | `{ entries, next_cursor }` |

### 10.2 `session.*`

| Capability | Role | Input | Output |
|---|---|---|---|
| `session.list` | V | `{ workspace?, include_exited: bool }` | `{ sessions: [SessionInfo] }` |
| `session.get` | V | `{ session }` | `SessionInfo` |
| `session.create` | O | `{ workspace, kind, cwd?, env?, size?, pane_target? }` | `{ session, pane }` — effects: `SPAWNS_PROCESS` |
| `session.close` | O | `{ session, force: bool }` | `{ status }` — effects: `DESTRUCTIVE` |
| `session.restart` | O | `{ session }` | `{ session }` — effects: `SPAWNS_PROCESS`, `DESTRUCTIVE` |
| `session.rename` | O | `{ session, title }` | `SessionInfo` |
| `session.attach` | V | `{ session, mode, since_seq?, viewport }` | `Attached { snapshot, seq } \| ResyncRequired { snapshot, seq }` — `viewport` is a report, not a claim; see [07 §4.3](07-remote-protocol.md#43-the-resize-problem) |
| `session.detach` | V | `{ session }` | `Ack` |
| `session.resize` | O | `{ session, cols, rows, pixel_width?, pixel_height?, request_authoritative }` | `{ authoritative: GridSize, owner: SizeOwner }` — per [07 §4.3](07-remote-protocol.md#43-the-resize-problem); requires the writer token only when `request_authoritative` |
| `session.send_text` | O | `{ session, text, submit }` | `{ seq }` — effects: `WRITES_PTY`; requires writer token |
| `session.send_keys` | O | `{ session, keys: [KeySpec] }` | `{ seq }` — effects: `WRITES_PTY`; requires writer token |
| `session.write_bytes` | O | `{ session, bytes }` | `{ seq }` — effects: `WRITES_PTY`; requires writer token |
| `session.signal` | O | `{ session, signal }` | `Ack` — effects: `DESTRUCTIVE` |
| `session.scrollback.get` | V | `{ session, from: Position?, lines, mode }` | `{ lines: [StyledLine], from, to }` |
| `session.search` | V | `{ session, query, cursor? }` | `{ matches, cursor, exhausted }` |
| `session.target_at` | V | `{ session, position }` | `Option<Target>` |
| `session.target_resolve` | V | `{ session, position }` | `ResolvedTarget` (carries `explorer: Option<ExplorerRef>`, [15 §8.1](15-workspace-explorer.md#81-fileline-from-terminal-output)) — effects: `READS_FS` |
| `session.blocks.list` | V | `{ session, before?, limit, filter? }` | `{ blocks: [BlockInfo], next_cursor }` |
| `session.blocks.get` | V | `{ session, block, include_output, max_lines }` | `{ block, output: [StyledLine], truncated }` |
| `session.blocks.rerun` | O | `{ session, block, target_session? }` | `{ seq }` — effects: `WRITES_PTY`, `DESTRUCTIVE` |
| `session.blocks.fold` | V | `{ session, block, folded }` | `Ack` |
| `session.writer.acquire` | O | `{ session, reason?, force, keep_size }` | `Acquired { epoch } \| Conflict { holder }` |
| `session.writer.release` | O | `{ session }` | `Ack` |
| `session.writer.keep` | O | `{ session }` | `Ack` — holder cancels a pending takeover |
| `session.writer.status` | V | `{ session }` | `WriterStatus` |
| `session.history` | V | `HistoryQuery` | `{ entries, next_cursor }` |

`session.blocks.rerun` is marked `DESTRUCTIVE` deliberately: re-running an
arbitrary previous command with one tap must require a confirm gesture, per
[03 §2](03-capability-catalog.md#2-declaring-a-capability). Note that this is a
property of **omt's own** capability, applied identically on the TUI, the CLI and
the phone — not a remote-only gate
([D2](decisions.md#d2--remote-is-exactly-equivalent-to-local)) and not a
judgement about the agent's tools
([D1](decisions.md#d1--omt-adds-no-policy-layer-over-an-agents-permission-semantics)).

### 10.3 `pane.*`

| Capability | Role | Input | Output |
|---|---|---|---|
| `pane.list` | V | `{ workspace }` | `{ panes: [PaneInfo] }` |
| `pane.split` | O | `{ pane, dir, ratio?, session? }` | `{ pane, session }` — creates a session when `session` is omitted |
| `pane.close` | O | `{ pane, close_session: bool }` | `{ focus: PaneId? }` |
| `pane.focus` | O | `{ pane }` | `{ focused }` |
| `pane.navigate` | O | `{ from, dir }` | `{ focused }` |
| `pane.move` | O | `{ pane, to, dir }` | `{ layout }` |
| `pane.swap` | O | `{ a, b }` | `{ layout }` |
| `pane.resize` | O | `{ pane, edge, delta }` | `{ layout }` |
| `pane.zoom` | O | `{ pane, zoomed }` | `{ layout }` |
| `pane.set_session` | O | `{ pane, session }` | `PaneInfo` — retarget a pane at a different session |
| `pane.scroll` | V | `{ pane, to }` | `{ view }` — per-client view state |
| `pane.select` | V | `{ pane, anchor, head, mode }` | `{ text_len }` |

### 10.4 Events emitted

Per [03 §4](03-capability-catalog.md#4-events-are-the-read-side-twin), events are
derived from state changes, never published by hand from handlers.

| Event | When |
|---|---|
| `WorkspaceOpened` / `WorkspaceClosed` / `WorkspaceRenamed` | tree change |
| `LayoutChanged` | any layout mutation, with the new tree and geometry |
| `SessionCreated` / `SessionExited` / `SessionClosed` / `SessionRenamed` | lifecycle |
| `SessionStateChanged` | `Live` → `Exited` → `Orphaned` transitions |
| `SessionResized` | negotiated size changed, with the reason |
| `TerminalDamage` | *not an event* — damage is polled, see [04 §9.3](04-terminal-core.md#93-batching-and-coalescing) |
| `BlockOpened` / `BlockClosed` / `BlockUpdated` | from `omt-term`'s block tracker |
| `CwdChanged` / `TitleChanged` / `Bell` | from `omt-term` host actions |
| `WriterChanged` / `WriterTakeoverRequested` / `WriterTakeoverResolved` | §5, semantics in [12 §3](12-collaboration.md#3-the-writer-token) |
| `presence.changed` | §6, shape in [12 §2](12-collaboration.md#2-presence-is-first-class-state) |
| `HistoryAppended` | §9 |
| `FocusChanged` | §3 |

Every one carries `(instance, session?, seq, ts, source)` per the `omt-events`
envelope.

---

## 11. Invariants

Asserted in debug builds and checked by a `Instance::check_invariants()` that the
property tests call after every operation:

1. Every `PaneId` in a `Layout` exists in exactly one workspace's pane set, and
   every pane is in exactly one layout.
2. Every `Pane::session` names a live entry in `Instance::sessions`.
3. No `Split` has a child `Split` of the same direction (canonical tree, §2).
4. Split child weights sum to 1.0 ± 1e-4, and every weight is > 0.
5. At most one `WriterToken` holder per session, and its `epoch` never decreases.
6. `Session::seq` is strictly monotonic and never decreases across restore.
7. A session in `Exited` or `Orphaned` has no writer and refuses PTY writes.
8. `Workspace::sessions` and `Session::workspace` agree in both directions.
9. Every `ClientId` in `Session::viewers` exists in `Instance::clients`.
10. The authoritative PTY size equals what `ViewportPolicy` resolves to
    ([07 §4.3](07-remote-protocol.md#43-the-resize-problem)): the writer's
    viewport, the pinned size, or the minimum over attached viewports under
    `Smallest`; and the last authoritative size when no viewer remains.

---

## 12. Testing

- **Deterministic simulation.** Because `omt-session` owns no runtime and no
  clock, tests drive it with a virtual clock and a scripted event source: attach
  three clients, contend for the writer, resize, crash, restore, assert. This is
  the primary test style for the crate and it is only possible because of the
  no-async rule.
- **Property tests** over layout: any sequence of split/close/move/swap/resize
  leaves the tree canonical, all weights valid, and every pane reachable;
  `close` never orphans a session.
- **Restore round-trip**: snapshot after an arbitrary operation sequence,
  serialize, restore, and assert structural equality modulo process state.
  Includes torn-tail and corrupt-record cases.
- **Writer-token model check**: a small TLA+-style exhaustive search over 3
  clients × {acquire, release, takeover, respond, detach, timeout} asserting the
  mutual-exclusion and no-deadlock properties. Cheap to write, catches the
  interleavings review misses.
- **Third-party impl test** (`tests/third_party_impl.rs`), per
  [P2](01-principles.md#p2--pluggable-extension-without-modification): a
  `Store` implemented from outside the crate using only the public API.

---

## 13. Open questions

1. **OPEN QUESTION — process survival across daemon restart.** §8 restores
   sessions as `Orphaned`. Reparenting PTYs to a small supervisor process (the
   `tmux` server model) would let processes survive an `omt` upgrade, which is a
   real user expectation. It is a substantial architectural addition (a second
   long-lived process, a handoff protocol, its own crash semantics). Deferred,
   but the `PtyHandle` abstraction in `omt-pty` should be designed so the handle
   *could* come from elsewhere. Needs a decision with the `omt-pty` change owner.

2. **OPEN QUESTION — sizing policy when the only viewer is a phone.** §2.2 takes
   the minimum over participants. If a laptop detaches and only an observer phone
   remains, the session keeps its last size — correct, but it means an agent may
   render for a width nobody can see. Alternative: shrink to a "headless"
   canonical size (e.g. 120×40). Needs data from the agent CLIs on how badly they
   handle a resize mid-run. Affects [06](06-agent-layer.md).

3. **OPEN QUESTION — writer token granularity for agents.** §5.3 has agents hold
   the token when injecting text. But an agent's *hook-channel* answers do not
   touch the PTY and are not gated. If a user is mid-command and an agent's
   deferred `PreToolUse` resolves, the agent may then write to the PTY as its own
   next action. Do we queue that behind the human's token, or does an
   agent-initiated write pre-empt? Lean: queue, with a visible indicator.
   Tracked jointly as [06 §10.10](06-agent-layer.md#10-open-questions) and
   [12 §9.5](12-collaboration.md#9-open-questions).

4. **OPEN QUESTION — history scope for worktrees.** §7 makes each worktree its
   own workspace, but users almost certainly want history shared across worktrees
   of the same repo (you run the same `cargo test` in both). Proposal: history
   queries default to scope `WorktreeGroup` rather than `Workspace`. Cheap to
   implement; needs a UX call.

5. **OPEN QUESTION — per-client vs per-workspace zoom.** §2.1 makes zoom
   workspace-level so collaborators see the same thing. On a phone, zoom is
   effectively mandatory (one pane fits), which would force a laptop into zoom
   too. Likely resolution: the phone uses a *client-local* view override rather
   than the shared zoom, and shared zoom stays for deliberate "look at this"
   moments. Needs the [08 — Web client](08-web-client.md) design to confirm.

6. **OPEN QUESTION — history redaction defaults.** §9's redaction list is a
   guess and false negatives are a security problem while false positives are an
   annoyance. Should the default be aggressive (redact anything matching
   `=[A-Za-z0-9_\-]{20,}`) or conservative? Needs a call in
   [13 — Security](13-security.md).
