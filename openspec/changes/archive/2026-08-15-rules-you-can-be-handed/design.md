## Context

Rules are already a file. What is missing is everything around it: writing one
means knowing what the agent draws, and having one means nobody else does.

The terminal this was read from fetches rule sets and caches them, with a
version and a minimum engine. The mechanism is sound for a product that ships
its own matcher against agents it chose. omt's situation differs in one way
that decides the design: a rule set is code-shaped data that runs against
whatever is on somebody's screen, including their work. Fetching it in the
background is not the same act as updating a library.

## Goals

- Make writing a correct rule possible without retyping a screen.
- Make handing one to somebody else possible.
- Make it impossible for a rule set to arrive without a person deciding.

## Non-goals

- **No auto-update.** Not on start, not on a timer, not "check once a week".
- **No registry.** A file and a path; if somebody wants to publish a set, a
  repository is a thing that already exists.
- **No content extraction, still.** Capture writes what is on the screen for a
  person to read; it does not become a source of text omt shows as an agent's
  words.

## Decisions

**Capture writes the regions, not just the screen.** The screen alone is what
somebody already has. What they cannot see is where `after_last_rule` starts,
which is the single thing that decides whether their rule fires on the question
or on the scrollback above it.

**Import shows a diff and waits.** Not a confirmation prompt with a default —
what changes is printed, and installing is a second command. A prompt with a
default is a prompt people answer without reading, and this one installs a
matcher.

**A URL is fetched once, by the import that was asked for.** Refusing URLs
entirely would push people to `curl | omt agent-rules import -`, which is worse:
it hides the source from omt, so nothing can record where a set came from.
Fetching it inside the command means the file records its origin.

**Every check is on the way in.** A set that has been installed is trusted to
the extent that a person installed it; the checks that matter are the ones
before that. In particular a rule with no test is refused, because it matches
every screen and reports its state forever — the one mistake whose output is
always confident.

## What could go wrong

- **Somebody imports a hostile set.** It can make omt report the wrong state.
  It cannot execute anything, read a file, or reach the network: the matcher
  has four operations and they all take a string and return a bool. That is the
  containment, and it is why the format has no escape hatch.
- **Capture leaks something private.** It writes the screen, which is what the
  person asked for, to a path they chose. It is not uploaded and omt never
  reads it back.
- **A trace disagrees with the live state.** The trace runs the same matcher
  over the same grid, so it cannot — unless the screen changed between the two,
  which the output says.

## Testing

The checks are pure functions over a parsed set and are tested as such,
including each way a set is refused. Capture and trace are tested against a
grid built in the test rather than a live agent, because what they must get
right is the region boundaries.
