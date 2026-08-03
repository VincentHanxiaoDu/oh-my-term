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

**Roles** are `V`iewer < `O`perator < `A`dmin. They answer *who you shared this
instance with*; an authenticated `Operator` is equivalent to sitting at the TUI
([D2](../architecture/decisions.md#d2--remote-is-exactly-equivalent-to-local)).
Every capability below is reachable identically from the TUI, the CLI, the HTTP/WS
API and the web client, or carries an explicit parity exemption.

**CI rule** ([13 §4](../architecture/13-security.md#4-roles-and-their-mapping-onto-the-catalog)):
a capability declaring `role = Viewer` together with `WRITES_PTY`,
`SPAWNS_PROCESS`, `WRITES_FS`, `NETWORK` or `DESTRUCTIVE` fails the build. Every
row below satisfies it.

---

## `instance`

| Name | Verb | Kind | Role | Effects | Owner | Description |
|---|---|---|---|---|---|---|
| `instance.info` | `info` | Query | V | — | [07 §1.2](../architecture/07-remote-protocol.md#12-instance-identity) | Instance descriptor: id, name, version, proto, catalog hash, platform. |
| `instance.health` | `health` | Query | V | — | [13 §9.1](../architecture/13-security.md#91-egress) | Liveness plus which egress paths are enabled. |
| `instance.catalog` | `catalog` | Query | V | — | [07 §3.3](../architecture/07-remote-protocol.md#33-handshake-and-capability-negotiation) | The capability list, keyed by `catalog_hash` so a client can cache it. |
| `instance.peers.list` | `peers-list` | Query | V | — | [07 §1](../architecture/07-remote-protocol.md#1-topology-and-federation) | Other instances this one knows of. Federation is client-side; this is a hint list. |
| `instance.peers.add` | `peers-add` | Command | A | — | 07 §1 | Record a peer instance. |
| `instance.detach` | `detach` | Command | V | — | [05 §4](../architecture/05-session-model.md#4-attachment-detach-and-multi-client-viewing) | Detach this client from everything. |
| `instance.shutdown` | `shutdown` | Command | A | `DESTRUCTIVE` | 07 | Stop the daemon. **Parity-exempt** (`reason: admin, no phone affordance`). |

## `workspace`

| Name | Verb | Kind | Role | Effects | Owner | Description |
|---|---|---|---|---|---|---|
| `workspace.list` | `list` | Query | V | — | [05 §10.1](../architecture/05-session-model.md#101-workspace) | All open workspaces plus worktree groupings. |
| `workspace.get` | `get` | Query | V | — | 05 §10.1 | One workspace. |
| `workspace.open` | `open` | Command | O | `READS_FS` | 05 §10.1 | Open a path as a workspace; idempotent by canonical path. |
| `workspace.close` | `close` | Command | O | — | 05 §10.1 | Detach a workspace; sessions survive unless `close_sessions`. |
| `workspace.rename` | `rename` | Command | O | — | 05 §10.1 | Set the display name. |
| `workspace.layout.get` | `layout-get` | Query | V | — | 05 §10.1 | The BSP tree plus computed geometry. |
| `workspace.layout.set` | `layout-set` | Command | O | — | 05 §10.1 | Replace the tree. |
| `workspace.layout.preset` | `layout-preset` | Command | O | — | 05 §10.1 | `Even \| MainVertical \| MainHorizontal \| Tiled`. |
| `workspace.focus` | `focus` | Command | O | — | 05 §10.1 | Focus a pane. Focus is not write permission. |
| `workspace.history` | `history` | Query | V | — | [05 §9](../architecture/05-session-model.md#9-command-history) | Command history scoped to this workspace. |
| `workspace.worktree.list` | `worktree-list` | Query | V | `READS_FS`, `SPAWNS_PROCESS` | [15 §6](../architecture/15-workspace-explorer.md#6-capabilities) | Linked worktrees. Alias of `workspace.vcs.worktrees`. |
| `workspace.worktree.add` | `worktree-add` | Command | O | `WRITES_FS`, `SPAWNS_PROCESS` | 05 §10.1 | `git worktree add`; returns the new workspace. |
| `workspace.git.status` | `git-status` | Query | V | `READS_FS`, `SPAWNS_PROCESS` | 15 §6 | **Deprecated alias** for `workspace.vcs.summary`; kept two minor versions. |

### `workspace.files.*` / `workspace.vcs.*` — [15 §6](../architecture/15-workspace-explorer.md#6-capabilities)

All read-only. `omt-workspace-fs` ships **no** write capability: no `stage`,
`unstage`, `discard`, `apply` or `commit` ([15 §1.1](../architecture/15-workspace-explorer.md#11-decision-no-vcs-mutation-in-v1-including-stageunstage)).

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `workspace.files.list` | `files-list` | Query | V | `READS_FS` | Direct children of one directory. Never recurses. Etag-aware. |
| `workspace.files.list_many` | `files-list-many` | Query | V | `READS_FS` | Batch of the above; restores an expansion set in one round trip. |
| `workspace.files.stat` | `files-stat` | Query | V | `READS_FS` | One node's metadata. |
| `workspace.files.read` | `files-read` | Query | V | `READS_FS` | Bounded, byte-exact file read. Binary sniffed before decoding. |
| `workspace.files.find` | `files-find` | Query | V | `READS_FS` | Budgeted fuzzy filename search. |
| `workspace.files.watch` | `files-watch` | **Command** | V | `READS_FS` | Ref-counted watch lease. A Command because it mutates instance state. |
| `workspace.files.unwatch` | `files-unwatch` | Command | V | — | Release this client's lease; 1→0 drops the watcher. |
| `workspace.files.reveal` | `files-reveal` | Command | **O** | `READS_FS`, `SPAWNS_PROCESS` | Open the file in the daemon machine's `$EDITOR`. Positional argv, never a shell string. |
| `workspace.vcs.summary` | `vcs-summary` | Query | V | `READS_FS`, `SPAWNS_PROCESS` | Branch, HEAD, upstream, ahead/behind, dirty counts, in-progress operation. |
| `workspace.vcs.status` | `vcs-status` | Query | V | `READS_FS`, `SPAWNS_PROCESS` | Per-file status; `index` and `worktree` as separate axes. |
| `workspace.vcs.diff` | `vcs-diff` | Query | V | `READS_FS`, `SPAWNS_PROCESS` | Structured hunks for one file. Never a raw patch string. |
| `workspace.vcs.diff_many` | `vcs-diff-many` | Query | V | `READS_FS`, `SPAWNS_PROCESS` | "Review everything the agent changed" in one round trip. |
| `workspace.vcs.worktrees` | `vcs-worktrees` | Query | V | `READS_FS`, `SPAWNS_PROCESS` | Linked worktrees and their group. |

## `session` — [05 §10.2](../architecture/05-session-model.md#102-session)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `session.list` | `list` | Query | V | — | Sessions, optionally filtered by workspace, with presence and writer status. |
| `session.get` | `get` | Query | V | — | One session. |
| `session.create` | `create` | Command | O | `SPAWNS_PROCESS` | Spawn a shell, a command, or an agent with known argv — never by typing into a shell. |
| `session.close` | `close` | Command | O | `DESTRUCTIVE` | SIGHUP then SIGKILL after `close_grace`. |
| `session.restart` | `restart` | Command | O | `SPAWNS_PROCESS`, `DESTRUCTIVE` | Re-spawn the same argv/cwd/env, keeping old scrollback above a separator. |
| `session.rename` | `rename` | Command | O | — | Set the title override. |
| `session.attach` | `attach` | Command | V | — | Attach with `mode`, `since_seq`, viewport. Replies with a snapshot or `resync_required`. |
| `session.detach` | `detach` | Command | V | — | Leave; the session always keeps running. |
| `session.resize` | `resize` | Command | O | — | Report a viewport; `request_authoritative` needs the writer token ([07 §4.3](../architecture/07-remote-protocol.md#43-the-resize-problem)). |
| `session.signal` | `signal` | Command | O | `DESTRUCTIVE` | Send a signal to the foreground process group. |
| `session.send_text` | `send-text` | Command | O | `WRITES_PTY` | Type text; `submit` controls the trailing newline. Requires the writer token. |
| `session.send_keys` | `send-keys` | Command | O | `WRITES_PTY` | Typed key specs. Requires the writer token. |
| `session.write_bytes` | `write-bytes` | Command | O | `WRITES_PTY` | Raw bytes. Requires the writer token. Implicit under `send_text` for parity. |
| `session.scrollback.get` | `scrollback-get` | Query | V | — | `StyledLine`s from a `Position`. |
| `session.search` | `search` | Query | V | — | Resumable, budgeted search over logical lines. Results are `Position`s. |
| `session.target_at` | `target-at` | Query | V | — | The `Target` (URL, path, custom rule) at a position. Pure. |
| `session.target_resolve` | `target-resolve` | Query | V | `READS_FS` | Resolve against the block's `cwd`; carries `explorer: Option<ExplorerRef>`. |
| `session.blocks.list` | `blocks-list` | Query | V | — | Block summaries with `origin`, `attribution`, `failed`. |
| `session.blocks.get` | `blocks-get` | Query | V | — | One block's styled output, bounded. |
| `session.blocks.rerun` | `blocks-rerun` | Command | O | `WRITES_PTY`, `DESTRUCTIVE` | Re-run a previous command. Confirm gesture on every surface. |
| `session.blocks.fold` | `blocks-fold` | Command | V | — | Per-client fold state. |
| `session.writer.acquire` | `writer-acquire` | Command | O | — | Acquire, or `force: true` to open a 5 s takeover ([12 §3.3](../architecture/12-collaboration.md#33-lifecycle)). |
| `session.writer.release` | `writer-release` | Command | O | — | Release; token becomes `Free`. |
| `session.writer.keep` | `writer-keep` | Command | O | — | Holder cancels a pending takeover. Once per takeover. |
| `session.writer.status` | `writer-status` | Query | V | — | Who is driving, since when, at which epoch. |
| `session.history` | `history` | Query | V | — | Command history, any scope. |

## `pane` — [05 §10.3](../architecture/05-session-model.md#103-pane)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `pane.list` | `list` | Query | V | — | Panes in a workspace with geometry. |
| `pane.split` | `split` | Command | O | `SPAWNS_PROCESS` when it creates a session | Split; creates a session when `session` is omitted. |
| `pane.close` | `close` | Command | O | `DESTRUCTIVE` when `close_session` | Remove from the layout; the session survives by default. |
| `pane.focus` | `focus` | Command | O | — | Focus a pane. |
| `pane.navigate` | `navigate` | Command | O | — | Directional move by geometric adjacency. |
| `pane.move` | `move` | Command | O | — | Re-parent a pane. |
| `pane.swap` | `swap` | Command | O | — | Exchange two panes. |
| `pane.resize` | `resize` | Command | O | — | Adjust split weights. |
| `pane.zoom` | `zoom` | Command | O | — | Non-destructive, per-workspace zoom. |
| `pane.set_session` | `set-session` | Command | O | — | Retarget a pane at a different session. |
| `pane.scroll` | `scroll` | Command | V | — | Per-client view state. |
| `pane.select` | `select` | Command | V | — | Per-client selection. |

## `agent` — [06](../architecture/06-agent-layer.md)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `agent.state` | `state` | Query | V | — | The merged `AgentState` for a session's binding. |
| `agent.explain` | `explain` | Query | V | — | Every source, its tier, freshness, last event, which won and why. |
| `agent.bind` | `bind` | Command | O | — | Force a binding when detection is wrong. |
| `agent.unbind` | `unbind` | Command | O | — | Drop the binding and its retained evidence. |
| `agent.prompt` | `prompt` | Command | O | `WRITES_PTY` only when the sole path is synthesized keystrokes | Send a prompt through the agent's native channel where one exists. |
| `agent.interrupt` | `interrupt` | Command | O | `WRITES_PTY` | Interrupt the current turn. |
| `agent.queue.list` | `queue-list` | Query | V | — | The agent's own pending message queue, mirrored. |
| `agent.queue.enqueue` | `queue-enqueue` | Command | O | — | Queue work for an agent that is mid-turn. Additive, conflict-free. |
| `agent.queue.remove` | `queue-remove` | Command | O | — | Remove a queued item. |
| `agent.commands.list` | `commands-list` | Query | V | — | The agent's **own** resolved slash-command list. Never omt guessing. |
| `agent.commands.run` | `commands-run` | Command | O | — | Invoke one, through the agent's own channel. |

## `interaction` — [06 §5](../architecture/06-agent-layer.md#5-interactions--the-flagship-path) · [12 §4](../architecture/12-collaboration.md#4-interaction-ownership)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `interaction.list` | `list` | Query | V | — | Open interactions, optionally by session. |
| `interaction.get` | `get` | Query | V | — | One interaction, including its `viewers`. |
| `interaction.resolve` | `resolve` | Command | **O** | — | The flagship path. Exactly-once; idempotent by `(interaction, actor, response)`. An `Operator` may resolve **any** interaction the agent posed — omt adds no policy over the agent's permission semantics (D1) and no remote-only restriction (D2). |
| `interaction.cancel` | `cancel` | Command | O | — | Withdraw without answering, where the mechanism allows it. |

## `media` — [09](../architecture/09-ssh-and-media.md)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `media.clipboard.read` | `clipboard-read` | Query | O | — | Read the OS clipboard on *this* instance. `unsupported` with a diagnosed reason where the terminal cannot supply it. |
| `media.clipboard.write` | `clipboard-write` | Command | O | `WRITES_FS` (blob fallback) | OSC 52 / chunked / blob, per the tier ladder. Never claims success it cannot observe. |
| `media.blob.begin` | `blob-begin` | Command | O | `WRITES_FS` | Declare a transfer; answers `have: true` on a dedup hit. |
| `media.blob.commit` | `blob-commit` | Command | O | `WRITES_FS` | Verify size + digest and admit the blob. |
| `media.image.upload` | `image-upload` | Command | O | `WRITES_FS` | Web/drag-drop/camera ingress; composed over `blob.*`. |
| `media.image.paste` | `image-paste` | Command | O | `WRITES_FS` | Read clipboard → store → materialize → inject the agent's own reference syntax. |
| `media.file.push` | `file-push` | Command | O | `WRITES_FS` | Write a blob to a path on the instance, confined to the workspace root. |
| `media.file.pull` | `file-pull` | Command | O | `READS_FS` | Read a path into a blob for download. Directories are `tar+zstd`'d. |

## `stt` — [08 §7](../architecture/08-web-client.md#7-voice-input)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `stt.providers.list` | `providers-list` | Query | V | — | Configured providers, whether streaming, and whether audio leaves the machine. |
| `stt.session.start` | `session-start` | Command | O | `NETWORK` for hosted providers | Open a transcription session; audio rides the existing WebSocket as binary frames. |
| `stt.session.stop` | `session-stop` | Command | O | — | Finalize and return the transcript. |

## `config` — [10 §10](../architecture/10-configuration.md#10-config-capabilities)

| Name | Verb | Kind | Role | Effects | Description |
|---|---|---|---|---|---|
| `config.get` | `get` | Query | V | — | Value plus provenance `(layer, file, span)`. Secrets return `{ secret, present }`. |
| `config.schema` | `schema` | Query | V | — | `SettingDescriptor[]` plus JSON Schema. Drives both editors. |
| `config.set` | `set` | Command | per-key `min_role` | `WRITES_FS` (+ `DESTRUCTIVE` when lossy) | Explicit target layer; `toml_edit` preserves comments; validated before writing. |
| `config.unset` | `unset` | Command | per-key | `WRITES_FS` | Remove a key from one layer. |
| `config.validate` | `validate` | Query | V | `READS_FS` | The full five-pass diagnostic pipeline; same code as CI. |
| `config.reload` | `reload` | Command | O | `READS_FS` | Transactional reload; never partially applies. |
| `config.sources` | `sources` | Query | V | — | Per-key provenance across all six layers. |
| `config.default` | `default` | Query | V | — | The annotated default document. |
| `config.export` | `export` | Query | A | — | TOML for one layer; secrets as references, never values. |
| `config.import` | `import` | Command | A | `WRITES_FS` | With `dry_run` and diagnostics. |
| `config.pending` | `pending` | Query | V | — | Settings awaiting a restart. |
| `config.project.trust` | `project-trust` | Command | A | `WRITES_FS` | Trust or revoke a repo's `.omt/config.toml`. |

## `theme` / `keys` / `workflow` / `launch` — [10 §8–9](../architecture/10-configuration.md#8-themes-and-keybindings)

| Name | Kind | Role | Effects | Description |
|---|---|---|---|---|
| `theme.list` / `theme.get` | Query | V | `READS_FS` | Built-in and user themes. |
| `theme.import` | Command | A | `READS_FS`, `WRITES_FS` | another terminal YAML / iTerm2 `.itermcolors` → an omt theme file. Schema is an interface fact (P9). |
| `keys.list` | Query | V | — | The resolved keymap with provenance. |
| `keys.conflicts` | Query | V | — | `OMT-C4xx` chord/prefix/duplicate diagnostics. |
| `workflow.list` / `.get` | Query | V | `READS_FS` | Saved parameterized commands; the argument schema renders a mobile form. |
| `workflow.run` | Command | O | `WRITES_PTY` | Render placeholders and run in the PTY, or send to the agent when `kind: prompt`. |
| `workflow.save` / `.delete` | Command | O | `WRITES_FS` | — |
| `launch.list` / `.get` | Query | V | `READS_FS` | Named workspace/session/layout templates. |
| `launch.run` | Command | O | `SPAWNS_PROCESS` | Idempotent by name; attaches rather than duplicating. |
| `launch.save` / `.delete` | Command | O | `WRITES_FS` | `save` snapshots the current workspace. |

## `plugin` — [11 §5](../architecture/11-plugins.md#5-lifecycle)

| Name | Kind | Role | Effects | Description |
|---|---|---|---|---|
| `plugin.list` / `plugin.info` | Query | V | — | Registry entries, including `status = "invalid"` with diagnostics. |
| `plugin.install` | Command | A | `NETWORK`, `WRITES_FS`, `SPAWNS_PROCESS` | Registry / git / local path, with the consent screen and a pinned digest. |
| `plugin.uninstall` | Command | A | `WRITES_FS` | — |
| `plugin.enable` / `plugin.disable` | Command | A | `SPAWNS_PROCESS` | Live; contributions register/unregister transactionally. |
| `plugin.upgrade` | Command | A | `NETWORK`, `WRITES_FS`, `SPAWNS_PROCESS` | Re-prompts only on widened permissions. |
| `plugin.health` | Query | V | — | Liveness, drops, restarts, resource use. |
| `plugin.logs` | Query | A | — | Captured stderr ring buffer. |
| `plugin.call` | Command | per contributed capability | per declaration | Invoke a plugin-contributed capability. Role is `min(granted.role, triggering_actor.role)` ([11 §4.2](../architecture/11-plugins.md#42-the-escalation-rule)). |
| `plugin.permissions.get` / `.set` | Q / C | A | — | Inspect and reduce the granted set. |

## `notification` — [07 §8](../architecture/07-remote-protocol.md#8-notifications-to-a-closed-tab)

| Name | Kind | Role | Effects | Description |
|---|---|---|---|---|
| `notification.push.subscribe` | Command | O | — | Register a Web Push subscription per device credential. |
| `notification.push.unsubscribe` | Command | O | — | Drop it; a revoked device stops receiving pushes. |
| `notification.test` | Command | O | `NETWORK` | Send a test notification through the configured sinks. |

Sending a push is the **only** outbound connection omt ever makes, it is off by
default, and enabling it is an explicit consent step
([13 §9.1](../architecture/13-security.md#91-egress)).

## `presence`, `audit`, `events`

| Name | Kind | Role | Effects | Owner | Description |
|---|---|---|---|---|---|
| `presence.list` | Query | V | — | [12 §2](../architecture/12-collaboration.md#2-presence-is-first-class-state) | Who is attached, what they view, who is writing. Viewers are never hidden. |
| `audit.query` | Query | **A** | — | [12 §8](../architecture/12-collaboration.md#8-audit-log) | The append-only record. Never contains PTY bytes. |
| `events.subscribe` | Command | V | — | [07 §3.7](../architecture/07-remote-protocol.md#37-subscriptions) | Coarse `(sessions × workspaces × kinds)` filter plus `since_seq` and a lag policy. |
| `events.resume` | Command | V | — | 07 §5.2 | Resume from `since_seq`; replays, or answers `Resync` with a snapshot. |

---

## Parity exemptions

The allow-list is committed in the repo and printed in the generated docs; a new
exemption cannot be added silently
([03 §5](../architecture/03-capability-catalog.md#5-the-parity-contract)).

| Capability | Reason |
|---|---|
| `instance.shutdown` | Admin; no phone affordance is needed or wanted. |
| `debug.dump_grid` | Local-only diagnostic dump. |

## Known gaps in this draft

- Per-capability `since` versions are omitted; the generator emits them from the
  declarations.
- Input/output type names are given in the owning documents, not repeated here.
- The `debug.*` group is not enumerated beyond its one exemption.
