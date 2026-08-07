# Terminal Core — `omt-term`

`omt-term` is a **pure state machine**: bytes in, state and damage out. No I/O,
no async, no runtime. It is L2 in the [crate map](02-crate-map.md) and depends
only on `omt-types`, `omt-util` and the contract crates. Everything above it —
`omt-session`, the TUI, the web client — consumes the same snapshot and the same
damage record.

Related: [00 — Overview](00-overview.md) · [01 — Principles](01-principles.md) ·
[03 — Capability catalog](03-capability-catalog.md) ·
[05 — Session model](05-session-model.md) ·
[08 — Web client](08-web-client.md) · [14 — Licensing](14-licensing.md) ·
[17 — Panes and layout](17-panes-and-layout.md).
Research inputs: [iTerm2](../research/iterm2.md), [another terminal](../research/another terminal.md).

The crate is deliberately large in scope but small in surface. Its public API is
roughly:

```rust
pub struct Terminal { /* private */ }

impl Terminal {
    pub fn new(cfg: TermConfig) -> Self;
    /// Feed a chunk of PTY output. Returns host actions the caller must perform
    /// (write a reply to the PTY, set the title, ring the bell, …).
    pub fn advance(&mut self, bytes: &[u8]) -> HostActions<'_>;
    /// Resize with reflow. See §3.
    pub fn resize(&mut self, cols: u16, rows: u16) -> ResizeReport;
    /// Immutable, cheap (copy-on-write) view for renderers and for snapshotting.
    pub fn snapshot(&self) -> Snapshot;
    /// Take and clear accumulated damage. See §4.
    pub fn take_damage(&mut self) -> Damage;
    pub fn blocks(&self) -> &BlockIndexView;
    pub fn search(&self, q: &Query, cursor: &mut SearchCursor) -> Option<Match>;
}
```

`advance` never blocks, never allocates unboundedly, and never performs a side
effect itself. A reply to DA/DSR/OSC-52 is *returned* as a `HostAction`, so the
core is trivially testable and the session layer keeps all policy. `HostAction`
is enumerated in [§1.6](#16-hostaction--what-advance-returns).

---

## 1. Parser architecture

### 1.1 Decision

**Build on the `vte` crate (Alacritty's byte-level VT state machine, MIT/Apache-2.0)
for byte→event tokenization, and write omt's own semantic layer on top of it.**
Extend `vte` only where it is genuinely insufficient (DCS/APC hooks and
multi-megabyte OSC payloads — §1.4).

This is [D6](decisions.md#d6--terminal-emulation-is-built-on-a-third-party-byte-level-parser)
in the decision log, recorded there because it is foundational and hard to
reverse; the evaluation below is its rationale.

### 1.2 Options considered

| Option | Verdict | Reasoning |
|---|---|---|
| Vendor `ghostty-vt` (as another tool does) | **Rejected** | Licence is fine (MIT) but it is Zig: a Zig toolchain in the build, an FFI boundary in the hottest loop, and a foreign memory model for the grid. another tool accepts that because it only needs a screen scrape; omt must own the grid, marks and blocks in Rust to do reflow and block tracking. A Rust grid over a Zig parser is the worst of both. |
| Write the byte state machine from scratch | **Rejected** | Paul Williams' DEC ANSI state machine is ~1000 lines of table-driven code that is boring, well-specified, and already correct in `vte`. Rewriting it buys nothing and costs a year of tail bugs (C1 in UTF-8, sub-parameters, intermediates, `ST` variants). The interesting correctness lives above the tokenizer. |
| `vte` + omt semantic layer | **Chosen** | `no_std`-capable, dependency-light, Apache-2.0/MIT (compatible with omt's Apache-2.0 — no copyleft contamination, cf. [P9](01-principles.md#p9--clean-room-with-respect-to-studied-code)), battle-tested by Alacritty. Its `Perform` trait is exactly the seam we want: `print`, `execute`, `csi_dispatch`, `esc_dispatch`, `osc_dispatch`, `hook`/`put`/`unhook`. |
| `termwiz` | Rejected | Excellent, but brings a much larger surface (its own cell/surface model, terminfo, widgets) we would fight rather than use. |

Licence note: we may read iTerm2 (GPL-2.0) and another terminal (AGPL-3.0) to learn *what
sequences exist and what they mean* — those are facts. We may not copy or
translate their code. Every design here is an interface plus an algorithm;
implementers write it from the description, not from the studied sources.

### 1.3 Layering

```
PTY bytes
  │
  ├─▶ FastPath              ASCII-run scanner (§9.2). Handles the common case of
  │                         printable ASCII with no ESC, in one memchr-driven pass.
  │
  └─▶ vte::Parser  ──▶  impl Perform for Interpreter
                              │
                              ├─ Charset / mode state  (Modes, Charsets, SavedCursor)
                              ├─ Grid mutation         (Screen: primary | alternate)
                              ├─ Sink for OSC semantics (Osc133, Osc7, Osc8, Osc52, Osc1337)
                              ├─ Payload hooks         (Sixel, Kitty-APC, omt backchannel)
                              └─ HostActions queue     (replies, title, bell, clipboard req)
                                        │
                              Damage accumulator + BlockTracker
```

`Interpreter` is the only type that knows about VT semantics. `Screen`, `Grid`,
`Scrollback` and `BlockTracker` know nothing about escape sequences — they take
structured operations (`insert_cells`, `scroll_region_up`, `set_mark`). Module
split, chosen to stay under the 1200-line advisory limit from
[P1](01-principles.md#p1--clean-small-crates-explicit-seams): `parser/`
(`vte` glue, fast path, hooks) · `interp/{csi,osc,modes}.rs` · `grid/` ·
`scrollback/` · `reflow/` · `block/` · `damage/` · `search/`, `select/`, `link/`

**Blocks exist without shell integration.** `block/` opens one on output when
none is open, marked `attributed: false` — which is what makes a bare `ssh`, a
container, or any shell that emits no OSC 133 produce something rather than an
empty session. An unattributed block carries no command and never guesses one:
scraping the line above for something that "looks like" a prompt is the
heuristic the tier ladder exists to keep out of structured content, and a
"re-run" button wired to a guess is the failure it prevents.

Exit codes 130 and 141 are not failures. 130 is Ctrl-C, which the user meant,
and 141 is SIGPIPE, which is what `… | head` does to everything upstream of it
every single time. A terminal that paints those red teaches people that red
means nothing.
· `graphics/`.

### 1.4 Where `vte` is not enough

Two extensions, both implemented as omt-side wrappers, not forks:

**(a) Streaming payload hooks.** `vte` already surfaces DCS as
`hook`/`put`/`unhook`, and APC as `esc_dispatch` plus raw bytes if configured;
what it does not do is let us *route* a payload to a stateful consumer chosen by
the introducer. We add:

```rust
pub trait PayloadHook: Send {
    /// Called once per chunk of payload bytes. Return `Continue` or `Unhook`.
    fn put(&mut self, bytes: &[u8]) -> HookFlow;
    /// Called on ST / abort. Produces the semantic result, if any.
    fn finish(self: Box<Self>, reason: HookEnd) -> Option<HookOutput>;
    /// Hard cap; exceeding it aborts the hook and emits `HookOutput::Aborted`.
    fn budget(&self) -> usize { 16 * 1024 * 1024 }
}
```

Selected by `(private_marker, intermediates, final_byte)`:

| Introducer | Hook |
|---|---|
| `DCS <params> q` | `SixelHook` |
| `APC G <kv> ; <b64>` | `KittyGraphicsHook` |
| `OSC 1337 ; File=` / `MultipartFile=` | `FileTransferHook` |
| `DCS + q` | XTGETTCAP responder |
| anything else | `DiscardHook` with a budget |

Every hook is budgeted. A runaway DCS (binary garbage on the wire, a `cat` of a
JPEG) hits its budget, aborts, and the terminal resynchronizes — it never
accumulates unbounded memory. This is the same lesson iTerm2 encodes as
`dataLooksLikeBinaryGarbage`.

**(b) Chunked OSC bodies.** A 20 MB inline image must never be buffered as one
`Vec<u8>` inside the parser before we see it. `FileTransferHook` streams into an
`ImageAssembler` that decodes base64 incrementally and writes into a
pre-allocated, size-capped buffer (the `size=` key gives the decoded length up
front; a payload exceeding it by more than a slack factor is rejected). Images
above `TermConfig::max_image_bytes` (default 32 MiB) are dropped with a
`HostAction::Warn`.

**Partial-sequence resumption is free** with `vte`: the parser is a resumable
state machine by construction, so a chunk boundary in the middle of a CSI or OSC
is not a special case. This is the single property that made iTerm2 build
`_savedStateForPartialParse` by hand, and the main reason not to write our own.

### 1.5 Nested parsing (SSH / tmux passthrough)

We do **not** implement iTerm2's conductor. `Interpreter` does, however, expose a
`Passthrough` hook slot so that [09 — SSH and media](09-ssh-and-media.md) can
later install a nested `Terminal` for multiplexed remote streams without
changing the parser. Reserved, unimplemented, and documented as such.

### 1.6 `HostAction` — what `advance` returns

`HostActions<'_>` is the drain handle over the queue §1.3 shows: an iterator of
`HostAction` borrowed from the `Terminal`, valid until the next mutation.
`advance` is the crate's primary method, so this is the crate's primary output
type alongside `Damage`, and it is enumerated here in full rather than being
discovered one variant at a time.

```rust
pub struct HostActions<'a> { /* private; drains the queue */ }
impl<'a> Iterator for HostActions<'a> { type Item = HostAction<'a>; }

pub enum HostAction<'a> {
    // ── Replies the host must write back to the PTY ──────────────────────────
    /// Primary/secondary DA, DSR cursor/status, XTVERSION, XTGETTCAP, kitty
    /// keyboard query, DECRQM. Pre-encoded; the host writes the bytes verbatim
    /// and never interprets them.
    Reply(&'a [u8]),

    // ── Window/session properties the session layer owns ─────────────────────
    SetTitle(&'a str),                       // OSC 0 / 2
    SetIconName(&'a str),                    // OSC 1
    SetCwd { url: &'a str },                 // OSC 7 → Session::cwd (05 §1.1)
    SetUserVar { key: &'a str, value: &'a str }, // OSC 1337 SetUserVar
    Bell,                                    // BEL → 05's `Bell` event
    /// The program asked to resize/move/report the window (CSI t). omt reports
    /// truthfully and **never** resizes on a program's request; the size is
    /// 07 §4.3's to negotiate.
    WindowOp(WindowOp),

    // ── Clipboard, which is policy, not emulation ────────────────────────────
    /// OSC 52 write. Honoured per 13's clipboard policy, never automatically.
    ClipboardWrite { selection: ClipboardSelection, data: &'a [u8] },
    /// OSC 52 read request. Answered only with explicit user consent; the reply
    /// is sent back in as `Reply` bytes by the host.
    ClipboardRead { selection: ClipboardSelection },

    // ── Structured semantics the layers above consume ────────────────────────
    /// OSC 133 A/B/C/D reached the block tracker. Carries the resulting
    /// transition so 05 can emit `BlockOpened`/`BlockClosed` without re-parsing.
    Block(BlockEvent),
    /// OSC 8 hyperlink opened/closed on the current cell run.
    Hyperlink(HyperlinkEvent),
    /// A DEC private mode the host must know about because it changes how input
    /// is encoded or forwarded: bracketed paste (2004), focus reporting (1004),
    /// mouse modes, kitty keyboard (`>u`/`<u`), and the **alternate screen
    /// (1049)** — the last is what §6.4's capability downgrade keys on.
    ModeChanged { mode: TermMode, enabled: bool },
    /// A payload hook (§1.4) produced a result: an image, a file transfer, an
    /// omt backchannel message.
    Payload(HookOutput),

    // ── Things the host must be told about, not asked to do ──────────────────
    /// The grid was resized by the program's own escape sequence (DECCOLM,
    /// CSI t) rather than by `resize()`. The host must push the new size to the
    /// PTY. Distinct from a host-initiated resize, which returns `ResizeReport`.
    NotifyResize { cols: u16, rows: u16 },
    /// A recoverable anomaly the user or the log should see: an oversized image
    /// (§1.4), a budget-aborted hook, an unsupported sequence at a level the
    /// config asked to be told about.
    Warn(Warning),
}
```

Rules that make the type behave:

- **Every variant is a request to the *host*, never a mutation the core
  performed.** The grid is already updated by the time `advance` returns; these
  are the effects the core is forbidden to perform itself.
- **The queue is bounded** (`TermConfig::max_host_actions`, default 256 per
  `advance`). Overflow drops the oldest *coalescible* actions — `SetTitle`,
  `SetCwd` and `NotifyResize` are last-wins by construction — and emits one
  `Warn`. `Reply` and `ClipboardWrite` are never dropped; if they cannot be
  queued, `advance` stops consuming and the caller re-enters, which applies
  backpressure rather than losing a protocol reply.
- **Order is preserved**, and it is the byte order the sequences arrived in. A
  `Reply` must not be reordered ahead of a `ModeChanged` the program is about to
  depend on.
- The host must drain `HostActions` before the next `advance`; the borrow makes
  that a compile-time requirement rather than a convention.

## 2. Grid and storage

### 2.1 The cell

Design goals: 16 bytes, no allocation for the common case, truecolor native,
grapheme-correct, and reflow-safe.

```rust
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// Base character, or a `GraphemeId` when `Flags::COMPLEX` is set.
    ch: CharOrGrapheme,   // u32
    fg: Color,            // u32  (tag + 24-bit payload)
    bg: Color,            // u32
    flags: Flags,         // u16
    /// Index into the line's `ExtraAttrs` side table; 0 == none.
    extra: ExtraId,       // u16
}
// size_of::<Cell>() == 16, align 4.
```

`Color` is a tagged u32:

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Color(u32);
// bits 30..32: 0 = Default, 1 = Indexed(u8), 2 = Rgb(u8,u8,u8)
```

Packing colours as a tag + payload rather than iTerm2's split
`mode`/`r`/`g`/`b` bitfields costs 2 bytes per cell versus their 12-byte struct
and buys a single-word equality test, which is the hot operation in damage
comparison and in the renderer's run-length coalescing.

`Flags` (u16):

| Bit | Meaning |
|---|---|
| 0 | `BOLD` |
| 1 | `FAINT` |
| 2 | `ITALIC` |
| 3 | `BLINK` |
| 4 | `INVERSE` |
| 5 | `INVISIBLE` |
| 6 | `STRIKETHROUGH` |
| 7–9 | `UNDERLINE_STYLE` (none/single/double/curly/dotted/dashed) — 3 contiguous bits, deliberately not split the way `screen_char_t` had to |
| 10 | `COMPLEX` — `ch` is a `GraphemeId` |
| 11 | `WIDE` — left half of a double-width cluster |
| 12 | `WIDE_SPACER` — right half (`DWC_RIGHT` equivalent) |
| 13 | `PROTECTED` — SPA/EPA guarded area |
| 14 | `IMAGE` — cell is covered by an inline image placement |
| 15 | reserved |

Three sentinel roles that iTerm2 encodes as private-use codepoints are encoded
here as flags or as `ch` sentinels instead:

- **Wide right half**: `WIDE_SPACER`. Copying/searching skips it.
- **Wrap padding** (`DWC_SKIP`): the last column of a line when a double-width
  cluster could not fit; stored as `ch == '\0'` with `WIDE_SPACER`.
- **Tab filler**: plain spaces plus a per-line bitmap of tab-origin columns
  (`Line::tab_origins`), so a copy can restore real tabs and a `cat` of a file
  with tabs round-trips.

Size reasoning: 16 B/cell × 200 cols × 50 000 scrollback lines = **160 MB** if
lines were stored densely at full width. They are not — see §2.3.

### 2.2 Graphemes, width, normalization

```rust
pub struct GraphemeTable {
    /// Interned multi-codepoint clusters. `GraphemeId(0)` is never used.
    data: Vec<Box<str>>,
    index: HashMap<Box<str>, GraphemeId>,
    refcounts: Vec<u32>,
}
```

- Base characters (single scalar, width 1 or 2) live inline in `ch`. Only true
  clusters — combining marks, ZWJ emoji, regional indicators, variation
  selectors — are interned. Measured on real terminal traffic this is well under
  0.1 % of cells, so the table stays tiny and lookups stay cold.
- Refcounts are maintained on cell overwrite and on scrollback eviction; the
  table is compacted when free entries exceed 50 %.
- **Width** comes from a generated table, not a hand-written `match`. `build.rs`
  consumes UCD `EastAsianWidth.txt` + `emoji-data.txt` for a **pinned Unicode
  version** and emits a static range table. Config exposes
  `unicode_version` (default: the pinned one; overridable at runtime by
  `OSC 1337 ; UnicodeVersion=`) and `ambiguous_is_double_width` (default false).
  Both are per-`Terminal`, because a session ssh'd into an old host needs to
  agree with that host's `wcwidth`, not ours.
- **Grapheme segmentation** uses `unicode-segmentation` with the extended
  grapheme cluster rules, applied incrementally: a combining mark arriving after
  a printed base character mutates the previous cell in place (promoting it to
  `COMPLEX`) rather than occupying a new cell.
- **Normalization** is a mode (`None | Nfc | Nfd`), default `None`. We normalize
  only in `search` and `selection→string`, never in storage, because storage
  normalization loses bytes the user may want back.

### 2.3 Lines

```rust
pub struct Line {
    cells: Vec<Cell>,        // trimmed: no trailing default cells stored
    wrap: Wrap,              // Hard | Soft | SoftWide
    extras: Option<Box<ExtraAttrs>>,   // hyperlinks, per-cell URLs, image refs
    tab_origins: Option<Box<BitVec>>,
    ts: Timestamp,           // when the line was completed (u32 secs since epoch base)
    gen: Generation,         // bumped on any mutation
}
```

- `cells` is **right-trimmed**: a 200-column terminal showing a 12-character
  line stores 12 cells, not 200. Reads past the end return the line's default
  cell. This is the single biggest memory win and it is why the dense worst-case
  above is not the real case; typical scrollback averages 30–60 cells per line.
- `Wrap::Hard` = a real newline terminated this logical line. `Wrap::Soft` = the
  line continues on the next one because it hit the right margin.
  `Wrap::SoftWide` = it wrapped because a double-width cluster would not fit,
  which matters for reflow (the padding cell must be dropped, not re-emitted).
  This tri-state is the `EOL_HARD/EOL_SOFT/EOL_DWC` distinction, and it is
  exactly what makes reflow a pure function of raw lines and width.
- `ExtraAttrs` is a side table of `(range, Attr)` runs, not per-cell data:

```rust
pub struct ExtraAttrs {
    hyperlinks: SmallVec<[(Range<u16>, HyperlinkId); 2]>,
    images:     SmallVec<[(Range<u16>, ImagePlacementId); 1]>,
}
```

OSC 8 hyperlink identities and image placements are rare and run-structured;
paying 2 bytes per cell for them (as `Cell::extra` would if used densely) is
wasteful, so `extra` is reserved for future per-cell needs and hyperlinks use
the run table.

### 2.4 Scrollback: blocks of logical lines

The core decision, borrowed as a *concept* from iTerm2's `LineBuffer`:

> **Scrollback stores logical (unwrapped) lines. Wrapping to a display width is
> computed on demand and memoized, never stored.**

```rust
pub struct Scrollback {
    blocks: VecDeque<LineChunk>,
    /// Prefix sums of wrapped-line counts per chunk, for the current width.
    index: WidthIndex,
    /// Absolute index of the oldest logical line still resident.
    base: LogicalLineIndex,
    /// Total logical lines ever evicted; makes positions monotonically stable.
    overflow: u64,
    limits: ScrollbackLimits,
    bytes: usize,   // running total of resident cell bytes
}

pub struct LineChunk {
    lines: Vec<Line>,          // ~256 logical lines per chunk
    /// Memoized wrapped-line count, keyed by the width it was computed for.
    wrapped_at: Cell2<(u16, u32)>,
    may_have_wide: bool,       // sticky fast-path flag
    gen: Generation,
    bytes: usize,
}
```

- **Chunking** at ~256 lines gives cheap head eviction (`pop_front` one chunk)
  and cheap COW snapshots (`Arc<LineChunk>` clone), while keeping the prefix-sum
  index short enough that "which logical line is display row N at width W" is
  `O(log chunks)` via binary search over `WidthIndex`, then `O(lines in chunk)`.
- **`may_have_wide`** is sticky: once a chunk has seen a double-width cluster it
  uses the slow wrap-counting path forever; otherwise wrapped count is
  `ceil(len / width)` arithmetic with no scan. The overwhelming majority of
  sessions never trip it.
- **`overflow`** is never reset. Every externally held coordinate is absolute
  (`AbsLine = overflow + relative`), so evicting scrollback does not invalidate
  a mark, a selection or a block boundary — it just makes it resolve to
  `Evicted`.

### 2.5 Bounding memory

Three independent caps, all enforced on append:

```rust
pub struct ScrollbackLimits {
    pub max_lines: u32,             // default 10_000
    pub max_bytes: usize,           // default 64 MiB per session
    /// Total resident image payload for this session. Distinct from
    /// `TermConfig::max_image_bytes` (§1.4), which caps a *single* image.
    pub max_image_bytes_total: usize, // default 64 MiB per session
}
```

Eviction drops whole chunks from the front until *all* caps are satisfied. The
chunk is therefore the unit of eviction as well as of storage, so the caps are
honoured **to within one chunk**, not exactly, and the last resident chunk is
never dropped — a single enormous line must make the terminal look like it
scrolled, not like it erased itself. At the default 64 MiB the overshoot is
noise; it only matters if a cap is configured smaller than a chunk. The
byte cap is the one that matters: a session that `cat`s a 200-column log holds
~13 KB per 64-line chunk, so 10 000 lines is ~2 MB; a session full of emoji and
hyperlinks can be 10× that, and only a byte cap catches it. Image payloads are
refcounted in a separate `ImageStore` and dropped when the last placement is
evicted.

There is deliberately **no compression tier** in v1. iTerm2 has one
(`CompressibleBuffers`); we note it as a future optimization and keep the
`Scrollback` API narrow enough (`chunk_at`, `evict_front`) that a compressed
cold tier can be added behind it without touching callers.

### 2.6 The grid (viewport)

```rust
pub struct Grid {
    rows: Vec<Line>,           // exactly `size.rows` entries, index 0 = top
    size: GridSize,            // cols, rows
    cursor: Cursor,
    saved_cursor: Option<Cursor>,
    scroll_top: u16, scroll_bot: u16,      // DECSTBM
    scroll_left: u16, scroll_right: u16,   // DECSLRM (mode 69)
    origin_mode: bool,
    pending_wrap: bool,        // cursor is "past" the last column
}

pub struct Screen {
    primary: Grid,
    alternate: Grid,
    active: Which,
    scrollback: Scrollback,    // primary only; alt screen never scrolls back
}
```

`pending_wrap` (the deferred-wrap / "last column" state) is explicit, because
getting it wrong is the classic source of off-by-one corruption when a program
writes exactly `cols` characters and then a control sequence.

The alternate screen has **no scrollback** and never emits block events (§6.5).

---

## 3. Reflow on resize

The hard part, and the place where a naive implementation destroys data.

### 3.1 The rule

> **Reflow is not an operation on the grid. It is: push the grid's used rows
> into scrollback, change the width, re-wrap logical lines at the new width,
> and pull rows back out. Every coordinate anyone holds is converted through a
> width-independent position.**

Because scrollback stores logical lines (§2.4), re-wrapping is a pure function
of `(logical lines, new width)`. There is no bespoke "join and re-split rows"
algorithm to get wrong.

### 3.2 Positions

```rust
/// A width-independent location in the session's content. Stable across
/// reflow; invalidated only by eviction.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    /// Absolute logical line index (includes evicted lines).
    pub line: u64,
    /// Offset in cells from the start of that logical line.
    pub offset: u32,
    /// True for a position meaning "end of this logical line", so that a
    /// selection to end-of-line stays at end-of-line at any width.
    pub to_eol: bool,
}
```

Everything durable is a `Position`: selections, search results, block
boundaries, marks, hyperlink anchors, agent attributions. Nothing durable is an
`(x, y)`. Resolving `Position → (row, col)` at the current width is a query;
it may return `Resolved(Point)`, `Evicted`, or `NotYetVisible`.

### 3.3 Algorithm

```rust
pub fn resize(&mut self, cols: u16, rows: u16) -> ResizeReport;

pub struct ResizeReport {
    pub converter: CoordConverter,   // old (row,col) → Position → new (row,col)
    pub lines_scrolled_off: u32,
    pub cursor: Point,
    pub blocks_reflowed: Range<BlockId>,
}
```

Steps, in order:

1. **Quiesce.** If a synchronized-output block (DECSET 2026) is open, resize is
   deferred until it closes or its 150 ms watchdog fires. Resizing mid-frame is
   the fastest way to tear a TUI.
2. **Capture** the cursor as a `Position`, and the saved cursor, and the current
   selection, as positions. Selections are first trimmed of leading and trailing
   default cells, because trimmed line storage means those cells do not exist.
3. **If the alternate screen is active**, reflow only the primary screen's
   content (which lives entirely in scrollback plus the primary grid) and
   *clear-and-resize* the alternate grid without reflow, then emit
   `HostAction::NotifyResize`. Full-screen applications redraw on `SIGWINCH`;
   attempting to reflow their frame produces garbage. This matches every serious
   terminal and is not a shortcut.
4. **Push used rows to scrollback.** The number of rows to push is chosen so
   that shrinking the window does not scroll live content off the top:

   ```rust
   let used = grid.used_height();               // last non-empty row + 1
   let n = if grid.rows.saturating_sub(new_rows) >= used {
       used.max(new_rows)
   } else if new_rows < grid.rows {
       used
   } else {
       grid.rows
   };
   ```

   Consecutive rows joined by `Wrap::Soft`/`SoftWide` are appended as *one*
   logical line, with `SoftWide` dropping the padding cell.
5. **Set the width** and invalidate `WidthIndex`. Memoized per-chunk wrapped
   counts whose key width differs are recomputed lazily, chunk by chunk, as the
   index is walked — a 100 000-line scrollback is not re-wrapped eagerly on a
   drag-resize.
6. **Pull rows back** into the new grid, newest-first, up to `new_rows`.
   `lines_scrolled_off` records how many wrapped rows the old viewport's top
   lost, which the renderer uses to keep the scroll anchor stable.
7. **Build the `CoordConverter`** and return it. Callers holding coordinates
   (`omt-session` selections, the web client's viewport anchor) apply it.

### 3.4 Cursor

The cursor is converted through its `Position`, with two refinements:

- If the cursor was in `pending_wrap`, the flag is preserved and re-evaluated
  against the new width; a narrower terminal may make it a real wrap.
- If the cursor's logical line now wraps to more rows than fit, the viewport
  scrolls so the cursor row is visible, and `ResizeReport::cursor` reports the
  final position. The cursor never lands outside the grid.
- The saved cursor (DECSC) is converted the same way. If its position was
  evicted, it collapses to `(0, 0)`, matching xterm's behaviour on scrollback
  loss.

### 3.5 Blocks and marks during reflow

Block boundaries are `Position`s, so **they survive reflow by construction** —
this is the whole reason for the position indirection. Concretely:

- A `Block`'s `prompt_start`, `command_start`, `output_start` and `output_end`
  are positions. After resize they resolve to different `(row, col)` but the
  same content.
- The **open block's** `output_end` is `None` until OSC 133 D; while open, its
  extent is "from `output_start` to the cursor", which is re-derived, not
  stored.
- If a block's `output_start` has been evicted, the block enters
  `Truncated { visible_from }`. It stays listable and searchable, but
  `blocks.get` reports partial content. Blocks whose *entire* range is evicted
  are dropped from the live index — their metadata (command, exit code, cwd,
  timing) has already been persisted by `omt-store`, so history is not lost.
- **Historical block re-wrap is lazy.** Following another terminal's pragmatic choice, only
  the blocks intersecting the current viewport (plus one screen of margin) have
  their wrapped-row counts recomputed synchronously; the rest are recomputed
  when scrolled into view. `WidthIndex` makes this natural because the per-chunk
  memoization is already lazy.

### 3.6 Property tests (see also §10)

Reflow must satisfy, for any content and any sequence of widths:

1. `reflow(w1); reflow(w2); reflow(w1)` yields content identical to
   `reflow(w1)` — width round-trips are lossless for logical content.
2. The concatenation of all logical lines is invariant under any resize.
3. Every `Position` that resolved before a resize either resolves after it or
   reports `Evicted`; it never resolves to *different* content.
4. Cursor position is inside the grid, always.

---

## 4. Damage tracking and the render contract

### 4.1 What a renderer gets

`omt-term` hands out two things and nothing else: an immutable `Snapshot`, and a
`Damage` record describing what changed since the last snapshot the renderer
acknowledged.

```rust
pub struct Snapshot {
    pub epoch: Epoch,              // bumped on resize / screen swap / reset
    pub seq: u64,                  // monotonic; matches the session event seq
    pub size: GridSize,
    pub cursor: CursorView,
    pub rows: Arc<[Arc<Line>]>,    // viewport rows, cheap to clone
    pub scroll: ScrollView,        // viewport offset into scrollback
    pub modes: ModeView,           // alt screen, mouse mode, paste mode, …
    pub palette: PaletteView,
}

pub struct Damage {
    pub epoch: Epoch,
    pub from_seq: u64,
    pub to_seq: u64,
    pub kind: DamageKind,
}

pub enum DamageKind {
    /// Nothing changed.
    None,
    /// Specific rows changed; each carries the changed column span.
    Rows(SmallVec<[RowDamage; 16]>),
    /// The viewport scrolled by `n` rows within `region`, plus residual rows.
    Scroll { region: Range<u16>, delta: i32, residual: SmallVec<[RowDamage; 8]> },
    /// Too much changed to describe; redraw everything.
    Full,
    /// Structural change (resize, alt-screen swap, RIS). Renderer must re-fetch
    /// a snapshot and discard all cached state.
    Reset,
}

pub struct RowDamage { pub row: u16, pub cols: Range<u16>, pub gen: Generation }
```

### 4.2 How damage is produced and capped

- Every `Line` carries a `Generation`. A renderer that cached row *r* at
  generation *g* can skip it when the snapshot reports the same *g* —
  **equal generations imply equal content**, and generations are drawn from a
  global monotonic counter so they never collide across copy-on-write clones.
- Per-row column spans are accumulated as a single `Range` (min..max), not an
  index set. Two writes at column 3 and column 180 mark 3..181 dirty. This
  over-reports slightly and costs nothing; an index set costs allocation in the
  hot loop.
- **Scroll is a first-class damage kind**, because it is the difference between
  a `cat` costing one row of work per line and costing a full screen. When the
  interpreter performs a region scroll it records `delta` instead of marking
  every row dirty.
- **The cap**: if dirty rows exceed `rows * 3 / 4`, or if `Rows` would exceed 64
  entries, damage collapses to `Full`. If `advance` is called many times before
  a renderer collects, damage merges; merging `Scroll` with a different `delta`
  or `region` collapses to `Full`. This bounds the damage record to O(rows)
  regardless of input volume — a fuzzer cannot make it grow.
- **Synchronized output (DECSET 2026)** suppresses damage emission entirely
  while the update is open, then emits one merged record. A watchdog closes the
  block after 150 ms so a buggy program cannot freeze the display.

### 4.3 The TUI mapping

`omt-tui` (ratatui) holds a per-row cache keyed by `Generation` and re-renders
only damaged rows into the ratatui `Buffer`; ratatui's own diff then handles the
terminal write. `DamageKind::Scroll` maps to shifting the cache and rendering
`residual` — the cache shift is the win, since ratatui will still emit a full
frame diff but over a buffer that mostly matches.

### 4.4 The web mapping (xterm.js)

The web client does **not** receive `Cell` structs. It receives one of two
encodings, chosen per subscription (see [07 — Remote protocol](07-remote-protocol.md)):

**(a) Byte-stream mode** — the default for the "full terminal" view. The daemon
forwards raw PTY bytes (from a resumable ring keyed by `seq`) and xterm.js does
its own emulation. `omt-term` stays authoritative for blocks, search and
scrollback; xterm.js is a pixel renderer. This is the lowest-latency path and
requires no serialization of grid state.

Two things make this correct rather than a hack, both specified on the wire by
[07 §4.2](07-remote-protocol.md#42-the-decision-c-hybrid-byte-stream-primary),
which owns the remote terminal-streaming format:
- On attach or resync, the daemon first sends a **snapshot** — the authoritative
  grid, run-length encoded as `grid_v1` — and then live bytes from that
  snapshot's `seq`. This avoids "attach shows a blank screen until output
  arrives". `omt-term`'s contribution is the state; the encoding is `omt-proto`'s.
  For the local TUI and for debugging, `Snapshot::to_ansi(&mut impl Write)` emits
  the equivalent SGR + cursor-positioning redraw; it is a pure, testable function
  of the snapshot and is not the remote path.
- Resize is authoritative on the daemon, and *which* client owns the
  authoritative size is [07 §4.3](07-remote-protocol.md#43-the-resize-problem)'s
  `ViewportPolicy`, not this crate's business. When the daemon reflows it emits a
  new `epoch`; the client calls `term.reset()` and takes a fresh snapshot on
  epoch change.

**(b) Structured mode** — used for the block list, the mobile default. The client
receives `BlockUpdate` events carrying, per block, the styled text as
`(text, spans)` where a span is `{start, len, fg, bg, flags, link?}`. No grid, no
cursor, no reflow — the browser re-wraps text with CSS. This is what makes a
phone usable: a 40-column phone renders a 200-column block by soft-wrapping
words, not by horizontally scrolling a grid. (A `native` session
([05 §1.3](05-session-model.md#13-session-modes-d8),
[D8](decisions.md#d8--two-session-modes-pty-default-and-native-acp)) is
structured by construction — it has no grid to derive structure from in the
first place — so this question does not arise for it.)

The two modes coexist in one session; the block list is the default view and
"open full terminal" upgrades that pane to byte-stream mode.

---

## 5. Escape-sequence support matrix

Priorities: **Must** = required for v1, a real terminal is wrong without it.
**Should** = v1.1, needed for parity with iTerm2/WezTerm on real workloads.
**Later** = tracked, not scheduled.

### 5.1 Core CSI / ESC

| Sequence | Priority | Rationale |
|---|---|---|
| `CUU/CUD/CUF/CUB/CNL/CPL/CHA/CUP/VPA/HVP` | Must | Cursor motion; nothing works without it |
| `ED (0/1/2/3)`, `EL (0/1/2)` | Must | `clear`, every TUI |
| `IL/DL/ICH/DCH/ECH` | Must | Line editing; readline and every editor |
| `SU/SD` (scroll up/down) | Must | Pagers |
| `DECSTBM` (scroll region) | Must | Every full-screen app; also the damage `Scroll` fast path |
| `DECSC/DECRC`, `ESC 7/8` | Must | Save/restore cursor + SGR + charset |
| `RIS` (`ESC c`), `DECSTR` (`CSI ! p`) | Must | `reset`; must clear alt screen, modes, margins, charsets |
| `IND/RI/NEL/HTS/TBC`, `CSI g` | Must | Tabs and index; `tput` relies on them |
| `DA1/DA2/DA3` | Must | Applications probe before enabling features; a wrong answer disables truecolor |
| `DSR 5/6`, `DECDSR ?6` | Must | Cursor position report; shells use it for prompt width |
| `DECSCUSR` (cursor shape) | Should | vim/nvim mode indication; cheap |
| `REP` (repeat) | Should | Emitted by some `ncurses` optimizers |
| `DECSLRM` + mode 69 (left/right margins) | Should | Rare but load-bearing when used; gated on emulation level ≥ 400 |
| `DECCARA/DECRARA/DECCRA/DECFRA/DECERA` (rectangular ops) | Later | Almost unused outside DEC test suites |
| `DECDHL/DECDWL/DECSWL` (double-height/width lines) | Later | Only `banner`-style toys; needs renderer support on three surfaces |
| `SPA/EPA` guarded areas | Later | Flag bit reserved (`PROTECTED`); no behaviour in v1 |
| `XTPUSHSGR/XTPOPSGR` | Should | Used by modern prompt frameworks to avoid SGR leakage |
| `XTPUSHCOLORS/XTPOPCOLORS/XTREPORTCOLORS` | Later | Niche |
| `XTVERSION` (`CSI > 0 q`) | Should | Lets programs detect omt and enable features |
| Window ops (`CSI t`: report size/position) | Should | Report subset only. **Never** implement iconify/raise/lower/set-title-by-report — those are the classic escape-sequence attack surface |
| `SET_MODIFIERS` (`CSI > Ps ; Pm m`) | Later | Superseded by the Kitty keyboard protocol |
| SCS charsets G0–G3, DEC Special Graphics | Must | Box-drawing in older TUIs and in `dialog`/`whiptail` still uses it |

### 5.2 SGR

| Feature | Priority | Rationale |
|---|---|---|
| `0,1,2,3,4,5,7,8,9` and their resets `21–29` | Must | Baseline |
| 16-colour `30–37/40–47`, bright `90–97/100–107` | Must | Baseline |
| 256-colour `38;5;n` / `48;5;n` | Must | Universal |
| **Truecolor `38;2;r;g;b` / `48;2;r;g;b`** | Must | Native in `Cell::Color`; the single most-noticed missing feature |
| Colon-separated subparameters `38:2::r:g:b` | Must | The ITU-standard form; emitted by `neovim` and `delta`. `vte` surfaces subparameters, so this is nearly free |
| Underline styles `4:1–4:5` | Should | Curly underline for diagnostics is table stakes in editors |
| Underline colour `58` / `59` | Should | Ships with 4:3; needs a slot in `ExtraAttrs` |
| Overline `53` | Later | Rare |

### 5.3 DEC private modes

| Mode | Priority | Rationale |
|---|---|---|
| 1 DECCKM (cursor key application mode) | Must | Arrow keys in vim/readline are wrong without it |
| 6 DECOM (origin mode) | Must | Interacts with DECSTBM; getting it wrong corrupts scroll regions |
| 7 DECAWM (autowrap) | Must | Baseline |
| 12 (cursor blink) | Should | Cosmetic, trivially cheap |
| 25 DECTCEM (cursor visible) | Must | Every TUI hides the cursor |
| 45 (reverse wraparound) | Should | Backspace over a wrap in readline |
| 47 / 1047 / 1048 / **1049** (alt screen) | Must | 1049 is what everything uses; the other three must behave per the xterm truth table |
| 66 DECNKM (keypad) | Should | Numeric keypad in `emacs` |
| 69 DECLRMM | Should | Enables DECSLRM; see §5.1 |
| 1000 / 1002 / 1003 mouse | Must | Click, drag, motion. `1001` highlight tracking: never |
| 1004 focus reporting | Must | Agents and editors use it; **policy-gated** (§5.6) |
| 1005 (UTF-8 mouse ext) | Later | Superseded and broken by design |
| **1006 SGR mouse** | Must | The only mouse encoding that works past column 223 |
| 1007 alternate scroll | Should | Scroll wheel → arrow keys in pagers; policy-gated |
| 1015 (urxvt mouse) | Later | Legacy |
| 1016 SGR-pixel mouse | Later | Only useful with pixel-accurate rendering |
| 1036 (meta sends escape) | Should | Alt-key behaviour in emacs/readline |
| **2004 bracketed paste** | Must | Security-relevant: without it, a pasted `\n` executes. Policy-gated |
| **2026 synchronized output** | Must | The difference between a smooth and a tearing TUI; also our damage-batching signal |
| 2031 (palette change notification) | Should | Dark-mode-aware TUIs; we can answer authoritatively |
| 2048 (in-band resize) | Should | Lets an app learn pixel size without `ioctl`; nice with the web client |
| 2 (VT52 mode) | **Never** | Deliberately unimplemented, following iTerm2's judgement — it breaks everything and is used by nothing |
| 3 DECCOLM (80/132) | Later | Destructive and surprising; off by default, gated behind mode 40 |
| 9 (X10 mouse) | Later | Obsolete |

### 5.4 OSC

| OSC | Priority | Rationale |
|---|---|---|
| `0/1/2` title | Must | Tab titles; feeds `session.title` |
| `4` set palette entry, `104` reset | Must | Colour schemes; `vim` sets palette entries |
| `10/11/12` fg/bg/cursor + `110/111/112` reset | Must | Needed for correct default colours and for 2031 |
| **`7` current working directory** | Must | Powers workspace identity, new-pane-in-same-dir, and block `cwd`. See [05](05-session-model.md) |
| **`8` hyperlinks** | Must | `ls --hyperlink`, `delta`, `cargo`. Stored as `ExtraAttrs` runs |
| `9` notification | Should | Maps to an `omt` notification event; **rate-limited** |
| **`52` clipboard — write only** | Must (write) / **Never (read)** | Write is sanitized (strip C0 except TAB/LF/CR, cap at 1 MiB) and policy-gated. Read is refused unconditionally: an unprivileged remote process must never be able to exfiltrate the clipboard. iTerm2 reached the same conclusion |
| **`133` semantic prompt A/B/C/D** | Must | The entire block model (§6) |
| **`1337 ; CurrentDir=` / `RemoteHost=`** | Must | Cheap, widely emitted, complements OSC 7 |
| **`1337 ; SetUserVar=`** | Should | The sanctioned shell→terminal side channel; useful for agent attribution |
| `1337 ; SetMark` | Should | User marks, jump-to-mark |
| `1337 ; UnicodeVersion=` | Should | Width-table agreement with remote hosts (§2.2) |
| `1337 ; File=` (inline image / download) | Should | `imgcat` compatibility; one transport, `inline=0` means download. See [09](09-ssh-and-media.md) |
| `1337 ; MultipartFile=`/`FilePart=`/`FileEnd` | Should | Required so large images stream instead of buffering |
| `1337 ; RequestUpload=` | Should | The "paste a local image into an ssh'd session" flow |
| `1337 ; Copy=` (chunked clipboard) | Later | OSC 52 covers text; chunked clipboard is a `it2copy` nicety |
| `22` pointer shape | Later | Cosmetic |
| `6`, `17/19`, `21337` | Later | iTerm2-specific |
| **omt backchannel** | Must | See §7.3. Uses `OSC 133` + `OSC 1337;SetUserVar=` rather than a new number |

### 5.5 Keyboard, graphics, misc

| Feature | Priority | Rationale |
|---|---|---|
| **Kitty keyboard protocol** (`CSI > 1 u` push / `CSI < u` pop / `CSI = Ps ; Pm u`) | Must, flags 0b1–0b1111 | Agent CLIs and modern editors need unambiguous `Ctrl+I` vs `Tab`, `Shift+Enter`, and key-release. This is not optional for omt's audience. Progressive-enhancement stack semantics must be implemented, including per-alt-screen stacks |
| **Bracketed paste** | Must | See 2004 |
| Kitty graphics protocol (APC `G`) | Should | Transmission (`a=t/T`), placement (`a=p`), delete (`a=d`), chunked `m=1`. Unicode placeholders and animation: Later |
| Sixel (`DCS … q`) | Should | Still the only graphics many tools emit; budgeted hook (§1.4) |
| iTerm2 inline images | Should | See OSC 1337 above |
| XTGETTCAP (`DCS + q`) | Should | Lets programs query capabilities without terminfo |
| tmux control-mode passthrough | **Never** | Explicitly out of scope per [00 §8](00-overview.md) |
| DCS `$q` (DECRQSS) | Should | `DECSTBM`/`SGR` readback; used by `tmux` and `vim` |

### 5.6 Two orthogonal gating axes

Both exist from day one, mirroring iTerm2's design:

```rust
pub struct TermPolicy {
    pub emulation_level: EmulationLevel,   // Vt100 | Vt220 | Vt420 | Vt520
    pub allow_clipboard_write: bool,       // default true, size-capped
    pub allow_clipboard_read: bool,        // default false; there is no "true" path in v1
    pub allow_focus_reporting: bool,       // default true
    pub allow_bracketed_paste: bool,       // default true
    pub allow_window_ops: WindowOpPolicy,  // default ReportOnly
    pub allow_alternate_scroll: bool,      // default true
    pub allow_file_transfer: bool,         // default false until session is trusted
    pub allow_notifications: RateLimit,    // default 1 per 3 s
    pub trusted: bool,                     // set by omt-session, not by the wire
}
```

`trusted` is never settable by an escape sequence. Nothing arriving over the PTY
can raise its own privileges — that is the terminal analogue of
[P8](01-principles.md#p8--security-by-default-no-ambient-trust).

---

## 6. The block model

### 6.1 Why blocks

A phone cannot render a 200×50 grid. It can render a scrollable list of
collapsible cards, each showing a command, its status, its duration, and its
output — with the output soft-wrapped to the phone's width. **Blocks are the
data structure that makes the mobile client a first-class surface rather than a
VNC viewer**, and per
[P3](01-principles.md#p3--parity-one-capability-three-surfaces) mobile is a
target, not a fallback.

They also pay for themselves on the desktop: jump to previous command, copy just
this output, re-run, filter within a command's output, and — critically for omt —
attribute a command to the agent that ran it.

### 6.2 What a block owns

Unlike another terminal, an omt `Block` **does not own grids**. It owns metadata plus a
range of positions into the one shared scrollback. another terminal's three-grid design is
tied to another terminal owning the input line; omt runs the user's real shell in a real PTY
([P4](01-principles.md#p4--native-semantics-observe-never-re-implement)), so the
shell owns the prompt and omt must not fragment the byte stream.

```rust
pub struct Block {
    pub id: BlockId,                 // stable, monotonic, never reused
    pub state: BlockState,
    pub origin: BlockOrigin,         // Osc133 | Heuristic | Injected

    // Extent — all width-independent positions (§3.2).
    pub prompt: Option<Range<Position>>,
    pub command: Option<Range<Position>>,
    pub output: Range<Position>,     // end == start while running

    /// Command text as the shell reported it (OSC 133 B..C region or the
    /// `omt` hook), never re-derived from pixels once we have a real source.
    pub command_text: Option<String>,
    pub exit: Option<ExitStatus>,
    pub cwd: Option<PathBuf>,        // from OSC 7 / OSC 1337 CurrentDir at prompt time
    pub host: Option<RemoteHost>,    // from OSC 1337 RemoteHost
    pub git: Option<GitContext>,     // branch, head, dirty — from the shell snippet
    pub started_at: Option<Timestamp>,
    pub finished_at: Option<Timestamp>,
    pub attribution: Attribution,    // who ran it — see below
    pub folded: bool,
}

pub enum ExitStatus { Code(i32), Signal(i32) }

pub enum Attribution {
    Human { client: Option<ClientId> },
    Agent { kind: AgentKind, run_id: Option<String> },
    Unknown,
}
```

**Attribution** is what makes blocks worth more in omt than in another terminal. When the
agent layer ([06](06-agent-layer.md)) knows that a `Bash` tool call is in flight,
it stamps the block. The result: a phone shows "claude ran `cargo test` — failed
in 42 s" as a card, and the human answers from there. `omt-term` does not compute
attribution; it accepts it via `Terminal::attribute_open_block(Attribution)` and
stores it. Keeping the policy out of the state machine keeps `omt-term` pure.

**Failure is not `exit_code != 0`.** `Block::failed()` returns true only when
`state == Finished` and the code is non-zero *and* not 130 (SIGINT) or 141
(SIGPIPE). Colouring a Ctrl-C'd block red is a well-known papercut; we avoid it
from the start.

### 6.3 Lifecycle

```rust
pub enum BlockState {
    AtPrompt,       // OSC 133 A seen; shell is drawing/reading the prompt
    Submitted,      // OSC 133 B seen; command line captured, not yet executing
    Running,        // OSC 133 C seen
    Finished,       // OSC 133 D seen (with or without an exit code)
    NoExecution,    // prompt → prompt with no C (empty Enter, Ctrl-C at prompt)
    Background,     // output arrived with no owning command (§6.4)
    Truncated { visible_from: Position },  // head evicted from scrollback
}
```

Transitions (`input × state → action`), a total function with no panics:

| State | `A` | `B` | `C` | `D` | Output bytes | Alt-screen enter |
|---|---|---|---|---|---|---|
| *(none)* | open `AtPrompt` | open `Submitted` (tolerate missing A) | open `Running` (tolerate missing A,B) | ignore | open `Background` (lazily, §6.4) | — |
| `AtPrompt` | close as `NoExecution`, open new `AtPrompt` | → `Submitted` | → `Running` (missed B) | close `NoExecution` | extend prompt region | suspend |
| `Submitted` | close `NoExecution`, open new | ignore (coalesce) | → `Running` | close `NoExecution` (Ctrl-C before exec) | extend command region | suspend |
| `Running` | **close as `Finished{unknown}`**, open new `AtPrompt` | ignore | ignore (repeated C) | → `Finished` | extend output | suspend |
| `Finished` | open new `AtPrompt` | open new `Submitted` | open new `Running` | ignore | open `Background` | suspend |

Three properties are deliberate and each corresponds to a real failure mode:

- **`A` while `Running` self-heals.** A missed or garbled `D` (the command was
  killed, the shell crashed, output was truncated by a slow ssh link) must not
  wedge the block list forever. The next prompt closes the stale block with an
  unknown exit status.
- **`C` while `Submitted` is the only path to `Running`.** A second `C` is
  ignored, so a shell that emits `C` in both `preexec` and a `DEBUG` trap does
  not double-open.
- **Alt-screen suspends the machine entirely.** While the alternate screen is
  active, OSC 133 marks are *recorded and discarded*, not applied. A vim session
  that emits prompt marks from a `:terminal` buffer must not shred the outer
  block list. On alt-screen exit the machine resumes in its pre-entry state.
  This is the single most important rule in the section.

### 6.4 The fallback heuristic — no shell integration

Most users, the first time they run omt, have no integration installed. The
block list must still be useful, and it must never be *wrong* in a way that
loses data.

**The design: never guess a prompt. Segment on activity instead.**

```rust
pub struct HeuristicSegmenter {
    idle_after: Duration,       // default 300 ms
    min_content_rows: u16,      // default 1
    pending: Option<PendingBlock>,
}
```

Rules:

1. When output arrives with no open block, do **not** create one immediately.
   Buffer it. Shells emit reset sequences, right-prompt repaints and empty lines
   between commands; creating a block per burst produces a list of empty cards.
2. Create a `Background` block once the buffer has at least `min_content_rows`
   of non-blank content, anchored at the position where the buffer started.
3. Close the `Background` block when either (a) the PTY has been quiet for
   `idle_after` **and** the foreground process group has returned to the shell
   (a signal `omt-session` supplies via `Terminal::set_foreground_hint`), or (b)
   a real OSC 133 `A` arrives.
4. **Retroactive repair.** If a real OSC 133 `C` with a command arrives while a
   `Background` block is open and that block's content is short and looks like
   an echoed command line, reclassify: the background block's content becomes
   the new block's `command` region and the block is promoted to `Running`. This
   handles the very common case of integration loading *after* the first prompt.
5. **Typeahead does not pollute the next block.** If the user types while a
   command runs and the command does not read it, the shell echoes it at the
   next prompt. With integration we get the input buffer explicitly; without it,
   text arriving between `D` and the next `A` is attached to the *closing*
   block's tail, not the opening one.

What the heuristic explicitly does **not** do: match prompt regexes. another tool and
several others derive structure from prompt-shaped strings, and it fails on
Starship, on multi-line prompts, on any non-English locale, and on any program
that prints a `$`. Per
[P4](01-principles.md#p4--native-semantics-observe-never-re-implement),
structured output requires a structured source. A heuristic block is always
flagged `origin: Heuristic` in the event stream and in the API, so every surface
can render it as "unstructured output" rather than claiming an exit code it does
not have.

The upgrade path is explicit: when a session has no integration, the block list
UI shows a one-tap "enable shell integration" affordance (§7).

**The segmenter must never open a block it cannot close.** Rule 3's first close
condition is *unreachable* whenever the foreground process never returns to the
shell. A long-lived agent CLI is exactly that case: the foreground process group
is the agent for the entire session, so the quiet-plus-returned test never
becomes true; and because there is no shell in the loop, no real OSC 133 `A`
ever arrives either, so the second condition never fires. Both close conditions
being unreachable, a naive segmenter would produce one unbounded, never-closing
`Background` block whose contents are a cursor-addressed redraw stream flattened
to lines.

Note that §6.3's alt-screen suspension rule does not save us here either, and
**not for the reason an earlier draft of this section gave.** That draft stated
"Claude Code is *not* an alt-screen program", which is a *default*, not a
property. [`spike-card-answering.md` §6](../research/spike-card-answering.md)
established VERIFIED-LIVE that v2.1.220 ships a second renderer:

```js
tui: E.enum(["default","fullscreen"]).optional().describe(
  'Terminal UI renderer. "fullscreen" uses the flicker-free alt-screen renderer
   with virtualized scrollback (equivalent to CLAUDE_CODE_NO_FLICKER=1).
   "default" uses the classic main-screen renderer.')
```

reachable via `/tui fullscreen`, `CLAUDE_CODE_NO_FLICKER=1`, or
`viewMode: "focus"` — and shipping with an **active in-product upsell**
(`fullscreen-upsell`, `fullscreenUpsellSeenCount`, plus a downsell survey for
switching back) that is deliberately growing the alt-screen population. In the
default renderer `ESC[?1049h` appears zero times across ten captures; what
Claude Code emits at startup is `ESC[?2004h`, `ESC[?1004h`, `ESC[?2031h` and a
kitty-keyboard push/pop. So both renderers exist in one shipped version and the
choice is the user's.

**Therefore the alternate screen is detected at runtime, never assumed.**
`omt-term` already tracks `ESC[?1049h` / `ESC[?1049l` for §6.3's suspension
rule; that same signal is exported per session as a capability input, and
entering the alt screen **downgrades capabilities** rather than merely
suspending segmentation:

- Block segmentation is suspended outright (§6.3) — which only *adds* intervals
  where the block view is unavailable, so D14's conclusion is strengthened, not
  weakened.
- The **transcript view is unaffected**: it is fed by the agent event stream,
  not by the grid ([D14](decisions.md#d14--agent-sessions-get-a-transcript-surface-blocks-are-for-shell-work)),
  so it is the surface that keeps working in `fullscreen`.
- **Remote card answering is disabled** while the pane is on the alternate
  screen. The gated transaction's preconditions are screen-derived
  ([12 §4.6](12-collaboration.md#46-preconditions-on-a-synthetic-delivery),
  precondition P5) and cannot be trusted against a virtualised scrollback omt
  does not model. The card is shown read-only with the reason and the terminal
  view one tap away.

**D14's conclusion is unaffected by any of this**, and its stated reason in
[`decisions.md`](decisions.md#d14--agent-sessions-get-a-transcript-surface-blocks-are-for-shell-work)
is already the correct one: *there is no shell in the loop, therefore no
OSC 133* — true in **both** renderers, whether or not the alternate screen is in
use. It was this section that carried the wrong premise.

Therefore, per [D14](decisions.md#d14--agent-sessions-get-a-transcript-surface-blocks-are-for-shell-work):

- **Block segmentation is suppressed for a session that has an agent binding and
  has never observed an OSC 133 sequence.** The segmenter does not open a
  `Background` block at all in that state; it reports `segmentation:
  suppressed { reason: agent_session_no_osc133 }` so every surface can say why
  the block list is empty rather than showing an empty list.
- Suppression is lifted the moment a real OSC 133 mark arrives (the agent exited
  and an integrated shell is drawing a prompt again), and §6.4 rule 4's
  retroactive repair applies from that point forward.
- The mobile surface for such a session is the **transcript view**, not the
  block view — see [06 §7.3](06-agent-layer.md) for what each coverage tier can
  supply and [08 §4](08-web-client.md) for the view itself.

### 6.5 Interface facts borrowed from another terminal

For the record, and to make the clean-room boundary explicit
([P9](01-principles.md#p9--clean-room-with-respect-to-studied-code),
[14](14-licensing.md)): from the another terminal study we take **only observations about
the problem**, not code and not structure —

- that a lifecycle state machine needs an absorbing terminal state and explicit
  ignore reasons;
- that alt screen must swallow hooks;
- that a background-output fallback plus a repair path beats prompt regexes;
- that 130/141 must not read as failure;
- that reflowing all historical blocks on drag-resize is not viable.

We do **not** take another terminal's three-grid block, its sum-tree, its OSC 9278 JSON hook
protocol, its serialization format, or any of its code. omt's block owns
positions rather than grids, and omt's wire format is standard OSC 133.

---

## 7. Shell integration

### 7.1 What omt ships

`omt integrations install [--shell auto]` writes snippets to a
version-stamped file under `~/.config/omt/shell/` and appends one `source` line
to the user's rc file (with a marked, idempotent block that
`omt integrations uninstall` removes exactly). Supported: **bash, zsh, fish,
pwsh**. Bash uses a vendored `bash-preexec` (MIT — attribution recorded in
[14](14-licensing.md)); zsh uses native `precmd_functions`/`preexec_functions`;
fish uses `fish_preexec`/`fish_prompt` events; pwsh wraps `prompt`.

### 7.2 What they emit — standard OSC 133, not a private namespace

**Decision: omt emits and consumes standard `OSC 133` and `OSC 7`, plus
`OSC 1337;SetUserVar=` for the extra metadata. omt does not define a private OSC
number for shell hooks.**

Rationale:

1. **Consumption is where the value is.** A large fraction of users already have
   OSC 133 from Starship, oh-my-zsh, the iTerm2 or WezTerm integrations, or
   `vscode-shell-integration`. Consuming the standard means those users get real
   blocks with zero setup — which is worth far more than the extra metadata a
   private protocol would carry.
2. **Emitting the standard makes omt a good citizen.** A user who installs omt's
   snippet and later opens the same shell in WezTerm or VS Code still gets
   working marks. A private namespace would make our snippet worthless
   everywhere else, which is a reason for users not to install it.
3. **The extra metadata fits in existing standards.** `cwd` is OSC 7. Exit code
   is OSC 133 D's positional argument. Everything else — git branch, virtualenv,
   node version, the omt session correlation id — is a key/value, and
   `OSC 1337;SetUserVar=<name>=<base64>` is an existing, widely-implemented
   channel for exactly that. Inventing `OSC 92xx` to carry JSON would buy
   marginal efficiency at the cost of interoperability.
4. **Attack surface.** A JSON-over-OSC hook protocol invites a remote process to
   forge state. Our snippet includes a per-session nonce in
   `SetUserVar=omt_session`, and `omt-term` ignores metadata whose nonce does not
   match the session's — the same anti-spoofing property, without a new parser.

Emitted, per prompt cycle (zsh shown; the others are equivalent):

```
precmd:   OSC 133 ; D ; <exit> ; aid=<nonce>   ST      # close previous
          OSC 7 ; file://<host><pwd>           ST
          OSC 1337 ; SetUserVar=omt_git=<b64>  ST      # branch|head|dirty
          OSC 1337 ; SetUserVar=omt_env=<b64>  ST      # venv|conda|node, if set
PS1:      OSC 133 ; A ; aid=<nonce> ST  …prompt…  OSC 133 ; B ; aid=<nonce> ST
preexec:  OSC 133 ; C ; aid=<nonce>          ST
```

Implementation constraints taken directly from the studied scripts, because they
are hard-won facts about shells rather than anyone's code:

- **`D` before the expensive part.** Emit `D` with the exit code first (cheap,
  closes the block immediately), then gather git/env state, then emit `A`. The
  UI must not wait on `git` to close a block.
- **Never shell out in the hot path** beyond one `git` call, and that call is
  `GIT_OPTIONAL_LOCKS=0 git ...` so it never takes the index lock.
- **Wrap marks in `%{…%}` (zsh) / `\[…\]` (bash)** so the prompt's width maths
  stays correct. This is the number-one bug in hand-rolled integrations.
- **Capture `$?` as the first statement of `precmd`**, before anything else can
  clobber it.
- **Handle Ctrl-C at the prompt**: if `precmd` runs without a preceding
  `preexec`, the state machine would desync; the snippet detects this and the
  terminal-side machine already tolerates it via the `A`-while-`Submitted`
  transition (§6.3), so the snippet does not need to synthesize anything.
- **Refuse to install** when non-interactive, when `TERM` is `dumb`, or when
  already installed (checked via `OMT_SHELL_INTEGRATION` version stamp).

### 7.3 Propagation into subshells and over ssh

Three tiers, in preference order:

1. **omt-spawned processes** inherit `OMT_SESSION`, `OMT_SHELL_INTEGRATION` and
   the nonce through the environment, so any subshell re-sources the snippet
   from `~/.config/omt/shell/` automatically. This covers `bash -l`, `tmux`
   (which we do not observe through, but the shell inside still emits marks),
   `sudo -E`, and `docker exec` with `-e`.
2. **ssh, no remote install.** omt detects an interactive `ssh` invocation
   (argv parse: no `-T`, no `-W`, no trailing remote command) and, once the
   remote shell reaches a prompt, offers a one-keystroke "enable integration
   here" action. Accepting appends a small snippet to the remote rc file — the
   snippet is a *self-contained emitter*, not a `source` of a local file, and
   installs no binary on the remote host. Nothing is written without explicit
   confirmation; this is an outward-facing change to someone's machine.
3. **Degradation.** If neither applies, the heuristic segmenter (§6.4) runs, and
   the block list is labelled `origin: Heuristic`.

Detecting readiness for step 2 is genuinely heuristic (watching for `Last
login:`, password/passphrase prompts, or a prompt-shaped trailing character).
Because the consequence of a wrong guess is only "the offer appears at a slightly
wrong moment", and never a data change, a heuristic is acceptable here — unlike
in block segmentation.

---

## 8. Search, selection, hyperlinks, semantic click targets

### 8.1 Search

```rust
pub struct Query {
    pub pattern: Pattern,        // Literal { s, case: Case } | Regex(regex::Regex)
    pub direction: SearchDirection,   // Forward | Backward
    pub scope: Scope,            // Viewport | Scrollback | Block(BlockId)
    pub wrap: bool,
}

/// A resumable cursor. Searching a 100k-line scrollback must never block the
/// caller for more than one budget's worth of work.
pub struct SearchCursor {
    at: Position,
    budget_lines: u32,      // default 5_000 per call
    seen_wrap: bool,
}

pub struct Match { pub range: Range<Position>, pub line_preview: String }
```

- Search runs over **logical lines**, so a match spanning a soft wrap is found
  naturally — no "soft boundary extender" special case is needed. This falls out
  of storing unwrapped lines and is one of the clearest wins of that decision.
- Results are `Position`s, so they survive reflow.
- Literal search uses `memchr`-accelerated substring search over a per-line
  lazily-built `String` (built only for lines that could match, filtered by a
  cheap byte-level prefilter). Regex uses the `regex` crate with a compiled
  `RegexSet` prefilter when multiple patterns are active.
- Case-insensitive matching folds ASCII inline and falls back to full Unicode
  simple case folding only when the pattern has non-ASCII.
- **Live tail search**: a cursor at the end of the buffer can be re-driven as new
  lines arrive, which is what powers "highlight all matches while output
  streams".

### 8.2 Selection

```rust
pub enum SelectionMode { Character, Word, Line, Block /* rectangular */, Semantic }
pub struct Selection { pub anchor: Position, pub head: Position, pub mode: SelectionMode }
```

`selection.to_string(&Snapshot, TrailingWhitespace)` reconstructs text from
logical lines: soft wraps join without a newline, hard wraps emit `\n`, tab
origins restore `\t`, and `WIDE_SPACER` cells are skipped. Rectangular selection
is the one mode that operates on display coordinates and is therefore
re-anchored (not preserved) across resize.

Word boundaries use a configurable character class
(`word_chars`, default `[A-Za-z0-9_./-]` plus letters/digits by Unicode
category) so that `--flag=value` and `src/main.rs` select sensibly.

`SelectionMode::Semantic` is **not** `word_chars` widened. Per
[18 §5.4](18-semantic-open.md#54-selection-versus-click) it snaps to the span of
the `Match` under the anchor — so a semantic selection over `src/lib.rs:42`
takes the whole target including the line suffix, and falls back to `Word` when
no match covers the position.

### 8.3 Hyperlinks and detection

Two sources, in priority order:

1. **Explicit OSC 8.** Authoritative. Stored as `ExtraAttrs::hyperlinks` runs
   with an interned `HyperlinkId → (uri, id_param)`. Runs with the same `id`
   parameter on different lines are one logical link (so hovering a wrapped URL
   highlights all of it).
2. **Detected.** A scanner runs over completed logical lines (never over the
   line under the cursor, which changes constantly) producing:

```rust
pub enum Target {
    Url(Url),
    /// A path, optionally with line and column. `file:line:col`, `file(line,col)`,
    /// `file", line N` (Python/Ruby tracebacks), `at file:line:col` (JS stacks).
    Path { raw: String, line: Option<u32>, col: Option<u32> },
    /// A git object reference — SHA, tag or branch name.
    GitRef { raw: String },
    /// An issue or PR reference (`#123`, `ORG/REPO#123`, `PROJ-456`).
    Issue { raw: String, repo: Option<String> },
    /// User-defined regex rules with attached actions (§8.4). Captures are
    /// **named slots** (`{path}`, `{line}`, …), not positional ranges, so a
    /// rule's action template can refer to them stably.
    Custom { rule: RuleId, slots: SmallVec<[(Slot, Range<u16>); 4]> },
}
```

`GitRef`, `Issue` and the named-slot form of `Custom` are required by
[18 §2.3](18-semantic-open.md#23-match-kinds); `omt-term` produces them, and
`omt-open` resolves and acts on them.

Detection is **budgeted and lazy**: it runs on lines entering the viewport, with
a per-frame cap, and results are cached on the line keyed by `Generation`.

> **Where `Target` lives.** [18 §1.2](18-semantic-open.md#12-the-new-crate)
> widens `Target` (and `Match`) and **moves both into `omt-types`**, so that
> `omt-open` can consume them without an L2 dependency exception; the
> [crate map](02-crate-map.md) records the same. The definition text stays here
> because this is where detection is specified: `omt-term` remains the *producer*
> of matches, and this document owns the scanner. Only the type's home crate
> changes.

### 8.4 Semantic click targets

`Terminal::target_at(Position) -> Option<Target>` is a pure query. Turning a
`Target` into an action — resolving a relative path against the block's `cwd`,
`stat`ing it, opening an editor — is **not** `omt-term`'s job; it happens in
`omt-session` and is exposed as a capability so a phone can act on it too:

```
open.targets.list   { session, position }         -> [Target]
open.resolve        { session, position }         -> ResolvedTarget
```

Both belong to the `open.*` group, declared in
[18 §9](18-semantic-open.md#9-capabilities); this document does not restate their
shapes.

`ResolvedTarget` carries the absolute path (resolved against the owning block's
`cwd`, which is why blocks carry `cwd`), whether it exists, and the suggested
action. `stat` results are cached with a short TTL — naive existence checking on
every mouse move is the classic performance killer here.

---

## 9. Performance

### 9.1 Targets

Measured on a 2020-class laptop, single session, release build:

| Workload | Target |
|---|---|
| `cat` of a 100 MB ASCII log, no renderer attached | ≥ 400 MB/s sustained through `advance` |
| Same, with TUI attached at 60 fps | ≥ 200 MB/s; frame time p99 < 8 ms |
| `advance` of a 64 KiB chunk of mixed SGR + text | < 250 µs |
| Resize (reflow) of a 10 000-line scrollback | < 30 ms p99 |
| Search over 100 000 logical lines, literal | < 60 ms |
| Steady-state memory, 10 000-line scrollback, 200 cols | < 8 MB per session |
| Idle session (no output) | 0 allocations, 0 wakeups |

### 9.2 The `cat` fast path

The single most important optimization. When the parser is in the ground state
and the next bytes are printable ASCII with no ESC/C0 except `\r`/`\n`:

1. `memchr3(b'\x1b', b'\r', b'\n')` finds the end of the run in one SIMD pass.
2. The run is written into the current line with a single `extend_from_slice` of
   pre-styled cells — style is constant across the run, so cells are built by
   filling a stack array with the current `(fg, bg, flags)` and splatting the
   characters in.
3. Wrapping is computed arithmetically (`run_len` vs `cols - cursor.x`) rather
   than per character, because no character in an ASCII run is wide or
   combining.
4. `\n` at the end of the run is handled by the scroll fast path, which records
   `DamageKind::Scroll` rather than dirtying rows.

The result is that the dominant cost of `cat` is `memcpy` plus one damage
record per frame, not per byte. Everything else — the full `vte` state machine,
grapheme segmentation, width lookup — is on the slow path where it belongs.

### 9.3 Batching and coalescing

- `omt-session` reads the PTY into a 64 KiB buffer and calls `advance` once per
  read, not once per byte. Under load it drains up to `max_batch` (default
  1 MiB) before yielding, so a burst is one pass.
- Damage is collected by the renderer at its own cadence (60 Hz foreground,
  4 Hz when the pane is not visible, 1 Hz when no client is attached). This is
  the "backgrounded tab costs nothing" property, and it is worth an order of
  magnitude on a machine running ten agent panes.
- Terminal *events* to the event bus (block opened/closed, title changed, bell)
  are emitted immediately — they are rare and latency matters. Grid damage is
  never an event; it is polled.

### 9.4 Damage capping

Covered in §4.2. The invariant to test: for any input, the size of one `Damage`
record is O(rows), and the work to produce it is O(cells actually written) plus
O(rows).

### 9.5 Benchmark plan

`benches/` with `criterion`, run in CI on every PR with a 10 % regression
threshold: `throughput_ascii` (64 MiB of words), `throughput_sgr`
(`ls --color -R` capture), `throughput_tui` (recorded `htop`/`nvim`, heavy cursor
addressing + 2026), `throughput_unicode` (CJK + emoji, forces the slow path),
`reflow` (10k/100k lines, widths 80→200→40→80), `search_literal`/`search_regex`
(100k lines), `damage_merge` (10 000 `advance` calls without collection),
`alloc_idle` (assert zero allocations across 1000 empty `advance` calls).
Capture corpora are checked in as compressed fixtures so results are comparable
across machines and over time.

---

## 10. Testing

Per [P5](01-principles.md#p5--production-grade-from-the-first-commit), none of
this is optional.

**1. VT conformance corpus.** `esctest` (the xterm/iTerm2 conformance suite) is
driven against a headless `Terminal` via a small harness that feeds its
sequences and asserts the resulting grid and reports. Every test is either
passing or listed in `tests/esctest_known_failures.toml` with a reason and a
priority from §5 — an unlisted failure fails CI, and removing an entry is how
"Should" becomes "done". `vttest` is run manually against the TUI for the
interactive parts.

**2. Fixture-driven snapshot tests.** The primary regression net.

```
tests/fixtures/<name>/input.bin       # raw bytes
tests/fixtures/<name>/config.toml     # size, policy, unicode version
tests/fixtures/<name>/expected.txt    # rendered grid, cursor, modes
tests/fixtures/<name>/expected.json   # blocks, marks, hyperlinks, host actions
```

Fixtures are captured from real programs (`nvim`, `htop`, `less`, `git log`,
`claude`, `codex`, `fzf`, `starship` prompts in four shells) with
`omt debug record`. Snapshots are regenerated with `UPDATE_SNAPSHOTS=1` and the
diff is reviewed — a changed snapshot in a PR is a deliberate statement.

**3. Fuzzing.** Required by P5 for any crate parsing untrusted input.

| Target | Property |
|---|---|
| `fuzz_advance` | arbitrary bytes → no panic, no OOM, invariants hold |
| `fuzz_advance_chunked` | same bytes split at arbitrary boundaries produce identical state as one call — the resumption property |
| `fuzz_resize` | arbitrary byte/resize interleavings → invariants hold |
| `fuzz_osc` / `fuzz_dcs` | payload hooks respect budgets and always terminate |

Structural invariants checked after every fuzz step (`Terminal::check_invariants`,
compiled in under `debug_assertions` and in fuzzing):

- cursor within grid bounds; `pending_wrap` implies `cursor.x == cols - 1`
- every `WIDE` cell is followed by a `WIDE_SPACER`, and no `WIDE_SPACER` is
  orphaned
- scrollback `bytes` equals the sum of chunk `bytes`; all limits satisfied
- every `GraphemeId` referenced by a live cell has refcount ≥ 1
- block positions are ordered and non-overlapping; at most one block is open
- damage row indices are all `< rows`

The corpus is seeded from the snapshot fixtures and from `esctest`, and
crash-reproducers are promoted into `tests/regressions/` permanently.

**4. Property tests (`proptest`).**

- **Reflow**: the four properties in §3.6.
- **Positions**: for arbitrary content and widths, `position_of(point_of(p)) == p`
  whenever `p` resolves.
- **Selection round-trip**: `to_string` of a selection covering the whole buffer,
  re-fed through a fresh `Terminal` of the same width, produces the same logical
  lines.
- **Block machine totality**: for any sequence of `{A, B, C, D, output,
  alt-enter, alt-exit}`, the machine never panics, never has two open blocks,
  and every `Finished` block has `output.start <= output.end`.
- **Damage soundness**: replaying only the damaged regions of `Damage` onto a
  copy of the previous snapshot yields the new snapshot. This is the one test
  that makes the render contract real rather than aspirational, and it must run
  on every fixture.

**5. Differential testing.** For the byte-stream web path (§4.4) we assert that
`omt-term`'s grid and xterm.js's grid agree, by running the fixture corpus
through a headless xterm.js in CI and diffing the serialized screens. Divergence
is either an omt bug or a documented, listed difference.

---

## 11. Open questions

1. **OPEN QUESTION — image lifetime across reflow.** Inline images occupy real
   cells (so they scroll and select like text), but a narrower width changes how
   many cells a placement covers. Options: re-scale the placement, clip it, or
   drop it. Current lean: clip and mark the placement `Reflowed`, with the web
   client re-requesting the original at its own scale. Needs a decision before
   graphics work starts, and it affects [09 — SSH and media](09-ssh-and-media.md).

2. **Resolved — who owns the byte ring for web replay.** `omt-session` owns the
   resumable ring of raw PTY bytes keyed by `seq`; `omt-term` stays a pure state
   machine and exposes `Snapshot::to_ansi(&mut impl Write)` for the local/debug
   redraw. The remote path uses `grid_v1` from
   [07 §4.2](07-remote-protocol.md#42-the-decision-c-hybrid-byte-stream-primary),
   whose concrete encoding remains open —
   [07 §9.1](07-remote-protocol.md#9-open-questions) owns that question.

3. **OPEN QUESTION — Kitty keyboard protocol vs. agent CLIs.** Several agent CLIs
   negotiate the protocol themselves. When omt is the outer terminal *and* the
   inner program pushes flags, our stack must nest correctly per-screen. Needs a
   spike against `claude`, `codex` and `nvim` before we commit to the flag set in
   §5.5.

4. **OPEN QUESTION — scrollback persistence granularity.** [05](05-session-model.md)
   and `omt-store` need to snapshot scrollback for restore-after-daemon-restart.
   Per-chunk generations make delta persistence natural (write only chunks whose
   generation changed), but the cell encoding must then be versioned on disk from
   v1. Who owns the serialization — `omt-term` (knows the types) or `omt-store`
   (knows the format)? Lean: `omt-term` exposes a versioned, `serde`-able
   `ChunkSnapshot`; `omt-store` decides when to write it.

5. **OPEN QUESTION — block attribution source of truth.** §6.2 has `omt-term`
   accept an `Attribution` from above. If two sources disagree (the agent layer
   says "agent" from a hook, the writer token says "human client X"), who wins?
   Proposed rule: hook/protocol-sourced attribution outranks writer-token
   attribution, and the loser is retained as `Attribution::contested`. Tracked
   as [06 §10.9](06-agent-layer.md#10-open-questions), which owns it, because the
   decision is the agent layer's; `omt-term` only stores the result.

6. **OPEN QUESTION — heuristic idle threshold.** §6.4's 300 ms is a guess. It
   should be measured against real sessions (a slow `npm install` with long
   silent stretches versus a chatty build) and may need to be adaptive. Ships
   configurable; default may change.
