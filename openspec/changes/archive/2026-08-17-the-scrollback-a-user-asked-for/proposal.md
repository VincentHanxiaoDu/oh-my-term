# The scrollback a user asked for

## Why

The first group out of `UNWIRED`, and the one with the plainest symptom.

`terminal.scrollback_lines` is in the settings reference, the settings screen
and the importer that maps other terminals' configurations onto omt's. Every
session was built with `ScrollbackLimits::default()`, so every session kept the
built-in ten thousand lines — while `config.get` reported the number the user
had asked for, with the file it came from. Somebody who set fifty thousand
lines got ten thousand and a settings screen agreeing with them.

`terminal.default_shell` is the same shape: documented, importable, and read by
nothing. The shell came from `$SHELL` or `/bin/sh`, so the one thing a user set
on purpose was the one thing ignored.

## What changes

- The scrollback cap reaches `ScrollbackLimits` at every place `omt` starts a
  session: the local terminal, `session.open` and `layout.open`.
- `terminal.default_shell` is preferred over `$SHELL`, after an explicit
  argument. An empty setting is not a shell — treated as one it becomes a
  program named "" and the pane dies with an error nobody can read.
- Both come out of `UNWIRED`, which is what the gate checks.

## Non-goals

- The daemon's own default when it creates a session without a client is
  unchanged: `omt-daemon` is below the configuration layer and cannot read it.
  The caller passes the limits in, which is the only direction the layering
  allows.
- `terminal.default_cwd` and the rest of the terminal group stay declared
  unwired; they are their own change.

## Impact

- `omt`: three spawn sites and two small readers.
- Depends on the gate change that declared the group.
