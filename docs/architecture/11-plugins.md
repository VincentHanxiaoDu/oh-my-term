# Plugin System

Design of the `omt-plugin-host` crate (L5, a leaf — see
[02 — Crate map](02-crate-map.md)) and the plugin model it hosts.

This is the concrete realization of **P2 — Pluggable: extension without
modification** ([01 — Principles](01-principles.md)). The test of the design is
blunt: *a third party must be able to add an agent adapter, a notification sink,
a theme or an STT provider without forking omt and without omt shipping a
release.*

Related: [03 — Capability catalog](03-capability-catalog.md) (plugins consume and
contribute capabilities), [10 — Configuration](10-configuration.md) (plugins
contribute config sub-trees), [13 — Security](13-security.md) (the actor model
plugins inherit), [06 — Agent layer](06-agent-layer.md) (adapters and event
sources).

---

## 1. Two flavours, and which to build first

| Flavour | What it is | Who uses it |
|---|---|---|
| **In-process Rust** | a crate implementing `AgentAdapter` / `EventSource` / `SttProvider` / … registered at startup | first-party adapters shipped in the binary |
| **Out-of-process** | a subprocess speaking the omt plugin protocol over stdio (NDJSON), launched from a manifest | third parties, v1 |
| **WASM component** | a `wasm32-wasip2` component implementing a WIT world, run in Wasmtime | third parties, v2 |

### Decision

**Build out-of-process subprocess plugins first. Ship the WASM component model
second, behind the same manifest and the same protocol.**

Rationale:

1. **Language freedom is the point.** The people who will write an adapter for a
   new agent CLI write TypeScript and Python. A subprocess plugin is `bun run
   index.ts` or `python main.py`; a WASM plugin is, today, realistically Rust
   only. another tool reached the same conclusion — every extension point is "run this
   argv" ([research/another tool.md §7](../research/another tool.md)) — and it works.
2. **Crash isolation is free.** A subprocess that segfaults, hangs, or leaks is
   killed by the host; a panicking in-process plugin takes the daemon with it.
   That matters more than performance here: plugins sit on the *event* path, not
   the PTY byte path.
3. **The OS already has a sandbox.** Process-level limits (rlimits, `seccomp` on
   Linux, `sandbox_init`/App Sandbox on macOS, job objects on Windows) exist and
   are auditable. Wasmtime's sandbox is better on capability granularity but
   worse on ecosystem.
4. **WASM's advantage is deployment, not security** — one artifact, no runtime
   dependency. That is real, and it is why v2 exists. But it is not worth
   blocking v1 on a toolchain most contributors do not have.
5. **In-process stays first-party only.** A dynamically-linked `cdylib` plugin
   ABI is explicitly rejected: Rust has no stable ABI, and a version skew
   produces silent UB rather than an error.

The protocol is designed so that the WASM host is a *transport swap*: the same
manifest, the same message schema, the same permission model. A plugin's
`entrypoint` becomes `{ kind = "wasm", module = "plugin.wasm" }` instead of
`{ kind = "process", command = [...] }`, and nothing else changes.

Performance envelope: the host budgets **one process per plugin**, kept warm, with
NDJSON over stdio. Event fan-out to plugins is asynchronous and lossy-by-policy
(a slow plugin gets a bounded queue and then dropped events with a counter), so a
plugin can never back-pressure the terminal.

---

## 2. Manifest

One directory per plugin, containing `omt-plugin.toml`.

```toml
#:schema https://omt.dev/schemas/plugin.schema.json

id      = "dev.example.aider-adapter"      # reverse-DNS, immutable, the registry key
name    = "Aider adapter"
version = "0.3.1"                          # semver
description = "Observes aider sessions and exposes its confirmations as omt interactions."
authors = ["Ada Lovelace <ada@example.dev>"]
license = "Apache-2.0"
homepage = "https://github.com/example/omt-aider-adapter"

# REQUIRED. The plugin is refused if the host is outside this range.
# (another tool's `min_another tool_version` lesson, generalized to a range.)
omt_api = ">=1.0, <2.0"

platforms = ["linux", "macos"]             # omitted = all

[entrypoint]
kind    = "process"                        # process | wasm
command = ["bun", "run", "dist/index.js"]  # argv, resolved relative to the plugin root
                                           # PATH lookup only for the argv[0] binary
restart = "on-crash"                       # never | on-crash | always
max_restarts = 5                           # per hour, then the plugin is quarantined

[build]                                    # optional, run on install/upgrade
command = ["bun", "install", "--frozen-lockfile"]
timeout = "120s"

# ── What the plugin needs from omt ────────────────────────────────────────
[permissions]
capabilities = [                           # capabilities it may call
  "session.list",
  "agent.state",
  "interaction.*",
]
events = [                                 # events it may subscribe to
  "agent.*",
  "session.created",
  "session.closed",
]
role = "operator"                          # ceiling; see §4
fs   = { read = ["$PLUGIN_ROOT", "~/.aider.conf.yml"], write = ["$PLUGIN_STATE"] }
net  = { allow = [] }                      # empty = no network
exec = { allow = [] }                      # subprocesses it may spawn
env  = ["HOME", "PATH", "AIDER_MODEL"]     # everything else is stripped

[limits]
memory   = "128MiB"
cpu      = "10%"                           # sustained, averaged over 60s
timeout  = "10s"                           # per request
queue    = 256                             # pending events before dropping

# ── What the plugin contributes ───────────────────────────────────────────
[[contributes.agent_adapters]]
id = "aider"
detect = { process_names = ["aider"], argv_contains = [] }
resume_argv = ["aider", "--restore-chat-history"]
state_labels = { working = "thinking", blocked = "confirm" }

[[contributes.event_sources]]
id = "aider-history"
agent = "aider"
tier = 3                                   # Transcript; may emit structured content
kind = "file-tail"
path = "{workspace}/.aider.chat.history.md"

[[contributes.capabilities]]
name  = "aider.set_model"
group = "aider"
verb  = "set-model"
kind  = "command"
role  = "operator"
input_schema  = "schemas/set_model.input.json"
output_schema = "schemas/set_model.output.json"
description = "Switch the model aider is using in a session."

[[contributes.ui]]
kind = "session_action"                    # session_action | dashboard_panel | settings_page
id   = "aider-model"
title = "Switch model"
icon = "sparkles"
capability = "aider.set_model"
surfaces = ["tui", "web"]

[[contributes.themes]]
path = "themes/aider-dark.toml"

[[contributes.keybindings]]
trigger = "ctrl-b m"
capability = "aider.set_model"
when = "agent_bound && agent_id == 'aider'"

[config_schema]                            # JSON Schema for `[plugins."<id>"]` in config.toml
path = "schemas/config.schema.json"
```

Rules:

- `id`, `version`, `omt_api`, `entrypoint` are required; everything else has a
  default. A manifest missing `omt_api` is refused outright, with the error
  naming the field — this is deliberate, and it is why another tool's equivalent field
  is required too.
- Unknown top-level keys are **warnings** (forward compatibility for plugins
  targeting a newer omt), unknown keys inside a known table are **errors**
  (typos). Same diagnostic machinery as [10 §5](10-configuration.md), codes
  `OMT-P###`.
- A manifest that fails to parse does not remove the plugin from the registry; it
  keeps the entry with `status = "invalid"` and its diagnostics, so the user sees
  *why* their plugin vanished rather than it silently vanishing.

---

## 3. The plugin-facing API

### 3.1 Transport and framing

NDJSON over the plugin's stdin/stdout — one JSON object per line, UTF-8, no
embedded newlines. stderr is captured verbatim into the plugin's log ring buffer
and is never parsed. This is the same framing as the omt remote protocol
([07](07-remote-protocol.md)) minus the auth handshake, so the message schemas
are generated from the same `omt-proto` types.

Four message kinds, each with `id` for correlation:

```jsonc
// host → plugin
{ "t": "req",  "id": 7, "method": "init",            "params": { ... } }
{ "t": "event","id": 0, "topic": "agent.state_changed", "payload": { ... } }
// plugin → host
{ "t": "res",  "id": 7, "ok": true,  "result": { ... } }
{ "t": "res",  "id": 7, "ok": false, "error": { "code": "invalid_input", "message": "..." } }
{ "t": "call", "id": 3, "capability": "interaction.resolve", "input": { ... } }
{ "t": "log",  "level": "info", "message": "...", "fields": { ... } }
```

### 3.2 Handshake

The host sends `init` first and the plugin must answer within
`limits.timeout` or be killed:

```jsonc
// host → plugin
{ "t": "req", "id": 1, "method": "init", "params": {
    "omt_version": "1.2.0",
    "api_version": "1.0",
    "instance_id": "01J...A",
    "plugin_id": "dev.example.aider-adapter",
    "granted": {
      "capabilities": ["session.list", "agent.state", "interaction.get", "interaction.resolve"],
      "events": ["agent.*", "session.created", "session.closed"],
      "role": "operator"
    },
    "config": { "model": "gpt-4o" },        // resolved [plugins."<id>"] section
    "paths": { "root": "...", "state": "...", "config": "...", "cache": "..." }
} }

// plugin → host
{ "t": "res", "id": 1, "ok": true, "result": {
    "api_version": "1.0",
    "capabilities_implemented": ["aider.set_model"],
    "ready": true
} }
```

`granted` is the *intersection* of what the manifest requested and what the user
consented to (§4). A plugin must read it and degrade gracefully — the host
rejects calls outside it, so ignoring `granted` produces errors, not privilege.

### 3.3 What a plugin may call

Any capability in `granted.capabilities`, using exactly the wire types from
[03](03-capability-catalog.md). There is no separate "plugin API" — the plugin
API *is* the capability catalog, filtered. This is the single most important
design decision in this document: it means a plugin can never do something a
remote client cannot, the audit log is uniform, and adding a capability
automatically makes it available (subject to consent) to plugins.

Host-only methods outside the catalog are limited to four, all plugin-lifecycle:
`init`, `shutdown`, `health`, and `config_changed`.

### 3.4 What a plugin may subscribe to

Any event topic in `granted.events`, from the same `omt-events` bus, with the
same envelope (`instance`, `session`, `seq`, `ts`, `source`, `payload`).
Wildcards are allowed at a segment boundary (`agent.*`, not `agent.st*`).
Delivery is at-most-once with a bounded queue; on overflow the host emits
`plugin.events_dropped { plugin_id, count }` and the plugin can request a resync
via `events.resume`.

### 3.5 What a plugin may contribute

| Contribution | Mechanism | Notes |
|---|---|---|
| **New capabilities** | `[[contributes.capabilities]]` + JSON Schemas | registered in the catalog under the plugin's group; calls are proxied to the plugin as `req`; they appear in the CLI, the HTTP routes, the TS client and the docs like any other, tagged `provider = "plugin:<id>"` |
| **Agent adapters** | `[[contributes.agent_adapters]]` | a data-declared adapter: detection fingerprints, resume argv, state labels, hook installer spec. No Rust required for the common case |
| **Event sources** | `[[contributes.event_sources]]` | `file-tail`, `process-poll`, `http-sse`, or `push` (the plugin emits events itself). Tier is the `Tier` enum from [06 §3](06-agent-layer.md#3-source-model) — higher is more authoritative — and is declared but **clamped**: a plugin may not claim tier ≥ 3 (`Transcript`/`Hook`/`Protocol`, the tiers permitted to emit structured content) unless its source kind is a real structured one (P4). A `process-poll` source claiming tier 5 is clamped to 1 |
| **UI affordances** | `[[contributes.ui]]` | `session_action` (a button/menu entry bound to a capability), `dashboard_panel` (a list rendered from a query capability), `settings_page` (generated from `config_schema`). Declarative only — no plugin-supplied rendering code, no HTML, no JS injected into the web client |
| **STT providers** | `[[contributes.stt_providers]]` | audio frames streamed to the plugin, transcripts back |
| **Themes** | `[[contributes.themes]]` | plain theme files ([10 §8.1](10-configuration.md)) merged into the theme list |
| **Keybindings** | `[[contributes.keybindings]]` | defaults only; the user's `keybindings.toml` always wins, and conflicts are reported as `OMT-C4xx` warnings naming the plugin |
| **Notification sinks** | `[[contributes.notification_sinks]]` | a sink id usable in `notifications.rules[].sinks` as `plugin:<id>` |

**A plugin can be an entire client, and that is the point.** The table above
lists what a plugin *contributes to omt*; it does not bound what a plugin can
*be*. Because §3.3 makes the plugin API the capability catalog itself, a plugin
that subscribes to `agent.*` and `interaction.*` and calls `session.list`,
`session.send_text`, `agent.prompt` and `interaction.resolve` **is a channel** —
a surface through which a human drives sessions — without contributing anything
from the table at all.

A Telegram bot is the canonical example: it speaks Telegram's API on one side
and the ordinary plugin API on the other, and it needs no new omt concept. It
inherits the whole model for free — exactly-once interaction resolution, the
intent classes that make a retry safe, the writer token, and an audit trail that
records the action as `plugin:<id>` acting for a named actor. The same shape
covers a Slack app, an IRC bridge, an email responder, a Shortcuts action, or a
physical button on a desk.

Two boundaries make this safe rather than alarming:

- A plugin **cannot exceed a remote client**, because it is calling the same
  catalog through the same dispatch with the same role check. There is no
  privileged back door to be careless with.
- A plugin **cannot contribute a `Transport` or an `AuthBackend`** in v1 (§9).
  A channel does not need to: it is a *consumer* of the protocol, not a new way
  to speak it. The distinction is that it adds a client, not a socket.

**UI contributions are declarative on purpose.** Letting a plugin ship code into
the web client would mean shipping arbitrary JS into a page that holds session
credentials. A `session_action` is a title, an icon name, a capability and an
argument form generated from that capability's input schema — enough for real
features, not enough to exfiltrate a token.

---

## 4. Permissions

### 4.1 Declared scopes and consent

The manifest declares; the user consents; the host enforces. Install shows a
consent screen built from the manifest — the same one in the TUI and the web UI,
generated from the manifest schema:

```
Install dev.example.aider-adapter 0.3.1?

  Runs                 bun run dist/index.js          (subprocess, restarts on crash)
  Calls capabilities   session.list, agent.state, interaction.get, interaction.resolve
  Receives events      agent.*, session.created, session.closed
  Acts with role       operator            ← can answer interactions and send text
  Reads files          <plugin root>, ~/.aider.conf.yml
  Writes files         <plugin state dir>
  Network              none
  Spawns processes     none
  Limits               128 MiB, 10% CPU, 10s per request

  [Install]  [Install with reduced permissions…]  [Cancel]
```

"Reduced permissions" lets the user deselect individual capabilities and events;
the plugin receives the reduced set in `granted` and must cope. Consent is stored
per `(plugin_id, permission-set hash)`; an upgrade that widens permissions
re-prompts, an upgrade that does not is silent.

### 4.2 The escalation rule

> **A plugin may never act with more authority than the actor that triggered it.**

Mechanically:

- Every plugin-originated capability call carries a `CallContext`
  ([03 §3](03-capability-catalog.md)) whose `Actor` is
  `Actor::Plugin { id, on_behalf_of: Option<Actor> }` and whose `Role` is
  **`min(plugin.granted.role, triggering_actor.role)`**.
- When a plugin acts because of an event (no triggering actor), the role is
  `min(plugin.granted.role, Role::Operator)` and the effective actor is
  `Actor::Plugin { on_behalf_of: None }`. A plugin can never reach `Admin`
  through an event; `Admin` is only reachable when an `Admin` user explicitly
  invokes a plugin capability.
- A plugin capability declared `role = "operator"` and invoked by a `Viewer`
  is rejected at dispatch, *before* the plugin is contacted — so a plugin cannot
  be used as a confused deputy by a read-only invite link.
- The audit log records both actors, so `interaction.resolve` performed by a
  plugin on behalf of a phone shows as such in every surface, including the TUI
  card ("answered by aider-adapter on behalf of ada@phone").

This rule is enforced in `omt-daemon`'s dispatch, not in `omt-plugin-host`, so a
bug in the host cannot bypass it. A test constructs a plugin call chain with a
`Viewer` trigger and asserts every `Operator`+ capability is refused.

### 4.3 Sandboxing

| Platform | Mechanism |
|---|---|
| Linux | new user/PID/mount namespace where available, `seccomp` filter (deny `ptrace`, `mount`, raw sockets), `rlimits`, cgroup v2 for memory/CPU when running under systemd |
| macOS | `sandbox-exec` profile generated from the manifest's fs/net/exec scopes; `setrlimit` |
| Windows | Job object with memory/CPU caps, low-integrity token |

Sandboxing is **best-effort and declared**: `omt plugin info <id>` prints which
mechanisms are active on this host, and the consent screen says "sandbox:
namespaces + seccomp" or "sandbox: rlimits only" so the user knows what they are
trusting. We do not pretend a plugin is contained when it is not. Filesystem
scopes are additionally enforced by the host for any *path-bearing capability
argument* the plugin passes, which covers the case where the OS sandbox is weak.

Network is denied by default at the sandbox level. `net.allow` is a list of
host patterns; on platforms where per-process network policy is unavailable, a
plugin requesting network gets a prominent warning at consent time rather than a
silent grant.

### 4.4 Resource limits

Enforced by the host regardless of sandbox availability: request timeout
(default 10 s, then the request fails with `timeout` and the plugin is marked
degraded), event queue depth, restart budget (`max_restarts` per hour, then
quarantine), memory and CPU sampling with a kill at 2× the declared limit
sustained for 30 s. All limits are visible in `plugin.health`.

---

## 5. Lifecycle

```
discover → validate manifest → consent → install (build) → enable → running
                                                              │
                                      ┌───────────────────────┼──────────────┐
                                      ▼                       ▼              ▼
                                   healthy                 degraded      quarantined
                              (health ok, no drops)   (timeouts/drops)  (crash budget)
```

| Stage | Behaviour |
|---|---|
| **Discovery** | scan `plugins.search_paths` ([10 §7.10](10-configuration.md)) for directories containing `omt-plugin.toml`; plus explicit `--path` installs |
| **Install** | `omt plugin install <source>` where source is a registry id, a git URL, a GitHub `owner/repo`, or a local path. Copies/clones into `~/.config/omt/plugins/<id>/`, verifies the manifest, runs `[build]` with the same sandbox as the runtime, shows consent, records the resolved version and a content hash |
| **Enable/disable** | membership in `plugins.enabled`; disabling stops the process and unregisters its contributions live (no restart). Contributions are removed transactionally: a capability provided by a disabled plugin returns `unsupported`, and the web client's catalog diff arrives as an event |
| **Upgrade** | `omt plugin upgrade <id>` resolves a new version, checks `omt_api` against the running host, re-prompts on widened permissions, then does a stop → swap → start. Plugin state dirs are never touched; migrations are the plugin's own responsibility (the state dir is path discovery only, as in another tool) |
| **Health** | the host sends `health` every 30 s; a plugin answers with `{ ok, detail }`. Three missed or failed health checks ⇒ degraded; degraded plugins keep running but their contributions are flagged in the UI |
| **Crash isolation** | a plugin process exit is never fatal to omt. Pending requests fail with `plugin_unavailable`. Restart follows `entrypoint.restart` and the restart budget; on quarantine the plugin is disabled with a persistent, dismissible notification carrying the last 200 lines of its stderr |
| **Shutdown** | `shutdown` request, then `SIGTERM` after 5 s, then `SIGKILL` after 10 s |

Capabilities (per [03 §6](03-capability-catalog.md)): `plugin.list`,
`plugin.info`, `plugin.install`, `plugin.uninstall`, `plugin.enable`,
`plugin.disable`, `plugin.upgrade`, `plugin.health`, `plugin.logs`,
`plugin.call`, `plugin.permissions.get`, `plugin.permissions.set`.

---

## 6. Distribution

Three sources, in decreasing trust:

1. **Registry index.** A static, signed JSON index served over HTTPS
   (`https://registry.omt.dev/index.json`), listing `id`, versions, source URL,
   a manifest digest, and a maintainer key fingerprint. The index is a *pointer
   list*, not a package host — artifacts live in the plugin's own repository.
   Mirrorable and self-hostable via `plugins.registries = [...]`.
2. **Git / GitHub topic discovery.** `omt plugin search <term>` queries the
   GitHub API for `topic:omt-plugin`, as another tool's marketplace worker does
   ([research/another tool.md §7](../research/another tool.md)). Results are clearly labelled
   "unreviewed" and installing one always shows the full consent screen. This
   costs nothing and bootstraps an ecosystem before a registry has one.
3. **Local path.** `omt plugin install --path ./my-plugin` with
   `plugins.allow_unsigned = true` (the default) — the development loop.
   `omt plugin dev ./my-plugin` runs it with hot restart on file change.

Integrity: the manifest digest recorded at install is re-checked on every start;
a mismatch disables the plugin and reports it, so an edited plugin directory
cannot silently change behaviour. Signing (minisign/sigstore over the manifest
digest) is supported and required when `plugins.allow_unsigned = false`.

We do **not** fetch and execute code from an unauthenticated URL at runtime; the
only network fetch is install-time, over TLS, against a pinned digest. (another tool's
`curl` shell-out for remote manifests is exactly the pattern we avoid.)

---

## 7. Versioning and stability

- **`api_version`** is a separate, slowly-moving semver from the omt version.
  `1.x` is stable: no message field is removed, no required field is added, no
  capability is removed without a two-minor-version deprecation alias — the same
  rules as [03 §7](03-capability-catalog.md), because it is the same catalog.
- A plugin declares an `omt_api` **range**. The host refuses to load a plugin
  whose range excludes the host's `api_version`, with a message naming both.
- **Capability drift is survivable.** `granted.capabilities` at `init` is the
  authoritative list for that run; a plugin that hardcodes a capability name
  which no longer exists gets `unsupported` at call time, not a crash.
- **Contributed capability names** are namespaced by the plugin's group and must
  not collide with a core group; collisions are refused at registration with a
  diagnostic. If two plugins contribute the same name, the first enabled wins and
  the second is marked `conflicted`.
- **Deprecation policy**: an `api_version` minor bump may add; a major bump may
  remove, and the host then supports the previous major for one release cycle by
  running old plugins through a shim host. Whether that shim is worth its cost is
  an open question (§9).

---

## 8. Worked example: an ntfy notification sink

A complete, real plugin — chosen because it is the smallest thing that exercises
consent, config, events, network, and a contribution.

**Goal.** Push a notification to [ntfy.sh](https://ntfy.sh) when any agent needs
attention, so a phone that is not running the web client still gets pinged.

### 8.1 Layout

```
omt-ntfy/
├── omt-plugin.toml
├── schemas/config.schema.json
├── package.json
└── src/index.ts
```

### 8.2 Manifest

```toml
id      = "dev.example.ntfy"
name    = "ntfy notifications"
version = "1.0.0"
description = "Sends omt notifications to an ntfy topic."
license = "Apache-2.0"
omt_api = ">=1.0, <2.0"

[entrypoint]
kind    = "process"
command = ["bun", "run", "src/index.ts"]
restart = "on-crash"
max_restarts = 5

[build]
command = ["bun", "install", "--frozen-lockfile"]

[permissions]
capabilities = ["session.get", "workspace.get"]   # to enrich the message
events       = []                                  # none: it is a sink, not a subscriber
role         = "viewer"                            # it never mutates anything
net          = { allow = ["ntfy.sh", "$config.server"] }
fs           = { read = ["$PLUGIN_ROOT"], write = [] }

[limits]
memory  = "64MiB"
timeout = "5s"

[[contributes.notification_sinks]]
id    = "ntfy"
title = "ntfy"

[config_schema]
path = "schemas/config.schema.json"
```

Note `role = "viewer"`: this plugin cannot answer an interaction or type into a
PTY even if it tried, and the consent screen says so in one line. Note also
`events = []` — a notification sink is *called*, not subscribed; the routing
lives in `notifications.rules`, which the user already controls
([10 §7.7](10-configuration.md)).

### 8.3 User configuration

```toml
# ~/.config/omt/config.toml
[plugins]
enabled = ["dev.example.ntfy"]

[plugins."dev.example.ntfy"]
server = "ntfy.sh"
topic  = "ada-omt"
token  = { secret = "plugins.dev.example.ntfy.token" }

[[notifications.rules]]
when  = "agent.blocked"
sinks = ["ntfy"]
title = "{agent} is blocked in {workspace}"
priority = "high"
```

The `[plugins."dev.example.ntfy"]` table is validated against the plugin's own
`config.schema.json` by omt's validator, so a typo in `topik` produces the same
caret diagnostic as a core setting — `OMT-C101` with a "did you mean" for the
plugin's key set. The settings UI page for this plugin is generated from that
schema; the plugin ships no UI code.

### 8.4 The plugin

```ts
// src/index.ts — reads NDJSON requests on stdin, writes NDJSON responses on stdout.
type Req = { t: "req"; id: number; method: string; params: any };

let cfg: { server: string; topic: string; token?: string } | null = null;
let granted: { capabilities: string[]; role: string } | null = null;

function send(o: unknown) { process.stdout.write(JSON.stringify(o) + "\n"); }
function log(level: string, message: string) { send({ t: "log", level, message }); }

async function handle(req: Req) {
  switch (req.method) {
    case "init":
      cfg = req.params.config;
      granted = req.params.granted;
      log("info", `ntfy sink ready for topic ${cfg!.topic}`);
      return { api_version: "1.0", capabilities_implemented: [], ready: true };

    case "config_changed":
      cfg = req.params.config;
      return {};

    // Invoked by the notification router for every rule whose sinks include "ntfy".
    case "notification.deliver": {
      const { title, body, priority, session_id, url } = req.params;
      const res = await fetch(`https://${cfg!.server}/${cfg!.topic}`, {
        method: "POST",
        headers: {
          ...(cfg!.token ? { Authorization: `Bearer ${cfg!.token}` } : {}),
          Title: title,
          Priority: priority === "high" ? "high" : "default",
          // Tapping the notification deep-links into the web client's session view.
          Click: url ?? "",
          Tags: "robot",
        },
        body,
      });
      if (!res.ok) throw new Error(`ntfy responded ${res.status}`);
      return { delivered: true, session_id };
    }

    case "health":   return { ok: true, detail: `topic=${cfg?.topic}` };
    case "shutdown": setTimeout(() => process.exit(0), 0); return {};
    default:         throw new Error(`unknown method ${req.method}`);
  }
}

for await (const line of readLines(process.stdin)) {
  if (!line.trim()) continue;
  const msg = JSON.parse(line) as Req;
  if (msg.t !== "req") continue;
  try {
    send({ t: "res", id: msg.id, ok: true, result: await handle(msg) });
  } catch (e) {
    send({ t: "res", id: msg.id, ok: false,
           error: { code: "internal", message: String(e) } });
  }
}
```

`cfg.token` arrives already resolved from `secrets.toml` — the plugin never sees
the secret *name*, only the value, and only because it declared the config schema
field as `format: "omt-secret"`. It is redacted in every host-side log.

### 8.5 End to end

```
$ omt plugin install github:example/omt-ntfy
Fetching manifest… dev.example.ntfy 1.0.0 (unreviewed, from GitHub topic search)

  Runs                 bun run src/index.ts
  Calls capabilities   session.get, workspace.get
  Acts with role       viewer              ← cannot type, cannot answer prompts
  Network              ntfy.sh
  Sandbox              namespaces + seccomp (linux)

Install? [y/N] y
Running build: bun install --frozen-lockfile … ok
Installed. Enable with: omt plugin enable dev.example.ntfy

$ omt config set plugins.dev.example.ntfy.topic ada-omt
$ omt plugin enable dev.example.ntfy
$ omt plugin health dev.example.ntfy
dev.example.ntfy  running  healthy  pid 48123  rss 31MiB  0 drops  topic=ada-omt
```

Then, when Claude Code parks an `AskUserQuestion` in the `api` workspace: the
agent layer opens an `Interaction`, the notification router matches the
`agent.blocked` rule, calls `notification.deliver` on the ntfy plugin,
and the phone gets a push whose tap target is the web client's card for that
interaction. The plugin itself could not have answered that question — it is a
`Viewer`. The human answers it, `interaction.resolve` runs as *them*, and the
hook decision goes back to Claude Code through the channel it came from (P4).

### 8.6 The second example, sketched

An **agent adapter for a new CLI** needs no code at all in the common case: the
`[[contributes.agent_adapters]]` and `[[contributes.event_sources]]` tables in
§2 are sufficient for detection, resume, state labels and transcript tailing. A
plugin only ships a process when it needs to *translate* — e.g. converting
aider's confirmation prompts into structured `Interaction`s, which requires
parsing that tool's own history format. That is the intended gradient: data for
the common case, code when the format is genuinely bespoke.

---

## 9. Open questions

- **OPEN QUESTION — WASM timing.** Is the component model (`wasm32-wasip2` +
  WIT) stable enough in Wasmtime for a v2 within the first year, and is the
  guest-language story outside Rust (JS via ComponentizeJS, Python via
  componentize-py) good enough that it actually widens the contributor pool
  rather than narrowing it?
- **OPEN QUESTION — should plugins be able to contribute a `Transport` or an
  `AuthBackend`?** They are P2 extension points, but a third-party auth backend
  is a very sharp edge. Proposed: no for v1; revisit with a signed-plugins-only
  requirement.
- **OPEN QUESTION — UI richness ceiling.** Declarative `session_action` /
  `dashboard_panel` / `settings_page` covers a lot, but not a plugin that wants a
  custom visualization. Is a sandboxed iframe with a postMessage bridge to a
  *restricted* capability set worth the risk later, or is "declarative forever"
  the right permanent answer?
- **OPEN QUESTION — the major-version shim.** Running `api_version` 1.x plugins
  under a 2.x host requires a translation layer whose cost we cannot estimate
  before we know what 2.0 changes. The alternative is a hard cutover with a
  migration guide. Decide when 2.0 is on the horizon, not now.
- **OPEN QUESTION — per-workspace plugin enablement.** `plugins.enabled` is
  global. A project config cannot enable a plugin (correctly — it would be code
  execution from a clone), but should a *user* be able to enable a plugin only
  for certain workspaces?
- **OPEN QUESTION — event tier clamping for `push` sources.** A plugin declaring
  a `push` event source asserts its own tier. We clamp claims of tier ≥ 3 to
  sources we can verify, but a plugin that genuinely speaks a native agent
  protocol *should* be allowed tier 5. What evidence does the host accept?
