# Themes

## The format

The sixteen ANSI colours plus the handful of terminal-wide ones. That is what
every terminal emulator has agreed on for decades, and omt adds nothing to it
deliberately — a theme only omt can read is a theme nobody ports, and sharing
is the entire point of a theme format.

```toml
name = "Solarized Dark"
appearance = "dark"

foreground = "#839496"
background = "#002b36"
cursor     = "#268bd2"
selection  = "#073642"

[palette]
normal = ["#073642", "#dc322f", "#859900", "#b58900",
          "#268bd2", "#d33682", "#2aa198", "#eee8d5"]
bright = ["#002b36", "#cb4b16", "#586e75", "#657b83",
          "#839496", "#6c71c4", "#93a1a1", "#fdf6e3"]
```

Drop it in `~/.config/omt/themes/` and set `appearance.theme = "Solarized Dark"`.

`#rgb`, `#rrggbb` and the same without the hash all parse. Being strict there
just refuses a value that is obviously a colour.

### `appearance` is stated, not inferred

The theme's author knows whether it is meant for a light or dark terminal.
Guessing from the background gets borderline themes wrong, and that wrong answer
then propagates into every contrast decision the UI makes.

## Importing

```sh
omt theme import ~/.another terminal/themes/solarized.yaml
omt theme import ~/Downloads/dracula.json      # VS Code
```

Both formats are read. Each import reports what it could **not** map:

```
imported "Dracula" — 3 keys have no terminal equivalent:
  activityBar.background
  statusBar.background
  editorGroup.border
```

Reported rather than dropped. Somebody who spent an hour on a VS Code theme
should be told which parts a terminal cannot show, instead of wondering why it
looks different.

Two details worth knowing:

- **VS Code themes without `terminal.*` keys fall back to the editor colours.**
  Most themes never set the terminal ones, and refusing them would reject most
  of the ecosystem.
- **Eight-digit colours are truncated to their RGB.** A terminal palette has no
  alpha channel, and truncating is closer to the author's intent than refusing.

## Contrast

omt checks the contrast between your text and background and **warns**:

```
theme "Midnight": text-to-background contrast is 2.8:1, below the 4.5:1
usually considered readable
```

A warning, never a refusal. A theme is somebody's taste, and refusing to load
one because a checker disliked it would be omt overruling the user about their
own screen. Some people want low contrast; they are allowed to have it.

It also warns about a cursor that is nearly the background colour, and a
selection you will not be able to tell from unselected text — both of which
happen constantly when a light theme is imported into a dark terminal.

## Fonts

```toml
[appearance.font]
family = "JetBrains Mono"
size = 13.0
line_height = 1.2
letter_spacing = 0.0
ligatures = false
fallback = ["Apple Color Emoji", "Noto Sans CJK SC"]
```

**Ligatures are off by default.** They turn `!=` into a glyph that does not
look like what is in the file, and somebody debugging a comparison should see
the characters they typed.

The default fallbacks cover emoji and CJK, which are the two that wreck a
terminal's grid when they land on something proportional. If your terminal's
columns drift when a filename has an emoji in it, this is the setting.
