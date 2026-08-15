## Context

The comparison so far was architectural: how they observe an agent, draw a
frame, order a list. That is where the defects were and it was the right place
to look.

It was not an inventory. Their keybinding configuration declares 47 actions —
the closest thing either codebase has to a machine-readable list of what the
product does — and omt answers 14. Most of the gap is not deliberate; it had
simply never been counted.

## Goals

- Close the gap where the missing action is a thing people do constantly.
- Record the deliberate gaps with the reason, so the count stays honest.

## Non-goals

- **Not tabs.** Their model is tabs in windows in workspaces; omt's is sessions
  in workspaces. Five of their actions are about tabs and have nowhere to go.
- **Not a second navigation axis.** They navigate workspaces with their own
  keys; omt reaches a workspace through the session list, and a second axis is
  a second thing to learn for the same journeys.
- **Not modes.** `resize_mode` exists to hold arrow keys. omt resizes through
  the layout tree by ratio, and a mode is a state somebody can be stuck in.

## Decisions

**Zoom is a view, not a rearrangement.** The tree is untouched; a zoomed pane
is drawn over the whole area and the arrangement returns exactly, ratios
included. Rearranging and restoring would be a second implementation of the
tree that has to agree with the first.

**Swap moves the pane and the focus follows it.** The alternative — focus stays
where it was, so the pane you just moved is no longer the one you are typing
in — is the behaviour every multiplexer that does it that way gets bug reports
about.

**Jumping to a waiting agent is one key, and says when there is nothing.** A
key that silently does nothing is one people press repeatedly. This is the
product's own subject, and needing a list and a choice to reach it is two steps
for the thing omt exists to make one.

**Renaming is a capability, not a TUI feature.** A roster of six things called
`shell` is what omt shows today; detection answers it for agents and a name
answers it for everything else. Both surfaces need it, so it is on the wire.

## What could go wrong

- **Zoom and split disagree.** Splitting while zoomed is the case that breaks
  in implementations that rearrange; here the tree is unchanged, so a split
  lands where it would have and zoom is simply dropped.
- **Swap loses a pane.** The tree operation exchanges two leaves and is tested
  for the invariant every tree test has: no overlap, nothing outside the area,
  and the same set of panes before and after.

## Testing

The tree operations are pure functions and are tested as such, including the
"same panes before and after" invariant. Zoom, swap and the jump are driven
through omt's own emulator against a real terminal.
