## Why

The previous two changes were written from reading their *architecture*. This
one is written from reading their **source, file by file**, and checking every
user-facing surface in it against omt. The full checklist, with a verdict and a
reason per row, is [`docs/reference/read-from-the-others.md`](../../docs/reference/read-from-the-others.md).

Three things came out of it that omt should have, and one defect it found.

## What Changes

**An agent can report over the terminal, with no hook installed.** One of them
publishes structured lifecycle events as `OSC 777 ; notify` with JSON behind a
marker. It is the only structured source that requires *nothing* to be
installed — hooks need a file written into the agent's own configuration, a
transcript tailer needs to know where the file is, and this needs the agent to
have been started. omt reads it now, through the same per-agent normalizer a
hook goes through: one vocabulary, two doors. The tier differs permanently and
says so — `Marker`, not `Hook`, because these are bytes anything in that pane
could have written.

**A program in a pane can reach somebody who left the window.** `OSC 9` and
`OSC 777 ; notify` from *inside* a session become a `Notified` event and go out
through the terminal omt is drawing on. The long build that finishes while its
window is behind a browser is the case; a multiplexer that swallows these is
why people run their builds outside one.

**Quitting says what it is about to end.** They confirm before ending a session
with an agent mid-turn. omt did not — and looking for the equivalent found the
defect below.

## The defect

`⟨leader⟩ d` was labelled **detach**, which promises the session survives. omt
run as a terminal owns every pty in its own process, so the key ended all of
them. Somebody who read "detach and leave the session running" and lost an
agent mid-turn was told the wrong thing by omt, not by their agent.

The key now says `quit`, asks when anything is running, and the question is the
list of what will end with what each agent is doing. [05 §4.1b](../../docs/architecture/05-session-model.md)
records what it would take for a real detach — the terminal attaching to a
daemon that owns the ptys, which both exist and which the terminal does not use.

## Capabilities

### New Capabilities

- `osc-agent-reports`: reading an agent's own structured events off the pty.

### Modified Capabilities

- `attention`: notifications now travel inward as well as outward.
- `session-lifetime`: quitting is a question when it would end work.

## Impact

- `omt-term` gained an `osc` module; the emulator was over the hard 2000-line
  limit and the OSC surface is the widest thing in it.
- `TerminalEvent` gained `Notified`, which is additive on the wire.
- No competitor name enters the repository, here or anywhere else.
