# Coding-Agent CLIs: Detection, State, and Structured-Interaction Research

Research for **oh-my-term (`omt`)** — a Rust TUI that runs coding-agent CLIs inside PTYs, detects *which* agent is running and *what state* it is in, and mirrors structured interactions (e.g. Claude Code's `AskUserQuestion` cards) to a remote web client for native rendering.

**Method.** Everything below marked "verified locally" was obtained by running the CLI or reading its on-disk state on this machine (macOS 25.1.0, 2026-08-03). Everything else came from official docs and is cited. Items I could not confirm are marked **UNCERTAIN**.

**Locally installed and verified on this machine:**

| CLI | Path | Version |
|---|---|---|
| Claude Code | `/Users/hanxiao.du/.local/bin/claude` | 2.1.220 |
| Codex CLI | `/opt/homebrew/bin/codex` | 0.145.0-alpha.18 (from session meta) |
| opencode | `~/.nvm/versions/node/v24.13.1/bin/opencode` | 1.17.x |
| Gemini CLI | `/opt/homebrew/bin/gemini` | 0.46.0 |
| Cursor CLI | `/opt/homebrew/bin/cursor-agent` | 2026.07.23-e383d2b |

Not installed here (docs-only): Amp, Aider, Goose, Qwen Code, Crush.

---

## 1. Claude Code (Anthropic)

### 1.1 Binary and process-tree detection

- Binary: `claude`. On this machine it is a launcher shim at `~/.local/bin/claude` that execs a native build under `~/.local/share/claude/versions/<ver>/`. Historically it was `node .../cli.js`, so **do not rely on the process name alone** — the PTY child may be `claude`, `node`, or a versioned native binary.
- **Best signal: environment variables it exports into its own process and into every child shell.** Verified locally by dumping `env` inside a Claude Code Bash tool call:

```
CLAUDECODE=1
CLAUDE_CODE_ENTRYPOINT=cli            # "cli" | "sdk-ts" | "sdk-py" | "vscode" ... (values UNCERTAIN beyond "cli")
CLAUDE_CODE_EXECPATH=/Users/hanxiao.du/.local/share/claude/versions/2.1.220
CLAUDE_CODE_SESSION_ID=5129fa43-a66a-4b4d-8c27-481a4701a652
CLAUDE_CODE_CHILD_SESSION=1           # set for subagent/teammate sessions
CLAUDE_PID=5013
CLAUDE_EFFORT=low
CLAUDE_CODE_BRIDGE_SESSION_ID=session_01UZ...   # claude.ai session link
AI_AGENT=claude-code_2-1-220_agent    # NOT official — set by a local wrapper on this machine, do not depend on it
```

  For omt this matters twice over: (a) `omt` spawns the agent, so it can read `/proc`-equivalent env (on macOS: `ps -E -p <pid>`, or better, `libproc`/`sysctl KERN_PROCARGS2`) to classify the PTY child; (b) `CLAUDE_CODE_SESSION_ID` gives the session UUID *directly*, which is the join key to the transcript file — no slug-guessing needed.
- Argv detection: `claude`, `claude -p ...`, `claude --resume <uuid>`.
- Terminal title: Claude Code sets the terminal title (OSC 0/2) to the session name / current task. Hooks can also emit arbitrary OSC via the `terminalSequence` output field (see 1.4) — this is the officially sanctioned OSC channel.

### 1.2 Machine-readable modes

All headless flags require `-p/--print` (verified from `claude --help`):

| Flag | Meaning |
|---|---|
| `-p, --print` | Non-interactive; print response and exit |
| `--output-format text\|json\|stream-json` | Output shape |
| `--input-format text\|stream-json` | **Realtime streaming input** — this is the key to a bidirectional headless session |
| `--include-partial-messages` | Emit `stream_event` deltas as they arrive |
| `--include-hook-events` | **Emit all hook lifecycle events into the output stream** (stream-json only) |
| `--replay-user-messages` | Echo stdin user messages back on stdout for ack (stream-json in+out) |
| `--forward-subagent-text` | Forward subagent text/thinking with `parent_tool_use_id` |
| `--verbose` | Required to get full stream-json in `-p` |
| `--json-schema <schema>` | Structured final output validation |
| `--session-id <uuid>` | Pin the session UUID (so omt can pre-compute the transcript path) |
| `--permission-mode default\|acceptEdits\|auto\|bypassPermissions\|manual\|dontAsk\|plan` | |
| `--remote-control [name]` | **Starts an *interactive* session with Remote Control enabled** — the closest thing to an official "drive my TUI from outside" API |
| `--mcp-config`, `--strict-mcp-config` | MCP wiring |
| `--settings <file-or-json>` | Inject settings (incl. hooks) without touching the user's config |
| `--no-session-persistence` | Disables transcript writing (breaks file-tailing) |

Also: `claude mcp serve` (Claude Code as an MCP server), and the **Claude Agent SDK** (TS/Python) which is a first-class programmatic wrapper over the same stream-json protocol.

**Real stream-json capture** (verified locally: `echo "Reply with only: ok" | claude -p --output-format stream-json --verbose --model haiku`), abridged:

```json
{"type":"system","subtype":"hook_started","hook_id":"23eb77a0-...","hook_name":"SessionStart:startup","hook_event":"SessionStart","uuid":"bfa2...","session_id":"6580fab8-dd6d-4631-a44f-016687e40cec"}
{"type":"system","subtype":"hook_response","hook_id":"23eb77a0-...","hook_name":"SessionStart:startup","hook_event":"SessionStart","output":"","stdout":"","stderr":"","exit_code":0,"outcome":"success","uuid":"e798...","session_id":"6580fab8-..."}
{"type":"system","subtype":"init","cwd":"/private/tmp","session_id":"6580fab8-...","tools":["Task","Bash","Edit","Read","Write",...],"mcp_servers":[{"name":"playwright","status":"pending"},{"name":"cep-ro","status":"failed"},{"name":"claude.ai Atlassian","status":"needs-auth"}],"model":"claude-haiku-4-5-20251001","permissionMode":"default","slash_commands":["fabric-report","dataviz","update-config","code-review","clear","compact","config","context","model","review","usage", ...]}
{"type":"system","subtype":"thinking_tokens","estimated_tokens":93,"estimated_tokens_delta":63,"uuid":"...","session_id":"..."}
{"type":"assistant","message":{"model":"claude-haiku-4-5-20251001","id":"msg_011Cdf...","type":"message","role":"assistant","content":[{"type":"thinking","thinking":"...","signature":"..."}],"stop_reason":null,"usage":{"input_tokens":9,"cache_creation_input_tokens":7589,"cache_read_input_tokens":17703,"output_tokens":4}},"parent_tool_use_id":null,"session_id":"6580fab8-...","uuid":"5c8cecdd-...","timestamp":"2026-08-03T07:54:12.386Z","request_id":"req_011Cdf..."}
{"type":"assistant","message":{...,"content":[{"type":"text","text":"ok"}],...},"parent_tool_use_id":null,"session_id":"...","uuid":"028f17c3-...","timestamp":"..."}
{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":1785755400,"rateLimitType":"five_hour","overageStatus":"rejected","isUsingOverage":false},"uuid":"...","session_id":"..."}
{"type":"result","subtype":"success","is_error":false,"duration_ms":1861,"duration_api_ms":1811,"num_turns":1,"stop_reason":"end_turn","result":"ok","session_id":"6580fab8-...","total_cost_usd":0.0171623,"usage":{...},"modelUsage":{"claude-haiku-4-5-20251001":{"inputTokens":9,"outputTokens":41,"costUSD":0.0171623,"contextWindow":200000,...}},"permission_denials":[],"terminal_reason":"completed","ttft_ms":1736}
```

Decision-relevant observations:
- `system/init` is a **complete capability manifest**: tool list, MCP server list *with health status*, model, permission mode, and **the full slash-command list**. This single event solves requirement (5) for an external UI for free.
- `parent_tool_use_id` distinguishes subagent output from main-thread output.
- `rate_limit_event` and `system/thinking_tokens` are pure-state events omt can surface without any heuristics.
- `result.permission_denials` is an array — the wire-level record of denied tool calls.
- Hook lifecycle appears in-stream as `system/hook_started` + `system/hook_response` *even without* `--include-hook-events` for SessionStart; `--include-hook-events` adds the rest.

### 1.3 Transcript files on disk

Path: `~/.claude/projects/<cwd-slug>/<session-uuid>.jsonl`
Slug = the absolute cwd with `/` and `.` replaced by `-`, e.g.
`/Users/hanxiao.du/Desktop/vincent/projects/oh-my-term` → `-Users-hanxiao-du-Desktop-vincent-projects-oh-my-term`.

Sibling state also under `~/.claude/`: `history.jsonl`, `sessions/`, `session-env/<session-uuid>/`, `file-history/`, `shell-snapshots/`, `tasks/`, `teams/`, `transcripts/`, `settings.json`, `settings.local.json`, `commands/`, `skills/`, `plugins/`, `hooks/`.

**Line types actually observed** in a single 700-line transcript (verified locally):

| `type` | Count | Purpose |
|---|---|---|
| `assistant` | 260 | Anthropic Message object |
| `user` | 149 | user text, or an array containing `tool_result` blocks |
| `mode` | 37 | `{"type":"mode","mode":"normal","sessionId":"..."}` — permission/mode changes |
| `last-prompt` | 36 | `{"type":"last-prompt","lastPrompt":"...","leafUuid":"...","sessionId":"..."}` |
| `ai-title` | 36 | `{"type":"ai-title","aiTitle":"检查项目 agent dev workflow 是否正常","sessionId":"..."}` |
| `attachment` | 33 | pasted/attached content |
| `file-history-snapshot` / `file-history-delta` | 11 / 7 | edit-undo state |
| `queue-operation` | 10 | **message queueing** — see 1.7 |
| `system` | 3 | subtypes `local_command`, `away_summary` |

Common envelope fields on message lines:
`parentUuid, uuid, sessionId, session_id, timestamp, type, userType ("external"), version ("2.1.220"), cwd, gitBranch, entrypoint ("cli"), isSidechain, isMeta, requestId, effort, permissionMode, promptId, promptSource, origin`.

`isSidechain: true` ⇒ subagent thread. `origin: {"kind":"human"}` and `promptSource: "typed"` distinguish real user input from injected/system prompts — important so omt doesn't mirror synthetic turns to the web client.

Real user-prompt line (verified locally):

```json
{"parentUuid":"289cf971-...","isSidechain":false,"promptId":"2f5c8bce-...","type":"user",
 "message":{"role":"user","content":"你看下这个项目的 agent dev workflow在正常工作么"},
 "uuid":"c84bbe75-...","timestamp":"2026-08-02T21:56:20.920Z","permissionMode":"bypassPermissions",
 "origin":{"kind":"human"},"promptSource":"typed","userType":"external","entrypoint":"cli",
 "cwd":"/Users/hanxiao.du/Desktop/vincent/projects/oh-my-workspace","sessionId":"65d265b7-...",
 "version":"2.1.220","gitBranch":"main"}
```

Real `system` lines (verified locally):

```json
{"type":"system","subtype":"local_command","content":"<local-command-stdout></local-command-stdout>","level":"info", ...}
{"type":"system","subtype":"away_summary","content":"Goal was to fix the agent dev workflow's dying watcher: that's done and pushed to all three repos. Next action is clearing the backlog...", ...}
```

`away_summary` is notable: Claude Code already produces a natural-language "what's the state of this session" blurb, ideal for an omt session card.

### 1.3.1 `AskUserQuestion` — the structured-interaction case

**Real `tool_use`, verbatim from `~/.claude/projects/-Users-hanxiao-du-Desktop-botim-eclipse/ffcd3eaf-....jsonl`:**

```json
{"type":"tool_use","id":"toolu_01EN58Za9e39k51vnxWzV5c3","name":"AskUserQuestion",
 "input":{"questions":[
   {"question":"Which GitHub org should the Botim Eclipse repo be created under? (Your Astratech account has access to these.)",
    "header":"GitHub Org","multiSelect":false,
    "options":[
      {"label":"BotimMoney","description":"Botim product / money org — natural home for a product named Botim Eclipse."},
      {"label":"AstraPlayGround","description":"Experimentation / playground org."},
      {"label":"FintechBU-CoE","description":"Fintech BU Center of Excellence org."},
      {"label":"AI-Infra-Gurus","description":"AI infrastructure org — fits an agentic platform."}]},
   {"question":"Repository visibility?","header":"Visibility","multiSelect":false,
    "options":[
      {"label":"Private (Recommended)","description":"Only org members with access can see it. Standard for internal enterprise projects."},
      {"label":"Internal","description":"Visible to all members of the enterprise."},
      {"label":"Public","description":"Visible to everyone on the internet."}]},
   {"question":"What language should the official docs be written in? (The source drafts are in Chinese.)",
    "header":"Doc Language","multiSelect":false,
    "options":[
      {"label":"English (Recommended)","description":"Rewrite all docs in English for broad collaboration. Identifiers/terms already English."},
      {"label":"Bilingual","description":"English prose with key Chinese context retained where helpful."},
      {"label":"Chinese","description":"Keep Chinese prose as in the source drafts, with English identifiers."}]}]},
 "caller":{"type":"direct"}}
```

**Its matching `tool_result` (verbatim):**

```json
{"type":"tool_result","tool_use_id":"toolu_01EN58Za9e39k51vnxWzV5c3",
 "content":"Your questions have been answered: \"Which GitHub org should the Botim Eclipse repo be created under? (Your Astratech account has access to these.)\"=\"AI-Infra-Gurus\", \"Repository visibility?\"=\"Private (Recommended)\", \"What language should the official docs be written in? (The source drafts are in Chinese.)\"=\"English (Recommended)\". You can now continue with these answers in mind."}
```

Schema, precisely:
- `input.questions: Question[]` (1..4 in practice).
- `Question = { question: string, header: string (short tab label, ~12 chars), multiSelect: boolean, options: Option[] }`
- `Option = { label: string, description: string }`
- `caller: { type: "direct" }` — `"direct"` when the main agent asks; other values exist for subagent callers (**UNCERTAIN** on the full enum).
- The **answer is not machine-readable**: the `tool_result` is a prose sentence of `"question"="label"` pairs. If omt renders the card remotely and injects the answer, it must reproduce that exact phrasing, or (better) inject the answer via the stream-json input channel and let Claude Code format the result itself.

**Implication for omt:** the transcript file gives you the full card *after the fact*. For live rendering you want either (a) a `PreToolUse` hook matching `AskUserQuestion` (fires *before* the TUI draws the card), or (b) `--include-hook-events` in stream-json mode. The hook payload contains `tool_input` verbatim — i.e. the exact `questions` array above.

### 1.4 Hooks

Config in `settings.json` (user `~/.claude/settings.json`, project `.claude/settings.json`, `.claude/settings.local.json`, or injected via `--settings`). Verified local shape:

```json
{"hooks":{"SessionStart":[{"matcher":"*","hooks":[
  {"type":"command","command":"bash '/Users/hanxiao.du/.claude/hooks/another tool-agent-state.sh' session","timeout":10}]}]}}
```

**Full event list (30, from official docs):** `SessionStart`, `Setup`, `UserPromptSubmit`, `UserPromptExpansion`, `PreToolUse`, `PermissionRequest`, `PermissionDenied`, `PostToolUse`, `PostToolUseFailure`, `PostToolBatch`, `Notification`, `MessageDisplay`, `SubagentStart`, `SubagentStop`, `TaskCreated`, `TaskCompleted`, `Stop`, `StopFailure`, `TeammateIdle`, `InstructionsLoaded`, `ConfigChange`, `CwdChanged`, `DirectoryAdded`, `FileChanged`, `WorktreeCreate`, `WorktreeRemove`, `PreCompact`, `PostCompact`, `Elicitation`, `ElicitationResult`.

**Common stdin payload (every event):**

```json
{"session_id":"abc123","prompt_id":"550e8400-e29b-41d4-a716-446655440000",
 "transcript_path":"/home/user/.claude/projects/.../transcript.jsonl",
 "cwd":"/home/user/my-project","permission_mode":"default","hook_event_name":"PreToolUse"}
```
Plus `effort:{level}`, `agent_id`, `agent_type` where applicable.

**Event-specific additions relevant to omt:**

| Event | Extra stdin fields |
|---|---|
| `PreToolUse` | `tool_name`, `tool_input` (full object), `tool_use_id` |
| `PostToolUse` | `tool_name`, `tool_input`, `tool_output`, `tool_use_id`, `duration_ms` |
| `UserPromptSubmit` | `prompt_text` |
| `Stop` / `SubagentStop` | `last_assistant_message`, `turn_count` |
| `SessionStart` | `source` ∈ `startup\|resume\|clear\|compact\|fork` |
| `SessionEnd` | `reason` ∈ `clear\|resume\|logout\|prompt_input_exit\|bypass_permissions_disabled\|other` |
| `Notification` | `notification_type` (`permission_prompt`, `idle_prompt`, `auth_success`, `elicitation_dialog`, …), `message` |
| `PreCompact` | `trigger` ∈ `auto\|manual`, `current_token_count` |

**Universal stdout schema:**

```json
{"continue":true,"stopReason":"...","suppressOutput":false,"systemMessage":"...",
 "terminalSequence":"]777;notify;Title;Message",
 "hookSpecificOutput":{"hookEventName":"EventName","additionalContext":"..."}}
```

`PreToolUse` decision control: `hookSpecificOutput.permissionDecision ∈ allow|deny|ask|defer`, `permissionDecisionReason`, `updatedInput`, `additionalContext`. Exit code 2 = blocking error (stderr becomes the reason); other non-zero = non-blocking.

**How omt should use this.** Install a single hook binary (`omt-hook`) matched on `*` for the events omt cares about; it POSTs the JSON payload to omt's local Unix socket / HTTP endpoint keyed by `session_id`, and returns `{}` (or `{"suppressOutput":true}`) immediately. That gives omt:
- exact session identity + `transcript_path` on `SessionStart` (no slug math);
- `busy`/`idle` transitions from `UserPromptSubmit` → `Stop`;
- `waiting_for_permission` from `Notification{notification_type:"permission_prompt"}` and `PermissionRequest`;
- structured cards from `PreToolUse{tool_name:"AskUserQuestion"}` — and, critically, `permissionDecision:"defer"` lets omt *hold* the tool call while the remote web client renders and answers. This is the single most valuable hook capability for omt's core feature.
- `terminalSequence` is a sanctioned way to push OSC markers into the PTY stream (OSC 0/1/2/9/99/777 + BEL only) — a belt-and-braces channel when the socket is unavailable.

`Setup` and `SubagentStart`/`TeammateIdle`/`TaskCreated` also make multi-agent supervision cheap.

### 1.5 Slash commands

Three sources, all discoverable from disk:
- **User commands:** `~/.claude/commands/*.md` (verified: `init-workflow.md` present). Frontmatter carries `description`, `argument-hint`, `allowed-tools`, `model`.
- **Project commands:** `.claude/commands/**/*.md` (nesting → namespaced `/dir:name`).
- **Skills:** `~/.claude/skills/<name>/SKILL.md` and `.claude/skills/` (verified: `fabric-report`, `dataviz`, … present). Invoked as `/<name>`.
- **Plugins:** `~/.claude/plugins/` contribute both.

**But you don't need to parse any of it for a live session:** the `system/init` stream-json event carries the resolved `slash_commands` array (verified above, ~60 entries including built-ins like `/clear`, `/compact`, `/model`, `/usage`). For a *remote* UI, the practical design is: read `slash_commands` from `init` for completion candidates; read the `.md` frontmatter `description`/`argument-hint` from disk to enrich the completion popup. For a TUI-attached (not `-p`) session, omt has no `init` event, so it must enumerate the three directories itself — or run a throwaway `claude -p --output-format stream-json` with the same cwd/settings just to harvest `init`. That throwaway-probe trick is cheap and worth doing.

### 1.6 Permission/approval semantics on the wire

- Interactive TUI: a modal card. On the wire (stream-json/hooks) it is `Notification{notification_type:"permission_prompt"}`, plus the `PermissionRequest` hook event, plus (if denied) `PermissionDenied`.
- Headless: `result.permission_denials[]` accumulates denials; `--permission-mode` and `--allowedTools`/`--disallowedTools` set policy up front.
- The Agent SDK exposes `canUseTool` as an explicit control request/response pair over stream-json (a `control_request` with `subtype: "can_use_tool"`), which the client answers with allow/deny + optional `updatedInput`. **UNCERTAIN** whether the raw CLI (as opposed to the SDK) emits `control_request` without SDK-side handshake; the hook `defer` path is the safer mechanism for omt.
- Modes: `default`, `plan`, `acceptEdits`, `auto`, `dontAsk`, `bypassPermissions`, `manual`. Transcript records the active mode both in the `mode` line type and in `permissionMode` on user lines.

### 1.7 Message queueing

Verified locally — Claude Code writes explicit queue events to the transcript:

```json
{"type":"queue-operation","operation":"enqueue","timestamp":"2026-08-02T21:57:33.077Z","sessionId":"65d265b7-...","content":"现在有issues是没被认领的么？"}
{"type":"queue-operation","operation":"remove","timestamp":"2026-08-02T21:57:37.917Z","sessionId":"65d265b7-...","content":"现在有issues是没被认领的么？"}
```

So: typing while the agent works enqueues a message; it is dequeued (`remove`) when consumed or cancelled. **omt can mirror the pending queue to the web client verbatim** — a differentiating feature, and no other CLI examined exposes queue state this cleanly. Operations observed: `enqueue`, `remove` (others **UNCERTAIN**).

### 1.8 Voice / STT

Official, documented (`/docs/en/voice-dictation`):
- `/voice`, `/voice hold`, `/voice tap`, `/voice off`. Default hold-to-talk on `Space`.
- Settings: `{"voice":{"enabled":true,"mode":"tap","autoSubmit":true}}` in `~/.claude/settings.json`; language via the top-level `"language"` setting (20 languages: cs, da, nl, en, fr, de, el, hi, id, it, ja, ko, no, pl, pt, ru, es, sv, tr, uk).
- Keybinding action `voice:pushToTalk` in `~/.claude/keybindings.json` (context `Chat`).
- **Audio is streamed to Anthropic's servers; not local.** Requires a Claude.ai account (not API key / Bedrock / Vertex / Foundry), no HIPAA org policy, a local microphone. **Explicitly does not work over SSH or in remote environments.** Native audio module on macOS/Linux/Windows, falling back to ALSA `arecord` or SoX `rec` on Linux.
- Transcription is free (no tokens, not counted in `/usage`).

**Consequences for omt:** the official path is unusable for omt's remote/web client — it's gated on a first-party account and is explicitly local-mic-only. omt should ship BYOK STT: capture in the browser (MediaRecorder → Opus/WebM), and offer (a) OpenAI `gpt-4o-transcribe`/`whisper-1` via `POST /v1/audio/transcriptions`, (b) Deepgram streaming `wss://api.deepgram.com/v1/listen` (lowest latency, real word-level timing), (c) local `whisper.cpp`/`faster-whisper` for offline/privacy. Then inject the final transcript as a normal prompt. Deepgram-style streaming is the right default because it lets omt show interim text exactly like Claude Code's dimmed live transcript.

---

## 2. OpenAI Codex CLI

### 2.1 Detection

- Binary: `codex` (Rust, single static binary — process name is reliably `codex`).
- Env vars it sets/reads (extracted from the binary, verified locally): `CODEX_HOME`, `CODEX_SANDBOX`, `CODEX_THREAD_ID`, `CODEX_NON_INTERACTIVE`, `CODEX_CI`, `CODEX_EXEC_SERVER_URL`, `CODEX_ESCALATE_SOCKET`, `CODEX_INTERNAL_ORIGINATOR_OVERRIDE`, `CODEX_TUI_DISABLE_KEYBOARD_ENHANCEMENT`, `CODEX_API_KEY`, `CODEX_ACCESS_TOKEN`, `CODEX_SQLITE_HOME`, `CODEX_MANAGED_PACKAGE_ROOT`.
  `CODEX_SANDBOX` (set inside sandboxed child commands) and `CODEX_THREAD_ID` are the useful detection/correlation keys.
- `CODEX_HOME` defaults to `~/.codex`.

### 2.2 Machine-readable modes

| Command / flag | Meaning |
|---|---|
| `codex exec [PROMPT]` (alias `codex e`) | Non-interactive run |
| `codex exec --json` | **Print events to stdout as JSONL** |
| `codex exec -o/--output-last-message <FILE>` | Write final message to a file |
| `codex exec --output-schema <FILE>` | JSON Schema for the final response shape |
| `codex exec resume [--last]` | Resume a session non-interactively |
| `codex exec --ephemeral` | Run without persisting session files (breaks tailing) |
| `codex mcp-server` | **Codex as an MCP server over stdio** |
| `codex app-server` | **[experimental] Full app-server protocol** with `generate-ts` and `generate-json-schema` subcommands — i.e. the protocol is formally schematized and you can dump the JSON Schema yourself |
| `codex app-server proxy` / `daemon` | Proxy stdio bytes to a running app-server control socket; manage the daemon |
| `codex remote-control` | [experimental] Manage the app-server daemon with remote control enabled |
| `codex --remote <ADDR>` | **Connect the TUI to a remote app server** (`ws://`, `wss://`, `unix://PATH`) |
| `codex exec-server` | [EXPERIMENTAL] standalone exec-server service |
| `codex sandbox` | Run commands inside Codex's sandbox |
| `-s/--sandbox read-only\|workspace-write\|danger-full-access` | |
| `--dangerously-bypass-approvals-and-sandbox` | YOLO |

**`codex app-server generate-json-schema` is the single highest-leverage discovery here** — omt can generate an exact, versioned schema for Codex's protocol rather than reverse-engineering it, and `codex --remote unix://PATH` means omt could in principle host the session and attach a TUI, or attach its own client to a user-started daemon.

### 2.3 Session files on disk

Path: `~/.codex/sessions/<YYYY>/<MM>/<DD>/rollout-<ISO8601>-<uuid>.jsonl`
e.g. `~/.codex/sessions/2026/07/29/rollout-2026-07-29T12-04-16-019face7-2044-7100-adea-d6fb1e9378da.jsonl`

Also: `~/.codex/session_index.jsonl` (an index — cheapest thing to tail for "new session appeared"), `history.jsonl`, `config.toml`, `hooks.json`, `skills/`, `plugins/`, `shell_snapshots/`, `process_manager/`, `ipc/`, and several SQLite DBs (`state_5.sqlite`, `logs_2.sqlite`, `memories_1.sqlite`, `goals_1.sqlite`).

**Top-level line types** (verified locally): `session_meta`, `turn_context`, `event_msg`, `response_item`, `compacted`, `world_state`, `inter_agent_communication_metadata`.

Every line is `{"timestamp": ISO, "type": <above>, "payload": {...}}`.

`session_meta` payload (verbatim, abridged):

```json
{"session_id":"019facdb-67bb-7413-a777-efe9259ead72","id":"019face7-2044-7100-adea-d6fb1e9378da",
 "forked_from_id":"019facdb-...","parent_thread_id":"019facdb-...","timestamp":"2026-07-29T08:04:16.068Z",
 "cwd":"/Users/hanxiao.du/Desktop/botim-eclipse","originator":"Codex Desktop","cli_version":"0.145.0-alpha.18",
 "source":{"subagent":{"thread_spawn":{"parent_thread_id":"019facdb-...","depth":1,
   "agent_path":"/root/control_plane_consistency","agent_nickname":"Ramanujan","agent_role":null}}},
 "thread_source":"subagent","agent_nickname":"Ramanujan","agent_path":"/root/control_plane_consistency",
 "model_provider":"openai","base_instructions":{"text":"You are Codex, an agent based on GPT-5. ..."}}
```

`originator` values seen: `"Codex Desktop"`; `source` values seen: `"vscode"`, and the subagent object above. This is how omt tells apart a TUI session it spawned from one started by the IDE or desktop app.

**`event_msg` payload subtypes** (census over 60 local sessions, verified):
`agent_message` (21885), `task_started` (1054), `user_message` (1050), `task_complete` (1047), `token_count` (356), `agent_reasoning` (106), `web_search_end` (9), `thread_settings_applied` (8), `sub_agent_activity` (6), `turn_aborted` (4), `context_compacted` (4), `patch_apply_end` (2), `mcp_tool_call_end` (2). (Also documented but not present in my sample: `exec_command_begin` / `exec_command_end`, `exec_approval_request`, `apply_patch_approval_request` — **UNCERTAIN** on exact names for the approval variants since my local sessions ran with approvals bypassed.)

Real payloads (verbatim):

```json
{"type":"task_started","turn_id":"019facdc-55d5-78a2-a349-b16db246358f","started_at":1785311548,"model_context_window":258400,"collaboration_mode_kind":"default"}
{"type":"agent_message","message":"我会按 `review-agent` 做一次只读、缺陷优先的全项目审查；…","phase":"commentary","memory_citation":null}
{"type":"agent_reasoning","text":"**Evaluating test side effects and cache usage**"}
{"type":"token_count","info":{"total_token_usage":{"input_tokens":19086,"cached_input_tokens":14080,"cache_write_input_tokens":0,"output_tokens":349,"reasoning_output_tokens":117,"total_tokens":19435},"last_token_usage":{...},"model_context_window":258400},"rate_limits":{"limit_id":"codex","credits":{...}}}
{"type":"task_complete","turn_id":"019facdc-55d5-78a2-a349-b16db246358f","last_agent_message":"本次审查发现 7 个高优先级问题…"}
```

`task_started` / `task_complete` (with `turn_id`) are a clean **busy/idle** signal, and `turn_aborted` covers cancellation. `agent_message.phase` (`"commentary"` vs presumably `"final"`) lets omt distinguish narration from answers.

**`response_item` payload types** (census, verified): `message` (22937), `reasoning` (143), `custom_tool_call` / `custom_tool_call_output` (98 each), `function_call` / `function_call_output` (61 each), `web_search_call` (9), `agent_message` (6). These are raw OpenAI Responses-API items — omt can reuse any OpenAI-shaped parser.

### 2.4 Hooks

`~/.codex/hooks.json`, **structurally identical to Claude Code's** (verified local file):

```json
{"hooks":{"SessionStart":[{"hooks":[{"command":"bash '/Users/hanxiao.du/.codex/another tool-agent-state.sh' session","timeout":10,"type":"command"}]}]}}
```

Event names present in the binary (verified by string extraction): `SessionStart`, `SessionEnd`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Notification`, `Stop`, `PreCompact`. So Codex has deliberately adopted the Claude Code hook vocabulary — **omt can ship one hook handler for both**. Note `codex exec --dangerously-bypass-hook-trust` and the persisted "hook trust" concept: Codex requires hooks to be trusted before they run, so omt's installer must account for a trust prompt.

Payload field names are **UNCERTAIN** (likely snake_case mirroring Claude Code, but I did not capture a live payload).

### 2.5 Slash commands, permissions, queueing

- Slash commands / prompts: `~/.codex/skills/` (verified: `video-understanding/` present) and `$CODEX_HOME/prompts/*.md` (**UNCERTAIN** — not present on this machine). Config profiles layer via `-p/--profile <name>` → `$CODEX_HOME/<name>.config.toml`.
- Permissions: `--sandbox {read-only,workspace-write,danger-full-access}` × approval policy in `config.toml` (`approval_policy`). Approval requests surface as `event_msg` variants and, in the app-server protocol, as explicit request/response methods. `CODEX_ESCALATE_SOCKET` hints at an out-of-band escalation channel.
- Queueing: Codex's TUI accepts typed input while working and queues it. Not recorded as a distinct rollout line type in my samples — **UNCERTAIN**; omt likely must observe it via the app-server protocol or PTY heuristics.
- Voice/STT: none documented for the CLI. The Codex desktop/IDE apps are separate. **UNCERTAIN.**

---

## 3. opencode (sst/opencode)

### 3.1 Detection

- Binary: `opencode` (Bun-compiled single binary; process name `opencode`).
- Env vars (extracted, verified): `OPENCODE_CONFIG`, `OPENCODE_CONFIG_DIR`, `OPENCODE_CONFIG_CONTENT`, `OPENCODE_DB`, `OPENCODE_CLIENT`, `OPENCODE_API_KEY`, `OPENCODE_SERVER_PASSWORD`/`_USERNAME`, `OPENCODE_ACP_PROFILE`, `OPENCODE_AUTO_SHARE`, `OPENCODE_DISABLE_AUTOCOMPACT`, `OPENCODE_DISABLE_CLAUDE_CODE` (it can consume Claude Code prompts/skills!), `OPENCODE_CONSOLE_TOKEN`, `OPENCODE_DIRECT_TRACE`.

### 3.2 Machine-readable modes — the richest of any CLI here

| Command | Meaning |
|---|---|
| `opencode serve --port N --hostname H [--cors ...] [--mdns]` | **Headless HTTP server with a full REST + SSE API** |
| `opencode web` | Server + browser UI |
| `opencode attach <url>` | Attach a TUI to a running server |
| `opencode acp [--cwd DIR]` | **ACP (Agent Client Protocol) server** |
| `opencode run [msg] --format json` | One-shot, **raw JSON events** on stdout |
| `opencode run --attach http://localhost:4096 --session <id> --fork` | Drive a remote server session from the CLI |
| `opencode export [sessionID]` / `import <file>` | Session JSON round-trip |
| `opencode mcp`, `opencode agent`, `opencode session`, `opencode stats`, `opencode db` | Management surfaces |
| `-p/--password`, `-u/--username` | Basic auth for the server |

`opencode run` also takes `--agent`, `-m provider/model`, `--variant` (reasoning effort), `--thinking`, `--share`, `--title`.

The server + `attach` model means **omt does not need PTY heuristics for opencode at all**: spawn `opencode serve --port 0`, parse the port, subscribe to its event stream, and separately run the TUI attached to the same server. This is the cleanest integration target of the whole set.

### 3.3 Session storage

SQLite at `~/.local/share/opencode/opencode.db` (verified). Tables: `session`, `message`, `part`, `event`, `event_sequence`, `permission`, `todo`, `project`, `project_directory`, `workspace`, `session_context_epoch`, `session_input`, `session_message`, `session_share`, `account`, `credential`, `migration`.

Config at `~/.config/opencode/` (`opencode.json`, `agent/*.md`, `plugins/`, `themes/`, `tui.json`, `memory/`). Ancillary state at `~/.local/share/opencode/` (`storage/`, `snapshot/`, `tool-output/`, `log/`, `auth.json`).

`session` columns (verified): `id, project_id, parent_id, slug, directory, title, version, share_url, summary_additions, summary_deletions, summary_files, summary_diffs, revert, permission, time_created, time_updated, time_compacting, time_archived, workspace_id, path, agent, model, cost, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, metadata`. Note `parent_id` (subagents) and `permission` (per-session permission state).

`message.data` and `part.data` are JSON blobs. **Part type census** (verified): `tool` (26713), `step-start` (21323), `step-finish` (20890), `text` (14419), `reasoning` (6397), `compaction` (6066), `patch` (2355), `file` (211).

Real tool part (verbatim, truncated output):

```json
{"type":"tool","callID":"call_function_xs7xh2e7jost_1","tool":"webfetch",
 "state":{"status":"completed","input":{"url":"https://github.com/...","format":"markdown"},"output":"..."}}
```

`state.status` census (verified): `completed` (26376), `error` (318), `running` (15), `pending` (4). **`pending` / `running` is a direct, queryable "agent is currently doing X" state** — omt can poll or tail the DB for it.

`event` table types (verified): `message.part.updated.1` (58155), `message.updated.1` (20061), `session.updated.1` (5223), `session.created.1` (113), `session.next.model.switched.1` (9), `session.next.agent.switched.1` (9). Versioned event names (`.1` suffix) — good forward-compat hygiene, and exactly the stream omt wants.

Message `data` example (verbatim):

```json
{"role":"assistant","parentID":"msg_fc48516f8001dj7VVfXzUGeqO3","mode":"compaction","agent":"compaction",
 "summary":true,"path":{"cwd":"/private/tmp/.../fakerepo","root":"/"},"cost":0,
 "tokens":{"output":0,"input":0,"reasoning":0,"cache":{"read":0,"write":0}},
 "modelID":"deepseek-v4-flash-free","providerID":"opencode","time":{"created":1785708484449}}
```

Session IDs are ULID-ish: `ses_2e70d8700ffe3Sfps9jaDaYMOG`; messages `msg_...`; parts `prt_...`.

### 3.4 Hooks / plugins, commands, permissions, queueing

- **Plugins, not hooks**: `~/.config/opencode/plugins/*.js` (+ `opencode plugin <module>`). A plugin is a JS module exporting handlers for events (`tool.execute.before/after`, `chat.message`, `permission.ask`, `event`) with full access to the client SDK. Richer than shell hooks, but requires JS. **omt should ship an opencode plugin** that forwards events to omt's socket — it's the sanctioned path and avoids DB polling.
- Slash commands: `command` object in `~/.config/opencode/opencode.json`, plus agents as `~/.config/opencode/agent/*.md` (verified: `gemini-frontend.md`). Also consumes Claude Code skills unless `OPENCODE_DISABLE_CLAUDE_CODE_SKILLS=1`.
- Permissions: `permission` column on `session` + the `permission` table (`project_id, action, resource`) for persisted grants; live approvals go through the plugin `permission.ask` hook and the server API. Config `permission: {edit, bash, webfetch}` = `ask|allow|deny`.
- Queueing: `session_input` table exists (verified) and is the likely queue store — **UNCERTAIN** on exact semantics.
- ACP: `opencode acp` makes opencode drivable by any ACP client (Zed, etc.). See §11.
- Voice/STT: none. **UNCERTAIN.**

---

## 4. Gemini CLI (google-gemini/gemini-cli)

### 4.1 Detection

- Binary: `gemini` (Node; on this machine `/opt/homebrew/bin/gemini` → `.../@google/gemini-cli/bundle/gemini.js`, so **the PTY child may appear as `node`**).
- Env: `GEMINI_CLI_HOME`, `GEMINI_CLI_NO_RELAUNCH` (verified by string extraction); also reads `GEMINI_API_KEY`, `GOOGLE_API_KEY`, `GOOGLE_CLOUD_PROJECT`. Whether it exports a `GEMINI_CLI=1`-style marker into child shells is **UNCERTAIN** (I found no such string).

### 4.2 Machine-readable modes (verified from `gemini --help`, v0.46.0)

| Flag | Meaning |
|---|---|
| `-p, --prompt <text>` | Non-interactive (headless) |
| `-i, --prompt-interactive` | Run prompt then stay interactive |
| `-o, --output-format text\|json\|stream-json` | **stream-json is first-class** |
| `--acp` | **Start the agent in ACP mode** (`--experimental-acp` deprecated) |
| `--approval-mode default\|auto_edit\|yolo\|plan` | |
| `-y, --yolo` | Auto-approve everything |
| `--policy <files>` / `--admin-policy <files>` | Policy Engine files (replaces `--allowed-tools`) |
| `-r, --resume latest\|<n>` / `--session-file <json>` / `--session-id <uuid>` / `--list-sessions` / `--delete-session` | Session management |
| `-s, --sandbox`, `-w, --worktree` | |
| `--raw-output` / `--accept-raw-output-risk` | Disable ANSI sanitization (**note: gemini sanitizes model output by default — relevant to omt's PTY parsing**) |
| `--screen-reader` | Flat, decoration-free rendering — a useful fallback rendering mode for scraping |
| `gemini mcp`, `gemini extensions`, `gemini skills`, `gemini hooks` | Subcommands |

`--session-id <uuid>` is valuable: omt can mint the ID and know exactly which session file to tail.

### 4.3 Session files

`~/.gemini/tmp/<sha256-of-project-path>/chats/session-<ISO>-<short>.json` (verified locally) plus `~/.gemini/tmp/<hash>/logs.json`. Note the project directory is a **SHA-256 hash of the project path**, not a slug — omt must compute `sha256(cwd)` to locate it.

Top-level keys (verified): `["lastUpdated","messages","projectHash","sessionId","startTime"]`.
`messages[]` entries observed: `{"id": uuid, "timestamp": ISO, "type": "error"|"info", "content": "..."}` — my only local sample is an auth-failure session, so I could not observe the assistant/tool message shape. **UNCERTAIN** on the full message schema; expect Gemini `Content`/`Part` objects with `functionCall`/`functionResponse`.

Config: `~/.gemini/settings.json` (verified keys on this machine: `hasSeenIdeIntegrationNudge`, `mcpServers`, `security`), `~/.gemini/oauth_creds.json`, `~/.gemini/installation_id`, `~/.gemini/projects.json`.

### 4.4 Hooks — Claude-Code-compatible by design

`gemini hooks migrate` exists specifically to **migrate hooks from Claude Code to Gemini CLI** (verified from `gemini hooks --help`). The vocabulary differs though.

Events: `BeforeTool`, `AfterTool`, `BeforeAgent`, `AfterAgent`, `BeforeModel`, `BeforeToolSelection`, `AfterModel`, `SessionStart`, `SessionEnd`, `Notification`, `PreCompress`.

`settings.json` shape:

```json
{"hooks":{"BeforeTool":[{"matcher":"regex-for-tools","sequential":true,
  "hooks":[{"type":"command","command":"...","name":"...","timeout":60000,"description":"..."}]}]}}
```

Common stdin: `{"session_id","transcript_path","cwd","hook_event_name","timestamp"}` — note `timestamp` is extra vs Claude Code, and there is no `permission_mode`.
Common stdout: `{"systemMessage","suppressOutput","continue","stopReason","decision":"allow|deny|block","reason"}`.

Per-event: `BeforeTool` adds `tool_name`, `tool_input`, `mcp_context`, `original_request_name`; stdout `hookSpecificOutput.tool_input` can rewrite the call. `AfterTool` adds `tool_response{llmContent,returnDisplay,error}`. `BeforeAgent` adds `prompt`. `AfterAgent` adds `prompt_response`, `stop_hook_active`. `SessionStart` adds `source ∈ startup|resume|clear`. `SessionEnd` adds `reason ∈ exit|clear|logout|prompt_input_exit|other`. `Notification` adds `notification_type`, `message`, `details`. `PreCompress` adds `trigger ∈ auto|manual`.

`BeforeModel`/`AfterModel`/`BeforeToolSelection` are unusual and powerful — they expose and let you rewrite the raw `llm_request`/`llm_response`.

**Mapping for omt:** `BeforeAgent` ≈ `UserPromptSubmit`, `AfterAgent` ≈ `Stop`, `BeforeTool` ≈ `PreToolUse`, `AfterTool` ≈ `PostToolUse`, `PreCompress` ≈ `PreCompact`.

### 4.5 Slash commands, permissions, queueing, voice

- Commands: `~/.gemini/commands/**/*.toml` and `<project>/.gemini/commands/**/*.toml` (TOML with `description` + `prompt`, `{{args}}` substitution) — **note TOML, unlike Claude Code's Markdown**. Plus `gemini extensions` and `gemini skills`.
- Permissions: the **Policy Engine** (`--policy`, `--admin-policy`, `--approval-mode`). Approvals surface via the `Notification` hook and the `BeforeTool` `decision:"deny"` path.
- Queueing: **UNCERTAIN.**
- Voice/STT: none in the CLI. **UNCERTAIN.**

---

## 5. Cursor CLI (`cursor-agent`)

### 5.1 Detection

- Binary: `cursor-agent`. Node bundle at `/opt/homebrew/Caskroom/cursor-cli/<ver>/dist-package/index.js` (~9.5 MB).
- Env vars (extracted from the bundle, verified): `CURSOR_AGENT`, `CURSOR_CLI`, `CURSOR_CONVERSATION_ID`, `CURSOR_CONFIG_DIR`, `CURSOR_DATA_DIR`, `CURSOR_AGENT_STORE`, `CURSOR_AGENT_STORE_FILES_DIR`, `CURSOR_AGENT_WORKER_ID`, `CURSOR_API_KEY`, `CURSOR_AUTH_TOKEN`, `CURSOR_API_BASE_URL`, `CURSOR_AGENT_DISABLE_DEBUG_LOG`, `CURSOR_AGENT_DEBUG_PORT`, `CURSOR_ASKPASS`/`_SOCKET`/`_SECRET`, `CURSOR_FORCED_SHELL_EGRESS[_ALLOW|_DENY]_DOMAINS`.
  **`CURSOR_AGENT` + `CURSOR_CONVERSATION_ID` are the detection and correlation keys.**
- Debug logs: `$TMPDIR/cursor-agent-logs-<uid>/session-<ISO>-<pid>-<n>.log` (rotation: keeps newest 50, max age 7 days — decoded from the bundle). Suppressed by `CURSOR_AGENT_DISABLE_DEBUG_LOG`. Useful as a last-resort state source.

### 5.2 Machine-readable modes

Flags found in the bundle (verified): `--print`, `--json`, `--stream-partial-output`, `--with-diffs`, `--force`, `--resume`, `--continue`, `--auto-review`, `--approve-mcps`, `--authless`, `--list-models`, `--disable-indexing`, `--disable-codebase-ref`, `--disable-project-configs`, `--endless-retries`, `--network`, `--include-ui`, `--exclude-workspace-context`.

Documented usage is `cursor-agent -p "prompt" --output-format json|stream-json|text`. I could **not** confirm the literal string `--output-format` in the bundle (only `"json"` and `"text"` literals appear near the print path), so treat the exact spelling as **UNCERTAIN** — but `--print` + `--json` + `--stream-partial-output` are confirmed present.

`cursor-agent --help` on this machine **crashes with a raw JS dump** rather than printing help — worth knowing, since omt shouldn't shell out to it for capability probing.

### 5.3 Hooks

`~/.cursor/hooks.json` (verified local file):

```json
{"hooks":{"sessionStart":[{"command":"bash '/Users/hanxiao.du/.cursor/another tool-agent-state.sh' session"}]},"version":1}
```

**camelCase event names** (extracted from the bundle, verified): `sessionStart`, `sessionEnd`, `beforeSubmitPrompt`, `beforeShellExecution`, `beforeMCPExecution`, `beforeReadFile`, `afterFileEdit`, `stop`, `preCompact`.

Note there is a top-level `"version": 1` field — Cursor versions its hook config. Payloads are JSON on stdin, same general pattern; exact fields **UNCERTAIN**. `beforeShellExecution` / `beforeMCPExecution` returning a deny is the permission-interception point.

### 5.4 Session storage, permissions, commands

- `~/.cursor/chats/<workspace-hash>/<conversation-uuid>/store.db` — SQLite with schema `blobs(id TEXT PRIMARY KEY, data BLOB)` and `meta(key TEXT PRIMARY KEY, value TEXT)` (verified). **The transcript is opaque blobs**, so file-tailing is impractical; omt should rely on hooks + `--json` output for Cursor.
- Also `~/.cursor/agents/`, `~/.cursor/projects/`, `~/.cursor/prompt_history.json`, `~/.cursor/mcp.json`, `~/.cursor/skills-cursor/`, `~/.cursor/plugins/`, `~/.cursor/cli-config.json`.
- Commands: `.cursor/commands/*.md` and `.cursor/rules/*.mdc` (project-level).
- Permissions: `--force` (YOLO), plus per-tool hook gating. `CURSOR_ASKPASS_SOCKET` indicates an out-of-band credential-prompt channel.
- Queueing / voice: **UNCERTAIN.**

---

## 6. Amp (Sourcegraph)

- Binary: `amp`. Env: `AMP_API_KEY`, `AMP_REMOTE_CONTROL_TERMINAL`, `AMP_FORCE_BEL`, `AMP_SKIP_UPDATE_CHECK`.
- Modes:
  - `amp` — TUI.
  - `amp -x/--execute "prompt"` — one-shot, prints final response.
  - `amp --no-tui --runner-id <id>` — **runner mode: accepts remotely created threads with no TUI**. Directly relevant to omt's remote-client story.
  - `--stream-json` (requires `--execute`) — newline-delimited JSON message events. **Documented as Claude-Code-compatible**, with `--stream-json-thinking` adding thinking blocks and explicitly noted as *incompatible with Claude Code* when enabled. So Amp's default stream-json is a deliberate clone of Claude Code's schema — omt can share a parser.
  - `--stream-json-input` — newline-delimited JSON user messages on stdin (text + image blocks) for multi-turn.
  - `--remote-control-terminal` / `--no-remote-control-terminal` — web/app terminal interaction, gated by `AMP_REMOTE_CONTROL_TERMINAL=1|0`.
  - `--mcp-config`, `--settings-file <path>`, `--fast`, `--jetbrains`, `--plugin-ready-timeout`.
  - `amp mcp add|approve|oauth login`.
- Settings: `~/.config/amp/settings.json[c]`; workspace `.amp/settings.json[c]`; managed `/Library/Application Support/ampcode/managed-settings.json` (macOS), `/etc/ampcode/managed-settings.json` (Linux).
- **Threads are server-side** (`ampcode.com/feed`, referenced as `@T-<id>`). There is no rich local transcript file to tail — omt must use `--stream-json` or the Amp API. Confirmed by the manual: "persistent storage occurs server-side."
- Skills: `.agents/skills/*/mcp.json` for skill-bundled MCP servers.
- Hooks: not documented in the manual. **UNCERTAIN.**
- Slash commands: not documented as a distinct CLI feature; there is a command palette on `Ctrl+O`. **UNCERTAIN.**
- Queueing / voice: **UNCERTAIN.** (`Ctrl+G` opens `$EDITOR` for prompt composition.)

---

## 7. Aider

- Binary: `aider` (Python). Detection: process name `aider` or `python .../aider`; no documented env marker it *sets*. Reads `AIDER_*` env vars for every CLI flag (e.g. `AIDER_MODEL`, `AIDER_YES`) — so a wrapper can inject config, but that doesn't help detection. **Detection must fall back to argv/process name.**
- Machine-readable: **weakest of the set.** `--message/-m "..."` (single message then exit), `--message-file`, `--yes` (auto-confirm), `--dry-run`, `--auto-commits/--no-auto-commits`, `--commit`, `--exit`, `--show-repo-map`, `--show-prompts`. There is **no JSON/stream output mode**.
- Python API: `from aider.coders import Coder; Coder.create(main_model=Model("..."), fnames=[...]).run("...")` — explicitly documented as **"not officially supported or documented, and could change without backwards compatibility."**
- On-disk state (in the *repo root*, not a home directory): `.aider.chat.history.md` (Markdown transcript), `.aider.input.history` (raw input log), `.aider.tags.cache.v*/` (repo map cache), `.aider.model.settings.yml`, `.aider.conf.yml`. **UNCERTAIN** on exact current filenames per version — they have changed across releases; omt should glob `.aider.*` in the cwd.
- Hooks / slash-command files / MCP / voice: aider has in-REPL slash commands (`/add`, `/drop`, `/run`, `/undo`, `/voice`, `/web`, `/ask`, `/architect`) but they are **built-in, not file-defined**, so an external UI must hardcode the list. `/voice` exists and uses OpenAI Whisper via API (`--voice-language`, `--voice-format`, `--voice-input-device`) — the only CLI here with **BYOK STT already built in**, and a useful reference implementation.
- Permissions: confirmation prompts in the PTY only; `--yes` to bypass. Nothing structured on the wire.
- **omt strategy for aider: PTY heuristics + `.aider.chat.history.md` tailing only.** Treat it as tier-3.

---

## 8. Goose (block/goose)

- Binary: `goose` (Rust). Config `~/.config/goose/config.yaml`.
- Modes: `goose session` (interactive), **`goose run`** (headless: one user message in, one agent turn out, then exit — for CI), `goose run --recipe <file>` (parameterized recipes), `goose mcp`, `goose web` / server mode, `goose bench`.
  `--no-session` suppresses writing session state to disk. `--path <PATH>` selects a legacy file-based session (e.g. `./playground.jsonl`).
- **Session storage changed in v1.10.0: SQLite `sessions.db` replaced per-session `.jsonl` files.** Location is `~/.local/share/goose/sessions/` on Linux/macOS per XDG (docs also reference `~/.config/goose/sessions/`) — **UNCERTAIN** which is current; omt should probe both.
- Export/import: `SessionManager::get_session(id, true)` returns full state serializable to JSON; there is a JSON-based session export/import path.
- **ACP: `goose-acp-server` wraps the agent behind the Agent Client Protocol** — this is the recommended integration surface for omt.
- Hooks: **UNCERTAIN** — Goose's extensibility is MCP-extension-based rather than shell hooks.
- Slash commands: in-session (`/exit`, `/mode`, `/extension`, …), built-in. Recipes (`recipe.yaml`) are the file-defined analogue.
- Permissions: "goose mode" — `auto`, `approve`, `chat`, `smart_approve` — set in config or `--mode`. Approval prompts appear in the PTY; over ACP they become `session/request_permission`.
- Queueing / voice: **UNCERTAIN.**

---

## 9. Qwen Code (QwenLM/qwen-code)

- Binary: `qwen`, installed via `npm i -g @qwen-code/qwen-code`. **A direct fork of Gemini CLI**, adapted for Qwen3-Coder and any OpenAI-completions-compatible endpoint.
- Consequence: **assume Gemini CLI's entire surface applies** — `-p/--prompt`, `-o/--output-format text|json|stream-json`, `--approval-mode`, `--acp`, `--yolo`, `--session-id`, TOML slash commands, and (in recent versions) the hooks system. Config home is `~/.qwen/` (`settings.json`, `oauth_creds.json`, `tmp/<project-hash>/chats/*.json`).
- Env: `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `OPENAI_MODEL` (it reuses OpenAI-style vars). A `QWEN_*` marker is **UNCERTAIN**.
- **All of the above is inference from the fork relationship, not verified** — Qwen Code is not installed here and lags Gemini CLI by some releases. Mark tier-3 until tested.

---

## 10. Crush (charmbracelet)

- Binary: `crush` (Go). Data dir `~/.local/share/crush/` (or `--data-dir`), SQLite at `~/.local/share/crush/crush.db`.
- Modes: `crush` (TUI), **`crush run [prompt]`** (single prompt, non-interactive, also reads stdin), `--yolo` (bypass all permission prompts), `--json` for automation output, `-q/--quiet`, `-d/--debug`.
- Config: global `~/.config/crush/crush.json` + project `./crush.json` / `.crush.json`, with `$schema` support. LSP and MCP servers configured there.
- Session storage: SQLite (`crush.db`) with sessions/messages tables. Schema **UNCERTAIN** (not installed here).
- Hooks: **none documented. UNCERTAIN.**
- Slash commands: custom commands as Markdown under `.crush/commands/` and `~/.config/crush/commands/` with `$ARGUMENTS` placeholders — **UNCERTAIN** on exact paths.
- Permissions: TUI prompts; `--yolo` or `permissions.allowed_tools` in config.
- Queueing / voice: **UNCERTAIN.**

---

## 11. Cross-cutting: ACP (Agent Client Protocol)

Three of the ten agents speak ACP natively today: **opencode (`opencode acp`)**, **Gemini CLI (`gemini --acp`)**, **Goose (`goose-acp-server`)**. Qwen Code inherits it from Gemini. This makes ACP the single best *portable* integration for omt after Claude Code's hooks.

- Transport: **JSON-RPC 2.0 over stdio.** Methods (request/response) + Notifications (one-way).
- Agent methods: `initialize`, `authenticate`, `session/new`, `session/prompt`, `session/load` (optional), `session/cancel` (notification).
- Client methods (i.e. **what omt would implement**): `session/request_permission`, `fs/read_text_file`, `fs/write_text_file`, `terminal/create`, `terminal/output`, `terminal/wait_for_exit`, `terminal/kill`.
- `session/update` notification variants: `agent_message_chunk`, `tool_call`, `tool_call_update`, `plan`, `available_commands_update`, mode changes.
- Conventions: **all file paths MUST be absolute**; line numbers are 1-based; properties `camelCase`, discriminators `snake_case`.

`available_commands_update` is exactly the slash-command discovery mechanism omt needs for a remote UI, and `session/request_permission` is exactly the approval mechanism — both, portably, across three agents.

---

## 12. Synthesis: detection + state strategy for omt

### 12.1 Layered detection (highest confidence first)

**Layer 0 — omt spawns it, so omt knows.** In the common case the user types `omt claude` / `omt codex` and omt controls argv. omt should still verify, because a session can `exec` into something else, and because omt wants to support attaching to pre-existing PTYs.

**Layer 1 — process inspection of the PTY foreground process group.**
- Resolve the PTY's foreground pgid (`tcgetpgrp` on the master fd), then walk children.
- On macOS, read argv + environ via `sysctl KERN_PROCARGS2`; on Linux, `/proc/<pid>/cmdline` + `/proc/<pid>/environ`. (`libproc`'s `proc_pidinfo` is the portable-ish macOS route; the `sysctl` path is what gives you **environ**, which is the whole point.)
- Classify by env marker first, exe path second, argv last:

| Agent | Primary env marker | Session-id env | Exe/argv fallback |
|---|---|---|---|
| Claude Code | `CLAUDECODE=1` + `CLAUDE_CODE_ENTRYPOINT` | `CLAUDE_CODE_SESSION_ID` | `claude`, or exe under `~/.local/share/claude/versions/` |
| Codex | `CODEX_HOME` / `CODEX_SANDBOX` | `CODEX_THREAD_ID` | `codex` |
| opencode | `OPENCODE_CONFIG_DIR` / `OPENCODE_CLIENT` | — (use server API) | `opencode` |
| Gemini CLI | `GEMINI_CLI_HOME` (weak) | — (use `--session-id`) | `node .../gemini-cli/bundle/gemini.js` |
| Cursor | `CURSOR_AGENT` | `CURSOR_CONVERSATION_ID` | `cursor-agent` |
| Amp | `AMP_API_KEY` (weak) | — | `amp` |
| Aider | none | — | `aider` / `python .../aider` |
| Goose | none confirmed | — | `goose` |
| Crush | none confirmed | — | `crush` |
| Qwen | none confirmed | — | `qwen` |

  Env markers beat process names because four of the ten ship as Node bundles that appear as `node`.

**Layer 2 — omt-injected environment + OSC markers.** Because omt spawns the PTY, it should inject its own correlation env (`OMT_SESSION=<uuid>`, `OMT_SOCK=/path`) so that *any* hook, plugin, or wrapper the agent runs can phone home already knowing which omt pane it belongs to. Complementarily, Claude Code hooks can emit `terminalSequence` (OSC 777/9/99) which omt's terminal parser intercepts — a zero-dependency backchannel that survives when the socket doesn't. Codex/Cursor/Gemini hooks can `printf` OSC to their own stdout for the same effect, though whether that reaches omt's PTY depends on how the CLI wires hook stdout (**UNCERTAIN** — verify per agent).

**Layer 3 — hooks / plugins (the workhorse).** Coverage is genuinely good:
- Claude Code: 30 events, `PreToolUse` with `permissionDecision:"defer"`.
- Codex: same 8-event Claude-Code vocabulary in `~/.codex/hooks.json` (note the hook-trust gate).
- Gemini CLI + Qwen: 11 events, `BeforeTool`/`AfterTool`/`BeforeAgent`/`AfterAgent`.
- Cursor: 9 camelCase events in `~/.cursor/hooks.json`.
- opencode: JS plugin API (`tool.execute.before/after`, `permission.ask`, `event`).
- Amp / Aider / Goose / Crush: nothing. Fall through to layers 4–5.

  omt should ship **one hook binary** (`omt-agent-hook`) that reads JSON on stdin, normalizes it (see 12.2), POSTs to `$OMT_SOCK`, and echoes back either `{}` or a decision omt's remote client supplied. Install it into all four hook config files, merging rather than overwriting (users already have hooks — this machine has `another tool-agent-state.sh` wired into all three of `~/.claude/settings.json`, `~/.codex/hooks.json`, `~/.cursor/hooks.json`).

**Layer 4 — protocol/API attach where available.** Prefer this over file-tailing when the agent offers it:
- opencode: `opencode serve` REST+SSE, or `opencode acp`.
- Codex: `codex app-server` (+ `generate-json-schema` to derive types), `codex --remote unix://…`.
- Gemini/Qwen/Goose: ACP.
- Amp: `--stream-json` + `--no-tui --runner-id`.
- Claude Code: `--remote-control`, or stream-json with `--input-format stream-json`.

**Layer 5 — transcript-file tailing.** Reliable, retroactive, and works for TUI sessions where no hook fired:

| Agent | Path | Format |
|---|---|---|
| Claude Code | `~/.claude/projects/<cwd-dashed>/<uuid>.jsonl` | JSONL, rich |
| Codex | `~/.codex/sessions/YYYY/MM/DD/rollout-<ISO>-<uuid>.jsonl` + `~/.codex/session_index.jsonl` | JSONL, rich |
| opencode | `~/.local/share/opencode/opencode.db` (SQLite `event`/`message`/`part`) | queryable, rich |
| Gemini/Qwen | `~/.gemini/tmp/<sha256(cwd)>/chats/session-*.json` | JSON, rewritten whole |
| Cursor | `~/.cursor/chats/<hash>/<uuid>/store.db` | **opaque blobs — unusable** |
| Goose | `~/.local/share/goose/sessions/sessions.db` (or `~/.config/goose/sessions/`) | SQLite, **UNCERTAIN** |
| Crush | `~/.local/share/crush/crush.db` | SQLite, **UNCERTAIN** |
| Aider | `<repo>/.aider.chat.history.md` | Markdown |
| Amp | none local | server-side |

  Implementation notes: use `notify`/FSEvents on the parent directory (not the file) so newly created sessions are caught; keep a byte offset per file and handle truncation-on-rewrite for Gemini's whole-file JSON; for SQLite, prefer polling `max(rowid)`/`seq` over WAL parsing, and open **read-only with `immutable=0`** to avoid interfering with the agent's writers.

**Layer 6 — PTY output heuristics (fallback only).** For Aider, Amp, Crush, Goose in TUI mode, and any agent whose hooks the user refused to install. Practical signals, in rough order of robustness:
- **Cursor position + alternate screen** (`\e[?1049h`) tells you the agent is running a full-screen TUI.
- **Bracketed-paste and DECSET/DECRST** transitions around input prompts.
- **Idle/busy from spinner animation**: most of these TUIs animate a braille/dot spinner at 80–120 ms; detecting repeated same-line rewrites in that cadence is a decent `busy` signal, and its cessation plus a redrawn prompt row is `idle`.
- **The prompt glyph on the last non-empty row** (`> ` for Claude Code/Amp, `›`/`▌` variants elsewhere).
- **BEL / OSC 9 / OSC 777** — several agents already ring the bell when they need attention; treat any BEL as `needs_attention`.
- Do **not** try to parse tool calls or question cards out of ANSI. It is fragile across versions and locales, and the whole point of layers 3–5 is to avoid it. Heuristics should only ever produce `busy | idle | needs_attention | unknown`.

**Recommended composition.** Run layers 1, 3, 5, 6 concurrently and merge into one state machine per pane, with a per-source confidence and a monotonic sequence number. Rule: a higher layer's state always wins within a staleness window (say 5 s); heuristics only fill gaps. Correlate everything on `(omt_session_id, agent_session_id)` — obtainable from env (Claude/Codex/Cursor), from `--session-id` that omt mints (Gemini/Claude), or from the `SessionStart` hook payload.

### 12.2 Proposed normalized `AgentEvent` model

```rust
struct AgentEvent {
    // --- identity ---
    omt_session: Uuid,            // omt pane/session
    agent: AgentKind,             // ClaudeCode | Codex | OpenCode | Gemini | Cursor | Amp | Aider | Goose | Qwen | Crush | Unknown
    agent_version: Option<String>,
    agent_session: Option<String>,// native session id (uuid / ses_xxx / thread id)
    thread: ThreadRef,            // { id, parent: Option<id>, is_subagent: bool, label: Option<String> }
    seq: u64,                     // monotonic per omt_session
    ts: DateTime<Utc>,
    source: Source,               // Hook | Protocol | Transcript | Pty | Process   (== confidence tier)
    cwd: PathBuf,
    git_branch: Option<String>,

    payload: Payload,
}

enum Payload {
    // ---- lifecycle ----
    SessionStart { reason: StartReason /* Startup|Resume|Clear|Compact|Fork */, model: Option<String>,
                   permission_mode: Option<PermissionMode>, transcript_path: Option<PathBuf> },
    SessionEnd   { reason: String },
    Capabilities { tools: Vec<String>, slash_commands: Vec<SlashCommand>,
                   mcp_servers: Vec<McpServer /* {name, status: Ok|Pending|Failed|NeedsAuth} */>,
                   models: Vec<String>, permission_modes: Vec<PermissionMode> },

    // ---- turn state (drives the pane badge) ----
    TurnStart    { turn_id: Option<String>, trigger: Trigger /* Human|Queued|Hook|Remote */ },
    TurnEnd      { turn_id: Option<String>, outcome: Outcome /* Completed|Aborted|Error */,
                   last_message: Option<String>, turn_count: Option<u32> },
    Thinking     { delta_tokens: Option<u64>, text: Option<String> },
    Usage        { input: u64, output: u64, cache_read: u64, cache_write: u64, reasoning: u64,
                   context_window: Option<u64>, cost_usd: Option<f64> },
    RateLimit    { status: String, resets_at: Option<DateTime<Utc>>, kind: Option<String> },
    Compaction   { phase: Phase /* Before|After */, trigger: String, token_count: Option<u64> },

    // ---- content ----
    UserMessage  { text: String, origin: Origin /* Human|Queue|Remote|Synthetic */, images: Vec<Blob> },
    AssistantText{ text: String, partial: bool, phase: Option<String> /* codex: commentary|final */ },
    Reasoning    { text: String, partial: bool },

    // ---- tools ----
    ToolCall     { call_id: String, name: String, input: serde_json::Value,
                   status: ToolStatus /* Pending|Running|Completed|Error */, parent_call_id: Option<String> },
    ToolResult   { call_id: String, output: ToolOutput, error: Option<String>, duration_ms: Option<u64> },
    Plan         { steps: Vec<PlanStep /* {title, status} */> },
    FileChanged  { path: PathBuf, change: Change /* Created|Modified|Deleted */, diff: Option<String> },

    // ---- interaction: THE core of omt's remote-mirroring feature ----
    Interaction  { id: String, kind: InteractionKind, prompt: String, timeout_ms: Option<u64> },
    InteractionResolved { id: String, response: InteractionResponse, by: Actor /* Local|Remote|Timeout|Policy */ },

    // ---- queue ----
    QueueChanged { op: QueueOp /* Enqueue|Remove|Consume */, index: Option<usize>, text: String,
                   pending: Vec<String> },

    // ---- fallback ----
    Notification { kind: String, message: String },
    RawPty       { state_guess: PtyState /* Busy|Idle|NeedsAttention|Unknown */ },
}

enum InteractionKind {
    /// Claude Code AskUserQuestion; ACP's structured elicitation; MCP elicitation
    Choice { questions: Vec<ChoiceQuestion> },   // { question, header, multi_select, options:[{label, description}] }
    /// Tool/command approval
    Permission { tool: String, input: serde_json::Value,
                 suggestions: Vec<PermissionOption /* Allow|AllowAlways|Deny|DenyAlways|Edit */>,
                 diff: Option<String>, command: Option<String> },
    /// Free-text follow-up
    Text { placeholder: Option<String>, multiline: bool },
    /// Plan / mode confirmation
    PlanReview { plan: String },
}

enum InteractionResponse {
    Choices(Vec<Vec<String>>),       // per-question selected labels
    Permission { decision: PermissionDecision, updated_input: Option<serde_json::Value>, reason: Option<String> },
    Text(String),
    Cancelled,
}
```

**Why this shape.**
- `Interaction` / `InteractionResolved` is deliberately a *request/response pair with an id*, because that's the only structure that works across all four mechanisms omt must bridge: Claude Code's `AskUserQuestion` tool + `PreToolUse{defer}`, ACP's `session/request_permission`, opencode's `permission.ask` plugin hook, and MCP elicitation. The remote web client renders `InteractionKind` natively and posts back an `InteractionResolved`.
- `Choice` maps 1:1 onto the verified `AskUserQuestion` schema (`question`/`header`/`multiSelect`/`options[{label,description}]`), so no lossy translation for the flagship case.
- `source: Source` doubles as the confidence tier for the layer-merge rule in 12.1.
- `thread.is_subagent` unifies Claude Code's `isSidechain`/`parent_tool_use_id`, Codex's `thread_source:"subagent"` + `agent_nickname`, and opencode's `session.parent_id`.
- `QueueChanged` exists because Claude Code alone gives it to us cleanly (verified `queue-operation` lines) and it's a visible differentiator; other agents simply never emit it.

### 12.3 Concrete per-agent mapping (abridged)

| `AgentEvent` | Claude Code | Codex | opencode | Gemini/Qwen | Cursor |
|---|---|---|---|---|---|
| `SessionStart` | hook `SessionStart` / `system.init` | `session_meta` | `session.created.1` | hook `SessionStart` | hook `sessionStart` |
| `Capabilities` | `system.init` (`tools`,`slash_commands`,`mcp_servers`) | app-server `initialize` | `GET /session` + config | ACP `available_commands_update` | — (probe disk) |
| `TurnStart` | hook `UserPromptSubmit` | `event_msg.task_started` | `message.updated.1` (assistant created) | hook `BeforeAgent` | hook `beforeSubmitPrompt` |
| `TurnEnd` | hook `Stop` (`last_assistant_message`,`turn_count`) | `task_complete` / `turn_aborted` | `step-finish` part | hook `AfterAgent` | hook `stop` |
| `ToolCall` | `PreToolUse` / transcript `tool_use` | `response_item.function_call` / `custom_tool_call` | `part.type=tool` `state.status=pending\|running` | hook `BeforeTool` | `beforeShellExecution`/`beforeMCPExecution` |
| `ToolResult` | `PostToolUse` (`tool_output`,`duration_ms`) | `function_call_output` | `state.status=completed\|error` | hook `AfterTool` (`tool_response`) | `afterFileEdit` |
| `Interaction::Choice` | `PreToolUse{AskUserQuestion}` → defer | **n/a** | plugin `chat.message` | **n/a** | **n/a** |
| `Interaction::Permission` | `PermissionRequest` / `Notification{permission_prompt}` | approval `event_msg` / app-server | plugin `permission.ask` | hook `BeforeTool` deny | `beforeShellExecution` |
| `Usage` | `assistant.message.usage`, `result.modelUsage` | `event_msg.token_count` | `session.tokens_*`, `message.tokens` | **UNCERTAIN** | **UNCERTAIN** |
| `Compaction` | `PreCompact`/`PostCompact` | `context_compacted` | `part.type=compaction` | `PreCompress` | `preCompact` |
| `QueueChanged` | `queue-operation` transcript lines | **UNCERTAIN** | `session_input` table (**UNCERTAIN**) | **UNCERTAIN** | **UNCERTAIN** |

`Interaction::Choice` is a **Claude-Code-only capability today**. That is a strategic point, not a gap: omt's differentiator is native remote rendering of Claude Code question cards, and for every other agent omt degrades to `Interaction::Permission` and `Interaction::Text`, which are universal.

### 12.4 Voice/STT recommendation

No CLI here exposes a reusable STT service. Claude Code's `/voice` is first-party-account-gated, local-mic-only, and explicitly non-functional over SSH — i.e. structurally incompatible with omt's remote client. Aider's `/voice` (OpenAI Whisper API, `--voice-language`, `--voice-format`) is the only BYOK precedent and a good reference.

Recommended for omt: capture in the browser, transcribe server-side in omt, inject as text.
- **Default:** Deepgram streaming (`wss://api.deepgram.com/v1/listen?model=nova-3&interim_results=true`) — sub-300 ms interim results, so the web client can show dimmed live text exactly like Claude Code does.
- **Simple/cheap:** OpenAI `POST /v1/audio/transcriptions` with `gpt-4o-transcribe` or `whisper-1` (batch, one request per utterance).
- **Offline/privacy:** `whisper.cpp` (`ggml-large-v3-turbo`) or `faster-whisper` running in-process next to omt's daemon.
- Reuse Claude Code's UX decisions, which are well-tested: hold-to-talk and tap-to-send modes, a ≥3-word auto-submit threshold, 15 s silence / 2 min hard stop, and biasing the recognizer with the project name and git branch as hints (measurably improves repo-name accuracy).

### 12.5 Open questions to resolve before implementation

1. Does Claude Code's `PreToolUse` `permissionDecision:"defer"` actually park the tool call long enough for a remote round-trip, and what is the timeout? (Not documented; needs an experiment.) **This gates the flagship feature.**
2. Exact hook stdin payload field names for Codex and Cursor — assume Claude-Code-shaped, verify with a logging hook.
3. Whether hook stdout OSC (`terminalSequence` equivalents) reaches omt's PTY for Codex/Cursor/Gemini, or is swallowed.
4. Codex `codex app-server generate-json-schema` output — dump it and generate Rust types; also determine whether approval requests come through it as first-class methods.
5. Gemini CLI session-file message schema (my only local sample was an auth-failure session).
6. opencode's `session_input` table semantics (is it the type-ahead queue?), and the plugin event catalogue.
7. Goose session DB location (`~/.local/share/goose/` vs `~/.config/goose/`) and schema; Crush `crush.db` schema.
8. Whether Amp's `--stream-json` is byte-compatible enough with Claude Code's that a single parser suffices, or only structurally similar.
9. Cursor `--output-format` exact spelling (bundle shows `--print`, `--json`, `--stream-partial-output` but not the literal `--output-format`).
10. Aider's current `.aider.*` filenames per version.

---

## 13. GitHub Copilot CLI

Verified against `GitHub Copilot CLI 0.0.398` installed on this machine, 2026-08.

### 13.1 Detection

`copilot` on PATH, a Node bundle. Env markers `COPILOT_AGENT`,
`GH_COPILOT_TOKEN`, `COPILOT_MODEL`; argv carries `@github/copilot`.

### 13.2 Machine-readable modes

**`copilot --acp` starts an Agent Client Protocol server.** This is in its own
`--help` and is the reason Copilot reaches protocol tier rather than hook tier.
Also present: `--resume [sessionId]`, `--continue`, `--log-dir`, `--log-level`,
and `--share`/`--share-gist` for exporting a session.

### 13.3 Hooks

`~/.copilot/hooks/*.json`, one file per integration — this machine has
`another tool.json` alongside a `another tool-agent-state.sh`, which is how both of those tools
attach. The vocabulary is Claude Code's plus four:

| | |
|---|---|
| Shared with Claude Code | `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `Stop`, `SubagentStop`, `PermissionRequest`, `Notification` |
| Copilot's own | `SessionEnd`, `ErrorOccurred`, `PreCompact`, `subagentStart` |

`subagentStart` is lowercase-s in the file on disk. That is not a transcription
error here: an adapter that "corrects" it stops seeing subagents start, and does
so silently.

`ErrorOccurred` matters more than it looks. Claude Code has no equivalent, so a
Claude-shaped adapter infers failure from a missing `Stop` — with Copilot it is
told, and a turn that ended badly can be red rather than green.

### 13.4 Interactions

`PermissionRequest` carries the options. They are taken verbatim: omt neither
adds, removes nor reorders, because the labels are how an answer is delivered.
A payload whose options cannot all be read raises no card at all — presenting
three of four means somebody chooses without ever seeing the fourth.

---

## 14. Gemini CLI: the hook names are not Claude Code's

§4.4 above says Gemini's hooks are "Claude-Code-compatible by design". Verified
against `gemini 0.46.0` and a real `~/.gemini/settings.json`, that is not what
compatible means here.

**Gemini's native hook events are `BeforeAgent`, `AfterAgent`, `BeforeTool`,
`AfterTool`.** Four events, none spelled the way Claude Code spells them. What
exists is `gemini hooks migrate`, which *translates* a Claude configuration
across — so a user who ran it has Claude's names and a user who did not has
Gemini's.

omt's adapter accepts both, because an adapter that knew only one vocabulary
would work for exactly half the users and there is no way to tell which half
from the outside.

The payload field names differ too: Gemini says `args` and `call_id` where a
migrated Claude config says `tool_input` and `tool_use_id`.

ACP is `gemini --acp`. `--experimental-acp` still exists and the binary's own
help marks it deprecated, so the adapter spawns the former.

---

## 15. What this leaves

Every CLI in this document now has an adapter. The ones with a dedicated
adapter — Claude Code, Codex, Copilot, Cursor, Gemini — have knowledge that is
theirs alone: an environment marker, a hook vocabulary, a protocol flag. The
rest are covered by the generic ACP client or, where there is nothing to speak
to, by the heuristic floor, which says so rather than pretending.

The dedicated adapters are registered *after* the generic ones, because
registration replaces by kind and a dedicated adapter must win. There is a test
for that, because a comment would not survive somebody tidying the function.
