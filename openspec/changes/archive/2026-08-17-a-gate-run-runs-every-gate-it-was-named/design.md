# Design

## Crates

`xtask` only. No library crate changes; nothing else links it.

## The shape of the bug

```rust
match std::env::args().nth(1).as_deref() {
    Some("layering") => layering(),
    Some("licensing") => licensing(std::env::args().nth(2)),
    ...
}
```

`nth(1)` is a single word, and `match` returns after the first arm. The rest of
the command line is not an error — it is not read at all. `licensing` compounds
it by claiming `nth(2)` for itself, so a positional argument and the next task
name occupy the same slot.

## The plan, as a value

Argument handling becomes a pure function so it can be tested without running a
gate:

```rust
struct Plan { tasks: Vec<String>, root: Option<String> }

fn plan(args: impl IntoIterator<Item = String>) -> Result<Plan>
```

Rules:

- A word that does not start with `-` is a task name, appended in order.
- `--root <path>` or `--root=<path>` sets the licensing fixture directory.
- Any other `-`-prefixed word is an error. Silently ignoring an option is how
  this defect started.
- An empty list is not an error: it prints usage, as it did before.

Validation of task *names* happens in `plan`, not at dispatch, so a typo in the
fifth name fails before the first gate spends two minutes running.

## Running them

`plan.tasks` is walked in order. Each task prints `== <name> ==` before it runs,
so its output is attributable; a failing gate is recorded and the walk
continues, because the reason to name five gates is to learn about all five.
The process then exits non-zero naming every gate that failed.

Continuing past a failure is safe here: the gates are read-only checks. The two
tasks that write — `codegen` and `install` — are named explicitly by whoever
runs them, and a failure in an earlier read-only gate does not make writing more
dangerous than running the write task alone.

## Why not a parser dependency

The grammar is a list of words plus one option, and `xtask` is the crate whose
dependencies the licensing gate itself walks. A hand-written loop is about
fifteen lines and carries no new licence to account for.

## Compatibility

`cargo xtask licensing /some/fixture` stops working and becomes
`cargo xtask licensing --root /some/fixture`. Nothing in the repository used the
positional form: it had no caller at all, which is part of why the collision
went unnoticed.
