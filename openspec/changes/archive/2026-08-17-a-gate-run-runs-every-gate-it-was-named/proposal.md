# A gate run runs every gate it was named

## Why

`HANDOFF.md` §4 tells the next person to check their work with:

```
cargo run -p xtask -- layering licensing file-length codegen-check test-isolation
```

That command runs **one** gate. `xtask` reads `std::env::args().nth(1)`, matches
it, and returns; the other four names are never looked at. It then exits `0`,
so the run looks like five passes and is one. Worse, `licensing` takes
`args().nth(2)` as a directory to check, so in that line the licensing gate was
pointed at a path called `file-length` — a path that does not exist.

This is the second shape of the defect this repository keeps finding: not a
line that is missing, but a line that runs and watches the wrong thing. Nothing
fails, nothing logs, and the reviewer reads a green run. Every gate here exists
because breaking it cost somebody real work — `.research/` being vendored,
generated artifacts drifting from the catalog, a test joining the machine's own
instance — and four of them were being skipped by the very command that was
written to run them.

Each gate does pass when run on its own today, so this change fixes the tool
rather than the repository it checks.

## What changes

- `xtask` takes **every** task named on its command line and runs them in the
  order given.
- One failing gate does not hide the rest: the run continues, each gate's
  outcome is named, and the process exits non-zero with the failures listed.
- Every task run announces itself before it runs, so a line of output belongs
  to a gate by name rather than by the reader's guess.
- The licensing gate's optional fixture directory becomes `--root <path>`
  instead of a bare second word, because a bare second word is indistinguishable
  from the name of the next gate.
- An unknown task or an unknown option fails the run rather than being ignored.
- Tests over the argument plan, so "it ran one of five and said nothing" cannot
  come back.

## Non-goals

- No change to what any individual gate checks. All five pass today; only the
  runner is wrong.
- No new gate, and no change to the CI workflow, which already invokes gates one
  per step and was therefore never affected.
- No argument parser dependency. The grammar is a list of words and one option.

## Impact

- `xtask`: argument handling, task dispatch and reporting.
- `HANDOFF.md`, `README.md`: the multi-gate line now means what it said.
- Depends on nothing; can run in parallel with anything.
