# What omt has not built yet

Features omt has decided are worth having and has not built. Kept separate from
`docs/status.md`, which lists what exists against what the architecture already
specifies — this is the list of things the architecture does not yet mention.

Each entry says what it is, why it is worth doing, and what it would cost.
Nothing here is a promise; the list exists so the next decision is made against
a written trade-off rather than against whatever was most recently discussed.

## ~~Worktree fan-out~~ — built, except the comparison view

`worktree.add` / `list` / `remove` make real git worktrees, each a workspace by
the id its path derives. `fanout.start` gives one prompt an agent per worktree
and reports every arm including the ones that could not be prepared;
`fanout.choose` records which won.

Choosing **merges nothing**. A tool that merges when you tap "choose" is one you
stop trusting the first time it picks wrong — it names the branch and `git
merge` is one command away. The losing worktrees are kept, because comparing
them is what you would do to check the choice was right.

What is still missing is the expensive half: a *view* that puts two attempts
side by side. `git.hunks` supplies the content it would need.

## Human-initiated review — half built

`git.hunks` reports the changed lines of a file, which is what reviewing an
agent's work needs and what `git.diff` always stopped short of. Returned as
lines rather than rendered: a terminal, a phone and a browser each want to draw
a diff differently.

What is missing is the round trip — leaving a comment on a line and sending it
back as context. **What it costs**: anchors that survive the file changing
underneath, which is more subtle than it sounds and is the whole difficulty.

## Agents as callers

The capability catalog already generates a CLI tree, so an agent could drive omt
the way a person does — list sessions, read a screen, answer a card.

What is missing is the decision, not the code: an agent invoking capabilities is
a caller with a credential, and the role that credential carries is the whole
question. **What it costs**: a role narrower than operator, and the discipline to
keep it narrow.

## ~~Account and quota visibility~~ — built

`usage.report` reports tokens per session and in total, with whatever the agent
said about its rate limit. Cost appears only when the agent stated one: a price
table in omt would go stale the week any provider changed pricing, and the
estimate would sit beside counts that are right with nothing to tell them apart.

What is deliberately still absent is a *policy* — no threshold, no budget, no
cutoff. `Headroom::Unknown` stays distinct from a low number so a surface can
decline to draw a bar rather than drawing an empty one, and what to do about a
low number belongs in a configuration key or a plugin.

## Explicitly not planned

**An embedded browser.** Click an element on a page and send its markup and a
screenshot into the prompt. It is a genuinely good feature and it means shipping
a browser engine — which contradicts the one-binary bet omt made deliberately.
If that bet is ever revisited, this is the feature that would justify it.

**Issue tracker browsing in-app.** omt is a terminal. A pane running `gh` is
already this feature, without the maintenance.
