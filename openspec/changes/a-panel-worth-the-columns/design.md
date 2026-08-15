## Context

19 §2.1 says "one row, never more than one row, in any configuration", and it
is right about rows. A row of chrome on a 24-row terminal is 4% of the screen,
permanently, and the document makes every cell justify its columns to earn it.

This adds chrome that takes *columns*, which is a different trade with a
different answer at different widths. On 170 columns, 24 of them is 14% and
buys the list this product exists to show. On 80 columns the same 24 leaves a
pane at 56, which is below what a shell prompt and a diff need, and the panel
has taken more than it gave.

So the rule is not relaxed — it is applied to a new axis, with the same test.

## Goals

- See which agent is waiting without pressing anything.
- Cost nothing when it cannot afford itself.
- One entry model, so the surfaces cannot disagree.

## Non-goals

- **Not a second row of chrome.** The status bar stays exactly as 19 §2.1
  specifies: one row, at the bottom, always.
- **Not on by default.** Somebody running omt in a 80×24 terminal must not have
  their panes narrowed by a change they did not ask for.
- **Not a tree.** Workspaces group rows; they do not fold. A fold is state to
  keep, a key to learn, and a place for a row to hide.

## Decisions

**Columns, not rows, and the width decides.** The panel takes a fixed 24
columns when it fits and nothing when it does not. Fixed rather than
proportional because a list of session names does not read better at 40 columns
and does read worse at 16.

**Refusing is silent, and reversible.** A panel that vanished because the
window was resized is not an error and does not warrant a message; widening the
window brings it back. What must not happen is a panel that stays and squeezes.

**The entry model is shared.** `omt-tui` computes what a row says — the mark,
the words, the order — from the instance, and the same computation answers the
web client. This is the half of "the two surfaces agree" that discipline cannot
provide: they already drifted once, and calling one state "blocked" on one
screen and "waiting on you" on another was found by a person, not a test.

**Grouped by workspace, ordered by attention within the whole list.** A blocked
agent in the third workspace is above an idle one in the first, because the
question the panel answers is "who needs me", not "what is in this project".

## What could go wrong

- **It steals a column somebody needed.** The width check is against the same
  minimum the split code uses, so a panel can never do what a split is refused
  for.
- **It repaints constantly.** The frame is gated on change; the panel's entries
  change when the roster does, which is the same signal that already forces a
  repaint.
- **It disagrees with the list behind the leader key.** Both come from the
  entry model. That is the point.

## Testing

The width rule and the ordering are pure functions and are tested as such. The
panel appearing, refusing and toggling are tested through omt's own emulator
against a real terminal, at a width where it fits and a width where it must
not.
