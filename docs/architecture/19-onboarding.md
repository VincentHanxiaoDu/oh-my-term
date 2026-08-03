# Onboarding, Discoverability, Migration and TERM

This document owns two questions the rest of the architecture leaves open:

> **What happens the first time someone runs `omt`?**
> **How does a tmux user of ten years stop resenting it?**

It is the owner for gap [G1](../design/scenarios.md#g1--first-run-experience-onboarding-and-in-product-help--no-owner)
(first-run experience, onboarding, in-product help), gap
[G6](../design/scenarios.md#g6--tmuxzellij-migration-and-coexistence--no-owner)
(tmux/zellij migration and coexistence), and requirements
[R64](../design/scenarios.md#configuration-onboarding-operability),
[R10](../design/scenarios.md#terminal-and-multiplexer-fundamentals),
[R6](../design/scenarios.md#terminal-and-multiplexer-fundamentals) (the `TERM`
and terminfo story, which was unowned and is claimed here in §7).

Related: [01 — Principles](01-principles.md) ·
[03 — Capability catalog](03-capability-catalog.md) (everything user-facing in
§3 is generated from it) · [04 — Terminal core](04-terminal-core.md) (shell
integration, escape-sequence surface) · [06 — Agent layer §7.1](06-agent-layer.md#71-integration-installer)
(the hook installer this document wraps in a UI) ·
[09 — SSH and media](09-ssh-and-media.md) (OSC under tmux) ·
[10 — Configuration](10-configuration.md) (what `omt setup` writes) ·
[16 — Input and keymap](16-input-and-keymap.md) (leader, palette, probing,
keymaps-as-data) · [17 — Panes and layout](17-panes-and-layout.md) ·
[research/multiplexers.md](../research/multiplexers.md).

The document reduces to six commitments:

> 1. **`omt` with no config starts a shell and gets out of the way.** No wizard,
>    no splash, no modal. One status line and at most **one** unsolicited hint,
>    ever, per install.
> 2. **Everything is discoverable from inside the product** — status bar →
>    which-key strip → `⟨leader⟩ ?` → palette — and every one of those surfaces
>    is *generated from the capability catalog*, so it cannot rot.
> 3. **`omt setup` never writes to a file the user has not seen a diff of.**
> 4. **A tmux user can be at parity in one config line** (`keymap = "tmux"`) and
>    can import their `.tmux.conf` with a report that names every dropped
>    directive and why.
> 5. **tmux inside omt is supported as a terminal and unsupported as an
>    observation surface**, said out loud, in the UI, once.
> 6. **omt advertises a `TERM` it actually implements** and degrades to
>    `xterm-256color` rather than lying (§7).

---

## 1. First run, second run, tenth run

### 1.1 The principle being optimized for

**Time-to-first-normal-command must be zero, and time-to-first-omt-feature must
be under thirty seconds — for the user who wants it, and never for the user who
does not.**

The two failure modes are symmetric and both fatal:

| Failure | Who it kills | The mechanism that prevents it |
|---|---|---|
| A wall of tutorial | Kenji ([P2](../design/scenarios.md#p2--kenji-the-terminal-purist)) uninstalls in 60 s | No modal, no wizard, no full-screen anything on first run. The entire first-run surface is **two lines of text**, one of which is permanent chrome. |
| A bare screen | Sam ([P5](../design/scenarios.md#p5--sam-the-beginner)) never learns a single feature and concludes omt is a slower terminal | The status bar always shows the help chord; the leader always shows the which-key strip; the palette always contains everything. |

The resolution is **progressive disclosure driven by the user's own keystrokes**:
omt volunteers almost nothing, but every gesture a curious user makes returns
more than it cost.

### 1.2 The literal first screen

`brew install omt && omt`, in a repo, on a machine with Claude Code installed
and no omt config:

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                                                                              │
│  omt is not set up yet — shell integration and agent hooks are missing.      │
│  Run `omt setup` (2 min, shows a diff before writing anything), or press      │
│  ⌃b i now.  Dismiss with ⌃b Esc — you will not be asked again.               │
│                                                                              │
│ ~/code/app on  main $ ▊                                                      │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
├──────────────────────────────────────────────────────────────────────────────┤
│ omt  1:zsh                        ~/code/app   main*   ⌃b ? help             │
└──────────────────────────────────────────────────────────────────────────────┘
```

Facts about that screen, each of which is a decision:

- **The shell is already running and already has focus.** The hint is printed
  *into the scrollback above the first prompt*, as ordinary text — not an
  overlay, not a floating window. It scrolls away the moment the user runs
  anything. This is the cheapest possible hint: it cannot intercept a key, it
  cannot break `ls`, and it disappears by itself.
- **It names the cost ("2 min"), the safety property ("shows a diff"), and the
  exit ("you will not be asked again").** A hint that does not tell you how to
  make it stop is a hint that gets the product uninstalled.
- **The status bar is the only permanent chrome**, one row, and its rightmost
  cell is the help chord. §2.1 defends every cell.
- **Nothing is written to disk on first run** except `~/.local/state/omt/` (the
  instance socket, the store) and, on dismissal, a single
  `onboarding.hints_dismissed` entry. The user's `~/.config/omt/` does not exist
  until they ask for it. `omt` uninstalled at this point leaves one state
  directory, and `omt uninstall` removes it (§8).

**The at-most-one-hint rule.** omt shows **at most one unsolicited hint per
install**, and the hint above is it. Not one per launch, not one per feature,
not one per session. Once dismissed or once `omt setup` has been run, the
onboarding hint channel is closed permanently and the state key
`onboarding.hints_dismissed = true` is written to `~/.config/omt/state.toml`.
There is no second hint about the phone client, no "tip of the day", no "did you
know". Everything else omt wants to tell the user is available *when asked* and
is silent otherwise. This rule is worth more than any individual thing omt might
have said.

**What does not count as a hint** and is therefore still allowed: diagnostics
about a thing the user just did (a config error, a failed paste with a reason
per [D7](decisions.md#d7--image-paste-over-ssh-is-only-promised-where-omt-controls-both-ends)),
the once-per-pane nested-multiplexer badge (§6.4), and state that lives in the
status bar. Those are *responses*, not solicitations.

### 1.3 The 30-second path to first value

Three paths exist, for three different users, and all three are reachable from
the first screen:

| User | Path | Elapsed |
|---|---|---|
| Sam (curious) | presses `⌃b`, pauses, sees the which-key strip, presses `|` → a split | ~8 s |
| Mara (came for agents) | types `claude` in the pane; omt detects it, the status bar grows an agent chip, and `⟨leader⟩ A` opens the dashboard | ~15 s |
| Kenji (came from tmux) | `⌃b c`, `⌃b |`, `⌃b z`, `⌃b [` all do what tmux does; nothing has to be learned | ~5 s |

Kenji's row is the reason the default leader is `Ctrl+B` and the reason the
leader namespace mirrors tmux's mnemonics where it can
([16 §3.2](16-input-and-keymap.md#32-default-leader-ctrlb),
[16 §8.2](16-input-and-keymap.md#82-the-leader-namespace)). It is the single
highest-leverage onboarding decision in the product, and it was made in another
document; this one only has to not squander it.

### 1.4 Second run

Identical to the first, minus the hint, plus whatever the user configured. There
is no "welcome back". If `omt setup` was run, the status bar's rightmost cell is
still `⌃b ? help` — that never goes away, because it costs eight columns and it
is the entire escape hatch.

### 1.5 Tenth run, and the competence ramp

The screen does not change with usage, and this is deliberate: an interface that
mutates as it decides you are experienced is an interface you cannot form habits
in. What changes instead is what the user *reaches for*, and omt makes that ramp
visible rather than automatic:

| Stage | Surface used | omt's contribution |
|---|---|---|
| 1 | status bar | the help chord is always in view |
| 2 | which-key strip after `⌃b` | 250 ms delay, so it never interrupts muscle memory ([16 §3.3](16-input-and-keymap.md#33-pending-leader-ui)) |
| 3 | palette (`⌃b p`) | search in plain words; **every entry shows its chord**, which is how a palette teaches the keyboard |
| 4 | direct chords | learned from stage 3's right-hand column |
| 5 | `omt keys`, config, keymaps, plugins | reached deliberately, never advertised |

The palette showing the chord next to every entry is the whole ramp mechanism.
It is the same trick VS Code and Emacs's `M-x` use, and it is why omt needs no
tutorial: the discovery surface *is* the teaching surface.

The one automatic behaviour: after a capability has been invoked **from the
palette five times** and has a bound chord the user has never pressed, the
palette row for it renders its chord in the accent colour for the next three
appearances. No toast, no modal, no persistence beyond a counter. If the user
does not notice, nothing was lost. (`onboarding.chord_nudges = false` disables
it; it is the only adaptive behaviour in the product and it is listed in §10 as
a question, not a certainty.)

---

## 2. Discoverability without documentation

### 2.1 The status bar contract

**One row. Always present unless `appearance.status_bar = "off"`. Never more
than one row, in any configuration.**

```
 omt  1:zsh  2:claude ●  3:build ✗      ~/code/app   main*   ⌃b ? help
 └┬┘  └──────────┬───────────┘          └────┬────┘  └─┬──┘  └───┬───┘
  A              B                            C         D        E

 omt  1:zsh  2:claude ● ⟨native⟩  3:build ✗   ~/code/app  main*  ⌃b ? help
                        └───┬───┘
                            M
```

Every cell must justify its columns; here is the justification, and a cell that
cannot pass this test does not ship:

| Cell | Content | Why it earns the space |
|---|---|---|
| **A** | `omt`, or the instance name when not the default, or `omt→box` in a thin client ([09](09-ssh-and-media.md)) | **"Which program am I in, and is it local?"** A user who forgets they are inside omt will type `⌃b` into a bare shell and be confused; a user who forgets they are *remote* will `rm` the wrong file. Six columns to prevent both. |
| **B** | Session list: index, name, agent-state glyph | The only always-visible answer to "what else is running and does any of it need me". Glyphs are the agent state machine's, coloured from the theme's agent-state slots ([10 §8.1](10-configuration.md#81-theme-format)): `●` working, `◍` blocked/needs you, no glyph = idle, `✗` last command failed. **This cell is the fastest possible answer to "which agent needs me" — one glance, no navigation, ten characters.** |
| **M** | `⟨native⟩` on a session running in `native` mode ([D8](decisions.md#d8--two-session-modes-pty-default-and-native-acp), [05 §1.3](05-session-model.md#13-session-modes-d8)) | D8 requires `SessionMode` to be visible on **every** surface, and this is the TUI's. It is absent for `pty` sessions — the default is unlabelled, the opt-in is labelled, because the thing the user must never be confused about is *which product they are talking to*. It is state, not a hint, and it does not participate in the overflow drop order below: if B is summarised, the summary still carries the mode. |
| **C** | cwd of the focused pane, `~`-abbreviated, middle-elided | Comes free from OSC 7 ([04 §5.4](04-terminal-core.md#54-osc)). Answers "which worktree is this pane" — the question [J19](../design/scenarios.md#j19--running-several-agents-across-worktrees) is entirely about. |
| **D** | VCS branch + dirty marker, from the shell integration's `omt_git` user var | Same question as C, one level finer. **Degrades to absent** when shell integration is not installed — which is itself a legible signal, and is the honest one. |
| **E** | `⌃b ? help`, showing the *resolved* leader | The escape hatch. It is last (rightmost, least scanned) and never blinks. If a user learns exactly one thing about omt, this is the thing. |

Rules:

- **The bar is chrome, not a pane.** It is drawn in a reserved row and the PTY
  is sized to exclude it; toggling it emits exactly one `SIGWINCH`, and toggling
  it is not bound by default because resizing an agent mid-render is rude
  ([16 §3.3](16-input-and-keymap.md#33-pending-leader-ui) makes the same
  argument for the which-key strip).
- **Overflow drops from the left of B**, never truncates E. On an 80-column
  terminal the bar degrades to `omt 3s ●  ~/code/app  ⌃b ?`; at 40 columns
  (a phone in landscape terminal, or a split) to `omt 3● ⌃b?`. The degradation
  order is a fixed priority list — E, A, B-summary, C, D — and is tested at
  every width from 20 to 200.
- **`NO_COLOR` and `--no-chrome`** are honoured: `--no-chrome` removes the bar
  entirely, for screen-reader users and for `asciinema`-style recordings
  ([G10](../design/scenarios.md#g10--accessibility-of-the-tui--no-owner)).
  Everything the bar shows is composed from capabilities that already exist —
  `instance.info` ([22 §10](22-operations.md#10-capabilities)) for cell A,
  `session.list` ([05 §10.2](05-session-model.md#102-session)) for cells B, C, D
  and the `mode` indicator M, and `agent.state`
  ([06 §4](06-agent-layer.md#4-merging-confidence-tiers-not-voting)) for B's
  glyphs — so removing the bar removes no information, only its ambient
  presentation, and §2.1 is parity-checked without a new capability. **No
  `instance.status` capability is declared**: a composed view whose only consumer
  is one row of chrome is a catalog entry that exists to be a screenshot.
- **No spinners, no clocks, no CPU meters, no ASCII art.** The bar redraws only
  when its content changes, because a bar that redraws on a timer keeps a laptop
  awake and shows up in every `strace` a user runs while debugging something
  else.

### 2.2 `⟨leader⟩ ?` — the help overlay

`⟨leader⟩ ?` (`tui.open_keymap_help`, [16 §8.2](16-input-and-keymap.md#82-the-leader-namespace))
opens a scrollable, searchable overlay — `Exclusive` capture
([16 §4.2](16-input-and-keymap.md#42-focus-and-exclusivity)), so nothing leaks
to the pane — with **three panes of content, generated, never hand-written**:

```
┌─ omt help ────────────────────────────────────────────── / to filter ── Esc ─┐
│                                                                              │
│  YOUR KEYS                          THIS PANE IS RUNNING                     │
│  ⌃b p   command palette             Claude Code 2.4.0  (hooks + transcript)  │
│  ⌃b c   new session                 ⌃o  its command palette                  │
│  ⌃b |   split vertically            Esc its interrupt / back                 │
│  ⌃b -   split horizontally          ⇧⇥  its permission-mode cycle            │
│  ⌃b z   zoom pane                   omt passes all three through untouched.  │
│  ⌃b [   copy mode / scrollback                                               │
│  ⌃b a   focus the latest question   NOT AVAILABLE HERE                       │
│  ⌃b v   paste image / file          ⌃⇧p  palette — needs the kitty keyboard  │
│  ⌃b A   agent dashboard                   protocol; Terminal.app has none.   │
│  ⌃b ,   settings                          `⌃b p` does the same thing.        │
│  ⌃b ⌃b  send ⌃b to the program                                               │
│  … 14 more — press / or ⌃b p for all 163 capabilities                        │
│                                                                              │
│  omt 0.4.1 · keymap: default · leader: ⌃b · TERM=omt-256color · docs: ⌃b p → │
└──────────────────────────────────────────────────────────────────────────────┘
```

Three columns, three different jobs, and the middle one is the novel one:

1. **YOUR KEYS** — `keys.list`, resolved, with provenance, filtered to bindings
   whose `requires` is met on this terminal
   ([16 §5.5](16-input-and-keymap.md#55-terminal-capability-probing)).
2. **THIS PANE IS RUNNING** — the *inner program's* keymap from the registry
   ([16 §5.1](16-input-and-keymap.md#51-the-inner-program-keymap-registry)),
   with an explicit statement that omt passes those keys through. This directly
   answers the anxiety a multiplexer produces ("what is it stealing?") and it
   costs nothing, because the registry already exists for conflict detection.
3. **NOT AVAILABLE HERE** — bindings that exist but are undeliverable on this
   terminal, **with the reason and the working alternative**. A help screen that
   lists a chord the user cannot press is worse than one that omits it; a help
   screen that omits it silently teaches the user that omt's docs lie. Naming it
   and explaining it is the only honest option.

The footer states version, active keymap, resolved leader and advertised `TERM`
— the four facts every support conversation starts by asking for.

### 2.3 Which-key: pending-chord hints

Specified in [16 §3.3](16-input-and-keymap.md#33-pending-leader-ui) and
[16 §6.10](16-input-and-keymap.md#610-discoverability); this document adds only
the onboarding-specific requirements, because the strip is where most users
learn most of what they learn:

- **Grouped, not alphabetical.** `panes | sessions | agent | clipboard | omt`,
  in that order, with the group labels dimmed. [J2](../design/scenarios.md#j2--discovering-a-feature-without-reading-docs)'s
  failure mode is "60 bindings unfiltered"; grouping is what makes 24 legible.
- **Ranked by frequency within a group**, using a local counter, so the strip
  becomes *the user's* strip. No network, no sync, no telemetry
  ([P: no telemetry](../design/scenarios.md#part-5--anti-requirements)).
- **`?` always expands** to §2.2's overlay, filtered to the pending prefix.
- **Suppressible** (`input.which_key = false`) and delay-tunable
  (`input.which_key_delay`, default 250 ms). Kenji sets it to `false` in his
  first minute and never sees it again; that is a supported outcome, not a
  failure.

### 2.4 The command palette as the universal escape hatch

The palette is specified in [16 §3.5](16-input-and-keymap.md#35-the-palette).
What this document owns is the guarantee that **every capability is findable by
someone who does not know its name** — and the mechanism that keeps that true
as the catalog grows.

**Palette entries are derived from the catalog declaration
([03 §2](03-capability-catalog.md#2-declaring-a-capability)), never written by
hand.** Three fields are added to `capability!` for this purpose:

```rust
capability! {
    /// Split the focused pane, creating a new session in the same directory.
    name  = "pane.split",
    group = "pane",
    verb  = "split",
    kind  = Command,
    role  = Role::Operator,
    input  = PaneSplit { pane: PaneId, dir: Direction2D, cwd: Option<PathBuf> },
    output = SplitAck { pane: PaneId },
    effects = [Effects::SPAWNS_PROCESS],

    // ── added by this document ────────────────────────────────────────────
    /// Imperative, ≤ 40 chars, Title-less. THE palette row title.
    title   = "Split pane",
    /// Words a user might search that are not in the title or the doc comment.
    aliases = ["divide", "new pane", "tmux %", "tmux \"", "vsplit", "hsplit"],
    /// Hidden from the palette but still in the API/CLI. Requires a reason.
    hidden  = false,
}
```

- **`title` is mandatory for every non-`hidden` capability**, and CI fails on a
  missing one, on one longer than 40 characters, and on one that merely restates
  `group.verb` (a lint: the title may not equal the verb with punctuation
  removed). This is the fix for [J3](../design/scenarios.md#j3--command-palette-as-the-discovery-path)'s
  failure mode — a palette full of `session.__replay`.
- **The description is the capability's doc comment**, which already exists and
  is already the source for the generated reference docs
  ([03 §5](03-capability-catalog.md#5-the-parity-contract) artifact #4). One
  string, four consumers, no drift.
- **`aliases` carry the vocabulary of the tools users are coming from.** The
  tmux and zellij command names in §5.1's table are injected into `aliases`
  automatically by a build step that reads that table, so typing `new-window`
  into omt's palette finds `session.create` and typing `%` finds `pane.split`.
  **This is the single cheapest migration feature in the document.**
- **`hidden = true`** requires a `hidden_reason` and appears in the generated
  docs, exactly like `Parity::Exempt`. Internal/debug capabilities use it;
  nothing user-facing may.
- **Enum-valued arguments enumerate into rows.** `pane.split` with
  `Direction2D::{Left, Right, Up, Down}` produces one row per direction, not one
  row and a form, per [16 §3.5](16-input-and-keymap.md#35-the-palette).

**Ranking**, in order: exact title match → prefix of title → alias match →
subsequence over title → subsequence over description → recency of the user's own
last 200 invocations as a tiebreak. Non-capability targets (sessions,
workspaces, files, workflows) are interleaved by score with a type glyph.

**The rot-proofing test.** A CI test enumerates the registry and asserts, for
every non-`hidden` capability: a `title` exists and passes the lints; a doc
comment exists and is ≥ 20 characters; the entry is reachable in the palette's
own ranker by searching its `title` (guarding against a scorer regression that
makes an entry unfindable). The palette is a parity artifact, in the same sense
as [03 §5](03-capability-catalog.md#5-the-parity-contract)'s four.

---

## 3. `omt setup`

An interactive, resumable, fully previewed first-run flow. Also available as
`⟨leader⟩ i` from the first-run hint, as `setup.*` capabilities (so the web
client can run it against a remote instance), and non-interactively as
`omt setup --non-interactive --accept shell,hooks` for provisioning
([R70](../design/scenarios.md#automation)).

### 3.1 The invariant

> **`omt setup` never writes to, installs into, or modifies anything outside
> `~/.config/omt/` and `~/.local/state/omt/` without displaying the exact diff
> and receiving an explicit `y`.** Not "press Enter to continue" — a typed `y`,
> defaulting to *no*, per step, with a per-step "show the full file" option.

This mirrors [06 §7.1](06-agent-layer.md#71-integration-installer)'s rules
(merge never overwrite, preserve formatting, stamp the version, always
reversible, always previewable) and extends them from agent hooks to every
outward-facing write.

Never done at all, with or without asking:

- No network request of any kind, ever (`--offline` is not a flag because there
  is no online mode; [G16](../design/scenarios.md#g16--offline--air-gapped-operation--mostly-fine-needs-a-statement)).
- No installation of an agent CLI, no `npm`/`brew`/`pip` invocation.
- No change to the user's `$SHELL`, login shell, or terminal emulator's
  configuration (§3.5 is the sole, explicitly-confirmed exception, and it writes
  only when the user asks for a specific chord).
- No modification of an existing hook belonging to another tool. omt merges
  alongside; a conflicting entry is reported, not resolved.
- No `sudo`, ever. If something needs root, omt prints the command for the user
  to run.

### 3.2 The flow

```
$ omt setup

omt 0.4.1 setup — nothing is written until you say so.

┌ 1/6  Shell integration ──────────────────────────────────────────────────────┐
│ Detected: zsh 5.9 (login shell), ~/.zshrc (312 lines, writable)              │
│ Also present: bash 5.2, fish 3.7 — not configured (add with `omt setup       │
│ --shells bash,fish`)                                                          │
│                                                                              │
│ Enables: command blocks, exit codes, per-block cwd and branch, `file:line`   │
│ opening, and the status bar's branch cell. Without it omt falls back to a    │
│ heuristic segmenter (04 §6.4) and those degrade.                             │
│                                                                              │
│ Diff to ~/.zshrc:                                                            │
│   313 + # >>> omt shell integration >>>                                      │
│   314 + [ -f "$HOME/.config/omt/shell/omt.zsh" ] && \                        │
│   315 +   source "$HOME/.config/omt/shell/omt.zsh"   # omt-integration v3    │
│   316 + # <<< omt shell integration <<<                                      │
│ New file: ~/.config/omt/shell/omt.zsh (2.1 KB) — [f] to view in full         │
│                                                                              │
│ Install? [y/N/f/s(kip all)]                                                  │
└──────────────────────────────────────────────────────────────────────────────┘
```

The six steps, in this order, because each is more invasive than the last:

| # | Step | Writes | Default |
|---|---|---|---|
| 1 | Shell integration | `~/.zshrc` + `~/.config/omt/shell/` | ask |
| 2 | Agent hook integrations | `~/.claude/settings.json` etc. | ask, per agent |
| 3 | Terminal capability probe | nothing (a report) | run automatically |
| 4 | Theme and keymap | `~/.config/omt/config.toml` | ask |
| 5 | Terminal-emulator key remap | the emulator's config | **off unless asked** |
| 6 | Remote / Tailscale | `~/.config/omt/config.toml`, `secrets.toml` | **off unless asked** |

### 3.3 Step 2 — detecting agent CLIs and installing hooks

Detection is `PATH` lookup plus version invocation plus config-directory
existence — the same manifest data
[06 §7.3](06-agent-layer.md#73-coverage-matrix-initial) uses for adapters, read
as data, not a hard-coded list:

```
┌ 2/6  Agent integrations ─────────────────────────────────────────────────────┐
│ Found on this machine:                                                       │
│   claude    Claude Code 2.4.0    ~/.claude/settings.json   (4 hooks already) │
│   codex     Codex CLI 0.9.1      ~/.codex/config.toml                        │
│   aider     Aider 0.62           — no hook mechanism (heuristic tier only)   │
│ Not found: opencode, gemini, goose, qwen, cursor-agent, amp                  │
│                                                                              │
│ Installing omt's hooks gives, per 06 §3: precise turn boundaries, tool-call  │
│ visibility, and — for Claude Code — permission questions that can be parked  │
│ long enough to answer from your phone. Without them omt still works at the   │
│ heuristic floor (busy / idle / needs you) and says so on every session card. │
│                                                                              │
│ ! Deferred (parked) questions depend on Claude Code's `permissionDecision:   │
│   "defer"` holding a tool call open for a phone round-trip. That is not yet  │
│   verified (06 §5.3, D9 consequence 1 — it is the highest-priority risk in   │
│   the product). If it does not hold, omt still shows the question everywhere │
│   and you answer it at the terminal; and `--native` (below) is the designed  │
│   fallback, because ACP's permission request is blocking and has no timeout. │
│                                                                              │
│ Session modes (D8). `pty` is the default for every agent here and is what    │
│ the rest of this screen configures. These also speak ACP and can be run as   │
│ `omt <agent> --native`, where omt renders the whole session and the agent    │
│ has no TUI:                                                                  │
│   opencode · gemini · cursor-agent · goose · qwen  — first-class ACP modes   │
│   claude, codex  — via their official ACP adapters                           │
│ ! The Claude ACP adapter wraps the **Agent SDK, not the Claude Code CLI**.   │
│   A `native` Claude session is therefore NOT running Claude Code: no slash   │
│   commands, no `/voice`, no on-disk Claude sessions, no permission UX you    │
│   recognise. That is a deliberate, informed choice and never a default; omt  │
│   labels every native session `⟨native⟩` on every surface (D8, 05 §1.3).     │
│                                                                              │
│ ── claude ─────────────────────────────────────────────────────────────────  │
│ ~/.claude/settings.json — MERGE, existing content preserved, comments kept:  │
│                                                                              │
│    {                                                                          │
│      "hooks": {                                                               │
│        "PreToolUse": [                                                        │
│          { "matcher": "*", "hooks": [ { "type": "command",                    │
│            "command": "another tool-hook pre" } ] },                                 │
│  +       { "matcher": "*", "hooks": [ { "type": "command",                    │
│  +         "command": "omt hook pre", "timeout": 5 } ] }                      │
│        ],                                                                     │
│  +     "Stop":        [ { "hooks": [ { "type": "command",                     │
│  +                        "command": "omt hook stop", "timeout": 5 } ] } ],   │
│  +     "SessionStart":[ { "hooks": [ { "type": "command",                     │
│  +                        "command": "omt hook session-start" } ] } ]         │
│      },                                                                       │
│  +   "omt-integration-version": 3                                             │
│    }                                                                          │
│                                                                              │
│ ! another tool is already hooked on PreToolUse. Both will run; omt does not remove  │
│   or reorder another tool's hooks. If Claude Code feels slow, that is where  │
│   to look.                                                                    │
│ ! Every omt hook exits 0 immediately when $OMT_SOCK is unset, so these are   │
│   inert when you run claude outside omt. (06 §7.1)                            │
│                                                                              │
│ Install for claude? [y/N/f/d(iff full file)]                                 │
└──────────────────────────────────────────────────────────────────────────────┘
```

The `!` lines are the point of this screen. Writing into a file another tool
already owns, without saying so, is how a terminal multiplexer earns a
reputation for breaking people's setups.

### 3.4 Step 3 — terminal capability probe

Runs [16 §5.5](16-input-and-keymap.md#55-terminal-capability-probing)'s probe
(150 ms budget) and renders `omt doctor keys` inline, then states the
consequences in terms of *chords*, not of protocols:

```
┌ 3/6  Your terminal ──────────────────────────────────────────────────────────┐
│ Ghostty 1.2.0 (XTVERSION)   TERM=xterm-ghostty   kitty keyboard: yes 0b1111  │
│                                                                              │
│ Because your terminal supports the kitty keyboard protocol, omt enables:     │
│   ⌃⇧p  command palette      ⌃⇧v  paste image     ⇧⏎  newline in agent prompt │
│ On Terminal.app none of those are possible; `⌃b p`, `⌃b v`, `⌥⏎` are the     │
│ portable equivalents and are always bound.                                   │
│                                                                              │
│ Nothing to install. Re-run any time with `omt doctor keys`.        [Enter]    │
└──────────────────────────────────────────────────────────────────────────────┘
```

### 3.5 Steps 4–6

**Step 4 — theme and keymap.** Three questions, each one keystroke, each with a
live preview drawn into the step's box:

- *Theme*: `auto` (follow the terminal's background via OSC 11 —
  [04 §5.4](04-terminal-core.md#54-osc)), `dark`, `light`, or import
  (`omt theme import` accepts the formats [10 §8.1](10-configuration.md#81-theme-format)
  lists). Default `auto`, because it is the only answer that is right on a
  machine the user reconfigures later.
- *Keymap*: `default`, `tmux` (§5.2), `vim`, `emacs`
  ([16 §6](16-input-and-keymap.md)). **If `~/.tmux.conf` or
  `~/.config/zellij/` exists, this step leads with the importer** (§5.3) rather
  than the list. Detecting the file the user already has is worth more than any
  amount of explanation.
- *Leader*: `⌃b` (default), `⌃a`, `⌃space`, or a recorded chord validated live by
  `keys.explain` ([16 §5.3](16-input-and-keymap.md#53-omt-keys-explain-chord)).

**Step 5 — terminal-emulator key remap.** *Skipped unless the user asks.* This
is the only step that writes to software omt does not own, and it exists solely
for [16 §7.4](16-input-and-keymap.md#74-making-cmdshiftv-work-honestly)'s
`Cmd+Shift+V` case. It prints the snippet, offers to apply it, backs up the
original with a timestamp, and — per
[16 §13 Q7/Q8](16-input-and-keymap.md#13-open-questions) — **refuses to write
Terminal.app's plist at all** and refuses to write iTerm2's while iTerm2 is
running, printing manual instructions instead.

**Step 6 — remote / Tailscale.** *Skipped unless the user asks*, because it is
the only step with a security boundary. When asked, it detects `tailscaled`, the
tailnet name and the machine's tailnet IP; explains that omt binds loopback by
default and that public bind is refused without an auth backend
([13 §2](13-security.md)); offers to bind to the tailnet interface with tailnet
identity as the auth backend; and prints the phone-pairing QR at the end.
Credentials go to `secrets.toml` at mode 0600
([10 §1.3](10-configuration.md#13-secrets-are-a-separate-file-with-enforced-permissions)).
Nothing here is ever a default.

### 3.6 Resumability and the summary

`omt setup` is resumable (`omt setup --resume`) and idempotent: re-running it
detects every already-installed artifact by its `omt-integration-version` stamp
and offers *upgrade* or *skip*, never a blind re-write. It ends with a manifest,
which is also written to `~/.local/state/omt/setup-manifest.json` and is what §8
reads to uninstall:

```
Setup complete. Changed:
  ~/.zshrc                    +4 lines          (backup: ~/.zshrc.omt-20260803)
  ~/.config/omt/shell/omt.zsh created
  ~/.claude/settings.json     +3 hooks, +1 key  (backup: …settings.json.omt-20260803)
  ~/.config/omt/config.toml   created (theme=auto, keymap=tmux, leader=ctrl-a)
Not changed: Ghostty config, ~/.codex/config.toml (declined), network settings.
Undo everything: `omt uninstall --keep-config`     Details: `omt setup --status`
```

---

## 4. In-product help

### 4.1 The four entry points

| Command / chord | Answers | Source |
|---|---|---|
| `omt help [<group> [<verb>]]` | "what can this tool do" | catalog: groups, titles, doc comments, CLI flags |
| `⟨leader⟩ ?` | "what do my keys do *right now*" | `keys.list` + registry + terminal profile (§2.2) |
| `omt keys explain <chord>` | "why did that key do nothing" | [16 §5.3](16-input-and-keymap.md#53-omt-keys-explain-chord) |
| `omt agent explain [<session>]` | "why does omt think this agent is blocked" | [06 §4](06-agent-layer.md#4-merging-confidence-tiers-not-voting) |

Plus **contextual help**: `?` inside any omt overlay (palette, picker, explorer,
settings, copy mode, card focus) opens that surface's key list, filtered to the
active context set. It is one binding — `?` when
`!terminal_focused && !search_active` — resolving to `tui.open_keymap_help` with
`{ context: "current" }`. One capability, one binding, N surfaces; no per-surface
help text exists to fall out of date.

### 4.2 `omt help`, generated

```
$ omt help agent

  agent — observe and drive the agent CLI bound to a session

  omt agent state    [<session>]        What the agent is doing, and how sure omt is
  omt agent explain  [<session>]        Why omt believes that: sources, tiers, staleness
  omt agent prompt   <session> <text>   Send a prompt; queued if the agent is mid-turn
  omt agent interrupt <session>         Native interrupt where one exists, else ⌃C
  omt agent queue    list|enqueue|remove
  omt agent commands list|run           The agent's own slash commands
  omt agent bind|unbind <session> [<agent>]

  Every command takes --json (03 §5) and --instance. See also:
    omt help interaction   answering questions from any surface
    ⌃b A                   the dashboard version of all of this
```

The group blurb comes from the group's declaration; each line is
`title` + the input schema's positional rendering; "See also" comes from `group`
adjacency plus a `see_also` field on the group. `omt help` with no argument
lists groups only — never 163 capabilities. **CI diffs `omt help` output against
the generated reference docs**, so the two cannot disagree
([03 §5](03-capability-catalog.md#5-the-parity-contract) artifact #4).

### 4.3 How help stays accurate

The rule, stated as a constraint on contributors: **no user-facing help string
exists outside the catalog declaration, the keymap data files, or the inner-
keymap registry.** Consequences —

- A capability without a `title` fails CI (§2.4).
- A binding whose action names a nonexistent capability fails CI
  ([10 §8.2](10-configuration.md#82-keybinding-format)).
- Help text mentioning a chord is forbidden; help renders chords by *resolving*
  them, so a user with `leader = "ctrl-a"` never reads `Ctrl+B` anywhere in the
  product. (Grep lint: no `⌃b`/`Ctrl+B` literal in any `.rs` string or `.md`
  help template; the docs in `docs/` are exempt and use `⟨leader⟩`.)
- The `verified_against` staleness note
  ([16 §5.1](16-input-and-keymap.md#51-the-inner-program-keymap-registry))
  surfaces in the help overlay's middle column too, not only in diagnostics:
  *"checked against Claude Code 2.1.x; you are running 2.4.0"*.

---

## 5. tmux and zellij: migration and coexistence

Kenji judges omt in 60 seconds, with his fingers, not his eyes. Everything in
this section exists to make those 60 seconds succeed, and the honest §5.5 exists
so that minute 61 does not undo it.

### 5.1 Command and keybinding equivalence

tmux's default prefix is `C-b`; omt's default leader is `C-b`
([16 §3.2](16-input-and-keymap.md#32-default-leader-ctrlb)). Chords below are
omt's **default** keymap; the `tmux` keymap (§5.2) makes the middle column
literal for every row.

| # | tmux command (and default chord) | omt capability | omt default chord |
|---|---|---|---|
| 1 | `new-session` (`:new`) | `session.create` | `⟨leader⟩ c` |
| 2 | `new-window` (`c`) | `session.create` | `⟨leader⟩ c` |
| 3 | `kill-pane` (`x`) | `session.close` | `⟨leader⟩ x` |
| 4 | `kill-window` (`&`) | `session.close` (all panes) | `⟨leader⟩ X` |
| 5 | `detach-client` (`d`) | `tui.detach` | `⟨leader⟩ d` |
| 6 | `attach-session` | `session.attach` | `omt attach [<name>]` |
| 7 | `choose-tree` / `choose-window` (`w`, `s`) | `tui.open_session_picker` | `⟨leader⟩ w` |
| 8 | `split-window -h` (`%`) | `pane.split{vertical}` | `⟨leader⟩ \|` |
| 9 | `split-window -v` (`"`) | `pane.split{horizontal}` | `⟨leader⟩ -` |
| 10 | `select-pane -L/-D/-U/-R` (`←↓↑→`) | `pane.navigate` ([17 §9.1](17-panes-and-layout.md#91-pane)) | `⟨leader⟩ h j k l` / arrows |
| 11 | `select-pane -t :.+` (`o`) | `pane.focus_cycle` | `⟨leader⟩ o` |
| 12 | `last-pane` (`;`) | `pane.focus_last` | `⟨leader⟩ ;` |
| 13 | `display-panes` (`q`) | `pane.show_numbers` — **proposed, not yet declared**⁴ | `⟨leader⟩ q` |
| 14 | `resize-pane -L/-D/-U/-R` (`M-←` …) | `pane.resize` (repeatable) | `⟨leader⟩ ⌃h j k l` |
| 15 | `resize-pane -Z` (`z`) | `pane.zoom` | `⟨leader⟩ z` |
| 16 | `swap-pane -D/-U` (`}`/`{`) | `pane.swap` | `⟨leader⟩ }` / `{` |
| 17 | `rotate-window` (`C-o`) | `pane.rotate` | `⟨leader⟩ ⌃o` |
| 18 | `select-layout` (`M-1`…`M-5`, `Space`) | `layout.preset`; `layout.apply_saved` for a named one ([17 §9.2](17-panes-and-layout.md#92-layout)) | `⟨leader⟩ Space` cycles presets |
| 19 | `break-pane` (`!`) | `pane.stack.break_out` ([17 §9.1](17-panes-and-layout.md#91-pane)) | `⟨leader⟩ !`¹ |
| 20 | `join-pane` | `pane.move_to_workspace` (17 §9.1) | palette |
| 21 | `next-window` / `previous-window` (`n`/`p`) | `pane.focus_cycle { reverse }`⁵ | `⟨leader⟩ n` / `p` |
| 22 | `last-window` (`l`) | `pane.focus_last`⁵ | `⟨leader⟩ l` |
| 23 | `select-window -t N` (`0`–`9`) | `pane.focus_index`⁵ | `⟨leader⟩ 0`–`9` |
| 24 | `rename-window` (`,`) | `session.rename` | `⟨leader⟩ .`² |
| 25 | `rename-session` (`$`) | `workspace.rename` | `⟨leader⟩ $` |
| 26 | `copy-mode` (`[`) | `tui.enter_copy_mode` | `⟨leader⟩ [` |
| 27 | `paste-buffer` (`]`) | `media.paste_buffer` — **proposed, not yet declared**⁴ | `⟨leader⟩ ]` |
| 28 | `list-buffers` / `choose-buffer` (`=`) | `media.buffers.list` — **proposed, not yet declared**⁴ | `⟨leader⟩ =` |
| 29 | `save-buffer` / `load-buffer` | `media.buffers.save` / `.load` — **proposed, not yet declared**⁴ | palette |
| 30 | `command-prompt` (`:`) | `tui.open_command_palette` | `⟨leader⟩ p`³ |
| 31 | `list-keys` (`?`) | `tui.open_keymap_help` | `⟨leader⟩ ?` |
| 32 | `send-prefix` (`C-b`) | `SendKey(leader)` | `⟨leader⟩ ⟨leader⟩` |
| 33 | `clock-mode` (`t`) | — | **not implemented** (§5.5) |
| 34 | `capture-pane -p` | `session.capture` | CLI / palette |
| 35 | `send-keys` | `session.send_text` / `session.send_keys` | CLI / palette |
| 36 | `pipe-pane` | `session.pipe` — **proposed, not yet declared**⁴ | CLI |
| 37 | `synchronize-panes` | `layout.synchronize` ([17 §9.2](17-panes-and-layout.md#92-layout)) | `⟨leader⟩ S` |
| 38 | `set-option` / `show-options` | `config.set` / `config.get` | `⟨leader⟩ ,` |
| 39 | `source-file` | `config.reload` | auto (live reload, [10 §6](10-configuration.md#6-live-reload)) |
| 40 | `list-sessions` (`ls`) | `session.list` | `omt session list` |
| 41 | `respawn-pane` | `session.restart` | palette |
| 42 | `swap-window` / `move-window` | `pane.swap` / `pane.move_to_workspace` (17 §9.1) | palette |
| 43 | `find-window` (`f`) | palette search | `⟨leader⟩ p` |
| 44 | `refresh-client` (`r`) | `tui.redraw` — **proposed, not yet declared**⁴ | `⟨leader⟩ r` |

¹ `⟨leader⟩ !` is `agent.interrupt` in the default keymap
([16 §8.2](16-input-and-keymap.md#82-the-leader-namespace)); in the `tmux`
keymap it is `pane.stack.break_out` and `agent.interrupt` moves to `⟨leader⟩ ⌃c`. This
is the one genuine collision between the two maps and it is resolved in favour
of tmux inside the tmux keymap, by construction.
² tmux's `,` is omt's settings chord in the default keymap; the `tmux` keymap
restores `,` to rename and moves settings to `⟨leader⟩ ⌃,`.
³ omt has no `:` command line. The palette is strictly more capable (it fuzzy-
searches, shows chords, and builds argument forms from schemas), and the `tmux`
keymap binds `:` to it so the muscle memory lands somewhere correct.
⁴ **Proposed, not yet declared.** These four names appear nowhere in the
capability catalog: [17 §9](17-panes-and-layout.md#9-capabilities) owns `pane.*`
and `layout.*`, [05 §10](05-session-model.md#10-capability-surface) owns
`session.*`, and neither declares them. They are recorded here as the tmux
commands omt has no equivalent for yet, so that the generated reference does not
inherit invented capabilities from a migration table. Each needs a real
declaration in its owning document before it can be bound.
⁵ tmux's *window* is omt's *session occupying a pane*, so window navigation is
pane focus in omt and uses [17 §9.1](17-panes-and-layout.md#91-pane)'s
`pane.focus_*` family. There is no separate `session.focus_*` family; the
session-level list operation is `session.list`.

**Zellij**, abbreviated, because the population is smaller and its model differs
more: `Ctrl+p` pane mode → `⟨leader⟩` prefix; `Ctrl+t` tab mode → sessions;
`Ctrl+s` scroll/search → `⟨leader⟩ [` and `⟨leader⟩ /`; `Ctrl+o` session manager
→ `⟨leader⟩ w`; floating panes (`Ctrl+p w`) → **unsupported in v1**
([17](17-panes-and-layout.md)); stacked panes → unsupported; KDL layouts →
importable to omt layouts on a best-effort basis with the same report format as
§5.3. Zellij's *modal* input model (a sticky mode until `Esc`) is deliberately
not reproduced: [16 §12](16-input-and-keymap.md#12-what-this-document-deliberately-does-not-do)
refuses ambient modes over a pane, and that refusal outranks migration comfort.

### 5.2 The `tmux` keymap, shipped as data

Per [16 §6.5](16-input-and-keymap.md)'s keymap-as-data design, `tmux` is a
shipped keymap file, not code. One config line adopts it:

```toml
# ~/.config/omt/keybindings.toml
keymap = "tmux"          # `default` | `tmux` | `vim` | `emacs` | a file in keymaps/
leader = "ctrl-b"        # tmux's own prefix; set `ctrl-a` if that is what you use
```

```toml
# keymaps/tmux.toml  (shipped)
#:schema https://omt.dev/schemas/keymap.schema.json
id       = "tmux"
display  = "tmux compatibility"
extends  = "default"                 # inherits everything not overridden
modal    = false
notes    = """
Reproduces tmux 3.4's default prefix table over omt capabilities.
Rows tmux has and omt does not are listed in `unmapped` and reported by
`omt keys list --keymap tmux --unmapped`.
"""

["<leader> c"]      = "session.create"
["<leader> x"]      = { capability = "session.close",  args = { confirm = true } }
["<leader> &"]      = { capability = "session.close",  args = { scope = "window", confirm = true } }
["<leader> d"]      = "tui.detach"
["<leader> %"]      = { capability = "pane.split",     args = { direction = "vertical" } }
['<leader> "']      = { capability = "pane.split",     args = { direction = "horizontal" } }
["<leader> o"]      = "pane.focus_next"
["<leader> ;"]      = "pane.focus_last"
["<leader> q"]      = "pane.show_numbers"      # proposed, §5.1 note 4
["<leader> z"]      = "pane.zoom"
["<leader> {"]      = { capability = "pane.swap",      args = { direction = "prev" } }
["<leader> }"]      = { capability = "pane.swap",      args = { direction = "next" } }
["<leader> !"]      = "pane.stack.break_out"
["<leader> ctrl-c"] = "agent.interrupt"      # displaced by the row above
["<leader> space"]  = "layout.preset"          # cycles presets
["<leader> ,"]      = "session.rename"
["<leader> ctrl-,"] = "tui.open_settings"    # displaced by the row above
["<leader> $"]      = "workspace.rename"
["<leader> ["]      = "tui.enter_copy_mode"
["<leader> ]"]      = "media.paste_buffer"     # proposed, §5.1 note 4
["<leader> ="]      = "media.buffers.list"     # proposed, §5.1 note 4
["<leader> :"]      = "tui.open_command_palette"
["<leader> ?"]      = "tui.open_keymap_help"
["<leader> n"]      = { capability = "pane.focus_cycle", args = { reverse = false } }
["<leader> p"]      = { capability = "pane.focus_cycle", args = { reverse = true } }   # NOTE: not the palette, in this keymap
["<leader> l"]      = "pane.focus_last"
["<leader> f"]      = "tui.open_command_palette"
["<leader> r"]      = "tui.redraw"             # proposed, §5.1 note 4
["<leader> t"]      = "none"                 # clock-mode: unmapped, see `unmapped`
["<leader> up"]     = { capability = "pane.navigate", args = { dir = "up" } }
# … down/left/right, `<leader> 0`–`9` → pane.focus_index, `<leader> ctrl-<arrow>`
# resize (repeatable = true)

[[unmapped]]
tmux = "clock-mode"
reason = "omt has no clock mode and does not plan one."

[[unmapped]]
tmux = "choose-client"
reason = "omt's client model is different: see `omt presence list` and 12 §3."
```

Two properties matter more than the contents:

- **`p` is not the palette in this keymap.** tmux users press `⟨leader⟩ p` for
  *previous window* thousands of times a year. Stealing it would break exactly
  the muscle memory this keymap exists to serve. The palette keeps `⟨leader⟩ :`,
  `⟨leader⟩ f`, and `Ctrl+Shift+P` where deliverable — and the which-key strip
  shows all three, so it is not hidden.
- **The keymap goes through [16 §5.2](16-input-and-keymap.md#52-static-validation-at-config-load)'s
  validation like any user config.** A shipped keymap that shadows a `critical`
  inner-program key produces `OMT-C406` at build time, in a test, not on the
  user's screen.

`omt keys list --keymap tmux --diff default` prints the delta; that output is
the reviewable artifact for the file, and a golden test.

### 5.3 The `.tmux.conf` importer

```
omt migrate tmux [--from ~/.tmux.conf] [--dry-run] [--out <dir>] [--json]
```

Parses tmux's config grammar (commands, `-flag` arguments, quoting, `\`
continuations, `if-shell`/`%if` blocks) — **not** by running tmux, because a
config that sources other files or shells out cannot be safely executed, and
because the importer must work on a machine with no tmux installed.

| Translatable | Into |
|---|---|
| `set -g prefix C-a`, `set -g prefix2` | `leader` (+ a note: omt has one leader, §10 Q4) |
| `bind [-n] [-r] [-T <table>] <key> <command>` | a `[[binding]]`, with `-n` → no `<leader>`, `-r` → `repeatable = true`, `-T copy-mode-vi` → `when = "copy_mode"` |
| `unbind <key>` | `"<chord>" = "none"` |
| `set -g mouse on` | `terminal.mouse_reporting = true` |
| `set -g base-index` / `pane-base-index` | `appearance.index_base` |
| `set -g history-limit N` | `terminal.scrollback_lines = N` |
| `set -sg escape-time N` | `input.esc_timeout = "Nms"` |
| `set -g mode-keys vi` / `status-keys` | `keymap = "vim"` (copy mode) |
| `set -g default-terminal`, `set -ga terminal-overrides ",*:RGB"` | recognized, mapped onto §7's `TERM` decision, reported not copied |
| `set -g renumber-windows`, `allow-rename`, `set-titles` | direct config keys |
| `set -g status-position top`, `status-style`, `status-interval` | `appearance.status_bar.*` (position, colours) |
| `set -g status-left/right` with **literal text and `#S #I #P #W #h` only** | `appearance.status_bar.template` |
| `set -g pane-border-style`, `pane-active-border-style` | theme roles |
| `setw -g automatic-rename` | `session.auto_title` |
| `set -g set-clipboard on` | `media.osc52 = true` |
| `set -g focus-events on` | already always on ([04](04-terminal-core.md)) |

| Not translatable | Why, and what the report says |
|---|---|
| `run-shell`, `if-shell`, `%if` | Arbitrary shell at config-eval time. omt's config is data ([10 §1.1](10-configuration.md#11-decision-toml-for-the-main-config)) and will not gain an eval step. Reported with the line, verbatim. |
| `set -g @plugin`, TPM, `run '~/.tmux/plugins/tpm/tpm'` | Plugins are omt plugins ([11](11-plugins.md)), a different model. Each named plugin is reported with a one-line note where an omt equivalent exists (`tmux-resurrect` → omt persists sessions natively, [05 §8](05-session-model.md); `tmux-yank` → `⟨leader⟩ y`; `tmux-sensible` → mostly defaults). |
| `set-hook`, `bind ... run-shell` | tmux hooks fire shell commands; omt has no equivalent by design. |
| Format strings beyond the `#S #I #P #W #h` subset (`#{?pane_synchronized,…}`, `#(command)`) | A conditional mini-language with command substitution. Reported in full; the status bar's own template language is deliberately smaller (§2.1). |
| `bind` to `command-prompt -p ... "%%"` | Interactive command construction; the palette replaces it but not mechanically. |
| `link-window`, `choose-client`, `clock-mode`, `wait-for`, `switch-client -T` | No omt concept. Named individually. |
| `terminal-features`/`terminal-overrides` beyond RGB | omt probes rather than trusts a table (§7.3). Reported as "ignored — omt probes your terminal instead; see `omt doctor keys`". |

**The report** is the deliverable, not the config. Written to stdout, and to
`~/.config/omt/migration-report.md` so it can be read after the fact:

```
$ omt migrate tmux

Read ~/.tmux.conf (74 lines, 2 sourced files: ~/.tmux/keys.conf, ~/.tmux/theme.conf)

  ✓ mapped        41 directives
  ~ mapped, review 4 directives
  ✗ unmapped       9 directives
  · ignored        7 directives (no-ops or already omt defaults)

WOULD WRITE (nothing written yet — re-run without --dry-run):
  ~/.config/omt/keybindings.toml   new, 38 bindings, leader = "ctrl-a"
  ~/.config/omt/config.toml        new, 14 settings

── mapped, review ───────────────────────────────────────────────────────────
  ~ tmux.conf:22   bind -n C-h select-pane -L
      → [[binding]] trigger = "ctrl-h", capability = "pane.navigate"
                    args = { dir = "left" }
      ! UNPREFIXED. warning[OMT-C406]: `ctrl-h` is a critical key for `nvim`
        (backspace/left in insert mode) and for readline. It will not reach any
        program in an omt pane. This is also true in your tmux today — you may
        simply not have noticed on this machine. See 16 §5.
      ! In the legacy encoding `ctrl-h` is also `Backspace` (16 §1.2).
      → keep it, scope it (`when = "!agent_bound"`), or drop it: [k/s/d]

  ~ tmux.conf:31   set -g status-right '#(gitmux "#{pane_current_path}")'
      → status bar right cell mapped to omt's own branch cell (§2.1), which
        needs no subprocess. Your `#(…)` command substitution is NOT run.

  ~ tmux.conf:44   set -g default-terminal "screen-256color"
      → not copied. omt advertises TERM=omt-256color and falls back to
        xterm-256color (19 §7). Your value described tmux, not omt.

  ~ tmux.conf:58   bind -T copy-mode-vi 'v' send -X begin-selection
      → keymap = "vim" covers this and 6 sibling bindings; individual copy-mode
        rebindings were folded into the vim keymap rather than re-declared.

── unmapped ─────────────────────────────────────────────────────────────────
  ✗ tmux.conf:3    set -g @plugin 'tmux-plugins/tmux-resurrect'
      omt persists sessions, layout, blocks and scrollback natively across
      daemon restart (05 §8). No plugin needed; PTY processes still do not
      survive a restart today (05 §13 / G12) — resurrect did not save those
      either.
  ✗ tmux.conf:4    set -g @plugin 'tmux-plugins/tmux-continuum'   (see above)
  ✗ tmux.conf:12   set-hook -g after-new-window 'run-shell "~/bin/tag-window"'
      omt has no config-time shell hooks by design. Closest: a launch
      configuration (10 §9.1) that runs setup commands when you start a session.
  ✗ tmux.conf:19   bind C-j run-shell "~/bin/fzf-panes.sh"
      run-shell is not translatable. You can bind a workflow (10 §9.2) that
      executes the same script: an example is written to
      ~/.config/omt/migration-report.md §3.
  ✗ tmux.conf:37   set -g status-left '#[fg=green]#{?client_prefix,◆,◇} #S'
      Format-string conditionals are not supported (§5.3). omt's prefix
      indicator is the which-key strip (16 §3.3).
  … 4 more, all listed in the report file.

Nothing about your tmux setup was modified. `tmux` still works exactly as before.
```

The last line is not decoration. A user running an importer on a config they
depend on daily needs to be told, in the tool's own voice, that the source was
not touched.

**Failure policy.** A line the parser cannot parse at all is reported as
`? line N: could not parse` with the text, and the import continues. The
importer never aborts on one bad line, and never writes a partial config: it
builds the whole result in memory, runs it through
[10 §5](10-configuration.md#5-validation-and-diagnostics)'s validation, and
writes only if validation passes — printing the diagnostics if it does not.
`--json` emits the same structure for tooling
([R69](../design/scenarios.md#automation)).

### 5.4 Coexistence: tmux inside an omt pane

Some users will keep tmux and run it inside omt — a team runbook that says
`tmux attach -t deploy`, a remote host where tmux is the only thing installed,
or simple preference ([J5](../design/scenarios.md#j5--coexisting-with-a-tmux-he-refuses-to-give-up)).
This is supported, with a stated boundary.

**What works, unchanged:** the terminal. omt runs a real PTY with a real VT
emulator ([04](04-terminal-core.md)); tmux inside it behaves exactly as it does
in any other terminal — alt-screen, mouse, resize, colours, its own status bar.
Shell integration inside the nested tmux still emits OSC 133, so blocks continue
to work for the shells inside it ([04 §7.3](04-terminal-core.md#73-propagation-into-subshells-and-over-ssh)).

**What does not work, and is not going to:**

| Broken | Why |
|---|---|
| Agent observation inside the nested tmux | [00 §8](00-overview.md#8-what-omt-is-not) states it, and tiers 1–2 depend on process/environ correlation with panes omt spawned; tmux's server owns those processes and re-parents them. The binding reports `Unknown`. |
| omt's block model for the nested panes' output | omt sees tmux's *rendered composite*, not per-pane streams. |
| `file:line` opening and semantic actions on nested content | Same reason; the coordinates omt computes belong to tmux's rendering ([18](18-semantic-open.md)). |
| OSC round-trips (clipboard read-back, image protocols, XTGETTCAP) | tmux intercepts and, without `allow-passthrough`, swallows them (§5.5 below and [09 §3.1](09-ssh-and-media.md)). |

**Detection and the one warning.** omt detects tmux in a pane by, in order:
`$TMUX` in the pane's spawned environment; the foreground process name (already
tracked for the block heuristic, [04 §6.4](04-terminal-core.md#64-the-fallback-heuristic--no-shell-integration));
and `TERM` beginning `screen`/`tmux`. On detection:

1. The status bar's session cell gains a `nested:tmux` badge, permanently, for
   that pane. It is state, not a hint ([R11](../design/scenarios.md#terminal-and-multiplexer-fundamentals)).
2. The agent binding for the pane goes `Unknown` with the reason string
   *"observation unavailable inside a nested multiplexer"*, visible in the
   session card and in `omt agent explain`.
3. **Once per install** (not per pane, not per session), omt prints one line into
   that pane's scrollback:

```
omt: tmux detected in this pane. The terminal works normally; agent observation
     does not (19 §5.4). Your tmux prefix reaches tmux — omt's ⌃b is consumed by
     omt first; use ⌃b ⌃b to send it through, or change one of the two prefixes.
     This message appears once.  `omt help nesting`
```

**The double-prefix problem, resolved explicitly.** omt sees keys first
([16 §1.1](16-input-and-keymap.md#11-the-four-layers)). If both use `C-b`, omt
consumes it and tmux never sees it. Three supported resolutions, in the order
omt recommends them: (a) `⟨leader⟩ ⟨leader⟩` sends the leader through
([16 §3.4](16-input-and-keymap.md#34-timeout-and-escape-hatches)) — costs one
keystroke, requires no config; (b) change omt's leader (`leader = "ctrl-a"`),
one line; (c) change tmux's prefix. omt never changes tmux's config, and the
`tmux` inner-keymap registry entry
([16 §5.1](16-input-and-keymap.md#51-the-inner-program-keymap-registry)) is what
makes omt able to say *"`ctrl-b` is tmux's prefix too"* at config-load time.

### 5.5 `allow-passthrough` and OSC

tmux does not forward unrecognized escape sequences to the outer terminal unless
`set -g allow-passthrough on`, and even then only inside a `DCS tmux;` wrapper
with doubled `ESC` ([09 §3.1](09-ssh-and-media.md)). Consequences, per direction:

- **omt → outer terminal, through a nested tmux** (omt emitting OSC for
  clipboard, images, or notifications on behalf of a pane): omt is the outer
  terminal here, so nothing is nested from omt's perspective. Unaffected.
- **A program inside nested tmux → omt**: tmux swallows OSC 52, OSC 1337 images,
  kitty graphics and XTGETTCAP replies unless passthrough is on. omt therefore
  **detects the nesting and reports the capability as absent rather than
  emitting sequences that will vanish** — the same policy
  [09](09-ssh-and-media.md) applies to the outer direction. `omt doctor media`
  prints the chain and names tmux as the breaking link, with the exact
  `set -g allow-passthrough on` line to fix it and the warning that passthrough
  lets any program in any tmux pane emit sequences to the real terminal, which
  is a genuine (small) security consideration the user should make knowingly.
- **omt inside tmux** (the outer direction: a user runs `tmux` then `omt`): the
  same wrapping rules apply to everything omt emits, keyed off `TERM` beginning
  `screen`/`tmux` rather than `$TMUX` ([09 §3.1](09-ssh-and-media.md)). omt
  detects this at startup and prints it in `omt doctor`. It is supported but
  costs the kitty keyboard protocol on tmux versions that do not forward it,
  which narrows the default keymap ([16 §5.5](16-input-and-keymap.md#55-terminal-capability-probing))
  — visibly, in the help overlay's third column.

### 5.6 Why switch, and when not to

Overselling here produces angry users, and angry ex-tmux users are loud. This
section is shipped verbatim as `omt help migrate` and in the README, and is
therefore held to [D9](decisions.md#d9--positioning-what-omt-may-and-may-not-claim)'s
table of what omt may and may not claim.

**Switch if:**

- **You want your scrollback to be a structure rather than a wall.** omt
  segments output into command blocks (OSC 133, [04 §6](04-terminal-core.md)),
  which is what makes a day of terminal work a *collapsible list on a phone*
  with the full terminal one tap away — and makes it survive a daemon restart,
  with search over it. This is the specific thing no other product does: real VT
  emulation, panes and layouts, and a mobile client, in one tool
  ([D9](decisions.md#d9--positioning-what-omt-may-and-may-not-claim)).
- You want a real terminal underneath: 24-bit colour, images, hyperlinks, the
  kitty keyboard protocol, correct reflow. omt runs **your real CLI, in a real
  PTY, with its own TUI, keybindings and slash commands, observed from outside
  rather than instrumented from within**. tmux is a multiplexer on top of your
  terminal and is a lossy layer for all of those.
- You already reach for a phone or a second machine mid-task.
- You run agent CLIs and want to see which of them are blocked on a question,
  and answer from your phone — **while your real interactive TUI is still on
  screen at your desk**. Remote question cards themselves are not unique to omt;
  several products ship them. Answering one *without* replacing the agent's own
  terminal UI is the part that is omt's ([D9](decisions.md#d9--positioning-what-omt-may-and-may-not-claim),
  [06 §5.3](06-agent-layer.md#53-the-deferral-mechanism-and-its-risk)).

**Do not switch if:**

- **You need panes to survive the multiplexer process dying.** tmux's server
  outlives its clients and, in practice, outlives crashes and upgrades. omt
  persists metadata, layout, blocks and scrollback across daemon restart, but
  **the PTY processes do not survive today** ([05 §13](05-session-model.md),
  [G12](../design/scenarios.md#g12--session-survival-across-daemon-restart-and-upgrade--owned-but-the-answer-is-weak)).
  If your workflow is "start a build, detach, reboot the client, reattach", tmux
  is currently better and we will not pretend otherwise.
- **You depend on tmux's scripting surface.** `run-shell`, hooks, `if-shell`,
  format strings, `pipe-pane` into arbitrary pipelines, and the plugin ecosystem
  are a programmable layer omt deliberately does not have
  ([10 §1.1](10-configuration.md#11-decision-toml-for-the-main-config)). omt's
  answer is capabilities + `--json` + workflows, which is more structured and
  much less flexible.
- **You are on native Windows.** WSL2 only, v1 ([G11](../design/scenarios.md#g11--windows-target-or-not--no-owner)).
  tmux has the same constraint, but say it anyway.
- **You need floating or stacked panes**, or tmux's `link-window` sharing model.
  Not in v1 ([17](17-panes-and-layout.md)).
- **Your tmux is on a server you `ssh` into and omt is not installed there.**
  You can run omt locally and `ssh` from a pane, but the remote features need a
  remote omt — which `omt ssh` will offer to install for you over the existing
  ssh connection ([22 §7](22-operations.md#7-bootstrap-onto-a-host-with-no-omt-r38),
  [R38](../design/scenarios.md#remote-and-multi-machine)).
- **You have a decade-old config full of `run-shell` and TPM plugins and no
  appetite to rebuild it.** §5.3 will tell you honestly how much survives. Run
  `omt migrate tmux --dry-run` before deciding; it writes nothing.

**And the honest middle:** you can keep tmux for the servers and use omt
locally, or run tmux inside omt for the one runbook that needs it (§5.4). omt is
designed to lose that argument gracefully rather than to win it by lock-in.

---

## 6. Progressive disclosure of advanced features

The default experience mentions none of the following. Each is surfaced by a
**trigger the user themselves produces**, once, and then never again.

| Feature | Trigger | Surfacing | Never |
|---|---|---|---|
| Remote / phone client | The user runs `omt server`/`omt pair`, **or** an interaction has been open > 10 min with no attached client on three separate occasions | On the third occasion, one line in the dashboard (not the pane): *"3 questions have waited > 10 min. `omt pair` puts these on your phone."* Counter resets on run. | Never on first run. Never as a pane hint. Never a QR code nobody asked for. |
| Plugins | `omt plugin` typed, or a capability is missing that a known plugin provides | `omt help plugin`; the missing-capability error names the plugin ([11](11-plugins.md)) | No plugin marketplace UI, no "featured" list |
| Worktrees | The pane's cwd is inside a repo with > 1 worktree, **and** the user opens the explorer | The explorer shows a worktree switcher ([15](15-workspace-explorer.md)) | No suggestion to create one — [anti-requirement 6](../design/scenarios.md#part-5--anti-requirements) forbids VCS mutation |
| Vim / emacs keymaps | Setup step 4; `$EDITOR`/`VISUAL` contains `vim`; or `keymap` typed in the palette | Setup offers it once with the reason ("your `$EDITOR` is nvim"); afterwards only via config | No mode inference at runtime |
| Voice / STT | `omt stt` typed, or the web client's mic button | [08 §7](08-web-client.md) | Never on the TUI unprompted |
| Collaboration / sharing | `omt share` typed | [12](12-collaboration.md) | Never suggested |
| Workflows / launch configs | The user runs the same 3+ command sequence in a fresh session 3 times | **Nothing.** Recorded as an open question (§10 Q6) rather than shipped — an unrequested "we noticed a pattern" toast is exactly the behaviour §1.2's rule exists to forbid. | — |

The pattern: **capability discovery lives in the palette (searchable, silent);
feature *advocacy* is limited to at most one occasion per feature, and only
after the user has demonstrably hit the problem the feature solves.**

---

## 7. `TERM` and terminfo — R6

Requirement [R6](../design/scenarios.md#terminal-and-multiplexer-fundamentals)
was unowned. It is owned here.

> **The whole of §7 applies to `pty` sessions only.** `TERM`, terminfo and the
> outer-terminal probe describe the environment omt hands to a process it
> spawned in a pseudo-terminal. A `native` session
> ([D8](decisions.md#d8--two-session-modes-pty-default-and-native-acp),
> [05 §1.3](05-session-model.md#13-session-modes-d8)) has no PTY and no child
> process reading terminfo: omt renders it from typed JSON-RPC events, so there
> is nothing to advertise and nothing to fall back to. `omt doctor term` reports
> `n/a (native)` for such sessions rather than a passing check.

This is a classic source of subtle breakage:
advertise too much and `less` emits sequences omt does not implement; advertise
too little and `nvim` runs in 8 colours.

### 7.1 The decision

> **omt advertises `TERM=omt-256color`, ships the terminfo source for it,
> installs it into `~/.terminfo` at setup, and falls back to
> `TERM=xterm-256color` whenever the entry is not resolvable by the program in
> the pane — including on every remote host reached by `ssh` from inside omt.**

Additionally, and unconditionally: `COLORTERM=truecolor`, plus `OMT_INSTANCE`,
`OMT_SESSION`, `OMT_SOCK` ([06 §7.2](06-agent-layer.md#72-correlation)) and
`OMT_TERM=omt-256color` (which survives even when `TERM` has been downgraded, so
a program that wants to know can ask).

Rationale against the alternatives:

| Option | Verdict |
|---|---|
| `TERM=xterm-256color` always | Safe and universally resolvable, but it under-describes omt: no `Tc`/`RGB`, no `Smulx`/`Setulc` (styled underlines), no `Ms` (OSC 52), no `XF` (focus events), no kitty-keyboard hint. Programs that consult terminfo — `tmux`, `vim`, `less`, `ncurses` apps — would degrade for no reason. **Rejected as the primary, adopted as the fallback.** |
| `TERM=xterm-kitty` / `xterm-ghostty` (borrow someone's) | Immediately claims *their* full capability set, which omt does not implement byte-for-byte, and misattributes bug reports to them. Dishonest. **Rejected.** |
| A brand-new base entry, `omt` | Correct but maximally fragile: nothing resolves it anywhere until it lands in ncurses, and `ssh` to any box breaks. |
| **`omt-256color`, `use=xterm-256color` plus deltas, shipped and installed** ✅ | Honest (we claim only what we implement), resolvable everywhere we install it, and degrades to a name every host on earth has. The `-256color` suffix means naive `TERM`-sniffing code (`case $TERM in *256color*)`) does the right thing. |

### 7.2 The entry

```terminfo
omt-256color|oh-my-term with 24-bit colour and modern extensions,
    use=xterm-256color,
#   colour and styling
    Tc, setrgbf=\E[38:2:%p1%d:%p2%d:%p3%dm, setrgbb=\E[48:2:%p1%d:%p2%d:%p3%dm,
    Smulx=\E[4:%p1%dm, Setulc=\E[58:2::%p1%{65536}%/%d:%p1%{256}%/%{255}%&%d:%p1%{255}%&%dm,
#   clipboard (OSC 52 write; read is refused, 04 §5.4)
    Ms=\E]52;%p1%s;%p2%s\007,
#   focus events, bracketed paste, synchronised output
    XF, XR=\E[>0q, fd=\E[?1004h, fe=\E[?1004l,
    BD=\E[?2004l, BE=\E[?2004h, PS=\E[200~, PE=\E[201~,
    Sync=\E[?2026%?%p1%{1}%-%tl%eh%;,
#   cursor styling and colour
    Ss=\E[%p1%d q, Se=\E[2 q, Cs=\E]12;%p1%s\007, Cr=\E]112\007,
#   hyperlinks
    Hls=\E]8;%?%p1%tid=%p1%s%;;%p2%s\E\\,
#   NOT claimed: sixel (Sxl), kitty graphics — probe-gated, see §7.4
```

Every line is a capability [04 §5](04-terminal-core.md) marks **Must**. A
capability at **Should** priority is *not* in the entry until it is implemented
and covered by a conformance test — that is the rule that keeps this honest, and
CI enforces it by cross-referencing the entry against the implemented-sequence
table.

`omt-256color` is the only entry shipped. No `omt-direct` variant (`Tc` covers
it), no `omt` base entry (§7.1), no `-nel`/`-m` variants.

### 7.3 Installation and resolution

- **Installed at `omt setup`** into `~/.terminfo/o/omt-256color` via `tic -x -o
  ~/.terminfo`, or written directly in compiled form if `tic` is absent (omt
  ships the compiled binary form for the platforms it supports; the source
  `.terminfo` file is also written to `~/.config/omt/terminfo/` so the user can
  `tic` it anywhere, including onto a remote host).
- **Verified before it is advertised.** At instance start, omt resolves
  `omt-256color` through the same path a child process would (`$TERMINFO`,
  `$TERMINFO_DIRS`, `~/.terminfo`, the system database). **If resolution fails,
  omt sets `TERM=xterm-256color` for every pane and records the reason**, shown
  in `omt doctor` and in the help overlay's footer. omt never advertises a
  `TERM` a program in its own pane cannot look up — that is the failure mode
  where `clear` prints garbage and nobody knows why.
- **`ssh` from inside a pane.** `TERM` propagates, and the remote host will not
  have the entry. Three tiers, matching
  [04 §7.3](04-terminal-core.md#73-propagation-into-subshells-and-over-ssh)'s
  shell-integration tiers: (1) omt detects an interactive `ssh` invocation and
  offers, **once per host, with confirmation**, to copy the terminfo entry to
  `~/.terminfo` on the remote — this is an outward-facing write to someone
  else's machine and is never automatic; (2) declined or non-interactive, omt
  rewrites `TERM=xterm-256color` in the child environment for that `ssh`
  invocation only (`terminal.ssh_term_fallback = "xterm-256color"`, settable to
  `"preserve"`); (3) `omt ssh` ([09 §6](09-ssh-and-media.md)) has omt on both
  ends and needs none of this.
- **Escape hatch:** `terminal.term = "xterm-256color"` forces the fallback
  globally, for a user who hits any of this and wants it to stop. It is
  documented at the top of §7 in the generated reference, because it is the
  first thing a frustrated user will search for.

### 7.4 What is claimed, what is probed, and the poorer-outer-terminal case

**terminfo describes what omt implements toward the program in the pane. It says
nothing about what omt's *outer* terminal can do.** These are different
questions and conflating them is the classic bug:

- **Text-mode capabilities** (colour, styling, cursor, focus, bracketed paste,
  sync) are implemented *by omt's own emulator* and rendered as cells. omt
  renders them itself for any outer terminal, downgrading at draw time: true
  colour → 256 → 16 → mono, per the outer terminal's probed profile. **These are
  safe to claim unconditionally**, which is exactly why they are in the entry.
- **Passthrough capabilities** — sixel, kitty graphics, OSC 52 clipboard write,
  OSC 8 hyperlinks — depend on the outer terminal. They are **not** claimed in
  terminfo except `Ms` and `Hls`, both of which have safe failure modes (a
  clipboard write that silently does nothing, per
  [09 §3.1](09-ssh-and-media.md)'s explicit no-acknowledgement policy; a
  hyperlink that renders as plain text). Graphics are advertised **only through
  runtime probes** — XTGETTCAP, the kitty graphics query, and the iTerm2
  `1337` handshake — which programs actually use for images, and which omt
  answers based on the *outer* terminal's real capability. So `imgcat` in an omt
  pane inside Terminal.app gets an honest "no" rather than a garbage blob.
- **The poorer-outer-terminal fallback, concretely:** omt's `ModeView` and
  renderer already downgrade attributes at draw time
  ([04 §4.1](04-terminal-core.md#41-what-a-renderer-gets)). What §7 adds is that
  **omt does not change `TERM` in response to a poor outer terminal.** A pane's
  `TERM` is a property of omt, not of what omt happens to be displayed in today
  — otherwise a session reattached from Terminal.app to Ghostty would change
  `TERM` under a running `vim`, which nothing tolerates. The downgrade happens
  at render, invisibly and per-frame; the pane's contract is stable for the
  process's whole life.
- `omt doctor term` prints both sides: what omt advertises inward, what the
  outer terminal was probed to support, and every capability where the two
  disagree — which is the diagnostic that turns "my images do not work" into a
  one-line answer.

---

## 8. Uninstall and rollback

A tool that cannot cleanly uninstall does not get installed. This is a feature
with a test, not a paragraph in a README.

```
omt uninstall [--keep-config] [--keep-data] [--dry-run] [--yes]
```

**Three different things are called "uninstall" in this corpus, and they are
distinguished by capability group, not by prose.** `uninstall.plan` /
`uninstall.apply` (this section) remove **omt's own footprint** from the machine.
`service.uninstall` ([22 §10](22-operations.md#10-capabilities),
[22 §1.2](22-operations.md#12-omt-service-install)) removes only the launchd or
systemd **user service unit**, leaving omt installed and start-on-demand intact.
`plugin.uninstall` ([11](11-plugins.md)) removes **one plugin**. `omt uninstall`
runs the first and, if a service unit is installed, calls the second as one of
its steps; neither touches plugins beyond deleting `~/.config/omt/` wholesale.

Behaviour, derived from `~/.local/state/omt/setup-manifest.json` (§3.6), which
records every outward-facing write with its path, a hash of what omt wrote, and
the backup's location:

1. **Agent hooks.** Remove only the entries stamped `omt-integration-version`,
   from the JSONC concrete syntax tree, preserving every other key, comment and
   formatting byte ([06 §7.1](06-agent-layer.md#71-integration-installer)). If
   the surrounding array becomes empty, remove the array only if omt created it.
2. **Shell rc files.** Remove exactly the `# >>> omt shell integration >>>` …
   `# <<< omt shell integration <<<` block. **If the block has been edited since
   omt wrote it** (hash mismatch), omt does not delete it: it comments it out,
   prints it, and says so. Deleting a line a user has customized is the sin this
   check exists to prevent.
3. **Terminfo.** Remove `~/.terminfo/o/omt-256color` if omt installed it and it
   is unchanged.
4. **omt's own directories.** `~/.config/omt/` unless `--keep-config`,
   `~/.local/state/omt/` (sockets, store, scrollback, blocks) unless
   `--keep-data`. Data removal prints a byte-count manifest, sharing the
   implementation with `store.purge`
   ([G4](../design/scenarios.md#g4--data-lifecycle-retention-export-deletion--partial-owner-weak)).
5. **Never touched:** anything not in the manifest. Emulator configs written by
   §3.5 are restored from their timestamped backups only with
   `--restore-terminal-config`, because the user may have edited them since.
6. **The binary itself** is the package manager's business. omt prints
   `brew uninstall omt` / the appropriate line, and does not delete itself.

`--dry-run` output is the deliverable:

```
$ omt uninstall --dry-run

Would restore:
  ~/.zshrc                   remove 4 lines (313–316)   verified unmodified since install
  ~/.claude/settings.json    remove 3 hooks + 1 key     verified unmodified since install
  ~/.terminfo/o/omt-256color remove                     verified unmodified since install
Would remove:
  ~/.config/omt/             12 files, 48 KB
  ~/.local/state/omt/        1.9 GB  (scrollback 1.7 GB, blocks 140 MB, audit 21 MB)
Would NOT touch:
  ~/.config/ghostty/config   omt never wrote to it
  ~/.codex/config.toml       you declined this at setup
Leftover after uninstall: nothing.

Run without --dry-run to proceed. `--keep-data` preserves ~/.local/state/omt/.
```

**The test.** An integration test that: snapshots a home directory (rc files
with pre-existing content, a `~/.claude/settings.json` with another tool's
hooks, an existing `~/.terminfo`), runs `omt setup --non-interactive --accept
all`, runs `omt uninstall --yes`, and asserts the home directory is **byte-
identical** to the snapshot. That test is the actual guarantee; everything above
is its specification.

---

## 9. Capabilities this document requires

Declared in [03 §2](03-capability-catalog.md#2-declaring-a-capability)'s style.
All are palette-visible with a `title` (§2.4) unless marked hidden.

| Capability | Kind | Role | Effects | Notes |
|---|---|---|---|---|
| `setup.status` | Query | Viewer | — | What is installed, versions, manifest |
| `setup.plan` | Query | Operator | `READS_FS` | The full six-step plan with diffs, unapplied. The web client renders this |
| `setup.apply` | Command | Admin | `WRITES_FS` | One step at a time; `{ step, accept: true }`; refuses without a matching plan hash |
| `setup.detect_agents` | Query | Viewer | `READS_FS` | §3.3 |
| `onboarding.hint_state` | Query | Viewer | — | Which hints remain; §1.2's one-hint rule is observable |
| `onboarding.dismiss_hints` | Command | Operator | `WRITES_FS` | Permanent |
| `help.topics` | Query | Viewer | — | §4.2's generated tree |
| `help.render` | Query | Viewer | — | `{ topic \| context }` → the help overlay's model |
| `migrate.tmux.preview` | Query | Operator | `READS_FS` | Parse + map + report, writing nothing |
| `migrate.tmux.apply` | Command | Admin | `WRITES_FS` | Requires the preview's hash |
| `migrate.zellij.preview` / `.apply` | " | " | " | §5.1, best-effort |
| `keys.keymaps` | Query | Viewer | — | Existing ([16 §11](16-input-and-keymap.md#11-capabilities-introduced-here)); gains `unmapped` for §5.2 |
| `term.profile` | Query | Viewer | — | Advertised `TERM`, entry resolution result, outer-terminal probe, disagreements (§7.4) |
| `term.install_terminfo` | Command | Admin | `WRITES_FS` | Local or, with an explicit host, remote via ssh — confirmation required |
| `system.doctor` `{ groups: [Term] }` | Query | Viewer | `READS_FS` | §7.4's rendering. **Not a new capability**: doctor is one parameterized capability owned by [22 §10](22-operations.md#10-capabilities), and this document contributes the `Term` group to it rather than a `doctor.term` of its own. The CLI spelling is `omt doctor term` ([G5](../design/scenarios.md#g5--observability-of-omt-itself--no-owner)) |
| `uninstall.plan` | Query | Admin | `READS_FS` | §8's dry run |
| `uninstall.apply` | Command | Admin | `WRITES_FS`, `DESTRUCTIVE` | Confirm gesture required on every surface. **Not** `service.uninstall` or `plugin.uninstall` — see §8 |
| `nesting.state` | Query | Viewer | — | Per-pane multiplexer detection, feeding the badge (§5.4) |

Catalog additions required by §2.4, as declaration *fields* rather than
capabilities: `title` (mandatory), `aliases`, `hidden` + `hidden_reason`. These
are a change to [03 §2](03-capability-catalog.md#2-declaring-a-capability)'s
table and are recorded as such in §10 Q1.

---

## 10. OPEN QUESTIONS

1. **`title`/`aliases`/`hidden` are additions to
   [03 §2](03-capability-catalog.md#2-declaring-a-capability)'s field table**,
   proposed here and not edited there. `title` mandatory-with-lints is the
   part likely to be argued: it adds a required field to ~163 declarations and
   will be resisted at review time. Recorded so the argument happens once.

2. **Does the palette's alias injection from §5.1's table scale?** Generating
   `aliases` from the migration table means the table is code-adjacent build
   input. Fine for tmux (44 rows); unclear whether zellij, screen and another terminal
   vocabularies get the same treatment or a separate "vocabulary file" per tool.

3. **The chord nudge (§1.5) is the only adaptive behaviour in the product**, and
   it contradicts the spirit of the at-most-one-hint rule. Keep it, or delete it
   and rely purely on the palette's chord column? Leaning keep, but it should
   survive a real user test before shipping, not a design argument.

4. **tmux's `prefix2`.** tmux supports two prefixes; omt has one `leader`. The
   importer currently reports the second as unmapped. Should `leader` become
   `leader = ["ctrl-b", "ctrl-a"]`? It is cheap in the resolver (two trie roots)
   and complicates every message that prints "the leader". Undecided.

5. **`bind -n` (unprefixed) bindings are the ones tmux users care most about and
   the ones most likely to be undeliverable or to shadow a critical inner key.**
   §5.3 asks per-binding (`[k/s/d]`). For a config with 15 of them that is 15
   prompts. Batch them ("14 unprefixed bindings, 3 with warnings — review just
   those?") or accept the friction? Currently per-binding, which is probably
   wrong at scale.

6. **Workflow suggestion from repeated command sequences** (§6, deliberately
   shipped as nothing). Genuinely valuable and genuinely a violation of §1.2.
   The compromise might be a *passive* surface — the palette ranks a detected
   sequence, with no notification. Unresolved.

7. **Should `omt setup` run automatically on first launch when stdin is a TTY
   and no config exists?** Currently no: the first screen is a shell. But a
   large fraction of users will never type `omt setup`, and will therefore run
   omt permanently at the heuristic floor with no idea that the flagship
   *question-card* path — a card raised on the phone while the agent's own TUI
   keeps running at the desk — needed a hook. (What may be claimed about that
   path, and in what words, is
   [D9](decisions.md#d9--positioning-what-omt-may-and-may-not-claim)'s table,
   not this document's.) A middle option — the status bar carrying a
   persistent `setup` cell (state, not a hint) until setup has been run or
   declined — is probably right and is not specified above. **Most consequential
   open question in this document.**

8. **terminfo `Ms` (OSC 52) is claimed unconditionally in §7.2** even though the
   *outer* terminal may not support it and omt cannot detect success
   ([09 §3.1](09-ssh-and-media.md)). Claiming a capability whose failure is
   silent is defensible (the fallback is a diagnosed message) but it is exactly
   the kind of small lie §7.4 says omt does not tell. Should `Ms` be dropped
   from the entry and clipboard writes be handled only through omt's own
   `⟨leader⟩ y`?

9. **Installing terminfo on a remote host over `ssh` (§7.3 tier 1)** requires
   detecting the ssh invocation and writing to someone else's `~/.terminfo`.
   [04 §7.3](04-terminal-core.md#73-propagation-into-subshells-and-over-ssh)
   already does this for shell integration, so the mechanism exists — but doing
   it for *two* things doubles the prompts on the first ssh to a host. They
   should probably be one combined offer, which means one of the two documents
   has to own the combined flow.

10. **`omt-256color` vs. contributing an `omt` entry to ncurses.** Long term, an
    upstream entry means no installation step at all. It also means a multi-year
    lag and an entry we cannot revise. Recorded as the obvious next question
    once the capability set stops moving.

11. **The nested-tmux warning is once *per install* (§5.4).** A user who tries
    tmux-in-omt once in March and again in September will not see the
    explanation the second time and may have forgotten it. Once per install is
    the correct default per §1.2 but this specific message is diagnostic rather
    than promotional; per-90-days may be the better rule. Unresolved.

12. **What happens to the status bar on a 20-column pane?** §2.1 specifies a
    degradation order down to `omt 3● ⌃b?` but at some width the honest answer
    is to hide the bar entirely rather than render six illegible characters.
    Where that threshold sits is untested.

13. **Zellij migration is specified thinly and deliberately** (§5.1). If zellij
    adoption among agent-CLI users is higher than assumed, the KDL layout
    importer and a `zellij` keymap deserve the same depth tmux gets here, and
    the modal-input refusal will be re-litigated. Worth measuring before
    investing.
