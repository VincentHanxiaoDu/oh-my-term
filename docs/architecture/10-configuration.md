# Configuration

Design of the `omt-config` crate (L3, see [02 — Crate map](02-crate-map.md)).

This document is the concrete realization of **P7 — Configuration is data, and
errors are precise** ([01 — Principles](01-principles.md)). Everything here is
derived from one rule: *the Rust type is the schema, and every other artifact —
the file grammar, the JSON Schema, the TUI editor, the web settings UI, the
reference documentation, the validator — is generated from it.*

Related: [03 — Capability catalog](03-capability-catalog.md) (config is reached
through capabilities like everything else), [11 — Plugins](11-plugins.md)
(plugins contribute config sub-trees), [13 — Security](13-security.md) (secrets,
0600, redaction), [07 — Remote protocol](07-remote-protocol.md) (how a web
client edits a remote instance's config).

---

## 1. Format and file layout

### 1.1 Decision: TOML for the main config

**Decision.** The main configuration file is **TOML** (`config.toml`). Themes are
**TOML** with a documented YAML importer. Keybindings are **TOML**. Workflows and
launch configurations are **YAML**, because their upstream corpora are YAML and
importing them verbatim is a feature.

Rationale, against the alternatives:

| Format | Why not (or why) |
|---|---|
| **TOML** ✅ | Unambiguous scalar typing; comments survive round-trips with `toml_edit`; byte spans available for every key and value (`toml_edit`/`toml::Spanned`), which is what makes §5's caret diagnostics possible; flat section headers map cleanly onto a settings UI's page/section structure; already the format most Rust CLI users expect. another terminal's `settings.toml` and another tool's single TOML file both landed here independently. |
| YAML | Indentation-significant, so machine edits from a settings UI are risky; the Norway problem and other implicit-typing traps; `serde_yaml` is unmaintained; span quality is worse. Still used where the ecosystem already is (themes from another terminal, workflows). |
| JSON | No comments. Disqualifying for a hand-edited file. JSON5/JSONC fixes that but has no canonical Rust ecosystem. |
| KDL | Genuinely nice for node-shaped data (Zellij uses it) but our config is a settings tree, not a node tree; smaller ecosystem, unfamiliar to most users, and no schema-generation story. |

Two consequences of choosing TOML we accept deliberately:

- **Deep nesting is ugly.** We cap nesting at three levels (`section.subsection.key`)
  and enforce it in the schema test. Anything deeper is a modelling error.
- **Arrays of tables are verbose.** Accepted: `[[agents.claude_code.env]]`-style
  repetition is rare because most collections are keyed maps
  (`[agents.claude_code]`, `[plugins.notify-ntfy]`), which TOML expresses well.

### 1.2 File layout

```
$XDG_CONFIG_HOME/omt/                     # macOS: ~/.config/omt (we do NOT use ~/Library)
├── config.toml                           # the main user config
├── secrets.toml                          # 0600, never merged into config.toml, never logged
├── keybindings.toml                      # keymap overrides (separate file, see §8.2)
├── instances.toml                        # device-local: known instances + per-instance overrides
├── themes/
│   ├── nord.toml                         # omt-native theme
│   └── imported/solarized-dark.toml      # produced by `omt theme import`
├── workflows/
│   └── run-integration-test.yaml         # one workflow per file
├── launch/
│   └── review.yaml                       # one launch configuration per file
├── plugins/
│   └── <plugin-id>/                      # plugin install roots (see 11-plugins.md)
└── state/                                # NOT config: caches, last layout, dismissals
```

Per-project, discovered by walking up from a workspace root to the nearest
enclosing `.omt/` (stopping at the filesystem root or a `.git` boundary,
whichever comes first — `.git` first, then keep walking one more level for the
monorepo case):

```
<repo-root>/.omt/
├── config.toml                           # project layer (restricted key set, §2.4)
├── workflows/*.yaml
├── launch/*.yaml
└── themes/*.toml
```

Environment overrides, all resolved once at startup:

| Variable | Effect |
|---|---|
| `OMT_CONFIG_DIR` | replaces `$XDG_CONFIG_HOME/omt` entirely |
| `OMT_CONFIG_FILE` | replaces just `config.toml` |
| `OMT_SECRETS_FILE` | replaces just `secrets.toml` |
| `OMT_INSTANCE_ID` | selects which `[instance.<id>]` override block applies |
| `OMT_NO_PROJECT_CONFIG=1` | disables the project layer (for untrusted checkouts) |
| `OMT_<SECTION>__<KEY>` | single-setting override, `__` is the path separator |

macOS and Linux use the XDG path above; Windows is supported through WSL2, which
is Linux and uses the same path
([D10](decisions.md#d10--platform-targets-macos-and-linux-windows-via-wsl2)).
There is no native-Windows `%APPDATA%` location in v1. `omt config path` prints
all of them, resolved.

### 1.3 Secrets are a separate file with enforced permissions

`secrets.toml` holds exactly one shape: opaque named credentials.

```toml
# ~/.config/omt/secrets.toml   (mode 0600, owner-only)
[stt.deepgram]
api_key = "dg_live_..."

[stt.openai]
api_key = "sk-..."

[server.bearer]
tokens = ["..."]                    # hashed at rest; see 13-security.md

[plugins."notify-ntfy"]
token = "tk_..."
```

Rules, enforced in code and covered by tests:

1. On load, `omt` stats the file. If mode is not `0600` (or the file is
   group/world readable on any Unix), loading **fails** with a diagnostic telling
   the user to run `chmod 600`. There is no "warn and continue".
2. Secrets are never merged into the `Config` value. They are resolved lazily
   through a `SecretRef` newtype; `Debug`, `Display` and `Serialize` on
   `SecretRef` emit `"<redacted>"`. This is a type-level guarantee, not a
   convention.
3. The main config refers to a secret by name, never by value:
   `api_key = { secret = "stt.deepgram.api_key" }`. A literal secret-looking
   value in `config.toml` is a **validation error** (§5), because it would end up
   in dotfile repositories.
4. `omt config get` and every capability response return `SecretRef` as
   `{ "secret": "stt.deepgram.api_key", "present": true }` — presence is
   observable, the value never is.
5. Secrets may also come from the OS keychain (`keyring` crate) or from an
   environment variable; the `secret` reference resolves through a provider chain
   `env → keychain → secrets.toml`, and `omt config sources` shows which provider
   answered.

---

## 2. The layering model

### 2.1 The layers, in precedence order

Later wins. Six layers, all present in every resolution:

| # | Layer | Source | Writable at runtime? |
|---|---|---|---|
| 0 | `Builtin` | compiled-in defaults from the schema | no |
| 1 | `User` | `~/.config/omt/config.toml` | yes (default target) |
| 2 | `Project` | `<repo>/.omt/config.toml` | yes, explicitly targeted |
| 3 | `Instance` | `[instance.<id>]` block in `instances.toml` | yes, explicitly targeted |
| 4 | `Session` | per-session overrides carried in the session record | yes |
| 5 | `Runtime` | in-memory, set by `config.set --ephemeral` or `OMT_*` env | yes, never persisted |

```
Builtin ◄─ User ◄─ Project ◄─ Instance ◄─ Session ◄─ Runtime
 weakest                                              strongest
```

`Runtime` above `Session` looks surprising but is right: an env var or an
explicit ephemeral override is the operator saying "for this process, now", and
it must beat stored state so that `OMT_SERVER__BIND=127.0.0.1:0 omt` is a
reliable escape hatch.

### 2.2 Merge semantics

Merging is **per leaf key**, not per table. A table in a lower layer is not
replaced wholesale by a higher one.

- **Scalars, enums, strings**: higher layer replaces.
- **Structs/tables**: recursive per-field merge.
- **Maps keyed by id** (`[agents.*]`, `[plugins.*]`, `[instance.*]`): merged by
  key; a key present in both is merged recursively.
- **Arrays**: replaced wholesale by default. Two explicit exceptions, marked in
  the schema with `#[omt(merge = "append")]`: `plugins.search_paths` and
  `terminal.env_passthrough`. For every other array, "replace" is the only
  predictable rule, and the schema test asserts no new array gains an append
  merge without a doc entry.
- **`null` / explicit unset**: a key set to the reserved value `"@unset"` at a
  higher layer restores the value from the layer below it. This is how a project
  config drops a user-level keybinding.

Every resolved leaf carries its provenance: `(value, layer, file, span)`. That
tuple is what `omt config sources` prints and what the TUI/web editors show as
"inherited from project config, line 14".

### 2.3 How a runtime change is persisted

`config.set` takes an explicit target layer. There is no implicit target.

```
omt config set appearance.theme nord                  # → User layer (default)
omt config set --project terminal.scrollback 50000    # → Project layer
omt config set --instance work-laptop server.bind …   # → Instance layer
omt config set --session s_7f2 agents.auto_attach false
omt config set --ephemeral log.level trace            # → Runtime, not written
```

Writes are performed with `toml_edit`, which means **comments, key order and
formatting are preserved**; only the targeted value's span is rewritten. The
write is atomic (temp file in the same directory, `fsync`, `rename`), and the
file watcher (§6) recognizes its own write by inode+mtime so a self-write does
not trigger a reload storm.

If the target file does not exist, it is created with the section header and a
generated comment pointing at the docs for that section.

The TUI editor and the web settings UI both call `config.set` — they have no
other path to disk. That is what makes P3 parity structural here.

### 2.4 The project layer is restricted

A `.omt/config.toml` arrives with a `git clone` and is therefore untrusted input.
Settings are tagged in the schema with a `scope`:

| Scope | Meaning | Example |
|---|---|---|
| `Any` | settable in any layer | `terminal.scrollback_lines` |
| `UserOrAbove` | ignored (with a diagnostic) if it appears in a project file | `server.bind`, `plugins.enabled` |
| `DeviceLocal` | only `instances.toml`/`Runtime`; never in `config.toml` | `appearance.font.family` |

A project file containing a `UserOrAbove` key produces a **warning diagnostic and
the key is dropped**, never applied. Additionally, the *first* time omt loads a
project config from a given repo it prompts for trust (TUI banner / web toast),
records the decision keyed by canonical path + a hash of the file, and re-prompts
when the file changes materially. `OMT_NO_PROJECT_CONFIG=1` and
`config.project.trust` / `config.project.revoke` capabilities control this.

---

## 3. Per-instance and multi-instance configuration

The requirement: **one web client attaches to several omt instances (laptop,
desktop, a server in a tailnet) and must be able to configure each one
differently** — while some things (font size on this phone) are properties of the
*viewing device*, not of any instance.

### 3.1 The ownership split

Every setting in the schema carries an `owner`:

| Owner | Lives where | Authoritative copy | Example |
|---|---|---|---|
| `Instance` | on the instance's disk | the instance | `server.bind`, `terminal.scrollback_lines`, `agents.*`, `plugins.*`, `notifications.rules` |
| `Device` | on the client (browser `localStorage` / TUI `state/`) | the client | `appearance.font.size`, `appearance.density`, `web.haptics`, `web.default_view` |
| `Shared` | instance, but a device may shadow it | instance, device may override locally | `appearance.theme` — the instance has a theme, a phone may pin a different one |

`omt config schema` returns the owner for every key, so the web UI renders three
tabs — **This device**, **This instance**, **All instances** — without hardcoding
any list.

### 3.2 Addressing an instance's config

Instances are addressed by `InstanceId` (a stable ULID minted on first run,
stored in `state/instance-id`) with a user-assignable label. Config capabilities
are instance-scoped by construction because *they are dispatched on that
instance*: the web client holds N connections, and `config.set` on connection *B*
edits instance *B*'s `config.toml`. There is no cross-instance write path and no
central config server. This falls directly out of the federation model in
[00 — Overview §7](00-overview.md) and [07](07-remote-protocol.md): each instance
is authoritative for itself.

The web client's own record of the instances it knows lives on the device:

```jsonc
// browser localStorage: omt.instances
[
  { "id": "01J...A", "label": "laptop",  "url": "wss://laptop.tail1234.ts.net:7878",
    "device_overrides": { "appearance.theme": "gruvbox-light", "appearance.font.size": 13 } },
  { "id": "01J...B", "label": "gpu-box", "url": "wss://gpu-box.tail1234.ts.net:7878",
    "device_overrides": { "appearance.font.size": 13 } }
]
```

The TUI keeps the same structure in `~/.config/omt/instances.toml`, which doubles
as the `Instance` layer source when running locally:

```toml
# ~/.config/omt/instances.toml     (device-local)
[instance."01J...A"]
label = "laptop"

[instance."01J...A".overrides]           # applied as layer 3 when this instance runs here
"server.bind" = "127.0.0.1:7878"

[instance."01J...B"]
label = "gpu-box"
url = "wss://gpu-box.tail1234.ts.net:7878"
```

Note the asymmetry, which is intentional: `overrides` under an instance id are
applied **only on the machine that hosts that instance**. Remote instances listed
here are connection records, not config.

### 3.3 Editing and syncing

- **Editing** an instance's config from the web client is `config.set` over that
  instance's connection, authorized by that connection's role (`Admin` required
  for `server.*`, `plugins.*`, `agents.*.command`; `Operator` for the rest —
  declared per key in the schema as `min_role`).
- **Syncing** is push-only and per-key, never a whole-file sync. There is no
  "config cloud". If a user wants the same settings on two instances, they run
  `omt config export --layer user > f.toml` and
  `omt config import f.toml --instance gpu-box`, or point both at a dotfiles
  checkout. Automatic multi-instance sync is explicitly out of scope: it needs
  conflict resolution, and two machines legitimately differ (paths, GPUs, fonts).
- **Live propagation**: a successful `config.set` emits a `ConfigChanged { keys,
  layer, actor }` event on that instance's bus. Every attached surface —
  including the TUI on the host machine — re-renders from the new resolved
  values. A second web client with the settings page open sees the change
  immediately.
- **Conflict**: `config.set` carries an optional `if_version` (the config's
  content hash from the last `config.get`). A mismatch returns
  `precondition_failed` with the current value, so two phones editing the same
  key produce a visible conflict rather than a lost write.

### 3.4 What a device may shadow

Device overrides apply **client-side only** and are keyed by
`(instance_id, key)` with a global fallback (`instance_id = "*"`). The instance
never learns about them. Only keys whose owner is `Device` or `Shared` may be
shadowed; the generated TS client type-checks this, so shadowing an `Instance`
key is a compile error in the web app.

---

## 4. Typed schema and generation

### 4.1 One type, many artifacts

```rust
/// Root configuration. Every field is documented; the doc comment becomes the
/// description in the JSON Schema, the TUI editor and the reference docs.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, OmtConfig)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub appearance: AppearanceConfig,
    pub terminal: TerminalConfig,
    pub keybindings: KeybindingsConfig,
    pub shell: ShellIntegrationConfig,
    pub agents: AgentsConfig,
    pub server: ServerConfig,
    pub notifications: NotificationsConfig,
    pub stt: SttConfig,
    pub media: MediaConfig,
    pub plugins: PluginsConfig,
    pub telemetry: TelemetryConfig,
    /// Per-instance override blocks; merged as layer 3.
    #[serde(default)]
    pub instance: BTreeMap<InstanceId, InstanceOverrides>,
}
```

The `OmtConfig` derive is the omt analogue of another terminal's `define_setting!` +
`inventory` registry (see [research/another terminal.md §5.3](../research/another terminal.md)). It walks
the type and emits, per leaf, a `SettingDescriptor`:

```rust
pub struct SettingDescriptor {
    pub path: &'static str,             // "terminal.scrollback_lines"
    pub ty: SettingType,                // Int{min,max} | Bool | Enum{variants} | String{pattern}
                                        // | Path | Duration | Color | Map | Array | Secret
    pub default: fn() -> serde_json::Value,
    pub doc: &'static str,              // from the doc comment
    pub scope: Scope,                   // Any | UserOrAbove | DeviceLocal
    pub owner: Owner,                   // Instance | Device | Shared
    pub min_role: Role,                 // Viewer | Operator | Admin
    pub reload: Reload,                 // Hot | Restart | RestartServer
    pub surfaces: Surfaces,             // bitflags: TUI | WEB | CLI (default: all)
    pub since: Version,
}
inventory::collect!(SettingDescriptor);
```

Attributes on fields set the non-default values:

```rust
pub struct TerminalConfig {
    /// Number of scrollback lines retained per session.
    #[omt(min = 0, max = 1_000_000, reload = "hot")]
    pub scrollback_lines: u32,

    /// Interface the API server binds to.
    #[omt(scope = "user_or_above", min_role = "Admin", reload = "restart_server")]
    pub bind: SocketAddr,
}
```

### 4.2 Generated artifacts and their CI checks

| Artifact | Generated into | CI check |
|---|---|---|
| JSON Schema (Draft 2020-12) | `schemas/config.schema.json` | regenerate + `git diff --exit-code` |
| TypeScript types + form metadata | `web/src/generated/config.ts` | same |
| Reference docs | `docs/reference/configuration.md` | same |
| Default annotated config | `omt config default` output, golden-tested | fixture diff |
| TUI editor form | built at runtime from the `inventory` registry | see below |
| Web settings UI | built at runtime from `config.schema` capability | see below |

**"A setting present in the file but absent from the TUI/web must be
impossible."** Two mechanisms make that true rather than aspirational, mirroring
[03 §5](03-capability-catalog.md):

1. The TUI editor does **not** contain a hand-written form. It renders the
   descriptor registry: every `SettingType` has exactly one widget, and
   `SettingType` is a closed enum, so a new type without a widget is a
   non-exhaustive `match` — a compile error.
2. The web settings UI renders from `config.schema` at runtime with the same
   closed union in generated TypeScript
   (`type SettingType = "int" | "bool" | ...`), and an exhaustiveness check on
   the renderer switch fails the web build.
3. A `tests/surface_parity.rs` walks `inventory::iter::<SettingDescriptor>` and
   asserts that each descriptor either declares a widget for both surfaces or
   carries an explicit `surfaces = "cli_only"` entry whose path appears in a
   committed allow-list (`config-surface-exemptions.toml`). Exemptions are
   printed in the generated reference docs.

---

## 5. Validation and diagnostics

The user requirement is explicit: **automatic syntax checking with detailed error
messages**. This section is the contract.

### 5.1 Pipeline

Validation is five passes, and **every pass runs to completion collecting
diagnostics**; none aborts on the first problem. Only a pass that cannot produce
a usable value for the next pass stops the pipeline (i.e. only pass 1 can be
fatal for a given file, and even then other files are still validated).

1. **Parse** — `toml_edit::DocumentMut`, retaining spans for every key and value.
   Syntax errors are reported with the exact byte range.
2. **Shape** — walk the document against the descriptor registry. Produces
   `UnknownKey`, `TypeMismatch`, `OutOfRange`, `BadEnumVariant`,
   `WrongScope`, `InsufficientRole`.
3. **Deserialize** — `serde` into `Config` with `deny_unknown_fields`. Any error
   here that pass 2 did not already report is a schema/registry bug and is
   surfaced as an internal diagnostic (with a "please file a bug" hint).
4. **Cross-field** — rules that need more than one key (§5.4).
5. **Referential** — theme names resolve to a file, `secret = "..."` references
   resolve to a present secret, plugin ids in `plugins.enabled` exist, keybinding
   actions name real capabilities, workflow/launch files parse.

### 5.2 Unknown keys are reported, never silently ignored

`deny_unknown_fields` alone gives a poor message and stops at the first offender.
Instead pass 2 enumerates *all* unknown keys with full dotted paths and, for
each, computes a suggestion: Damerau-Levenshtein distance against every known key
in the same table plus every known key anywhere, accepting a suggestion when
`distance <= max(1, len/4)`, preferring the same-table candidate. Case and
`-`/`_` differences are distance 0 for suggestion purposes (so `scrollbackLines`
suggests `scrollback_lines`).

Unknown keys are **errors** by default (`config.strict_unknown_keys = true`).
another tool treats them as diagnostics-only, which is friendlier for forward
compatibility; we split the difference: unknown keys under a namespace owned by a
plugin that is not installed (`plugins.<id>.*`) are warnings, everything else is
an error. Rationale: a typo in `terminal.scrolback_lines` silently doing nothing
is exactly the failure P7 exists to prevent.

### 5.3 Diagnostic rendering

Diagnostics are `ariadne`/`miette`-style: file, line, column, the source line, a
caret span, a code, and a help line. Every diagnostic has a stable code
(`OMT-C###`) that is documented and greppable.

Verbatim output of `omt config validate` on a broken config:

```
$ omt config validate

error[OMT-C101]: unknown key `terminal.scrolback_lines`
  ┌─ ~/.config/omt/config.toml:14:1
  │
14 │ scrolback_lines = 50000
  │ ^^^^^^^^^^^^^^^ no such setting in section `terminal`
  │
  = help: did you mean `scrollback_lines`?
  = note: run `omt config schema --section terminal` to list valid keys

error[OMT-C112]: value out of range for `terminal.scrollback_lines`
  ┌─ ~/.config/omt/config.toml:15:19
  │
15 │ scrollback_lines = 100000000
  │                    ^^^^^^^^^ maximum is 1000000
  │
  = help: 1000000 lines is roughly 400 MB per session at 80 columns

error[OMT-C120]: expected a boolean for `agents.claude_code.auto_install_hooks`
  ┌─ ~/.config/omt/config.toml:31:24
  │
31 │ auto_install_hooks = "yes"
  │                      ^^^^^ found a string
  │
  = help: use `true` or `false` (TOML booleans are unquoted and lowercase)

error[OMT-C205]: `server.bind` is not a loopback address but no auth backend is configured
  ┌─ ~/.config/omt/config.toml:44:8
  │
44 │ bind = "0.0.0.0:7878"
  │        ^^^^^^^^^^^^^^ binds to all interfaces
  │
  ┌─ ~/.config/omt/config.toml:47:1
  │
47 │ [server.auth]
48 │ backends = []
  │            ^^ no backend enabled
  │
  = help: set `server.auth.backends = ["bearer"]` and add a token with
          `omt auth token create`, or use `server.tailscale.enabled = true`
  = note: refusing to start would be worse than refusing to load; omt will not
          bind until this is resolved (see docs/architecture/13-security.md)

error[OMT-C301]: literal secret in `config.toml`
  ┌─ ~/.config/omt/config.toml:58:11
  │
58 │ api_key = "dg_live_9f2c4a1b8e7d6c5a4b3f2e1d"
  │           ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ looks like a Deepgram API key
  │
  = help: move it to ~/.config/omt/secrets.toml and reference it:
              api_key = { secret = "stt.deepgram.api_key" }
  = note: `config.toml` is commonly committed to dotfile repositories

warning[OMT-C150]: setting `server.bind` ignored in project configuration
  ┌─ /home/x/src/api/.omt/config.toml:3:1
  │
 3 │ bind = "0.0.0.0:9000"
  │ ^^^^ scope is `user_or_above`
  │
  = note: project configuration cannot change how the daemon is exposed

warning[OMT-C402]: keybinding `ctrl-b s` shadows `ctrl-b` prefix binding `session.list`
  ┌─ ~/.config/omt/keybindings.toml:9:1
  │
 9 │ "ctrl-b s" = "session.split"
  │ ^^^^^^^^^^ chord starts with an existing single-key binding
  │
  = help: unbind the prefix with `"ctrl-b" = "none"` if the chord is intended

5 errors, 2 warnings — configuration not applied, previous configuration retained.
```

Machine-readable form, for the web UI and for editors:

```
$ omt config validate --format json
{
  "ok": false,
  "diagnostics": [
    {
      "code": "OMT-C101",
      "severity": "error",
      "message": "unknown key `terminal.scrolback_lines`",
      "path": "terminal.scrolback_lines",
      "file": "/home/x/.config/omt/config.toml",
      "span": { "start": { "line": 14, "column": 1, "offset": 289 },
                "end":   { "line": 14, "column": 16, "offset": 304 } },
      "suggestion": { "kind": "replace_key", "value": "scrollback_lines" },
      "help": "did you mean `scrollback_lines`?"
    }
  ],
  "summary": { "errors": 5, "warnings": 2 }
}
```

`suggestion` is structured (`replace_key`, `replace_value`, `remove_key`,
`move_to_secrets`) so the TUI and web editors can offer a one-click fix that is
applied through `toml_edit` at the reported span.

### 5.4 Cross-field validation rules (initial set)

| Code | Rule |
|---|---|
| `OMT-C201` | `server.tls.enabled` requires `cert_file` and `key_file`, both readable |
| `OMT-C202` | `server.tls` and `server.tailscale.enabled = true` with `tailscale.mode = "tsnet"` conflict (tsnet terminates TLS itself) |
| `OMT-C203` | `stt.default_provider` must appear in `stt.providers` and have a resolvable key |
| `OMT-C205` | non-loopback `server.bind` requires ≥1 auth backend (P8) |
| `OMT-C206` | `server.auth.backends` containing `"password"` requires at least one user in `secrets.toml` |
| `OMT-C210` | `appearance.theme` must resolve in `themes/` or the built-in set |
| `OMT-C220` | `agents.<id>.command` must exist on `PATH` (warning, not error — it may exist only on the host) |
| `OMT-C230` | `media.osc_bridge.max_bytes` ≤ `media.quota.max_blob_bytes` ≤ `media.quota.max_total_bytes` |
| `OMT-C240` | every `plugins.enabled` id has an installed, manifest-valid plugin |
| `OMT-C250` | `telemetry.enabled` may only be `false` (see §7.11) |
| `OMT-C401`–`OMT-C412` | keybinding conflicts: duplicate trigger, chord/prefix shadowing, unknown capability or args, unknown `when` context, inner-keymap shadowing, undeliverable or ambiguous chords, refusal-list violations. Individual codes are defined and owned by [16 §5.2](16-input-and-keymap.md#52-static-validation-at-config-load) |
| `OMT-C420`–`OMT-C425` | modal-keymap conflicts (vim/emacs): ambient-mode bindings, `modes` without a modal engine, operator-pending shadowing, `Esc` timing. Owned by [16 §6.8](16-input-and-keymap.md#68-conflict-validation-for-modal-keymaps) |

### 5.5 Where validation runs

- `omt config validate [--file F] [--all] [--format json]` — capability
  `config.validate`, so the web UI's "Validate" button and CI use the same code.
- Automatically on every load and every reload.
- On `config.set`, *before* writing: the proposed document is validated in memory
  and rejected with the same diagnostics if it would break.
- As a pre-commit-installable hook: `omt config validate --all` exits non-zero.
- The generated JSON Schema is published so editors (VS Code, Helix, Zed) give
  completion and inline errors in `config.toml` via `taplo`/`even-better-toml`
  with a `#:schema` directive that `omt config default` emits at the top of a new
  file.

---

## 6. Live reload

### 6.1 Watching

`notify` (inotify on Linux, FSEvents on macOS — the v1 targets per
[D10](decisions.md#d10--platform-targets-macos-and-linux-windows-via-wsl2))
watches the config *directory*,
not individual files, so editor atomic-rename saves (vim, VS Code) are caught.
Events are debounced 150 ms and coalesced. The project config for each open
workspace is watched too, subject to the trust decision in §2.4.

### 6.2 Atomic apply

Reload is transactional:

```
1. read all layers  →  2. validate (§5)  →  3. build new resolved Config
                              │ fail
                              ▼
                       keep current config, emit ConfigReloadFailed{diagnostics},
                       show a persistent banner in TUI/web with the diagnostics
4. diff old vs new, per leaf key → Vec<ChangedKey>
5. for each changed key, dispatch to its Reload class:
     Hot           → apply now via the owning subsystem's `apply_config` hook
     Restart       → record in `pending_restart`, surface in UI
     RestartServer → restart just the HTTP/WS listener (sessions survive)
6. swap the Config into the ArcSwap; emit ConfigChanged{keys, layer, actor}
```

No subsystem reads `Config` directly at use time. Each subsystem receives an
`ArcSwap<Config>` handle and implements
`fn apply_config(&self, old: &Config, new: &Config) -> Result<(), ApplyError>`.
If any `apply_config` returns an error, step 6 is rolled back: the previous
`Config` is re-applied to every subsystem that already accepted the new one (each
`apply_config` must therefore be idempotent and reversible — asserted by a test
that applies A→B→A and compares subsystem state).

### 6.3 Reload classes

| Class | Settings (examples) | Behaviour |
|---|---|---|
| `Hot` | theme, colours, keybindings, scrollback size (grow only), notification rules, agent detection overrides, block rendering, log level, workflows, launch configs, plugin enable/disable | applied immediately, visible in every attached surface within one frame |
| `RestartServer` | `server.bind`, `server.tls.*`, `server.auth.backends`, `server.tailscale.*` | listener is rebuilt; existing WS clients are told to reconnect; PTYs untouched |
| `Restart` | `store.backend`, `store.path`, `terminal.vt.*` parser feature flags, `media.tmp_dir` | recorded as pending; `omt config pending` lists them; the TUI shows "restart required for 2 settings" |

Shrinking `terminal.scrollback_lines` is `Hot` but lossy, so it prompts on the
surface that initiated the change (an `effects = [DESTRUCTIVE]` capability call
per [03 §2](03-capability-catalog.md)).

### 6.4 Failure behaviour, precisely

- A reload that fails validation **never** partially applies. The previous
  configuration keeps running.
- The diagnostics are broadcast as an event, so a phone sees "your laptop's
  config.toml has 3 errors" with the caret rendering, and can fix it remotely via
  `config.set`.
- If the config is broken **at startup**, omt starts with builtin defaults, in a
  clearly-marked degraded mode: no server binding at all (loopback local socket
  only), a banner, and a non-zero exit from `omt config validate`. It does not
  refuse to start, because the user's sessions matter more than their settings.

---

## 7. The settings surface

Types are Rust types; defaults are shown as TOML. This is the initial surface;
each subsystem's document owns its own section's details.

### 7.1 `[appearance]`

| Key | Type | Default | Owner | Reload | Notes |
|---|---|---|---|---|---|
| `theme` | string | `"omt-dark"` | Shared | Hot | name of a built-in or `themes/*.toml` |
| `theme_light` | string? | `"omt-light"` | Shared | Hot | used when the OS reports light mode |
| `theme_mode` | enum `auto\|dark\|light` | `"auto"` | Device | Hot | |
| `density` | enum `compact\|comfortable` | `"comfortable"` | Device | Hot | affects TUI chrome and web padding |
| `show_agent_badges` | bool | `true` | Device | Hot | agent-state badges on panes/tabs |
| `block_style` | enum `bordered\|gutter\|minimal` | `"gutter"` | Device | Hot | how command blocks are delimited |
| `cursor.style` | enum `block\|bar\|underline` | `"block"` | Shared | Hot | |
| `cursor.blink` | bool | `false` | Shared | Hot | |

### 7.2 `[appearance.font]` (Device-owned)

| Key | Type | Default |
|---|---|---|
| `family` | string | `"monospace"` (web: CSS stack) |
| `size` | float | `13.0` |
| `line_height` | float | `1.2` |
| `ligatures` | bool | `false` |
| `fallbacks` | array<string> | `[]` |

The TUI cannot change the terminal emulator's font; these keys are `surfaces =
"web"` there and the TUI editor shows them read-only with an explanatory note
rather than hiding them (which would violate §4.2's rule).

### 7.3 `[terminal]`

| Key | Type | Default | Reload |
|---|---|---|---|
| `scrollback_lines` | u32 (0..=1_000_000) | `10000` | Hot |
| `default_shell` | path? | `null` (use `$SHELL`) | Restart |
| `default_cwd` | enum `home\|last\|workspace` | `"workspace"` | Hot |
| `word_separators` | string | `" \t()[]{}'\"<>,;:"` | Hot |
| `bell` | enum `off\|visual\|audible\|both` | `"visual"` | Hot |
| `mouse_reporting` | bool | `true` | Hot |
| `bracketed_paste` | bool | `true` | Hot |
| `paste_confirm_lines` | u16 | `10` | Hot |
| `paste_confirm_on_newline` | bool | `true` | Hot |
| `env_passthrough` | array<string> (append-merge) | `["TERM_PROGRAM","COLORTERM","LANG"]` | Restart |
| `blocks.enabled` | bool | `true` | Hot |
| `blocks.heuristic_fallback` | bool | `true` | Hot |
| `blocks.max_retained` | u32 | `2000` | Hot |
| `reflow.on_resize` | enum `active_block\|all\|none` | `"active_block"` | Hot |

### 7.4 `[shell]` — shell integration

| Key | Type | Default |
|---|---|---|
| `auto_install` | bool | `true` |
| `shells` | array<enum `bash\|zsh\|fish\|nu\|pwsh`> | auto-detected |
| `osc133` | enum `emit\|consume\|both` | `"both"` |
| `inject_env` | bool | `true` (adds `OMT_INSTANCE`, `OMT_SESSION`, `OMT_SOCK`, and `OMT_SHELL_INTEGRATION` — see [04 §7.3](04-terminal-core.md#73-propagation-into-subshells-and-over-ssh) and [06 §7.2](06-agent-layer.md#72-correlation)) |
| `propagate_over_ssh` | bool | `false` (see [09](09-ssh-and-media.md)) |
| `command_metadata` | bool | `true` (pwd, git branch, exit code per block) |

### 7.5 `[agents]`

Global, then a map keyed by agent id.

```toml
[agents]
auto_detect = true                  # tier 1/2 detection
auto_attach = true                  # bind an observed agent to the session
detection_manifest_dir = "~/.config/omt/agents"   # local overrides for detection rules
prefer_source = "hooks"             # hooks | protocol | transcript | auto
idle_debounce_ms = 700

[agents.claude_code]
enabled = true
command = "claude"                  # detection + spawn
install_hooks = "prompt"            # always | prompt | never
hook_events = ["PreToolUse", "PostToolUse", "SessionStart", "Notification", "Stop"]
defer_tools = ["AskUserQuestion"]   # tools whose PreToolUse is parked for remote answer
transcript_dir = "~/.claude/projects"
permission_mode = "default"         # passed through; omt never overrides silently

[agents.codex]
enabled = true
command = "codex"
protocol = "app-server"             # app-server | acp | none

[agents.opencode]
enabled = true
protocol = "acp"

[agents.detect_overrides]
# process-name → agent id, for wrappers/sandboxes that hide the real binary
"my-claude-wrapper" = "claude_code"
```

Per-agent keys (`enabled`, `command`, `args`, `env`, `install_hooks`,
`hook_events`, `defer_tools`, `protocol`, `transcript_dir`, `resume_argv`,
`state_labels`) are declared once in an `AgentConfig` struct and the map is
`BTreeMap<AgentId, AgentConfig>`, so a plugin-contributed adapter
([11](11-plugins.md)) gets the same schema, editor and validation for free.
Adding an agent must not require a new Rust `match` arm — that is the explicit
lesson from [research/another tool.md §8](../research/another tool.md).

### 7.6 `[server]`

```toml
[server]
enabled = false                     # off until the user opts in
bind = "127.0.0.1:7878"             # non-loopback requires auth (OMT-C205)
advertise_url = ""                  # used when minting invite links
max_clients = 32
idle_client_timeout = "10m"

[server.auth]
backends = []                       # "invite" | "bearer" | "password" | "tailnet"
default_role = "operator"           # viewer | operator | admin
                                    # Roles are a *sharing* control (D2): the
                                    # owner's own devices are operator/admin.
invite_ttl = "24h"
invite_default_role = "viewer"

[server.tls]
enabled = false
cert_file = ""
key_file = ""

[server.tailscale]
enabled = false
mode = "host"                       # host: bind to the tailscale0 IP of the host daemon
                                    # tsnet: embed tsnet, appear as its own tailnet node
hostname = "omt"                    # tsnet node name
auth_key = { secret = "server.tailscale.auth_key" }
trust_tailnet_identity = true       # accept the tailnet peer identity as an auth backend
allowed_users = []                  # empty = any user in the tailnet; else login names
funnel = false                      # never enabled implicitly; public exposure is opt-in
```

`tailscale.mode = "tsnet"` is the recommended deployment: omt appears as
`omt.<tailnet>.ts.net`, gets a MagicDNS name and a WireGuard-encrypted transport,
and `trust_tailnet_identity` maps the verified peer to a role without any token.
`funnel = true` puts the instance on the public internet and is therefore gated
by an extra confirmation and a mandatory second auth backend (`OMT-C207`), plus
the startup checklist in [13 §10](13-security.md#10-checklist--publishing-an-instance-over-tailscale-funnel),
which `omt serve` enforces rather than merely documents. See
[07](07-remote-protocol.md) and [13](13-security.md).

### 7.7 `[notifications]`

```toml
[notifications]
enabled = true
sinks = ["desktop"]                 # desktop | web-push | plugin:<id>
quiet_hours = { from = "23:00", to = "08:00" }
min_interval = "5s"                 # per-session rate limit

[[notifications.rules]]
when = "agent.blocked"              # event selector; matches AgentState::Blocked
sessions = "*"                      # or a glob over workspace/session names
sinks = ["desktop", "web-push"]
title = "{agent} needs you in {workspace}"
priority = "high"

[[notifications.rules]]
when = "agent.finished"
sinks = ["desktop"]
min_duration = "30s"                # only notify for runs longer than this
```

### 7.8 `[stt]`

```toml
[stt]
enabled = false
default_provider = "whisper-local"
push_to_talk = true
auto_submit = false                 # never submit a transcript to an agent without confirmation
language = "en"

[stt.providers.deepgram]
kind = "deepgram"
model = "nova-3"
api_key = { secret = "stt.deepgram.api_key" }     # BYOK

[stt.providers.openai]
kind = "openai"
model = "whisper-1"
api_key = { secret = "stt.openai.api_key" }

[stt.providers."whisper-local"]
kind = "whisper-cpp"
model_path = "~/.local/share/omt/models/ggml-base.en.bin"
threads = 4
```

BYOK is the only mode: omt has no keys of its own and no proxy. A provider whose
key is absent is listed as `unavailable` rather than erroring at load.

### 7.9 `[media]`

Owned by [09 — SSH and media](09-ssh-and-media.md); the defaults there are
authoritative and this table mirrors them.

| Key | Type | Default | Owning section |
|---|---|---|---|
| `quota.max_blob_bytes` | bytes | `"32MiB"` | [09 §2](09-ssh-and-media.md#2-the-blob-store) |
| `quota.max_total_bytes` | bytes | `"512MiB"` | 09 §2 |
| `quota.max_blobs_per_session` | u32 | `200` | 09 §2 |
| `quota.ttl` | duration | `"24h"` | 09 §2 |
| `quota.ttl_referenced` | duration | `"7d"` | 09 §2 |
| `blob_dir` | path | `$XDG_RUNTIME_DIR/omt/<instance>/blobs` | 09 §2 |
| `images.enabled` | bool | `true` | 09 §7.2 |
| `images.protocol` | enum `auto\|kitty\|sixel\|iterm2\|none` | `"auto"` | 09 §7.2 |
| `images.max_rows` | u16 | `20` | 09 §7.2 |
| `images.thumbnail_max_px` | u32 | `512` | 09 §7.3 |
| `clipboard.osc52_write` | bool | `true` | 09 §3.1 |
| `clipboard.osc52_max_bytes` | bytes | `"64KiB"` | 09 §3.1 |
| `clipboard.osc52_read` | bool | `false` (reading the user's clipboard from a foreign terminal is off by default) | 09 §3.2 |
| `osc_bridge.max_bytes` | bytes | `"8MiB"` | 09 §5.3.1 |
| `reverse_socket.hosts` | array\<string\> | `[]` (opt-in per host) | 09 §5.2 |

Note the distinct per-*image* cap in the terminal core:
`TermConfig::max_image_bytes` (32 MiB) bounds one inline image in the parser, and
`ScrollbackLimits::max_image_bytes_total` (64 MiB) bounds a session's resident
image payload — see [04 §1.4](04-terminal-core.md#14-where-vte-is-not-enough) and
[04 §2.5](04-terminal-core.md#25-bounding-memory).

### 7.10 `[plugins]`

```toml
[plugins]
enabled = ["notify-ntfy", "aider-adapter"]
search_paths = ["~/.config/omt/plugins", "/usr/local/share/omt/plugins"]  # append-merge
allow_unsigned = true               # local development
auto_update = false
default_timeout = "10s"

[plugins."notify-ntfy"]
# free-form, validated against the plugin's own schema from its manifest
topic = "my-omt"
server = "https://ntfy.sh"
token = { secret = "plugins.notify-ntfy.token" }
```

### 7.11 `[telemetry]`

```toml
[telemetry]
enabled = false        # the only accepted value
```

The key exists so that its absence cannot be mistaken for an oversight and so
that `omt config get telemetry` answers the question. Setting it to `true` is
`OMT-C250`: *"omt has no telemetry endpoint; this setting exists only to be
false."* There is no build flag, no environment variable, and no plugin
capability that changes this — a plugin that wants to phone home must declare a
`net` scope and is visible in the plugin permission list
([11 §4](11-plugins.md)).

### 7.12 `[log]`

| Key | Type | Default |
|---|---|---|
| `level` | enum `error\|warn\|info\|debug\|trace` | `"info"` |
| `file` | path? | `$XDG_STATE_HOME/omt/omt.log` |
| `max_size` | bytes | `"32MiB"` |
| `redact_secrets` | bool | `true` (cannot be set to `false`; `OMT-C251`) |

---

## 8. Themes and keybindings

### 8.1 Theme format

omt themes are TOML. The colour model is deliberately small: 16 ANSI colours plus
a handful of UI roles, because a TUI cannot render gradients or background images
and pretending otherwise produces themes that look wrong.

`~/.config/omt/themes/nord.toml`, complete:

```toml
#:schema https://omt.dev/schemas/theme.schema.json
name = "Nord"
author = "Arctic Ice Studio"
appearance = "dark"                 # dark | light — drives theme_mode = "auto"

[colors]
foreground = "#d8dee9"
background = "#2e3440"
cursor      = "#d8dee9"
cursor_text = "#2e3440"
selection_background = "#434c5e"
selection_foreground = "#eceff4"

[colors.normal]
black   = "#3b4252"
red     = "#bf616a"
green   = "#a3be8c"
yellow  = "#ebcb8b"
blue    = "#81a1c1"
magenta = "#b48ead"
cyan    = "#88c0d0"
white   = "#e5e9f0"

[colors.bright]
black   = "#4c566a"
red     = "#bf616a"
green   = "#a3be8c"
yellow  = "#ebcb8b"
blue    = "#81a1c1"
magenta = "#b48ead"
cyan    = "#8fbcbb"
white   = "#eceff4"

# UI roles. All optional: each is derived from the palette above when absent,
# so a 18-colour theme is a valid theme.
[ui]
accent          = "#88c0d0"
border          = "#434c5e"
border_focused  = "#88c0d0"
status_bar_bg   = "#3b4252"
status_bar_fg   = "#d8dee9"
muted           = "#616e88"          # hint/disabled text
success         = "#a3be8c"
warning         = "#ebcb8b"
error           = "#bf616a"
# Agent-state colours — omt-specific, this is the part no imported theme has.
agent_idle      = "#616e88"
agent_working   = "#88c0d0"
agent_blocked   = "#ebcb8b"
agent_done      = "#a3be8c"
agent_failed    = "#bf616a"

[block]
gutter_running  = "#88c0d0"
gutter_success  = "#a3be8c"
gutter_failure  = "#bf616a"
gutter_bg       = "#333a47"
```

Derivation rules when `[ui]` is partly absent are deterministic and documented:
`accent = normal.cyan`, `border = mix(background, foreground, 0.2)`,
`muted = mix(background, foreground, 0.45)`, `success/warning/error =
normal.green/yellow/red`, agent colours from `muted/cyan/yellow/green/red`. Two
runs of the derivation on the same input must produce identical output (golden
test), so a theme file is fully reproducible.

Contrast: `omt theme lint <file>` reports WCAG contrast ratios for
foreground-on-background and each UI role, warning below 4.5:1. It does not
refuse the theme.

#### Importers

`omt theme import <file>` detects the format by extension and content:

**another terminal YAML** (schema in [research/another terminal.md §3.2](../research/another terminal.md)) —
`accent`, `background`, `foreground`, `cursor`, `details`,
`terminal_colors.normal.*`, `terminal_colors.bright.*`, optional
`background_image`, and gradient objects for `accent`/`background`.

Mapping:

| another terminal | omt |
|---|---|
| `foreground` / `background` / `cursor` | `colors.foreground` / `colors.background` / `colors.cursor` |
| `accent` | `ui.accent` |
| `terminal_colors.normal.*` | `colors.normal.*` |
| `terminal_colors.bright.*` | `colors.bright.*` |
| `details: darker\|lighter` | `appearance = "dark"\|"light"`; drives `ui.muted` derivation |
| `background_image` | dropped, with an informational note |
| gradient `{top,bottom}` / `{left,right}` | first stop taken as the solid colour, note emitted |
| `name` absent | derived from the filename (kebab → Title Case), as another terminal does |

**iTerm2 `.itermcolors`** — an Apple XML property list whose keys are
`Ansi 0 Color` … `Ansi 15 Color`, `Background Color`, `Foreground Color`,
`Cursor Color`, `Cursor Text Color`, `Bold Color`, `Selection Color`,
`Selected Text Color`, `Link Color`, `Badge Color`, `Cursor Guide Color`, each a
dict of `Red Component` / `Green Component` / `Blue Component` (floats 0.0–1.0)
plus `Color Space` (`sRGB` or `Calibrated`). Mapping: ANSI 0–7 → `colors.normal`
in black/red/green/yellow/blue/magenta/cyan/white order, 8–15 →
`colors.bright`, the named colours to their omt equivalents; `Bold Color`,
`Badge Color` and `Cursor Guide Color` are dropped with a note. Components are
multiplied by 255 and rounded; `Calibrated` is treated as sRGB with a warning.

Both importers write a normal omt theme file into `themes/imported/` — there is
no runtime dependency on the foreign format, and no foreign code is used. This is
the clean-room line from [14 — Licensing](14-licensing.md): the *schema* is an
interface fact, the *implementation* is ours.

`omt theme import --dir ~/.another terminal/themes` bulk-imports and prints a table of what
was dropped per file.

### 8.2 Keybinding format

`keybindings.toml` is a flat map of *trigger → action*, where an action is a
capability name with optional arguments. Flat because it is greppable, diffable,
and machine-editable; contexts are expressed on the trigger, not by nesting.

```toml
#:schema https://omt.dev/schemas/keybindings.schema.json

# ── Top-level keys: exactly two ───────────────────────────────────────────
leader = "ctrl-b"      # relocates the whole `<leader>` namespace with one edit
keymap = "default"     # "default" | "vim" | "emacs" | a name in keymaps/

# ── Global ────────────────────────────────────────────────────────────────
"<leader> c"      = "session.create"
"<leader> x"      = { capability = "session.close", args = { confirm = true } }
"<leader> |"      = { capability = "pane.split", args = { direction = "vertical" } }
"<leader> -"      = { capability = "pane.split", args = { direction = "horizontal" } }
"<leader> z"      = "pane.zoom"
"<leader> ["      = "ui.copy_mode.enter"
"<leader> w"      = "ui.open_session_picker"
"<leader> a"      = "ui.open_agent_dashboard"
"<leader> ,"      = "ui.open_settings"
"ctrl-shift-p"    = "ui.open_command_palette"

# ── Context-scoped: `when` restricts the binding ──────────────────────────
[[binding]]
trigger = "enter"
when    = "card_focused"
capability = "interaction.resolve"

[[binding]]
trigger = "esc"
when    = "copy_mode"
capability = "ui.copy_mode.exit"

[[binding]]
trigger = "ctrl-c"
when    = "terminal_focused && !copy_mode"
force   = true            # `ctrl-c` is on 16 §5.4's refusal list
capability = "session.send_keys"
args     = { keys = "" }

# ── Unbinding ─────────────────────────────────────────────────────────────
"<leader> d" = "none"
```

Three more per-binding fields, shown together because they only exist in the
`[[binding]]` table form:

```toml
[[binding]]
trigger  = "ctrl-shift-v"
requires = "kitty_keyboard"     # or "modify_other_keys", "cmd_forwarding"
platform = ["macos", "linux"]
capability = "media.image.paste"

[[binding]]
trigger    = "d d"
modes      = ["normal"]         # only meaningful under a modal keymap
capability = "explorer.delete"

[[binding]]
trigger    = "<leader> ctrl-h"
repeatable = true               # exempt from 16 §9.4's repeat guard
capability = "pane.resize"
args       = { direction = "left" }
```

Grammar:

- **Trigger** — `mod-mod-key`, modifiers from `ctrl|alt|shift|cmd|super` in any
  order (normalized to that order on write), key names from a closed set
  (`a`–`z`, `0`–`9`, `f1`–`f24`, `enter`, `esc`, `tab`, `space`, `backspace`,
  `delete`, `up`, `down`, `left`, `right`, `home`, `end`, `pageup`, `pagedown`,
  and printable punctuation). **Chords** are space-separated keystrokes in one
  string: `"ctrl-b c"`. This is the another terminal convention and it is a good one.
  A trigger may also name a **mouse event with modifiers** (`"shift-mouse1"`);
  the spelling is [16 §2.3](16-input-and-keymap.md#23-resolution)'s.
- **`<leader>`** — legal in **any** trigger position, and expands to whatever the
  top-level `leader` key names. Writing the literal `ctrl-b` is still legal and
  still means literally `ctrl-b`; the point of `<leader>` is that relocating the
  prefix is one edit rather than forty.
- **Top-level keys** — exactly two: `leader` (default `"ctrl-b"`) and `keymap`
  (`"default" | "vim" | "emacs"`, or a name resolved in `keymaps/`, see
  [16 §6.5](16-input-and-keymap.md#65-the-keymap-abstraction)). Everything else
  at the top level is a trigger.
- **`"none"`** unbinds, exactly as another terminal's `REMOVED_KEYBINDING_SERIALIZATION`.
- **Action** — a bare string (a capability name) or a table with `capability`
  and `args`. `args` is validated against that capability's input schema at
  config-validation time, so a typo in `direction = "verticl"` is caught by
  `omt config validate`, not at keypress time.
- **`when`** — a small boolean predicate with `&&`, `||`, `!`, parentheses over
  named contexts. **The vocabulary is
  [16 §4.1](16-input-and-keymap.md#41-the-context-set)'s `ContextSet`**, which is
  the authoritative superset; this document does not enumerate it, so the two
  cannot drift.
- **Resolution** — how competing bindings are ordered is **not** decided here.
  [16 §2.3](16-input-and-keymap.md#23-resolution) owns it, in five rules
  (pending chord, modal context, specificity, config layer, passthrough). This
  document owns the file's shape; 16 owns what the file means.
- **Platform and capability gates** — optional `platform = ["macos"]` and
  `requires = "kitty_keyboard" | "modify_other_keys" | "cmd_forwarding"` on a
  `[[binding]]`. `platform` is about the OS; `requires` is about a negotiated
  terminal capability ([16 §5.5](16-input-and-keymap.md#55-terminal-capability-probing)).
  A binding whose gate is unmet is not installed, and a *user* binding in that
  state is reported (`OMT-C408`) rather than silently dropped.
- **`force = true`** — required to bind a key on
  [16 §5.4](16-input-and-keymap.md#54-conflict-policy)'s refusal list. Legal only
  in the `[[binding]]` table form; without it the binding is an `OMT-C410` error.
- **`modes = [...]`** — restricts a binding to named modes of a modal keymap
  ([16 §6](16-input-and-keymap.md#6-modal-keymaps-vim-mode-and-emacs-mode)).
  Table form only; declaring it under a non-modal keymap is an `OMT-C421` error.
- **`repeatable = true`** — exempts the binding from the key-repeat guard in
  [16 §9.4](16-input-and-keymap.md#94-key-repeat). Table form only.

Because actions are capability names, **the keymap is parity-checked**: a test
asserts every action in the default keymap names a registered capability, and
[03 §5](03-capability-catalog.md)'s parity test asserts every non-Admin
capability has a TUI binding or an explicit exemption. Keybindings for the web
client come from the same file (the instance serves it; device-local overrides
may shadow it), so a phone with a hardware keyboard gets the same map.

`omt keys list` prints the resolved map with provenance; `omt keys conflicts`
runs just the `OMT-C4xx` checks.

---

## 9. Profiles: launch configurations and workflows

### 9.1 Launch configurations

A launch configuration is a named workspace + session + layout template, launched
by name. The recursive pane-tree shape is adopted from another terminal's launch
configurations ([research/another terminal.md §5.2](../research/another terminal.md)) because it maps
one-to-one onto our BSP layout tree; the file is YAML because that corpus is
YAML and because a deeply recursive structure is genuinely nicer in YAML than in
TOML.

`~/.config/omt/launch/review.yaml`:

```yaml
# omt launch configuration — `omt launch review`
name: review
description: Agent + tests + shell on the api repo
active_window: 0

windows:
  - active_tab: 0
    tabs:
      - title: api
        color: blue
        layout:
          split: columns               # columns | rows (canonical)
          ratio: [0.6, 0.4]
          panes:
            - cwd: ~/src/api
              focused: true
              # Spawn an agent directly as a PTY child with known argv —
              # never by typing into a shell (see research/another tool.md §8).
              agent:
                id: claude_code
                args: ["--permission-mode", "plan"]
                prompt: "Review the diff on this branch and list risks."
            - split: rows
              panes:
                - cwd: ~/src/api
                  commands: ["cargo watch -x test"]
                - cwd: ~/src/api
      - title: infra
        layout:
          cwd: ~/src/infra

env:
  RUST_LOG: info

on_attach:
  - capability: agent.queue.enqueue
    args: { text: "summarize the test failures when they finish" }
```

Semantics:

- `omt launch review` creates the workspace/sessions/panes if absent; if a
  workspace launched from this config already exists, it **attaches** rather than
  duplicating (idempotent by `name`), unless `--fresh`.
- `layout` is either a leaf (`cwd`, plus optional `commands`, `agent`, `shell`,
  `focused`) or a branch (`split`, `ratio`, `panes`) — the recursion is the
  entire schema.
- **`split` is `columns` | `rows`**, matching the `Axis` type
  ([17 §1.2](17-panes-and-layout.md#12-types),
  [17 §4.2](17-panes-and-layout.md#42-the-serialization-format)). `columns` puts
  children side by side; `rows` stacks them. The spellings `horizontal` and
  `vertical` that tmux, another terminal and hand-written files use **are accepted on read**
  — there is a corpus of them and breaking it buys nothing — and map to
  `columns` and `rows` respectively. Everything omt *writes*, including
  `omt launch save`, uses the canonical spelling, and `config.validate` reports
  the foreign spellings as a note rather than an error. omt writes `columns`/
  `rows` because "horizontal split" means opposite things in tmux and in most GUI
  terminals, so those words cannot be read without a footnote.
- `agent:` is the omt-specific extension: it declares which adapter to bind, its
  argv, and an optional initial prompt delivered through the adapter's native
  channel, not synthesized keystrokes (P4).
- `commands:` runs in the pane's shell after shell integration is ready.
- `on_attach` is a list of capability calls, so anything the TUI can do at
  startup, a launch config can do.
- Project-local launch configs (`<repo>/.omt/launch/*.yaml`) shadow user ones of
  the same name and are subject to the same trust prompt as project config, with
  an extra rule: a project launch config may not set `agent.args` containing
  `--dangerously-*` style flags without confirmation (`OMT-C260`).

Capabilities: `launch.list`, `launch.get`, `launch.run`, `launch.save`
(`omt launch save <name>` snapshots the current workspace into a file — the
round-trip is what makes these worth maintaining), `launch.delete`.

### 9.2 Workflows

A workflow is a saved, parameterized command. another terminal's format
([research/another terminal.md §4](../research/another terminal.md)) is adopted deliberately unchanged
for the command variant, so the public `a terminal vendor/workflows` corpus loads as-is:
no `type:` key, `{{placeholder}}` substitution, one workflow per file.

`~/.config/omt/workflows/rebase-onto-main.yaml`:

```yaml
---
name: Rebase onto main
command: git fetch origin && git rebase origin/{{branch}}
description: Fetch and rebase the current branch onto a base branch.
tags: [git]
arguments:
  - name: branch
    description: Base branch to rebase onto.
    default_value: main
shells: [bash, zsh, fish]
author: omt
```

omt extensions, all optional and ignored by another terminal's parser:

```yaml
omt:
  kind: command            # command | prompt
  confirm: true            # require an explicit confirm before running (mobile-friendly)
  cwd: workspace           # workspace | pane | <path>
  arguments:
    branch:
      type: enum
      values: [main, develop, release]
```

`kind: prompt` sends the rendered text to the bound agent through
`agent.prompt` instead of the PTY — the equivalent of another terminal's `AgentMode`
workflow, expressed without needing a second top-level variant.

Load order (later shadows earlier, by `name`): built-in → user
`workflows/` → project `<repo>/.omt/workflows/`. Capabilities: `workflow.list`,
`workflow.get`, `workflow.run` (with `args: map<string,string>`),
`workflow.save`, `workflow.delete`. On a phone, `workflow.list` plus the
argument schema is what renders a form with a Run button — this is one of the
highest-value mobile affordances and it costs one YAML parser.

---

## 10. Config capabilities

Consistent with [03 — Capability catalog](03-capability-catalog.md); every one of
these is available in the TUI, over HTTP/WS, and as `omt config <verb>`.

| Capability | Kind | Role | Input → Output |
|---|---|---|---|
| `config.get` | Query | Viewer | `{ path?: string, layer?: Layer, resolved?: bool }` → `{ value, layer, file, span }` |
| `config.schema` | Query | Viewer | `{ section?: string }` → `{ settings: SettingDescriptor[], json_schema }` |
| `config.set` | Command | per-key `min_role` | `{ path, value, layer, if_version? }` → `{ applied, requires_restart, version }` |
| `config.unset` | Command | per-key | `{ path, layer }` → `{ applied }` |
| `config.validate` | Query | Viewer | `{ content?: string, file?: path, all?: bool }` → `{ ok, diagnostics[], summary }` |
| `config.reload` | Command | Operator | `{}` → `{ changed_keys[], pending_restart[] }` |
| `config.sources` | Query | Viewer | `{ path? }` → per-key provenance across all six layers |
| `config.default` | Query | Viewer | `{ annotated: bool }` → the default TOML document |
| `config.export` | Query | Admin | `{ layer }` → TOML text (secrets emitted as references, never values) |
| `config.import` | Command | Admin | `{ content, layer, dry_run }` → `{ diagnostics, changed_keys }` |
| `config.pending` | Query | Viewer | `{}` → settings awaiting a restart |
| `config.project.trust` | Command | Admin | `{ workspace, decision }` → `{}` |
| `theme.list` / `theme.get` / `theme.import` | Q/Q/C | Viewer/Admin | theme management |
| `keys.list` / `keys.conflicts` | Query | Viewer | resolved keymap + diagnostics |
| `workflow.*`, `launch.*` | mixed | Operator | §9 |

`effects` declarations: `config.set` carries `Effects::WRITES_FS`;
`config.set` on a `Restart`-class key additionally carries
`Effects::DESTRUCTIVE` when the change is lossy (scrollback shrink, store
backend change), which is what makes the mobile client require a confirm gesture
([03 §2](03-capability-catalog.md)).

---

## 11. Testing

| Property | Test |
|---|---|
| Every descriptor has a widget in both editors | `tests/surface_parity.rs` (§4.2) |
| Defaults round-trip | serialize `Config::default()` → parse → compare |
| Annotated default parses and validates clean | golden fixture |
| Every diagnostic code is documented | test scrapes `OMT-C###` from source and diffs against `docs/reference/diagnostics.md` |
| Layer precedence | table-driven test over all six layers per merge kind |
| `apply_config` reversibility | A→B→A equals A for every subsystem |
| Importers | fixture corpus of 20 another terminal YAML + 20 `.itermcolors` files, golden output |
| Parser robustness | `cargo-fuzz` target on the TOML→`Config` path (required by P5) |
| Comment preservation | property test: random `config.set` sequences never lose a comment |
| Schema stability | committed `schemas/config.schema.json` diffed in CI; a removed key or a narrowed type fails without a documented deprecation |

---

## 12. Open questions

- **OPEN QUESTION — project config trust granularity.** Per-repo trust is
  proposed. Should trust be per-key-class instead (e.g. always trust
  `terminal.*` from a project, always prompt for `agents.*`)? Per-repo is
  simpler; per-class is safer for `agents.*.command`, which is close to arbitrary
  code execution.
- **OPEN QUESTION — device-override storage for the TUI.** The web client uses
  `localStorage`. The TUI's device layer is `instances.toml`, which is also the
  instance-override source. If a user runs two TUIs on one machine against
  different instances, is "device" the machine or the process? Proposed: the
  machine; revisit if it bites.
- **OPEN QUESTION — should `keybindings.toml` be merged into `config.toml`?**
  Separate files are easier to share and to diff, but three files (config,
  keybindings, secrets) is already the practical maximum before discoverability
  suffers. Keeping them separate for now.
- **OPEN QUESTION — theme inheritance.** Should a theme be able to declare
  `extends = "nord"` and override a few roles? Useful for the agent-state
  colours on imported themes, but it adds a resolution order and a cycle check.
  Deferred until the importer corpus shows it is needed.
- **OPEN QUESTION — workflow argument types beyond `text`/`enum`.** another terminal has
  cloud-backed enums we cannot reproduce. Do we need `path` (with completion) and
  `session` types? Likely yes for mobile; not blocking v1.
- **OPEN QUESTION — hot-reload of `terminal.vt.*`.** Currently `Restart`.
  Making VT feature flags hot would require the parser to be swappable
  mid-stream, which is feasible but interacts with the block model; deferred to
  [04 — Terminal core](04-terminal-core.md).
- **OPEN QUESTION — `secrets.toml` vs. keychain as the default.** The provider
  chain supports both. Should the OS keychain be the *default* target for
  `omt auth token create` on macOS/Windows, with `secrets.toml` as the fallback?
  Better security, worse portability of a dotfiles setup.
