# Getting started

## Run it

```sh
omt
```

That is a shell, in a real terminal emulator, in a session `omt` is watching.
Type as you normally would. Everything you press goes to the shell, except one
chord.

## The one key omt takes

`Ctrl-A` is the prefix. On its own it does nothing visible; it waits for the
next key.

| After `Ctrl-A` | Does |
|---|---|
| `d` | Detach — leave the session running and exit |
| `Ctrl-A` | Send a literal `Ctrl-A` to the program |

A prefix rather than a bare key, because every bare key belongs to whatever you
are running. If you have ever lost `Ctrl-B` to tmux inside emacs, you know why
this budget is one key wide.

**`Ctrl-C`, `Ctrl-D`, `Ctrl-Z` and `Ctrl-\` always reach the program.** omt
refuses to bind them, and refuses to let your config bind them. A terminal
where `Ctrl-C` does not interrupt is a terminal you cannot rescue.

## Run an agent

```sh
omt
$ claude
```

Nothing special. `omt` notices — by the environment variable Claude Code sets,
not by reading the screen — and starts tracking:

- whether it is working, idle, or waiting for you
- which subagents it spawned and what each is doing
- any question or permission card it raised

To see that from elsewhere, you need a remote surface.

## Reach it from your phone

```sh
omt web
# omt web on http://127.0.0.1:7717
# token: omt_c_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
# (shown once — it cannot be recovered)
```

The token appears once. It is stored hashed, so a leaked state directory is not
a leaked credential — and there is no way to print it again. Lost it? Mint
another.

The server binds loopback. To reach it from a phone you need one of:

- **Tailscale or a VPN** — point the phone at the machine's tailnet address
- **`omt ssh`** from another machine, which needs no open port at all
- **your own reverse proxy** with TLS

omt does not implement TLS. Doing it well means certificate management,
renewal and a trust store, and `caddy` and `ssh` are both better at it than a
terminal multiplexer would be. What omt refuses to do is bind every interface
by default — that would put your shell on the coffee-shop network, and the
person it happens to is not the person who chose the default.

## Reach another machine

```sh
omt ssh my-dev-box
```

This runs *your* `ssh`. Your config, your agent, your jump hosts, your hardware
key — all already work, because omt did not reimplement any of it. The
destination string is passed through untouched, so `Host` aliases resolve.

On the far side it runs `omt bridge`, which connects to that machine's own
socket. One hop more than forwarding a path, and it buys the thing that
matters: a forwarded path can point at a socket that has since been replaced,
and you would attach to somebody else's sessions without either end noticing.

## Bring your existing setup

```sh
omt import other terminals
omt import vscode
```

Fonts, sizes, cursor style, scrollback, and themes come across. Anything omt
has no equivalent for is **listed**, not dropped — a migration that silently
lost half your config is worse than one that did nothing, because you would
believe you had moved.

See [Importing](importing.md).

## Where things live

```
~/.config/omt/
├── config.toml # your settings
├── secrets.toml # 0600, never merged into config.toml
├── keybindings.toml # your key overrides
├── themes/ # your themes
└── plugins/<id>/ # installed plugins
```

Per-project settings go in `<repo>/.omt/config.toml`. That file arrives with a
`git clone`, so it is restricted: it cannot set `server.bind`, or anything else
that would let a repository change what your machine does. Those keys are
dropped with a diagnostic rather than silently ignored.

## Next

- [Keybindings](keybindings.md) — the full list, generated from the keymap
- [Themes](themes.md) — the format, and importing one
- [Plugins](plugins.md) — writing one
