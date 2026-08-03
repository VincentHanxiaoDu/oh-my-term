# Glossary — canonical names

Every type, trait and concept that appears in more than one architecture
document, with **the one document that owns its definition** and a one-line
statement of what it is.

The rule this file exists to enforce: *one concept, one name, one owner.* If you
need to change a shape listed here, change it in the owning document and update
the others to defer. If you find a document restating a definition rather than
linking to it, that is drift and it is a bug.

Serde conventions, so the wire, the Rust type and the generated TypeScript agree:
**`snake_case` field names, `snake_case` enum variants, `type` as the internal
tag on payload/kind enums, and `t` as the tag on top-level `ProtoMessage`
frames.** No camelCase renaming anywhere.

---

## Identity and primitives

| Name | Owner | Definition |
|---|---|---|
| `InstanceId` | [02 `omt-types`](02-crate-map.md#omt-types) | UUIDv7 for one omt daemon on one machine; generated once, persisted. The join key for federation ([07 §1.2](07-remote-protocol.md#12-instance-identity)). |
| `WorkspaceId` | [05 §1.2](05-session-model.md#12-identity-and-lifetimes) | `blake3(canonical_root)[..16]` — derived, so two instances opening the same path agree without coordination. |
| `SessionId` | [05 §1.2](05-session-model.md#12-identity-and-lifetimes) | UUIDv7 for one logical terminal. Unique only *within* an instance. |
| `PaneId` | [05 §1.2](05-session-model.md#12-identity-and-lifetimes) | UUIDv7 for one viewport onto a session inside a layout. |
| `ClientId` | [05 §1.2](05-session-model.md#12-identity-and-lifetimes) | UUIDv7 minted at attach; one connection. A reconnect is a new `ClientId`. |
| `ActorId` / `DeviceId` | [12 §1](12-collaboration.md#1-actors) | `ActorId` is stable for one connection; `DeviceId` survives reconnects and is what presence and audit group by. |
| `Seq` | [03 §4](03-capability-catalog.md#4-events-are-the-read-side-twin) | Monotonic sequence number, **per session** (or per workspace for workspace-scoped events). Terminal bytes share the session's space ([07 §5.1](07-remote-protocol.md#51-sequence-spaces)). Never sort by `ts`. |
| `RequestId` | [07 §3.2](07-remote-protocol.md#32-the-envelope) | Client-generated, unique per connection; echoed on the response and carried as `caused_by` on the resulting events. |
| `Position` | [04 §3.2](04-terminal-core.md#32-positions) | A width-independent `(logical line, offset, to_eol)` location in a session's content. Everything durable is a `Position`; nothing durable is an `(x, y)`. |
| `Role` | [13 §4](13-security.md#4-roles-and-their-mapping-onto-the-catalog) | `Viewer < Operator < Admin`. Exactly three; there is no sub-`Viewer` role. A **sharing** control, not a way to degrade the owner's own devices (D2). |
| `CredentialScope` | [13 §4.1](13-security.md#41-credential-scope) | `{ visibility: InviteScope, capabilities: Option<Set<CapabilityPattern>> }` — the only narrowing mechanism besides the role. |
| `CanonicalPath` | [05 §7](05-session-model.md#7-workspace-identity) | A fully resolved path (symlinks, `..`, case per filesystem). Workspace identity. |
| `RelPath` | [15 §9.1](15-workspace-explorer.md#91-path-confinement) | Workspace-relative, `/`-separated, no trailing slash, validating constructor. Directories carry no trailing `/`. |

## Capabilities

| Name | Owner | Definition |
|---|---|---|
| `Capability` | [03 §2](03-capability-catalog.md#2-declaring-a-capability) | One named operation, declared once with `name`, `group`/`verb`, `kind`, `role`, `input`/`output`, `effects`, `since`. |
| `CapabilityRegistry` / dispatch | [03 §3](03-capability-catalog.md#3-dispatch) | `name → ErasedHandler`; the single dispatch path. Authorization happens here, not in a transport, so no surface can bypass it. |
| `CallContext` | [03 §3](03-capability-catalog.md#3-dispatch) | `{ actor, role, request_id, deadline }` carried into every handler. |
| `Effects` | [03 §2](03-capability-catalog.md#2-declaring-a-capability) | Closed bit set: `WRITES_PTY`, `SPAWNS_PROCESS`, `READS_FS`, `WRITES_FS`, `NETWORK`, `DESTRUCTIVE`. Describes what **omt** does, never what an agent's tool does (D1). |
| Capability error codes | [03 §3](03-capability-catalog.md#3-dispatch) | Closed enum: `not_found`, `conflict`, `unauthorized`, `precondition_failed`, `unsupported`, `internal`. Protocol adds `unsupported_proto`, `auth_failed`, `rate_limited`, `frame_too_large` ([07 §3.5](07-remote-protocol.md#35-capability-call--result)). Clients switch on `code`, never on `message`. |
| Consolidated capability list | [`docs/reference/capabilities-draft.md`](../reference/capabilities-draft.md) | Hand-written interim table; replaced by generated output per [03 §5](03-capability-catalog.md#5-the-parity-contract). |

## Events

| Name | Owner | Definition |
|---|---|---|
| `Event` envelope | [02 `omt-events`](02-crate-map.md#omt-events) | `{ instance, session?, workspace?, seq, ts, source, caused_by?, payload }`. Emitted as `EventEnvelope` in generated TS ([08 §2.1](08-web-client.md#21-what-codegen-emits)). |
| `EventSource` (trait) | [06 §3](06-agent-layer.md#3-source-model) | An observation source: `id()`, `tier()`, `supports(kind)`, `attach(binding, sink)`. Never writes to the PTY. |
| `source` tag | [08 §2.1](08-web-client.md#21-what-codegen-emits) | `hook \| protocol \| transcript \| marker \| process \| pty \| workspace_fs \| system`. |
| `Tier` | [06 §3](06-agent-layer.md#3-source-model) | `Heuristic=0 < Process=1 < Marker=2 < Transcript=3 < Hook=4 < Protocol=5`. **Higher is more authoritative.** Only tiers ≥ 3 may emit structured content. |
| `AgentEvent` | [06 §8](06-agent-layer.md#8-ancillary-semantics) | The normalized per-session agent stream: state, tool calls, `FileChanged`, queue, usage, subagents, summaries. |
| `EventBus` / backpressure | [07 §6](07-remote-protocol.md#6-backpressure) | Per-*subscription* lossy (terminal bytes) and lossless (everything else) queues. An `Interaction` is never silently dropped. |
| `Resync` | [07 §5.2](07-remote-protocol.md#52-replay-window) | "Discard local state for this session and rebuild." A normal, expected message, not an error. |

## Agent layer

| Name | Owner | Definition |
|---|---|---|
| `AgentAdapter` | [06 §7](06-agent-layer.md#7-adapters) | Per-agent knowledge: fingerprint, spawn env, sources, responders, integration installer, commands, `path_mention`, `image_reference`. |
| `AgentBinding` | [06 §2](06-agent-layer.md#2-the-two-axis-model) | *Which* agent occupies a session, with a lifetime and the agent's own session id. Retained evidence is cleared on binding end. |
| `AgentState` | [06 §4](06-agent-layer.md#4-merging-confidence-tiers-not-voting) | `Starting \| Idle \| Working \| Blocked \| Exited \| Unknown`. The **only** vocabulary for agent activity. There is no `busy` or `needs_attention` state. |
| `Activity` | [06 §6](06-agent-layer.md#6-the-heuristic-floor) | `Busy \| Idle \| NeedsAttention \| Unknown` — an internal *input* of the heuristic source, never an observable state. |
| `Interaction` | [06 §5](06-agent-layer.md#5-interactions--the-flagship-path) | A request from an agent for a human decision, promoted to an addressable object. Shape owned by 06; **concurrency semantics owned by [12 §4](12-collaboration.md#4-interaction-ownership)**. |
| `InteractionKind` | [06 §5](06-agent-layer.md#5-interactions--the-flagship-path) | `choice \| permission \| plan_review \| text`. `permission` carries `{ tool, input, command, diff: Option<FileDiff>, options }`. The field is `options`, not `suggestions`. |
| `InteractionState` | [12 §4.1](12-collaboration.md#41-the-invariant) | `Open \| Resolving \| Resolved \| Cancelled \| Abandoned`. |
| `InteractionResponse` | [06 §5.0](06-agent-layer.md#50-interactionresponse) | `choices \| permission \| text \| plan_review`. `choices` carries `answers: Vec<ChoiceAnswer>`, index-aligned to the questions. |
| `ChoiceQuestion` | [06 §5](06-agent-layer.md#5-interactions--the-flagship-path) | `{ question, header, multi_select, options, allow_free_text }`. Maps 1:1 onto Claude Code's `AskUserQuestion`. |
| `PermissionOption` | [06 §5](06-agent-layer.md#5-interactions--the-flagship-path) | The agent's own suggestion, passed through unchanged: `{ id, label, kind }` where kind is `allow \| allow_always \| deny \| deny_always \| edit`. omt never adds, removes or reorders them (D1). |
| `Responder` | [06 §5.2](06-agent-layer.md#52-responders--how-the-answer-gets-back) | How an answer reaches the agent. Reports `fidelity: Native \| Synthetic` and `state_dependence: Independent \| Inferred`. |
| `StateDependence` | [D3](decisions.md#d3--synthetic-input-is-bounded-by-state-dependence-not-by-tool-danger) | The axis that bounds synthetic PTY input. `Independent` (position-independent answers) is on by default; `Inferred` (arrow-key counting) is off by default. **Not** tool danger. |
| `InteractionLedger` | [12 §4.1](12-collaboration.md#41-the-invariant) | The compare-and-swap that makes resolution exactly-once. Lives in `omt-agent`. |

## Session tree

| Name | Owner | Definition |
|---|---|---|
| `Instance` / `Workspace` / `Session` / `Pane` | [05 §1](05-session-model.md#1-the-object-model) | The object graph. Pane → Session is many-to-one; a pane is presentation only. |
| `Layout` | [05 §2](05-session-model.md#2-layout-the-bsp-tree) | An n-ary BSP tree with fractional weights; no `Split` has a same-direction child. |
| `SessionState` | [05 §1.1](05-session-model.md#11-types) | `Starting \| Live \| Exited \| Orphaned \| Closing`. `Orphaned` is a session restored after a daemon restart whose process is gone — **PTYs do not survive a restart in v1.** |
| `WriterToken` | [12 §3.2](12-collaboration.md#32-state) | `{ holder, acquired_at, last_input_at, epoch, keep_size, takeover }`. Gates `Effects::WRITES_PTY` only. Data model in [05 §5](05-session-model.md#5-the-writer-token); **semantics in [12 §3](12-collaboration.md#3-the-writer-token)**, which wins. |
| `Presence` / `ViewFocus` / `Liveness` | [12 §2](12-collaboration.md#2-presence-is-first-class-state) | First-class broadcast state: who is attached, what they are viewing, whether they are writing. Projected per session by `omt-session` ([05 §6](05-session-model.md#6-presence)). |
| `Actor` / `ActorKind` | [12 §1](12-collaboration.md#1-actors) | Everything that can cause a state change. `Local \| Remote \| Cli \| Plugin \| System \| Agent`. There are no anonymous mutations. |
| `ViewMode` | [05 §4](05-session-model.md#4-attachment-detach-and-multi-client-viewing) | `Grid \| Blocks` — which surface a client attached with. Block view receives no PTY bytes. |
| `HistoryEntry` / `HistoryQuery` | [05 §9](05-session-model.md#9-command-history) | The durable, structured list of commands, populated from block closure. |

## Terminal core

| Name | Owner | Definition |
|---|---|---|
| `Terminal` / `Snapshot` / `Damage` | [04](04-terminal-core.md) | Pure state machine: bytes in, state and damage out. No I/O, no async. |
| `Block` / `BlockState` / `BlockOrigin` | [04 §6](04-terminal-core.md#6-the-block-model) | A command block: metadata plus a range of `Position`s. States are `at_prompt \| submitted \| running \| finished \| no_execution \| background \| truncated`; origins are `osc133 \| heuristic \| injected`. |
| `Attribution` | [04 §6.2](04-terminal-core.md#62-what-a-block-owns) | `human \| agent \| unknown` — who ran a block. Computed by the agent layer, stored by `omt-term`. |
| `StyledLine` | [04 §4.4](04-terminal-core.md#44-the-web-mapping-xtermjs) | `{ text, spans: [{ start, len, fg, bg, flags, link? }] }` — the structured encoding used by block view and `session.scrollback.get`. Not raw bytes. |
| `TermPolicy` | [04 §5.6](04-terminal-core.md#56-two-orthogonal-gating-axes) | Escape-sequence gating (clipboard, focus reporting, window ops, file transfer). `trusted` is never settable from the wire. |
| `ScrollbackLimits` | [04 §2.5](04-terminal-core.md#25-bounding-memory) | `max_lines`, `max_bytes`, `max_image_bytes_total` (per session). Distinct from `TermConfig::max_image_bytes`, which caps *one* image. |

## Remote protocol

| Name | Owner | Definition |
|---|---|---|
| `Transport` | [07 §2.1](07-remote-protocol.md#21-the-trait) | Framing only, no auth and no routing: WebSocket, Unix socket, SSH stdio. |
| `Frame` | [07 §2.1](07-remote-protocol.md#21-the-trait) | Exactly two kinds: `Text` (JSON `ProtoMessage`) and `Binary` (8-byte header + payload). |
| `ProtoMessage` | [07 §3.2](07-remote-protocol.md#32-the-envelope) | The control-message enum, tagged `t`. |
| `ViewportPolicy` / `SizeOwner` | [07 §4.3](07-remote-protocol.md#43-the-resize-problem) | One authoritative `(cols, rows)` per session, owned by `Writer` (default), `Pinned { by }`, or `Smallest` (opt-in). Everyone else renders scaled-to-fit and letterboxed. |
| `grid_v1` | [07 §4.2](07-remote-protocol.md#42-the-decision-c-hybrid-byte-stream-primary) | The run-length grid snapshot sent on attach, resume-outside-window and resync; live bytes follow it. Encoding still open ([07 §9.1](07-remote-protocol.md#9-open-questions)). |
| `AuthBackend` / `Grant` | [13 §3.1](13-security.md#31-the-trait) | Issues and verifies credentials; performs no transport work and no routing. |

## Storage, config, plugins, media, files

| Name | Owner | Definition |
|---|---|---|
| `Store` | [02 `omt-store`](02-crate-map.md#omt-store) | Append-only log plus snapshots for the session tree, scrollback, blocks, ledger, audit and credentials. |
| `Config` / `SettingDescriptor` | [10 §4.1](10-configuration.md#41-one-type-many-artifacts) | The Rust type is the schema; every other artifact is generated from it. Carries `scope`, `owner`, `min_role`, `reload`, `surfaces`. |
| `SecretRef` | [10 §1.3](10-configuration.md#13-secrets-are-a-separate-file-with-enforced-permissions) | A named reference to a secret. `Debug`/`Display`/`Serialize` emit `"<redacted>"` — a type-level guarantee. |
| Diagnostic codes `OMT-C###` / `OMT-P###` | [10 §5.3](10-configuration.md#53-diagnostic-rendering) | Stable, documented, greppable config and plugin diagnostic codes. |
| `SttProvider` | [02 `omt-stt`](02-crate-map.md#omt-stt) | Audio in, interim + final transcripts out. BYOK only; keys live on the instance, never in the browser ([08 §7.3](08-web-client.md#73-provider-selection)). |
| `BlobStore` / `BlobId` / `BlobMeta` | [09 §2](09-ssh-and-media.md#2-the-blob-store) | One content-addressed, quota'd, TTL'd landing zone. Every media path is a different way of getting bytes into it. |
| `ImageReference` | [09 §7.1](09-ssh-and-media.md#71-handing-the-image-to-the-agent) | How an agent wants to be handed an on-disk image: `PromptText \| Command \| Structured`. `Structured` is always preferred. |
| Media tiers 0–4 | [09 §5.1](09-ssh-and-media.md#51-tier-overview) | The ladder for image paste over SSH: omt-on-both-ends → reverse socket → in-band OSC bridge → terminal-native → out-of-band. A tier is never claimed without a positive handshake. |
| `FileTreeProvider` / `VcsProvider` | [15 §3](15-workspace-explorer.md#3-traits-and-types) | The two extension points of `omt-workspace-fs`. Read-only: the crate ships zero write capabilities. |
| `FileDiff` / `Hunk` / `DiffLine` | [15 §3.2](15-workspace-explorer.md#32-vcs-model) | Structured hunks on the wire, never a unified-diff string. Used by the explorer **and** by permission cards ([15 §8.5](15-workspace-explorer.md#85-diffs-inside-permission-cards)), so there is one diff renderer per surface. |
| `VcsSummary` / `VcsFileState` | [15 §3.2](15-workspace-explorer.md#32-vcs-model) | Repository summary, and per-file status with `index` and `worktree` as *separate* axes. |
| Plugin manifest / `granted` | [11 §2](11-plugins.md#2-manifest) | `omt-plugin.toml` declares, the user consents, the host enforces. The plugin API *is* the capability catalog, filtered. |

---

## Names that were unified, and what they replaced

Recorded so a stale draft is recognisable.

| Canonical | Replaced |
|---|---|
| `AgentState::Blocked` / `Working` | `needs_attention`, `busy` |
| `Tier::Heuristic = 0 … Protocol = 5` | the inverted 0–6 table in an earlier 00 §5 |
| `InteractionKind::Permission { options }` | `suggestions` |
| `Interaction::timeout_at` | `expires_at`, `timeout_ms` |
| `interaction.resolve { interaction, response }` | `{ session, interaction_id, response }` |
| `InteractionResponse::Choices { answers }` | `{ kind: "choices", choices: [[…]] }` |
| `BlockState::{at_prompt,…,truncated}` | another terminal-shaped `before_execution`/`executing`/`done`/`static` |
| `Effects::{READS_FS, WRITES_FS}` | `TOUCHES_FS` |
| `WriterToken` + `writer.acquire { force }` + `writer.keep` | `Writer` + `writer.takeover` + `writer.respond` + `TakeoverPolicy` |
| `ViewportPolicy`/`SizeOwner` ([07 §4.3](07-remote-protocol.md#43-the-resize-problem)) | "minimum over non-lazy viewers" + `SizePolicy::{Participant,Observer}`; client-side `fit_width` |
| `CredentialScope` | `CredentialPolicy` (removed by [D1](decisions.md#d1--omt-adds-no-policy-layer-over-an-agents-permission-semantics)) |
| `Viewer` + capability scope | the proposed `Media` role |
| `notification.push.subscribe` | `notify.subscribe` |
| `workspace.vcs.summary` | `workspace.git.status` (deprecated alias) |
| port `7878` | `7681` |
