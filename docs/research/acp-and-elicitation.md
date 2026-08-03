# ACP and Elicitation: A Spec-Grade Reference for omt

Research date: **2026-08-03**. Sources: `agentclientprotocol.com` (v1 stable docs +
`llms-full.txt`), the `agentclientprotocol/agent-client-protocol` schema repo
(`schema/v1/schema.json`, `schema/v2/schema.json`, `meta.json`), the
`agentclientprotocol/rust-sdk` repo, the ACP registry manifest
(`https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json`), the MCP draft
elicitation spec, and **live handshakes and full prompt turns run locally against
`opencode acp` (v1.18.9) and `gemini --acp` (v0.46.0)**. Every JSON block marked
*captured* is verbatim wire data from those runs, not reconstructed from the schema.

This supersedes [`agent-clis.md` §11](agent-clis.md), which is thin and, in three places,
now wrong. Corrections are flagged inline as **[CORRECTION]**.

---

## 0. Executive orientation — read this first

Five facts that change omt's plan:

1. **ACP is far bigger than the four agents [11 §11](agent-clis.md) names.** The
   ecosystem is ~40 agents, including *official-adapter* coverage for **Claude
   (`@agentclientprotocol/claude-agent-acp`, 2.3k★, authored by Anthropic + Zed +
   JetBrains)**, **Codex (`@agentclientprotocol/codex-acp`, authored by OpenAI + Zed +
   JetBrains)**, **Cursor (`cursor-agent acp`, first-party)**, and **GitHub Copilot
   (`@github/copilot --acp`)**. The generic ACP adapter is not a 4-agent play; it is
   potentially a 10+-agent play. **[CORRECTION]**
2. **ACP v2 exists, in Draft since 2026-07-20**, and it changes things omt's design
   depends on: `session/prompt` no longer returns the stop reason, terminals invert
   direction, `fs/*` and `terminal/*` are gone from the client method set, and there is a
   new `state_update` notification (`running` / `idle` / `requires_action`) that is
   *literally* omt's `AgentState`. Build against v1, negotiate both. §8.
3. **ACP has its own `elicitation/create`**, adapted from MCP, stable since before the
   research date. It is the missing `InteractionKind::Choice` path for non-Claude agents.
   **[CORRECTION]** — [12 §12.3](agent-clis.md) claims `Choice` is "a Claude-Code-only
   capability today". That is no longer true in principle; it is still true in practice
   because no agent I probed emits it yet. §6.
4. **`session/request_permission` is confirmed live and is exactly as good as hoped.**
   Verbatim capture in §4.3. It is a blocking bidirectional JSON-RPC request with no
   timeout, no expiry, and no cancellation except client-initiated — the ideal shape for
   a phone round-trip. This is materially *better* than Claude Code's unverified
   `permissionDecision: "defer"` ([06 §5.3](../architecture/06-agent-layer.md#53-the-deferral-mechanism-and-its-risk)).
5. **The TUI tension is real and it is not resolvable.** An ACP-mode agent has no TUI. §10
   is the honest analysis and the recommendation.

---

## 1. Transport, framing, conventions

- **JSON-RPC 2.0 over the agent subprocess's stdin/stdout**, newline-delimited
  (one JSON object per line; no Content-Length framing, unlike LSP). Confirmed empirically
  — a bare `{...}\n` written to `opencode acp`'s stdin got a response.
- The agent's **stderr is free-form logging**, not protocol. omt should capture it for
  `agent.explain`.
- Two message kinds: **Methods** (request/response) and **Notifications** (no `id`, no
  response).
- **Bidirectional.** The agent issues requests *back* to the client on the same pipe.
  The two directions have **independent `id` spaces** — captured: opencode sent its first
  `session/request_permission` with `"id": 0` while the client's `id: 0` (`initialize`)
  had long since been answered. An omt implementation MUST NOT share a request map
  between directions.
- **All file paths MUST be absolute.** Line numbers are **1-based**.
- Property keys are `camelCase`; discriminator *values* are `snake_case`; JSON-RPC
  envelope fields per JSON-RPC 2.0.
- `_meta` is reserved on essentially every object for extension data. Custom methods are
  prefixed `_`. Custom capabilities go in `_meta` during `initialize`.
- Transport is not actually pinned to stdio in the SDK: `agent-client-protocol-http`
  offers HTTP/SSE and WebSocket transports. Relevant to [07 — Remote Protocol](../architecture/07-remote-protocol.md)
  if omt ever wants a daemon-hosted agent on another machine.

---

## 2. Initialization and capability negotiation

### 2.1 `initialize` (client → agent)

Request:

```json
{
  "jsonrpc": "2.0",
  "id": 0,
  "method": "initialize",
  "params": {
    "protocolVersion": 1,
    "clientCapabilities": {
      "fs": { "readTextFile": true, "writeTextFile": true },
      "terminal": true,
      "elicitation": { "form": {}, "url": {} },
      "session": { "configOptions": {} }
    },
    "clientInfo": { "name": "omt", "title": "oh-my-term", "version": "0.1.0" }
  }
}
```

Response — **captured verbatim from `opencode acp` v1.18.9**:

```json
{
  "jsonrpc": "2.0",
  "id": 0,
  "result": {
    "protocolVersion": 1,
    "agentCapabilities": {
      "loadSession": true,
      "mcpCapabilities": { "http": true, "sse": true },
      "promptCapabilities": { "embeddedContext": true, "image": true },
      "sessionCapabilities": { "close": {}, "fork": {}, "list": {}, "resume": {} }
    },
    "authMethods": [
      { "id": "opencode-login", "name": "Login with opencode",
        "description": "Run `opencode auth login` in the terminal" }
    ],
    "agentInfo": { "name": "OpenCode", "version": "1.18.9" }
  }
}
```

**Captured from `gemini --acp` v0.46.0** (auth methods elided):

```json
{
  "protocolVersion": 1,
  "agentInfo": { "name": "gemini-cli", "title": "Gemini CLI", "version": "0.46.0" },
  "agentCapabilities": {
    "loadSession": true,
    "promptCapabilities": { "image": true, "audio": true, "embeddedContext": true },
    "mcpCapabilities": { "http": true, "sse": true }
  }
}
```

### 2.2 Versioning rules

`protocolVersion` is a **single integer** naming a MAJOR version, bumped only on breaking
changes. Client sends the latest it supports; if the agent supports it, the agent MUST
echo it; otherwise the agent MUST return the latest *it* supports, and the client SHOULD
disconnect if it can't speak that. Non-breaking features arrive as capabilities, never as
version bumps. Omitted capability ⇒ **UNSUPPORTED**, always.

### 2.3 The full v1 capability surface

| Path | Type | Gates |
|---|---|---|
| `clientCapabilities.fs.readTextFile` | bool | `fs/read_text_file` |
| `clientCapabilities.fs.writeTextFile` | bool | `fs/write_text_file` |
| `clientCapabilities.terminal` | bool | **all** `terminal/*` methods |
| `clientCapabilities.elicitation.form` | `{}`\|null | form-mode `elicitation/create` |
| `clientCapabilities.elicitation.url` | `{}`\|null | URL-mode `elicitation/create` |
| `clientCapabilities.session.configOptions` | obj\|null | `config_option_update`, `session/set_config_option` |
| `agentCapabilities.loadSession` | bool | `session/load` |
| `agentCapabilities.promptCapabilities.image` | bool | `ContentBlock::Image` in prompts |
| `agentCapabilities.promptCapabilities.audio` | bool | `ContentBlock::Audio` in prompts |
| `agentCapabilities.promptCapabilities.embeddedContext` | bool | `ContentBlock::Resource` in prompts |
| `agentCapabilities.mcpCapabilities.{http,sse}` | bool | non-stdio MCP server configs |
| `agentCapabilities.sessionCapabilities.{list,delete,resume,close,fork,additionalDirectories}` | `{}`\|null | the matching methods |
| `agentCapabilities.auth.logout` | `{}`\|null | `logout` |

Note the deliberate asymmetry in elicitation: ACP requires **each mode to be explicitly
present and non-null**, unlike MCP where a bare `{}` means form-only. `{}` in ACP
advertises *nothing*. Agents MUST NOT request an unadvertised mode; doing so is `-32602`.

**Baseline** (no capability needed): agent MUST support `initialize`, `session/new`,
`session/prompt`, `session/cancel`; client MUST support `session/update` and
`session/request_permission`.

---

## 3. Agent-side methods (client → agent)

### 3.1 `authenticate`

`{"methodId": "<AuthMethodId>"}` → `{}`. Called when the agent returned `-32000`
(Authentication required) or when the client wants to pre-authenticate against a
`authMethods[]` entry from `initialize`. Note `AuthMethod` entries carry `_meta` — Gemini
uses it to say `{"api-key": {"provider": "google"}}` and
`{"gateway": {"protocol": "google", "restartRequired": "false"}}`. Real, and useful for a
remote auth UI.

Live example of the failure path, **captured** from `gemini --acp` on this machine:

```json
{"jsonrpc":"2.0","id":1,"error":{"code":-32000,
 "message":"This client is no longer supported for Gemini Code Assist for individuals. …"}}
```

Which is the concrete demonstration that omt must render agent errors verbatim rather
than mapping them: the code is "auth required" but the message is a product deprecation.

### 3.2 `session/new`

```json
{ "jsonrpc": "2.0", "id": 1, "method": "session/new",
  "params": { "cwd": "/abs/path", "mcpServers": [], "additionalDirectories": ["/abs/other"] } }
```

Response — **captured** (opencode, truncated to one option):

```json
{ "jsonrpc": "2.0", "id": 1, "result": {
  "sessionId": "ses_0390fb5deffeVTYrjRSyL6ZfHu",
  "configOptions": [
    { "id": "model", "name": "Model", "category": "model", "type": "select",
      "currentValue": "opencode/big-pickle",
      "options": [ { "value": "anthropic/claude-opus-5", "name": "Anthropic/Claude Opus 5" } ] }
  ] } }
```

`sessionId` is the **`AgentSessionId`** that [06 §2](../architecture/06-agent-layer.md#2-the-two-axis-model)'s
`AgentBinding.agent_session` wants. In ACP mode omt gets it for free, immediately, with
no inference.

`configOptions` is a genuine bonus not mentioned in [11 §11](agent-clis.md): a typed
model/mode selector with current value and full option list, remotely settable via
`session/set_config_option`. That is a shipped feature for omt's web client with zero
per-agent work. opencode also exposes a `mode` option ("Sisyphus - Ultraworker" etc.).

### 3.3 `session/load` (cap: `loadSession`) and `session/resume` (cap: `sessionCapabilities.resume`)

`session/load` takes `{sessionId, cwd, mcpServers, additionalDirectories?}` and returns
`{modes?, configOptions?}`. **Critically: the agent MUST replay the entire conversation as
`session/update` notifications before returning.** That is how omt can attach to a prior
session and populate its transcript view.

`session/resume` is the newer, lighter variant. In v2 it gains `replayFrom`, an inclusive
cursor so a client can ask for replay from the start or from a point.

### 3.4 `session/prompt`

```json
{ "jsonrpc": "2.0", "id": 2, "method": "session/prompt",
  "params": { "sessionId": "ses_…",
    "prompt": [
      { "type": "text", "text": "Can you analyze this code for potential issues?" },
      { "type": "resource", "resource": {
          "uri": "file:///home/user/project/main.py", "mimeType": "text/x-python",
          "text": "def process_data(items):\n    for item in items:\n        print(item)" } }
    ] } }
```

Response is `{"stopReason": "end_turn" | "max_tokens" | "max_turn_requests" | "refusal" | "cancelled"}`.
opencode adds a non-standard `usage` object and an empty `_meta`; both are legal.

**In v1 this response does not arrive until the entire turn is over** — it is the turn's
completion signal. This is the single most important structural fact about v1 for omt: the
whole turn is one outstanding request.

### 3.5 `session/cancel` (notification)

`{"sessionId": "ses_…"}`. No response. Semantics, verbatim from the spec:

- Client SHOULD preemptively mark all unfinished tool calls `cancelled` on send.
- Client **MUST** respond to all pending `session/request_permission` requests with the
  `cancelled` outcome.
- Agent SHOULD stop model requests and tool invocations ASAP.
- Agent **MUST** eventually answer the original `session/prompt` with
  `stopReason: "cancelled"` — *not* a JSON-RPC error. The spec explicitly warns agents to
  catch abort exceptions so cancellation is never surfaced as an error.
- Agent MAY keep sending `session/update` after receiving cancel, but MUST flush them
  before responding. Client SHOULD accept them.

### 3.6 `session/set_mode` / `session/set_config_option` / `session/list` / `session/delete` / `session/close` / `logout`

All capability-gated. `session/list` returns `SessionInfo[]` (`sessionId`, `cwd`,
`additionalDirectories`, `title`, `updatedAt`) — a ready-made session picker for omt's
web client, supported today by opencode.

---

## 4. Client-side methods (agent → client) — what omt implements

### 4.1 `fs/read_text_file`

```json
{"jsonrpc":"2.0","id":7,"method":"fs/read_text_file",
 "params":{"sessionId":"ses_…","path":"/abs/file.rs","line":10,"limit":40}}
```
→ `{"content": "…"}`. `line` is 1-based. The point of routing this through the client is
that the *editor* may hold unsaved buffer state the agent can't see on disk. For omt this
is mostly a passthrough, but it is the natural hook for [15 — Workspace Explorer](../architecture/15-workspace-explorer.md)
to serve the agent from omt's own view of the tree.

### 4.2 `fs/write_text_file`

`{sessionId, path, content}` → `{}`. Creates the file (and parents) if absent.

### 4.3 `session/request_permission` — the flagship path

**This is the mechanism omt renders remotely. Captured verbatim from a live `opencode acp`
turn**, with `opencode.json` set to `{"permission":{"bash":"ask","edit":"ask"}}`:

```json
{ "jsonrpc": "2.0", "id": 0, "method": "session/request_permission",
  "params": {
    "sessionId": "ses_039089924ffeCxstHAX4uLcwpW",
    "toolCall": {
      "toolCallId": "call_00_Ie9rtNeeUbIT6eUxYnWW1832",
      "title": "echo hello",
      "kind": "execute",
      "status": "pending",
      "locations": [],
      "rawInput": { "command": "echo hello" }
    },
    "options": [
      { "optionId": "once",   "kind": "allow_once",   "name": "Allow once" },
      { "optionId": "always", "kind": "allow_always", "name": "Always allow" },
      { "optionId": "reject", "kind": "reject_once",  "name": "Reject" }
    ] } }
```

The client answers with exactly one of two outcome shapes:

```json
{ "jsonrpc": "2.0", "id": 0, "result": { "outcome": { "outcome": "selected", "optionId": "once" } } }
{ "jsonrpc": "2.0", "id": 0, "result": { "outcome": { "outcome": "cancelled" } } }
```

**Option kinds** (`PermissionOptionKind`, v1 closed enum; v2 open):
`allow_once`, `allow_always`, `reject_once`, `reject_always`.
These map **1:1** onto omt's `PermissionOptionKind::{Allow, AllowAlways, Deny, DenyAlways}`.
omt's fifth variant, `Edit`, has no ACP equivalent — see §7.

**Properties that matter to omt, each verified or spec-stated:**

- **No timeout exists anywhere in the protocol.** Not on this method, not generally. The
  request stays outstanding until the client answers or the turn is cancelled. omt's
  phone round-trip is bounded only by omt's own policy. This is strictly better than
  Claude Code's `defer`, whose parking duration is [06 §5.3](../architecture/06-agent-layer.md#53-the-deferral-mechanism-and-its-risk)'s
  named unverified risk.
- **Cancellation** arrives one of two ways: the client sends `session/cancel` and must
  then answer pending permission requests with `outcome: "cancelled"`; or the *agent*
  sends `$/cancel_request` with this request's `id`, to which the client responds with
  either a valid result or error `-32800`. Both must be wired into omt's ledger as an
  `InteractionState::Cancelled` transition — this is the concrete source of
  [12 §4](../architecture/12-collaboration.md#4-interaction-ownership)'s external-cancel case.
- **`toolCall` is a `ToolCallUpdate`**, so the same object shape omt already models for
  tool calls. `rawInput` is the verbatim tool input that [06 §5](../architecture/06-agent-layer.md#5-interactions--the-flagship-path)'s
  `Permission { input }` requires; `locations[]` gives the affected paths for the file
  card; the tool's `content[]` may carry a `diff` for the edit card.
- **The options list is the agent's own, in the agent's order** — which is precisely what
  [D1](../architecture/decisions.md#d1--omt-adds-no-policy-layer-over-an-agents-permission-semantics)
  requires omt to pass through unmodified. ACP is architecturally aligned with D1.
- **Concurrency:** nothing forbids multiple simultaneous outstanding permission requests
  in one session. omt's ledger must key on the JSON-RPC request id, not on session.

### 4.4 `terminal/*` — the methods omt could serve better than any editor

Capability-gated behind a single boolean `clientCapabilities.terminal`; it is all-or-nothing.

| Method | Params | Result |
|---|---|---|
| `terminal/create` | `{sessionId, command, args?, env?: [{name,value}], cwd?, outputByteLimit?}` | `{terminalId}` |
| `terminal/output` | `{sessionId, terminalId}` | `{output, truncated, exitStatus?}` |
| `terminal/wait_for_exit` | `{sessionId, terminalId}` | `{exitCode?, signal?}` |
| `terminal/kill` | `{sessionId, terminalId}` | `{}` |
| `terminal/release` | `{sessionId, terminalId}` | `{}` |

`ToolCallContent` has a `{"type":"terminal","terminalId":"…"}` variant, so a tool call can
*embed a live terminal by reference* and the client renders streaming output inline.

**This is genuinely omt's home turf**, and it is the strongest strategic argument in this
document. Every other ACP client is an editor faking a terminal in a panel. omt has a real
PTY, a real parser ([04 — Terminal Core](../architecture/04-terminal-core.md)), real
scrollback, real resize, real semantic zones ([18](../architecture/18-semantic-open.md)).
An omt ACP client can implement `terminal/create` by **spawning the command in a real omt
pane** — the user watches the agent's build run in a first-class terminal, can scroll it,
search it, and the agent's `terminal/output` reads come from the same buffer. No other
client can do that. If omt ships an ACP client at all, this is the differentiating feature,
not permission cards.

Caveat: `outputByteLimit` and `truncated` imply a byte-capped ring buffer, which omt has;
and `terminal/release` means the client owns the lifetime, which fits pane lifetime.

### 4.5 `elicitation/create` and `elicitation/complete`

See §6.

### 4.6 `session/update` (notification) — see §5

---

## 5. `session/update` variants, complete

`SessionNotification` is `{sessionId, update}` where `update` carries a `sessionUpdate`
discriminator. **v1 has exactly eleven variants.** [11 §11](agent-clis.md) lists five.
**[CORRECTION]**

| `sessionUpdate` | Payload | Notes |
|---|---|---|
| `user_message_chunk` | `ContentChunk` | echo of the user message; how load/resume replay works |
| `agent_message_chunk` | `ContentChunk` | `{content: ContentBlock, messageId?}` |
| `agent_thought_chunk` | `ContentChunk` | reasoning stream |
| `tool_call` | `ToolCall` | first announcement |
| `tool_call_update` | `ToolCallUpdate` | patch by `toolCallId` |
| `plan` | `{entries: PlanEntry[]}` | full replacement each time |
| `available_commands_update` | `{availableCommands: AvailableCommand[]}` | slash commands |
| `current_mode_update` | `{currentModeId}` | |
| `config_option_update` | `{configOptions: SessionConfigOption[]}` | full set, current values |
| `session_info_update` | `{title?, updatedAt?}` | partial patch; `null` clears |
| `usage_update` | `{used, size, cost?: {amount, currency}}` | tokens + money |

### 5.1 Content chunks — captured

```json
{"jsonrpc":"2.0","method":"session/update","params":{
  "sessionId":"ses_0390abfe9ffejOtWnHlTJRlU3P",
  "update":{"sessionUpdate":"agent_thought_chunk",
    "messageId":"msg_fc6f60d8b0019hZGXNrKYBFDzZ",
    "content":{"type":"text","text":"The"}}}}
```

Chunking is **per-token** in practice: a single 2-tool-call turn produced **3,463
notifications**, of which 1,925 were `agent_thought_chunk` and 1,529 were
`agent_message_chunk`, many carrying a single word or fragment (`"ance"`, `"("`, `"-"`).

**Implication for omt, and it is not small.** [07 — Remote Protocol](../architecture/07-remote-protocol.md)
cannot forward these 1:1 to a phone. omt's ACP source must **coalesce by `messageId`** on
a time budget (~50 ms) before broadcasting, exactly as a terminal renderer coalesces
writes. `messageId` is the correct coalescing key: same id ⇒ same message, changed id ⇒
new message. Gemini and opencode both emit it.

### 5.2 `tool_call` / `tool_call_update` — captured

```json
{"sessionUpdate":"tool_call","toolCallId":"call_00_ET_4oqJ6g3XnsymfuANJLXe1002",
 "title":"bash","kind":"execute","status":"pending",
 "locations":[{"path":"/tmp/acpwork"}],"rawInput":{"cwd":"/tmp/acpwork"}}
```
```json
{"sessionUpdate":"tool_call_update","toolCallId":"call_00_ET_…","status":"in_progress",
 "kind":"execute","title":"ls -la","locations":[{"path":"/tmp/acpwork"}],
 "rawInput":{"command":"ls -la","cwd":"/tmp/acpwork"}}
```
```json
{"sessionUpdate":"tool_call_update","toolCallId":"call_00_ET_…","status":"completed",
 "title":"ls -la",
 "content":[{"type":"content","content":{"type":"text","text":"total 8\ndrwxr-xr-x@ …"}}],
 "rawOutput":{"output":"total 8\n…","metadata":{"exit":0,"truncated":false}}}
```

Note the **streaming `rawInput`**: the first `tool_call` had only `{"cwd":…}` because the
model hadn't finished emitting the `command` argument. omt's permission card must not
render until `rawInput` is settled — in practice, until `session/request_permission`
arrives, which carries the complete input.

`ToolKind`: `read | edit | delete | move | search | execute | think | fetch | switch_mode | other`.
`ToolCallStatus`: `pending | in_progress | completed | failed`.
`ToolCallContent`: `{type:"content", content: ContentBlock}` | `{type:"diff", path, oldText?, newText}` | `{type:"terminal", terminalId}`.

The `diff` variant maps directly onto [15 §3.2](../architecture/15-workspace-explorer.md#32-vcs-model)'s
`FileDiff`, satisfying [06 §5](../architecture/06-agent-layer.md#5-interactions--the-flagship-path)'s
`Permission { diff }` with no reconstruction. (v1's `oldText`/`newText` is a full-file
before/after; omt computes the hunks itself.)

### 5.3 `available_commands_update` — captured

```json
{"sessionUpdate":"available_commands_update","availableCommands":[
  {"name":"cancel-ralph","description":"(builtin) Cancel active Ralph Loop"},
  {"name":"dcp-compress","description":"Trigger DCP manual compression with: /dcp-compress [focus]"}]}
```

22 commands, including this machine's user-defined ones. This is exactly the slash-command
discovery [11 §11](agent-clis.md) hoped for, and it works. `AvailableCommandInput` is
currently only `{"type":"unstructured","hint":"…"}` — the free-text tail after the command
name. There is no typed-argument variant in v1.

Timing note: opencode emitted this **after** `session/prompt`, not after `session/new`.
omt cannot assume commands are known at session start; the UI must populate lazily.

### 5.4 `usage_update` — captured

```json
{"sessionUpdate":"usage_update","used":0,"size":200000,"cost":{"amount":0,"currency":"USD"}}
```

`used`/`size` required; `cost` optional, ISO 4217. This fills [12 §12.2](agent-clis.md)'s
`Usage` event portably — for Gemini and Qwen it resolves an **UNCERTAIN** in
[12 §12.3](agent-clis.md).

---

## 6. Elicitation: MCP's, ACP's, and how they differ

### 6.1 MCP `elicitation/create` (draft, 2026-07-28 RC)

Server → client. Two modes.

**Form mode.** `{mode: "form"|omitted, message, requestedSchema}`. The schema is a
deliberately **restricted subset**: a *flat object* whose properties are primitives only.
Permitted property schemas:

- **string**: `title`, `description`, `minLength`, `maxLength`, `pattern`, `format`
  (`email` | `uri` | `date` | `date-time`), `default`
- **number** / **integer**: `title`, `description`, `minimum`, `maximum`, `default`
- **boolean**: `title`, `description`, `default`
- **single-select enum**: `type: "string"` + `enum: [...]`, or + `oneOf: [{const,title,description?}]`
- **multi-select enum**: `type: "array"`, `minItems`, `maxItems`, `items: {type:"string", enum:[...]}`
  or `items: {anyOf: [{const,title}]}`, `default: [...]`

Nested objects, arrays of objects, `allOf`, `$ref`, and every other JSON-Schema feature are
**intentionally excluded** so any client can render a form.

**URL mode.** `{mode:"url", message, url}`. For OAuth, payments, credentials — anything
that must not transit the client or the model context.

**Response** — a three-action model:

```json
{ "action": "accept",  "content": { "strategy": "balanced", "confirm": true } }
{ "action": "decline" }
{ "action": "cancel" }
```

`accept` = user submitted; `decline` = explicit refusal; `cancel` = dismissed without
choosing (Esc, click-away, load failure). `content` is meaningful only on `accept`.

Note a live divergence: **in the current MCP draft, elicitation is delivered inside an
`InputRequiredResult` under the multi-round-trip-request (MRTR) pattern** — the server
returns the request as a *result*, the client re-issues the original `tools/call` carrying
the response. It is no longer a plain server→client JSON-RPC request. ACP did **not**
adopt MRTR.

### 6.2 ACP `elicitation/create` — the differences

ACP took MCP's data model and made four changes, all of which favour omt:

1. **It remains a direct agent→client JSON-RPC request** on the ACP connection. No MRTR,
   no retry dance. One request, one response, blocking — the same shape as
   `session/request_permission`, so omt uses one code path.
2. **`mode` is required.** No form-by-default.
3. **Capability handshaking is stricter**: each mode must be explicitly present and
   non-null. `{}` advertises nothing (MCP: `{}` means form).
4. **URL mode keeps `elicitationId` and adds `elicitation/complete`**, a notification the
   agent sends when the out-of-band flow finishes. MCP dropped this. It means omt can
   *close the card* on the phone when the OAuth actually completes, rather than leaving a
   "did you finish?" dangling.

Scope is flattened onto the params — exactly one of:
- `sessionId` (+ optional `toolCallId`) — session-scoped
- `requestId` — request-scoped, for pre-session auth/config phases

```json
{ "jsonrpc": "2.0", "id": 43, "method": "elicitation/create",
  "params": {
    "sessionId": "sess_abc123",
    "mode": "form",
    "message": "How should I approach this refactoring?",
    "requestedSchema": {
      "type": "object",
      "properties": {
        "strategy": { "type": "string", "enum": ["conservative", "balanced", "aggressive"] }
      },
      "required": ["strategy"] } } }
```

```json
{ "jsonrpc": "2.0", "id": 44, "method": "elicitation/create",
  "params": { "requestId": 12, "mode": "url", "elicitationId": "github-oauth-001",
    "url": "https://agent.example.com/connect?elicitationId=github-oauth-001",
    "message": "Please authorize access to your repositories." } }
```

**Client obligations omt must honour** (these are MUSTs, and several bite a *remote* client
specifically):

- Clearly identify which agent is asking.
- Provide clear **decline** and **cancel** controls, distinct from each other.
- Form mode: let the user review and modify before sending; pre-populate `default`s;
  SHOULD validate against the schema client-side.
- URL mode: **show the full URL**, highlight the domain, warn on Punycode, never prefetch,
  never open without explicit consent, and **open in a context the client and the model
  cannot inspect**.
- Never render URLs as clickable outside a URL-mode request's `url` field.
- Form mode MUST NOT be used for secrets. If the client lacks URL mode, the agent must
  fail rather than downgrade.

The URL-mode requirements are the awkward ones for omt. "Open in a secure context the
client cannot inspect" is straightforward in the phone's system browser and *impossible*
in a terminal. **Recommendation: omt advertises `elicitation: {form: {}}` only, from the
TUI, and adds `url: {}` only when the resolving surface is the web client.** Capabilities
are per-connection, so this is a clean per-adapter-instance decision.

---

## 7. Mapping onto omt's `InteractionKind`

Against [06 §5](../architecture/06-agent-layer.md#5-interactions--the-flagship-path).

| Source | omt `InteractionKind` | Fidelity | Where it's lossy |
|---|---|---|---|
| ACP `session/request_permission` | `Permission { tool, input, command, diff, options }` | **Native, lossless** | `PermissionOptionKind::Edit` has no ACP counterpart — omt must not synthesise it. `title`/`description` (v2) have nowhere to live in omt's struct. |
| ACP `elicitation/create` form, single `oneOf`/`enum` string prop | `Choice { questions: [one] }` | **Native, near-lossless** | ACP has no `header` (omt's 12-char tab label) — omt must derive one from `title`, truncated. ACP has no `allow_free_text`; omt's synthetic "Other…" row would submit a value the schema rejects. |
| ACP `elicitation/create` form, `type:"array"` + enum items | `Choice { multi_select: true }` | Native | `minItems`/`maxItems` are not expressible in `ChoiceQuestion`; omt must enforce them itself or drop the constraint. |
| ACP `elicitation/create` form, N properties | `Choice { questions: [N] }` | **Lossy** | omt's `Choice` is N *independent* questions; an elicitation form is one atomic submission with a `required[]` set. omt has no "required" concept and no all-or-nothing submit. |
| ACP form, `string` with no enum | `Text { prompt, placeholder, multiline }` | Native | `minLength`/`maxLength`/`pattern`/`format` are all dropped. `format: "email"` becomes an unvalidated text box. |
| ACP form, `number`/`integer`/`boolean` | **no mapping** | **Broken** | omt has no numeric or boolean kind. Falls back to `Text` and string-parses, or the adapter declines. See below. |
| ACP `elicitation/create` URL mode | **no mapping** | **Broken** | omt has no `InteractionKind` for "open this URL and consent". |
| MCP `elicitation/create` (via an MCP server omt hosts) | same as ACP rows | — | Plus: MCP's MRTR delivery means the *agent*, not omt, mediates. omt likely never sees it. |
| Claude Code `AskUserQuestion` | `Choice` | **Native, 1:1** | None — omt's `Choice` was designed from this schema. |
| Claude Code plan-mode approval | `PlanReview { plan }` | Native | ACP has **no** plan-approval interaction. ACP `plan` updates are informational only; there is no way for a client to reject a plan. |
| PTY heuristics | `Permission` / `Text`, `fidelity: synthetic` | Degraded | Per [D3](../architecture/decisions.md#d3--synthetic-input-is-bounded-by-state-dependence-not-by-tool-danger). |

### Three concrete gaps and what to do about them

**(a) `InteractionKind` has no form.** Elicitation's atomic multi-field form with a
`required[]` set does not fit `Choice`, which is a list of independent questions.
Recommendation: **add `InteractionKind::Form { message, fields: Vec<FormField>, required: Vec<String> }`**
to [06 §5](../architecture/06-agent-layer.md#5-interactions--the-flagship-path), with
`FormField` covering string/number/integer/boolean/enum/multi-enum — i.e. mirror the
restricted subset directly, since MCP and ACP have already agreed on it and it is
deliberately small. Corresponding `InteractionResponse::Form { action, content }` with
`action: accept|decline|cancel`. Trying to squeeze this into `Choice` will produce a
renderer that is wrong for both.

**(b) `InteractionResponse` has no decline/cancel distinction.** omt currently has
`InteractionState::Cancelled` for *omt-side* cancellation and no way to express "the user
explicitly declined". Elicitation requires all three. Recommendation: make
`decline` a first-class response, not a state.

**(c) `PermissionOptionKind::Edit`.** Present in omt, absent from ACP v1 (and v2 keeps the
same four plus an open string). It exists for Claude Code. Keep it, but the ACP adapter
must never emit it — per D1, omt passes the agent's options through and adds nothing.

---

## 8. ACP v2 (Draft, published 2026-07-20) — what changes, and why omt should care

v2 is a Draft. The maintainers are explicit: *"Don't ship it by default in production
until we are closer to stabilization"*, and *"Adding v2 support should not mean dropping
v1"*. Build v1, negotiate both. But three v2 changes bear directly on omt's design and
one of them should influence v1 code today.

**8.1 The prompt turn is gone.** `PromptResponse` is now an **empty acknowledgement** —
"the message was accepted", not "the turn is over". The stop reason moves into a new
notification:

```
state_update → { state: "running" }
             | { state: "idle", stopReason?: StopReason }
             | { state: "requires_action" }
```

This is, almost verbatim, omt's own `AgentState`
([06 §4](../architecture/06-agent-layer.md)): `Working` / `Idle` / `Blocked`. ACP v2
converged on omt's model independently. **`requires_action` is a first-class
"needs you" signal from the agent itself** — the exact thing [06 §1](../architecture/06-agent-layer.md#1-what-this-layer-must-deliver)
says omt exists to surface across twenty sessions. That is a strong validation of the
architecture, and a reason to make omt's ACP source structurally ready for it: **do not
build the v1 adapter such that "turn over" is synonymous with "prompt response returned"**,
because in v2 it isn't.

It also legalises what v1 left ambiguous: updates outside a turn, queueing, steering,
background work, and multiple clients observing one session.

**8.2 `fs/*` and `terminal/*` are removed from the client method set.** v2's
`clientMethods` are exactly `session/request_permission`, `session/update`,
`elicitation/create`, `elicitation/complete`. Terminals **invert**: the *agent* now owns
them and reports them to the client via new notifications:

```
terminal_update       → { terminalId, command?, cwd?, output?: {data: <base64>}, exitStatus? }
terminal_output_chunk → { terminalId, data: <base64> }
```

`ToolCallContent::Terminal` becomes "a display-only reference to an agent-owned terminal."

**This is bad news for §4.4's strategic argument.** In v1, omt can *host* the agent's
command in a real omt pane. In v2, the agent runs it and streams bytes at omt, and omt is
a viewer. The differentiator degrades from "we execute your agent's commands in a first-
class terminal" to "we render base64 chunks nicely". File access moves to MCP-over-ACP
(`mcp/connect`, `mcp/message`, currently unstable). Worth watching; worth raising in the
ACP RFD process if omt cares, because omt is the one client for whom the v1 direction was
strictly better.

**8.3 Permission requests get richer.** v2 adds a required `title`, an optional
`description`, and an extensible `subject`:

```json
{ "sessionId": "…", "title": "Run `cargo test`?",
  "description": "This will execute the test suite in the workspace.",
  "subject": { "type": "command", "command": "cargo test",
               "cwd": "/repo", "toolCallId": "call_1", "terminalId": "term_2" },
  "options": [ … ] }
```

`subject` is `tool_call` | `command` | open string. A dedicated `command` subject is
precisely what omt's exec-shaped permission card wants and removes the need to sniff
`rawInput` for a command string. **Recommendation: add `title`/`description` to omt's
`InteractionKind::Permission` now**, optional, so the v2 adapter has somewhere to put them
and v1 leaves them `None`.

**8.4 Other v2 deltas** worth noting: diffs become structured `DiffChange[]`
(add/delete/modify/move/copy, plus `binary`/`directory`/`symlink` file types) with an
optional `git_patch` — much better for [15](../architecture/15-workspace-explorer.md);
messages and tool-call content stream and *patch* by stable id with uniform semantics
(omit = unchanged, `null` = clear, value = replace, chunk = append); every enum is open
with `_`-prefixed extension values; `authenticate` → `auth/login`; `session/load` → merged
into `session/resume` with a `replayFrom` cursor.

---

## 9. Who actually implements ACP, and how completely

Launch commands below are taken from the **official registry manifest**
(`cdn.agentclientprotocol.com/registry/v1/latest/registry.json`, fetched 2026-08-03) —
these are authoritative and CI-verified, since the registry requires a valid `authMethods`
handshake for inclusion.

| Agent | Launch | Verified how | Status |
|---|---|---|---|
| **opencode** 1.18.9 | `opencode acp` | **Probed live, full turn** | **Excellent.** v1. `loadSession`, `image`, `embeddedContext`, `sessionCapabilities: {close, fork, list, resume}`, `configOptions` (model + mode). Emits `available_commands_update`, `usage_update`, `tool_call`, `tool_call_update`, `agent_message_chunk`, `agent_thought_chunk`, `messageId`. Real `session/request_permission` with 3 options. **No `audio`.** Did not emit `plan` in my runs. |
| **Gemini CLI** 0.46.0 | `gemini --acp` (`--experimental-acp` still aliased) | **Probed live (handshake; prompt blocked by account policy)** | **Good.** v1. `loadSession`, **`image` + `audio` + `embeddedContext`** — the only agent I found advertising audio. Four `authMethods` with `_meta` hints. **No `sessionCapabilities`** ⇒ no `list`/`resume`/`fork`/`close`. Dedicated `packages/cli/src/acp/` with session manager, RPC dispatcher, FS service, resume support, command handler — a serious implementation. |
| **Qwen Code** 0.21.4 | `npx @qwen-code/qwen-code --acp --experimental-skills` | Registry manifest | Gemini-CLI fork; inherits the ACP layer. Note the registry ships `--experimental-skills` alongside. **Not probed** (not installed). Assume Gemini-shaped, verify before relying. |
| **Goose** 1.45.0 | `goose acp` (binary distribution) | Registry manifest | **[CORRECTION]** — [11 §11](agent-clis.md) says `goose-acp-server`; the current invocation is `goose acp`. **Not probed.** |
| **Claude** 0.64.2 | `npx @agentclientprotocol/claude-agent-acp` | Registry + repo README | **Official-org adapter**, authors listed as *Anthropic, Zed Industries, JetBrains*, 2.3k★. Supports @-mentions, **images**, tool calls with permission requests, following, edit review, TODO lists, custom slash commands, client MCP servers, nested subagent transcripts (opt-in capability). **Wraps the Claude Agent SDK, not the Claude Code CLI.** See §10.3. |
| **Codex** 1.1.9 | `npx @agentclientprotocol/codex-acp` | Registry | Official-org adapter; authors *OpenAI, JetBrains, Zed*. Note: **`codex acp` is not a subcommand** — I probed the installed `codex` binary and got `Error: stdin is not a terminal`. The adapter is required. |
| **Cursor** 2026.07.23 | `cursor-agent acp` | Registry | First-party, in the registry with per-platform binaries. My local Homebrew cask install crashes on *every* invocation (bundled-JS error), so unverified here; treat the registry entry as authoritative. |
| **GitHub Copilot** 1.0.77 | `npx @github/copilot --acp` | Registry; public preview since 2026-01-28 | Not in [11](agent-clis.md) at all. |
| Others in the registry | — | — | Amp (`amp-acp` wrapper, 0.9.0), Kimi CLI (`kimi acp`), Augment/Auggie, Cline, OpenHands, Kiro, Junie, Factory Droid, Mistral Vibe, Docker cagent, Blackbox, Stakpak, and ~20 more. |

**The honest summary:** ACP support is real, current, and much broader than assumed. The
gaps are in *depth*, not presence — capability sets differ meaningfully (audio: Gemini
only; session listing: opencode only), and **no agent I probed emitted `elicitation/create`
or `plan`**. omt must treat every optional variant as absent until observed per agent, and
`agent.explain` should report exactly which variants each binding has actually produced.

---

## 10. The TUI tension — the load-bearing analysis

### 10.1 Statement of the problem

omt's premise, per [01 — Principles](../architecture/01-principles.md) and the whole of
[04](../architecture/04-terminal-core.md), is that **the user runs their real CLI in a real
terminal and omt observes and augments it**. The user types `omt claude`, gets Claude
Code's actual TUI, and omt adds remote visibility and remote answering on top.

An ACP-mode agent is a **different program mode with no TUI at all**. `opencode acp` draws
nothing; it reads JSON-RPC on stdin and writes JSON-RPC on stdout. There is no interactive
UI to observe, no PTY to attach to, no screen to scroll. If omt spawns `opencode acp`, the
pane shows either nothing or omt's own rendering of the ACP event stream.

These are mutually exclusive. You cannot run `opencode` (TUI) and speak ACP to it. **ACP
is not an observability sidecar; it is a replacement front end.** [11 §11](agent-clis.md)
calls ACP "the single best portable integration for omt after Claude Code's hooks" without
noting this, and that framing is wrong. **[CORRECTION]**

### 10.2 Why the obvious workarounds don't work

- *"Run the TUI and an ACP connection side by side."* No. They are separate processes with
  separate sessions and separate state. `session/list` + `session/resume` lets a second
  connection *observe a stored session*, but only for agents that support it (opencode
  does; Gemini doesn't), only at replay granularity, and — decisively — the TUI process
  isn't publishing into ACP, so there is no live stream and no permission requests. A
  permission prompt raised inside the TUI is answered inside the TUI.
- *"Wrap the TUI's PTY and translate."* That is [06 §6](../architecture/06-agent-layer.md#6-the-heuristic-floor)'s
  `PtyHeuristics`, not ACP. It gains nothing from the protocol.
- *"Ask the agents to publish ACP events from TUI mode."* Reasonable to propose upstream —
  ACP v2's explicit support for *multiple clients observing one session* and for
  out-of-turn updates makes it conceivable — but it does not exist today in any
  implementation and omt cannot plan on it.

### 10.3 The same problem, sharper, for Claude Code

`@agentclientprotocol/claude-agent-acp` wraps the **Claude Agent SDK**, not the Claude Code
CLI. Using it means omt is not running Claude Code at all. The user loses: the Claude Code
TUI, its keybindings, `/voice`, its exact permission UX, its settings and hooks behaviour,
its session files on disk, and — for anyone whose mental model is "I use Claude Code" — the
product they chose. In exchange omt gets one uniform code path.

**Compared against omt's hook-based approach, honestly:**

| | ACP adapter | Hooks ([11 §1.4](agent-clis.md), [06 §5.3](../architecture/06-agent-layer.md#53-the-deferral-mechanism-and-its-risk)) |
|---|---|---|
| Runs the user's real Claude Code | **No** | **Yes** |
| Permission round-trip | Blocking request, **no timeout** — verified shape | `permissionDecision: "defer"`, **unverified**, timeout unknown |
| `AskUserQuestion` / `Choice` fidelity | Would arrive as a tool call needing permission, or not at all — the adapter has no `AskUserQuestion` concept | **1:1, verified** |
| Structured tool calls, plans, usage, slash commands | Free, uniform, typed | Per-agent, partly UNCERTAIN |
| Install burden | Spawn a process | Merge JSON into 4 config files without clobbering user hooks |
| Breakage mode | Version-negotiated protocol; capabilities degrade gracefully | Undocumented payload shapes change silently |
| Works when the user starts the session themselves | **No** | **Yes** — hooks fire regardless of who spawned the CLI |

The last row is decisive and it is not close. omt's value proposition includes "you already
have twenty sessions running; omt tells you which need you." Hooks serve that. ACP
structurally cannot: it only sees sessions omt itself started, in a mode the user did not
choose.

### 10.4 Recommendation

**Adopt ACP, but scope it correctly: ACP is omt's *headless agent runner*, not its
*agent observer*. It is a second product surface, not a replacement for the detection
stack.**

Concretely:

1. **Keep the detection/hook/PTY stack as the primary path**, unchanged. It is what makes
   omt work on the user's real, self-started CLI sessions. Nothing in this document
   weakens [12 §12.1](agent-clis.md)'s layered detection.
2. **Add ACP as an explicit, user-chosen mode** — `omt agent run opencode`, or a "New agent
   session" button in the web client — that spawns the agent in ACP mode and renders it
   with omt's *own* native UI: transcript, tool-call cards, permission cards, plan,
   slash-command palette, model selector. In this mode omt is a full ACP client and the
   experience is better than the TUI's, because it is remote-first, multi-surface, and
   collaborative ([12](../architecture/12-collaboration.md)).
3. **Do not spend the ACP adapter's budget pretending it observes TUI sessions.** Label
   the mode honestly in the UI: this is an omt-hosted agent session, not your terminal.
4. **The killer feature of omt-as-ACP-client is `terminal/*` (§4.4)**, while v1 lasts: the
   agent's commands run in real omt panes. Lead with that. Track v2's inversion (§8.2) and
   push back upstream if it matters.
5. **`AgentSource` must model both.** An ACP binding has `sources: [Acp]`, no PTY
   heuristics, and `agent_session` known at `session/new`. A TUI binding never has `Acp`.
   `agent.explain` should say which.

The one thing that would change this recommendation is agents publishing ACP events *from*
their interactive TUI over a side channel. v2's out-of-turn updates and multi-observer
framing make that a coherent RFD to file. It is not a thing to plan around.

---

## 11. Rust implementation plan for omt's generic ACP adapter

### 11.1 Use the official crate. Do not hand-roll.

`agent-client-protocol` (repo: `agentclientprotocol/rust-sdk`, docs.rs published, powers
Zed's external-agent integration, 1.0 reached). The workspace also ships
`agent-client-protocol-http` (HTTP/SSE + WebSocket transports),
`agent-client-protocol-rmcp` (rmcp/MCP integration), `-derive`, `-conductor` (proxy
chains), `-polyfill`, `-test`, and `-cookbook`. Schema types are generated from the same
`schema.json` this document quotes, and both `v1` and `v2` module trees exist behind
feature flags (`unstable_protocol_v2`).

Hand-rolling JSON-RPC over stdio is ~300 lines and looks tempting. Don't: the parts that
bite are bidirectional id spaces, `$/cancel_request` semantics, `x-deserialize-default-on-error`
(the schema explicitly marks most fields as "fall back to default rather than fail" — that
forward-compatibility behaviour is *in the generated types* and is why a v1 client survives
an agent emitting v1.6 fields), and open-enum handling in v2. All of that is done.

The verified client shape, from the SDK's own `yolo_one_shot_client.rs` example:

```rust
use agent_client_protocol::schema::v1::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, TextContent,
};
use agent_client_protocol::{AcpAgent, Agent, ConnectionTo, schema::ProtocolVersion};

let agent = AcpAgent::from_str("opencode acp")?;   // AcpAgent is itself the transport

agent_client_protocol::Client
    .builder()
    .on_receive_notification(
        async move |n: SessionNotification, _cx| { /* → omt AgentEvent */ Ok(()) },
        agent_client_protocol::on_receive_notification!(),
    )
    .on_receive_request(
        async move |req: RequestPermissionRequest, responder, _conn| {
            // responder is held across an arbitrarily long await — this is the
            // handle omt parks in the interaction ledger.
            responder.respond(RequestPermissionResponse::new(
                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(id))))
        },
        agent_client_protocol::on_receive_request!(),
    )
    .connect_with(agent, |conn: ConnectionTo<Agent>| async move {
        conn.send_request(InitializeRequest::new(ProtocolVersion::V1)).block_task().await?;
        let s = conn.send_request(NewSessionRequest::new(cwd)).block_task().await?;
        conn.send_request(PromptRequest::new(s.session_id, vec![
            ContentBlock::Text(TextContent::new(prompt))])).block_task().await?;
        Ok(())
    })
    .await?;
```

The `responder` object is the crux: it is an owned handle that can be moved into omt's
ledger and answered minutes later from a phone. That is the entire remote-permission
feature, and the SDK hands it over cleanly.

### 11.2 Fitting omt's traits

```rust
/// One ACP connection = one agent subprocess = one omt AgentBinding.
pub struct AcpSource {
    binding: BindingId,
    conn: ConnectionTo<Agent>,
    agent_session: AgentSessionId,          // from session/new — never inferred
    /// Parked client-side requests, keyed by omt InteractionId.
    pending: Mutex<HashMap<InteractionId, PendingAcpRequest>>,
    /// Coalescing buffers keyed by messageId. See §5.1 — 3,463 notifications/turn.
    chunks: Mutex<HashMap<MessageId, ChunkBuffer>>,
}

enum PendingAcpRequest {
    Permission(Responder<RequestPermissionResponse>),
    Elicitation(Responder<CreateElicitationResponse>),
}
```

**`EventSource`.** `session/update` → `AgentEvent`, mostly mechanical:

| ACP | omt `AgentEvent` ([12 §12.2](agent-clis.md)) |
|---|---|
| `session/new` returns | `SessionStart` + `Capabilities` (from `initialize` + `configOptions`) |
| first `agent_message_chunk` after prompt | `TurnStart` |
| `session/prompt` response (v1) / `state_update:idle` (v2) | `TurnEnd { stop_reason }` |
| `tool_call` | `ToolCall` |
| `tool_call_update` status `completed`/`failed` | `ToolResult` |
| `plan` | `Plan` (informational — **no** `PlanReview`, §7) |
| `available_commands_update` | `Capabilities { slash_commands }` |
| `usage_update` | `Usage` |
| `current_mode_update` / `config_option_update` | `ModeChanged` / `ConfigChanged` |
| `session_info_update` | `SessionInfo` |
| `session/request_permission` | `Interaction::Permission` |
| `elicitation/create` (form) | `Interaction::Choice` / `Form` / `Text` per §7 |
| — | `Compaction`, `QueueChanged`: **ACP has no equivalent.** Genuine gaps. |

**`Responder`.** `fidelity() = Native`, `state_dependence() = Independent`. `respond()`
pops the parked handle and calls `responder.respond(...)`. Map
`InteractionResponse::Permission { decision }` back to an `optionId` **by finding the
option whose `kind` matches** — never by index and never by a synthesised id, because the
agent's ids are its own (`"once"`, `"always"`, `"reject"` for opencode; different
elsewhere). If no option of that kind exists, the resolution must fail loudly rather than
guess. This is D1 as code.

### 11.3 Session correlation

Trivial and exact, which is the point: `session/new` returns the `sessionId`, omt stores it
in `AgentBinding.agent_session`, and every notification carries it. One connection may host
multiple sessions (the architecture docs say "each connection can support several
concurrent sessions"), so `AcpSource` should key by `SessionId` internally rather than
assume one. **Recommendation: one subprocess per omt pane anyway** — simpler lifetime,
simpler crash isolation, and the memory cost is the user's choice.

### 11.4 Process lifecycle

- **omt spawns it.** No attaching to a pre-existing ACP process; there is nothing to attach
  to. Resolve the launch command from the **ACP registry manifest**
  (`cdn.agentclientprotocol.com/registry/v1/latest/registry.json`) rather than hardcoding —
  it is CI-verified, versioned, carries per-platform binaries with SHA256s and `npx`
  specs, and gets omt ~40 agents from one table. Cache it; fall back to a vendored copy.
- **stdin/stdout are the protocol.** Nothing else may touch them. stderr → omt's log.
- **Death** = binding end. Every parked responder becomes `InteractionState::Abandoned`
  with the exit status in `detail`.
- **Auth**: `-32000` from any method → surface `authMethods` as an omt interaction, call
  `authenticate`, retry. This is a case where `elicitation/create` request-scope
  (`requestId`, no session) is the agent's own preferred flow, so implementing form-mode
  elicitation pays for itself at first login.
- **Cancellation**: omt's stop/interrupt → `session/cancel`, then answer every pending
  permission request with `{"outcome":"cancelled"}` (a MUST), then await the
  `stopReason: "cancelled"` response. Handle inbound `$/cancel_request` for omt's own
  outstanding client-side work.

### 11.5 Sequencing

1. `spike-acp-client` — SDK, opencode + Gemini, handshake, one turn, one permission card
   answered from the web client end-to-end. Everything in §4.3 is already verified, so
   this is a low-risk spike, unlike `spike-defer-semantics`.
2. Chunk coalescing by `messageId` before any broadcast (§5.1). Non-optional.
3. `terminal/*` served by real omt panes (§4.4). This is the differentiator.
4. Elicitation form mode + the `InteractionKind::Form` addition (§7).
5. Registry-driven agent discovery.
6. v2 behind a feature flag once it stabilises, negotiating both.

---

## 12. Open questions

1. Does **any** shipping agent emit `elicitation/create` today? I found none. Until one
   does, the `Choice`-for-non-Claude story is theoretical.
2. Does any agent emit `plan`? opencode did not in my runs; the docs imply Zed sees them.
3. Goose, Qwen, Cursor, Copilot capability sets — unverified here; probe each with the §2
   handshake and record `agentCapabilities` in the adapter table.
4. Does `claude-agent-acp` expose `AskUserQuestion` in any form, or does the Agent SDK
   flatten it into a tool call? This decides whether the ACP path can ever match the hook
   path on omt's flagship case.
5. v2 timeline and whether the terminal inversion (§8.2) is final. Worth an RFD comment
   from omt's perspective — omt is the client with the strongest stake in v1's direction.
6. Whether `agent-client-protocol-http` can carry ACP between omt's daemon and a remote
   machine's agent, which would fold [07](../architecture/07-remote-protocol.md) and this
   layer together for the headless case.
