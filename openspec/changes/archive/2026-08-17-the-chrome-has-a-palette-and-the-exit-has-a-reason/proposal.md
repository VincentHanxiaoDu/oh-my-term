# The chrome has a palette, and leaving has a reason

## Why

Two things reported from a real terminal, in one sitting.

**omt vanished.** It went back to a shell prompt with nothing said, which
cannot be told apart from a crash — and was reported as one. It was not a
crash: the last session's shell had been stopped by a signal, omt closed
because there was nothing left to show, and the session had been carrying that
exit status the whole time. Nothing asked it, and nothing said it.

**The chrome is a white slab.** The status bar and every panel were drawn in
reverse video, which is the one highlight every terminal has and needs no
theme. That is right for a *selection*. Across a whole row it is a solid block
of the foreground colour, so on a dark theme the bar is a white bar and a panel
is a white box. A theme has existed here the whole time — background,
foreground, sixteen colours, shared with the web client — and **no chrome ever
read it**.

## What changes

- A palette derived from the theme: surface, picked, border, muted, accent —
  small steps from the background rather than an inversion of it.
- It degrades completely: truecolor, then the 256-colour cube, then the reverse
  video it always had. Nothing in it carries meaning, so a terminal that loses
  it loses nothing.
- Panels float: rounded corners, and the centred margin left unpainted so a
  panel is a panel rather than a full-width band with a box inside it.
- When the last session ends, omt says why — including which signal, where the
  status stands for one — **after** the terminal is handed back, because a
  message written onto the alternate screen is thrown away with it.
- A panic leaves a note in a fixed place, for the same reason.

## Impact

- `omt-tui`: a paint module, and panel lines that say what they are.
- `omt`: the bar, the panels, the separators, the copy-mode hint, and the
  reason on the way out.
- No capability, no wire change, no configuration key.
