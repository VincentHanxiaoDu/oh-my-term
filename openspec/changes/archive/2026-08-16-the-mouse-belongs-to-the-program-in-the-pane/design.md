# Design

## The modes, held apart

`TermMode::MouseTracking` becomes three:

```rust
MouseClicks,        // 1000: presses and releases
MouseButtonMotion,  // 1002: and motion while a button is down
MouseAnyMotion,     // 1003: and every motion
AlternateScroll,    // 1007
```

`Modes::mouse_tracking()` answers "any of the three", so nothing that only
wanted to know *whether* the mouse is grabbed changes. What is new is being
able to ask *which*, which is the difference between forwarding what a program
asked for and forwarding a stream it will misparse.

## Encoding

`omt_input::mouse::encode(event, encoding) -> Vec<u8>`, with the button byte
built from button, drag bit and modifier bits, and the two encodings:

* SGR (1006): `CSI < b ; col ; row M|m`, and the only one that survives past
  column 223.
* Legacy: `CSI M` with three bytes offset by 32. A coordinate that does not fit
  in a byte is **dropped rather than wrapped** — a wrapped column is a click
  somewhere else, which is worse than a click that did not happen.

Modifier bits are included. A gesture with Ctrl held is a different gesture,
and delivering it as a plain click silently changes what the user did.

## Routing

Per event, in order:

1. Not over a pane's grid → omt's, whatever the modes say.
2. Shift held → omt's. The convention every terminal implements, and the reason
   applications do not expect shifted clicks.
3. The pane's program asked for this class of event → forwarded.
4. Otherwise → omt's.

A press forwarded to a program pins the rest of that gesture — motion and
release — to the same pane, so a drag that leaves the pane still ends where it
started. Otherwise a program sees a button go down and never come up.

## Selection

Reuses copy mode rather than adding a second selection: `CopyMode::entering`,
position from the click, `mark` on press, position on drag, `yank` on release.
That gets absolute rows, wide-character handling, and the same text extraction
the keyboard path already produces — and it means a mouse selection can be
continued with the keyboard, which two separate implementations could not.

A press that never moved selects nothing. Copying on release is what a click
would otherwise silently replace the clipboard with.

## The wheel

Decided per pane, in the order in the spec. The middle case reads 1007 rather
than assuming it from the alternate screen: a full-screen program that did not
ask for alternate scroll is an editor whose cursor would move when somebody
scrolls.
