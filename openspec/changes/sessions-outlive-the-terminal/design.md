## Context

The terminal owns its ptys; the daemon owns nothing anybody uses. Both halves
work. This is the join, and the join is where the design decisions are.

## Goals / Non-Goals

**Goals.** Sessions outlive the terminal, with every surface the terminal has
today intact — splits, the panel, copy mode, the conversation view, the agent
observation, the notifications.

**Non-Goals.** A second renderer. A second emulator. A second way to reconstruct
a screen. Each of those would be a path that is exercised only in the case
nobody tests.

## Decisions

**The client keeps running an emulator; the daemon streams bytes.** The
alternative — the daemon renders and ships rows — would mean two renderers and a
protocol wide enough to carry everything a cell can be. omt already ships an
emulator, the client already runs one per pane, and the copy mode, the search
and the block model all read that emulator. Streaming bytes keeps all of it and
changes only where the bytes come from.

**Seeding is rendering the daemon's terminal back into escape sequences.** Then
"attach to a running session" and "here is what just happened" are the same kind
of thing, fed to the same parser. The alternative is a structured snapshot type
and a second way to apply it, which is the second path this design exists to
avoid. It costs a `Terminal → bytes` renderer, which is also what `omt attach`
should have been using all along.

**A gap is a re-seed, never a replay.** The bytes are commands, not a document.
Re-sending the tail of a shell command is how a retry becomes a disaster, which
is what the `ack` field in the frame header has always been for.

**The buffer is bounded and small.** It exists so a client that blinked can
catch up, not so a client that left can. A client that left is re-seeded, which
is correct and cheap.

**The daemon is started, not required.** A user who has to know about `omt
serve` to keep their agent alive has been handed omt's internals as a chore.
The terminal discovers, and starts one if there is none — the same ladder
`omt attach` already walks.

**One seam in the frame loop.** The loop asks for a pane's terminal, writes
input and pumps. Those become the seam; everything above them — the layout tree,
the renderer, the panel, the overlays — does not know the difference. That is
also the test: if a surface has to change, the seam is in the wrong place.

## Risks / Trade-offs

**A round trip per keystroke.** Local sockets make this microseconds, and the
input path is already asynchronous with the frame loop. Over ssh it is a real
cost, and it is the cost every remote multiplexer pays; predicted local echo is
what the `ack` field exists for and is not in this change.

**Two processes where there was one.** A crashed daemon takes every session with
it, where before a crashed terminal did. The daemon is the smaller and quieter
of the two, which is the direction that helps, and its lifecycle is now a thing
to get right — a stale socket is already handled by the discovery ladder.

**Migration.** The local path stays until the attached one is at parity,
because a half-migrated terminal is worse than either end of the migration.
