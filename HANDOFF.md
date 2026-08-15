# Handoff

Written for whoever picks this up next. It says what is being built right now,
what is already true, what is not, and the things that have already bitten
somebody here — so they bite nobody twice.

Read `openspec/changes/sessions-outlive-the-terminal/` first: the proposal is
why, the design is the decisions and their reasons, and `tasks.md` is the
checklist this document summarises. If the two disagree, the change is right and
this is stale.

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

## 2. What is being built

**Sessions that outlive the terminal.** A multiplexer exists to make one
promise — close the terminal, the work keeps running — and omt did not keep it:
the terminal owned every pty in its own process, so quitting hung up on all of
them, including an agent mid-turn.

The intermediate honest state shipped first: the key says `quit`, and it lists
what it is about to end. `docs/architecture/05-session-model.md` §4.1b records
the gap and what closing it takes. **Delete that section when this change
lands** — it describes a gap that will no longer exist.

## 3. Where it stands: 21 of 32 tasks

Three commits, each green on the full suite when it landed:

| Commit | What |
|---|---|
| `4dabde5` | the instance pushes a session's output; seeding; the tail buffer |
| `b1e0c7c` | the client: socket, reader thread, mirrored emulator |
| `0a26f7b` | the seam in the frame loop; finding and starting an instance |

### Proven, with tests that cross the wire

- `crates/omt-term/src/seed.rs` — a terminal's state rendered back into escape
  sequences. Nine tests: colours, wide characters, cursor, alternate screen,
  empty screen, and that a *reused* emulator ends up right.
- `crates/omt-daemon/src/tail.rs` — recent output with absolute positions, so a
  subscriber can be behind of something.
- `crates/omt/src/stream.rs` + `crates/omt/tests/streaming.rs` — six tests over
  a real socket. The two that matter most:
  - `a_client_emulator_ends_up_showing_what_the_session_shows` — the instance
    owns the pty, the client owns an emulator, and the client's screen equals
    the session's, *including what happened before it attached*.
  - `a_session_created_through_an_instance_outlives_the_client_that_made_it` —
    the client vanishes entirely; the session and its process do not notice.
- `crates/omt/src/backing.rs` — the seam. Thirty-two screen tests drive a real
  terminal through it and say what they said before, which is what makes the
  refactor behaviour-neutral rather than merely believed to be.
- `crates/omt/src/lifecycle.rs` — find an instance, start one if there is none,
  refuse when several are running.

### Not done — this is the next work, in order

The frame loop draws from the seam and both sides of the seam work. What is
missing is the half of the terminal that reads the **instance** rather than the
**terminal**: the agent panel, the agent list, the status bar's chips and the
quit confirmation all take a local `Instance` today (`crates/omt/src/rows.rs`,
`crates/omt/src/terminal_bits.rs`).

1. **One view model both modes produce.** `backing::Reported` is the start of
   it: id, title, agent state as a word, thread and blocked counts. Local mode
   has more (the `Detection`'s evidence string, the rich `AgentState`); the
   honest shape is `Reported` plus optional fields only a local probe can fill.
2. **The row builders take that** instead of `&mut Instance`. They are
   `agent_rows`, `session_rows`, `panel_sessions` (rows.rs) and `still_working`
   (terminal_bits.rs).
3. **Look the vocabulary up by word.** `omt_tui::state_word` / `attention` /
   `state_mark` take `&AgentState`; over the wire what arrives is the word.
   `web/src/generated/vocabulary.json` is the same table the web client uses —
   do it the way the web already does, and keep `cargo xtask vocabulary`
   passing so the three surfaces cannot drift.
4. **Then** the terminal attaches by default, `⟨leader⟩ d` goes back to saying
   `detach`, and the confirmation keeps only the case it still describes
   (ending sessions on purpose). Update the screen test
   `quitting_says_what_it_is_about_to_end` — today it asserts the session
   *does not* survive, which is the current truth and will become the bug.

`session.list` already carries everything step 1 needs, including which agent is
in the session. No new capability is required.

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
