# Reading guide

This directory is the architecture of `omt`. It is large, and it is meant to be
read in an order, not front to back.

Three files are not documents about a subsystem and are read differently:

- [`decisions.md`](decisions.md) — the decision log. **Binding.** Anything here
  overrides a contradicting statement anywhere else in `docs/`. If you find a
  contradiction, the other document is the bug.
- [`glossary.md`](glossary.md) — the canonical name registry. One concept, one
  name, one owner. If two documents disagree about a shape, the glossary names
  which one is authoritative.
- [`00-overview.md`](00-overview.md) — the entry point and the index.

---

## Reading order for a new contributor

**Read these four first, in this order.** They are short, and nothing else makes
sense without them.

1. [`00-overview.md`](00-overview.md) — the problem, the shape of the solution,
   the domain model.
2. [`01-principles.md`](01-principles.md) — the nine invariants every change is
   held to. `P3` (parity) and `P4` (native semantics) do the most work.
3. [`decisions.md`](decisions.md) — nine decisions that constrain everything.
   `D1` (no omt permission policy), `D2` (remote equals local), `D8` (two session
   modes) and `D9` (what omt may claim) change how you read every other document.
4. [`03-capability-catalog.md`](03-capability-catalog.md) — the single door.
   Every mutation and every read goes through it; it is why parity is
   mechanically checkable rather than aspirational.

**Then read the two that define the object graph**, because almost every other
document is written in their vocabulary:

5. [`02-crate-map.md`](02-crate-map.md) — the layering, and the seams that let
   changes land in parallel worktrees.
6. [`05-session-model.md`](05-session-model.md) — instance → workspace → session
   → pane, and `SessionMode`.

**After that, read by area.** Nothing below depends on anything else below.

| If you are working on… | Read |
|---|---|
| the terminal, VT parsing, blocks, reflow | [04](04-terminal-core.md), then [17](17-panes-and-layout.md) |
| agents, interactions, adapters | [06](06-agent-layer.md), then [12](12-collaboration.md) §4 |
| the wire, remote, federation | [07](07-remote-protocol.md), then [13](13-security.md), [23](23-identity-and-devices.md) |
| the web client or anything mobile | [08](08-web-client.md), then [`../design/remote-continuity.md`](../design/remote-continuity.md) |
| input, keybindings, the palette | [16](16-input-and-keymap.md), then [10](10-configuration.md) §8 |
| storage, search, retention, privacy | [20](20-recall-and-usage.md), [21](21-data-lifecycle.md) |
| running it in anger | [22](22-operations.md), then [19](19-onboarding.md) |

Two documents in [`../design/`](../design/) are **not** architecture. They are
design work that has not yet been decomposed into owning documents:
[`scenarios.md`](../design/scenarios.md) (personas, journeys, gaps, and the
prioritized requirement list every document is checked against) and
[`remote-continuity.md`](../design/remote-continuity.md) (working across devices).
Types and capabilities they propose are marked *proposed* in the glossary and in
the capability reference; do not implement them as if they were settled.

---

## What each document owns

"Owns" means: the full specification lives here, and every other document that
touches the same mechanism defers to this one with a cross-reference. This is
the table to check before writing a spec — if a mechanism already has an owner,
extend the owner rather than restating it.

| # | Document | Owns |
|---|---|---|
| 00 | [Overview](00-overview.md) | The index, the domain model, the elevator description. |
| 01 | [Principles](01-principles.md) | The nine invariants, and their names (`P1`…`P9`). |
| 02 | [Crate map](02-crate-map.md) | Crate boundaries, the layering rules, and which change owns which crate. |
| 03 | [Capability catalog](03-capability-catalog.md) | The `capability!` declaration, dispatch, `CallContext`, `Effects`, error codes, the parity contract, versioning. |
| 04 | [Terminal core](04-terminal-core.md) | `omt-term`: the VT parser, the grid, scrollback, reflow, `Position`, the block model, `TermPolicy`, selection and search primitives. |
| 05 | [Session model](05-session-model.md) | The object graph, `SessionState`, **`SessionMode`**, attach/detach, the writer token's *data model*, persistence of the session tree, command history. |
| 06 | [Agent layer](06-agent-layer.md) | `AgentAdapter`, `EventSource`, `Tier`, `AgentState`, the tiered merge, **the `Interaction` and `InteractionResponse` shapes**, `Responder`, argument editing. |
| 07 | [Remote protocol](07-remote-protocol.md) | `Transport`, `Frame`, `ProtoMessage`, the handshake, **event `seq`, resume, replay and `Resync`**, **viewport negotiation (`ViewportPolicy`)**, backpressure, the push transport. |
| 08 | [Web client](08-web-client.md) | The web package, codegen consumption, the two view modes, card rendering, gestures, PWA and offline. |
| 09 | [SSH and media](09-ssh-and-media.md) | **The blob store**, `BlobClass`, quotas and TTL, the clipboard tiers, the OSC bridge, `AttachmentReference`. |
| 10 | [Configuration](10-configuration.md) | The layered config model, the typed schema, validation and diagnostic codes, **the keybinding file format**, themes, workflows, launch configs. |
| 11 | [Plugins](11-plugins.md) | The manifest, the host protocol, the grant model and the escalation rule. |
| 12 | [Collaboration](12-collaboration.md) | `Actor`, **presence**, **the writer token's semantics**, **interaction ownership and exactly-once resolution**, ordering guarantees, optimistic UI, the audit log. |
| 13 | [Security](13-security.md) | The threat model, bind policy, `AuthBackend`, `Role`, `CredentialScope`, credential storage and rotation, egress, **the redactor's tracing/event integration**. |
| 14 | [Licensing](14-licensing.md) | Provenance, the clean-room line, `deny.toml`, `NOTICE`. |
| 15 | [Workspace explorer](15-workspace-explorer.md) | `FileTreeProvider`, `VcsProvider`, **`FileDiff`**, path confinement, the explorer surfaces. |
| 16 | [Input and keymap](16-input-and-keymap.md) | **Input semantics**: decoding, `Chord`, mouse events, `ContextSet`, resolution order, modal engines, the palette, terminal capability probing. |
| 17 | [Panes and layout](17-panes-and-layout.md) | **`LayoutTree` and geometry**, **per-client layout views**, `SessionSizing`, floats, stacks, the `pane.*` and `layout.*` capabilities. |
| 18 | [Semantic open](18-semantic-open.md) | Recognition rules, `Match`, `ResolvedTarget`, **`OpenHandler` and the `open.*` group**, hint mode, **the mouse activation policy**, remote file mirrors. |
| 19 | [Onboarding](19-onboarding.md) | First run, the status bar, help and discovery, agent detection and setup, tmux/zellij migration, **`TERM` and terminfo**, uninstall. |
| 20 | [Recall and usage](20-recall-and-usage.md) | **The recall index**, search ranking, the timeline and digest, usage accounting, the attention detector. |
| 21 | [Data lifecycle](21-data-lifecycle.md) | The on-disk inventory, **the redactor itself**, retention and the sweeper, export, purge, **durability and store versioning**. |
| 22 | [Operations](22-operations.md) | Startup and shutdown, multi-user machines, `system.doctor`, `system.health`, logging, the diagnostics panel, service install, upgrade, remote bootstrap, the automation contract. |
| 23 | [Identity and devices](23-identity-and-devices.md) | `Identity`, **`Device` and `DeviceId`**, `DeviceGrant`, pairing, the instance registry, revocation fan-out, recovery, step-up. |

### Mechanisms with a split owner

These are the places where two documents legitimately both have something to
say. The split is deliberate; the boundary is stated in both.

| Mechanism | Shape / data model | Semantics / policy |
|---|---|---|
| Writer token | [05 §5](05-session-model.md#5-the-writer-token) | [12 §3](12-collaboration.md#3-the-writer-token) — **wins on any disagreement** |
| `Interaction` | [06 §5](06-agent-layer.md#5-interactions--the-flagship-path) | [12 §4](12-collaboration.md#4-interaction-ownership) |
| Session size | [07 §4.3](07-remote-protocol.md#43-the-resize-problem) — one authoritative PTY size | [17 §3](17-panes-and-layout.md) — per-client layout around it |
| Keybindings | [10 §8.2](10-configuration.md#82-keybinding-format) — the file format | [16](16-input-and-keymap.md) — what a key means |
| Mouse | [16](16-input-and-keymap.md) — decoding and binding | [18 §5](18-semantic-open.md) — activation policy |
| Redaction | [21 §2](21-data-lifecycle.md#2-redaction-before-write) — the detector | [13 §8](13-security.md#8-secret-redaction) — the tracing/serializer integration; [20 §5](20-recall-and-usage.md) — the ordering guarantee |
| Persistence | [05 §8](05-session-model.md#8-persistence-and-restore) — the session tree | [21 §6–§7](21-data-lifecycle.md) — durability, versioning, migration |
| Credentials | [13 §3](13-security.md#3-authentication) — `AuthBackend`, `Grant`, scope | [23](23-identity-and-devices.md) — identity, devices, pairing, revocation |
| Notifications | [07 §8](07-remote-protocol.md#8-notifications-to-a-closed-tab--none-in-v1) — none in v1 (D12); the reserved `Notifier` extension point | [`../design/remote-continuity.md` §5.6](../design/remote-continuity.md#56-noise-control--deferred-and-why-the-design-is-kept) — the policy, kept but *deferred* |
| Blobs | [09 §2](09-ssh-and-media.md#2-the-blob-store) — the store | [18 §6](18-semantic-open.md) — the mirror class, as a caller |
| Doctor / health | [22 §3–§4](22-operations.md) | — everything else defers; there is no per-group `doctor.*` capability |

---

## Reference

- [`../reference/capabilities-draft.md`](../reference/capabilities-draft.md) —
  the consolidated capability table. Hand-written and temporary; the
  `capability!` declaration in the owning crate is the source of truth, and a
  disagreement means this table is stale. It is replaced by generated output
  when the codegen lands ([03 §5](03-capability-catalog.md#5-the-parity-contract)).

## Adding a document

If you are about to write a new architecture document, first check the ownership
table above. Most of what feels like a missing document is a missing *section*
in an existing one, and a new document that re-specifies an owned mechanism is
how this corpus drifts. When a new document is genuinely warranted: give it the
next number, add it to [`00-overview.md`](00-overview.md)'s index and to the
table above, register its shared types in [`glossary.md`](glossary.md), and add
its capabilities to the reference table.
