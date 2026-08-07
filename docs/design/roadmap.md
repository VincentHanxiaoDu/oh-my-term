# What omt has not built yet

Features omt has decided are worth having and has not built. Kept separate from
`docs/status.md`, which lists what exists against what the architecture already
specifies — this is the list of things the architecture does not yet mention.

Each entry says what it is, why it is worth doing, and what it would cost.
Nothing here is a promise; the list exists so the next decision is made against
a written trade-off rather than against whatever was most recently discussed.

## Worktree fan-out

Run one prompt across several agents at once, each in its own git worktree, and
compare the results.

Most of the machinery already exists: `WorkspaceId` is derived from a canonical
path, so N worktrees are already N workspaces with no special case. What is
missing is the fan-out itself, a view that compares the results, and a way to
take one.

**Why it is worth it.** It is the thing people already do by hand with three
terminal windows, badly. **What it costs**: a comparison UI, which is the
expensive half — everything else is capability calls omt already has.

## Human-initiated review

omt models the interactions an *agent* raises. It has nothing for a human
starting one: reading a diff, leaving a line comment, and sending that back as
context.

**Why it is worth it.** Reviewing what an agent did is most of the work in using
one, and it currently happens outside omt entirely. **What it costs**: a diff
view with anchors that survive the file changing underneath, which is more
subtle than it sounds.

## Agents as callers

The capability catalog already generates a CLI tree, so an agent could drive omt
the way a person does — list sessions, read a screen, answer a card.

What is missing is the decision, not the code: an agent invoking capabilities is
a caller with a credential, and the role that credential carries is the whole
question. **What it costs**: a role narrower than operator, and the discipline to
keep it narrow.

## Account and quota visibility

Which subscription a session is spending, and how much is left. Several agent
CLIs report rate limits over their own protocols, and omt currently drops that.

**Why it is worth it.** Running out mid-task with no warning is a specific,
common, avoidable annoyance. **What it costs**: little — the events already
arrive; nothing consumes them.

## Explicitly not planned

**An embedded browser.** Click an element on a page and send its markup and a
screenshot into the prompt. It is a genuinely good feature and it means shipping
a browser engine — which contradicts the one-binary bet omt made deliberately.
If that bet is ever revisited, this is the feature that would justify it.

**Issue tracker browsing in-app.** omt is a terminal. A pane running `gh` is
already this feature, without the maintenance.
