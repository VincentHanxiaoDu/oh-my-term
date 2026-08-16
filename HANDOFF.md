# Handoff

Written for whoever picks this up next. It says what is true now, what is not,
and the things that have already bitten somebody here — so they bite nobody
twice.

Read `openspec/changes/archive/` for the reasoning behind anything below: each
change carries its proposal, its design and the decisions with their reasons. If
one of them disagrees with this file, the change is right and this is stale.

There is no change in flight. Section 6 is what is worth doing next.

---

## 1. Standing constraints

These are not preferences. Breaking one has cost real work here before.

- **English only** — code, comments, documentation, commit messages. Discussion
  in chat can be in any language; nothing that lands in the repository is.
- **No competitor names.** Two comparable products were read directly and their
  source is under `.research/`. Their names appear nowhere in the repository:
  not in code, not in docs, not in commit messages, not in this file. Refer to
  them as "the others", or by what they do. `docs/reference/read-from-the-others.md`
  is the survey and follows the rule.
- **`.research/` is gitignored and is never vendored, built or linked.**
  `cargo xtask licensing` fails if it is.
- **The SDD workflow**: change the documents, then make an OpenSpec change, then
  implement, then archive it. `openspec validate <change>` before archiving.
- **Install with `cargo xtask install`**, never `cp`. macOS caches a code
  signature per inode; overwriting the binary in place gets it killed with
  signal 9 and no explanation. The task copies beside, renames, and runs
  `--version` to check. It installs `omt` *and* `omt-hook`.
- **Commit messages** end with:
  ```
  Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
  Claude-Session: <the session URL>
  ```

## 2. What was just finished

**Sessions that outlive the terminal.** `omt` is a client now: it finds an
instance or starts one, and the sessions it shows are that instance's. Closing
the terminal closes the terminal. The change is archived as
`2026-08-16-sessions-outlive-the-terminal`; read it for the reasoning, not this
summary.

A terminal that cannot reach or start an instance still opens, owning its own
sessions, and says so in one line. `OMT_LOCAL` asks for that path deliberately —
and any test that starts the omt binary **must** set it or point at a runtime
directory of its own, which `cargo xtask test-isolation` now fails on rather
than warns about: the other things that gate checks cost a confusing test run,
and this one costs somebody else's sessions.

## 3. What that work found, and what it cost to find

Every one of these was only visible with real processes. They are the reason
this file exists.

- **The daemon died with the terminal**, intermittently. A child in the
  terminal's foreground process group takes the hangup; moving it to another
  group in the same session is not enough, because that terminal is still its
  controlling one. `setsid` is the only call that severs it.
- **Nothing was ever discovered.** Discovery scanned `$XDG_RUNTIME_DIR/omt/`
  and daemons bound one directory up, so every terminal started a daemon that
  found the path taken and died — and it all worked because the *previous*
  daemon was still answering. The document had said the right thing all along.
- **An empty variable is still set.** `$XDG_RUNTIME_DIR=""` yields a relative
  socket path; one of those was found still running hours later, bound in
  whatever directory it started in.
- **A later edit silently dropped an earlier branch**, and copy mode drew
  twenty-nine blank rows with no error anywhere. The diagnosis came from
  capturing the bytes omt actually wrote — not from reading the code again.
- **Sessions created by an instance had no `$OMT_SOCK`**, so hooks in them
  exited at their guard clause: indistinguishable from hooks nobody installed.

Two things left behind for the next person: a client writes a call that never
answered to `$OMT_CLIENT_LOG`, and a history that cannot be fetched says so on
the screen rather than being drawn as emptiness.

## 4. How to check your work

```
cargo fmt --all
cargo clippy --workspace --all-targets      # warnings are failures here
cargo test --workspace                      # slow: ~15 min from cold
cargo test -p omt --test screen             # a real pty, ~3 min, serialised
cargo run -p xtask -- layering licensing file-length codegen-check test-isolation
cd web && npm run check
```

`cargo xtask codegen` regenerates `web/src/generated/*` and
`docs/reference/*`; the parity gate refuses to build when the catalog and the
web client disagree, which is deliberate and is not something to work around.

Adding a capability means: the `capability!` declaration, a web handler in
`web/src/capabilities.ts`, `cargo xtask handlers`, and a schema snapshot
(`UPDATE_SCHEMA_SNAPSHOT=1 cargo test -p omt --test schema_compatibility`). All
three gates will tell you which one you forgot.

## 5. Traps that have already cost time here

- **A second lock on the same thread deadlocks the terminal, silently.** The
  frame loop holds the instance for a whole iteration. A row builder that locked
  again froze the whole TUI with no error anywhere. Row builders take what the
  caller already holds.
- **Fixed sleeps in screen tests are a guess about the machine.** Three tests
  passed alone and failed under a full run. `Screen::settle_until` waits for a
  condition; use it.
- **A hook with no `$OMT_SOCK` exits at its guard clause**, which is
  indistinguishable from a hook that was never installed. Every pty omt spawns
  carries `OMT_SESSION`, `OMT_SOCK` and `OMT_AGENT_EVENTS`.
- **Test binaries leak.** Hung `screen` and `remote_call` processes from earlier
  days were found holding ptys and sockets. `pgrep -f target/debug/deps` before
  blaming a flake.
- **The disk fills.** `target/` reached tens of gigabytes and stopped the whole
  session — no tool could run, because the harness could not even write its own
  output file. `cargo clean` between long sessions.
- **A screen test's shell is `/bin/sh`.** `printf '\033[...'` works; `$(...)`
  inside a typed command line has bitten someone; a shell running a single
  command execs it, so a marker put in `argv` is gone by the time anything looks
  for it.

## 6. The longer backlog

From reading the others, in `docs/reference/read-from-the-others.md`. Everything
there has a verdict and a reason, including the rows omt is deliberately
different on. Still open, roughly by value:

- per-agent identity colouring in the panel
- the agent's own slash commands, surfaced by the host
- a command-history menu in the terminal
- mouse selection (the keys document calls this deliberate; revisit it)
- kitty graphics passthrough
- copy mode's named registers and the emacs motion set

## 7. One request

The comments in this repository explain *why*, not what — including the wrong
turns, so the next person does not retake them. That convention is load-bearing:
several bugs this month were found by writing the reason down and noticing it
was not true. Please keep it.
