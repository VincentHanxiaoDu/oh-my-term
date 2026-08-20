# Attaching to what was left running

## Why

`omt` with no arguments attaches to the first session that is still there —
which, after a daemon restart, is an orphan restored from a snapshot: a
terminal with the session's last screen and no pty behind it. The first thing
an attach does is resize, and `session.resize` refused orphans:

```
Error: no such session
```

The orphan's screen is readable, searchable and copyable by design (its
restorer says so), and the sibling handlers — `session.history` and
`session.blocks` — tolerate orphans through `orphan_terminal`. Only `resize`
did not, so the one session that was *worth* coming back to made the attach
fail outright. Found in real use: `omt session list` showed the orphan,
`omt` fell over on it, and the refusal came from a handler whose only job was
to say a size.

## What changes

- `session.resize` tolerates an orphan: there is no pty to resize, so it
  reports success and moves on — the same tolerance its siblings show.
- A unit test drives the handler against a restored orphan and a live
  runtime, including the deadlock trap: the handler locks the instance
  itself, so the test must not hold the lock while calling it.

## Non-goals

- Writing to an orphan stays refused with its existing, clearer message
  ("restart it first"). Readable, not writable, is the design.

## Impact

- `omt`: one capability handler and its test.
