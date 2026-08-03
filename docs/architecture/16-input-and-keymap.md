# Input, Keymap and Conflict Resolution

omt's TUI is a program that draws with keys, sitting inside another program that
draws with keys, hosting a third program that draws with keys. Every layer wants
`Ctrl+O`. This document decides who gets it.

It owns the **semantics** of input: what a key means, in which context, and what
happens when two layers claim the same chord. The **file format** for
keybindings is owned by [10 — Configuration §8.2](10-configuration.md#82-keybinding-format);
where the two disagree, §13 records the mismatch rather than silently diverging.

Related: [01 — Principles](01-principles.md) ([P3](01-principles.md#p3--parity-one-capability-three-surfaces),
[P4](01-principles.md#p4--native-semantics-observe-never-re-implement)) ·
[03 — Capability catalog](03-capability-catalog.md) ·
[04 — Terminal core §5.5](04-terminal-core.md#55-keyboard-graphics-misc) (kitty
keyboard protocol, bracketed paste) ·
[08 — Web client §4.3.1, §8.4](08-web-client.md) (virtual key bar, gestures) ·
[09 — SSH and media](09-ssh-and-media.md) (the paste transport) ·
[15 — Workspace explorer §7.1](15-workspace-explorer.md#71-tui-panel) (panel keys).

The whole document reduces to five commitments:

> 1. **Unmatched keys pass through to the inner program, byte-identical.** That
>    is the default, not a fallback.
> 2. **omt's un-prefixed key budget is five chords.** Everything else lives
>    behind a leader or in the command palette.
> 3. **omt knows the inner program's keymap as data**, so it can refuse to
>    shadow `Ctrl+O` in Claude Code and say so at config-load time.
> 4. **When omt owns both ends (`omt ssh`, `omt --remote`, the web client) it
>    offers chords a foreign terminal could never deliver** — and says plainly
>    which those are.
> 5. **Vim mode and emacs mode govern omt's own surfaces and nothing else**, so
>    running vim inside an omt pane while omt is in vim mode is not a conflict
>    at all. `default`, `vim` and `emacs` are three data files over one action
>    set, not three code paths.

---

## 1. The layer model

### 1.1 The four layers

```
┌─ L1  OS / window manager ───────────────────────────────────────────────┐
│   macOS: Cmd+Tab, Cmd+Space, Ctrl+↑ (Mission Control), Cmd+H/Q/M.       │
│   Linux: WM/compositor grabs (Super+…, Alt+Tab).                        │
│   Windows: Win+…, Alt+Tab, Ctrl+Alt+Del.                                │
│   omt cannot see these and must never document a binding that collides.  │
├─ L2  Outer terminal emulator ──────────────────────────────────────────┤
│   iTerm2 / Ghostty / kitty / WezTerm / Terminal.app / Windows Terminal.  │
│   Owns: tabs, splits, profiles, Cmd/menu chords, its own copy & paste.   │
│   Decides which of the remaining keys become BYTES ON THE PTY, and in    │
│   which encoding. This is the layer that actually constrains omt.        │
├─ L3  omt ──────────────────────────────────────────────────────────────┤
│   Sees only what L2 forwarded, as bytes. Decodes, normalizes, resolves   │
│   against the keymap in the current context, and either calls a          │
│   capability or writes the ORIGINAL BYTES to the inner PTY.              │
├─ L4  The inner program ────────────────────────────────────────────────┤
│   Claude Code, Codex, opencode, vim, less, fzf, a bare shell.            │
│   Receives whatever L3 passed through, and has its own full keymap.      │
└─────────────────────────────────────────────────────────────────────────┘
```

Two asymmetries drive every decision below:

- **L2 is upstream and unnegotiable.** omt cannot ask iTerm2 to stop eating
  `Cmd+V`. It can only *detect* what arrives and adapt (§5.5), or ask the user to
  reconfigure L2 (§7.4).
- **L4 is downstream and observable.** omt knows which agent is bound to a pane
  ([06 — Agent layer](06-agent-layer.md)), so it can know L4's keymap as data and
  reason about shadowing (§5.1). This is the only layer omt can be *smart* about.

### 1.2 What can physically reach a TUI process

A terminal delivers keys as bytes. In the **legacy encoding** the byte alphabet
is tiny, and several distinct physical chords are literally indistinguishable:

| Physical chord | Legacy bytes | Collides with |
|---|---|---|
| `Ctrl+I` | `0x09` | `Tab` |
| `Ctrl+M` | `0x0D` | `Enter`, `Ctrl+Enter`, `Shift+Enter` |
| `Ctrl+[` | `0x1B` | `Esc`, and the prefix of every escape sequence |
| `Ctrl+H` | `0x08` | `Backspace` on some configurations |
| `Ctrl+Shift+X` | same as `Ctrl+X` | **`Ctrl+Shift+<letter>` is not representable at all** |
| `Alt+X` | `ESC x` (meta-prefix) or `0xF8`-set (8-bit meta) | a fast-typed `Esc` then `x` |
| `Ctrl+Space` | `0x00` | `Ctrl+@` |

The consequences are absolute, not stylistic:

- **`Ctrl+Shift+<letter>` cannot be a default binding.** In the legacy encoding
  the shift bit is simply not transmitted for control characters. Any design
  that assumes `Ctrl+Shift+P` works universally is wrong. It becomes available
  *only* under the kitty keyboard protocol or `modifyOtherKeys` (§1.4).
- **`Esc` is ambiguous with the start of any escape sequence**, so omt needs a
  timeout-based disambiguation (§2.2) — and `Esc` is a key Claude Code uses
  heavily, which makes getting the timeout right a correctness issue, not a
  polish issue.
- **`Cmd` is not in the alphabet at all.** There is no legacy byte encoding for
  a Command-key chord. See §1.5.

### 1.3 Deliverability matrix (legacy encoding, default configuration)

Legend: **Y** delivered as distinguishable bytes · **A** delivered but
*ambiguous* with another chord · **N** never reaches the PTY · **C** reaches it
only after the user configures the terminal.

| Chord | iTerm2 (macOS) | Ghostty | kitty | WezTerm | Terminal.app | Windows Terminal | Linux xterm-alikes |
|---|---|---|---|---|---|---|---|
| `Ctrl+<letter>` | Y | Y | Y | Y | Y | Y | Y |
| `Ctrl+I` / `Tab` | A | A | A | A | A | A | A |
| `Ctrl+[` / `Esc` | A | A | A | A | A | A | A |
| `Ctrl+Shift+<letter>` | N | N | N | N | N | N | N |
| `Alt+<letter>` | C¹ | Y | Y | Y | C¹ | Y | Y |
| `Shift+Enter` | A² | A² | A² | A² | A² | A² | A² |
| `Ctrl+Enter` | A² | A² | A² | A² | A² | A² | A² |
| `F1`–`F12` | Y | Y | Y | Y | Y³ | Y | Y |
| `Shift+F1`–`F12` | Y | Y | Y | Y | Y | Y | Y |
| `Cmd+<anything>` | **N** | **N** | **N** | **N** | **N** | n/a | n/a |
| `Super/Win+<anything>` | n/a | N⁴ | N⁴ | N⁴ | n/a | N | N⁴ |

¹ macOS terminals default `Option` to "compose special characters"; the user
must set *Left Option = Esc+* (iTerm2) or the equivalent for `Alt` chords to
arrive. This is the single most common reason an `Alt` binding "does nothing" on
a Mac.
² Distinguishable **only** under kitty protocol / `modifyOtherKeys`; otherwise
all three of `Enter`, `Shift+Enter`, `Ctrl+Enter` are `0x0D`.
³ Terminal.app's function keys are bound to its own profile actions in several
default profiles; verify per profile.
⁴ Normally consumed by the window manager (L1) before the terminal sees them.

**Verification status.** The `Ctrl+Shift+<letter>` row, the `Cmd` row and the
`Ctrl+I`/`Ctrl+[`/`Shift+Enter` ambiguities are **verified** — they follow from
the encoding itself and are reproducible with `cat -v` in any terminal. The
per-emulator `Alt` and function-key cells are **assumed from documentation and
common configuration** and must be re-checked by the conformance corpus in §10.1
before this table is quoted in user-facing docs. Every cell in this table is
also a row in that corpus, so the table becomes machine-verified rather than
editorial.

### 1.4 What changes under the kitty keyboard protocol

[04 §5.5](04-terminal-core.md#55-keyboard-graphics-misc) makes the kitty
keyboard protocol a **Must** for `omt-term` as a *consumer*. Here it matters in
the other direction: omt as a *client* of the outer terminal, asking it to send
richer key events.

omt pushes flags at startup with `CSI > 1 u` (disambiguate escape codes) and,
when the terminal reports support for them, `0b1111` (report event types, report
alternate keys, report all keys as escape codes, report associated text), then
pops with `CSI < u` on exit and on suspend.

Under the protocol, keys arrive as `CSI unicode-key ; modifiers [: event-type] u`
(or `~`/letter-final forms for special keys), and:

| Previously | Under kitty protocol |
|---|---|
| `Ctrl+Shift+P` unrepresentable | `CSI 112 ; 6 u` — **fully available** |
| `Esc` ambiguous with sequence start | `CSI 27 ; 1 u` — **unambiguous, no timeout needed** |
| `Ctrl+I` == `Tab` | `CSI 105;5u` vs `CSI 9;1u` — distinct |
| `Shift+Enter` == `Enter` | `CSI 13;2u` vs `CSI 13;1u` — distinct |
| No key-release | `:3` event type — release available |
| No key repeat signal | `:2` event type — repeat distinguishable from a new press |

`modifyOtherKeys` (xterm's `CSI > 4 ; 2 m`) is a weaker, older mechanism that
also disambiguates `Ctrl+Shift+<letter>` and `Esc`, without release events or
alternate keys. omt negotiates it as a **second-choice fallback** when kitty
protocol is unavailable, and records which one it got.

**Support** (**assumed**, to be pinned by §10.1): kitty and Ghostty implement the
kitty protocol; WezTerm and foot implement it; iTerm2 has an opt-in
implementation in recent versions; Alacritty implements a subset; Terminal.app
and Windows Terminal implement neither protocol. `modifyOtherKeys` is
implemented by xterm, and by several emulators in xterm-compatibility mode.

Negotiation is a *query*, never an assumption: omt sends `CSI ? u` and waits
(80 ms budget, §5.5) for the `CSI ? <flags> u` reply. **No reply means no
protocol**, and omt's default keymap silently narrows (§8.3). omt never emits
protocol-dependent bindings into the help UI when the protocol is absent — a
binding the user can see but cannot press is worse than one that does not exist.

### 1.5 The `Cmd` problem, stated once

On macOS, `Cmd`-based chords are consumed by the terminal emulator (L2) or the
OS (L1). **There is no encoding by which `Cmd+Shift+V` reaches a TUI process in
its default configuration, in any macOS terminal.** This is not an omt
limitation, a kitty-protocol gap, or something a future version fixes — the
kitty protocol defines a `super` modifier bit, but the emulator still has to
decide to *send* it rather than run its own Paste menu item, and every macOS
terminal binds `Cmd+V` to Paste by default.

The consequences run through the whole document:

- No omt default may use `Cmd` (§8.1).
- A user who wants `Cmd+Shift+V` must remap it **in their terminal** to emit a
  sequence omt understands. omt supports this, prints the exact config snippet
  per terminal, and can install it (§7.4, §7.5).
- The web client is the exception: a browser genuinely receives
  `Cmd+Shift+V` — and more importantly a real `paste` event with the clipboard
  attached (§7.6).

---

## 2. The dispatch model

### 2.1 The pipeline

```
outer terminal
     │  bytes
     ▼
┌──────────────────────────────────────────────────────────────────────┐
│ 1. KeyDecoder      bytes → RawKey            (legacy | kitty | mOK)  │
│                    keeps the ORIGINAL BYTES alongside the decode     │
│ 2. Normalizer      RawKey → KeyEvent         (canonical modifiers,   │
│                    canonical key names, layout-independent codes)     │
│ 3. ContextStack    → ContextSet              (what is focused now)    │
│ 4. Resolver   (KeyEvent, ContextSet, PendingChord) → ChordResolution  │
│ 5a. Capability     dispatch through the catalog, exactly as remote    │
│ 5b. Passthrough    write ORIGINAL BYTES to the PTY, unmodified        │
└──────────────────────────────────────────────────────────────────────┘
```

Step 5b writing *original bytes* rather than a re-encoding of the `KeyEvent` is
load-bearing. If omt re-encodes, then every decode bug becomes a corruption bug
in the inner program, and any sequence omt does not model is destroyed. Keeping
the bytes means **omt's passthrough is lossless by construction**, including for
sequences omt does not understand at all (a future protocol, a mouse report, a
paste body). The decoded `KeyEvent` exists only to answer "is this bound?".

### 2.2 Types

```rust
/// A decoded key, still carrying the bytes that produced it.
pub struct RawKey<'a> {
    pub bytes: &'a [u8],
    pub decoded: KeyEvent,
    pub encoding: KeyEncoding,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum KeyEncoding { Legacy, ModifyOtherKeys, Kitty { flags: u8 } }

/// Canonical key event. Two events are equal iff they should resolve alike.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub mods: Mods,
    pub kind: KeyEventKind,      // Press | Repeat | Release
    /// Text the key produced, when the terminal reported it (kitty flag 0b1000).
    /// Used for insertion, never for binding lookup.
    pub text: Option<SmolStr>,
    /// The key on the *base* (US-ASCII) layout, when reported. See §9.1.
    pub base_layout_code: Option<KeyCode>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
    Char(char),                  // already lowercased; case lives in Mods::SHIFT
    Enter, Tab, Backspace, Esc, Space, Delete, Insert,
    Up, Down, Left, Right, Home, End, PageUp, PageDown,
    F(u8),                       // 1..=24
    Menu, PrintScreen, ScrollLock, Pause, NumLock,
    KeypadChar(char), KeypadEnter,
    /// A media/modifier key reported only under kitty flag 0b100.
    Other(u32),
}

bitflags! {
    pub struct Mods: u8 {
        const SHIFT = 1; const ALT = 2; const CTRL = 4;
        const SUPER = 8;              // Cmd on macOS, Win/Super elsewhere
        const HYPER = 16; const META = 32;
        const CAPS_LOCK = 64; const NUM_LOCK = 128;
    }
}
```

`CAPS_LOCK`/`NUM_LOCK` are decoded but **masked out before binding lookup**;
they are reported by the kitty protocol and binding on them is a trap.

Ambiguity resolution in the decoder, all of it explicit:

- **`Esc` vs. escape sequence.** Under kitty protocol: no ambiguity, decide
  immediately. Under legacy: a lone `0x1B` starts a `esc_timeout` timer
  (default **25 ms**, configurable `input.esc_timeout`). If more bytes arrive
  that continue a known sequence, it is a sequence; if the timer fires, it is
  `Esc`. **Over a slow ssh link this timeout is wrong**, so `omt ssh`
  ([09 §6](09-ssh-and-media.md)) raises it to 100 ms on the local end, where the
  measurement is accurate, and the remote end never sees a bare `Esc` ambiguity
  because the thin client sends structured `KeyEvent`s (§7.1). This is a
  concrete case of "local feels remote-native": the thin client is *more*
  correct than a raw ssh, not less.
- **`Ctrl+I` vs `Tab`, `Ctrl+M` vs `Enter`, `Ctrl+[` vs `Esc`.** Under legacy,
  omt decodes to the *named* key (`Tab`, `Enter`, `Esc`) and records
  `KeyEvent::ambiguous_with()` so `omt keys explain` can tell the truth (§5.3).
  A binding on `ctrl-i` in a legacy session is a config-load warning (§5.2).
- **`Alt+x` vs `Esc` then `x`.** Same timer. `input.alt_is_esc_prefix` (default
  `true`) controls whether the `ESC x` form is decoded as `Alt+x` at all; users
  who type `Esc` then a letter deliberately (vi-mode readline) can set it false
  and use the kitty protocol for real `Alt`.
- **Bracketed paste** (`ESC [ 200 ~` … `ESC [ 201 ~`) is decoded as a single
  `InputEvent::Paste(body)` and never as keys. It is passed through inside
  bracketed-paste markers to the inner program when the inner program has
  enabled mode 2004, and stripped to plain text when it has not — because
  otherwise a program that never asked for the markers receives literal
  `[200~` garbage.

```rust
pub enum InputEvent {
    Key(KeyEvent),
    Paste(String),
    Mouse(MouseEvent),
    FocusGained, FocusLost,
    /// Anything omt decoded as "a sequence I do not model". Never dropped.
    Opaque,
}

/// A decoded mouse report. Bindable, exactly like a `KeyEvent` (§2.3).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct MouseEvent {
    pub kind: MouseKind,
    pub button: MouseButton,
    pub mods: Mods,              // same bitflags as KeyEvent; CAPS/NUM masked out
    pub pos: (u16, u16),         // (col, row), 0-based, in the *pane's* coordinates
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseKind { Press, Release, Drag, Motion, Wheel(WheelDir) }

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum WheelDir { Up, Down, Left, Right }

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton { Left, Middle, Right, Other(u8) }
```

**The decoded encoding is SGR 1006** (`CSI < b ; x ; y M|m`). omt requests it
from the outer terminal and normalizes the legacy X10/1005/urxvt forms into the
same `MouseEvent`, so coordinates beyond column 223 are not a special case and a
release event always carries its button. What omt *forwards* to the inner
program is the encoding that program asked for, unmodified — §2.1's no-re-encoding
rule applies to mouse reports exactly as it does to keys.

### 2.3 Resolution

```rust
pub struct Resolver {
    /// Ordered by specificity; built once per config load, immutable at runtime.
    bindings: Vec<CompiledBinding>,
    /// Trie over chord sequences, for pending-chord state.
    chords: ChordTrie,
    pending: Option<PendingChord>,
}

pub struct CompiledBinding {
    pub trigger: Chord,
    pub when: ContextPredicate,
    pub action: Action,
    pub specificity: u16,             // #context terms, ties broken by layer
    pub source: BindingSource,        // Builtin | User { file, span } | Project | Runtime
}

pub enum Action {
    Capability { name: CapabilityName, args: serde_json::Value },
    /// Send the leader key itself, or any literal key, to the PTY.
    SendKey(KeyEvent),
    /// Explicitly unbound: resolution stops here and falls through to passthrough.
    None,
}

/// A trigger: a sequence of one or more key steps, or a single mouse step.
pub enum Chord {
    Keys(SmallVec<[KeyEvent; 3]>),
    /// A mouse event with modifiers, e.g. `"shift-mouse1"`. Never a sequence:
    /// a two-step mouse chord is not a gesture any terminal can deliver.
    Mouse(MouseTrigger),
}

pub struct MouseTrigger {
    pub kind: MouseKind,          // defaults to Press when the spelling omits it
    pub button: MouseButton,
    pub mods: Mods,
}

pub enum ChordResolution<'a> {
    /// Call this capability. The key is consumed.
    Dispatch(&'a CompiledBinding),
    /// A chord prefix matched; hold input and show the pending-chord hint.
    Pending { prefix: Chord, deadline: Instant, candidates: &'a [CompiledBinding] },
    /// No binding. Write `bytes` to the PTY verbatim. THE DEFAULT.
    Passthrough,
}
```

The enum is `ChordResolution`, not `Resolution`: the bare name belongs to
[18 §3](18-semantic-open.md#3-resolution)'s target-resolution outcome, and the
two are unrelated. `ChordResolution::Dispatch | ::Pending | ::Passthrough` are
the only outcomes of matching input against the keymap.

**Mouse triggers.** A trigger may name a mouse event with modifiers, spelled
`mod-…-mouse<N>` where `N` is `1` (left), `2` (middle), `3` (right) or a number
for `Other`, optionally suffixed with `-up`, `-drag` or replaced by
`wheel-up`/`wheel-down`/`wheel-left`/`wheel-right`. A bare `shift-mouse1` means
*press*; activation semantics that fire on release are the binding's business,
not the grammar's ([18 §5.4](18-semantic-open.md#54-selection-versus-click)).
Mouse triggers take a `when` predicate like any other binding, which is how
[18 §5.1](18-semantic-open.md#51-mouse-reporting--the-inner-program-owns-the-click)
expresses "consume `Shift`+click only while the inner program has grabbed the
mouse":

```toml
[[binding]]
trigger    = "shift-mouse1"
when       = "terminal_focused && mouse_reporting"
capability = "open.activate"
```

An unmatched mouse event passes through exactly as an unmatched key does
(rule 5 below) — that is what keeps vim's mouse working.

**Precedence, in order, first match wins:**

1. **Pending chord.** If a chord is in flight, only its continuations are
   considered. A non-continuation is handled per §3.4.
2. **Modal context.** If a modal context is active (palette open, search active,
   copy mode), its bindings are considered before any global binding, and the
   modal context declares whether it is *exclusive* (§4.3).
3. **Specificity.** More context terms in `when` beats fewer. `when = "explorer_focused && !search_active"` (2) beats `when = "explorer_focused"` (1) beats no `when` (0).
4. **Layer.** Ties are broken by config layer, higher wins:
   `Runtime > Session > Instance > Project > User > Builtin`. Same as
   [10 §2.1](10-configuration.md#21-the-layers-in-precedence-order); a keybinding
   is a config value and obeys the config precedence, with no second rulebook.
5. **Passthrough.** No match, at any specificity, in any layer.

Rule 5 is the invariant. It is asserted by a property test (§10.2): for a keymap
containing only the defaults, the set of `KeyEvent`s that do **not** pass through
is exactly the set enumerated in §8.1, and it has cardinality ≤ 6.

---

## 3. The leader key

### 3.1 The tension, and the resolution

[P3](01-principles.md#p3--parity-one-capability-three-surfaces) requires every
capability to be reachable from the TUI. The capability catalog has on the order
of 150 entries. [P4](01-principles.md#p4--native-semantics-observe-never-re-implement)
requires omt not to steal keys the inner program needs — and Claude Code alone
uses `Esc`, `Ctrl+O`, `Ctrl+E`, `Ctrl+R`, `Shift+Tab`, `Ctrl+C`, `Ctrl+D`, the
arrows, `n` on a question card, and `/` at the start of a line.

These are not actually in conflict, because **"reachable" does not mean "has a
dedicated chord"**. The resolution is two mechanisms:

- **A leader key.** One chord opens a namespace. `<leader> s` costs omt nothing
  from the inner program's budget except the leader itself, and the leader can
  be sent through with `<leader> <leader>`.
- **A command palette.** One chord opens a fuzzy-searchable list of *every*
  capability, with its current binding, its arguments, and its docs. This is
  what makes P3 satisfiable with a key budget of five: the palette is a
  *complete* surface over the catalog, generated from it, so a new capability is
  reachable the moment it is registered — no keymap edit, no CI exemption.

The parity test in [03 §5](03-capability-catalog.md) is amended accordingly: a
capability satisfies "has a TUI affordance" if it has a binding **or** appears in
the palette, and the palette's contents are the catalog, so the second clause is
always true. Bindings become an *optimization for frequency*, not a requirement
for reachability. That is the whole argument for a tiny key budget.

### 3.2 Default leader: `Ctrl+B`

**Decision: the default leader is `Ctrl+B`, changeable in one setting, with a
first-run prompt offering `Ctrl+A` and `Ctrl+Space`.**

Rationale, against the real alternatives:

| Candidate | Verdict |
|---|---|
| **`Ctrl+B`** ✅ | tmux's default for two decades, so the largest single group of incoming users needs no relearning. In an agent CLI it is unused (**verified for Claude Code and Codex against the registry in §5.1**). In readline it is `backward-char`, which is redundant with `←` and is the least-used of the readline motions. In vim it is `scroll-back-page`, which is the one real cost — mitigated because `<leader> <leader>` sends it, and because vim users are precisely the population that already remaps a multiplexer prefix. |
| `Ctrl+A` | screen's default and beloved by its users, but in readline it is `beginning-of-line`, which people use constantly in a shell — and a shell is what half of omt's panes contain. Offered at first run, not defaulted. |
| `Ctrl+Space` | Ergonomically excellent and free in most shells, but it is `NUL`, which emacs binds to `set-mark`, several IMEs bind to layout switching (§9.3), and some terminals do not transmit at all. Offered, not defaulted. |
| `Ctrl+O` | **Refused.** Claude Code's command palette. Exactly the key this document exists to protect. |
| `Ctrl+S` / `Ctrl+Q` | **Refused.** Software flow control. Even with `IXON` disabled on omt's PTYs, an inner `ssh` or `screen` can re-enable it, and a terminal that appears frozen is the worst possible failure. |
| `Ctrl+\` | **Refused.** `SIGQUIT` with a core dump. |
| `` Ctrl+` `` / `Ctrl+;` | Not representable in the legacy encoding for most terminals. |
| A `Cmd` chord | **Refused.** §1.5. |

Configuration:

```toml
# ~/.config/omt/keybindings.toml
leader = "ctrl-a"                 # single setting; everything under <leader> follows
leader_timeout = "1500ms"
```

Changing `leader` rewrites nothing else: every default binding is stored as
`"<leader> s"`, and `<leader>` is resolved at compile-to-`CompiledBinding` time.
A user who binds `leader = "ctrl-a"` gets their whole map moved, and `omt keys
list` prints the resolved chords.

### 3.3 Pending-leader UI

After the leader is pressed, omt is in `Pending`. It shows a **non-modal hint
strip** at the bottom of the focused pane — one line, drawn over the pane's last
row, restored on resolution:

```
 ctrl-b ▸  s sessions  p palette  e explorer  |−split  z zoom  v paste  ? all
```

Design points, each for a reason:

- **It appears after 250 ms, not instantly.** A user executing `<leader> s` from
  muscle memory never sees a flash. A user who forgot gets the menu. This is
  which-key's insight and it costs one timer.
- **It is drawn over the pane, not in a new region.** Reflowing the pane on a
  leader press would resize the inner program, which sends `SIGWINCH` to an
  agent CLI mid-render. Never do that for a transient UI.
- **`?` opens the full list** as a scrollable overlay, which is the palette
  filtered to the leader namespace.
- **The inner program is not told anything.** No bytes are written while
  pending. The pane is visually unchanged apart from the strip.

### 3.4 Timeout and escape hatches

| Event while pending | Behaviour |
|---|---|
| A continuation key | Resolve, dispatch, clear pending. |
| `leader` again | Send the leader's original bytes to the PTY, clear pending. This is how `Ctrl+B` reaches vim. |
| `Esc` or `Ctrl+G` | Cancel. **Nothing is sent to the PTY.** |
| Any other key | Cancel, and — per `input.leader_miss` — either `drop` (default) or `passthrough_both` (send the leader's bytes then this key's bytes). Default is `drop` because sending `Ctrl+B` into an agent prompt after a mistyped chord is a worse surprise than losing a keystroke, and the hint strip makes the miss visible. |
| `leader_timeout` expires (default 1500 ms) | Cancel, drop, hint strip fades. |
| Focus lost / pane closed / config reload | Cancel. |

A chord may be longer than two keys (`<leader> g s`), and the trie handles
arbitrary depth; the timeout restarts on each accepted continuation.

### 3.5 The palette

One chord, always: **`<leader> p`**, plus **`Ctrl+Shift+P`** bound *automatically
and only* when the kitty protocol or `modifyOtherKeys` is negotiated (§1.4). The
second binding is a progressive enhancement that is present in the help UI only
when it is real.

The palette is generated from the catalog:

- Every registered capability, its group, verb, description (from the doc
  comment), current binding, and required role.
- Capabilities whose `role` exceeds the actor's role are shown disabled with the
  reason, not hidden — a hidden capability is indistinguishable from a missing
  one, which is a support burden.
- Capabilities with required arguments open an **argument form** built from the
  input JSON Schema, the same generator [10 §4](10-configuration.md#4-typed-schema-and-generation)
  uses for the settings editor. `pane.split` with `direction` becomes two rows
  ("Split vertically" / "Split horizontally") by enumerating the enum, so the
  common case needs no form at all.
- It also indexes **non-capability targets**: sessions, workspaces, agents,
  workflows, launch configs, files from the explorer's `files.find`. One search
  box, ranked by recency then fuzzy score.
- `effects = [DESTRUCTIVE]` entries require a confirm step, matching the mobile
  rule in [03 §2](03-capability-catalog.md).

The web client has the same palette (`⌘K`/`Ctrl+K` on desktop, a toolbar button
on mobile), rendered from the same `capability.list` response. That is the P3
guarantee for every entry in §8.1's "web equivalent" column.

---

## 4. Modes and contexts

### 4.1 The context set

```rust
bitflags! {
    pub struct ContextSet: u32 {
        const TERMINAL_FOCUSED     = 1 << 0;   // a pane's PTY has focus
        const EXPLORER_FOCUSED     = 1 << 1;   // 15-workspace-explorer panel
        const PALETTE_OPEN         = 1 << 2;
        const SEARCH_ACTIVE        = 1 << 3;   // omt's own find-in-scrollback
        const COPY_MODE            = 1 << 4;   // scroll/selection mode
        const CARD_FOCUSED         = 1 << 5;   // omt-rendered interaction card
        const PICKER_OPEN          = 1 << 6;   // session/agent picker overlay
        const SETTINGS_OPEN        = 1 << 7;
        const PENDING_CHORD        = 1 << 8;
        const AGENT_BOUND          = 1 << 9;   // pane has a live agent binding
        const ALT_SCREEN           = 1 << 10;  // inner program is full-screen
        const APP_CURSOR_KEYS      = 1 << 11;  // DECCKM set by the inner program
        const MOUSE_REPORTING      = 1 << 12;
        const BRACKETED_PASTE      = 1 << 13;
        const ZOOMED               = 1 << 14;
        const REMOTE_THIN_CLIENT   = 1 << 15;  // this TUI is `omt ssh`'s local end
        const WEB                  = 1 << 16;
        const TUI                  = 1 << 17;
        const HINT_MODE            = 1 << 18;  // 18 §5.2 hint overlay is up
        const OPEN_MENU_FOCUSED    = 1 << 19;  // 18 §4.3 action menu has focus
    }
}
```

`ALT_SCREEN`, `APP_CURSOR_KEYS`, `MOUSE_REPORTING` and `BRACKETED_PASTE` come
straight from `omt-term`'s `ModeView`
([04 §4.1](04-terminal-core.md#41-what-a-renderer-gets)) — they are *facts about
the inner program*, and binding on them is how omt stays out of the way. The
default keymap uses `ALT_SCREEN` in exactly one place: omt's mouse-driven pane
resize is disabled while the inner program has requested mouse reporting,
because otherwise omt eats vim's mouse. `MOUSE_REPORTING` additionally gates the
one sanctioned exception to that rule, `shift-mouse1` (§2.3, §6.2).

`HINT_MODE` and `OPEN_MENU_FOCUSED` are owned by
[18 — Semantic open](18-semantic-open.md): the first is set while a hint session
is live for this client (§5.2 there), the second while its action menu holds
focus. They are listed here because this list is the authoritative superset and
must actually contain every name a `when` predicate may use.

`when` predicates in `keybindings.toml` are boolean expressions over these
names; [10 §8.2](10-configuration.md#82-keybinding-format) specifies the file
shape and points back here for the vocabulary. The name list here is the
authoritative superset.

### 4.2 Focus and exclusivity

```rust
pub enum FocusOwner {
    Pty(PaneId),
    Overlay(OverlayId),     // palette, picker, settings, search bar
    Panel(PanelId),         // explorer
    Card(InteractionId),    // omt-rendered interaction card
}

pub struct ContextStack {
    base: FocusOwner,
    /// Overlays, innermost last. Each declares its capture policy.
    overlays: Vec<(OverlayId, Capture)>,
}

pub enum Capture {
    /// Every key goes to the overlay; nothing reaches the PTY. Palette, settings.
    Exclusive,
    /// Overlay bindings win; unmatched keys still pass through. Search bar
    /// while the PTY keeps rendering.
    Priority,
}
```

`Exclusive` is the important one: while the palette is open, `Ctrl+C` closes the
palette, it does **not** interrupt the agent. Users expect that, and the
alternative — an overlay that leaks `Ctrl+C` to a running build — is a data-loss
bug. The exclusivity is declared per overlay and rendered visibly (the pane is
dimmed), so there is never a question about where keys are going.

### 4.3 Copy mode

Copy mode is `Priority` with a full vi/emacs-style motion map (`h j k l`, `w b`,
`/` search, `v` select, `y` yank, `q`/`Esc` exit). It is entered by
`<leader> [` and — critically — **also by scrolling with the mouse wheel or
trackpad when the inner program has not enabled mouse reporting**, because that
is what users actually do. When mouse reporting *is* on, wheel events go to the
inner program untouched (`less` and `vim` handle their own scrolling).

Copy mode captures keys but does not stop PTY output; the viewport is pinned and
a "▲ 412 lines" indicator shows the offset, matching the web client's unpin
behaviour in [08 §4](08-web-client.md).

### 4.4 Interaction cards — who owns the keyboard

This is the subtlest rule in the document, because two programs can be drawing a
question at the same moment.

An `Interaction` ([06 — Agent layer](06-agent-layer.md)) reaches omt through a
structured source: a deferred `PreToolUse` hook, an ACP permission request, an
`AskUserQuestion` tool call. omt can render it natively as a card. Meanwhile,
the agent CLI is *also* rendering it in its own TUI inside the pane, because omt
runs the real CLI in a real PTY ([P4](01-principles.md#p4--native-semantics-observe-never-re-implement))
and does not rewrite its output.

**The rule: omt renders the card, but does not focus it, unless the user asks.**

| State | `CARD_FOCUSED` | Keys go to |
|---|---|---|
| Interaction arrives, user is at the laptop | no | the agent — its own TUI is visible and correct |
| User presses `<leader> a` (answer) or clicks/taps the card | yes | omt — the card |
| User presses `Esc` on a focused card | no | back to the agent, card stays visible |
| Interaction arrives, no local surface is attached (phone only) | n/a | the web client's card — the only surface there is |

Why not focus automatically: an auto-focusing card would steal the keystroke of
a user who was already typing `1` into Claude Code's own prompt, and would
resolve the interaction twice — once natively, once by the user's original
keystroke arriving late. That is the double-handling failure this section
exists to prevent.

**Double-handling is prevented structurally, not by focus discipline:**

1. An `Interaction` has exactly one resolution, enforced by the
   exactly-once ownership rule in [12 — Collaboration](12-collaboration.md)
   ([P6](01-principles.md#p6--collaboration-is-a-runtime-feature-not-just-a-workflow)).
   The second resolution attempt — from any surface — returns
   `already_resolved` with the winning actor, and every surface re-renders.
2. When omt resolves natively, the answer goes back *the way it came* (hook
   decision, ACP response). The agent's own TUI then advances on its own,
   because it received a real answer. omt does not type into the PTY.
3. When the **agent** resolves it — the user answered in the pane, as usual —
   omt observes the resolution through the same structured source and marks the
   card resolved on every surface. The card is a *view*, not a gate.
4. `Interaction`s whose only response channel is synthetic input are bounded by
   [D3](decisions.md); the card for those is labelled and the keys omt would
   type are shown before it types them.

The `n`-key comment affordance ([08 §5.2.1](08-web-client.md)) is bound to
literal `n` **only** in `CARD_FOCUSED`, so it never shadows `n` in the agent's
own card — a direct instance of the registry rule in §5.1.

---

## 5. Conflict detection and resolution

This is the part of the design that is actually novel, and the reason omt can
have any bindings at all without violating P4.

### 5.1 The inner-program keymap registry

omt ships a **data** registry of known programs' keymaps. Not code, not a match
arm: TOML files, loadable from `~/.config/omt/keymaps/` and contributable by
plugins ([11 — Plugins](11-plugins.md)), so a new agent CLI is a file.

```toml
# keymaps/claude-code.toml
#:schema https://omt.dev/schemas/inner-keymap.schema.json
id          = "claude_code"
display     = "Claude Code"
match_agent = "claude_code"          # AgentId from 06-agent-layer
match_process = ["claude"]           # fallback when no adapter is bound
verified_against = "2.1.x"           # what this file was checked against

[[keys]]
chord    = "ctrl-o"
does     = "open the command palette"
severity = "critical"                # critical | important | minor

[[keys]]
chord = "esc"
does  = "interrupt / go back"
severity = "critical"

[[keys]]
chord = "shift-tab"
does  = "cycle permission mode (plan / accept-edits / default)"
severity = "critical"

[[keys]]
chord = "ctrl-e"
does  = "expand the current output block"
severity = "important"

[[keys]]
chord = "ctrl-r"
does  = "reverse search / transcript search"
severity = "important"

[[keys]]
chord = "ctrl-c"
does  = "cancel the in-flight turn"
severity = "critical"

[[keys]]
chord = "ctrl-d"
does  = "exit (on an empty prompt)"
severity = "critical"

[[keys]]
chord = "n"
does  = "add a comment to a question card"
severity = "important"
when  = "question_card"              # a program-declared sub-context, advisory

[[keys]]
chord = "/"
does  = "slash-command completion (at start of line)"
severity = "important"
when  = "prompt_start"

[[keys]]
chord = "up|down"
does  = "prompt history / card navigation"
severity = "important"
```

Shipped files: `claude-code`, `codex`, `opencode`, `gemini-cli`, `aider`,
`vim`, `neovim`, `emacs`, `tmux`, `screen`, `less`, `fzf`, `htop`, `bash-readline`,
`zsh-zle`, `fish`. Two of these deserve comment:

- **`bash-readline` / `zsh-zle`** are the ones that matter most, because most
  panes contain a shell most of the time. They carry the full readline map, with
  `ctrl-a`, `ctrl-e`, `ctrl-k`, `ctrl-r`, `ctrl-w`, `ctrl-u` marked `critical`.
- **`tmux`** exists so omt can warn a user running tmux *inside* omt that their
  leader collides — a real configuration, however redundant.

```rust
pub struct InnerKeymap {
    pub id: InnerProgramId,
    pub display: String,
    pub keys: Vec<InnerKey>,
    pub verified_against: Option<String>,
}

pub struct InnerKey {
    pub chord: Chord,
    pub does: String,
    pub severity: Severity,          // Critical | Important | Minor
    pub when: Option<String>,        // advisory sub-context, not evaluated
}

pub trait InnerKeymapSource: Send + Sync {
    fn for_agent(&self, id: &AgentId) -> Option<&InnerKeymap>;
    fn for_process(&self, comm: &str) -> Option<&InnerKeymap>;
}
```

**Selecting the active keymap** at runtime: the bound agent's adapter names it
directly; failing that, the pane's foreground process name is matched
(`omt-session` already tracks the foreground process group for the block
heuristic, [04 §6.4](04-terminal-core.md#64-the-fallback-heuristic--no-shell-integration)),
falling back to the shell keymap. The active keymap is part of the pane's state
and is shown in `omt keys explain`.

**Staleness is acknowledged, not hidden.** A registry file records
`verified_against`; when the detected agent version is newer, diagnostics say
so ("checked against Claude Code 2.1.x, you are running 2.4.0 — this may be out
of date"). A wrong registry produces a wrong *warning*, never a wrong dispatch,
which is why this is safe to ship as data with imperfect coverage.

### 5.2 Static validation at config load

Runs as pass 4/5 of the [10 §5.1](10-configuration.md#51-pipeline) pipeline,
collecting all diagnostics. Codes continue the `OMT-C4xx` keybinding range
already opened by `OMT-C402`; this document owns the individual codes in
`OMT-C401`–`OMT-C412` (below) and `OMT-C420`–`OMT-C425` (§6.8), and
[10 §5.4](10-configuration.md#54-cross-field-validation-rules-initial-set) names
the ranges rather than restating them.

> **Every new code defined here must also land in
> `docs/reference/diagnostics.md`** — a generated
> artifact whose completeness [10 §11](10-configuration.md#11-testing) already
> asserts CI checks. A code that exists only in this document fails that check.

| Code | Severity | Rule |
|---|---|---|
| `OMT-C401` | error | Duplicate trigger in the same `when` context and layer |
| `OMT-C402` | warning | A chord prefix is also bound as a complete binding (existing code) |
| `OMT-C403` | error | Action names a capability that does not exist |
| `OMT-C404` | error | `args` fail the capability's input schema |
| `OMT-C405` | error | Unknown context name in `when` |
| `OMT-C406` | **warning** | Binding shadows a `critical` key of a keymap in the registry |
| `OMT-C407` | note | Binding shadows an `important` key |
| `OMT-C408` | warning | Binding is undeliverable on the detected terminal |
| `OMT-C409` | warning | Binding is ambiguous in the legacy encoding (`ctrl-i`, `ctrl-m`, `ctrl-[`) |
| `OMT-C410` | error | Binding uses a key on omt's refusal list (§5.4) without `force = true` |
| `OMT-C411` | warning | `leader` itself shadows a registry `critical` key |
| `OMT-C412` | note | A longer chord is unreachable because its prefix is bound with a shorter complete chord in a higher layer |

Verbatim output, in the style of [10 §5.3](10-configuration.md#53-diagnostic-rendering):

```
$ omt config validate

warning[OMT-C406]: `ctrl-o` shadows a key Claude Code needs
  ┌─ ~/.config/omt/keybindings.toml:12:1
  │
12 │ "ctrl-o" = "tui.open_command_palette"
  │ ^^^^^^^^ bound here, in every context
  │
  = note: Claude Code binds `ctrl-o` to "open the command palette" (severity: critical)
  = note: a pane running Claude Code will never receive this key
  = help: use the leader namespace instead:
              "<leader> p" = "tui.open_command_palette"
  = help: or scope the binding so it does not apply to agent panes:
              [[binding]]
              trigger = "ctrl-o"
              when    = "terminal_focused && !agent_bound"
              capability = "tui.open_command_palette"
  = note: registry checked against Claude Code 2.1.x — `omt keys registry claude_code`

warning[OMT-C408]: `ctrl-shift-v` cannot be delivered by your terminal
  ┌─ ~/.config/omt/keybindings.toml:19:1
  │
19 │ "ctrl-shift-v" = "media.image.paste"
  │ ^^^^^^^^^^^^^^ requires the kitty keyboard protocol or modifyOtherKeys
  │
  = note: detected terminal: Apple_Terminal (Terminal.app), TERM=xterm-256color
  = note: Terminal.app transmits `ctrl-shift-v` as `ctrl-v`; the shift bit is lost
  = help: `<leader> v` is bound to the same capability and works everywhere
  = help: or remap the chord in your terminal: `omt doctor keys --fix`
  = note: this binding is kept, and will start working if you switch terminals

error[OMT-C410]: omt refuses to bind `ctrl-c` globally
  ┌─ ~/.config/omt/keybindings.toml:24:1
  │
24 │ "ctrl-c" = "session.close"
  │ ^^^^^^^^ on omt's refusal list
  │
  = note: `ctrl-c` must reach the inner program: it is how every CLI is interrupted
  = help: scope it, e.g. `when = "copy_mode"`, or set `force = true` on the binding
          if you genuinely want to make interrupt unreachable

note[OMT-C412]: `<leader> g s` is unreachable
  ┌─ ~/.config/omt/keybindings.toml:31:1
  │
31 │ "ctrl-b g s" = "explorer.cycle_filter"
  │ ^^^^^^^^^^^^ prefix `ctrl-b g` is bound to `explorer.goto` at the user layer
  │
  = help: unbind the prefix with `"ctrl-b g" = "none"`, or choose another chord

1 error, 2 warnings, 1 note — keybindings not applied, previous keymap retained.
```

`OMT-C406` is a **warning, not an error**, on purpose (§5.4).

Machine-readable form carries the same structure as
[10 §5.3](10-configuration.md#53-diagnostic-rendering), with an extra
`conflict` object so an editor or the web settings UI can render the "this
shadows Claude Code's palette" badge inline:

```json
{
  "code": "OMT-C406", "severity": "warning",
  "message": "`ctrl-o` shadows a key Claude Code needs",
  "path": "ctrl-o",
  "conflict": {
    "layer": "inner_program",
    "program": "claude_code",
    "program_display": "Claude Code",
    "does": "open the command palette",
    "program_severity": "critical",
    "registry_verified_against": "2.1.x"
  },
  "suggestion": { "kind": "replace_key", "value": "<leader> p" }
}
```

### 5.3 `omt keys explain <chord>`

The keyboard analogue of `omt agent explain`. It answers one question — *what
actually happens when I press this?* — across all four layers, in the current
context, with reasons.

```
$ omt keys explain ctrl-o --session s_7f2

ctrl-o  →  passes through to Claude Code

  L1  OS                 not intercepted
  L2  Ghostty 1.2.0      delivered as 0x0F (legacy) / CSI 111;5u (kitty, negotiated)
                         kitty keyboard protocol: ACTIVE (flags 0b1111)
  L3  omt                no binding in context {terminal_focused, agent_bound}
                         (2 bindings exist for `ctrl-o` in other contexts:
                            when=copy_mode          → tui.copy_mode.open_link
                            when=explorer_focused   → explorer.open_in_editor)
  L4  Claude Code 2.4.0  binds ctrl-o → "open the command palette" (critical)

  verdict: Claude Code receives 0x0F. This is what you want.


$ omt keys explain 'cmd-shift-v'

cmd-shift-v  →  never reaches omt

  L1  macOS              not intercepted
  L2  iTerm2 3.5.x       CONSUMED — bound to Edit ▸ Paste Special
                         no macOS terminal forwards Cmd chords to the pty by default
  L3  omt                unreachable
  L4  —                  unreachable

  verdict: this chord cannot work as written.
  alternatives:
    <leader> v      media.image.paste     works everywhere            ← recommended
    ctrl-shift-v    media.image.paste     needs kitty protocol; iTerm2 supports it
                                          if `Report modifiers using CSI u` is on
    remap in iTerm2 so Cmd+Shift+V sends a sequence omt understands:
        omt doctor keys --fix --terminal iterm2


$ omt keys explain '<leader> v' --json
{ "chord": "ctrl-b v", "consumed_by": "omt", "capability": "media.image.paste",
  "context": ["terminal_focused","agent_bound"], "encoding": "kitty",
  "deliverable": true, "shadows": [], "source": { "layer": "builtin" } }
```

It is a capability (`keys.explain`), so the web settings UI's keybinding editor
shows the same analysis live as the user types a chord into the "record a
shortcut" field — which is where it does the most good, before the config is
saved.

Sibling capabilities: `keys.list` (resolved map with provenance, existing),
`keys.conflicts` (the `OMT-C4xx` set, existing), `keys.registry` (dump an inner
keymap), `keys.probe` (§5.5's terminal capability report).

### 5.4 Conflict policy

**Warn and allow, with one small refusal list.**

The policy for shadowing an inner program's keys is a **warning that names the
program, the key's purpose, and a working alternative** — and then omt does what
the user said. Reasons:

- The registry is data with imperfect coverage and a version lag. Refusing on
  registry evidence would let a stale file break a legitimate config.
- Shadowing is sometimes exactly right: a user who never uses Claude Code's
  palette may want `Ctrl+O` for omt's. omt is not better placed than the user to
  decide that.
- Warnings are cheap and visible: the diagnostic appears at validate time, in
  `omt keys list`, in the web editor, and in `omt doctor`.

The **refusal list** is different: these are keys omt will not claim *globally*
by default, and binding them globally requires `force = true` on the binding.
Scoped bindings (with a `when`) are always allowed.

| Key | Why omt refuses it globally |
|---|---|
| `ctrl-c` | Interrupt. The one key every CLI user reaches for when something is wrong. |
| `ctrl-d` | EOF. Shadowing it makes shells unexitable. |
| `ctrl-z` | Suspend. Shadowing it breaks job control for every program in the pane. |
| `ctrl-s`, `ctrl-q` | Flow control. A shadowed `ctrl-s` produces an apparently frozen terminal. |
| `ctrl-\` | `SIGQUIT`. |
| `enter`, `tab`, `space`, `backspace`, `esc` (unmodified, `terminal_focused`) | Typing. A global binding on any of these makes the pane unusable. `esc` in particular is Claude Code's interrupt. |
| `ctrl-[` , `ctrl-i`, `ctrl-m` | Aliases of the above in the legacy encoding; refusing them prevents an accidental alias. |
| `ctrl-a` … `ctrl-e`, `ctrl-k`, `ctrl-u`, `ctrl-w`, `ctrl-r` | **Not refused** — warned (`OMT-C406` against `zsh-zle`/`bash-readline`). They are heavily used but not safety-critical, and users legitimately rebind them. |
| `cmd-*` | Refused with a *different* message: not dangerous, just undeliverable (§1.5). |

`force = true` produces an audit-log entry and a persistent line in
`omt doctor`, so a config that made `Ctrl+C` unreachable is discoverable six
months later.

### 5.5 Terminal capability probing

omt discovers what L2 can deliver, once per session start, with a total budget
of **150 ms** — after which every probe is treated as unsupported and the result
is cached.

```rust
pub struct TerminalProfile {
    pub fingerprint: TerminalFingerprint,
    pub kitty_keyboard: Option<u8>,      // negotiated flag set, if any
    pub modify_other_keys: bool,
    pub bracketed_paste: bool,
    pub focus_reporting: bool,
    pub alt_sends_esc: Tri,              // Yes | No | Unknown — the macOS Option trap
    pub cmd_forwarding: CmdForwarding,   // Never | ConfiguredSequences(Vec<Chord>)
    pub source: ProbeSource,             // Probed | Terminfo | Fingerprint | UserOverride
}

pub struct TerminalFingerprint {
    pub term: String,                    // $TERM
    pub term_program: Option<String>,    // $TERM_PROGRAM: iTerm.app, Apple_Terminal, WezTerm, ghostty
    pub term_program_version: Option<String>,
    pub xtversion: Option<String>,       // CSI > 0 q reply — the authoritative one
    pub over_ssh: bool,
    pub multiplexer: Option<Multiplexer>, // tmux/screen detected from $TERM and $TMUX
}
```

Order, each step falling through:

1. **`XTVERSION`** (`CSI > 0 q`). The most reliable identifier because it comes
   from the terminal itself, not from an environment variable that survived an
   `ssh` unchanged. Most modern emulators answer.
2. **Kitty protocol query** `CSI ? u`. A reply is proof; silence is proof of
   absence. Then push the flags omt wants and re-query to learn what was
   accepted — terminals may accept a subset.
3. **`modifyOtherKeys`**: set `CSI > 4 ; 2 m`, then `DECRQSS`-style readback
   where supported; otherwise infer from terminfo/fingerprint. Marked
   `source: Fingerprint` when inferred, and *never* used to enable a default
   binding — only to stop warning about a user's existing one.
4. **Terminfo / `XTGETTCAP`** (`DCS + q`) for `kbs`, `kcuu1`, the function-key
   set and `Ms` (OSC 52 support). `omt-term` already has the XTGETTCAP responder
   ([04 §1.4](04-terminal-core.md#14-where-vte-is-not-enough)); this is the
   client side of the same facility.
5. **`TERM_PROGRAM` fingerprinting** as the last resort, against a table with
   the same shape as the registry in §5.1 — data, not code.

**Adaptation.** Defaults that require a capability are declared with it:

```rust
pub struct DefaultBinding {
    pub chord: Chord,
    pub requires: Requires,      // Nothing | KittyKeyboard | ModifyOtherKeys | CmdForwarding
    pub fallback_of: Option<&'static str>,   // the always-works chord it enhances
}
```

If `requires` is unmet, the binding is **not installed** — not installed-and-
broken. `omt keys list` shows it in a dimmed "unavailable on this terminal"
section with the reason and the fallback, so it is discoverable without being a
lie. When a probe result changes (the user switches terminals, or reattaches
from a different one), the keymap is recompiled and a `KeymapChanged` event is
broadcast; the TUI shows a one-line toast: *"ctrl-shift-p is now available
(Ghostty supports the kitty keyboard protocol)"*.

`omt doctor keys` prints the whole profile plus every affected binding:

```
$ omt doctor keys

terminal      Ghostty 1.2.0        (XTVERSION, authoritative)
TERM          xterm-ghostty        TERM_PROGRAM=ghostty
kitty kbd     active, flags 0b1111 (requested 0b1111)
modifyOther   n/a (kitty protocol takes precedence)
alt sends esc yes
cmd chords    not forwarded — no macOS terminal does by default
multiplexer   none
bracketed     enabled
focus report  enabled

available because of the kitty protocol:
  ctrl-shift-p   tui.open_command_palette   (also <leader> p)
  ctrl-shift-v   media.image.paste          (also <leader> v)
  shift-enter    session.send_newline       (also alt-enter)

unavailable:
  (nothing)

2 suggestions:
  ▸ Ghostty can forward Cmd+Shift+V to omt. Add to ~/.config/ghostty/config:
        keybind = cmd+shift+v=text:\x1b[118;9u
    Install it?  [y/N]
  ▸ `input.esc_timeout` is 25ms and this session is over ssh (rtt 84ms).
    Raise to 120ms?  [y/N]
```

---

## 6. Modal keymaps: vim mode and emacs mode

Users who edit modally expect to edit modally everywhere. omt must offer a
**vim mode** and an **emacs mode** for its own editing and navigation surfaces.
Both are designed here; emacs mode may ship second, but the abstraction is built
now so that shipping it is adding a data file, not adding a code path.

This section sits after conflict detection because it *is* the hardest conflict
case in the document. A user running vim inside an omt pane while omt is in vim
mode has two programs that both believe `hjkl`, `:`, `/`, `v`, `y` and `p` are
theirs. The resolution is not clever arbitration — it is a scope rule strict
enough that the two never contend.

### 6.1 Scope: where a modal keymap applies

> **A modal keymap governs omt's own surfaces only. It never governs a pane
> whose keys are passing through to the inner program.**

| Surface | Modal keymap applies? | Why |
|---|---|---|
| Copy / scroll mode (`<leader> [`) | **Yes — the headline case** | This is tmux's `copy-mode-vi`, the single most-missed feature for vim users moving to a new multiplexer. It is an omt-owned modal surface by construction: the inner program is not receiving keys. |
| Command palette | Yes (navigation + the input line) | `Ctrl+N`/`Ctrl+P` or `j`/`k` in normal mode to move; `i`/`Esc` to enter/leave the filter field. |
| Workspace explorer tree ([15 §7.1](15-workspace-explorer.md#71-tui-panel)) | Yes | Its default map is already vi-flavoured (`j`/`k`/`h`/`l`, `g`-prefixes); vim mode makes that consistent and emacs mode gives `C-n`/`C-p`/`C-f`/`C-b` instead. |
| Session / agent picker | Yes (navigation only) | A list. Motions and search apply; operators do not. |
| omt's search bar (`<leader> /`) | Yes | Vim mode gives `n`/`N`, `*`, and search-history motions in the field. |
| Interaction cards (`CARD_FOCUSED`) | **Navigation only, explicitly** | §6.6 — operators and registers are meaningless on a card, and a modal misfire here resolves a permission prompt. |
| Settings editor, diff viewer | Yes | Both are readers/editors omt draws. |
| **A terminal pane (`terminal_focused`, no overlay)** | **No. Never.** | This is where the inner program lives. See §6.2. |
| An omt-drawn *text input* that sits over a pane (the paste confirm, a rename prompt) | Yes, but insert-first | It is omt's widget; it opens in insert mode so a user who does not know they are in vim mode can just type. |

The line is drawn by `FocusOwner` (§4.2), not by a heuristic: if the focus owner
is `Pty(_)` and no overlay is active, the modal keymap is **not consulted at
all** — the resolver skips it before it looks at any binding. That is a
structural guarantee, asserted by a property test (§10.2, extended in §6.8).

### 6.2 The hard case — omt vim mode vs. a real vim in a pane

#### The rule

**While a terminal pane has focus and omt is not in an explicit mode, every key
passes through.** omt's vim bindings are live only in omt's own surfaces, and
those surfaces are entered *deliberately* — `<leader> [`, `<leader> p`,
`<leader> e`, `<leader> a`. There is no ambient omt-vim state layered over a
pane, because there cannot be one that is also correct.

Concretely, with `keymap = "vim"` set and vim running in the pane:

| Key | Goes to |
|---|---|
| `Esc` | vim |
| `h j k l`, `w b e`, `gg`, `G`, `0`, `$` | vim |
| `:`, `/`, `?`, `n`, `N` | vim |
| `v`, `V`, `Ctrl+V` | vim |
| `y`, `p`, `d`, `c`, `.` | vim |
| `Ctrl+B` (the leader) | **omt** — the one key omt claims, per §3.2 |
| `<leader> <leader>` | vim, as `Ctrl+B` (page-back) — the escape hatch that matters most for exactly this user |

That last row is why §3.4's `<leader> <leader>` passthrough is not a nicety. A
vim user's `Ctrl+B` is `scroll-back-page`, and the leader design is only
defensible because sending it through costs one extra keystroke.

#### What structural signals change

omt knows a full-screen program is running because `omt-term` reports it, not
because omt looked at the screen:

- **Alternate screen** (`ALT_SCREEN`, DECSET 1049 — [04 §5.3](04-terminal-core.md#53-dec-private-modes)).
  vim, less, htop and most agent CLIs enter it.
- **Application cursor keys** (`APP_CURSOR_KEYS`, DECCKM).
- **Mouse reporting** (`MOUSE_REPORTING`, modes 1000/1002/1003/1006).
- **Bracketed paste** (`BRACKETED_PASTE`, mode 2004).

These are facts the program *asserted about itself* over the wire. What they
change:

| Signal | Effect |
|---|---|
| `ALT_SCREEN` | Copy mode's *entry-by-scroll* is disabled (§4.3) — the wheel goes to the program. `<leader> [` still works and shows the primary screen's scrollback, which is correct: the alt screen has no scrollback ([04 §2.6](04-terminal-core.md#26-the-grid-viewport)). |
| `MOUSE_REPORTING` | omt's mouse chrome (pane resize by drag, click-to-focus inside a pane) is suppressed, **with exactly one exception — `Shift`+click, which omt consumes for semantic activation ([18 §5.1](18-semantic-open.md#51-mouse-reporting--the-inner-program-owns-the-click))**. That mirrors what terminal emulators themselves do one level up: `Shift` has been the "talk to the multiplexer, not the app" escape from application mouse reporting since xterm, applications overwhelmingly do not expect shifted clicks, and the alternative is either stealing plain clicks from vim or having no mouse activation at all inside a TUI. Everything else — plain, `Alt`, drag, motion, wheel — is forwarded untouched. |
| `APP_CURSOR_KEYS` | Arrow keys are passed through in application form, unmodified; omt never rewrites them. |
| `BRACKETED_PASTE` | Pastes are wrapped for the inner program; without it they are stripped (§2.2). |

**None of these signals turn omt's vim mode on or off.** They only change what
omt does with the mouse and with paste. The modal keymap's scope is decided by
focus (§6.1), full stop — because a program in the alt screen might be `less`
(where copy mode would be welcome) or `vim` (where it would be theft), and the
alt-screen bit cannot tell them apart.

#### No screen-content inference. Ever.

omt does **not** detect "the user is in vim" by looking at the screen — not by
matching a status line, not by looking for `-- INSERT --`, not by title
sniffing. That would be a tier-0 heuristic ([06 §3](06-agent-layer.md#3-source-model))
driving a *structured* decision about where keystrokes go, which is exactly what
[P4](01-principles.md#p4--native-semantics-observe-never-re-implement) and the
tier discipline forbid: heuristics may produce a coarse activity guess, never a
routing decision. A wrong guess here does not mis-label a badge; it sends `dd`
to the wrong program.

The registry (§5.1) *does* carry a `vim` entry, and omt does use the foreground
process name to select it — but that only feeds **warnings** (`OMT-C406`), never
dispatch. A wrong registry produces a spurious diagnostic; it can never
misroute a key. That asymmetry is the whole reason the registry is safe to ship
as imperfect data.

### 6.3 The `Esc` problem

`Esc` is simultaneously:

1. vim's most important key,
2. Claude Code's interrupt (§5.1, severity `critical`),
3. the natural "leave the current mode" key inside omt's own surfaces,
4. `0x1B`, the first byte of every escape sequence — so a lone `Esc` is
   indistinguishable from the start of an arrow key until more bytes arrive or a
   timer fires (§2.2).

Four claims on one byte. The design resolves them in that priority order.

**Rule 1 — outside omt's surfaces, `Esc` is never omt's.** It is on the refusal
list (§5.4) for `terminal_focused`. A pane running vim or Claude Code receives
`0x1B`, always. This costs omt nothing because omt is never in a mode there.

**Rule 2 — inside omt's surfaces, `Esc` goes up exactly one level.** §8.3's
"`Esc` always leaves" rule is unchanged by modal keymaps. In vim mode the levels
are: operator-pending → visual → insert → normal → *close the surface* → the
pane. Three presses from anywhere returns to a plain pane.

**Rule 3 — the timeout is a property of the encoding, not of the mode.**

```rust
pub struct EscPolicy {
    /// Legacy encoding only. How long to wait for a continuation byte.
    pub timeout: Duration,          // input.esc_timeout, default 25ms
    /// Raised automatically when a modal keymap is active in an omt surface.
    pub modal_timeout: Duration,    // input.esc_timeout_modal, default 15ms
}
```

Under **legacy encoding**, `Esc` in an omt modal surface is decoded with a
*shorter* timeout (15 ms), not a longer one. The reasoning is asymmetric costs:
in a modal surface the plausible continuations are arrow keys, and a user who
presses `Esc` to leave insert mode and is made to wait 25 ms perceives lag on
the single most-pressed key in vim. Getting it wrong in the rare direction — an
arrow key mis-decoded as `Esc` then `A` — is recoverable and visible. Getting it
wrong in the common direction is a mode that feels broken. tmux's
`escape-time 10` exists for exactly this reason and is the right precedent.

Under **kitty protocol or `modifyOtherKeys`** the ambiguity does not exist:
`Esc` arrives as `CSI 27 ; 1 u` and is dispatched with **zero delay**. This is
the single largest quality-of-life difference the protocol makes for a vim user,
and `omt doctor keys` calls it out by name:

```
esc handling  immediate (kitty keyboard protocol) — no timeout, no lag
```

versus, on Terminal.app:

```
esc handling  15ms timeout in omt modal surfaces, 25ms in panes (legacy encoding)
              ▸ your terminal cannot disambiguate Esc from an escape sequence
              ▸ switching to a terminal with the kitty keyboard protocol removes
                this delay entirely
```

Over `omt ssh` the local end does the decoding (§7.1), so the timeout is
measured against local key timing and the network RTT never enters it. A vim
user on a 200 ms link gets the same `Esc` feel as locally — which is the
concrete meaning of "local feels remote-native" for this section.

**Rule 4 — `Ctrl+[` is `Esc`, and binding it separately is a diagnostic.**
Already covered by `OMT-C409` (§5.2). In vim mode this matters more than usual
because `Ctrl+[` is a documented vim alias for `Esc` and users type it.

### 6.4 Agent CLIs with their own modal input

Some agent CLIs offer a vi-style input mode for their prompt (readline's
`set editing-mode vi` is inherited by several; others implement their own).
**The same rule applies without modification:** while keys pass through to the
pane, they are the agent's, including `Esc`, `i`, `a`, `dd` and everything else.
omt has no modal state there to conflict with.

This is worth stating explicitly because it is the case where a user is most
likely to *expect* omt to help — "I'm in vim mode, why doesn't `Esc` do
anything in Claude Code's prompt?" — and the answer is that it does exactly what
Claude Code decided it should, which is the point. The registry carries
per-agent entries for these keys so that a user who binds over them gets
`OMT-C406`, and `omt keys explain esc` reports the agent as the consumer.

### 6.5 The keymap abstraction

A **`Keymap` is data**: a named, layered set of `(context, mode, chord) → action`
bindings over the *same* action set as every other keymap — the capability
catalog. `default`, `vim` and `emacs` are three shipped data files, not three
code paths. Nothing in `omt-tui` branches on which keymap is active.

```rust
pub struct Keymap {
    pub id: KeymapId,                    // "default" | "vim" | "emacs" | user-defined
    pub display: String,
    /// Inheritance. `vim` and `emacs` both extend `default`, so a capability
    /// that is not modal (open the palette, split a pane) is defined once.
    pub extends: Option<KeymapId>,
    /// The modal engine this keymap drives, if any.
    pub modal: Option<ModalEngine>,
    pub bindings: Vec<Binding>,
}

pub enum ModalEngine { Vim(VimConfig), Emacs(EmacsConfig) }

pub struct Binding {
    pub trigger: Chord,
    pub when: ContextPredicate,          // §4.1 contexts
    /// Modes this binding is live in. Empty == every mode (and every keymap
    /// without a modal engine).
    pub modes: ModeSet,
    pub action: Action,                  // §2.3 — capability | send-key | none
    pub repeatable: bool,                // §9.4
}

bitflags! {
    pub struct ModeSet: u8 {
        const NORMAL = 1; const INSERT = 2; const VISUAL = 4;
        const OPERATOR_PENDING = 8; const REPLACE = 16;
        // Emacs has one mode; it uses NORMAL and ignores the rest.
    }
}
```

The active keymap is one setting, resolvable per layer like any other
([10 §2.1](10-configuration.md#21-the-layers-in-precedence-order)):

```toml
# ~/.config/omt/keybindings.toml
keymap = "vim"                 # "default" | "vim" | "emacs" | a user keymap id
leader = "ctrl-b"              # orthogonal — the leader is not part of the mode

# Override two bindings inside the inherited vim keymap, leaving the rest.
[[binding]]
trigger    = "space"
when       = "copy_mode"
modes      = ["normal"]
capability = "tui.copy_mode.toggle_selection"

# Unbind one.
[[binding]]
trigger = "s"
when    = "explorer_focused"
modes   = ["normal"]
action  = "none"
```

A user keymap is a file, composable by inheritance:

```toml
# ~/.config/omt/keymaps/my-vim.toml
id      = "my-vim"
display = "vim, but with my copy-mode habits"
extends = "vim"

[[binding]]
trigger = "H"
when    = "copy_mode"
modes   = ["normal","visual"]
capability = "tui.copy_mode.goto_viewport_top"
```

Resolution order is unchanged from §2.3, with one term inserted:

1. pending chord → 2. modal state (`modes` must contain the current mode) →
3. modal context → 4. specificity → 5. layer → 6. **passthrough**.

Mode is checked *before* specificity, so a `NORMAL`-only binding never fires in
insert mode regardless of how specific its `when` is. And step 6 is still the
default: in a keymap with a modal engine, an unmatched key in `terminal_focused`
still passes through, because §6.1 skipped the modal keymap entirely there.

#### Why inheritance rather than three full keymaps

`vim` and `emacs` differ in maybe 80 bindings, all of them motion, selection and
editing. The other ~40 — split a pane, open the palette, toggle the explorer,
paste — are identical and belong to `default`. Three independent files would
drift, and a new capability would need three edits. `extends` makes the shipped
diff small enough to review, and makes "emacs mode ships later" a matter of
writing one file that inherits the same base.

#### The parity consequence

Because all three keymaps target the same action set, **every vim/emacs binding
is by construction reachable another way**: through the palette, by name. That
is what makes §6.9's mobile story work without special-casing, and it is why the
parity test needs no new rule for modal keymaps.

### 6.6 Vim mode: the implemented subset, and the deliberate omissions

`omt-input`'s vim engine is a small state machine over omt's own surfaces. It is
**not** a vim, and does not aspire to be — there is a real vim one keystroke
away, in a pane.

```rust
pub struct VimState {
    pub mode: VimMode,
    pub pending: Option<PendingOperator>,
    pub count: Option<u32>,              // prefix count, capped at 10_000
    pub register: Option<char>,          // "a-"z, "+ (system), "" (unnamed)
    pub last_search: Option<Search>,
    pub last_change: Option<Repeatable>, // powers `.`
}

pub enum VimMode { Normal, Insert, Visual(VisualKind), OperatorPending, Replace }
pub enum VisualKind { Char, Line, Block }

pub struct PendingOperator { pub op: Operator, pub count: Option<u32> }
pub enum Operator { Yank, Delete, Change }
```

**Implemented:**

| Group | What |
|---|---|
| Modes | `normal`, `insert`, `visual` (char/line/block), `operator-pending`, `replace` (single-char `r` only) |
| Motions | `h j k l`, `w W b B e E`, `0 ^ $`, `gg G`, `{ }`, `( )`, `H M L`, `Ctrl+D/U/F/B`, `f F t T` + `; ,`, `%` |
| Counts | `3w`, `d2j`, `10G` — a decimal prefix on motions and operators |
| Operators | `y`, `d`, `c` over any motion, plus doubled forms `yy`/`dd`/`cc` and visual-mode application |
| Text objects | `iw aw`, `i" a"`, `i' a'`, `i( a(`, `i[ a[`, `i{ a{`, `ip ap` — the ones that pay for themselves when yanking from scrollback |
| Registers | Named `"a`–`"z`, unnamed, and `"+` mapped to `media.clipboard.write` ([09 §3](09-ssh-and-media.md)) so `"+y` in copy mode puts text on the *local* clipboard, over ssh, correctly |
| Search | `/`, `?`, `n`, `N`, `*`, `#`, with `incsearch` behaviour and `smartcase` |
| Marks | `` m<a-z> `` and `` `<a-z> ``, stored as `Position` ([04 §3.2](04-terminal-core.md#32-positions)) so they survive reflow |
| Repeat | `.` for the last change, `;`/`,` for the last `f`/`t` |
| Paste | `p`, `P` into omt's own input fields (never into a pane) |
| Insert-mode | `Ctrl+W`, `Ctrl+U`, `Ctrl+R <reg>`, `Ctrl+V <literal>` — the readline habits vim users keep |

**Deliberately not implemented**, each with a reason:

| Omitted | Why |
|---|---|
| **Ex commands beyond a fixed set** (`:w`, `:e`, `:s`, ranges, `:g`) | omt's surfaces are not buffers; there is nothing to write. `:` is instead bound to **the command palette**, which is the honest mapping: it is the "type a command by name" affordance, and it makes the palette reachable from vim muscle memory. A small fixed set (`:q`, `:qa`, `:noh`, `:<number>` to jump to a line) is recognized. |
| **Macros** (`q`/`@`) | Recording arbitrary key sequences that replay through omt's dispatcher is a capability-replay engine with no undo. If users want automation, workflows ([10 §9.2](10-configuration.md#92-workflows)) are the designed answer and they are inspectable, shareable and parameterized. |
| **Undo/redo** (`u`, `Ctrl+R`) | omt's modal surfaces are readers and single-line inputs. There is no edit history to undo. `u` is left unbound rather than mapped to something surprising. |
| **Windows and buffers** (`Ctrl+W` chords, `:bn`) | omt already has panes and sessions with a leader namespace (§8.2). Two competing window systems in one program is worse than one. `Ctrl+W` keeps its insert-mode meaning (delete word back). |
| **Folds, jumplist, tags, quickfix** | No corresponding structure in omt's surfaces. The explorer's diff viewer has its own `]c`/`[c` hunk motions ([15 §7.1](15-workspace-explorer.md#71-tui-panel)), which is the useful 5 %. |
| **`:map` / user-defined vim-syntax remapping** | Remapping is `keybindings.toml` (§6.5). A second remapping language inside the first would need its own validator and would bypass §5.2's conflict detection entirely — which is the one thing this document cannot allow. |
| **Modal state on interaction cards** | §6.1. Cards get motions (`j`/`k`, digits) and nothing else. An operator-pending state on a permission prompt is a way to answer the wrong question. |

**Mode entry and exit.** `i`, `a`, `I`, `A`, `o`, `O` enter insert where an input
field exists; in a read-only surface they are unbound and produce a one-line
status (`-- read-only --`) rather than silence. `Esc` follows §6.3 rule 2.
`Ctrl+C` in a modal surface behaves as `Esc` — it is an *overlay* context, so
the refusal list (§5.4) permits it, and vim users type it.

**Copy mode is where this lives.** In practice ~90 % of vim-mode use is
`<leader> [`, then motions, then `v`, then `y`. That path is the one that gets
the conformance tests (§6.8) and the one whose latency budget matters.

### 6.7 Emacs mode

Emacs mode has one mode, so `ModeSet` is always `NORMAL` and the engine is
smaller — but it needs three pieces of state vim does not.

```rust
pub struct EmacsState {
    /// The mark. Selection is the region between mark and point.
    pub mark: Option<Position>,
    pub mark_active: bool,               // transient-mark behaviour
    pub kill_ring: KillRing,             // bounded, default 60 entries
    pub last_command: LastCommand,       // powers append-to-kill and `C-y`/`M-y`
    pub prefix_arg: Option<PrefixArg>,   // C-u, C-u 4, M-<digit>
}

pub enum PrefixArg { Universal(u32), Numeric(i32) }
```

**Implemented:**

| Group | What |
|---|---|
| Motion | `C-f C-b C-n C-p`, `C-a C-e`, `M-f M-b`, `M-< M->`, `C-v M-v` |
| Mark & region | `C-SPC` set mark, `C-x C-x` exchange point and mark, region highlighting |
| Kill & yank | `C-w`, `M-w`, `C-k`, `C-y`, `M-y` (cycle the kill ring), append-on-consecutive-kills |
| Search | `C-s`, `C-r` incremental, `M-%` query-replace **in omt's input fields only** |
| Prefix args | `C-u`, `C-u <n>`, `M-<digit>` |
| `C-x` prefix map | `C-x C-x`, `C-x o` (other pane), `C-x 2`/`C-x 3` (split), `C-x 0`/`C-x 1` (close/zoom pane), `C-x b` (session picker), `C-x k` (close session), `C-x C-f` (explorer) |
| `C-g` | Universal cancel — clears prefix args, pending chords, the mark, and closes the surface. §3.4 already binds it as the leader's cancel, so it is consistent. |
| **`M-x` → the command palette** | See below. |

**`M-x` is the elegant part.** Emacs's `M-x` is "invoke a command by name, with
completion, over the complete set of commands". omt's command palette is
"invoke a capability by name, with completion, over the complete catalog". These
are the same thing, and mapping one onto the other means emacs mode gets a
genuinely native-feeling `M-x` for free — including `M-x` on a capability with
arguments opening the argument form, which is the direct analogue of
`interactive` prompting. `C-h k` (describe key) maps onto `keys.explain`
(§5.3), which is the same coincidence a second time.

**The `C-x` prefix vs. the leader.** Emacs mode ships with `C-x` as a *second*
prefix alongside `<leader>`. This is deliberate duplication: `C-x 2` and
`<leader> |` both split, and both are listed. Emacs users type `C-x`; the leader
is what everything else in this document is built on and must keep working. The
validator has a specific rule for the resulting collision class (§6.8).

**Deliberately not implemented:** the minibuffer as a general concept (the
palette is it), `M-x` on arbitrary elisp (there is none), keyboard macros (same
reasoning as vim's `q`/`@`), rectangle commands, and `C-x C-c` — which would
mean "quit omt" and is far too easy to hit by muscle memory next to `C-x C-x`.
`C-x C-c` is bound to `none` with a status line explaining that `<leader> d`
detaches and `omt kill-server` exits.

**Shipping order.** Vim mode ships first (larger constituency in this audience,
and copy-mode-vi is the concrete demand). Emacs mode is a data file plus the
`EmacsState` engine; nothing in §6.5's types changes to accommodate it, which is
the test of whether the abstraction was right.

### 6.8 Conflict validation for modal keymaps

Modal keymaps add six new failure modes. Each gets a code, continuing the
`OMT-C4xx` range from §5.2, and each is reported by `keys.conflicts` and covered
by §10.3's matrix on the same terms as the 400-range.

| Code | Severity | Rule |
|---|---|---|
| `OMT-C420` | warning | A modal binding whose `when` includes `terminal_focused` — i.e. someone tried to make vim mode ambient over a pane |
| `OMT-C421` | error | A binding declares `modes` but the active keymap has no modal engine |
| `OMT-C422` | warning | An operator-pending sequence swallows a chord: a binding whose first key is an `Operator` and which would be unreachable because the operator consumes the next key |
| `OMT-C423` | warning | `C-x` prefix (emacs mode) collides with a `<leader>`-rooted chord or with a registry `critical` key |
| `OMT-C424` | note | A modal binding shadows the active agent's key *in a context where it can never fire* — reported as a note so the user knows the `OMT-C406` they expected is absent for a reason |
| `OMT-C425` | warning | `input.esc_timeout` ≥ 100 ms with a modal keymap active and no kitty protocol — the mode will feel laggy |

Verbatim:

```
$ omt config validate

warning[OMT-C420]: vim-mode binding would apply inside a terminal pane
  ┌─ ~/.config/omt/keymaps/my-vim.toml:14:1
  │
14 │ when = "terminal_focused"
  │        ^^^^^^^^^^^^^^^^^^ modal bindings never apply here
  │
  = note: while a pane has focus and no omt surface is open, every key passes
          through to the program in the pane — that is what makes it safe to
          run vim inside omt while omt is in vim mode
  = note: this binding is compiled, but it can never fire
  = help: did you mean `copy_mode`? that is omt's modal scrollback surface:
              when = "copy_mode"
  = note: docs/architecture/16-input-and-keymap.md §6.1

warning[OMT-C422]: `d` swallows the chord `d g s`
  ┌─ ~/.config/omt/keymaps/my-vim.toml:22:1
  │
22 │ trigger = "d g s"
  │           ^^^^^^^ unreachable in operator-pending mode
  │
  = note: `d` is the delete operator; after it, the next key is consumed as a
          motion or a text object, so `g` is read as the start of `gg`/`ge`
  = help: bind this outside the operator namespace, e.g. `<leader> g s`
  = help: or define `g s` as a motion with `kind = "motion"` so the operator
          can compose with it

warning[OMT-C423]: `ctrl-x` collides with an emacs prefix map
  ┌─ ~/.config/omt/keybindings.toml:31:1
  │
31 │ "ctrl-x" = "session.close"
  │ ^^^^^^^^ bound as a complete chord
  │
  = note: keymap `emacs` defines 9 chords under the `ctrl-x` prefix
          (ctrl-x o, ctrl-x 2, ctrl-x 3, ctrl-x b, …)
  = note: binding `ctrl-x` as a complete chord makes all 9 unreachable
  = help: `omt keys list --prefix ctrl-x` shows them
  = help: unbind the prefix map instead if that is what you want:
              "ctrl-x" = "none"   # then rebind ctrl-x

warning[OMT-C425]: Esc will feel slow in modal surfaces on this terminal
  ┌─ ~/.config/omt/config.toml:8:20
  │
 8 │ esc_timeout = "150ms"
  │                ^^^^^^^ with keymap = "vim" and no kitty keyboard protocol
  │
  = note: detected terminal: Apple_Terminal — cannot disambiguate Esc
  = help: 15ms is the default for modal surfaces; 150ms is a perceptible delay
          on every mode exit
  = note: a terminal with the kitty keyboard protocol removes the delay entirely

3 warnings, 1 note — keybindings applied.
```

`OMT-C420` is a **warning, not an error**, for the same reason as `OMT-C406`
(§5.4): the user may be writing a keymap for a future context, and a dead
binding is inert, not dangerous. But it is the diagnostic most likely to be the
user's actual misunderstanding, so it carries the longest explanation and a
pointer back to §6.1.

**Test matrix additions** (extending §10.3):

- Every mode transition in `VimState`, as a table-driven test over
  `(mode, key) → (mode, action)`, asserting totality — no key in any mode
  reaches an unhandled arm.
- `d{motion}` for every implemented motion and text object, asserting the
  resulting `Range<Position>` against a fixture scrollback, at three widths, to
  catch reflow interaction ([04 §3.6](04-terminal-core.md#36-property-tests-see-also-10)).
- A property test: **for any keymap and any modal state, a key delivered while
  `FocusOwner == Pty(_)` with no overlay always resolves to `Passthrough`.**
  This is §6.1 and §6.2's guarantee, and it is the single most important test in
  the section.
- A property test: from any `VimState`, at most three `Esc` presses reach
  `VimMode::Normal` with no surface open.
- Emacs: `C-g` from any state returns to a quiescent state with no prefix arg,
  no pending chord, and `mark_active == false`.
- Golden diagnostics for `OMT-C420`–`OMT-C425`, so the output above is the
  fixture.

### 6.9 Parity: the web client and mobile

**Web desktop is genuinely easy**, and this is one of the few places the browser
is a *better* substrate than a terminal. The web client receives real
`KeyboardEvent`s with `key`, `code`, `location` and every modifier — no encoding
limits, no `Ctrl+Shift` ambiguity, no `Esc` timeout. So:

- The same three keymaps are served by the instance and interpreted client-side
  by the same logic, ported once. `code` gives layout-independent physical keys
  for free, which is what §9.1 has to negotiate the kitty protocol to obtain.
- `Esc` is unambiguous, so mode exit is instantaneous.
- The scope rule is identical: while the xterm.js terminal view has focus,
  every key goes to the PTY. Modal bindings live in block view, the palette, the
  explorer rail, the diff viewer and the card sheets.
- One browser-specific hazard: `Ctrl+N`, `Ctrl+W`, `Ctrl+T` and `Cmd+W` are
  browser chords the page cannot reliably take. Emacs mode's `C-n`/`C-p` are
  fine (`Ctrl+N` opens a window only when not preventable — and in a focused
  text context `preventDefault` works), but `C-w` is a real collision with
  "close tab" on some platforms. The web keymap therefore ships `C-w` as
  kill-region **only inside a focused input**, and the palette entry is the
  documented alternative. `omt keys explain` reports this per-browser.

**Mobile has no modifiers and no `Esc` key**, so a modal keymap in the vim/emacs
sense does not exist there. Parity is preserved by the mechanism §6.5 already
guarantees — every modal binding names a capability, and every capability is in
the palette:

| Vim/emacs affordance | Mobile equivalent |
|---|---|
| `v` then motion then `y` | Long-press to start a selection, drag handles, **Copy** in the action row ([08 §8.4](08-web-client.md#84-gestures)) |
| `/` then `n`/`N` | The search field with next/prev buttons |
| `gg` / `G` | Jump-to-top / jump-to-bottom buttons on the scroll rail |
| `:` / `M-x` | The palette button — literally the same surface |
| `C-x 2` / `<leader> \|` | The split control in the session view |
| `"+y` | **Copy** — the phone's clipboard is the system clipboard, so the register distinction evaporates |
| Mode itself | An optional mode chip in the header (§6.10), off by default on mobile |

The virtual key bar ([08 §4.3.1](08-web-client.md)) already provides `Esc`,
`Tab`, sticky `Ctrl` and sticky `Alt` — which is what a mobile user needs to
drive **the inner program's** vim, and that is the case that actually matters on
a phone. omt's own modal keymap is not reproduced there, and the parity test is
satisfied by the palette, not by pretending a phone has a `Meta` key.

### 6.10 Discoverability

Three affordances, all of which exist for `default` too and simply carry more
information when a modal keymap is active.

**1. The mode indicator.** A chip in the focused surface's status line, using
the theme's agent-state colour slots ([10 §8.1](10-configuration.md#81-theme-format))
so it is legible in every theme:

```
 ┌ scrollback ──────────────────────────────── ▲ 412 lines ─┐
 │ …                                                        │
 └ NORMAL  "a  3   /err                        vim  ⌃b ? ───┘
    │      │   │    │                           │
    mode   reg count last search           active keymap
```

It is shown **only in omt's own modal surfaces** — never over a pane, because a
`NORMAL` chip floating over vim would claim a mode omt does not have there, and
that is precisely the confusion §6.2 exists to prevent. That rule is worth more
than the pixels it costs.

**2. Pending-chord hints.** §3.3's which-key strip is not leader-specific; it is
driven by the `ChordTrie` and therefore covers operator-pending and `C-x` too:

```
 d ▸   w word   iw inner word   d line   $ to end   j line down   } paragraph
```

```
 C-x ▸   o other pane   2 split below   3 split right   b sessions   C-f explorer
```

Same 250 ms delay, same `?` for the full list, same non-modal rendering over the
surface.

**3. `omt keys explain`** reports the keymap and mode as first-class fields:

```
$ omt keys explain d --context copy_mode

d  →  omt, vim keymap, operator

  keymap    vim (extends default)      set at: ~/.config/omt/keybindings.toml:3
  context   {copy_mode}
  mode      normal → operator-pending
  L2        Ghostty — delivered as 0x64
  L3        omt — `d` is the delete operator; the next key is read as a motion
            or text object. In copy mode `d` yanks-and-scrolls rather than
            deleting: scrollback is immutable.
  L4        not reached (copy mode is an omt surface)

  next keys: w W b B e E 0 ^ $ j k gg G { } ( ) f F t T iw aw i" a" i( a( ip ap d


$ omt keys explain d

d  →  passes through

  context   {terminal_focused, agent_bound}   no overlay
  keymap    vim — NOT CONSULTED here (§6.1: modal keymaps never govern a pane)
  L4        Claude Code receives 0x64

  verdict: your vim mode does not apply inside a pane. This is deliberate —
           it is what lets you run vim in this pane.
```

That second output is the one that answers the support question this whole
section generates, which is why it is specified verbatim.

`omt keys list --keymap vim --diff default` prints only what vim mode changes,
which is the reviewable artifact for the shipped data file.

---

## 7. `omt ssh` — local feels remote-native

When the user runs `omt ssh <target>` or `omt --remote <target>`, omt is the
program on **both** ends ([09 §6](09-ssh-and-media.md)). It owns the keyboard
end to end, and can therefore offer things a foreign terminal never could.

### 7.1 What the thin client intercepts vs. forwards

The local process runs only `omt-tui` and holds no session state. But it is a
full omt binary, so it has the local OS clipboard, the local filesystem, and the
local terminal profile (§5.5).

```rust
/// Where a resolved binding must execute.
pub enum Locality {
    /// Runs on the machine the human is sitting at. Never forwarded.
    Local,
    /// Runs on the instance that owns the session. Forwarded.
    Remote,
    /// Runs locally, then calls a remote capability with the local result.
    Composite,
}
```

| Locality | Bindings | Why |
|---|---|---|
| `Local` | `tui.zoom_font`, `tui.detach`, `media.clipboard.write` (writing to the *local* clipboard), local terminal profile actions, `keys.explain` for L2 questions | These need the local OS or the local terminal, and forwarding them would target the wrong machine. |
| `Remote` | Everything else: `session.*`, `pane.*`, `agent.*`, `explorer.*`, `workflow.*`, the palette's contents | The state lives there. |
| `Composite` | `media.image.paste`, `media.file.push`, `session.paste_from_local`, `keys.explain` on a full chord | Read locally, act remotely. §7.2. |

**Keymap composition.** There is exactly one keymap, and it is the **remote
instance's**, fetched at attach and cached. Local device overrides
([10 §3.4](10-configuration.md#34-what-a-device-may-shadow)) shadow it for
`Device`-owned entries. The user never chooses between "local keymap" and
"remote keymap", because there is only one to choose from; the `Locality` on
each binding decides where it runs, and that is a property of the *capability*,
declared in the catalog, not of the user's mental model.

Two consequences that make this feel native rather than remote:

- **Decoding happens locally**, against the *local* terminal's profile. The
  remote never sees raw bytes for bound keys; it receives structured
  `KeyEvent`s. So `Esc` disambiguation, `Alt` handling and kitty-protocol
  negotiation are all done where the measurements are accurate (§2.2), and a
  200 ms link does not turn `Esc` into a coin flip.
- **Passthrough is still bytes.** Unmatched keys are forwarded as their original
  bytes on the input stream ([07 — Remote protocol](07-remote-protocol.md)),
  multiplexed at higher priority than media frames, so the inner program on the
  remote box sees exactly what it would have seen over a plain ssh.

### 7.2 File and screenshot paste

The user requirement: press one chord, and whatever is on the **local**
clipboard — an image, a file, a path — ends up in the **remote** instance's
managed temp directory, with the right reference inserted at the cursor.

**Default chord: `<leader> v`.** Plus `ctrl-shift-v` when the kitty protocol is
available, plus an opt-in `cmd-shift-v` via terminal remapping (§7.4). All three
resolve to the same capability.

The flow, with the transport delegated to [09](09-ssh-and-media.md) rather than
redefined here:

```
<leader> v  (local, thin client)
 │
 1. media.clipboard.read on the LOCAL instance          [09 §4.1]
 │     macOS NSPasteboard / X11 / Wayland / Windows
 │     → ClipboardContents { text, blobs, source }
 │
 2. classify (§7.3) → ContentPlan
 │
 3. media.blob.begin { hash, len, mime, filename }       over the ssh-stdio
 │     control channel                                   [09 §6.1]
 │     ← { have: true }  → skip to 5    (dedup: same screenshot twice is free)
 │     ← { have: false } → 4
 │
 4. binary frames on the media stream, lower priority than input/output
 │     → media.transfer.progress events → progress UI (§7.3)
 │
 5. media.blob.commit  → remote materializes
 │     refs/<session>/<short>-<name>.png                 [09 §2]
 │
 6. insert the reference at the cursor (§7.3), via the agent's own channel
 │     when one exists (agent.prompt / ACP resource block), else as text
```

Nothing here is new transport: it is exactly [09 §6.1](09-ssh-and-media.md)'s
thin-client media path, reached by a key instead of by a web upload. That is the
point — the keymap contributes the *trigger* and the *insertion*, and the media
crate contributes everything else.

### 7.3 What is on the clipboard, and what gets inserted

```rust
pub enum ContentPlan {
    /// Image bytes on the clipboard (a screenshot).
    Image { mime: Mime, len: u64 },
    /// One or more file URLs (Finder/Explorer copy, or a drag).
    Files { paths: Vec<PathBuf>, total: u64 },
    /// Text that is a plausible existing local path.
    LocalPath { path: PathBuf, len: u64 },
    /// Ordinary text.
    Text { len: usize, lines: usize },
    Empty,
}
```

| Clipboard content | What omt transfers | What is inserted at the cursor |
|---|---|---|
| Image bytes | the decoded image, EXIF-stripped ([09 §8](09-ssh-and-media.md)) | the agent's image reference via `AgentAdapter::image_reference` — `@<remote-path>` for Claude Code, a structured resource block over ACP, `/add <path>` for Aider ([09 §7.1](09-ssh-and-media.md)) |
| One file | the file | `@<remote-path>` (or the adapter's form) |
| Several files | all of them, one transfer id | one reference per file, space-separated, on one line |
| A local path (text that resolves to a local file) | **the file's contents**, not the string | `@<remote-path>`, with a one-line confirm: *"paste the file `report.pdf` (2.1 MB), or the literal path text?"* — the ambiguity is real and omt asks rather than guessing |
| Ordinary text ≤ `paste_confirm_lines` | nothing | the text, inside bracketed paste if the inner program enabled mode 2004 |
| Ordinary text > `paste_confirm_lines` (default 10) or containing a newline | nothing | a preview + confirm first, per [10 §7.3](10-configuration.md#73-terminal)'s `paste_confirm_*` |
| Empty | — | a one-line status, no dialog |

**Progress.** Transfers over 256 KiB show an inline progress line in the pane's
status row — `paste  report.pdf  1.4 / 2.1 MB  ▓▓▓▓▓▓░░░  ⌫ cancel` — driven by
`media.transfer.progress` **events** ([09 §5.3](09-ssh-and-media.md#53-tier-2--the-in-band-osc-bridge) owns it; it is an event, never a capability), which the web client renders as a bar for the
same transfer. At ssh throughput a 20 MB file is seconds, not instant, and
silence would read as a hang.

**Cancel** is `Ctrl+C` *while the progress line is showing*, which is the one
place omt scopes `ctrl-c` (a scoped binding, `when = "transfer_active"`, which
the refusal list permits). Cancelling sends `media.blob.abort` ([09 §5](09-ssh-and-media.md#5-case-b-image-paste-over-ssh--the-core-mechanism), which owns it); the remote
discards the partial and unlinks it. The user's next `Ctrl+C` goes to the agent
as normal.

**Failure** is instantaneous and diagnosed, never a retry loop: quota exceeded
names the limit, an unreadable clipboard names the reason, and a broken control
channel offers the [09 §5.5](09-ssh-and-media.md) alternatives (QR to phone,
`omt paste --to`). Nothing is inserted at the cursor on failure — a half-pasted
reference in an agent prompt is worse than no paste.

### 7.4 Making `Cmd+Shift+V` work, honestly

It cannot work by default (§1.5). It **can** work if the user's terminal is
configured to send a sequence omt recognizes, because at that point the chord is
just an escape sequence like any other.

omt reserves `CSI 118 ; 9 u` — kitty-protocol encoding for `super+shift+v` — as
the sequence to remap onto. Using the protocol's own encoding rather than a
private sequence means the binding is expressible in `keybindings.toml` as
`"cmd-shift-v"` and behaves identically to a natively-delivered chord.

Exact snippets, printed by `omt doctor keys` for the detected terminal:

**kitty** (`~/.config/kitty/kitty.conf`):
```conf
# omt: forward Cmd+Shift+V to the running application
map cmd+shift+v send_text all \x1b[118;9u
```

**Ghostty** (`~/.config/ghostty/config`):
```conf
# omt: forward Cmd+Shift+V to the running application
keybind = cmd+shift+v=text:\x1b[118;9u
```

**WezTerm** (`~/.wezterm.lua`):
```lua
-- omt: forward Cmd+Shift+V to the running application
config.keys = config.keys or {}
table.insert(config.keys, {
  key = 'v', mods = 'CMD|SHIFT',
  action = wezterm.action.SendString '\x1b[118;9u',
})
```

**iTerm2** — no text config file for keys; it is a plist. omt prints the manual
path and offers to write it:
```
Preferences ▸ Keys ▸ Key Bindings ▸ +
  Keyboard Shortcut : ⌘⇧V
  Action            : Send Escape Sequence
  Esc+              : [118;9u
```
`omt doctor keys --fix --terminal iterm2` writes the entry into
`~/Library/Preferences/com.googlecode.iterm2.plist` under `GlobalKeyMap` after
(a) showing the exact change, (b) taking a timestamped backup, and (c) an
explicit confirm. It refuses while iTerm2 is running (the plist is cached in
memory and would be overwritten on quit) and says so.

**Terminal.app** — supports "Send Text" key bindings in profile settings, so the
same remap is possible by hand; omt prints the steps but does not automate the
plist, because Terminal.app's profile plist structure is nested per-profile and
a wrong write breaks the user's profile. **ASSUMED**, needs verification (§13).

**Windows Terminal** — `Cmd` does not exist; the equivalent is `Ctrl+Shift+V`,
which Windows Terminal binds to Paste by default. `settings.json`:
```jsonc
{ "command": { "action": "sendInput", "input": "[118;6u" },
  "keys": "ctrl+shift+v" }
```

**Alacritty** (`alacritty.toml`):
```toml
[[keyboard.bindings]]
key = "V"; mods = "Command|Shift"; chars = "[118;9u"
```

`omt doctor keys --fix` for each: show the diff, back up the file, write, and
tell the user to reload the terminal's config. For every terminal it prints the
same closing line: *"`<leader> v` already works and needs no configuration."*

### 7.5 The `omt doctor keys` flow

```
$ omt doctor keys
  … profile as in §5.5 …

paste chords
  <leader> v      ✓ works now
  ctrl-shift-v    ✓ works now (kitty keyboard protocol active)
  cmd-shift-v     ✗ consumed by Ghostty
                    ▸ fix available: add one line to ~/.config/ghostty/config
                      Install it?  [y/N] y
                      ✓ wrote ~/.config/ghostty/config (backup: config.omt-bak-20260803)
                      ! reload Ghostty's config (Cmd+Shift+,) to activate

conflicts
  ! ctrl-o shadows Claude Code's command palette   (keybindings.toml:12)
    ▸ omt keys explain ctrl-o
```

The `omt doctor keys` flow is the `keys` group of the `system.doctor` capability
([22 §10](22-operations.md#10-capabilities)), so the web settings UI runs the
same detection and shows the same fixes with a copy button instead of a prompt.

### 7.6 Where the web client differs

| | TUI (foreign terminal) | TUI (`omt ssh`) | Web desktop | Web mobile |
|---|---|---|---|---|
| `Cmd+Shift+V` | never | only via §7.4 remap | **yes** — the browser receives it, and a real `paste` event carries the clipboard | n/a |
| Clipboard image | tiered ([09 §5](09-ssh-and-media.md)) | native local read | `ClipboardEvent.clipboardData.files` — no permission prompt needed on a genuine paste | share sheet / photo picker |
| Modifiers | encoding-limited | encoding-limited | full | **none at all** |
| Leader chord | yes | yes | yes | no — no modifiers |

The browser case is genuinely better: a `paste` event hands the page the
clipboard contents *because the user pasted*, which is the permission model
omt wants and cannot have in a terminal.

Mobile has no modifiers, so parity is preserved by the two mechanisms P3 already
requires: the **palette** (every capability, searchable) and the **virtual key
bar** ([08 §4.3.1](08-web-client.md)) with sticky `Ctrl`/`Alt` for the inner
program's chords. `<leader>` itself is a key-bar button — tapping it enters the
pending state and the key bar's second row becomes the leader menu, which is the
touch rendering of §3.3's hint strip. Every entry in §8.1 has a web equivalent
for exactly this reason; a binding with no web affordance fails the parity test.

---

## 8. Defaults

### 8.1 The un-prefixed budget: five chords

These are the *only* keys omt claims when a terminal pane has focus and no
overlay is open. Everything else in this document is behind the leader or in the
palette.

| Chord | Context | Capability | Web equivalent | Rationale |
|---|---|---|---|---|
| `<leader>` (`ctrl-b`) | `terminal_focused` | *(prefix)* | key-bar `⌘` button → leader menu | The one key that buys the whole namespace. |
| `ctrl-shift-p` | any | `tui.open_command_palette` | `⌘K` / palette button | Only installed when the kitty protocol or `modifyOtherKeys` is negotiated; otherwise `<leader> p` is the only palette chord. Universal muscle memory where it exists. |
| `ctrl-shift-v` | `terminal_focused` | `media.image.paste` | `⌘V` / paste button | Same conditional installation. The chord users try first. |
| wheel / trackpad scroll | `terminal_focused && !mouse_reporting` | `tui.enter_copy_mode` + scroll | native scroll | Not a key, but it is input, and taking it only when the inner program has not asked for the mouse is the whole rule. |
| `shift-enter` | `terminal_focused && agent_bound` | `session.send_newline` | composer newline button | Only under the kitty protocol; inserts a literal newline in an agent prompt instead of submitting. The single most-requested agent-CLI ergonomic. Falls back to `alt-enter`, which *is* deliverable in legacy encoding. |

That is it. Four conditional, one unconditional.

### 8.2 The leader namespace

All `when = "terminal_focused"` unless noted; all reachable in the palette by
name, which is the web equivalent for every row without a more specific one.

| Chord | Capability | Web equivalent | Rationale |
|---|---|---|---|
| `<leader> <leader>` | `SendKey(leader)` | key-bar `Ctrl`+`b` | Escape hatch. Non-negotiable for a prefix design. |
| `<leader> p` | `tui.open_command_palette` | `⌘K` / palette button | The P3 workhorse. |
| `<leader> ?` | `tui.open_keymap_help` | help sheet | Shows the resolved map *and* the active inner keymap side by side. |
| `<leader> c` | `session.create` | `+` in the session list | tmux parity. |
| `<leader> x` | `session.close` (confirm) | long-press ▸ Close | tmux parity; confirm because `effects` includes `DESTRUCTIVE`. |
| `<leader> w` | `tui.open_session_picker` | session list | tmux parity. |
| `<leader> \|` / `<leader> -` | `pane.split` v/h | split buttons (desktop) | Mnemonic glyphs beat tmux's `%`/`"`. |
| `<leader> h j k l` / arrows | `pane.focus_direction` | tap a pane | Directional focus; arrows for non-vi users. |
| `<leader> z` | `pane.zoom` | expand button | tmux parity. |
| `<leader> [` | `tui.enter_copy_mode` | scroll / select | tmux parity. |
| `<leader> /` | `tui.search` | search field | Searches omt's scrollback, not the inner program's. |
| `<leader> e` | `explorer.toggle` | explorer rail / sheet | Already specified in [15 §7.1](15-workspace-explorer.md#71-tui-panel). |
| `<leader> a` | `interaction.focus_latest` | tap the card | §4.4 — the *only* way a card takes focus. |
| `<leader> v` | `media.image.paste` | paste button / `⌘V` | **The universal paste chord.** Works in every terminal, on every platform, with no configuration. §7.2. |
| `<leader> V` | `media.picker.open` | attach button | Choose what to paste when the clipboard has several representations. |
| `<leader> y` | `media.clipboard.write` (selection) | copy button | Sends the current selection to the *local* clipboard via the best available path ([09 §3](09-ssh-and-media.md)). |
| `<leader> d` | `tui.detach` | close the tab | Detach, leaving sessions running. `Local` in thin-client mode (§7.1). |
| `<leader> ,` | `tui.open_settings` | settings | Parity with [10](10-configuration.md). |
| `<leader> f` | `open.hints.begin` (default action) | hint overlay / tap a match | Hint mode over every match in the viewport ([18 §5.2](18-semantic-open.md#52-hint-mode--the-primary-mechanism)). The primary, mouse-free way to act on a `file:line` or a URL. |
| `<leader> F` | `open.hints.begin` `{ action = "menu" }` | hint overlay ▸ menu | Same, but always show the action menu on select rather than the per-kind default. |
| `<leader> g` | `open.hints.begin` `{ kinds = ["url"] }` when `terminal_focused`; *(prefix)* when `explorer_focused` | hint overlay, URLs only | URL-restricted hint mode. The same chord is the explorer's `g`-prefixed map ([15 §7.1](15-workspace-explorer.md#71-tui-panel)) when the explorer has focus — two bindings differing by `when`, resolved by §2.3 rule 3, not a collision. |
| `<leader> A` | `tui.open_agent_dashboard` | dashboard tab | The mobile-first view, on the desktop too. |
| `<leader> !` | `agent.interrupt` | swipe left on a dashboard row | Interrupt the bound agent *without* sending `Ctrl+C` to the pane — for agents with a native interrupt channel. |

### 8.3 Contextual maps

Copy mode, the explorer, the palette, the picker and card focus each have their
own map. Two rules govern all of them:

1. **They are `Priority` or `Exclusive` (§4.2), so they do not need to avoid the
   inner program's keys** — while an overlay owns the keyboard, the inner
   program is not receiving keys anyway. This is why `j`/`k` in copy mode is
   fine and `j`/`k` globally would be absurd.
2. **`Esc` always leaves.** In every omt context, `Esc` goes up one level, and
   at the top level it passes through to the inner program. A user who presses
   `Esc` three times is always back to a plain pane.

The explorer's map is specified in [15 §7.1](15-workspace-explorer.md#71-tui-panel)
and is not restated here; card focus uses `↑`/`↓`/digits to select, `n` for a
comment ([08 §5.2.1](08-web-client.md)), `Enter` to resolve, `Esc` to unfocus.

---

## 9. Accessibility and internationalization

### 9.1 Non-US layouts

Binding on a *character* is wrong for anyone not on a US layout: `<leader> |`
requires `AltGr` on a German keyboard and does not exist on a French AZERTY
without a dead-key dance.

The rule: **bindings resolve against the base-layout key when the terminal
reports it, and against the produced character otherwise.** The kitty protocol's
alternate-key reporting (flag `0b100`) gives omt the base-layout code, so
`<leader> |` matches the physical key that produces `|` on the user's layout
*and* the US-layout position. Under legacy encoding only the produced character
is available, and omt says so in `omt keys explain`.

Consequences baked into §8:

- No default binds a character that requires `AltGr` on a common layout, except
  `<leader> |` and `<leader> -`, which are paired with `<leader> v`/`<leader> s`
  aliases (vertical/split) installed automatically when the detected layout
  makes `|` awkward. (**Layout detection is only possible under the kitty
  protocol's alternate-key reports; elsewhere both aliases are installed
  unconditionally**, which costs two chords in a namespace that has room.)
- Digits and letters are preferred over punctuation throughout.

### 9.2 Dead keys

A dead key produces no character until the next keypress. Under legacy encoding
omt sees only the final composed character and passes it through — correct
behaviour, nothing to do. Under the kitty protocol, dead keys may be reported as
key events with no associated text; omt **never treats a key with
`text: None` and a dead-key code as a binding trigger**, because a user
composing `ô` must not trip a chord.

### 9.3 IME composition

The real problem. A CJK user typing into an agent's prompt goes through an input
method that composes over several keystrokes, and the composition is owned by
the OS/terminal (L1/L2), not by omt.

What actually happens: the terminal delivers the *committed* text, typically as
a burst of UTF-8, sometimes as a bracketed paste. omt's obligations:

- **Never bind on a printable character in `terminal_focused` without a
  modifier.** §5.4's refusal list already forbids it. This is the rule that
  makes IME work at all — a mid-composition commit must not resolve a binding.
- **Treat a multi-byte burst as text, not as keys.** The decoder emits
  `InputEvent::Paste` for any run of printable UTF-8 arriving within
  `input.burst_window` (default 8 ms) that exceeds 3 characters, and text is
  never matched against the keymap.
- **Do not swallow the composition indicator.** Some terminals draw the
  composition preview themselves, over the pane; omt's damage tracking must not
  repaint over it. `omt-term` treats the region as damaged on the next frame
  after focus changes, which is sufficient in practice and is called out here so
  it is tested (§10.4) rather than discovered.
- **`Ctrl+Space` is an IME toggle** on many Linux setups, which is the second
  reason it is not the default leader (§3.2).
- **The web client has a real IME**, with `compositionstart`/`compositionend`
  events. [08 §8.3](08-web-client.md) already handles xterm.js's hidden
  textarea; the keymap rule is the same — no binding fires between
  `compositionstart` and `compositionend`.

### 9.4 Key repeat

Under the kitty protocol, repeats arrive as `event-type: 2` and are
distinguishable. omt's rule: **a binding fires on `Press`, never on `Repeat`**,
except for bindings explicitly marked `repeatable = true` (pane resize, copy-mode
motion, explorer navigation). Holding `<leader> |` must not create forty splits.
Under legacy encoding repeats are indistinguishable from presses, so omt applies
a `repeat_guard` (default 50 ms) to non-repeatable bindings and documents that
this is a heuristic on terminals without the protocol.

Accessibility beyond repeat: every chord in §8 is reachable through the palette
by name, which is the sticky-keys-and-screen-reader path, and the palette is
navigable with `↑`/`↓`/`Enter` alone. There is no omt capability that requires a
chord, a modifier, or a mouse.

---

## 10. Testing

### 10.1 Decoder conformance corpus

A checked-in corpus of `(terminal, platform, chord, encoding, bytes)` rows, one
per cell of §1.3 and §1.4, stored as TOML fixtures:

```toml
[[case]]
terminal = "ghostty"; version = "1.2.0"; platform = "macos"
chord = "ctrl-shift-p"; encoding = "kitty"
bytes = "[112;6u"
expect = { code = "Char(p)", mods = ["CTRL","SHIFT"], kind = "Press" }

[[case]]
terminal = "Apple_Terminal"; platform = "macos"
chord = "ctrl-shift-p"; encoding = "legacy"
bytes = ""
expect = { code = "Char(p)", mods = ["CTRL"], kind = "Press" }
expect_ambiguous_with = ["ctrl-p"]
```

- The corpus is **generated by a capture harness**, not hand-written:
  `omt keys capture` runs in a terminal, prompts for each chord, records the
  bytes, and emits the fixture. Contributors run it in their terminal and send a
  file. That is how §1.3's "assumed" cells become "verified".
- The decoder test is a pure function over the corpus, no terminal needed in CI.
- A round-trip test asserts `passthrough(bytes) == bytes` for **every** case,
  including the ones omt cannot decode — the §2.1 losslessness invariant.
- A fuzz target ([P5](01-principles.md#p5--production-grade-from-the-first-commit))
  feeds random bytes to the decoder and asserts it never panics, never
  allocates unboundedly, and always eventually resynchronizes.

### 10.2 Property tests for resolution

For arbitrary keymaps and arbitrary context sets:

1. **Passthrough default.** With the builtin keymap only, the set of consumed
   `KeyEvent`s in `terminal_focused` equals §8.1's list, cardinality ≤ 6.
2. **Determinism.** `resolve` is a pure function of
   `(KeyEvent, ContextSet, PendingChord, keymap)` — same inputs, same output,
   always.
3. **Totality.** Every `(KeyEvent, ContextSet)` resolves to exactly one of
   `Dispatch | Pending | Passthrough`. No panic, no "unhandled".
4. **Chord termination.** From any pending state, `Esc` reaches `Passthrough` in
   ≤ 1 step and the pending state is cleared.
5. **Prefix soundness.** If `c` is a prefix of `d` and both are bound, resolving
   `c` yields `Pending` and the config produced an `OMT-C402`/`OMT-C412`.
6. **Layer monotonicity.** Adding a binding at a higher layer never changes the
   resolution of a chord it does not mention.
7. **Leader relocation.** For any leader `L`, recompiling the default map with
   `leader = L` yields a map isomorphic to the default under the substitution —
   no binding is lost or duplicated.

### 10.3 Conflict-detection matrix

Table-driven, one row per `(binding, registry entry, terminal profile)` triple,
asserting the exact diagnostic code, severity, span and suggestion. It covers:

- Every code `OMT-C401`–`OMT-C412` and `OMT-C420`–`OMT-C425`, with a passing and
  a failing fixture.
- Every entry on the refusal list, unscoped (error) and scoped (clean).
- Every `critical` key of `claude-code.toml` and `zsh-zle.toml`.
- A terminal profile with no kitty protocol, asserting that
  `ctrl-shift-p`/`ctrl-shift-v`/`shift-enter` are *not installed* and that
  `omt keys list` reports them as unavailable with a reason.
- A registry file whose `verified_against` is older than the detected version,
  asserting the staleness note appears.
- A golden test on the rendered diagnostics, so the §5.2 output above is
  literally the test fixture and cannot drift from the docs.

### 10.4 IME and composition

Genuinely hard to test, so it is tested at three levels rather than one:

1. **Unit** — the burst detector: a synthetic stream of committed UTF-8 within
   the burst window resolves to `InputEvent::Paste`, never to key bindings.
   Fixtures for Japanese (kana→kanji), Korean (jamo composition, which commits
   incrementally), and Chinese (pinyin with a candidate list).
2. **Integration** — a PTY test that writes a committed CJK burst into omt with
   a keymap that binds every printable ASCII character, asserting nothing fires
   and the bytes arrive at the inner PTY unchanged.
3. **Manual, scripted** — a checklist in `docs/testing/ime.md` run per release
   on macOS (Japanese/Chinese IMEs), Linux (fcitx5, ibus) and the web client,
   because the composition *preview* is drawn by software omt does not control
   and cannot be asserted from inside a test.

The web client's composition path is testable in Playwright with
`CompositionEvent` dispatch, and that test asserts no binding fires between
`compositionstart` and `compositionend`.

---

## 11. Capabilities introduced here

| Capability | Kind | Role | Notes |
|---|---|---|---|
| `keys.list` | Query | Viewer | Existing ([10 §10](10-configuration.md#10-config-capabilities)); extended with `deliverable` and `shadows` per binding |
| `keys.conflicts` | Query | Viewer | Existing; now covers `OMT-C401`–`OMT-C412` (§5.2) and `OMT-C420`–`OMT-C425` (§6.8) |
| `keys.explain` | Query | Viewer | §5.3; reports the active keymap and mode (§6.10) |
| `keys.keymaps` | Query | Viewer | List available keymaps and their inheritance (§6.5) |
| `keys.registry` | Query | Viewer | Dump an `InnerKeymap` |
| `keys.probe` | Query | Viewer | The `TerminalProfile` (§5.5) |
| `keys.capture` | Command | Operator | Record a chord's bytes for the corpus (§10.1) |
| `system.doctor` | Query | Viewer | Owned by [22 §10](22-operations.md#10-capabilities); §7.5 is its `keys` group (`DoctorGroup::Keys`), spelled `omt doctor keys` on the CLI. This document introduces no doctor capability of its own |
| `system.doctor.fix` | Command | Admin | Owned by [22 §10](22-operations.md#10-capabilities); §7.4/§7.5's terminal remap is `omt doctor keys --fix`, with `Effects::WRITES_FS` |
| `tui.open_command_palette` | Command | Viewer | §3.5 |
| `tui.open_keymap_help` | Command | Viewer | §8.2 |
| `interaction.focus_latest` | Command | Operator | §4.4 |
| `media.picker.open` | Command | Operator | Owned by [09 §4.3](09-ssh-and-media.md#4-getting-images-in-the-easy-cases); §7.3 binds it. This document introduces no picker capability of its own — the earlier name `media.paste_picker` was a duplicate and is gone |
| `media.blob.abort` | Command | Operator | Owned by [09 §5](09-ssh-and-media.md#5-case-b-image-paste-over-ssh--the-core-mechanism); §7.3 binds it to cancel a transfer. The earlier name `media.transfer.cancel` was a duplicate and is gone |
| `session.send_newline` | Command | Operator | §8.1 |
| `tui.copy_mode.*` | Command | Operator | Motions, selection, yank — the action set the vim and emacs keymaps both target (§6.6, §6.7) |
| `tui.set_keymap` | Command | Operator | Switch keymap at runtime (`Runtime` layer); `omt keys use vim` |

All of them are surfaced in the palette and in the web settings UI by
construction, per [03 §5](03-capability-catalog.md).

---

## 12. What this document deliberately does not do

- **No *new* mouse-driven omt chrome while the inner program has mouse reporting
  on.** §4.3. Stated as a non-goal so it is not "improved" later. The one
  sanctioned exception is `Shift`+click for semantic activation, decided in
  [18 §5.1](18-semantic-open.md#51-mouse-reporting--the-inner-program-owns-the-click)
  and recorded in §6.2's signal table; adding a second exception requires
  re-opening that decision, not a local judgement call.
- **No key-release bindings**, even though the kitty protocol offers them. They
  are unavailable on most terminals, and a keymap whose behaviour differs by
  terminal in a way the user cannot see is exactly what §5.5 exists to prevent.
- **No *ambient* modal mode over a pane.** omt ships vim and emacs keymaps (§6),
  but they govern omt's own surfaces only. A persistent "omt normal mode" layered
  over a pane would either swallow the inner program's keys or require an
  explicit exit, and both violate the passthrough default. The leader is a
  *transient* mode with a 1500 ms life; a modal surface is one you opened.
- **No vim emulation beyond the subset in §6.6, and no macros in either mode.**
  There is a real vim one keystroke away, in a pane.
- **No `:map`-style remapping language inside the modal keymaps.** Remapping is
  `keybindings.toml`, so every binding goes through §5.2's conflict detection.
- **No auto-focus of interaction cards.** §4.4.
- **No re-encoding of passed-through input.** §2.1.

---

## 13. OPEN QUESTIONS

1. **Resolved, see [10 §8.2](10-configuration.md#82-keybinding-format) — context
   names.** 10 no longer enumerates a context vocabulary; it points at §4.1's
   `ContextSet`, which is the authoritative superset. The two renamed names
   (`session_focused` → `terminal_focused`, `interaction_card_focused` →
   `card_focused`) are accepted as deprecated aliases, diagnosed as a note.
2. **Resolved, see [10 §8.2](10-configuration.md#82-keybinding-format) — the
   `leader` key itself.** `keybindings.toml` now has a top-level `leader` key and
   `<leader>` is legal in any trigger position; literal `ctrl-b` remains legal
   and means literally `ctrl-b`.
3. **Resolved, see [10 §8.2](10-configuration.md#82-keybinding-format) —
   `platform` vs. `requires`.** `requires` is now a sibling of `platform` on a
   `[[binding]]`. Still open, and deliberately: whether a *user* binding with an
   unmet `requires` is a warning (current answer, `OMT-C408`) or silently inert.
4. **Resolved, see [10 §8.2](10-configuration.md#82-keybinding-format) —
   `force`.** Per-binding `force = true` exists for §5.4's refusal list, and it
   is legal only in the `[[binding]]` table form — the flat
   `"chord" = "capability"` form cannot carry it, which is the intended friction.
5. **Claude Code's keymap is under-verified.** `Ctrl+O`, `Esc`, `Shift+Tab`,
   `Ctrl+C`, `Ctrl+D` are well-attested;
   [research/agent-clis.md §7](../research/agent-clis.md) already flags the
   command-palette detail as **UNCERTAIN** for Amp, and our `ctrl-e`, `ctrl-r`
   and the `n`-on-card entries need a hands-on pass before shipping. Since a
   wrong registry entry produces only a spurious warning, this is a quality
   problem, not a correctness one — but a spurious warning on a key the user
   legitimately wants is corrosive.
6. **Per-emulator kitty-protocol support levels are assumed, not measured.**
   §1.4's support list, and the whole "assumed" half of §1.3, come from
   documentation. The §10.1 capture harness is designed to fix this, but the
   corpus does not exist yet and the table should not be quoted to users until
   it does.
7. **iTerm2 plist writing.** §7.4's `--fix --terminal iterm2` writes a binary
   plist that iTerm2 caches in memory. Refusing while iTerm2 runs means refusing
   in the only situation where the user is asking. Is there a supported
   alternative (a dynamic profile in `~/Library/Application Support/iTerm2/DynamicProfiles/`
   that can carry key mappings)? **Unverified** — if dynamic profiles support
   `Keyboard Map`, that is a much better mechanism and should replace the plist
   write entirely.
8. **Terminal.app "Send Text" bindings.** §7.4 assumes these exist and can carry
   an escape sequence. Unverified, and Terminal.app's profile plist is nested
   enough that omt should probably never write it.
9. **`leader_miss` default.** §3.4 defaults to `drop`. Some users will find a
   silently-eaten keystroke worse than a stray `Ctrl+B`. Worth a preference
   probe after first contact with real users; the setting exists either way.
10. **Should the leader be per-context?** A user might want `Ctrl+B` in a shell
    pane and something else in a vim pane, resolved via the registry. Rejected
    for v1 as too clever — a prefix whose identity depends on what is running is
    a prefix you cannot trust — but recorded because it is the obvious next idea.
11. **Shadowing detection for the *shell*, not just the agent.** §5.1 ships
    `zsh-zle`/`bash-readline`, but a pane's foreground program changes every
    time a command runs. Do we re-evaluate warnings live (noisy) or only at
    config-load against the *configured* shell (current proposal)? Current
    proposal is quieter and slightly less accurate.
12. **Chord conflicts between omt and a *nested* omt.** Running `omt` inside
    `omt ssh` is plausible (a remote instance you then ssh onward from). The
    inner omt sees `Ctrl+B` first. Today the answer is "change the inner
    instance's leader", which is the same answer tmux gives; whether omt should
    detect the nesting via `OMT_SESSION` in the environment and auto-relocate is
    open.
13. **Whether `session.send_newline` should be a default at all.** It only works
    under the kitty protocol, which means the most-requested ergonomic in §8.1 is
    invisible to Terminal.app users. `alt-enter` is the legacy fallback and is
    installed, but `Alt` on macOS needs the Option remap (§1.3 note ¹), so on a
    default macOS Terminal.app there is **no** working chord for "newline without
    submitting". That is an honest gap with no clean fix inside omt.
14. **Resolved, see [10 §8.2](10-configuration.md#82-keybinding-format) — `modes`
    and `keymap`.** Both exist: `keymap` and `leader` are the two top-level keys
    of `keybindings.toml`, and `modes` is a per-binding field legal only in the
    `[[binding]]` table form. Still open: whether a per-keymap file
    (`keymaps/*.toml`, §6.5) is a fourth config file or a section of the third —
    10 §12 already notes that three files is the practical maximum.
15. **Does the `default` keymap need a modal engine slot at all?** §6.5 makes
    `modal: Option<ModalEngine>` a field, so `default` sets `None`. If a third
    modal style ever appears (kakoune's object-verb order is the obvious
    candidate, and it is genuinely different — selection first, then operator),
    `ModalEngine` gains a variant and `VimState`'s operator-pending machinery
    does not fit it. Whether `ModalEngine` should be a trait rather than an enum
    is a real design question that shipping vim first will answer.
16. **`Esc` timeout defaults are asserted, not measured.** §6.3 picks 15 ms for
    modal surfaces and 25 ms for panes by analogy with tmux's `escape-time 10`.
    Nobody has measured the mis-decode rate at those values on a slow link, over
    a serial console, or under a loaded tmux. The values are configurable and the
    diagnostic (`OMT-C425`) exists, but they should be validated before the vim
    keymap ships, because a mis-decoded arrow key in normal mode executes a
    command.
17. **Which vi keymap do users actually mean?** tmux's `copy-mode-vi`, vim
    proper, and neovim differ in small ways (`H`/`M`/`L` semantics in a scroll
    buffer, whether `y` exits copy mode). §6.6 follows vim proper and exits copy
    mode on `y`, matching tmux's default. **Unverified against user expectation**
    — worth a preference probe, and `omt keys list --keymap vim --diff default`
    exists partly so this is arguable from a concrete artifact.
18. **Emacs `C-w` in the browser.** §6.9 restricts kill-region to focused inputs
    because `Ctrl+W` closes a tab on some platform/browser combinations. Whether
    `preventDefault` reliably suppresses it in a focused `<textarea>` across
    Chrome, Firefox and Safari is **unverified**, and the answer differs per
    platform. If it does not, emacs mode on the web needs a documented
    substitute rather than a binding that sometimes closes the user's session.
19. **Registers vs. the system clipboard over ssh.** §6.6 maps `"+` onto
    `media.clipboard.write`, which over `omt ssh` reaches the *local* clipboard
    ([09 §3](09-ssh-and-media.md)). But `"+p` — pasting *from* the local
    clipboard into an omt input field — goes through the same tiering as image
    paste (§7.2) and can therefore fail or be slow, which a register paste is not
    expected to be. Current proposal: `"+p` reads through `media.clipboard.read`
    and shows the §7.3 progress line if it takes longer than 150 ms. Untested.
20. **Vim mode on interaction cards.** §6.1 restricts cards to motions. A user
    who is deep in vim muscle memory may try `dd` on a card and get nothing.
    Silence versus a status line versus a "not applicable here" hint is a UX
    question with no obviously right answer; currently a status line.
