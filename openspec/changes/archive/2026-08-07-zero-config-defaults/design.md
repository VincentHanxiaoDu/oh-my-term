# Design

## Detection, not configuration

Whether the shell emits marks is *observed* rather than asked. The terminal
already tracks blocks; a block with `attributed: true` is proof that OSC 133
arrived. So integration status is a property of what has been seen, not a
setting somebody set — which means it cannot be wrong, and it needs no key.

The one edge: a brand-new session has seen nothing yet, and "no marks yet" is
not "no integration". Status is therefore reported alongside how many blocks
have been seen, so a caller can tell the two apart rather than telling somebody
to install a snippet they already have.

## The snippet is per shell and printed, never written

Each shell needs a different line, and the file it goes in differs too. omt
returns both. It does not write the file: editing somebody's `.zshrc` uninvited
is the kind of helpfulness people uninstall software over, and asking first
would be the wizard this change exists to avoid.

A shell omt has no snippet for says so. Returning bash's line for fish produces
an error message in their terminal on every login, which is worse than no
snippet at all.

## The first run prints and continues

Not a prompt. Not a wizard. Print what was found, then start the session — the
user is in a working terminal either way, and the summary is scrollback they can
read or ignore. Absence of a configuration directory is the trigger, because it
is what "first run" actually means and needs no state of its own.

## The schema comes from the declarations

`config.get` already reads a declaration per setting to resolve values. The
schema is the same source with the values left out. Deriving it means a setting
cannot exist without appearing in the schema — a hand-maintained list would go
stale on the first new key, and the failure would be a setting nobody can find,
which is exactly what this change is for.
