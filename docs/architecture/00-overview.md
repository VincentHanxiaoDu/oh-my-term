# oh-my-term — Architecture Overview

`omt` is a terminal multiplexer that understands the coding agents running
inside it, and exposes every one of its capabilities identically to a native
TUI, a public API, and a mobile-optimized web client.

This document is the entry point. It states the problem, the shape of the
solution, and the invariants every other document and every change must respect.

- [Decision log](decisions.md) — binding decisions; overrides anything that contradicts it
- [01 — Principles and invariants](01-principles.md)
- [02 — Crate map](02-crate-map.md)
- [03 — Capability catalog and surface parity](03-capability-catalog.md)
- [04 — Terminal core](04-terminal-core.md)
- [05 — Session model](05-session-model.md)
- [06 — Agent layer](06-agent-layer.md)
- [07 — Remote protocol, transport and auth](07-remote-protocol.md)
- [08 — Web client](08-web-client.md)
- [09 — SSH, clipboard and image bridge](09-ssh-and-media.md)
- [10 — Configuration](10-configuration.md)
- [11 — Plugin system](11-plugins.md)
- [12 — Concurrency and collaboration](12-collaboration.md)
- [13 — Security model](13-security.md)
- [14 — Licensing and provenance](14-licensing.md)
- [15 — Workspace explorer: files and version control](15-workspace-explorer.md)
- [16 — Input, keymap and conflict resolution](16-input-and-keymap.md) — what a key means, in which context, and who wins when omt, the terminal and the inner program all want the same chord
- [17 — Panes and layout](17-panes-and-layout.md) — how a workspace's panes are arranged, resized, navigated, degraded onto a phone, serialized and restored
- [18 — Semantic open](18-semantic-open.md) — recognizing pointers in terminal output (paths, URLs, commits, stack frames), resolving them to targets, and acting on them
- [19 — Onboarding, discoverability, migration and TERM](19-onboarding.md) — what happens the first time someone runs `omt`, and how a ten-year tmux user stops resenting it
- [20 — Recall, timeline and usage](20-recall-and-usage.md) — finding past work across sessions and machines, the session digest, and normalized agent usage/cost
- [21 — Data lifecycle](21-data-lifecycle.md) — what omt writes, for how long, and how to get it back out or destroy it
- [22 — Operations](22-operations.md) — running omt as a service, diagnosing it, upgrading it, and driving it from CI with no TTY
- [23 — Identity and devices](23-identity-and-devices.md) — durable answers, without a cloud, to *which instances do I have?* and *which devices may reach them?*
- [Glossary](glossary.md) — the canonical name for every shared type and concept

New here? [README.md](README.md) is the reading guide — which of these to read,
in what order, for what you are trying to do.

### Design notes

Cross-cutting notes that validate the architecture rather than specify part of
it:

- [`design/scenarios.md`](../design/scenarios.md) — the user-facing scenario and
  requirement catalogue the architecture is checked against, including the gaps
  it does not yet describe.
- [`design/remote-continuity.md`](../design/remote-continuity.md) — working
  across devices as one continuous session rather than as a second screen.

Background research that informed these decisions lives in
[`docs/research/`](../research/): [another tool](../research/another tool.md),
[another terminal](../research/another terminal.md), [iTerm2](../research/iterm2.md),
[coding-agent CLIs](../research/agent-clis.md).

---

## 1. The problem

Coding agents moved into the terminal, and the terminal did not move with them.

Three gaps follow from that:

1. **The terminal does not know what the agent is doing.** A pane that is
   thinking, a pane that finished twenty minutes ago, and a pane that has been
   blocked on a permission prompt since then all look the same from the outside.
   With ten panes across four projects, the human becomes the scheduler.

2. **Structured agent interactions are trapped in ANSI.** Claude Code's
   `AskUserQuestion` is a real data structure — a list of questions, each with a
   header, a multi-select flag, and labelled options with descriptions. By the
   time it reaches the screen it is a box drawn out of Unicode, answerable only
   by a human at that keyboard. It cannot be answered from a phone, forwarded,
   or automated.

3. **Leaving the desk means leaving the work.** Agents run for minutes at a
   time and block on questions that take five seconds to answer. Existing
   remote options are either a raw terminal on a phone screen (unusable) or a
   separate product with its own agent runtime (not your CLI, not your config).

`omt` addresses all three without replacing anything the user already runs.

## 2. The solution in one paragraph

`omt` runs your agent CLIs in real PTYs — unmodified, with their own TUI, their
own slash commands, their own keybindings, **observed from outside rather than
instrumented from within** ([D9](decisions.md#d9--positioning-what-omt-may-and-may-not-claim)),
with a `native` (ACP) mode available as a deliberate opt-in
([D8](decisions.md#d8--two-session-modes-pty-default-and-native-acp)). Alongside the byte stream it runs an
**observation layer**: process/env inspection, agent-native hooks, agent-native
protocols (ACP, app-server, REST/SSE), transcript tailing, and PTY heuristics —
merged into one normalized `AgentEvent` stream per session. Anything structured
enough to render — a question card, a permission request, a plan, a message
queue — becomes a first-class `Interaction` that can be answered from the TUI,
from the API, or from a phone, and the answer is injected back through the same
channel the agent expects. Every capability is declared once in a
**capability catalog**; the TUI, the API and the web client are three renderers
over the same catalog, and CI fails if they drift apart.

## 3. Shape of the system

```
                        ┌──────────────────────────────────────────┐
                        │              omt daemon                  │
                        │                                          │
  PTY ◄──── bytes ─────►│  terminal core   session/workspace tree  │
  (agent CLI, shell)    │  (VT parser,     (workspace → session →  │
       ▲                │   grid, blocks)   pane, layout, history) │
       │                │        │                    │            │
       │                │        └────────┬───────────┘            │
  hooks / OSC / env     │                 ▼                        │
       │                │        capability catalog                │
       ├───────────────►│     (commands + queries + events)        │
       │                │        ▲        ▲         ▲              │
  agent observers       │        │        │         │              │
  (hooks, ACP,          │      TUI    API server   plugin host     │
   transcripts,         │   (ratatui)   (WS/HTTP)   (wasm/proc)    │
   process, heuristics) │                 │                        │
                        └─────────────────┼────────────────────────┘
                                          │
                        ┌─────────────────┴────────────────────────┐
                        │  web client (TS + xterm.js, mobile-first)│
                        │  attaches to N omt instances on N devices│
                        └──────────────────────────────────────────┘
```

Two hard rules are visible in that diagram and enforced everywhere:

- **The TUI is a client.** It sits beside the API server, not above it. It has
  no privileged path into the core.
- **The capability catalog is the only door.** Every mutation and every read
  goes through it, which is what makes parity mechanically checkable.

## 4. Domain model

```
Instance          one omt daemon on one machine
 └─ Workspace     a project root (usually a git repo or worktree)
     └─ Session   one logical terminal (a PTY + its scrollback + its agent)
         └─ Pane  a viewport onto a session inside a layout
```

- **Workspace** is identified by its canonical path. Multiple sessions in the
  same directory is the normal case, not an edge case — that is how people run
  an agent next to a dev server next to a shell.
- **Session** owns the PTY and the terminal state. It survives detach,
  reattach, client disconnect, and daemon restart (via replayable state).
- **Pane** is presentation only. Layouts are a BSP tree; a session may be shown
  in several panes, on several clients, at once.
- **Agent** is not a separate object — it is an observed property of a session,
  because a session's foreground process changes over time (shell → agent →
  shell). A session therefore has an *agent binding* with a lifetime.

## 5. The agent observation pipeline

Six independent sources, one merged state machine per session. Sources are
ranked by confidence; a higher-confidence source wins inside a staleness
window, and lower ones only fill gaps.

This tiered source model applies to `pty` sessions only
([D8](decisions.md#d8--two-session-modes-pty-default-and-native-acp)); a
`native` session is driven entirely by ACP, where the whole stream is already
typed and there is nothing to infer.

Tier numbers are the `Tier` enum in
[06 §3](06-agent-layer.md#3-source-model) — **higher is more authoritative**,
and a lower tier may never contradict a live higher one.

| Tier | Source | What it gives | Coverage |
|---|---|---|---|
| 5 `Protocol` | agent-native protocol (ACP / app-server / REST+SSE) | full structured stream | opencode, Gemini, Goose, Qwen, Codex, Amp |
| 4 `Hook` | agent-native hooks | precise lifecycle, tool calls, **deferred approvals** | Claude Code, Codex, Gemini, Qwen, Cursor, opencode |
| 3 `Transcript` | transcript tailing | retroactive truth, works with no cooperation | Claude Code, Codex, opencode, Gemini, Aider |
| 2 `Marker` | omt-injected env + OSC backchannel | correlation, out-of-band signals | all |
| 1 `Process` | process + environ inspection | which agent, native session id | all |
| 0 `Heuristic` | PTY heuristics | `Busy \| Idle \| NeedsAttention` only | all, fallback |

Being the process that spawned the agent is not a tier — it is a property of the
binding that makes tiers 1 and 2 exact (known argv, injected correlation ids)
rather than inferred.

Tier 0 is deliberately capped: heuristics may never produce structured content.
Anything omt renders as a card must come from tier 3, 4 or 5, so that a locale
change or a version bump degrades attentiveness, never correctness. This is the
main departure from another tool, whose entire state model is tier 0.

The flagship mechanism is Claude Code's `PreToolUse` hook returning
`permissionDecision: "defer"` on `AskUserQuestion`: it parks the tool call,
hands omt the exact `questions` array, and lets a phone answer it. That
assumption is load-bearing and is validated by a spike before anything is built
on it — see [06 — Agent layer](06-agent-layer.md).

## 6. Terminal core

A real terminal, not a good-enough one. The core is a VT parser plus a grid
with scrollback and reflow, and on top of it a **block model**: output is
segmented into command blocks using OSC 133 semantic prompts (with a
heuristic fallback when the shell is not integrated). Blocks are what make the
mobile client usable — a phone shows a scrollable list of collapsible command
blocks, with a full terminal view one tap away.

The required escape-sequence surface, the grid/reflow design, and the
performance strategy are specified in [04 — Terminal core](04-terminal-core.md),
derived from the iTerm2 study.

## 7. Remote

One instance serves many clients; one client attaches to many instances across
many devices. The first client to attach to an instance does not become the
store — instead, **each instance is authoritative for its own sessions**, and
the web client is a federating view that aggregates instances it has
credentials for. This avoids a single point of failure and makes the Tailscale
deployment (publish one instance, add others from the phone) fall out naturally.

Auth is pluggable with three built-in methods — signed invite link, bearer
token, and username/password — plus transport-level trust when running inside a
Tailscale tailnet. See [07](07-remote-protocol.md) and [13](13-security.md).

## 8. What omt is not

- Not an agent. omt never talks to a model; it runs your CLI.
- Not a replacement for tmux inside a pane — but it will not pretend to see
  through one either (an agent inside tmux inside omt is explicitly unsupported
  for observation; the terminal still works).
- Not a hosted service. There is no omt cloud, no telemetry, and no required
  network egress.
- Not a shell. omt integrates with your shell; it does not implement one.
