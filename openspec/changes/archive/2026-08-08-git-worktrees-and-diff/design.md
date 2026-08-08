# Design

## Worktrees are git's, not omt's

`git worktree add` is invoked rather than reimplemented. The alternative — omt
maintaining its own notion of a checkout — would be a second source of truth
about what is checked out where, and git's own `worktree list` is the one thing
that can never disagree with reality.

This is also why a worktree needs no new identity. `WorkspaceId` is derived from
a canonical path, so a worktree *is* a workspace the moment it exists. Nothing
in the session tree, the pane model or the capability surface needs a case for
it, which is the payoff of having derived that id in the first place.

## Removal refuses rather than asks

A worktree with uncommitted changes is not removed without `force`. Not a
prompt — omt has no place to ask from, and a capability that blocked on a
question would be unanswerable from a phone. It refuses and says why, and the
caller decides whether to pass `force`.

## Hunks are parsed, not rendered

`git.hunks` returns the added and removed lines with their positions. It does
not return a rendered diff: colouring and gutters are a surface's decision, and
a terminal, a phone and a web client each want a different one. The parser
already exists and is tested; this exposes it.

## Choosing is recording

`fanout.choose` writes down which arm won. It does not merge, and the proposal
says why: a tool that merges when you tap "choose" is one you stop trusting the
first time it picks wrong. What it does give you is the branch name, and
`git merge` is one command away.

The arm that lost keeps its worktree. That is deliberate — comparing two
attempts is most of the value, and deleting the loser at the moment of choosing
destroys the thing you would look at to check you chose right.
