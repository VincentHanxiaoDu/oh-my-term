# Handoff

Written for whoever picks this up next. It says what is true now, what is not,
and the things that have already bitten somebody here — so they bite nobody
twice.

Read `openspec/changes/archive/` for the reasoning behind anything below: each
change carries its proposal, its design and the decisions with their reasons. If
one of them disagrees with this file, the change is right and this is stale.

There is no change in flight. Section 6 is the one thing still open, and it is
a question rather than a feature.

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

**A feature-by-feature pass against the two comparable products**, whose source
is under `.research/`. Six changes, each archived with its reasoning:
command history, reading the agent's own session file, the mouse, images in a
pane, the rest of copy mode, and drawing those images where a terminal says it
can. `docs/reference/read-from-the-others.md` is the
checklist and carries a verdict per row, including the rows where omt is
deliberately different and the rows where **their** answer is worse than it
looks.

**Then the first real use of that build**, which is a different kind of
evidence and found things no suite had: omt closing without a word, and chrome
drawn as a white slab. Archived as
`2026-08-17-the-chrome-has-a-palette-and-the-exit-has-a-reason`.

**The pattern worth knowing before you start anything here.** Every one of those
found the same class of defect underneath the feature, and it is the expensive
kind:

> **A vocabulary that exists everywhere and a producer that exists nowhere.**

- `recall.suggest` had a capability, a web handler and a menu; **nothing had
  ever called `recall.record`**, so it always answered empty.
- `Tier::Transcript` and `EventSourceTag::Transcript` had existed since the
  agent layer was designed; **no source ever emitted either**.
- The mouse had a passthrough table, a `MouseEvent` type and a keymap grammar;
  **omt never asked its terminal for mouse reporting**.
- `BlockTracker::command_text` had exactly one caller, and it was a test — so
  every block in production carried an empty command.
- `keymap = "vim" | "emacs"` was documented in the configuration reference and
  parsed into the binding file; **no code read the field**.
- `omt-theme` has held a background, a foreground and sixteen colours since the
  web client existed, and **none of the terminal's own chrome ever read it** —
  which is why the status bar was a slab of reverse video.
- A session records how its program ended. **Nothing asked it**, so omt closed
  in silence and looked like a crash.

From outside, every one of these is indistinguishable from "there was nothing
to show". They do not fail, they do not log, and they do not look wrong in a
review — the code that is missing is missing, not incorrect. **When a feature
here answers with nothing, check that something writes to it before you believe
the emptiness.** `grep` for the producer, not the type.

There is a second form of it, and it is worse because the line *is* there:
**code that runs and watches the wrong thing.** `omt --version` reported a
commit the binary was not built from for as long as the stamp existed — the
build script asked to rerun when `.git/HEAD` changed, and on a branch that file
does not change when you commit. The stamp existed precisely to stop somebody
wondering whether they were running the fix.

Three documents also claimed things that were not true, which is the same
defect wearing a third hat: that `vte` surfaces APC (it consumes it with no
callback at all), that `omt-term` already parsed inline images, and that the
transcript reader watches a directory.

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

### 3.1 What the first real use added

A build that passed every suite was run in somebody's own terminal and
"disappeared when I clicked". None of that was reproducible here; what settled
it was **asking the machine instead of the code**:

- **The session's own state said it.** `omt session list` on the still-running
  instance reported `exited:130` — the pane's shell had been stopped by
  `SIGINT`, so the last pane went and omt closed with nothing left to show.
- **macOS keeps crash reports.** `~/Library/Logs/DiagnosticReports` had no omt
  in it, which rules out every signal that would have produced one. "It
  crashed" and "it exited" look identical on screen and are not the same bug.

Both of those are cheaper than a debugger and neither needs the person who hit
it to still be at the keyboard. Reach for them first.

**Why it took so long to see:** the exit *was* reported — to stderr, on the
alternate screen, which `LeaveAlternateScreen` then threw away. Not silence. A
sentence written where nobody could read it.

**Still unreproduced:** what sent that `SIGINT`. Section 6.

## 4. How to check your work

```
cargo fmt --all
cargo clippy --workspace --all-targets      # warnings are failures here
cargo test --workspace                      # slow: ~15 min from cold
cargo test -p omt --test screen             # a real pty, ~3 min, serialised
cargo run -p xtask -- layering licensing file-length codegen-check test-isolation settings-readers
cd web && npm run check
```

`cargo xtask codegen` regenerates `web/src/generated/*` and
`docs/reference/*`; the parity gate refuses to build when the catalog and the
web client disagree, which is deliberate and is not something to work around.

Adding a capability means: the `capability!` declaration, a web handler in
`web/src/capabilities.ts`, `cargo xtask handlers`, and a schema snapshot
(`UPDATE_SCHEMA_SNAPSHOT=1 cargo test -p omt --test schema_compatibility`). All
three gates will tell you which one you forgot.

**When it misbehaves in a real terminal rather than in a test**, in the order
that costs least:

```
omt --version                 # the commit it was actually built from
cat /tmp/omt-crash.log        # a panic left a note here
omt session list              # a session records how its program ended
ls ~/Library/Logs/DiagnosticReports   # macOS logs anything killed by a signal
```

omt also says why it closed, on the line after it hands the terminal back.
That line is the first thing to ask for.

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
- **Anything that reads the terminal's own input eats somebody's keystroke.**
  The graphics handshake reads the raw descriptor for ~120 ms at startup, and
  the first version swallowed whatever else arrived in that window — the
  keystroke never reached the shell, and only `runs_for_real` caught it. What
  is read there and is not the reply belongs to the person at the keyboard and
  has to be handed on. Any future query has the same obligation, which is most
  of why omt does not query terminals mid-session at all.
- **A screen test's shell is `/bin/sh`.** `printf '\033[...'` works; `$(...)`
  inside a typed command line has bitten someone; a shell running a single
  command execs it, so a marker put in `argv` is gone by the time anything looks
  for it.
- **Anything printed before the guard drops is printed onto a screen that is
  about to be discarded.** Errors, panics, "why omt closed" — all of it goes
  after the alternate screen is handed back, or it does not exist. This is the
  single reason a real failure looked like a silent one.
- **The screen suite must stay one test binary.** Cargo runs test binaries in
  parallel, and sixteen real ptys at once is a test of the machine, not of omt
  — it fails on timing, in a pattern that changes with load. The harness lives
  in `crates/omt/tests/common/screen_harness.rs` so the file can be split
  without splitting the binary.
- **A gate can fire on code that is correct.** Moving that harness made
  `test-isolation` report a suite that *was* isolated, because the isolation
  moved with it. The gate now follows `mod common;`. Worth remembering the
  shape: the tempting fix for a false alarm is to weaken the gate, and that is
  how a gate stops being one.

## 6. What is open

### 6.1 The survey: nothing

From reading the others, in `docs/reference/read-from-the-others.md`. Every row
has a verdict and a reason, including the rows omt is deliberately different on
and the rows where their answer turned out to be worse than it looked. The rows
omt does not implement say why, rather than sitting on a list as though
somebody forgot.

What that leaves is not a backlog of features but the habit that found them:
when a surface here answers with nothing, check that something writes to it
before believing the emptiness.

### 6.2 The one open question: who sent that `SIGINT`

A pane's shell was stopped by `SIGINT` in ordinary use (§3.1) and it has not
been reproduced here. Ruled out, each by a real run rather than by reading:

- mouse input of every shape — 360 random events, SGR and the legacy encoding,
  clicks landing during the startup handshake, over both a local terminal and
  one attached to a daemon;
- a real `claude` and a real `pi` in a pane, 108 events each, plus copy mode
  over their output;
- restoring an older snapshot, two instances present at once, and `Ctrl-C` sent
  straight at an idle shell — which an interactive shell survives, as it should.

So it is something about that machine's terminal or shell that this machine's
ptys do not have. **The instrumentation to catch it is now in place**: omt
names the reason on the way out, and a panic leaves a note at
`/tmp/omt-crash.log`. The next occurrence should report itself.

If you pick this up: the shape to suspect is a signal reaching the pane's
process group from outside it, and the thing to compare is the pty's terminal
settings and process-group layout between a daemon-created session and a
locally-owned one.

## 7. One request

The comments in this repository explain *why*, not what — including the wrong
turns, so the next person does not retake them. That convention is load-bearing:
several bugs this month were found by writing the reason down and noticing it
was not true. Please keep it.
