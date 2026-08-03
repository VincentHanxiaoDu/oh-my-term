## Why

`omt`'s architecture is organized around one idea: every capability is declared
once, and the TUI, the API and the web client are three renderers over that
declaration ([P3](../../../docs/architecture/01-principles.md)). That makes
surface parity mechanically checkable instead of aspirational — but only if the
declaration machinery exists first.

Nothing else can be built until it does.
[02 — Crate map](../../../docs/architecture/02-crate-map.md) calls the contracts
layer *"the one true serialization point"*: the terminal core, the session tree,
the agent adapters, the daemon, the server and the web client all depend on
these four crates and on none of each other. Getting a type wrong here is a
multi-crate rewrite; getting it right lets every subsequent change land in its
own worktree.

## What Changes

- **A Cargo workspace** with the layering of doc 02 enforced by a
  dependency-direction test: no cycles, no upward edges, `omt-tui`/`omt-server`
  as leaves.
- **`omt-types`** — the primitives every crate names: `SessionId`,
  `WorkspaceId`, `DeviceId`, `IdentityId`, `Seq` (u64 throughout), `AgentKind`,
  `AgentState`, `SessionMode`, `Role`, `PeerInfo`.
- **`omt-catalog`** — the `capability!` macro, the `Capability` /
  `CapabilityHandler` / `ErasedHandler` / `CapabilityRegistry` machinery, the
  declaration-level types (`Effects`, `Intent`, `Parity`, `CapabilityError`,
  `RequestId`), the `linkme` distributed slice that collects declarations, and
  `cargo xtask codegen` reading the linked binary rather than parsing source.
- **`omt-events`** — the `Event` envelope, the ten `EventKind` payload sets, the
  closed `EventSourceTag` vocabulary, `AgentEvent`/`AgentPayload`, and the
  `Interaction` family (`InteractionState`, `Deliverable`,
  `InteractionResponse`).
- **`omt-proto`** — the wire protocol: handshake and capability negotiation,
  capability call/result, event subscribe/resume, the 24-byte binary terminal
  frame header with `u64` `seq` and `ack`, and the `HookEvent`/`HookAck` pair
  that carries the flagship observation path.
- **Generated artifacts committed and diffed in CI** — JSON Schema, the
  TypeScript client, the route table, the CLI tree and the capability reference.
  A drifted generated file fails the build.

## Capabilities

### New Capabilities

- `capability-catalog`: declaring a capability once and deriving its dispatch,
  routes, schemas, client, CLI and documentation from that single declaration —
  including the parity contract that fails CI when a surface is missing.
- `event-model`: the event envelope, its sequence spaces and resume semantics,
  the closed kind and source vocabularies, and the agent and interaction
  payloads every surface renders.
- `wire-protocol`: how an instance and a remote client talk — framing,
  handshake, capability calls, event subscription and resume, terminal byte
  frames, and the hook ingress.

### Modified Capabilities

None. This is the first change; no specs exist yet.

## Non-goals

- **No handlers.** This change declares capabilities and dispatches them; it
  implements none. A capability with no handler is registered and returns
  `unsupported` until the change that owns it lands.
- **No I/O.** No PTY, no sockets, no storage, no rendering. `omt-proto` defines
  the messages; `omt-transport` (a later change) moves the bytes.
- **No agent knowledge.** `AgentEvent` is the normalized shape; per-agent
  adapters that produce it are a later change.
- **Not the whole catalog.** Capability declarations live in the crates that own
  them, so this change ships the machinery plus only the handful of declarations
  needed to prove it (`instance.info`, `instance.catalog`, `events.subscribe`).
  [`docs/reference/capabilities-draft.md`](../../../docs/reference/capabilities-draft.md)
  is the interim reconciliation table and is deleted once codegen produces the
  real one.
- **No plugin host.** `Origin::Plugin` and the generic renderer path are
  declared so the type system accounts for them; hosting is doc 11's change.

## Dependencies and parallelism

- **Depends on:** nothing. This is the root change.
- **Blocks:** every other change. Doc 02's ownership table lists nine changes
  whose only stated blocker is this one.
- **Runs in parallel with:** nothing, by choice. Per the agreed sequencing, the
  contracts layer is built serially and validated by one end-to-end vertical
  slice before parallel work begins — precisely because a defect here is not
  local.

## Impact

- Creates the workspace root, `crates/omt-{types,catalog,events,proto}`,
  `xtask/`, and the committed `schemas/` + `web/src/generated/` outputs.
- Establishes the CI gates every later change inherits: `cargo fmt`,
  `clippy -D warnings`, the codegen `--check` diff, the layering test, and the
  parity test over the registry.
- Fixes the shape of the eight artifacts listed in
  [`docs/architecture/README.md`](../../../docs/architecture/README.md) as
  contracts-layer obligations, including the `omt-hook` wire that the flagship
  path depends on.
