# Draw the image, where the terminal can

## Why

An image a program draws now reserves its rows and says what it is. What it
does not do is appear. The card is honest and it is not the point of drawing a
graph.

omt is a terminal application, so showing an image means asking the *outer*
terminal to draw it. That is only possible where the outer terminal can, and
omt's standing rule is that support is **confirmed, never assumed** — the media
tier already refuses "probably kitty, so assume its protocol", because probably
means a feature that silently does nothing and a user who cannot tell whether
they mis-typed or omt mis-guessed.

Two things make this awkward, and both are already known:

- **Nothing in omt can see the reply.** `vte` swallows APC in the pane and
  crossterm swallows it on omt's own input, which is the same defect that made
  the image invisible in the first place. The handshake has to read the raw
  descriptor.
- **omt refuses to query a terminal mid-session** — the reply arrives in the
  same stream as keystrokes, and a query whose answer is mistaken for typing
  puts escape text into somebody's shell. So the handshake happens once, after
  raw mode is entered and before the first keystroke can be read.

## What changes

- The payload is kept, bounded, so there is something to re-send. An image
  whose bytes were evicted keeps its card.
- A handshake, on the raw descriptor, before the input loop. No reply means no
  drawing, and no drawing means exactly today's behaviour.
- Where the terminal confirmed support, the image is drawn as an overlay after
  the pane is rendered, clipped to the pane's rectangle, and re-sent only when
  something about its placement changed.
- The card stays underneath it. That is the fallback made structural: if the
  image does not arrive, what was already going to be there is.

## Impact

- `omt-term`: the payload, bounded, with the format needed to re-send it.
- `omt`: the handshake, the placement, the overlay pass.
- Nothing changes on a terminal that does not answer, which is most of them.
