# A client that works on a phone

## Why

The rebuilt client was verified in a desktop browser at phone dimensions. That
is not the same as a phone, and it hid the two failures that matter most.

**You cannot type.** Input is a `keydown` listener on `document`. A desktop has
a physical keyboard so that works; a phone has no keyboard until something
focusable is focused, and there is no focusable element anywhere in the client.
So the terminal is read-only on the device the whole product exists for. A
390×844 desktop window cannot show this, because the desktop keyboard is always
there.

**It spins forever.** The terminal screen renders a spinner whenever its
snapshot is null, and `store.subscribe(() => this.render())` fires on every
store change — including ones with nothing to do with the terminal. A refresh
arriving between a route change and the first snapshot repaints the drawn
terminal back into a spinner. Two independent things call `render()` and only
one of them knows whether the screen has content.

**And the checklist missed a category.** It tracked capabilities: is there a
call, is it reachable, is it tested. Every entry was true. But
`docs/architecture/08-web-client.md` also specifies **signed invite links**, a
**QR code** on the desktop, an **in-app QR scanner**, and **device-bound
credentials** — none of which exist, and none of which a capability checklist
would ever have caught, because none of them are capabilities. Pairing a second
device today means copy-pasting a token from a terminal into a phone by hand.

## What changes

- **A real input element**, off-screen but focused, so the soft keyboard opens
  and IME, autocorrect-off, paste and dictation all work through the path
  browsers already have.
- **The terminal owns its state.** One place decides what the terminal screen
  shows, so an unrelated store update cannot repaint content back to loading.
- **Pairing that is not copy-paste**: `omt web` prints a QR, and the client has
  a scanner for adding an instance.
- **More than one instance**, listed and switchable, as `08-web-client.md`
  describes.
- Touch behaviour a phone expects: tap-to-focus, the key bar above the keyboard,
  and a viewport that does not jump when the keyboard opens.

## What this deliberately does not do

**No on-screen keyboard of omt's own.** The device keyboard is the one people
can type on — theirs has their layout, their languages, their swipe, their
dictation. A custom key grid is a worse keyboard that also has to be maintained.

**No re-verification in a desktop browser only.** Every claim in this change is
checked at a real touch viewport with `hasTouch`, because the last round's
verification is exactly what missed all of this.
