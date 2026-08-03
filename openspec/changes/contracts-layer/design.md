## Context

See [proposal.md](proposal.md) for motivation. The architecture is already
specified in detail — this document does not re-derive it, it records the
implementation decisions that the architecture leaves open and that must be
settled before code is written.

Authoritative sources, which win over anything restated here:

- [03 — Capability catalog](../../../docs/architecture/03-capability-catalog.md)
  §2–§3 (the declaration types, the machinery, collection, codegen, dispatch)
- [06 — Agent layer](../../../docs/architecture/06-agent-layer.md) §5, §8
  (`Interaction` and its state machine, `AgentEvent`/`AgentPayload`)
- [07 — Remote protocol](../../../docs/architecture/07-remote-protocol.md) §2–§5,
  §3.8 (framing, handshake, events, resume, the hook wire)
- [02 — Crate map](../../../docs/architecture/02-crate-map.md) (layering, and the
  rules the dependency test enforces)
- [decisions.md](../../../docs/architecture/decisions.md) — all sixteen bind;
  D15 (intent classes) and D2 (remote equals local) shape this change most.

**Crates:** creates `omt-types`, `omt-catalog`, `omt-events`, `omt-proto`, and
`xtask`. Modifies nothing — there is nothing yet to modify.

## Goals / Non-Goals

Beyond the proposal's scope, at the design level:

**Goals**
- The four crates compile and test with no runtime and no I/O, so that later
  crates can use them under deterministic simulation
  ([05 §12](../../../docs/architecture/05-session-model.md)).
- Every trait in this layer is provably implementable from outside its crate.
- Generated artifacts are reproducible byte-for-byte from a given binary.

**Non-Goals**
- Performance tuning. The one exception is the sequence allocator, which sits on
  the PTY hot path and is benchmarked here rather than discovered later
  ([07 §9](../../../docs/architecture/07-remote-protocol.md) open question 2).
- Backwards compatibility with anything. This is v1; the compatibility *machinery*
  ships, with nothing yet to be compatible with.

## Decisions

### D-1. `linkme` for declaration collection, and codegen reads the binary

Follows [03 §3.3](../../../docs/architecture/03-capability-catalog.md#33-how-declarations-are-collected).

Declarations live in the crates that own them, so `omt-catalog` never sees them
at compile time. The runtime list is a `linkme` distributed slice; codegen builds
and runs `omt debug catalog-dump --json` and consumes *that*, so its input is
byte-for-byte the list the process actually registers.

*Alternatives:* `inventory` registers via life-before-`main` constructors — order
is unspecified, it interacts badly with `--gc-sections`, and a dropped entry
means a capability vanishes with no error. A build-script source scan re-derives
the list by parsing Rust text and can diverge from what links. A manual
`register_all()` is correct but silently wrong when someone forgets a line.

The deciding property is not elegance, it is **which failure is loud**: with
`linkme` plus dump-from-binary, a declaration that does not link is absent from
the dump and the diff fails; with the alternatives it is silently missing.

Ordering is not left to the linker — the registry sorts by name at construction
and rejects duplicates, so output is deterministic and a collision is a boot
failure rather than a shadowed entry.

### D-2. `Seq` is `u64` everywhere, including on the wire

The frame header is 24 bytes with `u64` `seq_or_off` and `u64` `ack`
([07 §3.6](../../../docs/architecture/07-remote-protocol.md)). A `u32` would wrap,
and a resume after a wrap rejoins at a point that looks valid and is not — a
client that believes it is caught up and is not. Four bytes per frame buys away a
class of bug that no test would reliably catch.

### D-3. Dispatch owns authorization and retry semantics; transports own neither

One dispatch path applies the role check and enforces the intent class
([03 §3.5](../../../docs/architecture/03-capability-catalog.md)). Transports frame
bytes. This is what makes D2 ("remote is exactly equivalent to local") true by
construction rather than by discipline: the TUI, the CLI socket and a WebSocket
client differ only in how they reach dispatch.

The recent-results cache keyed by request identity lives in dispatch for the same
reason — implemented per transport it would be three implementations and three
sets of bugs.

### D-4. The hook ingress is a protocol message pair, not a capability

`HookEvent`/`HookAck` bypass `CapabilityRegistry::dispatch`, per
[07 §3.8.1](../../../docs/architecture/07-remote-protocol.md). A hook is not an
actor requesting a mutation — it reports an observation, exactly as a transcript
tailer or an ACP client does, and neither of those is a capability call. The
authorization that matters is the peer-credential check at the socket. Dispatch's
per-call machinery is also the wrong cost on a path with a single-digit-millisecond
budget, and is keyed by a request identity a hook does not have.

Auditability is preserved in the right place: the observation is durably recorded
in the agent event log, and `agent.explain` reports the source's health. "What did
the hook tell us and when" is answerable; it is simply not an audit question.

### D-5. Interaction shape: `deliverable` on the struct, not inside `Open`

[D13](../../../docs/architecture/decisions.md) writes the shape as
`Open { deliverable }`; [06 §5.2.1](../../../docs/architecture/06-agent-layer.md)
places it on the `Interaction` struct instead and records why. Deliverability
stays meaningful in `Submitted` and `Undelivered`, where a client must say
whether a retry is even possible — putting it inside `Open` deletes exactly the
information the failure surface needs. This change implements 06's placement.

### D-6. Errors are typed per crate; no `anyhow` below the binaries

Per [P5](../../../docs/architecture/01-principles.md). Each crate defines its own
error enum with `thiserror`. `CapabilityError`'s closed code set plus structured
detail is what lets a caller distinguish "already resolved" from "withdrawn" from
"timed out" — a distinction D15 requires and that a stringly-typed error destroys.

### D-7. Schema generation via `schemars`, checked in

Input and output types derive `JsonSchema`. Generated artifacts are committed and
diffed in CI rather than produced at build time, so a schema change is visible in
review as a diff rather than invisible until a client breaks.

## Risks / Trade-offs

- **Freezing traits before their second implementation exists** → `AgentAdapter`
  and `EventSource` are named by this layer but validated by the *agent adapters*
  change against Claude Code *and* a generic ACP adapter. D5 orders the ACP
  adapter early for precisely this reason. Mitigation: this change ships the
  event and interaction types those traits carry, and defers the traits
  themselves to the change that can validate them against two shapes.
- **`linkme` is a link-section trick** → it can be disturbed by unusual linker
  settings or `--gc-sections`. Mitigation: a startup assertion that the dump is
  non-empty and matches the committed artifact's count, so a stripped slice fails
  immediately rather than producing a silently smaller catalog.
- **The sequence allocator is on the PTY hot path** → a per-event allocation cost
  that looks negligible in isolation may not be at 10 MB/s. Mitigation: benchmark
  it inside this change (task 8), against
  [04 §9.1](../../../docs/architecture/04-terminal-core.md)'s throughput target,
  before the wire is frozen. If it does not hold, the fix is two sequence spaces —
  a contracts-layer change, which is why it must not be discovered later.
- **Codegen requires building and running the binary** → slower than a source
  scan and awkward for cross-compilation. Accepted: correctness of the list is
  worth more than codegen speed, and cross-compiled targets can consume the
  committed artifacts.
- **A capability declared with no handler returns `unsupported`** → a surface may
  offer something that does not work yet. Mitigation: the parity test lists them,
  so the gap is visible rather than discovered by a user.

## Migration Plan

Not applicable. Nothing exists to migrate from, and there are no consumers to
break. The compatibility machinery — protocol versioning, capability negotiation,
schema versioning from v1 — ships in this change so that future changes have it.

## Open Questions

- **The distributed-slice startup assertion's exact form.** Whether it compares a
  count, a hash of the sorted names, or both. Either satisfies the requirement;
  the implementer picks when writing it.
- **Whether `xtask` is a workspace member or a standalone binary.** Affects
  developer ergonomics only; no artifact depends on the answer.
