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
[12 — Collaboration](12-collaboration.md) · [13 — Security](13-security.md) ·
[17 — Panes and layout](17-panes-and-layout.md) (the deepening of §2).

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
Pane                     a viewport onto a session, a leaf of a LayoutTree
LayoutView               one arrangement of panes; a Workspace owns one or more
Client                   an attached surface (TUI, phone, CLI) with presence
```

Two relationships deserve emphasis because they are where most multiplexers get
it wrong:

- **Pane → Session is many-to-one.** A session may be visible in several panes,
  in several layout views, on several clients, simultaneously. A pane is
  presentation only; closing a pane never kills a session.
- **Pane → LayoutView is one-to-one.** A pane belongs to exactly one view; two
  views showing the same session hold two different `PaneId`s pointing at one
  `SessionId` ([17 §3.3](17-panes-and-layout.md#33-decision-per-client-layout-views-one-shared-default)).
  This is why anything crossing client boundaries — presence, focus reporting,
  deep links — is keyed on `(ViewId, SessionId)` and never on a bare `PaneId`.
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
    /// Named arrangements over this workspace's sessions. Shape and semantics
    /// owned by [17 §3.3]; this crate stores them. Focus and zoom live *inside*
    /// a view (`Layout::focus`, `Layout::zoom`), not on the workspace.
    pub views: IndexMap<ViewId, LayoutView>,
    /// The view a client is put in when it attaches and asks for nothing.
    pub primary: ViewId,
    pub sessions: Vec<SessionId>,     // membership, ordered by creation
    pub created_at: Timestamp,
}

pub struct Session {
    pub id: SessionId,
    pub workspace: WorkspaceId,
    pub title: SessionTitle,          // explicit override, else OSC 2, else command
    pub kind: SessionKind,            // Shell | Command { argv } | Agent { kind }
    pub mode: SessionMode,            // D8; §1.3
    pub state: SessionState,
    pub surface: SessionSurface,      // §1.3 — replaces the old `term` + `pty` pair
    pub cwd: Option<PathBuf>,         // live, from OSC 7 / proc inspection
    pub agent: Option<AgentBinding>,  // see 06; a property, not a child object
    pub writer: WriterState,          // §5
    pub viewers: SmallVec<[ClientId; 4]>,
    pub seq: Seq,                     // monotonic per session; every event carries it
    pub created_at: Timestamp,
    pub exited_at: Option<Timestamp>,
    pub env: SessionEnv,              // what omt injected, for reproducible restore
}

/// D8. Chosen at creation, immutable for the session's life.
pub enum SessionMode { Pty, Native }

bitflags! {
    /// A set of `SessionMode`s. Used by adapters to declare which modes they
    /// can be driven in ([06 §7](06-agent-layer.md#7-adapters)). Named
    /// `SessionModeSet`, not `ModeSet` — that name belongs to the keymap's
    /// editing-mode flags ([16 §6.5](16-input-and-keymap.md#65-the-keymap-abstraction)).
    pub struct SessionModeSet: u8 {
        const PTY_ONLY = 0b01;
        const NATIVE   = 0b10;
    }
}

/// Makes "a native session has no PTY" unrepresentable rather than an unwrap site.
pub enum SessionSurface {
    /// `SessionMode::Pty` — the user's real CLI in a real PTY, observed.
    Pty { pty: PtyHandle, term: Terminal },
    /// `SessionMode::Native` — the agent spawned in ACP mode; omt renders the
    /// whole session from typed events. No PTY, no grid, no authoritative size.
    Native { conn: AcpConnection, transcript: Transcript },
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

`Session::mode()` is the discriminant accessor over `surface`, and the stored
`mode` field must always agree with the `SessionSurface` variant (§11).

### 1.2 Identity and lifetimes

| Type | Identity | Lifetime | Stable across |
|---|---|---|---|
| `InstanceId` | UUIDv7, generated once, persisted | forever | daemon restart, hostname change |
| `WorkspaceId` | derived: `blake3(canonical_root)[..16]` | as long as the path is open | daemon restart, rename |
| `SessionId` | UUIDv7 | until closed *and* evicted from history | detach, reattach, daemon restart |
| `PaneId` | UUIDv7 | until the pane is closed | client disconnect |
| `ViewId` | UUIDv7 | `Primary`/`Named`: with the workspace. `Adaptive`: until its last client detaches | `Adaptive` views are never persisted |
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

### 1.3 Session modes (D8)

Two modes, chosen at creation and immutable thereafter
([D8](decisions.md#d8--two-session-modes-pty-default-and-native-acp)):
`pty` — the user's real CLI in a real PTY, observed from outside — is the
default and the product premise; `native` — the agent spawned in ACP mode with
no TUI, omt rendering the whole session from typed events — is opt-in.

- A `native` session has **no PTY, no grid, and no authoritative size**. It never
  participates in size negotiation ([07 §4.3](07-remote-protocol.md#43-the-resize-problem));
  every client simply renders it at its own width.
- `SessionMode` is visible on every surface, and a `native` session is **always
  labelled as such** — the user must never be unsure which product they are
  talking to (D8).
- The tiered `EventSource` model of [06 §3](06-agent-layer.md) applies to `pty`
  sessions only. In `native` mode the ACP connection is the sole source, so there
  is nothing to merge and nothing to degrade.
- **Closing** a native session closes the JSON-RPC connection and then terminates
  the adapter process, rather than SIGHUP-ing a PTY (§1.2 lifetime rule 2 is the
  `pty` form).
- History (§9) and blocks come from block closure, so they exist **only for `pty`
  sessions**. A native session's timeline is reconstructed from its typed events
  ([20 — Recall and timeline](20-recall-and-usage.md)).

### 1.4 The types §1.1 names

§1.1's structs reference nine types that this document uses and no document
defined. They are defined here, in this crate, because this crate is where they
live. `Transcript` gets the most room because it is the largest of them by far.

#### 1.4.1 `Transcript` — the entire content of a `native` session

For a `pty` session the content is the grid plus scrollback plus blocks, all
owned by [04](04-terminal-core.md). A `native` session has none of those: per
§1.3 it has no PTY and no grid, so **the transcript is not a view of the session,
it is the session.** It is what §8.1 persists as an append-only log, what
[08 §4.4](08-web-client.md#44-transcript-view) renders, what
[20](20-recall-and-usage.md) indexes for search and recall, and what
[21](21-data-lifecycle.md) must retain, redact and purge. Losing it loses
everything the user did.

It is also the source for the transcript **view** of a `pty` agent session
([D14](decisions.md#d14--agent-sessions-get-a-transcript-surface-blocks-are-for-shell-work)),
where it is populated from the merged agent event stream instead of from an ACP
connection. One type, two producers.

```rust
pub struct Transcript {
    pub session: SessionId,
    /// Append-only, ordered by `seq`. Never mutated in place — a correction is
    /// a new entry that supersedes an earlier one by `supersedes`.
    entries: Vec<TranscriptEntry>,
    /// Byte budget for the in-memory tail. Older entries are evicted from
    /// memory only; the on-disk log is authoritative and 21 §3 owns its
    /// retention. Default 8 MiB, matching §8.2's per-session scrollback cap.
    resident_budget: usize,
    /// First entry still resident. Everything before it is on disk only, and a
    /// client scrolling past it pages from the store rather than getting a hole.
    resident_from: Seq,
}

pub struct TranscriptEntry {
    pub seq: Seq,                     // the session's sequence space (§1.1)
    pub ts: Timestamp,
    /// Which observation tier produced this, so a surface can render
    /// confidence honestly ([06 §3](06-agent-layer.md)). `Protocol` for every
    /// entry in a `native` session.
    pub source: EventSourceTier,
    /// Set when a later entry replaces this one — a streamed message that was
    /// finalized, a tool call that gained its result. Renderers show the newest.
    pub supersedes: Option<Seq>,
    pub body: TranscriptBody,
}

pub enum TranscriptBody {
    /// A user turn, whoever originated it. `actor` is 12 §1's, so "who said
    /// this from which device" survives into recall.
    UserMessage { actor: Actor, content: Vec<ContentBlock> },
    /// An assistant turn. `streaming: true` while it is still being appended.
    AssistantMessage { content: Vec<ContentBlock>, streaming: bool },
    /// Agent-internal reasoning, where the agent exposes it. Rendered collapsed
    /// by default and **excluded from recall indexing by default** (20).
    Thinking { text: String },
    ToolCall { id: ToolCallId, name: String, input: serde_json::Value },
    ToolResult { id: ToolCallId, outcome: ToolOutcome, content: Vec<ContentBlock> },
    /// The interaction is owned by the ledger (06 §5); the transcript carries a
    /// reference and its terminal state, so replay shows what was asked and
    /// what was decided without duplicating the ledger.
    Interaction { id: InteractionId, terminal_state: Option<InteractionState> },
    /// Mode changes, model changes, context-window events, agent errors.
    Notice { kind: NoticeKind, text: String },
}

pub enum ContentBlock {
    Text { text: String },
    Markdown { text: String },
    /// Content-addressed; the bytes live in the store, not in the transcript.
    Image { blob: BlobId, mime: String },
    /// A fenced snippet the surface may render with highlighting, and 18 may
    /// offer semantic actions on.
    Code { lang: Option<String>, text: String },
    Diff { diff: FileDiff },          // 15 §3.2's type, one renderer everywhere
}
```

Properties that are load-bearing:

- **Append-only and `seq`-ordered.** The transcript shares the session's
  sequence space, so a client resumes it with `since_seq` exactly as it resumes
  terminal bytes ([12 §5.1](12-collaboration.md#51-guarantees) G1, G4), and the
  same replay window covers both.
- **Superseding rather than mutation** is what makes streaming safe to persist:
  a partial assistant message is a durable entry, and the finalized one
  supersedes it. A crash mid-stream leaves a truthful partial record rather than
  a hole.
- **Redaction happens on write**, through
  [21 §2](21-data-lifecycle.md#2-redaction-before-write)'s single choke point,
  like every other persisted stream. The transcript carries model output and
  tool inputs, so it is one of the highest-risk streams for secrets.
- **Purge is per entry, by `seq` range**, so
  [21](21-data-lifecycle.md)'s "delete everything about session X before date
  Y" is expressible without rewriting the log's framing.
- **Blobs are referenced, never inlined.** A pasted screenshot must not make one
  transcript entry megabytes wide.

#### 1.4.2 The rest, briefly

```rust
/// A PTY master, owned by `omt-pty`. Deliberately an abstraction rather than a
/// raw fd: §13.1's process-survival question needs a handle that could come
/// from a supervisor rather than from this process, and D10 keeps a Windows
/// seam open at no cost. Unix-only in v1 (D10).
pub struct PtyHandle {
    pub child: ChildHandle,           // pid, pgid, wait/kill
    read: PtyReadHalf,
    /// Private to the session's `InputGate` — the serialization point of
    /// [12 §3.5](12-collaboration.md#35-the-serialization-point). Nothing else
    /// in the daemon holds a writable handle.
    write: PtyWriteHalf,
    pub size: GridSize,               // the authoritative size; TIOCSWINSZ target
}

/// A live JSON-RPC connection to an agent spawned in ACP mode (D8). Owns the
/// framing, the request-id space, the negotiated protocol version and the
/// agent's declared capabilities; surfaces incoming notifications as typed
/// events for `omt-agent` to normalize.
pub struct AcpConnection {
    pub protocol_version: AcpVersion,     // v1 is the build target; v2 negotiated
    pub agent_caps: AcpCapabilities,
    pub acp_session: AcpSessionId,        // the agent's own id, not omt's
    pub state: ConnectionState,           // Handshaking | Ready | Closing | Closed { reason }
}

/// Per-*pane* view state. Never shared: two panes on one session scroll
/// independently (§4), so nothing here belongs on `Session`.
pub struct PaneView {
    pub scroll: ScrollPosition,       // Follow (pinned to live) | At(Position)
    pub selection: Option<Selection>,
    pub search: Option<SearchCursor>,
    pub folded_blocks: HashSet<BlockId>,
    pub mode: ViewMode,               // Grid | Blocks | Transcript
}

/// Exactly what omt injected into the child's environment, recorded so a
/// restart (§8.2) is reproducible and so a user can see what omt did to their
/// shell. Not the child's full environment — only omt's additions and removals.
pub struct SessionEnv {
    pub injected: BTreeMap<String, String>,   // OMT_SESSION, TERM, integration hooks
    pub removed: BTreeSet<String>,
    /// The inherited environment is captured as a hash, not a copy: it contains
    /// the user's secrets and must never reach the store (13 §8).
    pub inherited_digest: Blake3Hash,
}

/// Per-instance resource ceilings. Enforced at creation; exceeding one is a
/// loud `resource_exhausted`, never a silent degradation.
pub struct InstanceLimits {
    pub max_sessions: usize,              // default 128
    pub max_sessions_per_workspace: usize,// default 32
    pub max_panes_per_view: usize,        // default 64
    pub max_clients: usize,               // default 32
    /// Total scrollback + transcript bytes resident across all sessions.
    /// 21 §3 owns the on-disk budget; this bounds memory.
    pub max_resident_bytes: usize,        // default 512 MiB
}

/// Allocates the per-session `Seq` values every event carries. Monotonic,
/// never reused, and **persisted with the session** so it does not restart at
/// zero after a daemon restart (§11 invariant 6). One generator per session;
/// `Instance::seq` is the instance-wide allocator used by the audit log
/// (12 §8), which is a different space.
pub struct SeqGenerator { next: u64 }

/// The instance-wide command history of §9, backed by `omt-store`'s SQLite.
/// Owns the FTS index, the per-scope query paths and the `HistoryAppended`
/// emission; §9 defines `HistoryEntry` and `HistoryQuery`.
pub struct HistoryStore { /* store handle + prepared statements */ }

/// What a client can render, reported at attach and used to choose an encoding
/// rather than to grant permission — a client that cannot render images is
/// sent a placeholder, never denied the session (D2).
pub struct ClientCapabilities {
    pub images: ImageSupport,         // None | Sixel | KittyGraphics | WebImg
    pub mouse: bool,
    pub kitty_keyboard: bool,         // affects key encoding (16)
    pub true_color: bool,
    pub bracketed_paste: bool,
    pub clipboard: ClipboardSupport,  // None | Osc52 | Native
    pub max_frame_bytes: usize,       // caps a snapshot the client must decode
}
```

---

## 2. Layout: the BSP tree

A workspace's tiling arrangement is an **n-ary split tree** (historically, and in
this heading, "BSP"). **[17 — Panes and layout](17-panes-and-layout.md) owns
these types**; the sketch below is the shape this crate stores, and where the two
disagree, 17 wins and this section is the bug.

A tiling tree does not stand alone: it is one layer of a `Layout`, which a
`LayoutView` owns, of which a workspace has one or more (§1.1,
[17 §3.3](17-panes-and-layout.md#33-decision-per-client-layout-views-one-shared-default)).

```rust
/// The tiling layer. Geometry is never stored here.
pub enum LayoutTree {
    /// The view has no panes. Only ever the root.
    Empty,
    Leaf(PaneId),
    Split(Split),
}

pub struct Split {
    pub id: SplitId,
    /// Column-wise (children side by side) or row-wise (children stacked).
    /// Named for what is divided: "horizontal split" is ambiguous in every
    /// multiplexer's documentation ([17 §1.2](17-panes-and-layout.md#12-types)).
    pub axis: Axis,
    /// Children with fractional weights summing to 1.0. N-ary, not binary —
    /// see rationale below.
    pub children: Vec<Child>,
}

pub enum Axis { Columns, Rows }

/// The whole layout of one view: tiling plus the layers that never tile.
pub struct Layout {
    pub tiles: LayoutTree,
    pub floats: Vec<FloatingPane>,
    /// Non-destructive zoom, a flag rather than a tree swap. Per *view*.
    pub zoom: Option<PaneId>,
    pub stacks: HashMap<StackId, Stack>,
    pub focus: Option<PaneId>,
    pub last_focus: Option<PaneId>,
}
```

**Zoom is a flag beside the tree, not a variant in it.** Carrying zoom as a tree
node holding a saved layout — tmux's design — forces an unzoom-resize-rezoom on
every container resize. A flag consulted by `compute`
serializes trivially and — the reason that decides it — lets one client render
zoomed while another renders the tiling from one tree
([17 §1.3](17-panes-and-layout.md#13-the-whole-layout-of-a-workspace),
[17 §5.1](17-panes-and-layout.md#51-zoom)).

**Decision: an n-ary tree, not a strictly binary one.** A binary tree makes
"three equal columns" representable only as nested splits, which then behave
wrongly when the middle one is closed (the remaining two get 1/2 and 1/2 of an
oddly-shaped region rather than 1/2 and 1/2 of the whole). The n-ary form makes
even-split, balance, and close-and-redistribute all trivial and total. Splits
with the same axis as their parent are automatically flattened into it,
which keeps the tree canonical: **no `Split` ever has a child `Split` of the same
axis**, and that invariant is asserted after every operation.

### 2.1 Operations

Every operation acts on one view's `Layout`. The full signatures, the
normalization rules and the remainder rule live in
[17 §1.4](17-panes-and-layout.md#14-normalization) and
[17 §2](17-panes-and-layout.md#2-geometry-and-resize).

```rust
impl Layout {
    pub fn split(&mut self, at: PaneId, dir: Direction2D, new: PaneId, ratio: f32)
        -> Result<(), LayoutError>;
    pub fn close(&mut self, pane: PaneId) -> Result<Option<PaneId>, LayoutError>; // returns new focus
    pub fn resize(&mut self, target: ResizeTarget, amount: ResizeAmount) -> Result<(), LayoutError>;
    pub fn swap(&mut self, a: PaneId, b: PaneId) -> Result<(), LayoutError>;
    pub fn move_pane(&mut self, pane: PaneId, to: PaneId, dir: Direction2D) -> Result<(), LayoutError>;
    /// Sets or clears `self.zoom`. The tree is untouched.
    pub fn set_zoom(&mut self, pane: Option<PaneId>) -> Result<(), LayoutError>;
    pub fn navigate(&self, from: PaneId, dir: Direction2D) -> Option<PaneId>;
    pub fn apply_preset(&mut self, preset: LayoutPreset);   // Even | MainVertical | MainHorizontal | Tiled
    /// Geometry for a given viewport, honouring the renderer's constraints.
    pub fn compute(&self, area: Rect, c: Constraints) -> Geometry;
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
- **Zoom is non-destructive and per *view*** ([17 §5.1](17-panes-and-layout.md#51-zoom)).
  Two clients sharing the `Primary` view share its zoom; a phone in its own
  `Adaptive` view — which is effectively always zoomed — does not force zoom on
  anyone else. A deliberate "everyone look at this" is `layout.promote` after
  zooming, or `pane.zoom { view: <primary> }` explicitly.

### 2.2 Sizing with multiple clients

A session's PTY has exactly one size. When N clients view the same workspace at
different terminal sizes, we must choose one.

**The negotiation rule is specified once, in
[07 §4.3](07-remote-protocol.md#43-the-resize-problem), which owns it**, and the
**vocabulary is [17 §3.4](17-panes-and-layout.md#34-the-pty-size-question-which-per-client-layout-does-not-solve)'s**,
which supersedes 07's earlier `SizeOwner` naming. This document uses 17's names
throughout and never the old ones:

| 17 §3.4 — use this | superseded name |
|---|---|
| `SizePolicy::Driver` (default) | `SizeOwner::Writer` |
| `SizePolicy::Smallest` | `SizeOwner::Smallest` |
| `SizePolicy::Pinned { size, by }` | `SizeOwner::Pinned { by }` |
| `Participation::{Participant, Observer}` | *(no earlier name)* |

So: each session has one authoritative `(cols, rows)` and a `SizePolicy` that is
`Driver` (the writer-token holder's view drives the PTY, falling back to the
smallest `Participant` viewport when nobody holds the token), `Pinned`, or
`Smallest` (opt-in). This crate stores the policy on the session, applies the
resulting size to the PTY, and reports every client's viewport for presence.
A client in block view is always an `Observer`. 07 §4.3 remains the authority
for the negotiation itself; 17 §3.4 for the names.

A `Native` session ([§1.3](#13-session-modes-d8)) has no authoritative size at
all and is excluded from this negotiation entirely.

What follows from that here:

- Every client reports its viewport; a non-authoritative client's report updates
  presence and nothing else. Clients whose viewport differs from the
  authoritative size render scaled-to-fit-width and letterboxed, never cropped.
- `SizePolicy::Smallest` is the "nobody is cropped" pairing mode, and is the only
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
    /// Per *view*, not per workspace: two views of one workspace focus
    /// independently. Stored as `Layout::focus`; this map is the index over it.
    pub focused_pane: HashMap<ViewId, PaneId>,
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
    /// The `LayoutView` this client is rendering, chosen by
    /// [17 §3.3](17-panes-and-layout.md#33-decision-per-client-layout-views-one-shared-default)'s
    /// `choose_view`, or pinned explicitly by `layout.views.select`.
    pub view: ViewId,
    pub pinned_view: Option<ViewId>,
    /// What is on screen right now. **`(ViewId, SessionId)`, not `PaneId`**: a
    /// `PaneId` belongs to one view and is meaningless to a client in another,
    /// so a bare `PaneId` cannot express "the phone is watching this" to a
    /// laptop. The `SessionId` is the shared identity and is what every surface
    /// actually highlights.
    pub viewing: SmallVec<[(ViewId, SessionId); 4]>,
    /// Reported viewport. Authoritative only per the session's `SizePolicy`
    /// (17 §3.4), negotiated per 07 §4.3.
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
If the seq is too old, the reply is `Resync`
([07 §5.2](07-remote-protocol.md#52-replay-window)) plus a fresh snapshot.

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

> **At most one actor may write to a session's *input channel* at any moment.
> Holding the writer token is a visible, explicit, revocable state, and every
> client always knows who holds it.**

In a `pty` session the input channel is the PTY. In `native` mode ([§1.3](#13-session-modes-d8))
the only input path is `agent.prompt`, so the token gates that instead of the
PTY-write capabilities. [12 §3.1](12-collaboration.md#31-what-it-governs) owns
what the token governs.

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
session.writer.acquire   { session, reason?, force: bool, keep_size: bool }
                         -> Acquired { epoch } | Conflict { holder }
session.writer.release   { session }                  -> Ack
session.writer.keep      { session }                  -> Ack   // holder cancels a takeover
session.writer.status    { session }                  -> WriterStatus
```

Takeover is `acquire { force: true }`, not a separate capability: it opens a
`PendingTakeover` with a 5 s grace window that the holder may cancel once with
`writer.keep`. There is no queue of pending claims and no `TakeoverPolicy` enum —
a takeover storm resolves to the most recent requester (12 §6 C5).

Consequences for the data model:

- **`auto_acquire`.** The field lives here; its semantics — when it applies, what
  epoch it produces, and how it is audited — are
  [12 §3.3](12-collaboration.md#33-lifecycle)'s first row.
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
  rather than mysterious characters, and can take over. **This is the current
  lean, not a settled decision**: writer-token semantics for `ActorKind::Agent`
  are [12 §9](12-collaboration.md#9-open-questions)'s open question 5 and are
  waiting on a real multi-agent use case. If 12 settles differently, 12 wins.
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

- `interaction.resolve` **only when the interaction's
  [`deliverable`](06-agent-layer.md#521-deliverable--a-field-of-interaction-computed-by-the-normalizer)
  is `Native`** — an ACP `session/request_permission` reply, an opencode-plugin
  response, a Codex app-server call. Those do not touch the PTY.
  Where `deliverable` is `Synthetic`, the resolve is a PTY write and is a
  **gated transaction** that acquires the token, verifies input quiescence
  (`injection_quiescence`, default 750 ms —
  [12 §3.3.1](12-collaboration.md#331-lease-parameters)) and re-verifies the
  interaction before writing
  ([D13](decisions.md#d13--synthetic-delivery-is-a-gated-transaction-never-a-bare-write),
  [12 §3.1](12-collaboration.md#31-what-it-governs)). Data-model consequences
  for this crate:
  - The transaction runs inside the session's **`InputGate`**
    ([12 §3.5](12-collaboration.md#35-the-serialization-point)), which owns
    `PtyHandle::write` exclusively (§1.4.2). The local TUI's passthrough goes
    through the same gate — that is what makes the two writers orderable.
  - Re-verification is the six preconditions of
    [12 §4.6](12-collaboration.md#46-preconditions-on-a-synthetic-delivery), and
    they are **screen-derived**: the card's presence, the printed row number at
    the computed index, the card not being in text-entry mode, an option list of
    ≤ 9, the pane **not being on the alternate screen**
    ([04 §6.4](04-terminal-core.md#64-the-fallback-heuristic--no-shell-integration)),
    and no imminent agent-side AFK auto-advance. So the gate reads `omt-term`'s
    freshest snapshot, not the hook payload, and a `Native` session — having no
    grid — never takes this path.
  - The write itself is raw bytes **outside bracketed paste**, one key per
    `write(2)`. A `Synthetic` resolve therefore cannot be expressed as a
    `session.send_text` call, which bracket-wraps; it is a distinct gate entry
    point.
  - `WriterState` is unchanged by all of this: the transaction acquires and
    releases the ordinary token, so a human always sees who is writing.

  The old blanket exemption
  rested on "it goes back through the agent's own channel", which
  [D11](decisions.md#d11--omt-mirrors-the-agents-own-card-it-does-not-intercept-or-replace-it)
  deleted.
- `agent.prompt` where the adapter has a structured submit path **in a `pty`
  session**. Where the only path is synthesized keystrokes, the token **is**
  required. In a `native` session `agent.prompt` is the *only* input path and is
  always token-gated (§5.1); 12 §3.1 states this normatively.
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

`Presence::viewing` carries `(ViewId, SessionId)` pairs, mirroring
`ClientView::viewing` (§4) — never bare `PaneId`s, which do not survive the
crossing from one view to another
([17 §3.3](17-panes-and-layout.md#33-decision-per-client-layout-views-one-shared-default)).

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
| Instance/workspace/session/pane tree, `Primary` and `Named` views (tiles, floats, stacks, zoom, focus) | **Snapshot** on every structural change, debounced 500 ms | Small, changes rarely, must be exactly right |
| `Adaptive` views | **never persisted** | Derived from a client's viewport; recreated by `choose_view` on the next attach ([17 §3.3](17-panes-and-layout.md#33-decision-per-client-layout-views-one-shared-default)) |
| Session env, argv, cwd at spawn | Snapshot at creation, immutable | Needed to offer a faithful re-spawn |
| Block metadata (command, exit, cwd, git, timing, attribution) | **Append-only log**, one record per block close | The valuable, permanent artifact; also feeds history (§9) |
| Scrollback | **Chunk snapshot**, delta by chunk generation | Per [04 §2.4](04-terminal-core.md#24-scrollback-blocks-of-logical-lines) each chunk has a generation; only changed chunks are written. Written every `scrollback_flush` (default 10 s) and on clean shutdown |
| Live grid (viewport) | Snapshot with the scrollback flush | Small and bounded; makes restore show the last screen |
| Raw PTY byte ring (`pty` sessions only) | **Not persisted** | Bounded, ephemeral, only used for live client resume |
| Native session transcript (`native` sessions only) | **Append-only log**, like block metadata | It *is* the session — there is no scrollback to fall back on (§1.3) |
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

- **The durability policy is [21 §6](21-data-lifecycle.md#6-crash-consistency)'s**
  and is not restated here: the `Record` frame (length prefix + CRC), torn-tail
  truncation, temp-file-plus-`rename` for snapshots, the three fsync classes, and
  the repair path when this enum returns `Partial`. What follows is only what is
  specific to the session tree.
- A restore never *silently* drops data. `Partial` surfaces as a warning event
  and a banner in every surface.
- Restored sessions come back as `SessionState::Orphaned`. Their content is
  readable, searchable and copyable; writes return `precondition_failed`. The UI
  offers **"restart"** — re-spawn the same argv, in the same cwd, with the same
  injected env, into the same pane, keeping the old scrollback above a
  separator. This is materially better than either silently killing the session
  or pretending it is alive.
- **Retention is [21 §3](21-data-lifecycle.md#3-retention-and-compaction)'s**,
  including the per-instance cap and the budget arithmetic behind it. The only
  number this document needs inline is the per-session one, because it bounds a
  single session's restore: scrollback snapshots are capped per session at
  `[store.retention] scrollback_max_bytes_per_session`, default 8 MiB, evicting
  oldest-first.

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
- **Redaction** — including what is redacted, in which streams, with which rules
  and with which marker — is
  [21 §2](21-data-lifecycle.md#2-redaction-before-write)'s, and it covers the
  block's persisted *output* as well as its command text. History is written
  through the same choke point as everything else.

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
| `workspace.focus` | O | `{ workspace, pane, view? }` | `{ view: ViewId, focused: PaneId }` |
| `workspace.worktree.add` | O | `{ workspace, path, branch, create_branch }` | `{ workspace: WorkspaceId }` — effects: `WRITES_FS`, `SPAWNS_PROCESS` |
| `workspace.history` | V | `HistoryQuery` (scope forced to this workspace) | `{ entries, next_cursor }` |

### 10.2 `session.*`

| Capability | Role | Input | Output |
|---|---|---|---|
| `session.list` | V | `{ workspace?, include_exited: bool }` | `{ sessions: [SessionInfo] }` |
| `session.get` | V | `{ session }` | `SessionInfo` |
| `session.create` | O | `{ workspace, kind, mode: SessionMode = Pty, cwd?, env?, size?, pane_target? }` | `{ session, pane }` — effects: `SPAWNS_PROCESS`; `mode` per [§1.3](#13-session-modes-d8) |
| `session.close` | O | `{ session, force: bool }` | `{ status }` — effects: `DESTRUCTIVE` |
| `session.restart` | O | `{ session }` | `{ session }` — effects: `SPAWNS_PROCESS`, `DESTRUCTIVE` |
| `session.rename` | O | `{ session, title }` | `SessionInfo` |
| `session.attach` | V | `{ session, mode, since_seq?, viewport }` | `Attached { snapshot, seq } \| ResyncRequired { snapshot, seq }` — `viewport` is a report, not a claim; see [07 §4.3](07-remote-protocol.md#43-the-resize-problem) |
| `session.detach` | V | `{ session }` | `Ack` |
| `session.resize` | O | `{ session, cols, rows, pixel_width?, pixel_height?, request_authoritative }` | `{ authoritative: GridSize, policy: SizePolicy }` — per [07 §4.3](07-remote-protocol.md#43-the-resize-problem); requires the writer token only when `request_authoritative` |
| `session.send_text` | O | `{ session, text, submit }` | `{ seq }` — effects: `WRITES_PTY`; requires writer token |
| `session.send_keys` | O | `{ session, keys: [KeySpec] }` | `{ seq }` — effects: `WRITES_PTY`; requires writer token |
| `session.write_bytes` | O | `{ session, bytes }` | `{ seq }` — effects: `WRITES_PTY`; requires writer token |
| `session.signal` | O | `{ session, signal }` | `Ack` — effects: `DESTRUCTIVE` |
| `session.scrollback.get` | V | `{ session, from: Position?, lines, mode }` | `{ lines: [StyledLine], from, to }` |
| `session.search` | V | `{ session, query, cursor? }` | `{ matches, cursor, exhausted }` |
| `session.blocks.list` | V | `{ session, before?, limit, filter? }` | `{ blocks: [BlockInfo], next_cursor }` |
| `session.blocks.get` | V | `{ session, block, include_output, max_lines }` | `{ block, output: [StyledLine], truncated }` |
| `session.blocks.rerun` | O | `{ session, block, target_session? }` | `{ seq }` — effects: `WRITES_PTY`, `DESTRUCTIVE` |
| `session.blocks.fold` | V | `{ session, block, folded }` | `Ack` |
| `session.writer.acquire` | O | `{ session, reason?, force, keep_size }` | `Acquired { epoch } \| Conflict { holder }` |
| `session.writer.release` | O | `{ session }` | `Ack` |
| `session.writer.keep` | O | `{ session }` | `Ack` — holder cancels a pending takeover |
| `session.writer.status` | V | `{ session }` | `WriterStatus` |
| `session.history` | V | `HistoryQuery` | `{ entries, next_cursor }` |

`session.send_text`, `session.send_keys`, `session.write_bytes`,
`session.resize` and `session.signal` return `unsupported` on a `Native`
session — there is no PTY to write to, size, or signal ([§1.3](#13-session-modes-d8)).

`session.blocks.rerun` is marked `DESTRUCTIVE` deliberately: re-running an
arbitrary previous command with one tap must require a confirm gesture, per
[03 §2](03-capability-catalog.md#2-declaring-a-capability). Note that this is a
property of **omt's own** capability, applied identically on the TUI, the CLI and
the phone — not a remote-only gate
([D2](decisions.md#d2--remote-is-exactly-equivalent-to-local)) and not a
judgement about the agent's tools
([D1](decisions.md#d1--omt-adds-no-policy-layer-over-an-agents-permission-semantics)).

### 10.3 `pane.*`

`pane.*` and `layout.*` are owned by [17 §9](17-panes-and-layout.md#9-capabilities),
including their input and output shapes; this document does not restate them.

The names this document relies on: `pane.list`, `pane.split`, `pane.close`,
`pane.focus`, `pane.navigate`, `pane.move`, `pane.swap`, `pane.resize`,
`pane.zoom`, `pane.set_session`, `pane.scroll`, `pane.select`, `layout.get`,
`layout.set`, `layout.preset`, `layout.views.*`, `layout.promote`.

`pane.navigate` is the one name for directional focus movement
([17 §9.1](17-panes-and-layout.md#91-pane)).

**Every layout capability carries an optional `view: Option<ViewId>`**, defaulting
to the caller's current view, because a `PaneId` alone does not say which
arrangement it belongs to and an operation issued from a phone must not silently
mutate the laptop's `Primary` view. Pushing a change across views is the explicit
`layout.promote` / `layout.adopt` pair
([17 §3.6](17-panes-and-layout.md#36-cross-view-operations)).

### 10.4 Events emitted

Per [03 §4](03-capability-catalog.md#4-events-are-the-read-side-twin), events are
derived from state changes, never published by hand from handlers.

| Event | When |
|---|---|
| `WorkspaceOpened` / `WorkspaceClosed` / `WorkspaceRenamed` | tree change |
| `LayoutChanged { view, .. }` | any layout mutation, with the new tree and a geometry hint |
| `ViewCreated` / `ViewClosed` / `ViewSelected` | [17 §3.3](17-panes-and-layout.md#33-decision-per-client-layout-views-one-shared-default) |
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
   every pane is in exactly one `LayoutView`.
2. Every `Pane::session` names a live entry in `Instance::sessions`.
3. No `Split` has a child `Split` of the same axis (canonical tree, §2).
4. Split child weights sum to 1.0 ± 1e-4, and every weight is > 0.
5. At most one `WriterToken` holder per session, and its `epoch` never decreases.
6. `Session::seq` is strictly monotonic and never decreases across restore.
7. A session in `Exited` or `Orphaned` has no writer and refuses PTY writes.
8. `Workspace::sessions` and `Session::workspace` agree in both directions.
9. Every `ClientId` in `Session::viewers` exists in `Instance::clients`.
10. For a session with `mode == Pty`, the authoritative PTY size equals what its
    `SizePolicy` resolves to
    ([17 §3.4](17-panes-and-layout.md#34-the-pty-size-question-which-per-client-layout-does-not-solve)
    for the names, [07 §4.3](07-remote-protocol.md#43-the-resize-problem) for the
    negotiation): under `Driver`, the writer's viewport, falling back to the
    minimum over `Participation::Participant` viewports when nobody holds the
    token; under `Pinned`, the pinned size; under `Smallest`, that same minimum
    over `Participant`s; and the last authoritative size when no viewer remains.
11. A `Native` session has no `PtyHandle`, no authoritative size, and emits no
    `SessionResized`. Its `mode` field and its `SessionSurface` variant agree.
12. Every workspace has exactly one view of kind `Primary`, and
    `Workspace::primary` names it. `layout.views.close` refuses it.
13. `Layout::zoom`, when set, names a pane present in that same view's `tiles`,
    `floats` or `stacks`. Zoom in one view never constrains another.
14. Every `ClientView::view` names a live view of `ClientView::workspace`, and
    that view's `clients` list contains the client — the two directions agree.

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

2. **OPEN QUESTION — sizing policy when the only viewer is a phone.** §2.2's
   default is `SizePolicy::Driver`, with the minimum over `Participant`s as the
   no-writer fallback. If a laptop
   detaches and only an observer phone
   remains, the session keeps its last size — correct, but it means an agent may
   render for a width nobody can see. Alternative: shrink to a "headless"
   canonical size (e.g. 120×40). Needs data from the agent CLIs on how badly they
   handle a resize mid-run. Affects [06](06-agent-layer.md).

3. **OPEN QUESTION — writer token granularity for agents.** §5.3 has agents hold
   the token when injecting text as `ActorKind::Agent`. The question is what
   happens when an agent-initiated write meets a human who is mid-command: do we
   queue it behind the human's token, or does it pre-empt? Lean: queue, with a
   visible indicator. Tracked jointly as
   [06 §10.10](06-agent-layer.md#10-open-questions) and
   [12 §9.5](12-collaboration.md#9-open-questions).

   > **Rewritten.** The original form of this question rested on *"an agent's
   > hook-channel answers do not touch the PTY and are not gated"*, and on a
   > deferred `PreToolUse` resolving.
   > [D11](decisions.md#d11--omt-mirrors-the-agents-own-card-it-does-not-intercept-or-replace-it)
   > deleted both: omt does not defer, and the hook is an observation channel
   > with no response slot. A `Synthetic` resolution now *is* a PTY write and is
   > gated as a transaction (§5.4,
   > [12 §3.1](12-collaboration.md#31-what-it-governs)); a `Native` one goes over
   > the agent's own RPC and is ungated. Neither is an ungated PTY write, so the
   > premise is gone. What survives is the narrower question above, which is
   > about `ActorKind::Agent` as a *writer*, not about interaction delivery.

4. **OPEN QUESTION — history scope for worktrees.** §7 makes each worktree its
   own workspace, but users almost certainly want history shared across worktrees
   of the same repo (you run the same `cargo test` in both). Proposal: history
   queries default to scope `WorktreeGroup` rather than `Workspace`. Cheap to
   implement; needs a UX call.

5. **CLOSED — per-client vs per-workspace zoom.** Resolved by
   [17 §5.1](17-panes-and-layout.md#51-zoom) in the way this question
   anticipated: zoom is **per `LayoutView`**, which is neither per-workspace nor
   per-client. Two clients sharing the `Primary` view share its zoom; a phone in
   its own `Adaptive` view is effectively always zoomed and forces nothing on
   anyone. Deliberate "everyone look at this" is `layout.promote` after zooming.
   §2.1 states the adopted rule.

6. **OPEN QUESTION — history redaction defaults.** §9's redaction list is a
   guess and false negatives are a security problem while false positives are an
   annoyance. Should the default be aggressive (redact anything matching
   `=[A-Za-z0-9_\-]{20,}`) or conservative? Needs a call in
   [13 — Security](13-security.md).
