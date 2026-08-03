# oh-my-term (omt)

An agent-aware terminal multiplexer with a remote web client.

`omt` is a modern, modular terminal TUI that runs your coding-agent CLIs
(Claude Code, opencode, Codex, Gemini CLI, ...) natively — no wrappers, no
lost semantics — while understanding what they are doing. It detects which
agent is running, tracks its working state, captures structured interactions
such as Claude Code's `AskUserQuestion` cards, and mirrors them to a web
client you can drive from your phone over Tailscale.

> Status: early design. Architecture and specs live in `docs/` and `openspec/`.

## Goals

- **Native semantics.** Slash commands, permission prompts, question cards and
  message queueing behave exactly as they do in the agent's own CLI.
- **Workspaces and sessions.** Multiple terminals per project directory, with
  the history, completion and block model you expect from a modern terminal.
- **Remote by design.** One `omt` instance can serve many web clients; one web
  client can attach to many `omt` instances across many machines.
- **SSH-friendly.** Copy/paste and image paste work when you are `ssh`'d into a
  remote box running `omt`.
- **Modular.** Every layer — agent adapters, transports, auth, storage, STT —
  is a plugin behind a stable interface.

## License

Apache-2.0
