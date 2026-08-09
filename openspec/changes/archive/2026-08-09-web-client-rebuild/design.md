# Design

## What is kept

The store, the protocol layer, `terminal.ts`, `touch.ts`, `screen.ts` and
`push.ts` stay as they are. They hold every rule that matters — exactly-once
answering, the intent id on commands, reflow, inverse-video swapping, the
hold-to-confirm threshold — and they are tested without a DOM. The rebuild is
above them.

That line is why this is a rebuild of the *client* and not of the client
*library*: 149 tests do not move.

## One state, not four overlapping ones

A screen renders `Loading | Empty | Error | Content`. Not flags that can all be
true — a sum type, so "loading and empty" cannot be expressed. The screenshot
that started this showed three states at once precisely because they were
independent pieces of DOM appended in sequence.

The header's count comes from the same value the body renders, for the same
reason: two counts derived independently will disagree, and the user has no way
to know which to believe.

## Navigation is a stack over history

`history.pushState` per screen, and `popstate` pops. The phone's back gesture is
the one navigation control every user already has, and a web app that ignores it
throws them out to the previous site instead of the previous screen.

The back affordance is drawn as well, because a gesture with no visible
counterpart is one people do not discover on a page that does not look like a
web page.

## Errors are per-screen and transient

An error lives on the screen whose call failed and is replaced when that call
succeeds. The current client pushes every failure onto one global list that only
grows — which is why a session id that could not be found was still on screen
under an unrelated roster.

## Tokens, not a framework

CSS custom properties for the type scale, spacing, radii, colour roles and
elevation; a handful of composable classes over them. No framework: the store
and renderer are the hard parts and already exist, and a diffing algorithm for a
list of six things is not worth a dependency whose upgrade cadence outlives the
code it renders.

Dark and light both, driven by `prefers-color-scheme`, because a terminal client
that is bright white at 2 a.m. is one people close.
