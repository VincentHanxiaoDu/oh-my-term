## Why

The comparison so far was written from reading their *architecture* — how they
observe an agent, how they draw a frame, how they order a list. That was the
right place to start and it is where the defects were.

It is not a feature inventory. Their keybinding configuration declares **47
actions**, which is the closest thing either codebase has to a list of what the
product does, and omt answers **14** of them. Some of the missing ones are
deliberate; most are not, and nobody had counted.

This is that count, and the work it implies.

## What Changes

Their 47 actions, against omt:

**Present** (14): detach, new session, split (two directions), focus by
direction, cycle pane, close pane, help, palette, sessions, agents, panel
toggle, resize (terminal-driven).

**Missing, and worth having** — the work of this change:

- **zoom** — one pane fills the screen and comes back. The most-used key in
  every multiplexer, and omt has no answer at all.
- **last_pane** — back to the one before, which is how somebody moves between
  two panes without thinking about direction.
- **swap_pane** in four directions — moving a pane rather than the focus.
- **rename_pane / rename_session** — a roster of things called `shell` is what
  omt shows today, and the fix is letting somebody say what a thing is.
- **next_agent / previous_agent** — cycle straight to what is waiting, which is
  the product's whole subject and currently needs a list and a choice.
- **copy_mode** — read the scrollback, select, copy. omt has scrollback and no
  way to look at it.
- **goto** — a session by number, which is what everybody's fingers already do.

**Missing, deliberately not built**, recorded so the count is honest:

- **tabs** — omt's model is sessions in workspaces, not tabs in windows.
  Their `new_tab`, `next_tab`, `previous_tab`, `close_tab`, `rename_tab` have no
  place to go here.
- **workspaces as a navigation axis** — `next_workspace`, `workspace_picker`,
  `new_workspace`, and the worktree keys. omt reaches a workspace through the
  session list; a second axis is a second thing to learn for the same journeys.
- **resize_mode** — a mode for dragging dividers. omt resizes by ratio through
  the layout tree; a mode that exists to hold arrow keys is a mode.
- **reload_config, settings, plugin actions** — omt has capabilities for these
  and reaches them through the palette rather than through chords.
- **edit_scrollback** — opening the scrollback in `$EDITOR`. Worth having, and
  it needs copy mode first.
- **open_notification_target** — their notifications carry a target; omt's
  notifications are 20's and are a different shape.

## Capabilities

### New Capabilities

- `pane-control`: zoom, swap, last, and naming.

### Modified Capabilities

_None._

## Impact

- `crates/omt-tui` — the layout tree gains zoom and swap; the key table grows
- `crates/omt/src/run.rs` — the loop learns the new inputs
- `crates/omt/src/capabilities/` — renaming a session is a capability, so both
  surfaces get it
- A copy mode is large enough to be its own change and is proposed separately
