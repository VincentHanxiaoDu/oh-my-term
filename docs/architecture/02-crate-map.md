# Crate Map

A Cargo workspace of small crates with a strict, acyclic dependency order.
Layer *n* may depend on layers `< n` only. This layering is what lets many
changes be implemented in parallel worktrees without collisions: a change owns
one or two crates, and depends on others only through their published traits.

```
L6  binaries        omt · omt-hook
L5  surfaces        omt-tui · omt-server · omt-plugin-host
L4  orchestration   omt-daemon
L3  domain          omt-session · omt-agent · omt-config · omt-store
L2  subsystems      omt-term · omt-pty · omt-agent-adapters · omt-transport
                    omt-auth · omt-stt · omt-media
L1  contracts       omt-catalog · omt-proto · omt-events
L0  foundation      omt-types · omt-util
```

Web client (`web/`) is a separate TypeScript package, not a Cargo crate; it
consumes generated schemas from `omt-proto` and `omt-catalog`.

---

## L0 — Foundation

### `omt-types`
Primitive domain types shared by everything: `InstanceId`, `WorkspaceId`,
`SessionId`, `PaneId`, `ClientId`, `Seq`, `AgentKind`, `AgentState`,
`PermissionMode`, `Role`, and the newtypes/IDs that keep them from being mixed
up. No behaviour, no I/O, no dependencies beyond `serde`, `uuid`, `time`.

Rule: if two crates need to name the same thing, it belongs here.

### `omt-util`
Cross-cutting helpers with no domain knowledge: monotonic sequence generators,
debouncers, the `Registry<T>` used by every extension point, span-based tracing
setup, atomic file write, path canonicalization/slugging.

---

## L1 — Contracts

These three crates are the interfaces the whole system is organized around.
They are the first thing built, and the thing every parallel change reads.

### `omt-catalog`
The **capability catalog** (see [03](03-capability-catalog.md)): the declarative
list of every command and query omt supports, with their input/output types,
required role, and metadata. Provides the `Capability` trait, the
`CapabilityRegistry`, the dispatch types, and the build-time codegen that emits
JSON Schema + a TypeScript client + the reference documentation.

Contains no implementations — only declarations and the dispatch machinery.

### `omt-events`
The event model: `Event` envelope (`instance`, `session`, `seq`, `ts`,
`source`, `payload`), `AgentEvent` and its `Payload`/`InteractionKind` variants,
terminal events, session-tree events, presence events. Plus the broadcast bus
(`EventBus`) with per-subscriber backpressure policy and resume-from-seq.

### `omt-proto`
The wire protocol between an omt instance and a remote client: framing, the
handshake, capability negotiation, subscription messages, resume semantics, and
versioning. Encodes catalog calls and events; owns no policy. Emits the
TypeScript type definitions the web client imports.

---

## L2 — Subsystems

Each is independently useful, independently testable, and owns no global state.

### `omt-term`
Terminal emulation: VT/xterm parser, grid, scrollback, reflow, selection,
search, hyperlinks, graphics, and the **block model** built on OSC 133. Pure
state machine — bytes in, state and damage out. No I/O, no async.
See [04 — Terminal core](04-terminal-core.md).

### `omt-pty`
PTY lifecycle across Unix and Windows: spawn with injected env, resize,
foreground-process-group inspection, signal handling, read/write actors, and
process-tree/environ introspection (used by agent detection tier 1).

### `omt-agent-adapters`
One module per agent, each implementing `AgentAdapter` and contributing
`EventSource`s: hook installer + payload normalizer, protocol client (ACP,
app-server, REST/SSE), transcript tailer, and detection fingerprints. Depends on
`omt-events` for the normalized output and nothing above it.

### `omt-transport`
`Transport` implementations: WebSocket (axum-side and client-side), Unix domain
socket, SSH stdio bridge. Framing only; no auth, no routing.

### `omt-auth`
`AuthBackend` implementations and the role model: invite links (signed,
expiring, scoped), bearer tokens, password (argon2id), tailnet identity.
Issues and verifies credentials; performs no transport work.

### `omt-stt`
`SttProvider` implementations: Deepgram streaming, OpenAI batch, local
whisper.cpp. Audio in, interim + final transcripts out.

### `omt-media`
Clipboard and image bridging: OSC 52, the local↔remote file/image relay used
for pasting images into an SSH'd session, thumbnailing, temp-file lifecycle and
quotas. See [09 — SSH and media](09-ssh-and-media.md).

---

## L3 — Domain

### `omt-session`
The workspace/session/pane tree, layout (BSP), focus, the writer-token
arbitration model, presence, history, and session persistence/restore. Owns
`omt-term` and `omt-pty` instances; emits `omt-events`.

### `omt-agent`
The observation pipeline and state machine: source registry, confidence-tiered
merge, agent binding lifetime, the `Interaction` ledger (open, resolve exactly
once, broadcast), and the message-queue mirror. Depends on
`omt-agent-adapters` only through traits.

### `omt-config`
The layered configuration model: typed schema, loader, validator with rich
diagnostics, live reload, per-instance overrides, and generated schema/forms.

### `omt-store`
`Store` trait and backends. Persists the session tree, scrollback checkpoints,
block history, interaction ledger and credentials. Append-only log plus
snapshots so a crash loses at most the tail.

---

## L4 — Orchestration

### `omt-daemon`
The instance: wires the domain crates together, hosts the capability registry
with concrete handlers, owns the event bus, and enforces roles. This is the
only crate that knows about all of the domain at once — and it is *wiring*,
which is why it is allowed to.

---

## L5 — Surfaces

### `omt-tui`
ratatui/crossterm front end. A **client** of the catalog (in-process) and a
subscriber to the event bus. Contains rendering, input handling, keybindings,
the config editor and the session/agent dashboard. It may not reach into L3
directly.

### `omt-server`
axum HTTP/WebSocket server. Generated routes over the catalog, subscription
management, auth enforcement, the static web bundle, and instance federation
endpoints.

### `omt-plugin-host`
Out-of-process and WASM plugin hosting: manifest parsing, sandboxing, the
plugin-side capability surface, lifecycle and health.

---

## L6 — Binaries

### `omt`
The single user-facing binary. Subcommands: run the TUI (attaching to or
spawning a daemon), run headless, manage config, install integrations, manage
instances/credentials, and speak the catalog from the shell (`omt session list`,
`omt agent explain`, …) so that scripting has parity too.

### `omt-hook`
A tiny, dependency-light binary installed into each agent's hook configuration.
Reads a hook payload on stdin, forwards it to the local instance socket,
optionally waits for a decision (this is how a remote answer parks and then
resolves a `PreToolUse`), and writes the agent's expected response on stdout.
Kept separate so it starts in single-digit milliseconds and never blocks an
agent.

---

## Dependency rules (mechanically checked)

1. No cycles; no crate depends on a higher layer.
2. `omt-tui`, `omt-server`, `omt-plugin-host` are leaves — nothing depends on
   them.
3. L2 crates do not depend on each other, with two allowed exceptions:
   `omt-agent-adapters → omt-pty` (process inspection) and
   `omt-media → omt-term` (OSC 52 emission). Anything else is a design error.
4. Only L4+ may use `anyhow`; L0–L3 use crate-local `thiserror` types.
5. Only L4+ may spawn tasks or own a runtime; L0–L2 are runtime-agnostic
   (async traits where needed, no `tokio::spawn`).

## Ownership for parallel implementation

Because the seams are the crates, a change can be assigned to one worktree with
a clear blast radius:

| Change theme | Primary crates | Blocked by |
|---|---|---|
| Contracts | `omt-types`, `omt-catalog`, `omt-events`, `omt-proto` | — |
| Terminal core | `omt-term` | contracts |
| PTY + process | `omt-pty` | contracts |
| Session tree | `omt-session` | terminal, pty |
| Agent adapters | `omt-agent-adapters` | contracts |
| Agent pipeline | `omt-agent` | adapters, session |
| Config | `omt-config` | contracts |
| Store | `omt-store` | contracts |
| Transport + auth | `omt-transport`, `omt-auth` | contracts |
| Server | `omt-server` | daemon |
| TUI | `omt-tui` | daemon |
| Web client | `web/` | proto, catalog |
| Media/SSH | `omt-media` | terminal |
| STT | `omt-stt` | contracts |
| Plugins | `omt-plugin-host` | catalog, daemon |

The contracts layer is the one true serialization point. Everything downstream
of it fans out.
