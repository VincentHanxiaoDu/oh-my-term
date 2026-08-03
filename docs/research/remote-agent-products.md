# Remote Agent-Control Products: Competitive Survey

Research for **oh-my-term (`omt`)**. The question this document answers: a whole
category of "drive your coding agent from your phone" products already exists —
what do they actually do, how do they do it, what do users love and hate, and
what is genuinely still missing?

**Date:** 2026-08-03. **Method:** GitHub source reads (raw files and file
listings), official docs, and community discussion on HN. Every claim is tagged:

- **[V]** VERIFIED-from-source — I (or a delegated agent) read the code; file path cited.
- **[D]** DOCUMENTED — official docs or README.
- **[C]** COMMUNITY-REPORT — HN/Reddit/blog/press; treat as sentiment, not fact.
- **[?]** could not confirm; stated as unknown rather than guessed.

**Known gaps in this research.** reddit.com and x.com were not fetchable from
this environment, so all Reddit/X sentiment reaches us secondhand through HN or
review sites and is marked accordingly. `openai.com/index/work-with-codex-from-anywhere/`
returned 403, so every Codex Remote GA detail is press-sourced. Neither
Anthropic nor OpenAI discloses its cloud isolation primitive. Conductor,
Cursor, and Devin are closed source and their wire protocols are unknown.

---

## 0. Executive map of the category

There are two architectural camps, and almost nobody is in the middle.

**Camp 1 — headless stream-json in a cloud sandbox.** Run `claude -p
--output-format stream-json` (or the vendor's equivalent) inside a container or
VM, render the resulting event stream as chat + diff cards on the phone. There
is no terminal, and there cannot be one. Members: Terragon, Conductor Cloud,
Claude Code on the web, Codex cloud, Cursor cloud agents, Devin.

**Camp 2 — real PTY plus git worktrees on your own machine.** Full fidelity,
full local trust, and phone-hostile. Members: Sculptor, Conductor local, Droid
CLI, Claude Code Desktop, crystal, claude-squad, uzi, container-use.

**The bridge products** — happy, Omnara, another terminal remote-control, Anthropic's own
Remote Control, Codex Remote — run the agent locally but stream *structured
state*, not bytes, to the phone. This is the fastest-moving part of the
category and the part omt competes with most directly.

The single most load-bearing finding: **nobody verified-ships a real
interactive PTY on a phone as a usable primary interface.** VibeTunnel and
Omnara come closest (both do stream a real terminal to a mobile browser) and
both are widely reported as awkward on touch. Everyone else either has no
terminal at all on mobile, or a terminal only on desktop.

---

## 1. happy / happy-coder — the closest thing to omt that exists

The most important entry in this survey. Read it first.

### 1.1 What it is

`slopus/happy` — **23,053★, 1,947 forks, MIT, TypeScript**, created 2025-07-18,
**last push 2026-08-03** (today) [V, GitHub API]. Self-described: "Mobile and Web
client for Codex and Claude Code, with realtime voice, encryption and fully
featured."

Critical correction to the received wisdom: `slopus/happy-cli` (558★) and
`slopus/happy-server` (367★) were **frozen on 2026-02-14** and absorbed into the
`slopus/happy` monorepo [V]. Anyone evaluating happy by reading `happy-cli`
standalone — as several HN commenters clearly did — is reading a six-month-stale
fork and will reach wrong conclusions. In particular the widely repeated
"happy can't handle Claude's new question format" complaint is **fixed** in the
monorepo (see §1.4).

Monorepo packages [V `packages/`]: `happy-app` (Expo/RN + web + Tauri desktop),
`happy-cli` (npm `happy`, v1.2.0), `happy-agent`, `happy-server` (npm
`happy-server-self-host`), `happy-wire` (shared protocol types), `codium` (a new
Electron IDE), `happy-app-logs`. In-repo docs: `docs/encryption.md`,
`docs/realtime-sync-and-rpc.md`, `docs/permission-resolution.md`,
`docs/voice-architecture.md`, and — telling — a `docs/competition/` folder.

It is not a fork of Claude Code and not a hosted agent. It is a wrapper plus a
relay plus a client, self-hostable end to end.

### 1.2 Technical mechanism — two modes, neither a PTY

**Remote mode** [V `packages/happy-cli/src/claude/claudeRemote.ts`, 384 lines]:
happy uses the **official Claude Agent SDK** (`@anthropic-ai/claude-agent-sdk
^0.3.179` [V `package.json`]), calling `query({ prompt: messages, options:
sdkOptions })` at line 168. Options include `cwd, resume, mcpServers,
permissionMode, model, customSystemPrompt, appendSystemPrompt, allowedTools,
disallowedTools, canCallTool, abort, settingsPath`. It points the SDK at its own
launcher shim via `pathToClaudeCodeExecutable` rather than the global `claude`.
happy vendored a copy of the SDK's query layer, so the exact spawned flags are
readable in-repo [V `happy-cli@main/src/claude/sdk/query.ts`]: `claude
--output-format stream-json --verbose --input-format stream-json`, plus
`--permission-prompt-tool stdio` when `canCallTool` is supplied.

So remote mode is **stream-json in both directions with permissions on the SDK's
stdio control channel** — notably *not* an MCP `--permission-prompt-tool
mcp__x__y`. `--resume <uuid>` is supported; bare `--resume` (the interactive
picker) is explicitly unsupported [V lines 58–77].

**Local mode** [V `packages/happy-cli/src/claude/claudeLocal.ts`, 431 lines]:
the user keeps a real interactive Claude TUI in their own terminal and happy
mirrors it to the phone. There is **no node-pty**. It uses `cross-spawn` with
`stdio: ['inherit','inherit','inherit','pipe']` — inheriting the user's real TTY
and adding **fd 3** as a side channel [V line 316]. The command is `node
scripts/claude_local_launcher.cjs [args]` with `--append-system-prompt`,
`--mcp-config`, `--allowedTools`, `--settings <hookSettingsPath>`, and
`--resume|--session-id <uuid>` [V lines 215–249].

**The launcher shim is the cleverest single mechanism in this survey**
[V `packages/happy-cli/scripts/claude_local_launcher.cjs`]. It sets
`DISABLE_AUTOUPDATER=1`, **monkey-patches `global.fetch`** to emit
`{type:'fetch-start'|'fetch-end', id, hostname, path, method, timestamp}` JSON
lines to fd 3, then in-processes the user's globally installed `claude` via
`require('./claude_version_utils.cjs').runClaudeCli(getClaudeCliPath())`. Only
hostname and path are emitted, never bodies — deliberately privacy-conscious.

**Hooks:** exactly one [V `src/claude/utils/generateHookSettings.ts`]. It writes
`~/.happy/tmp/hooks/session-hook-<pid>.json` with a `SessionStart` hook (matcher
`"*"`) running `node scripts/session_hook_forwarder.cjs <port>`, passed via
`--settings`. Its sole purpose is learning the real Claude session id after
`--continue`/`--resume`/compaction. **No `PreToolUse`, no `Stop`, no
`Notification` hooks.** This is a significant gap relative to what the hook
surface offers, and one omt's design already exploits harder.

**happy's own MCP server** [V `src/claude/utils/startHappyServer.ts`] is a
stateless StreamableHTTP `McpServer` exposing exactly **one tool:
`change_title`**, so Claude can name the chat for the phone's session list. It
is not a permission tool.

No use of `--remote-control` anywhere.

### 1.3 State capture

- **Thinking (local mode)** is inferred from the fd-3 fetch events: `fetch-start`
  → `activeFetches.set(...)`, `updateThinking(true)`; `fetch-end` → when the set
  empties, `setTimeout(() => updateThinking(false), 500)` [V `claudeLocal.ts`].
  In other words, **"thinking" means an in-flight HTTP request from the Claude
  process.** Elegant, high-signal, and completely free of PTY scraping.
- **Content** comes from transcript tailing [V `src/claude/utils/sessionScanner.ts`]:
  watches `~/.claude/projects/<slug>/<sessionId>.jsonl`, parses each line against
  `RawJSONLinesSchema`, dedupes by `messageKey()`, and handles session-id
  switching on compaction/fork with a `deadSessions` blacklist for phantom ids
  whose `.jsonl` never materializes.
- **Fork backfill** [V `src/claude/runClaude.ts` ~295–331]: on resume the SDK
  reads the JSONL silently and never re-emits it, so happy reads the file itself
  and replays every line to the phone, content-deduping against SDK-emitted
  messages. This is a real-world bug class omt will hit too.
- **Blocked** = a pending entry in `PermissionHandler.pendingRequests`.

This is a tier-3/4/5 architecture in omt's own vocabulary. **happy has
independently arrived at roughly omt's source model, minus hooks.**

### 1.4 Permission prompts and AskUserQuestion — the part that matters most

[V `packages/happy-cli/src/claude/utils/permissionHandler.ts`, 443 lines]

The entry point is the SDK's `canCallTool(toolName, input, mode, options)`
callback, returning `{behavior:'allow', updatedInput}` or `{behavior:'deny',
message}`. Note `updatedInput` is merged as `{...originalInput,
...response.updatedInput}` [V line 131] — **the phone can edit the tool
arguments before approving.** The denial message reproduces Claude Code's
verbatim rejection string [V line 136].

**`AskUserQuestion` is special-cased and never auto-approved, even under
`bypassPermissions`/yolo** [V lines 149–151]. There is a dedicated notification
path [V `src/claude/utils/questionNotification.ts`, `getAskUserQuestionToolCallIds`]
and a **dedicated phone renderer**
[V `packages/happy-app/sources/components/tools/views/AskUserQuestionView.tsx`],
registered in `components/tools/knownTools.tsx` / `_all.tsx`, with i18n strings
in 11+ languages.

**This is decisive for omt's positioning: native remote rendering of
AskUserQuestion cards is already shipped, in a 23k-star MIT project, in eleven
languages.** It is not a differentiator. See §12(b).

`ExitPlanMode` is also never auto-approved [V line 107]; on approval happy
switches the SDK permission mode *first*, then allows — replacing an older
`PLAN_FAKE_RESTART` sentinel-injection hack. There is a session-scoped
`allowedTools: Set<string>` for "don't ask again", and Bash requests are parsed
into literal-command or prefix patterns (`parseBashPermission`) for granular
allowlisting.

**Answer injection is not keystrokes.** It is a typed RPC: the app calls the
`'permission'` handler registered at
`session.client.rpcHandlerManager.registerHandler<PermissionResponse, void>`
[V line 367], which resolves the promise the SDK is awaiting. Clean, exactly
what omt intends.

Default permission mode is **`yolo`** [V `runClaude.ts:60`
`DEFAULT_CLAUDE_PERMISSION_MODE = 'yolo'`], gated by
`applySandboxPermissionPolicy`; `@anthropic-ai/sandbox-runtime ^0.0.37` is a
dependency.

### 1.5 Transport, auth, encryption

**Socket.IO over WebSocket**, one endpoint `/v1/updates`, three scopes:
user / session / machine [V `docs/realtime-sync-and-rpc.md`; server
`sources/app/api/socket.ts`, `eventRouter.ts`, `rpcHandler.ts`]. RPC: caller
emits `rpc-call`, server resolves room `rpc:<userId>:<method>`, forwards as
`rpc-request`, acks the result. One daemon per machine holds a machine-scoped
socket; the CLI opens caller sockets to `spawn`/`resume` sessions remotely.

**Crypto** [V `docs/encryption.md`, `src/api/encryption.ts`,
`packages/happy-agent/src/encryption.ts`]:

- Legacy: `tweetnacl.secretbox` (XSalsa20-Poly1305), layout `[nonce(24) | ct+tag]`.
- Current "dataKey": **AES-256-GCM**, layout `[version(1) | nonce(12) | ct | tag(16)]`,
  per-session or per-machine content keys.
- Content key wrapped with `tweetnacl.box` + ephemeral keypair:
  `[ephPub(32) | nonce(24) | ct]`, version-byte prefixed, base64.
- Key tree is **BIP32-style HMAC-SHA512** [V `src/utils/deriveKey.ts`]:
  `deriveSecretKeyTreeRoot(seed, usage)` = `HMAC(usage + ' Master Seed', seed)`
  → `{key: I[0:32], chainCode: I[32:]}`; children `HMAC(chainCode, 0x00||index)`.
- Auth is challenge/response with `tweetnacl.sign.detached` → `POST /v1/auth`
  [V `src/api/auth.ts`].
- **Pairing:** the CLI prints a QR encoding `handy://<base64url(secret)>`
  [V `auth.ts:generateAppUrl`] — **the QR literally carries the root secret**;
  the phone scans and derives everything. Simple, and a real threat-model
  consideration (shoulder-surf, screenshot, screen-share).
- App-side crypto is **libsodium** (`@more-tech/react-native-libsodium`,
  `libsodium-wrappers`, `rn-encryption`), not tweetnacl.
- The server stores session metadata, messages, machine state, artifacts, and KV
  as **opaque base64**. Server-side encryption exists only for third-party
  tokens (GitHub OAuth) under a KeyTree from `HANDY_MASTER_SECRET` — not E2E.

**Self-hosting: yes, genuinely.** `HAPPY_SERVER_URL` (default
`https://api.cluster-fluster.com`), `HAPPY_WEBAPP_URL` (default
`https://app.happy.engineering`), `HAPPY_HOME_DIR`, `HAPPY_EXPERIMENTAL`,
`HAPPY_DISABLE_CAFFEINATE`, `HAPPY_VARIANT` [V `src/configuration.ts`]. The
server package is literally `happy-server-self-host`; deps show Prisma +
**PGlite** (`@electric-sql/pglite`, `pglite-prisma-adapter`) — embedded
Postgres, i.e. a single-binary self-host target. Redis
(`@socket.io/redis-streams-adapter`) only for scale-out; MinIO for blobs.

### 1.6 Mobile UX

Expo 55 / RN 0.83 / React 19, `react-native-web` for web, **Tauri 2** for
desktop — one codebase, three targets. State: zustand + MMKV.

The phone renders **structured cards, not a terminal**: per-tool renderers under
`components/tools/views/*` (`AskUserQuestionView`, `PermissionPrompt`),
`ToolGroupView`, diffs (`@pierre/diffs`, `react-native-diff-view`), a
**CodeMirror file editor** with 14 language packs at `session/[id]/file.tsx` and
`files.tsx`, mermaid rendering, syntax highlighting, seti file icons.

**There is no terminal emulator in the mobile app.** `app/(app)/terminal/index.tsx`
and `terminal/connect.tsx` are the QR/deep-link *pairing* screens (they read a
`publicKey` from search params via `useConnectTerminal`) — the naming misleads.
The only real terminal is in the new Electron `codium` package (`node-pty
^1.1.0`, `@wterm/dom`/`@wterm/react`, `components/terminal/TerminalHost.tsx`,
`TerminalPane.tsx`, `TerminalPreview.tsx`) [V]. **Terminal = desktop only.**

**Voice is a first-class realtime agent** [V `docs/voice-architecture.md`,
`sources/realtime/*`]: **ElevenLabs** (`@elevenlabs/react`,
`@elevenlabs/react-native`) over **LiveKit + WebRTC**. A module-level
`currentSessionId` routes voice tool calls; the voice agent is given the tools
`messageClaudeCode` and **`processPermissionRequest`**
[V `realtimeClientTools.ts`] — **you can approve a permission prompt by voice.**
Server has `voiceRoutes.ts`, an `elevenlabs` dep, and a `voice_conversation`
migration dated 2026-04-07. With `docs/paid-voice.md` and
`docs/plans/elevenlabs-voice-usage-gating.md`, **voice is the monetization
surface.**

Multi-session and multi-machine are real: `session/recent.tsx`, `machine/[id].tsx`,
a per-machine daemon with `spawn`/`resume` RPC, `EmptySessionsTablet` split
view, daemon install at `src/daemon/install.ts` with a macOS LaunchAgent under
`src/daemon/mac/`. The CLI also bundles **ripgrep** and **difftastic**
(`src/modules/ripgrep`, `src/modules/difftastic`, `scripts/download-tools.sh`)
so the phone can grep and diff the repo, with `pathSecurity.ts` guarding
traversal, plus `src/utils/tmux.ts` and `caffeinate` to keep the Mac awake.

### 1.7 Notifications

**Expo Push** [V `src/api/pushNotifications.ts`]: `expo-server-sdk ^3.15.0`,
`Expo.isExpoPushToken()` filter → `chunkPushNotifications()` →
`sendPushNotificationsAsync()`, exponential backoff `min(1000*2**attempt, 30000)`
retried for five minutes. No direct APNs/FCM/VAPID.

### 1.8 Other agents

Extensive [V]: `src/codex/` (`runCodex.ts`, `codexMcpClient.ts`,
`happyMcpStdioBridge.ts`, `utils/permissionHandler.ts`, `diffProcessor.ts`,
`reasoningProcessor.ts`), `src/gemini/` (`runGemini.ts` + config/history/
permission utils), and a generic **ACP backend**
[V `src/agent/acp/AcpBackend.ts`, `@agentclientprotocol/sdk ^0.14.1`] with
`AgentRegistry`/`AgentBackend`/`MessageAdapter` abstractions. Shared base
classes `BasePermissionHandler.ts`, `BaseReasoningProcessor.ts`. `codium`
bundles `@anthropic-ai/claude-code 2.1.143` and `@openai/codex 0.130.0`.

**happy's multi-agent architecture is essentially omt's agent-layer design,
already built.**

### 1.9 Reception and business model

Free, no account required [D happy.engineering/docs/faq], MIT, self-hostable
relay. But `@revenuecat/purchases-js` and `react-native-purchases` are in
`happy-app` deps [V] alongside the paid-voice docs, and an undocumented iOS IAP
"Plus Plus Monthly $19.99" was reported [C https://news.ycombinator.com/item?id=44904039].
PostHog (`posthog-react-native`) is bundled [V], which conflicts with community
claims of no telemetry [C https://news.ycombinator.com/item?id=46991591].

happy has never had a big HN moment: Show HN 2025-08-14, **30 points, 8
comments** (id=44904039); a 2026-02-12 resubmit got 2 points (id=46994716). It
grows by word of mouth inside other threads.

Praise [C]: *"I'm at the point I'm running happy.engineering on my phone and
don't even need to sit in front of the computer anymore"* (id=46771564); *"very
easy to set up"* (id=46517458); *"decent mobile clients and currently
MIT-licensed"* (id=46991591); a blog post about deploying Jellyfin to k8s from a
phone (https://blog.denv.it/posts/im-happy-engineer-now/).

Complaints [C] cluster on reliability and perceived staleness: *"rarely works
reliably"*, *"can't deal with claude's new question format"*, *"appears
abandoned"* (https://news.ycombinator.com/item?id=46742298); *"when it works oh
boy"* (id=45787595); issue #149 "Allow use of Claude Code v2.0.0" (23 comments),
#276 "Failed to connect terminal both on Mobile App and Web Browser", #358
"codex hangs"; QR unscannable in Zed's terminal; the Android app telling
Bluetooth it is "on a call".

**The source read contradicts the abandonment charge** — the monorepo shipped
today, AskUserQuestion is implemented, and the ACP/Codex/Gemini backends are
new. But the *perception* of flakiness is real and widespread, and that is the
opening. happy's weakness is not architecture; it is polish, reliability, and
the absence of a terminal.

---

## 2. Omnara — the PTY-scraping counterexample

### 2.1 What it is

`omnara-ai/omnara` — **2,658★, 199 forks, Apache-2.0**, created 2025-07-09,
**last push 2026-01-19** [V]. YC S25, ~4 people, founders Ishaan Sehgal and
Kartik Sarangmath. The repo description is now "The API for production-grade
agents" and the README **explicitly deprecates the Claude-Code-wrapper version**
in favour of a new closed platform built on the Claude Agent SDK [D]. Layout:
`apps/{web,mobile,packages/shared}` (new TS front-ends) over `src/{omnara,
servers,integrations,relay_server,shared,backend,mcp-installer}` (legacy Python).

### 2.2 Mechanism — three paths, and the legacy one is instructive

**(a) Legacy interactive wrapper — real PTY plus screen scraping**
[V `src/integrations/cli_wrappers/claude_code/claude_wrapper_v3.py`, 1633 lines]:
`import pty`; `self.child_pid, self.master_fd = pty.fork()`; the child sets
`CLAUDE_CODE_ENTRYPOINT="jsonlog-wrapper"` and `os.execvp(cmd[0], cmd)`
[V lines 899–904]. Command is `[claude_path, "--session-id",
<agent_instance_id>]` plus optional permission-mode flags [V lines 868–879].
`find_claude_cli()` probes `which claude` and six fallback paths. Window size is
set with `fcntl.ioctl(master_fd, TIOCSWINSZ, ...)` using per-OS constants
(0x5414 Linux, 0x80087467 macOS). **No MCP permission tool, no hooks, no
stream-json.** Sibling wrappers exist for Amp and Codex.

**(b) Headless / SDK path** [V `src/integrations/headless/claude_code.py`, 608
lines]: `ClaudeSDKClient(options=ClaudeCodeOptions(...,
permission_prompt_tool_name="mcp__omnara__approve"))`. **This is a genuine MCP
`--permission-prompt-tool` deployment** — Omnara's MCP server exposes an
`approve` tool [V `src/servers/mcp/{server,tools,stdio_server}.py`]. Wrapped by
a FastAPI webhook server with an optional cloudflared tunnel.

**(c) GitHub Actions**: a vendored fork of Anthropic's `claude-code-action` with
an added `src/run-omnara.ts` [V].

### 2.3 State capture and permission prompts — the fragile path, documented

Content comes from transcript tailing: `monitor_claude_jsonl()` [V line 429]
waits for `~/.claude/projects/**/<agent_instance_id>.jsonl` (it *forced* the id
via `--session-id`, so the filename is known) and follows session-file switches
via `session_reset_handler.py`.

**Idle detection is pure PTY screen-scraping**: `is_claude_idle()` means "hasn't
shown `esc to interrupt` for `idle_delay` seconds" [V lines 631–635], also
matching `ctrl+b to run in background` [V lines 1069–1071]. Default delay 3.5 s.
The composite trigger for "ask the human" is `last_was_tool_use and
is_claude_idle()` [V line 940].

**Permission prompts are parsed by regex over ANSI box-drawing** [V lines
712–860]: strip `│ ╭ ─ ╮ ╰ ╯ ❯` and ANSI codes from `self.terminal_buffer`, scan
backwards for a line containing `"Do you want"`, find the last `^1\.\s+` line,
walk consecutive `^N\.\s+` lines (max 10) into `options` and
`options_map {text: "1"}`. Plan mode is detected separately with a **hardcoded**
question string and hardcoded options 1/2/3. The fallback when parsing fails is
hardcoded `1. Yes / 2. Yes, and don't ask again this session / 3. No` [V line
976]. Transmission is a plain text message with a sentinel block:
`f"{question}\n\n[OPTIONS]\n{options_text}\n[/OPTIONS]"`, which the phone parses.

**Answer injection is literal keystrokes**: the chosen number is appended to
`self.pending_write_buffer` and written with `os.write(self.master_fd, ...)`,
with EAGAIN/EWOULDBLOCK retry loops for errno 35 (macOS) / 11 (Linux) [V lines
1180–1235]. **There is no structured `AskUserQuestion` support at all** — a new
Claude prompt format silently breaks it, which is exactly what the community
reports.

Free-text input arrives by **HTTP long-poll**:
`send_message(requires_user_input=True, poll_interval=3.0,
timeout_minutes=1440)` [V `src/omnara/sdk/client.py:179–285`].

**This is the single best empirical argument for omt's tier discipline.**
Omnara is a well-funded, YC-backed, 2.6k-star product whose entire
structured-interaction feature is a regex over box-drawing characters with a
hardcoded fallback. omt's rule — "anything rendered as a card must come from
tier 3, 4 or 5" — is a direct correction of exactly this failure mode, and the
failure is observable in the wild.

### 2.4 Transport, encryption, self-hosting

**No end-to-end encryption anywhere** [V — no nacl/sodium/crypto in the Python
tree]. Auth is API keys plus **Supabase** JWTs
[V `src/relay_server/auth.py`]; server-side data is plaintext in Postgres. This
is the top structural critique on HN [C https://news.ycombinator.com/item?id=46991591].

Two transports: (i) REST + long-poll to `https://agent.omnara.com`; (ii) a
**binary terminal relay over WebSocket** [V `src/relay_server/{websocket,
protocol,sessions}.py`, `src/omnara/session_sharing.py`] at
`wss://relay.omnara.com/agent` (env `OMNARA_RELAY_URL`), frame header
`struct("!BI")`, types `OUTPUT=0 / INPUT=1 / RESIZE=2 (struct "!HH") /
METADATA=3`, auth via WS subprotocol prefixes `omnara-key.<key>` /
`omnara-supabase.<jwt>`. The client wraps `pty` in a `_WebSocketChannel` with a
Paramiko-like API — they clearly started from SSH.

Self-hosting is **partial**: `OMNARA_API_URL`, `OMNARA_BASE_URL`,
`OMNARA_RELAY_URL`, `OMNARA_API_KEY`, `OMNARA_AGENT_INSTANCE_ID` [V
`docs/cli/environment-variables.mdx`]. The legacy Apache-2.0 stack runs; the new
managed platform does not.

### 2.5 Mobile UX, notifications, agents, model

The phone UI is **fleet-oriented rather than chat-first**
[V `apps/mobile/src/components/dashboard/*`]: `AgentFleetStatus.tsx`,
`AgentsList.tsx`, `LaunchAgentModal.tsx`, `AgentConfigModal.tsx`,
`WebhookConfigModal.tsx`; web adds `CommandCenter.tsx`, `AllInstances.tsx`,
`agents/AgentGrid.tsx`, `AgentManagementHub.tsx`. Because of the binary relay it
**does have a real streaming terminal view on mobile** — Omnara's terminal
fidelity beats happy's; happy's structured cards are far richer.

Notifications [V `src/servers/shared/notifications.py`, `twilio_service.py`]:
**Expo Push plus email plus SMS via Twilio**, gated on
`user.push_notifications_enabled`. Multi-channel is a genuine differentiator —
nobody else in this survey does SMS.

Agents: `AGENT_CHOICES = ["claude", "amp", "codex"]` [V `src/omnara/cli.py:24`],
`omnara --agent codex --name "Backend Work"` [D]. Codex has a native Rust
integration; Amp is "sophisticated terminal output parsing" (i.e. also
scraping). Beyond CLIs: an **n8n node** with a `sendAndWait` human-in-the-loop
operation [V], GitHub Actions, MCP server, Python SDK (`OmnaraClient` /
`AsyncOmnaraClient` — `send_message(requires_user_input=...)` is the entire "any
agent" story), and REST. `--continue`/`--resume` are **unsupported** and bypass
Omnara with a warning [V lines 1545–1548].

Model: free 10 agent sessions/month; **$20/mo unlimited** (README still shows the
older $9 Pro tier, so ~2× price rise at the Feb-2026 relaunch); Enterprise
custom.

Reception is much larger than happy's: Show HN 2025-08-12, **310 points, 168
comments** (id=44878650); Launch HN 2026-02-12, 147 points, 161 comments
(id=46991591, contentious). Praise [C]: *"the mobile first coding agent workflow
really feels like a fundamental shift"*; *"this is genius"*. Complaints [C]:
price (*"$20/month for something an engineer can hack in hours with tailscale"*),
moat (*"how is this a company?"*, *"what's your moat against Anthropic just
launching the same thing"*), **no E2EE** (*"The lack of E2E encryption was why I
didn't chose Omnara"*), unmet self-hosting demand, and 2025 bugs (output not
appearing, XML tags leaking, copy/paste broken).

**The moat question is the one omt must also answer.**

---

## 3. First-party benchmark: Anthropic

### 3.1 Claude Code on the web (claude.ai/code)

**What** [D https://code.claude.com/docs/en/claude-code-on-the-web]: hosted
service, research preview for Pro/Max/Team; tasks run on Anthropic-managed
infrastructure. Fresh **Ubuntu 24.04 VM per session**, ~4 vCPU / 16 GB / 30 GB,
repo cloned in [D /cloud-environments].

**Mechanism**: it runs the real Claude Code binary — docs reference "Claude Code
v2.1.205 or later in the session's environment", repo `.claude/` settings and
hooks, `.mcp.json`, skills/agents/commands, plugins, subagents, `/compact`,
`CLAUDE_CODE_REMOTE=true`, and a setup script that runs "before Claude Code
launches" [D — strong inference; never explicitly stated to be the npm binary [?]].

**No PTY is exposed**: *"You don't get a shell into the session VM. Claude runs
every command for you."* [D] **There is no terminal on web or phone.** Isolation
primitive undisclosed [?]. No worktrees in cloud — isolation is per-VM.

**Egress** [D /cloud-environments#access-levels]: None / Trusted (default) /
Full / Custom allowlist with `*.sub.example.com` wildcards; published trusted
list (npm, PyPI, crates, Maven, Docker Hub, ghcr, AWS/GCP/Azure); all traffic
through a security proxy with malicious-request filtering and a DNS-level audit
trail. Caveat: even at None, "Claude Code can still communicate with the
Anthropic API, which may allow data to exit the VM."

**Auth and data** [D]: the repo *is* cloned into Anthropic's cloud. GitHub App
or `/web-setup` syncing your local `gh` token. Documented caveat: "a cloud
session can access **any repository the connecting GitHub account can see**." A
dedicated GitHub proxy keeps real credentials outside the VM (`GH_TOKEN` reads
literally as `proxy-injected`); push is restricted to the session's branch.
**No secrets store** — env vars are plaintext to anyone using the environment.
Non-GitHub: local bundle upload <100 MB, cannot push back. Not self-hostable;
blocked for ZDR orgs.

**Mobile UX** [D https://code.claude.com/docs/en/mobile]: the Claude iOS/Android
app has a **Code** tab. Chat plus structured diff cards (`+42 -18` → diff view
with **inline line comments batched into the next message**). No raw terminal.
Cloud sessions expose only Accept-edits / Plan / Auto (no Manual, no Bypass), so
you mostly *don't* get per-tool prompts in cloud; you answer Claude's questions
as chat messages. Steering a running session is supported.

**Multi-session and teleport** [D]: `claude --cloud "task"` fires N parallel
cloud sessions; `/tasks` lists them; **`--teleport` / `/tp`** pulls a cloud
session *and its branch* into the local terminal with full history (requires a
clean tree, same repo not a fork, branch pushed, same account). Handoff is
one-way from the CLI; Desktop adds "Continue in → Claude Code on the Web" for
local→cloud. The terminal copy diverges: "new work there stays local and doesn't
appear in the cloud session."

**Notifications: a real gap.** Push is documented for Remote Control and
Dispatch, but **no doc states that cloud sessions push to the phone when they
finish** [?].

### 3.2 Claude Code Remote Control — the most relevant prior art in existence

[D https://code.claude.com/docs/en/remote-control]

`claude remote-control` / `--rc` / `/remote-control`, also in VS Code. Available
on **all plans** (Team/Enterprise off by default). The local process makes
**outbound HTTPS only and never opens inbound ports**; it registers with the
Anthropic API and polls, relayed over a streaming connection with multiple
short-lived scoped credentials. **The transcript is stored on Anthropic servers**
for cross-device sync, so ZDR orgs cannot use it.

Server mode takes `--capacity 32` concurrent sessions, and **`--spawn worktree`**
gives each on-demand session its own git worktree (press `w` to toggle).
**QR-code pairing** on spacebar. The session dies if the `claude` process dies;
network loss beyond ~10 minutes kills it. **Trusted Devices** beta (Team/Ent):
per-device WebAuthn with 18-hour freshness and Face ID / Touch ID / Windows
Hello step-up.

Push toggles [D]: "Push when Claude decides" and "Push when actions required"
(permission prompts and questions). Claude decides when to push; you can say
"notify me when the tests finish". Push is suppressed while you are typing at
the terminal, and `CLAUDE_CLIENT_PRESENCE_FILE` extends the presence heuristic.
**The phone answers real permission prompts here** (Manual / Accept edits /
Plan). Phone attachments are downloaded to your machine and passed as `@` refs.

Two UX details worth copying outright [D]: after several permission prompts the
CLI surfaces "**Approve tool calls from your phone**"; on long turns it surfaces
"**Still working — check in from your phone**."

**Assessment for omt:** Remote Control is the first-party version of omt's
remote story, it is free on every plan, it has QR pairing, worktree spawning,
presence-aware push, and hardware-attested device trust. It is a *single-agent*
feature with no multiplexer, no other CLIs, no self-hosting, and a mandatory
Anthropic relay that stores your transcript. Those four are the seams.

### 3.3 Claude Code Desktop

[D https://code.claude.com/docs/en/desktop] Electron, macOS universal / Windows
x64+ARM64 / Linux beta. Tabs: Chat, Cowork, Code. Reads the same settings files
as the CLI. Per-session **environment selector — Local / Cloud / SSH connection /
WSL distro**, so it is simultaneously a local host and a client for the cloud VMs.

Parallel sessions each get **their own git worktree** at
`<project-root>/.claude/worktrees/`, with configurable location and branch
prefix, and **`.worktreeinclude` to copy gitignored files like `.env`** — the
reference solution to the loudest worktree complaint in the category.

**Real terminal? Yes** — an integrated terminal pane (Ctrl+`) opening in the
session cwd, sharing Claude's environment, with multiple tabs. **Local sessions
only** — not cloud, not phone. Plus an in-app file editor (local and SSH), diff
viewer, **browser preview pane** (dev server, DOM inspection, self-verification),
**iOS Simulator pane** (macOS), task/subagent panes, and drag-and-drop pane
layout.

Multi-session: sidebar with status/project/environment filters, Cmd-click split
view, `/btw` side chat (Cmd+;) reading main context without polluting it,
**cross-session messaging** (Claude can list/read/message/rename/archive its
other Desktop sessions, 20 most recent), auto-archive after PR merge. Explicit
limitation [D]: "Claude doesn't see cloud sessions, and doesn't see sessions you
started from the terminal CLI or the VS Code extension, even in worktrees of the
same project."

**That limitation is an omt-shaped hole in Anthropic's own product.**

Phone bridge: **Dispatch** (Cowork tab, Pro/Max only) — message a task from the
phone, Dispatch may spawn a Desktop Code session on your machine badged
"Dispatch", and you get a push when it finishes or needs approval [D].

---

## 4. First-party benchmark: OpenAI Codex

**Surfaces** [D]: Codex CLI (**open source, Rust, Apache-2.0**, `openai/codex`,
~99.6K stars [C]), Codex cloud (chatgpt.com/codex), and Codex mode inside the
ChatGPT desktop app (the standalone Codex app was merged into ChatGPT desktop
~2026-07-09 [C]).

**Cloud** [D https://learn.chatgpt.com/docs/cloud]: repo cloned into an isolated
container/microVM; **each task gets its own git worktree** so parallel agents do
not collide. Two-phase runtime — a setup phase with network on, then **the agent
phase runs offline by default** unless internet is enabled; full internet or a
domain allow-list.

**Local sandbox — materially more transparent than Anthropic's** [D]:
macOS = Seatbelt via `sandbox-exec`; Linux = `bwrap` + `seccomp`; Windows =
native sandbox or WSL2. Approval policies `read-only` / `workspace-write`
(default) / `on-request` / `never`. Network off by default locally; allowlist
supports `*.example.com` / `**.example.com`; `.git`, `.agents`, `.codex` are
read-only; DNS-rebinding and local-binding protections.

**Bidirectional stateful handoff** [D-ish + C]: "pair with Codex locally,
delegate tasks to the cloud to execute asynchronously without losing state, then
continue tasks from Codex web in your IDE"; you can preview the cloud diff in
the IDE, ask follow-ups, then apply locally. Three execution shapes: direct
local checkout, local worktrees created by the desktop app, cloud tasks at a
chosen branch/commit. CLI v0.138 added `/app` desktop handoff.

**Codex Remote — the mirror image of Anthropic's** [C, press-sourced; the
first-party page 403s]: Codex in ChatGPT mobile shipped 2026-05-14 (iOS +
Android, **all plans including Free**); Codex Remote GA 2026-06-25 for all paid
tiers. It is **primarily a control surface for a session running on your own
Mac/Windows host**, not a cloud console — the phone is a steering wheel for a
Codex session on your laptop. **QR pairing**: Codex for Mac generates a QR you
scan; GA hardened it to **authenticated one-to-one QR pairing per device per
host**, auditable. From the phone: start threads in existing host projects, send
follow-ups, **answer agent questions, approve/deny commands**, review diffs,
test results, terminal output, and screenshots, switch models. *"Nothing from
the codebase or its credentials is stored on or transmitted to the phone."*
2026-07-06 added branch/change filters, transcript-to-composer selection,
attachment previews, and **SSH host support**.

**No raw terminal on the phone, but it renders terminal output and screenshots** —
the richest read-only fidelity of any first-party mobile client.

**Community** [C https://news.ycombinator.com/item?id=48140529, 439 pts]:
`blevinstein` — the mobile version "doesn't actually offer cloud coding agents",
so no truly independent work when the laptop is away (**the sharpest criticism of
the local-host-tether model, and it applies to omt verbatim**). `nextaccountic` —
impractical for big Rust projects: 10 GB `target/`, 10-minute compiles.
`brtkwr` — connection failures, "repo selection is disabled" on mobile though
"fine on laptop". `jumploops` — enthusiasm faded; short mobile prompts create
ambiguity and *more* babysitting; saves big tasks for "keyboard time".
`miohtama` — uses third-party **Omnara** for remote Claude/Codex, direct evidence
of unmet need. Praise: `osullip` ships whole apps from the phone via GitHub +
Vercel "on the way back from a meeting"; `weird-eye-issue` works features "an
hour or two per day" on mobile for "a really good first draft".

---

## 5. The hosted-platform tier (Cursor, Devin, Conductor, Terragon, Sculptor, Factory)

Condensed; these are less directly comparable to omt but set user expectations.

**Cursor cloud agents + iOS** [D https://cursor.com/docs/cloud-agent]. Not a CLI
in a PTY — Cursor's own agent loop emitting structured tool-call events. The
**agent loop always runs in Cursor's cloud**; only tool execution location varies
across three runtimes: Cursor-managed VMs (default), **"My Machines"** (tool
calls on your laptop, must stay online), and **Self-Hosted Pool** (Enterprise;
k8s / Cloud Run workers running an `agent` CLI, **outbound-only HTTPS to
api.cursor.com**, no inbound ports) [V https://cursor.com/docs/cloud-agent/choose-runtime].
So execution is self-hostable; the agent loop is not. Env setup via agent-led
interactive setup → **saved VM snapshot**, or `.cursor/Dockerfile` +
`.cursor/environment.json`, with build secrets, layer caching, env version
history/rollback, and egress restrictions.

Native **iOS** app since 2026-06-29 (iOS 26+), public beta on all paid plans; no
native Android. Phone shows chat + diffs + logs + screenshots + computer-use
videos, voice input, slash commands, model picker. **No interactive terminal on
mobile** [?, inferred from omission]. **No per-command approve/deny — cloud
agents auto-run every terminal command** [C]. **Cursor is the only product in
this survey with true native push plus iOS Live Activities for agent status.**
Separately, Remote Control lets the phone steer agents on your desktop.

Privacy wedge [V, Cursor employee `leerob`, https://news.ycombinator.com/item?id=48737226]:
*"The new privacy mode is needed because we have to store some state to enable
running agents in the cloud… Currently the mobile app requires this new privacy
mode and won't work without it."* Community response: *"my account was changed to
the softer Privacy Mode and the previous setting… disappeared from all menus"*;
*"Happened to me too, incredibly dark pattern"*.

The #1 functional complaint is **latency**: forum.cursor.com/t/cloud-agents-too-slow-to-be-usable/157505
(10+ minutes just to start), /t/background-agent-is-painfully-slow-and-limited/112298
("hiding a menu nav link takes 10+ mins in cloud vs seconds locally"),
/t/cursor-web-cloud-is-horribly-slow/155735. Reliability: stuck on "Running
update script", broken stop button, unresponsive cloud terminals
(/t/cloud-agents-broken-ii/161036, /t/the-cloud-agent-is-stuck/160974) — **with
users billed while idle**, since cloud agents bill at model API pricing with no
separate VM line item.

**Devin** [D https://docs.devin.ai]. Hosted cloud, per-session VM with Shell +
embedded IDE + Browser, snapshots core ("workspace resets to a saved machine
state at the start of every session", `--snapshot-id` first-class), VNC viewer,
Windows machine sessions. A real local **Rust** CLI exists that hands off to
cloud Devin. **Self-hosting is dead** [V https://devin.ai/blog/self-hosted-deployment-maintenance-mode] —
maintenance mode 2025-05-12, stated reason: frontier models need "a full rack of
frontier GPUs". **No native mobile app**; the mobile story is a PWA (2026-03-07),
and **Slack is the de facto mobile surface** — `@Devin` in-thread, config
suggestions rendering with an "Apply button so you can accept it without leaving
Slack", thread controls, bang modifiers. Cognition acquired **Poke** (the
Apple-Messages-native agent) on 2026-07-23 [C], a clear signal they know mobile
is a gap. Pricing: Free / Pro $20 / Max $200 / Teams $80 + $40/seat / Enterprise;
ACU = Agent Compute Unit, ~15 min active work, and users find them opaque
(*"entirely too opaque/confusing/complicated… shrouded in mystery"*
[C https://news.ycombinator.com/item?id=46711589]). The canonical takedown remains
Answer.AI's 20-task trial: **3 success / 14 fail / 3 inconclusive**
(https://www.answer.ai/posts/2025-01-08-devin.html). But the same 2026 HN thread
contains a directly on-thesis datapoint: *"I managed to review a PR on an
airplane (without starlink) with it earlier this week."*

**Conductor** (conductor.build) [D]. Closed-source **macOS-only** desktop app,
YC, ~$22M Series A [C]. Orchestrates third-party CLIs (Claude Code, Codex,
Cursor, OpenCode) with a **git worktree per task**. A real terminal exists behind
Settings → Experimental → **Big Terminal Mode** (⌘⇧T), which the founder notes
"you don't get notifications etc (yet)" [C https://news.ycombinator.com/item?id=47871018].
PTY vs SDK invocation is undocumented [?]. Cloud shipped 2026-07-30: each agent
in an isolated cloud sandbox plus a shared 8 vCPU / 16 GB org "Cloud Computer".
**Mobile does not exist yet** — the pricing page lists "mobile app (coming
soon)". The **GitHub scope scandal** is the loudest reaction in the whole survey:
the app demanded full read-write on the entire account including org settings,
webhooks, and deploy keys — *"INSANE to authorize this app on anything other than
throwaway code"* [C https://biggo.com/news/202507210115_Conductor_App_GitHub_Permissions_Controversy].
Second-loudest: worktree friction — *"clones the repo from Github. This is bad,
because you need to run all the dependency installs, etc. for every workspace"*,
with untracked `.env` not carried over. Free local / Pro $50 / Teams $60 per user.

**Terragon / Terry** — **dead** (shut down ~2026-02-09), repo Apache-2.0, last
push 2026-02-10, docs domain has an expired TLS cert [V]. **It is the most
instructive corpse in the category.** `packages/daemon/src/claude.ts` builds a
literal shell string: `cat <tmpfile> | claude -p --model <m> --resume <sid>
--verbose --output-format stream-json [--mcp-config …]
[--permission-prompt-tool mcp__terry__PermissionPrompt] --append-system-prompt
"…"` [V]. So: headless print mode, stream-json, no PTY, no reimplemented loop,
continuity via `--resume`. **The `--permission-prompt-tool` routing approvals
through Terry's own MCP server so the phone renders an approve/deny card is the
proven non-terminal permission solution in the wild** [V]. Execution in **E2B**
sandboxes; transport a dedicated WebSocket broadcast service on **PartyKit**;
mobile a **PWA** with no terminal. Its README claims "browser notifications" but
**no VAPID/web-push/APNs keys exist anywhere in the repo** [V] — likely in-page
Notification API only, i.e. not real background push. Sharpest complaint:
*"Letting Sonnet do tasks on Terry, unsupervised is kinda useless as the fixes I
have to do afterwards eat the time I saved"*
[C https://news.ycombinator.com/item?id=45604301].

**Sculptor** (Imbue) — **MIT**, `imbue-ai/sculptor`, local desktop app for macOS
Apple Silicon **and Linux x64/ARM64** (the only Linux-shipping product here),
pushed 2026-08-02 [V]. Correction to the received narrative: the current README
describes workspaces as **git worktrees**, with **Docker demoted to an
experimental "Container Backend"** [V `docs/help/experimental/container_backend.md`] —
contradicting the widely cited 2025 "every agent in its own container" pitch.
Wrapper, not fork; integrates Claude Code plus Imbue's own Pi harness and "any
terminal-based agents", with a first-class built-in terminal scoped to the
workspace [V `docs/help/terminal.md`]. Its one remoting seam is notable [V]: the
app "can spawn a user-provided shell command instead of using its built-in
backend. The command prints a URL to stdout, and the app connects to it" —
usable in Docker, over SSH to a remote server, or a VM. **No mobile at all.** No
notifications found [?]. Worth stealing: bundled workflow skills and an MCP that
surfaces suggestions to human and agent alike, e.g. "the agent didn't actually
run the tests after writing them" [V; C].

**Factory Droid** (factory.ai) [D]. Multi-surface: Factory App, Droid CLI, web +
mobile at app.factory.ai, and Droid Computers. The Droid CLI is a **real local
closed-source CLI**, Bun-built, not a Claude Code wrapper [V
https://agent-safehouse.dev/docs/agent-investigations/droid], with a TUI REPL
including **bash passthrough** (`!` toggles raw bash) and `droid exec` for
headless with `text|json|stream-json|stream-jsonrpc`. Best "your code stays
local" statement of the survey: *"Droid reads and edits files on the machine
where it runs. It does not require uploading or indexing a static copy of your
repository."* [D /cli/account/security]. **Only product with worktrees as a
documented CLI flag**: `-w/--worktree [name]` on both `droid` and `droid exec`,
created at `../<repo>-wt-<branch>/`, auto-removed if clean. **Droid Computers**
(2026-04-22): persistent remote machines, Factory-managed *or* **BYOM** (your
VPS/workstation/on-prem), over **secure WebSocket tunnels rather than exposed
SSH**, with Ed25519 keys in `~/.factory/.ssh/`, `droid computer ssh|port-forward`,
and BYOM connecting **outbound through a Factory relay with no public ports** [D].
Cross-surface resumability is the explicit pitch: "start a task on one surface
and finish it on another". Mobile is **mobile web, not native**; the phone shows
chat + diff viewer + terminal *output* "tuned for small screens", and approvals
are answerable — but there is **no evidence of a real interactive PTY on mobile**
[?]. A **TypeScript SDK** wraps Droid as a subprocess *plus* a long-running droid
daemon controllable over WebSocket for concurrent sessions across remote machines
(github.com/Factory-AI/droid-sdk-typescript) — **the closest thing anyone has to
an open remote-control substrate.** No true mobile push [?]; Slack/Linear/email/
audio only. Pro $20 / Plus $100 / Max $200 / Enterprise; BYOM computers free on
all plans.

---

## 6. The orchestrator tier (crystal, claude-squad, uzi, mux, container-use)

**crystal** (`stravu/crystal`) — Electron multi-session GUI, MIT, ~3.1k★,
**deprecated Feb 2026** in favour of Nimbalyst. Its service layout [V
`main/src/services/`] is the most architecturally interesting in this cluster:
`worktreeManager.ts`, `terminalSessionManager.ts`, `terminalPanelManager.ts`,
`sessionManager.ts`, `gitDiffManager.ts`, `gitStatusManager.ts`,
`gitFileWatcher.ts`, `panelManager.ts`/`panelEventBus.ts`,
`cliManagerFactory.ts`/`cliToolRegistry.ts` (pluggable Claude Code + Codex
backends) — and, critically, `permissionManager.ts`, `permissionIpcServer.ts`,
`mcpPermissionBridge.ts`, `mcpPermissionServer.ts`. **Crystal stood up an MCP
permission server so approve/deny is a real UI event rather than an injected
keystroke.** No tmux, local Electron IPC only, no remote, no mobile, no
notifications. **The one project that solved permissions properly in this tier is
abandoned; that capability is currently unowned.**

**claude-squad** (`smtg-ai/claude-squad`) — Go TUI, single binary `cs`,
**AGPL-3.0**, ~8.2k★, the highest-adoption tool here. **tmux is the real
multiplexer** [V packages `session/tmux`, `session/git`]: one tmux session per
agent instance bound to a git worktree, output read via `tmux capture-pane`
[C issue #189]. State detection is pane-scraping. Permission prompts get no
rendering at all — `--autoyes`/`-y` means "all instances will automatically
accept prompts" [D]. Requires tmux + `gh`. AGPL is a real embedding constraint.

**uzi** (`devflowinc/uzi`) — Go CLI, MIT, ~581★. Worktrees + tmux per agent,
plus **automatic dev-server port allocation** per agent via `uzi.yaml`
(`devCommand`, `portRange`) — a genuinely good idea nobody else has. `uzi ls -w`
shows `AGENT MODEL STATUS DIFF ADDR` from tmux inspection. `uzi auto`
"automatically presses Enter to confirm all tool calls" — keystroke injection.
`uzi broadcast` sends one instruction to every agent.

**mux** = `coder/mux` (Coder Technologies), **AGPL-3.0**, ~2k★. Desktop app plus
a **browser web-app server mode**. Has its own agent loop but deliberately clones
Claude Code UX (Plan/Exec modes, vim input, `/compact`). Three selectable
runtimes per task — **Local / Worktree / SSH (remote compute)** [D] — the only
real multi-machine story in this cluster besides VibeTunnel. Claims a
**"responsive mobile-compatible server mode"** [D], the closest competitor here
to a browser-first phone story, though with no detail on answering permission
prompts from a phone. Permission mechanism unverified [?].

**container-use** (`dagger/container-use`, `cu`) — Apache-2.0, ~3.9k★, an MCP
server + CLI. Per-agent isolation is a **dedicated git branch plus a Dagger
container**, worktree at `~/.config/container-use/worktrees/<project>/` mounted
at `/workdir`, a dedicated git remote at `~/.config/container-use/repos/<project>/`,
tracking refs `cu-<env-id>`, and container state snapshotted via **git notes** on
refs `container-use-state`/`container-use` [D]. Needs Docker/Podman/Colima. No
agent-state detection and no permission UI — it is a tool surface, so approvals
stay in whatever host client you use.

---

## 7. The terminal-in-a-browser lineage

**ttyd** (C, libwebsockets) — forks a PTY, frames raw terminal I/O as **binary
WebSocket** messages, client is **xterm.js**, resize via fit-addon control
message → `ioctl(TIOCSWINSZ)`. No session persistence (bring your own tmux).
Auth = **HTTP Basic** (`-c user:pass`). Fully self-hosted. One process, one
session. No notifications. Mobile: **no key bar at all**; Ctrl/Esc/arrows depend
entirely on the soft keyboard [C]. MIT, active. HN praise is consistently "does
one thing well", with recurring asks for auth beyond Basic.

**gotty** — yudai/gotty is dormant; **sorenisanerd/gotty is the live fork**. Go +
`creack/pty` + xterm.js over WebSocket. Auth `--credential user:pass`, TLS flags,
`--random-url` (security by obscurity). Maintainers themselves list "rework of
the mobile GUI" as a known TODO [C]. MIT.

**wetty** — Node/Express + xterm.js + ssh2. Non-root spawns `ssh` to localhost;
root spawns `/bin/login` — so **auth is delegated to SSH**, cleaner than Basic
Auth. No mobile key bar. MIT.

**sshx** (`ekzhang/sshx`) — Rust workspace: client, relay server, protobuf/tonic
core. Crypto [V `crates/sshx/src/encrypt.rs`]: **Argon2id** (v0x13, 19,456 KiB,
2 iterations, 1 thread, 16-byte output) over a **fixed public salt** (source
comment: "non-random salt for sshx.io, since we want to stretch the security of
83-bit keys!") → AES-128 key; cipher is **AES-128-CTR (Ctr64BE)** with nonce =
8-byte BE stream number + 8 zero bytes, using `seek()` for random-access
encryption at arbitrary offsets. The key lives only in the **URL fragment**, so
the relay sees ciphertext only. **Weakness** [C https://news.ycombinator.com/item?id=38152109]:
CTR with **no AEAD/integrity** — a malicious relay can bit-flip terminal output
undetected. Data flow: default path **relays through hosted sshx.io** (Fly.io +
Redis Cloud); `sshx-server` is buildable but maintainer docs state
**"self-hosted deployments are not supported at the moment."** Genuine
multiplayer: infinite canvas, live per-viewer cursors, independently movable and
zoomable panes — Figma-for-terminals. Mobile is cursor-centric and awkward on
touch, with no key bar. MIT, ~7.6k★.

**VibeTunnel** (`amantus-ai/vibetunnel`) — MIT, 4,622★, actively maintained
(v1.0.0-beta.18, 2026-07-11 — still beta after a year). `web/` (Bun/TS server +
frontend), `mac/` (Swift menu-bar app), `ios/` (SwiftUI). PTY via **vendored
node-pty** driven by `web/src/server/pty/pty-manager.ts` [V], plus integrations
for existing multiplexers (`tmux-manager.ts`, `zellij-manager.ts`,
`multiplexer-manager.ts`). The `vt` CLI talks to the server over **Unix domain
sockets with a custom binary protocol** [V `services/socket-protocol.ts`].

**Agent-state detection is a three-layer heuristic** [V]: (a) an idle timer
`activity-status.ts` (5s `DEFAULT_ACTIVITY_IDLE_TIMEOUT_MS` over last-output /
input / modified timestamps); (b) shell-prompt regex `prompt-patterns.ts`
(`UNIFIED_PROMPT_END_REGEX` matching `$ > # % ❯ ➜`); and (c) —
**`claude-patcher.ts`, which patches/wraps the Claude Code binary itself**, with
backup and restore on exit/SIGINT/SIGTERM. **That binary-patching is the most
aggressive state-detection approach found anywhere in this survey** and a
serious fragility and trust liability. It is precisely the class of thing omt's
tier model exists to forbid.

Transport [D]: **fully self-hosted, no relay** — vibetunnel.sh is a download page
(pay-what-you-want via Polar). Remote access is BYO tunnel: Tailscale Serve
(recommended; binds 127.0.0.1 with identity headers), ngrok, Cloudflare Quick
Tunnel, Pinggy, Pangolin. Auth: password+JWT (default), `--enable-ssh-keys`
(Ed25519 in localStorage), `--disallow-user-password`, `--no-auth`,
`--enable-tailscale-serve`.

Mobile: responsive web UI with a terminal renderer; the native iOS app is
self-described "work in progress, not recommended for production". No confirmed
custom arrow/esc key bar. So answering a y/n prompt from a phone means tapping
into a raw terminal.

**Multi-machine: yes — "HQ" mode** (`docs/hq.md`, `hq-client.ts`,
`remote-registry.ts`) aggregates multiple remote VibeTunnel servers into one
pane, with `mdns-service.ts` for LAN discovery. **This is the best multi-machine
story in the entire survey**, and it is the direct precedent for omt's
federation plan.

**Notifications: real and verified** [V] — VAPID **web push** (`vapid-manager.ts`,
keys at `~/.vibetunnel/vapid/keys.json`, mode 0600) plus native macOS
notifications over WebSocket `/ws`. Events: `sessionStart`, `sessionExit`,
`commandFinished`, `commandError`, `bell`.

**opencode** (`sst/opencode`) — `packages/opencode` (server/core, HTTP API +
SSE), `packages/tui` (a **separate Go client process**), `packages/web`
(SolidJS), `packages/opencode-ui` (shared Solid components). **`opencode serve`
is a headless backend with an HTTP API, and the TUI is a thin client that can
`opencode attach`** — TUI, web, and mobile browser all attach to the same
session. Share links use a short public URL backed by a **Cloudflare Durable
Object**. Critically, this is a **structured agent-session viewer (messages, tool
calls, thinking steps), not a byte-for-byte tty stream** — which is why its
mobile story sidesteps the soft-keyboard problem entirely.

**opencode is the strongest architectural precedent for omt's "TUI is just
another client" invariant, and it is already shipped.**

**another terminal** — closed-source Rust GPU terminal positioned as an "agentic dev
environment". The **`/remote-control` chip** publishes a live session link
[D docs.another terminal.dev/agent-platform/cli-agents/remote-control/]; viewers get
view-only **or edit access — they can steer and approve commands** from a web
browser, the desktop app, or a **mobile/tablet browser with no install**. another terminal
orchestrates multiple third-party agents (Claude Code, Codex, OpenCode) in
side-by-side agent panels. **Cloud-only, no self-host** — sessions upload to
another terminal's service. Like opencode, it renders structured agent activity rather than a
raw PTY.

---

## 8. The status quo baseline: tmux + a phone

This is what omt actually has to beat for most users, because it is free and
already installed.

**Canonical stack** [C]: always-on dev box → tmux → Tailscale (WireGuard mesh,
defeats NAT and dynamic IP) → sshd or Tailscale SSH → phone client → **Mosh**
for roaming. Mosh owns transport resilience; tmux owns persistence and scrollback
(Mosh has no scrollback, which is exactly why tmux is layered inside it).

Primary sources [C/D]:
- Harper Reed, "Claude Code is better on your phone" — https://harper.blog/2026/01/05/claude-code-is-better-on-your-phone/ — without Mosh you must "redo the connection, run Claude –continue, and live this life of lots of typing."
- rogs.me, "Claude Code from the beach" — https://rogs.me/2026/02/claude-code-from-the-beach-my-remote-coding-setup-with-mosh-tmux-and-ntfy/ — "mosh handles the flaky mobile connection (WiFi to cellular transitions, dead zones, phone sleeping), while tmux handles session persistence." And: **"Mouse support is essential when you're using your phone."**
- Elliot Bonneville — https://elliotbonneville.com/phone-to-mac-persistent-terminal/ — "Your Mac is behind a router, probably on a dynamic IP, definitely behind NAT." / "Phone SSH is flaky… apps get backgrounded." / "Tiny keyboard, no modifier keys, scrolling with your thumb."
- petesena, Tailscale + Termius + tmux — https://petesena.medium.com/how-to-run-claude-code-from-your-iphone-using-tailscale-termius-and-tmux-2e16d0e5f68b
- skeptrune, Android/Termux variant — https://skeptrune.substack.com/p/claude-code-on-mobile-termux-tailscale
- felipeelias, Tailscale + ntfy — https://felipeelias.github.io/2026/02/25/claude-code-notifications.html — **"easy to lose track of what is going on across all those sessions."**

**Clients.** Blink Shell $19.99/yr, native Mosh, open source, Vim modes, jump
hosts — subscription-only is the standing complaint. Termius: free Starter
(single device, no sync) / Pro $10/mo annual / Team $20 / Business $30. iSH:
emulated 32-bit x86 Alpine, slow typing, battery drain, Go binaries crash — used
only to SSH *out*, never to run the agent. **Secure ShellFish** is the standout:
documented shell-integration `notify` and `widget` commands let any script push
encrypted notifications and update Home/Lock Screen widgets from a backgrounded
state. Tailscale SSH removes keypair management via node identity + ACLs.

**Pain points** [C, with URLs]:
- iOS backgrounding kills bare SSH — the entire reason Mosh+tmux is universal.
- **Modifier keys**: mtmux.com/blog/tmux-from-phone — "A soft keyboard has no key you can hold down while tapping another. Ctrl-b then x… assumes exactly that." The universal workaround is binding PageUp/F1 straight to copy-mode, bypassing the prefix.
- **Narrow-width TUI rendering**: anthropics/claude-code#27291 (TUI renders at 1-char column width on startup); #52731 (default renderer leaves ~80% of the viewport blank, fixed by a `/tui` no-op redraw; root cause is the viewport over-reserving lines for worst-case markdown-table wrapping). Phone widths amplify all of it. [C https://news.ycombinator.com/item?id=47011533] "I love Claude Code, but how they made the TUI is just plain stupid."
- **Permission prompts need precise arrow/number selection** — this is the stated motivation behind the entire ntfy/Telegram Allow-Deny-button ecosystem.
- **No built-in notification when the agent finishes or asks** → people poll manually. anthropics/claude-plugins-official#798 is literally "notify user when Claude finishes working — stop babysitting your terminal", still open.

**DIY notification bridges** (all real repos): claude-push (ntfy + inline
Allow/Deny, github.com/coa00/claude-push); claude-ntfy-hook
(github.com/nickknissen/claude-ntfy-hook); claudecode-pushover-integration
(rate-limited 1/30s); **ccgram** — self-hosted Telegram bridge with inline
approval buttons **and keystroke injection back into the session**
(github.com/jsayubi/ccgram).

**Unmet wants** stated repeatedly [C]: a one-time-purchase terminal client;
built-in "text me when you need me" without hand-wiring hooks + ntfy + curl; a
**prefix-free, touch-designed control scheme** (mtmux's entire thesis is that
this does not exist); reliable narrow-width TUI rendering; and a unified mobile
view across simultaneous repos and agents with real notification triage.

---

## 9. What omt must match

Table stakes. If omt lacks any of these it will feel worse than free
alternatives, several of which are MIT-licensed and shipping today.

| # | Capability | Who established it | Bar to clear |
|---|---|---|---|
| 1 | **QR-code pairing, outbound-only, no inbound ports** | Claude Remote Control, Codex Remote, happy (`handy://`) | Spacebar-to-QR. No port forwarding, no manual token paste as the primary path. omt's signed invite link is fine but the QR must be the default. |
| 2 | **Real push notification when the agent needs attention** | Cursor (native push + iOS Live Activities), VibeTunnel (VAPID), happy/Omnara (Expo push), Claude (two granular toggles) | Web push (VAPID) at minimum, since omt has no app store presence. Terragon shipped "browser notifications" with **no push keys in the repo** and it showed. |
| 3 | **Presence-aware notification suppression** | Claude Remote Control (`CLAUDE_CLIENT_PRESENCE_FILE`; push suppressed while typing at the terminal) | If omt pushes to the phone while the user is sitting at the TUI, it will be uninstalled in a week. |
| 4 | **Structured permission approve/deny from the phone, not keystrokes** | Terragon (`--permission-prompt-tool` → MCP), Crystal (MCP permission server), happy (SDK `canCallTool` + typed RPC) | Typed RPC resolving a held request. Injecting `"2\n"` into a PTY (Omnara, uzi, ccgram) is the failure mode to avoid. |
| 5 | **Editing tool arguments before approving** | happy (`{...originalInput, ...response.updatedInput}`) | Not just allow/deny — "allow, but change the command". |
| 6 | **Session-scoped "don't ask again" with Bash prefix patterns** | happy (`parseBashPermission`) | Coarse allow/deny per invocation is too noisy for phone use. |
| 7 | **Git worktree per parallel session, with untracked files carried over** | Claude Desktop (`.worktreeinclude`), Factory (`-w/--worktree`), Conductor, Codex cloud | The single loudest orchestrator complaint is reinstalling dependencies and losing `.env` per worktree. `.worktreeinclude` is the reference solution — copy it. |
| 8 | **Multi-session fleet view with per-session status** | claude-squad, uzi (`ls -w`), Conductor, Omnara (`AgentFleetStatus`), Claude Desktop sidebar | Status, project, and "needs attention" filters. Anthropic's own Desktop **cannot see CLI or cloud sessions** — that gap is omt's opening, not an excuse. |
| 9 | **Structured diff rendering with inline comments** | Claude mobile (`+42 -18` cards, comments batched into the next message), happy (`@pierre/diffs`), Factory | Reviewing a diff on a phone is the second most common remote action after answering a question. |
| 10 | **Multi-agent support beyond Claude Code** | happy (Claude + Codex + Gemini + generic ACP backend), Omnara (claude/amp/codex), Conductor, another terminal, claude-squad | omt's six-tier model is stronger, but the *coverage* bar is already set by an MIT project. |
| 11 | **Session survives detach, reattach, daemon restart, network loss** | tmux (the baseline everyone already has), Mosh | Claude Remote Control dies on >10 min network loss — beat that, don't match it. |
| 12 | **Self-hostable with no mandatory relay** | VibeTunnel (BYO tunnel, no relay), happy (`HAPPY_SERVER_URL`, `happy-server-self-host` with PGlite) | happy already ships a single-binary self-host path. omt's "no omt cloud" stance only differentiates against the hosted tier, not against happy or VibeTunnel. |
| 13 | **End-to-end encryption where a relay exists** | happy (AES-256-GCM + BIP32-ish key tree + libsodium) | *"The lack of E2E encryption was why I didn't chose Omnara"* [C]. If omt ever adds a relay, E2EE is required on day one. |
| 14 | **Voice input on mobile** | happy (ElevenLabs + LiveKit, with `processPermissionRequest` as a voice tool) | Claude Code's own `/voice` is explicitly local-mic-only and does not work over SSH, so BYOK STT is mandatory for remote. happy has already shipped **approving a permission prompt by voice**. |
| 15 | **A touch-designed control scheme, not a soft keyboard** | Nobody — this is a documented, universal, unsolved complaint | If omt ships a raw xterm.js with no key bar it inherits every ttyd/gotty/sshx/VibeTunnel complaint verbatim. |
| 16 | **Correct narrow-width rendering** | Nobody (see claude-code#27291, #52731) | omt owns the VT parser and the grid; it can fix reflow at narrow widths where the agent TUIs cannot. |

---

## 10. Where the real gap is

Things nobody does well. This is where omt should spend its effort.

**10.1 The hybrid — structured cards *and* a real terminal on the same session.**
This is the clearest open seam in the entire category. The split is total: the
structured camp (happy, another terminal, opencode share, Claude mobile, Codex mobile,
Cursor, Devin, Factory) has no terminal on the phone; the raw-PTY camp (ttyd,
gotty, wetty, sshx, VibeTunnel, tmux+Blink) has *only* a terminal and therefore
inherits the soft-keyboard problem. happy is the sharpest illustration: it has a
`node-pty` terminal, but **only in its Electron desktop app**, never on the
phone. omt's block model — a scrollable list of collapsible OSC 133 command
blocks with a full terminal one tap away — is a genuinely novel answer, and no
one has shipped it.

**10.2 Touch-native terminal control.** mtmux's thesis is correct and unrefuted:
every phone terminal today assumes a modifier key that does not exist. The
universal workaround is hand-rolled PageUp/F1 rebinds. Nobody ships a designed
answer. This is small, unglamorous, and would be immediately felt.

**10.3 Notification triage across many sessions.** Everyone can notify. Nobody
prioritizes. *"easy to lose track of what is going on across all those
sessions"* [C felipeelias] is the recurring complaint, and the fleet views that
exist (Omnara, claude-squad) are flat lists. The interesting primitive already
exists and is unused: Claude Code writes an `away_summary` system line — a
natural-language "what is the state of this session" blurb — straight into the
transcript. Nobody in this survey surfaces it.

**10.4 Trustworthy state detection.** The field's methods are: `tmux
capture-pane` scraping (claude-squad), `"esc to interrupt"` regex with a 3.5s
timer (Omnara), prompt regex + idle timers (VibeTunnel), and **patching the
Claude Code binary** (VibeTunnel `claude-patcher.ts`). happy's fd-3 fetch
instrumentation is the only elegant one, and even it infers "thinking" from HTTP
traffic. omt's tier model with a hard cap on tier 0 is a real correction to a
real, observable, widespread failure — but note that it is correctness
engineering, not a user-visible feature. Users will not buy it; they will only
notice its absence.

**10.5 Rendering the message queue.** Claude Code writes explicit
`queue-operation` `enqueue`/`remove` lines to the transcript. Nothing in this
survey mirrors pending queued messages to a remote client. Small, unique,
genuinely useful when you have typed three follow-ups from a phone and cannot
tell which have landed.

**10.6 Federation across your own machines.** Only two products address it:
VibeTunnel HQ mode and coder/mux's SSH runtime. Both are partial. See §12(d).

**10.7 What omt cannot fix, and should stop pretending it can.** The sharpest
criticism of the entire local-execution model comes from HN on Codex mobile
[C `blevinstein`, id=48140529]: a phone tethered to your laptop "doesn't actually
offer cloud coding agents", so there is no truly independent work when the laptop
is away or asleep. happy ships `caffeinate` and a LaunchAgent daemon precisely
because of this, and it is a patch, not a fix. The hosted tier wins this outright.
omt should say so plainly in its own docs rather than let users discover it.

Two related realities from the same threads: short mobile prompts create
ambiguity and cause *more* babysitting, not less [C `jumploops`]; and large native
toolchains (10 GB Rust `target/`, 10-minute compiles) make phone-driven work
impractical for many real repos [C `nextaccountic`]. Neither is omt's fault.
Neither is solved by omt either.

---

## 11. Sober competitive assessment

The uncomfortable summary: **happy is 80% of omt's remote story, MIT-licensed,
23k stars, shipped today, in eleven languages, with E2EE, voice, self-hosting,
Codex/Gemini/ACP backends, and native AskUserQuestion cards.** Any positioning
that does not start from that fact is fiction.

What happy is not, and what remains open:

- happy is **not a multiplexer**. No panes, no layouts, no BSP tree, no terminal on the phone at all, no block model, no VT parser of its own. It is a chat client with tool cards.
- happy has **no TUI**. There is no local interface; you run `happy` instead of `claude` and then look at your phone or the Electron app.
- happy uses **one hook** (`SessionStart`) and ignores the other twenty-nine. No `PreToolUse` defer, no `Notification`, no `Stop`, no transcript-derived attention states beyond what the SDK emits.
- happy's reliability reputation is **bad and load-bearing** [C], even though the current source contradicts it. Reliability is a shippable differentiator.
- happy has **no public API surface and no parity discipline**. omt's capability catalog with CI-enforced parity across TUI/API/web is genuinely novel in this category — nobody else even attempts it.

And the second uncomfortable fact: **Anthropic's Remote Control is free on every
plan, has QR pairing, worktree spawning, presence-aware push, hardware-attested
device trust, and gets better with every Claude Code release.** The moat question
HN asked Omnara — *"what's your moat against Anthropic just launching the same
thing"* — has already been answered for the single-agent case. Anthropic launched
it. omt's answer must be the things Anthropic structurally will not build:
multi-vendor, multi-machine, self-hosted, no relay, real terminal.

---

## 12. omt's five intended differentiators, evaluated

### (a) Running the user's *real* CLI unmodified, rather than a wrapper or hosted agent

**Verdict: partially differentiated, and weaker than it sounds.**

Against the hosted tier (Cursor, Devin, Terragon, Conductor Cloud) this is real
and valuable — those reimplement or relocate the agent loop, and users
demonstrably notice (*"Same model, opus, works better in 3P harnesses"*;
Cursor's 10-minute cold starts; Devin's 3/20 success rate).

Against the peers it is thinner than it looks:

- Claude Code on the web already runs the real binary with your `.claude/` settings, hooks, MCP config, skills, and plugins.
- Factory Droid's pitch is stronger than omt's: *"does not require uploading or indexing a static copy of your repository."*
- Omnara's legacy wrapper `pty.fork()`s the real `claude` — it just scrapes the screen afterwards.
- happy's local mode inherits your real TTY and in-processes your globally installed `claude` — arguably *more* unmodified than omt's plan, since it does not even open a new PTY.

But note the qualifier every one of these violates: **unmodified**. happy
in-processes `claude` through a `fetch`-monkey-patching shim and injects
`--append-system-prompt`, `--mcp-config`, and `--settings`. VibeTunnel **patches
the binary on disk**. Omnara forces `--session-id` and rejects `--continue`
outright. omt's genuine claim is narrower and defensible: *the user's real CLI,
in a real PTY, with its own TUI, its own keybindings, and its own slash commands,
observed from outside rather than instrumented from within.* Say exactly that.
Claiming "runs your real CLI" unqualified will be met with "so does happy".

### (b) Native rendering of AskUserQuestion cards remotely

**Verdict: commoditized. This is not a differentiator.**

happy ships `AskUserQuestionView.tsx` with i18n in 11+ languages, special-cases
`AskUserQuestion` so it is never auto-approved even in yolo mode, has a dedicated
notification path for it, and injects the answer over a typed RPC. Claude's own
mobile app renders questions natively. Terragon proved the MCP-permission-tool
route in 2025. **happy does this better than omt's plan currently describes,
because happy also allows editing tool arguments before approval** — a capability
omt's docs do not mention.

The residual differentiation is narrow but real:

1. **Mechanism.** omt plans `PreToolUse` returning `permissionDecision: "defer"`, which parks the tool call without requiring the SDK's `canCallTool` and therefore works with a **TUI-attached interactive session**, not just an SDK-driven one. happy's remote mode is SDK-driven; its local mode uses no permission hook at all. If omt's defer spike holds, omt can render and answer a card from a phone *while the user's real interactive TUI is on screen*. Nobody does that. **This, not "we render cards", is the claim.**
2. **Answering the same card from the TUI, the API, or the phone**, with parity enforced in CI.
3. **Multi-agent generalization** — the same card abstraction over Codex approvals, Gemini `BeforeTool`, and opencode `permission.ask`.

The defer spike is therefore not one risk among many; **it is the load-bearing
assumption for the only part of (b) that is differentiated.** If defer does not
work as specified, omt's flagship feature degrades to a worse copy of happy.

### (c) Being a real terminal multiplexer rather than a chat UI

**Verdict: genuinely differentiated. The strongest of the five.**

No product in this survey combines a real VT parser, panes and layouts, and a
mobile client. The closest attempts each miss:

- VibeTunnel streams a real PTY to a mobile browser but has no layout model, no block model, and detects state by patching the Claude binary.
- Claude Desktop has a real terminal — local sessions only, never on the phone.
- happy has `node-pty` — Electron desktop only.
- Conductor has "Big Terminal Mode" — macOS desktop, explicitly without notifications.
- another terminal is a real terminal but cloud-only, closed, and its remote view is structured, not a tty.
- coder/mux claims "mobile-compatible server mode" but is AGPL and unverified on permissions.

The **block model is the specific unclaimed idea.** OSC 133 segmentation turning
scrollback into a collapsible list of command blocks on a phone, with the full
terminal one tap away, is the answer to the raw-PTY-vs-structured-cards split
that the whole category is stuck on. Nobody has shipped it.

Caveat, and it is not small: this is also the most expensive thing on omt's
roadmap. A correct VT parser with grid, scrollback, and reflow is a multi-month
project that competes directly against iTerm2 and another terminal on their home ground, and
the payoff is only realized once the mobile block view exists. Sequencing
matters: **a mediocre terminal plus great cards loses to happy; a great terminal
with no cards loses to VibeTunnel.**

### (d) Multi-instance federation across the user's own machines

**Verdict: genuinely differentiated, and the most under-defended territory in
the survey.**

Prior art in full:

- **VibeTunnel HQ mode** — the only real implementation. `hq-client.ts`, `remote-registry.ts`, `mdns-service.ts` aggregate multiple remote VibeTunnel servers into one pane, with LAN discovery. MIT, works, and almost nobody knows about it.
- **coder/mux SSH runtime** — per-task remote compute, not federation.
- **Factory Droid Computers** — persistent remote machines including BYOM over an outbound relay, but Factory's SaaS is the control plane.
- **happy** — one daemon per machine with a machine-scoped socket and `spawn`/`resume` RPC, and `machine/[id].tsx` in the app. This is closer to federation than most people realize, but it federates *through happy's relay*, so it is star-topology, not peer aggregation.
- Everyone else: nothing.

omt's specific design — **each instance authoritative for its own sessions, with
the web client as a federating view over instances it holds credentials for, and
no store-of-record** — is stronger than all of these. It has no single point of
failure, it falls out naturally on Tailscale, and it needs no relay at all. This
is the differentiator with the least competition and the clearest architectural
argument.

The risks are execution risks, not positioning risks: credential management
across N instances, clock/ordering across independent authorities, partial
availability in the fleet view, and a genuinely hard first-run UX. VibeTunnel's
HQ mode being obscure despite working is a warning that federation is easy to
build and hard to make legible.

### (e) Full TUI / API / web parity

**Verdict: genuinely differentiated as engineering discipline; near-invisible as
a user-facing feature.**

Nothing in this survey attempts mechanically enforced surface parity. opencode
comes closest structurally — `opencode serve` is the backend and the Go TUI is
just another client that can `attach`, so TUI/web/mobile inherently share one
session — but there is no capability catalog and no CI parity gate. Factory's
TypeScript SDK plus WebSocket daemon is the closest thing to an open remote-control
substrate but has no parity discipline. Cursor is the anti-example: its mobile
app requires a privacy-mode downgrade that removes settings from the UI entirely.

The honest assessment: **parity is how omt avoids the failure everyone else has**
(Claude Desktop cannot see CLI sessions; Cursor mobile requires a different
privacy mode; Conductor's Big Terminal Mode has no notifications; Devin's phone
story is a PWA afterthought; Factory's mobile "terminal" is rendered output, not
a terminal). Every one of those is a parity failure that shipped. So the
discipline is validated by the field's failures.

But no user has ever written an HN comment praising a capability catalog. Parity
sells only through its consequences: "everything you can do at your desk, you can
do from your phone" is the sentence; the catalog is the machinery. And it carries
a real cost — every feature must be designed three ways before it ships once,
which is precisely the tax that makes competitors ship the desktop path first
and the phone path never. **That tax is the differentiator; it is also the thing
most likely to slow omt down enough to matter.**

---

## 13. Recommendations

1. **Do the `PreToolUse` defer spike first, before anything else.** It is the only part of differentiator (b) that survives contact with happy, and it gates the flagship feature.
2. **Lead with (c) and (d) — multiplexer and federation.** Those are the two that hold up unambiguously. Do not lead with AskUserQuestion cards; happy will be cited within one HN comment.
3. **Do not build a raw xterm.js mobile view without the block model and a touch key bar.** Without both, omt inherits every complaint from ttyd through VibeTunnel and adds nothing.
4. **Copy, without hesitation:** QR pairing on spacebar (Claude/Codex/happy); `.worktreeinclude` for untracked files (Claude Desktop); presence-aware push suppression (Claude); VAPID web push with keys at 0600 (VibeTunnel); `updatedInput` merge on approval (happy); Bash prefix-pattern allowlists (happy); per-agent dev-server port allocation (uzi); multi-channel notification including SMS (Omnara); "Approve tool calls from your phone" and "Still working — check in from your phone" nudges (Claude).
5. **Surface `away_summary` and the `queue-operation` stream.** Both are free, both are unique, both directly address the loudest baseline complaint ("easy to lose track of what is going on across all those sessions").
6. **State the local-execution limitation in omt's own docs.** Your laptop must be awake and online. The hosted tier beats omt on that axis and always will. Saying it first is cheaper than having `blevinstein` say it for you.
7. **Avoid AGPL dependencies** (claude-squad, coder/mux). It is a documented adoption blocker for exactly the embedders omt wants.
8. **Treat reliability as a feature, not a baseline.** happy's architecture is excellent and its reputation is *"rarely works reliably"*. That reputation is the opening — but only if omt does not earn the same one.
