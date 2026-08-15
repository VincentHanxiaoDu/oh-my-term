## Context

Read their source file by file rather than their architecture. The full
checklist is in `docs/reference/read-from-the-others.md`; this is the design for
the three rows that turned into work.

## Goals / Non-Goals

**Goals.** Take what is genuinely better and say where omt stays different on
purpose.

**Non-Goals.** Copying code, copying their protocol identifiers, or naming them
anywhere in this repository. What is taken is a question and, once, a public
escape sequence.

## Decisions

**One vocabulary, two doors.** An agent's report over the pty carries the same
event name and the same tool call a hook carries, so it goes through the same
per-agent normalizer. Two parsers for one message is two things to keep in step,
and the one that is used less is the one that rots.

**The tier is the difference, and it is permanent.** A hook arrives on a socket
whose peer credentials the kernel recorded. A pty report is bytes; a shell
script in that pane can write them. `Marker` — the vocabulary's own word for an
OSC backchannel — is the honest rung: something put this here deliberately, and
that is all it proves.

**Which agent never comes from the message.** It comes from observation, or the
report is dropped. A message that could name its own agent could name somebody
else's.

**Advertised, not negotiated.** There is no reply on this channel. The version
goes in the environment of every pty omt spawns, where an agent can read it
before its first byte.

**Notifications travel both ways.** Outward, omt tells your terminal an agent
needs you. Inward, a program in a pane tells omt it needs you. Same idea, same
sanitizing, opposite direction — and the inward one is the older, more widely
implemented sequence, which is why it is `Must` in [04](../../docs/architecture/04-terminal-core.md) now.

**A confirmation is an overlay, not a flag.** An overlay takes every key while
it is open, so the state that decides what Enter means has to *be* the overlay.
The first attempt used a pending-quit flag and was unanswerable: pressing the
leader again cleared the flag the first press had set.

## Risks / Trade-offs

**A hostile pane can forge agent reports.** It can, and the tier says so. The
alternative — ignoring the channel — costs every honest agent that has no hook
installed, which is most of them. Nothing at `Marker` may contradict a live
higher tier, which is the rule that contains the damage.

**`OSC 9` is now `Must` rather than `Should`.** It is a sequence anything can
emit, so a noisy program becomes a noisy notification. That is the program's
choice to make and the user's to configure; swallowing it is not omt's decision
to take on their behalf.
