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

`Ctrl+B`, then:

| Key | Does |
|---|---|
| `d` | Detach |
| `Ctrl+B` | Send a literal `Ctrl+B` |

It is `Ctrl+B` because that is the prefix the largest group of incoming users
already has in their fingers. `Ctrl+A` is the one alternative worth
considering and it is rejected: in readline it is `beginning-of-line`, a key
people press constantly in a shell, and a shell is what half of omt's panes
contain.

Moving it is one line — see below.

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

`~/.config/omt/keybindings.toml`, and `.omt/keybindings.toml` in a project,
which outranks it — a keybinding is a config value and obeys the same
precedence as everything else in `config.toml`:

```toml
leader = "ctrl-a"     # relocates the whole `<leader>` namespace with one edit
keymap = "emacs"      # "default" | "vim" | "emacs"

"<leader> c" = "session.create"
"<leader> |" = { capability = "pane.split", args = { direction = "vertical" } }
"<leader> d" = "none"  # drop a default without knowing where it came from
"ctrl-shift-p" = "palette.open"

[[binding]]
trigger    = "enter"
when       = "card_focused"
capability = "interaction.resolve"
```

`"none"` matters more than it looks: a plain absence means "no opinion", and no
opinion is exactly what inherits. Only an explicit `"none"` removes a default,
and it stops there rather than revealing whatever the layer below had bound.

A binding that cannot be installed **says so** rather than disappearing: a
reserved chord without `force = true` is `OMT-C410`, a `platform` or `requires`
that this machine or terminal cannot meet is `OMT-C408`, and a `when` naming a
context omt does not have is refused with the word quoted back. One bad line
never costs you the rest of the file.

Chords canonicalize, so `ctrl-shift-p` and `shift-ctrl-p` are one binding. Two
spellings of one chord would otherwise let a rebind silently shadow nothing.

## Generating this list

Your actual keymap, including your own bindings:

```sh
omt keys --cheatsheet          # a table
omt keys --cheatsheet --json   # for a script
omt keys list                  # every binding, with the file it came from
```

`omt keys list` names the layer and the file for each binding, because "why
does this key do that" is answered by a path rather than by a list.

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
