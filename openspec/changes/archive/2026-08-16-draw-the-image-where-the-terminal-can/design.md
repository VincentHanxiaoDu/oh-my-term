# Design

## Keeping the payload

The scanner already sees every byte of the transmission; it now keeps them. The
bytes are kept **as they arrived** — base64 — because that is the form they go
back out in. Decoding to re-encode would be work done twice to arrive at the
same string.

Bounded per terminal, oldest first. An image whose payload was evicted keeps
its rows and its card: the placement is small and the payload is not, and a
scrollback of a hundred screenshots must not be a process that grows until it
is killed.

Only what can be re-sent is kept. kitty takes PNG data directly; a JPEG cannot
be handed to it without decoding, which would mean an image codec inside the
terminal core. A payload omt cannot re-send is dropped and its card stays,
which is the same outcome as eviction and needs no new state.

## The handshake

kitty's own query — a one-pixel transmission with `a=q` — answered with `OK`.
It is the only reliable positive test, and it is a test rather than a guess.

It has to read the **raw descriptor**. `vte` swallows APC in the pane and
crossterm swallows it on omt's own input, which is exactly the defect that made
an image invisible in the first place; a handshake that went through either
would never see its answer.

It happens **once, after raw mode and before the first keystroke is read**. omt
already refuses to query a terminal mid-session — the reply arrives in the same
stream as typing, and an answer mistaken for keystrokes puts escape text into
somebody's shell. There is exactly one moment where that is not true, and this
is it.

No reply within a short deadline means no support. Not an error, not a retry:
most terminals will not answer, and that is the ordinary case rather than a
failure.

## Drawing

An **overlay after the pane is rendered**, not a change to the renderer. The
diffing renderer keeps writing the card cells exactly as it does now, and the
image is drawn over them.

That is what makes the fallback structural. Every path that ends in no image —
no support, no cell size, evicted bytes, a clip that leaves nothing visible —
lands on the card, because the card was already drawn. There is no branch that
has to remember to draw something instead.

Clipping uses kitty's source rectangle, which is in pixels, so it needs the
terminal's cell size. Where that is unknown, nothing is drawn: a guessed cell
size crops the wrong part of the image, and a wrong crop looks like a rendering
bug rather than a missing measurement.

## Re-sending

A signature per placement: the image, the pane, the row it lands on, and what
is clipped off it. Unchanged signature, nothing sent. A frame is drawn many
times a second and an image is measured in megabytes.

Deletion is by the same id when a placement goes away, so a scrolled-off image
does not stay on the host's screen after omt has stopped drawing its rows.
