# Design

## Seeing APC at all

`vte` routes an APC string into `SosPmApcString` and calls nothing. So the
bytes have to be taken before the parser sees them: `omt-term` scans the input
for `ESC _` and holds everything up to `ESC \` or `BEL` itself, feeding the
parser what remains.

This is a scanner, not a second parser. It has exactly one job — find the
extent of an APC string — and it has to survive the sequence arriving in
pieces, because a pty read boundary falls anywhere. State is a small enum
across `advance` calls, the same shape the OSC path already needs.

Rejected: forking `vte`. The scan is thirty lines and a fork is a permanent
tax on every future upgrade of a crate whose whole value is that it is boring
and already correct.

## What is read out of it

Only what is needed to know an image arrived and how much room it takes:
`i` (id), `a` (action), `m` (more chunks), `f`/`s`/`v` (format and pixel size),
`c`/`r` (size in cells). Everything else is carried as-is and not interpreted,
because a key omt does not model is not a key omt should refuse.

Cells: `c`/`r` where given, otherwise derived from pixel size and the cell size
the pane knows, and otherwise a single row — an image whose height cannot be
worked out still gets somewhere to say so.

## Chunks, and their bound

`m=1` accumulates. The bound is on the *accumulated* transmission, and passing
it drops the whole thing: rendering an image from a prefix of its own bytes is
a wrong image, and a wrong image is worse than an absent one. The iTerm2
multipart form (`MultipartFile=` / `FilePart=` / `FileEnd`) accumulates through
the same buffer with the same bound, since it is the same problem twice.

An unfinished transmission is dropped when the pane produces something else
that ends it, rather than being held forever.

## Reserving the cells

The image is recorded against the row it started on and the number of rows it
covers, and the cursor moves past those rows exactly as a program printing that
many lines would move it. That is what makes the following output land below
the image and what makes it scroll normally — the rows are ordinary rows, and
the image is a property of them.

Kitty's `C=1` ("do not move the cursor") is honoured, because a program that
asked for that is drawing something it means to write over.

## The card

`GraphicsProtocol::None` in [09 §7.2](../../../docs/architecture/09-ssh-and-media.md)
already specifies it: a bordered card with what the thing is and how to open
it. Drawing the image for real means asking the *outer* terminal to do it,
clipped to the pane's rectangle and diffed per frame — a machine worth building
once this layer is right, and not before.
