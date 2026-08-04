# Keybindings

## The rule

omt is a terminal before it is anything else. A key it has no opinion about
reaches the program you are talking to, byte for byte. The default keymap is
deliberately small — every binding is a key taken from something else, and each
one has to earn that.

## Reserved: omt will never take these

| Key | Why |
|---|---|
| `Ctrl+C` | It is how every CLI is interrupted |
| `Ctrl+D` | It is end-of-input to every shell and REPL |
| `Ctrl+Z` | It is how a job is suspended |
| `Ctrl+\` | It is the quit signal of last resort |

Binding any of these globally is an **error**, not a warning:

```
error[OMT-C410]: omt refuses to bind `ctrl-c` globally
  = note: it must reach the inner program: it is how every CLI is interrupted
```

They *can* be bound inside an overlay, and that is not an exception so much as
the point: while a palette is open, `Ctrl+C` should close the palette. An
overlay that leaked `Ctrl+C` to a running build would be a data-loss bug.

## The prefix

`Ctrl+A`, then:

| Key | Does |
|---|---|
| `d` | Detach |
| `Ctrl+A` | Send a literal `Ctrl+A` |

A mistyped chord does nothing rather than reaching the program. Silently
running something you did not mean is worse than losing a keystroke.

## Defaults

| Key | Does |
|---|---|
| `Cmd+K` / `Ctrl+Shift+P` | Open the command palette |
| `Cmd+T` | New session |
| `Cmd+D` | Split right |
| `Cmd+Shift+D` | Split down |
| `Cmd+W` | Close pane |
| `Cmd+F` | Search |
| `Cmd+Alt+←→↑↓` | Focus a neighbouring pane |

`cmd` and `super` are the same modifier under two names, so a config written on
a Mac loads on Linux instead of silently binding nothing.

**Everything is reachable from the palette**, so a binding is an optimization
for frequency rather than a requirement. That is what lets the un-prefixed key
budget stay one key wide instead of inventing a chord for every operation.

## Rebinding

`~/.config/omt/keybindings.toml`:

```toml
[global]
"cmd-j" = "palette.open"
"cmd-k" = "@unset"        # drop a default without knowing where it came from

[overlay]
"ctrl-c" = "ui.close"
```

`@unset` matters more than it looks: a plain absence means "no opinion", and no
opinion is exactly what inherits. Only an explicit unset removes a default.

Chords canonicalize, so `ctrl-shift-p` and `shift-ctrl-p` are one binding. Two
spellings of one chord would otherwise let a rebind silently shadow nothing.

## Generating this list

Your actual keymap, including your own bindings:

```sh
omt keys --cheatsheet          # a table
omt keys --cheatsheet --json   # for a script
```

Generated from the keymap rather than maintained by hand. A hand-written
cheatsheet is wrong the first time anybody rebinds anything, and being
confidently wrong about a keybinding is worse than having no list at all.

The generated sheet includes the reserved keys, marked as reaching the program —
"`Ctrl+C` interrupts what is running" is the most useful line on it, and it
would be invisible if only bindings were shown.

## Vim and emacs modes

Both are keymap layers rather than reimplementations. omt does not emulate vim;
it gives vim users a mode where the motions they expect are bound, and gets out
of the way when a real vim is running — the alternate screen is the signal, and
a full-screen program owns every key while it is on screen.
