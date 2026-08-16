# The mouse belongs to the program in the pane

## Why

omt has a complete mouse design — the passthrough table in
[18 §5.1](../../../docs/architecture/18-semantic-open.md), mouse triggers in
the keymap grammar, a `MouseEvent` in the input document — and no
implementation of any of it. It never enables mouse reporting on its own
terminal, never decodes a mouse event, and has no selection by pointer. A user
dragging across a pane gets whatever the outer terminal does over omt's frame,
which is a rectangle of the whole screen including the chrome of every other
pane.

The emulator has always tracked mouse modes, so what a program asked for is
already known; nothing has ever asked it.

## What changes

- omt asks its own terminal for mouse reporting, and stops on the way out.
- The three tracking modes are held apart, because 1000, 1002 and 1003 are
  three different requests and a click-only program sent motion reports
  receives input it cannot parse.
- DECSET 1007 is tracked, so the wheel is translated to arrow keys when a
  program asked for that and not merely because the alternate screen is active.
- Forwarding re-encodes the event for the program: SGR where it asked for SGR,
  the legacy encoding otherwise, with the modifier bits kept and every press
  followed by its release.
- Where no program has grabbed the mouse, a drag selects — through the copy
  mode that already exists, in absolute rows, and the release puts the text on
  the clipboard the same way `y` does.
- Shift takes an event back from a program that has grabbed the mouse, which
  is the long-standing convention every terminal omt runs inside implements.

## Impact

- `omt-term`: two mode variants split out, one added; no behaviour change for
  anything reading `MouseTracking`.
- `omt-input`: encoding a mouse event for a program.
- `omt`: enabling reporting, the routing rule, drag selection, the wheel.
- No capability, no wire change.
