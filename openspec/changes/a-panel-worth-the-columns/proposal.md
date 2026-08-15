## Why

omt shows what is running only when you ask it to. Press the leader and `a`,
read the list, dismiss it. Everything omt knows about four agents in three
workspaces — which one is waiting for a person, which finished while you were
elsewhere — is behind a keystroke you have to think of pressing.

That is the wrong shape for the thing this product is *for*. The whole premise
is that agents work while you do something else, and the moment worth catching
is the one where an agent stops and waits. A modal list catches it only if you
happen to open the list.

19 §2.1 forbids more than one row of chrome, "in any configuration". Its
reasoning is sound and this change keeps almost all of it: every cell must
justify its columns, and a cell that cannot does not ship. But the rule was
written about a **status bar** — a horizontal strip competing with the pane for
rows, where each row costs 4% of a 24-row screen. A column is not a row. On a
170-column terminal, twenty-four columns is 14% of the width and buys a list of
every agent on the machine; on an 80-column one it buys nothing worth having,
and the panel should not be there.

The reference for this is a terminal that gives its agent panel 2848 lines of
code and shares the same entry model with its phone layout — which is also the
answer to "the two surfaces should agree", because it makes them agree by
construction rather than by discipline.

## What Changes

- **19 §2.1 is amended**: one row of chrome *below* the panes stays absolute.
  An optional vertical panel is added, off by default, with its own
  justification test — a panel must earn its columns the way each status cell
  earns its own.
- **A panel listing every session and its agent**, ordered by what each row
  wants, marked by shape, grouped by workspace.
- **It is off by default and refuses to appear** below a width where the panes
  would suffer. A panel that squeezes a shell to 56 columns has taken more than
  it gave.
- **One entry model**, computed once, rendered by the TUI panel and available
  to the web client — so the two surfaces cannot drift into describing the same
  agent differently, which they already did once.
- **Toggled with a key and remembered**, because a panel somebody has to
  re-open every session is one they stop using.

## Capabilities

### New Capabilities

- `agent-panel`: what the panel shows, when it is allowed to appear, and what
  it does when there is no room.

### Modified Capabilities

_None._ No capability declaration changes; this is a surface.

## Impact

- `docs/architecture/19-*.md` §2.1 — the amendment, with its own justification
  test
- `crates/omt-tui` — the panel, and the entry model behind it
- `crates/omt/src/run.rs` — the layout gains a column, and a key toggles it
- `crates/omt-config` — one setting, off by default
- The web client renders the same entries in its own shape
