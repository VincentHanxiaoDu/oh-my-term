# Capability Catalog and Surface Parity

This is the mechanism behind principle **P3**: *anything the native TUI can do,
the API exposes, and the web client can do*. It is the most load-bearing piece
of the architecture, because every other subsystem is reached through it.

---

## 1. The idea

A **capability** is a single named operation on an omt instance: a *command*
(mutates) or a *query* (reads). Capabilities are declared once, in one place,
with their types. From that single declaration omt derives:

1. the in-process dispatch used by the TUI,
2. the HTTP/WebSocket routes used by remote clients,
3. the JSON Schema for inputs and outputs,
4. the TypeScript client the web app imports,
5. the `omt <group> <verb>` CLI subcommand tree,
6. the reference documentation,
7. the parity test matrix.

Nothing is hand-written twice, so nothing can drift.

```
                      ┌───────────────────────┐
                      │  capability catalog   │   declared once
                      └───────────┬───────────┘
        ┌─────────────┬───────────┼───────────┬─────────────┐
        ▼             ▼           ▼           ▼             ▼
   in-process     HTTP/WS     JSON Schema   TS client    CLI tree
   dispatch        routes                                 + docs
        │             │                         │
     omt-tui      omt-server                 web client
```

## 2. Declaring a capability

Capabilities are declared with a macro that produces both a type and a registry
entry. The macro exists so the declaration is data — it can be enumerated at
build time for codegen and at run time for the parity test.

```rust
capability! {
    /// Send text to a session's PTY as if typed by the user.
    name  = "session.send_text",
    group = "session",
    verb  = "send-text",
    kind  = Command,
    role  = Role::Operator,
    input  = SendText { session: SessionId, text: String, submit: bool },
    output = SendTextAck { seq: Seq },
    /// Human-facing name. Shown in the command palette and the CLI help.
    title = "Send text",
    aliases = ["type"],
    // hidden = false is the default; `hidden = true` requires `hidden_reason`.
    /// Effects declared for auditing and for the mobile UI's confirmation rules.
    /// This is the *maximum* set; see "conditional effects" below.
    effects = [Effects::WRITES_PTY],
    /// D15: which delivery class this mutation belongs to, and therefore
    /// whether dispatch may retry it.
    intent = Intent::RawStream,
}
```

Fields:

| Field | Purpose |
|---|---|
| `name` | stable dotted id; the wire name; never renamed without a deprecation shim |
| `group`/`verb` | derive the CLI (`omt session send-text`) and the REST path (`POST /v1/session/send-text`) |
| `kind` | `Command` or `Query`; queries are cacheable and safe to retry |
| `role` | minimum role: `Viewer` < `Operator` < `Admin` |
| `input`/`output` | `serde` + `schemars` types; the schema source of truth |
| `effects` | declared side-effect bits, the **maximum** over all inputs; the closed set is `WRITES_PTY`, `SPAWNS_PROCESS`, `READS_FS`, `WRITES_FS`, `NETWORK`, `DESTRUCTIVE` |
| `refine_effects` | optional `fn(&Input) -> EffectBits`, narrowing `effects` for one call. See below |
| `intent` | D15 delivery class; determines whether a retry is safe |
| `title` | short human-facing name; **imperative, ≤ 40 chars** ("Send text", not "Session Send-Text"). Required unless `hidden` |
| `aliases` | extra strings the command palette and CLI match on |
| `hidden` | omit from the palette and from `--help`; still callable and still parity-tested. Requires `hidden_reason` |
| `hidden_reason` | why it is hidden; enumerated in the generated docs so hiding is never silent |
| `since` | version introduced; drives compatibility docs |

**`title` is required for every non-`hidden` capability, not derived.** A derived
title (title-casing `group` + `verb`) yields "Session Send-Text" and "Layout
Apply Saved", which is what a command palette must never show — the palette is
[19 — Onboarding](19-onboarding.md)'s primary discovery surface and its whole
value is that entries read like things a person wants to do. Deriving it also
makes the palette silently worse every time a `verb` is chosen for CLI ergonomics
rather than for prose. The parity test (§5) enforces the requirement, so the cost
of writing one is paid once, at declaration, by the person with the most context.
`aliases` exist because users search for the word they know ("type", "paste",
"maximize") rather than the word omt chose, and they are matched but not
displayed. `hidden` removes a capability from the palette and from `--help`
without removing it from the catalog, the API, or the parity test — it is a
presentation flag, never an access control.

`effects` describes what **omt** does when the capability runs. It matters for
surfaces — the mobile client uses `DESTRUCTIVE` to require a confirm gesture —
and for the audit log, which records effects per call. It is deliberately *not*
a permission input: authorization is the `role` compare plus credential scope,
and per [D1](decisions.md#d1--omt-adds-no-policy-layer-over-an-agents-permission-semantics)
`effects` never describes an agent's tool call, only omt's own operation. The
`Viewer`+`WRITES_FS`/`DESTRUCTIVE` consistency check is
[13 §4](13-security.md#4-roles-and-their-mapping-onto-the-catalog).

### 2.1 Conditional effects

Several capabilities' effects genuinely depend on their input.
`pane.split { session: None }` spawns a process and `pane.split { session: Some }`
does not; `layout.apply_saved` spawns only for the panes it must create;
`pane.close { close_session: true }` is destructive and `close_session: false`
is not. A purely static `effects` field forces a bad choice between two wrong
answers: under-declare and the audit log is false, or always declare and a
`DESTRUCTIVE` confirm gesture fires on the harmless nine calls out of ten, which
teaches users to tap through confirmations — the failure that makes every
confirmation in the product worthless.

**Decision: `effects` becomes a declared maximum, narrowable per call by an
optional pure `refine_effects`.**

```rust
capability! {
    name = "pane.split",
    /// The maximum. `pane.split` may spawn a process.
    effects = [Effects::SPAWNS_PROCESS],
    /// Evaluated on the validated input, before the handler runs.
    /// MUST be pure, MUST NOT widen: `refine_effects(i) ⊆ effects`.
    refine_effects = |i: &PaneSplit| {
        if i.session.is_none() { Effects::SPAWNS_PROCESS } else { Effects::empty() }
    },
    intent = Intent::Cas,
}
```

Rules:

- **`refine_effects` may only remove bits, never add them.** Asserted in debug
  builds and checked for every capability by a property test over generated
  inputs, so the declared maximum stays a sound upper bound.
- **Static consumers use `effects`.** The generated docs, the
  `Viewer`+`DESTRUCTIVE` consistency check ([13 §4](13-security.md)) and
  credential scoping all read the maximum, because they reason about a capability
  without an input in hand. A capability that *can* be destructive is treated as
  destructive for authorization; refinement is never an authorization bypass.
- **Per-call consumers use the refined set.** The confirm gesture and **the audit
  log** record what this call actually did. The audit-log consequence, stated
  explicitly: an entry's `effects` is the refined set, and an investigator reading
  "`pane.split`, effects: `{}`" learns that call spawned nothing. Both are
  recorded when they differ (`effects` and `effects_declared`), so a reviewer can
  always see that a narrowing happened rather than inferring it.
- **The refinement runs before the handler**, on validated input, so a surface
  can ask "would this call need a confirm?" without performing it. That query is
  `instance.catalog`'s `dry_run_effects`.

### 2.2 Intent class (D15)

Every capability of `kind = Command` declares an `intent`, naming which of
[D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)'s
five classes it belongs to. Dispatch reads it to decide whether a repeated
`RequestId` may be served from the recent-results cache, replayed, or must be
rejected — so retry safety is a declared property of each capability rather than
a judgement call in each transport.

| `Intent` | Mechanism | On a repeat |
|---|---|---|
| `Cas` | CAS on version-or-state, plus `(identity, intent_id)` | **return the original result** |
| `Append { dedup }` | client `intent_id` + bounded server dedup cache | **return the original entry**, do not append |
| `RawStream` | writer `epoch` + consumed-offset `ack` | **reject loudly**; never replayed |
| `ExternallyConfirmed` | at-most-once write, confirmed by observation, `Undelivered` on timeout | **never retry**, by any actor except a human who can see the screen |
| `Lww` | CAS on `version`, visible loser | return the current value |

`Query` capabilities declare no `intent`; they are safe to retry by definition.
A `Command` with no `intent` fails the build — the omission is exactly the defect
D15 was written to fix, and defaulting it would reintroduce the defect silently.
`intent` is orthogonal to `effects`: the first says how to deliver a mutation,
the second says what the mutation does.

## 3. Dispatch

```rust
#[async_trait]
pub trait CapabilityHandler<C: Capability>: Send + Sync {
    async fn call(&self, ctx: &CallContext, input: C::Input) -> Result<C::Output, CapabilityError>;
}

pub struct CallContext {
    pub actor: Actor,        // which client/credential, or Local for the TUI
    pub role: Role,
    pub request_id: RequestId,
    pub deadline: Option<Instant>,
}
```

The registry maps `name → ErasedHandler`. There is exactly one dispatch path:

```
caller → CapabilityRegistry::dispatch(name, json, ctx) → handler → output
```

The TUI calls it directly with `Actor::Local`. `omt-server` calls it after
authenticating and mapping the credential to a `Role`. The CLI calls it over the
local socket. Because authorization is applied in dispatch and not in the
transport, no surface can accidentally bypass it.

**Errors** are a closed enum with stable codes (`not_found`, `conflict`,
`unauthorized`, `precondition_failed`, `unsupported`, `internal`), each carrying
a human message and optional structured detail, so every surface can render them
consistently.

## 4. Events are the read-side twin

Capabilities are request/response. Live state flows through the event bus
(`omt-events`), and subscription is itself a capability
(`events.subscribe`) so the same auth and the same schema generation apply.

Invariants:

- Every event has `(session_id, seq)` with `seq` monotonic per session, in the
  envelope defined by [`omt-events`](02-crate-map.md#omt-events). Terminal byte
  frames share that sequence space
  ([07 §5.1](07-remote-protocol.md#51-sequence-spaces)); workspace-scoped events
  carry `workspace` instead of `session` and use the workspace's own space
  ([15 §4.6](15-workspace-explorer.md#46-file-watching)).
- A client may resume with `since_seq`; the instance replays from its log or
  responds `Resync` ([07 §5.2](07-remote-protocol.md#52-replay-window)) with a
  snapshot.
- The TUI subscribes to the same bus with the same envelope. There is no
  internal-only event.
- Events are *derived from* state changes, never the mechanism of them — a
  handler mutates state and the state layer emits; handlers do not publish
  events by hand.

## 5. The parity contract

For each capability, four artifacts must exist. The parity test enumerates the
registry and asserts all four; a missing one fails CI with the capability name.

| # | Artifact | Where it lives | How it is checked |
|---|---|---|---|
| 1 | API route + schema | generated by `omt-catalog` codegen | generated file is committed; CI regenerates and diffs |
| 2 | TUI binding | `omt-tui`'s action table | test asserts every non-`Admin` capability has an action, and every action names a real capability |
| 3 | Web handler | `web/src/capabilities/registry.ts` | generated TS declares the union; a type-level exhaustiveness check fails the web build if a case is unhandled |
| 4 | Docs entry | generated `docs/reference/capabilities.md` | regenerated and diffed in CI |
| 5 | Palette entry | generated from `title`/`aliases`/`hidden` (§2) | test asserts every non-`hidden` capability has a `title`; `hidden` ones are still checked by artifacts 1–4 |

Two mechanical checks worth naming, because both have already caught a real
contradiction in these documents:

- **Artifact 2 in both directions.** *Every action names a real capability* is
  the half that catches drift: a keybinding in [16 §11](16-input-and-keymap.md)
  that names an action no capability declares fails on the first CI run.
- **`refine_effects` is sound** (§2.1) and **every `Command` declares an
  `intent`** (§2.2). Both are enumerated over the registry by the same test.

Escape hatch, deliberately narrow: a capability may declare
`parity = Parity::Exempt { reason }`. Exemptions are enumerated in the generated
docs so they are visible, and the test asserts the list matches an explicit
allow-list in the repo — you cannot add one silently. Legitimate examples:
`instance.shutdown` (admin, no phone affordance needed), local-only debug dumps.

**The exhaustiveness trick for the web client** is what makes surface #3 real
rather than aspirational: codegen emits

```ts
export type CapabilityName = "session.send_text" | "session.list" | ...;
export const handlers: { [K in CapabilityName]: Handler<K> } = { ... };
```

A new capability breaks the web build until someone writes the handler. That is
the desired failure mode.

## 6. Capability groups (initial surface)

This is the shape, not the exhaustive list; each change adds its own.

| Group | Representative capabilities |
|---|---|
| `instance` | `info`, `health`, `catalog`, `shutdown`, `peers.list`, `peers.add` |
| `workspace` | `list`, `open`, `close`, `rename`, `vcs.*`, `files.*`, `worktree.*`, `history` |
| `session` | `list`, `create`, `close`, `attach`, `detach`, `resize`, `send_text`, `send_keys`, `write_bytes`, `scrollback.get`, `search`, `blocks.list`, `blocks.get`, `blocks.rerun`, `writer.acquire`, `writer.release` |
| `pane` | `split`, `close`, `focus`, `navigate`, `move`, `zoom`, `stack.*`, `float` |
| `layout` | `get`, `set`, `preset`, `balance`, `views.*`, `promote`, `save`, `apply_saved`, `import_tmux` |
| `agent` | `state`, `explain`, `bind`, `unbind`, `prompt`, `interrupt`, `queue.list`, `queue.enqueue`, `queue.remove`, `commands.list`, `commands.run` |
| `interaction` | `list`, `get`, `resolve`, `cancel` |
| `media` | `clipboard.read`, `clipboard.write`, `image.upload`, `image.paste`, `file.push`, `file.pull` |
| `stt` | `session.start`, `session.stop`, `providers.list` |
| `media` (cont.) | `blob.begin`, `blob.commit`, `transfer.progress` |
| `config` | `get`, `schema`, `set`, `unset`, `validate`, `reload`, `sources`, `default`, `export`, `import`, `pending`, `project.trust` |
| `theme` / `keys` | `theme.list/get/import`, `keys.list/conflicts` |
| `workflow` / `launch` | `list`, `get`, `run`, `save`, `delete` |
| `plugin` | `list`, `info`, `install`, `uninstall`, `enable`, `disable`, `upgrade`, `health`, `logs`, `call`, `permissions.get/set` |
| `notification` | `push.subscribe`, `push.unsubscribe`, `test` |
| `presence` | `list` |
| `audit` | `query` (Admin) |
| `events` | `subscribe`, `resume` |

The full consolidated list, with kinds, roles and effects, is the hand-written
[`docs/reference/capabilities-draft.md`](../reference/capabilities-draft.md),
which §5 artifact #4 replaces with generated output.

`layout.*` is a group of its own rather than a `pane.*` sub-group, because these
operations act on a **view** and not on a pane
([17 §9.3](17-panes-and-layout.md#93-how-this-surface-sits-in-the-catalog)).

Two of these deserve emphasis:

- **`interaction.resolve`** is the flagship path. It is how a phone answers an
  `AskUserQuestion` card. It is resolvable exactly once and idempotent by
  `(interaction_id, identity_or_device, intent_id)` — **not** by the actor, which
  changes on every reconnect and would make a device's own retry read as a
  stranger overriding it
  ([D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)
  consequence 6). A retry under the same key returns the original outcome; a
  different identity gets `conflict` with a discriminating `detail.state`. Full semantics in
  [12 §4](12-collaboration.md#4-interaction-ownership). Its result is broadcast
  to every surface — including the TUI, which then shows the card as answered by
  whoever answered it.
- **`agent.commands.list` / `agent.commands.run`** give the web client native
  slash-command semantics: the catalog exposes the agent's own resolved command
  list (from `system/init`, ACP `available_commands_update`, or disk
  enumeration), so a phone gets the same completion popup the TUI has.

## 7. Versioning

- Capability names are stable. Renames ship as an alias for two minor versions,
  with the old name marked deprecated in the generated docs.
- Input types may gain optional fields; required fields may not be added.
  Output types may gain fields; clients ignore unknown ones.
- The protocol carries a catalog version; on handshake, a client learns which
  capabilities the instance actually has. This is how a phone talks to several
  instances on different versions at once — it renders the intersection and
  greys out the rest, rather than failing.

## 8. Why this is worth the machinery

Without a catalog, parity is a promise that decays: someone adds a keybinding
that calls a session method directly, and six months later the web client is a
second-class citizen and mobile users are told to "use the TUI for that".

With the catalog, the cheapest way to add a feature is also the correct one —
declare it, implement one handler, and the API, the CLI, the schema, the docs
and the web type-checking come along. Doing it the wrong way is *more* work,
which is the only enforcement that survives contact with a deadline.
