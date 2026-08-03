# Panes and Layout — `omt-session`

How a workspace's panes are arranged, resized, navigated, degraded onto a phone,
serialized and restored. This deepens the sketch in
[05 — Session model §2](05-session-model.md#2-layout-the-bsp-tree); where the two
disagree, the contradictions are enumerated in §12 rather than silently
resolved.

Lives in `omt-session` (L3, [02 — Crate map](02-crate-map.md)) as a
self-contained `layout` module with no PTY, transport or UI knowledge: a pure
function library plus a state machine, so it is property-testable without a
runtime.

Related: [01 — Principles](01-principles.md) (P3 parity, P6 collaboration) ·
[03 — Capability catalog](03-capability-catalog.md) ·
[04 — Terminal core](04-terminal-core.md) (reflow) ·
[06 — Agent layer](06-agent-layer.md) (status badges) ·
[07 — Remote protocol](07-remote-protocol.md) (viewport negotiation) ·
[08 — Web client](08-web-client.md) (block view, mobile) ·
[10 — Configuration](10-configuration.md) (launch configs) ·
[15 — Workspace explorer](15-workspace-explorer.md) (overlay panel) ·
[16 — Input and keymap](16-input-and-keymap.md) · prior art in
[research/multiplexers.md](../research/multiplexers.md).

---

## 1. The layout model

### 1.1 The three candidate models, and the decision

| Model | Representative | Why not |
|---|---|---|
| **Absolute-cell tree** | tmux ([research §1.1](../research/multiplexers.md#11-tmux--an-n-ary-cell-tree-carrying-absolute-geometry)) | Geometry *is* the state, so every container resize is a proportional-rebuild pass (`layout_new_pane_size`) that fakes ratios badly, and the layout is permitted to exceed the window — which is where tmux's cropped-window experience comes from. Unusable when the container size differs per client, which is omt's normal case. |
| **Flat set + constraint solver** | zellij ([research §1.2](../research/multiplexers.md#12-zellij--no-tree-at-all-a-flat-pane-set-plus-a-constraint-solver)) | Structure is *rediscovered* from coordinates on every resize. That makes the tree invariants unstateable, which forfeits [05 §12](05-session-model.md#12-testing)'s property-test strategy, and it needs a Cassowary dependency whose failure mode zellij itself works around with an `is_layout_valid` "abandon ship" hack. |
| **N-ary ratio tree** | another terminal's `PaneBranch`, and [05 §2](05-session-model.md#2-layout-the-bsp-tree) | — |

**Decision: an n-ary ratio tree, geometry always derived, never stored.**

This confirms doc 05's choice, for three reasons that the research made concrete:

1. **Ratios are size-independent, and omt renders the same layout at several
   sizes at once.** another tool's `panes(area) -> Vec<PaneInfo>`
   ([research §1.3](../research/multiplexers.md#13-another tool--a-strict-binary-bsp-tree-with-f32-ratios))
   proves the property we need: geometry is a pure function of `(tree, area)`, so
   the *same tree* can be laid out for a laptop and a phone in the same tick with
   no state duplication. An absolute-cell tree cannot do this at all.
2. **N-ary makes close-and-redistribute total.** Binary trees make three equal
   columns `Split(a, Split(b, c))`; closing `b` leaves 1/3 : 2/3, which is
   another tool's actual behaviour and is wrong. tmux avoids it by being n-ary, and
   another terminal — a GUI terminal with no tmux lineage — independently chose n-ary flex.
   Three implementations converging is enough evidence.
3. **The invariants are stateable**, so §11's property tests can be written.

**Rejected: binary.** The only argument for it is that "BSP" is the familiar
word. [05 §1](05-session-model.md#1-the-object-model) already says "BSP tree"
while defining an n-ary type; §12 records this as a wording contradiction to fix.

### 1.2 Types

```rust
/// A workspace's tiling arrangement. Geometry is never stored here.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LayoutTree {
    /// The workspace has no panes. Only ever the root.
    Empty,
    Leaf(PaneId),
    Split(Split),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Split {
    pub id: SplitId,
    pub axis: Axis,
    /// Invariant: `children.len() >= 2`, weights are finite, strictly positive,
    /// and sum to 1.0 within `WEIGHT_EPSILON`.
    pub children: Vec<Child>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Child {
    pub weight: Weight,
    pub node: LayoutTree,
}

/// Column-wise (children side by side) or row-wise (children stacked).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis { Columns, Rows }

/// A fraction of the parent's usable extent along its axis. Newtyped so the
/// normalization invariant has one place to live.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Weight(f32);

pub const WEIGHT_EPSILON: f32 = 1e-4;
/// Below this a pane is not worth having; splits producing it are refused.
pub const MIN_WEIGHT: f32 = 1e-3;
```

`Axis` deliberately does **not** reuse doc 05's `Direction { Horizontal,
Vertical }`. "Horizontal split" is ambiguous in every multiplexer's
documentation — tmux's `split-window -h` produces a *vertical* divider. `Columns`
and `Rows` describe the arrangement of the children and cannot be misread. A
`Direction2D { Left, Right, Up, Down }` remains, for navigation and for
"split and put the new pane on the left".

Weights are `f32`, not a rational or fixed-point type. The alternative
(`u16` permille, exact sums) was considered and rejected: `compute` multiplies
by an extent and floors regardless, so exactness in the weight buys nothing that
the deterministic remainder rule (§2.2) does not already provide, and `f32`
keeps the serialized form human-editable. The normalization invariant is
asserted with `WEIGHT_EPSILON` slack and re-established by `renormalize` after
every mutation, so drift cannot accumulate across a long session.

Minimum sizes are **not** stored per pane. They are a property of the *renderer*
and are supplied to `compute`:

```rust
#[derive(Clone, Copy, Debug)]
pub struct Constraints {
    /// Smallest usable content area for a pane, in cells. Default 20×3.
    pub min: GridSize,
    /// Cells consumed by a divider between two siblings. 1 with borders, 0 without.
    pub divider: u16,
    /// Extra rows a pane spends on its own title bar. 0 or 1. See §9.
    pub title_rows: u16,
}
```

Making minimums a `compute` input rather than tree state is what lets the same
tree be laid out for a 200-column laptop and a 40-column phone, and it is why
`Constraints` carries `divider` and `title_rows` — tmux proves those are part of
the arithmetic, not decoration ([research §6.3](../research/multiplexers.md#63-borders-and-titles)).

### 1.3 The whole layout of a workspace

Tiling is one of three layers, following zellij and tmux, which both keep floats
out of the tiling arithmetic entirely
([research §5.2](../research/multiplexers.md#52-floating-panes)):

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layout {
    pub tiles: LayoutTree,
    /// Overlay layer, z-ordered back to front. Never participates in tiling.
    pub floats: Vec<FloatingPane>,
    /// Non-destructive zoom. The tree underneath is untouched.
    pub zoom: Option<PaneId>,
    /// Stacks collapse N panes into one tile. Keyed by the tile that hosts them.
    pub stacks: HashMap<StackId, Stack>,
    pub focus: Option<PaneId>,
    /// The previously focused pane, for `pane.focus_last`.
    pub last_focus: Option<PaneId>,
}
```

**Zoom is a flag, not a tree swap.** tmux implements zoom by building a whole
new single-leaf tree and stashing the old root
([research §5.1](../research/multiplexers.md#51-zoom)), which forces it to
unzoom-resize-rezoom on every window resize. A flag consulted by `compute` is
strictly simpler, is trivially serializable, and — crucially for §3 — lets one
client render zoomed while another renders the tiling, from one tree.

### 1.4 Normalization

Four rules, re-established by `LayoutTree::normalize` after every mutation and
asserted by `check_invariants` in debug builds:

| N1 | A `Split` with one child is replaced by that child, its weight folded into the parent's slot. |
| N2 | A `Split` child whose axis equals its parent's is spliced into the parent, its children's weights scaled by its own slot weight. |
| N3 | Weights are renormalized so each split's children sum to 1.0. |
| N4 | `Empty` appears only as the root, and only when the workspace has no panes. |

N2 is what keeps "three equal columns" a single node, and it is applied
*bottom-up* so a chain of same-axis splits collapses in one pass. The scaling in
N2 is exact in the sense that matters: a child at weight `w` in a spliced split
whose slot weight was `s` becomes `w * s`, and the parent's weights still sum to
1 because `sum(w) == 1`.

N1 and N2 together mean the tree has a **canonical form**: two operation
sequences producing visually identical arrangements produce equal trees. That is
what makes the golden tests in §11 possible and what makes `LayoutChanged`
events diffable.

---

## 2. Geometry and resize

### 2.1 `compute` — the only place geometry exists

```rust
pub struct Geometry {
    /// Every pane that received a rect, in stable pre-order.
    pub panes: Vec<PanePlacement>,
    /// Every divider, addressable and hit-testable. See §2.4.
    pub dividers: Vec<Divider>,
    /// Panes that exist in the tree but could not be placed at `min`.
    pub hidden: Vec<PaneId>,
    /// Set when `hidden` is non-empty, so a client can explain itself.
    pub degraded: Option<Degradation>,
}

pub struct PanePlacement {
    pub pane: PaneId,
    /// Including the title bar, excluding dividers.
    pub outer: Rect,
    /// The pane's content area; for a `pty` session this is the PTY-visible grid.
    pub content: Rect,
    pub edges: EdgeFlags,     // which sides touch the container, for border drawing
    pub stack: Option<StackId>,
}

pub struct Divider {
    /// Addresses the split node. Stable across renders; see §2.4.
    pub split: SplitId,
    /// Which gap in that split: between `children[index]` and `children[index+1]`.
    pub index: usize,
    pub axis: Axis,
    /// The divider's own rect, `divider` cells thick.
    pub rect: Rect,
}

impl Layout {
    pub fn compute(&self, area: Rect, c: Constraints) -> Geometry;
}
```

`compute` is pure, allocates one `Vec` per output field, and is called once per
client per frame. It is `O(panes)`.

### 2.2 Distributing an extent — the remainder rule

Given a split of `n` children with weights `w_i` inside an extent `E`:

```
usable   = E - divider * (n - 1)
raw_i    = usable * w_i                       // f32
floor_i  = floor(raw_i)
frac_i   = raw_i - floor_i
leftover = usable - sum(floor_i)              // integer, in [0, n)
```

**The `leftover` cells are given, one each, to the children with the largest
`frac_i`; ties are broken by ascending child index.** This is the rule; it is
specified rather than implied because every implementation studied does
something different and none of them documents it:

- tmux hands out ±1 round-robin in child order, so early children win
  ([research §2.1](../research/multiplexers.md#21-tmux--the-canonical-prior-art-explained));
- tmux's proportional path instead dumps the entire truncation onto the *last*
  child, which can be `n-1` cells;
- zellij sorts by rounded size and sweeps ±1
  ([research §2.6](../research/multiplexers.md#26-zellij--a-cassowary-solver-and-explicit-remainder-repair)).

Largest-fraction-first is the standard largest-remainder apportionment, it is
the one that minimises total deviation from the requested ratios, and the
index tie-break makes it deterministic — which the others are not, because both
depend on the current sizes rather than only on the weights.

Two properties fall out and are property-tested (§11): the placements exactly
tile the extent with no gap and no overlap, and every child's size differs from
`usable * w_i` by less than one cell.

### 2.3 Minimums, and what happens when they cannot be met

`compute` enforces `Constraints::min` **at placement time**, never by mutating
the tree. The algorithm, applied per split, top-down:

1. Compute `need_i` = the minimum extent the subtree at child `i` requires along
   this axis: for a leaf, `min` plus `title_rows`; for a split on the same axis,
   the sum of its children's needs plus dividers; for a split on the other axis,
   the maximum of its children's needs.
2. If `sum(need_i) + dividers <= usable`, distribute by §2.2, then **lift** any
   child below its `need_i` up to it and take the deficit from the children with
   the most surplus, largest-surplus-first. This terminates because step 2's
   guard guarantees enough total surplus.
3. If `sum(need_i) + dividers > usable`, the split cannot be shown in full.
   **Drop children from the tail of a priority order until it fits.** The
   priority order is: the focused pane's subtree first, then the remaining
   children in index order. Dropped panes are reported in `Geometry::hidden`,
   and the survivors are re-distributed by step 2 with the dropped children's
   weights excluded.
4. If even one child cannot meet its need, `compute` places the focused pane
   alone at the full extent and reports every other pane hidden with
   `Degradation::ForcedSolo`.

```rust
pub enum Degradation {
    /// Some panes were dropped; the rest fit. Client should offer a pane switcher.
    Partial { hidden: u16 },
    /// Nothing fits but one pane. Equivalent to an implicit zoom.
    ForcedSolo,
}
```

This is the mechanism [05 §2.1](05-session-model.md#21-operations) promises
("panes are dropped from the geometry, not from the tree") specified to the point
of implementability, and it is the load-bearing piece of the mobile story (§7):
**a phone does not get a different layout, it gets the same layout computed with
a small `area`, which degrades by construction.** tmux's alternative — letting
the layout exceed the window and cropping — is rejected outright; a cropped view
of a layout you cannot see is worse than an honest "3 panes hidden" affordance.

### 2.4 Manual resize — what dragging a border does

The single most important behavioural decision in this document, and the place
where naive implementations diverge from user expectation.

```rust
pub enum ResizeTarget {
    /// The addressable form. What a drag sends.
    Divider { split: SplitId, index: usize },
    /// The convenience form, for keybindings: "grow the focused pane rightwards".
    Edge { pane: PaneId, edge: Direction2D },
}

pub enum ResizeAmount {
    /// Cells, at the container size the client is looking at.
    Cells { delta: i32, area: Rect, constraints: Constraints },
    /// Fraction of the split's extent. Size-independent.
    Fraction(f32),
}

impl Layout {
    pub fn resize(&mut self, target: ResizeTarget, amount: ResizeAmount)
        -> Result<ResizeOutcome, LayoutError>;
}

pub struct ResizeOutcome {
    /// The change actually applied, which may be less than requested.
    pub applied: f32,
    pub clamped: bool,
}
```

**Rule: a resize moves exactly one divider, and changes exactly the weights of
the two children on either side of it. Their sum is conserved. Nothing else in
the tree changes.**

Concretely, for `Divider { split, index }` and a normalized delta `d`:

```
a = children[index].weight
b = children[index + 1].weight
a' = a + d
b' = b - d
```

with `a'` and `b'` clamped so that neither subtree falls below its `need`
expressed as a fraction of the split's usable extent, and so that neither drops
below `MIN_WEIGHT`. `ResizeOutcome::clamped` reports when the clamp bound.

Justification, and what is *not* done:

- **Not "renormalize all siblings".** If a 4-column split's second divider
  moves, columns 1 and 4 must not twitch. tmux gets this right by construction
  (`layout_resize_pane_grow` finds the nearest sibling with slack and transfers
  between exactly two cells); another tool gets it right by adjusting one split node's
  ratio. Both are right and this is the same rule.
- **Not "propagate to the next sibling when the neighbour is at minimum".** tmux
  *does* walk on to the next sibling for more slack. That produces a drag where
  the pane under your cursor stops moving but a pane two columns away starts
  shrinking, which is confusing on a screen and unexplainable on a touch device.
  omt clamps instead, and reports it. This is a deliberate, user-visible
  divergence from tmux.
- **The `Edge` form resolves to a `Divider` first**, using the same last-child
  inversion tmux discovered: walk from the pane to the nearest ancestor split
  whose axis matches the edge; the divider is the one on the requested side of
  the child that contains the pane; if the pane is in the last child and the
  edge points outward, there is no such divider, so use the divider *before* it
  and invert the sign. If no matching ancestor exists, return
  `LayoutError::NoDividerInDirection` — you cannot widen a pane in a workspace
  with no vertical divider, and saying so is better than silently doing nothing.
- **`SplitId` is stable and addressable**, so a drag is a sequence of small
  deltas against one named object, not a re-hit-test per frame. This is another tool's
  `SplitBorder { path }` idea with a stable id instead of a path, which survives
  a concurrent structural change from another client (a path would silently
  address a different split). If the `SplitId` no longer exists,
  `resize` returns `LayoutError::UnknownSplit` and the client drops the drag —
  the correct outcome under [P6](01-principles.md#p6--collaboration-is-a-runtime-feature-not-just-a-workflow).
- **`Cells` deltas are converted to fractions against the sending client's own
  `area`.** Two clients of different sizes dragging the same divider both get
  the movement they see under their own cursor. This is only coherent because
  weights, not cells, are the stored state.

### 2.5 Split and close

```rust
impl Layout {
    pub fn split(&mut self, at: PaneId, dir: Direction2D, new: PaneId, ratio: Option<f32>,
                 area: Rect, c: Constraints) -> Result<(), LayoutError>;
    pub fn close(&mut self, pane: PaneId) -> Result<Option<PaneId>, LayoutError>;
}
```

**Split.** `dir` gives both the axis and which side the new pane lands on
(wezterm's `target_is_second`, which tmux spells `SPAWN_BEFORE`;
[research §1.4](../research/multiplexers.md#14-wezterm--binary-tree-absolute-sizes-in-the-wire-protocol)).
`ratio` defaults to 0.5 and is the *new* pane's share of the target's slot.

1. Reject with `LayoutError::TooSmall` if the target's current extent, per
   `compute(area, c)`, cannot hold two panes at `min` plus a divider. Checking
   against the *caller's* area is deliberate: a phone must not be able to create
   a split it cannot see. (A client may pass the primary view's area to split
   "as the laptop sees it" — see §3.6.)
2. If the target's parent split already has this axis, **insert a sibling**
   rather than nesting: the target's slot weight `s` becomes `s * (1 - ratio)`
   and the new sibling takes `s * ratio`. This is what keeps N2 true without a
   normalization pass and is why "split right three times" yields four siblings,
   not three nested levels.
3. Otherwise replace the leaf with a two-child `Split` of the new axis.
4. Focus moves to the new pane.
5. `normalize`, then `check_invariants`.

A split with `top_level: true` (wezterm's flag) splits the *root* instead of the
target, which is how "put a full-width log pane along the bottom" is expressed.
It is a field on the capability input, not a separate operation.

**Close.** The closed child's weight is **redistributed proportionally among its
remaining siblings**, then N1 collapses a single-child parent.

This diverges from tmux, which gives the whole space to one neighbour
([research §2.5](../research/multiplexers.md#25-tmux--close)). In a binary tree
those are the same thing; in an n-ary tree tmux's rule means closing the middle
of three equal columns leaves 1/3 : 2/3, which is precisely the artifact that
motivated going n-ary in the first place. Proportional is the only choice
consistent with §1.1.

**Focus after close** goes to the spatially nearest surviving pane, computed by
running §6's directional search from the closed pane's last known rect in the
order right, left, down, up, and falling back to `last_focus` and then to the
first pane in tree order. Doc 05 §2.1 says "right, else left, else parent's
next"; this is that rule made total.

### 2.6 Container resize

**Free.** The tree stores no geometry, so a container resize is a re-`compute`
and nothing else. There is no `layout_resize`, no proportional rebuild, no
possibility of the layout exceeding the container.

What is *not* free is the consequence: a re-`compute` may change a pane's
content rect, which may change a session's PTY size (§3), which costs a reflow
([04 §3](04-terminal-core.md#3-reflow-on-resize)) and a redraw for any
alt-screen program. Therefore:

- Resizes are **debounced and coalesced** at 250 ms per doc 05 §2.2, and a drag
  produces one PTY resize, not sixty.
- A pane entering or leaving `Geometry::hidden` does **not** resize its session's
  PTY. A hidden pane is a *presentation* fact; the session keeps its size and
  keeps running. This matters because a phone rotating between portrait and
  landscape must not resize the laptop's agent twice per rotation.
- Per [04 §3.3](04-terminal-core.md#33-algorithm) step 1, a resize is deferred
  while a DECSET 2026 synchronized-output block is open.

---

## 3. Multi-client, multi-size — the crux

### 3.1 The constraint that decides everything

A PTY has exactly one size. `TIOCSWINSZ` takes one `struct winsize`, delivers
one `SIGWINCH`, and the program inside renders one frame for one geometry. There
is no per-observer variant and there never will be.

Two consequences are not negotiable:

- **A session that is visible to a laptop and a phone at the same time is
  rendered once, at one size.** Someone sees a size that is not theirs.
- **Server-side per-client re-rendering is impossible for the case that matters.**
  [07 §4.3](07-remote-protocol.md#43-the-resize-problem) already rejects it and
  the reasoning is correct: an alt-screen agent that drew a box at 120 columns
  cannot be re-rendered at 40, because the information is gone, which is true
  for a `pty` session; a `native` session's content is typed events and
  re-renders freely at any width (§3.7). Nothing in tmux,
  zellij, wezterm or another tool does this
  ([research §3](../research/multiplexers.md#3-multi-client-different-sizes--the-crux)).

But omt has one degree of freedom tmux does not, and it is the whole answer.

### 3.2 The degree of freedom: panes are not sessions

[05 §1](05-session-model.md#1-the-object-model) makes **Pane → Session
many-to-one**: a session may be shown in several panes, in several layouts, on
several clients. tmux cannot do this — a tmux pane *is* the process's home.

Therefore omt can decouple two questions that every other multiplexer conflates:

1. **"How are things arranged on my screen?"** — pure presentation. Has no
   effect on any process. Can be per-client at zero cost.
2. **"What size is this session's PTY?"** — genuinely shared, genuinely one
   value.

tmux, zellij and another tool all answer (1) globally because they must, and then have
to make (2) painful to compensate. omt should answer (1) per client and confine
the pain to (2), where it is irreducible.

This is also, independently, what zellij's mobile mode concluded: a phone gets
its **own tab** running a mobile plugin, plus a per-client **shadow focus**
pointer into the shared tabs, so the phone never constrains the laptop's tab
([research §3.2](../research/multiplexers.md#32-zellij--smallest-of-active-plus-a-genuine-mobile-mode)).
omt can express the same idea in its own object model without a parallel plugin
universe.

### 3.3 Decision: per-client layout views, one shared default

```rust
/// A workspace holds a small set of named arrangements over its sessions.
pub struct Workspace {
    // ... per 05 §1.1 ...
    pub views: IndexMap<ViewId, LayoutView>,
    /// The view a client is put in when it attaches and asks for nothing.
    pub primary: ViewId,
}

pub struct LayoutView {
    pub id: ViewId,
    pub name: String,              // "primary", "phone", "ada's laptop"
    pub layout: Layout,
    pub kind: ViewKind,
    /// Clients currently rendering this view.
    pub clients: SmallVec<[ClientId; 4]>,
}

pub enum ViewKind {
    /// The workspace's canonical arrangement. Exactly one per workspace.
    /// Shared: everyone in it sees the same splits, the same zoom, the same focus.
    Primary,
    /// Auto-created for a client too small for `Primary`. Destroyed when its
    /// last client detaches. Never persisted.
    Adaptive { derived_from: ViewId, owner: ClientId },
    /// Explicitly created by a user ("give me my own arrangement here").
    /// Persisted with the workspace.
    Named,
}
```

**A pane belongs to exactly one view.** Two views showing the same session hold
two different `PaneId`s pointing at one `SessionId`, each with its own
`PaneView` (scroll, selection, search) — which doc 05 §4 already specifies as
per-client state. The many-to-one relation is what makes this cost nothing.

**Attach policy**, evaluated when a client attaches or reports a new viewport:

```rust
pub fn choose_view(ws: &Workspace, client: &ClientView, cfg: &LayoutConfig) -> ViewId {
    if let Some(v) = client.pinned_view { return v; }                  // explicit wins
    let primary = &ws.views[ws.primary];
    let geom = primary.layout.compute(client.area(), cfg.constraints);
    if geom.hidden.is_empty() { return ws.primary; }                   // it fits: share it
    if !cfg.adaptive_views { return ws.primary; }                      // opted out: degrade in place
    ws.adaptive_view_for(client)                                       // fork a private view
}
```

An `Adaptive` view is seeded by projecting the primary layout onto the client's
size: run `compute`, take the panes that survived in priority order, and build
the largest preset (§4.1) that fits — in practice `Solo` on a phone, `Even` with
two panes on a tablet. It then tracks the primary view's *membership* (a session
added to the workspace appears in both) but not its *arrangement*.

**Why this default.** The two failure modes we must avoid are (a) a phone
attaching and reflowing a laptop's agent to 52 columns — tmux's `latest` and
`smallest` both do this, and it is the single most-complained-about multiplexer
behaviour — and (b) a phone being shown a cropped window into a four-way split it
cannot read, which is tmux's `largest`. Per-client layout avoids both by never
making the phone and the laptop share an arrangement they cannot both use.
[P3](01-principles.md#p3--parity-one-capability-three-surfaces) is preserved
because the phone can still perform every layout operation (§7.3) — it just
performs them on its own view, and can push them to the primary view when it
means to.

**Cost, stated honestly.** Two clients may now be looking at differently-arranged
views of the same workspace, so "focus the pane on the left" means different
things to each. That is correct — it is *their* left — but it means presence
([05 §6](05-session-model.md#6-presence)) must report `viewing: (ViewId,
SessionId)` and not merely `PaneId`, or the laptop cannot render "the phone is
watching this session". §12 records this as a required change to `Presence`.

### 3.4 The PTY size question, which per-client layout does not solve

Per-client layout removes *most* of the conflict, because a phone in an
`Adaptive` view usually shows one session while the laptop shows four, and the
three the phone is not showing are unaffected. It does not remove the conflict
for the session the phone *is* showing.

```rust
pub struct SessionSizing {
    /// The size actually on the PTY.
    pub current: GridSize,
    pub policy: SizePolicy,
    /// Everyone rendering this session, with the content rect their view gives it.
    pub observers: SmallVec<[(ClientId, GridSize, Participation); 4]>,
    /// Debounce state; see §2.6.
    pub pending: Option<(GridSize, Instant)>,
}

pub enum Participation {
    /// This observer's size is an input to the policy.
    Participant,
    /// This observer renders whatever it is given and letterboxes. Never an input.
    Observer,
}

pub enum SizePolicy {
    /// Default. The writer-token holder's view drives the PTY. Falls back to
    /// `Smallest` over participants when nobody holds the token.
    Driver,
    /// Every participant is fully visible. Opt-in, for pair programming.
    Smallest,
    /// "Keep this session at 120x40 whatever happens."
    Pinned { size: GridSize, by: ActorId },
}
```

**Decision: `Driver` is the default, matching
[07 §4.3](07-remote-protocol.md#43-the-resize-problem)'s `SizeOwner::Writer`,
and doc 05 §2.2's "minimum over participants" becomes the *fallback* used when
no writer holds the token, and the behaviour of the opt-in `Smallest` policy.**

Reasoning:

- The writer is the person whose keystrokes the program is responding to. Sizing
  for them is sizing for the interaction that is actually happening.
- The writer token already exists ([05 §5](05-session-model.md#5-the-writer-token)),
  is already visible in every UI, and already has a takeover protocol. Attaching
  size ownership to it costs no new concept and no new UI.
- Unconditional `Smallest` is exactly tmux's most-hated behaviour. It should be
  reachable, because pair programming genuinely wants it, but not default.
- A resize is disruptive for alt-screen programs (§2.6), so the policy should
  change the size *rarely*. `Driver` changes it only on takeover; `Smallest`
  changes it on every attach and detach.

**Consequences, all user-visible, all required to be surfaced:**

1. **Taking the writer token may resize the PTY.** `keep_size` and the 20 %
   threshold are *acquisition* semantics, so they belong to
   [12 §3.3](12-collaboration.md#33-lifecycle) alongside
   [07 §4.3](07-remote-protocol.md#43-the-resize-problem)'s negotiation. The
   client warns
   before the first takeover of a session whose size would change by more than
   20 %, and offers `writer.acquire { keep_size: true }`, which acquires the
   token while installing `Pinned` at the current size. That flag is now a
   `SizePolicy` transition, and it must be reflected in `session.writer.acquire`.
2. **Non-participants letterbox.** A client whose view gives a session a smaller
   content rect than `current` renders the full grid scaled to fit width and
   letterboxed vertically, with the badge doc 07 already specifies:
   `120×40 · driven by laptop`. The phone reads at full fidelity; only
   legibility degrades, and on a phone the block view
   ([08 §4.2](08-web-client.md#42-block-view)) is the default surface anyway.
3. **A client larger than `current` renders inactive margin**, never a stretch.
4. **The mobile web client attaches `Observer` by default**, per doc 05 §2.2, and
   switching it to "full terminal" makes it a `Participant` with a visible
   notice that it may resize the session. A phone therefore cannot shrink a
   laptop's agent by accident — only by choosing to.
5. **`Orphaned` and `Exited` sessions have no sizing.** Their content is
   readable at whatever width it was written; a resize would reflow scrollback
   for no benefit and is refused.

**The degradation story, end to end.** Laptop at 200×50 has a four-pane
workspace; a phone at 52×24 attaches. The phone does not fit the primary layout,
so it gets an `Adaptive` view showing one session — by default the focused one.
Nothing resizes; the laptop is untouched. The phone renders that session in
block view, which needs no grid fidelity at all. The user taps "terminal view":
the phone is still an `Observer`, so it letterboxes the 200-column grid with the
badge. The user needs to type: they request the writer token; omt computes that
taking it would move the PTY from 200×50 to 52×24, a 74 % change, and warns,
offering "take input without resizing". If they accept the resize, the laptop's
pane letterboxes instead and shows `52×24 · driven by phone`, and the agent
redraws once. Every step is visible, reversible, and named.

### 3.5 Rejected alternatives

- **Shared authoritative size with letterboxing for all** (no per-client layout).
  This is doc 07's model taken alone. It is correct for a *session* and wrong for
  a *workspace*: it means the phone renders a scaled-down four-way split, and a
  four-way split scaled to a phone is unreadable regardless of fidelity.
  Per-client layout is what makes doc 07's per-session answer survivable.
- **Server-side per-client rendering.** Rejected in §3.1 and in doc 07. It also
  costs one emulator per client per session, which is a real memory cost for a
  daemon expected to hold dozens of sessions.
- **"Primary client sets everything, others are read-only mirrors."** Violates
  [D2](decisions.md#d2--remote-is-exactly-equivalent-to-local) — remote is
  exactly equivalent to local — and it is precisely the second-class mobile
  experience the product exists to avoid.
- **Per-client PTY per session (fork the process view).** Not possible. One
  process, one controlling terminal.

### 3.6 Cross-view operations

Because arrangements are per-view, layout capabilities take an explicit view:

- `pane.*` and `layout.*` inputs carry `view: Option<ViewId>`, defaulting to the
  caller's current view.
- `layout.promote { view }` copies an `Adaptive` or `Named` view's arrangement
  onto `Primary` — "make everyone see what I see". Requires `Operator`.
- `layout.adopt { view }` replaces the caller's view with a copy of another's.
- A client may pin itself to `Primary` regardless of size
  (`client.pinned_view`), accepting the degradation; §7 gives it a real UI.

### 3.7 Native sessions

A `native` session ([D8](decisions.md#d8--two-session-modes-pty-default-and-native-acp),
[05 §1.3](05-session-model.md#13-session-modes-d8)) has no PTY and no grid, so
everything §3.4 arbitrates simply does not apply to it.

- **Size-independent, and excluded from `SessionSizing` entirely.** There is no
  `current`, no `policy`, no participants to weigh.
- **They never letterbox.** A native pane re-renders at whatever content rect it
  is given, independently per client — 200 columns on the laptop and 52 on the
  phone at the same instant, both at full fidelity.
- **They emit no `SessionResized`** (05 §11 invariant 11).
- `session.size_policy` and `layout.synchronize` **reject** a native pane; there
  is no PTY to size and none to broadcast keystrokes into.
- The 250 ms resize debounce (§2.6) is **skipped**: a re-`compute` costs a
  re-render and nothing else, so there is nothing to coalesce.

---

## 4. Presets, DSL, saved layouts

### 4.1 Presets

```rust
pub enum LayoutPreset {
    /// One pane fills the view. The degenerate case, and the phone default.
    Solo,
    /// All panes in one row, or one column.
    EvenColumns, EvenRows,
    /// One large pane plus a column/row of the rest. `main` is a fraction.
    MainColumn { main: f32, mirrored: bool },
    MainRow { main: f32, mirrored: bool },
    /// Balanced grid, last row short. Ordered row-major.
    Tiled,
    /// One flexible pane plus N collapsed title bars. See §5.3.
    Stacked,
}
```

The names are omt's; the set is tmux's minus the mirrored duplicates (folded
into a flag) plus `Solo` and `Stacked`. `MainColumn { main }` replaces tmux's
`main-pane-width`/`other-pane-width` pair — one fraction, because two absolute
widths that must be reconciled against each other and against the window is
three sources of truth for one number
([research §4.1](../research/multiplexers.md#41-tmux-presets-and-the-layout-string)).

`apply_preset` rebuilds the tree from the view's panes **in their current
pre-order**, so applying a preset twice is idempotent and applying `Tiled` then
`EvenColumns` then `Tiled` returns the original tree. Preset application never
creates or destroys a pane.

`layout.balance` is separate: it sets every child of a given split (or of every
split, with `recursive`) to equal weight, without changing the structure. This
is tmux's `layout_spread_cell` and kitty's `equalize`, and it is the operation
people actually reach for.

### 4.2 The serialization format

**Decision: one format, used for saved layouts, for persistence, for the wire,
and for launch configurations — the same text a user can hand-edit.** This is
zellij's bet ([research §4.2](../research/multiplexers.md#42-zellij--kdl-layout-files))
and it is what makes restore a feature rather than a plugin ecosystem, which is
what tmux's separate, unauthorable layout string cost it.

The recursive shape is already fixed by
[10 §9.1](10-configuration.md#91-launch-configurations), adopted from another terminal, and
this document does not get to change it. Restated as a type:

```rust
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum LayoutSpec {
    Leaf(PaneSpec),
    Branch {
        split: Axis,                    // "columns" | "rows"
        /// Weights. Omitted means equal. Length must match `panes`.
        #[serde(default)]
        ratio: Vec<f32>,
        panes: Vec<LayoutSpec>,
    },
    /// A named preset over `panes`, resolved at apply time.
    Preset { preset: LayoutPreset, panes: Vec<LayoutSpec> },
}

#[derive(Serialize, Deserialize)]
pub struct PaneSpec {
    pub cwd: Option<PathBuf>,
    pub session: Option<SessionRef>,    // bind an existing session instead of spawning
    pub commands: Vec<String>,
    pub agent: Option<AgentSpec>,
    pub title: Option<String>,
    #[serde(default)]
    pub focused: bool,
    /// Minimum this pane wants; below it the pane is dropped first (§2.3).
    pub min: Option<GridSize>,
    /// Ordering hint for §2.3's drop priority. Higher survives longer.
    #[serde(default)]
    pub priority: i8,
}
```

Notes:

- **`split` is `columns`/`rows`, not `horizontal`/`vertical`**, for the reason in
  §1.2. Doc 10 §9.1's example uses `horizontal`/`vertical`; §12 records that both
  spellings must be accepted, with `columns`/`rows` canonical on write.
- **`Preset` in the file** is what gives a saved layout responsiveness for free:
  `{ preset: tiled, panes: [...] }` is meaningful at any size and any pane count,
  where a fixed `ratio` list is not.
- **`priority` and `min`** are what let a launch config say "the agent pane
  survives on a phone; the log tail does not". Without them §2.3's drop order is
  a guess.
- YAML for launch configs (doc 10's decision, unchanged); the same structure is
  JSON on the wire and in the store.

**Responsive variants.** zellij's swap layouts are the best idea in this space
and omt takes them in a simplified form: a saved layout may carry alternates
guarded by a size or pane-count predicate.

```yaml
layout:
  split: columns
  ratio: [0.6, 0.4]
  panes: [...]
when_narrower_than: 100          # cols
  layout: { preset: stacked, panes: [...] }
when_narrower_than: 60
  layout: { preset: solo, panes: [...] }
```

Evaluated at `choose_view` time (§3.3) and on viewport change, most specific
first. Like zellij, a view whose arrangement the user has manually changed is
marked **damaged** and stops auto-swapping until they ask for it
(`layout.rearm`) — otherwise a rotation undoes their work.

### 4.3 Importing tmux layout strings

`layout.import_tmux { string }` accepts the format documented in
[research §4.1](../research/multiplexers.md#41-tmux-presets-and-the-layout-string):

```
checksum "," cell        cell := SX "x" SY "," XOFF "," YOFF [ "," pane-id | "[" cells "]" | "{" cells "}" ]
```

Import is worth building because there is a large corpus of these in people's
notes, dotfiles and tmux-resurrect saves, and because it is a 100-line parser.

- `{}` → `Axis::Columns`, `[]` → `Axis::Rows`.
- **Absolute cells become weights**: for each split, `weight_i = child_extent_i /
  sum(child_extent + divider)`. The format's own consistency rule
  (`sum(child.sx + 1) - 1 == parent.sx`) guarantees this is well-defined; a
  string that violates it is rejected with the offending node, not
  best-effort-parsed ([P5](01-principles.md#p5--production-grade-from-the-first-commit)).
- The checksum is **verified and reported but not trusted** — it is a 16-bit
  rotate-and-add typo guard, not integrity.
- Pane ids in the string are positional only; the caller supplies the mapping to
  omt sessions, or omt spawns shells in pre-order.
- Floating cells in the `<...>` suffix map to §5.2 floats.

Export is deliberately **not** offered. Emitting absolute cells for one size is a
lossy encoding of a ratio tree, and offering it would invite round-tripping
through a format that cannot represent presets, priorities or float pinning.

### 4.4 Saved layouts and per-workspace defaults

- `~/.config/omt/layouts/*.yaml` — a bare `LayoutSpec`, applicable to any
  workspace (`layout.apply_saved { name }`). Panes without a `session` spawn
  shells; panes with one bind by title or role.
- `<repo>/.omt/layouts/*.yaml` — project-local, shadowing user layouts of the
  same name, subject to doc 10's trust prompt.
- **Per-workspace default**: `workspace.default_layout` in config, keyed by path
  glob or by git remote, applied on `workspace.open` when the workspace has no
  persisted layout. This is the cheap version of a launch config, for people who
  want "any Rust repo opens with an agent pane and a test pane" without naming
  it.
- `layout.save { name, view }` snapshots a live view into a file. The round-trip
  (§4.2) is what makes these worth maintaining; doc 10 §9.1 already makes the
  same argument for `omt launch save`.

Launch configurations (doc 10 §9.1) remain the richer thing — they create
workspaces and sessions and bind agents. A saved layout only rearranges what is
already there, or spawns shells. `launch.save` and `layout.save` share the
`LayoutSpec` serializer.

---

## 5. Zoom, floats, stacks, and moving panes

### 5.1 Zoom

`Layout::zoom: Option<PaneId>`, consulted by `compute`: when set, the named pane
gets the whole area and every other pane is reported `hidden`. The tree is
untouched.

- **Zoom is per view, not per workspace.** This resolves
  [05 §13 open question 5](05-session-model.md#13-open-questions) in the way that
  document anticipated: a phone in an `Adaptive` view is effectively always
  zoomed, and that must not force the laptop into zoom. A deliberate shared
  "everyone look at this" is `layout.promote` (§3.6) after zooming, or
  `pane.zoom { view: primary }` explicitly.
- Zoom survives structural changes to the tree; closing the zoomed pane clears
  it and focuses per §2.5.
- `Degradation::ForcedSolo` (§2.3) is rendered identically to zoom but is
  distinguishable in the state, so the UI can say "too small to show the layout"
  rather than "zoomed" — different words for different situations.

### 5.2 Floating panes

```rust
pub struct FloatingPane {
    pub pane: PaneId,
    /// Fractions of the view area, so a float survives a resize and a phone.
    pub rect: FractionalRect,
    pub min: GridSize,
    /// Stays visible when the float layer is hidden.
    pub pinned: bool,
    pub kind: FloatKind,
}

pub enum FloatKind {
    /// A user-created floating terminal.
    Terminal,
    /// System overlay: the workspace explorer panel (15), an interaction card
    /// (06), the palette (16). Not persisted, not reorderable by the user.
    Overlay { role: OverlayRole },
}
```

Floats are a **parallel layer** — never nodes in `tiles`, never inputs to any
tiling arithmetic. Both tmux and zellij landed on this independently
([research §5.2](../research/multiplexers.md#52-floating-panes)), and it is what
keeps §2's algorithms free of `is_tiled` guards on every recursion, which is the
thing that makes tmux's `layout.c` hard to read.

- `rect` is fractional so that a float placed on a laptop lands somewhere
  sensible on a phone, clamped to the area and to `min`.
- The whole layer toggles with `layout.floats.toggle`, `pinned` floats excepted.
- `FloatKind::Overlay` is how [15 — Workspace explorer](15-workspace-explorer.md)'s
  panel and doc 06's interaction cards get placed without inventing a second
  overlay system. Doc 15 §7.1 describes a TUI side panel that "does not exist
  until toggled"; that is an `Overlay` float created on toggle and dropped on
  close, and the two documents should agree on the mechanism.
- **On a phone, floats become sheets.** §7.2.
- Floats are excluded from directional navigation by default
  (`pane.navigate` stays within `tiles`); `pane.focus_float` and `Esc` move in
  and out of the layer. Mixing them produces navigation nobody can predict.

### 5.3 Stacked panes

zellij's stacks are N panes occupying one tile, one expanded and the rest
collapsed to a title row
([research §5.3](../research/multiplexers.md#53-stacked-panes)). They are worth
having for exactly one reason: **they are the only arrangement that keeps six
panes addressable in a space that fits two**, which is the tablet and
narrow-laptop case.

```rust
pub struct Stack {
    pub id: StackId,
    /// Ordered. Exactly one is expanded.
    pub members: Vec<PaneId>,
    pub expanded: usize,
}
```

A stack occupies one `Leaf` slot; `LayoutTree::Leaf(PaneId)` where that pane is
a stack member resolves through `Layout::stacks`. `compute` gives the expanded
member `extent - (members.len() - 1) * title_rows` and one title row to each
other member. The stack's minimum need is
`min.rows + (members.len() - 1) * title_rows`, and if that does not fit, §2.3
drops trailing members into `hidden` rather than failing.

Operations: `pane.stack.create` (from a split's children),
`pane.stack.expand { pane }`, `pane.stack.move { pane, dir }` within the stack,
`pane.stack.break_out { pane }` back into the tiling tree. Directional
navigation into a stack lands on the expanded member (zellij's rule, and it is
right); navigation *within* a stack is `Up`/`Down` once focus is inside.

### 5.4 Swap, rotate, move, marks

```
pane.swap   { a, b }                      exchange two panes' slots; layout unchanged
pane.rotate { split?, reverse }           cycle panes through fixed slots
pane.move   { pane, to, dir }             remove, then split `to` and place there
pane.move_to_workspace { pane, workspace, view? }
pane.mark   { pane, mark: Option<MarkId> }
```

- **`rotate` cycles contents through slots, never the tree.** tmux's
  `rotate-window` does this and it is much simpler and much more predictable than
  rotating structure. kitty's `splits` layout has the same operation.
- **`move_to_workspace` moves the *pane*, and the session follows only if this
  was its last pane** — per doc 05's lifetime rule 1, a session outlives its
  panes. Moving a pane to another workspace does not change the session's
  `workspace` field ([05 §1](05-session-model.md#1-the-object-model): a session's
  workspace is fixed at creation), which means a workspace's view may show a
  session that is not a member of it. That is a real and useful thing — "show me
  the deploy log from the infra workspace next to my code" — and `pane.list`
  reports `foreign: bool` so the UI can label it.
- **Marks are per actor, not global.** tmux has exactly one server-wide marked
  pane ([research §5.5](../research/multiplexers.md#55-marked-pane-and-synchronize-panes)),
  which is a single-user assumption omt should not inherit under
  [D4](decisions.md#d4--single-user-many-devices--with-the-interfaces-left-open-for-many-users).
  `marks: HashMap<ActorId, PaneId>` plus optional named marks
  (`MarkId::Named(String)`) for "jump to the pane I call `build`". A mark is
  rendered in the border of the marking actor's own clients only, plus a subtle
  shared indicator, so a collaborator's mark does not confuse you.
- **`synchronize` (input broadcast)** is a property of a *view*, not of the
  workspace: `LayoutView::synchronize: Option<SyncGroup>` naming a set of panes.
  It requires the writer token on **every** target session, acquires them as a
  set or fails atomically, and is rendered with a loud persistent indicator per
  tmux's precedent. It is `Operator` and declares `WRITES_PTY` and `DESTRUCTIVE`
  — typing one command into eight production shells is exactly what
  [03 §2](03-capability-catalog.md#2-declaring-a-capability)'s confirm gesture
  exists for.

---

## 6. Directional focus navigation

```rust
impl Layout {
    pub fn navigate(&self, from: PaneId, dir: Direction2D, geom: &Geometry,
                    wrap: bool) -> Option<PaneId>;
}
```

**The algorithm is geometric, over the last computed `Geometry`, not over tree
order.** Tree order gives wrong answers in asymmetric layouts, which is why all
four studied implementations are geometric
([research §6.1](../research/multiplexers.md#61-directional--the-geometric-algorithm-three-implementations-agreeing)).

Let `f` be the source pane's `outer` rect. For `dir = Left`:

1. **Candidates.** Every placed, non-hidden pane `p ≠ from` with
   `p.right <= f.left` and `overlaps(p.top, p.bottom, f.top, f.bottom)`, where
   `overlaps(a0,a1,b0,b1) = a0 < b1 && a1 > b0`.
   The `<=` (rather than tmux's exact `p.right + divider == f.left`) means a
   pane two columns away is eligible when nothing is adjacent, which matters
   after a `hidden` drop leaves a gap.
2. **Rank** by the tuple, ascending, and take the minimum:

   ```
   ( edge_distance,            // f.left - p.right, saturating
     Reverse(overlap_cells),   // perpendicular overlap; more is better
     center_distance,          // |center(p.perp) - center(f.perp)|
     placement_index )         // stable pre-order index; total tie-break
   ```
3. **Wrap**, if `wrap` is on and there were no candidates: re-run against panes
   whose `p.left >= f.right`, ranking by *greatest* `edge_distance` — i.e. jump
   to the far side. Off by default; tmux wraps, and it surprises people.

Transposed identically for `Right`, `Up`, `Down`.

**Ties and non-rectangular cases.** The tuple is total: `placement_index` is
unique per pane, so `navigate` is a deterministic function of
`(geometry, from, dir)`. This is a deliberate divergence from tmux and zellij,
which both break ties by **most-recently-active**
(`window_pane_choose_best` compares `active_point`; zellij uses
`max_by_key(active_at)`). Recency means the same keypress from the same visual
arrangement can go to two different panes depending on history — an
unpredictable keybinding, and unexplainable in a UI. another tool's ranking tuple is
the better prior art and omt takes it.

Non-rectangular arrangements — an L-shaped region left of the source, a tall
pane facing three short ones — are handled by the overlap term: the neighbour
sharing the most edge wins, then the one whose centre is closest. A source pane
facing three equal neighbours picks the topmost (equal overlap, equal centre
distance is impossible for equal-height neighbours; if it were, the index
decides).

**Other navigation**, all thin wrappers:

| Capability | Behaviour |
|---|---|
| `pane.focus { pane }` | Direct. Fails `not_found` if the pane is not in the caller's view. |
| `pane.navigate { from, dir, wrap? }` | The above. |
| `pane.focus_last` | `Layout::last_focus`. |
| `pane.focus_index { n }` | `n`-th in placement order, 1-based; the TUI's `display-panes` overlay and the phone's pane switcher both use it. |
| `pane.focus_cycle { reverse }` | Placement order, wrapping. |
| `pane.focus_edge { dir }` | Outermost pane in a direction. |

Doc 16 §11 binds `<leader> h j k l` and arrows to what it calls
`pane.focus_direction`; §12 records the name mismatch with doc 05's
`pane.navigate`.

---

## 7. Mobile and web parity

A phone cannot show a four-way split, and no amount of scaling changes that.
[P3](01-principles.md#p3--parity-one-capability-three-surfaces) nevertheless
requires that a phone can *do* everything. The two are reconciled by separating
**what is arranged** from **what is shown**.

### 7.1 What a phone actually renders

Per [08 §4](08-web-client.md#4-two-view-modes), a phone defaults to **block
view** below 600 CSS px, and block view needs no grid at all. So the layout
question on a phone is not "how do I draw four panes" but "how do I let the user
move between four sessions".

**Decision: a phone gets an `Adaptive` view containing `LayoutPreset::Solo`,
rendered as a horizontally swipeable carousel over the view's panes, with a
persistent pane strip.**

- **Carousel, not tabs.** Doc 08 already binds "swipe left/right on the session
  header" to previous/next session, so the gesture exists. One pane fills the
  viewport; adjacent panes are one swipe away. This is the same information
  architecture as `Solo` + `pane.focus_cycle`, so it needs no special server
  support.
- **Pane strip.** A one-row strip of chips above the content: session title,
  agent status dot (§8), unread/attention badge. Tapping a chip is
  `pane.focus_index`. This is the phone's answer to "which panes exist", and it
  is also what a `Degradation::Partial` renders when a user has pinned themselves
  to a layout that does not fit.
- **Split preview.** A phone user who creates a split needs to see that it
  worked. The strip grows a chip and a toast says "split created — 2 panes";
  tapping the toast opens the **layout map** (§7.3).
- **Landscape tablet** (≥ 900 px) gets the primary view if it fits, else an
  `Adaptive` view with `EvenColumns` at two panes, else `Stacked`.

### 7.2 Floats and overlays on a phone

`FloatKind::Overlay` renders as a **bottom sheet**, not a floating rectangle:
the explorer panel, the palette and interaction cards are all sheets, per doc 08
§5's existing card variants (`inline | sheet | list`). `FloatKind::Terminal`
floats become ordinary carousel entries marked as floating — a genuinely
floating terminal on a 52-column screen is not a useful object, and pretending
otherwise would be a worse lie than converting it.

### 7.3 Performing every layout operation from a phone

Dragging a border on a phone is bad, and building a touch drag affordance for
1-cell precision would be a bad feature well implemented. Instead:

1. **The palette is the primary surface.** Doc 16 §4 already generates palette
   rows from capability inputs by enumerating enums — `pane.split` becomes
   "Split into columns" / "Split into rows", `layout.preset` becomes one row per
   preset. Every `pane.*` and `layout.*` capability is therefore reachable from
   a phone with no per-capability mobile work, which is
   [P3](01-principles.md#p3--parity-one-capability-three-surfaces) working as
   designed rather than as an aspiration.
2. **The layout map** — a dedicated full-screen editor, reached from the pane
   strip's overflow or the palette. It renders the view's `Geometry` as
   touch-sized boxes at whatever aspect the phone has, and supports:
   - tap a box → focus (and dismiss);
   - long-press a box → action sheet: split (4 directions), zoom, close, move to
     workspace, mark, stack;
   - **drag a box onto another → `pane.move { pane, to, dir }`**, with the drop
     direction taken from which quadrant of the target you release in. This is
     the same idea as another terminal's `PaneDragDropLocation` and it is a *good* touch
     interaction, unlike border dragging;
   - **tap a divider → a size stepper**, `−` / `+` / `=`, issuing
     `pane.resize { target: Divider{..}, amount: Fraction(±0.05) }` and
     `layout.balance`. Discrete 5 % steps against a named divider are precise,
     undoable, and require no fine motor control. A slider is offered for
     continuous adjustment on tablets.
   - the map edits the phone's own view by default, with a prominent
     **"apply to everyone"** button (`layout.promote`) so the phone can arrange
     the laptop deliberately.
3. **Presets are the ergonomic path.** On a phone, "make it tiled" is one tap and
   is what people actually want; the divider stepper exists for the rare case.
4. **Voice and STT** (`stt.*`, doc 03) reach the palette like any other input, so
   "split right" is a spoken command with no extra plumbing.

Nothing in this section is a reduced capability set. The phone edits a different
*view* by default, which is a scoping decision, not a permission one — consistent
with [D2](decisions.md#d2--remote-is-exactly-equivalent-to-local).

---

## 8. Rendering

What every surface must render, so the TUI and the web client agree.

**Borders and dividers.** A divider is `Constraints::divider` cells thick
(1 with borders). `PanePlacement::edges` says which sides touch the container so
the outer frame is not doubled. Border style carries three orthogonal signals and
must not overload one channel:

| Signal | Channel |
|---|---|
| Focused pane | Border colour + weight |
| Writer holder is elsewhere | A short label in the border: `driven by phone (Ada)` |
| Marked | A glyph at the border's start |

**Titles.** `Constraints::title_rows` is 0 or 1 and is part of the arithmetic
(§1.2). When 1, the title row carries, left to right: index, session title
(explicit override, else OSC 2, else command — doc 05 §1.1), agent badge, and
right-aligned status. When 0, the same information collapses into the border
line where there is room, and is otherwise available on demand.

**Agent status badge** — from `AgentState`
([06 §4](06-agent-layer.md)), rendered identically on every surface:

| `AgentState` | Badge | Notes |
|---|---|---|
| `Starting` | dim spinner | |
| `Idle` | dot, muted | |
| `Working { detail }` | animated dot + elapsed | `detail` on hover/long-press |
| `Blocked { interaction: Some(_) }` | **attention colour + count** | Tappable → `interaction.resolve`; the flagship path |
| `Blocked { interaction: None }` | attention colour, no count | "needs you" — open terminal view. Doc 06 is explicit that this degradation is visible, never silent. A `native` session (§3.7) has no terminal view; the action opens its event timeline instead |
| `Exited { code }` | exit code, red when non-zero | |
| `Unknown` | nothing | Not a badge; absence of information is not a state to advertise |

A badge on a **hidden** pane (§2.3) must still surface: the pane strip chip
carries it, and `Degradation::Partial` renders as "2 hidden · 1 needs you". A
phone must never be unable to see that an agent is blocked because the pane
holding it did not fit.

**Dead sessions.** A pane whose session is not `Live` renders its content
normally — scrollback stays readable, searchable and copyable — with a
persistent banner. For a `native` session (§3.7) there is no scrollback; the
persisted transcript takes its place and is equally readable, searchable and
copyable, and `Restart` re-spawns the adapter.

| State | Banner | Actions |
|---|---|---|
| `Exited { status }` | `exited 1 · 4m ago` | **Restart**, Close, Copy output |
| `Orphaned { .. }` | `daemon restarted — process gone` | **Restart** (re-spawn same argv/cwd/env into this pane, keeping old scrollback above a separator, per doc 05 §8.2), Close |
| `Closing` | dim, non-interactive | — |

The pane is dimmed but not greyed to unreadability, its border loses the focus
colour, and input returns `precondition_failed` with the reason — never silently
dropped.

---

## 9. Capabilities

Declared in the [doc 03](03-capability-catalog.md) style. Roles: `V`iewer <
`O`perator < `A`dmin. All layout mutations are `Operator`, per
[D2](decisions.md#d2--remote-is-exactly-equivalent-to-local); `Viewer` is the
share-a-read-only-link role, not a degraded device.

Every input below carries an optional `view: Option<ViewId>` defaulting to the
caller's current view, and every layout mutation emits `LayoutChanged { view,
layout, geometry_hint }`.

```rust
capability! {
    /// Split a pane, creating a session in the new pane when none is given.
    name  = "pane.split",
    group = "pane", verb = "split",
    kind  = Command,
    role  = Role::Operator,
    input  = PaneSplit {
        pane: PaneId,
        dir: Direction2D,
        ratio: Option<f32>,
        session: Option<SessionId>,
        top_level: bool,
        view: Option<ViewId>,
    },
    output = PaneSplitAck { pane: PaneId, session: SessionId, layout: LayoutTree },
    effects = [Effects::SPAWNS_PROCESS],   // only when `session` is None
}
```

### 9.1 `pane.*`

| Capability | Role | Input | Output | Effects |
|---|---|---|---|---|
| `pane.list` | V | `{ workspace, view? }` | `{ panes: [PaneInfo] }` | |
| `pane.split` | O | above | above | `SPAWNS_PROCESS` |
| `pane.close` | O | `{ pane, close_session: bool }` | `{ focus: Option<PaneId> }` | `DESTRUCTIVE` when `close_session` |
| `pane.focus` | O | `{ pane }` | `{ focused: PaneId }` | |
| `pane.navigate` | O | `{ from, dir, wrap: bool }` | `{ focused: Option<PaneId> }` | |
| `pane.focus_last` | O | `{ view? }` | `{ focused: Option<PaneId> }` | |
| `pane.focus_index` | O | `{ index: u16 }` | `{ focused: Option<PaneId> }` | |
| `pane.focus_cycle` | O | `{ reverse: bool }` | `{ focused: Option<PaneId> }` | |
| `pane.focus_edge` | O | `{ dir }` | `{ focused: Option<PaneId> }` | |
| `pane.resize` | O | `{ target: ResizeTarget, amount: ResizeAmount }` | `{ applied: f32, clamped: bool, layout }` | |
| `pane.move` | O | `{ pane, to, dir }` | `{ layout }` | |
| `pane.move_to_workspace` | O | `{ pane, workspace, view? }` | `{ pane }` | |
| `pane.swap` | O | `{ a, b }` | `{ layout }` | |
| `pane.rotate` | O | `{ split: Option<SplitId>, reverse: bool }` | `{ layout }` | |
| `pane.zoom` | O | `{ pane, zoomed: bool }` | `{ layout }` | |
| `pane.set_session` | O | `{ pane, session }` | `PaneInfo` | |
| `pane.mark` | O | `{ pane, mark: Option<MarkId> }` | `{ marks }` | |
| `pane.stack.create` | O | `{ panes: [PaneId] }` | `{ stack, layout }` | |
| `pane.stack.expand` | O | `{ pane }` | `{ layout }` | |
| `pane.stack.move` | O | `{ pane, dir }` | `{ layout }` | |
| `pane.stack.break_out` | O | `{ pane }` | `{ layout }` | |
| `pane.float` | O | `{ pane, floating: bool, rect: Option<FractionalRect> }` | `{ layout }` | |
| `pane.scroll` | V | `{ pane, to }` | `{ view }` | per-client |
| `pane.select` | V | `{ pane, anchor, head, mode }` | `{ text_len }` | per-client |

### 9.2 `layout.*`

| Capability | Role | Input | Output | Effects |
|---|---|---|---|---|
| `layout.get` | V | `{ workspace, view?, area?: Rect }` | `{ layout, geometry?: Geometry, degraded }` | |
| `layout.set` | O | `{ workspace, view?, layout: LayoutSpec }` | `{ layout }` | |
| `layout.preset` | O | `{ view?, preset: LayoutPreset }` | `{ layout }` | |
| `layout.balance` | O | `{ split: Option<SplitId>, recursive: bool }` | `{ layout }` | |
| `layout.floats.toggle` | O | `{ view?, visible: Option<bool> }` | `{ layout }` | |
| `layout.synchronize` | O | `{ view?, panes: Option<[PaneId]> }` | `{ group: Option<SyncGroup> }` | `WRITES_PTY`, `DESTRUCTIVE` |
| `layout.views.list` | V | `{ workspace }` | `{ views: [ViewInfo] }` | |
| `layout.views.create` | O | `{ workspace, name, from: Option<ViewId> }` | `ViewInfo` | |
| `layout.views.close` | O | `{ view }` | `Ack` | refuses `Primary` |
| `layout.views.select` | O | `{ view, pin: bool }` | `ViewInfo` | per-client |
| `layout.promote` | O | `{ view }` | `{ layout }` | mutates `Primary`, visible to all |
| `layout.adopt` | O | `{ from: ViewId }` | `{ layout }` | |
| `layout.rearm` | O | `{ view? }` | `Ack` | re-enable responsive swapping (§4.2) |
| `layout.save` | O | `{ name, view?, scope: User \| Project }` | `{ path }` | `WRITES_FS` |
| `layout.apply_saved` | O | `{ name, view? }` | `{ layout }` | `SPAWNS_PROCESS` |
| `layout.list_saved` | V | `{}` | `{ layouts: [SavedLayoutInfo] }` | |
| `layout.import_tmux` | O | `{ string, view? }` | `{ layout, warnings }` | |

One capability outside this group is specified here because this document owns
its semantics (§3.4):

| Capability | Role | Input | Output | Effects |
|---|---|---|---|---|
| `session.size_policy` | O | `{ session, policy: SizePolicy }` | `SessionSizing` | resizes the PTY |

It belongs to the `session` group, not `layout` — its prefix is authoritative for
naming, catalog registration and CLI shape ([05 §10.2](05-session-model.md#102-session)).
It rejects `native` sessions (§3.7).

### 9.3 Reconciliation with the existing catalog

[05 §10.3](05-session-model.md#103-pane) and
[03 §6](03-capability-catalog.md#6-capability-groups-initial-surface) already
sketch this surface. Differences, all listed in §12:

- Doc 03 lists `pane.layout.get`; this document puts layout operations in a
  `layout.*` group, because they act on a *view*, not a pane, and because the
  CLI reads better (`omt layout preset tiled`). `pane.layout.get` should be
  dropped in favour of `layout.get`.
- Doc 05 lists `workspace.layout.get` / `.set` / `.preset`. Same objection:
  those become `layout.*` with a `workspace` in the input. Aliases are kept for
  two minor versions per doc 03 §7.
- Doc 05's `pane.resize { pane, edge, delta }` becomes the richer
  `ResizeTarget`/`ResizeAmount` pair; `{ pane, edge, delta }` maps onto
  `Edge` + `Fraction` exactly, so the old shape is an accepted input variant.

### 9.4 Events

| Event | When |
|---|---|
| `LayoutChanged { view, layout }` | any tiling, float, stack or zoom mutation |
| `ViewCreated` / `ViewClosed` / `ViewSelected` | §3.3 |
| `FocusChanged { view, pane, actor }` | focus moves |
| `SessionResized { session, size, reason }` | `reason: WriterChanged \| ViewportChanged \| PolicyChanged \| Pinned` |
| `SizePolicyChanged { session, policy, by }` | §3.4 |
| `PaneHidden` / `PaneShown` | a pane enters or leaves `Geometry::hidden` for a given client |
| `SyncGroupChanged` | §5.4 |

Per [03 §4](03-capability-catalog.md#4-events-are-the-read-side-twin) these are
derived from state changes, never published by hand. `SessionResized` carrying a
`reason` is what lets a client say "the phone took over, so this resized" rather
than showing an unexplained redraw.

---

## 10. Persistence and restore

Layout is in doc 05 §8.1's **snapshot** class — small, structural, must be
exactly right, debounced 500 ms.

**What is persisted per workspace:**

- `views` with `ViewKind::{Primary, Named}` — their `LayoutTree`, floats
  (`Terminal` only; `Overlay` floats are transient by definition), stacks, zoom,
  focus, `synchronize` group, and named marks.
- Per-pane: `PaneId`, its `SessionId`, `priority` and `min` overrides.
- Per `pty` session only: `SessionSizing::{current, policy}`, so a `Pinned` session comes
  back pinned and an `Orphaned` session's content is readable at the width it
  was written. A `native` session has no sizing to persist (§3.7).

**What is not:**

- `ViewKind::Adaptive` views. They are a function of a client's viewport and are
  rebuilt on attach. Persisting them would restore a phone-shaped layout for a
  laptop.
- `Overlay` floats, per doc 15 §7.1's "does not exist until toggled".
- `Geometry`. Derived, always.
- Per-actor marks (`HashMap<ActorId, PaneId>`) — an actor's mark does not
  outlive the daemon; named marks do.

**Restore.** Per doc 05 §8.2 the tree comes back intact and its sessions come
back `Orphaned`. The layout must therefore tolerate panes whose sessions are
dead, which it does: §8's dead-session rendering is a normal state, not an
error, and `Restart` re-spawns into the same pane. A pane whose session cannot
be restored at all (its record was quarantined by a `Partial` restore) is
**kept in the tree** with a `SessionUnavailable` placeholder rather than being
silently removed — deleting a user's pane because a log record was torn is
exactly the "never silently drop data" violation doc 05 §8.2 forbids.

**Versioning.** `LayoutTree`, `LayoutSpec` and the persisted `Layout` carry
`format_version` from v1, with fixtures in `tests/fixtures/store/v1/` loaded by
CI (doc 05 §8.3). `LayoutSpec` is additionally the *wire* type for
`layout.get`/`layout.set`, so it follows doc 03 §7's compatibility rules:
optional fields may be added, required ones may not.

---

## 11. Testing

Following doc 05 §12's style — deterministic, no runtime, no clock.

**Property tests** (`proptest`) over an arbitrary operation sequence drawn from
`{split, close, resize, move, swap, rotate, zoom, unzoom, preset, balance,
stack, unstack, float, unfloat}` applied to an arbitrary starting tree, with
`check_invariants()` after each:

| P1 | Every split has ≥ 2 children (N1). |
| P2 | No split has a child split of the same axis (N2). |
| P3 | Every split's weights are finite, `> MIN_WEIGHT`, and sum to 1.0 ± `WEIGHT_EPSILON` (N3). |
| P4 | Every `PaneId` appears exactly once across `tiles`, `floats` and `stacks`. |
| P5 | `Empty` appears only as the root (N4). |
| P6 | Every `Pane::session` names a live entry in `Instance::sessions`. |
| P7 | `close` never removes a session, only a pane. |
| P8 | `focus`, when `Some`, names a pane in this view. |

**Geometry properties**, over an arbitrary tree × arbitrary `(area, constraints)`:

| G1 | Placements are pairwise non-overlapping. |
| G2 | Placements plus dividers exactly tile `area` — no gap, no overhang. |
| G3 | No placed rect is smaller than `min`; every dropped pane is in `hidden`. |
| G4 | `panes ∪ hidden` equals the tree's pane set exactly. |
| G5 | No rect has zero or negative extent. |
| G6 | Every placed extent is within 1 cell of `usable * weight` (§2.2). |
| G7 | `compute` is deterministic: same inputs, same output, byte for byte. |
| G8 | Monotonicity: growing `area` never *reduces* a pane's extent and never moves a pane from `panes` into `hidden`. |

G8 is the one that catches real bugs — a remainder rule that is not monotone
produces a pane that shrinks as you enlarge the window, which is the classic
resize artifact.

**Resize properties:**

| R1 | `resize` changes exactly two weights, and their sum is unchanged. |
| R2 | Every other split's weights are bit-identical before and after. |
| R3 | `resize(d)` then `resize(-d)` restores the tree exactly, when neither clamped. |
| R4 | Clamping is reported (`clamped: true`) whenever `applied != requested`. |
| R5 | A resize never produces a tree that fails `check_invariants`. |

**Golden tests** for presets: for each of `{Solo, EvenColumns, EvenRows,
MainColumn, MainRow, Tiled, Stacked}` × pane counts 1..9 ×
`{200x50, 120x40, 80x24, 52x24, 40x16}`, a checked-in snapshot of the resulting
`Geometry` rendered as ASCII art. These are read by humans in review and are the
cheapest possible regression net for "the tiled layout looks wrong at 7 panes".

**Import corpus:** a directory of real tmux layout strings (including ones with
floating suffixes, deep nesting, and deliberately corrupt checksums), each with
the expected `LayoutTree` or the expected error. §4.3's re-proportioning is
asserted to reproduce the original absolute cells when recomputed at the
original size, within the ±1 cell of G6.

**Multi-client scenarios**, driven by doc 05 §12's deterministic simulation with
a virtual clock:

1. Laptop 200×50 attaches with 4 panes; phone 52×24 attaches → assert the phone
   is placed in an `Adaptive` view, the primary layout is unchanged, and **no
   `SessionResized` is emitted**.
2. Phone requests the writer token → assert the 20 % warning fires, and that
   accepting emits exactly one `SessionResized { reason: WriterChanged }` after
   the debounce, not one per intermediate viewport report.
3. Phone rotates portrait→landscape→portrait within the debounce window →
   assert zero PTY resizes.
4. Policy set to `Smallest` with three clients attached, one detaching → assert
   the size follows the new minimum and that a detach during the debounce
   coalesces.
5. Two clients drag the same divider concurrently → assert both deltas apply in
   `seq` order and the result is the composition, not last-write-wins (weights
   are commutative under addition, which is why `Divider` deltas are the right
   wire form).
6. A client resizes a divider that another client just deleted → assert
   `UnknownSplit`, not a panic and not a silent no-op.
7. `layout.promote` from a phone → assert every primary-view client receives one
   `LayoutChanged` and that a laptop's zoom state is replaced, not merged.
8. Daemon restart mid-drag → assert the persisted layout is the last committed
   state, never a partial drag.

---

## 12. Open questions and contradictions with existing docs

### 12.1 Contradictions requiring a decision by the doc owners

**C1 — Sizing vocabulary in doc 05.** The substantive default is settled: doc 05
§2.2 already delegates the negotiation to
[07 §4.3](07-remote-protocol.md#43-the-resize-problem) with `SizeOwner::Writer`
as the default, which is what §3.4 above calls `SizePolicy::Driver`. What
remains is vocabulary: doc 05's **invariant 10** and **open question 2** still
describe sizing in the old "minimum over participants" framing and must adopt
`SizePolicy` / `Participation` terms so the two documents read as one model.
Doc 07's `SizeOwner` should likewise gain the `Driver` naming for consistency.

**C1b — `writer.acquire { keep_size }` and the 20 % takeover warning need
ratifying in [12 §3.3](12-collaboration.md#33-lifecycle).** §3.4 consequence 1
describes both, but they are *acquisition* semantics, and by doc 05's own
ownership rule (05 §5) doc 12 owns those. Doc 12 must declare the `keep_size`
flag on `writer.acquire`, its `SizePolicy::Pinned` transition, and the 20 %
change threshold that triggers the warning — or state a different number, which
this document then follows.

**C2 — Zoom scope: doc 05 §2.1 vs doc 05 §13 Q5.** Doc 05 §2.1 states *"Zoom is
non-destructive and per-workspace, not per-client"* and then §13 open question 5
proposes the opposite. §5.1 above decides **per view**, which is neither exactly
— it is per *arrangement*, so two clients sharing the primary view share the
zoom, and a phone in its own view does not force it on anyone. Doc 05 §2.1 needs
the sentence changed and §13 Q5 can be closed.

**C3 — "BSP tree" wording. Resolved, deliberately partially.** Doc 05 §1 and §2
called the layout a BSP tree while defining an n-ary
`children: Vec<(f32, Layout)>`. BSP means binary, so doc 05 §2's opening
sentence now says "an n-ary split tree (historically 'BSP')". The **heading
itself is retained** — four links in this document target
`05-session-model.md#2-layout-the-bsp-tree`, and link stability is worth more
than the word. Doc 05 §1's object-model diagram also says
`Layout: a BSP tree of panes, owned by a Workspace` — with §3's per-view model
that becomes "one or more `LayoutView`s, owned by a Workspace".

**C4 — Capability naming and grouping.** Three inconsistent names exist for the
same operations:
- doc 05 §10.1 `workspace.layout.get/set/preset`;
- doc 03 §6 `pane.layout.get`;
- this document `layout.get/set/preset`.
Similarly, doc 05 §10.3 has `pane.navigate` while
[doc 16 §11](16-input-and-keymap.md) binds `pane.focus_direction`. One name each
must win before any of these ship; §9 proposes `layout.*` and `pane.navigate`.

**C5 — `Presence.viewing` is `PaneId`-shaped.** Doc 05 §6 has
`viewing: SmallVec<[PaneId; 4]>`. With per-view layouts a `PaneId` is meaningless
to a client in a different view, so a laptop cannot render "the phone is watching
this". It must become `viewing: SmallVec<[(ViewId, SessionId); 4]>` — the
`SessionId` is the shared identity and is what the UI actually wants to
highlight. Same change to `ClientView::panes` in doc 05 §4.

**C6 — Doc 05's `Layout` enum has a `Zoom` variant.** `Zoom { pane, saved:
Box<Layout> }` makes zoom a tree transformation (tmux's design) with the costs
described in §1.3 and §5.1. This document makes it `Option<PaneId>` beside the
tree. The variant should be removed.

**C7 — Split direction vocabulary.** Doc 05 uses `Direction { Horizontal,
Vertical }` with a comment clarifying which is which; doc 10 §9.1's launch-config
YAML uses `split: horizontal | vertical`. §1.2 argues for `Axis { Columns,
Rows }`. The type should change; the YAML must keep accepting the old spellings
(there is a corpus) with `columns`/`rows` canonical on write.

**C8 — `pane.split` effects.** Doc 05 §10.3 declares no effects for
`pane.split`, but it spawns a process when `session` is omitted. It must declare
`SPAWNS_PROCESS`, conditionally, or the mobile confirm rules and the audit log
are wrong. Doc 03's `effects` model has no notion of a conditional effect —
either it gains one, or `pane.split` always declares it.

**C9 — Doc 15's TUI panel mechanism.** Doc 15 §7.1 describes the explorer panel
as a side panel that "does not exist until toggled". §5.2 above proposes it be a
`FloatKind::Overlay` float, so there is one overlay system rather than two. Doc
15's owner should confirm; if the panel must reflow the tiling (pushing panes
aside rather than overlaying them), that is a different feature and needs its own
type.

### 12.2 Open questions

**Q1 — Should `Adaptive` views be on by default?** §3.3 says yes. The cost is
that two people on two devices see different arrangements and may talk past each
other ("the pane on the left"). The alternative — always share, always degrade
in place — is simpler to explain and worse to use. Needs a decision informed by
actual two-device use; the config flag (`layout.adaptive_views`) exists either
way so this is reversible.

**Q2 — Does `Primary` need to be per-workspace or per-instance?** Currently
per-workspace. A user with eight workspaces open on a phone gets eight
`Adaptive` views. Cheap (they hold no sessions of their own) but the accounting
is noisy. An instance-level "this client's mobile view" is the alternative and
is closer to zellij's single mobile tab.

**Q3 — The web client's unstable attach viewport.** zellij's `MobileRenderGate`
blanks a client until its reported size matches the size actually painted, because
browsers report, reflow, and report again
([research §3.2](../research/multiplexers.md#32-zellij--smallest-of-active-plus-a-genuine-mobile-mode)).
omt will hit this. Is the gate a server concern (as in zellij) or purely a web
client concern? It affects whether `session.attach` needs a "size settled"
handshake. Needs a call with [07](07-remote-protocol.md) and
[08](08-web-client.md).

**Q4 — Stack minimum on a phone.** §5.3 drops trailing stack members into
`hidden` when they do not fit. At 24 rows a stack of 8 has 7 title rows and 17
content rows, which is workable; at 16 rows it is not. Should a stack that does
not fit degrade to `Solo` plus the pane strip (§7.1) instead of dropping
members? Probably yes, but it needs the strip to exist first.

**Q5 — Divider drag arbitration.** §11 scenario 5 asserts concurrent divider
drags compose. That is true of the arithmetic but may be wrong as UX — two people
dragging one border fight. Should a drag take a short-lived lease on a
`SplitId`, analogous to the writer token but much lighter? Related to
[12 — Collaboration](12-collaboration.md); a lease is easy but adds a concept.

**Q6 — `priority` in launch configs.** §4.2 adds `priority: i8` to `PaneSpec` to
drive §2.3's drop order. That is a new field in a format doc 10 already
specifies. Confirm with doc 10's owner, and decide whether the default should be
derived (focused pane and agent panes implicitly higher) rather than authored.

**Q7 — Does `pane.move_to_workspace` need a session-move counterpart?** §5.4
moves the pane and leaves `Session::workspace` fixed, producing "foreign" panes.
Doc 05 §7 makes workspace identity a canonical path and doc 05 §1.2 says a
session's identity must not change when it is moved between workspaces — which
implies a session *can* be moved. If so, `session.move_to_workspace` should
exist and this document does not define it.

**Q8 — Interaction with `session.resize`.** Doc 05 §10.2 exposes
`session.resize { session, cols, rows }` returning a negotiated size. With §3.4's
policy model, what does an explicit `session.resize` mean — install `Pinned`, or
a one-shot override that the next policy evaluation undoes? Lean: it installs
`Pinned { by: caller }`, because a one-shot that silently reverts is a bad
capability. Needs confirming, and doc 05's entry updated either way.
