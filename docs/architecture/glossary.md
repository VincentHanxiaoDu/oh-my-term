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
| `PaneId` | [05 §1.2](05-session-model.md#12-identity-and-lifetimes) | UUIDv7 for one viewport onto a session inside **one `LayoutView`**. Never crosses views: two views of one session hold two `PaneId`s. |
| `IntentId` | [12 §4.1](12-collaboration.md#41-the-invariant) | Client-minted, persisted client-side. Identifies *the intent*, not the attempt, so a retry is recognised as the same intent even when the response was edited in between. Part of every dedup key ([03 §2.2](03-capability-catalog.md#22-intent-class-d15)). |
| `ClientId` | [05 §1.2](05-session-model.md#12-identity-and-lifetimes) | UUIDv7 minted at attach; one connection. A reconnect is a new `ClientId`. |
| `ActorId` | [12 §1](12-collaboration.md#1-actors) | Stable for one connection. A reconnect is a new `ActorId` — which is why nothing needing to survive a reconnect may be keyed on it. Idempotency keys use the identity or `DeviceId` instead ([12 §4.1](12-collaboration.md#41-the-invariant)). |
| `DeviceId` | [23 §1.1](23-identity-and-devices.md#11-four-types) | Survives reconnects; what presence and audit group by. **Used** by [12 §1](12-collaboration.md#1-actors) (`ActorKind::Remote`) and [12 §2](12-collaboration.md#2-presence-is-first-class-state) (`Presence`), but **owned** by 23, which gives it a durable `Device` record. The id itself lives in [`omt-types`](02-crate-map.md#omt-types), not in whichever crate mints it. |
| `Seq` | [03 §4](03-capability-catalog.md#4-events-are-the-read-side-twin) | Monotonic sequence number, **per session** (or per workspace for workspace-scoped events). Terminal bytes share the session's space ([07 §5.1](07-remote-protocol.md#51-sequence-spaces)). Never sort by `ts`. |
| `RequestId` | [07 §3.2](07-remote-protocol.md#32-the-envelope) | Client-generated `(DeviceId, monotonic u64)`, persisted client-side so it is **stable across reconnects**; echoed on the response and carried as `caused_by` on the resulting events. Unique-per-*connection* was the old definition and left a client whose socket died mid-call unable to learn whether the call applied ([D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism) consequence 5). |
| `Position` | [04 §3.2](04-terminal-core.md#32-positions) | A width-independent `(logical line, offset, to_eol)` location in a session's content. Everything durable is a `Position`; nothing durable is an `(x, y)`. |
| `Role` | [13 §4](13-security.md#4-roles-and-their-mapping-onto-the-catalog) | `Viewer < Operator < Admin`. Exactly three; there is no sub-`Viewer` role. A **sharing** control, not a way to degrade the owner's own devices (D2). |
| `CredentialScope` | [13 §4.1](13-security.md#41-credential-scope) | `{ visibility: InviteScope, capabilities: Option<Set<CapabilityPattern>> }` — the only narrowing mechanism besides the role. |
| `CanonicalPath` | [05 §7](05-session-model.md#7-workspace-identity) | A fully resolved path (symlinks, `..`, case per filesystem). Workspace identity. |
| `RelPath` | [15 §9.1](15-workspace-explorer.md#91-path-confinement) | Workspace-relative, `/`-separated, no trailing slash, validating constructor. Directories carry no trailing `/`. |

## Capabilities

| Name | Owner | Definition |
|---|---|---|
| `Capability` | [03 §2](03-capability-catalog.md#2-declaring-a-capability) | One named operation, declared once with `name`, `group`/`verb`, `kind`, `role`, `input`/`output`, `effects`, `refine_effects`, `intent`, `title`/`aliases`/`hidden`, `since`. |
| `CapabilityRegistry` / dispatch | [03 §3](03-capability-catalog.md#3-dispatch) | `name → ErasedHandler`; the single dispatch path. Authorization happens here, not in a transport, so no surface can bypass it. |
| `CallContext` | [03 §3](03-capability-catalog.md#3-dispatch) | `{ actor, role, request_id, deadline }` carried into every handler. |
| `Effects` | [03 §2](03-capability-catalog.md#2-declaring-a-capability) | Closed bit set: `WRITES_PTY`, `SPAWNS_PROCESS`, `READS_FS`, `WRITES_FS`, `NETWORK`, `DESTRUCTIVE`. Describes what **omt** does, never what an agent's tool does (D1). The declared set is the **maximum** over all inputs. |
| `refine_effects` | [03 §2.1](03-capability-catalog.md#21-conditional-effects) | Optional pure `fn(&Input) -> EffectBits` narrowing a call's effects — `pane.split` spawns a process only when `session` is `None`, `pane.close` is destructive only when `close_session`. **May only remove bits, never add them.** Static consumers (authorization, generated docs) read the maximum; the confirm gesture and the audit log read the refined set. |
| `Intent` | [03 §2.2](03-capability-catalog.md#22-intent-class-d15) | `Cas \| Append { dedup } \| RawStream \| ExternallyConfirmed \| Lww` — [D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)'s five delivery classes. Every `Command` declares one (no default; omitting it fails the build) and dispatch reads it to decide whether a repeat may be replayed, deduped, or must be rejected. Orthogonal to `Effects`: `Intent` is how to deliver, `Effects` is what it does. |
| Capability error codes | [03 §3](03-capability-catalog.md#3-dispatch) | Closed enum: `not_found`, `conflict`, `unauthorized`, `precondition_failed`, `unsupported`, `internal`. Protocol adds `unsupported_proto`, `auth_failed`, `rate_limited`, `frame_too_large` ([07 §3.5](07-remote-protocol.md#35-capability-call--result)). Clients switch on `code`, never on `message`. |
| Consolidated capability list | [`docs/reference/capabilities-draft.md`](../reference/capabilities-draft.md) | Hand-written interim table; replaced by generated output per [03 §5](03-capability-catalog.md#5-the-parity-contract). |

## Events

| Name | Owner | Definition |
|---|---|---|
| `Event` envelope | [02 `omt-events`](02-crate-map.md#omt-events) | `{ instance, session?, workspace?, seq, ts, source, caused_by?, payload }`. Emitted as `EventEnvelope` in generated TS ([08 §2.1](08-web-client.md#21-what-codegen-emits)). |
| `EventKind` | [07 §3.7.1](07-remote-protocol.md#371-the-kinds-and-the-payload-each-carries) | The closed subscription grouping: `terminal, agent, interaction, session_tree, presence, config, plugin, audit, workspace_fs, instance`. Ten — `instance` was added so 22's `instance.degraded` is subscribable and so instance-scoped events have a `Seq` space. |
| `EventPayload` | [07 §3.7.1](07-remote-protocol.md#371-the-kinds-and-the-payload-each-carries) | The full variant set behind every `EventKind`, tagged `type`. Inner types are referenced from their owning documents, never restated. |
| `EventSource` (trait) | [06 §3](06-agent-layer.md#3-source-model) | An observation source: `id()`, `tier()`, `supports(kind)`, `attach(binding, sink)`. Never writes to the PTY. Tier-0 sources implement `HeuristicSource` instead. |
| `HeuristicSource` / `ActivitySink` | [06 §8.4](06-agent-layer.md#84-which-tier-may-produce-which-payload) | The tier-0 trait and its sink. `ActivitySink::emit` takes an `Activity` and nothing else, so "screen text became a tool call" is unrepresentable rather than forbidden by a rule. |
| `EventSourceTag` (the `source` tag) | [07 §3.7.3](07-remote-protocol.md#373-the-source-vocabulary--one-closed-set) | The single closed producer set: `heuristic, process, marker, transcript, hook, protocol` (exactly `Tier`'s variants, for agent observations) plus `core, fs, plugin` (producers with no tier). **There is no `pty`, `workspace_fs` or `system` tag**; 08 §2.1's earlier list conflated a confidence tier with a producer class and regenerates from this. |
| `Tier` | [06 §3](06-agent-layer.md#3-source-model) | `Heuristic=0 < Process=1 < Marker=2 < Transcript=3 < Hook=4 < Protocol=5`. **Higher is more authoritative.** Only tiers ≥ 3 may emit structured content; the per-payload table is [06 §8.4](06-agent-layer.md#84-which-tier-may-produce-which-payload). |
| `AgentEvent` | [06 §8.1](06-agent-layer.md#81-agentevent--the-envelope) | The envelope of the normalized per-binding agent stream: `{ session, binding, agent, agent_version, agent_session, thread, seq, ts, tier, source, cwd, git_branch, payload }`. Wrapped by `EventPayload::AgentEvent` under `kind: agent`; persisted standalone as `agent.jsonl.zst` ([21 §1](21-data-lifecycle.md)), which is why `seq`/`ts` appear in both envelopes as one value written twice. |
| `AgentPayload` | [06 §8.2](06-agent-layer.md#82-agentpayload--the-variants) | The 20 payload variants in seven groups: lifecycle, turn state, content, tools, interaction, queue, fallback. Other documents' `AgentEvent::FileChanged` is shorthand for `AgentPayload::FileChanged { path, change, tool, turn }`. |
| `EventBus` / backpressure | [07 §6](07-remote-protocol.md#6-backpressure) | Per-*subscription* lossy (terminal bytes) and lossless (everything else) queues. An `Interaction` is never silently dropped. |
| `Resync` | [07 §5.2](07-remote-protocol.md#52-replay-window) | "Discard local state for this session and rebuild." A normal, expected message, not an error. |

## Agent layer

| Name | Owner | Definition |
|---|---|---|
| `AgentAdapter` | [06 §7](06-agent-layer.md#7-adapters) | Per-agent knowledge: fingerprint, spawn env, sources, responders, integration installer, commands, `path_mention`, `attachment_reference`. |
| `SessionModeSet` | [05 §1.1](05-session-model.md#11-types) | `bitflags u8`: `PTY_ONLY, NATIVE` — which `SessionMode`s an adapter supports ([06 §7](06-agent-layer.md#7-adapters)); `PTY_ONLY` is the default. Distinct from the keymap's `ModeSet` ([16 §6.5](16-input-and-keymap.md#65-the-keymap-abstraction)), and defined in 05 alongside `SessionMode` because it is a set of those. |
| `AcpSpawn` | [06 §7](06-agent-layer.md#7-adapters) | Argv and env for spawning an agent in `SessionMode::Native`, returned by `AgentAdapter::acp_spawn`; `None` when the adapter has no native mode. Named but **defined nowhere** — open. |
| `ThreadRef` | [06 §8.1](06-agent-layer.md#81-agentevent--the-envelope) | `{ id: ThreadId, parent, is_subagent, label }` — the subagent tree inside one binding. Unifies Claude Code's `isSidechain`, Codex's subagent thread source and opencode's `session.parent_id`. Used by [20 §11.2](20-recall-and-usage.md#112-normalization)'s `UsageEvent`. |
| `AgentSessionId` | [06 §2](06-agent-layer.md#2-the-two-axis-model) | The **agent's own** session identifier (`CLAUDE_CODE_SESSION_ID`, `CODEX_THREAD_ID`, an ACP session id), carried on `AgentBinding` and on every `AgentEvent`. Not an omt id and never minted by omt. |
| `ThreadId` / `TurnId` / `ToolCallId` / `QueueEntryId` | [06 §8](06-agent-layer.md#8-ancillary-semantics) | Opaque ids inside one binding. `TurnId` and `ToolCallId` are **the agent's own** where it supplies them (a `tool_use_id`), which is what lets a `PreToolUse` be correlated with its `PostToolUse` and therefore what makes D15's confirm-by-observation work. |
| `QueueEntry` / `QueueView` | [06 §8.2](06-agent-layer.md#82-agentpayload--the-variants) | One queued message (`{ id, text, origin, created_at, valid_until, state }`) and the queue as reported: `Known(Vec<QueueEntry>) \| Unknown`. `Unknown` exists so a stale or absent reader cannot claim an empty queue — §8's fourth queue requirement, made a type. |
| `AgentBinding` | [06 §2](06-agent-layer.md#2-the-two-axis-model) | *Which* agent occupies a session, with a lifetime and the agent's own session id. Retained evidence is cleared on binding end. |
| `AgentState` | [06 §4](06-agent-layer.md#4-merging-confidence-tiers-not-voting) | `Starting \| Idle \| Working \| Blocked \| Exited \| Unknown`. The **only** vocabulary for agent activity. There is no `busy` or `needs_attention` state. |
| `Activity` | [06 §6](06-agent-layer.md#6-the-heuristic-floor) | `Busy \| Idle \| NeedsAttention \| Unknown` — an internal *input* of the heuristic source, never an observable state. |
| `Interaction` | [06 §5](06-agent-layer.md#5-interactions--the-flagship-path) | A request from an agent for a human decision, promoted to an addressable object. Shape owned by 06; **concurrency semantics owned by [12 §4](12-collaboration.md#4-interaction-ownership)**. |
| `InteractionKind` | [06 §5](06-agent-layer.md#5-interactions--the-flagship-path) | `choice \| permission \| plan_review \| text`. `permission` carries `{ tool, input, command, diff: Option<FileDiff>, options }`. The field is `options`, not `suggestions`. |
| `InteractionState` | [12 §4.1](12-collaboration.md#41-the-invariant) | `Open \| Resolving { by, response } \| Submitted { by, response, at } \| Resolved \| Undelivered { reason, response } \| Cancelled \| Abandoned`. `Submitted` means the bytes were written; **`Resolved` requires omt to have *observed* the agent record the answer** ([D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism) consequence 1, [12 §4.5](12-collaboration.md#45-the-local-keyboard-racing-an-injected-answer)). `Resolving` carries the response so a crash can report what was lost. |
| `InteractionResponse` | [06 §5.0](06-agent-layer.md#50-interactionresponse) | `choices \| permission \| text \| plan_review`. `choices` carries `answers: Vec<ChoiceAnswer>`, index-aligned to the questions. |
| `ChoiceQuestion` | [06 §5](06-agent-layer.md#5-interactions--the-flagship-path) | `{ question, header, multi_select, options, allow_free_text }`. Maps 1:1 onto Claude Code's `AskUserQuestion`. |
| `PermissionOption` | [06 §5](06-agent-layer.md#5-interactions--the-flagship-path) | The agent's own suggestion, passed through unchanged: `{ id, label, kind }` where kind is `allow \| allow_always \| deny \| deny_always \| edit`. omt never adds, removes or reorders them (D1). |
| `Responder` | [06 §5.2](06-agent-layer.md#52-responders--how-the-answer-gets-back) | How an answer reaches the agent. Reports `fidelity: Native \| Synthetic` and `state_dependence: Independent \| Inferred`. |
| `StateDependence` | [D3](decisions.md#d3--synthetic-input-is-bounded-by-state-dependence-not-by-tool-danger) | The axis that bounds synthetic PTY input. `Independent` (position-independent answers) is on by default; `Inferred` (arrow-key counting) is off by default. **Not** tool danger. |
| `InteractionLedger` | [12 §4.1](12-collaboration.md#41-the-invariant) | The compare-and-swap that makes resolution exactly-once. Lives in `omt-agent`. |

## Session tree

| Name | Owner | Definition |
|---|---|---|
| `Instance` / `Workspace` / `Session` / `Pane` | [05 §1](05-session-model.md#1-the-object-model) | The object graph. Pane → Session is many-to-one; a pane is presentation only. A `Workspace` holds `views: IndexMap<ViewId, LayoutView>` plus a `primary`, **not** a single `layout` — focus and zoom live inside a view. |
| `SessionMode` | [05 §1.1](05-session-model.md#11-types) | `Pty \| Native` ([D8](decisions.md#d8--two-session-modes-pty-default-and-native-acp)). Chosen at creation, immutable for the session's life. Lives in [`omt-types`](02-crate-map.md#omt-types). |
| `SessionSurface` | [05 §1.1](05-session-model.md#11-types) | `Pty { pty, term } \| Native { conn, transcript }` — makes "a native session has no PTY" unrepresentable rather than an unwrap site. `Session` carries both `mode` and `surface`. |
| `Layout` | [17 §1.3](17-panes-and-layout.md#13-the-whole-layout-of-a-workspace) | `{ tiles: LayoutTree, floats, zoom, stacks, focus, last_focus }` — the whole layout of **one `LayoutView`**, not of a workspace directly. A struct, never an enum; the tree itself is `LayoutTree`. `zoom` is `Option<PaneId>` beside the tree, so zoom is **per view** ([17 §5.1](17-panes-and-layout.md#51-zoom)) and there is no `Zoom` tree variant. |
| `SessionState` | [05 §1.1](05-session-model.md#11-types) | `Starting \| Live \| Exited \| Orphaned \| Closing`. `Orphaned` is a session restored after a daemon restart whose process is gone — **PTYs do not survive a restart in v1.** |
| `WriterToken` | [12 §3.2](12-collaboration.md#32-state) | `{ holder, acquired_at, last_input_at, epoch, keep_size, takeover }`. Gates `Effects::WRITES_PTY` only. Data model in [05 §5](05-session-model.md#5-the-writer-token); **semantics in [12 §3](12-collaboration.md#3-the-writer-token)**, which wins. |
| `Presence` / `ViewFocus` / `Liveness` | [12 §2](12-collaboration.md#2-presence-is-first-class-state) | First-class broadcast state: who is attached, what they are viewing, whether they are writing. Projected per session by `omt-session` ([05 §6](05-session-model.md#6-presence)). **`ViewFocus` is keyed on `(ViewId, SessionId)`, never a bare `PaneId`** — a pane belongs to one `LayoutView` and means nothing to a client in another. |
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
| `HookEvent` / `HookAck` | [07 §3.8](07-remote-protocol.md#38-the-omt-hook-wire-messages) | The two `ProtoMessage` variants `omt-hook` speaks, over the unix socket only. **Deliberately not a capability call**: a hook reports an observation, has no `Role` and no `RequestId`, and the `SO_PEERCRED` same-uid check is the authorization that fits. Audited via the agent event log and `agent.explain`, not the audit log. |
| `HookDirective` | [07 §3.8.2](07-remote-protocol.md#382-the-messages) | `Proceed \| Defer { budget_ms } \| Deny { reason }`. **Only `Proceed` is sent in v1** — `Defer` is 06 §5.3's demoted optimization and `Deny` would be omt adding policy, which D1 forbids. Rendering it into the agent-appropriate stdout document is the **hook binary's** job, not the daemon's, because the fail-open path is exactly the path where the daemon is unreachable. |
| `HookCorrelation` | [07 §3.8.2](07-remote-protocol.md#382-the-messages) | `{ instance, session, pid, ppid, cwd }` — from `OMT_INSTANCE`/`OMT_SESSION` ([06 §7.2](06-agent-layer.md#72-correlation)), with `session: None` for an agent omt did not spawn. The hook never guesses a session id. |
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
| `BlobClass` | [09 §2](09-ssh-and-media.md#2-the-blob-store) | `Runtime \| Mirror { host, remote_path, read_only }` — which lifetime/quota class a blob belongs to. `Mirror` is what [18 §6.3](18-semantic-open.md#63-where-the-file-lands-and-why-the-layout-matters)'s remote file mirror uses; it is not a second store. |
| `AttachmentReference` | [09 §4.3.7](09-ssh-and-media.md#437-what-the-agent-finally-receives) | How an agent wants to be handed an on-disk attachment: `PromptText \| InlineContent { fence, body, name } \| Command { command, then } \| Structured`. Returned by `AgentAdapter::attachment_reference`, whose trait is owned by [06 §7](06-agent-layer.md#7-adapters). One type for every attachment class; there is no separate image method. |
| Media tiers 0–4 | [09 §5.1](09-ssh-and-media.md#51-tier-overview) | The ladder for image paste over SSH: omt-on-both-ends → reverse socket → in-band OSC bridge → terminal-native → out-of-band. A tier is never claimed without a positive handshake. |
| `FileTreeProvider` / `VcsProvider` | [15 §3](15-workspace-explorer.md#3-traits-and-types) | The two extension points of `omt-workspace-fs`. Read-only: the crate ships zero write capabilities. |
| `FileDiff` / `Hunk` / `DiffLine` | [15 §3.2](15-workspace-explorer.md#32-vcs-model) | Structured hunks on the wire, never a unified-diff string. Used by the explorer **and** by permission cards ([15 §8.5](15-workspace-explorer.md#85-diffs-inside-permission-cards)), so there is one diff renderer per surface. |
| `VcsSummary` / `VcsFileState` | [15 §3.2](15-workspace-explorer.md#32-vcs-model) | Repository summary, and per-file status with `index` and `worktree` as *separate* axes. |
| `ExplorerRef` | [15 §8.1](15-workspace-explorer.md#81-fileline-from-terminal-output) | `{ workspace: WorkspaceId, rel: RelPath, line: Option<u32>, col: Option<u32> }` — how a resolved target names a place in the explorer. Carried as `ResolvedTarget::explorer`, which [18 §3](18-semantic-open.md#3-resolution) owns. |
| Plugin manifest / `granted` | [11 §2](11-plugins.md#2-manifest) | `omt-plugin.toml` declares, the user consents, the host enforces. The plugin API *is* the capability catalog, filtered. |

## Input and keymap

| Name | Owner | Definition |
|---|---|---|
| `RawKey<'a>` | [16 §2.2](16-input-and-keymap.md#22-types) | `{ bytes: &[u8], decoded: KeyEvent, encoding: KeyEncoding }` — the undecoded truth kept alongside the decode, so passthrough is byte-exact. |
| `KeyEncoding` | [16 §2.2](16-input-and-keymap.md#22-types) | `Legacy \| ModifyOtherKeys \| Kitty { flags: u8 }` — how the outer terminal encoded the key. |
| `KeyEvent` | [16 §2.2](16-input-and-keymap.md#22-types) | `{ code: KeyCode, mods: Mods, kind: KeyEventKind, text: Option<SmolStr>, base_layout_code: Option<KeyCode> }`. `kind` is `Press \| Repeat \| Release`. |
| `KeyCode` | [16 §2.2](16-input-and-keymap.md#22-types) | Closed enum: `Char(char)` (always lowercased), the named keys, arrows/navigation, `F(u8)` 1..=24, keypad, `Other(u32)`. |
| `Mods` | [16 §2.2](16-input-and-keymap.md#22-types) | `bitflags u8`: `SHIFT, ALT, CTRL, SUPER, HYPER, META, CAPS_LOCK, NUM_LOCK`. Lock bits are masked before binding lookup. |
| `InputEvent` | [16 §2.2](16-input-and-keymap.md#22-types) | `Key(KeyEvent) \| Paste(String) \| Mouse(MouseEvent) \| FocusGained \| FocusLost \| Opaque`. |
| `MouseEvent` | [16 §2.2](16-input-and-keymap.md#22-types) | `{ kind: MouseKind, button: MouseButton, mods: Mods, pos: (u16, u16) }` — 0-based `(col, row)` in **pane** coordinates. |
| `MouseKind` | [16 §2.2](16-input-and-keymap.md#22-types) | `Press \| Release \| Drag \| Motion \| Wheel(WheelDir)`. |
| `MouseButton` | [16 §2.2](16-input-and-keymap.md#22-types) | `Left \| Middle \| Right \| Other(u8)`. |
| `Chord` | [16 §2.3](16-input-and-keymap.md#23-resolution) | `Keys(SmallVec<[KeyEvent; 3]>) \| Mouse(MouseTrigger)` — what a binding triggers on. A chord is at most three keys. |
| `Binding` | [16 §6.5](16-input-and-keymap.md#65-the-keymap-abstraction) | `{ trigger: Chord, when: ContextPredicate, modes: ModeSet, action: Action, repeatable: bool }` — the authored form, before compilation. |
| `CompiledBinding` | [16 §2.3](16-input-and-keymap.md#23-resolution) | `{ trigger, when, action, specificity: u16, source: BindingSource }` — the resolved form the dispatcher matches against. |
| `BindingSource` | [16 §2.3](16-input-and-keymap.md#23-resolution) | `Builtin \| User { file, span } \| Project \| Runtime` — provenance, so a conflict can name the file and span that caused it. |
| `Action` | [16 §2.3](16-input-and-keymap.md#23-resolution) | `Capability { name, args } \| SendKey(KeyEvent) \| None`. A binding invokes the catalog or forwards a key; there is no third kind. |
| `ChordResolution<'a>` | [16 §2.3](16-input-and-keymap.md#23-resolution) | `Dispatch(&CompiledBinding) \| Pending { prefix, deadline, candidates } \| Passthrough` — the outcome of matching input against the keymap. Distinct from [18 §3](18-semantic-open.md#3-resolution)'s `Resolution`, which is a different concept. |
| `ContextSet` | [16 §4.1](16-input-and-keymap.md#41-the-context-set) | `bitflags u32` of 20 named context flags (`TERMINAL_FOCUSED`, `COPY_MODE`, `ALT_SCREEN`, `CARD_FOCUSED`, …). The single context vocabulary; supersedes 10 §8.2's. |
| `ContextPredicate` | [16 §4.1](16-input-and-keymap.md#41-the-context-set) | A boolean expression over `ContextSet` flag names, e.g. `"explorer_focused && !search_active"`. Authored as a string, compiled at config load. |
| `FocusOwner` | [16 §4.2](16-input-and-keymap.md#42-focus-and-exclusivity) | `Pty(PaneId) \| Overlay(OverlayId) \| Panel(PanelId) \| Card(InteractionId)` — exactly one thing owns the keyboard. |
| `ContextStack` | [16 §4.2](16-input-and-keymap.md#42-focus-and-exclusivity) | `{ base: FocusOwner, overlays: Vec<(OverlayId, Capture)> }`, innermost last. |
| `Capture` | [16 §4.2](16-input-and-keymap.md#42-focus-and-exclusivity) | `Exclusive \| Priority` — whether an overlay swallows unmatched input or merely gets first refusal. |
| `Keymap` | [16 §6.5](16-input-and-keymap.md#65-the-keymap-abstraction) | `{ id: KeymapId, display, extends: Option<KeymapId>, modal: Option<ModalEngine>, bindings: Vec<Binding> }`. Inheritance, not three full keymaps. |
| `KeymapId` | [16 §6.5](16-input-and-keymap.md#65-the-keymap-abstraction) | The keymap's name: `"default" \| "vim" \| "emacs"` or user-defined. |
| `ModalEngine` | [16 §6.5](16-input-and-keymap.md#65-the-keymap-abstraction) | `Vim(VimConfig) \| Emacs(EmacsConfig)` — the modal behaviour a keymap opts into, if any. |
| `ModeSet` | [16 §6.5](16-input-and-keymap.md#65-the-keymap-abstraction) | `bitflags u8`: `NORMAL, INSERT, VISUAL, OPERATOR_PENDING, REPLACE` — which **editing** modes a binding is live in. Emacs mode is always `NORMAL`. Not to be confused with `SessionModeSet` ([06 §7](06-agent-layer.md#7-adapters)). |
| `InnerKeymap` | [16 §5.1](16-input-and-keymap.md#51-the-inner-program-keymap-registry) | `{ id: InnerProgramId, display, keys: Vec<InnerKey>, verified_against: Option<String> }` — what a program *inside* a pane already binds, so omt can warn before shadowing it. |
| `InnerKeymapSource` | [16 §5.1](16-input-and-keymap.md#51-the-inner-program-keymap-registry) | Trait: `for_agent(&AgentId)`, `for_process(&str)` → `Option<&InnerKeymap>`. The extension point for the registry above. |
| `TerminalProfile` | [16 §5.5](16-input-and-keymap.md#55-terminal-capability-probing) | What the *outer* terminal can actually deliver: kitty flags, `modify_other_keys`, bracketed paste, focus reporting, `alt_sends_esc`, `cmd_forwarding`, and the `ProbeSource` it was learned from. |
| `TerminalFingerprint` | [16 §5.5](16-input-and-keymap.md#55-terminal-capability-probing) | `{ term, term_program, term_program_version, xtversion, over_ssh, multiplexer }` — how the outer terminal identifies itself. Never trusted over a probe. |
| `EscPolicy` | [16 §6.3](16-input-and-keymap.md#63-the-esc-problem) | `{ timeout: Duration (25 ms), modal_timeout: Duration (15 ms) }` — how long a lone `Esc` waits before it is a lone `Esc`. |
| `Locality` | [16 §7.1](16-input-and-keymap.md#71-what-the-thin-client-intercepts-vs-forwards) | `Local \| Remote \| Composite` — whether `omt ssh`'s thin client handles a chord itself, forwards it, or both. |

## Panes and layout

| Name | Owner | Definition |
|---|---|---|
| `LayoutTree` | [17 §1.2](17-panes-and-layout.md#12-types) | `Empty \| Leaf(PaneId) \| Split(Split)` — the n-ary split tree. [05 §2](05-session-model.md#2-layout-the-bsp-tree) uses the same names. |
| `Split` | [17 §1.2](17-panes-and-layout.md#12-types) | `{ id: SplitId, axis: Axis, children: Vec<Child> }`. Invariants: ≥ 2 children, weights sum to 1.0 ± `WEIGHT_EPSILON`, no same-axis child. |
| `Child` | [17 §1.2](17-panes-and-layout.md#12-types) | `{ weight: Weight, node: LayoutTree }`. |
| `Axis` | [17 §1.2](17-panes-and-layout.md#12-types) | `Columns \| Rows`. Names what is being divided rather than the direction of the divider, which is ambiguous in every multiplexer's documentation. Used by 05 §2 and by 10 §9.1's launch YAML, which writes `columns`/`rows` and also reads the `horizontal`/`vertical` that tmux and another terminal files use. |
| `Weight` | [17 §1.2](17-panes-and-layout.md#12-types) | `Weight(f32)` newtype, with `WEIGHT_EPSILON = 1e-4` and `MIN_WEIGHT = 1e-3`. Fractional, never absolute cells. |
| `Constraints` | [17 §1.2](17-panes-and-layout.md#12-types) | `{ min: GridSize (20×3), divider: u16, title_rows: u16 }` — what geometry must respect. |
| `LayoutView` | [17 §3.3](17-panes-and-layout.md#33-decision-per-client-layout-views-one-shared-default) | `{ id: ViewId, name, layout: Layout, kind: ViewKind, clients, synchronize: Option<SyncGroup> }` — one arrangement, watched by zero or more clients. A workspace holds several. |
| `ViewKind` | [17 §3.3](17-panes-and-layout.md#33-decision-per-client-layout-views-one-shared-default) | `Primary \| Adaptive { derived_from, owner } \| Named` — **what kind of arrangement a `LayoutView` is.** Unrelated to `ViewMode` ([05 §4](05-session-model.md#4-attachment-detach-and-multi-client-viewing)); see "Two name pairs that are easy to confuse" below. |
| `ViewId` | [17 §3.3](17-panes-and-layout.md#33-decision-per-client-layout-views-one-shared-default) | Opaque id for one `LayoutView` within a workspace. Used opaquely; 17 gives no representation. |
| `Geometry` | [17 §2.1](17-panes-and-layout.md#21-compute--the-only-place-geometry-exists) | `{ panes: Vec<PanePlacement>, dividers: Vec<Divider>, hidden: Vec<PaneId>, degraded: Option<Degradation> }` — the **only** place cell coordinates exist. Derived, never stored. |
| `PanePlacement` | [17 §2.1](17-panes-and-layout.md#21-compute--the-only-place-geometry-exists) | `{ pane, outer: Rect, content: Rect, edges: EdgeFlags, stack: Option<StackId> }`. |
| `Divider` | [17 §2.1](17-panes-and-layout.md#21-compute--the-only-place-geometry-exists) | `{ split: SplitId, index: usize, axis: Axis, rect: Rect }` — a draggable border, addressable so a drag can name it. |
| `Degradation` | [17 §2.3](17-panes-and-layout.md#23-minimums-and-what-happens-when-they-cannot-be-met) | `Partial { hidden: u16 } \| ForcedSolo` — what happened when the minimums could not be met. **Layout only**; the instance-level notion is `InstanceDegradation` ([22 §4.4](22-operations.md#44-per-session-fault-isolation-r7)). |
| `ResizeTarget` | [17 §2.4](17-panes-and-layout.md#24-manual-resize--what-dragging-a-border-does) | `Divider { split, index } \| Edge { pane, edge: Direction2D }` — *what* is being resized. |
| `ResizeAmount` | [17 §2.4](17-panes-and-layout.md#24-manual-resize--what-dragging-a-border-does) | `Cells { delta, area, constraints } \| Fraction(f32)` — *by how much*. Cells are converted against a known area; weights stay fractional. |
| `SessionSizing` | [17 §3.4](17-panes-and-layout.md#34-the-pty-size-question-which-per-client-layout-does-not-solve) | `{ current: GridSize, policy: SizePolicy, observers: [(ClientId, GridSize, Participation)], pending }` — the one authoritative PTY size and everyone watching it. |
| `SizePolicy` | [17 §3.4](17-panes-and-layout.md#34-the-pty-size-question-which-per-client-layout-does-not-solve) | `Driver` (default) `\| Smallest \| Pinned { size, by }` — who decides the PTY size. Supersedes 07's `SizeOwner::Writer` naming. |
| `Participation` | [17 §3.4](17-panes-and-layout.md#34-the-pty-size-question-which-per-client-layout-does-not-solve) | `Participant \| Observer` — whether a client's size counts toward `Smallest`. |
| `LayoutPreset` | [17 §4.1](17-panes-and-layout.md#41-presets) | `Solo \| EvenColumns \| EvenRows \| MainColumn { main, mirrored } \| MainRow { … } \| Tiled \| Stacked`. |
| `LayoutSpec` | [17 §4.2](17-panes-and-layout.md#42-the-serialization-format) | `#[serde(untagged)]` `Leaf(PaneSpec) \| Branch { split, ratio, panes } \| Preset { preset, panes }` — the authored/serialized layout, distinct from the runtime `Layout`. |
| `PaneSpec` | [17 §4.2](17-panes-and-layout.md#42-the-serialization-format) | `{ cwd, session, commands, agent, title, focused, min, priority }` — what one pane in a saved layout asks for. |
| `FloatingPane` | [17 §5.2](17-panes-and-layout.md#52-floating-panes) | `{ pane, rect: FractionalRect, min, pinned, kind: FloatKind }`. Floats live beside the tree, not in it. |
| `Stack` | [17 §5.3](17-panes-and-layout.md#53-stacked-panes) | `{ id: StackId, members: Vec<PaneId>, expanded: usize }` — several panes sharing one tile, one visible. |
| `SyncGroup` | [17 §5.4](17-panes-and-layout.md#54-swap-rotate-move-marks) | The set of panes that receive synchronized input, carried as `LayoutView::synchronize`. Named but not given a shape by 17 — **open**. |

## Semantic open

| Name | Owner | Definition |
|---|---|---|
| `Rule` | [18 §2.2](18-semantic-open.md#22-the-matcher) | `{ id: RuleId, kind: MatchKind, regex, precedence: i16, anchor: Anchor, scope, captures, enabled }` — one recognizer. Rules are untrusted input ([18 §8.4](18-semantic-open.md#84-rules-as-untrusted-input)). |
| `Anchor` | [18 §2.2](18-semantic-open.md#22-the-matcher) | `Anywhere \| LineStart \| AfterWhitespace` — where in a line a rule may start matching. |
| `Slot` | [18 §2.2](18-semantic-open.md#22-the-matcher) | `Path, Line, Col, Url, Sha, Issue, Repo, Owner, Host, User(u8)` — the named capture positions a rule may fill. |
| `MatchKind` | [18 §2.3](18-semantic-open.md#23-match-kinds) | `Url \| Path \| GitRef \| Issue \| Custom(RuleId)`. |
| `Match` | [18 §2.6](18-semantic-open.md#26-the-match-and-target-types) | `{ id, span: Range<Position>, rule, kind, origin, slots, text, block }` — a recognized region of session content. Anchored on `Position`, so it survives reflow. Lives in [`omt-types`](02-crate-map.md#omt-types). |
| `MatchOrigin` | [18 §2.6](18-semantic-open.md#26-the-match-and-target-types) | `Osc8 \| Osc8WithHeuristicLineCol \| Heuristic` — whether the program said so or omt guessed. Drives precedence ([18 §2.1](18-semantic-open.md#21-two-sources-and-their-precedence)). |
| `MatchRef` | [18 §9](18-semantic-open.md#9-capabilities) | `ById(MatchId) \| AtPosition(Position) \| RawText(String)` — the three ways a caller can name the thing to resolve. |
| `Target` | [18 §2.6](18-semantic-open.md#26-the-match-and-target-types) | `Url \| Path { raw, line, col } \| GitRef \| Issue { owner, repo, number, key } \| Custom { rule, slots }`. **Widened here and moved from [04 §8.3](04-terminal-core.md) into [`omt-types`](02-crate-map.md#omt-types)**; 04 defers. |
| `ResolutionContext<'a>` | [18 §3](18-semantic-open.md#3-resolution) | `{ host, block_cwd, block_host, pane_cwd, workspace_roots, git, home }` — everything needed to turn a `Target` into a real thing. The cwd question is the whole problem ([18 §3.1](18-semantic-open.md#31-the-cwd-question-which-is-the-whole-thing)). |
| `ResolvedTarget` | [18 §3](18-semantic-open.md#3-resolution) | The canonical definition: `{ target, resolution, existence, sensitivity, actions: Vec<ActionOffer>, host, explorer: Option<ExplorerRef> }`. |
| `Resolution` | [18 §3](18-semantic-open.md#3-resolution) | `File { path, workspace, line, col } \| Dir \| Url \| Commit \| IssueUrl \| Ambiguous { candidates } \| Unresolved { reason }` — what the target turned out to be. The keybinding-side name is `ChordResolution` ([16 §2.3](16-input-and-keymap.md#23-resolution)). |
| `Existence` | [18 §3](18-semantic-open.md#3-resolution) | `Exists { kind, len } \| Missing \| Unknown`. `Unknown` is honest, not a failure. |
| `OpenHandler` | [18 §4.1](18-semantic-open.md#41-the-trait) | Trait: `id`, `label`, `applicability`, `effects`, `activate`. The extension point; the built-ins are ordinary handlers. |
| `Applicability` | [18 §4.1](18-semantic-open.md#41-the-trait) | `{ rank: i16, reason: Option<&'static str> }` — how well a handler fits, and why, for the menu. |
| `Activation` | [18 §4.1](18-semantic-open.md#41-the-trait) | `Done { detail } \| ClientMust(ClientAction)` — the instance either did it or names what the client must do. |
| `ClientAction` | [18 §4.1](18-semantic-open.md#41-the-trait) | `OpenUrl \| OpenLocalFile { blob, suggested_name, line, col, provenance, read_only } \| CopyText \| ShowInline { blob, mime, line }`. The closed set of things a client is asked to do. |
| `ActionOffer` | [18 §3](18-semantic-open.md#3-resolution) | One handler's offer for a resolved target, carrying at least `effects`, as listed in `ResolvedTarget::actions`. Named but not given a full shape by 18 — **open**. |
| `HandlerId` | [18 §4.1](18-semantic-open.md#41-the-trait) | The handler's stable name (`"editor"`, `"explorer"`, `"browser"`, …), returned by `OpenHandler::id()`. |
| `TargetHost` | [18 §6.1](18-semantic-open.md#61-how-the-client-knows-the-file-is-remote) | `Local \| Instance { instance: InstanceId, host: RemoteHost }` — which machine the target lives on. |
| `TargetScope` | [18 §9](18-semantic-open.md#9-capabilities) | `Viewport \| Block(BlockId) \| Line(Position)` — how much content `open.targets.list` scans. |
| `Hint` | [18 §9](18-semantic-open.md#9-capabilities) | `{ label, span, kind, exists }` — one keyboard hint label in hint mode ([18 §5.2](18-semantic-open.md#52-hint-mode--the-primary-mechanism)). |
| `HintSessionId` | [18 §9](18-semantic-open.md#9-capabilities) | Opaque id for one round of hint mode, returned by `open.hints.begin`. |
| `RemoteMirror` | [18 §6.3](18-semantic-open.md#63-where-the-file-lands-and-why-the-layout-matters) | The sidecar record for a fetched remote file: remote path, host, fetched-at, hash, `read_only`, writeback state. Stored as `BlobClass::Mirror` in 09's blob store, not in a second store. |

## Recall, usage and attention

| Name | Owner | Definition |
|---|---|---|
| `IndexUnit` | [20 §3.3](20-recall-and-usage.md#33-write-path) | `{ kind: DocKind, scope, when, place, facts, text: FieldSet<Redacted> }` — one indexable thing. The text is `Redacted` by construction. |
| `DocKind` | [20 §3.2](20-recall-and-usage.md#32-schema) | What an indexed doc is: `block, user_msg, assistant_msg, tool_call, file_change, interaction, session_meta, summary`. |
| `DocId` | [20 §3.3](20-recall-and-usage.md#33-write-path) | Opaque id for one indexed doc, returned by the write path and carried on every `Hit`. |
| `Redacted` | [20 §5.1](20-recall-and-usage.md#51-the-ordering-rule) | A newtype over text with exactly one constructor, `Redacted::from_raw(&Redactor, &str)`. The type-level guarantee that redaction happened **before** indexing. |
| `FieldSet<T>` | [20 §3.3](20-recall-and-usage.md#33-write-path) | The per-doc text fields (`command`, `output`, `user_msg`, `assistant_msg`, `path`, `title`, `note`, `summary`), generic over the wrapper so the index can only hold `Redacted`. |
| `SearchQuery` | [20 §12.1](20-recall-and-usage.md#121-search) | Text plus every filter axis: fields, kinds, scope, time, attribution, exit, agent, path prefix, match mode, focus context, grouping, limit, cursor. |
| `SearchResults` | [20 §12.1](20-recall-and-usage.md#121-search) | `{ groups: Vec<SessionGroup>, total_estimate, coverage: Coverage, took_ms, cursor }`. `coverage` is how the index admits what it does not have. |
| `Hit` | [20 §12.1](20-recall-and-usage.md#121-search) | `{ doc, kind, session, block, at, score, snippet, match_ranges, anchor }` — one result, anchored so it can be jumped to. |
| `Timeline` | [20 §8.1](20-recall-and-usage.md#81-the-timeline) | `{ session, binding, span, entries: Vec<TimelineEntry>, stats: TimelineStats }` — what happened in a session, structured. |
| `TimelineEntry` | [20 §8.1](20-recall-and-usage.md#81-the-timeline) | `{ seq, at, duration, kind: EntryKind, repeat, children }`. Nested and run-length collapsed, so a loop is one row. |
| `EntryKind` | [20 §8.1](20-recall-and-usage.md#81-the-timeline) | `SessionStart, Turn, ToolCall, FileChanged, Block, Interaction, Compaction, Usage, RateLimit, Error, Note, Gap, SessionEnd`. |
| `TimelineStats` | [20 §8.1](20-recall-and-usage.md#81-the-timeline) | The aggregate footer of a `Timeline`, returned by `timeline.stats`. Named but not given a shape by 20 — **open**. |
| `ResolutionSummary` | [20 §8.1](20-recall-and-usage.md#81-the-timeline) | `{ outcome, by, device, response, latency, fidelity }` — how one `Interaction` ended. `outcome` is `Answered \| TimedOut \| Cancelled \| Abandoned`. |
| `Digest` | [20 §8.2](20-recall-and-usage.md#82-the-digest) | `{ scope, window, headline, sections, agent_summaries, counts }` — "what happened while I was away", per surface. |
| `DigestCounts` | [20 §8.2](20-recall-and-usage.md#82-the-digest) | The numeric spine of a digest: durations, turns, tool calls, files by change, blocks and failures, interactions by outcome, compactions, errors, usage, rate-limit state. |
| `AgentSummary` | [20 §8.2](20-recall-and-usage.md#82-the-digest) | `{ session, agent, source, text, at }` — the **agent's own** words, attributed to the source that produced them. omt never writes one. |
| `ComparisonRow` | [20 §9](20-recall-and-usage.md#9-comparing-parallel-agent-results-g18) | One agent's row in the parallel-results comparison: branch, base ref, state, elapsed, diffstat, files touched and disagreements, test runs, turns, interactions, usage, summary. |
| `UsageEvent` | [20 §11.2](20-recall-and-usage.md#112-normalization) | `{ session, binding, thread, at, model, tokens: Tokens, cost_usd, context_window, accounting, reported_by, source }`. `accounting` is `Cumulative \| Delta` — the field that makes normalization possible. |
| `Tokens` | [20 §11.2](20-recall-and-usage.md#112-normalization) | `{ input, output, cache_read, cache_write, reasoning }`, all `u64`. |
| `UsageReport` | [20 §11.3](20-recall-and-usage.md#113-storage-and-query) | `{ rows, totals, not_reporting: Vec<AgentKind>, cost_partial: bool }` — the last two fields exist so the report never implies coverage it does not have. |
| `RateLimitState` | [20 §11.3](20-recall-and-usage.md#113-storage-and-query) | Per `(agent, kind)`: `status`, `resets_at`, `detail`, `observed_at`. Observed from the agent, never inferred. |
| `AttentionSignal` | [20 §10.1](20-recall-and-usage.md#101-the-detector) | `{ session, binding, reasons: Vec<StuckReason>, since, confidence, detail }` — **a signal, never an action** ([20 §10.3](20-recall-and-usage.md#103-output-a-signal-never-an-action)). |
| `StuckReason` | [20 §10.1](20-recall-and-usage.md#101-the-detector) | `LongWorking \| RepeatedCall \| RepeatedFailure \| Silent \| BurnWithoutProgress`, each carrying the measurement that fired it. |
| `IndexQueue` | [20 §13.2](20-recall-and-usage.md#132-the-hot-stream-and-backpressure) | `{ cap: usize (4096), dropped: Counter, policy: ShedPolicy }` — indexing is best-effort and says so. |
| `ShedPolicy` | [20 §13.2](20-recall-and-usage.md#132-the-hot-stream-and-backpressure) | `ShedOutput \| ShedUnits` — drop command output first, whole units only as a last resort. |

## Data lifecycle

| Name | Owner | Definition |
|---|---|---|
| `Redactor` | [21 §2.2](21-data-lifecycle.md#22-the-detector) | `{ key_rules, shape_rules, entropy, user_rules, allow }` — the one detector. Runs **before** any write ([21 §2.1](21-data-lifecycle.md#21-the-placement-rule)), including before indexing. |
| `RedactionClass` | [21 §2.2](21-data-lifecycle.md#22-the-detector) | `Env \| Key \| Flag \| Header \| Shape(&'static str) \| Entropy \| User(String)` — why something was redacted. Appears in the marker. |
| `Finding` | [21 §2.2](21-data-lifecycle.md#22-the-detector) | `{ span: Range<usize>, class: RedactionClass, replacement: String }` — one detection, before substitution. |
| `RetentionPolicy` | [21 §3.1](21-data-lifecycle.md#31-the-principle-compact-then-delete) | `{ scope: RetentionScope, rules: BTreeMap<DataKind, RetentionRule> }`. Instance- or workspace-scoped. |
| `RetentionRule` | [21 §3.1](21-data-lifecycle.md#31-the-principle-compact-then-delete) | `{ compact_after, delete_after, max_bytes, max_rows, source: ConfigSource }` — compact first, delete second. |
| `DataKind` | [21 §3.1](21-data-lifecycle.md#31-the-principle-compact-then-delete) | The inventory rows retention is expressed over: `Scrollback, BlockOutput, Blocks, History, AgentEvents, Interactions, TranscriptIndex, Blobs, SearchIndex, Usage, Audit, DaemonLog, Crashes, Quarantine`. |
| `PersistFlags` | [21 §2.5](21-data-lifecycle.md#25-per-workspace-and-per-session-control) | Per-scope "write this at all?" switches — `scrollback`, `block_output`, `agent_events`, `index` — plus the `ConfigSource` that set them. Read and written by `store.persist.get`/`.set` ([21 §8](21-data-lifecycle.md#8-capabilities)). |
| `PurgeManifest` | [21 §4.3](21-data-lifecycle.md#43-storepurge--destruction-with-a-manifest) | What `store.purge` shows before it destroys anything, and the receipt it leaves after: counts and byte totals, **never content**. |
| `Sweeper` | [21 §3.2](21-data-lifecycle.md#32-the-sweeper) | `{ schedule: SweepSchedule, budget: SweepBudget }` — retention is enforced by a budgeted background pass, not at write time. |
| `SweepSchedule` | [21 §3.2](21-data-lifecycle.md#32-the-sweeper) | `{ light: Duration (5 min), full: Duration (6 h, plus startup after a 60 s settle) }`. |
| `SweepBudget` | [21 §3.2](21-data-lifecycle.md#32-the-sweeper) | `{ max_wall: 30 s, max_io_bytes: 256 MiB, lag_ceiling: 20 ms, nice: bool }` — the sweeper yields to the foreground, always. |
| `Record` | [21 §6.1](21-data-lifecycle.md#61-the-model) | The append-only log frame: `{ len: u32, crc: u32 (crc32c), kind: u8, payload: postcard }`. A torn tail is detectable, not corrupting. |
| `Migration` | [21 §7.2](21-data-lifecycle.md#72-migrating-forward) | Trait: `from()`, `to()`, `describe()`, `run(&mut StoreHandle, &mut dyn Progress)`. Forward-only, one version step at a time. |
| `StoreScope` | [21 §8](21-data-lifecycle.md#8-capabilities) | What a `store.usage` or `store.purge` call applies to — session, workspace, agent scope, time range, instance, or everything ([21 §4.3](21-data-lifecycle.md#43-storepurge--destruction-with-a-manifest)). Named but not given a Rust shape by 21 — **open**. |

## Operations

| Name | Owner | Definition |
|---|---|---|
| `Health` | [22 §4.1](22-operations.md#41-systemhealth) | `{ instance, sessions, clients, store, sources, egress }` — the structured health model returned by `system.health`. A `Query` at `Viewer`. |
| `InstanceHealth` | [22 §4.1](22-operations.md#41-systemhealth) | Process facts: version, uptime, pid, RSS, CPU, fds, threads, event-loop lag p50/p99, bus subscribers and dropped events, `degraded: Vec<InstanceDegradation>`. |
| `SessionHealth` | [22 §4.1](22-operations.md#41-systemhealth) | `{ session, state, mode: SessionMode, rss_estimate_bytes, agent, faults: Vec<SessionFault>, pty: Option<PtySessionHealth>, native: Option<NativeSessionHealth> }` — split by `SessionMode`, per D8. |
| `ClientHealth` | [22 §4.1](22-operations.md#41-systemhealth) | `{ client, kind, rtt_p50, send_queue_bytes, backpressure, subscriptions }`. |
| `StoreHealth` | [22 §4.1](22-operations.md#41-systemhealth) | The store's contribution to `Health`. Named but not given a shape by 22 — **open**. |
| `SourceHealth` | [22 §4.1](22-operations.md#41-systemhealth) | Per observation source ([06 §3](06-agent-layer.md#3-source-model)), one entry. Named but not given a shape by 22 — **open**. |
| `EgressStatus` | [22 §4.1](22-operations.md#41-systemhealth) | What can leave this machine and whether any of it is enabled — the same data `instance.health` ([13 §9.1](13-security.md)) reports, in structured form. Shape not given by 22 — **open**. |
| `InstanceDegradation` | [22 §4.1](22-operations.md#41-systemhealth) | `{ kind: NotPersisting \| MetricsOff \| RegistryUnreachable \| ClockSkew \| HooksStale, since, detail, remedy }` — a currently-degraded instance-level capability, added or cleared with the `instance.degraded` event ([22 §4.4](22-operations.md#44-per-session-fault-isolation-r7)). Distinct from [17 §2.3](17-panes-and-layout.md#23-minimums-and-what-happens-when-they-cannot-be-met)'s layout `Degradation`. |
| `SessionFault` | [22 §4.4](22-operations.md#44-per-session-fault-isolation-r7) | The fault-class vocabulary that isolates one session's failure from the rest: `ParserPanic, RunawayOutput, ScrollbackExhaustion, NativeTransportClosed, AgentOom, AgentCrash, SlowConsumer, StoreError, PluginFault`. |
| `DoctorGroup` | [22 §3.1](22-operations.md#31-the-checks) | Which family of checks to run: `term, shell, agents, keys, media, store, net, service`. Doctor is **one parameterized capability**; other documents contribute groups, not their own `doctor.*` capabilities. |
| `Check` | [22 §10](22-operations.md#10-capabilities) | `{ id, group, status, detail, remedy, auto_fixable }` — one diagnostic result. Every failure carries a remedy. |
| `AgentWait` | [22 §8.3](22-operations.md#83-the-primitives-scripts-need-r71) | `{ session, until: Vec<WaitCondition>, timeout, immediate }` — the blocking primitive scripts need instead of polling. |
| `WaitCondition` | [22 §8.3](22-operations.md#83-the-primitives-scripts-need-r71) | `Idle \| Blocked \| Working \| Exited \| Interaction`. The first four are states; `Interaction` is an edge. |
| `AgentWaitResult` | [22 §8.3](22-operations.md#83-the-primitives-scripts-need-r71) | `{ matched, state: AgentState, interaction, waited, timed_out }` — a timeout is a result, not an error. |
| `RestartPlan` | [22 §10](22-operations.md#10-capabilities) | What an `upgrade.apply` will restart and when, returned alongside `blocked_by`. Named but not given a shape by 22 — **open**. |
| `Listener` | [22 §10](22-operations.md#10-capabilities) | One bound endpoint reported by `instance.info`. Named but not given a shape by 22 — **open**. |

## Identity and devices

| Name | Owner | Definition |
|---|---|---|
| `IdentityId` | [23 §1.1](23-identity-and-devices.md#11-four-types) | `IdentityId([u8; 32])` — blake3-256 of the identity root public key, rendered `idn_<crockford-b32(first 16)>`. Lives in [`omt-types`](02-crate-map.md#omt-types). |
| `Identity` | [23 §1.1](23-identity-and-devices.md#11-four-types) | `{ id, display_name, root_pub: Ed25519PublicKey, created_at, home: Option<HomeRef>, registry_epoch, prefs }` — the person, decentralized; there is no server that owns it. |
| `HomeRef` | [23 §1.1](23-identity-and-devices.md#11-four-types) | `{ instance, label, endpoints, designated_at, designated_by, superseded_by }` — which instance plays the `home` role ([23 §3](23-identity-and-devices.md#3-the-home-instance-role)). Optional; an identity may have none. |
| `IdentityPrefs` | [23 §1.1](23-identity-and-devices.md#11-four-types) | `{ version: u64, entries: BTreeMap<String, Value> }` — the small, versioned bag of preferences that follows an identity across instances. |
| `Device` | [23 §1.1](23-identity-and-devices.md#11-four-types) | The durable record behind a `DeviceId`: identity, label, form, platform, `device_key`, `webauthn`, timestamps, `status`, `reauth`, `auth_source`. |
| `DeviceForm` | [23 §1.1](23-identity-and-devices.md#11-four-types) | `Phone \| Tablet \| Laptop \| Desktop \| Headless`. |
| `DevicePlatform` | [23 §1.1](23-identity-and-devices.md#11-four-types) | `{ os, os_version, browser, app_kind, standalone }` — enough to render "iPhone · Safari" and to explain a passkey failure. |
| `DeviceKey` | [23 §1.1](23-identity-and-devices.md#11-four-types) | `Ed25519(Ed25519PublicKey) \| EcdsaP256(P256PublicKey)` — the device's own key pair. |
| `DeviceStatus` | [23 §1.1](23-identity-and-devices.md#11-four-types) | `Pending { expires_at } \| Active \| Revoked { at, by, reason }`. |
| `RevocationReason` | [23 §1.1](23-identity-and-devices.md#11-four-types) | `Lost \| Stolen \| Retired \| Compromised \| IdentityRotated \| Other(String)`. |
| `Revocation` | [23 §1.1](23-identity-and-devices.md#11-four-types) | `{ device, at, by, reason, epoch, sig }` — root-signed, so any instance can verify it without asking home. |
| `RevocationSubject` | [13 §3.1](13-security.md#31-the-trait) | `Credential(CredentialId) \| Device(DeviceId)` — what `AuthBackend::is_revoked` takes, because **the unit a user revokes is a device**, and a `DeviceGrant` carries no `CredentialId` at all. |
| `AuthSource` | [23 §1.1](23-identity-and-devices.md#11-four-types) | `Paired \| Tailnet { login, node_stable_id } \| Local { uid }` — how this device got in the front door. |
| `InstanceCredential` | [23 §1.1](23-identity-and-devices.md#11-four-types) | The **persisted, per-instance form** of [13 §3.1](13-security.md#31-the-trait)'s `Grant`, with `identity` and `device` mandatory rather than optional. |
| `InstanceRegistration` | [23 §1.1](23-identity-and-devices.md#11-four-types) | `{ instance, label, endpoints, instance_pub, added_at, added_by, last_reachable_at, is_home, tags, catalog_hash_hint }` — one instance an identity knows about. |
| `Endpoint` | [23 §1.1](23-identity-and-devices.md#11-four-types) | `Tailnet { host, port } \| Https { url } \| Lan { addr, port } \| Local { socket } \| Ssh { target }` — the ways to reach an instance, tried in order. |
| `DeviceGrant` | [23 §1.3](23-identity-and-devices.md#13-devicegrant--the-certificate-that-makes-this-decentralized) | The root-signed certificate a device **presents** to be authenticated: `{ v, identity, device, device_key, label, form, issued_at, not_after (90 d), epoch }`, wire-encoded `base64url(CBOR) "." base64url(sig)`. Distinct from `Grant` ([13 §3.1](13-security.md#31-the-trait)), which is the **result** an `AuthBackend` returns, and from `[[auth.tailnet.mappings]]`, which is config. |
| `WebauthnBinding` | [23 §2.2](23-identity-and-devices.md#22-registration-and-authentication-end-to-end) | `{ rp_id, cred_id, cred_pub, sign_count, uv_capable, transports }` — a passkey bound to one relying-party id. |
| `IdentityFileBody` | [23 §4.2](23-identity-and-devices.md#42-format) | The plaintext inside the encrypted identity file: identity, root secret, devices, revocations, instances, prefs, credentials, recovery codes. It is a secret, and omt says so every time. |
| `ReauthPolicy` | [23 §7.2](23-identity-and-devices.md#72-policy) | `{ mode: ReauthMode, require: Set<CapabilityPattern>, require_effects: EffectBits, freshness (5 min, 30 s–12 h), fallback }` — which actions demand a fresh proof of presence. Enforced server-side ([23 §7.3](23-identity-and-devices.md#73-enforcement-is-server-side)). |
| `ReauthMode` | [23 §7.2](23-identity-and-devices.md#72-policy) | `Off \| On`. |
| `DeviceRef` | [23 §8.1](23-identity-and-devices.md#81-presence) | `{ id, label, form, platform_short, is_this_device }` — the renderable form of a device in presence. |

## Onboarding and discoverability

| Name | Owner | Definition |
|---|---|---|
| `capability!` fields `title` / `aliases` / `hidden` / `hidden_reason` | [03 §2](03-capability-catalog.md#2-declaring-a-capability) | Proposed by [19 §2.4](19-onboarding.md#24-the-command-palette-as-the-universal-escape-hatch) and now **adopted into 03's declaration table**. `title` is mandatory for non-hidden capabilities (imperative, ≤ 40 chars) and is deliberately *not* derived from `group`/`verb`; `aliases` are palette search terms, matched but not displayed; `hidden = true` requires a `hidden_reason` and is a presentation flag, never access control. The palette entry is parity artifact #5 ([03 §5](03-capability-catalog.md#5-the-parity-contract)). |
| Keymap file schema | [19 §5.2](19-onboarding.md#52-the-tmux-keymap-shipped-as-data) | The on-disk TOML form of a `Keymap` (`#:schema .../keymap.schema.json`): `id`, `display`, `extends`, `modal`, `notes`, a chord-keyed binding table, and repeated `[[unmapped]] { tmux, reason }`. The Rust `Keymap` is owned by [16 §6.5](16-input-and-keymap.md#65-the-keymap-abstraction); 19 owns the file form, `notes` and `unmapped`. |

## Proposed — remote continuity

**These names come from [`docs/design/remote-continuity.md`](../design/remote-continuity.md), a design document rather than an architecture document. They are proposed, not settled, and no architecture document owns them yet.**

| Name | Owner (proposed) | Definition |
|---|---|---|
| `ActorContinuity` | [rc §1.2](../design/remote-continuity.md#12-the-taxonomy) | `{ identity, recents, drafts, mutes, notify, read_marks, preferred_surface }` — the per-actor state that makes a second device a continuation rather than a second screen. Stored on the instance ([rc §1.3](../design/remote-continuity.md#13-where-per-actor-state-is-stored-and-why-on-the-instance)). |
| `Recent` | [rc §1.2](../design/remote-continuity.md#12-the-taxonomy) | `{ workspace, session: Option<SessionId>, last_active, via: DeviceId }` — one entry in "where you were". |
| `Draft` | [rc §2.4](../design/remote-continuity.md#24-drafts) | `{ key, text, caret, blobs, updated_at, updated_by, version }` — an unsent composition that follows the person across devices. |
| `DraftKey` | [rc §2.4](../design/remote-continuity.md#24-drafts) | `AgentPrompt { session } \| Interaction { interaction } \| PlanReview { interaction }` — what a draft is a draft *of*. |
| `NotifyPrefs` | [rc §5.6](../design/remote-continuity.md#56-noise-control--deferred-and-why-the-design-is-kept) | `{ level, sessions: HashMap<SessionId, NotifyLevel>, quiet_hours, suppress_when_present, coalesce }`. |
| `NotifyLevel` | [rc §5.6](../design/remote-continuity.md#56-noise-control--deferred-and-why-the-design-is-kept) | `Actionable \| Involved { turn_end_after } \| Everything \| Muted { until }`. `Actionable` is the default: an `Interaction` is worth a buzz, a turn ending usually is not. |
| `QuietHours` | [rc §5.6](../design/remote-continuity.md#56-noise-control--deferred-and-why-the-design-is-kept) | `{ from: Time, to: Time, tz: String, allow_actionable: bool }`. |
| `InteractionActivity` | [rc §3.3](../design/remote-continuity.md#33-answering-right-now--the-one-new-presence-signal) | `{ interaction, actor, device, activity: CardActivity, expires_at }` — the one new presence signal, so two devices do not race the same card. It is advisory; the ledger ([12 §4.1](12-collaboration.md#41-the-invariant)) still decides. |
| `CardActivity` | [rc §3.3](../design/remote-continuity.md#33-answering-right-now--the-one-new-presence-signal) | `Viewing \| Composing \| Submitting`. |
| `PresencePeer` | [rc §3.1](../design/remote-continuity.md#31-the-single-user-two-device-case-is-the-common-one) | `{ kind: "me", device, label, liveness } \| { kind: "other", actor, label, liveness }` — the rendering-side view of presence, which distinguishes *my other phone* from *another person*. |
| `SurfaceIntent` | [rc §1.4](../design/remote-continuity.md#14-the-one-deliberate-exception-surface-intent) | `Auto \| Blocks \| Terminal` — the one per-device preference that deliberately does **not** follow the actor across devices. |

---

## Two name pairs that are easy to confuse

`ViewKind` ([17 §3.3](17-panes-and-layout.md#33-decision-per-client-layout-views-one-shared-default),
`Primary | Adaptive | Named`) and `ViewMode`
([05 §4](05-session-model.md#4-attachment-detach-and-multi-client-viewing),
`Grid | Blocks`) are **different concepts with confusingly similar names**.
`ViewKind` says what kind of *arrangement* a `LayoutView` is; `ViewMode` says
which *surface* a client attached with. Neither is an alias of the other.

`Grant` ([13 §3.1](13-security.md#31-the-trait)), `DeviceGrant`
([23 §1.3](23-identity-and-devices.md#13-devicegrant--the-certificate-that-makes-this-decentralized))
and `[[auth.tailnet.mappings]]` ([13 §3.5](13-security.md#35-tailnet-identity))
are the auth *result*, the root-signed *certificate*, and a *config table*
respectively. They are never interchangeable.
