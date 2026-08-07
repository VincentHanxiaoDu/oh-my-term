# Terminal UX: the decisions behind what omt shows

Why omt's terminal behaves the way it does. Each of these is a decision with a
failure behind it — the thing that goes wrong when a terminal does the obvious
thing instead.

## The rule above all of these

**Nothing here costs the user a step.** Blocks appear without configuration,
layouts have a default, highlighting is on. Every one of them is also a
configuration key, and every one of them has a plugin hook — but a person who
installs omt and types `omt` gets all of it without reading anything.

That is the whole balance: *fewer steps, not fewer settings*. A feature that
needs a setting before it does anything has been shipped switched off, and a
feature with no setting at all is somebody else's opinion permanently.

## Command blocks

A terminal is a stream of bytes. A *block* is one command, its output, and how
it ended. Everything people actually want — jump to the previous command, copy
just the output, re-run this, show me the last error — is trivial with blocks
and a heuristic over text without them.

omt builds blocks from OSC 133, the shell-integration marks that prompt
frameworks already emit. The parser reports the transitions; the tracker turns
them into blocks positioned in absolute rows, so a block survives its output
scrolling off the screen.

### Blocks without shell integration

**Output with no open block opens one.** That single rule covers every case that
would otherwise need separate machinery, because they are all the same case —
the mark did not arrive first:

- a shell that emits no OSC 133 at all
- `ssh` to a host whose shell has none
- `docker exec`, `sudo`, a login shell from 2009
- output that races its own mark
- a background job

Without this rule, every one of those produces an empty session. With it they
produce blocks marked `attributed: false`: readable, copyable, and never offered
for re-run, because omt never saw the command. Scraping the line above for
something that "looks like a prompt" is exactly the guess the tier ladder exists
to keep out of structured content, and a re-run button wired to a guess is what
it prevents.

This is also why blocks need no setup. A user who has never heard of OSC 133
still gets them; a user whose prompt emits the marks gets command text and exit
codes on top. The feature degrades rather than switching off.

Repair happens on the **next** prompt mark rather than by scanning backwards. A
backwards scan needs a guess about where the previous command ended, and a guess
there lands a block boundary in the middle of somebody's output.

### Exit codes 130 and 141 are not failures

130 is Ctrl-C — the user meant that. 141 is SIGPIPE, which is what `… | head`
does to everything upstream of it every single time. A terminal that paints
those red teaches people that red means nothing, and then the one command that
really failed looks like the other forty.

A shell that reports `133;D` with no status at all is `Unknown`, not success.
Painting that green would be inventing the good news.

## Layouts

A workspace opens as a set of panes rather than one shell. The configuration is
a **flat list**, not a split tree: omt's geometry tiles a flat list, so a tree
would be a shape the renderer flattens immediately. It grows a tree when the
renderer does, and not before — a schema describing something nothing can draw
is a promise to nobody.

Opening one reports the outcome of **every** pane. A layout that half-opened and
said "error" leaves somebody to work out which of six is missing. A pane also
cannot point outside its workspace: a layout file is something people share, and
a shared file that opens a shell in somebody's home directory is a different
kind of thing from a layout.

## Command-line highlighting

Commands green, subcommands blue, flags yellow, arguments plain, variables and
operators their own. It is a classifier and deliberately **not** a shell parser:
reimplementing shell grammar to decide what colour a token is would be a great
deal of code to get subtly wrong, and subtly wrong colour is worse than coarse
colour.

It does track quoting, for one reason: a `--flag` inside a string highlighted as
a flag is the error people notice within a second. And it is the same table that
will drive completion — two tables would drift, and the drift shows up as a
token highlighted as a command that the completer does not believe is one.

## What omt deliberately does not do

**No tree of block heights.** A balanced-sum-tree index over block positions
earns its complexity when a UI scrolls a hundred thousand blocks. omt caps the
tracker at two thousand, and a linear scan over two thousand is not measurable.
A data structure for a problem that has been capped away is complexity with
nothing behind it.

**No completion generators inside the user's shell.** Running a generator in the
live shell is the only way to inherit somebody's aliases and functions, and that
is a large enough surface — and a sharp enough one — to deserve its own design
rather than riding along with anything else.

Both of these are the same judgement: a feature nobody can point at a failure
for is a feature that has not earned its place yet. They are listed rather than
silently absent so the next person knows they were considered.

## Where these came from

Every decision here was checked against terminals that already ship: what they
do, where the format is documented, and what breaks when it is done the obvious
way. What omt took is the *behaviour*; implementations are somebody else's and
stay that way. Interfaces — escape sequences, file formats, configuration
schemas — are reimplemented from their public documentation, which is what
interfaces are for.
