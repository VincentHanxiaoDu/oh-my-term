# The window shrinks, and the pane stays

## Why

"Change your resolution" was reported broken, and the pty suite had never
resized a terminal — nothing tested what omt does when the window changes.

Two real defects surfaced the moment it could:

1. **omt never saw the window change.** `SIGWINCH` goes to the *foreground*
   process group of the pty, which is the shell in the pane. omt is not in
   it, so the layout kept the old geometry while the window moved under it —
   a 60-column window was drawn at 100 columns, with the second pane painted
   where nobody could see it.
2. **A shrink collapsed the split.** `MIN_PANE_COLS` was 40 — the width at
   which `git log --oneline` *looks* good, used as the floor for a pane
   *existing*. A 100-column window split into two 50s fine; shrink the
   window to 60 and `divide` refused, one pane took the whole screen and the
   other got a zero rect — which was still painted, writing its stale grid
   over its neighbour's live one. Both prompts vanished.

The two comparable products were read for how they do this: both poll the
terminal size rather than waiting for the signal only the foreground group
gets, both clamp to a *usable* minimum (two columns / 2×4, not forty), and
both coalesce per-frame resize bursts. The first two are adopted here
directly; the third is unnecessary at omt's frame rate, where a size read
is one ioctl.

## What changes

- The frame loop reads the terminal size each round and reflows when it
  changed — the signal remains, but no longer the only channel.
- `MIN_PANE_COLS` falls from 40 to 12, with the reasoning recorded: a pane
  exists while a prompt fits in it; whether it is *good* is a different
  question. The layout tests that pinned the old number are updated to the
  new floor, and the shrink cases are what they assert now.
- A pane whose rect came out zero is not painted: its grid still has real
  content, and painting it where its neighbour lives overwrites a live
  screen.
- The pty harness can resize, and the suite covers it four ways: grow a
  local terminal, grow an attached one, shrink a single pane, and shrink a
  split — the last one being the reported bug, green because the second
  pane now survives the shrink instead of vanishing.

## Non-goals

- No debounce/coalescing layer: at TICK a "resize storm" is one ioctl per
  round, and the layout is a pure reflow of stored ratios (the shape the two
comparable products both use),
  so repeated events are idempotent and cheap.
- The agent notification and hook tests that flaked under load got longer
  wait ceilings, not weaker assertions: they wait for the same thing, just
  longer.

## Impact

- `omt`: the size poll and the zero-rect skip.
- `omt-tui`: the floor, and the tests that matched it.
- `omt-pty`, the harness: resize and the child pid, both for tests.
- The two products' resize machinery was read; the mechanism notes follow
  the repository's rule of naming nothing readable was borrowed from.
