# Blocks that survive a shell without marks

## Why

`docs/design/terminal-ux.md` §10 ranks fifteen recommendations by value over effort.
Three of the top ones are unbuilt, and they share a cause: omt's block model
only exists when the shell cooperates.

**#3, `EarlyOutput`, is the one that matters.** A command that produces output
before its `OutputStart` mark — or a session where OSC 133 never arrives at all
— currently produces no block. That is not a cosmetic gap: bare `ssh`,
`docker exec`, `sudo` prompts and every background job land in exactly that
hole. the own note is that background blocks and the retroactive repair path
are what make those "not look broken", and omt is currently the version that
looks broken.

**#9, launch configs**, and **#11, the completer as syntax highlighter**, are
the other two. Both are self-contained and neither exists.

## What changes

- An **early block**: output arriving with no open block opens one anyway,
 marked as unattributed. When a `PromptStart` later arrives, the early block is
 repaired retroactively rather than left as a stray.
- **Launch configurations**: a recursive pane template, loadable from the
 config layer, so a workspace opens as a layout rather than one shell.
- **Command-line highlighting** derived from the same table that will drive
 completion, so the two cannot disagree about what a token is.

## What this deliberately does not do

the `sum_tree` for block heights (#5) is not adopted. It matters at the
scale where a UI scrolls a hundred thousand blocks; omt caps the tracker at two
thousand and a linear scan over that is not measurable. Adopting a data
structure for a problem that has been capped away is complexity with nothing
behind it.

In-band completion generators (#13) are also out: running a generator inside the
user's live shell is the only way to inherit their aliases and functions, and
that is a large enough surface to deserve its own change rather than riding
along with this one.
