## Context

Four surfaces that were specified, declared, bound and documented, and connected
to nothing. The common shape: each half had tests, and the seam between them had
none, so the suite was green while the product did nothing.

## Goals / Non-Goals

**Goals.** Close each seam, with a test that crosses it — a real hook binary
against a real socket, a real terminal receiving real bytes.

**Non-Goals.** Answering an agent's card from another surface when the agent is
on a pty. Selecting an option there means moving a highlight omt cannot see, and
a surface that offered it would be guessing.

## Decisions

**A hook is answered before dispatch, without a handshake.** It reports an
observation; it is not an actor asking for a mutation, and the authorization
that matters happened at the socket where the kernel recorded the peer. Greeting
would spend two round trips of an agent's deadline negotiating nothing.

**A terminal serves but does not own.** It binds a socket so its agents have
somewhere to report and so `omt ls` in another window sees these sessions. It
runs no pump, no restore and no autosave, because its own frame loop already
does — a second of either is a process racing itself.

**Guessing an attribution is worse than dropping an observation.** A hook that
cannot name its session, or names one this instance never had, is dropped.

**A card raised from a hook is not answerable from elsewhere on a pty**, and
says why. The alternative is somebody pressing a button, believing the agent
heard, and finding out when the wrong option was taken.

**The decision of when to notify is the feature.** A tool that buzzes for
everything is muted, and then it does not buzz for the thing that mattered. It
lives in its own module with its own tests; the escape sequence is plumbing.

**Copy mode's coordinates are absolute rows.** Screen-relative positions move
under a reader as output arrives, so a selection would cover different text a
second later than the one that was highlighted.

**Resume is a query.** It says what would be run and does not run it: which
conversation is resumed is whatever that id names, and that is a decision with a
cost.

## Risks / Trade-offs

**The frame loop holds the instance for a whole iteration**, so a hook waits at
most one tick. That is inside a hook's deadline by two orders of magnitude. It
also means a second acquisition on the same thread deadlocks the terminal with
no error anywhere — which happened, and is why the row builders take the
instance the caller already holds rather than locking again.

**Recognized-or-silent for notifications** means a terminal omt has not been
taught about gets nothing. The alternative is drawing escape text into somebody's
work, and there is no query to distinguish the two cases.
