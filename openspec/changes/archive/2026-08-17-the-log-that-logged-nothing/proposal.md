# The log that logged nothing

## Why

The `log.*` settings — level, file, max size, redaction — have been in the
reference and the settings screen since either existed, and **nothing ever
logged anything**: no subsystem, no call sites. A settings screen that
promises a log and produces none is the vocabulary-without-a-consumer shape,
and it is the last subsystem-scale instance of it — every other group in
`UNWIRED` was a wire into something that existed, and this one was a promise
with nothing behind it.

## What changes

- `Diag`: a level, an optional file, rotation, redaction. Deliberately not a
  framework — no logger crate, no spans. The places that speak to a person
  (the reason omt closes, the crash note) stay exactly where a person reads
  them; this is the layer under those.
- `log.level` admits what it says: `error` writes nothing below an error, and
  everything else is read off the same key.
- `log.file` writes to the named file and rotates at `log.max_size` — the old
  file becomes `.log.1`, because a log that grows forever is how disks fill
  (HANDOFF §5 lists that exact failure).
- `log.redact_secrets` replaces the values of secret *shapes* — `token=`,
  `password:`, `key=` — keeping the first eight characters so two secrets can
  be told apart, and keeping the line readable.
- A startup line is the first call site; verified through a real pty twice:
  under `error` level nothing lands in the file, under the default the
  startup line does.

## Non-goals

- No framework, and no sweep of `eprintln!` — the lines that speak to the
  person stay put.
- The appearance group remains `UNWIRED`, its own change.

## Impact

- `omt`: one module, one call site, four settings out of `UNWIRED`.
