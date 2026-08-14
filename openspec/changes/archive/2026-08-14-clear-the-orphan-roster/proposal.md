## Why

A restored session is an orphan by design: a pty does not survive the daemon
that opened it, so what comes back is readable and searchable and refuses
writes. 05's session model says the surface "offers to restart it". No surface
does. So orphans accumulate across every restart, and on a machine that has run
omt for a week the roster's first screen is a column of identical dead shells —
sixteen of them on this one — with no way to restart one, close one, or tell
them apart.

That is not only ugly. It broke the end-to-end suite: the tests open the first
session in the roster, the first session in the roster is a corpse, and a test
about the terminal was typing into something with nothing on the other end. The
suite passed for months only because whoever ran it had leftover sessions that
happened to be alive.

## What Changes

- An orphaned session offers **restart** and **close** on the surface that
  shows it — the roster row and the session's own screen — using
  `session.restart` and `session.close`, which both already exist and are
  reachable from nowhere on the phone.
- The roster **groups** orphans below live sessions rather than interleaving
  them, and says how many there are, so a live session is never buried under a
  fortnight of dead ones.
- Closing all of them is offered **once, as one action**, and states the count
  it will close. Sixteen confirmations for sixteen corpses is a control nobody
  uses; one unlabelled "clear" is one somebody presses by accident.
- The TUI needs nothing. Its status bar lists this process's panes rather than
  the instance's sessions, so an orphan cannot appear there; the session
  chooser already prints each session's state, and `omt session close` and
  `omt session restart` work from the CLI. This was wrong in the first draft
  of this proposal and is left here rather than quietly deleted.
- **Not** automatic pruning on restore. What survives a restart is scrollback
  somebody may want, and deciding on their behalf that yesterday's output is
  worthless is the one thing this must not do.

## Capabilities

### New Capabilities

_None._ `session.restart` and `session.close` exist and are declared; this
change is about their being reachable.

### Modified Capabilities

- `web-client`: three requirements are **added** to it — the per-session
  restart and close, the roster not burying a live session under dead ones,
  and clearing every orphan as one action that names its cost. Added rather
  than modified: none of the existing requirements changes, because the client
  had nothing to say about an orphan at all.

## Impact

- `web/src/app.ts` — the roster's rows and the session screen's controls
- `web/src/federation.ts` — the roster subtitle, which already counts what is
  running and now separates what is not
- `web/e2e/surfaces.spec.ts` — the helper that opens "a session" must open a
  live one, which is the defect that exposed all of this
- No capability declarations change, so the parity gate and the schema snapshot
  are untouched.
