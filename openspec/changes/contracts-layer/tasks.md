## 1. Workspace skeleton

- [ ] 1.1 Create the Cargo workspace root with a pinned stable toolchain, shared
      lints (`clippy -D warnings`, deny `unwrap`/`expect` outside tests), and a
      shared dependency table
- [ ] 1.2 Create `crates/omt-types`, `omt-util`, `omt-catalog`, `omt-events`,
      `omt-proto` with `#![deny(missing_docs)]` and a crate-level doc comment
      each
- [ ] 1.2a Create the stub surfaces the parity gate needs (D-6): `crates/omt`
      (the binary codegen runs), `crates/omt-tui` with an action table and no
      rendering, `crates/omt-server` with a route table and no listener, and a
      `web/` package with a tsconfig and a hand-written placeholder handler map
      (7.3 replaces it; without a placeholder, 8.2 cannot run before 7.3)
- [ ] 1.3 Create `xtask` with a `codegen` subcommand stub that exits non-zero
      with "not implemented"
- [ ] 1.4 Write the layering test: assert the dependency graph matches
      [02](../../../docs/architecture/02-crate-map.md) — no cycles, no upward
      edges, the L1 order `omt-catalog → omt-events → omt-proto` with the
      reverse edges forbidden (rule 3 — the `omt-catalog → omt-events` edge is
      the one that closes a cycle), and L2 crates independent except the
      **three** documented exceptions (`omt-agent-adapters → omt-pty`,
      `omt-media → omt-term`, `omt-auth → omt-identity`)
- [ ] 1.5 Write the CI workflow: `fmt`, `clippy -D warnings`, `test`, the
      layering test, and a `codegen --check` step that fails on a stale
      committed artifact. Mark the `codegen` step `continue-on-error` until 7.6
      lands — it runs against 1.3's deliberately-failing stub until then, and a
      knowingly-red pipeline trains people to ignore it
- [ ] 1.6 Write the file-length check (warn 1200, fail 2000) per P1

## 2. `omt-types`

- [ ] 2.1 Identifier newtypes with their documented derivations: `InstanceId`,
      `WorkspaceId` (content-derived from the canonical root), `SessionId`,
      `PaneId`, `ClientId`, `DeviceId`, `IdentityId`, `CredentialId`,
      `BindingId`, `InteractionId`, `IntentId`
- [ ] 2.2 `Seq` as `u64` and the reserved instance-scope id — one type across
      all three scopes (D-2). The *generator* is behaviour and lives in
      `omt-util` per [02 §L0](../../../docs/architecture/02-crate-map.md), which
      says `omt-types` carries none
- [ ] 2.3 Domain enums: `AgentKind` (enumerated, not open-ended), `AgentState`,
      `SessionMode`, `Role`, `PermissionMode`, `PeerInfo`
- [ ] 2.4 The shared types more than one crate names, hoisted here per D-7:
      `Actor`/`ActorId`, `Timestamp`, `Version`, `Tier`, `SourceId`,
      `AgentSessionId`, `ResponderRef`, and `FileDiff` — the last because
      `06 §5` types a permission's diff as one while L2 owns the crate that
      produces it
- [ ] 2.5 Serde round-trip tests for every public type, with the wire spelling
      asserted (snake_case tags), so a rename is caught as a test failure

## 3. `omt-catalog` — declaration types

- [ ] 3.1 `Effects` as a bitflags type over the closed six-value set, with the
      wire form **a sorted array of lower-snake strings, not an integer**
      ([03 §2.3](../../../docs/architecture/03-capability-catalog.md)) — this
      goes into the committed schema and the TypeScript client, so guessing it
      is expensive to undo
- [ ] 3.2 `Intent` with D15's five classes **including their parameters** —
      `Append`'s `dedup`/`DedupKey` and `ExternallyConfirmed`'s `confirm_within`,
      which is what D15 consequence 1 hangs the `Undelivered` transition on —
      and the compile-time rule that a `Command` without one fails the build
- [ ] 3.3 `Parity`, including `Exempt { surfaces, reason }` — an exemption names
      the surfaces it covers rather than waiving the check wholesale (B3) — and
      the allow-list file it is checked against, whose path and format this task
      fixes
- [ ] 3.4 `CapabilityError`: closed code set plus a structured `detail` able to
      carry D15's three-way discrimination
- [ ] 3.5 `RequestId` as `(DeviceId, monotonic u64)` with its wire encoding
- [ ] 3.6 `Decl`: the all-`const` metadata struct, including `title`, `aliases`,
      `hidden` with its required `hidden_reason`, and `since`

## 4. `omt-catalog` — machinery

- [ ] 4.1 The `Capability` trait with its associated input/output types, `DECL`,
      and `refine_effects` with the never-widen rule
- [ ] 4.2 `CapabilityHandler<C>`, `CallContext`, and the `ErasedHandler` JSON
      boundary with its single adapter
- [ ] 4.3 `CapabilityRegistry`: construction, registration, name-sorted
      determinism, duplicate rejection at startup, and a **strict** `seal()`
      that fails naming any declared capability with no handler (D-8)
- [ ] 4.4 The `capability!` macro, expanding to the type, the `Decl` and the
      `DECLS` slice entry
- [ ] 4.5 The `linkme` distributed slice, plus the startup assertion that the
      slice is non-empty and matches the committed artifact (D-1 risk
      mitigation)
- [ ] 4.6 Dispatch: role check, intent enforcement, and the recent-results cache
      keyed by request identity. There is no unimplemented-capability path —
      `seal()` makes that state unreachable (D-8)
- [ ] 4.7 `tests/third_party_impl.rs` — implement `Capability` and
      `CapabilityHandler` from outside the crate using only its public API

## 5. `omt-events`

- [ ] 5.1 The `Event` envelope with its scope rules, and the closed `EventKind`
      and `EventSourceTag` vocabularies
- [ ] 5.2 Payload bodies for the four kinds this change owns — `terminal` (the
      subset the frame path needs), `agent`, `interaction`, `instance` — and
      the stated rule that the remaining five kinds are frozen as *kinds* with
      their bodies added by the change that owns the types they name (D-7)
- [ ] 5.3 `AgentEvent` envelope and the `AgentPayload` variants
      ([06 §8](../../../docs/architecture/06-agent-layer.md))
- [ ] 5.3a `EventSink` and `ActivitySink` — the type-level tier gate of
      [06 §8.4](../../../docs/architecture/06-agent-layer.md), where a tier-0
      source gets a sink whose only method emits an activity guess, so "screen
      text became a tool call" is not a bug that can be written. Their shape
      follows the payload set this change owns, not any agent, so unlike
      `AgentAdapter` they cannot wait for the adapters change without leaving
      the spec's heuristic-source scenario unverifiable
- [ ] 5.4 `Interaction`, `InteractionKind`, `InteractionState` (seven variants),
      `Deliverable`, `NotDeliverableReason`, `UndeliveredReason`,
      `InteractionResponse`, plus the types they name: `ChoiceQuestion`,
      `ChoiceOption`, `ChoiceAnswer`, `PermissionOption`, `PermissionOptionKind`,
      `PlanDecision`, `CancelReason` — 06 §5 is authoritative; `deliverable` on
      the struct per D-5, and `Resolving` carries the response
- [ ] 5.5 `EventBus`: subscribe, filter by scope and kind, resume from a
      sequence position, and the per-subscriber backpressure policy
- [ ] 5.6 Property test: resume from any position yields exactly the events after
      it, in order, with no duplication
- [ ] 5.7 Property test: a lossless event is never dropped without a notice, and
      a permanently-slow subscriber is disconnected with a stated reason rather
      than buffered without bound. Collapse-to-snapshot is *not* tested here —
      [07 §6.2](../../../docs/architecture/07-remote-protocol.md) routes it
      through `omt-term`, which is L2; this change owns the notice, not the
      collapse

## 6. `omt-proto`

- [ ] 6.1 Framing: length-prefixed text and binary frames. Per-transport size
      bounds belong to `omt-transport`; what freezes here is the header layout
      (6.4)
- [ ] 6.2 The `ProtoMessage` catalogue: handshake, auth, capability call/result,
      subscribe/resume/resync/lagged
- [ ] 6.3 Handshake and capability negotiation, including the catalog hash and
      the intersection rule for a version-skewed peer
- [ ] 6.4 The 24-byte binary terminal frame header: `u64` `seq_or_off`, `u64`
      `ack`, with the byte layout asserted by test (D-2)
- [ ] 6.5 `HookEvent`/`HookAck` as `ProtoMessage` variants, with the verbatim
      payload field, the raw catch-all, the size bound and its truncation
      marker (D-4) — including the case
      [07 §3.8.3](../../../docs/architecture/07-remote-protocol.md) leaves open,
      where `tool_input` alone exceeds the limit and the marker must still
      identify what was cut
- [ ] 6.6 The fail-open contract **in wire terms only**: the directive a hook can
      construct without a reply. The per-agent stdout rendering table lives in
      `omt-hook`, keyed by the `AgentKind` it was installed for
      ([07 §3.8.3](../../../docs/architecture/07-remote-protocol.md)) — putting
      it here would give `omt-proto` agent knowledge, against this change's own
      non-goal (E3)
- [ ] 6.7 Round-trip property tests over every message, including chunk-boundary
      resumption. The continuous fuzz target rides with `omt-transport`, whose
      change owns the byte path and where the message set has stopped growing
- [ ] 6.8 A checked-in fixture per message with a compatibility test, per P5

## 7. Codegen and generated artifacts

- [ ] 7.1 `omt debug catalog-dump --json` — the declarations plus generated
      schemas, from the linked binary
- [ ] 7.2 `cargo xtask codegen`: build, run the dump, emit `schemas/catalog.v1.json`
- [ ] 7.3 Emit the TypeScript client, including the union and the exhaustive
      handler map that fails the web build on an unhandled capability
- [ ] 7.4 Emit the server route table and the CLI subcommand tree into the stub
      crates from 1.2a
- [ ] 7.5 Emit `docs/reference/capabilities.md`, and delete
      `docs/reference/capabilities-draft.md` — the generator supersedes it
- [ ] 7.6 `codegen --check` for CI, reporting which artifact is stale
- [ ] 7.7 Determinism test: two runs against an unchanged binary are
      byte-identical

## 8. Proving capabilities and the parity gate

- [ ] 8.1 Declare `instance.info` and `instance.catalog` in `crates/omt`, and
      `events.subscribe` in `omt-events` — where the events live, which is the
      edge the L1 order exists to permit (02 rule 3). All three ship with real
      handlers, since `seal()` is strict (D-8); together they exercise query,
      metadata and subscription. Plus the types `instance.catalog` returns:
      `CatalogEntry`,
      `CatalogHash`, and `Origin` including its `Plugin` variant, which the
      proposal promises is declared even though hosting is doc 11's change
- [ ] 8.2 The parity test over the registry, all five artifacts and both
      directions: route + schema, TUI action (non-`Admin` only), web handler,
      docs entry, palette entry — or a listed per-surface exemption — **and**
      the reverse check that every bound action names a real capability, which
      [03 §5](../../../docs/architecture/03-capability-catalog.md) singles out
      as the half that catches drift
- [ ] 8.2a The declaration-soundness checks the same test enumerates: every
      `Command` declares an `intent`, and no `refine_effects` widens beyond its
      declaration
- [ ] 8.3 Assert the parity test actually fails when a surface is removed
      (a test for the test — this gate is load-bearing for P3)
- [ ] 8.4 Benchmark the sequence allocator against
      [04 §9.1](../../../docs/architecture/04-terminal-core.md)'s throughput
      target; if it does not hold, stop and escalate — the fix is a
      contracts-layer change (D-2 risk)

## 9. Close-out

- [ ] 9.0 A compatibility test over capability schemas, not just protocol
      messages: a checked-in snapshot plus an assertion that a later schema only
      adds optional input fields and only adds output fields
      ([03 §7](../../../docs/architecture/03-capability-catalog.md)). Task 6.8's
      fixtures cover `omt-proto`; nothing covered the catalog
- [ ] 9.1 `cargo fmt`, `clippy -D warnings`, `cargo test` all clean
- [ ] 9.2 Rustdoc on every public item; `cargo doc` builds with no warnings
- [ ] 9.3 Update `docs/architecture/02-crate-map.md` if any crate boundary moved
      during implementation, and record why
- [ ] 9.4 Record in the change any architecture-document correction the
      implementation forced, so the docs stay the source of truth
