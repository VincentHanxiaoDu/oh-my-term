## Context

Two mature terminals observe third-party agent CLIs. They were read for this
change. They disagree about almost everything and agree about the one thing
that matters: **the agent already tells the terminal what it is doing, and the
place it tells it is the OSC channel.**

One defines its own notification protocol — a sentinel URL, a versioned JSON
payload with the agent, the event, the session, the cwd, the tool being called
— and ships a plugin for the agent to emit it, advertising the host's support
through environment variables. That is a cooperating protocol, and omt's own
documents already describe that tier: it is the hook and protocol level, worth
having and only available where the agent cooperates.

The other requires no cooperation at all. It reads the OSC title and progress
the agent already sets, and below that matches the pane's own grid against
per-agent rule sets — priorities, combinators, and *named regions* rather than
a search over the whole screen. The rules are versioned data, so an agent
changing its interface is a data update rather than a release.

omt has the top of that ladder (hooks, protocols) and the bottom (a heuristic
floor watching for output and bells). It has nothing in between, and the
middle is where every agent on a plain pty lives.

## Goals

- Identify the agent and its state without permissions, cooperation, or luck.
- Keep every reading's provenance, so a wrong answer can be traced to what was
  matched rather than argued about.
- Make it impossible for the test suite to pass because of the environment it
  happens to run in.

## Non-goals

- **Not a new protocol for agents to adopt.** omt cannot make an agent emit
  anything. The hook and protocol tiers already cover the cooperating case.
- **Not screen scraping for content.** Regions decide *state*, never what the
  agent said. What an agent said comes from its own stream, or not at all.
- **Not a rewrite of the renderer, here.** The single-buffer frame and the
  agent panel are the second half and get their own change, so the fix for a
  reported bug does not wait behind an architecture change.

## Decisions

**The OSC title is a marker, not a heuristic.** The tier ladder is
weakest-first: `Heuristic < Process < Marker < Transcript < Hook < Protocol`.
A title an agent sets deliberately is stronger than a process name — a process
name says what was started, a title says what it is doing right now — and
weaker than a hook, which is the agent reporting through an interface built for
reporting. `Marker` is the right rung and it already exists.

**Regions, because a screen is not a string.** "Does this screen contain the
word yes" is true of a diff, of scrollback, of a file the agent is displaying.
"Does the text after the last horizontal rule contain *enter to select*" is
true when an agent is asking a question. The region is the whole engineering.

**Rules are data.** An agent's interface changes on its own schedule, and a
release of omt is a bad unit of response. Data also makes a wrong rule a thing
somebody can read, quote and correct without building anything.

**Done is a state.** An agent that finished while its pane was off screen is
the single most valuable thing this product knows. Both surfaces get it.

**The probe reports what it read.** Reading a process's arguments needs
permission that a hardened binary does not grant; reading its name does not.
Returning nothing because the second half failed threw away the first half.

## What could go wrong

- **A title that lies.** A shell that sets its title to the last command run
  would name an agent that has exited. Titles are matched against per-agent
  patterns, not searched for a substring, and the process probe disagreeing is
  a higher tier saying otherwise.
- **Rules that fire on a transcript.** An agent showing its own history puts
  yesterday's question back on the screen. The rule set has an explicit
  suppression for that, which is why the region model needs one.
- **Cost.** Matching runs against a bounded region on a timer, not per byte.

## Testing

The harness that exists cannot test this: it inherits an agent environment and
therefore passes for the wrong reason. First it is fixed, then the tests that
passed are shown to fail, then the fix is made and they pass again for the
right reason. Anything else is the same mistake with more steps.
