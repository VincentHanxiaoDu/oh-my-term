# Design

## Where the colour comes from

`omt-theme` already holds a background, a foreground and sixteen colours, and
the web client already renders from it. Deriving the furniture from the same
theme is what keeps a session looking like itself in a browser and in a
terminal.

The steps are small on purpose. A surface far from the background reads as a
separate window rather than as part of this one — which is precisely what a
full-width reverse-video bar looked like.

## Degrading

Three depths: truecolor when `COLORTERM` says so, the 256-colour cube
otherwise, and nothing at all for `TERM=dumb`, an unset `TERM`, or anybody who
set `NO_COLOR`. The last case emits exactly the reverse video that was there
before, so the terminal that could not take colour loses nothing.

Near-greys are matched against the 256-colour **grey ramp** rather than the
cube. omt's surfaces are a shade apart, and the cube's greys are coarse enough
that two of them land on the same square — which would make the surface vanish
into the background on precisely the terminals that need it most.

## Panel lines say what they are

The renderer returns each line with its kind — edge, filter, choice, picked,
note — rather than the painter sniffing the text. A painter that decided "this
is the selected row" by looking for a marker character would colour a row whose
*label* happened to contain it.

## Why omt left

The session records how its program ended and has since the session model
existed. The exit path reads it, and an exit status above 128 is reported as
the signal it stands for, because "stopped by signal 2" is a different sentence
from "exited with 130" to everybody except a shell.

**Ordering is the whole point.** The guard is dropped — mouse reporting off,
cursor back, alternate screen given back — and only then is anything printed.
A message written before that lands on a screen that is about to be discarded,
which is exactly how the original failure managed to be invisible.

## A note for the times there is no session to ask

A panic hook that puts the terminal back and writes to a fixed path. The
evidence otherwise destroys itself: the message goes to stderr, stderr is on
the alternate screen, and leaving the alternate screen throws it away.
