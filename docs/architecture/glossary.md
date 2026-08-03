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
| `ActorId` | [12 §1](12-collaboration.md#1-actors) | Stable for one connection. A reconnect is a new `ActorId`. |
| `DeviceId` | [23 §1.1](23-identity-and-devices.md#11-four-types) | Survives reconnects; what presence and audit group by. **Used** by [12 §1](12-collaboration.md#1-actors) (`ActorKind::Remote`) and [12 §2](12-collaboration.md#2-presence-is-first-class-state) (`Presence`), but **owned** by 23, which gives it a durable `Device` record. The id itself lives in [`omt-types`](02-crate-map.md#omt-types), not in whichever crate mints it. |
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
| `SessionModeSet` | [05 §1.1](05-session-model.md#11-types) | `bitflags u8`: `PTY_ONLY, NATIVE` — which `SessionMode`s an adapter supports ([06 §7](06-agent-layer.md#7-adapters)); `PTY_ONLY` is the default. **Renamed from 06's `ModeSet`**, which collided with the keymap `ModeSet` ([16 §6.5](16-input-and-keymap.md#65-the-keymap-abstraction)), and defined in 05 alongside `SessionMode` because it is a set of those. |
| `AcpSpawn` | [06 §7](06-agent-layer.md#7-adapters) | Argv and env for spawning an agent in `SessionMode::Native`, returned by `AgentAdapter::acp_spawn`; `None` when the adapter has no native mode. Named but **defined nowhere** — open. |
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
| `SessionMode` | [05 §1.1](05-session-model.md#11-types) | `Pty \| Native` ([D8](decisions.md#d8--two-session-modes-pty-default-and-native-acp)). Chosen at creation, immutable for the session's life. Lives in [`omt-types`](02-crate-map.md#omt-types). |
| `SessionSurface` | [05 §1.1](05-session-model.md#11-types) | `Pty { pty, term } \| Native { conn, transcript }` — makes "a native session has no PTY" unrepresentable rather than an unwrap site. `Session` carries both `mode` and `surface`. |
| `Layout` | [17 §1.3](17-panes-and-layout.md#13-the-whole-layout-of-a-workspace) | `{ tiles: LayoutTree, floats, zoom, stacks, focus, last_focus }` — the whole layout of a workspace. **17's struct, not 05's enum**; the tree itself is `LayoutTree` (see *Panes and layout*). |
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
| `BlobClass` | [09 §2](09-ssh-and-media.md#2-the-blob-store) | `Runtime \| Mirror { host, remote_path, read_only }` — which lifetime/quota class a blob belongs to. `Mirror` is what [18 §6.3](18-semantic-open.md#63-where-the-file-lands-and-why-the-layout-matters)'s remote file mirror uses; it is not a second store. |
| `AttachmentReference` | [09 §4.3.7](09-ssh-and-media.md#437-what-the-agent-finally-receives) | How an agent wants to be handed an on-disk attachment: `PromptText \| InlineContent { fence, body, name } \| Command { command, then } \| Structured`. Returned by `AgentAdapter::attachment_reference`, whose trait is owned by [06 §7](06-agent-layer.md#7-adapters). One type for every attachment class; there is no separate image method. |
| `ImageReference` | [09 §7.1](09-ssh-and-media.md#71-handing-the-image-to-the-agent) | **Deprecated alias** of `AttachmentReference`, kept only so older references resolve. `AttachmentReference` is a strict superset (it adds `InlineContent`). New code names `AttachmentReference` directly. |
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
| `ChordResolution<'a>` | [16 §2.3](16-input-and-keymap.md#23-resolution) | `Dispatch(&CompiledBinding) \| Pending { prefix, deadline, candidates } \| Passthrough` — the outcome of matching input against the keymap. **Renamed from 16's `Resolution`** to free that name for [18 §3](18-semantic-open.md#3-resolution); see the unified-names table. |
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
| `LayoutTree` | [17 §1.2](17-panes-and-layout.md#12-types) | `Empty \| Leaf(PaneId) \| Split(Split)` — the n-ary split tree. **This is what 05 called `Layout`.** |
| `Split` | [17 §1.2](17-panes-and-layout.md#12-types) | `{ id: SplitId, axis: Axis, children: Vec<Child> }`. Invariants: ≥ 2 children, weights sum to 1.0 ± `WEIGHT_EPSILON`, no same-axis child. |
| `Child` | [17 §1.2](17-panes-and-layout.md#12-types) | `{ weight: Weight, node: LayoutTree }`. |
| `Axis` | [17 §1.2](17-panes-and-layout.md#12-types) | `Columns \| Rows`. Deliberately **not** 05's `Direction { Horizontal, Vertical }`, which was ambiguous about what was being divided. |
| `Weight` | [17 §1.2](17-panes-and-layout.md#12-types) | `Weight(f32)` newtype, with `WEIGHT_EPSILON = 1e-4` and `MIN_WEIGHT = 1e-3`. Fractional, never absolute cells. |
| `Constraints` | [17 §1.2](17-panes-and-layout.md#12-types) | `{ min: GridSize (20×3), divider: u16, title_rows: u16 }` — what geometry must respect. |
| `LayoutView` | [17 §3.3](17-panes-and-layout.md#33-decision-per-client-layout-views-one-shared-default) | `{ id: ViewId, name, layout: Layout, kind: ViewKind, clients, synchronize: Option<SyncGroup> }` — one arrangement, watched by zero or more clients. A workspace holds several. |
| `ViewKind` | [17 §3.3](17-panes-and-layout.md#33-decision-per-client-layout-views-one-shared-default) | `Primary \| Adaptive { derived_from, owner } \| Named` — **what kind of arrangement a `LayoutView` is.** Unrelated to `ViewMode` ([05 §4](05-session-model.md#4-attachment-detach-and-multi-client-viewing)); see the unified-names table. |
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
| `InstanceDegradation` | [22 §4.1](22-operations.md#41-systemhealth) | `{ kind: NotPersisting \| MetricsOff \| RegistryUnreachable \| ClockSkew \| HooksStale, since, detail, remedy }` — a currently-degraded instance-level capability, added or cleared with the `instance.degraded` event ([22 §4.4](22-operations.md#44-per-session-fault-isolation-r7)). **Renamed from 22's `Degradation`** to free that name for [17 §2.3](17-panes-and-layout.md#23-minimums-and-what-happens-when-they-cannot-be-met). |
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
| `capability!` fields `title` / `aliases` / `hidden` / `hidden_reason` | [19 §2.4](19-onboarding.md#24-the-command-palette-as-the-universal-escape-hatch) | An **extension to [03 §2](03-capability-catalog.md#2-declaring-a-capability)'s declaration table**, proposed by 19 and not yet edited into 03. `title` is mandatory for non-hidden capabilities (imperative, ≤ 40 chars); `aliases` are palette search terms; `hidden = true` requires a `hidden_reason`. Recorded as such in [19 §9](19-onboarding.md#9-capabilities-this-document-requires). |
| Keymap file schema | [19 §5.2](19-onboarding.md#52-the-tmux-keymap-shipped-as-data) | The on-disk TOML form of a `Keymap` (`#:schema .../keymap.schema.json`): `id`, `display`, `extends`, `modal`, `notes`, a chord-keyed binding table, and repeated `[[unmapped]] { tmux, reason }`. The Rust `Keymap` is owned by [16 §6.5](16-input-and-keymap.md#65-the-keymap-abstraction); 19 owns the file form, `notes` and `unmapped`. |
| `Direction` | — | **No owner.** Used by [05 §2](05-session-model.md#2-layout-the-bsp-tree) and [19 §2.4](19-onboarding.md#24-the-command-palette-as-the-universal-escape-hatch) but never defined in a code block anywhere. For layout it is superseded by `Axis` ([17 §1.2](17-panes-and-layout.md#12-types)); for directional focus, by `Direction2D { Left, Right, Up, Down }` ([17 §2.4](17-panes-and-layout.md#24-manual-resize--what-dragging-a-border-does)). |

## Proposed — remote continuity

**These names come from [`docs/design/remote-continuity.md`](../design/remote-continuity.md), a design document rather than an architecture document. They are proposed, not settled, and no architecture document owns them yet.**

| Name | Owner (proposed) | Definition |
|---|---|---|
| `ActorContinuity` | [rc §1.2](../design/remote-continuity.md#12-the-taxonomy) | `{ identity, recents, drafts, mutes, notify, read_marks, preferred_surface }` — the per-actor state that makes a second device a continuation rather than a second screen. Stored on the instance ([rc §1.3](../design/remote-continuity.md#13-where-per-actor-state-is-stored-and-why-on-the-instance)). |
| `Recent` | [rc §1.2](../design/remote-continuity.md#12-the-taxonomy) | `{ workspace, session: Option<SessionId>, last_active, via: DeviceId }` — one entry in "where you were". |
| `Draft` | [rc §2.4](../design/remote-continuity.md#24-drafts) | `{ key, text, caret, blobs, updated_at, updated_by, version }` — an unsent composition that follows the person across devices. |
| `DraftKey` | [rc §2.4](../design/remote-continuity.md#24-drafts) | `AgentPrompt { session } \| Interaction { interaction } \| PlanReview { interaction }` — what a draft is a draft *of*. |
| `NotifyPrefs` | [rc §5.6](../design/remote-continuity.md#56-noise-control) | `{ level, sessions: HashMap<SessionId, NotifyLevel>, quiet_hours, suppress_when_present, coalesce }`. |
| `NotifyLevel` | [rc §5.6](../design/remote-continuity.md#56-noise-control) | `Actionable \| Involved { turn_end_after } \| Everything \| Muted { until }`. `Actionable` is the default: an `Interaction` is worth a buzz, a turn ending usually is not. |
| `QuietHours` | [rc §5.6](../design/remote-continuity.md#56-noise-control) | `{ from: Time, to: Time, tz: String, allow_actionable: bool }`. |
| `InteractionActivity` | [rc §3.3](../design/remote-continuity.md#33-answering-right-now--the-one-new-presence-signal) | `{ interaction, actor, device, activity: CardActivity, expires_at }` — the one new presence signal, so two devices do not race the same card. It is advisory; the ledger ([12 §4.1](12-collaboration.md#41-the-invariant)) still decides. |
| `CardActivity` | [rc §3.3](../design/remote-continuity.md#33-answering-right-now--the-one-new-presence-signal) | `Viewing \| Composing \| Submitting`. |
| `PresencePeer` | [rc §3.1](../design/remote-continuity.md#31-the-single-user-two-device-case-is-the-common-one) | `{ kind: "me", device, label, liveness } \| { kind: "other", actor, label, liveness }` — the rendering-side view of presence, which distinguishes *my other phone* from *another person*. |
| `SurfaceIntent` | [rc §1.4](../design/remote-continuity.md#14-the-one-deliberate-exception-surface-intent) | `Auto \| Blocks \| Terminal` — the one per-device preference that deliberately does **not** follow the actor across devices. |

---

## Names that were unified, and what they replaced

Recorded so a stale draft is recognisable.

**Not** a unification, and the trap most likely to catch a reader: `ViewKind`
([17 §3.3](17-panes-and-layout.md#33-decision-per-client-layout-views-one-shared-default),
`Primary | Adaptive | Named`) and `ViewMode`
([05 §4](05-session-model.md#4-attachment-detach-and-multi-client-viewing),
`Grid | Blocks`) are **different concepts with confusingly similar names**.
`ViewKind` says what kind of *arrangement* a `LayoutView` is; `ViewMode` says
which *surface* a client attached with. Neither is an alias of the other, and
neither has been renamed.

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
| `Layout` (struct, [17 §1.3](17-panes-and-layout.md#13-the-whole-layout-of-a-workspace)) + `LayoutTree` ([17 §1.2](17-panes-and-layout.md#12-types)) | 05 §2's `Layout` **enum**, including its `Zoom { pane, saved }` variant (now `Option<PaneId>` beside the tree) |
| `Axis { Columns, Rows }` ([17 §1.2](17-panes-and-layout.md#12-types)) | `Direction { Horizontal, Vertical }` (YAML still accepts `horizontal`/`vertical` on read; `columns`/`rows` canonical on write) |
| `SizePolicy::Driver` ([17 §3.4](17-panes-and-layout.md#34-the-pty-size-question-which-per-client-layout-does-not-solve)) | `SizeOwner::Writer` ([07 §4.3](07-remote-protocol.md#43-the-resize-problem)); "minimum over participants" becomes `SizePolicy::Smallest` |
| `layout.*` ([17 §9.2](17-panes-and-layout.md#92-layout)) | `workspace.layout.get`/`.set`/`.preset` and `pane.layout.get` (deprecated aliases for two minor versions, per [03 §7](03-capability-catalog.md)) |
| `ChordResolution` ([16 §2.3](16-input-and-keymap.md#23-resolution)) | 16's `Resolution`, which collided with [18 §3](18-semantic-open.md#3-resolution)'s `Resolution` |
| `SessionModeSet` ([05 §1.1](05-session-model.md#11-types)) | 06's `ModeSet`, which collided with the keymap `ModeSet` ([16 §6.5](16-input-and-keymap.md#65-the-keymap-abstraction)) |
| `InstanceDegradation` ([22 §4.4](22-operations.md#44-per-session-fault-isolation-r7)) | 22's `Degradation`, which collided with [17 §2.3](17-panes-and-layout.md#23-minimums-and-what-happens-when-they-cannot-be-met)'s layout `Degradation` |
| `ContextSet` flags `terminal_focused` / `card_focused` ([16 §4.1](16-input-and-keymap.md#41-the-context-set)) | 10 §8.2's `session_focused` / `interaction_card_focused` (deprecated aliases) |
| `Target` in [`omt-types`](02-crate-map.md#omt-types), widened ([18 §2.6](18-semantic-open.md#26-the-match-and-target-types)) | the narrower `Target` declared in 04 §8.3, which now defers |
| `open.targets.list` / `open.resolve` ([18 §9](18-semantic-open.md#9-capabilities)) | `session.target_at` / `session.target_resolve` (sketched in 04 §8.3) |
| `AttachmentReference` ([09 §4.3.7](09-ssh-and-media.md#437-what-the-agent-finally-receives)) | `ImageReference` (kept as a `#[deprecated]` type alias) |
| `system.doctor { groups: Vec<DoctorGroup> }` ([22 §10](22-operations.md#10-capabilities)) | per-area doctor capabilities — a `doctor.term` ([19 §9](19-onboarding.md#9-capabilities-this-document-requires)), a `doctor.keys` ([16 §11](16-input-and-keymap.md)) or a `doctor.media` of their own. Doctor is one parameterized capability; documents contribute *groups*. The CLI spellings `omt doctor term`/`keys`/`media` remain |
| `system.health` (structured, [22 §4.1](22-operations.md#41-systemhealth)) **alongside** `instance.health` (egress only, [13 §9.1](13-security.md)) | two capabilities, one source of truth — `Health.egress` *is* what `instance.health` reports. No `instance.status` capability is declared ([19 §1.4](19-onboarding.md#14-second-run)) |
| dotted-lowercase event names — `history.appended`, `recall.doc.indexed`, `attention.raised`/`.cleared`, `usage.updated`, `digest.available` ([20 §12.5](20-recall-and-usage.md#125-attention)) | `HistoryAppended`, `DocIndexed`, `AttentionRaised`/`AttentionCleared`, `UsageUpdated`, `DigestAvailable`. **A PascalCase event name anywhere is a stale draft.** |
| `timeline.get` paging by opaque `from_cursor`/`next_cursor` ([20 §12.3](20-recall-and-usage.md#123-timeline--digest)) | `from_seq` — protocol `Seq` ([07 §5.1](07-remote-protocol.md#51-sequence-spaces)) is a different sequence space |
| `[store.retention] scrollback_max_bytes_per_session` ([21 §3.1](21-data-lifecycle.md#31-the-principle-compact-then-delete)) | `store.max_scrollback_bytes` (still spelled the old way in [22 §4.4](22-operations.md#44-per-session-fault-isolation-r7) and §9 — a stale reference) |
| `<redacted:CLASS[:detail]:len=N>` ([21 §2.3](21-data-lifecycle.md#23-what-gets-written-instead)) | every other redaction-marker format; this is *the* marker, and every document referring to one refers to this |
| `RevocationSubject` ([13 §3.1](13-security.md#31-the-trait)) | `is_revoked(&CredentialId)` — the unit a user revokes is a **device**, and a `DeviceGrant` carries no `CredentialId` |
| `Grant` ([13 §3.1](13-security.md#31-the-trait)) / `DeviceGrant` ([23 §1.3](23-identity-and-devices.md#13-devicegrant--the-certificate-that-makes-this-decentralized)) / `[[auth.tailnet.mappings]]` ([13 §3.5](13-security.md#35-tailnet-identity)) | one overloaded "grant". They are the auth *result*, the root-signed *certificate*, and a *config table* respectively, and are never interchangeable. `[[auth.tailnet.grants]]` is the stale spelling of the third |
| `Resync` ([07 §5.2](07-remote-protocol.md#52-replay-window)) | `resync_required` — the old spelling, now corrected everywhere in `docs/`; any reappearance is a stale draft |
