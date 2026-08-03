# Capability catalog — consolidated draft

> **This file is hand-written and temporary.** It is the reconciliation of every
> capability declared across `docs/architecture/`, assembled so that parallel
> implementation changes agree on names, kinds, roles and effects before the
> code that generates this file exists.
>
> Per [03 §5](../architecture/03-capability-catalog.md#5-the-parity-contract),
> artifact #4 of the parity contract is a **generated** `docs/reference/capabilities.md`,
> regenerated and diffed in CI. When that generator lands, this file is deleted
> and replaced by its output. Until then: the Rust `capability!` declaration in
> the owning crate is the source of truth, and a disagreement with this table
> means this table is stale.

**Effects** are the closed set `WRITES_PTY`, `SPAWNS_PROCESS`, `READS_FS`,
`WRITES_FS`, `NETWORK`, `DESTRUCTIVE`, and describe what **omt** does — never
what an agent's tool does
([D1](../architecture/decisions.md#d1--omt-adds-no-policy-layer-over-an-agents-permission-semantics)).
There is no `TOUCHES_FS`; it was split into `READS_FS` and `WRITES_FS`
([15 §6](../architecture/15-workspace-explorer.md#6-capabilities)) and every
prior declaration migrated.

The **Effects** column is always the declared **maximum** over all inputs. Where
a row reads "`DESTRUCTIVE` when `close_session`" or "only when `session` is
`None`", that is a `refine_effects` narrowing, not a conditional declaration:
the bit is declared unconditionally and narrowed per call by the pure
`refine_effects` of
[03 §2.1](../architecture/03-capability-catalog.md#21-conditional-effects).
Authorization, credential scoping and the `Viewer` CI rule below read the
maximum; the confirm gesture and the audit log read the refined set, and the
audit entry carries both when they differ.

**Intent.** Every `Command` also declares a
[D15](../architecture/decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)
delivery class. Rather than widen every table by a column, the classes are
consolidated in [one section at the end](#intent-classes-for-every-command),
which is also the only place the safety-relevant question — *which capabilities
must never be retried?* — can be answered at a glance. A `Command` absent from
that section is an unreconciled row, exactly as a `Command` with no `intent`
fails the build.

**Roles** are `V`iewer < `O`perator < `A`dmin. They answer *who you shared this
instance with*; an authenticated `Operator` is equivalent to sitting at the TUI
([D2](../architecture/decisions.md#d2--remote-is-exactly-equivalent-to-local)).
Every capability below is reachable identically from the TUI, the CLI, the HTTP/WS
API and the web client, or carries an explicit parity exemption.

**Kind** is `Query` or `Command`, spelled out. A `Command` is anything that
mutates instance state, even per-client state
([15 §6](../architecture/15-workspace-explorer.md#6-capabilities) on
`workspace.files.watch`).

**CI rule** ([13 §4](../architecture/13-security.md#4-roles-and-their-mapping-onto-the-catalog)):
a capability declaring `role = Viewer` together with `WRITES_PTY`,
`SPAWNS_PROCESS`, `WRITES_FS`, `NETWORK` or `DESTRUCTIVE` fails the build, with
one carve-out: `SPAWNS_PROCESS` **plus** `READS_FS` and no write bit is permitted
at `Viewer` when the capability spawns a fixed argv with no shell and the
subprocess is read-only, and is listed in the read-only subprocess allow-list.
Every row below satisfies the rule as amended, and no row carries a ⚠️ CI flag
any more: the `workspace.vcs.*` family is covered by the carve-out, and
`remote.probe` was corrected to `Operator` at its source ([22 §10](../architecture/22-operations.md#10-capabilities)).

---

## `instance` — [07 §1](../architecture/07-remote-protocol.md#1-topology-and-federation) · [22 §10](../architecture/22-operations.md#10-capabilities)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `instance.info` | `info` | Query | V | — | Descriptor: id, version, pid, socket, listeners, uptime, uid ([22 §10](../architecture/22-operations.md#10-capabilities)); plus name, proto and catalog hash ([07 §1.2](../architecture/07-remote-protocol.md#12-instance-identity)). |
| `instance.health` | `health` | Query | V | — | The **narrow, cheap** egress query: liveness plus which egress paths are enabled ([13 §9.1](../architecture/13-security.md#9-network-egress-and-supply-chain)). The full structured model is `system.health`. |
| `instance.catalog` | `catalog` | Query | V | — | The capability list, keyed by `catalog_hash` so a client can cache it ([07 §3.3](../architecture/07-remote-protocol.md#33-handshake-and-capability-negotiation)). |
| `instance.peers.list` | `peers-list` | Query | V | — | Other instances this one knows of. Federation is client-side; this is a hint list. |
| `instance.peers.add` | `peers-add` | Command | A | — | Record a peer instance. |
| `instance.detach` | `detach` | Command | V | — | Detach this client from everything ([05 §4](../architecture/05-session-model.md#4-attachment-detach-and-multi-client-viewing)). |
| `instance.shutdown` | `shutdown` | Command | A | `DESTRUCTIVE` | `{ force, wait, grace }` → `{ blocked_by, shutting_down }`. **Parity-exempt.** |
| `instance.restart` | `restart` | Command | A | `DESTRUCTIVE` | `{ force, wait }` → `{ blocked_by }` ([22 §10](../architecture/22-operations.md#10-capabilities)). |

`instance.status` does **not** exist — [19 §2.1](../architecture/19-onboarding.md#2-discoverability-without-documentation)
explicitly declines it ("a composed view whose only consumer is one row of chrome
is a catalog entry that exists to be a screenshot").

## `workspace` — [05 §10.1](../architecture/05-session-model.md#101-workspace)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `workspace.list` | `list` | Query | V | — | All open workspaces plus worktree groupings. |
| `workspace.get` | `get` | Query | V | — | One workspace. |
| `workspace.open` | `open` | Command | O | `READS_FS` | Open a path as a workspace; idempotent by canonical path. |
| `workspace.close` | `close` | Command | O | — | Detach a workspace; sessions survive unless `close_sessions`. |
| `workspace.rename` | `rename` | Command | O | — | Set the display name. |
| `workspace.focus` | `focus` | Command | O | — | Focus a pane. Focus is not write permission. |
| `workspace.history` | `history` | Query | V | — | Command history, scope forced to this workspace. A pre-scoped convenience over `history.query` ([20 §12.2](../architecture/20-recall-and-usage.md#122-history)). |
| `workspace.worktree.add` | `worktree-add` | Command | O | `WRITES_FS`, `SPAWNS_PROCESS` | `git worktree add`; returns the new workspace. |

### `workspace.files.*` / `workspace.vcs.*` — [15 §6](../architecture/15-workspace-explorer.md#6-capabilities)

All read-only. `omt-workspace-fs` ships **no** write capability: no `stage`,
`unstage`, `discard`, `apply` or `commit`
([15 §1.1](../architecture/15-workspace-explorer.md#11-decision-no-vcs-mutation-in-v1-including-stageunstage)).

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `workspace.files.list` | `files-list` | Query | V | `READS_FS` | Direct children of one directory. Never recurses. Etag-aware. |
| `workspace.files.list_many` | `files-list-many` | Query | V | `READS_FS` | Batch of the above; restores an expansion set in one round trip. |
| `workspace.files.stat` | `files-stat` | Query | V | `READS_FS` | One node's metadata. |
| `workspace.files.read` | `files-read` | Query | V | `READS_FS` | Bounded, byte-exact file read. Binary sniffed before decoding. |
| `workspace.files.find` | `files-find` | Query | V | `READS_FS` | Budgeted fuzzy filename search. |
| `workspace.files.watch` | `files-watch` | Command | V | `READS_FS` | Ref-counted watch lease. A Command because it mutates instance state. |
| `workspace.files.unwatch` | `files-unwatch` | Command | V | — | Release this client's lease; 1→0 drops the watcher. |
| `workspace.files.reveal` | `files-reveal` | Command | **O** | `READS_FS`, `SPAWNS_PROCESS` | Open the file in the daemon machine's `$EDITOR`. Positional argv, never a shell string. |
| `workspace.vcs.summary` | `vcs-summary` | Query | V | `READS_FS`, `SPAWNS_PROCESS` | Branch, HEAD, upstream, ahead/behind, dirty counts, in-progress operation. |
| `workspace.vcs.status` | `vcs-status` | Query | V | `READS_FS`, `SPAWNS_PROCESS` | Per-file status; `index` and `worktree` as separate axes. |
| `workspace.vcs.diff` | `vcs-diff` | Query | V | `READS_FS`, `SPAWNS_PROCESS` | Structured hunks for one file. Never a raw patch string. |
| `workspace.vcs.diff_many` | `vcs-diff-many` | Query | V | `READS_FS`, `SPAWNS_PROCESS` | "Review everything the agent changed" in one round trip. |
| `workspace.vcs.worktrees` | `vcs-worktrees` | Query | V | `READS_FS`, `SPAWNS_PROCESS` | Linked worktrees and their group. |

The `workspace.vcs.*` family is `Viewer` + `READS_FS`, `SPAWNS_PROCESS`, and
that is legal: it is the motivating case for the **read-only subprocess
carve-out** in [13 §4](../architecture/13-security.md#4-roles-and-their-mapping-onto-the-catalog).
Fixed `git` argv, no shell, no write bit, allow-listed. `workspace.files.reveal`
is not covered — it launches the user's editor — which is why it is `Operator`.

## `session` — [05 §10.2](../architecture/05-session-model.md#102-session)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `session.list` | `list` | Query | V | — | Sessions, optionally filtered by workspace, with presence and writer status. |
| `session.get` | `get` | Query | V | — | One session. |
| `session.create` | `create` | Command | O | `SPAWNS_PROCESS` | Spawn a shell, a command, or an agent with known argv — never by typing into a shell. `mode: Pty \| Native` ([D8](../architecture/decisions.md#d8--two-session-modes-pty-default-and-native-acp)). |
| `session.close` | `close` | Command | O | `DESTRUCTIVE` | SIGHUP then SIGKILL after `close_grace`. |
| `session.restart` | `restart` | Command | O | `SPAWNS_PROCESS`, `DESTRUCTIVE` | Re-spawn the same argv/cwd/env, keeping old scrollback above a separator. |
| `session.rename` | `rename` | Command | O | — | Set the title override. |
| `session.attach` | `attach` | Command | V | — | Attach with `mode`, `since_seq`, viewport. Replies with a snapshot or `Resync` ([07 §5.2](../architecture/07-remote-protocol.md#52-replay-window)). |
| `session.detach` | `detach` | Command | V | — | Leave; the session always keeps running. |
| `session.resize` | `resize` | Command | O | `WRITES_PTY` | Report a viewport. The `WRITES_PTY` bit — and the writer token — apply **only when `request_authoritative` is set** ([12 §3.1](../architecture/12-collaboration.md#31-what-it-governs), [07 §4.3](../architecture/07-remote-protocol.md#43-the-resize-problem)). |
| `session.size_policy` | `size-policy` | Command | O | `WRITES_PTY` | `{ session, policy: SizePolicy }` → `SessionSizing`; resizes the PTY. Owned semantically by [17 §9.2](../architecture/17-panes-and-layout.md#92-layout), but its `session` prefix is authoritative. Rejects `native` sessions. |
| `session.signal` | `signal` | Command | O | `DESTRUCTIVE` | Send a signal to the foreground process group. |
| `session.send_text` | `send-text` | Command | O | `WRITES_PTY` | Type text; `submit` controls the trailing newline. Requires the writer token. |
| `session.send_keys` | `send-keys` | Command | O | `WRITES_PTY` | Typed key specs. Requires the writer token. |
| `session.send_newline` | `send-newline` | Command | O | `WRITES_PTY` | Submit the composed line ([16 §8.1](../architecture/16-input-and-keymap.md#8-defaults)). |
| `session.write_bytes` | `write-bytes` | Command | O | `WRITES_PTY` | Raw bytes. Requires the writer token. |
| `session.scrollback.get` | `scrollback-get` | Query | V | — | `StyledLine`s from a `Position`. |
| `session.search` | `search` | Query | V | — | Resumable, budgeted search over logical lines. Results are `Position`s. |
| `session.capture` | `capture` | Query | V | `READS_FS` | `{ range: Blocks\|Lines\|All, format: Text\|Ansi\|Jsonl, max_bytes }` ([22 §10](../architecture/22-operations.md#10-capabilities)). `Ansi` returns `unsupported` for a `native` session. |
| `session.blocks.list` | `blocks-list` | Query | V | — | Block summaries with `origin`, `attribution`, `failed`. |
| `session.blocks.get` | `blocks-get` | Query | V | — | One block's styled output, bounded. |
| `session.blocks.rerun` | `blocks-rerun` | Command | O | `WRITES_PTY`, `DESTRUCTIVE` | Re-run a previous command. Confirm gesture on every surface — a property of omt's own capability, not a remote-only gate. |
| `session.blocks.fold` | `blocks-fold` | Command | V | — | Per-client fold state. |
| `session.writer.acquire` | `writer-acquire` | Command | O | — | Acquire, or `force: true` to open a 5 s takeover ([12 §3.3](../architecture/12-collaboration.md#3-the-writer-token)). |
| `session.writer.release` | `writer-release` | Command | O | — | Release; token becomes `Free`. |
| `session.writer.keep` | `writer-keep` | Command | O | — | Holder cancels a pending takeover. Once per takeover. |
| `session.writer.status` | `writer-status` | Query | V | — | Who is driving, since when, at which epoch. |
| `session.history` | `history` | Query | V | — | Command history, pre-scoped to this session over `history.query`. |

`session.send_text`, `session.send_keys`, `session.write_bytes`,
`session.resize` and `session.signal` return `unsupported` on a `Native` session.

## `pane` — [17 §9.1](../architecture/17-panes-and-layout.md#91-pane)

Owned by 17, **not** by [05 §10.3](../architecture/05-session-model.md#103-pane),
which defers to it for names and shapes. Every input carries an optional
`view: Option<ViewId>`; every mutation emits `LayoutChanged`.

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `pane.list` | `list` | Query | V | — | Panes in a workspace/view with geometry. |
| `pane.split` | `split` | Command | O | `SPAWNS_PROCESS` | Only when `session` is `None`. `{ pane, dir, ratio?, session?, top_level }`. |
| `pane.close` | `close` | Command | O | `DESTRUCTIVE` when `close_session` | Remove from the layout; the session survives by default. |
| `pane.focus` | `focus` | Command | O | — | Focus a pane. |
| `pane.navigate` | `navigate` | Command | O | — | `{ from, dir, wrap }` — directional move by geometric adjacency. |
| `pane.focus_last` | `focus-last` | Command | O | — | Return to the previously focused pane. |
| `pane.focus_index` | `focus-index` | Command | O | — | `{ index: u16 }`. |
| `pane.focus_cycle` | `focus-cycle` | Command | O | — | `{ reverse }`. |
| `pane.focus_edge` | `focus-edge` | Command | O | — | `{ dir }` — jump to the edge pane. |
| `pane.resize` | `resize` | Command | O | — | `{ target: ResizeTarget, amount: ResizeAmount }` → `{ applied, clamped, layout }`. 05's `{ pane, edge, delta }` is an accepted input variant. |
| `pane.move` | `move` | Command | O | — | `{ pane, to, dir }` — re-parent a pane. |
| `pane.move_to_workspace` | `move-to-workspace` | Command | O | — | `{ pane, workspace, view? }`. |
| `pane.swap` | `swap` | Command | O | — | Exchange two panes. |
| `pane.rotate` | `rotate` | Command | O | — | `{ split?, reverse }`. |
| `pane.zoom` | `zoom` | Command | O | — | Non-destructive, per-view zoom. |
| `pane.set_session` | `set-session` | Command | O | — | Retarget a pane at a different session. |
| `pane.mark` | `mark` | Command | O | — | `{ pane, mark: Option<MarkId> }`. |
| `pane.stack.create` | `stack-create` | Command | O | — | `{ panes: [PaneId] }`. |
| `pane.stack.expand` | `stack-expand` | Command | O | — | Expand a stacked pane. |
| `pane.stack.move` | `stack-move` | Command | O | — | Move within the stack. |
| `pane.stack.break_out` | `stack-break-out` | Command | O | — | Leave the stack. |
| `pane.float` | `float` | Command | O | — | `{ pane, floating, rect: Option<FractionalRect> }`. |
| `pane.scroll` | `scroll` | Command | V | — | Per-client view state. |
| `pane.select` | `select` | Command | V | — | Per-client selection. |

## `layout` — [17 §9.2](../architecture/17-panes-and-layout.md#92-layout)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `layout.get` | `get` | Query | V | — | `{ workspace, view?, area? }` → `{ layout, geometry?, degraded }`. |
| `layout.set` | `set` | Command | O | — | Replace the tree from a `LayoutSpec`. |
| `layout.preset` | `preset` | Command | O | — | `Even \| MainVertical \| MainHorizontal \| Tiled`. |
| `layout.balance` | `balance` | Command | O | — | `{ split?, recursive }`. |
| `layout.floats.toggle` | `floats-toggle` | Command | O | — | `{ view?, visible: Option<bool> }`. |
| `layout.synchronize` | `synchronize` | Command | O | `WRITES_PTY`, `DESTRUCTIVE` | Broadcast input to a pane group. |
| `layout.views.list` | `views-list` | Query | V | — | `{ views: [ViewInfo] }`. |
| `layout.views.create` | `views-create` | Command | O | — | `{ workspace, name, from: Option<ViewId> }`. |
| `layout.views.close` | `views-close` | Command | O | — | Refuses `Primary`. |
| `layout.views.select` | `views-select` | Command | O | — | Per-client; `{ view, pin }`. |
| `layout.promote` | `promote` | Command | O | — | Mutates `Primary`; visible to all. |
| `layout.adopt` | `adopt` | Command | O | — | Adopt another view's layout. |
| `layout.rearm` | `rearm` | Command | O | — | Re-enable responsive swapping ([17 §4.2](../architecture/17-panes-and-layout.md#4-presets-dsl-saved-layouts)). |
| `layout.save` | `save` | Command | O | `WRITES_FS` | `{ name, view?, scope: User \| Project }` → `{ path }`. |
| `layout.apply_saved` | `apply-saved` | Command | O | `SPAWNS_PROCESS` | `{ name, view? }` → `{ layout }`. |
| `layout.list_saved` | `list-saved` | Query | V | — | `{ layouts: [SavedLayoutInfo] }`. |
| `layout.import_tmux` | `import-tmux` | Command | O | — | `{ string, view? }` → `{ layout, warnings }`. |

## `open` — [18 §9](../architecture/18-semantic-open.md#9-capabilities)

Parity exemptions: **none**. Every one has a TUI binding, a web handler and a
generated doc entry.

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `open.targets.list` | `targets-list` | Query | V | — | Every match in a scope, with spans and rule ids. Pure; no filesystem access. |
| `open.resolve` | `resolve` | Query | V | `READS_FS` | Resolve one match: cwd resolution, cached stat, sensitivity, ordered `ActionOffer`s. Idempotent. |
| `open.activate` | `activate` | Command | O | `READS_FS`, `SPAWNS_PROCESS`, `WRITES_PTY`, `NETWORK` | The one mutating entry point. Declares the **union** of its handlers' effects; the audit record carries the actual handler's. |
| `open.handlers.list` | `handlers-list` | Query | V | — | Registry contents, with per-target applicability when a match is named. |
| `open.hints.begin` | `hints-begin` | Command | O | — | Enter hint mode for one client's viewport. Instance-owned, so TUI, browser and phone share one label assignment. |
| `open.hints.select` | `hints-select` | Command | O | `READS_FS`, `SPAWNS_PROCESS`, `WRITES_PTY`, `NETWORK` | Feed keystrokes in; returns the narrowed set or the activation. |
| `open.hints.cancel` | `hints-cancel` | Command | O | — | Close the hint session. |
| `open.rules.list` | `rules-list` | Query | V | — | `{rules: [RuleInfo]}` — id, kind, precedence, enabled, source layer. |
| `open.rules.test` | `rules-test` | Query | V | — | `{text, rules?}` → `{matches, overlaps_resolved}`; powers `omt open rules test` and the config editor preview. |
| `open.remote.list` | `remote-list` | Query | V | `READS_FS` | The §6.3 sidecars: remote path, host, fetched-at, hash, `read_only`, writeback state. |
| `open.remote.discard` | `remote-discard` | Command | O | `WRITES_FS` | Drop a mirrored file and stop its watcher. |

## `agent` — [06](../architecture/06-agent-layer.md) · [22 §8.3](../architecture/22-operations.md#8-automation-and-ci-g14)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `agent.state` | `state` | Query | V | — | The merged `AgentState` for a session's binding. |
| `agent.explain` | `explain` | Query | V | — | Every source, its tier, freshness, last event, which won and why. |
| `agent.bind` | `bind` | Command | O | — | Force a binding when detection is wrong. |
| `agent.unbind` | `unbind` | Command | O | — | Drop the binding and its retained evidence. |
| `agent.prompt` | `prompt` | Command | O | `WRITES_PTY` only when the sole path is synthesized keystrokes | Send a prompt through the agent's native channel where one exists. The writer token is required only on the keystroke path ([12 §3.1](../architecture/12-collaboration.md#31-what-it-governs)). |
| `agent.interrupt` | `interrupt` | Command | O | `WRITES_PTY` | Interrupt the current turn. |
| `agent.wait` | `wait` | Query | V | — | Block until an agent reaches a terminal state → `AgentWaitResult` ([22 §8.3](../architecture/22-operations.md#8-automation-and-ci-g14)). |
| `agent.queue.list` | `queue-list` | Query | V | — | The agent's own pending message queue, mirrored. |
| `agent.queue.enqueue` | `queue-enqueue` | Command | O | — | Queue work for an agent that is mid-turn. Additive, conflict-free. |
| `agent.queue.remove` | `queue-remove` | Command | O | — | Remove a queued item. |
| `agent.commands.list` | `commands-list` | Query | V | — | The agent's **own** resolved slash-command list. Never omt guessing. |
| `agent.commands.run` | `commands-run` | Command | O | — | Invoke one, through the agent's own channel. |

## `interaction` — [06 §5](../architecture/06-agent-layer.md#5-interactions--the-flagship-path) · [12 §4](../architecture/12-collaboration.md#4-interaction-ownership) · [16 §4.4](../architecture/16-input-and-keymap.md#4-modes-and-contexts)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `interaction.list` | `list` | Query | V | — | Open interactions, optionally by session. |
| `interaction.get` | `get` | Query | V | — | One interaction, including its `viewers`. |
| `interaction.resolve` | `resolve` | Command | **O** | — | The flagship path. Exactly-once; idempotent by `(interaction_id, identity_or_device, intent_id)` — **not** by the actor, which changes on every reconnect ([D15](../architecture/decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism) c6, [12 §4.1](../architecture/12-collaboration.md#4-interaction-ownership)). A losing caller gets `conflict` with a discriminating `detail.state` of `already_resolved` / `cancelled` / `abandoned` (D15 c10). **Not** writer-token-gated ([12 §3.1](../architecture/12-collaboration.md#31-what-it-governs)). An `Operator` may resolve **any** interaction the agent posed (D1, D2). |
| `interaction.cancel` | `cancel` | Command | O | — | Withdraw without answering, where the mechanism allows it. |
| `interaction.focus_latest` | `focus-latest` | Command | O | — | Jump to the most recent open interaction ([16 §11](../architecture/16-input-and-keymap.md#11-capabilities-introduced-here)). |

## `media` — [09](../architecture/09-ssh-and-media.md) · [16 §7.3](../architecture/16-input-and-keymap.md#7-omt-ssh--local-feels-remote-native)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `media.clipboard.read` | `clipboard-read` | Query | O | — | Read the OS clipboard on *this* instance. Returns `unsupported` with a diagnosed `source` where the terminal cannot supply it. |
| `media.clipboard.write` | `clipboard-write` | Command | O | `WRITES_FS` (blob fallback) | OSC 52 / chunked / blob, per the tier ladder. Never claims success it cannot observe. |
| `media.blob.begin` | `blob-begin` | Command | O | `WRITES_FS` | Declare a transfer `{ len, mime, filename, hash }`; answers `have: true` on a dedup hit. |
| `media.blob.commit` | `blob-commit` | Command | O | `WRITES_FS` | Verify size + digest and admit the blob. |
| `media.blob.abort` | `blob-abort` | Command | O | `WRITES_FS` | Cancel an in-flight transfer; the partial is unlinked immediately ([09 §5](../architecture/09-ssh-and-media.md#5-case-b-image-paste-over-ssh--the-core-mechanism)). This is *the* cancel operation — 16 §7.3's `media.transfer.cancel` was a second name for it and is gone. |
| `media.image.upload` | `image-upload` | Command | O | `WRITES_FS` | Web/drag-drop/camera ingress; composed over `blob.*`. |
| `media.image.paste` | `image-paste` | Command | O | `WRITES_FS` | Read clipboard → store → materialize → inject the agent's own reference syntax. |
| `media.picker.open` | `picker-open` | Command | O | `READS_FS` | omt's own fuzzy file browser over the workspace, reusing the explorer index ([09 §4.3](../architecture/09-ssh-and-media.md#4-getting-images-in-the-easy-cases)). A TUI has no OS file dialog. 16 §11's `media.paste_picker` was a second name for this one and is gone. |
| `media.file.push` | `file-push` | Command | O | `WRITES_FS` | Write a blob to a path on the instance, confined to the workspace root unless the caller is `Admin` and `allow_outside_workspace` is set. |
| `media.file.pull` | `file-pull` | Command | O | `READS_FS` | Read a path into a blob for download. Directories are `tar+zstd`'d. |

## `keys` — [10 §10](../architecture/10-configuration.md#10-config-capabilities) · [16 §11](../architecture/16-input-and-keymap.md#11-capabilities-introduced-here) · [19 §9](../architecture/19-onboarding.md#9-capabilities-this-document-requires)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `keys.list` | `list` | Query | V | — | The resolved keymap with provenance; extended with `deliverable` and `shadows` per binding. |
| `keys.conflicts` | `conflicts` | Query | V | — | `OMT-C401`–`OMT-C412` and `OMT-C420`–`OMT-C425` chord/prefix/duplicate diagnostics. |
| `keys.explain` | `explain` | Query | V | — | What a chord does here; reports the active keymap and mode. |
| `keys.keymaps` | `keymaps` | Query | V | — | Available keymaps and their inheritance; gains `unmapped` for [19 §5.2](../architecture/19-onboarding.md#5-tmux-and-zellij-migration-and-coexistence). |
| `keys.registry` | `registry` | Query | V | — | Dump an `InnerKeymap`. |
| `keys.probe` | `probe` | Query | V | — | The `TerminalProfile` ([16 §5.5](../architecture/16-input-and-keymap.md#5-conflict-detection-and-resolution)). |
| `keys.capture` | `capture` | Command | O | — | Record a chord's bytes for the corpus. |

## `tui` — [16 §11](../architecture/16-input-and-keymap.md#11-capabilities-introduced-here)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `tui.open_command_palette` | `open-command-palette` | Command | V | — | Open the palette ([16 §3.5](../architecture/16-input-and-keymap.md#3-the-leader-key)). |
| `tui.open_keymap_help` | `open-keymap-help` | Command | V | — | Open the keymap cheat sheet. |
| `tui.set_keymap` | `set-keymap` | Command | O | — | Switch keymap at runtime (`Runtime` layer); `omt keys use vim`. |
| `tui.copy_mode.*` | `copy-mode-*` | Command | O | — | Motions, selection, yank — the action set the vim and emacs keymaps both target ([16 §6.6](../architecture/16-input-and-keymap.md#6-modal-keymaps-vim-mode-and-emacs-mode), §6.7). Enumerated in 16, not restated here. |

## `setup` / `onboarding` / `help` / `migrate` / `term` / `uninstall` / `nesting` — [19 §9](../architecture/19-onboarding.md#9-capabilities-this-document-requires)

All palette-visible with a `title` unless marked hidden.

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `setup.status` | `status` | Query | V | — | What is installed, versions, manifest. |
| `setup.plan` | `plan` | Query | O | `READS_FS` | The full six-step plan with diffs, unapplied. The web client renders this. |
| `setup.apply` | `apply` | Command | A | `WRITES_FS` | One step at a time; `{ step, accept: true }`; refuses without a matching plan hash. |
| `setup.detect_agents` | `detect-agents` | Query | V | `READS_FS` | Which agent CLIs are present ([19 §3.3](../architecture/19-onboarding.md#3-omt-setup)). |
| `onboarding.hint_state` | `hint-state` | Query | V | — | Which hints remain; the one-hint rule is observable. |
| `onboarding.dismiss_hints` | `dismiss-hints` | Command | O | `WRITES_FS` | Permanent. |
| `help.topics` | `topics` | Query | V | — | The generated help tree. |
| `help.render` | `render` | Query | V | — | `{ topic \| context }` → the help overlay's model. |
| `migrate.tmux.preview` | `tmux-preview` | Query | O | `READS_FS` | Parse + map + report, writing nothing. |
| `migrate.tmux.apply` | `tmux-apply` | Command | A | `WRITES_FS` | Requires the preview's hash. |
| `migrate.zellij.preview` | `zellij-preview` | Query | O | `READS_FS` | Best-effort ([19 §5.1](../architecture/19-onboarding.md#5-tmux-and-zellij-migration-and-coexistence)). |
| `migrate.zellij.apply` | `zellij-apply` | Command | A | `WRITES_FS` | Best-effort. |
| `term.profile` | `profile` | Query | V | — | Advertised `TERM`, entry resolution result, outer-terminal probe, disagreements. |
| `term.install_terminfo` | `install-terminfo` | Command | A | `WRITES_FS` | Local or, with an explicit host, remote via ssh — confirmation required. |
| `uninstall.plan` | `plan` | Query | A | `READS_FS` | The dry run of removing omt's whole footprint. |
| `uninstall.apply` | `apply` | Command | A | `WRITES_FS`, `DESTRUCTIVE` | Confirm gesture on every surface. **Not** `service.uninstall` or `plugin.uninstall`. |
| `nesting.state` | `state` | Query | V | — | Per-pane multiplexer detection, feeding the badge. |

**Doctor is one parameterized capability.** [19 §7.4](../architecture/19-onboarding.md#7-term-and-terminfo--r6)
contributes a `Term` group to `system.doctor { groups }` rather than declaring a
`doctor.term`. `doctor.term`, `doctor.keys`, `doctor.keys.fix` and `doctor.media`
are **CLI spellings** (`omt doctor term`), not catalog entries.

## `search` / `history` / `timeline` / `digest` / `compare` — [20 §12](../architecture/20-recall-and-usage.md#12-capabilities)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `search.query` | `query` | Query | V | — | `SearchQuery` → `SearchResults`, grouped by session, with coverage. |
| `search.explain` | `explain` | Query | V | — | `{ factors: [(name, f64)], total, bm25 }` for one doc. |
| `search.suggest` | `suggest` | Query | V | — | `{ terms, recent_queries }`. |
| `search.stats` | `stats` | Query | V | — | `{ docs, bytes, oldest, newest, by_kind, excluded_sessions }`. |
| `search.reindex` | `reindex` | Command | **A** | `READS_FS`, `WRITES_FS` | `{ scope, since? }` → `{ job: JobId }`. |
| `history.query` | `query` | Query | V | — | The **instance-scoped** query and the only implementation; `session.history` and `workspace.history` are pre-scoped conveniences over it. |
| `history.get` | `get` | Query | V | — | `HistoryEntry` + `{ block, output_available }`. |
| `history.import` | `import` | Command | O | `READS_FS`, `WRITES_FS` | `{ shell, path?, dry_run }` → `{ imported, skipped, redacted, preview }`. |
| `history.forget` | `forget` | Command | O | `DESTRUCTIVE`, `WRITES_FS` | Selector-shaped; deletes rows *and* FTS entries in one transaction. The narrow counterpart to `store.purge`. |
| `timeline.get` | `get` | Query | V | — | `{ session, from_cursor?, limit, collapse, kinds? }`. `TimelineCursor` is deliberately not `seq`. |
| `timeline.stats` | `stats` | Query | V | — | `TimelineStats`. |
| `digest.get` | `get` | Query | V | — | `{ scope, window }` → `Digest`. |
| `digest.since_last_seen` | `since-last-seen` | Query | V | — | Window from the caller's durable **read mark**, not from presence. |
| `compare.sessions` | `sessions` | Query | V | — | `{ sessions, base_ref? }` → `{ rows: [ComparisonRow], base_resolved }`. Single-instance; multi-instance is client-side fan-out. |

## `usage` / `attention` — [20 §12.4–12.5](../architecture/20-recall-and-usage.md#124-usage)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `usage.query` | `query` | Query | V | — | `{ scope, since, until?, group_by }` → `UsageReport`. |
| `usage.limits` | `limits` | Query | V | — | `{ limits: [RateLimitState] }`. |
| `usage.session` | `session` | Query | V | — | `{ totals, by_model, context_window, last_event }`. |
| `attention.get` | `get` | Query | V | — | `{ session }` → the current attention state for one session: `{ signals, since, snoozed_until, cleared_at }`. **Required by the protocol, not merely convenient:** [07 §5.2](../architecture/07-remote-protocol.md#52-replay-window) makes refetching it one of the two mandatory refetches after a `Resync`, because live state plus the replay window cannot reconstruct attention that came and went inside an offline gap. It was assumed by 07 and declared nowhere; added here. `attention.list` is the instance-wide form of the same data. |
| `attention.list` | `list` | Query | V | — | `{ signals: [AttentionSignal] }`. |
| `attention.explain` | `explain` | Query | V | — | `{ baselines, reasons, thresholds, snoozes }`. |
| `attention.snooze` | `snooze` | Command | O | `WRITES_FS` | Snoozes are persisted with the binding. |
| `attention.clear` | `clear` | Command | O | `WRITES_FS` | `{ cleared: u32 }`. |

## `store` — [21 §8](../architecture/21-data-lifecycle.md#8-capabilities)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `store.usage` | `usage` | Query | V | `READS_FS` | `{ total_bytes, free_bytes, entries, last_sweep, next_sweep, notes }`. |
| `store.paths` | `paths` | Query | V | `READS_FS` | `{ purpose: All\|BackupExclusion }` → paths with kind, mode and advice. |
| `store.export` | `export` | Command | O | `READS_FS`, `WRITES_FS` | `Omtz\|Jsonl`, fidelity `Text\|Ansi\|Full` → `{ path, bytes, counts, manifest_sha256 }`. |
| `store.export.progress` | `export-progress` | Query | V | — | `{ job }` → `{ done, total, phase }`. |
| `store.import` | `import` | Command | O | `WRITES_FS` | `{ archive, as_workspace? }` → `{ workspace, counts, warnings }`. |
| `store.purge` | `purge` | Command | O | `WRITES_FS`, `DESTRUCTIVE` | Bulk, scope-shaped destruction with a `PurgeManifest`, `dry_run` and a typed `confirm`. |
| `store.quarantine.list` | `quarantine-list` | Query | A | `READS_FS` | `{ items: [{ path, bytes, created, reason }] }`. |
| `store.repair` | `repair` | Command | A | `WRITES_FS`, `DESTRUCTIVE` | `{ dry_run, only? }` → `{ plan: [RepairStep], executed }`. |
| `store.sweep` | `sweep` | Command | O | `WRITES_FS` | `{ full }` → `{ reclaimed_bytes, duration, resumed }`. |
| `store.retention.get` | `retention-get` | Query | V | — | `RetentionPolicy` for a scope. |
| `store.retention.set` | `retention-set` | Command | A | `WRITES_FS` | Replace the policy. |
| `store.persist.get` | `persist-get` | Query | V | — | `{ scrollback, block_output, agent_events, index, source }`. |
| `store.persist.set` | `persist-set` | Command | O | `WRITES_FS`, `DESTRUCTIVE` when `delete_existing` | Per-session or per-workspace persistence flags. |
| `store.redaction.explain` | `redaction-explain` | Query | O | `READS_FS` | `{ findings: [{ class, span, replacement, block, at }], stats }`. |
| `store.redaction.test` | `redaction-test` | Query | V | — | `{ text }` → `{ findings, redacted }`. How a user validates an `extra_patterns` entry without running a session. |
| `store.migrate.status` | `migrate-status` | Query | V | — | `{ store_version, binary_version, pending, backups }`. |

## `system` / `service` / `upgrade` / `remote` — [22 §10](../architecture/22-operations.md#10-capabilities)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `system.health` | `health` | Query | V | — | `{ include: [Sessions\|Clients\|Store\|Sources] }` → the full structured `Health` model ([22 §4.1](../architecture/22-operations.md#4-health-metrics-logs-and-fault-isolation)). `Health.egress` is the same data the narrow `instance.health` reports. |
| `system.doctor` | `doctor` | Query | V | `READS_FS` | `{ groups: Vec<DoctorGroup>, strict }` → `{ checks, passed, failed, warnings }`. Both a leaf capability and a namespace prefix; the catalog permits it because names resolve as whole strings. CLI: `omt doctor`, `omt doctor term`, `omt doctor keys`. |
| `system.doctor.fix` | `doctor-fix` | Command | A | `WRITES_FS` | `{ checks: Vec<CheckId> }` → `{ fixed, failed }`. CLI: `omt doctor fix`. |
| `system.log.tail` | `log-tail` | Query | A | `READS_FS` | Stream of `{ ts, level, target, span, message }`. `Admin` because a log tail, even redacted, is closer to the raw record than anything else. |
| `system.log.level` | `log-level` | Command | A | — | `{ directive }` → `{ applied, previous }`. |
| `system.metrics` | `metrics` | Query | A | — | Prometheus text, when `[metrics].enabled`. |
| `system.bug_report` | `bug-report` | Command | A | `READS_FS`, `WRITES_FS` | `{ include, dest, review }` → `{ path, bytes, manifest, redaction_findings }`. |
| `service.install` | `install` | Command | A | `WRITES_FS`, `SPAWNS_PROCESS` | `Auto\|Launchd\|Systemd`, `autostart`. |
| `service.status` | `status` | Query | V | — | `{ installed, loaded, restarts, since, lingering }`. |
| `service.uninstall` | `uninstall` | Command | A | `WRITES_FS`, `DESTRUCTIVE` | Removes the **service unit and nothing else**; omt stays installed. |
| `upgrade.check` | `check` | Query | O | `NETWORK` | `{ current, available, notes, store_migration }`. Emits `upgrade.available` only after an explicit call — never from a background poll. |
| `upgrade.apply` | `apply` | Command | A | `NETWORK`, `WRITES_FS`, `SPAWNS_PROCESS`, `DESTRUCTIVE` | `{ installed, restart: RestartPlan, blocked_by }`. |
| `remote.bootstrap` | `bootstrap` | Command | O | `NETWORK`, `WRITES_FS`, `SPAWNS_PROCESS` | Install omt onto a host that lacks it, with explicit consent (R38). |
| `remote.probe` | `probe` | Query | O | `NETWORK` | `{ host }` → `{ present, version?, os, arch, libc, has_zstd }`. `Operator`, matching `remote.bootstrap`: it opens an outbound SSH connection, and there is no carve-out for `Viewer` + `NETWORK`. |

## `identity` — [23 §12](../architecture/23-identity-and-devices.md#12-capabilities)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `identity.get` | `get` | Query | V | — | `Identity` + `this_device` + `devices` + `instances`. |
| `identity.create` | `create` | Command | A | `WRITES_FS` | `{ display_name }` → `{ identity, recovery_codes: [String; 10] }`. |
| `identity.enroll` | `enroll` | Command | A | `WRITES_FS` | `{ identity_id, root_pub, home_hint? }`. |
| `identity.export` | `export` | Command | A | `WRITES_FS` | Passphrase-encrypted; `include_credentials`, `include_recovery`. |
| `identity.import` | `import` | Command | A | `WRITES_FS` | `{ blob, passphrase, merge }` → `{ registry_epoch, devices, instances }`. |
| `identity.inspect` | `inspect` | Query | A | `READS_FS` | The identity file header, unencrypted. |
| `identity.rotate_key` | `rotate-key` | Command | A | `DESTRUCTIVE`, `NETWORK` | `{ confirm: "rotate" }` → `{ new_identity, updated, unreachable }`. |
| `identity.prefs.get` | `prefs-get` | Query | V | — | `IdentityPrefs`. |
| `identity.prefs.set` | `prefs-set` | Command | O | `WRITES_FS` | `{ key, value, version }` → `{ version }`. |
| `identity.recovery.generate` | `recovery-generate` | Command | A | `DESTRUCTIVE` | Invalidates the previous set of 10 codes. |
| `identity.recovery.use` | `recovery-use` | Command | A¹ | `WRITES_FS` | `{ code, device }` → `{ grant, registry }`. |

¹ Declared `Admin` **because** it carries `WRITES_FS` and the CI rule forbids a
`Viewer` there. It is nevertheless reachable pre-authentication — it *is* an
authentication path — via a short-lived `Admin` grant scoped by
`capabilities = {identity.recovery.use}`. Rate-limited 3/hour/instance, audited
loudly, and it notifies every registered device.

## `device` — [23 §12](../architecture/23-identity-and-devices.md#12-capabilities)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `device.list` | `list` | Query | V | — | `{ include_revoked }` → `[Device]` with `DeviceRef` rendering fields. |
| `device.get` | `get` | Query | V | — | One `Device`. |
| `device.rename` | `rename` | Command | O | `WRITES_FS` | `{ device, label }` → `{ registry_epoch }`. |
| `device.pair.begin` | `pair-begin` | Command | A | `WRITES_FS` | `qr \| code \| link` → `{ pairing, code, url, qr_svg_ref, expires_at }`. |
| `device.pair.complete` | `pair-complete` | Command | A² | `WRITES_FS` | → `{ grant, registry, verification_words: [String; 4] }`. |
| `device.pair.confirm` | `pair-confirm` | Command | A | `WRITES_FS` | `{ pairing, confirmed }` → `{ device }`. |
| `device.pair.cancel` | `pair-cancel` | Command | A | — | Abandon a pairing. |
| `device.revoke` | `revoke` | Command | A | `DESTRUCTIVE`, `NETWORK` | `{ device, reason }` → `{ applied_on, pending_on }`. |
| `device.revoke_all` | `revoke-all` | Command | A | `DESTRUCTIVE`, `NETWORK` | `{ except, confirm: "revoke-all" }`. |
| `device.reauth.get` | `reauth-get` | Query | V | — | `ReauthPolicy`. |
| `device.reauth.set` | `reauth-set` | Command | A | `WRITES_FS` | Replace the policy. |
| `device.stepup.challenge` | `stepup-challenge` | Command | V | — | `{ capability }` → `{ challenge, methods, allow_credentials? }`. The authentication path itself; carries no effect bits. |
| `device.stepup.verify` | `stepup-verify` | Command | V | — | `{ challenge, assertion }` → `{ verified_at, freshness_s }`. |

² Authorized by the pairing token, mapped by the dispatch layer to a short-lived
`Admin` grant scoped by `capabilities = {device.pair.complete}` — a use of the
existing scope mechanism, not a new one.

## `instance.registry` — [23 §12](../architecture/23-identity-and-devices.md#12-capabilities)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `instance.registry.get` | `registry-get` | Query | V | — | `{ since_epoch? }` → the registry or `{ unchanged: true }`. |
| `instance.registry.add` | `registry-add` | Command | A | `WRITES_FS` | `{ label, endpoints, instance_pub }` → `InstanceRegistration`. |
| `instance.registry.remove` | `registry-remove` | Command | A | `DESTRUCTIVE` | `{ instance }` → `{ registry_epoch }`. |
| `instance.registry.set_home` | `registry-set-home` | Command | A | `WRITES_FS`, `NETWORK` | → `{ home, registry_epoch, passkey_reregistration_required }`. |
| `instance.registry.import` | `registry-import` | Command | A | `WRITES_FS` | `{ snapshot, signature }` → `{ registry_epoch }`. |
| `instance.registry.sync` | `registry-sync` | Command | O | `NETWORK`, `WRITES_FS` | → `{ registry_epoch, merged, conflicts }`. |
| `instance.registry.revocations.get` | `revocations-get` | Query | V | — | `{ since_epoch }` → `[Revocation]`. |
| `instance.registry.revocations.push` | `revocations-push` | Command | A | `WRITES_FS` | `{ revocations, epoch }` → `{ applied }`. **Parity-exempt.** |

## `invite` / `join` — [13 §3.2](../architecture/13-security.md#3-authentication) · [07 §1.3](../architecture/07-remote-protocol.md#13-adding-an-instance)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `invite.create` | `create` | Command | **A** | `WRITES_FS` | `{ role, ttl?, scope, max_uses? }` → `{ id, token, url, expires_at }`. The token is returned once and never stored recoverably; only the `jti` record is kept, until expiry. CLI: `omt invite`. |
| `invite.list` | `list` | Query | **A** | — | `{ include_expired, include_consumed }` → `[InviteRecord { id, role, scope, issued_at, expires_at, max_uses, uses, consumed_by }]`. Never returns a token. CLI: `omt invite list`. |
| `invite.revoke` | `revoke` | Command | **A** | `WRITES_FS`, `DESTRUCTIVE` | `{ id }` → `{ revoked, was_consumed }`. Idempotent. Does **not** revoke a credential the invite already produced — that is `device.revoke`. CLI: `omt invite revoke <id>`. |
| `join.exchange` | `exchange` | Command | **A**³ | `WRITES_FS` | `{ token, device: DeviceRegistration }` → `{ credential, role, scope, instance, registry }`. Consumes the invite's `jti`, binds the resulting credential to the device's public key, and returns a long-lived bearer credential. Replaying the invite fails. |

³ `Admin` because it carries `WRITES_FS`, exactly as `identity.recovery.use` is.
It is nevertheless reachable **pre-authentication by construction — it *is* an
authentication path**: no credential is presented, the invite signature is the
authorization, and reachability is a named, enumerated exemption in the dispatch
chain rather than a weakened role. Rate-limited per source address and per `jti`,
and every exchange is audited.

## `config` — [10 §10](../architecture/10-configuration.md#10-config-capabilities)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `config.get` | `get` | Query | V | — | Value plus provenance `(layer, file, span)`. Secrets return `{ secret, present }`. |
| `config.schema` | `schema` | Query | V | — | `SettingDescriptor[]` plus JSON Schema. Drives both editors. |
| `config.set` | `set` | Command | per-key `min_role` | `WRITES_FS` (+ `DESTRUCTIVE` when lossy) | Explicit target layer; `toml_edit` preserves comments; validated before writing. `DESTRUCTIVE` on a lossy `Restart`-class change is what makes mobile require a confirm gesture. |
| `config.unset` | `unset` | Command | per-key | `WRITES_FS` | Remove a key from one layer. |
| `config.validate` | `validate` | Query | V | `READS_FS` | The full five-pass diagnostic pipeline; same code as CI. |
| `config.reload` | `reload` | Command | O | `READS_FS` | Transactional reload; never partially applies. |
| `config.sources` | `sources` | Query | V | — | Per-key provenance across all six layers. |
| `config.default` | `default` | Query | V | — | The annotated default document. |
| `config.export` | `export` | Query | A | — | TOML for one layer; secrets as references, never values. |
| `config.import` | `import` | Command | A | `WRITES_FS` | With `dry_run` and diagnostics. |
| `config.pending` | `pending` | Query | V | — | Settings awaiting a restart. |
| `config.project.trust` | `project-trust` | Command | A | `WRITES_FS` | Trust or revoke a repo's `.omt/config.toml`. |

## `theme` / `workflow` / `launch` — [10 §8–9](../architecture/10-configuration.md#8-themes-and-keybindings)

| Name | Kind | Role | Effects | Description |
|---|---|---|---|---|
| `theme.list` / `theme.get` | Query | V | `READS_FS` | Built-in and user themes. |
| `theme.import` | Command | A | `READS_FS`, `WRITES_FS` | another terminal YAML / iTerm2 `.itermcolors` → an omt theme file. Schema is an interface fact (P9). |
| `workflow.list` / `workflow.get` | Query | V | `READS_FS` | Saved parameterized commands; the argument schema renders a mobile form. |
| `workflow.run` | Command | O | `WRITES_PTY` | Render placeholders and run in the PTY, or send to the agent when `kind: prompt`. |
| `workflow.save` / `workflow.delete` | Command | O | `WRITES_FS` | — |
| `launch.list` / `launch.get` | Query | V | `READS_FS` | Named workspace/session/layout templates. |
| `launch.run` | Command | O | `SPAWNS_PROCESS` | Idempotent by name; attaches rather than duplicating. |
| `launch.save` / `launch.delete` | Command | O | `WRITES_FS` | `save` snapshots the current workspace. |

## `plugin` — [11 §5](../architecture/11-plugins.md#5-lifecycle)

| Name | Kind | Role | Effects | Description |
|---|---|---|---|---|
| `plugin.list` / `plugin.info` | Query | V | — | Registry entries, including `status = "invalid"` with diagnostics. |
| `plugin.install` | Command | A | `NETWORK`, `WRITES_FS`, `SPAWNS_PROCESS` | Registry id / git URL / `owner/repo` / local path, with the consent screen and a pinned content hash. |
| `plugin.uninstall` | Command | A | `WRITES_FS` | Removes one plugin. Not `uninstall.apply`, not `service.uninstall`. |
| `plugin.enable` / `plugin.disable` | Command | A | `SPAWNS_PROCESS` | Live; contributions register/unregister transactionally, no restart. |
| `plugin.upgrade` | Command | A | `NETWORK`, `WRITES_FS`, `SPAWNS_PROCESS` | Stop → swap → start. Re-prompts only on widened permissions. |
| `plugin.health` | Query | V | — | Liveness, drops, restarts, resource use. |
| `plugin.logs` | Query | A | — | Captured stderr ring buffer. |
| `plugin.call` | Command | per contributed capability | per declaration | Invoke a plugin-contributed capability. Role is `min(granted.role, triggering_actor.role)` ([11 §4.2](../architecture/11-plugins.md#4-permissions)). |
| `plugin.permissions.get` | Query | A | — | Inspect the granted set. |
| `plugin.permissions.set` | Command | A | — | Reduce the granted set. |

## `stt` — [08 §7](../architecture/08-web-client.md#7-voice-input)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `stt.providers.list` | `providers-list` | Query | V | — | Configured providers, whether streaming, and whether audio leaves the machine. |
| `stt.session.start` | `session-start` | Command | O | `NETWORK` for hosted providers | Open a transcription session; audio rides the existing WebSocket as binary frames, with a container hint for Safari's `audio/mp4`. |
| `stt.session.stop` | `session-stop` | Command | O | — | Finalize and return the transcript. |

## `notification` — none in v1 — [07 §8](../architecture/07-remote-protocol.md#8-notifications-to-a-closed-tab--none-in-v1)

**There is no `notification` capability group.**
[D12](../architecture/decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead)
removes push from v1 outright, so `notification.push.subscribe`,
`notification.push.unsubscribe` and `notification.test` are **not v1 capabilities
and must not appear in the catalog**. Clients discover pending work by
reconnecting and replaying
([07 §8.2](../architecture/07-remote-protocol.md#82-what-replaces-it-open-and-replay)).

The property this buys: **omt makes no outbound network connection at all** — the
daemon accepts connections and never initiates one. The `Notifier` trait is
specified and reserved with zero implementations
([07 §8.3](../architecture/07-remote-protocol.md#83-the-reserved-extension-point))
for a future native app or a user plugin.

## `presence`, `audit`, `events`

| Name | Kind | Role | Effects | Owner | Description |
|---|---|---|---|---|---|
| `presence.list` | Query | V | — | [12 §2](../architecture/12-collaboration.md#2-presence-is-first-class-state) | Who is attached, what they view, who is writing. Viewers are never hidden. |
| `audit.query` | Query | **A** | — | [12 §8](../architecture/12-collaboration.md#8-audit-log) | The append-only record, `0600`. Never contains PTY bytes. |
| `events.subscribe` | Command | V | — | [07 §3.7](../architecture/07-remote-protocol.md#3-message-protocol) | Coarse `(sessions × workspaces × kinds)` filter plus `since_seq` and a lag policy. `kinds` includes `workspace_fs` ([15 §6](../architecture/15-workspace-explorer.md#6-capabilities)). |
| `events.resume` | Command | V | — | [07 §5.2](../architecture/07-remote-protocol.md#5-resume-and-reliability) | Resume from `since_seq`; replays, or answers `Resync` with a snapshot. |

**`media.transfer.progress` is an event, not a capability.** It is emitted while
a `media.blob.*` transfer is in flight ([09 §5.3](../architecture/09-ssh-and-media.md#53-tier-2--the-in-band-osc-bridge)),
and both the TUI progress line and the web progress bar are driven by it
([16 §7.3](../architecture/16-input-and-keymap.md#7-omt-ssh--local-feels-remote-native)).
It is not in the `media` capability table above and must not be added to it.

---

## `continuity` — **proposed, not yet in the catalog**

> ⚠️ [`docs/design/remote-continuity.md`](../design/remote-continuity.md) is a
> **design** document, not an architecture document. Its §10 opens with
> "Everything below **does not exist yet**." Nothing in this section is declared;
> it is reproduced so that names are not accidentally reused, and it must not be
> counted as part of the catalog until the design is promoted.

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `continuity.get` | `get` | Query | O | — | This actor's continuity state: `recents`, `drafts`, `notify`, `read_marks`. |
| `continuity.touch` | `touch` | Command | O | — | Record that this actor is (or stopped) working somewhere. Called on session focus, debounced 5 s client-side. |
| `continuity.draft.set` | `draft-set` | Command | O | — | Upsert or clear a draft. `version` is CAS; a mismatch returns `precondition_failed` with the current draft. |
| `continuity.notify.set` | `notify-set` | Command | O | — | Notification preferences, mutes, quiet hours. |
| `continuity.notification.ack` | `notification-ack` | Command | O | — | A device acknowledges a delivered notification so the instance can dismiss it on this identity's other devices and record latency. **Parity-exempt on the CLI.** |
| `interaction.activity` | `activity` | Command | O | — | Advisory card activity ("answering right now"). Ephemeral, TTL 8 s, never persisted. Note this lands in the **existing `interaction` group**. |

One modification to an existing capability is also proposed:
**`session.writer.acquire` gains `assume_idle: bool`**, honoured only when
`holder.identity == requester.identity` and the holder is soft-free by the
server's clock. It is not `force`: it opens no `PendingTakeover`, fires no
countdown, and is audited as `acquire`.

---

## Intent classes for every `Command`

Per [03 §2.2](../architecture/03-capability-catalog.md#22-intent-class-d15) and
[D15](../architecture/decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism).
`Query` capabilities declare no `intent` and are omitted. Dispatch reads the
class to decide what a repeated `RequestId` means
([03 §3.5](../architecture/03-capability-catalog.md#35-the-dispatch-path)).

### `ExternallyConfirmed` — never retried, by anything

The whole list, deliberately short and deliberately first. These write into a
sink omt does not own; a duplicate is not a duplicate row but a keystroke
landing somewhere else entirely. Dispatch rejects a repeated `RequestId` with
`conflict`, and an unconfirmed write goes to `Undelivered`, never to a replay.

| Capability | Confirmed by |
|---|---|
| `interaction.resolve` | the agent recording the answer — hook `PostToolUse`, a transcript entry, or a tool result ([06 §5.1](../architecture/06-agent-layer.md#5-interactions--the-flagship-path)) |
| `agent.prompt` | the agent's own echo of the submitted prompt |
| `agent.commands.run` | the agent's command-invocation receipt |
| `agent.interrupt` | the observed transition out of `Working` |
| `media.image.paste` | the injected reference appearing in the agent's input |

> **Choice made here, and it is not free.** `interaction.resolve` is
> `ExternallyConfirmed` **unconditionally**, even though a `native`-mode or ACP
> resolve has a real response channel and would qualify as `Cas`. `intent` is a
> static declaration and cannot vary per call, so the class must be the
> strictest the capability can require; declaring `Cas` would let dispatch
> replay a synthetic injection from cache, which is the one thing D15 forbids
> outright. The cost is that a native-channel resolve gives up dispatch-level
> retry it could have had — it still gets the ledger's own CAS on
> `(interaction_id, identity_or_device, intent_id)`, which is where exactly-once
> actually lives. Flagged rather than assumed.

### `RawStream` — rejected loudly on repeat

`session.send_text`, `session.send_keys`, `session.send_newline`,
`session.write_bytes`, `session.blocks.rerun`, `layout.synchronize`,
`session.signal`, and `workflow.run` (it renders placeholders and runs the
result in the PTY; a replay runs the command twice).

Resumption is the writer `epoch` plus the consumed-offset `ack`
([07 §3.6](../architecture/07-remote-protocol.md#36-binary-payloads)), never
repetition.

`open.activate` and `open.hints.select` are **also `RawStream`**, for the same
reason and against first appearances: both declare `WRITES_PTY` because one of
their handlers inserts text at the shell prompt, and a replayed insertion lands
in whatever the user is typing now. The class is set by the most dangerous
handler in the union, not by the common case — the same rule that makes their
`effects` a union in the first place ([18 §9](../architecture/18-semantic-open.md#9-capabilities)),
and it is removed from the `Cas` residue below.

### `Append { dedup }`

`agent.queue.enqueue` (D15 c3: also carries a `BindingId`, requires
`AgentState::Working`, and carries `valid_until`), `history.import`,
`keys.capture`, `store.export`, `search.reindex`, `system.bug_report`.

### `Lww`

Per-client or per-identity soft state where a visible loser is the right
outcome: `pane.scroll`, `pane.select`, `session.blocks.fold`,
`layout.views.select`, `identity.prefs.set`, and the proposed
`continuity.draft.set`.

### `Cas`

**Everything else** — every remaining `Command` in every table above. These have
a CAS target (a version, a tree epoch, a registry epoch, a file layer) plus
`(identity, intent_id)`, so a repeated `RequestId` returns the original result.
This is the large majority: the whole of `workspace.*`, `pane.*` (other than the
`Lww` rows), `layout.*` (other than `synchronize`), `session.*` lifecycle,
`config.*`, `store.*`, `identity.*`, `device.*`, `instance.*`, `plugin.*`,
`service.*`, `upgrade.*`, `setup.*`, `migrate.*`, `theme.*`, `workflow.*`,
`launch.*`, `stt.*`, `media.*` other than `image.paste`, `open.*` other than
`activate` and `hints.select`, `workflow.*` other than `run`,
`attention.snooze`/`clear`, `interaction.cancel`, `agent.bind`/`unbind`,
`agent.queue.remove`, `events.subscribe`/`resume`.

It is stated as a residue rather than enumerated **because the enumeration is
the generator's job**, and a second hand-maintained list of ~120 names would
drift from the declarations within a week — the failure this whole document
exists to postpone. The three unsafe classes are enumerated exhaustively because
they are short, they are the ones a reader must be able to audit, and a mistake
in them is a silently wrong answer rather than a redundant round trip.

## Parity exemptions

The allow-list is committed in the repo and printed in the generated docs; a new
exemption cannot be added silently
([03 §5](../architecture/03-capability-catalog.md#5-the-parity-contract)).

| Capability | Surface | Reason (verbatim from the source) | Source |
|---|---|---|---|
| `instance.shutdown` | all | admin, no phone affordance | [07](../architecture/07-remote-protocol.md) |
| `debug.dump_grid` | all | local-only diagnostic dump | [03](../architecture/03-capability-catalog.md#5-the-parity-contract) |
| `instance.registry.revocations.push` | all | `peer-to-peer replication; no human surface` | [23 §12](../architecture/23-identity-and-devices.md#12-capabilities) |
| `continuity.notification.ack` | **CLI only** | `no notification surface` | [remote-continuity §10.4](../design/remote-continuity.md#104-parity-notes) — proposed, along with the rest of `continuity.*` |

[18 §9](../architecture/18-semantic-open.md#9-capabilities) states explicitly that
the `open` group takes **no** exemptions, and
[23 §12](../architecture/23-identity-and-devices.md#12-capabilities) that
`identity.export`/`import` and `device.pair.*` need none.

## Known gaps in this draft

**Proposed names that are deliberately *not* listed above.**
[19 §5.1 note 4](../architecture/19-onboarding.md#5-tmux-and-zellij-migration-and-coexistence)
marks these "**proposed, not yet declared** — these names appear nowhere in the
catalog". They are the tmux-parity backlog, they appear in 19's example keymap
TOML with a `# proposed` comment, and they must not be treated as catalog
entries: `pane.show_numbers`, `media.paste_buffer`, `media.buffers.list`,
`media.buffers.save`, `media.buffers.load`, `session.pipe`, `tui.redraw`.
([21 §9](../architecture/21-data-lifecycle.md#9-open-questions)'s
`session.blocks.forget` is likewise proposed only, and must be specified against
both `store.purge` and `history.forget` before it is declared.)

**The `continuity.*` group is design-document status, not architecture.** It has
its own section above, fenced off deliberately. `interaction.activity` is the
one proposed name that lands inside an *existing* group, which makes it the
easiest to mistake for a declared capability.

**Unresolved.**

- **`since` versions are omitted.** Only [15 §6](../architecture/15-workspace-explorer.md#6-capabilities)
  (`since = "0.4"`) and [18 §9](../architecture/18-semantic-open.md#9-capabilities)
  (`since = "0.5"`) state them; every other document omits the field. The
  generator emits them from the declarations.
- **`tui.copy_mode.*` is a wildcard.** 16 §11 declares the family without
  enumerating leaves; the individual motion/selection/yank capability names are
  in §6.6–6.7 prose and are not reproduced here.
- Input/output type names are given in the owning documents, not repeated here.
- The `debug.*` group is not enumerated beyond its one exemption.
