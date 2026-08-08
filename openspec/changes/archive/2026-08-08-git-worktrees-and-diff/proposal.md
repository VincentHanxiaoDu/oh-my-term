# Worktrees, and seeing what an agent changed

## Why

Running several agents on one problem is what people already do, badly, with
three terminal windows and a lot of stashing. A worktree per agent is the
correct shape — separate checkouts, one repository, no collisions — and omt is
unusually close to it: `WorkspaceId` is derived from a canonical path, so N
worktrees are already N workspaces with no special case anywhere.

Two things are missing, and both are missing in the same way as the usage
ledger was: **the model exists and nothing can reach it.**

- `omt-session/src/fanout.rs` has `Fanout`, `Arm` and `ArmState`, tested. No
  capability exposes it, and **nothing creates a worktree** — the struct holds a
  path that nobody makes.
- `omt-workspace-fs::hunks` returns the actual diff of a file, tested. `git.diff`
  reports *which* files changed and never what changed in them, so a phone can
  see that six files moved and not one line of any of them.

Reviewing what an agent did is most of the work in using one, and it currently
happens entirely outside omt.

## What changes

- `git.worktree.add` / `list` / `remove` — real worktrees, created and removed
  through git, each becoming a workspace by the id its path derives.
- `git.hunks` — the diff of one file, the content `git.diff` has always
  stopped short of.
- `fanout.start` / `status` / `choose` — one prompt, several agents, each in its
  own worktree, and a way to say which one won.

## What this deliberately does not do

**No merge.** Choosing an arm records the choice and leaves the branch alone.
Merging is a decision with conflicts in it, and a tool that merges on your
behalf when you tap "choose" is one you stop trusting the first time it picks
wrong. The chosen branch is named; `git merge` is one command away and the user
runs it.

**No commit, no push, no checkout of the user's main worktree.** `git.status`
and `git.diff` are read-only today and that stays true of everything here except
worktree creation, which is additive: `git worktree add` touches nothing that
already exists.

**No automatic cleanup.** Removing a worktree is explicit. An agent that
crashed leaves its worktree, and that is correct — the output is usually why you
wanted the worktree in the first place.
