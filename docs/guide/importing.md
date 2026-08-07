# Importing your setup

A fresh install that looks nothing like what you were using is one you close.

```sh
omt import other terminals
omt import vscode
omt import --dry-run vscode # show what would change, change nothing
```

## What comes across

| Yours | Becomes |
|---|---|
| font family, size, line height | `appearance.font.*` |
| letter spacing, ligatures | `appearance.font.*` |
| cursor style and blink | `appearance.cursor.*` |
| scrollback lines | `terminal.scrollback_lines` |
| theme colours | a theme in `~/.config/omt/themes/` |

## What does not, and why you are told

```
imported 6 settings from VS Code.
2 settings have no omt equivalent:
 terminal.integrated.shellIntegration.decorationsEnabled
 — omt has no equivalent setting
 terminal.integrated.gpuAcceleration
 — omt has no equivalent setting
```

Every unmapped key is listed. A migration that silently dropped half a config
is worse than one that did nothing: you would believe you had moved, and find
out weeks later that something you rely on never came across.

An unknown setting does not abort the import either. A migration that refused
because it met one key it did not recognize is a migration nobody completes.

## Conversions that are not one-to-one

- **VS Code writes a font *stack*** — `"'Fira Code', Menlo, monospace"`. omt
 takes the first real family. Passing the whole string through would name a
 font that does not exist.
- **Cursor styles are the same set under different names.** `line` becomes
 `bar`; `block` and `underline` carry over. A style omt does not have is
 reported rather than guessed at.
- **A value of the wrong type is reported, never coerced.** Turning `"large"`
 into a number would give you a font size you never chose.

## Where your settings usually are

omt suggests these; it does not go looking. Reading a file you did not point at
is a surprise, however convenient.

| | |
|---|---|
| other terminals | `~/.other terminals/settings.json` |
| VS Code (macOS) | `~/Library/Application Support/Code/User/settings.json` |
| VS Code (Linux) | `~/.config/Code/User/settings.json` |
| iTerm2 | `~/Library/Preferences/com.googlecode.iterm2.plist` |

## After importing

```sh
omt config sources appearance.font.size
# appearance.font.size = 13
# from ~/.config/omt/config.toml (User layer)
```

Every setting knows where it came from. When something is not what you expect,
that is the command that says why — across all six layers, without guessing.
