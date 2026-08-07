# Research — Multiplexer Pane and Layout Systems

Prior art for [17 — Panes and layout](../architecture/17-panes-and-layout.md).
Read for **interface facts** — data-structure shapes, algorithms, option
semantics, file formats. Per [P9](../architecture/01-principles.md#p9--clean-room-with-respect-to-studied-code)
and [14 — Licensing](../architecture/14-licensing.md), no code from any of these
is copied or translated; the algorithms below are described in prose so they can
be reimplemented.

Sources, with licence and how they were consulted:

| Source | Licence | How consulted |
|---|---|---|
| `tmux/tmux` | ISC | **Source read** — `.research/tmux/`, HEAD at clone time (files dated 2026-07) |
| `zellij-org/zellij` | MIT | **Source read** — `.research/zellij/` |
| `wez/wezterm` | MIT | **Source read** — `.research/wezterm/mux/src/tab.rs` |
| `other terminals/other terminals` | Apache-2.0 | **Source read** — `.research/other terminals/src/layout.rs` |
| other terminals | AGPL-3.0 | **Source read, interfaces only.** Never copied. See [other terminals.md] |
| kitty | GPL-3.0 | **Docs only** — <https://sw.kovidgoyal.net/kitty/layouts/>. No source read |

Every claim below is tagged **[verified]** (I read the code and can point at the
function), **[docs]** (read in official documentation or an options table), or
**[inferred]** (my reading of behaviour that the code implies but that I did not
execute).

---

## 1. Layout data structures

### 1.1 tmux — an n-ary cell tree carrying *absolute* geometry

`.research/tmux/tmux.h:1571` **[verified]**

```c
struct layout_cell {
 enum layout_type type; /* LEFTRIGHT | TOPBOTTOM | WINDOWPANE */
 int flags; /* LAYOUT_CELL_FLOATING 0x1 */
 struct layout_cell *parent;
 struct layout_geometry g; /* { u_int sx, sy; int xoff, yoff; } */
 struct layout_geometry fg; /* saved geometry for a floating cell */
 struct window_pane *wp; /* set iff type == WINDOWPANE */
 struct layout_cells cells; /* TAILQ of children */
 TAILQ_ENTRY(layout_cell) entry;
};
```

Four facts that matter more than the shape:

1. **It is n-ary, not binary.** `cells` is a queue, so `LEFTRIGHT` with five
 children is one node. This is what makes `layout_spread_cell` (§2.5) a local
 operation rather than a tree rebalance.
2. **Geometry is absolute and stored, not derived.** `sx/sy/xoff/yoff` are cell
 counts. There are no ratios anywhere in tmux's layout engine. Every operation
 mutates integers directly, and `layout_fix_offsets` recomputes offsets from
 sizes afterwards. **This is the single biggest divergence from every Rust
 multiplexer**, and it is why tmux resize behaviour is so hard to predict
 from the outside: proportions are an emergent property of a sequence of
 integer adjustments, not a stored invariant.
3. **Borders are part of the arithmetic.** `layout_fix_offsets1`
 (`layout.c:325`) advances `xoff += lcchild->g.sx + 1` — the `+1` *is* the
 border column. A parent of width `sx` with `n` children satisfies
 `sum(child.sx) + (n-1) == sx`. `layout_check` (`layout-custom.c:137`)
 asserts exactly this when parsing a layout string. **[verified]**
4. **The invariant "no child has the same type as its parent" is not enforced
 in tmux** the way doc 05 proposes for omt. `layout_split_pane` reuses the
 parent when types match (`layout.c:1337` ff.) so in practice it holds, but
 `layout_resize_check`'s `lc->type == type` branch (below) is written to cope
 either way.

`PANE_MINIMUM` is the floor. Cells are also flagged floating; a floating cell is
excluded from tiling arithmetic everywhere via `layout_cell_is_tiled` /
`layout_cell_has_tiled_child` guards. **[verified]**

### 1.2 zellij — **no tree at all**; a flat pane set plus a constraint solver

This was the most surprising finding, and the most useful one.

`.research/zellij/zellij-utils/src/pane_size.rs` **[verified]**

```rust
pub struct PaneGeom {
 pub x: usize, pub y: usize,
 pub rows: Dimension, pub cols: Dimension,
 pub stacked: Option<usize>, // stack id
 pub is_pinned: bool, // floating only
 pub logical_position: Option<usize>, // for layout placement
}

pub struct Dimension { pub constraint: Constraint, inner: usize }
pub enum Constraint { Fixed(usize), Percent(f64) }
```

A tab holds `HashMap<PaneId, Box<dyn Pane>>`, each with a `PaneGeom`. There is
no parent/child relationship in the runtime model. Structure is *recovered
geometrically* when needed:

- `PaneResizer::grid_boundaries(direction)`
 (`zellij-server/src/panes/tiled_panes/pane_resizer.rs:226`) collects the
 perpendicular spans' end edges, sorts and dedups them, and produces
 `(last_edge, next_edge)` bands. **[verified]**
- `spans_in_boundary` then selects the panes whose perpendicular extent falls in
 a band, giving a row (or column) of panes that must share `space`.

So zellij reconstructs "these five panes form a column" from coordinates on
every resize. The layout *file* format (§4.2) is a tree, but it is a
constructor, not the runtime representation.

`Dimension` is the other half of the idea: a pane's size carries its
*constraint* (`Fixed` or `Percent`) alongside its resolved `inner` cell count.
`adjust_inner(full_size)` recomputes `inner = floor(percent/100 * full_size)`
and returns the fractional leftover, so rounding error is a first-class return
value rather than something swallowed. **[verified]**

### 1.3 other terminals — a strict binary BSP tree with `f32` ratios

`.research/other terminals/src/layout.rs:73` **[verified]**

```rust
pub enum Node {
 Pane(PaneId),
 Split { direction: Direction, ratio: f32, first: Box<Node>, second: Box<Node> },
}
pub struct TileLayout { root: Node, focus: PaneId }
```

Geometry is *never stored*. `TileLayout::panes(area) -> Vec<PaneInfo>` walks the
tree with `split_rect(area, direction, ratio)` on every render
(`collect_panes`, `layout.rs:413`). `splits(area) -> Vec<SplitBorder>` produces
divider positions for mouse drag, each carrying a `path: Vec<bool>` (false =
first child) so a drag can address a split node by path. **[verified]**

This is the cheapest possible design and it is genuinely good: the tree is tiny,
pure, trivially serializable, and every geometry question is a pure function of
`(tree, area)`. Its two costs are real though:

- **Binary means three equal columns is `Split(a, Split(b, c))` with ratios
 1/3 and 1/2.** Closing `b` leaves `a | c` at 1/3 : 2/3, not 1/2 : 1/2. This is
 exactly the failure doc 05 §2 calls out.
- **`ratio` is clamped to `0.1..=0.9`** (`set_ratio_at`, `layout.rs:210`) rather
 than enforcing a cell-count minimum, so on a 40-column phone a 0.1 ratio is
 4 columns — under any usable minimum. Minimum sizes are a *display*-time
 concern elsewhere, not a layout-time one.

### 1.4 wezterm — binary tree, absolute sizes, in the wire protocol

`.research/wezterm/mux/src/tab.rs:2103` **[verified]**

```rust
pub enum PaneNode {
 Empty,
 Split { left: Box<PaneNode>, right: Box<PaneNode>, node: SplitDirectionAndSize },
 Leaf(PaneEntry),
}
pub struct SplitDirectionAndSize { direction: SplitDirection, first: TerminalSize, second: TerminalSize }
```

with a runtime `bintree::Tree<PaneEntry, SplitDirectionAndSize>`. Note the
comment in the source: *"This type is used directly by the codec, take care to
bump the codec version if you change this"* — wezterm ships the layout tree over
its mux protocol as a versioned type. That is the right instinct and omt should
copy the *discipline* (not the type).

Splits are requested with a richer intent than tmux's:

```rust
pub struct SplitRequest {
 direction: SplitDirection,
 target_is_second: bool, // new pane goes right/bottom?
 top_level: bool, // split the whole tab, not the active pane
 size: SplitSize, // Cells(usize) | Percent(u8), default Percent(50)
}
```

`top_level` is the analogue of tmux's `SPAWN_FULLSIZE`, and `target_is_second`
of `SPAWN_BEFORE`. Both are worth having. **[verified]**

Sizes are stored absolutely per side (`first`/`second: TerminalSize`), and
window resize is handled by `adjust_x_size` / `adjust_y_size`
(`tab.rs:338`/`408`) which walk the tree pushing ±1 cell at a time — the same
"nudge by one until the change is consumed" shape as tmux's
`layout_resize_adjust`. **[verified]**

### 1.5 other terminals — n-ary flex tree (interfaces only, no code read into the design)

`.research/other terminals/app/src/pane_group/tree.rs:110` **[verified, interface only]**

```rust
pub enum PaneNode { Branch(PaneBranch), Leaf(PaneId) }
pub struct PaneBranch { axis: SplitDirection, nodes: Vec<(PaneFlex, PaneNode)>, dividers: Vec<Divider> }
pub struct PaneFlex(pub f32);
```

An n-ary branch of `(flex, node)` pairs — structurally the same shape doc 05
already proposes for omt, arrived at independently by a GUI terminal. Removal
returns `BranchRemoveResult::{NotFound, Removed, Collapse(PaneNode)}` — an
explicit "this branch is now single-child, hoist me" signal, which is a clean
way to express the normalization step. **[verified]**

other terminals also carries a `HiddenPane` concept with reasons
(`HiddenPaneReason::{Move, Job, TemporaryReplacement, Close, ChildAgent}`) —
panes that exist in the model but are not laid out. Relevant to omt because
"agent spawned a child pane you can reveal from its status card" is a shape omt
will want in [06](../architecture/06-agent-layer.md). **[verified]**

### 1.6 kitty — named algorithms, only one of which nests

**[docs]** kitty has no general layout tree. It has seven *layouts*:
`stack` (one window, rest hidden), `tall` (N full-height left, rest tiled
right), `fat` (transpose of tall), `grid` (balanced grid, last column short),
`horizontal`, `vertical`, and `splits`. Only `splits` supports arbitrary
nesting; the rest are functions from an ordered window list to geometry.
`tall`/`fat` take `bias` (10–90), `full_size` (count of full-span windows) and
`mirrored`. `splits` takes `equalize_on_close` and `split_axis`.

The interesting design point: because six of seven layouts are *pure functions
of an ordered list*, kitty gets "reflow onto a phone-shaped screen" almost free —
change the layout name, not the data. omt's preset system should preserve that
property (see design §4).

---

## 2. The resize algorithm

### 2.1 tmux — the canonical prior art, explained

Four functions do all the work. All in `.research/tmux/layout.c`. **[verified]**

**`layout_resize_check(w, lc, type) -> u_int`** (`:532`) — "how much can this
cell shrink along `type` before something hits `PANE_MINIMUM`?" Three cases:

- Leaf: `available = (dimension along type) - minimum`, saturating at 0. The
 minimum is `PANE_MINIMUM`, plus the scrollbar width when scrollbars are
 `always`, plus 1 when the cell needs a horizontal border for a pane status
 line (`layout_add_horizontal_border`).
- Node whose type **equals** the resize axis: **sum** over children. Shrinking a
 row of columns can take slack from any of them.
- Node whose type **differs**: **minimum** over children. Shrinking a column of
 rows horizontally means every row must shrink by that amount, so the tightest
 child governs.

That sum/min duality is the whole insight, and it is what any correct
implementation needs, whatever its data structure.

**`layout_resize_adjust(w, lc, type, change)`** (`:586`) — applies a change that
the caller has *already bounded* by `layout_resize_check`. It adds `change` to
the cell's own dimension, then recurses:

- Different axis: pass the *same* `change` to every tiled child (they all span
 the parent along this axis).
- Same axis: distribute. The loop is deliberately dumb — repeatedly walk the
 children handing out **±1 cell at a time**, skipping children with no slack
 (`layout_resize_check(child) == 0` when shrinking), until `change` reaches 0
 or a full pass changes nothing. Growth never checks slack, so growth always
 succeeds; shrink stops when everyone is at minimum.

Two consequences worth internalizing:

- **Remainder distribution is round-robin by construction.** Handing out one
 cell at a time in child order means a 7-cell surplus over 3 children goes
 3/2/2, favouring earlier children. There is no rounding step and no
 accumulated error, because there are no fractions. This is a real advantage of
 the integer model.
- **It is O(change × children)** per call. Fine at terminal scale, and it makes
 the "cannot shrink further" case fall out naturally rather than needing a
 special path.

**`layout_fix_offsets` / `layout_fix_offsets1`** (`:359`, `:325`) — after any
size change, offsets are recomputed top-down: children of a `LEFTRIGHT` node get
`xoff = running`, `yoff = parent.yoff`, and `running += child.sx + 1`. Sizes are
authoritative; offsets are always derived. Floating and non-tiled cells are
skipped so they do not consume band space. **[verified]**

**`layout_fix_panes`** (`:443`) then pushes each cell's geometry onto its
`window_pane` (and issues the actual PTY resize).

### 2.2 tmux — the three entry points

**Container (window) resize — `layout_resize(w, sx, sy)`** (`:787`):
compute `xchange = sx - root.sx`; compute `xlimit = layout_resize_check(root, LEFTRIGHT)`;
clamp a shrink to `-xlimit`. Then the subtle part, which the source comments at
length: if `xlimit == 0` the layout is already at its minimum, so a shrink is
refused entirely (`xchange = 0`) and the *window* ends up smaller than the
layout. Growing from that state uses the raw difference rather than a
proportional spread, so the layout snaps back to exactly fitting the window.
Same again vertically, then `fix_offsets` + `fix_panes`. **[verified]**

The user-visible consequence — and this is the behaviour omt must decide whether
to reproduce — is that **tmux lets the layout be larger than the window**, and
`resize_window` (`resize.c:44`) then clamps the *window* up to the layout size
(`if (sx < w->layout_root->g.sx) sx = w->layout_root->g.sx;`). The client that
is too small sees a cropped view with the rest off-screen. That is where tmux's
famous "half the screen is dashes" comes from. **[verified]**

**Manual resize — `layout_resize_pane(wp, type, change, opposite)`** (`:974`):

1. Walk up from the pane's cell to the nearest ancestor whose type matches the
 resize axis. If none, the resize is a no-op (you cannot widen a pane in a
 window with no vertical divider).
2. **If the found cell is the last child, step back one.** This is the answer to
 "what does dragging a border actually do": tmux does not resize a *pane*, it
 moves *the border after cell `lc`*. The last cell has no border after it, so
 the operation is rewritten as moving the previous border.
3. `layout_resize_layout` then loops calling `layout_resize_pane_grow` or
 `_shrink` until the requested change is consumed or no progress is made.

**`layout_resize_pane_grow`** (`:1000`): add to `lc`; find the donor by walking
**towards the tail** for the first sibling with `layout_resize_check > 0`; if
none and `opposite` is set, walk towards the head. Transfer
`min(donor_slack, needed)`. **`_shrink`** (`:1041`): walk **towards the head**
from `lc` for a cell that can give up space, and add to `TAILQ_NEXT(lc)`.

So: **dragging a border moves exactly two cells — the pair on either side of
that border — and everything else in the tree is untouched.** Only when the
immediate neighbour is already at minimum does the effect propagate to the next
sibling along. Panes in other branches never move. This is the behaviour users
expect and it is much better than a "renormalize all ratios" approach, which
makes an unrelated pane on the far side of the screen twitch.

**Absolute resize — `layout_resize_pane_to`** (`:841`) converts a target size to
a delta, with the same last-child inversion (`change = size - new_size` when
last, else `new_size - size`). **[verified]**

### 2.3 tmux — proportional redistribution and rounding

`layout_new_pane_size(w, previous, lc, type, size, count_left, size_left)`
(`:1085`) is the only place tmux does proportional arithmetic. **[verified]**

```
if count_left == 1: return size_left /* last child absorbs everything */
new_size = (lc.dim * size) / previous /* integer division, truncates */
min = max((PANE_MINIMUM+1)*(count_left-1), lc.dim - resize_check(lc))
max = size_left - min
clamp new_size into [PANE_MINIMUM, max]
```

**The rounding policy is: truncate everyone, and give the last child the
remainder.** `layout_resize_child_cells` (`:1180`) drives it, tracking
`available -= child_size + 1` as it goes so the border columns are accounted.
The last child therefore absorbs up to `n-1` cells of accumulated truncation —
biased, but total and never off by more than the child count. `layout_set_size_check`
(`:1123`) runs the identical arithmetic in a dry run and returns 0 if any child
would end below `PANE_MINIMUM`, so the caller can refuse rather than produce a
broken layout.

**`layout_spread_cell(w, parent)`** (`:1520`) — the "balance" primitive:

```
size = parent.sx (or parent.sy, minus 1 if a horizontal border is needed)
each = (size - (number - 1)) / number
remainder = size - (number * (each + 1)) + 1
```

then walk children assigning `each`, incrementing by one while `remainder > 0`.
**Remainder goes to the first `remainder` children**, in order. `layout_spread_out`
(`:1587`) walks up from a pane trying each ancestor until one reports a change.
Returns 0 (no change) when `each == 0`, i.e. it refuses to balance rather than
producing sub-minimum panes. **[verified]**

### 2.4 tmux — split

`layout_split_check_space` (`:1265`) rejects a split unless the cell has
`2*PANE_MINIMUM + 1` along the axis (+1 more when a pane-status border is
needed, or scrollbar width when scrollbars are always on). `layout_split_sizes`
(`:1305`):

```
ss = cell size along axis
s2 = (size < 0) ? ((ss+1)/2) - 1 : (before ? ss - size - 1 : size)
clamp s2 into [PANE_MINIMUM, ss-2]
s1 = ss - 1 - s2
```

The `-1` throughout is the new border. A 50/50 split of an 80-column cell is
therefore 40 | *border* | 39, not 40/40. **[verified]**

### 2.5 tmux — close

`layout_destroy_cell` (`:708`) **[verified]**:

1. `layout_cell_get_neighbour(lc)` picks the recipient: **the next sibling,
 falling back to the previous** (`layout_cell_get_neighbour_direction` skips
 non-tiled cells). Preference for "after" is documented in the source as
 defining redistribution order.
2. Give the recipient the dead cell's size **plus one for the reclaimed border**.
3. Remove from the parent's queue.
4. **Normalize:** if the parent now has exactly one child, splice that child
 into the grandparent's position and free the parent.

So tmux does *not* redistribute proportionally on close — the whole space goes
to one neighbour. Doc 05 §2.1 specifies proportional redistribution for omt
instead; that is a deliberate divergence and it is the better choice for an
n-ary tree (see design §2.5).

### 2.6 zellij — a Cassowary solver, and explicit remainder repair

`PaneResizer::layout(direction, space)`
(`zellij-server/src/panes/tiled_panes/pane_resizer.rs:45`) **[verified]**:

1. `solve` — build bands via `grid_boundaries`, and for each band emit
 constraints into a `kasuari` (Cassowary) solver:
 - `sum(span vars) == space` at strength **REQUIRED**;
 - `Fixed(n)` spans pinned `== n` at **REQUIRED**;
 - `Percent(p)` spans get `var / flex_space == p/100` at **STRONG**, where
 `flex_space = space - sum(fixed sizes)`.
2. `discretize_spans` — the solver returns `f64`. Round each with
 `stable_round(x) = round(round(x*100)/100)` (a deliberate guard against
 `x.4999999` rounding down). Then compute `error = space - sum(rounded)`, take
 the **non-fixed, not-yet-finalised** spans, **sort them by rounded size** —
 ascending when short, descending when over — and hand out `±1` to each in
 turn until `error` is 0. Positions are then recomputed as a running offset,
 and any span landing below 1 aborts with "Ran out of room for spans".
3. `is_layout_valid` — a stated hack: if a stacked pane's band is shorter than
 `min_stack_height`, **abandon the entire resize** rather than apply a bad one.
4. `apply_spans` — writes geometry; returns `Err(PaneSizeUnchanged)` when nothing
 moved, with a source comment noting that this is an error for an explicit
 user resize but not for a window resize.

Two takeaways for omt. First, **remainder distribution must be a specified,
deterministic rule, not a byproduct of rounding** — zellij makes it a sorted
±1 sweep, tmux makes it round-robin, and both are defensible; leaving it
implicit is not. Second, **"refuse the whole resize" is a legitimate outcome**,
and having a name for it (`PaneSizeUnchanged`, `is_layout_valid`) is better than
partially applying one.

Minimums are constants `MIN_TERMINAL_WIDTH` / `MIN_TERMINAL_HEIGHT`, consulted
directly in the floating-pane grid (`floating_pane_grid.rs:151` ff.) where
resize saturates rather than refuses. **[verified]**

### 2.7 other terminals and wezterm

other terminals: `resize_focused(nav, delta, area)` (`layout.rs:215`) finds the nearest
split border in the requested direction (`nearest_resize_split`, falling back to
the *opposite* direction when there is no border on that edge — the same
last-child inversion tmux does, expressed geometrically), then adjusts that one
split's `ratio` by ±delta, clamped to `0.1..=0.9`. One split node changes; the
rest of the tree is untouched. Container resize is free — the tree stores no
geometry. **[verified]**

wezterm: `adjust_x_size` / `adjust_y_size` (`tab.rs:338`, `:408`) recurse
pushing single-cell adjustments into whichever side has room, tracking pixel
dimensions alongside cells (wezterm cares about pixel sizes for image protocols
and reports both). **[verified]**

---

## 3. Multi-client, different sizes — the crux

### 3.1 tmux — precise semantics

tmux's answer lives in `.research/tmux/resize.c` and two options.

**`window-size`** (`options-table.c:1805`), window-scope, default **`latest`**.
Values, quoting the option text **[verified]**: *"'latest' uses the size of the
most recently used client, 'largest' the largest client, 'smallest' the smallest
client and 'manual' a size set by the `resize-window` command."*

`clients_calculate_size(type, current, c, s, w, skip_client, ...)`
(`resize.c:133`) **[verified]**:

- Seeds `sx = sy = 0` for `largest`, `UINT_MAX` for `smallest`/`latest`, and the
 stored `w->manual_sx/sy` for `manual` (which then skips the client loop
 entirely).
- For `latest`, first counts clients showing this window; **if more than one, all
 clients except `w->latest` are skipped**, so "latest" degenerates to "the one
 client that most recently touched this window". With exactly one client it
 behaves as `smallest` — the source says so explicitly.
- Each client's size is `tty.sx × (tty.sy - status_line_size(client))`, unless a
 control client declared a per-window size via `control_get_window_size`.
- `ignore_client_size` (`resize.c:86`) excludes clients with no session,
 `CLIENT_NOSIZEFLAGS`, and — importantly — clients flagged `CLIENT_IGNORESIZE`
 **only if at least one non-flagged client is attached**. So a `-x`/`-y`-pinned
 or read-only client stops constraining the window, but if it is the *only*
 client its size is used after all. That fallback is good design and omt should
 copy the idea (see design §3).
- After the main loop there is a **second, unconditional clamp**: any client
 with a per-window control size clamps the result *down*. Control-mode clients
 can shrink the window regardless of `window-size`.
- `default_window_size` (`:284`) falls back to the session's `default-size`
 option, then to hard-coded 80×24, and finally clamps into
 `[WINDOW_MINIMUM, WINDOW_MAXIMUM]`.

**`aggressive-resize`** (`options-table.c:1247`), window-scope, default **off**.
Its text: *"When 'window-size' is 'smallest', whether the maximum size of a
window is the smallest attached session where it is the current window ('on') or
the smallest session it is linked to ('off')."* **[verified]** So it only has
meaning under `smallest`, and it narrows the constraining set from "every session
this window is linked into" to "sessions currently displaying it". With
`aggressive-resize on`, a window shrinks only while a small client is actually
looking at it, and grows back when that client switches away.

**What the user actually experiences.** Under the default `latest`: attaching a
phone at 52 columns immediately resizes the window to 52 columns for
*everybody*, because the phone is now the latest client — the laptop's full-screen
agent redraws into a 52-column box, and the laptop user's screen is 52 columns of
content with the rest blank. Under `smallest`: the same, permanently, for as long
as the phone is attached — the classic "someone attached from their laptop and my
terminal is now 80 columns" complaint, and the reason `aggressive-resize` exists.
Under `largest`: the phone client sees a window bigger than its terminal; tmux
crops it and draws the off-screen remainder as nothing, so the phone user pans a
window they cannot see all of. `resize_window` (`resize.c:44`) additionally
clamps the window *up* to the layout size, so even `smallest` cannot make a
window smaller than the layout's own minimum. **[verified for mechanism,
inferred for the experiential description]**

There is no per-client rendering anywhere in tmux: one window, one size, one
grid, N clients drawing the same grid.

### 3.2 zellij — smallest-of-active, plus a genuine mobile mode

`Screen::recompute_tab_size(tab_id)` (`zellij-server/src/screen.rs:2523`)
**[verified]**: collect `client_sizes[client_id]` for every client whose
**active tab** is this tab, sort rows and cols independently, take
`rows[0]` and `cols[0]` — i.e. **componentwise minimum over clients currently
looking at the tab**. Note this is per-*tab*, so a client on another tab does not
constrain this one — structurally the same idea as tmux's
`aggressive-resize on`, but as the only behaviour rather than an option.
`client_sizes` is dropped on client removal (`:4844`).

**Mobile mode** (`zellij-server/src/mobile_mode.rs`, 733 lines) is the most
directly relevant prior art omt has for its own problem. **[verified]**

```rust
pub(crate) struct MobileState {
 mobile_tab_for_client: HashMap<ClientId, usize>,
 tab_before_mobile_for_client: HashMap<ClientId, usize>,
 auto_entered_clients: HashSet<ClientId>,
 fit_override_for_tab: HashMap<usize, FitOverride>,
}
struct FitOverride {
 owning_client: ClientId,
 fullscreened_pane: PaneId,
 embedded_content_size: Size,
 pane_was_fullscreen_before_fit: bool,
}
```

The design, as the code implies it:

- A mobile client gets **its own dedicated tab**, whose layout is a single
 borderless pane running the `zellij:mobile` plugin
 (`MobileState::mobile_tab_layout`). The phone is therefore *not* in the tab
 the laptop is in, and does not constrain it. The client's previous tab is
 remembered for exit.
- **Shadow focus**: `apply_shadow_focus(client, pane_id, tabs)` marks a pane in a
 *non-mobile* tab as this client's focus without making the client active in
 that tab, and `clear_shadow_focus` unwinds it. This is how a phone "is looking
 at" a pane in the laptop's tab while formally living in its own tab — a
 per-client focus pointer decoupled from the per-client viewport.
- **Fit**: `set_fit(client, tab, pane, embedded_content_size, tabs)`
 fullscreens the target pane and installs a `FitOverride`.
 `compute_fit_size` then computes the tab size *backwards from the phone's
 content box*: `embedded_content_size + tab_bar_rows/cols + pane frame
 rows/cols`. `recompute_tab_size` iterates this at most `FIT_RESIZE_MAX_ITERS
 = 3` times because changing the tab size changes the chrome sizes, so it is a
 fixed-point search. While a fit is installed it **completely replaces** the
 smallest-client rule for that tab.
- **Render gating** (`MobileRenderGate`): a newly attached client is *blanked*
 (`\x1b[2J\x1b[H`) until its reported size matches the size actually painted
 for it — for web clients, `size_settled && reported_size == paint_size`. This
 exists because a browser reports a viewport, gets a render, then reflows and
 reports a different viewport; showing the intermediate frames looks broken.
 omt's web client will hit exactly this and should plan for it.

So zellij's real answer is: **the phone gets its own layout container, and the
shared one is left alone** — with an explicit escape hatch for "make the shared
tab fit my phone", owned by one client and reversible.

### 3.3 wezterm and other terminals

wezterm **[verified, partial]**: panes live in a mux; a GUI window attaches to a
tab and `PaneEntry` carries `size: TerminalSize` plus `top_row`/`left_col`.
Attaching a second window to the same tab resizes the tab. wezterm's practical
answer to differing sizes is different at a level above layout: its **workspace**
and **domain** model encourages a second client to attach to a *different*
workspace rather than mirror the same tab, so the conflict usually does not
arise. I did not find a size-negotiation policy comparable to tmux's
`window-size`.

other terminals **[verified]**: `clamp_terminal_size(cols, rows)`
(`src/server/client_transport.rs:358`) is simply
`(cols.max(MIN_CLIENT_COLS), rows.max(MIN_CLIENT_ROWS))` — clamp up to a floor
and use it. Its own tests assert `clamp_terminal_size(40, 12) == (40, 12)`, i.e.
a narrow client is preserved as-is. The layout is recomputed per render from
`(tree, area)`, so other terminals can and does render the same workspace at different
areas — but the PTY still has one size, so the terminal content is authored for
whichever size was last pushed. There is no negotiation policy; last writer wins.

### 3.4 Summary of the design space observed

| Approach | Seen in | Cost |
|---|---|---|
| Smallest attached | tmux `smallest`, zellij (per active tab) | A phone shrinks everyone |
| Largest attached | tmux `largest` | Small clients see a cropped window |
| Most-recent client | tmux `latest` (default) | Size flaps as focus moves |
| Manual/pinned | tmux `manual`, `resize-window` | Predictable; someone is always wrong-sized |
| Exclude some clients | tmux `CLIENT_IGNORESIZE`, with an "unless it's the only one" fallback | Needs a UI to express |
| Per-client container | zellij mobile tab + shadow focus | The clients see different *arrangements* |
| Fit-to-client override | zellij `FitOverride` | Owned by one client, reversible, iterative |

Nobody does per-client server-side re-rendering of the same PTY, and zellij's
mobile mode is an explicit vote for **per-client layout over per-client
rendering**.

---

## 4. Named layouts, serialization, and layout files

### 4.1 tmux presets and the layout string

Presets (`.research/tmux/layout-set.c:38`) **[verified]**: `even-horizontal`,
`even-vertical`, `main-horizontal`, `main-horizontal-mirrored`, `main-vertical`,
`main-vertical-mirrored`, `tiled`. `layout_set_lookup` accepts unambiguous
prefixes. `main-vertical` (`layout_set_main_v`) reads `main-pane-width` and
`other-pane-width` (each may be a percentage string), clamps so the main pane
leaves at least `PANE_MINIMUM` for the others, builds a `LEFTRIGHT` root with the
main pane and a `TOPBOTTOM` node of the rest, then calls `layout_spread_cell` on
that node. Note it also computes `sy = max(w->sy, n*(PANE_MINIMUM+1) - 1)` and
**resizes the window to the layout** afterwards — presets can make the layout
bigger than the window.

**The layout string format** (`layout-custom.c`) **[verified]** — precise
grammar, worth adopting as an interchange format:

```
layout-string := checksum "," cell [ "<" floating-cells ">" ]
checksum := 4 lowercase hex digits
cell := SX "x" SY "," XOFF "," YOFF [ pane-id | children ]
children := "[" cell ("," cell)* "]" ; top-bottom split
 | "{" cell ("," cell)* "}" ; left-right split
pane-id := "," decimal ; leaf only
```

- Emitted by `layout_dump`; the leaf form is `%ux%u,%d,%d,%u` (size, offsets,
 pane id) and the node form drops the pane id.
- `[`…`]` is `LAYOUT_TOPBOTTOM`, `{`…`}` is `LAYOUT_LEFTRIGHT`. (In the source
 the string `"]["` / `"}{"` is indexed backwards, so the *open* bracket is
 `brackets[1]`.)
- The optional `<`…`>` suffix carries floating cells, appended in z-order.
- The checksum is a 16-bit rotate-and-add over the body:
 `csum = (csum >> 1) + ((csum & 1) << 15); csum += *c;` — not a real integrity
 check, just a typo guard.
- `layout_check` validates on parse: for a `{}` node every child's `sy` equals
 the parent's and `sum(child.sx + 1) - 1 == parent.sx`; transposed for `[]`.
 A string with inconsistent arithmetic is rejected outright.

Example: `bb62,158x48,0,0{79x48,0,0,0,78x48,80,0,1}` — checksum `bb62`, a
158×48 root at origin, left-right, two 79/78-wide leaves being panes 0 and 1,
with the second at `xoff=80` (79 + 1 border).

**This format is genuinely good as an import target** — it is dense, total, and
there is a large corpus of them in people's shell history and in
tmux-resurrect files. Its weakness is that it encodes *absolute cells for one
specific window size*, so importing it at a different size requires
re-proportioning (convert each child's share of its parent into a ratio; the
`layout_check` invariant guarantees the shares are well-formed).

### 4.2 zellij — KDL layout files

**[verified]** from `.research/zellij/zellij-utils/assets/layouts/` and
`example/layouts/`:

```kdl
layout {
 default_tab_template {
 pane size=1 borderless=true { plugin location="zellij:tab-bar" }
 children
 pane size=2 borderless=true { plugin location="zellij:status-bar" }
 }
 tab split_direction="Vertical" {
 pane split_direction="Vertical" {
 pane size="50%"
 pane size="50%" split_direction="Horizontal" { pane size="50%"; pane size="50%" }
 }
 }
}
```

Key features:
- `size` is `SplitSize::{Fixed(usize), Percent(usize)}` — bare integer is cells,
 quoted `"NN%"` is a percentage. Mixed fixed/percent siblings are exactly what
 the Cassowary solver's `flex_space` exists to handle.
- **Templates with a `children` hole.** `default_tab_template` / `tab_template`
 wrap every tab in chrome; `children` marks where the tab's own panes are
 spliced. This is a much better answer to "every layout needs a status bar" than
 repeating it.
- **Swap layouts** (`*.swap.kdl`) — the most interesting idea here. A
 `swap_tiled_layout` is a named *sequence* of layouts each guarded by a
 `LayoutConstraint`: `max_panes=N`, `min_panes=N`, `exact_panes=N`. zellij picks
 the applicable one automatically as panes are added, so "vertical" means a
 different arrangement at 2, 5, 8 and 12 panes. `swap_layouts.rs` tracks
 `is_tiled_damaged` — once the user manually resizes, the layout is marked
 damaged and auto-swapping stops until the user asks for it. There is also
 `swap_tiled_layout name="stacked" { ui min_panes=4 { pane stacked=true { children; } } }`,
 i.e. a preset that degrades a many-pane layout into a *stack*. This is
 precisely the mechanism omt needs for phone degradation, generalized.

**Session serialization** (`zellij-utils/src/session_serialization.rs`)
**[verified]**: `serialize_session_layout(GlobalLayoutManifest) -> (String, BTreeMap<String,String>)`
— a KDL document **plus a map of filename → pane contents**. Per pane it
records `geom, run, cwd, is_borderless, title, is_focused, pane_contents,
default_fg, default_bg`. So a resurrected session is *the same layout file
format the user writes by hand*, with scrollback carried in sidecar files. The
round-trip property (dump → the format you can hand-edit → reload) is the thing
that makes the format worth maintaining; doc 10 §9.1 already makes the same bet
for omt's launch configs.

### 4.3 wezterm and other terminals

wezterm: `PaneNode` is serde-serializable and shipped over the codec; `.wezterm.lua`
builds workspaces programmatically rather than declaratively. **[verified]**

other terminals: launch configurations are YAML with a recursive `layout: {split, ratio,
panes: [...]}` shape — already documented in [other terminals.md] §5.2 and adopted
in [10 — Configuration](../architecture/10-configuration.md) §9.1. **[docs +
verified interface]**

---

## 5. Zoom, floating, stacked, swap, marks, sync

### 5.1 Zoom

tmux `window_zoom(wp)` (`window.c`) **[verified]**: refuses if already zoomed or
if there is only one pane. Saves **every** pane's `layout_cell` into
`saved_layout_cell`, nulls them, saves `w->layout_root` into
`saved_layout_root`, then calls `layout_init(w, wp)` — building a brand-new
single-leaf tree. Unzoom restores the saved pointers. So zoom is a *tree swap*,
not a flag consulted during geometry computation, and the panes that are hidden
genuinely have no cell. `resize_window` unzooms before resizing and re-zooms
after (`resize.c:59`, `:79`) — because the saved tree must be resized in its own
right. **[verified]** Zoom is per-window, hence shared by all clients.

zellij calls it fullscreen (`toggle_pane_fullscreen`), and mobile-mode fit
composes with it — `FitOverride` records `pane_was_fullscreen_before_fit` so
exiting fit restores the prior state rather than blindly unfullscreening.
**[verified]**

kitty's `stack` layout is zoom-as-a-layout: one window visible, the rest hidden,
navigable. **[docs]**

### 5.2 Floating panes

tmux (recent versions) **[verified]**: `LAYOUT_CELL_FLOATING` on the cell, a
saved geometry `fg`, a `w->z_index` queue for stacking order, and
`layout_cell_is_tiled` / `layout_cell_has_tiled_child` guards throughout the
tiling arithmetic so floats neither consume nor donate band space.
`layout_split_pane` fatals on an attempt to split a floating cell. Floats are
serialized in the `<...>` suffix of the layout string. `layout_resize_floating_pane`
and `layout_resize_floating_pane_to` are separate entry points that do not touch
the tiled tree at all.

zellij **[verified]**: an entirely separate `FloatingPanes` collection with its
own `FloatingPaneGrid` (`move_pane_by`, `move_pane_{left,right,up,down}`,
`change_pane_size`, `find_room_for_new_pane`), its own `is_pinned` flag
(a pinned float stays on top when the float layer is hidden), `resize(space)`
that proportionally rescales floats on container resize saturating at
`MIN_TERMINAL_WIDTH/HEIGHT`, and its own swap-layout track
(`SwapFloatingLayout`). The whole float layer can be shown/hidden as a unit
(`hide_floating_panes` is serialized per tab).

**The consistent design across both: floats are a second, parallel layer with
their own geometry rules, not special nodes in the tiling tree.** That is the
right structure and omt should adopt it.

### 5.3 Stacked panes

zellij only. `stacked_panes.rs` (1233 lines) **[verified]**. `PaneGeom.stacked:
Option<usize>` is a stack id. A stack renders as one flexible (full-height) pane
plus one-line title rows for the others; `min_stack_height` is what the resizer
checks. API surface: `move_up`/`move_down` within a stack, `expand_pane`,
`flexible_pane_id_in_stack`, `position_and_size_of_stack`,
`make_room_for_new_pane[_in_stack]`, `new_stack(root, count)`,
`combine_vertically_aligned_panes_to_stack` /
`combine_horizontally_aligned_panes_to_stack`, `break_pane_out_of_stack`.
The resizer treats a whole stack as a single span (`get_span` returns `None` for
non-flexible members so they do not double-count).

Conceptually a stack is **N panes in the space of one, with N-1 of them collapsed
to their title bar** — i.e. tabs-within-a-split. Directional navigation into a
stack lands on `flexible_pane_id_in_stack` (the expanded one), which is a nice
touch. **[verified]**

### 5.4 Swap, rotate, move

tmux **[verified]**: `swap-pane` exchanges two panes' cells;
`layout_assign_pane(lc, wp, do_not_resize)` retargets a cell at a different pane,
which is the primitive underneath both swap and `join-pane`/`break-pane`
(moving a pane between windows). `rotate-window` cycles panes through the
existing cells — the *layout* is fixed and the *contents* rotate, which is
exactly right and much simpler than rotating the tree.

kitty `splits` has `rotate`, `equalize`, `maximize`, and directional
`move_window`. **[docs]**

zellij `move_pane`, `break_pane_out_of_stack`, and moving panes between tabs.
other terminals: `PaneData::move_pane(id, target_pane_id, direction)` plus a
`PaneDragDropLocation::{TabBar, PaneGroup(PaneId), Other}` for drag-and-drop
between tabs. **[verified]**

### 5.5 Marked pane and synchronize-panes

tmux **[verified]**: `marked_pane` is a single **global** `cmd_find_state`
(`server.c:51`) — one mark for the whole server, not one per window. It is
targetable as `~` in commands, `select-pane -m/-M` sets/clears it, and it is
what `join-pane`/`swap-pane` default their source to. `format.c:2415` exposes
`pane_marked`, and `screen-redraw.c:1611` renders the marked pane's border
distinctly. Session/window deletion fixes up the mark (`session.c:760`).

`synchronize-panes` (`options-table.c:1724`) is a window-scope flag: input to
one pane is written to every pane in the window. The status line renders a
warning style when it is on (`options-table.c:1545`). It is a per-window option,
so it is inherently shared across clients.

---

## 6. Pane navigation

### 6.1 Directional — the geometric algorithm, three implementations agreeing

**tmux** `window_pane_find_up(wp)` (`window.c`) **[verified]**:

1. Compute the pane's full-size offset/size; the "edge" is `yoff`, with special
 cases mapping `yoff == 0` (or 1 with a top status line) to `w->sy + 1` so that
 moving up from the top row **wraps to the bottom**.
2. Candidate filter: `next.yoff + next.sy + 1 == edge` — an *exact* adjacency
 test using the border row. Only panes whose bottom border is this pane's top
 edge qualify.
3. Overlap filter, on the perpendicular axis, three accepted cases: candidate
 spans the whole of the source (`xoff < left && end > right`), candidate's
 start is inside the source's range, or candidate's end is inside it.
4. **Tie-break: `window_pane_choose_best` picks the candidate with the greatest
 `active_point`** — a monotonically increasing counter stamped whenever a pane
 is activated. That is, **most-recently-used among the eligible neighbours**.

**zellij** `next_selectable_pane_id_to_the_left`
(`tiled_pane_grid.rs`) **[verified]**: filter to `selectable` panes satisfying
`is_directly_left_of(current) && horizontally_overlaps_with(current)`, then
`max_by_key(|c| c.active_at)` — literally the same rule, including the
recency tie-break. If the winner is stacked, redirect to
`flexible_pane_id_in_stack`.

**other terminals** `find_in_direction(focused, direction, panes)` (`layout.rs:280`)
**[verified]** — the most explicit version, and the one worth reimplementing:

- Filter: for `Left`, `r.x + r.width <= fr.x && ranges_overlap(r.y, r.height, fr.y, fr.height)`;
 transposed for the other three. Note `<=` rather than an exact border match, so
 a candidate need not be *immediately* adjacent.
- Rank by the tuple `(edge_distance, Reverse(overlap_amount), center_distance, index)`:
 nearest edge first; among equals, **largest perpendicular overlap**; then
 closest centre; then a stable index. `min_by_key` over that tuple.

the tuple is strictly better than "most recently used" for predictability:
the same keypress from the same layout always goes to the same pane, whereas
tmux/zellij's recency tie-break means the same keypress can go two different
places depending on history. omt should take the ranking and drop recency.

other terminals has the same shape: `panes_by_direction` returns
`FindPaneByDirectionResult::Found(HashSet<PaneId>)` — a *set* of candidates,
disambiguated above. **[verified]**

### 6.2 Other navigation

tmux **[verified]**: by index (`select-pane -t %N`, `display-panes` overlay),
`last-pane` (`select-pane -l`), next/previous in tree order
(`select-pane -n/-p`, wrapping), and `pane_id_on_edge(direction)` in zellij for
"focus the leftmost pane".

### 6.3 Borders and titles

tmux **[verified]**: `pane-border-status` is `off | top | bottom`, and it costs a
row — `layout_add_horizontal_border(root, lc, status)` returns whether a given
cell needs one, and that answer feeds directly into `layout_resize_check`'s
minimum and into `layout_spread_cell`'s `each` computation. **A pane title bar
is not decoration; it is part of the layout arithmetic.** `layout_cell_is_top` /
`_is_bottom` (`layout.c:392`, `:410`) walk to the root checking first/last-tiled
at every `TOPBOTTOM` ancestor to decide whether a cell touches the window edge,
which is how tmux knows whether to draw a border there.

`layout_search_by_border(lc, x, y)` (`:159`) maps a mouse coordinate to the cell
whose border was hit — the hit-test for drag-resize. the equivalent is
`splits(area) -> Vec<SplitBorder{pos, direction, ratio, area, path}>`, where
`path: Vec<bool>` addresses the split node directly. the is the better
interface for a networked client: the border is a nameable object with a stable
address, not a coordinate to be re-hit-tested server-side. **[verified]**

---

## 7. Persistence and restore

**tmux** has none built in — `layout_dump`/`layout_parse` (§4.1) exist, and
tmux-resurrect (a plugin) shells out to `list-panes`/`display-message` to
capture pane commands, cwds and the `window_layout` string, writing a TSV to
`~/.tmux/resurrect/`, then replays it by respawning panes and applying
`select-layout <string>`. Because the layout string carries absolute sizes for
the *saved* window size, restore at a different size relies on
`layout_set_size_check` / `layout_resize` to re-proportion. **[docs + inferred
from the format]**

**zellij** serializes to KDL on a timer (§4.2) — layout plus per-pane
`pane_contents` sidecars — and resurrects by running the layout file. Because
the serialized form is the authorable form, restore has no separate code path.
`src/tests/fixtures/layout_for_resurrection.kdl` is a checked-in fixture.
**[verified]**

**wezterm** has `resurrect.wezterm` (third-party) doing the same trick over its
`PaneNode` serde form. **[docs]**

**other terminals** persists a serializable BSP tree (`src/persist/snapshot.rs:126`,
"Serializable BSP tree") and `PaneId::from_raw` exists specifically so ids
survive a reload. **[verified]**

The consistent lesson: **make the persisted layout format identical to the
authorable layout format.** zellij's is; tmux's isn't (the string is a different
language from anything a user writes), and that is why tmux restore is a plugin
ecosystem rather than a feature.

---

## 8. Alt-screen panes during resize

- **tmux** keeps a separate `alternate_screen` save and, on resize, does not
 attempt to reflow it — it resizes the alternate grid and lets `SIGWINCH` drive
 the application's redraw. **[inferred from structure; the definitive statement
 for omt is already in [04 §3.3](../architecture/04-terminal-core.md#33-algorithm)
 step 3, which independently specifies exactly this.]**
- **zellij** `PaneResizer::is_layout_valid` refuses a resize outright when it
 would violate a stack's minimum, i.e. **"don't resize" is preferred to "resize
 badly"**. **[verified]** Its render gate additionally blanks a client until its
 size has settled, precisely so an alt-screen app's half-reflowed frame is never
 shown. **[verified]**
- **doc 07 §4.3** already states omt's position bluntly and correctly: *"an agent
 that drew a box at 120 columns cannot be re-rendered at 40; the information is
 gone."* Nothing in any of these codebases contradicts that. There is no
 multiplexer that re-renders an alt-screen app per client.
- The practical consequence everywhere: **a resize of a pane hosting an
 alt-screen app is a visible, disruptive event**, so the number of resizes
 matters more than their cost. Debouncing (doc 05 §2.2's 250 ms) and
 DECSET 2026 quiescing (doc 04 §3.3 step 1) are the mitigations, and
 `window-size manual` / pinning is the escape hatch.

---

## 9. What omt should take, and what it should not

**Take:**

1. tmux's **sum/min duality** in `resize_check`. Any correct resize algorithm
 needs it whatever the data structure.
2. tmux's **"a drag moves the border, i.e. exactly two adjacent cells"**
 semantics, including the last-child inversion. This is the single most
 important behavioural detail in this document.
3. tmux's **layout string** as an *import* format, with a documented
 re-proportioning step.
4. zellij's **explicit, deterministic remainder distribution**, and its
 willingness to **refuse** a resize that cannot be satisfied.
5. zellij's **swap layouts with pane-count constraints** — generalized, this is
 responsive layout, which is what a phone needs.
6. zellij's **per-client container + shadow focus** as the answer to
 multi-client sizing, and its **render gate** for web clients whose reported
 viewport is unstable at attach.
7. zellij's **floats and stacks as parallel layers**, never as tiling-tree nodes.
8. the **deterministic navigation ranking tuple** in place of recency.
9. the **`SplitBorder` with a stable path** as the addressable handle for a
 resize, rather than a screen coordinate.
10. zellij's **serialized form == authorable form** for persistence.

**Do not take:**

1. tmux's **absolute-cell storage**. Ratios survive a container resize; absolute
 cells require the whole `layout_new_pane_size` proportional-rebuild machinery
 to fake it, and still let the layout exceed the window.
2. tmux's **`window-size latest` default**. A size that flaps as attention moves
 is the worst of the options when one client is a phone.
3. tmux's **give-everything-to-one-neighbour close**. Proportional
 redistribution is correct in an n-ary tree.
4. **Recency tie-breaks in navigation.** Non-deterministic keybindings.
5. zellij's **runtime geometric structure recovery**. Rediscovering "these panes
 form a column" from coordinates on every resize is clever but it makes the
 invariants unstateable, which is fatal for a crate whose selling point is
 property-tested determinism ([05 §12](../architecture/05-session-model.md#12-testing)).
6. A **Cassowary solver**. It is the right tool when constraints are
 user-authored and heterogeneous; for a tree of ratios with minimums it is a
 large dependency, a non-obvious failure mode (`is_layout_valid`'s "abandon
 ship" hack exists because of it), and non-trivially non-deterministic to
 test.
