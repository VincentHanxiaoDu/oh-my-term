# Working before it is configured

## Why

P11 says every feature works before it is configured and is also a setting.
Most of omt already does. Three things do not, and each fails in the way P11
describes.

**Shell integration is a setting nobody will find.** Blocks with command text
and exit codes need OSC 133, which needs a line in the user's shell rc. omt
knows how to emit it and never offers. So the good version of blocks is behind
a step, and the step is one only somebody who already knows about OSC 133 would
take — which is the "shipped switched off" failure exactly.

**A first run has nothing to say.** `omt` on a machine with no configuration
starts a shell and stops there. Nothing mentions that `Ctrl-A ?` exists, that a
phone can attach, or that an agent in this session would be detected. The hint
line does some of this and only inside a session.

**Config values have no discoverable shape.** `config.get` reports what is set
and where it came from. Nothing reports what *could* be set — so the second P11
failure applies to any setting whose name a user does not already know.

## What changes

- `shell.integration` — omt reports whether the current shell emits the marks,
  and prints the one line that makes it, for the shell actually in use.
- A first-run summary: what omt found (shell, agents on PATH, whether an
  instance is already running) and the two keys worth knowing. Once, on a
  machine with no config, and never again.
- `config.schema` — every setting, its default, its type and one line about
  what it does, generated from the same declarations `config.get` reads.

## What this deliberately does not do

**No wizard, no prompt, no question at first run.** Every question is a step,
and a step is where people stop. The first run prints and continues; it does
not wait for an answer, and running `omt` a second time prints nothing.

**No automatic edit of the user's shell rc.** omt prints the line. Writing to
somebody's `.zshrc` without asking is the kind of helpfulness people uninstall
software over, and asking would be the wizard this change refuses.
