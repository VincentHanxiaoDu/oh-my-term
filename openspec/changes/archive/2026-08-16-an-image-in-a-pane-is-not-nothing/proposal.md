# An image in a pane is not nothing

## Why

A program that draws an image inside omt — `imgcat`, a plotting tool, an agent
showing a screenshot it just took — produces nothing at all. Not a placeholder,
not a warning, not a gap: the bytes are consumed and the following output lands
exactly where the image should have been, so the screen looks like the program
never ran.

Two layers cause it. `vte` consumes an APC string in a parser state that makes
no callback, so kitty's graphics sequences are **invisible to omt**, not merely
unhandled. And nothing reserves cells for an image, so even a recognised one
would be overwritten by the next line.

Reading the two comparable products was worth doing before building:

- Neither implements **sixel**, so omt's absence there is not a gap against
  them and this change does not chase it.
- One of them **silently drops** the iTerm2 inline-image sequence — it parses
  the key and logs it as unimplemented.
- The multiplexer among them does not rasterize anything: it keeps placements
  per pane and re-emits kitty's protocol to the host terminal, clipped to the
  pane's rectangle and diffed every frame. That machine is real work — host
  cell size, cropping, image ids, deletion — and in their build it is
  experimental and **off by default**.

So the first thing to build is not the re-emitting machine. It is the layer
underneath it that has to be right either way: seeing the image, knowing how
much room it takes, and never showing nothing.

## What changes

- APC is read before the parser can swallow it, and kitty's graphics keys are
  understood well enough to know an image arrived and how large it is.
- The iTerm2 inline-image sequence is read from the OSC omt already receives,
  including its multipart form, so a large image is not buffered whole.
- Chunked transmission is accumulated under a bound, and a transfer that
  exceeds it is dropped as a whole rather than rendered from a piece of itself.
- An image reserves the cells it occupies, so the text after it does not land
  on top of it.
- Those cells show a card naming what arrived and how to open it. Never
  nothing, which is the rule that made this worth doing.

## Impact

- `omt-term`: an APC scanner ahead of the parser, image records, reserved
  cells.
- `omt-tui`: the card.
- No capability, no wire change. Rendering the image itself — the host
  re-emission — is deliberately not in this change, and is recorded as the
  next step rather than implied by it.
