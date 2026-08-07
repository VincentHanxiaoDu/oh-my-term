# oh-my-term (`omt`)

An agent-aware terminal multiplexer with a remote web client.

`omt` runs your coding-agent CLIs — Claude Code, Codex, opencode, Gemini CLI
and others — **unchanged**, while understanding what they are doing. It knows
which agent is running, whether it is working or waiting for you, and which of
its subagents needs an answer. That state reaches your phone, and you can
answer from there while the real TUI is still on your screen, in sync.

```
omt                    # run a shell here
omt run zsh            # run something specific
omt serve              # listen on a Unix socket
omt web                # listen for browsers, prints a token once
omt serve              # a daemon whose sessions outlive every terminal
omt attach             # attach a terminal to it; Ctrl-A d leaves it running
omt ssh my-dev-box     # attach to an omt on another machine
```

`Ctrl-A d` detaches. Every other key belongs to whatever is running.

---

## Why

Every terminal multiplexer treats an agent as an opaque program that prints
bytes. Every agent-monitoring tool replaces the agent's own UI with its own.
`omt` does neither: it runs the real CLI and *observes* it, so slash commands,
permission prompts and question cards behave exactly as their authors built
them — and are simultaneously legible to a phone.

The design principle that constrains everything else: **omt reports what it was
told, never what it inferred.** A screen heuristic can say "something is
happening". Only a source that was told — the agent's own hook, its own
protocol — can say *what*. That boundary is enforced by the type system and
tested across every adapter, because the failure it prevents is a card someone
taps "Allow" on that was assembled from pixels.

## Status

Working, and honest about what is not:

- ✅ Runs a real shell with a real terminal emulator, renders it, takes input
- ✅ Detects agents, tracks state, tracks subagents, mirrors interactions
- ✅ Remote over a Unix socket, over ssh, and over HTTP/WebSocket
- ✅ Worktree fan-out, scheduled runs, git status and diff, file transfer
- ✅ TLS: `omt web --bind HOST:PORT --tls-cert C --tls-key K` (loopback by default)
- ⚠️ Hook installation reformats JSON configs (content is preserved exactly)
- ✅ Panes: `Ctrl-A s` splits, `Ctrl-A o` cycles, `Ctrl-A x` closes
- ✅ Command blocks, including for shells that emit no OSC 133 at all

## Install

```sh
cargo install --path crates/omt      # from a clone
```

Requires Rust 1.90+. macOS and Linux; Windows via WSL2.

## Documentation

| If you want to | Read |
|---|---|
| Get going in five minutes | [Getting started](docs/guide/getting-started.md) |
| Know what every key does | [Keybindings](docs/guide/keybindings.md) |
| Bring your another terminal/VS Code setup | [Importing](docs/guide/importing.md) |
| Write a plugin | [Plugin guide](docs/guide/plugins.md) |
| Make a theme | [Themes](docs/guide/themes.md) |
| Drive omt from a script or an agent | [Capability reference](docs/reference/capabilities.md) |
| Understand how it is built | [Architecture](docs/architecture/README.md) |
| Know why a decision was made | [Decisions](docs/architecture/decisions.md) |

## How it is put together

28 crates in one workspace, layered so a change lands in one place:

```
L0  omt-types · omt-util                         vocabulary
L1  omt-catalog · omt-events · omt-proto         contracts
L2  omt-term · omt-pty · omt-transport · …       subsystems
L3  omt-session · omt-agent · omt-config · …     domain
L4  omt-daemon                                   the instance
L5  omt-tui · omt-server · omt-plugin-host       surfaces
L6  omt · omt-hook                               binaries
```

A crate may depend downward only, and `cargo xtask layering` fails the build if
that stops being true.

The spine is the **capability catalog**. Every operation is declared once, and
that declaration generates the dispatch, the routes, the TypeScript client, the
CLI tree and the reference table. A capability that reaches the TUI but not the
web client fails a test called the parity gate — which is proven to fail, not
assumed to work.

## Contributing

```sh
cargo test --workspace          # 900+ tests
cargo clippy --workspace --all-targets
cargo run -p xtask -- layering  # the crate graph
cargo run -p xtask -- codegen   # regenerate what is generated
cd web && npm test              # the browser client
```

Every test name is a sentence about behaviour, and every non-obvious decision
carries the reason it was made. If you find a comment that says *what* the code
does rather than *why*, that is a bug.

## License

Apache-2.0
