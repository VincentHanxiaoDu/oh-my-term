## Context

A pty dies with the daemon that opened it. Everything else survives — the
scrollback, the cwd, the argv — so `persist.rs` brings the session back as an
`Orphaned` one, readable and searchable, refusing writes rather than swallowing
them. That is the right design and this change does not touch it.

What is missing is the other half, which `SessionState::Orphaned`'s own
documentation already promises: *"The surface offers to restart it."* No
surface does. The capabilities exist — `session.restart` respawns the same
command, `session.close` removes it — and neither is reachable from the phone.

## Goals

- An orphan can be restarted or closed from the surface that shows it.
- A live session is never below a dead one.
- Being rid of sixteen of them is one action, and it says what it will do.

## Non-goals

- **Pruning on restore.** Deciding on somebody's behalf that yesterday's
  output is worthless is the one thing this must not do. The scrollback is
  most of why the session came back at all.
- **A TTL on orphans.** Same reason, deferred rather than delivered: a session
  nobody has looked at for a month is *probably* rubbish, and "probably" is not
  a licence to delete somebody's terminal history. If it is ever wanted it is a
  setting with a default of off, not a behaviour.
- **Changing what `session.restart` does.** It is declared, tested and used by
  the TUI.

## Decisions

**Group, do not filter.** The first instinct is to hide orphans behind a
toggle. That is worse: a session somebody deliberately kept is then behind a
control they have to know about, and the roster silently disagrees with
`session.list`. Grouping keeps everything on one screen and puts the live work
where the thumb lands.

**Two numbers, not one.** The subtitle already had this bug once — it said
"7 running" over seven rows that all read "orphaned", because it counted the
total. It now counts what is actually running; this adds the second number
rather than folding orphans back into the first.

**The clear-all offer states its count before acting, and is absent at zero.**
A control that is present and inert teaches people that controls do nothing.

**The e2e helper opens a *live* session.** This is the defect that exposed the
whole thing: the suite opened the roster's first row, which on any machine that
had run omt before was a corpse. It passed for months on leftover live
sessions. A test whose correctness depends on the machine's history is not a
test, and this one hid a real product gap for exactly as long as it was green.

## What could go wrong

- **Closing the wrong thing.** `session.close` on a running session kills a
  process. The clear-all path filters to orphans and the test asserts a running
  session is never among them at any count.
- **A restarted orphan losing its scrollback.** `session.restart` keeps the old
  output above a separator; that is its existing behaviour and the scenario
  pins it.
- **The TUI and the web disagreeing about the count.** Both read
  `session.list`, and the state is one string on the wire.

## Testing

The roster grouping and the count are pure functions over a session list, so
they are unit tests. Restart, close and clear-all are Playwright cases against
a real instance on both device profiles — and the suite runs against a **clean
state file**, so the empty-roster path is exercised on every run rather than
only on a fresh checkout.
