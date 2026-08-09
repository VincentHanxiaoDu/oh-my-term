# The web client, rebuilt

## Why

The first real use of the web client on a phone produced two screenshots. They
show more than "it is ugly".

**Three contradictory statements on one screen.** The header said `1 running`,
the body said `nothing running`, and beneath both sat a red `no session
sess_7ed…`. Each is produced by different code that never asks what the others
are saying: the header counts sessions, the body counts *agent threads*, and the
error is a failed capability call rendered as a permanent problem. A user cannot
act on any of them, and one of the three is always wrong.

**Tapping a shell goes to the wrong screen.** The roster routes every tap to the
subagent grid, which is empty for a plain shell — so the one thing somebody
opens a terminal for is two taps away and neither tap says so.

**No way back.** There is no navigation. Once on a session screen, the only
route to the roster is the browser's back button, which the client does not
handle either.

And it is ugly, which is not a separate problem: a `<ul>` with a bullet beside
a `<button>` is what the DOM produces when nobody decided what the thing should
look like. There is no type scale, no spacing scale, no elevation, no states —
seventeen CSS rules in a `<style>` block.

## What changes

A rebuild of the client's presentation and navigation. The protocol layer,
store, terminal renderer and touch rules stay: they are tested and they are
right. What is replaced is everything above them.

- **A design system**: type scale, spacing scale, colour roles, elevation and
  motion as tokens, so a screen is composed rather than hand-styled.
- **Real navigation**: a stack with a back affordance and browser history, so
  the phone's own back gesture works.
- **One state per screen.** Loading, empty, error and content are four states of
  one component, and only one shows. A screen never says two things at once.
- **The right screen on tap**: a shell opens its terminal, an agent session
  opens its threads. Decided from what the session *is*, not from one route for
  everything.
- **Errors that belong to what caused them**, and clear. A failed call shows on
  the thing that failed and disappears when it succeeds — it does not stack up
  as permanent red text under unrelated content.

## What this deliberately does not do

**No framework.** The store and the renderer are already the hard parts and are
tested without a DOM. Adding React here buys a diffing algorithm for screens
that redraw a list of six things, and costs a dependency whose upgrade cadence
outlives the code it renders.

**No visual redesign of the terminal itself.** It renders the user's own theme
and their program's own colours. Restyling that would be omt deciding what
somebody's shell looks like.
