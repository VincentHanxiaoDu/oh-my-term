# Web Client

The `web/` package is a TypeScript single-page application that attaches to one
or more omt instances. It is a **peer of the TUI**, not a companion to it: by
[P3](01-principles.md#p3--parity-one-capability-three-surfaces) every capability
in the [catalog](03-capability-catalog.md) is reachable from it, and the build
fails if that stops being true.

It is designed for a phone held in one hand, on a flaky LTE connection, by
someone who has ten seconds to unblock an agent. Desktop is the same
application with more room.

- [00 — Overview](00-overview.md)
- [03 — Capability catalog](03-capability-catalog.md) — the generated contract
- [07 — Remote protocol](07-remote-protocol.md) — transport, auth, resume
- [09 — SSH and media](09-ssh-and-media.md) — image upload / paste path
- [12 — Collaboration](12-collaboration.md) — writer token, presence, optimistic UI
- [13 — Security](13-security.md) — auth, roles, credential scope
- [15 — Workspace explorer](15-workspace-explorer.md) — file tree, diffs, the
  shared diff renderer
- [Decision log](decisions.md) — in particular
  [D1](decisions.md#d1--omt-adds-no-policy-layer-over-an-agents-permission-semantics)
  (no omt-side permission policy) and
  [D2](decisions.md#d2--remote-is-exactly-equivalent-to-local) (remote is
  equivalent to local)

---

## 1. Stack

**Decision: Vite + TypeScript (strict) + Solid.js + xterm.js. No component
library. Vanilla CSS with custom properties. Vitest + Playwright for tests.**

| Layer | Choice | Rationale |
|---|---|---|
| Build | Vite 5, `build.target: es2022` | Fast dev server, first-class TS, trivially emits a single static bundle that `omt-server` embeds via `rust-embed`. No SSR: there is no server to render on — an omt instance is a daemon on someone's laptop. |
| Language | TypeScript, `strict`, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes` | The parity mechanism (§2) is a *type-level* exhaustiveness check. It only works if the compiler is not lying to us. |
| Reactivity | Solid.js | Fine-grained signals, no virtual DOM, ~7 KB runtime. A terminal client is a stream of thousands of small state deltas per second; a VDOM diff per event is the wrong shape. Solid's JSX compiles to direct DOM updates, so an `AgentEvent` touching one field re-renders one text node. React's ecosystem advantage does not apply because we are not using a component library. |
| Terminal | `@xterm/xterm` + `addon-webgl`, `addon-fit`, `addon-unicode11`, `addon-web-links` | Nothing else is close. It is the reference VT renderer for the browser and it is what the server-side [`omt-term`](04-terminal-core.md) model is designed to be reproducible in. |
| Styling | Plain CSS modules + custom properties | Themes come from the instance's config (§9) as CSS custom-property values. A CSS-in-JS runtime would mean re-serializing the theme on every render for no gain. |
| State | Solid stores, one per instance, plus a federation root | §3. |
| Tests | Vitest (unit/component), Playwright (E2E against a mock instance) | §10. |

**Explicitly rejected:**

- *React/Vue/Svelte* — all workable; Solid wins on the update pattern above and
  loses only on hiring, which is not a constraint for a package this small.
- *A UI kit (MUI, shadcn, Ionic)* — every one of them fights us on the two
  screens that matter (block list, terminal). Mobile ergonomics here are
  specific enough (§8) that adopting a kit means overriding it everywhere.
- *Canvas-rendered custom terminal* — xterm.js already solved it.
- *Service-worker-based offline session cache* — the SW exists for
  installability and app-shell precache (§8.6), not for pretending we have state
  we do not. It carries no push handler ([D12](decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead)).

### 1.1 Package layout

```
web/
  src/
    generated/           # written by `cargo xtask codegen`; committed; never hand-edited
      capabilities.ts    # CapabilityName union, input/output types, Handler<K>
      events.ts          # Event envelope, AgentEvent payloads, InteractionKind
      config-schema.ts   # JSON Schema types for the config editor
    proto/               # hand-written transport: framing, resume, backoff
    federation/          # InstanceClient, InstanceRegistry, unified selectors
    capabilities/
      registry.ts        # the exhaustive handlers map (§2)
      handlers/*.ts
    views/
      blocks/            # block view (mobile default)
      terminal/          # xterm.js view + virtual key bar
      interactions/      # AskUserQuestion, permission, plan, elicitation cards
      dashboard/         # cross-instance agent session dashboard
      config/            # generated config forms
    voice/               # MediaRecorder capture, streaming, transcript UI
    ui/                  # primitives: Sheet, Chip, Card, KeyCap, Diff
    platform/            # safe-area, viewport, haptics, push, wake-lock
  tests/
  mock-instance/         # a TS omt instance simulator (§10.2)
```

---

## 2. Consuming generated artifacts

`web/src/generated/` is produced by the same `xtask` that emits JSON Schema and
the CLI tree ([03 §1](03-capability-catalog.md#1-the-idea)). It is committed, and
CI regenerates and diffs it. Hand-editing it is a build failure.

### 2.1 What codegen emits

```ts
// generated/capabilities.ts  (excerpt)
export type CapabilityName =
  | "session.send_text"
  | "session.blocks.list"
  | "interaction.resolve"
  | "media.image.paste"
  | "agent.commands.list"
  /* … */;

export interface CapabilityIO {
  "session.send_text": {
    input:  { session: SessionId; text: string; submit: boolean };
    output: { seq: Seq };
    role: "operator";
    kind: "command";
    effects: readonly ("writes_pty")[];
    since: "1.0";
  };
  "interaction.resolve": {
    input:  { interaction: InteractionId; response: InteractionResponse };
    output: { resolved_by: Actor; seq: Seq };
    role: "operator";
    kind: "command";
    effects: readonly [];              // resolving is not itself a side effect on omt
    since: "1.0";
  };
  /* … one entry per capability … */
}

export type Input<K extends CapabilityName>  = CapabilityIO[K]["input"];
export type Output<K extends CapabilityName> = CapabilityIO[K]["output"];
export type Effects<K extends CapabilityName> = CapabilityIO[K]["effects"];

export const PARITY_EXEMPT = ["instance.shutdown", "debug.dump_grid"] as const;
export type ExemptName = (typeof PARITY_EXEMPT)[number];
```

Events are emitted as a discriminated union keyed on `payload.type`, generated
from `omt-events` (the envelope is [02](02-crate-map.md#omt-events)'s `Event`,
the interaction types are [06 §5](06-agent-layer.md#5-interactions--the-flagship-path)'s).
Field names are the serde names — `snake_case`, no camelCase renaming, so the
JSON in a devtools panel matches the Rust type and the docs.

```ts
// generated/events.ts (excerpt)
// The one closed set, owned by [07 §3.7.3](07-remote-protocol.md#373-the-source-vocabulary--one-closed-set):
// the six agent-observation tiers, plus three producers that have no tier.
// `pty` was this document's rename of `heuristic` and is gone; `workspace_fs`
// is an `EventKind`, not a source, and `system` is now `core`.
export type EventSourceTag =
  | "heuristic" | "process" | "marker" | "transcript" | "hook" | "protocol"
  | "core" | "fs" | "plugin";

export interface EventEnvelope<P = AgentPayload | TermPayload | TreePayload | WorkspaceFsPayload> {
  instance: InstanceId;
  session: SessionId | null;
  workspace: WorkspaceId | null;      // set for workspace-scoped events (15 §4.6)
  seq: Seq; ts: string;
  source: EventSourceTag;
  caused_by: RequestId | null;        // 12 §5.3
  payload: P;
}

export type InteractionKind =
  | { type: "choice"; questions: ChoiceQuestion[] }
  | { type: "permission"; tool: string; input: unknown;
      options: PermissionOption[]; diff: FileDiff | null; command: string | null }
  | { type: "text"; prompt: string; placeholder: string | null; multiline: boolean }
  | { type: "plan_review"; plan: string };

export interface ChoiceQuestion {
  question: string;
  header: string;              // ~12 char tab label
  multi_select: boolean;
  options: { label: string; description: string }[];
  allow_free_text: boolean;
}
```

### 2.2 The exhaustiveness mechanism

This is the concrete realization of artifact #3 in the
[parity contract](03-capability-catalog.md#5-the-parity-contract).

```ts
// src/capabilities/registry.ts
import type { CapabilityName, Input, Output, ExemptName } from "../generated/capabilities";

export interface HandlerCtx { instance: InstanceClient; ui: UiBus; }

export type Handler<K extends CapabilityName> = {
  /** Where this capability is reachable from in the UI. Used by the E2E parity test. */
  surface: SurfaceHint;
  /** Optional client-side pre-flight: confirmation gestures, optimistic updates. */
  invoke(ctx: HandlerCtx, input: Input<K>): Promise<Output<K>>;
};

type Handled = Exclude<CapabilityName, ExemptName>;

export const handlers: { [K in Handled]: Handler<K> } = {
  "session.send_text":    sendText,
  "session.blocks.list":  blocksList,
  "interaction.resolve":  interactionResolve,
  /* … */
};
```

Because `handlers` is annotated as a **mapped type over the full union**, adding
`capability!{ name = "session.bookmark", … }` in Rust regenerates
`CapabilityName`, and `tsc` immediately reports
`Property 'session.bookmark' is missing in type '...'`. The web build fails.
That is the desired failure mode: the cheapest way to ship the capability is to
write the handler.

Two supporting checks:

1. **No stray keys.** The mapped type also rejects a handler for a capability
   that no longer exists, so deleting a Rust capability fails the web build
   until the dead handler is removed.
2. **Surface reachability.** `surface: SurfaceHint` is not decorative. It is one
   of `{ kind: "action", menu: string } | { kind: "gesture", … } | { kind:
   "implicit", by: CapabilityName }`, and the E2E parity test (§10.3) drives
   every non-`implicit` handler through the real UI. `implicit` requires naming
   the capability that covers it (e.g. `session.write_bytes` is implicit under
   `session.send_text`), which keeps the escape hatch auditable.

### 2.3 Effects drive UI policy, not just audit

`Effects<K>` is consumed at runtime by a generic wrapper:

```ts
export async function call<K extends CapabilityName>(
  ctx: HandlerCtx, name: K, input: Input<K>,
): Promise<Output<K>> {
  const meta = CAPABILITY_META[name];
  if (meta.effects.includes("destructive")) {
    await ctx.ui.confirm({ title: meta.title, detail: describe(name, input) });
  }
  if (meta.role === "admin" && ctx.instance.role !== "admin") throw new Unauthorized(name);
  return ctx.instance.rpc(name, input);
}
```

So a capability declared `DESTRUCTIVE` in Rust automatically gets a confirm
sheet on the phone without anyone writing mobile code for it.

---

## 3. Multi-instance federation

The web client is **the** federating layer. Instances do not know about each
other beyond `instance.peers.*`; the client holds N credentials and presents one
world. This follows the overview's rule that each instance is authoritative for
its own sessions.

### 3.1 Model

```ts
export type InstanceStatus =
  | { state: "disconnected" }
  | { state: "connecting"; attempt: number; nextRetryMs: number }
  | { state: "handshaking" }
  | { state: "connected"; sinceSeqPerSession: Map<SessionId, Seq>; rttMs: number }
  | { state: "degraded"; reason: "resync" | "slow" }   // 07 §5.2: the message is `Resync`
  | { state: "auth_failed"; detail: string }
  | { state: "version_incompatible"; instanceCatalog: string; clientCatalog: string };

export interface InstanceClient {
  id: InstanceId;
  label: string;                       // user-editable; defaults to instance.info.hostname
  origin: string;                      // wss://host:port
  role: Role;
  status: Accessor<InstanceStatus>;
  /** Capability names this instance actually reports at handshake. */
  available: ReadonlySet<CapabilityName>;
  rpc<K extends CapabilityName>(name: K, input: Input<K>): Promise<Output<K>>;
  events: EventStream;
}
```

Credentials live in IndexedDB (not `localStorage` — we want structured storage
and a larger quota), encrypted at rest with a key derived from a device
passphrase when the user opts into it. Tokens are never placed in a URL after
first use; the invite-link flow immediately exchanges the link for a stored
token and `history.replaceState`s the URL clean.

### 3.2 Adding an instance

Three methods, matching the three built-in `AuthBackend`s
([07](07-remote-protocol.md), [13](13-security.md)):

| Method | Flow | Mobile affordance |
|---|---|---|
| **Signed invite link** | `https://<host>:<port>/#/join?i=<token>` ([13 §3.2](13-security.md#32-invite-links-the-primary-onboarding-path)); the token lives in the **fragment**, so it never reaches the server in a request line or an access log. The client exchanges it with a `join.exchange` message over the WebSocket ([07 §1.3](07-remote-protocol.md#13-adding-an-instance)), receiving a device-bound credential. | The primary path. On desktop omt prints a QR; the phone camera opens the link. "Add instance" also has an in-app QR scanner (`BarcodeDetector`, falling back to a WASM decoder). |
| **Bearer token** | Paste a token minted by `omt token create --role operator` ([13 §3.3](13-security.md#33-bearer-tokens)). | A single monospace field with paste-detect and a scan button. |
| **User + password** | argon2id verified server-side; the response is a token with the same shape as the other two, so nothing downstream knows the difference. | Standard form; `autocomplete="username"` / `"current-password"` so password managers work. |

Tailnet-identity instances need no credential at all: the client attempts an
unauthenticated handshake first, and if the instance reports
`auth: { method: "tailnet", identity: "user@example.com" }` it is added
directly.

After the handshake the client stores `available` — the instance's actual
catalog — and its catalog version.

### 3.3 The unified session list

The home screen is one list across all instances, not a per-instance drill-down.

```ts
interface UnifiedSession {
  instance: InstanceId; instanceLabel: string;
  session: SessionId; title: string;
  // SessionMode, owned by 05 §1. Serde names of the Rust enum. D8.
  mode: "pty" | "native";
  workspace: { path: string; name: string; gitBranch: string | null };
  agent: { kind: AgentKind; state: AgentState; model: string | null } | null;
  openInteractions: number;
  queuedMessages: number;
  lastActivity: string;
  writer: { client: ClientId; label: string } | null;   // who is driving
  reachable: boolean;                                    // instance connected?
}
```

Sort key, in order: (1) open interactions descending, (2) `agent.state ===
`"blocked"`, (3) `"working"`, (4) recency. `AgentState` variant names are
owned by [06 §4](06-agent-layer.md#4-merging-confidence-tiers-not-voting).
This is the same ordering the
dashboard uses (§6) — the home screen *is* the dashboard, filtered to sessions.

A `native` session carries a visible `native` label on its row **and** in the
session header —
[D8](decisions.md#d8--two-session-modes-pty-default-and-native-acp)
requires the mode to be visible on every surface, because the user must never be
in doubt about which product they are talking to (a `native` Claude session is
the Agent SDK, not Claude Code). `pty` is the default and is not labelled.

Sessions on a disconnected instance stay in the list, greyed, with their last
known state and a relative timestamp. Disappearing rows on a subway is worse
than stale rows.

### 3.4 Graceful degradation across catalog versions

Per [03 §7](03-capability-catalog.md#7-versioning) the client renders the
intersection. Concretely:

```ts
export function guard<K extends CapabilityName>(inst: InstanceClient, name: K): Availability {
  if (inst.available.has(name)) return { ok: true };
  const meta = CAPABILITY_META[name];
  return { ok: false, reason: "unsupported", since: meta.since, instance: inst.label };
}
```

Rules:

- A control bound to an unavailable capability renders **disabled with a
  reason**, never hidden. Hiding it makes the phone look like a different
  product than the laptop, which is the exact confusion P3 exists to prevent.
- Tapping a disabled control shows: *"`session.blocks.rerun` needs omt ≥ 1.3 on
  `mini` (running 1.1). Update that instance."*
- Cross-instance aggregate views degrade per-row, not per-view: if one instance
  lacks `agent.queue.list`, that instance's rows show "—" for queue depth and
  everything else still works.
- If the instance's catalog has a capability the client does **not** know, it is
  listed under Settings → Instance → Unsupported, with its generated docs link.
  An older phone PWA against a newer daemon is a normal state.

---

## 4. View modes

Every session can be viewed several ways, toggled by a persistent segmented
control in the header. The choice is per-session and remembered per device.
Each is a complete surface; none is a preview of another.

A `pty` session has **three** surfaces, not two, per
[D14](decisions.md#d14--agent-sessions-get-a-transcript-surface-blocks-are-for-shell-work):

| Surface | For | Source | §|
|---|---|---|---|
| **Block view** | ordinary shell work | OSC 133 segmentation ([04 §6](04-terminal-core.md#6-the-block-model)) | §4.2 |
| **Transcript view** | agent sessions | the merged agent event stream ([06](06-agent-layer.md)), available whenever the binding has a tier ≥ Transcript source | §4.4 |
| **Terminal view** | always available | the grid | §4.3 |

**The default selection rule on mobile is by session kind, not by viewport
alone.** On viewports narrower than 600 CSS px:

- a session with **no agent binding** → **block view**;
- a session with an **agent binding at tier ≥ Transcript source** → **transcript
  view**;
- a session with an agent binding **below** that tier → **terminal view**, with
  the segmented control showing block and transcript as unavailable and saying
  why (see §4.4);
- **terminal view is always exactly one tap away** from any of the above.

Above 600 CSS px terminal view remains the default, unchanged.

The old rule — "block view on mobile" — was wrong for the primary use case. An
agent session produces no OSC 133 at all, so [04 §6.4](04-terminal-core.md#64-the-fallback-heuristic--no-shell-integration)
now *suppresses* segmentation for it rather than emitting one unbounded block of
flattened redraw output. Block view for an agent session is therefore not a
worse option, it is **not an option**, and the control renders it disabled.

**Except in `native` mode.** A `native` session
([D8](decisions.md#d8--two-session-modes-pty-default-and-native-acp),
[06 §2.1](06-agent-layer.md#21-session-modes)) has no PTY and therefore **no
terminal view**; the segmented control is absent. Its two surfaces are the
structured transcript and the interaction cards, both of which omt renders
itself. That is D8's honest selling point — in `native` mode omt owns the
rendering end to end, so the block view, the cards and the whole mobile
experience are strictly better than anything derived from observing a TUI — and
the honest cost is stated next to it, on the same screen: the user is not running
their own CLI.

### 4.1 Why several

A phone is roughly 40 columns at a readable font size. Almost every useful
terminal output — a test run, a `git status`, an agent's tool output — is
designed for 80–120. Reflowing it to 40 columns produces something technically
correct and practically unreadable, and pinch-zooming a VT grid means
horizontal panning through a wall of monospace.

The block model ([04](04-terminal-core.md), derived from
[another terminal's BLOCKS](../research/another terminal.md)) gives us the escape: output is already
segmented into `(command, output, exit code, timings, cwd, git branch)` records
via OSC 133. A list of collapsible cards is a *native mobile shape*. You scroll
it with a thumb, you expand what you care about, and you never pan horizontally
because each block body is its own scroll container.

Terminal view exists because neither structured view can represent an alt-screen
TUI (vim, `htop`, the agent's own interactive card UI), and because sometimes the
answer really is "I need to see the actual screen".

Transcript view exists because the block model's escape does **not** generalise
to agent sessions, and cannot be made to. There is no shell in the loop, so no
OSC 133 arrives and there are no command boundaries to segment on — and Claude
Code compounds it by drawing inline via Ink rather than on the alt screen, so
even the alt-screen suspension rule does not fire. What is available instead is
better than blocks anyway: the tier 3/4/5 sources carry full assistant messages,
tool calls and tool results as typed events. That is a *more* structured input
than a byte grid, and it was already being rendered — just only for `native`
sessions.

### 4.2 Block view

Backed by `session.blocks.list` / `session.blocks.get`, and live-updated by
block events on the subscription.

Per [D9](decisions.md#d9--positioning-what-omt-may-and-may-not-claim) the block
model is omt's **strongest and specifically unclaimed idea** — no shipping
product combines a real VT parser, panes and a mobile client, and OSC 133
segmentation is what resolves the raw-PTY-versus-cards split the whole category
is stuck on. It is the differentiator, not a mobile-rendering convenience, and
this section should be read as product surface rather than an adaptation layer.

```ts
interface BlockSummary {
  id: BlockId; index: number;
  command: string | null;          // null for background output
  // BlockState, owned by 04 §6.3. Serde names of the Rust enum.
  state: "at_prompt" | "submitted" | "running" | "finished"
       | "no_execution" | "background" | "truncated";
  // BlockOrigin, owned by 04 §6.2. "heuristic" means no shell integration:
  // the UI must render it as unstructured output and must not claim an exit code.
  origin: "osc133" | "heuristic" | "injected";
  exit: { code: number } | { signal: number } | null;
  failed: boolean;                 // NOT exit.code !== 0 — excludes 130/141, see below
  started_at: string | null; finished_at: string | null;
  cwd: string | null; git_branch: string | null;
  output_bytes: number; output_lines: number;
  truncated: boolean;              // body must be fetched with blocks.get
  // Attribution, owned by 04 §6.2.
  attribution:
    | { type: "human"; client: ClientId | null }
    | { type: "agent"; kind: AgentKind; run_id: string | null }
    | { type: "unknown" };
}
```

`failed` is computed server-side by `Block::failed()`
([04 §6.2](04-terminal-core.md#62-what-a-block-owns)) and deliberately excludes
exit 130 (Ctrl-C) and 141 (SIGPIPE). Painting a Ctrl-C'd block red is a papercut
we are not reproducing.

**Card anatomy** (top to bottom):

1. **Header row** — a `$`/agent glyph, the command in monospace (one line,
   `text-overflow: ellipsis`, tap to expand full), and on the right an
   **exit-code chip**: green check for 0, red `✗ 1` for failure, grey `⏵` with a
   spinner for `executing`, amber `⎋ 130` for interrupted. The chip is a real
   button: tapping it copies `echo $?`-worthy context (command, exit, duration).
2. **Meta row** (only when it differs from the previous block) — `~/proj ·
   main · 2.4s · 14:03`. Suppressing repeats keeps the list scannable.
3. **Body** — collapsed by default when `outputLines > 12` or the block
   succeeded and is older than the current turn; expanded when the block failed
   or is running. The body is an xterm.js-free renderer over `StyledLine`
   (`{ text, spans: [{ start, len, fg, bg, flags, link? }] }`) — the structured
   encoding from [04 §4.4(b)](04-terminal-core.md#44-the-web-mapping-xtermjs),
   also returned by `session.scrollback.get`. No raw bytes, so there is no
   second VT parser in the browser. Long lines get `overflow-x: auto` inside the
   card.
4. **Action row** (revealed on tap-and-hold, or always on desktop hover) —
   **Copy** (output / command / both), **Re-run** (`session.blocks.rerun`),
   **Share** (§4.2.1), **Bookmark**, **Filter lines** (a regex box that filters
   the body in place), **Open in terminal view** (scrolls the VT view to that
   block's start line).

**Agent attribution.** When a block was produced by an agent's tool call rather
than a human keystroke, the header shows the agent glyph plus the tool name
(`⌁ claude · Bash`), and the card carries a left border in the agent's accent
color. Attribution comes from correlating the block's start timestamp and PTY
writer with the `ToolCall` event stream; when correlation is ambiguous the
attribution is simply omitted rather than guessed. This is the same discipline
as [P4](01-principles.md#p4--native-semantics-observe-never-re-implement):
structured claims need structured evidence.

**Virtualization.** The list is windowed (a `<div>` per block with measured
heights cached by `BlockId`) because a long-lived session has thousands of
blocks. Height is stable per block once measured, so no reflow thrash on scroll.

**Live tailing.** While a block is `executing`, its body auto-scrolls and the
list pins to the bottom. Any upward scroll gesture unpins and shows a
"↓ jump to live" pill.

#### 4.2.1 Share

`Share` serializes the block to a self-contained payload (command, output as
styled cells, exit code, timings, redacted cwd) and offers: copy as Markdown,
copy as an ANSI-preserving text blob, or `navigator.share()` on mobile. There is
**no** omt cloud — sharing produces bytes, never a hosted URL
([00 §8](00-overview.md#8-what-omt-is-not)).

### 4.3 Terminal view

xterm.js with the WebGL renderer, `unicode11`, and a `SerializeAddon` used only
for tests.

- **Input.** Keystrokes go through `session.write_bytes`; pasted text goes
  through `session.send_text` (which applies bracketed-paste and chunk pacing
  server-side — the pacing logic lives in `omt-term`, not duplicated here).
  **This path is for human typing and pasting only.** A synthetic answer to an
  agent card never goes through it — see §5.1.2.
- **Writer token.** The view is read-only until the client holds the writer
  token ([12](12-collaboration.md)). The header shows *"`laptop` is driving —
  tap to take over"*; takeover is explicit and broadcast.
- **Resume.** On reconnect, the client sends `since_seq`; the instance replays
  or answers `Resync` ([07 §5.2](07-remote-protocol.md#52-replay-window)), in
  which case the client fetches a scrollback
  snapshot and re-seeds the emulator. Detail in
  [07](07-remote-protocol.md).

#### 4.3.1 Virtual key bar

A single row, `env(safe-area-inset-bottom)`-padded, docked above the software
keyboard:

```
[Esc] [Tab] [Ctrl] [Alt] [←][↑][↓][→] [⌫] [⏎]   ⋯
```

- `Ctrl` and `Alt` are **sticky modifiers**: tap once for the next key, double
  tap to lock (a filled state shows the lock). This avoids chording on a
  touchscreen, which is impossible one-handed.
- `⋯` opens a second row: `Ctrl-C`, `Ctrl-D`, `Ctrl-Z`, `Ctrl-R`, `|`, `~`,
  `-`, `/`, `PgUp`, `PgDn`, `Home`, `End`, F-keys.
- The bar is **configurable** and reads its layout from the instance config
  (`web.key_bar.rows`), so it is one schema-driven form
  ([P7](01-principles.md#p7--configuration-is-data-and-errors-are-precise)),
  not hard-coded.
- Every key is ≥ 44×44 CSS px with 8 px gutters (§8.1).
- Long-press on an arrow key auto-repeats at 40 ms after a 400 ms delay,
  matching typical key-repeat feel.

#### 4.3.2 Pinch-zoom and viewport negotiation

Pinch changes **font size**, not CSS transform scale — scaling a WebGL glyph
atlas produces mush. `FitAddon` then recomputes `(cols, rows)`.

Viewport negotiation is specified once, in
[07 §4.3](07-remote-protocol.md#43-the-resize-problem), and this document does
not restate it, in
[17 §3.4](17-panes-and-layout.md#34-the-pty-size-question-which-per-client-layout-does-not-solve)'s
naming, which supersedes 07's `SizeOwner`: **the session has one authoritative
`(cols, rows)`, resolved by its `SizePolicy` — `Driver` (the writer's view) by
default, `Pinned` when a user pins it, or `Smallest` when opted in.** A web
client attaches as a `Participation::Observer` by default and becomes a
`Participant` only by explicitly choosing the full terminal.
Every client reports its viewport on `TermAttach`/`TermResize`;
`request_authoritative: true` is the explicit ask and is refused with
`precondition_failed` unless the client holds the writer token.

What is *this* document's business is how a non-authoritative client renders:

```ts
// Client-side rendering mode. Derived from the server's ViewportPolicy —
// not a fourth negotiation scheme.
type RenderMode =
  | { mode: "follow" }        // render the authoritative grid, scaled to fit width,
                              // letterboxed vertically (07 §4.3)
  | { mode: "drive" };        // this client holds the writer token and its viewport
                              // is authoritative
```

Defaults:

- **Phone, terminal view, not the writer** → `follow`. The session stays at
  whatever the laptop set; the phone renders it scaled to fit, and pinch-zoom
  changes font size / pans rather than resizing the remote. Resizing a session
  from a phone is how you make a colleague's vim redraw into confetti.
- **Phone, terminal view, writer** → `drive`, but the client first shows the
  07 §4.3 warning when taking the token would change the size by more than 20 %,
  and offers "take input without resizing", which acquires the token with
  `keep_size: true`.
- **Desktop, writer** → `drive`.

The mode is visible in the UI (a small `↔ 120×34 · following` chip) and the
`Pinned`/`Smallest` owners are settable from the same control. When a resize
lands for a reason this client did not initiate, the server says why —
`terminal.resized { cols, rows, cause, actor }`
([12 §6 C3](12-collaboration.md#c3--a-client-resizes-while-another-is-attached)) —
and the client surfaces it rather than silently reflowing.

`session.resize` carries `{ cols, rows, pixel_width, pixel_height }`; the pixel
dimensions matter for inline images
([09 §7.2](09-ssh-and-media.md#72-inline-display-in-the-tui)).

### 4.4 Transcript view

The third surface for a `pty` session, required by
[D14](decisions.md#d14--agent-sessions-get-a-transcript-surface-blocks-are-for-shell-work).
It is a scrollable list of the agent's turn history — user prompts, assistant
messages, tool calls with their inputs and results, file changes, errors —
rendered from the **merged agent event stream** ([06 §4](06-agent-layer.md#4-merging-confidence-tiers-not-voting)),
never from the grid. Interaction cards (§5) appear inline in it, in place.

**Reuse the `native` renderer; do not design a second one.** The transcript
component built for `native` sessions ([06 §2.1](06-agent-layer.md#21-session-modes))
already consumes exactly this event shape, because the tiered `pty` sources
normalize into the same `AgentEvent` stream that ACP produces. The component
therefore takes a stream of `AgentEvent` and is agnostic to which mode produced
it; `pty` and `native` differ in *coverage* and in a provenance chip, not in
rendering. A second implementation would drift, and a phone would show two
different pictures of the same conversation.

**Availability is a function of the binding's tier**, and is reported, not
guessed:

- **tier ≥ Transcript source** → transcript view available, and it is the mobile
  default for that session.
- **below that tier** (heuristic-only agents — Aider, Amp, Crush in TUI mode;
  [06 §7.3](06-agent-layer.md#73-coverage-matrix-initial)'s "mobile surface"
  column) → the control shows transcript **disabled** with a plain reason:
  *"omt can see that this agent is busy or waiting, but not what it said — no
  structured source."* The session falls back to terminal view plus the
  busy/idle/needs-you state chip. This is the honest floor and it is shown, not
  discovered.

**Provenance is visible.** Every entry carries the tier that produced it, and
`agent.explain` is reachable from the view, so a user who sees a gap can find out
why it is there rather than assuming the agent said nothing. Entries derived from
a lower-confidence source are marked as such, exactly as heuristic blocks are in
block view (§4.2).

**Completeness is bounded and says so.** A transcript reader that starts
mid-session, or one disabled by a schema-version mismatch
([06 §9](06-agent-layer.md#9-failure-modes-and-their-handling)), yields a
transcript with a known-missing head or tail. The view renders an explicit gap
marker with the terminal view offered, and never presents a partial transcript as
a whole one — the same rule [20](20-recall-and-usage.md)'s `Coverage` applies to
recall.

---

## 5. Native rendering of agent interactions

This is the flagship — in engineering depth. The *claim* is narrower:
[D9](decisions.md#d9--positioning-what-omt-may-and-may-not-claim) rates remote
question cards as commoditized, so what this section may be sold on is answering
one of them from a phone **while the user's real interactive TUI is on screen**.
An `Interaction` is a structured request from an agent,
surfaced by tier-3/4/5 sources only
([00 §5](00-overview.md#5-the-agent-observation-pipeline)), rendered natively,
and answered through `interaction.resolve`.

### 5.1 The shared lifecycle

```
agent emits           omt normalizes            web renders          web resolves
────────────────────────────────────────────────────────────────────────────────
PreToolUse{           Event{payload:            <InteractionCard>    interaction.resolve
 AskUserQuestion}  →   Interaction{id, kind, →   picks a component →  {interaction,
 → hook reports        timeout_at, state}}       by kind.type          response, intent_id}
   and returns {}      the agent draws its                                 │
   (no defer, D11)     own card locally                                    ▼
                                                          ledger CAS: Open → Resolving
                                                                           │
                                                              responder delivers
                                                       (Native RPC, or D13's gated
                                                        PTY transaction into the
                                                        agent's own card)
                                                                           │
                                                                     → Submitted
                                                                           │
                                    ┌──────────────────────────────────────┴────────┐
                        omt observes the agent record it          window elapses with
                        (PostToolUse / transcript / tool result)   no observation
                                    ▼                                      ▼
                              → Resolved                            → Undelivered
                                    └──────── broadcast to ALL surfaces (TUI too) ───┘
```

[D11](decisions.md#d11--omt-mirrors-the-agents-own-card-it-does-not-intercept-or-replace-it)
deleted the `permissionDecision` return path: the hook **observes** and returns
`{}`, the agent's own CLI draws its own card locally, and a remote answer is
delivered to *that card*. The web client never sees the hook; what changed for
it is that `Resolved` is no longer the state a successful resolve produces.

Invariants the UI must respect:

- **Resolve-once.** `interaction.resolve` is idempotent by
  `(interaction, identity_or_device, intent_id)` — a client-minted `intent_id`
  persisted in the outbox (§8.5) plus the stable identity behind the actor, per
  [D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)
  consequence 6 and [12 §4.1](12-collaboration.md#41-the-invariant). A retry
  under the same key returns the original outcome, which is what makes a flaky
  mobile network survivable; a *different* identity gets `conflict`. The old key
  `(interaction, actor, response)` broke on exactly that retry — `ActorId` is
  minted per connection, so a reconnecting phone read as a stranger overriding
  itself — and the client must not key its own dedup on the actor or the
  response either.
  A losing resolution must be surfaced, never silently corrected.
- **Five renderable states, not three.** See §5.1.1.
- **Timeouts are real.** `timeout_at` (when the agent gives one) drives a thin
  progress bar on the card's top edge. At <10 s remaining the card vibrates once
  (`navigator.vibrate(30)`) and the bar turns amber. On expiry the ledger records
  `Cancelled { Timeout }` by `Actor::System`
  ([12 §4.3](12-collaboration.md#43-timeouts)) and the card shows "timed out —
  the agent continued with its own default", because we cannot un-time-out it.
  This is the **only** non-human resolution in the system; omt never
  auto-answers ([D1](decisions.md#d1--omt-adds-no-policy-layer-over-an-agents-permission-semantics)).
- **Never synthesize.** If the interaction's `source` is `heuristic`, it is not
  an interaction and the card is not rendered. An interaction must be *observed*
  by a tier that carries the payload (`marker` and above); a guess derived from
  scraping the grid is a guess, and rendering it as a card would put words in the
  agent's mouth. (`heuristic` is 07 §3.7.3's spelling; this branch previously
  read `pty`, which was this document's own rename of the same tier and is no
  longer in the closed set — the check is unchanged, only its spelling.)
  [P4](01-principles.md#p4--native-semantics-observe-never-re-implement). In
  `native` mode the rule is vacuous rather than relaxed: every event on a
  `native` session is `protocol`-sourced, so nothing can ever fail it
  ([06 §2.1](06-agent-layer.md#21-session-modes)).

#### 5.1.1 The card's state machine

The client renders `InteractionState` ([06 §5](06-agent-layer.md#5-interactions--the-flagship-path),
transitions in [12 §4.1](12-collaboration.md#41-the-invariant)) — **seven
variants, five of them renderable as distinct card states.** An earlier draft
listed three (open, resolving, resolved-by-X), which cannot express the two
states [D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)
consequence 1 added, and §8.5 already refers to one of them. This table is the
client's single state set; §8.5 defers to it.

| State | Rendering | Copy |
|---|---|---|
| `Open` | interactive; options tappable | — |
| `Resolving` | controls disabled, spinner on the chosen option | "sending…" |
| `Submitted` | controls disabled, chosen option shown selected, a **distinct pending style** (not a success style) | **"Sent — waiting for the agent to confirm."** |
| `Resolved { by }` | settled, success style, attribution | "Answered: Postgres — by iPhone (Vincent)" |
| `Undelivered { response, reason }` | **warning** style, the preserved response text shown verbatim, and a primary **"Open terminal view"** action — never a "retry" | **"Your answer may not have reached the agent — check the terminal."** |
| `Cancelled` / `Abandoned` | settled, muted | "This question was withdrawn" / "timed out — the agent used its own default" / "Too late — the agent continued without an answer", discriminated by `detail.state` (12 §4.1) |

Three rules the component must not violate:

- **`Submitted` is not a success state and must not be styled as one.** It is
  the honest intermediate: the bytes went out, and on the `Synthetic` path a
  successful write proves nothing because the sink is a UI omt does not own.
  This is also why `interaction.resolve` is never optimistic
  ([12 §7.1](12-collaboration.md#71-what-may-be-optimistic)) — `Submitted` is a
  real server state, not a local guess.
- **`Undelivered` never offers a retry.** An injection is at-most-once; the only
  actor permitted to re-answer is a human who can see the screen. The offered
  action is the terminal view.
- A card resolved from the laptop while you were reading it **animates to its
  new state — it does not vanish.**

Cards appear in three places, driven by the same component: inline in block view
at the position they occurred, as a **bottom sheet** when they arrive while that
session is focused, and in the dashboard's "needs you" list (§6).

#### 5.1.2 Answerability is read from `deliverable`, never inferred from the kind

[D16](decisions.md#d16--remote-answering-is-per-card-type-and-the-preconditions-are-empirical)
settled that remote answering of a `pty`-mode card is **per card type**, and that
the constraints are empirical rather than derivable. Three of Claude Code's four
card types cannot be answered position-independently at all. So the web client
does **not** decide answerability. It reads one field:

```ts
// From the generated `Interaction` (06 §5.2.1 owns it):
type Deliverable =
  | { type: "native" }
  | { type: "synthetic"; requires_token: boolean }
  | { type: "none"; reason: NotDeliverableReason };

type NotDeliverableReason =
  | "not_position_independent"    // submitting needs cursor navigation
  | "index_not_derivable"         // the option's absolute index is not in the payload
  | "inferred_responder_disabled" // D3: the responder is a guess and is off
  | "no_responder";               // nothing covers this kind for this agent
```

`deliverable` is computed **once, in the daemon's normalizer**, from
`(responder, kind, agent, mode)` and D16's verified table
([06 §5.2.1](06-agent-layer.md#521-deliverable--a-field-of-interaction-computed-by-the-normalizer)).
The view layer never branches on `kind.type` to decide whether something is
answerable — that is precisely the inference D16 forbids, and it is what would
let a Claude Code version bump silently turn a correct card into a wrong one.
`kind.type` selects the *renderer*; `deliverable` gates the *controls*. Three
rules follow:

- **`{ type: "none" }` renders the card in full, read-only** — §5.1.3.
- **`{ type: "synthetic", requires_token: true }`** renders the controls, but
  the resolve is gated on the writer token
  ([12 §3.1](12-collaboration.md#31-what-it-governs)); a client that does not
  hold it shows the ordinary "`laptop` is driving — tap to take over" affordance
  on the action row rather than a disabled card.
- **`{ type: "native" }`** renders the controls ungated.

A kind whose per-option answerability differs — permissions, §5.3 — reads the
per-option detail from `PermissionOption.kind` and never widens what
`deliverable` allows.

**The synthetic write is not `session.send_text`, and this is a silent failure
if got wrong.** Claude Code enables bracketed paste (`ESC[?2004h`); a digit
wrapped in `ESC[200~ … ESC[201~` does **nothing**, while the same bare digit
resolves the option and submits (D16 §1, verified live). `session.send_text`
bracket-wraps and paces by design — correct for the prose a human types into
terminal view (§4.3), wrong for an answer. A `Synthetic` resolve therefore
**cannot be expressed as a `session.send_text` call; it is a distinct gate entry
point** ([05 §5.4](05-session-model.md#54-what-is-not-gated)), raw bytes outside
bracketed paste, one key per write. The web client never composes those bytes: it
posts an `InteractionResponse` to `interaction.resolve` and the daemon owns the
delivery. There is no path from this client to a card answer that runs through
`session.send_text`, and adding one would fail silently rather than loudly.

#### 5.1.3 The read-only "not answerable here" state

When `deliverable.type === "none"`, the card still **renders completely** — the
question, every option with its description, the diff, the command, the plan.
That is not a concession, it is the point:
[D11](decisions.md#d11--omt-mirrors-the-agents-own-card-it-does-not-intercept-or-replace-it)
mirrors the agent's own card to every surface, and knowing *what you are being
asked* while away from the desk is most of the value. What is removed is the
**answering affordance**, and only that.

The read-only presentation has four parts, and `<ReadOnlyFooter>` renders all
four for every kind:

1. **The controls are inert, not disabled-looking.** Options render as a
   **list**, not as radio rows or checkboxes: no tap targets, no selection
   affordance, no press-and-hold ring. A greyed-out button invites a tap and
   then punishes it; a plain list never promised anything. The card keeps its
   `Open` chrome — the timeout bar (§5.1.1) still runs, because the question is
   still live and may still expire.
2. **A reason, stating *why*, in the agent's terms.** Not "unavailable". The
   copy is a fixed map from `NotDeliverableReason`, written so the user learns
   something true about the mechanism rather than about omt's limits:

   | `reason` | Copy |
   |---|---|
   | `not_position_independent` | **"Answer this one at the terminal."** Submitting this card means moving the cursor to a Submit row, and omt can only send a keystroke that resolves an option outright — it cannot navigate the agent's own list without seeing where the cursor is. |
   | `index_not_derivable` | **"Answer this one at the terminal."** This prompt shows a different number of options depending on what the agent has already been allowed, and omt is not told which — so it cannot know which number means *No*. (`Esc` is offered separately, see §5.3.) |
   | `inferred_responder_disabled` | **"Answer this one at the terminal."** omt is inferring this agent's card from screen output rather than being told about it, so remote answering is off ([D3](decisions.md#d3--synthetic-input-is-bounded-by-state-dependence-not-by-tool-danger)). |
   | `no_responder` | **"Answer this one at the terminal."** omt has no channel back to this agent for this kind of question — it can watch it, not answer it. |

   The reason is a **disclosure**, collapsed to its bold first line with a "why?"
   toggle. The headline is what the user needs in two seconds; the explanation is
   there for the one time they want it, and does not push the question itself off
   a phone screen.
3. **One primary action: `Open terminal view`.** Full-width, in the footer,
   focused. It switches the surface (§4.3) and scrolls the grid to the bottom,
   because the agent's own card is on screen there and is fully answerable by a
   human who can see it. This is the same action `Undelivered` offers (§5.1.1)
   and it is deliberately the same control in the same place. On a **`native`
   session there is no terminal view** — but `native` mode has no `none`
   deliverable either (every row of D16's table is `Native` under ACP), so the
   combination does not arise; if a future agent produces one, the fallback is
   `<UnknownInteraction>`'s (§5.6).
4. **No secondary action, and never a "notify me" or "try anyway".** The honest
   set of things the user can do here is one thing.

When the card is later answered at the keyboard, it transitions through the
ordinary `Resolved { by }` path and animates in place like any other card — a
read-only card is a normal card that is watching, not a dead one.

### 5.2 `AskUserQuestion` cards — `kind.type === "choice"`

The data shape is verbatim from
[agent-clis §1.3.1](../research/agent-clis.md#131-askuserquestion--the-structured-interaction-case):

```ts
interface ChoiceQuestion {
  question: string;
  header: string;                  // short tab label, ~12 chars
  multi_select: boolean;
  options: { label: string; description: string }[];
}
// Interaction kind: { type: "choice", questions: ChoiceQuestion[] }   // 1..4 in practice
```

**Component: `<ChoiceCard>`**

```
┌────────────────────────────────────────────┐
│ ⌁ claude · botim-eclipse            2 of 3 │   ← agent + workspace, progress
├────────────────────────────────────────────┤
│ [GitHub Org] [Visibility] [Doc Language]   │   ← header chips (tabs)
├────────────────────────────────────────────┤
│ Repository visibility?                     │   ← question
│                                            │
│ ○ Private (Recommended)                    │
│   Only org members with access can see it. │   ← description, 2-line clamp
│ ○ Internal                                 │
│   Visible to all members of the enterprise.│
│ ○ Public                                   │
│   Visible to everyone on the internet.     │
│ ○ Other…                                   │   ← free-text escape (§5.2.2)
├────────────────────────────────────────────┤
│  💬 Add a comment          [ Back ] [Next] │
└────────────────────────────────────────────┘
```

Behavior:

- **Header chips are the navigation.** With `questions.length > 1` the `header`
  strings become a horizontally scrollable chip row. A chip shows a filled dot
  once its question is answered. Tapping a chip jumps to that question — the
  card is a wizard, not a scroll of everything, because a phone shows one
  question comfortably and four badly. On a wide viewport all questions render
  stacked and the chips become a sticky in-page index.
- **`multi_select: false`** → radio semantics; selecting an option advances to the
  next question after a 250 ms confirmation flash. This makes the common case
  (3 single-select questions) three taps. This is the one Claude Code card type
  D16 rates **fully** answerable: one ASCII digit resolves the option at that
  absolute index and submits in the same keystroke.
- **`multi_select: true`** → **not answerable from this surface in `pty` mode.**
  The normalizer sets `deliverable = None { NotPositionIndependent }` for a
  `Choice` where *any* question is multi-select, and the card renders read-only
  per §5.1.3: every option listed with its description, no checkboxes, no
  **Next**, and the terminal-view action in the footer. The mechanism reason is
  worth stating because it is not obvious — the digit accelerator *does* work
  here, but on a multi-select card a digit only **toggles** the row; *submitting*
  requires navigating to a Submit row, and cursor navigation is exactly what omt
  cannot do without seeing where the cursor is (D16). Half a working mechanism is
  not a working mechanism. In `native` mode the row is `Native` and the checkbox
  UI below applies unchanged.
- **When it is answerable** (`native` mode, or a non-Claude agent whose responder
  reports otherwise), `multi_select: true` → checkbox semantics with an explicit
  **Next**; no auto-advance. The component keeps this branch; `deliverable`
  decides whether it is reachable, and the component does not test
  `multi_select` to make that decision (§5.1.2).
- **Descriptions** are clamped to two lines with a "more" affordance; they are
  the whole reason this card beats reading ANSI on a phone, so they are never
  hidden entirely.
- Options are ≥ 56 px tall rows with the full row as the hit target.

#### 5.2.1 The `n`-key "add comment" affordance

Claude Code's TUI lets the user press `n` on the question card to attach a
free-text note alongside the chosen option — the answer is then "this option,
and also here is context". That affordance must exist on a phone.

**Mapping:** a persistent `💬 Add a comment` button in the card footer (not a
hidden gesture — discoverability on touch is poor and this is a feature people
will not find by accident). Tapping it opens a `<textarea>` sheet with the
selected option shown as a chip above it. The comment travels with the
resolution:

```ts
type ChoiceAnswer = {
  /** Selected option labels for this question. Length 1 unless multi_select. */
  labels: string[];
  /** Free-text entered via "Other…". Mutually exclusive with labels being non-empty. */
  other?: string;
  /** The `n`-key comment: extra context attached to a chosen option. */
  comment?: string;
};

// InteractionResponse for a choice interaction:
{ type: "choices", answers: ChoiceAnswer[] }   // one per question, index-aligned
```

Keyboard users on desktop get the literal `n` key bound to the same button, so
muscle memory transfers.

#### 5.2.2 "Other" / free text

`AskUserQuestion`'s options are a closed list, but real answers sometimes are
not. omt appends a synthetic `Other…` row to every question. Choosing it opens a
single-line (or multiline, if the question is long) input.

The `Other…` row and §5.2.1's comment button are **answering affordances**, so
§5.1.3 removes both along with the options when `deliverable.type === "none"`.
Neither is an escape hatch around the gate.

**How it is delivered back matters.** `omt-agent` renders the resolution into
the exact prose form Claude Code expects — `"<question>"="<label>"` pairs joined
with `, `, per the verified `tool_result` shape — and appends comments and
free-text as a trailing sentence:

```
Your questions have been answered: "Repository visibility?"="Private (Recommended)".
Additional context from the user: "use the org-level ruleset, not per-repo".
```

The alternative (inject via the stream-json input channel and let Claude Code
format it) is preferred where available; the web client is agnostic to which,
because it posts `ChoiceAnswer[]` and the *daemon* owns the rendering. Keeping
that formatting server-side is deliberate: one implementation, testable against
a fixture, not duplicated per surface.

#### 5.2.3 Non-Claude agents

`Interaction::Choice` is Claude-Code-only today
([agent-clis §12.3](../research/agent-clis.md#123-concrete-per-agent-mapping-abridged)).
The component is not Claude-specific — anything that produces a `choice` kind
(ACP structured elicitation, MCP elicitation, opencode's `chat.message` plugin
hook) renders identically. No agent branching in the view layer.

### 5.3 Permission / approval cards — `kind.type === "permission"`

```ts
{ type: "permission";
  tool: string;                      // "Bash" | "Edit" | "mcp__foo__bar" | …
  input: unknown;                    // the verbatim tool_input
  options: PermissionOption[];       // the agent's own option list, verbatim
  diff: FileDiff | null;             // structured diff for edit-shaped tools
  command: string | null }           // the shell command for exec-shaped tools
```

Shape owned by [06 §5](06-agent-layer.md#5-interactions--the-flagship-path);
`FileDiff` by [15 §3.2](15-workspace-explorer.md#32-vcs-model).

**Component: `<PermissionCard>`** — the anatomy varies by which of `diff` /
`command` is present, but the action row is uniform.

- **Command preview** (`command != null`): the command in a monospace block with
  shell-aware syntax highlighting. Long commands wrap rather than scroll — you
  must be able to read the whole thing before allowing it, and it is never
  ellipsized.
  There is deliberately **no risk strip and no danger badge.** omt does not
  classify an agent's tool calls
  ([D1](decisions.md#d1--omt-adds-no-policy-layer-over-an-agents-permission-semantics));
  the agent CLI owns that judgement and has already decided to ask. A strip that
  says "looks safe" on something omt failed to recognise is worse than no strip
  at all.
- **Diff preview** (`diff != null`): rendered by the **same** diff component the
  explorer uses ([15 §7.4](15-workspace-explorer.md#74-reading-a-diff-on-a-phone)) —
  structured hunks, line-number gutter, server-computed intra-line ranges,
  hunk collapsing, unified-only on narrow viewports with a desktop split
  toggle. One renderer, one palette, one mobile behaviour. Where the diff was
  reconstructed by omt rather than supplied by the agent it is marked
  `source: "computed"` (15 §8.5).
- **Generic tool input** (neither): pretty-printed JSON, redacted per
  [13 §8](13-security.md#8-secret-redaction).
- **Actions** are rendered from `options`, **in the order the agent sent them,
  with the agent's own labels** — including `allow_always` / `deny_always` when
  the agent offered them. omt does not add, remove, reorder or relabel options
  (D1), and does not withhold `allow_always` from a remote client (D2): a phone
  the owner holds is the laptop keyboard. Where the agent's option carries a
  persistence scope, that scope is shown as a subtitle
  (`"for Bash(git *) in this project"`) so the user knows what they are
  granting.
- **In `pty` mode the permission card is *partly* answerable, and the split is
  per option.** This is the one kind where `deliverable` alone is not the whole
  story (06 §5.2.1). D16 verified that option 1 on a Claude Code permission
  prompt is **always** `Yes`, so allow is position-independent — but the list is
  2–4 options long depending on prior grants that the hook payload does not
  carry, so the index of a *specific* `No` is not derivable. Guessing it is how
  you deny a write by allowing it. So, when
  `deliverable.type === "synthetic"`:
  - options whose `kind` is `allow` or `allow_always` render as **actions**, with
    the press-and-hold gesture below;
  - options whose `kind` is `deny` or `deny_always` render **inert**, in place,
    in the agent's order — the user still sees exactly what the agent offered —
    each carrying the `index_not_derivable` reason from §5.1.3's table on a
    shared "why?" disclosure, not one per row;
  - the footer gains a single **`Cancel this request`** action, which sends
    `Esc` — D16's **universal safe negative**, the one keystroke that is
    position-independent on every card. It travels through the same D13 gated
    transaction as an allow, never through §4.3's input path. It is labelled for
    what it does (it
    withdraws the request; it does not record a reasoned denial) and is styled as
    a secondary destructive action, never as the agent's own `No`. `y` and `n` do
    nothing on these cards and are not bound;
  - `Open terminal view` sits next to it, because a *reasoned* deny — the one
    that carries `reason` to the agent and the only one `deny_always` can be — is
    a keyboard action.

  When `deliverable.type === "native"` every option is an action and the footer
  has no `Esc` row; ACP carries a real denial.
- **Edit before approving.** The card gains an edit affordance whenever the
  responder reports `supports_edit` — a property of the channel, not of the
  agent's option list ([06 §5.4](06-agent-layer.md#54-editing-an-argument-before-approving)),
  so omt is not adding an option the agent did not offer. A `Synthetic` responder
  does not report it: submitting an edited argument means typing a payload into
  the agent's own card, which is text entry, not an indexed keystroke. The
  affordance is therefore absent in `pty` mode rather than disabled, and the
  client tests `supports_edit` — it does not infer the answer from
  `deliverable`. Tapping it switches the
  input pane from read-only pretty-printed JSON to an editor: schema-aware
  (typed fields, enums, required keys) where the channel supplied the tool's own
  input schema, a plain JSON editor otherwise, with the same
  [13 §8](13-security.md#8-secret-redaction) redaction rules and no client-side
  "fixing up". Before submit the card shows a **diff of original versus edited
  input**, rendered by the §5.3 diff component, because approving a change you
  cannot see is exactly the failure this feature exists to prevent. On a phone
  the editor is a **full-height sheet**, never an inline field — JSON editing in
  a 3-line box above a software keyboard is unusable. The submit button reads
  **"Approve with changes"**, and the resolution is attributed as user-edited on
  every surface.

```ts
// InteractionResponse for permission (shape owned by 06 §5.0; `edit` +
// `updated_input` semantics and who may offer them by 06 §5.4):
{ type: "permission";
  decision: "allow" | "allow_always" | "deny" | "deny_always" | "edit";
  updated_input?: unknown;
  reason?: string }               // shown to the agent; required for deny_always
```

Every permission resolution requires a **deliberate** gesture on mobile: the
Allow control is a press-and-hold (350 ms) with a filling ring, a plain tap on
desktop. This is a property of the touch input model applied uniformly to every
permission card — it does not depend on what the tool does and it never changes
which options are offered. It is the one place we accept extra friction.

### 5.4 Plan review — `kind.type === "plan_review"`

The agent's plan arrives as Markdown. Rendered with a strict Markdown pipeline
(no raw HTML, no external images) into a scrollable sheet with:

- a **step checklist** when the plan parses as a task list, mirroring the
  `Plan { steps: [{title, status}] }` payload so status updates from the agent
  animate in live;
- actions: **Approve**, **Approve and switch mode** (a dropdown of the agent's
  `permission_modes` from the `Capabilities` event — `acceptEdits`, `auto`, …),
  **Request changes** (opens a text sheet; resolves as
  `{ type: "text", value }` semantics carried in `reason`), **Reject** —
  **rendered only when `deliverable.type === "native"`.**

**In `pty` mode a plan is read-only here.** The normalizer sets
`deliverable = None { NotPositionIndependent }` for every `PlanReview` on a
`pty` binding (06 §5.2.1, from D16's table), so the card renders per §5.1.3: the
full Markdown plan, the step checklist, live status updates — everything except
the action row, which is replaced by the reason and `Open terminal view`.

The reason is worth being concrete about, because a plan is the card a user most
wants to approve from a phone and the disappointment should be explained rather
than absorbed. `ExitPlanMode` presents **2–5 options, conditional on state the
hook payload does not carry**, and the last of them is a **text input** rather
than a selectable row. So neither of the two things omt would need is true: the
index of `Approve` is not fixed, and the card cannot be dismissed into a
decision by a single keystroke. Reading the plan on the phone and approving it at
the keyboard is the honest split, and it is still most of the value — the plan is
the long thing to read, and approval is one keypress.

This is the strongest case for the ACP adapter's raised priority
([D13](decisions.md#d13--synthetic-delivery-is-a-gated-transaction-never-a-bare-write)):
in `native` mode every action above is available, because ACP carries a real
response channel and no keystroke is involved.

### 5.5 Free-text elicitation — `kind.type === "text"`

The simplest card and the most used one after permissions:

```ts
{ type: "text"; placeholder: string | null; multiline: boolean }
// → { type: "text", value: string }
```

Single-line renders as an inline input with an inline send button; multiline
opens a sheet with an auto-growing textarea capped at 40% viewport height,
a voice button (§7), and `⌘/Ctrl+Enter` to submit. Draft text is persisted per
`interaction.id` so an accidental navigation does not lose it.

### 5.6 Card component contract

```tsx
export interface InteractionCardProps {
  /** The generated `Interaction` type verbatim — 06 §5 owns the shape:
   *  { id, session, binding, kind, opened_at, timeout_at, state, responder,
   *    deliverable, viewers }. Note **`binding`, not `agent`**: the card is bound to an
   *  `AgentBinding`, and the agent kind and label are read from it. There is no
   *  `agent` field, and no `OpenInteraction` type — the card renders every
   *  state in §5.1.1, not only the open one. */
  interaction: Interaction;
  variant: "inline" | "sheet" | "list";      // block view / focused / dashboard
  /** Called only while `interaction.state.type === "open"` **and**
   *  `interaction.deliverable.type !== "none"` (§5.1.2). The client mints the
   *  `intent_id` (§5.1) and the outbox owns it (§8.5). */
  onResolve(response: InteractionResponse): void;
}

export function InteractionCard(props: InteractionCardProps) {
  return (
    <Switch fallback={<UnknownInteraction kind={props.interaction.kind} />}>
      <Match when={props.interaction.kind.type === "choice"}>      <ChoiceCard …/></Match>
      <Match when={props.interaction.kind.type === "permission"}>  <PermissionCard …/></Match>
      <Match when={props.interaction.kind.type === "plan_review"}> <PlanCard …/></Match>
      <Match when={props.interaction.kind.type === "text"}>        <TextCard …/></Match>
    </Switch>
  );
}
```

**`kind.type` is the only thing this switch may test.** Answerability is not in
it: each card renders its body unconditionally and asks a shared
`useAnswerable(interaction)` for its footer, which returns the controls, the
token gate, or §5.1.3's read-only footer from `deliverable` alone. Putting the
D16 table in the view layer — a `kind.type === "plan_review"` test, or a
`multi_select` test — is the specific mistake §5.1.2 exists to prevent: the table
is empirical and per agent version, it lives in the normalizer, and a second copy
in TypeScript would drift from it silently. §10.1 asserts this: every card
component is rendered against all three `Deliverable` shapes, and the `none` case
asserts **no** element with a click handler inside the card body.

`<UnknownInteraction>` is not dead code: a newer instance may send a kind this
client does not know. It renders `prompt` verbatim plus a "answer in the
terminal view" button, which is strictly better than a blank card. **On a
`native` session there is no terminal view to open** (§4): the button is replaced
by the raw payload in a disclosure, plus "this instance is newer than this
client — update the client to answer here", because in `native` mode no other
surface exists.

---

## 6. Agent session dashboard

The dashboard answers one question: **what needs me right now, anywhere?**

Rows are `UnifiedSession` (§3.3) grouped into three sections:

1. **Needs you** — any session with an open `Interaction`, or an agent in
   `AgentState::Blocked`. (A `blocked` agent with no interaction id is a
   "needs you" omt can see but not render — 06 §4 — and the row says so, offering
   "open the terminal view". On a `native` session that fallback does not exist
   (§4), so the row instead says the agent is blocked on something its ACP
   connection did not describe, and offers the transcript; this is rare by
   construction, because `native` sessions have only a structured source.) Or
   `agent.state === "blocked"`. Rows here render a compact preview of
   the interaction (the first question's `header` chips, or the permission's
   tool name) and a **primary action inline** — for a single-question,
   single-select choice with ≤3 options **whose `deliverable.type !== "none"`**,
   the options render as buttons directly in the row. Answering a question from
   the list without opening the session is the entire point. Where `deliverable`
   is `none` the row still appears — it *does* need you, and hiding it would be
   the worst possible reading of §5.1.3 — but its inline action is
   `Open terminal view` and its subtitle is the reason (§5.1.3), so the dashboard
   never offers an answer the card itself would refuse.
2. **Working** — `working` sessions, with the current tool call, elapsed time, and
   a token/cost readout from the `Usage` payload.
3. **Idle** — everything else, most recent first.

Each row: instance chip · workspace/branch · agent glyph + model · state ·
elapsed. Swipe left on a row → `agent.interrupt`. Swipe right → mark read.

### 6.1 Live message queue

Claude Code emits explicit `queue-operation` transcript lines
([agent-clis §1.7](../research/agent-clis.md#17-message-queueing)), normalized to
`QueueChanged { op, index, text, pending }`. The dashboard mirrors `pending`
verbatim under a busy session:

```
▸ claude · botim-eclipse · working (2m14s) · Edit(src/lib.rs)
    Queue (2)
    1. run the migration script after this        [×]
    2. then push and open a PR                    [×]
    + add to queue…
```

- `+ add to queue…` calls `agent.queue.enqueue`; `[×]` calls
  `agent.queue.remove`. Both are ordinary catalog capabilities, so they work
  from the CLI too.
- Reordering is offered **only** if the instance reports a queue-reorder
  capability; Claude Code's observed operations are `enqueue` and `remove` only,
  so reorder is implemented as remove+enqueue and labelled as such in the
  optimistic UI (it briefly shows the item moving, then reconciles against the
  event).
- For agents that emit no queue events, the section is absent — not shown empty.

### 6.2 Slash commands as a real completion popup

`agent.commands.list` returns the agent's own resolved command list — from
`system/init`'s `slash_commands`, ACP `available_commands_update`, or disk
enumeration ([03 §6](03-capability-catalog.md#6-capability-groups-initial-surface)).

The web composer implements it as a true completion popup, not a menu:

- Typing `/` in the prompt composer opens a filtered list above the keyboard.
- Each entry shows `name`, `description`, and `argument-hint` (from the command
  `.md` frontmatter, which the daemon enriches the list with).
- Fuzzy matching on name and description; arrow keys / swipe to move; Enter or
  tap to insert.
- Selecting a command with an `argument-hint` inserts it and places the caret in
  an argument slot, showing the hint as placeholder text.
- Commands are also invocable directly via `agent.commands.run` from a long-press
  menu on the session row, for the "just run /compact" case.

The list refreshes on `Capabilities` events, so installing a skill on the laptop
makes it appear on the phone without a reload.

---

## 7. Voice input

Rationale for BYOK is settled in
[agent-clis §12.4](../research/agent-clis.md#124-voicestt-recommendation):
Claude Code's `/voice` is account-gated, local-mic-only, and explicitly does not
work over SSH — structurally unusable for a remote client. omt captures in the
browser, transcribes in the instance via `SttProvider`, and injects text.

### 7.1 Capture

```ts
const stream = await navigator.mediaDevices.getUserMedia({
  audio: { channelCount: 1, sampleRate: 16000, echoCancellation: true,
           noiseSuppression: true, autoGainControl: true },
});
const rec = new MediaRecorder(stream, {
  mimeType: pickMime(),                 // audio/webm;codecs=opus → audio/mp4 on Safari
  audioBitsPerSecond: 24_000,
});
rec.ondataavailable = (e) => sttSocket.send(e.data);   // binary frames on the same WS
rec.start(250);                                        // 250 ms chunks
```

- **Safari does not support `audio/webm`.** `pickMime()` falls back to
  `audio/mp4` (AAC); the daemon's `SttProvider` receives a container hint in
  `stt.session.start` and transcodes if the provider needs PCM. Tested on iOS 17+.
- Audio frames ride the **existing WebSocket** as binary frames tagged with the
  STT session id — no second connection, no CORS, works over the same Tailscale
  path as everything else.
- A `<canvas>` waveform is driven by an `AnalyserNode` on the same stream for
  "is it hearing me" feedback. It is cheap and it is the difference between
  users trusting the feature and not.

### 7.2 Transcript UX

Modeled on Claude Code's dictation, which is well-tuned:

| Behavior | Value | Source |
|---|---|---|
| Modes | **hold-to-talk** (default) and **tap-to-toggle** | Claude Code `/voice hold` / `/voice tap` |
| Auto-submit | on, when the final transcript has ≥ 3 words | Claude Code `voice.autoSubmit` |
| Silence stop | 15 s of silence ends the utterance | Claude Code |
| Hard stop | 2 min | Claude Code |
| Interim text | dimmed, italic, replaced in place by the final | Claude Code |
| Recognizer hints | project name, git branch, agent name, recent file basenames | measurably improves repo-name accuracy |

Hold-to-talk on touch is the mic button with `pointerdown`/`pointerup`, with
`touch-action: none` and a pointer capture so a finger sliding off the button
does not cut the recording. Releasing over a "✕ cancel" zone (revealed above the
button while holding, WhatsApp-style) discards the utterance. Tap mode is a
toggle with a running timer.

Interim results require a streaming provider (Deepgram
`interim_results=true`); with a batch provider (OpenAI) the UI shows a
"transcribing…" state instead and never fakes interim text.

### 7.3 Provider selection

`stt.providers.list` returns what the instance has configured:

```ts
interface SttProviderInfo {
  id: string;                     // "deepgram" | "openai" | "whisper-local" | …
  label: string;
  streaming: boolean;             // drives interim-text availability
  configured: boolean;            // credential present on the instance
  languages: string[] | "auto";
  note: string | null;            // "audio leaves this machine" for hosted providers
}
```

The picker is a sheet from the mic button's long-press. Rules:

- **Keys never touch the browser.** Credentials are configured on the instance
  (`config.set` → `stt.providers.<id>.api_key`, stored outside the main config
  file per [P8](01-principles.md#p8--security-by-default-no-ambient-trust)). The
  web UI can *set* a key through the config capability but never stores or
  echoes one.
- Hosted providers show `note` prominently the first time they are selected —
  "audio is sent to Deepgram" is information the user is entitled to before
  speaking, not buried in settings.
- If no provider is configured, the mic button is disabled with a link to the
  config form, per the §3.4 degradation rule.

---

## 8. Mobile specifics

### 8.1 Touch targets and reach

- Minimum hit target **44×44 CSS px**, 8 px minimum gutter. Enforced by a
  Playwright audit that walks every interactive element in every view at
  390×844 and fails on violations (§10).
- **Thumb zone.** Primary actions live in the bottom third. The header carries
  identity and navigation only; nothing destructive is reachable in the top
  ~15% of the screen where a stretching thumb lands unpredictably.
- Sheets are the default modal, always dismissible by downward drag, always with
  the confirm action on the right at the bottom.
- Destructive actions are press-and-hold, never a bare tap (§5.3).

### 8.2 Safe areas and viewport

```css
:root {
  --safe-top: env(safe-area-inset-top, 0px);
  --safe-bottom: env(safe-area-inset-bottom, 0px);
  --kb: 0px;                     /* keyboard height, set from visualViewport */
}
.app { height: 100dvh; padding-top: var(--safe-top); }
.dock { bottom: calc(var(--kb) + var(--safe-bottom)); }
```

`<meta name="viewport" content="width=device-width, initial-scale=1,
viewport-fit=cover, interactive-widget=resizes-content">`.

### 8.3 iOS keyboard and viewport quirks

These are not hypothetical; each has a specific mitigation:

| Quirk | Mitigation |
|---|---|
| `100vh` includes the URL bar; content is clipped | Use `100dvh` with a `100vh` fallback, and never rely on `vh` for the terminal grid — measure with `visualViewport.height`. |
| The software keyboard does not resize the layout viewport | Subscribe to `visualViewport` `resize`/`scroll` and write `--kb` = `innerHeight - visualViewport.height - visualViewport.offsetTop`. All docked UI positions off `--kb`. |
| Safari scrolls the page to reveal a focused input, breaking a fixed layout | On `focusin`, record `window.scrollY`; on the following frame, `window.scrollTo(0, 0)` and let `--kb` do the work. |
| Double-tap zooms; long-press shows the callout | `touch-action: manipulation` on controls; `-webkit-touch-callout: none` and `user-select: none` on the key bar and card chrome (but **not** on output text — selecting output must work). |
| Font size < 16px on an input triggers auto-zoom | All text inputs are ≥ 16px. The terminal is not an input, so it can be smaller. |
| Rubber-band scrolling drags the whole page while panning the terminal | `overscroll-behavior: contain` on scroll containers; `preventDefault` on the terminal's `touchmove` only when panning. |
| xterm.js's hidden textarea fights the on-screen keyboard | Keep it focused only while the key bar is visible; blur it on view change so the keyboard dismisses predictably. |

### 8.4 Gestures

| Gesture | Context | Action |
|---|---|---|
| Swipe left/right on the session header | any session view | previous/next session in the unified list |
| Swipe down from the top of a card | interaction sheet | dismiss (defer, does **not** resolve) |
| Swipe left on a dashboard row | dashboard | interrupt agent |
| Swipe right on a dashboard row | dashboard | mark read / dismiss badge |
| Long-press on a block | block view | reveal the action row |
| Two-finger pinch | terminal view | font size |
| Two-finger pan | terminal view | viewport pan when the grid exceeds the screen |
| Long-press on the mic | composer | provider picker |
| Pull-to-refresh | dashboard | force a `resync` on all instances |

No gesture is the *only* way to reach a capability — every one has a visible
control, because gesture-only affordances break the parity test's ability to
drive them and break discoverability for real users.

**Interrupt in particular gets a visible control**, since it is the one a user
reaches for under pressure and the swipe would otherwise be its only path. A
session whose agent is `Working` shows a **`⏹ Stop`** button in the session
header — persistent, in the thumb zone, and the only destructive-looking control
that does not require a confirm sheet, because hesitating to stop a runaway
agent is the failure this exists to prevent. It calls `agent.interrupt`, which
uses the agent's native interrupt channel where one exists and falls back to
`Ctrl+C` where it does not ([06 §7.3](06-agent-layer.md)). The swipe on a
dashboard row remains as the fast path for someone triaging several sessions.

### 8.5 Offline and reconnect

- A single connection-state banner, per instance, at the top: `connecting…`,
  `reconnecting in 4s (attempt 3)`, `offline`. Exponential backoff with jitter,
  capped at 30 s, reset on any successful frame.
- **The durable outbox.** A capability call made while disconnected is queued if
  its intent class permits a retry
  ([D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism):
  `interaction.resolve`, `agent.queue.enqueue`, `config.set`, drafts) and
  rejected immediately otherwise (`session.write_bytes` — the raw-byte-stream
  class is never replayed; typing into a dead socket must fail loudly).

  This is the owner of requirement **R19**, which was previously unassigned. The
  outbox is **backed by IndexedDB, not by memory** — a phone's tab is evicted by
  the OS without warning, and an in-memory queue silently loses the user's
  answer while showing them that it was accepted. Each entry stores:

  ```ts
  interface OutboxEntry {
    intent_id: string;        // stable; survives reconnect and reload
    request_id: string;       // `${deviceId}:${n}`, per 07 §3.5
    capability: string;
    input: unknown;
    created_at: number;
    valid_until: number;      // default created_at + 15 min
    binding?: BindingId;      // required for agent-targeted calls — see below
  }
  ```

  - **`binding` is mandatory for anything targeting a running agent.**
    `agent.queue.enqueue` without it can land in a shell prompt and be executed
    as a command if the agent exited during the offline gap
    ([06 §8](06-agent-layer.md#8-ancillary-semantics)). The server enforces it;
    the client must not construct such an entry at all.
  - **Age is visible and discarding is manual.** The pending pill shows count
    *and* age ("2 actions pending · oldest 6 min"), expands to a list, and every
    entry has a discard control. A user must never be unable to see or cancel
    something the app will do on their behalf.
  - **Expiry requires re-confirmation, never silent replay.** On reconnect, an
    entry past `valid_until` is **not** sent. It is presented as *"queued 40
    minutes ago — send now?"* with its full text, and expires from the outbox on
    dismissal. Replaying a stale mutation into a session that has moved on is the
    failure mode this rule exists to prevent.
  - On reconnect, live entries are replayed in order; a `conflict` from
    `interaction.resolve` renders as "already answered by X", and the stable
    `request_id` means a repeat of an already-applied call returns the original
    result rather than executing twice (07 §3.5).
  - **The outbox never retries an injection's delivery.** `interaction.resolve`
    is retry-safe as a *capability call* (it is CAS'd), but if the ledger reports
    `Undelivered` the client shows that state and offers the terminal view — it
    does not re-queue ([06 §5.1](06-agent-layer.md#51-lifecycle)). The card's
    rendering of `Undelivered`, and of every other state, is
    [§5.1.1](#511-the-cards-state-machine)'s single table — this section defines
    no state set of its own.
- On reconnect the client resumes with `since_seq` per session; on `Resync` it
  fetches a snapshot and rebuilds. The user sees a brief
  skeleton, never a wrong state.
- `navigator.onLine` is used only as a hint to shorten backoff, never as truth.
- A wake lock (`navigator.wakeLock`) is held while a session is `busy` **and**
  the user opted in (`busy` here meaning `AgentState::Working`), so watching a
  long agent run does not require tapping the
  screen.

### 8.6 PWA — installable, with no push

- **Installable**: web app manifest with maskable icons, `display:
  "standalone"`, `theme_color` synced to the active theme.
- The **service worker caches the app shell only** (precache the Vite build
  manifest, network-first for `index.html`). It never caches API responses or
  event data — stale session state is worse than no session state. The one
  exception is the durable outbox (§8.5), which is user intent rather than
  server state and lives in IndexedDB.
- **No Web Push.** [D12](decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead)
  ships no notification backend, so there is no `pushManager.subscribe`, no
  VAPID key in `instance.info`, no `notification.push.subscribe` capability, no
  `push` or `notificationclick` handler in the service worker, and no
  lock-screen answer path. The reason is in [07 §8](07-remote-protocol.md#8-notifications-to-a-closed-tab--none-in-v1):
  reaching a closed browser tab requires the browser vendor's relay, which means
  omt making an outbound connection and leaking *"this machine needs its owner,
  now"*.
- **What the user is told.** Onboarding says it plainly: *omt does not notify
  you when the app is closed. Open it and it will show you what needs you
  first.* Not a footnote — a stated limitation, because the alternative is a
  user who believes they will be alerted and is not.
- **What replaces it is the open path.** Cold start → reconnect → `Resync` →
  refetch the attention log and attention state (07 §5.2) → rank → present. That
  sequence is designed in [`../design/remote-continuity.md` §5](../design/remote-continuity.md#5-open-and-replay--from-cold-start-to-the-right-screen)
  and it is the client's most important flow, not a fallback.
- The service worker is still worth having for the app-shell precache alone: it
  is what makes the cold start fast enough for that flow to feel like a
  notification tap used to.

---

## 9. Accessibility and theming

### 9.1 Accessibility

- Every card is a landmark with a heading; the interaction sheet is a
  `role="dialog"` with focus trapping and `aria-modal`.
- Choice options are a real `role="radiogroup"` / `role="group"` with
  `aria-checked`; descriptions are wired with `aria-describedby`. A screen
  reader hears the same information the sighted user sees, which is exactly the
  information the terminal card *cannot* convey.
- New interactions announce via a polite live region; a session going to
  `blocked` announces via an assertive one.
- The terminal view uses xterm.js's screen-reader mode when a screen reader is
  detected, and always exposes "read last block" as an explicit action, because
  a live VT grid is hostile to assistive tech.
- Full keyboard operation on desktop: every gesture has a key binding, and the
  binding table is generated from the same config the TUI uses where the actions
  overlap.
- Respect `prefers-reduced-motion` (no auto-advance flash, no sheet spring),
  `prefers-contrast: more` (thicker borders, no low-contrast meta text), and
  `prefers-color-scheme`.
- Color is never the sole signal: the exit-code chip carries a glyph, the diff
  carries `+`/`-` gutters, the agent-state dot carries a shape.
- Target: WCAG 2.2 AA, verified with `axe-core` in the Playwright suite.

### 9.2 Theming

Themes are instance configuration
([10 — Configuration](10-configuration.md)), not a web-only setting, so the
laptop TUI and the phone look like the same product.

```ts
interface Theme {
  name: string;
  mode: "light" | "dark";
  terminal: { background: string; foreground: string; cursor: string;
              selection: string; ansi: string[16]; brightAnsi?: string[16] };
  ui: { surface: string; surfaceAlt: string; text: string; textMuted: string;
        accent: string; danger: string; success: string; warning: string; border: string };
  agentAccents: Record<AgentKind, string>;
}
```

- The client fetches `config.get { path: "theme" }` and writes every value into
  CSS custom properties on `:root`; the same object seeds xterm.js's `theme`.
- A config carrying **both** a light and a dark theme lets the client follow
  `prefers-color-scheme`; a config with one theme pins it. A per-device override
  ("always dark on my phone") is stored locally and clearly marked as an
  override in settings.
- Contrast is validated at load: if a theme's UI text/background pair falls
  below 4.5:1, the client keeps the theme's colors for the *terminal* and falls
  back to a built-in accessible UI palette, with a one-time warning. A pretty
  terminal theme should not make the permission card unreadable.
- another terminal-style YAML themes and iTerm2 `.itermcolors` are importable — those are
  file *formats*, i.e. interface facts
  ([P9](01-principles.md#p9--clean-room-with-respect-to-studied-code)); the
  conversion lives in `omt-config`, not the browser.

---

## 10. Testing

### 10.1 Component tests

Vitest + `@solidjs/testing-library`, no browser.

- **Fixture-driven interaction cards.** The verbatim `AskUserQuestion` JSON from
  [agent-clis §1.3.1](../research/agent-clis.md#131-askuserquestion--the-structured-interaction-case)
  is a checked-in fixture. Tests assert: three header chips render, the wizard
  advances on single-select, `multi_select` on a `native` binding requires
  explicit Next, "Other…" produces `{ other: "…" }`, and the comment path
  produces `{ comment: "…" }`.
- **The `deliverable` matrix, for every card kind.** Each of `<ChoiceCard>`,
  `<PermissionCard>`, `<PlanCard>` and `<TextCard>` is rendered against all three
  `Deliverable` shapes (§5.1.2). For `{ type: "none" }` the test asserts the
  question, options and descriptions are all **present** in the DOM (D11's
  mirroring survives), that the reason copy for that `NotDeliverableReason` is
  rendered, that `Open terminal view` is the only action — and that **no element
  inside the card body carries a click handler**, which is the assertion that
  catches a disabled-but-tappable regression. For `<PermissionCard>` the matrix
  additionally covers the per-option split of §5.3: allow rows active, deny rows
  inert, one `Esc` footer action. A parallel Rust test asserts the normalizer
  produces exactly these shapes from D16's table, so the two ends cannot drift.
- **Resolution shape tests** compare the produced `InteractionResponse` against
  a JSON fixture shared with the Rust side, so both ends test the same bytes.
- **Reducer tests** for the event stream: out-of-order `seq` is rejected, gaps
  trigger resync, `InteractionResolved` for an unknown id is ignored.
- Snapshot tests are used only for the diff renderer and the block card, where
  the DOM shape *is* the contract.

### 10.2 Mock instance harness

`web/mock-instance/` is a TypeScript server implementing the omt wire protocol
against the generated schemas. It can:

- serve a scripted session (a JSONL of `EventEnvelope`s with timestamps, replayed
  at real speed or instantly);
- inject an interaction on demand and assert the `interaction.resolve` payload;
- simulate transport faults: drop the socket, delay frames, force
  `Resync`, return `conflict` on resolve, report a **reduced catalog**
  (for §3.4 degradation tests) and a **newer catalog** (for
  `<UnknownInteraction>`);
- run multiple instances at once, so federation is tested for real.

Scripted sessions are recorded from real agent runs by an `omt record` command,
which dumps the event stream. The recording of the verified Claude Code session
that produced the `AskUserQuestion` above is the canonical fixture.

### 10.3 The parity E2E test

This is the test that makes P3 real from the web side. It runs in Playwright
against a real `omt` daemon (not the mock) on a Linux runner.

```
for each capability C in registry, where C ∉ PARITY_EXEMPT:
    h := handlers[C]
    if h.surface.kind == "implicit":
        assert h.surface.by ∈ registry and is itself covered   # transitively reachable
    else:
        drive the UI along h.surface's declared route
        assert the daemon observed a dispatch of C
        assert the resulting UI state matches the capability's declared output
```

The daemon runs with a dispatch recorder that logs every `(name, actor)`. The
test fails if any non-exempt capability was never dispatched by a *UI-driven*
interaction — calling `client.rpc(name, …)` from the test does not count, and
the recorder distinguishes them by `Actor`.

Complementary suites in the same run:

- **Touch-target audit** at 390×844 (iPhone 14), 360×800 (Android), 768×1024
  (tablet): every interactive element ≥ 44×44 with adequate spacing.
- **Axe accessibility scan** on every top-level view, in both themes.
- **Reconnect chaos**: kill the WebSocket at random points during an
  interaction resolve; assert exactly-once semantics and no lost answers.
- **Offline queue**: go offline, resolve an interaction, come back, assert the
  resolution landed once.
- **Visual regression** on the block card, the choice card, and the diff
  renderer at three widths, in both themes.

### 10.4 What is deliberately not tested here

xterm.js's own VT correctness. The authoritative terminal model is
[`omt-term`](04-terminal-core.md) and it has its own conformance suite; the web
client asserts only that the bytes it renders match a `SerializeAddon` dump of
the server's grid for a set of fixtures.

---

## 11. OPEN QUESTIONS

1. ~~**Notification action buttons on iOS.**~~ **Retired** by
   [D12](decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead):
   there is no push and therefore no lock-screen answer path to degrade. What
   replaces it as the measurable question is **cold-start to a useful screen**
   (§8.6, [`../design/remote-continuity.md` §5](../design/remote-continuity.md#5-open-and-replay--from-cold-start-to-the-right-screen)).
2. **How long does an agent's own card stay answerable?** Under
   [D11](decisions.md#d11--omt-mirrors-the-agents-own-card-it-does-not-intercept-or-replace-it)
   the call is not parked — the agent's CLI is showing its own live card, and the
   remote answer is delivered into it. So the budget is not "how long does
   `defer` hold", it is "how long before the card times out or the agent moves
   on", which differs per agent and is undocumented
   ([agent-clis §12.5 Q1](../research/agent-clis.md#125-open-questions-to-resolve-before-implementation)).
   Where it is short, the card must **expire** on every surface rather than
   linger — a late remote answer lands in whatever the agent is doing now
   ([D13](decisions.md#d13--synthetic-delivery-is-a-gated-transaction-never-a-bare-write)).
3. **`caller` enum on `AskUserQuestion`.** Only `{"type":"direct"}` is observed.
   Subagent callers presumably use another value; the card should attribute the
   asking subagent when it does, and today it cannot.
4. **Solid vs. the terminal's update rate.** Signals are the right shape for the
   card UI; the block list under a `yes`-style firehose still needs a coalescing
   layer (rAF-batched appends). Whether that layer belongs in the store or the
   view is unresolved.
5. **Safari `audio/mp4` chunking.** `MediaRecorder` on Safari emits fragmented
   MP4 whose first chunk carries the init segment; streaming providers that
   expect a continuous container may need server-side remuxing. The cost of that
   remux in `omt-stt` is unmeasured.
6. **Diff rendering budget.** `shiki`'s WASM is ~1 MB. Acceptable on a laptop,
   questionable as part of the initial bundle on LTE. Likely answer: lazy-load
   it on first diff render and ship a plain-text diff until it arrives.
7. **Block-body transport format.** Sending styled cells rather than raw bytes
   avoids a second VT parser but is more verbose on the wire. The crossover
   point (where raw bytes + a WASM `omt-term` build wins) is unknown; a WASM
   build of the real terminal core is the more principled long-term answer and
   is not in scope for v1.
8. **Federated identity.** Today each instance is a separate credential. A user
   with eight machines has eight credentials to manage. Whether omt should offer
   a client-side credential bundle (export/import a signed set) or lean entirely
   on tailnet identity is unresolved and belongs to
   [13 — Security](13-security.md).
