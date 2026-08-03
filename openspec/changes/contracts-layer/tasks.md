## 1. Workspace skeleton

- [ ] 1.1 Create the Cargo workspace root with a pinned stable toolchain, shared
      lints (`clippy -D warnings`, deny `unwrap`/`expect` outside tests), and a
      shared dependency table
- [ ] 1.2 Create empty `crates/omt-types`, `omt-catalog`, `omt-events`,
      `omt-proto` with `#![deny(missing_docs)]` and a crate-level doc comment
      each
- [ ] 1.3 Create `xtask` with a `codegen` subcommand stub that exits non-zero
      with "not implemented"
- [ ] 1.4 Write the layering test: assert the dependency graph matches
      [02](../../../docs/architecture/02-crate-map.md) — no cycles, no upward
      edges, L2 crates independent except the two documented exceptions
- [ ] 1.5 Write the CI workflow: `fmt`, `clippy -D warnings`, `test`, the
      layering test, and a `codegen --check` step that fails on a stale
      committed artifact
- [ ] 1.6 Write the file-length check (warn 1200, fail 2000) per P1

## 2. `omt-types`

- [ ] 2.1 Identifier newtypes with their documented derivations: `InstanceId`,
      `WorkspaceId` (content-derived from the canonical root), `SessionId`,
      `PaneId`, `ClientId`, `DeviceId`, `IdentityId`, `CredentialId`,
      `BindingId`, `InteractionId`, `IntentId`
- [ ] 2.2 `Seq` as `u64` with its monotonic generator, plus the reserved
      instance-scope id — one type across all three scopes (D-2)
- [ ] 2.3 Domain enums: `AgentKind` (enumerated, not open-ended), `AgentState`,
      `SessionMode`, `Role`, `PermissionMode`, `PeerInfo`
- [ ] 2.4 Serde round-trip tests for every public type, with the wire spelling
      asserted (snake_case tags), so a rename is caught as a test failure

## 3. `omt-catalog` — declaration types

- [ ] 3.1 `Effects` as a bitflags type over the closed six-value set
- [ ] 3.2 `Intent` with D15's five classes, and the compile-time rule that a
      `Command` without one fails the build
- [ ] 3.3 `Parity`, including `Exempt { reason }`, and the exemption allow-list
      it is checked against
- [ ] 3.4 `CapabilityError`: closed code set plus a structured `detail` able to
      carry D15's three-way discrimination
- [ ] 3.5 `RequestId` as `(DeviceId, monotonic u64)` with its wire encoding
- [ ] 3.6 `Decl`: the all-`const` metadata struct, including `title`, `aliases`,
      `hidden` and `since`

## 4. `omt-catalog` — machinery

- [ ] 4.1 The `Capability` trait with its associated input/output types, `DECL`,
      and `refine_effects` with the never-widen rule
- [ ] 4.2 `CapabilityHandler<C>`, `CallContext`, and the `ErasedHandler` JSON
      boundary with its single adapter
- [ ] 4.3 `CapabilityRegistry`: construction, registration, name-sorted
      determinism, duplicate rejection at startup, and `seal()`
- [ ] 4.4 The `capability!` macro, expanding to the type, the `Decl` and the
      `DECLS` slice entry
- [ ] 4.5 The `linkme` distributed slice, plus the startup assertion that the
      slice is non-empty and matches the committed artifact (D-1 risk
      mitigation)
- [ ] 4.6 Dispatch: role check, intent enforcement, the recent-results cache
      keyed by request identity, and `unsupported` for a declared capability
      with no handler
- [ ] 4.7 `tests/third_party_impl.rs` — implement `Capability` and
      `CapabilityHandler` from outside the crate using only its public API

## 5. `omt-events`

- [ ] 5.1 The `Event` envelope with its scope rules, and the closed `EventKind`
      and `EventSourceTag` vocabularies
- [ ] 5.2 The payload variant set for each kind, cross-checked against the
      documents that consume them
- [ ] 5.3 `AgentEvent` envelope and the `AgentPayload` variants
      ([06 §8](../../../docs/architecture/06-agent-layer.md)), with the
      tier-permission table encoded so a heuristic source cannot emit structured
      content
- [ ] 5.4 `Interaction`, `InteractionKind`, `InteractionState` (seven variants),
      `Deliverable`, `NotDeliverableReason`, `UndeliveredReason`,
      `InteractionResponse` — 06 §5 is authoritative; `deliverable` on the
      struct per D-5
- [ ] 5.5 `EventBus`: subscribe, filter by scope and kind, resume from a
      sequence position, and the per-subscriber backpressure policy
- [ ] 5.6 Property test: resume from any position yields exactly the events after
      it, in order, with no duplication
- [ ] 5.7 Property test: a subscriber that cannot keep up is either collapsed
      with notice or disconnected with a reason — never silently short an event

## 6. `omt-proto`

- [ ] 6.1 Framing: length-prefixed text and binary frames, with the size bounds
- [ ] 6.2 The `ProtoMessage` catalogue: handshake, auth, capability call/result,
      subscribe/resume/resync/lagged
- [ ] 6.3 Handshake and capability negotiation, including the catalog hash and
      the intersection rule for a version-skewed peer
- [ ] 6.4 The 24-byte binary terminal frame header: `u64` `seq_or_off`, `u64`
      `ack`, with the byte layout asserted by test (D-2)
- [ ] 6.5 `HookEvent`/`HookAck` as `ProtoMessage` variants, with the verbatim
      payload field, the raw catch-all, the size bound and its truncation
      marker (D-4)
- [ ] 6.6 The fail-open contract in wire terms, and the per-agent stdout
      rendering table the hook binary owns
- [ ] 6.7 Round-trip fuzz target over every message, including chunk-boundary
      resumption
- [ ] 6.8 A checked-in fixture per message with a compatibility test, per P5

## 7. Codegen and generated artifacts

- [ ] 7.1 `omt debug catalog-dump --json` — the declarations plus generated
      schemas, from the linked binary
- [ ] 7.2 `cargo xtask codegen`: build, run the dump, emit `schemas/catalog.v1.json`
- [ ] 7.3 Emit the TypeScript client, including the union and the exhaustive
      handler map that fails the web build on an unhandled capability
- [ ] 7.4 Emit the server route table and the CLI subcommand tree
- [ ] 7.5 Emit `docs/reference/capabilities.md`, and delete
      `docs/reference/capabilities-draft.md` — the generator supersedes it
- [ ] 7.6 `codegen --check` for CI, reporting which artifact is stale
- [ ] 7.7 Determinism test: two runs against an unchanged binary are
      byte-identical

## 8. Proving capabilities and the parity gate

- [ ] 8.1 Declare `instance.info`, `instance.catalog` and `events.subscribe`
      with real handlers — the minimum that exercises query, metadata and
      subscription
- [ ] 8.2 The parity test over the registry: route + schema, TUI action, web
      handler, docs entry — or a listed exemption
- [ ] 8.3 Assert the parity test actually fails when a surface is removed
      (a test for the test — this gate is load-bearing for P3)
- [ ] 8.4 Benchmark the sequence allocator against
      [04 §9.1](../../../docs/architecture/04-terminal-core.md)'s throughput
      target; if it does not hold, stop and escalate — the fix is a
      contracts-layer change (D-2 risk)

## 9. Close-out

- [ ] 9.1 `cargo fmt`, `clippy -D warnings`, `cargo test` all clean
- [ ] 9.2 Rustdoc on every public item; `cargo doc` builds with no warnings
- [ ] 9.3 Update `docs/architecture/02-crate-map.md` if any crate boundary moved
      during implementation, and record why
- [ ] 9.4 Record in the change any architecture-document correction the
      implementation forced, so the docs stay the source of truth
