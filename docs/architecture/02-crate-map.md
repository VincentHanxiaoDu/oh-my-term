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
                    omt-recall
L2  subsystems      omt-term · omt-pty · omt-agent-adapters · omt-transport
                    omt-auth · omt-identity · omt-stt · omt-media
                    omt-workspace-fs · omt-open · omt-input
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
`PermissionMode`, `Role`, `SessionMode` (`pty` | `native`,
[D8](decisions.md#d8--two-session-modes-pty-default-and-native-acp)),
`DeviceId`, `IdentityId`, `CredentialId`, `PeerInfo`, and the newtypes/IDs that
keep them from being mixed up. No behaviour, no I/O, no dependencies beyond
`serde`, `uuid`, `time`.

`SessionMode` is named by `omt-session`, `omt-agent`, `omt-agent-adapters` and
every surface; `DeviceId`/`IdentityId`/`CredentialId` by `omt-identity`,
`omt-auth`, `omt-session` (presence and `ActorKind::Remote`) and `omt-store`;
`PeerInfo` by `omt-transport`, `omt-proto` and `omt-identity`. Each therefore
belongs here by the rule below, not in whichever crate happens to mint it.

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
expiring, scoped), bearer tokens, password (argon2id), tailnet identity, and
the WebAuthn/passkey backend (`webauthn-rs`,
[23 — Identity and devices](23-identity-and-devices.md)). Issues and verifies credentials; performs
no transport work. It *consumes* `omt-identity` — it authenticates a `Device`
against an `Identity`, and owns neither.

### `omt-identity`
The identity and device model of [23 — Identity and devices](23-identity-and-devices.md):
`Identity`, `Device`, `DeviceRegistration`, `DeviceGrant`,
`InstanceRegistration`, the signed append-only **registry log** and its
verification, pairing, recovery, revocation and step-up policy. Pure model plus
signature verification: it mints and checks records, and performs no transport,
no storage and no prompting (`omt-store` persists the log; `omt-daemon` drives
the flows).

This is the one place device and instance ownership is decided, which is why it
is separate from `omt-auth`: *who you are and what hardware is yours* outlives
any particular credential scheme.

### `omt-stt`
`SttProvider` implementations: Deepgram streaming, OpenAI batch, local
whisper.cpp. Audio in, interim + final transcripts out.

### `omt-media`
Clipboard and image bridging: OSC 52, the local↔remote file/image relay used
for pasting images into an SSH'd session, the content-addressed blob store,
thumbnailing, blob lifecycle and quotas. See
[09 — SSH and media](09-ssh-and-media.md).

### `omt-workspace-fs`
The workspace file tree and version-control state: `FileTreeProvider` and
`VcsProvider` traits, the `GitCli` provider, ignore matching, bounded reads,
fuzzy filename search, structured diffs, and the ref-counted file watcher behind
a `WatchDriver` seam. Handed a root path and a `GitIdentity`; owns no part of
the session tree. Read-only — it ships zero write capabilities. See
[15 — Workspace explorer](15-workspace-explorer.md).

### `omt-open`
Semantic open ([18 — Semantic open](18-semantic-open.md)): **resolution and
action** over the pointers `omt-term` recognizes in terminal output. Owns the
`ResolvedTarget` model, the `stat`/VCS resolution cache, the `OpenHandler`
registry (editor, explorer, browser, agent hand-off, copy) and the `open.*`
capability group. Recognition itself stays pure in `omt-term`; `omt-open`
receives a `Match` (defined in `omt-types`) plus a `ResolutionContext`, so it
takes no L2 exception. `omt-daemon` wires the two together.

### `omt-input`
Input semantics ([16 — Input and keymap](16-input-and-keymap.md)): byte-level
key **and mouse** decoding across the legacy, `modifyOtherKeys` and kitty
encodings; normalization to `KeyEvent`/`MouseEvent`; the terminal-capability
probe that produces a `TerminalProfile`; the `Chord`/`ContextSet`/`Resolver`
model with its specificity-and-layer precedence and pending-chord trie; the
inner-program keymap registry (`InnerKeymapSource`) and its conflict
diagnostics; and the `default`/`vim`/`emacs` modal engines over one shared
action set.

It owns **keymap resolution semantics**; [10 — Configuration §8.2](10-configuration.md#82-keybinding-format)
owns the file format that compiles into it. It resolves to *capability names*,
never to capability implementations, so it stays below the catalog's handlers.
`omt-tui` is its only consumer today, plus the web client through the generated
types.

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
`Store` trait and backends (SQLite is the default, `store.db`). Persists the
session tree and layout, scrollback chunk snapshots, block history, native
session transcripts, credentials, the identity registry log, and the
**storage** of the interaction ledger. Append-only logs plus snapshots so a
crash loses at most the tail.

The ledger's *logic* is not here: the `InteractionLedger` — open, resolve
exactly once, the per-session mutex — lives in `omt-agent`
([05 — Session model](05-session-model.md), [12 — Collaboration](12-collaboration.md)), and `omt-store`
only gives it a durable append-only log so exactly-once survives a restart.

`omt-store` also owns **data lifecycle** ([21 — Data lifecycle](21-data-lifecycle.md)):
the retention policy model and the background sweeper, redaction at write time,
targeted and bulk purge, export, and the schema-migration machinery every other
crate's tables go through. Lifecycle is a property of *stored* data, so it lives
with the store rather than in a crate of its own.

### `omt-recall`
Memory and awareness ([20 — Recall, timeline and usage](20-recall-and-usage.md)),
laid out as `crates/omt-recall/{src,tests,benches}`: the cross-session search
index (SQLite FTS5 over `store.db`, path tokenization, the query language and
its ranking), the per-session timeline, the morning digest and its clause
generator, normalized agent usage and rate-limit accounting, and the
stuck-agent/attention detector.

It sits at **L3**, beside `omt-agent` and `omt-store`, because it genuinely
depends on both `omt-store` (it owns the `recall_v1` migration and reads the
same database) and `omt-events` (it indexes and detects over the event stream).
Nothing at L2 could do that without inverting the layering.

---

## L4 — Orchestration

### `omt-daemon`
The instance: wires the domain crates together, hosts the capability registry
with concrete handlers, owns the event bus, and enforces roles. This is the
only crate that knows about all of the domain at once — and it is *wiring*,
which is why it is allowed to.

It is also the home of the **in-process half of operations**
([22 — Operations](22-operations.md)): `system.health`, `system.doctor` and
`system.doctor.fix`, `service.*` (install, start, stop, status) and `upgrade.*`
must observe and act on the live instance, so they are daemon capabilities, not
CLI-only code. The daemon likewise drives the identity flows over
`omt-identity` (pairing, grant issue and revocation) and hosts `omt-recall`'s
background indexer and `omt-store`'s retention sweeper. Everything a reader
might expect to be "unowned" in [22](22-operations.md) is here or in `omt`
below.

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

It **owns onboarding and the operator front end**, which have no crate of their
own:

- [19 — Onboarding](19-onboarding.md) — first-run flow, the guided tour and
  in-product help, `TERM`/terminfo setup, shell-integration install, and the
  tmux/zellij config importer and coexistence checks. These are one-shot,
  interactive and process-local, so they live in the binary rather than in a
  library nothing else would call.
- [22 — Operations](22-operations.md) — the CLI surface: `omt doctor` (and its
  `keys` group, spelled `omt doctor keys`), `omt bug-report`, `omt service …`,
  `omt upgrade`, the no-TTY/CI mode and the bootstrap installer. The parts that
  must observe a running instance are capabilities in `omt-daemon` (above); the
  binary is their front end and their offline fallback when no daemon is
  running.

Neither 19 nor 22 is unassigned: the split is *interactive/one-shot in `omt`,
in-process in `omt-daemon`*.

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
3. L2 crates do not depend on each other, with **three** allowed exceptions:
   - `omt-agent-adapters → omt-pty` — process inspection.
   - `omt-media → omt-term` — OSC 52 emission.
   - `omt-auth → omt-identity` — authentication must verify a credential
     against the `Device`/`Identity` records and the registry log that
     `omt-identity` defines; duplicating that model in `omt-auth` would give
     the system two answers to "is this device still trusted?".

   Anything else is a design error. Note which crates take *no* exception:
   `omt-workspace-fs` depends only on `omt-types`, `omt-util` and `omt-events`;
   `omt-open` receives `Match`/`Target` from `omt-types` rather than depending
   on `omt-term`; and `omt-input` decodes bytes and resolves to capability
   *names*, so it depends on `omt-catalog` and `omt-types` only.
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
| Panes/layout | `omt-session` (`layout` module) | session tree |
| Agent adapters | `omt-agent-adapters` | contracts |
| Agent pipeline | `omt-agent` | adapters, session |
| Config | `omt-config` | contracts |
| Store | `omt-store` | contracts |
| Data lifecycle | `omt-store` (sweeper, retention, redaction, purge, export, migrations) | store |
| Recall/search | `omt-recall` | store, contracts |
| Transport + auth | `omt-transport`, `omt-auth` | contracts, identity |
| Identity/devices | `omt-identity` (+ `omt-auth`, `omt-daemon` for the flows) | contracts |
| Semantic open | `omt-open` (+ `omt-term` for recognition) | contracts, terminal |
| Input/keymap | `omt-input` | contracts |
| Server | `omt-server` | daemon |
| TUI | `omt-tui` | daemon |
| Web client | `web/` | proto, catalog |
| Media/SSH | `omt-media` | terminal |
| Workspace explorer | `omt-workspace-fs` | contracts |
| STT | `omt-stt` | contracts |
| Plugins | `omt-plugin-host` | catalog, daemon |
| Onboarding | `omt` (first run, tour, TERM, tmux import) | daemon, tui |
| Operations | `omt` (CLI) + `omt-daemon` (`system.*`, `service.*`, `upgrade.*`) | daemon |

The contracts layer is the one true serialization point. Everything downstream
of it fans out.
