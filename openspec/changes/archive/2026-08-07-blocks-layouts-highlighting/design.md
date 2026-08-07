# Design

## Early blocks: open on output, repair on mark

The tracker gains one rule: output with no open block opens one. That single
rule covers every case the `EarlyOutput` covers, because they are all the
same case — the mark did not arrive, or did not arrive first.

Repair happens on the *next* `PromptStart` rather than by scanning backwards.
A backwards scan needs a heuristic for where the previous command ended, and a
heuristic there produces a block boundary in the middle of somebody's output.
The forward rule needs none: when a prompt appears, whatever was open ends.

An early block carries `command: ""` and an explicit flag rather than a guessed
command. Scraping the line above for what "looks like" a prompt is exactly the
heuristic the tier ladder exists to keep out of structured content, and a
re-run button wired to a guess is the failure mode.

## Launch configs: the schema, not the implementation

the `PaneBranchTemplate` is a recursive enum — a leaf with a command, or a
split with a direction and children. omt adopts the shape and not the code:
document formats are interfaces, and the implementation is AGPL.

Opening one is a sequence of `session.create` and `pane.open` calls, which is
why it needs no new session machinery. What it does need is to report *which*
pane failed: a layout that half-opened and said "error" leaves the user to work
out which of six panes is missing.

## Highlighting: one table, two consumers

The classifier returns spans with a class. It is deliberately not a parser —
shell grammar is not worth reimplementing for colour — but it does track quoting,
because a flag inside a string highlighted as a flag is the one error people
notice immediately.

The same table drives completion later. Two tables would drift, and the drift
would show as a token highlighted as a command that the completer does not
believe is one.
